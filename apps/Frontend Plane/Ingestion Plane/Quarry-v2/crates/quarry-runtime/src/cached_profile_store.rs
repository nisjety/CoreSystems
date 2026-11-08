//! Redis hot-restore cache in front of any `ProfileStore`.
//!
//! Cycle 24 / cluster #6.
//!
//! Wraps an inner store (Postgres, S3, in-memory) and caches each
//! profile's `SessionSnapshot` JSON in Redis under
//! `quarry:profile:{org_id}:{profile_id}` with a 1-hour TTL.
//!
//! Read-through: `load` checks Redis first; on miss it falls back to
//! the inner store and populates the cache. Invalidate-on-write (WS17.P1):
//! `save` and `delete` drop the Redis key after the inner store has
//! answered, and never write a payload themselves. A write-through `set_ex`
//! after an ignored (stale or tombstoned) save would cache a payload the
//! inner store had just refused, for a full TTL.
//!
//! `list` is pass-through (we don't cache the org's id-set because it can grow
//! unboundedly and is cheap to compute against Postgres anyway).
//!
//! Why this matters: the warm-path scrape that needs to restore a
//! profile pays a Postgres round-trip on every restore unless we
//! cache. With ~100k profiles spread across hosts, that's a
//! noticeable p50/p95 latency hit on every authenticated crawl.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

use quarry_browser::session::{
    ProfileMetadata, ProfileStore, ProfileSummary, SaveMeta, SaveOutcome, SessionSnapshot,
    StampOutcome,
};
use quarry_core::error::QuarryResult;
use quarry_core::ids::kinds::ProfileKind;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;

use crate::profile_blob_store::{BlobPayload, BlobPurge, BlobRow, PlaintextRow, ProfileBlobStore};

/// Default TTL for cached snapshot blobs. 1 hour balances:
///   - too long → stale state after a `save` from a peer instance
///     that didn't bust this instance's Redis (we DO write-through
///     locally, but a peer's write doesn't reach this instance's
///     cache until eviction)
///   - too short → cache is mostly cold, wasted hop
///
/// 1h is also longer than typical Quarry session windows, so the
/// cache covers the full lifetime of an active scrape session.
pub const DEFAULT_TTL_S: u64 = 3600;

/// Cache key shape — pinned for cross-instance interop.
///
/// `quarry:profile:<org_id>:<profile_id>` lets a separate process
/// (admin tool, sweeper) invalidate a specific org's cache cheaply
/// via `redis-cli DEL quarry:profile:org_a:*`.
fn cache_key(org_id: &str, profile_id: &ProfileKind) -> String {
    format!("quarry:profile:{org_id}:{profile_id}")
}

/// Wraps any `ProfileStore` with a Redis read-through / write-through
/// cache. The inner store is the authoritative source — Redis is a
/// performance optimization, not a durability tier.
pub struct CachedProfileStore {
    inner: Arc<dyn ProfileStore>,
    redis: ConnectionManager,
    ttl: Duration,
}

impl CachedProfileStore {
    /// Construct with the default 1-hour TTL.
    pub fn new(inner: Arc<dyn ProfileStore>, redis: ConnectionManager) -> Self {
        Self {
            inner,
            redis,
            ttl: Duration::from_secs(DEFAULT_TTL_S),
        }
    }

    /// Override the cache TTL — useful for tests that need rapid
    /// expiration without sleeping.
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }
}

