//! Org erasure for the profile families (WS17.E3).
//!
//! `OrgErasable` is the list the edge's internal erasure route walks. Profiles
//! come first; a later family joins by one line in the edge's wiring.
//! `ProfileErasure` deletes an org's `quarry_profiles` rows and tombstones,
//! drops its Redis entries (the v2 envelope keys and any legacy plaintext
//! keys) and evicts its cached DEKs in this process. Counts only leave this
//! module: no id, key name or payload is returned or logged.
//!
//! W2-U41a adds the families beyond profiles (step receipts, checkpoints, the
//! crawl frontier, change baselines, job history, artifacts), the table
//! inventory the plan's section 3.6 asks for, and the profile tombstone guard
//! (D2-077). They are off by default; see `FAMILIES_ENV` and
//! `PROFILE_TOMBSTONE_GUARD_ENV`.
//!
//! W2-Q-U41b adds the cryptographic hop: with `ERASE_SHRED_KEYS_ENV` on, the
//! profile family ends by destroying the org's KEKs in org-core (`DELETE
//! /internal/orgs/:org/keys`), so a copy of an `oc:`-sealed envelope that
//! survived the row delete (a backup, a replica) can no longer be opened. Off
//! by default; cryptographic only once the key provider is `org_core`.
//!
//! Limits, stated where they bite: the DEK eviction covers the replica that
//! answers the call only (other replicas keep cached DEKs for up to the 300 s
//! TTL), and the Redis step is skipped when no Redis connection was built.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use redis::aio::ConnectionManager;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::ProfileKind;

use crate::profile_blob_store::{BlobPurge, ProfileBlobStore};
use crate::profile_keys::{
    OrgCoreConfigError, ProfileKeyProvider, OC_KEY_ID_PREFIX, ORG_CORE_KEYS_PRINCIPAL,
    ORG_CORE_KEYS_TOKEN_ENV, ORG_CORE_KEYS_URL_ENV,
};

/// Namespace of the envelope-layer cache keys (`cached_profile_store::blob_cache_key`).
pub const V2_CACHE_PREFIX: &str = "quarry:profile:v2:";

/// Namespace of the legacy plaintext cache keys (`cached_profile_store::cache_key`).
pub const LEGACY_CACHE_PREFIX: &str = "quarry:profile:";

/// SCAN page size and UNLINK batch size.
const SCAN_COUNT: usize = 500;
const UNLINK_BATCH: usize = 500;

/// What one family removed for one org. Counts only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PurgeCounts {
    pub rows: u64,
    pub tombstones: u64,
    pub cache_keys: u64,
    pub keys_evicted: bool,
    /// Tombstones written after the purge by the profile tombstone guard
    /// (D2-077). Zero when the guard is off and for every other family.
    pub guard_tombstones: u64,
    /// The crypto-shred receipt (W2-Q-U41b). `None` when the hop is off and for
    /// every family but `profiles`.
    pub key_shred: Option<KeyShredOutcome>,
}

/// A data family that an org erasure must reach. Implementations are idempotent:
/// a second call for the same org removes nothing and reports zeros.
#[async_trait]
pub trait OrgErasable: Send + Sync {
    /// Stable, closed family name for logs and the response (`profiles`).
    fn family(&self) -> &'static str;

    /// Remove everything this family holds for `org_id`.
    async fn purge_org(&self, org_id: &str) -> QuarryResult<PurgeCounts>;
}

/// Escape the glob metacharacters of a SCAN MATCH pattern, so an org id is
/// matched literally.
pub(crate) fn glob_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn redis_err() -> QuarryError {
    QuarryError::new(ErrorCode::Internal, "profile cache command failed")
}

/// SCAN the keyspace for `pattern` and UNLINK every matching key for which
/// `skip` is false. Returns how many keys were removed. Used by the erasure
/// route and by the backfill, both of which are idempotent.
pub(crate) async fn scan_unlink(
    conn: &mut ConnectionManager,
    pattern: &str,
    skip: impl Fn(&str) -> bool,
) -> QuarryResult<u64> {
    let mut cursor: u64 = 0;
    let mut removed: u64 = 0;
    loop {
        let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(SCAN_COUNT)
            .query_async(conn)
            .await
            .map_err(|_| redis_err())?;
        let targets: Vec<&str> = keys
            .iter()
            .map(String::as_str)
            .filter(|k| !skip(*k))
            .collect();
        for chunk in targets.chunks(UNLINK_BATCH) {
            let n: u64 = redis::cmd("UNLINK")
                .arg(chunk.to_vec())
                .query_async(conn)
                .await
                .map_err(|_| redis_err())?;
            removed += n;
        }
        if next == 0 {
            break;
        }
        cursor = next;
    }
    Ok(removed)
}

/// The profile family: database rows and tombstones, Redis keys, cached DEKs.
pub struct ProfileErasure {
    blobs: Arc<dyn ProfileBlobStore>,
    keys: Option<Arc<dyn ProfileKeyProvider>>,
    redis: Option<ConnectionManager>,
    tombstone_guard: bool,
    /// W2-Q-U41b: the crypto-shred hop. Off unless `ERASE_SHRED_KEYS_ENV` is on.
    shred: KeyShredSetup,
    /// Orgs whose shred was needed but has not answered yet. A retry after the
    /// rows are gone still shreds (the probe would no longer see an `oc:` row).
    /// Process-local: see the depth notes.
    pending_shred: Mutex<HashSet<String>>,
}

impl ProfileErasure {
    pub fn new(
        blobs: Arc<dyn ProfileBlobStore>,
        keys: Option<Arc<dyn ProfileKeyProvider>>,
        redis: Option<ConnectionManager>,
    ) -> Self {
        Self {
            blobs,
            keys,
            redis,
            tombstone_guard: false,
            shred: KeyShredSetup::Off,
            pending_shred: Mutex::new(HashSet::new()),
        }
    }

    /// W2-Q-U41b: set the crypto-shred hop (`KeyShredSetup::from_env` at boot).
    pub fn with_key_shred(mut self, setup: KeyShredSetup) -> Self {
        self.shred = setup;
        self
    }

    /// D2-077: the purge deletes the org's tombstones, so a save that started
    /// before the purge and lands after it could recreate an erased row. With the
    /// guard on, the purge lists the org's profiles first and, after the purge,
    /// writes a fresh tombstone for each of them through `delete` (which also
    /// removes a row recreated in between). The tombstones hold ids only and
    /// expire with the store's tombstone TTL. A save for a profile id the list
    /// did not see, or one stamped after the purge, is not refused: the org is
    /// expected to be closed upstream by then. Off by default.
    pub fn with_tombstone_guard(mut self, on: bool) -> Self {
        self.tombstone_guard = on;
        self
    }

    /// Build with a Redis connection when `redis_url` is set and connects. A
    /// failed connection is logged by class and the Redis step is skipped; the
    /// database step still runs, and the route reports zero cache keys.
    pub async fn connect(
        blobs: Arc<dyn ProfileBlobStore>,
        keys: Option<Arc<dyn ProfileKeyProvider>>,
        redis_url: Option<&str>,
    ) -> Self {
        let redis = match redis_url.filter(|s| !s.is_empty()) {
            None => None,
            Some(url) => match redis::Client::open(url) {
                Ok(client) => match ConnectionManager::new(client).await {
                    Ok(manager) => Some(manager),
                    Err(_) => {
                        tracing::warn!("profile erasure: Redis connection failed; cache step skipped");
                        None
                    }
                },
                Err(_) => {
                    tracing::warn!("profile erasure: Redis client init failed; cache step skipped");
                    None
                }
            },
        };
        // W2-Q-U41b: the flag and the org-core settings are read by name here,
        // so the boot wiring stays the one `connect` call.
        Self::new(blobs, keys, redis).with_key_shred(KeyShredSetup::from_env())
    }

