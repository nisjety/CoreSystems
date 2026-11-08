//! Session restore/capture shape and profile store.
//!
//! `SessionSnapshot` carries all browser state needed to resume a session:
//! cookies, web storage, viewport, locale, timezone, and user-agent.
//!
//! `ProfileStore` is the persistence trait — implementations range from
//! in-memory (tests) to S3 (production).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use quarry_core::ids::kinds::ProfileKind;
use quarry_core::QuarryResult;

pub use crate::profile_snapshot::{
    CookiePriority, IdbDatabase, IdbIndex, IdbRecord, IdbStore, OriginState, PartitionKey,
    SameSite, SourceScheme, SNAPSHOT_VERSION,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub cookies: Vec<Cookie>,
    pub local_storage: Vec<(String, String)>,
    pub session_storage: Vec<(String, String)>,
    /// Cycle 20 / cluster #13 — IndexedDB capture. Per-origin entries
    /// keyed `"<origin>::<db>::<store>::<key>"` so a snapshot can
    /// faithfully restore object-store records that auth-gated SPAs
    /// (Notion, Linear, Figma, etc.) require. Empty for back-compat
    /// when the driver couldn't or didn't capture IndexedDB state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexed_db: Vec<IndexedDbEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Schema version (WS17.C1). 0 on a legacy snapshot, 2 when capture v2 wrote it. The
    /// flat `local_storage` / `session_storage` fields above stay readable: a legacy
    /// snapshot restores them into the initial URL's origin ([`Self::effective_origins`]).
    #[serde(default, skip_serializing_if = "crate::profile_snapshot::is_zero_u8")]
    pub snapshot_version: u8,
    /// Per-origin web storage and IndexedDB (capture v2). Empty on a legacy snapshot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<OriginState>,
    /// When the capture that produced this snapshot started (WS17.C2). Restore uses it for
    /// the age of a session cookie and reports it as `snapshot_age_s`. Absent on a snapshot
    /// written before this field existed: its age is unknown and nothing is skipped for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub captured_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl SessionSnapshot {
    /// Anything worth keeping: a cookie, any web storage, IndexedDB or an origin entry.
    pub fn has_state(&self) -> bool {
        !self.cookies.is_empty()
            || !self.local_storage.is_empty()
            || !self.session_storage.is_empty()
            || !self.indexed_db.is_empty()
            || !self.origins.is_empty()
    }

    /// The nearest expiry among cookies that have not expired at `now`. Session cookies
    /// have none and do not count.
    pub fn earliest_cookie_expiry(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        self.cookies
            .iter()
            .filter_map(|cookie| cookie.expires)
            .filter(|at| *at > now)
            .min()
    }

    /// The per-origin state a restore works from: `origins` when present, else one entry
    /// holding the legacy flat web storage under the initial URL's origin (nothing when that
    /// URL is not http(s) or the flat fields are empty).
    pub fn effective_origins(&self, initial_url: &str) -> Vec<OriginState> {
        if !self.origins.is_empty() {
            return self.origins.clone();
        }
        let Some(origin) = crate::profile_snapshot::normalize_origin(initial_url) else {
            return Vec::new();
        };
        if self.local_storage.is_empty() && self.session_storage.is_empty() {
            return Vec::new();
        }
        vec![OriginState {
            origin,
            local_storage: self.local_storage.clone(),
            session_storage: self.session_storage.clone(),
            ..Default::default()
        }]
    }

    /// The snapshot with every secret taken out and its shape kept: cookie values empty,
    /// storage values empty, IndexedDB records removed (their count stays in the store's
    /// `omitted` flag only when the store had any). Nothing here is a value.
    pub fn redacted_view(&self) -> SessionSnapshot {
        let blank = |entries: &[(String, String)]| -> Vec<(String, String)> {
            entries.iter().map(|(k, _)| (k.clone(), String::new())).collect()
        };
        SessionSnapshot {
            cookies: self
                .cookies
                .iter()
                .map(|cookie| Cookie {
                    value: String::new(),
                    ..cookie.clone()
                })
                .collect(),
            local_storage: blank(&self.local_storage),
            session_storage: blank(&self.session_storage),
            indexed_db: self
                .indexed_db
                .iter()
                .map(|entry| IndexedDbEntry {
                    value: serde_json::Value::Null,
                    ..entry.clone()
                })
                .collect(),
            origins: self
                .origins
                .iter()
                .map(|origin| OriginState {
                    local_storage: blank(&origin.local_storage),
                    session_storage: blank(&origin.session_storage),
                    indexed_db: origin
                        .indexed_db
                        .iter()
                        .map(|db| IdbDatabase {
                            stores: db
                                .stores
                                .iter()
                                .map(|store| IdbStore {
                                    omitted: store.omitted || !store.records.is_empty(),
                                    records: Vec::new(),
                                    ..store.clone()
                                })
                                .collect(),
                            ..db.clone()
                        })
                        .collect(),
                    ..origin.clone()
                })
                .collect(),
            ..self.clone()
        }
    }

    /// Size of the snapshot as stored in plaintext (its JSON), the unit of
    /// `QUARRY_BROWSER_PROFILE_MAX_BYTES`.
    pub fn plaintext_len(&self) -> usize {
        serde_json::to_vec(self).map_or(0, |bytes| bytes.len())
    }

    /// What a clear of the browsing data would remove. Web storage is counted from the per-origin
    /// state when the snapshot has any, because a capture v2 snapshot repeats the active origin's
    /// storage in the flat fields. IndexedDB is counted from both: the flat entries are a legacy
    /// shape that capture v2 never repeats in `origins`.
    pub fn browsing_data_counts(&self) -> BrowsingDataCounts {
        let per_origin = !self.origins.is_empty();
        BrowsingDataCounts {
            cookies: self.cookies.len(),
            local_storage_entries: if per_origin {
                self.origins
                    .iter()
                    .map(|state| state.local_storage.len())
                    .sum()
            } else {
                self.local_storage.len()
            },
            session_storage_entries: if per_origin {
                self.origins
                    .iter()
                    .map(|state| state.session_storage.len())
                    .sum()
            } else {
                self.session_storage.len()
            },
            indexed_db_rows: self.indexed_db.len()
                + self
                    .origins
                    .iter()
                    .map(OriginState::idb_record_count)
                    .sum::<usize>(),
            origins: self
                .origins
                .iter()
                .filter(|state| !state.is_empty())
                .count(),
        }
    }

    /// The snapshot with its browsing data removed: cookies, web storage and IndexedDB, flat and
    /// per origin. The device settings (user agent, viewport, locale, timezone) and the capture
    /// facts (version, capture time) stay, so the profile still restores the same device.
    pub fn without_browsing_data(&self) -> SessionSnapshot {
        SessionSnapshot {
            cookies: Vec::new(),
            local_storage: Vec::new(),
            session_storage: Vec::new(),
            indexed_db: Vec::new(),
            origins: Vec::new(),
            ..self.clone()
        }
    }
}