#[async_trait]
impl ProfileStore for CachedProfileStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        // The inner store is authoritative and decides whether this save is
        // stored (it may ignore a stale or tombstoned one). Inner first: if
        // Redis were touched first and Postgres then failed, the cache would
        // hold a value that is not durable. Afterwards only the key is
        // dropped, so the next load reads the inner store's answer.
        let outcome = self.inner.save(org_id, profile_id, snapshot, meta).await?;
        let mut redis = self.redis.clone();
        let _: redis::RedisResult<()> = redis.del(cache_key(org_id, profile_id)).await;
        Ok(outcome)
    }

    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        // Read-through.
        let key = cache_key(org_id, profile_id);
        let mut redis = self.redis.clone();
        let cached: redis::RedisResult<Option<Vec<u8>>> = redis.get(&key).await;
        if let Ok(Some(bytes)) = cached {
            if let Ok(snap) = serde_json::from_slice::<SessionSnapshot>(&bytes) {
                return Ok(Some(snap));
            }
            // Bad cached payload — drop the key and fall through.
            let _: redis::RedisResult<()> = redis.del(&key).await;
        }

        let snap = self.inner.load(org_id, profile_id).await?;
        if let Some(s) = &snap {
            if let Ok(bytes) = serde_json::to_vec(s) {
                let _: redis::RedisResult<()> = self
                    .redis
                    .clone()
                    .set_ex(&key, bytes, self.ttl.as_secs())
                    .await;
            }
        }
        Ok(snap)
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        // Inner first so a Redis failure can't leave a phantom value.
        self.inner.delete(org_id, profile_id).await?;
        let mut redis = self.redis.clone();
        let _: redis::RedisResult<()> = redis.del(cache_key(org_id, profile_id)).await;
        Ok(())
    }

    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        // Existence is not cached: it is a cheap inner probe and the cache
        // holds snapshots, not row presence.
        self.inner.exists(org_id, profile_id).await
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        // List bypasses the cache — see the module-level doc. The
        // inner store's `list` is already cheap because the schema
        // has an org_id-prefixed index.
        let _ = org_id;
        self.inner.list(org_id).await
    }

    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()> {
        // Pass-through, same rationale as `list`: name/scope changes are
        // low-frequency admin actions, not the hot restore path this
        // cache exists for. Caching them would also risk a stale name/
        // scope surviving past a rename until TTL expiry.
        self.inner.save_metadata(org_id, profile_id, metadata).await
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        self.inner.load_metadata(org_id, profile_id).await
    }

    async fn stamp_owner(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
        saved_at: chrono::DateTime<chrono::Utc>,
    ) -> QuarryResult<StampOutcome> {
        // Pass-through, like `save_metadata`: the stamp only creates a row that did not
        // exist, so there is no cached snapshot to invalidate. The inner store keeps the
        // tombstones.
        self.inner
            .stamp_owner(org_id, profile_id, metadata, saved_at)
            .await
    }
}

/// Cache key for envelope rows (WS17.E1). The `v2` segment keeps these keys
/// apart from the legacy plaintext keys (`quarry:profile:{org}:{profile}`),
/// which the envelope layer never reads.
pub fn blob_cache_key(org_id: &str, profile_id: &ProfileKind) -> String {
    format!("quarry:profile:v2:{org_id}:{profile_id}")
}

/// What the cache holds for one envelope row: the envelope bytes (base64 of
/// the `EnvelopeV1` JSON) and the header fields. Never a plaintext payload.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedEnvelopeRow {
    rev: i64,
    enc_v: i16,
    key_id: String,
    saved_at_ms: Option<i64>,
    envelope_b64: String,
}

/// Redis cache in front of a `ProfileBlobStore` for envelope rows (WS17.E1).
///
/// Only rows with an envelope are cached. A legacy plaintext row is read from
/// the inner store every time, so plaintext never reaches Redis through this
/// layer. Writes (`put`, `reseal`, `delete`) drop the key after the inner store
/// has answered, the same invalidate-on-write rule as `CachedProfileStore`.
pub struct CachedBlobStore {
    inner: Arc<dyn ProfileBlobStore>,
    redis: ConnectionManager,
    ttl: Duration,
}

impl CachedBlobStore {
    pub fn new(inner: Arc<dyn ProfileBlobStore>, redis: ConnectionManager) -> Self {
        Self {
            inner,
            redis,
            ttl: Duration::from_secs(DEFAULT_TTL_S),
        }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    async fn drop_key(&self, org_id: &str, profile_id: &ProfileKind) {
        let mut redis = self.redis.clone();
        let _: redis::RedisResult<()> = redis.del(blob_cache_key(org_id, profile_id)).await;
    }
}

fn row_from_cache(c: CachedEnvelopeRow) -> Option<BlobRow> {
    let envelope = B64.decode(c.envelope_b64.as_bytes()).ok()?;
    Some(BlobRow {
        envelope: Some(envelope),
        legacy_json: None,
        saved_at: c
            .saved_at_ms
            .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis),
        rev: c.rev,
        enc_v: Some(c.enc_v),
        key_id: Some(c.key_id),
    })
}

#[async_trait]
impl ProfileBlobStore for CachedBlobStore {
    async fn put(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        payload: BlobPayload,
        meta: &SaveMeta,
    ) -> QuarryResult<SaveOutcome> {
        // Inner first: the store decides, then the key is dropped.
        let outcome = self.inner.put(org_id, profile_id, payload, meta).await?;
        self.drop_key(org_id, profile_id).await;
        Ok(outcome)
    }

