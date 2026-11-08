//! Batch re-seal of plaintext profile rows (WS17.E3 backfill).
//!
//! Reads the plaintext rows in batches, seals each one under the current key
//! with a fresh DEK, and writes the envelope through `ProfileBlobStore::reseal`,
//! which is a compare-and-set on `rev`. A row that a live save moved on is left
//! alone and is picked up by a later run or by the lazy re-seal on load.
//!
//! Properties: resumable (the cursor is `(org_id, profile_id)`), idempotent
//! (a sealed row leaves the plaintext set), rate-limited (a pause between
//! batches), safe while the fleet is live, and counts only. A key failure that
//! is not a single bad row (the key provider is unavailable) stops the run with
//! an error, so no row is left half-done by a silent skip. After the rows, the
//! legacy plaintext cache keys are unlinked, because they hold the same
//! snapshots in Redis. The envelope cache keys (`quarry:profile:v2:*`) are kept.
//!
//! W2-U41a: `check_release_gate` is the boot-time release gate for migration
//! 0017 (`RELEASE_GATE_ENV`, off by default).
//!
//! W2-Q-U41b: `org_has_oc_rows` is the probe the erasure's crypto-shred hop
//! runs before the profile rows are deleted.

use std::time::Duration;

use redis::aio::ConnectionManager;

use quarry_browser::session::SessionSnapshot;
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::org_erasure::{scan_unlink, V2_CACHE_PREFIX};
use crate::profile_blob_store::{PlaintextRow, ProfileBlobStore};
use crate::profile_crypto::{self, CryptoError, ENC_V};
use crate::profile_keys::{KeyError, ProfileKeyProvider, OC_KEY_ID_PREFIX};

/// Default rows per batch.
pub const DEFAULT_BATCH: usize = 200;

/// Pattern of every Redis key that the legacy plaintext cache wrote.
const LEGACY_CACHE_PATTERN: &str = "quarry:profile:*";

/// How the run paces itself.
#[derive(Debug, Clone, Copy)]
pub struct BackfillOptions {
    /// Rows per batch (clamped to 1..=1000).
    pub batch: usize,
    /// Pause after each batch, to keep the database load low.
    pub pause_between_batches: Duration,
    /// Stop after this many rows were examined. `None` runs to the end.
    pub max_rows: Option<u64>,
}

impl Default for BackfillOptions {
    fn default() -> Self {
        Self {
            batch: DEFAULT_BATCH,
            pause_between_batches: Duration::from_millis(50),
            max_rows: None,
        }
    }
}

/// What one run did. Counts only; no id, no payload.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct BackfillReport {
    /// Plaintext rows read in this run.
    pub scanned: u64,
    /// Rows now holding an envelope (the compare-and-set applied).
    pub sealed: u64,
    /// Rows a live save changed first (the compare-and-set lost). Not an error.
    pub moved_on: u64,
    /// Rows that could not be sealed (the stored JSON is not a snapshot, or a
    /// seal failed). They stay plaintext for a later run and are not logged.
    pub failed: u64,
    /// Legacy plaintext cache keys removed from Redis.
    pub legacy_cache_keys_unlinked: u64,
}

fn key_err(e: KeyError) -> QuarryError {
    match e {
        KeyError::Unavailable | KeyError::Destroyed => QuarryError::new(
            ErrorCode::RuntimeNotReady,
            format!("profile key {}", e.class()),
        ),
        KeyError::Denied | KeyError::Invalid => {
            QuarryError::new(ErrorCode::Internal, format!("profile key {}", e.class()))
        }
    }
}

fn crypto_err(e: CryptoError) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("profile envelope {}", e.class()))
}

