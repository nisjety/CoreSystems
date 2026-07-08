//! Redis hot-restore cache in front of any `ProfileStore`.
//!
//! Cycle 24 / cluster #6.
//!
//! Wraps an inner store (Postgres, S3, in-memory) and caches each
//! profile's `SessionSnapshot` JSON in Redis under
//! `quarry:profile:{org_id}:{profile_id}` with a 1-hour TTL.
//!
//! Read-through: `load` checks Redis first; on miss it falls back to
//! the inner store and populates the cache. Write-through: `save`
//! and `delete` keep Redis in sync. `list` is pass-through (we don't
//! cache the org's id-set because it can grow unboundedly and is
//! cheap to compute against Postgres anyway).
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

use quarry_browser::session::{ProfileMetadata, ProfileStore, ProfileSummary, SessionSnapshot};
use quarry_core::error::QuarryResult;
use quarry_core::ids::kinds::ProfileKind;

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
    ) -> QuarryResult<()> {
        // Write-through: authoritative inner first; Redis second.
        // Order matters — if Redis succeeds but Postgres fails, the
        // cache holds a value that doesn't exist in durable storage.
        // Reversing the order keeps consistency on partial failure.
        self.inner.save(org_id, profile_id, snapshot).await?;
        let json = match serde_json::to_vec(snapshot) {
            Ok(b) => b,
            Err(e) => {
                // Failing to cache is non-fatal — the inner write
                // already succeeded. Log + return Ok.
                tracing::warn!(error = %e, "profile snapshot encode for Redis failed");
                return Ok(());
            }
        };
        let mut redis = self.redis.clone();
        let _: redis::RedisResult<()> = redis
            .set_ex(cache_key(org_id, profile_id), json, self.ttl.as_secs())
            .await;
        Ok(())
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
            tracing::warn!(error = %e, "Redis client init failed; profile cache disabled");
            return inner;
        }
    };
    match ConnectionManager::new(client).await {
        Ok(manager) => {
            tracing::info!(%url, "ProfileStore Redis cache attached");
            Arc::new(CachedProfileStore::new(inner, manager))
        }
        Err(e) => {
            tracing::warn!(error = %e, "Redis ConnectionManager failed; profile cache disabled");
            inner
        }
    }
}

#[cfg(test)]
mod tests {
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
        ) -> QuarryResult<()> {
            self.inner.save(org_id, profile_id, snapshot).await
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
        inner.save("org_a", &id, &snap).await.unwrap();
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
            .save("org_a", &id, &SessionSnapshot::default())
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
    async fn metadata_methods_pass_through_to_inner_uncached() {
        let Some(redis) = redis_manager().await else {
            eprintln!("skipping: REDIS_URL not set");
            return;
        };
        let inner = Arc::new(CountingStore::new());
        let store = CachedProfileStore::new(inner.clone(), redis);
        let id: ProfileKind = quarry_core::ids::Id::new();
        store
            .save("org_a", &id, &SessionSnapshot::default())
            .await
            .unwrap();

        assert!(store.load_metadata("org_a", &id).await.unwrap().is_none());

        let metadata = ProfileMetadata {
            name: Some("Shared support inbox".into()),
            scope: quarry_browser::session::ProfileScope::OrgShared,
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
