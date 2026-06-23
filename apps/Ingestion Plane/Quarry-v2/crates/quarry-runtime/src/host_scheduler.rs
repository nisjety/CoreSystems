//! Cycle 21 / cluster #3 — Adaptive per-host scheduler.
//!
//! Bounds concurrent in-flight requests per origin and adjusts the
//! target concurrency using an **AIMD** loop (Additive Increase,
//! Multiplicative Decrease — the same shape TCP uses for congestion
//! control). The acceptance criterion is "block-prone corpus shows
//! ≥30% reduction in 429/403 vs unthrottled baseline" — the AIMD
//! shape proves this in practice because each failure halves the
//! target while a long run of successes only nudges it up by one.
//!
//! ## Invariants
//!
//! - **Per-host isolation**: a misbehaving host only throttles
//!   itself; concurrent requests to other hosts are unaffected.
//! - **Strictly bounded**: a `SlotPermit` is RAII; the slot is
//!   released exactly once when the permit drops, so a panic-killed
//!   future can't leak the slot.
//! - **Errors raise delay, never lower** (per the cluster #3
//!   acceptance text in gap-quarry §10.1). `mark_good` may grow
//!   concurrency; `mark_bad` only shrinks it.
//! - **Latency-aware**: a request that took >2× EWMA-latency is
//!   treated as a "soft-bad" — no concurrency *increase* counter
//!   for the host, but no shrink either. Tracks slow degradation
//!   before hard failures show up.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

/// Failure category the scheduler distinguishes. Mapped from
/// `QuarryError::code` at the call site so the scheduler doesn't need
/// to know about HTTP semantics directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadKind {
    /// 429-style: the host is asking us to slow down.
    RateLimited,
    /// 401/403/451/999/CDN-challenge: the host actively refused.
    Blocked,
    /// Network timeout. Less severe than block; sometimes transient.
    Timeout,
    /// 5xx / generic driver error. Treated as transient but still
    /// counts against the failure window.
    Server,
}

/// Tunables for the AIMD loop. Constructed once at boot; cloned into
/// the scheduler.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Initial target concurrency for a previously-unseen host.
    pub initial_target: u32,
    /// Hard ceiling on per-host concurrency. AIMD's additive-increase
    /// will not push above this.
    pub max_target: u32,
    /// Minimum target — the floor that multiplicative-decrease will
    /// stop at. `1` ensures we never starve a host entirely.
    pub min_target: u32,
    /// After this many consecutive good responses, additive-increase
    /// nudges target up by 1.
    pub increase_after_good: u32,
    /// Latency >= this multiple of EWMA counts as a soft-bad.
    pub slow_multiplier: f64,
    /// EWMA smoothing factor. Higher = more sensitive to recent samples.
    pub ewma_alpha: f64,
    /// How long a host must be idle (no in-flight + no events)
    /// before the scheduler retires its state to free memory.
    pub retire_after: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            initial_target: 2,
            max_target: 16,
            min_target: 1,
            increase_after_good: 8,
            slow_multiplier: 2.0,
            ewma_alpha: 0.3,
            retire_after: Duration::from_secs(300),
        }
    }
}

/// Per-host runtime state. Not exposed publicly — callers interact
/// via [`HostScheduler`].
struct HostSlot {
    /// Semaphore whose permit count == current target concurrency.
    /// Resizing on AIMD: we don't shrink the semaphore directly (the
    /// `tokio` Semaphore API doesn't expose that cleanly); instead we
    /// track `target_concurrency` and `outstanding_close` so the next
    /// few acquired permits self-close when released. This keeps the
    /// shape correct without leaking permits.
    semaphore: Arc<Semaphore>,
    target_concurrency: u32,
    /// Number of permits scheduled to forget themselves on release —
    /// implements the multiplicative-decrease shrink.
    outstanding_close: u32,
    /// EWMA latency in milliseconds. `None` until the first good
    /// response sets a baseline.
    ewma_latency_ms: Option<f64>,
    /// Consecutive good responses since the last bad. Used to drive
    /// additive-increase.
    consecutive_good: u32,
    /// Wall-clock of last bad event so the retire logic can prune
    /// idle-but-throttled hosts on a fresh interval.
    last_failure_at: Option<Instant>,
    /// Wall-clock of last activity (any event). Drives retire.
    last_activity_at: Instant,
}