    /// The Redis step for one org: envelope keys, then legacy plaintext keys.
    /// Legacy keys that fall in the v2 namespace are never legacy keys, so the
    /// legacy pattern skips them (this only matters for an org whose id is `v2`).
    async fn purge_cache(&self, conn: &mut ConnectionManager, org_id: &str) -> QuarryResult<u64> {
        let escaped = glob_escape(org_id);
        let v2 = scan_unlink(
            conn,
            &format!("{V2_CACHE_PREFIX}{escaped}:*"),
            |_| false,
        )
        .await?;
        let legacy = scan_unlink(
            conn,
            &format!("{LEGACY_CACHE_PREFIX}{escaped}:*"),
            |k| k.starts_with(V2_CACHE_PREFIX),
        )
        .await?;
        Ok(v2 + legacy)
    }

    /// W2-Q-U41b: the edge seals new profiles under org-core KEKs (the provider
    /// is `org_core`, or a `KeyRouter` whose new wraps go to org-core).
    fn provider_is_org_core(&self) -> bool {
        self.keys
            .as_ref()
            .is_some_and(|k| k.current_key_id().starts_with(OC_KEY_ID_PREFIX))
    }

    fn pending(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.pending_shred
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// W2-Q-U41b: whether this purge must shred the org's KEKs. Never with the
    /// hop off. Always when the provider is `org_core` (the shred is idempotent:
    /// org-core answers 410 once the keys are destroyed and 404 once the org is
    /// gone, both done), so a retry after the rows are gone still shreds.
    /// Otherwise only when an earlier call left the shred pending or the org has
    /// an `oc:`-sealed row (a rollback to `env` after `org_core` ran). A failed
    /// probe counts as needed: an extra shred of an org being erased costs one call.
    async fn shred_needed(&self, org_id: &str) -> bool {
        if matches!(self.shred, KeyShredSetup::Off) {
            return false;
        }
        // The pending check is its own statement: the std mutex guard must not
        // live across the probe's await (the future must stay Send).
        let pending = self.pending().contains(org_id);
        let needed = if self.provider_is_org_core() || pending {
            true
        } else {
            crate::profile_backfill::org_has_oc_rows(self.blobs.as_ref(), org_id)
                .await
                .unwrap_or_else(|_| {
                    tracing::warn!("org erasure: oc-row probe failed; key shred treated as needed");
                    true
                })
        };
        if needed {
            self.pending().insert(org_id.to_string());
        }
        needed
    }

    /// W2-Q-U41b: run the crypto-shred hop and record its receipt. `None` when
    /// the hop is off. 404 and 410 from org-core are done (`AlreadyGone`); any
    /// other failure is an error, and the shred stays pending for the retry.
    async fn shred_hop(&self, org_id: &str, needed: bool) -> QuarryResult<Option<KeyShredOutcome>> {
        let shredder = match &self.shred {
            KeyShredSetup::Off => return Ok(None),
            _ if !needed => {
                tracing::info!(
                    outcome = KeyShredOutcome::NotNeeded.as_str(),
                    "org erasure: key shred receipt"
                );
                return Ok(Some(KeyShredOutcome::NotNeeded));
            }
            KeyShredSetup::Misconfigured(class) => {
                tracing::warn!(
                    class = *class,
                    "org erasure: key shred is on but not configured; the erasure stays retryable"
                );
                return Err(key_shred_err(class));
            }
            KeyShredSetup::Ready(shredder) => shredder.clone(),
        };
        match shredder.shred(org_id).await {
            Ok(outcome) => {
                self.pending().remove(org_id);
                tracing::info!(outcome = outcome.as_str(), "org erasure: key shred receipt");
                Ok(Some(outcome))
            }
            Err(e) => {
                tracing::warn!(
                    class = e.class(),
                    "org erasure: key shred failed; the erasure stays retryable"
                );
                Err(key_shred_err(e.class()))
            }
        }
    }
}

#[async_trait]
impl OrgErasable for ProfileErasure {
    fn family(&self) -> &'static str {
        "profiles"
    }

    async fn purge_org(&self, org_id: &str) -> QuarryResult<PurgeCounts> {
        if org_id.trim().is_empty() {
            return Err(QuarryError::new(ErrorCode::BadRequest, "org id is empty"));
        }
        // Database first: once the rows are gone, a cache entry can only be
        // stale, never the source of a read. A failed step returns an error and
        // the caller retries the whole purge; every step is idempotent.
        // W2-Q-U41b: whether the org's KEKs must be shredded is decided before
        // the purge, because the probe reads the rows the purge deletes.
        let shred_needed = self.shred_needed(org_id).await;
        let guarded: Vec<ProfileKind> = if self.tombstone_guard {
            self.blobs
                .list(org_id)
                .await?
                .into_iter()
                .map(|s| s.profile_id)
                .collect()
        } else {
            Vec::new()
        };
        let BlobPurge { rows, tombstones } = self.blobs.purge_org(org_id).await?;
        // Tombstones after the purge, never before it (the purge would delete them).
        for profile_id in &guarded {
            self.blobs.delete(org_id, profile_id).await?;
        }
        let cache_keys = match self.redis.clone() {
            Some(mut conn) => self.purge_cache(&mut conn, org_id).await?,
            None => 0,
        };
        let keys_evicted = match &self.keys {
            Some(keys) => {
                keys.evict_org(org_id);
                true
            }
            None => false,
        };
        // Last, after every other step of this family: a failed shred returns
        // an error (the caller retries the whole, idempotent purge) but never
        // keeps the rows, cache keys or DEKs above from going.
        let key_shred = self.shred_hop(org_id, shred_needed).await?;
        Ok(PurgeCounts {
            rows,
            tombstones,
            cache_keys,
            keys_evicted,
            guard_tombstones: guarded.len() as u64,
            key_shred,
        })
    }
}

// ---------------------------------------------------------------------------
// W2-U41a: the families beyond profiles, the inventory, and the flags.
//
// Every org-keyed table of the quarry-runtime migrations belongs to exactly one
// family below or carries a written exemption (`TABLE_EXEMPTIONS`); the
// inventory test reads the migration files and fails when a table is in
// neither list (plan section 3.6). The families beyond `profiles` register only
// when `QUARRY_ORG_ERASURE_FAMILIES` names them, so by default the internal
// route walks the profile family alone, exactly as before.
// ---------------------------------------------------------------------------

/// Env flag naming the families beyond `profiles` that the edge registers:
/// unset, empty, `off` or `none` registers none (the default); `all` registers
/// every family in [`EXTRA_FAMILIES`]; otherwise a comma list of family names.
pub const FAMILIES_ENV: &str = "QUARRY_ORG_ERASURE_FAMILIES";

/// Env flag for the profile tombstone guard (D2-077). Off by default.
pub const PROFILE_TOMBSTONE_GUARD_ENV: &str = "QUARRY_ORG_ERASURE_PROFILE_TOMBSTONE_GUARD";

/// The profile family's own name.
pub const PROFILES_FAMILY: &str = "profiles";

/// The artifact family's name (files under `org={org}/`, filesystem or S3).
pub const ARTIFACTS_FAMILY: &str = "artifacts";

/// Tables the profile family erases (`ProfileBlobStore::purge_org`).
pub const PROFILE_TABLES: &[&str] = &["quarry_profiles", "quarry_profile_tombstones"];

