use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};
use tokio::time::timeout;
use tracing::{debug, warn};

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::BrowserLease;

/// Internal atomic counters for lease pool activity.
#[derive(Default)]
struct LeaseMetrics {
    acquired: AtomicU64,
    released: AtomicU64,
    in_flight: AtomicU64,
    peak_in_flight: AtomicU64,
}

/// Point-in-time snapshot of lease pool metrics.
#[derive(Debug, Clone, Copy)]
pub struct LeaseMetricsSnapshot {
    pub acquired: u64,
    pub released: u64,
    pub in_flight: u64,
    pub peak_in_flight: u64,
}

/// Standalone runtime lease pool: pairs an affinity-keyed lease registry with
/// a semaphore that enforces a global concurrency cap.
#[derive(Clone)]
pub struct RuntimeLeasePool {
    leases: Arc<DashMap<String, BrowserLease>>,
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
    metrics: Arc<LeaseMetrics>,
}

impl RuntimeLeasePool {
    /// Create a new pool with the given concurrency cap.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            leases: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
            metrics: Arc::new(LeaseMetrics::default()),
        }
    }

    /// Snapshot of pool acquire/release counters.
    pub fn metrics_snapshot(&self) -> LeaseMetricsSnapshot {
        LeaseMetricsSnapshot {
            acquired: self.metrics.acquired.load(Ordering::Relaxed),
            released: self.metrics.released.load(Ordering::Relaxed),
            in_flight: self.metrics.in_flight.load(Ordering::Relaxed),
            peak_in_flight: self.metrics.peak_in_flight.load(Ordering::Relaxed),
        }
    }

    fn record_acquire(&self) {
        self.metrics.acquired.fetch_add(1, Ordering::Relaxed);
        let current = self.metrics.in_flight.fetch_add(1, Ordering::Relaxed) + 1;
        let mut peak = self.metrics.peak_in_flight.load(Ordering::Relaxed);
        while current > peak {
            match self.metrics.peak_in_flight.compare_exchange_weak(
                peak,
                current,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => peak = observed,
            }
        }
    }

    /// Maximum concurrent leases this pool will hand out.
    pub fn max_concurrent(&self) -> usize {
        self.max_concurrent
    }

    /// Currently available permits.
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Seed the registry with a lease for the given affinity key.
    pub fn seed(&self, affinity_key: impl Into<String>, lease: BrowserLease) {
        let key = affinity_key.into();
        self.leases.insert(key, lease);
    }

    /// Try to acquire a permit and lease without blocking.
    pub fn try_acquire(&self, affinity_key: &str) -> QuarryResult<LeaseGuard> {
        let permit = self
            .semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|e| match e {
                TryAcquireError::NoPermits => {
                    QuarryError::new(ErrorCode::RateLimited, "lease pool capacity exhausted")
                }
                TryAcquireError::Closed => {
                    QuarryError::new(ErrorCode::Internal, "lease pool semaphore closed")
                }
            })?;

        let lease = self.leases.remove(affinity_key).map(|(_, lease)| lease);

        self.record_acquire();

        Ok(LeaseGuard::new(
            lease,
            affinity_key.to_string(),
            self.leases.clone(),
            permit,
            self.metrics.clone(),
        ))
    }

    /// Acquire a permit and lease, waiting up to `deadline` if necessary.
    pub async fn acquire_timeout(
        &self,
        affinity_key: &str,
        deadline: Duration,
    ) -> QuarryResult<LeaseGuard> {
        let sem = self.semaphore.clone();
        let permit = match timeout(deadline, sem.acquire_owned()).await {
            Ok(Ok(p)) => p,
            Ok(Err(_)) => {
                return Err(QuarryError::new(
                    ErrorCode::Internal,
                    "lease pool semaphore closed",
                ))
            }
            Err(_) => {
                return Err(QuarryError::new(
                    ErrorCode::Timeout,
                    "timed out waiting for lease pool capacity",
                ))
            }
        };

        let lease = self.leases.remove(affinity_key).map(|(_, lease)| lease);

        self.record_acquire();

        Ok(LeaseGuard::new(
            lease,
            affinity_key.to_string(),
            self.leases.clone(),
            permit,
            self.metrics.clone(),
        ))
    }
}