/// What a clear of one snapshot's browsing data removes, by kind. Counts only: no name,
/// key or value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowsingDataCounts {
    pub cookies: usize,
    pub local_storage_entries: usize,
    pub session_storage_entries: usize,
    pub indexed_db_rows: usize,
    pub origins: usize,
}

impl BrowsingDataCounts {
    /// Whether a clear would remove nothing.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One row inside an IndexedDB object store. `origin`/`database`/`store`
/// identify the bucket; `key` is the IDB key (string-form). `value` is
/// the JSON-serialized record body. Drivers that don't support IDB
/// capture (Browserless static, Kernel without explicit IDB hook) emit
/// no entries — the empty vec round-trips cleanly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDbEntry {
    pub origin: String,
    pub database: String,
    pub store: String,
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub device_scale_factor: f64,
    #[serde(default)]
    pub is_mobile: bool,
}

/// Profile scope — mirrors the SPA's `BrowserProfileScope` union
/// (`ephemeral | user_private | org_shared | run_scoped`). Phase 3
/// continuation: promotes scope from an inferred gateway-side boolean
/// into first-class, queryable profile metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileScope {
    /// No persistence intended. A *session* can be ephemeral without any
    /// profile at all. This is the in-language default for "no scope was
    /// specified" (e.g. `ProfileMetadata::default()` when a store has no
    /// metadata row at all) — but a persisted `ProfileMetadata` row must
    /// never actually carry this value: `quarry-edge`'s create/update
    /// handlers reject `scope: ephemeral` outright, and the Postgres
    /// migration backfills/defaults un-metadata'd rows to `UserPrivate`,
    /// not this variant, precisely because a row that exists in storage
    /// was, by construction, deliberately persisted.
    #[default]
    Ephemeral,
    /// Persists for one signed-in user only.
    UserPrivate,
    /// Persists and is usable by any member of the org.
    OrgShared,
    /// Persists only for the lifetime of a single agent run/grant.
    RunScoped,
}

impl ProfileScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::UserPrivate => "user_private",
            Self::OrgShared => "org_shared",
            Self::RunScoped => "run_scoped",
        }
    }
}

impl std::fmt::Display for ProfileScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ProfileScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ephemeral" => Ok(Self::Ephemeral),
            "user_private" => Ok(Self::UserPrivate),
            "org_shared" => Ok(Self::OrgShared),
            "run_scoped" => Ok(Self::RunScoped),
            other => Err(format!("unknown profile scope: {other}")),
        }
    }
}

/// First-class profile metadata, additive alongside `SessionSnapshot`.
/// `name` is a caller-chosen display label; `scope` mirrors the SPA's
/// `BrowserProfileScope`. Stored/loaded independently of the snapshot so
/// naming/rescoping a profile never touches its captured cookies/storage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub scope: ProfileScope,
    /// The owner actor id (`Claims::actor_id()`), stamped on INSERT only (WS17.P2).
    /// `None` on a legacy row. Read by the edge owner check; not a display field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

impl ProfileMetadata {
    /// The metadata of a row that exists in storage but carries none of its own: a profile
    /// saved without a `save_metadata` call (D16 Q2). It is `user_private` with no name and
    /// no owner, in every store. It is NOT `ProfileMetadata::default()`, whose scope is the
    /// "no scope was specified" value `Ephemeral`: a row that is stored was deliberately
    /// persisted, and the Postgres migration 0005 defaults such a row to `user_private`.
    pub fn stored_default() -> Self {
        Self {
            name: None,
            scope: ProfileScope::UserPrivate,
            owner: None,
        }
    }
}

/// One row of `ProfileStore::list` — enough to render a profile picker
/// (name + scope) without a follow-up load per entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub profile_id: ProfileKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub scope: ProfileScope,
    /// The owner actor id, read from the same row as `scope` (WS17.P2). `None` on a legacy row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

/// Ordering metadata for one `ProfileStore::save`. `saved_at` is the START
/// time of the capture (not the commit time): an older capture never replaces
/// a newer one, and a capture that started before a delete cannot resurrect
/// the profile. `owner` is stamped on INSERT only (WS17.P1; the owner check
/// itself is WS17.P2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveMeta {
    pub saved_at: chrono::DateTime<chrono::Utc>,
    pub owner: Option<String>,
}

impl SaveMeta {
    /// Ordering metadata for a save whose capture starts now.
    pub fn now(owner: Option<String>) -> Self {
        Self {
            saved_at: chrono::Utc::now(),
            owner,
        }
    }
}

/// What a `ProfileStore::stamp_owner` did (WS17.P2, D16 Q2). Not an error in any case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StampOutcome {
    /// There was no row; one was created with the given name, scope and owner.
    Stamped,
    /// A row already exists (any owner, or none): nothing was changed.
    AlreadyExists,
    /// A delete newer than the stamp's `saved_at` is on record: nothing was written.
    IgnoredByTombstone,
}

impl StampOutcome {
    /// A short class for logs and counters. Never carries a profile id.
    pub fn reason_class(self) -> &'static str {
        match self {
            Self::Stamped => "stamped",
            Self::AlreadyExists => "already_exists",
            Self::IgnoredByTombstone => "tombstone",
        }
    }
}

/// Largest amount a `saved_at` may run ahead of the store's clock. Pods can
/// disagree about the time; a far-future stamp would otherwise lock a profile
/// against every later save.
pub const SAVED_AT_MAX_FUTURE_SKEW_S: i64 = 5;