/// Seal one plaintext row and write it back with a compare-and-set on `rev`.
/// `Ok(true)` when the envelope is stored, `Ok(false)` when the row moved on.
/// A key-provider outage is an error (the run stops); a row that cannot be
/// sealed is also an error here and is counted by the caller.
async fn reseal_one(
    blobs: &dyn ProfileBlobStore,
    keys: &dyn ProfileKeyProvider,
    row: &PlaintextRow,
) -> QuarryResult<bool> {
    // The stored JSON must still read as a snapshot before it is sealed.
    serde_json::from_value::<SessionSnapshot>(row.json.clone()).map_err(|_| {
        QuarryError::new(ErrorCode::Internal, "plaintext row is not a snapshot")
    })?;
    let plain = serde_json::to_vec(&row.json)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "snapshot encode failed"))?;
    let org = row.org_id.as_str();
    let profile = row.profile_id.to_string();
    let generated = keys.generate(org, &profile).map_err(key_err)?;
    let env = profile_crypto::seal(
        &generated.dek,
        org,
        &profile,
        &plain,
        &generated.key_id,
        &generated.wrapped,
    )
    .map_err(crypto_err)?;
    let bytes = serde_json::to_vec(&env)
        .map_err(|_| QuarryError::new(ErrorCode::Internal, "envelope encode failed"))?;
    blobs
        .reseal(
            org,
            &row.profile_id,
            row.rev,
            bytes,
            ENC_V as i16,
            &generated.key_id,
        )
        .await
}

/// Run the backfill over every plaintext row, then unlink the legacy plaintext
/// cache keys when `redis` is given. Safe to repeat; a second run over a fully
/// backfilled store does no writes.
pub async fn run_backfill(
    blobs: &dyn ProfileBlobStore,
    keys: &dyn ProfileKeyProvider,
    redis: Option<ConnectionManager>,
    opts: BackfillOptions,
) -> QuarryResult<BackfillReport> {
    let batch = opts.batch.clamp(1, 1000);
    let mut report = BackfillReport::default();
    let mut cursor: Option<(String, String)> = None;
    loop {
        if opts.max_rows.is_some_and(|max| report.scanned >= max) {
            break;
        }
        let page = blobs.plaintext_page(cursor.as_ref(), batch).await?;
        let Some(last) = page.last() else {
            break;
        };
        cursor = Some((last.org_id.clone(), last.profile_id.to_string()));
        let full_page = page.len() == batch;
        for row in &page {
            report.scanned += 1;
            match reseal_one(blobs, keys, row).await {
                Ok(true) => report.sealed += 1,
                Ok(false) => report.moved_on += 1,
                Err(e) if e.code == ErrorCode::RuntimeNotReady => return Err(e),
                Err(_) => report.failed += 1,
            }
        }
        if !full_page {
            break;
        }
        if !opts.pause_between_batches.is_zero() {
            tokio::time::sleep(opts.pause_between_batches).await;
        }
    }
    if let Some(mut conn) = redis {
        report.legacy_cache_keys_unlinked = scan_unlink(&mut conn, LEGACY_CACHE_PATTERN, |k| {
            k.starts_with(V2_CACHE_PREFIX)
        })
        .await?;
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// W2-U41a: the E3 release gate for migration 0017 (D2-077).
//
// Migration 0017 clears the legacy column only beside an envelope and adds a
// NOT VALID check, so it never removes the only copy of a snapshot. What it
// cannot prove is that the backfill ran: a plaintext row can still be left. The
// release that turns profile encryption `on` must not ship while one is. This
// gate asks the store for one plaintext row at boot. `observe` logs the result;
// `enforce` refuses to boot in `on` mode while a plaintext row is left. Off by
// default, and in `off` or `compat` mode `enforce` only logs.
// ---------------------------------------------------------------------------

/// Env flag for the release gate: `off` (default), `observe` or `enforce`.
pub const RELEASE_GATE_ENV: &str = "QUARRY_PROFILE_BACKFILL_GATE";

/// How the boot treats the release gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReleaseGateMode {
    #[default]
    Off,
    Observe,
    Enforce,
}

impl ReleaseGateMode {
    /// Parse [`RELEASE_GATE_ENV`]. Unset or empty is `off`; an unknown value is
    /// `None`, and boot refuses it rather than guessing.
    pub fn parse(raw: Option<&str>) -> Option<Self> {
        match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
            "" | "off" => Some(Self::Off),
            "observe" => Some(Self::Observe),
            "enforce" => Some(Self::Enforce),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Observe => "observe",
            Self::Enforce => "enforce",
        }
    }
}