/// RAII guard returned from the pool. On drop, returns the lease to the
/// registry under its affinity key (or evicts it if poisoned).
pub struct LeaseGuard {
    pub lease: Option<BrowserLease>,
    affinity_key: String,
    pool: Arc<DashMap<String, BrowserLease>>,
    _permit: OwnedSemaphorePermit,
    metrics: Arc<LeaseMetrics>,
    poisoned: bool,
}

impl LeaseGuard {
    fn new(
        lease: Option<BrowserLease>,
        affinity_key: String,
        pool: Arc<DashMap<String, BrowserLease>>,
        permit: OwnedSemaphorePermit,
        metrics: Arc<LeaseMetrics>,
    ) -> Self {
        Self {
            lease,
            affinity_key,
            pool,
            _permit: permit,
            metrics,
            poisoned: false,
        }
    }

    /// Mark this guard as poisoned so the lease is evicted on drop instead of
    /// returned to the pool.
    pub fn poison(&mut self) {
        self.poisoned = true;
    }

    /// Whether a lease is currently attached to this guard.
    pub fn has_lease(&self) -> bool {
        self.lease.is_some()
    }

    /// Take the lease out of the guard, leaving the affinity key but no lease
    /// to return on drop.
    pub fn take_lease(&mut self) -> Option<BrowserLease> {
        self.lease.take()
    }
}

impl std::fmt::Debug for LeaseGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeaseGuard")
            .field("affinity_key", &self.affinity_key)
            .field("has_lease", &self.lease.is_some())
            .field("poisoned", &self.poisoned)
            .finish()
    }
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        self.metrics.in_flight.fetch_sub(1, Ordering::Relaxed);
        self.metrics.released.fetch_add(1, Ordering::Relaxed);

        if self.poisoned {
            self.pool.remove(&self.affinity_key);
            warn!(affinity_key = %self.affinity_key, "lease guard poisoned; evicted");
            return;
        }

        if let Some(lease) = self.lease.take() {
            self.pool.insert(self.affinity_key.clone(), lease);
            debug!(affinity_key = %self.affinity_key, "lease returned to pool");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn try_acquire_respects_capacity() {
        let pool = RuntimeLeasePool::new(2);
        let _g1 = pool.try_acquire("a").expect("first acquire ok");
        let _g2 = pool.try_acquire("b").expect("second acquire ok");
        let err = pool.try_acquire("c").expect_err("third should fail");
        assert_eq!(err.code, ErrorCode::RateLimited);
    }

    #[tokio::test]
    async fn permit_released_on_guard_drop() {
        let pool = RuntimeLeasePool::new(1);
        {
            let _g = pool.try_acquire("a").expect("acquire ok");
        }
        // Give the spawned pool.put task a chance to complete
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(pool.available(), 1);
    }

    #[tokio::test]
    async fn acquire_timeout_returns_error_when_pool_full() {
        let pool = RuntimeLeasePool::new(1);
        let _g = pool.try_acquire("a").expect("acquire ok");
        let err = pool
            .acquire_timeout("b", Duration::from_millis(20))
            .await
            .expect_err("should time out");
        assert_eq!(err.code, ErrorCode::Timeout);
    }

    #[tokio::test]
    async fn metrics_track_acquire_release() {
        let pool = RuntimeLeasePool::new(2);
        let snap0 = pool.metrics_snapshot();
        assert_eq!(snap0.acquired, 0);
        assert_eq!(snap0.in_flight, 0);
        assert_eq!(snap0.peak_in_flight, 0);

        let g1 = pool.try_acquire("a").expect("acquire 1");
        let g2 = pool.try_acquire("b").expect("acquire 2");
        let snap1 = pool.metrics_snapshot();
        assert_eq!(snap1.acquired, 2);
        assert_eq!(snap1.in_flight, 2);
        assert_eq!(snap1.peak_in_flight, 2);
        assert_eq!(snap1.released, 0);

        drop(g1);
        drop(g2);
        let snap2 = pool.metrics_snapshot();
        assert_eq!(snap2.acquired, 2);
        assert_eq!(snap2.released, 2);
        assert_eq!(snap2.in_flight, 0);
        assert_eq!(snap2.peak_in_flight, 2);
    }
}