/// `saved_at` clamped to `now + SAVED_AT_MAX_FUTURE_SKEW_S`.
pub fn clamp_saved_at(
    saved_at: chrono::DateTime<chrono::Utc>,
    now: chrono::DateTime<chrono::Utc>,
) -> chrono::DateTime<chrono::Utc> {
    saved_at.min(now + chrono::Duration::seconds(SAVED_AT_MAX_FUTURE_SKEW_S))
}

/// What a `ProfileStore::save` did. An ignored save is not an error: the
/// store kept a newer capture (`IgnoredStale`) or a newer delete
/// (`IgnoredByTombstone`). Callers log `reason_class()` and carry on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Stored { rev: i64 },
    IgnoredStale,
    IgnoredByTombstone,
}

impl SaveOutcome {
    pub fn is_stored(self) -> bool {
        matches!(self, Self::Stored { .. })
    }

    /// A short class for logs. Never carries a profile id or any payload.
    pub fn reason_class(self) -> &'static str {
        match self {
            Self::Stored { .. } => "stored",
            Self::IgnoredStale => "stale",
            Self::IgnoredByTombstone => "tombstone",
        }
    }
}

/// Tenant-scoped profile storage. Every method takes the verified
/// `org_id` so the store can never return another tenant's session state
/// (cookies, storage, viewport). The trait is intentionally a leaky
/// abstraction here — the org_id parameter forces every backend to
/// enforce isolation, rather than relying on callers to remember.
#[async_trait]
pub trait ProfileStore: Send + Sync {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome>;
    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>>;
    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()>;
    /// Enumerate this org's profiles with name/scope metadata attached.
    /// Phase 3 continuation: previously returned bare `Vec<ProfileKind>`.
    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>>;

    /// Persist/overwrite a profile's name/scope. Additive: never touches
    /// the `SessionSnapshot` written by `save`. Implementations must not
    /// require a prior `save` call — metadata can be written for a
    /// freshly "created" named profile that has no browsing history yet.
    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()>;
    /// Load a profile's metadata. `Ok(None)` means no metadata (and, per
    /// `list`'s contract, no snapshot either) exists at all — callers
    /// that already know the profile exists should treat `None` here as
    /// `ProfileMetadata::stored_default()` (scope `UserPrivate`, no name, no owner).
    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>>;

    /// Create the row of a profile a run is starting on, with its owner, only if no row
    /// exists (WS17.P2, D16 Q2). Insert-only: an existing row is never touched, so a
    /// rename, a rescope or another user's owner cannot be changed by a run start. A
    /// store that keeps deletion tombstones must refuse the stamp when a delete newer than
    /// `saved_at` (the start time of the request) is on record, as `save` does, so a profile
    /// deleted while a run was starting does not come back. The default here has no tombstone
    /// knowledge and is only correct for a store without them; the stores that keep
    /// tombstones override it.
    async fn stamp_owner(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
        saved_at: chrono::DateTime<chrono::Utc>,
    ) -> QuarryResult<StampOutcome> {
        let _ = saved_at;
        if self.exists(org_id, profile_id).await? {
            return Ok(StampOutcome::AlreadyExists);
        }
        self.save_metadata(org_id, profile_id, metadata).await?;
        Ok(StampOutcome::Stamped)
    }

    /// Whether a snapshot row exists for this profile. Stores that can answer
    /// without decoding the snapshot override this.
    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        Ok(self.load(org_id, profile_id).await?.is_some())
    }
}

pub struct InMemoryProfileStore {
    /// Composite key `{org_id}\0{profile_id}` keeps tenants strictly
    /// partitioned in a single hashmap. The NUL separator prevents any
    /// accidental key collision between an org named "alpha" with profile
    /// "foo" and an org named "alphafoo" with no profile suffix.
    store: tokio::sync::RwLock<std::collections::HashMap<String, SessionSnapshot>>,
    /// Same composite-key scheme, kept as an independent map so naming
    /// or rescoping a profile never touches its captured snapshot.
    metadata: tokio::sync::RwLock<std::collections::HashMap<String, ProfileMetadata>>,
    /// Ordering state (WS17.P1), the in-memory mirror of `saved_at`, `rev`,
    /// `owner_user_id` and `quarry_profile_tombstones`.
    order: tokio::sync::RwLock<OrderBook>,
}

/// Ordering state for one `(org, profile)` key.
#[derive(Debug, Clone, Default)]
struct RowOrder {
    saved_at: Option<chrono::DateTime<chrono::Utc>>,
    rev: i64,
    owner: Option<String>,
}

#[derive(Debug, Default)]
struct OrderBook {
    rows: std::collections::HashMap<String, RowOrder>,
    /// `deleted_at` per key. Tombstones never expire in memory (dev and test
    /// only). The Postgres store sweeps them after 7 days.
    tombstones: std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>,
}

impl InMemoryProfileStore {
    pub fn new() -> Self {
        Self {
            store: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            metadata: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            order: tokio::sync::RwLock::new(OrderBook::default()),
        }
    }

    fn key(org_id: &str, profile_id: &ProfileKind) -> String {
        format!("{org_id}\0{profile_id}")
    }
}