/// The Postgres table families and their tables, in delete order (children
/// before parents, so no foreign key blocks a statement).
pub const PG_FAMILIES: &[(&str, &[&str])] = &[
    (
        "step_receipts",
        &["quarry_browser_timeline_events", "quarry_step_receipts"],
    ),
    (
        "checkpoints",
        &["quarry_run_checkpoints", "quarry_agent_run_checkpoints"],
    ),
    (
        "frontier",
        &["quarry_queue_items", "quarry_retry_events", "quarry_request_queues"],
    ),
    ("change_baselines", &["quarry_change_diffs", "quarry_baselines"]),
    ("job_history", &["quarry_job_history"]),
];

/// Every family beyond `profiles`, in the order the edge registers them.
pub const EXTRA_FAMILIES: &[&str] = &[
    "step_receipts",
    "checkpoints",
    "frontier",
    "change_baselines",
    "job_history",
    ARTIFACTS_FAMILY,
];

/// Org-keyed tables that no family erases, each with the reason. Empty today:
/// every org-keyed table of the runtime migrations has a family.
pub const TABLE_EXEMPTIONS: &[(&str, &str)] = &[];

/// Org-scoped stores that are not Postgres tables, and where each is erased or
/// why it is not. Kept beside the table lists so the inventory is in one place.
pub const NON_TABLE_STORES: &[(&str, &str)] = &[
    (
        "redis quarry:profile:v2:{org}:* and quarry:profile:{org}:*",
        "profiles family (ProfileErasure::purge_cache)",
    ),
    (
        "artifact objects org={org}/ (filesystem root or S3 bucket)",
        "artifacts family (ArtifactErasure)",
    ),
    (
        "artifact id index (filesystem .index lines, S3 .index/{id} objects)",
        "exempt for now: the lines map an id to a key and hold no content; a \
         rewrite races the store's appends, so compaction belongs to the store \
         (depth row)",
    ),
    (
        "CAS page objects org={org}/doc={doc}/ (dataplane-cas bucket)",
        "exempt here: document-scoped, erased by the document erasure path \
         (CasStore::delete_by_doc)",
    ),
    (
        "in-memory stores (dev and test backends)",
        "exempt: process-local and gone on restart; the durable readiness gate \
         refuses them in production",
    ),
    (
        "quarry-control tables (services/quarry-control)",
        "erased by quarry-control's own PurgeOrg (WS15), not by this route",
    ),
];

/// True for the usual spellings of an enabled boolean flag.
pub fn flag_on(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1" | "true" | "on" | "yes")
    )
}

/// Which families beyond `profiles` the edge registers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FamilySelection {
    names: Vec<&'static str>,
}

impl FamilySelection {
    /// Parse [`FAMILIES_ENV`]. An unknown name is an error carrying that name,
    /// so boot refuses a typo rather than erasing less than the operator asked.
    /// `profiles` is accepted and ignored (the profile family is always on).
    pub fn parse(raw: Option<&str>) -> Result<Self, String> {
        let raw = raw.unwrap_or("").trim();
        if raw.is_empty() || raw.eq_ignore_ascii_case("off") || raw.eq_ignore_ascii_case("none") {
            return Ok(Self::default());
        }
        if raw.eq_ignore_ascii_case("all") {
            return Ok(Self {
                names: EXTRA_FAMILIES.to_vec(),
            });
        }
        let mut names: Vec<&'static str> = Vec::new();
        for token in raw.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let token = token.to_ascii_lowercase();
            if token == PROFILES_FAMILY {
                continue;
            }
            let Some(known) = EXTRA_FAMILIES.iter().copied().find(|f| *f == token) else {
                return Err(token);
            };
            if !names.contains(&known) {
                names.push(known);
            }
        }
        // Registration order is the canonical order, not the env order.
        names.sort_by_key(|n| EXTRA_FAMILIES.iter().position(|f| f == n));
        Ok(Self { names })
    }

    /// No family beyond `profiles` is selected.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The family is selected.
    pub fn includes(&self, family: &str) -> bool {
        self.names.iter().any(|n| *n == family)
    }

    /// The selected families, in registration order.
    pub fn names(&self) -> &[&'static str] {
        &self.names
    }
}

/// One Postgres table family: `DELETE ... WHERE org_id = $1` on each table, in
/// one transaction. The table names are compile-time constants from
/// [`PG_FAMILIES`], never input, so the formatted statement carries no caller
/// text; the org id is always a bound parameter.
#[cfg(feature = "postgres-queue")]
pub struct PgTableErasure {
    family: &'static str,
    tables: &'static [&'static str],
    pool: sqlx::postgres::PgPool,
}

#[cfg(feature = "postgres-queue")]
impl PgTableErasure {
    pub fn new(
        family: &'static str,
        tables: &'static [&'static str],
        pool: sqlx::postgres::PgPool,
    ) -> Self {
        Self {
            family,
            tables,
            pool,
        }
    }

    /// The selected table families over one pool, in [`PG_FAMILIES`] order.
    pub fn families(
        pool: &sqlx::postgres::PgPool,
        selection: &FamilySelection,
    ) -> Vec<Arc<dyn OrgErasable>> {
        PG_FAMILIES
            .iter()
            .filter(|(family, _)| selection.includes(family))
            .map(|&(family, tables)| {
                Arc::new(Self::new(family, tables, pool.clone())) as Arc<dyn OrgErasable>
            })
            .collect()
    }

    /// The tables this family erases.
    pub fn tables(&self) -> &'static [&'static str] {
        self.tables
    }
}

#[cfg(feature = "postgres-queue")]
fn pg_err(family: &'static str, step: &'static str) -> QuarryError {
    // The class only: a sqlx error can echo bound values.
    QuarryError::new(
        ErrorCode::Internal,
        format!("org erasure {family}: database {step} failed"),
    )
}

#[cfg(feature = "postgres-queue")]
#[async_trait]
impl OrgErasable for PgTableErasure {
    fn family(&self) -> &'static str {
        self.family
    }

    async fn purge_org(&self, org_id: &str) -> QuarryResult<PurgeCounts> {
        if org_id.trim().is_empty() {
            return Err(QuarryError::new(ErrorCode::BadRequest, "org id is empty"));
        }
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| pg_err(self.family, "begin"))?;
        let mut rows: u64 = 0;
        for table in self.tables {
            let sql = format!("DELETE FROM {table} WHERE org_id = $1");
            rows += sqlx::query(&sql)
                .bind(org_id)
                .execute(&mut *tx)
                .await
                .map_err(|_| pg_err(self.family, "delete"))?
                .rows_affected();
        }
        tx.commit()
            .await
            .map_err(|_| pg_err(self.family, "commit"))?;
        Ok(PurgeCounts {
            rows,
            ..PurgeCounts::default()
        })
    }
}

/// Where the artifact family deletes from.
pub enum ArtifactEraseTarget {
    /// `FilesystemStore` root: the org's files live under `{root}/org={org}/`.
    Filesystem { root: std::path::PathBuf },
    /// `S3Store` bucket: the org's objects live under the `org={org}/` prefix.
    S3 {
        client: aws_sdk_s3::Client,
        bucket: String,
    },
}

/// The artifact family: every stored artifact body of one org. Counts only.
/// The id index is not rewritten (see [`NON_TABLE_STORES`]).
pub struct ArtifactErasure {
    target: ArtifactEraseTarget,
}

/// The `org={org}/` prefix of one org, refusing an org id that could address
/// another org's prefix or leave the root.
pub(crate) fn artifact_org_prefix(org_id: &str) -> QuarryResult<String> {
    if org_id.trim().is_empty()
        || org_id.contains('/')
        || org_id.contains('\\')
        || org_id.contains("..")
        || org_id.contains('\0')
    {
        return Err(QuarryError::new(
            ErrorCode::BadRequest,
            "org id is not a valid artifact prefix",
        ));
    }
    Ok(format!("org={org_id}/"))
}

fn artifact_err(step: &'static str) -> QuarryError {
    QuarryError::new(
        ErrorCode::Internal,
        format!("org erasure artifacts: {step} failed"),
    )
}

