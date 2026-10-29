//! Deterministic canary traffic splitter.
//!
//! Uses BLAKE3 over a salt and a routing key to produce a stable bucket
//! number in `[0, 100)`. A request is "in canary" when its bucket is below
//! the configured percentage. The same `(salt, key)` always yields the same
//! verdict, while changing the salt rotates the population without code
//! changes — useful for staged rollouts and A/B experiments at the edge.

use std::convert::TryInto;

/// Splits traffic into a canary cohort using a deterministic hash.
///
/// `pct` is clamped to `0..=100`. A value of `0` disables the canary
/// (nothing is in cohort) and `100` puts everything in cohort.
#[derive(Debug, Clone)]
pub struct CanarySplitter {
    pct: u8,
    salt: String,
}

impl CanarySplitter {
    /// Build a new splitter. `pct` above 100 is clamped to 100.
    pub fn new(pct: u8, salt: impl Into<String>) -> Self {
        Self {
            pct: pct.min(100),
            salt: salt.into(),
        }
    }

    /// Configured percentage in `0..=100`.
    pub fn pct(&self) -> u8 {
        self.pct
    }

    /// Returns `true` when `key` falls inside the canary cohort.
    pub fn in_canary(&self, key: &str) -> bool {
        if self.pct == 0 {
            return false;
        }
        if self.pct >= 100 {
            return true;
        }
        self.bucket(key) < u32::from(self.pct)
    }

    /// Bucket value in `[0, 100)` for the given key.
    fn bucket(&self, key: &str) -> u32 {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.salt.as_bytes());
        hasher.update(b":");
        hasher.update(key.as_bytes());
        let digest = hasher.finalize();
        let prefix: [u8; 4] = digest.as_bytes()[..4]
            .try_into()
            .expect("blake3 digest has at least 4 bytes");
        u32::from_le_bytes(prefix) % 100
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("tenant-{i}")).collect()
    }

    #[test]
    fn zero_percent_excludes_everyone() {
        let s = CanarySplitter::new(0, "salt-a");
        for k in keys(1_000) {
            assert!(!s.in_canary(&k));
        }
    }

    #[test]
    fn hundred_percent_includes_everyone() {
        let s = CanarySplitter::new(100, "salt-a");
        for k in keys(1_000) {
            assert!(s.in_canary(&k));
        }
    }

    #[test]
    fn pct_above_100_is_clamped() {
        let s = CanarySplitter::new(250, "salt-a");
        assert_eq!(s.pct(), 100);
        assert!(s.in_canary("anything"));
    }

    #[test]
    fn deterministic_for_same_inputs() {
        let s = CanarySplitter::new(37, "salt-a");
        for k in keys(500) {
            assert_eq!(s.in_canary(&k), s.in_canary(&k));
        }
    }

    #[test]
    fn distribution_is_within_tolerance() {
        let pct = 25u8;
        let s = CanarySplitter::new(pct, "distribution-salt");
        let n = 10_000usize;
        let hits = keys(n).iter().filter(|k| s.in_canary(k)).count();
        let expected = n * pct as usize / 100;
        // 3% absolute tolerance over 10k samples is plenty for blake3.
        let tolerance = n * 3 / 100;
        let diff = hits.abs_diff(expected);
        assert!(
            diff <= tolerance,
            "hits={hits} expected={expected} diff={diff} tolerance={tolerance}"
        );
    }

    #[test]
    fn salt_changes_membership() {
        let a = CanarySplitter::new(50, "salt-a");
        let b = CanarySplitter::new(50, "salt-b");
        let mut differences = 0;
        for k in keys(1_000) {
            if a.in_canary(&k) != b.in_canary(&k) {
                differences += 1;
            }
        }
        // With independent salts and 50% cohorts we expect ~50% to flip;
        // require at least 20% to guard against degenerate hashing.
        assert!(differences > 200, "salt had no effect: {differences}");
    }

    #[test]
    fn bucket_is_in_range() {
        let s = CanarySplitter::new(50, "salt-a");
        for k in keys(1_000) {
            assert!(s.bucket(&k) < 100);
        }
    }
}
