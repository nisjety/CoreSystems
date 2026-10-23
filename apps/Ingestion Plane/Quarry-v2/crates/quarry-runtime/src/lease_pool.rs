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
    /// WS6.I: refusals because the whole pool was full.
    refused_pool_full: AtomicU64,
    /// WS6.I: refusals because the organization held its own share.
    refused_org_cap: AtomicU64,
    /// WS6.I: waits that ran out of time on the global pool.
    wait_timeouts: AtomicU64,
    /// WS6.I: waits for a permit that ended (granted or timed out), and their total time.
    waits: AtomicU64,
    wait_nanos: AtomicU64,
}

/// Point-in-time snapshot of lease pool metrics.
#[derive(Debug, Clone, Copy)]
pub struct LeaseMetricsSnapshot {
    pub acquired: u64,
    pub released: u64,
    pub in_flight: u64,
    pub peak_in_flight: u64,
    pub refused_pool_full: u64,
    pub refused_org_cap: u64,
    pub wait_timeouts: u64,
    pub waits: u64,
    pub wait_nanos: u64,
}

/// Most distinct organizations whose idle semaphore is kept. Past this the idle
/// ones are dropped on the next acquire, so a long-lived edge does not keep one
/// entry for every tenant it has ever served.
const ORG_SEMAPHORE_SOFT_LIMIT: usize = 4096;

/// The `details.reason` of a refusal caused by an organization's own share of
/// the pool (as opposed to the pool being full for everyone).
pub const ORG_RENDER_CAP_REASON: &str = "org_render_cap";

/// Environment variable for the per-organization cap.
pub const ENV_ORG_MAX_PERMITS: &str = "QUARRY_EDGE__POOL_ORG_MAX_PERMITS";

/// Read the per-organization cap through `get`. Unset or empty is `0` (no
/// cap): turning it on is a deliberate choice, because on a single-tenant
/// deployment any cap below the pool size only slows that tenant down. A value
/// that is not a non-negative integer is an error naming the variable.
pub fn org_max_permits_from_env(get: impl Fn(&str) -> Option<String>) -> Result<usize, String> {
    match get(ENV_ORG_MAX_PERMITS).map(|value| value.trim().to_owned()) {
        None => Ok(0),
        Some(value) if value.is_empty() => Ok(0),
        Some(value) => value.parse::<usize>().map_err(|_| {
            format!("{ENV_ORG_MAX_PERMITS} must be a non-negative integer, got {value:?}")
        }),
    }
}

/// Standalone runtime lease pool: pairs an affinity-keyed lease registry with
/// a semaphore that enforces a global concurrency cap, and optionally a
/// per-organization cap so one tenant cannot hold every browser.
#[derive(Clone)]
pub struct RuntimeLeasePool {
    leases: Arc<DashMap<String, BrowserLease>>,
    semaphore: Arc<Semaphore>,
    max_concurrent: usize,
    metrics: Arc<LeaseMetrics>,
    /// Per-organization concurrent permits; `0` means no per-org cap.
    org_max: usize,
    org_semaphores: Arc<DashMap<String, Arc<Semaphore>>>,
}