/// Count the regular files under `dir` without following symlinks.
fn count_files(dir: &std::path::Path) -> std::io::Result<u64> {
    let mut n: u64 = 0;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            n += count_files(&entry.path())?;
        } else {
            n += 1;
        }
    }
    Ok(n)
}

impl ArtifactErasure {
    pub fn new(target: ArtifactEraseTarget) -> Self {
        Self { target }
    }

    pub fn filesystem(root: impl Into<std::path::PathBuf>) -> Self {
        Self::new(ArtifactEraseTarget::Filesystem { root: root.into() })
    }

    pub fn s3(client: aws_sdk_s3::Client, bucket: String) -> Self {
        Self::new(ArtifactEraseTarget::S3 { client, bucket })
    }

    /// The same client recipe as `S3Store::new` (standard AWS env).
    pub async fn s3_from_env(bucket: String) -> Self {
        let cfg = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        Self::s3(aws_sdk_s3::Client::new(&cfg), bucket)
    }

    async fn purge_filesystem(root: &std::path::Path, prefix: &str) -> QuarryResult<u64> {
        let dir = root.join(prefix.trim_end_matches('/'));
        tokio::task::spawn_blocking(move || -> std::io::Result<u64> {
            match std::fs::symlink_metadata(&dir) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
                Err(e) => return Err(e),
                // A file or a symlink where the org directory should be is not
                // followed: the store never writes one.
                Ok(meta) if !meta.is_dir() => return Ok(0),
                Ok(_) => {}
            }
            let n = count_files(&dir)?;
            std::fs::remove_dir_all(&dir)?;
            Ok(n)
        })
        .await
        .map_err(|_| artifact_err("filesystem join"))?
        .map_err(|_| artifact_err("filesystem delete"))
    }

    async fn purge_s3(client: &aws_sdk_s3::Client, bucket: &str, prefix: &str) -> QuarryResult<u64> {
        use aws_sdk_s3::types::{Delete, ObjectIdentifier};
        let mut removed: u64 = 0;
        let mut token: Option<String> = None;
        loop {
            let mut req = client.list_objects_v2().bucket(bucket).prefix(prefix);
            if let Some(t) = token.as_deref() {
                req = req.continuation_token(t);
            }
            let listed = req.send().await.map_err(|_| artifact_err("s3 list"))?;
            let keys: Vec<String> = listed
                .contents()
                .iter()
                .filter_map(|o| o.key().map(str::to_string))
                .collect();
            if !keys.is_empty() {
                let mut objects = Vec::with_capacity(keys.len());
                for k in &keys {
                    objects.push(
                        ObjectIdentifier::builder()
                            .key(k)
                            .build()
                            .map_err(|_| artifact_err("s3 object id"))?,
                    );
                }
                let delete = Delete::builder()
                    .set_objects(Some(objects))
                    .quiet(true)
                    .build()
                    .map_err(|_| artifact_err("s3 delete batch"))?;
                let out = client
                    .delete_objects()
                    .bucket(bucket)
                    .delete(delete)
                    .send()
                    .await
                    .map_err(|_| artifact_err("s3 delete"))?;
                let failed = out.errors().len() as u64;
                if failed > 0 {
                    // Retry the whole purge; deletes are idempotent.
                    return Err(artifact_err("s3 delete (partial)"));
                }
                removed += keys.len() as u64;
            }
            match listed.next_continuation_token() {
                Some(t) if !t.is_empty() => token = Some(t.to_string()),
                _ => break,
            }
        }
        Ok(removed)
    }
}

#[async_trait]
impl OrgErasable for ArtifactErasure {
    fn family(&self) -> &'static str {
        ARTIFACTS_FAMILY
    }

    async fn purge_org(&self, org_id: &str) -> QuarryResult<PurgeCounts> {
        let prefix = artifact_org_prefix(org_id)?;
        let rows = match &self.target {
            ArtifactEraseTarget::Filesystem { root } => {
                Self::purge_filesystem(root, &prefix).await?
            }
            ArtifactEraseTarget::S3 { client, bucket } => {
                Self::purge_s3(client, bucket, &prefix).await?
            }
        };
        Ok(PurgeCounts {
            rows,
            ..PurgeCounts::default()
        })
    }
}

// ---------------------------------------------------------------------------
// W2-Q-U41b: the cryptographic erasure hop (closes D2-093 in code).
//
// The profile family ends by asking org-core to destroy every KEK of the org
// (`DELETE /internal/orgs/:org/keys`, scope `org:keys:destroy:any`). After that
// no `oc:`-sealed envelope of the org can be unwrapped, wherever a copy of it
// survived. The hop is cryptographic only for rows sealed under org-core; rows
// sealed by the `env` provider are erased by the row delete alone.
// ---------------------------------------------------------------------------

/// Env flag for the crypto-shred hop. Off by default (1/true/on/yes turns it on).
/// The org-core URL and token are the key provider's
/// (`QUARRY_EDGE__ORG_CORE_KEYS_URL`, `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN`).
pub const ERASE_SHRED_KEYS_ENV: &str = "QUARRY_EDGE__ERASE_SHRED_KEYS";

/// Timeout of one shred call (connect and request). org-core's own handler
/// timeout is 5 s.
pub const KEY_SHRED_TIMEOUT: Duration = Duration::from_secs(5);

/// Longest org id org-core accepts on its key routes.
const KEY_SHRED_ORG_ID_MAX_LEN: usize = 128;

/// The receipt of one crypto-shred hop. Closed labels only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyShredOutcome {
    /// org-core destroyed the org's KEKs on this call (2xx).
    Destroyed,
    /// org-core answered 404 (the org is gone, its keys went with it) or 410
    /// (the keys were destroyed before): done.
    AlreadyGone,
    /// No shred was needed: the provider is not `org_core` and the org had no
    /// `oc:`-sealed row.
    NotNeeded,
}

impl KeyShredOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Destroyed => "destroyed",
            Self::AlreadyGone => "already_gone",
            Self::NotNeeded => "not_needed",
        }
    }
}

/// Why a shred did not complete. Every class keeps the erasure retryable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyShredError {
    /// org-core is down, slow, rate limiting, or answered 5xx (after one retry).
    Unavailable,
    /// 401 or 403: the credential drifted, or it lacks `org:keys:destroy:any`.
    Denied,
    /// Any other answer, or an org id org-core would refuse.
    Invalid,
}

impl KeyShredError {
    pub fn class(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Denied => "denied",
            Self::Invalid => "invalid",
        }
    }
}

/// Map one org-core status of `DELETE .../keys` to the hop's result.
pub fn key_shred_status(status: u16) -> Result<KeyShredOutcome, KeyShredError> {
    match status {
        200..=299 => Ok(KeyShredOutcome::Destroyed),
        404 | 410 => Ok(KeyShredOutcome::AlreadyGone),
        401 | 403 => Err(KeyShredError::Denied),
        408 | 429 => Err(KeyShredError::Unavailable),
        s if s >= 500 => Err(KeyShredError::Unavailable),
        _ => Err(KeyShredError::Invalid),
    }
}

fn key_shred_err(class: &str) -> QuarryError {
    QuarryError::new(
        ErrorCode::RuntimeNotReady,
        format!("org erasure profiles: key shred failed ({class}); retry the erasure"),
    )
}

/// The org-core client of the crypto-shred hop. Same principal and headers as
/// the key provider (`X-Service-Id: quarry-edge`, `X-Service-Token`), no
/// redirects, one retry on a 5xx or a transport error. The response body is
/// never read and nothing about the call but its class is logged.
pub struct OrgKeyShredder {
    client: reqwest::Client,
    base: reqwest::Url,
    token: reqwest::header::HeaderValue,
}