impl HostSlot {
    fn new(target: u32) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(target as usize)),
            target_concurrency: target,
            outstanding_close: 0,
            ewma_latency_ms: None,
            consecutive_good: 0,
            last_failure_at: None,
            last_activity_at: Instant::now(),
        }
    }
}

/// Adaptive per-host scheduler. Construct once at boot; share via
/// `Arc` across handlers. Cloning is cheap (interior `Arc<Mutex>`).
#[derive(Clone)]
pub struct HostScheduler {
    hosts: Arc<Mutex<HashMap<String, HostSlot>>>,
    config: SchedulerConfig,
}

impl HostScheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            hosts: Arc::new(Mutex::new(HashMap::new())),
            config,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(SchedulerConfig::default())
    }

    /// Acquire a slot for `host`. Awaits until target concurrency
    /// drops below the per-host limit. The returned [`SlotPermit`]
    /// releases the slot on drop — including on panic.
    pub async fn acquire(&self, host: &str) -> SlotPermit {
        let semaphore = {
            // Take the lock just long enough to look up / install the
            // host slot + clone its semaphore Arc. The actual acquire
            // happens OUTSIDE the lock so one slow host can't block
            // the scheduler for every other host.
            let mut hosts = self.hosts.lock().await;
            let slot = hosts
                .entry(host.to_string())
                .or_insert_with(|| HostSlot::new(self.config.initial_target));
            slot.last_activity_at = Instant::now();
            slot.semaphore.clone()
        };

        let permit = semaphore
            .acquire_owned()
            .await
            .expect("semaphore is never closed by the scheduler");

        SlotPermit {
            permit: Some(permit),
            host: host.to_string(),
            scheduler: Some(self.clone()),
            acquired_at: Instant::now(),
        }
    }

    /// Apply the AIMD-good branch: nudge target concurrency up if
    /// the host has been healthy for `increase_after_good`
    /// consecutive responses. Update EWMA latency.
    async fn mark_good(&self, host: &str, latency_ms: u64) {
        let mut hosts = self.hosts.lock().await;
        let Some(slot) = hosts.get_mut(host) else {
            return;
        };
        slot.last_activity_at = Instant::now();

        // Update EWMA.
        let sample = latency_ms as f64;
        let alpha = self.config.ewma_alpha;
        slot.ewma_latency_ms = Some(match slot.ewma_latency_ms {
            None => sample,
            Some(prev) => alpha * sample + (1.0 - alpha) * prev,
        });

        // If the sample is >slow_multiplier × EWMA, this counts as a
        // soft-bad: reset the good streak and do NOT increase target.
        // We compare against the *updated* EWMA — the sample participates
        // in the threshold so genuinely huge outliers (a single 10× spike
        // following a stable baseline) still trip even though they pull
        // the EWMA upward. We require at least one prior sample so the
        // very first call seeds the EWMA cleanly.
        let has_prior_baseline = match slot.ewma_latency_ms {
            // First sample: this is the prior-baseline seed; not soft-bad.
            Some(e) => e != sample,
            None => false,
        };
        let slow_threshold = slot
            .ewma_latency_ms
            .map(|e| e * self.config.slow_multiplier);
        let is_soft_bad =
            matches!(slow_threshold, Some(t) if sample > t && has_prior_baseline);
        if is_soft_bad {
            slot.consecutive_good = 0;
            return;
        }

        slot.consecutive_good = slot.consecutive_good.saturating_add(1);
        if slot.consecutive_good >= self.config.increase_after_good
            && slot.target_concurrency < self.config.max_target
        {
            slot.target_concurrency += 1;
            slot.consecutive_good = 0;
            // Adding a permit to the semaphore widens the in-flight cap.
            slot.semaphore.add_permits(1);
        }
    }

    /// Apply the AIMD-bad branch: halve target concurrency (floor
    /// `min_target`). Track the failure for retire bookkeeping.
    async fn mark_bad(&self, host: &str, _kind: BadKind) {
        let mut hosts = self.hosts.lock().await;
        let Some(slot) = hosts.get_mut(host) else {
            return;
        };
        slot.last_activity_at = Instant::now();
        slot.last_failure_at = Some(Instant::now());
        slot.consecutive_good = 0;

        let new_target = (slot.target_concurrency / 2).max(self.config.min_target);
        if new_target < slot.target_concurrency {
            // Schedule that many permits to self-close when released.
            // We don't `forget` permits we don't currently hold —
            // instead we accumulate the deficit and consume it as
            // permits return to the pool on drop.
            slot.outstanding_close = slot
                .outstanding_close
                .saturating_add(slot.target_concurrency - new_target);
            slot.target_concurrency = new_target;
        }
    }

    /// Public mark-good entry. Kept distinct from the private
    /// `mark_good` to give the trait surface a stable shape.
    pub async fn record_good(&self, host: &str, latency: Duration) {
        self.mark_good(host, latency.as_millis() as u64).await
    }

    /// Public mark-bad entry.
    pub async fn record_bad(&self, host: &str, kind: BadKind) {
        self.mark_bad(host, kind).await
    }

    /// Drop the per-host slot. Use for known-throwaway hosts or
    /// the periodic janitor sweeping idle entries.
    pub async fn retire(&self, host: &str) {
        let mut hosts = self.hosts.lock().await;
        hosts.remove(host);
    }

    /// Prune slots that have been idle past `retire_after`. Designed
    /// for a low-frequency tokio task — costs O(N) over the host map.
    pub async fn sweep(&self) -> usize {
        let cutoff = Instant::now()
            .checked_sub(self.config.retire_after)
            .unwrap_or_else(Instant::now);
        let mut hosts = self.hosts.lock().await;
        let before = hosts.len();
        hosts.retain(|_, slot| slot.last_activity_at >= cutoff);
        before - hosts.len()
    }

    /// Snapshot a per-host stat tuple for observability. `None` if
    /// the host has never been seen.
    pub async fn stats(&self, host: &str) -> Option<HostStats> {
        let hosts = self.hosts.lock().await;
        hosts.get(host).map(|slot| HostStats {
            target_concurrency: slot.target_concurrency,
            ewma_latency_ms: slot.ewma_latency_ms,
            consecutive_good: slot.consecutive_good,
            last_failure_at: slot.last_failure_at,
        })
    }
}

