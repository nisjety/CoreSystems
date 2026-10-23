//! Org erasure for the profile families (WS17.E3).
//!
//! `OrgErasable` is the list the edge's internal erasure route walks. Profiles
//! come first; a later family joins by one line in the edge's wiring.
//! `ProfileErasure` deletes an org's `quarry_profiles` rows and tombstones,
//! drops its Redis entries (the v2 envelope keys and any legacy plaintext
//! keys) and evicts its cached DEKs in this process. Counts only leave this
//! module: no id, key name or payload is returned or logged.
//!
//! Limits, stated where they bite: the DEK eviction covers the replica that
//! answers the call only (other replicas keep cached DEKs for up to the 300 s
//! TTL), and the Redis step is skipped when no Redis connection was built.

use std::sync::Arc;

use async_trait::async_trait;
use redis::aio::ConnectionManager;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::profile_blob_store::{BlobPurge, ProfileBlobStore};
use crate::profile_keys::ProfileKeyProvider;

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
}

impl ProfileErasure {
    pub fn new(
        blobs: Arc<dyn ProfileBlobStore>,
        keys: Option<Arc<dyn ProfileKeyProvider>>,
        redis: Option<ConnectionManager>,
    ) -> Self {
        Self { blobs, keys, redis }
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
        Self::new(blobs, keys, redis)
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
        let BlobPurge { rows, tombstones } = self.blobs.purge_org(org_id).await?;
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
        Ok(PurgeCounts {
            rows,
            tombstones,
            cache_keys,
            keys_evicted,
        })
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