impl OrgKeyShredder {
    /// Build from a base URL and the service token. Refuses a blank or malformed
    /// value with the provider's config classes.
    pub fn new(base_url: &str, token: &str) -> Result<Self, OrgCoreConfigError> {
        let base_url = base_url.trim();
        if base_url.is_empty() {
            return Err(OrgCoreConfigError::MissingUrl);
        }
        let base = reqwest::Url::parse(base_url).map_err(|_| OrgCoreConfigError::BadUrl)?;
        if !matches!(base.scheme(), "http" | "https")
            || base.host_str().is_none()
            || base.cannot_be_a_base()
            || base.query().is_some()
        {
            return Err(OrgCoreConfigError::BadUrl);
        }
        let token = token.trim();
        if token.is_empty() {
            return Err(OrgCoreConfigError::MissingToken);
        }
        let mut token = reqwest::header::HeaderValue::from_str(token)
            .map_err(|_| OrgCoreConfigError::MissingToken)?;
        token.set_sensitive(true);
        let client = reqwest::Client::builder()
            .timeout(KEY_SHRED_TIMEOUT)
            .connect_timeout(KEY_SHRED_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| OrgCoreConfigError::Client)?;
        Ok(Self {
            client,
            base,
            token,
        })
    }

    /// Build from the environment: the URL from `url` or
    /// `QUARRY_EDGE__ORG_CORE_KEYS_URL`, the token from
    /// `QUARRY_EDGE__ORG_CORE_KEYS_TOKEN` (read by name, never logged).
    pub fn from_env(url: Option<&str>) -> Result<Self, OrgCoreConfigError> {
        let url = match url.filter(|u| !u.trim().is_empty()) {
            Some(u) => u.to_string(),
            None => std::env::var(ORG_CORE_KEYS_URL_ENV).unwrap_or_default(),
        };
        let token = zeroize::Zeroizing::new(std::env::var(ORG_CORE_KEYS_TOKEN_ENV).unwrap_or_default());
        Self::new(&url, &token)
    }

    fn url_for(&self, org_id: &str) -> Result<reqwest::Url, KeyShredError> {
        if org_id.is_empty()
            || org_id.len() > KEY_SHRED_ORG_ID_MAX_LEN
            || org_id.trim() != org_id
            || org_id.chars().any(char::is_control)
        {
            return Err(KeyShredError::Invalid);
        }
        let mut url = self.base.clone();
        {
            let mut segments = url.path_segments_mut().map_err(|_| KeyShredError::Invalid)?;
            segments
                .pop_if_empty()
                .extend(["internal", "orgs", org_id, "keys"]);
        }
        Ok(url)
    }

    /// Destroy every KEK of `org_id` in org-core. Idempotent: a second call
    /// answers 410 (or 404 once the org is hard-deleted), which is `AlreadyGone`.
    pub async fn shred(&self, org_id: &str) -> Result<KeyShredOutcome, KeyShredError> {
        let url = self.url_for(org_id)?;
        let mut last = Err(KeyShredError::Unavailable);
        for attempt in 0..2 {
            let sent = self
                .client
                .delete(url.clone())
                .header("x-service-id", ORG_CORE_KEYS_PRINCIPAL)
                .header("x-service-token", self.token.clone())
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await;
            match sent {
                Ok(resp) => {
                    // The body (org id, state, a count) is not needed and not read.
                    let status = resp.status().as_u16();
                    last = key_shred_status(status);
                    if status >= 500 && attempt == 0 {
                        continue;
                    }
                    return last;
                }
                Err(_) => last = Err(KeyShredError::Unavailable),
            }
        }
        last
    }
}

