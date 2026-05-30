//! Simple sorted-on-demand latency histogram.
//!
//! Stores raw `Duration` samples; sorts once per query. Fine for batch SLO runs;
//! not intended for high-frequency streaming metrics (use `metrics` crate for that).

use std::time::Duration;

use crate::percentile::{nearest_rank, Percentile};

/// In-memory latency sample collector.
#[derive(Debug, Default, Clone)]
pub struct LatencyHistogram {
    samples: Vec<Duration>,
}

impl LatencyHistogram {
    /// New empty histogram.
    #[must_use]
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Record a latency sample.
    pub fn record(&mut self, d: Duration) {
        self.samples.push(d);
    }

    /// Sample count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Is the histogram empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Return the given percentile. Sorts a cloned copy; caller-side cost is O(N log N).
    #[must_use]
    pub fn percentile(&self, p: Percentile) -> Option<Duration> {
        let mut sorted = self.samples.clone();
        sorted.sort();
        nearest_rank(&sorted, p)
    }

    /// Sum of recorded durations.
    #[must_use]
    pub fn total(&self) -> Duration {
        self.samples.iter().sum()
    }

    /// Mean latency. `None` if empty.
    #[must_use]
    pub fn mean(&self) -> Option<Duration> {
        if self.samples.is_empty() {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        let avg_ns = (self.total().as_nanos() / self.samples.len() as u128) as u64;
        Some(Duration::from_nanos(avg_ns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_reports_percentiles() {
        let mut h = LatencyHistogram::new();
        for ms in [10, 20, 30, 40, 50, 60, 70, 80, 90, 100] {
            h.record(Duration::from_millis(ms));
        }
        assert_eq!(h.len(), 10);
        assert_eq!(
            h.percentile(Percentile::P50),
            Some(Duration::from_millis(50))
        );
        assert_eq!(
            h.percentile(Percentile::Max),
            Some(Duration::from_millis(100))
        );
    }

    #[test]
    fn mean_of_empty_is_none() {
        assert!(LatencyHistogram::new().mean().is_none());
    }

    #[test]
    fn mean_of_uniform_equals_sample() {
        let mut h = LatencyHistogram::new();
        for _ in 0..5 {
            h.record(Duration::from_millis(10));
        }
        assert_eq!(h.mean(), Some(Duration::from_millis(10)));
    }
}