/// What the gate saw. Counts only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseGateReport {
    /// Plaintext rows seen by the probe (0 or 1: the probe reads one row).
    pub plaintext_rows_seen: u64,
    /// No plaintext row is left: the backfill is complete.
    pub passed: bool,
}

/// Probe for one plaintext row. A pass means the backfill left no row.
pub async fn release_gate(blobs: &dyn ProfileBlobStore) -> QuarryResult<ReleaseGateReport> {
    let page = blobs.plaintext_page(None, 1).await?;
    Ok(ReleaseGateReport {
        plaintext_rows_seen: page.len() as u64,
        passed: page.is_empty(),
    })
}

/// Apply the gate at boot. `encryption_on` is true when the profile encryption
/// mode is `on` (the mode that 0017's header calls the no-rollback release).
/// Returns `Ok(None)` when the gate is off, the report otherwise, and an error
/// only in `enforce` mode with `encryption_on` and a plaintext row left.
pub async fn check_release_gate(
    blobs: &dyn ProfileBlobStore,
    mode: ReleaseGateMode,
    encryption_on: bool,
) -> QuarryResult<Option<ReleaseGateReport>> {
    if mode == ReleaseGateMode::Off {
        return Ok(None);
    }
    let report = release_gate(blobs).await?;
    if report.passed {
        tracing::info!(gate = mode.as_str(), "profile backfill gate: no plaintext row left");
        return Ok(Some(report));
    }
    if mode == ReleaseGateMode::Enforce && encryption_on {
        return Err(QuarryError::new(
            ErrorCode::RuntimeNotReady,
            "profile backfill gate: plaintext profile rows are left; run profiles_backfill \
             before profile encryption `on` (docs/RUNBOOKS.md, WS17.E3)",
        ));
    }
    tracing::warn!(
        gate = mode.as_str(),
        encryption_on,
        "profile backfill gate: plaintext profile rows are left; run profiles_backfill"
    );
    Ok(Some(report))
}