/// How the crypto-shred hop is set up for one edge process.
#[derive(Clone)]
pub enum KeyShredSetup {
    /// The flag is off (the default): no shred, no receipt.
    Off,
    /// The flag is on and the org-core client is built.
    Ready(Arc<OrgKeyShredder>),
    /// The flag is on but the URL or token is missing or malformed (the class).
    /// Every erasure that needs a shred fails, so the gap is loud and the
    /// erasure is retried once the setting is fixed, instead of erasing less
    /// than the operator asked.
    Misconfigured(&'static str),
}

impl KeyShredSetup {
    /// Read [`ERASE_SHRED_KEYS_ENV`] and, when it is on, build the client from
    /// the key provider's org-core settings.
    pub fn from_env() -> Self {
        if !flag_on(std::env::var(ERASE_SHRED_KEYS_ENV).ok().as_deref()) {
            return Self::Off;
        }
        match OrgKeyShredder::from_env(None) {
            Ok(shredder) => {
                tracing::info!("org erasure: key shred hop on (org-core DELETE keys after the profile purge)");
                Self::Ready(Arc::new(shredder))
            }
            Err(e) => {
                tracing::warn!(
                    class = e.class(),
                    "org erasure: QUARRY_EDGE__ERASE_SHRED_KEYS is on but the org-core keys URL or token is not usable; erasures that need a shred fail until fixed"
                );
                Self::Misconfigured(e.class())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;
    use chrono::Utc;

    use quarry_browser::session::{SaveMeta, SessionSnapshot};
    use quarry_core::ids::kinds::ProfileKind;

    use crate::profile_blob_store::{BlobPayload, InMemoryBlobStore};
    use crate::profile_keys::EnvKeyProvider;

    fn legacy_payload() -> BlobPayload {
        BlobPayload::Legacy {
            json: serde_json::to_value(SessionSnapshot::default()).expect("snapshot json"),
        }
    }

    fn meta() -> SaveMeta {
        SaveMeta {
            saved_at: Utc::now(),
            owner: None,
        }
    }

    fn keys() -> Arc<dyn ProfileKeyProvider> {
        let json = format!(r#"{{"k1":"{}"}}"#, B64.encode([7u8; 32]));
        Arc::new(EnvKeyProvider::from_config(&json, "k1").expect("test master key"))
    }

    #[test]
    fn glob_escape_neutralises_pattern_characters() {
        assert_eq!(glob_escape("org_a"), "org_a");
        assert_eq!(glob_escape("a*b?c[d]e\\f"), "a\\*b\\?c\\[d\\]e\\\\f");
    }

    /// Erasure removes the org's rows and tombstones, leaves another org alone,
    /// evicts the cached DEKs, and (when REDIS_URL is set) drops the org's
    /// envelope and legacy cache keys and nothing of another org.
    #[tokio::test]
    async fn purge_org_deletes_rows_tombstones_and_cache_keys() {
        let store = Arc::new(InMemoryBlobStore::new());
        let org_a = format!("org_erase_a_{}", ProfileKind::new());
        let org_b = format!("org_erase_b_{}", ProfileKind::new());
        let p1 = ProfileKind::new();
        let p2 = ProfileKind::new();
        let p3 = ProfileKind::new();
        store.put(&org_a, &p1, legacy_payload(), &meta()).await.unwrap();
        store.put(&org_a, &p2, legacy_payload(), &meta()).await.unwrap();
        store.put(&org_b, &p3, legacy_payload(), &meta()).await.unwrap();
        // Deleting p2 leaves a tombstone for org_a.
        store.delete(&org_a, &p2).await.unwrap();

        let key_provider = keys();
        // Populate the DEK cache for org_a so the eviction is observable.
        let g = key_provider.generate(&org_a, &p1.to_string()).unwrap();
        let _ = key_provider
            .unwrap(&org_a, &p1.to_string(), &g.key_id, &g.wrapped)
            .unwrap();

        let erasure = ProfileErasure::new(store.clone(), Some(key_provider), None);
        let counts = erasure.purge_org(&org_a).await.expect("purge");
        assert_eq!(counts.rows, 1, "org_a had one row left after the delete");
        assert_eq!(counts.tombstones, 1, "org_a had one tombstone");
        assert!(counts.keys_evicted);
        assert_eq!(counts.cache_keys, 0, "no Redis connection in this run");
        assert!(!store.exists(&org_a, &p1).await.unwrap());
        assert!(store.exists(&org_b, &p3).await.unwrap(), "other org untouched");
        // A second purge is a no-op.
        let again = erasure.purge_org(&org_a).await.expect("second purge");
        assert_eq!(again.rows, 0);
        assert_eq!(again.tombstones, 0);

        let Ok(url) = std::env::var("REDIS_URL") else {
            return;
        };
        let client = redis::Client::open(url.as_str()).expect("redis client");
        let mut conn = ConnectionManager::new(client).await.expect("redis");
        let v2_a = format!("{V2_CACHE_PREFIX}{org_a}:{p1}");
        let legacy_a = format!("{LEGACY_CACHE_PREFIX}{org_a}:{p1}");
        let v2_b = format!("{V2_CACHE_PREFIX}{org_b}:{p3}");
        for k in [&v2_a, &legacy_a, &v2_b] {
            let _: () = redis::cmd("SET").arg(k).arg("x").query_async(&mut conn).await.unwrap();
        }
        let erasure = ProfileErasure::new(store.clone(), None, Some(conn.clone()));
        let counts = erasure.purge_org(&org_a).await.expect("purge with redis");
        assert_eq!(counts.cache_keys, 2, "the org's v2 and legacy keys");
        let exists = |k: &str| {
            let k = k.to_string();
            let mut c = conn.clone();
            async move {
                let n: u64 = redis::cmd("EXISTS").arg(k).query_async(&mut c).await.unwrap();
                n
            }
        };
        assert_eq!(exists(&v2_a).await, 0);
        assert_eq!(exists(&legacy_a).await, 0);
        assert_eq!(exists(&v2_b).await, 1, "another org's key survives");
        let _: u64 = redis::cmd("UNLINK").arg(&v2_b).query_async(&mut conn).await.unwrap();
    }
}

/// W2-U41a: the inventory test (plan section 3.6), the selection flag, the
/// tombstone guard (D2-077), the artifact family, and the DSN-gated table
/// families.
#[cfg(test)]
mod inventory_tests {
    use super::*;
    use std::collections::BTreeSet;

    use chrono::{Duration as ChronoDuration, Utc};

    use quarry_browser::session::{SaveMeta, SessionSnapshot};
    use quarry_core::ids::kinds::{ProfileKind, RunKind};
    use quarry_core::ids::Id;

    use crate::artifact_store::{ArtifactStore, FilesystemStore};
    use crate::profile_blob_store::{BlobPayload, InMemoryBlobStore};

    /// Strip `--` comments so a comment that names `org_id` cannot count.
    fn strip_comments(sql: &str) -> String {
        sql.replace('\r', "")
            .lines()
            .map(|l| match l.find("--") {
                Some(i) => &l[..i],
                None => l,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Every table of the runtime migrations that has an `org_id` column,
    /// whether it is created with one or gains one later.
    fn org_keyed_tables() -> (BTreeSet<String>, BTreeSet<String>) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let create = regex::Regex::new(
            r"(?is)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([a-z0-9_]+)\s*\((.*?)\n\s*\)\s*;",
        )
        .unwrap();
        let column = regex::Regex::new(r"(?im)^\s*org_id\s+").unwrap();
        let alter = regex::Regex::new(
            r"(?is)ALTER\s+TABLE\s+(?:IF\s+EXISTS\s+)?(?:ONLY\s+)?([a-z0-9_]+)\s+ADD\s+(?:COLUMN\s+)?(?:IF\s+NOT\s+EXISTS\s+)?org_id\b",
        )
        .unwrap();
        let mut all = BTreeSet::new();
        let mut keyed = BTreeSet::new();
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("migrations dir")
            .map(|e| e.expect("dir entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "sql"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "no migration files found");
        for path in files {
            let sql = strip_comments(&std::fs::read_to_string(&path).expect("migration"));
            for cap in create.captures_iter(&sql) {
                let table = cap[1].to_ascii_lowercase();
                all.insert(table.clone());
                if column.is_match(&cap[2]) {
                    keyed.insert(table);
                }
            }
            for cap in alter.captures_iter(&sql) {
                keyed.insert(cap[1].to_ascii_lowercase());
            }
        }
        (all, keyed)
    }

    fn hop_tables() -> BTreeSet<&'static str> {
        PROFILE_TABLES
            .iter()
            .copied()
            .chain(PG_FAMILIES.iter().flat_map(|(_, t)| t.iter().copied()))
            .collect()
    }

    #[test]
    fn every_org_keyed_table_has_an_erasure_hop_or_a_written_exemption() {
        let (_, keyed) = org_keyed_tables();
        // The parse must see the known tables, so a broken regex cannot pass.
        for known in ["quarry_profiles", "quarry_step_receipts", "quarry_job_history"] {
            assert!(keyed.contains(known), "inventory parse missed {known}");
        }
        let hops = hop_tables();
        let exempt: BTreeSet<&str> = TABLE_EXEMPTIONS.iter().map(|(t, _)| *t).collect();
        let missing: Vec<&String> = keyed
            .iter()
            .filter(|t| !hops.contains(t.as_str()) && !exempt.contains(t.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "org-keyed tables with no erasure hop and no exemption: {missing:?}; \
             add each to a family in PG_FAMILIES or to TABLE_EXEMPTIONS with a reason"
        );
    }

    #[test]
    fn every_hop_table_exists_and_is_org_keyed() {
        let (all, keyed) = org_keyed_tables();
        for table in hop_tables() {
            assert!(all.contains(table), "hop names a table no migration creates: {table}");
            assert!(keyed.contains(table), "hop table has no org_id column: {table}");
        }
    }

    #[test]
    fn exemptions_have_reasons_and_never_overlap_a_hop() {
        let hops = hop_tables();
        for (table, reason) in TABLE_EXEMPTIONS {
            assert!(!reason.trim().is_empty(), "exemption without a reason: {table}");
            assert!(!hops.contains(table), "table is both a hop and exempt: {table}");
        }
        for (store, reason) in NON_TABLE_STORES {
            assert!(!store.trim().is_empty() && !reason.trim().is_empty());
        }
        // A table sits in one family only.
        let mut seen = BTreeSet::new();
        for t in PROFILE_TABLES
            .iter()
            .chain(PG_FAMILIES.iter().flat_map(|(_, t)| t.iter()))
        {
            assert!(seen.insert(*t), "table in two families: {t}");
        }
        // Every table family is a selectable family.
        for (family, _) in PG_FAMILIES {
            assert!(EXTRA_FAMILIES.contains(family), "{family} is not selectable");
        }
    }

    #[test]
    fn family_selection_parses_off_all_and_lists() {
        for off in [None, Some(""), Some(" off "), Some("NONE"), Some("profiles")] {
            assert!(FamilySelection::parse(off).unwrap().is_empty(), "{off:?}");
        }
        let all = FamilySelection::parse(Some("all")).unwrap();
        assert_eq!(all.names(), EXTRA_FAMILIES);
        let some = FamilySelection::parse(Some("job_history, Step_Receipts,job_history")).unwrap();
        assert_eq!(some.names(), &["step_receipts", "job_history"]);
        assert!(some.includes("step_receipts"));
        assert!(!some.includes(ARTIFACTS_FAMILY));
        assert_eq!(
            FamilySelection::parse(Some("step_receipts,receipt")).unwrap_err(),
            "receipt"
        );
        assert!(flag_on(Some(" TRUE ")));
        assert!(flag_on(Some("1")));
        assert!(!flag_on(Some("0")));
        assert!(!flag_on(None));
    }

    fn legacy_payload() -> BlobPayload {
        BlobPayload::Legacy {
            json: serde_json::to_value(SessionSnapshot::default()).expect("snapshot json"),
        }
    }

    fn meta_at(saved_at: chrono::DateTime<Utc>) -> SaveMeta {
        SaveMeta {
            saved_at,
            owner: None,
        }
    }

    /// D2-077: without the guard, a save that started before the purge and lands
    /// after it recreates the erased row (the purge removed the tombstones). With
    /// the guard, the purge leaves a tombstone per erased profile and the late
    /// save is refused.
    #[tokio::test]
    async fn tombstone_guard_refuses_a_late_save_after_the_purge() {
        let before = Utc::now() - ChronoDuration::seconds(5);
        for guard in [false, true] {
            let store = Arc::new(InMemoryBlobStore::new());
            let org = format!("org_guard_{}", ProfileKind::new());
            let p = ProfileKind::new();
            store.put(&org, &p, legacy_payload(), &meta_at(before)).await.unwrap();

            let erasure =
                ProfileErasure::new(store.clone(), None, None).with_tombstone_guard(guard);
            let counts = erasure.purge_org(&org).await.expect("purge");
            assert_eq!(counts.rows, 1);
            assert_eq!(counts.guard_tombstones, u64::from(guard));
            assert!(!store.exists(&org, &p).await.unwrap());

            // A save captured before the purge arrives after it.
            let _ = store
                .put(&org, &p, legacy_payload(), &meta_at(before + ChronoDuration::seconds(1)))
                .await;
            assert_eq!(
                store.exists(&org, &p).await.unwrap(),
                !guard,
                "guard={guard}: the late save recreates the row only without the guard"
            );
        }
    }

    #[test]
    fn artifact_prefix_refuses_path_tricks() {
        assert_eq!(artifact_org_prefix("org_a").unwrap(), "org=org_a/");
        for bad in ["", "  ", "a/b", "a\\b", "..", "a..b", "a\0b"] {
            assert!(artifact_org_prefix(bad).is_err(), "{bad:?}");
        }
    }

    /// The artifact family removes one org's files written by the real store
    /// layout, leaves another org's files, and is idempotent.
    #[tokio::test]
    async fn artifact_family_purges_one_org_on_the_filesystem() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = FilesystemStore::new(tmp.path()).expect("fs store");
        let run_a: RunKind = Id::new();
        let run_b: RunKind = Id::new();
        let a1 = store
            .put("org_art_a", &run_a, "p1", "html", b"a1".to_vec())
            .await
            .unwrap();
        let _a2 = store
            .put("org_art_a", &run_a, "p2", "markdown", b"a2".to_vec())
            .await
            .unwrap();
        let b1 = store
            .put("org_art_b", &run_b, "p1", "html", b"b1".to_vec())
            .await
            .unwrap();

        let erasure = ArtifactErasure::filesystem(tmp.path());
        assert_eq!(erasure.family(), ARTIFACTS_FAMILY);
        let counts = erasure.purge_org("org_art_a").await.expect("purge");
        assert_eq!(counts.rows, 2, "both files of org_art_a");
        assert!(!tmp.path().join("org=org_art_a").exists());
        assert!(store.get("org_art_a", &a1.artifact_id).await.is_err(), "the body is gone");
        assert_eq!(store.get("org_art_b", &b1.artifact_id).await.unwrap(), b"b1".to_vec());
        assert_eq!(erasure.purge_org("org_art_a").await.unwrap().rows, 0);
        assert!(erasure.purge_org("../org_art_b").await.is_err());
    }

    /// Canary rows in every table family for two orgs; the purge of one org
    /// empties its rows in every family table and leaves the other org's rows.
    /// Needs a Postgres DSN in DATABASE_URL; skipped without it.
    #[cfg(feature = "postgres-queue")]
    #[tokio::test]
    async fn table_families_purge_one_org_and_keep_another() {
        let Ok(dsn) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };
        let pool = sqlx::postgres::PgPool::connect(&dsn).await.expect("connect");
        crate::migrations::run_migrations(&pool).await.expect("migrations");
        let org_a = format!("org_u41a_a_{}", ulid::Ulid::new());
        let org_b = format!("org_u41a_b_{}", ulid::Ulid::new());
        for org in [&org_a, &org_b] {
            let run = ulid::Ulid::new().to_string();
            sqlx::query(
                "INSERT INTO quarry_browser_timeline_events \
                 (event_id, org_id, actor_id, run_id, occurred_at, event) \
                 VALUES ($1, $2, 'actor', $3, NOW(), '{}'::jsonb)",
            )
            .bind(ulid::Ulid::new().to_string())
            .bind(org)
            .bind(&run)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO quarry_agent_run_checkpoints (org_id, run_id, profile_id, state) \
                 VALUES ($1, $2, 'p', '{}'::jsonb)",
            )
            .bind(org)
            .bind(&run)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO quarry_run_checkpoints (run_id, org_id, state) \
                 VALUES ($1, $2, '{}'::jsonb)",
            )
            .bind(&run)
            .bind(org)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO quarry_retry_events \
                 (org_id, request_id, attempt, error_code, retry_class) \
                 VALUES ($1, 'r1', 1, 'timeout', 'transient')",
            )
            .bind(org)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO quarry_job_history \
                 (event_id, run_id, org_id, kind, stage, status, seq, ts) \
                 VALUES ($1, $2, $3, 'crawl', 'queued', 'ok', 1, NOW())",
            )
            .bind(ulid::Ulid::new().to_string())
            .bind(&run)
            .bind(org)
            .execute(&pool)
            .await
            .unwrap();
        }

        let all = FamilySelection::parse(Some("all")).unwrap();
        let families = PgTableErasure::families(&pool, &all);
        assert_eq!(families.len(), PG_FAMILIES.len());
        let mut removed = 0;
        for family in &families {
            removed += family.purge_org(&org_a).await.expect("purge").rows;
        }
        assert_eq!(removed, 5, "one canary row in each seeded table");

        let count = |table: &'static str, org: String| {
            let pool = pool.clone();
            async move {
                let (n,): (i64,) =
                    sqlx::query_as(&format!("SELECT COUNT(*) FROM {table} WHERE org_id = $1"))
                        .bind(org)
                        .fetch_one(&pool)
                        .await
                        .unwrap();
                n
            }
        };
        for (_, tables) in PG_FAMILIES {
            for &table in tables.iter() {
                assert_eq!(count(table, org_a.clone()).await, 0, "{table} still holds org_a");
            }
        }
        for table in [
            "quarry_browser_timeline_events",
            "quarry_agent_run_checkpoints",
            "quarry_run_checkpoints",
            "quarry_retry_events",
            "quarry_job_history",
        ] {
            assert_eq!(count(table, org_b.clone()).await, 1, "{table} lost org_b's row");
        }
        // Idempotent.
        for family in &families {
            assert_eq!(family.purge_org(&org_a).await.unwrap().rows, 0);
        }
        for family in &families {
            let _ = family.purge_org(&org_b).await;
        }
    }
}

/// W2-Q-U41b: the crypto-shred hop against a wiremock org-core.
#[cfg(test)]
mod key_shred_tests {
    use super::*;
    use chrono::Utc;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use quarry_browser::session::SaveMeta;
    use quarry_core::ids::kinds::ProfileKind;