    async fn reseal(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        expected_rev: i64,
        envelope: Vec<u8>,
        enc_v: i16,
        key_id: &str,
    ) -> QuarryResult<bool> {
        let done = self
            .inner
            .reseal(org_id, profile_id, expected_rev, envelope, enc_v, key_id)
            .await?;
        self.drop_key(org_id, profile_id).await;
        Ok(done)
    }

    async fn get(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<Option<BlobRow>> {
        let key = blob_cache_key(org_id, profile_id);
        let mut redis = self.redis.clone();
        let cached: redis::RedisResult<Option<Vec<u8>>> = redis.get(&key).await;
        if let Ok(Some(bytes)) = cached {
            if let Some(row) = serde_json::from_slice::<CachedEnvelopeRow>(&bytes)
                .ok()
                .and_then(row_from_cache)
            {
                return Ok(Some(row));
            }
            // Unreadable entry: drop it and fall through to the inner store.
            let _: redis::RedisResult<()> = redis.del(&key).await;
        }
        let row = self.inner.get(org_id, profile_id).await?;
        if let Some(r) = &row {
            if let (Some(envelope), Some(enc_v), Some(key_id)) = (&r.envelope, r.enc_v, &r.key_id) {
                let entry = CachedEnvelopeRow {
                    rev: r.rev,
                    enc_v,
                    key_id: key_id.clone(),
                    saved_at_ms: r.saved_at.map(|t| t.timestamp_millis()),
                    envelope_b64: B64.encode(envelope),
                };
                if let Ok(bytes) = serde_json::to_vec(&entry) {
                    let _: redis::RedisResult<()> = self
                        .redis
                        .clone()
                        .set_ex(&key, bytes, self.ttl.as_secs())
                        .await;
                }
            }
        }
        Ok(row)
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        self.inner.delete(org_id, profile_id).await?;
        self.drop_key(org_id, profile_id).await;
        Ok(())
    }

    async fn exists(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<bool> {
        self.inner.exists(org_id, profile_id).await
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
        self.inner.list(org_id).await
    }

    async fn save_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
    ) -> QuarryResult<()> {
        self.inner.save_metadata(org_id, profile_id, metadata).await
    }

    async fn load_metadata(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<ProfileMetadata>> {
        self.inner.load_metadata(org_id, profile_id).await
    }

    async fn stamp_owner(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        metadata: &ProfileMetadata,
        saved_at: chrono::DateTime<chrono::Utc>,
    ) -> QuarryResult<StampOutcome> {
        self.inner
            .stamp_owner(org_id, profile_id, metadata, saved_at)
            .await
    }

    async fn plaintext_page(
        &self,
        after: Option<&(String, String)>,
        limit: usize,
    ) -> QuarryResult<Vec<PlaintextRow>> {
        self.inner.plaintext_page(after, limit).await
    }

    // The database rows only. The Redis entries of the org are dropped by
    // org_erasure::ProfileErasure, which holds the connection.
    async fn purge_org(&self, org_id: &str) -> QuarryResult<BlobPurge> {
        self.inner.purge_org(org_id).await
    }
}

/// The URL with any `user:password@` removed, for logging. A Redis URL carries the
/// password in its authority, and the edge logs this one at INFO.
fn redact_credentials(url: &str) -> String {
    match (url.find("://"), url.rfind('@')) {
        (Some(scheme_end), Some(at)) if at > scheme_end => {
            format!("{}***@{}", &url[..scheme_end + 3], &url[at + 1..])
        }
        _ => url.to_string(),
    }
}

/// Small helper for `main.rs` — wraps an inner store iff `redis_url`
/// is set; otherwise returns the inner store unwrapped. Lets the
/// wire be uniform regardless of deployment shape.
pub async fn maybe_cache(
    inner: Arc<dyn ProfileStore>,
    redis_url: Option<&str>,
) -> Arc<dyn ProfileStore> {
    let Some(url) = redis_url.filter(|s| !s.is_empty()) else {
        return inner;
    };
    let client = match redis::Client::open(url) {
        Ok(c) => c,
        Err(e) => {
            // zdr-log-ok: boot-time Redis configuration error, no request span exists
            tracing::warn!(error = %e, "Redis client init failed; profile cache disabled");
            return inner;
        }
    };
    match ConnectionManager::new(client).await {
        Ok(manager) => {
            // zdr-log-ok: boot-time log of the operator-configured Redis URL (credentials redacted), no request span exists
            tracing::info!(url = %redact_credentials(url), "ProfileStore Redis cache attached");
            Arc::new(CachedProfileStore::new(inner, manager))
        }
        Err(e) => {
            // zdr-log-ok: boot-time Redis connection error, no request span exists
            tracing::warn!(error = %e, "Redis ConnectionManager failed; profile cache disabled");
            inner
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn logged_redis_url_never_carries_the_password() {
        assert_eq!(
            redact_credentials("redis://:s3cret@dragonfly:6379/6"),
            "redis://***@dragonfly:6379/6"
        );
        assert_eq!(
            redact_credentials("redis://user:s3cret@host/0"),
            "redis://***@host/0"
        );
        assert_eq!(redact_credentials("redis://host:6379/0"), "redis://host:6379/0");
    }

    use super::*;
    use quarry_browser::session::InMemoryProfileStore;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Wrap an InMemoryProfileStore + count inner-load calls so the
    /// cache-hit assertion is observable.
    struct CountingStore {
        inner: InMemoryProfileStore,
        loads: AtomicUsize,
    }

    impl CountingStore {
        fn new() -> Self {
            Self {
                inner: InMemoryProfileStore::new(),
                loads: AtomicUsize::new(0),
            }
        }
        fn load_count(&self) -> usize {
            self.loads.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl ProfileStore for CountingStore {
        async fn save(
            &self,
            org_id: &str,
            profile_id: &ProfileKind,
            snapshot: &SessionSnapshot,
            meta: &SaveMeta,
        ) -> QuarryResult<SaveOutcome> {
            self.inner.save(org_id, profile_id, snapshot, meta).await
        }
        async fn load(
            &self,
            org_id: &str,
            profile_id: &ProfileKind,
        ) -> QuarryResult<Option<SessionSnapshot>> {
            self.loads.fetch_add(1, Ordering::Relaxed);
            self.inner.load(org_id, profile_id).await
        }
        async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
            self.inner.delete(org_id, profile_id).await
        }
        async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileSummary>> {
            self.inner.list(org_id).await
        }
        async fn save_metadata(
            &self,
            org_id: &str,
            profile_id: &ProfileKind,
            metadata: &ProfileMetadata,
        ) -> QuarryResult<()> {
            self.inner.save_metadata(org_id, profile_id, metadata).await
        }
        async fn load_metadata(
            &self,
            org_id: &str,
            profile_id: &ProfileKind,
        ) -> QuarryResult<Option<ProfileMetadata>> {
            self.inner.load_metadata(org_id, profile_id).await
        }
    }

    /// Tests gracefully skip without a local Redis. The cache logic
    /// uses real Redis commands (`SETEX`, `GET`, `DEL`) — a stub
    /// would let us miss serialization bugs.
    async fn redis_manager() -> Option<ConnectionManager> {
        let url = std::env::var("REDIS_URL").ok().filter(|s| !s.is_empty())?;
        let client = redis::Client::open(url.as_str()).ok()?;
        ConnectionManager::new(client).await.ok()
    }

    #[test]
    fn cache_key_format_is_stable() {
        // Pin the key shape — admin tools rely on `quarry:profile:<org>:*`
        // for org-wide invalidation.
        let id: ProfileKind = quarry_core::ids::Id::new();
        let k = cache_key("org_alpha", &id);
        assert!(k.starts_with("quarry:profile:org_alpha:"));
        assert!(k.ends_with(&id.to_string()));
    }

    #[test]
    fn default_ttl_is_one_hour() {
        assert_eq!(DEFAULT_TTL_S, 3600);
    }

    #[tokio::test]
    async fn second_load_hits_cache_not_inner() {
        let Some(redis) = redis_manager().await else {
            eprintln!("skipping: REDIS_URL not set");
            return;
        };
        let inner = Arc::new(CountingStore::new());
        let store = CachedProfileStore::new(inner.clone(), redis);
        let id: ProfileKind = quarry_core::ids::Id::new();
        let snap = SessionSnapshot::default();

        // Seed via the raw inner store, NOT `store.save()` — the cache is
        // write-through on save (see module docs), so going through the
        // wrapper would pre-warm Redis and make every `load()` below a
        // guaranteed hit regardless of the read-through path under test.
        // Seeding `inner` directly leaves the cache genuinely cold so the
        // first `store.load()` below exercises a real cache miss.
        inner
            .save("org_a", &id, &snap, &SaveMeta::now(None))
            .await
            .unwrap();
        let _ = store.load("org_a", &id).await.unwrap();
        let _ = store.load("org_a", &id).await.unwrap();
        let _ = store.load("org_a", &id).await.unwrap();
        // First load populated the cache; subsequent loads should be
        // Redis hits → inner.load called only once.
        assert_eq!(inner.load_count(), 1);
    }

    #[tokio::test]
    async fn delete_invalidates_cache() {
        let Some(redis) = redis_manager().await else {
            return;
        };
        let inner = Arc::new(CountingStore::new());
        let store = CachedProfileStore::new(inner.clone(), redis);
        let id: ProfileKind = quarry_core::ids::Id::new();
        // Seed via `inner` directly for the same reason as above — a
        // cold cache makes the first `store.load()` a real, countable
        // inner hit instead of an immediate write-through cache hit.
        inner
            .save(
                "org_a",
                &id,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();
        let _ = store.load("org_a", &id).await.unwrap();
        store.delete("org_a", &id).await.unwrap();
        // Post-delete load must hit inner (cache should be busted)
        // AND inner returns None.
        let got = store.load("org_a", &id).await.unwrap();
        assert!(got.is_none());
        assert!(inner.load_count() >= 2, "delete should bust cache");
    }

    #[tokio::test]
    async fn cache_is_invalidated_not_populated_on_ignored_save() {
        let Some(redis) = redis_manager().await else {
            eprintln!("skipping: REDIS_URL not set");
            return;
        };
        let inner = Arc::new(InMemoryProfileStore::new());
        let store = CachedProfileStore::new(inner.clone(), redis);
        let id: ProfileKind = quarry_core::ids::Id::new();
        let now = chrono::Utc::now();
        // A newer capture reaches the inner store first.
        let newer = SessionSnapshot {
            user_agent: Some("newer".into()),
            ..SessionSnapshot::default()
        };
        inner
            .save(
                "org_a",
                &id,
                &newer,
                &SaveMeta {
                    saved_at: now,
                    owner: None,
                },
            )
            .await
            .unwrap();
        // An older capture arrives through the cache wrapper and is ignored.
        let older = SessionSnapshot {
            user_agent: Some("older".into()),
            ..SessionSnapshot::default()
        };
        let outcome = store
            .save(
                "org_a",
                &id,
                &older,
                &SaveMeta {
                    saved_at: now - chrono::Duration::seconds(10),
                    owner: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(outcome, SaveOutcome::IgnoredStale);
        // The ignored payload must not be cached: the next load reads the newer row.
        let loaded = store.load("org_a", &id).await.unwrap().unwrap();
        assert_eq!(loaded.user_agent.as_deref(), Some("newer"));
    }

    #[tokio::test]
    async fn metadata_methods_pass_through_to_inner_uncached() {
        let Some(redis) = redis_manager().await else {
            eprintln!("skipping: REDIS_URL not set");
            return;
        };
        let inner = Arc::new(CountingStore::new());
        let store = CachedProfileStore::new(inner.clone(), redis);
        let id: ProfileKind = quarry_core::ids::Id::new();
        store
            .save(
                "org_a",
                &id,
                &SessionSnapshot::default(),
                &SaveMeta::now(None),
            )
            .await
            .unwrap();

        // A row saved without metadata reads back as the stored default (D16 Q2): user_private,
        // no name. The old expectation, `None`, held for no store: the snapshot row exists.
        let default = store.load_metadata("org_a", &id).await.unwrap().unwrap();
        assert_eq!(
            default.scope,
            quarry_browser::session::ProfileScope::UserPrivate
        );
        assert!(default.name.is_none());

        let metadata = ProfileMetadata {
            name: Some("Shared support inbox".into()),
            scope: quarry_browser::session::ProfileScope::OrgShared,
            owner: None,
        };
        store.save_metadata("org_a", &id, &metadata).await.unwrap();

        let loaded = store.load_metadata("org_a", &id).await.unwrap().unwrap();
        assert_eq!(loaded.name.as_deref(), Some("Shared support inbox"));
        assert_eq!(
            loaded.scope,
            quarry_browser::session::ProfileScope::OrgShared
        );

        // list() is already documented pass-through; confirm it carries
        // the metadata through the wrapper too.
        let list = store.list("org_a").await.unwrap();
        let summary = list.iter().find(|s| s.profile_id == id).unwrap();
        assert_eq!(summary.name.as_deref(), Some("Shared support inbox"));
    }
}

#[cfg(test)]
mod blob_tests {
    use super::*;
    use crate::profile_blob_store::InMemoryBlobStore;

    async fn redis_manager() -> Option<ConnectionManager> {
        let url = std::env::var("REDIS_URL").ok().filter(|s| !s.is_empty())?;
        let client = redis::Client::open(url.as_str()).ok()?;
        ConnectionManager::new(client).await.ok()
    }

    fn meta() -> SaveMeta {
        SaveMeta::now(None)
    }

    #[tokio::test]
    async fn cache_holds_envelope_bytes_and_never_cookie_values() {
        let Some(redis) = redis_manager().await else {
            eprintln!("skipping: REDIS_URL not set");
            return;
        };
        let inner: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let store = CachedBlobStore::new(inner.clone(), redis.clone());
        let p = ProfileKind::new();
        let env = br#"{"enc_v":1,"alg":"A256GCM","key_id":"env:v1","wrapped_dek":"AA==","snapshot_id":"snap_x","z":"none","nonce":"AA==","ct":"AA=="}"#.to_vec();
        store
            .put("org_a", &p, BlobPayload::Envelope { envelope: env.clone(), enc_v: 1, key_id: "env:v1".into() }, &meta())
            .await
            .unwrap();
        let _ = store.get("org_a", &p).await.unwrap();
        let mut conn = redis.clone();
        let raw: Option<Vec<u8>> = conn.get(blob_cache_key("org_a", &p)).await.unwrap();
        let raw = raw.expect("envelope row is cached");
        assert!(!raw.windows(6).any(|w| w == b"CANARY"));
        let back = serde_json::from_slice::<CachedEnvelopeRow>(&raw).expect("cache entry parses");
        assert_eq!(B64.decode(back.envelope_b64.as_bytes()).unwrap(), env);
        let _: () = conn.del(blob_cache_key("org_a", &p)).await.unwrap();
    }

    #[tokio::test]
    async fn legacy_rows_are_never_cached() {
        let Some(redis) = redis_manager().await else {
            eprintln!("skipping: REDIS_URL not set");
            return;
        };
        let inner: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let store = CachedBlobStore::new(inner, redis.clone());
        let p = ProfileKind::new();
        store
            .put("org_a", &p, BlobPayload::Legacy { json: serde_json::json!({"cookies": []}) }, &meta())
            .await
            .unwrap();
        let _ = store.get("org_a", &p).await.unwrap();
        let mut conn = redis.clone();
        let raw: Option<Vec<u8>> = conn.get(blob_cache_key("org_a", &p)).await.unwrap();
        assert!(raw.is_none(), "plaintext rows must not reach the cache");
    }

    #[tokio::test]
    async fn delete_and_reseal_invalidate_the_blob_cache() {
        let Some(redis) = redis_manager().await else {
            eprintln!("skipping: REDIS_URL not set");
            return;
        };
        let inner: Arc<dyn ProfileBlobStore> = Arc::new(InMemoryBlobStore::new());
        let store = CachedBlobStore::new(inner, redis.clone());
        let p = ProfileKind::new();
        store
            .put("org_a", &p, BlobPayload::Envelope { envelope: b"{}".to_vec(), enc_v: 1, key_id: "env:v1".into() }, &meta())
            .await
            .unwrap();
        let _ = store.get("org_a", &p).await.unwrap();
        let rev = store.get("org_a", &p).await.unwrap().unwrap().rev;
        assert!(store.reseal("org_a", &p, rev, b"{\"x\":1}".to_vec(), 1, "env:v1").await.unwrap());
        let mut conn = redis.clone();
        let raw: Option<Vec<u8>> = conn.get(blob_cache_key("org_a", &p)).await.unwrap();
        assert!(raw.is_none(), "reseal drops the key");
        let _ = store.get("org_a", &p).await.unwrap();
        store.delete("org_a", &p).await.unwrap();
        let raw: Option<Vec<u8>> = conn.get(blob_cache_key("org_a", &p)).await.unwrap();
        assert!(raw.is_none(), "delete drops the key");
    }
}
