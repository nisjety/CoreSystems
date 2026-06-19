//! Lease pool with TTL, eviction, and session affinity.
//!
//! `LeasePool<D>` wraps a [`BrowserDriver`] and caches active
//! [`BrowserSession`]s keyed by `BrowserLease::session_affinity_key`.
//!
//! - Sessions stay warm until their TTL expires (`BrowserLease::ttl_s`).
//! - A tokio `Semaphore` caps concurrent live sessions.
//! - Expired entries are swept lazily on each `acquire`.
//! - Callers receive a [`PooledSession`] RAII guard that releases the
//!   permit on drop; the underlying session is kept in the pool for the
//!   next caller with the same affinity key.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{OwnedSemaphorePermit, RwLock, Semaphore};

use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::lease::BrowserLease;
use quarry_core::QuarryResult;

use crate::{BrowserDriver, BrowserSession};

struct Entry {
    session: Arc<BrowserSession>,
    expires_at: Instant,
}

/// Pool that reuses browser sessions across acquisitions sharing the
/// same `session_affinity_key`.
pub struct LeasePool<D: BrowserDriver> {
    driver: Arc<D>,
    sem: Arc<Semaphore>,
    entries: RwLock<HashMap<String, Entry>>,
}

impl<D: BrowserDriver> LeasePool<D> {
    pub fn new(driver: Arc<D>, max_concurrent: usize) -> Self {
        let cap = max_concurrent.max(1);
        Self {
            driver,
            sem: Arc::new(Semaphore::new(cap)),
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Acquire a pooled session for `lease`. Reuses an existing session
    /// when the affinity key matches a non-expired entry; otherwise asks
    /// the driver to provision a new one.
    pub async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<PooledSession> {
        self.sweep_expired().await;

        let permit =
            self.sem.clone().acquire_owned().await.map_err(|_| {
                QuarryError::new(ErrorCode::Internal, "lease pool semaphore closed")
            })?;

        let key = lease.session_affinity_key.clone();
        let ttl = Duration::from_secs(lease.ttl_s as u64);

        // Affinity hit: extend TTL and reuse.
        if let Some(entry) = self.entries.write().await.get_mut(&key) {
            entry.expires_at = Instant::now() + ttl;
            let session = entry.session.clone();
            return Ok(PooledSession {
                session,
                _permit: permit,
            });
        }

        // Miss: provision and cache.
        let new_session = self.driver.acquire(lease).await?;
        let session = Arc::new(new_session);
        self.entries.write().await.insert(
            key,
            Entry {
                session: session.clone(),
                expires_at: Instant::now() + ttl,
            },
        );

        Ok(PooledSession {
            session,
            _permit: permit,
        })
    }

    /// Drop expired entries from the cache. Sessions that still have
    /// outstanding `PooledSession` references stay alive via the Arc;
    /// the pool just stops handing them to new callers.
    async fn sweep_expired(&self) {
        let now = Instant::now();
        let mut map = self.entries.write().await;
        map.retain(|_, e| e.expires_at > now);
    }

    /// Manually evict the entry for `affinity_key`, if any.
    pub async fn evict(&self, affinity_key: &str) {
        self.entries.write().await.remove(affinity_key);
    }

    /// Number of cached entries (test/diagnostic helper).
    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }
}

/// RAII guard returned by [`LeasePool::acquire`]. Holds a semaphore
/// permit for the lifetime of the guard.
pub struct PooledSession {
    session: Arc<BrowserSession>,
    _permit: OwnedSemaphorePermit,
}

impl PooledSession {
    pub fn session(&self) -> &BrowserSession {
        &self.session
    }
}

impl std::ops::Deref for PooledSession {
    type Target = BrowserSession;
    fn deref(&self) -> &BrowserSession {
        &self.session
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionInner;
    use async_trait::async_trait;
    use quarry_core::ids::kinds;
    use quarry_core::lease::ProxyAffinity;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;

    struct MockDriver {
        acquires: AtomicUsize,
    }

    impl MockDriver {
        fn new() -> Self {
            Self {
                acquires: AtomicUsize::new(0),
            }
        }
        fn acquire_count(&self) -> usize {
            self.acquires.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl BrowserDriver for MockDriver {
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            self.acquires.fetch_add(1, Ordering::SeqCst);
            Ok(BrowserSession {
                lease: lease.clone(),
                inner: Arc::new(Mutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            Ok(())
        }
        async fn goto(&self, _s: &BrowserSession, _url: &str) -> QuarryResult<()> {
            Ok(())
        }
        async fn content(&self, _s: &BrowserSession) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
        async fn screenshot(
            &self,
            _s: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
        async fn pdf(&self, _s: &BrowserSession) -> QuarryResult<bytes::Bytes> {
            Ok(bytes::Bytes::new())
        }
    }

    fn make_lease(affinity: &str, ttl_s: u32) -> BrowserLease {
        BrowserLease {
            lease_id: kinds::LeaseKind::new(),
            profile_id: kinds::ProfileKind::new(),
            session_affinity_key: affinity.to_string(),
            proxy_affinity: ProxyAffinity {
                pool: "default".into(),
                sticky_key: None,
            },
            ttl_s,
            capabilities: vec![],
            artifact_bucket: "test".into(),
            persist_profile: false,
            viewport: None,
            org_id: "test_org".into(),
        }
    }

    #[tokio::test]
    async fn affinity_reuses_session() {
        let driver = Arc::new(MockDriver::new());
        let pool = LeasePool::new(driver.clone(), 4);
        let lease = make_lease("user-a", 60);

        let s1 = pool.acquire(&lease).await.unwrap();
        drop(s1);
        let s2 = pool.acquire(&lease).await.unwrap();
        drop(s2);

        assert_eq!(driver.acquire_count(), 1, "second acquire should hit cache");
        assert_eq!(pool.len().await, 1);
    }

    #[tokio::test]
    async fn distinct_affinity_keys_get_distinct_sessions() {
        let driver = Arc::new(MockDriver::new());
        let pool = LeasePool::new(driver.clone(), 4);

        let _a = pool.acquire(&make_lease("a", 60)).await.unwrap();
        let _b = pool.acquire(&make_lease("b", 60)).await.unwrap();

        assert_eq!(driver.acquire_count(), 2);
        assert_eq!(pool.len().await, 2);
    }

    #[tokio::test]
    async fn ttl_expiry_evicts_entry() {
        let driver = Arc::new(MockDriver::new());
        let pool = LeasePool::new(driver.clone(), 4);
        let lease = make_lease("ttl", 0); // expires immediately

        let _s = pool.acquire(&lease).await.unwrap();
        // Yield so wall clock advances past the zero-duration TTL.
        tokio::time::sleep(Duration::from_millis(5)).await;

        let _s2 = pool.acquire(&lease).await.unwrap();
        assert_eq!(
            driver.acquire_count(),
            2,
            "expired entry should force a new acquire"
        );
    }

    #[tokio::test]
    async fn manual_evict_drops_entry() {
        let driver = Arc::new(MockDriver::new());
        let pool = LeasePool::new(driver.clone(), 4);
        let lease = make_lease("evict-me", 60);

        let _s = pool.acquire(&lease).await.unwrap();
        assert_eq!(pool.len().await, 1);

        pool.evict("evict-me").await;
        assert_eq!(pool.len().await, 0);

        let _s2 = pool.acquire(&lease).await.unwrap();
        assert_eq!(driver.acquire_count(), 2);
    }

    #[tokio::test]
    async fn semaphore_caps_concurrency() {
        let driver = Arc::new(MockDriver::new());
        let pool = Arc::new(LeasePool::new(driver.clone(), 1));

        let g1 = pool.acquire(&make_lease("k1", 60)).await.unwrap();

        // Second acquire should block until g1 is dropped.
        let pool2 = pool.clone();
        let handle = tokio::spawn(async move {
            let _g = pool2.acquire(&make_lease("k2", 60)).await.unwrap();
        });

        // Give the spawned task a chance to attempt acquisition.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!handle.is_finished(), "second acquire must wait for permit");

        drop(g1);
        handle.await.unwrap();
    }
}
