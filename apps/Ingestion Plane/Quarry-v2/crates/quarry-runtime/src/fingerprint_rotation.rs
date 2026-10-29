//! Auto fingerprint/session rotation on block detection (OSS-parity P1 2D).
//!
//! Crawlee-style anti-block resilience: when a host returns a bot-block status
//! (403/429/503/…), rotate to the next TLS-impersonation profile (and, by
//! extension, session/profile) before retrying — instead of hammering the same
//! fingerprint. Bounded by `max_rotations` so a hard block can't spin forever.
//!
//! This is the rotation *policy* (dep-free, pure-logic, fully tested). The
//! driver/fallback layer consumes `on_block()` to pick the next profile name
//! and re-lease — wiring mirrors the HostScheduler rollout.

use std::sync::atomic::{AtomicUsize, Ordering};

/// True when an HTTP status signals an anti-bot block / soft denial worth
/// rotating fingerprint for (vs. a genuine 404/400 the rotation won't fix).
pub fn is_block_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 407 | 429 | 503)
}

/// Round-robin next index over `len` items.
pub fn next_index(current: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        (current + 1) % len
    }
}

/// Round-robin fingerprint rotator with a bounded rotation budget.
pub struct FingerprintRotator {
    profiles: Vec<String>,
    idx: AtomicUsize,
    rotations: AtomicUsize,
    max_rotations: usize,
}

impl FingerprintRotator {
    /// `profiles` is the impersonation set (e.g. `["chrome","firefox",
    /// "safari"]`). `max_rotations` caps total rotations per run.
    pub fn new(profiles: Vec<String>, max_rotations: usize) -> Self {
        Self {
            profiles,
            idx: AtomicUsize::new(0),
            rotations: AtomicUsize::new(0),
            max_rotations,
        }
    }

    /// The currently-selected profile name (empty string if no profiles).
    pub fn current(&self) -> &str {
        let i = self.idx.load(Ordering::Relaxed);
        self.profiles.get(i).map(String::as_str).unwrap_or("")
    }

    pub fn rotations_used(&self) -> usize {
        self.rotations.load(Ordering::Relaxed)
    }

    /// True once the rotation budget is spent or there's nothing to rotate to.
    pub fn exhausted(&self) -> bool {
        self.profiles.len() < 2 || self.rotations_used() >= self.max_rotations
    }

    /// Advance to the next profile after a block. Returns the new profile name,
    /// or `None` when rotation is exhausted (caller should give up / fail over).
    pub fn on_block(&self) -> Option<String> {
        if self.exhausted() {
            return None;
        }
        let cur = self.idx.load(Ordering::Relaxed);
        let next = next_index(cur, self.profiles.len());
        self.idx.store(next, Ordering::Relaxed);
        self.rotations.fetch_add(1, Ordering::Relaxed);
        self.profiles.get(next).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_statuses_detected() {
        for s in [401, 403, 407, 429, 503] {
            assert!(is_block_status(s), "{s} should be a block");
        }
        for s in [200, 301, 400, 404, 500] {
            assert!(!is_block_status(s), "{s} should NOT trigger rotation");
        }
    }

    #[test]
    fn next_index_wraps() {
        assert_eq!(next_index(0, 3), 1);
        assert_eq!(next_index(2, 3), 0);
        assert_eq!(next_index(0, 0), 0);
    }

    #[test]
    fn rotates_round_robin_then_exhausts() {
        let r =
            FingerprintRotator::new(vec!["chrome".into(), "firefox".into(), "safari".into()], 2);
        assert_eq!(r.current(), "chrome");
        assert_eq!(r.on_block().as_deref(), Some("firefox"));
        assert_eq!(r.on_block().as_deref(), Some("safari"));
        assert_eq!(r.rotations_used(), 2);
        assert!(r.exhausted());
        assert_eq!(r.on_block(), None); // budget spent
    }

    #[test]
    fn single_profile_never_rotates() {
        let r = FingerprintRotator::new(vec!["chrome".into()], 5);
        assert!(r.exhausted());
        assert_eq!(r.on_block(), None);
    }
}