impl RuntimeLeasePool {
    /// Create a new pool with the given concurrency cap.
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            leases: Arc::new(DashMap::new()),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_concurrent,
            metrics: Arc::new(LeaseMetrics::default()),
            org_max: 0,
            org_semaphores: Arc::new(DashMap::new()),
        }
    }

    /// Cap how many leases one organization may hold at once. `0` removes the
    /// cap. The cap is additive to the global one: an organization is never
    /// given more than the pool has, and a request without an organization
    /// (an anonymous fetch) is only subject to the global cap.
    pub fn with_org_max_permits(mut self, org_max: usize) -> Self {
        self.org_max = org_max;
        self
    }

    /// The per-organization cap (`0` = none).
    pub fn org_max_permits(&self) -> usize {
        self.org_max
    }

    fn org_semaphore(&self, org_id: &str) -> Arc<Semaphore> {
        if self.org_semaphores.len() > ORG_SEMAPHORE_SOFT_LIMIT {
            let org_max = self.org_max;
            self.org_semaphores
                .retain(|_, semaphore| semaphore.available_permits() < org_max);
        }
        self.org_semaphores
            .entry(org_id.to_owned())
            .or_insert_with(|| Arc::new(Semaphore::new(self.org_max)))
            .clone()
    }

    fn org_cap_error(&self, org_id: &str) -> QuarryError {
        self.metrics.refused_org_cap.fetch_add(1, Ordering::Relaxed);
        let active = self.org_semaphores.get(org_id).map_or(0, |semaphore| {
            self.org_max - semaphore.available_permits().min(self.org_max)
        });
        QuarryError::new(
            ErrorCode::RateLimited,
            "this organization already holds its share of the browser pool",
        )
        .with_details(serde_json::json!({
            "reason": ORG_RENDER_CAP_REASON,
            "scope": "org",
            "limit": self.org_max,
            "active": active,
            "retry_after_seconds": 5,
        }))
    }

    /// Snapshot of pool acquire/release counters.
    pub fn metrics_snapshot(&self) -> LeaseMetricsSnapshot {
        LeaseMetricsSnapshot {
            acquired: self.metrics.acquired.load(Ordering::Relaxed),
            released: self.metrics.released.load(Ordering::Relaxed),
            in_flight: self.metrics.in_flight.load(Ordering::Relaxed),
            peak_in_flight: self.metrics.peak_in_flight.load(Ordering::Relaxed),
            refused_pool_full: self.metrics.refused_pool_full.load(Ordering::Relaxed),
            refused_org_cap: self.metrics.refused_org_cap.load(Ordering::Relaxed),
            wait_timeouts: self.metrics.wait_timeouts.load(Ordering::Relaxed),
            waits: self.metrics.waits.load(Ordering::Relaxed),
            wait_nanos: self.metrics.wait_nanos.load(Ordering::Relaxed),
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
                    self.metrics.refused_pool_full.fetch_add(1, Ordering::Relaxed);
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

    /// Like [`try_acquire`](Self::try_acquire), but also refuses at once when
    /// `org_id` already holds its per-organization cap
    /// (`details.reason = "org_render_cap"`).
    pub fn try_acquire_for_org(
        &self,
        org_id: &str,
        affinity_key: &str,
    ) -> QuarryResult<LeaseGuard> {
        if self.org_max == 0 || org_id.is_empty() {
            return self.try_acquire(affinity_key);
        }
        let org_permit = self
            .org_semaphore(org_id)
            .try_acquire_owned()
            .map_err(|_| self.org_cap_error(org_id))?;
        let mut guard = self.try_acquire(affinity_key)?;
        guard.org_permit = Some(org_permit);
        Ok(guard)
    }

    /// Like [`acquire_timeout`](Self::acquire_timeout), but first takes one of
    /// `org_id`'s per-organization permits, so a tenant at its cap waits for
    /// its own releases instead of for everyone's. If the wait for that permit
    /// runs out the refusal is `RATE_LIMITED` with
    /// `details.reason = "org_render_cap"`, which tells the caller it was its
    /// own share that was full; running out of time on the global pool stays a
    /// plain timeout.
    pub async fn acquire_timeout_for_org(
        &self,
        org_id: &str,
        affinity_key: &str,
        deadline: Duration,
    ) -> QuarryResult<LeaseGuard> {
        if self.org_max == 0 || org_id.is_empty() {
            return self.acquire_timeout(affinity_key, deadline).await;
        }
        let started = tokio::time::Instant::now();
        let org_semaphore = self.org_semaphore(org_id);
        let org_permit = match timeout(deadline, org_semaphore.acquire_owned()).await {
            Ok(Ok(permit)) => permit,
            Ok(Err(_)) => {
                return Err(QuarryError::new(
                    ErrorCode::Internal,
                    "lease pool semaphore closed",
                ))
            }
            Err(_) => return Err(self.org_cap_error(org_id)),
        };
        let remaining = deadline.saturating_sub(started.elapsed());
        let mut guard = self.acquire_timeout(affinity_key, remaining).await?;
        guard.org_permit = Some(org_permit);
        Ok(guard)
    }

    /// Acquire a permit and lease, waiting up to `deadline` if necessary.
    pub async fn acquire_timeout(
        &self,
        affinity_key: &str,
        deadline: Duration,
    ) -> QuarryResult<LeaseGuard> {
        let sem = self.semaphore.clone();
        let wait_started = std::time::Instant::now();
        let waited = timeout(deadline, sem.acquire_owned()).await;
        self.metrics.waits.fetch_add(1, Ordering::Relaxed);
        self.metrics.wait_nanos.fetch_add(
            u64::try_from(wait_started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        let permit = match waited {
            Ok(Ok(p)) => p,
            Ok(Err(_)) => {
                return Err(QuarryError::new(
                    ErrorCode::Internal,
                    "lease pool semaphore closed",
                ))
            }
            Err(_) => {
                self.metrics.wait_timeouts.fetch_add(1, Ordering::Relaxed);
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
    /// The organization's share, released with the guard.
    org_permit: Option<OwnedSemaphorePermit>,
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
            org_permit: None,
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
    async fn org_cap_limits_one_org_without_blocking_another() {
        let pool = RuntimeLeasePool::new(4).with_org_max_permits(1);
        let _a1 = pool
            .try_acquire_for_org("org_a", "h1")
            .expect("org a first");
        let refused = pool
            .try_acquire_for_org("org_a", "h2")
            .expect_err("org a is at its cap");
        assert_eq!(refused.code, ErrorCode::RateLimited);
        let details = refused.details.expect("typed details");
        assert_eq!(details["reason"], "org_render_cap");
        assert_eq!(details["scope"], "org");
        assert_eq!(details["limit"], 1);
        assert_eq!(details["active"], 1);
        // Another tenant is unaffected while org a sits at its cap.
        let _b1 = pool
            .try_acquire_for_org("org_b", "h3")
            .expect("org b is independent");
        assert_eq!(pool.available(), 2);
    }

    #[test]
    fn the_org_cap_is_off_unless_configured_and_rejects_garbage() {
        let none = |_: &str| None;
        assert_eq!(org_max_permits_from_env(none), Ok(0));
        let get = |value: &'static str| {
            move |name: &str| (name == ENV_ORG_MAX_PERMITS).then(|| value.to_owned())
        };
        assert_eq!(org_max_permits_from_env(get("")), Ok(0));
        assert_eq!(org_max_permits_from_env(get(" 2 ")), Ok(2));
        assert_eq!(org_max_permits_from_env(get("0")), Ok(0));
        for bad in ["-1", "two", "1.5"] {
            assert!(
                org_max_permits_from_env(get(bad))
                    .unwrap_err()
                    .contains(ENV_ORG_MAX_PERMITS),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn org_permit_is_released_on_guard_drop() {
        let pool = RuntimeLeasePool::new(2).with_org_max_permits(1);
        let guard = pool.try_acquire_for_org("org_a", "h1").expect("first");
        assert!(pool.try_acquire_for_org("org_a", "h2").is_err());
        drop(guard);
        pool.try_acquire_for_org("org_a", "h2")
            .expect("the share came back");
    }

    #[tokio::test]
    async fn org_cap_zero_means_unlimited_and_anonymous_fetches_are_only_globally_capped() {
        let pool = RuntimeLeasePool::new(3);
        let _g1 = pool.try_acquire_for_org("org_a", "h1").unwrap();
        let _g2 = pool.try_acquire_for_org("org_a", "h2").unwrap();
        let _g3 = pool.try_acquire_for_org("org_a", "h3").unwrap();
        assert_eq!(
            pool.try_acquire_for_org("org_a", "h4").unwrap_err().code,
            ErrorCode::RateLimited,
            "the global cap still applies"
        );

        let capped = RuntimeLeasePool::new(2).with_org_max_permits(1);
        let _a = capped.try_acquire_for_org("", "h1").unwrap();
        let _b = capped.try_acquire_for_org("", "h2").unwrap();
    }

    #[tokio::test]
    async fn an_org_at_its_cap_waits_for_its_own_release_and_reports_the_org_reason_on_timeout() {
        let pool = RuntimeLeasePool::new(4).with_org_max_permits(1);
        let held = pool
            .acquire_timeout_for_org("org_a", "h1", Duration::from_millis(50))
            .await
            .unwrap();

        // Out of time while its own share is full: the org reason, not the pool's timeout.
        let error = pool
            .acquire_timeout_for_org("org_a", "h2", Duration::from_millis(30))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RateLimited);
        assert_eq!(error.details.unwrap()["reason"], "org_render_cap");

        // It waits for its own release when that arrives in time.
        let waiter = {
            let pool = pool.clone();
            tokio::spawn(async move {
                pool.acquire_timeout_for_org("org_a", "h3", Duration::from_secs(5))
                    .await
            })
        };
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(held);
        assert!(waiter.await.unwrap().is_ok());
    }

    #[tokio::test]
    async fn the_global_pool_running_out_is_still_a_plain_timeout_for_an_org() {
        let pool = RuntimeLeasePool::new(1).with_org_max_permits(1);
        let _other = pool.try_acquire("anyone").unwrap();
        let error = pool
            .acquire_timeout_for_org("org_a", "h1", Duration::from_millis(30))
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        // The org's own permit was given back when the global wait failed.
        drop(_other);
        pool.try_acquire_for_org("org_a", "h1")
            .expect("the org share was not leaked");
    }

    #[tokio::test]
    async fn idle_org_semaphores_are_pruned_past_the_soft_limit() {
        let pool = RuntimeLeasePool::new(1).with_org_max_permits(1);
        for index in 0..(ORG_SEMAPHORE_SOFT_LIMIT + 10) {
            drop(
                pool.try_acquire_for_org(&format!("org_{index}"), "h")
                    .unwrap(),
            );
        }
        pool.try_acquire_for_org("org_new", "h").unwrap();
        assert!(
            pool.org_semaphores.len() < 100,
            "{}",
            pool.org_semaphores.len()
        );
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

    #[tokio::test]
    async fn refusals_and_waits_are_counted() {
        let pool = RuntimeLeasePool::new(1);
        let held = pool.try_acquire("a").expect("acquire");
        assert!(pool.try_acquire("b").is_err());
        assert!(pool
            .acquire_timeout("c", Duration::from_millis(20))
            .await
            .is_err());
        let snap = pool.metrics_snapshot();
        assert_eq!(snap.refused_pool_full, 1);
        assert_eq!(snap.wait_timeouts, 1);
        assert_eq!(snap.waits, 1);
        assert!(snap.wait_nanos > 0);
        drop(held);
    }
}
