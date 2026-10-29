//! Sample recorder: pairs a label with its latency.
//!
//! Used by the harness to bundle per-operation measurements (e.g. step index,
//! request id) alongside the duration so failure reports pinpoint the culprit.

use std::time::Duration;

/// One timed observation with a caller-chosen label.
#[derive(Debug, Clone)]
pub struct Sample {
    /// Label (e.g. step index, request id, or URL).
    pub label: String,
    /// Measured duration.
    pub duration: Duration,
}

/// Growing log of samples.
#[derive(Debug, Default, Clone)]
pub struct Recorder {
    samples: Vec<Sample>,
}

impl Recorder {
    /// New recorder.
    #[must_use]
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
        }
    }

    /// Append a sample.
    pub fn push(&mut self, label: impl Into<String>, duration: Duration) {
        self.samples.push(Sample {
            label: label.into(),
            duration,
        });
    }

    /// All samples in insertion order.
    #[must_use]
    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// Sorted durations (ascending) — ready for percentile extraction.
    #[must_use]
    pub fn sorted_durations(&self) -> Vec<Duration> {
        let mut v: Vec<_> = self.samples.iter().map(|s| s.duration).collect();
        v.sort();
        v
    }

    /// Sample count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Is the recorder empty?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Slowest sample (max duration).
    #[must_use]
    pub fn slowest(&self) -> Option<&Sample> {
        self.samples.iter().max_by_key(|s| s.duration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slowest_returns_max() {
        let mut r = Recorder::new();
        r.push("a", Duration::from_millis(5));
        r.push("b", Duration::from_millis(50));
        r.push("c", Duration::from_millis(25));
        assert_eq!(r.slowest().unwrap().label, "b");
    }

    #[test]
    fn sorted_durations_preserve_count() {
        let mut r = Recorder::new();
        r.push("x", Duration::from_millis(9));
        r.push("y", Duration::from_millis(3));
        let sorted = r.sorted_durations();
        assert_eq!(
            sorted,
            vec![Duration::from_millis(3), Duration::from_millis(9)]
        );
    }
}