    use crate::profile_blob_store::{BlobPayload, InMemoryBlobStore};
    use crate::profile_keys::OrgCoreKeyProvider;

    const TOKEN: &str = "shred-test-token-not-a-secret";

    fn meta() -> SaveMeta {
        SaveMeta {
            saved_at: Utc::now(),
            owner: None,
        }
    }

    fn oc_payload() -> BlobPayload {
        BlobPayload::Envelope {
            envelope: b"{}".to_vec(),
            enc_v: 1,
            key_id: "oc:1".to_string(),
        }
    }

    fn shredder(server: &MockServer) -> KeyShredSetup {
        KeyShredSetup::Ready(Arc::new(
            OrgKeyShredder::new(&server.uri(), TOKEN).expect("shredder"),
        ))
    }

    fn keys_path(org: &str) -> String {
        format!("/internal/orgs/{org}/keys")
    }

    fn destroyed_body(org: &str) -> serde_json::Value {
        serde_json::json!({"data": {"org_id": org, "state": "destroyed", "versions_destroyed": 1}})
    }

    async fn seeded(org: &str) -> Arc<InMemoryBlobStore> {
        let store = Arc::new(InMemoryBlobStore::new());
        store
            .put(org, &ProfileKind::new(), oc_payload(), &meta())
            .await
            .unwrap();
        store
    }