impl Default for InMemoryProfileStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ProfileStore for InMemoryProfileStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        let key = Self::key(org_id, profile_id);
        let saved_at = clamp_saved_at(meta.saved_at, chrono::Utc::now());
        // Lock order: order book, then store (`delete` takes them in this order).
        let mut book = self.order.write().await;
        if book
            .tombstones
            .get(&key)
            .is_some_and(|deleted_at| *deleted_at >= saved_at)
        {
            return Ok(SaveOutcome::IgnoredByTombstone);
        }
        if book
            .rows
            .get(&key)
            .and_then(|row| row.saved_at)
            .is_some_and(|prev| prev >= saved_at)
        {
            return Ok(SaveOutcome::IgnoredStale);
        }
        let existed = {
            let mut store = self.store.write().await;
            let existed = store.contains_key(&key);
            store.insert(key.clone(), snapshot.clone());
            existed
        };
        let row = book.rows.entry(key).or_default();
        if !existed {
            // Owner is stamped on INSERT only, like the SQL upsert.
            row.owner = meta.owner.clone();
        }
        row.saved_at = Some(saved_at);
        row.rev += 1;
        Ok(SaveOutcome::Stored { rev: row.rev })
    }

    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        Ok(self
            .store
            .read()
            .await
            .get(&Self::key(org_id, profile_id))
            .cloned())
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        let key = Self::key(org_id, profile_id);
        let now = chrono::Utc::now();
        let mut book = self.order.write().await;
        let tomb = book.tombstones.entry(key.clone()).or_insert(now);
        if *tomb < now {
            *tomb = now;
        }
        book.rows.remove(&key);
        self.store.write().await.remove(&key);
        self.metadata.write().await.remove(&key);
        Ok(())
    }

    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        Ok(self
            .store
            .read()
            .await
            .contains_key(&Self::key(org_id, profile_id)))
    }

    async fn stamp_owner(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
        saved_at: chrono::DateTime<chrono::Utc>,
    ) -> QuarryResult<StampOutcome> {
        let key = Self::key(org_id, profile_id);
        let saved_at = clamp_saved_at(saved_at, chrono::Utc::now());
        // Lock order: order book, store, metadata (as in `save` and `save_metadata`).
        let mut book = self.order.write().await;
        if book
            .tombstones
            .get(&key)
            .is_some_and(|deleted_at| *deleted_at >= saved_at)
        {
            return Ok(StampOutcome::IgnoredByTombstone);
        }
        let mut stored = self.store.write().await;
        if stored.contains_key(&key) {
            return Ok(StampOutcome::AlreadyExists);
        }
        stored.insert(key.clone(), SessionSnapshot::default());
        book.rows.entry(key.clone()).or_default().owner = metadata.owner.clone();
        self.metadata.write().await.insert(
            key,
            ProfileMetadata {
                owner: None,
                ..metadata.clone()
            },
        );
        Ok(StampOutcome::Stamped)
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        let prefix = format!("{org_id}\0");
        // Lock order: order book, store, metadata (as in `save`, `delete`, `save_metadata`).
        let book = self.order.read().await;
        let stored = self.store.read().await;
        let metadata = self.metadata.read().await;
        Ok(stored
            .keys()
            .filter_map(|k| {
                let suffix = k.strip_prefix(&prefix)?;
                let profile_id: ProfileKind = suffix.parse().ok()?;
                let found = metadata
                    .get(k)
                    .cloned()
                    .unwrap_or_else(ProfileMetadata::stored_default);
                Some(ProfileSummary {
                    profile_id,
                    name: found.name,
                    scope: found.scope,
                    owner: book.rows.get(k).and_then(|row| row.owner.clone()),
                })
            })
            .collect())
    }

    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()> {
        let key = Self::key(org_id, profile_id);
        // Mirror Postgres/S3: a profile "created" via metadata alone (no
        // browsing history yet) must still surface from `list()`, whose
        // InMemory implementation enumerates `store`'s keys. Seed a
        // placeholder snapshot only if one doesn't already exist so a
        // rename/rescope of a profile with real captured cookies never
        // clobbers them.
        // Lock order: order book, store, metadata. The owner is stamped on INSERT only,
        // like the SQL upsert: a rename of an existing row never changes it (WS17.P2).
        let mut book = self.order.write().await;
        let mut stored = self.store.write().await;
        if !stored.contains_key(&key) {
            book.rows.entry(key.clone()).or_default().owner = metadata.owner.clone();
        }
        stored
            .entry(key.clone())
            .or_insert_with(SessionSnapshot::default);
        self.metadata.write().await.insert(
            key,
            ProfileMetadata {
                owner: None,
                ..metadata.clone()
            },
        );
        Ok(())
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        let key = Self::key(org_id, profile_id);
        let book = self.order.read().await;
        let metadata = self.metadata.read().await;
        if !metadata.contains_key(&key) && !book.rows.contains_key(&key) {
            return Ok(None);
        }
        let mut out = metadata
            .get(&key)
            .cloned()
            .unwrap_or_else(ProfileMetadata::stored_default);
        out.owner = book.rows.get(&key).and_then(|row| row.owner.clone());
        Ok(Some(out))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    /// As Chromium reports it: a leading dot means a domain cookie, none means host-only.
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// `None` for a session cookie (and on every legacy snapshot).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<chrono::DateTime<chrono::Utc>>,
    // WS17.C1 additive attributes: absent on a legacy snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub same_site: Option<SameSite>,
    /// Chromium's own session flag (a cookie with no expiry).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub session: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<CookiePriority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_key: Option<PartitionKey>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_scheme: Option<SourceScheme>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarry_core::ids::Id;

    #[tokio::test]
    async fn in_memory_store_save_load_roundtrip() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();

        let snapshot = SessionSnapshot {
            cookies: vec![Cookie {
                name: "session".into(),
                value: "abc123".into(),
                domain: "example.com".into(),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires: None,
                ..Default::default()
            }],
            local_storage: vec![("theme".into(), "dark".into())],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: Some("Quarry/1.0".into()),
            viewport: Some(Viewport {
                width: 1920,
                height: 1080,
                device_scale_factor: 1.0,
                is_mobile: false,
            }),
            locale: Some("en-US".into()),
            timezone: Some("America/New_York".into()),
            ..Default::default()
        };

        store
            .save("org_a", &profile_id, &snapshot, &SaveMeta::now(None))
            .await
            .unwrap();
        let loaded = store.load("org_a", &profile_id).await.unwrap().unwrap();

        assert_eq!(loaded.cookies.len(), 1);
        assert_eq!(loaded.cookies[0].name, "session");
        assert_eq!(loaded.local_storage.len(), 1);
        assert_eq!(loaded.user_agent.as_deref(), Some("Quarry/1.0"));
        assert_eq!(loaded.viewport.as_ref().unwrap().width, 1920);
        assert_eq!(loaded.locale.as_deref(), Some("en-US"));
        assert_eq!(loaded.timezone.as_deref(), Some("America/New_York"));
    }

    #[tokio::test]
    async fn in_memory_store_load_missing_returns_none() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();
        assert!(store.load("org_a", &profile_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_store_isolates_orgs() {
        // org_a saves a profile; org_b must NOT see it via load or list,
        // even when the profile_id ULID is identical (which it cannot be
        // in practice, but the test pins the contract).
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let snapshot = SessionSnapshot {
            cookies: vec![],
            local_storage: vec![],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: None,
            viewport: None,
            locale: None,
            timezone: None,
            ..Default::default()
        };
        store
            .save("org_a", &pid, &snapshot, &SaveMeta::now(None))
            .await
            .unwrap();

        // Cross-tenant load: must miss.
        assert!(store.load("org_b", &pid).await.unwrap().is_none());

        // Cross-tenant list: must be empty for org_b.
        let other = store.list("org_b").await.unwrap();
        assert!(other.is_empty(), "org_b leaked org_a's profile");

        // Own-tenant list: must contain it.
        let mine = store.list("org_a").await.unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].profile_id.to_string(), pid.to_string());

        // Cross-tenant delete: must NOT remove org_a's profile.
        store.delete("org_b", &pid).await.unwrap();
        assert!(store.load("org_a", &pid).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn in_memory_store_delete_removes_entry() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();
        let snapshot = SessionSnapshot::default();

        store
            .save("org_a", &profile_id, &snapshot, &SaveMeta::now(None))
            .await
            .unwrap();
        assert!(store.load("org_a", &profile_id).await.unwrap().is_some());

        store.delete("org_a", &profile_id).await.unwrap();
        assert!(store.load("org_a", &profile_id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn in_memory_store_list_returns_all_profiles() {
        let store = InMemoryProfileStore::new();
        let p1: ProfileKind = Id::new();
        let p2: ProfileKind = Id::new();

        store
            .save(
                "org_a",
                &p1,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        store
            .save(
                "org_a",
                &p2,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();

        let list = store.list("org_a").await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn metadata_save_load_roundtrip() {
        let store = InMemoryProfileStore::new();
        let profile_id: ProfileKind = Id::new();
        store
            .save(
                "org_a",
                &profile_id,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();

        // The save wrote a row, so the profile has metadata: no name, and the default scope.
        // The trait answers None only when there is neither a row nor a snapshot.
        let unnamed = store
            .load_metadata("org_a", &profile_id)
            .await
            .unwrap()
            .expect("a saved profile has a metadata row");
        assert_eq!(unnamed.name, None);

        let metadata = ProfileMetadata {
            name: Some("Work Gmail".into()),
            scope: ProfileScope::UserPrivate,
            owner: None,
        };
        store
            .save_metadata("org_a", &profile_id, &metadata)
            .await
            .unwrap();

        let loaded = store
            .load_metadata("org_a", &profile_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Work Gmail"));
        assert_eq!(loaded.scope, ProfileScope::UserPrivate);
    }

    #[tokio::test]
    async fn list_carries_name_and_scope_from_metadata() {
        let store = InMemoryProfileStore::new();
        let named: ProfileKind = Id::new();
        let unnamed: ProfileKind = Id::new();
        store
            .save(
                "org_a",
                &named,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        store
            .save(
                "org_a",
                &unnamed,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        store
            .save_metadata(
                "org_a",
                &named,
                &ProfileMetadata {
                    name: Some("Norwegian bank".into()),
                    scope: ProfileScope::OrgShared,
                    owner: None,
                },
            )
            .await
            .unwrap();

        let list = store.list("org_a").await.unwrap();
        let named_summary = list
            .iter()
            .find(|s| s.profile_id == named)
            .expect("named profile missing from list");
        assert_eq!(named_summary.name.as_deref(), Some("Norwegian bank"));
        assert_eq!(named_summary.scope, ProfileScope::OrgShared);

        // No metadata ever saved for `unnamed` — must default, not error.
        let unnamed_summary = list
            .iter()
            .find(|s| s.profile_id == unnamed)
            .expect("unnamed profile missing from list");
        assert!(unnamed_summary.name.is_none());
        // D16 Q2: a stored row with no metadata is user_private, as in Postgres (migration
        // 0005). The old expectation, `Ephemeral`, was the in-language "no scope" default
        // and contradicted the accepted decision.
        assert_eq!(unnamed_summary.scope, ProfileScope::UserPrivate);
    }

    #[tokio::test]
    async fn metadata_isolates_orgs() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        store
            .save_metadata(
                "org_a",
                &pid,
                &ProfileMetadata {
                    name: Some("secret".into()),
                    scope: ProfileScope::UserPrivate,
                    owner: None,
                },
            )
            .await
            .unwrap();

        assert!(store.load_metadata("org_b", &pid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_removes_metadata_too() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        store
            .save_metadata(
                "org_a",
                &pid,
                &ProfileMetadata {
                    name: Some("temp".into()),
                    scope: ProfileScope::RunScoped,
                    owner: None,
                },
            )
            .await
            .unwrap();

        store.delete("org_a", &pid).await.unwrap();
        assert!(store.load("org_a", &pid).await.unwrap().is_none());
        assert!(store.load_metadata("org_a", &pid).await.unwrap().is_none());
    }

    #[test]
    fn profile_scope_string_roundtrip() {
        for scope in [
            ProfileScope::Ephemeral,
            ProfileScope::UserPrivate,
            ProfileScope::OrgShared,
            ProfileScope::RunScoped,
        ] {
            let s = scope.to_string();
            let parsed: ProfileScope = s.parse().unwrap();
            assert_eq!(parsed, scope);
        }
    }

    #[test]
    fn profile_scope_default_is_ephemeral() {
        // The in-language "no scope was specified" value. A STORED row never reads back as
        // this: see `stored_default`.
        assert_eq!(ProfileScope::default(), ProfileScope::Ephemeral);
        assert_eq!(
            ProfileMetadata::stored_default(),
            ProfileMetadata {
                name: None,
                scope: ProfileScope::UserPrivate,
                owner: None,
            }
        );
    }

    #[tokio::test]
    async fn a_profile_saved_without_metadata_reads_back_user_private() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta::now(Some("user_a".into())),
            )
            .await
            .unwrap();
        let loaded = store.load_metadata("org_a", &pid).await.unwrap().unwrap();
        assert_eq!(loaded.scope, ProfileScope::UserPrivate);
        assert_eq!(loaded.owner.as_deref(), Some("user_a"));
        assert!(loaded.name.is_none());
        let listed = store.list("org_a").await.unwrap();
        assert_eq!(listed[0].scope, ProfileScope::UserPrivate);
    }

    fn stamp_metadata(owner: &str, scope: ProfileScope) -> ProfileMetadata {
        ProfileMetadata {
            name: None,
            scope,
            owner: Some(owner.into()),
        }
    }

    #[tokio::test]
    async fn stamp_owner_creates_a_missing_row_with_the_owner_and_scope() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let outcome = store
            .stamp_owner(
                "org_a",
                &pid,
                &stamp_metadata("user_a", ProfileScope::OrgShared),
                chrono::Utc::now(),
            )
            .await
            .unwrap();
        assert_eq!(outcome, StampOutcome::Stamped);
        let loaded = store.load_metadata("org_a", &pid).await.unwrap().unwrap();
        assert_eq!(loaded.owner.as_deref(), Some("user_a"));
        assert_eq!(loaded.scope, ProfileScope::OrgShared);
        assert!(store.exists("org_a", &pid).await.unwrap());
        // Other tenants see nothing.
        assert!(store.load_metadata("org_b", &pid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn stamp_owner_never_touches_an_existing_row() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        store
            .save_metadata(
                "org_a",
                &pid,
                &ProfileMetadata {
                    name: Some("Work".into()),
                    scope: ProfileScope::OrgShared,
                    owner: Some("user_a".into()),
                },
            )
            .await
            .unwrap();
        let outcome = store
            .stamp_owner(
                "org_a",
                &pid,
                &stamp_metadata("user_b", ProfileScope::UserPrivate),
                chrono::Utc::now(),
            )
            .await
            .unwrap();
        assert_eq!(outcome, StampOutcome::AlreadyExists);
        let loaded = store.load_metadata("org_a", &pid).await.unwrap().unwrap();
        assert_eq!(loaded.owner.as_deref(), Some("user_a"));
        assert_eq!(loaded.scope, ProfileScope::OrgShared);
        assert_eq!(loaded.name.as_deref(), Some("Work"));
        // An ownerless legacy row is not claimed by a stamp either.
        let legacy: ProfileKind = Id::new();
        store
            .save(
                "org_a",
                &legacy,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        let outcome = store
            .stamp_owner(
                "org_a",
                &legacy,
                &stamp_metadata("user_b", ProfileScope::UserPrivate),
                chrono::Utc::now(),
            )
            .await
            .unwrap();
        assert_eq!(outcome, StampOutcome::AlreadyExists);
        let loaded = store.load_metadata("org_a", &legacy).await.unwrap().unwrap();
        assert!(loaded.owner.is_none());
    }

    #[tokio::test]
    async fn stamp_owner_honours_a_tombstone_newer_than_the_request() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let request_started = chrono::Utc::now() - chrono::Duration::seconds(2);
        // The profile is deleted after the request began, before its stamp ran.
        store.delete("org_a", &pid).await.unwrap();
        let outcome = store
            .stamp_owner(
                "org_a",
                &pid,
                &stamp_metadata("user_a", ProfileScope::UserPrivate),
                request_started,
            )
            .await
            .unwrap();
        assert_eq!(outcome, StampOutcome::IgnoredByTombstone);
        assert!(!store.exists("org_a", &pid).await.unwrap());
        assert!(store.load_metadata("org_a", &pid).await.unwrap().is_none());

        // A request that began after the delete is a new profile under the same id.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let outcome = store
            .stamp_owner(
                "org_a",
                &pid,
                &stamp_metadata("user_a", ProfileScope::UserPrivate),
                chrono::Utc::now(),
            )
            .await
            .unwrap();
        assert_eq!(outcome, StampOutcome::Stamped);
    }

    #[test]
    fn snapshot_serde_roundtrip() {
        let snapshot = SessionSnapshot {
            cookies: vec![Cookie {
                name: "sid".into(),
                value: "xyz".into(),
                domain: ".example.com".into(),
                path: "/app".into(),
                secure: true,
                http_only: false,
                expires: None,
                ..Default::default()
            }],
            local_storage: vec![("k".into(), "v".into())],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: None,
            viewport: Some(Viewport {
                width: 375,
                height: 812,
                device_scale_factor: 3.0,
                is_mobile: true,
            }),
            locale: None,
            timezone: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: SessionSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.cookies[0].name, "sid");
        assert!(restored.viewport.unwrap().is_mobile);
    }

    fn meta_at(saved_at: chrono::DateTime<chrono::Utc>) -> SaveMeta {
        SaveMeta {
            saved_at,
            owner: None,
        }
    }

    #[tokio::test]
    async fn save_with_older_saved_at_is_ignored() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let now = chrono::Utc::now();
        let newer = SessionSnapshot {
            user_agent: Some("newer".into()),
            ..SessionSnapshot::default()
        };
        let older = SessionSnapshot {
            user_agent: Some("older".into()),
            ..SessionSnapshot::default()
        };
        let outcome = store
            .save("org_a", &pid, &newer, &meta_at(now))
            .await
            .unwrap();
        assert!(outcome.is_stored());
        let stale = store
            .save(
                "org_a",
                &pid,
                &older,
                &meta_at(now - chrono::Duration::seconds(10)),
            )
            .await
            .unwrap();
        assert_eq!(stale, SaveOutcome::IgnoredStale);
        let loaded = store.load("org_a", &pid).await.unwrap().unwrap();
        assert_eq!(loaded.user_agent.as_deref(), Some("newer"));
    }

    #[tokio::test]
    async fn save_after_delete_with_older_saved_at_is_ignored_by_tombstone() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let capture_started = chrono::Utc::now();
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &meta_at(capture_started),
            )
            .await
            .unwrap();
        store.delete("org_a", &pid).await.unwrap();
        let outcome = store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &meta_at(capture_started),
            )
            .await
            .unwrap();
        assert_eq!(outcome, SaveOutcome::IgnoredByTombstone);
        assert!(store.load("org_a", &pid).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn save_after_delete_with_newer_saved_at_creates_a_new_row() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        store.delete("org_a", &pid).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let outcome = store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        assert!(outcome.is_stored());
        assert!(store.load("org_a", &pid).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn saved_at_is_clamped_to_now_plus_5s() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let far_future = chrono::Utc::now() + chrono::Duration::hours(1);
        let outcome = store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &meta_at(far_future),
            )
            .await
            .unwrap();
        assert!(outcome.is_stored());
        // The stored stamp is now + 5 s, so a save stamped now + 1 s is older.
        let just_ahead = chrono::Utc::now() + chrono::Duration::seconds(1);
        let stale = store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &meta_at(just_ahead),
            )
            .await
            .unwrap();
        assert_eq!(stale, SaveOutcome::IgnoredStale);
    }

    #[tokio::test]
    async fn rev_increments_only_on_stored() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let now = chrono::Utc::now();
        let first = store
            .save("org_a", &pid, &SessionSnapshot::default(), &meta_at(now))
            .await
            .unwrap();
        assert_eq!(first, SaveOutcome::Stored { rev: 1 });
        let stale = store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &meta_at(now - chrono::Duration::seconds(10)),
            )
            .await
            .unwrap();
        assert_eq!(stale, SaveOutcome::IgnoredStale);
        let second = store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &meta_at(now + chrono::Duration::seconds(1)),
            )
            .await
            .unwrap();
        assert_eq!(second, SaveOutcome::Stored { rev: 2 });
    }

    #[tokio::test]
    async fn owner_is_stamped_on_insert_only() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        let now = chrono::Utc::now();
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta {
                    saved_at: now,
                    owner: Some("user_1".into()),
                },
            )
            .await
            .unwrap();
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta {
                    saved_at: now + chrono::Duration::seconds(1),
                    owner: Some("user_2".into()),
                },
            )
            .await
            .unwrap();
        let book = store.order.read().await;
        let row = book
            .rows
            .get(&InMemoryProfileStore::key("org_a", &pid))
            .unwrap();
        assert_eq!(row.owner.as_deref(), Some("user_1"));
        assert_eq!(row.rev, 2);
    }

    #[tokio::test]
    async fn exists_matches_load() {
        let store = InMemoryProfileStore::new();
        let pid: ProfileKind = Id::new();
        assert!(!store.exists("org_a", &pid).await.unwrap());
        store
            .save(
                "org_a",
                &pid,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        assert!(store.exists("org_a", &pid).await.unwrap());
        assert!(!store.exists("org_b", &pid).await.unwrap());
        store.delete("org_a", &pid).await.unwrap();
        assert!(!store.exists("org_a", &pid).await.unwrap());
    }

    #[test]
    fn legacy_flat_snapshot_deserializes_and_restores_into_initial_origin() {
        // A snapshot written before capture v2: no version, no origins, no cookie attributes.
        let legacy = r#"{
            "cookies": [{"name":"sid","value":"v","domain":".a.test","path":"/","secure":true,"http_only":true}],
            "local_storage": [["k","v"]],
            "session_storage": [["s","1"]]
        }"#;
        let snapshot: SessionSnapshot = serde_json::from_str(legacy).unwrap();
        assert_eq!(snapshot.snapshot_version, 0);
        assert!(snapshot.origins.is_empty());
        assert!(snapshot.cookies[0].same_site.is_none() && !snapshot.cookies[0].session);
        let origins = snapshot.effective_origins("https://a.test/start?x=1");
        assert_eq!(origins.len(), 1);
        assert_eq!(origins[0].origin, "https://a.test");
        assert_eq!(origins[0].local_storage, vec![("k".to_owned(), "v".to_owned())]);
        assert_eq!(origins[0].session_storage, vec![("s".to_owned(), "1".to_owned())]);
        // No http(s) initial URL: nothing to restore the flat entries into.
        assert!(snapshot.effective_origins("about:blank").is_empty());
        // A legacy snapshot with no storage has no origin entry either.
        assert!(SessionSnapshot::default().effective_origins("https://a.test").is_empty());
    }

    #[test]
    fn schema_v2_roundtrip_through_serde() {
        let snapshot = SessionSnapshot {
            snapshot_version: SNAPSHOT_VERSION,
            cookies: vec![Cookie {
                name: "sid".into(),
                value: "secret".into(),
                domain: ".a.test".into(),
                path: "/".into(),
                secure: true,
                http_only: true,
                expires: Some(chrono::Utc::now() + chrono::Duration::hours(24)),
                same_site: Some(SameSite::Lax),
                session: false,
                priority: Some(CookiePriority::Medium),
                partition_key: Some(PartitionKey {
                    top_level_site: "https://a.test".into(),
                    has_cross_site_ancestor: false,
                }),
                source_scheme: Some(SourceScheme::Secure),
            }],
            origins: vec![OriginState {
                origin: "https://a.test".into(),
                local_storage: vec![("k".into(), "v".into())],
                indexed_db: vec![IdbDatabase {
                    name: "db".into(),
                    version: 3,
                    stores: vec![IdbStore {
                        name: "items".into(),
                        key_path: serde_json::json!("id"),
                        auto_increment: true,
                        indexes: vec![IdbIndex {
                            name: "by_name".into(),
                            key_path: serde_json::json!("name"),
                            unique: true,
                            multi_entry: false,
                        }],
                        records: vec![IdbRecord {
                            key: serde_json::json!(1),
                            value: serde_json::json!({ "$t": "date", "v": 1_700_000_000_000.0 }),
                        }],
                        omitted: false,
                    }],
                }],
                dropped_values: 2,
                truncated: true,
                ..Default::default()
            }],
            ..Default::default()
        };
        let text = serde_json::to_string(&snapshot).unwrap();
        let back: SessionSnapshot = serde_json::from_str(&text).unwrap();
        assert_eq!(back.snapshot_version, 2);
        assert_eq!(back.origins, snapshot.origins);
        assert_eq!(back.cookies[0].same_site, Some(SameSite::Lax));
        assert_eq!(back.cookies[0].partition_key, snapshot.cookies[0].partition_key);
        assert_eq!(back.cookies[0].expires, snapshot.cookies[0].expires);
        // The additive fields are absent from a snapshot that has none of them.
        let plain = serde_json::to_string(&SessionSnapshot::default()).unwrap();
        assert!(!plain.contains("snapshot_version") && !plain.contains("origins"));
        let plain_cookie = serde_json::to_string(&Cookie::default()).unwrap();
        assert!(!plain_cookie.contains("same_site") && !plain_cookie.contains("session"));
    }

    #[test]
    fn redacted_view_keeps_shape_and_no_value() {
        let now = chrono::Utc::now();
        let soon = now + chrono::Duration::hours(1);
        let later = now + chrono::Duration::hours(9);
        let snapshot = SessionSnapshot {
            cookies: vec![
                Cookie {
                    name: "a".into(),
                    value: "SECRET-COOKIE".into(),
                    expires: Some(later),
                    ..Default::default()
                },
                Cookie {
                    name: "b".into(),
                    value: "SECRET-TWO".into(),
                    expires: Some(soon),
                    ..Default::default()
                },
                Cookie {
                    name: "gone".into(),
                    expires: Some(now - chrono::Duration::hours(1)),
                    ..Default::default()
                },
            ],
            local_storage: vec![("k".into(), "SECRET-LS".into())],
            origins: vec![OriginState {
                origin: "https://a.test".into(),
                local_storage: vec![("k".into(), "SECRET-ORIGIN".into())],
                indexed_db: vec![IdbDatabase {
                    name: "db".into(),
                    version: 1,
                    stores: vec![IdbStore {
                        name: "s".into(),
                        records: vec![IdbRecord {
                            key: serde_json::json!("SECRET-KEY"),
                            value: serde_json::json!("SECRET-VALUE"),
                        }],
                        ..Default::default()
                    }],
                }],
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(snapshot.earliest_cookie_expiry(now), Some(soon));
        let text = serde_json::to_string(&snapshot.redacted_view()).unwrap();
        assert!(!text.contains("SECRET"), "{text}");
        assert!(text.contains("\"db\"") && text.contains("\"https://a.test\""));
    }

    #[test]
    fn browsing_data_counts_a_legacy_snapshot_by_its_flat_fields() {
        let legacy = SessionSnapshot {
            cookies: vec![Cookie {
                name: "sid".into(),
                ..Default::default()
            }],
            local_storage: vec![("a".into(), "1".into()), ("b".into(), "2".into())],
            session_storage: vec![("c".into(), "3".into())],
            ..Default::default()
        };
        assert_eq!(
            legacy.browsing_data_counts(),
            BrowsingDataCounts {
                cookies: 1,
                local_storage_entries: 2,
                session_storage_entries: 1,
                indexed_db_rows: 0,
                origins: 0,
            }
        );
    }

    #[test]
    fn browsing_data_counts_a_v2_snapshot_without_counting_the_flat_copy_twice() {
        let v2 = SessionSnapshot {
            local_storage: vec![("a".into(), "1".into())],
            origins: vec![
                OriginState {
                    origin: "https://a.test".into(),
                    local_storage: vec![("a".into(), "1".into())],
                    ..Default::default()
                },
                OriginState {
                    origin: "https://b.test".into(),
                    session_storage: vec![("x".into(), "y".into())],
                    ..Default::default()
                },
                OriginState::default(),
            ],
            ..Default::default()
        };
        let counts = v2.browsing_data_counts();
        assert_eq!(counts.local_storage_entries, 1);
        assert_eq!(counts.session_storage_entries, 1);
        assert_eq!(counts.origins, 2, "an origin with no state is not counted");
    }

    #[test]
    fn without_browsing_data_removes_site_data_and_keeps_device_settings() {
        let snapshot = SessionSnapshot {
            cookies: vec![Cookie {
                name: "sid".into(),
                value: "SECRET-COOKIE".into(),
                ..Default::default()
            }],
            local_storage: vec![("k".into(), "SECRET-LS".into())],
            session_storage: vec![("k".into(), "SECRET-SS".into())],
            indexed_db: vec![IndexedDbEntry {
                origin: "https://a.test".into(),
                database: "db".into(),
                store: "s".into(),
                key: "k".into(),
                value: serde_json::json!("SECRET-IDB"),
            }],
            origins: vec![OriginState {
                origin: "https://a.test".into(),
                local_storage: vec![("k".into(), "SECRET-ORIGIN".into())],
                indexed_db: vec![IdbDatabase {
                    name: "db".into(),
                    version: 1,
                    stores: vec![IdbStore {
                        name: "s".into(),
                        records: vec![IdbRecord {
                            key: serde_json::json!("SECRET-KEY"),
                            value: serde_json::json!("SECRET-VALUE"),
                        }],
                        ..Default::default()
                    }],
                }],
                ..Default::default()
            }],
            user_agent: Some("Quarry/2.0 (test)".into()),
            locale: Some("nb-NO".into()),
            timezone: Some("Europe/Oslo".into()),
            viewport: Some(Viewport {
                width: 800,
                height: 600,
                device_scale_factor: 1.0,
                is_mobile: false,
            }),
            snapshot_version: 2,
            ..Default::default()
        };
        let counts = snapshot.browsing_data_counts();
        assert_eq!(counts.cookies, 1);
        assert_eq!(counts.local_storage_entries, 1);
        assert_eq!(
            counts.session_storage_entries, 0,
            "the origin holds no session storage"
        );
        assert_eq!(
            counts.indexed_db_rows, 2,
            "one flat entry and one origin record"
        );
        assert_eq!(counts.origins, 1);

        let cleared = snapshot.without_browsing_data();
        assert!(!cleared.has_state());
        assert_eq!(
            cleared.browsing_data_counts(),
            BrowsingDataCounts::default()
        );
        assert!(cleared.browsing_data_counts().is_empty());
        assert_eq!(cleared.user_agent.as_deref(), Some("Quarry/2.0 (test)"));
        assert_eq!(cleared.locale.as_deref(), Some("nb-NO"));
        assert_eq!(cleared.timezone.as_deref(), Some("Europe/Oslo"));
        assert!(cleared.viewport.is_some());
        assert_eq!(cleared.snapshot_version, 2);
        let text = serde_json::to_string(&cleared).unwrap();
        assert!(!text.contains("SECRET"), "{text}");
    }

    #[test]
    fn an_empty_snapshot_has_nothing_to_clear() {
        let empty = SessionSnapshot::default();
        assert!(empty.browsing_data_counts().is_empty());
        assert!(!empty.without_browsing_data().has_state());
    }
}
