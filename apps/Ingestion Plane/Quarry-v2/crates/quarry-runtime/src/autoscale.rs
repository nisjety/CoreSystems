//! Global concurrency governor (OSS-parity P1 2C) — Crawlee `AutoscaledPool`.
//!
//! Layers ABOVE the per-host `HostScheduler` (AIMD per origin): this caps
//! *total* in-flight fetch work across all hosts, sized from CPU parallelism
//! and shrunk under backpressure. Dep-free — uses `std::thread::
//! available_parallelism` + a resizable `tokio::sync::Semaphore`. AIMD:
//! additive-increase on healthy completions, multiplicative-decrease on
//! overload (429/503/resource pressure), never below `min` or above `max`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

/// Process-wide shared `AutoscaledPool`, lazily sized from CPU parallelism.
/// All `PageRunner`s in a process share one pool so total in-flight fetch
/// work is globally capped (and backs off together under overload).
pub fn global_autoscale() -> Arc<AutoscaledPool> {
    static GLOBAL: OnceLock<Arc<AutoscaledPool>> = OnceLock::new();
    GLOBAL
        .get_or_init(|| Arc::new(AutoscaledPool::from_parallelism()))
        .clone()
}

/// Compute the next concurrency target. Multiplicative decrease (halve) on
/// overload, additive increase (+1) otherwise, clamped to `[min, max]`.
pub fn next_target(current: usize, min: usize, max: usize, overload: bool) -> usize {
    let next = if overload {
        (current / 2).max(min)
    } else {
        (current + 1).min(max)
    };
    next.clamp(min, max)
}

/// Resource-aware global concurrency pool.
pub struct AutoscaledPool {
    sem: Arc<Semaphore>,
    target: AtomicUsize,
    min: usize,
    max: usize,
}

impl AutoscaledPool {
    /// Explicit bounds. `initial`/`min`/`max` are clamped so `1 ≤ min ≤
    /// initial ≤ max`.
    pub fn new(initial: usize, min: usize, max: usize) -> Self {
        let min = min.max(1);
        let max = max.max(min);
        let initial = initial.clamp(min, max);
        Self {
            sem: Arc::new(Semaphore::new(initial)),
            target: AtomicUsize::new(initial),
            min,
            max,
        }
    }

    /// Default sizing from CPU parallelism: start at `cores`, floor 1, ceiling
    /// `cores * 4` (I/O-bound crawl work tolerates oversubscription).
    pub fn from_parallelism() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self::new(cores, 1, cores * 4)
    }

    /// Acquire one global slot; the returned permit releases on drop.
    pub async fn acquire(&self) -> OwnedSemaphorePermit {
        // unwrap: the semaphore is never closed for the pool's lifetime.
        self.sem
            .clone()
            .acquire_owned()
            .await
            .expect("autoscale semaphore closed")
    }

    pub fn target(&self) -> usize {
        self.target.load(Ordering::Relaxed)
    }

    pub fn available(&self) -> usize {
        self.sem.available_permits()
    }

    /// Healthy completion → additive increase toward `max`.
    pub fn record_ok(&self) {
        let cur = self.target.load(Ordering::Relaxed);
        let next = next_target(cur, self.min, self.max, false);
        if next > cur {
            self.sem.add_permits(next - cur);
            self.target.store(next, Ordering::Relaxed);
        }
    }

    /// Overload signal (429/503/resource pressure) → multiplicative decrease.
    /// Best-effort: forgets currently-available permits down to the new target;
    /// in-flight permits shrink the effective ceiling as they return.
    pub fn record_overload(&self) {
        let cur = self.target.load(Ordering::Relaxed);
        let next = next_target(cur, self.min, self.max, true);
        if next < cur {
            self.sem.forget_permits(cur - next);
            self.target.store(next, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_target_halves_on_overload() {
        assert_eq!(next_target(8, 1, 16, true), 4);
        assert_eq!(next_target(3, 2, 16, true), 2); // floor at min
        assert_eq!(next_target(1, 1, 16, true), 1);
    }

    #[test]
    fn next_target_additive_increase() {
        assert_eq!(next_target(4, 1, 16, false), 5);
        assert_eq!(next_target(16, 1, 16, false), 16); // ceiling at max
    }

    #[test]
    fn new_clamps_bounds() {
        let p = AutoscaledPool::new(100, 2, 8);
        assert_eq!(p.target(), 8); // initial clamped to max
        let p2 = AutoscaledPool::new(0, 2, 8);
        assert_eq!(p2.target(), 2); // initial clamped up to min
    }

    #[tokio::test]
    async fn acquire_consumes_permits() {
        let p = AutoscaledPool::new(2, 1, 4);
        let _a = p.acquire().await;
        let _b = p.acquire().await;
        assert_eq!(p.available(), 0);
    }

    #[test]
    fn record_overload_then_ok_adjusts_target() {
        let p = AutoscaledPool::new(8, 1, 16);
        p.record_overload();
        assert_eq!(p.target(), 4);
        p.record_ok();
        assert_eq!(p.target(), 5);
    }

    #[test]
    fn global_autoscale_is_shared() {
        let a = global_autoscale();
        let b = global_autoscale();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[tokio::test]
    async fn overload_shrinks_available_ceiling() {
        let p = AutoscaledPool::new(8, 1, 16);
        p.record_overload(); // 8 -> 4
                             // 4 permits forgotten from the 8 available.
        assert_eq!(p.available(), 4);
    }
}