    #[test]
    fn key_shred_status_maps_done_and_retryable_answers() {
        assert_eq!(key_shred_status(200), Ok(KeyShredOutcome::Destroyed));
        assert_eq!(key_shred_status(204), Ok(KeyShredOutcome::Destroyed));
        assert_eq!(key_shred_status(404), Ok(KeyShredOutcome::AlreadyGone));
        assert_eq!(key_shred_status(410), Ok(KeyShredOutcome::AlreadyGone));
        assert_eq!(key_shred_status(401), Err(KeyShredError::Denied));
        assert_eq!(key_shred_status(403), Err(KeyShredError::Denied));
        assert_eq!(key_shred_status(429), Err(KeyShredError::Unavailable));
        assert_eq!(key_shred_status(503), Err(KeyShredError::Unavailable));
        assert_eq!(key_shred_status(400), Err(KeyShredError::Invalid));
        assert!(OrgKeyShredder::new("", TOKEN).is_err());
        assert!(OrgKeyShredder::new("ftp://org-core", TOKEN).is_err());
        assert!(OrgKeyShredder::new("http://org-core:8080", " ").is_err());
    }

    /// The hop off (the default) makes no call and leaves no receipt.
    #[tokio::test]
    async fn shred_is_off_by_default() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let org = format!("org_shred_off_{}", ProfileKind::new());
        let store = seeded(&org).await;
        let counts = ProfileErasure::new(store, None, None)
            .purge_org(&org)
            .await
            .expect("purge");
        assert_eq!(counts.rows, 1);
        assert_eq!(counts.key_shred, None);
    }

    /// With the org-core provider, every purge shreds exactly once: the first
    /// destroys the KEKs, the second meets 410 and counts it as done.
    #[tokio::test]
    async fn shred_is_called_once_per_purge_and_is_idempotent() {
        let server = MockServer::start().await;
        let org = format!("org_shred_a_{}", ProfileKind::new());
        Mock::given(method("DELETE"))
            .and(path(keys_path(&org)))
            .and(header("x-service-id", ORG_CORE_KEYS_PRINCIPAL))
            .and(header("x-service-token", TOKEN))
            .respond_with(ResponseTemplate::new(200).set_body_json(destroyed_body(&org)))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(keys_path(&org)))
            .respond_with(ResponseTemplate::new(410))
            .expect(1)
            .mount(&server)
            .await;
        let store = seeded(&org).await;
        let keys: Arc<dyn ProfileKeyProvider> =
            Arc::new(OrgCoreKeyProvider::new(&server.uri(), TOKEN).expect("provider"));
        let erasure =
            ProfileErasure::new(store.clone(), Some(keys), None).with_key_shred(shredder(&server));

        let first = erasure.purge_org(&org).await.expect("first purge");
        assert_eq!(first.rows, 1);
        assert_eq!(first.key_shred, Some(KeyShredOutcome::Destroyed));
        let second = erasure.purge_org(&org).await.expect("second purge");
        assert_eq!(second.rows, 0);
        assert_eq!(second.key_shred, Some(KeyShredOutcome::AlreadyGone));
        drop(erasure);
        server.verify().await;
    }

    /// Without the org-core provider, only an org with an `oc:` row is shredded;
    /// an org without one gets a `not_needed` receipt and no call.
    #[tokio::test]
    async fn shred_follows_oc_rows_when_the_provider_is_env() {
        let server = MockServer::start().await;
        let org_oc = format!("org_shred_oc_{}", ProfileKind::new());
        let org_plain = format!("org_shred_plain_{}", ProfileKind::new());
        Mock::given(method("DELETE"))
            .and(path(keys_path(&org_oc)))
            .respond_with(ResponseTemplate::new(200).set_body_json(destroyed_body(&org_oc)))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(keys_path(&org_plain)))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let store = seeded(&org_oc).await;
        store
            .put(
                &org_plain,
                &ProfileKind::new(),
                BlobPayload::Envelope {
                    envelope: b"{}".to_vec(),
                    enc_v: 1,
                    key_id: "env:k1".to_string(),
                },
                &meta(),
            )
            .await
            .unwrap();
        let erasure = ProfileErasure::new(store.clone(), None, None).with_key_shred(shredder(&server));

        let oc = erasure.purge_org(&org_oc).await.expect("oc purge");
        assert_eq!(oc.key_shred, Some(KeyShredOutcome::Destroyed));
        let plain = erasure.purge_org(&org_plain).await.expect("plain purge");
        assert_eq!(plain.rows, 1);
        assert_eq!(plain.key_shred, Some(KeyShredOutcome::NotNeeded));
        // The oc org again: the shred answered, nothing is pending, no row is left.
        let again = erasure.purge_org(&org_oc).await.expect("oc again");
        assert_eq!(again.key_shred, Some(KeyShredOutcome::NotNeeded));
        server.verify().await;
    }

    /// A failed shred fails the purge (the caller retries) after the rows are
    /// gone, and the retry still shreds although the probe no longer sees an
    /// `oc:` row.
    #[tokio::test]
    async fn a_failed_shred_keeps_the_erasure_retryable() {
        let server = MockServer::start().await;
        let org = format!("org_shred_retry_{}", ProfileKind::new());
        // 503 twice: the first call and its one retry.
        Mock::given(method("DELETE"))
            .and(path(keys_path(&org)))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path(keys_path(&org)))
            .respond_with(ResponseTemplate::new(200).set_body_json(destroyed_body(&org)))
            .expect(1)
            .mount(&server)
            .await;
        let store = seeded(&org).await;
        let erasure = ProfileErasure::new(store.clone(), None, None).with_key_shred(shredder(&server));

        let err = erasure.purge_org(&org).await.expect_err("shred failed");
        assert_eq!(err.code, ErrorCode::RuntimeNotReady);
        assert!(!err.message.contains(&org), "the error carries the class only");
        assert!(store.list(&org).await.unwrap().is_empty(), "the rows went anyway");

        let retry = erasure.purge_org(&org).await.expect("retry");
        assert_eq!(retry.rows, 0);
        assert_eq!(retry.key_shred, Some(KeyShredOutcome::Destroyed));
        server.verify().await;
    }

    /// The flag on without a usable URL or token fails every erasure that needs a
    /// shred, never silently skips it.
    #[tokio::test]
    async fn a_misconfigured_shred_fails_loudly() {
        let org = format!("org_shred_cfg_{}", ProfileKind::new());
        let store = seeded(&org).await;
        let erasure = ProfileErasure::new(store.clone(), None, None)
            .with_key_shred(KeyShredSetup::Misconfigured("missing_token"));
        let err = erasure.purge_org(&org).await.expect_err("misconfigured");
        assert!(err.message.contains("missing_token"));
        assert!(store.list(&org).await.unwrap().is_empty(), "the rows went anyway");
    }
}