/// W2-Q-U41b: whether any of the org's profile rows is sealed under an org-core
/// KEK (a `key_id` that starts with `oc:`). Reads the org's list, then each row
/// until one matches, so an org without an `oc:` row costs one list and one get
/// per row. The erasure's crypto-shred hop runs it before the rows are deleted,
/// when the edge's own key provider is not `org_core`. Ids, key ids and payloads
/// never leave this function.
pub async fn org_has_oc_rows(blobs: &dyn ProfileBlobStore, org_id: &str) -> QuarryResult<bool> {
    for summary in blobs.list(org_id).await? {
        let Some(row) = blobs.get(org_id, &summary.profile_id).await? else {
            continue;
        };
        if row
            .key_id
            .as_deref()
            .is_some_and(|k| k.starts_with(OC_KEY_ID_PREFIX))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;
    use chrono::{Duration as ChronoDuration, Utc};

    use quarry_browser::session::SaveMeta;
    use quarry_core::ids::kinds::ProfileKind;

    use crate::profile_blob_store::{BlobPayload, InMemoryBlobStore};
    use crate::profile_keys::EnvKeyProvider;

    fn keys() -> EnvKeyProvider {
        let json = format!(r#"{{"k1":"{}"}}"#, B64.encode([9u8; 32]));
        EnvKeyProvider::from_config(&json, "k1").expect("test master key")
    }

    fn legacy_payload() -> BlobPayload {
        BlobPayload::Legacy {
            json: serde_json::to_value(SessionSnapshot::default()).expect("snapshot json"),
        }
    }

    fn meta_now() -> SaveMeta {
        SaveMeta {
            saved_at: Utc::now(),
            owner: None,
        }
    }

    fn no_pause(batch: usize) -> BackfillOptions {
        BackfillOptions {
            batch,
            pause_between_batches: Duration::ZERO,
            max_rows: None,
        }
    }

    #[tokio::test]
    async fn backfill_seals_every_plaintext_row_and_is_idempotent() {
        let store = Arc::new(InMemoryBlobStore::new());
        let keys = keys();
        let mut ids = Vec::new();
        for n in 0..5 {
            let org = if n < 3 { "org_bf_a" } else { "org_bf_b" };
            let p = ProfileKind::new();
            store.put(org, &p, legacy_payload(), &meta_now()).await.unwrap();
            ids.push((org, p));
        }

        // Batches of two, so the cursor crosses pages.
        let first = run_backfill(store.as_ref(), &keys, None, no_pause(2))
            .await
            .expect("first run");
        assert_eq!(first.scanned, 5);
        assert_eq!(first.sealed, 5);
        assert_eq!(first.failed, 0);
        assert_eq!(first.moved_on, 0);
        for (org, p) in &ids {
            let row = store.get(org, p).await.unwrap().expect("row");
            assert!(row.is_envelope(), "every row now holds an envelope");
            assert_eq!(row.legacy_json, None, "the plaintext is gone from the row");
            assert_eq!(row.key_id.as_deref(), Some("env:k1"));
        }
        assert!(store.plaintext_page(None, 10).await.unwrap().is_empty());

        let second = run_backfill(store.as_ref(), &keys, None, no_pause(2))
            .await
            .expect("second run");
        assert_eq!(second.scanned, 0, "a fully backfilled store does no work");
        assert_eq!(second.sealed, 0);
    }

    #[tokio::test]
    async fn backfill_cas_loses_to_a_newer_save() {
        let store = InMemoryBlobStore::new();
        let keys = keys();
        let org = "org_bf_cas";
        let p = ProfileKind::new();
        store.put(org, &p, legacy_payload(), &meta_now()).await.unwrap();

        // The backfill reads the row at rev 1 ...
        let page = store.plaintext_page(None, 10).await.unwrap();
        let row = page
            .iter()
            .find(|r| r.org_id == org && r.profile_id == p)
            .expect("row in page");
        assert_eq!(row.rev, 1);

        // ... then a live save lands a newer snapshot (rev 2) before the seal.
        let newer = SaveMeta {
            saved_at: Utc::now() + ChronoDuration::seconds(1),
            owner: None,
        };
        store.put(org, &p, legacy_payload(), &newer).await.unwrap();

        // The stale seal must not overwrite the newer save.
        let applied = reseal_one(&store, &keys, row).await.expect("reseal call");
        assert!(!applied, "compare-and-set on rev lost to the newer save");
        let after = store.get(org, &p).await.unwrap().expect("row");
        assert!(!after.is_envelope(), "the newer plaintext row is untouched");
        assert_eq!(after.rev, 2);
    }

    #[tokio::test]
    async fn backfill_unlinks_old_plaintext_cache_keys() {
        let Ok(url) = std::env::var("REDIS_URL") else {
            return;
        };
        let client = redis::Client::open(url.as_str()).expect("redis client");
        let mut conn = ConnectionManager::new(client).await.expect("redis");
        let store = InMemoryBlobStore::new();
        let keys = keys();
        let org = format!("org_bf_redis_{}", ProfileKind::new());
        let p = ProfileKind::new();
        store.put(&org, &p, legacy_payload(), &meta_now()).await.unwrap();
        let legacy = format!("quarry:profile:{org}:{p}");
        let v2 = format!("{V2_CACHE_PREFIX}{org}:{p}");
        for k in [&legacy, &v2] {
            let _: () = redis::cmd("SET").arg(k).arg("x").query_async(&mut conn).await.unwrap();
        }

        let report = run_backfill(&store, &keys, Some(conn.clone()), no_pause(10))
            .await
            .expect("run with redis");
        assert!(report.legacy_cache_keys_unlinked >= 1);
        let exists = |k: String| {
            let mut c = conn.clone();
            async move {
                let n: u64 = redis::cmd("EXISTS").arg(k).query_async(&mut c).await.unwrap();
                n
            }
        };
        assert_eq!(exists(legacy.clone()).await, 0, "legacy plaintext key unlinked");
        assert_eq!(exists(v2.clone()).await, 1, "envelope cache key is kept");
        let _: u64 = redis::cmd("UNLINK").arg(&v2).query_async(&mut conn).await.unwrap();
    }

    #[test]
    fn release_gate_mode_parses() {
        assert_eq!(ReleaseGateMode::parse(None), Some(ReleaseGateMode::Off));
        assert_eq!(ReleaseGateMode::parse(Some(" ")), Some(ReleaseGateMode::Off));
        assert_eq!(ReleaseGateMode::parse(Some("Observe")), Some(ReleaseGateMode::Observe));
        assert_eq!(ReleaseGateMode::parse(Some("enforce")), Some(ReleaseGateMode::Enforce));
        assert_eq!(ReleaseGateMode::parse(Some("required")), None);
    }

    /// The gate fails while a plaintext row is left, refuses boot only in
    /// `enforce` with encryption `on`, and passes once the backfill ran.
    #[tokio::test]
    async fn release_gate_blocks_on_until_the_backfill_ran() {
        let store = InMemoryBlobStore::new();
        let keys = keys();
        store
            .put("org_gate", &ProfileKind::new(), legacy_payload(), &meta_now())
            .await
            .unwrap();

        assert_eq!(
            check_release_gate(&store, ReleaseGateMode::Off, true).await.unwrap(),
            None
        );
        let seen = release_gate(&store).await.unwrap();
        assert_eq!(seen.plaintext_rows_seen, 1);
        assert!(!seen.passed);
        let observe = check_release_gate(&store, ReleaseGateMode::Observe, true)
            .await
            .unwrap()
            .expect("report");
        assert!(!observe.passed, "observe reports and never refuses");
        assert!(check_release_gate(&store, ReleaseGateMode::Enforce, false)
            .await
            .is_ok(), "enforce in compat or off mode only logs");
        let err = check_release_gate(&store, ReleaseGateMode::Enforce, true)
            .await
            .expect_err("enforce with encryption on refuses");
        assert_eq!(err.code, ErrorCode::RuntimeNotReady);

        run_backfill(&store, &keys, None, no_pause(10)).await.expect("backfill");
        let after = check_release_gate(&store, ReleaseGateMode::Enforce, true)
            .await
            .unwrap()
            .expect("report");
        assert!(after.passed);
        assert_eq!(after.plaintext_rows_seen, 0);
    }

    /// W2-Q-U41b: the probe sees `oc:` rows only, never `env:` or plaintext rows,
    /// and never another org's rows.
    #[tokio::test]
    async fn org_has_oc_rows_sees_only_org_core_sealed_rows() {
        let store = InMemoryBlobStore::new();
        let envelope = |key_id: &str| BlobPayload::Envelope {
            envelope: b"{}".to_vec(),
            enc_v: 1,
            key_id: key_id.to_string(),
        };
        store
            .put("org_probe_env", &ProfileKind::new(), legacy_payload(), &meta_now())
            .await
            .unwrap();
        store
            .put("org_probe_env", &ProfileKind::new(), envelope("env:k1"), &meta_now())
            .await
            .unwrap();
        store
            .put("org_probe_oc", &ProfileKind::new(), legacy_payload(), &meta_now())
            .await
            .unwrap();
        store
            .put("org_probe_oc", &ProfileKind::new(), envelope("oc:2"), &meta_now())
            .await
            .unwrap();

        assert!(!org_has_oc_rows(&store, "org_probe_env").await.unwrap());
        assert!(org_has_oc_rows(&store, "org_probe_oc").await.unwrap());
        assert!(!org_has_oc_rows(&store, "org_probe_none").await.unwrap());
    }
}
