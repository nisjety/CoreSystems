//! Percentile computation using the nearest-rank method.
//!
//! Exact and deterministic for small sample sets (N ≤ 10k). No floating-point
//! interpolation — chosen index is `ceil(p/100 * N) - 1`, clamped to `[0, N-1]`.

use std::time::Duration;

/// Named percentile markers used in the SLO catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Percentile {
    P50,
    P90,
    P95,
    P99,
    Max,
}

impl Percentile {
    /// Percentile value in [0.0, 100.0].
    #[must_use]
    pub fn value(self) -> f64 {
        match self {
            Self::P50 => 50.0,
            Self::P90 => 90.0,
            Self::P95 => 95.0,
            Self::P99 => 99.0,
            Self::Max => 100.0,
        }
    }
}

/// Extract a percentile from a **sorted-ascending** slice of durations.
///
/// Returns `None` when the slice is empty.
#[must_use]
pub fn nearest_rank(sorted: &[Duration], p: Percentile) -> Option<Duration> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len();
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let rank = ((p.value() / 100.0) * n as f64).ceil() as usize;
    // index = rank - 1, clamped to [0, n-1]
    let idx = rank.saturating_sub(1).min(n - 1);
    Some(sorted[idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn durs(ms: &[u64]) -> Vec<Duration> {
        let mut v: Vec<_> = ms.iter().copied().map(Duration::from_millis).collect();
        v.sort();
        v
    }

    #[test]
    fn empty_returns_none() {
        assert!(nearest_rank(&[], Percentile::P95).is_none());
    }

    #[test]
    fn p50_of_ten_samples() {
        let v = durs(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
        assert_eq!(
            nearest_rank(&v, Percentile::P50),
            Some(Duration::from_millis(50))
        );
    }

    #[test]
    fn p95_of_twenty_samples() {
        let v: Vec<_> = (1..=20).map(Duration::from_millis).collect();
        // rank = ceil(0.95 * 20) = 19 → idx 18 → value 19
        assert_eq!(
            nearest_rank(&v, Percentile::P95),
            Some(Duration::from_millis(19))
        );
    }

    #[test]
    fn p99_always_le_max() {
        let v = durs(&[1, 2, 3, 4, 5, 100]);
        let p99 = nearest_rank(&v, Percentile::P99).unwrap();
        let max = nearest_rank(&v, Percentile::Max).unwrap();
        assert!(p99 <= max);
    }

    #[test]
    fn single_sample_all_percentiles_equal() {
        let v = durs(&[42]);
        for p in [
            Percentile::P50,
            Percentile::P95,
            Percentile::P99,
            Percentile::Max,
        ] {
            assert_eq!(nearest_rank(&v, p), Some(Duration::from_millis(42)));
        }
    }
}