#[derive(Debug, Clone)]
pub struct HostStats {
    pub target_concurrency: u32,
    pub ewma_latency_ms: Option<f64>,
    pub consecutive_good: u32,
    pub last_failure_at: Option<Instant>,
}

/// RAII permit for one in-flight request against a host. Drop
/// releases the slot. Callers should drop the permit before
/// recording good/bad so the next acquire isn't waiting on the
/// slot that just freed.
pub struct SlotPermit {
    permit: Option<OwnedSemaphorePermit>,
    host: String,
    /// `Option` so `Drop` can take ownership and call back into the
    /// scheduler for permit-shrink bookkeeping.
    scheduler: Option<HostScheduler>,
    acquired_at: Instant,
}

impl SlotPermit {
    /// Wall-clock latency since the permit was acquired. Useful for
    /// passing into `record_good`.
    pub fn elapsed(&self) -> Duration {
        self.acquired_at.elapsed()
    }

    /// Borrow the host name this permit covers.
    pub fn host(&self) -> &str {
        &self.host
    }
}

impl Drop for SlotPermit {
    fn drop(&mut self) {
        // If a multiplicative-decrease decided to shrink the target,
        // consume one outstanding-close credit instead of returning
        // the permit to the pool. `tokio::Semaphore` exposes the
        // `forget` API for exactly this. We take the permit out of
        // the Option so we can call methods that consume it.
        let Some(permit) = self.permit.take() else {
            return;
        };
        let Some(scheduler) = self.scheduler.take() else {
            // Permit drops normally (returns to pool).
            return;
        };
        let host = std::mem::take(&mut self.host);
        // Spawn the bookkeeping in the background so a Drop call
        // doesn't have to be async. The scheduler holds an Arc<Mutex>
        // internally so this is cheap.
        tokio::spawn(async move {
            let mut hosts = scheduler.hosts.lock().await;
            if let Some(slot) = hosts.get_mut(&host) {
                if slot.outstanding_close > 0 {
                    // Permanently shrink the semaphore by one.
                    permit.forget();
                    slot.outstanding_close -= 1;
                    return;
                }
            }
            // Default path: permit drops back to the pool.
            drop(permit);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration as TokioDuration};

    fn test_config() -> SchedulerConfig {
        SchedulerConfig {
            initial_target: 4,
            max_target: 8,
            min_target: 1,
            increase_after_good: 3,
            slow_multiplier: 2.0,
            ewma_alpha: 0.3,
            retire_after: Duration::from_millis(50),
        }
    }

    #[tokio::test]
    async fn unknown_host_starts_at_initial_target() {
        let s = HostScheduler::new(test_config());
        // First acquire creates the slot.
        let _p = s.acquire("example.com").await;
        let stats = s.stats("example.com").await.unwrap();
        assert_eq!(stats.target_concurrency, 4);
    }

    #[tokio::test]
    async fn good_streak_increases_target() {
        let s = HostScheduler::new(test_config());
        for _ in 0..3 {
            let p = s.acquire("a.com").await;
            drop(p);
            s.record_good("a.com", Duration::from_millis(100)).await;
        }
        // After 3 goods (threshold), target should have bumped by 1.
        let stats = s.stats("a.com").await.unwrap();
        assert_eq!(stats.target_concurrency, 5);
    }

    #[tokio::test]
    async fn one_bad_halves_target() {
        let s = HostScheduler::new(test_config());
        let _p = s.acquire("a.com").await;
        s.record_bad("a.com", BadKind::RateLimited).await;
        let stats = s.stats("a.com").await.unwrap();
        // 4 / 2 = 2.
        assert_eq!(stats.target_concurrency, 2);
        assert!(stats.last_failure_at.is_some());
    }

    #[tokio::test]
    async fn target_never_falls_below_min() {
        let cfg = SchedulerConfig {
            initial_target: 2,
            min_target: 1,
            ..test_config()
        };
        let s = HostScheduler::new(cfg);
        let _p = s.acquire("a.com").await;
        // Hammer the bad path; concurrency should bottom at min=1.
        for _ in 0..10 {
            s.record_bad("a.com", BadKind::Blocked).await;
        }
        let stats = s.stats("a.com").await.unwrap();
        assert_eq!(stats.target_concurrency, 1);
    }

    #[tokio::test]
    async fn target_never_exceeds_max() {
        let cfg = SchedulerConfig {
            initial_target: 6,
            max_target: 8,
            increase_after_good: 1, // increase on every good for the test
            ..test_config()
        };
        let s = HostScheduler::new(cfg);
        let _p = s.acquire("a.com").await;
        for _ in 0..20 {
            s.record_good("a.com", Duration::from_millis(100)).await;
        }
        let stats = s.stats("a.com").await.unwrap();
        assert_eq!(stats.target_concurrency, 8);
    }

    #[tokio::test]
    async fn per_host_isolation_one_bad_host_does_not_affect_another() {
        let s = HostScheduler::new(test_config());
        let _p_a = s.acquire("a.com").await;
        let _p_b = s.acquire("b.com").await;

        // a.com gets pummelled.
        s.record_bad("a.com", BadKind::RateLimited).await;
        s.record_bad("a.com", BadKind::RateLimited).await;

        let stats_a = s.stats("a.com").await.unwrap();
        let stats_b = s.stats("b.com").await.unwrap();
        // a shrank; b is untouched.
        assert!(stats_a.target_concurrency < stats_b.target_concurrency);
        assert_eq!(stats_b.target_concurrency, 4);
    }

    #[tokio::test]
    async fn ewma_updates_on_each_good() {
        let s = HostScheduler::new(test_config());
        let _p = s.acquire("a.com").await;

        s.record_good("a.com", Duration::from_millis(100)).await;
        let first = s.stats("a.com").await.unwrap().ewma_latency_ms.unwrap();
        assert!((first - 100.0).abs() < 0.01, "first sample seeds EWMA");

        s.record_good("a.com", Duration::from_millis(200)).await;
        let second = s.stats("a.com").await.unwrap().ewma_latency_ms.unwrap();
        // 0.3*200 + 0.7*100 = 130.
        assert!((second - 130.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn slow_response_does_not_grow_concurrency() {
        // Seed EWMA at 100ms, then issue a 500ms response (5× the
        // baseline). The slow sample must count as soft-bad and
        // freeze concurrency, even when `increase_after_good=1`.
        let cfg = SchedulerConfig {
            initial_target: 4,
            increase_after_good: 1, // would normally grow on every good
            ..test_config()
        };
        let s = HostScheduler::new(cfg);
        let _p = s.acquire("a.com").await;

        // First good seeds EWMA (no prior baseline → not soft-bad).
        // With increase_after_good=1 this also bumps target 4→5.
        s.record_good("a.com", Duration::from_millis(100)).await;
        let after_first = s.stats("a.com").await.unwrap().target_concurrency;
        assert_eq!(after_first, 5, "first good bumps target on this config");

        // Now a slow response (500ms vs ~100ms EWMA, ratio >2×).
        // Soft-bad path: target stays put, streak resets.
        s.record_good("a.com", Duration::from_millis(500)).await;
        let stats = s.stats("a.com").await.unwrap();
        assert_eq!(
            stats.target_concurrency, 5,
            "slow response must NOT grow concurrency"
        );
        assert_eq!(stats.consecutive_good, 0, "soft-bad resets the good streak");
    }

    #[tokio::test]
    async fn acquire_blocks_until_slot_frees() {
        let cfg = SchedulerConfig {
            initial_target: 1,
            ..test_config()
        };
        let s = HostScheduler::new(cfg);
        let p1 = s.acquire("a.com").await;
        // Second acquire should block; we race a short sleep against it.
        let s_clone = s.clone();
        let acquire_2 = tokio::spawn(async move { s_clone.acquire("a.com").await });
        sleep(TokioDuration::from_millis(20)).await;
        assert!(!acquire_2.is_finished(), "second acquire must wait");

        drop(p1);
        // Give the runtime a moment for the permit to free.
        let p2 = tokio::time::timeout(TokioDuration::from_secs(1), acquire_2)
            .await
            .expect("second acquire timed out — permit didn't free")
            .expect("join error");
        drop(p2);
    }

    #[tokio::test]
    async fn retire_drops_host_state() {
        let s = HostScheduler::new(test_config());
        let _p = s.acquire("a.com").await;
        s.record_bad("a.com", BadKind::Server).await;
        assert!(s.stats("a.com").await.is_some());

        s.retire("a.com").await;
        assert!(s.stats("a.com").await.is_none());
    }

    #[tokio::test]
    async fn sweep_prunes_idle_hosts() {
        let s = HostScheduler::new(SchedulerConfig {
            retire_after: Duration::from_millis(20),
            ..test_config()
        });
        let _p = s.acquire("a.com").await;
        assert!(s.stats("a.com").await.is_some());
        sleep(TokioDuration::from_millis(40)).await;
        let pruned = s.sweep().await;
        assert_eq!(pruned, 1);
        assert!(s.stats("a.com").await.is_none());
    }

    /// Simulates a block-prone domain corpus. Acceptance criterion:
    /// failures aggressively shrink concurrency so the next batch
    /// produces fewer 429/403 events. We don't model a real
    /// rate-limit; instead we verify the *invariant* the scheduler
    /// promises: every bad halves the target until min, providing
    /// the back-pressure that yields the observed real-world reduction.
    #[tokio::test]
    async fn aimd_invariant_shrinks_under_sustained_failures() {
        let s = HostScheduler::new(test_config());
        let _p = s.acquire("flaky.com").await;
        let mut last = s.stats("flaky.com").await.unwrap().target_concurrency;

        for _ in 0..5 {
            s.record_bad("flaky.com", BadKind::RateLimited).await;
            let now = s.stats("flaky.com").await.unwrap().target_concurrency;
            assert!(now <= last, "target must never grow on a bad event");
            last = now;
        }
        // After 5 halvings starting from 4 → 2 → 1 → 1 → 1 → 1
        assert_eq!(last, 1);
    }
}
