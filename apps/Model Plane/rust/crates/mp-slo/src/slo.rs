//! SLO definition + verdict types.
//!
//! A `Slo` pairs a name with the kind of measurement and a target Duration.
//! A `Verdict` reports the observed value, the target, and whether it breached.

use std::time::Duration;

use crate::percentile::{nearest_rank, Percentile};

/// What the SLO measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SloKind {
    /// p50 latency must be ≤ target.
    LatencyP50,
    /// p95 latency must be ≤ target.
    LatencyP95,
    /// p99 latency must be ≤ target.
    LatencyP99,
    /// Max (p100) latency must be ≤ target.
    LatencyMax,
    /// Mean latency must be ≤ target.
    LatencyMean,
}

impl SloKind {
    fn percentile(self) -> Option<Percentile> {
        match self {
            Self::LatencyP50 => Some(Percentile::P50),
            Self::LatencyP95 => Some(Percentile::P95),
            Self::LatencyP99 => Some(Percentile::P99),
            Self::LatencyMax => Some(Percentile::Max),
            Self::LatencyMean => None,
        }
    }
}

/// A named SLO with a target latency.
#[derive(Debug, Clone)]
pub struct Slo {
    /// Dotted name, e.g. `streaming.first_token.p95`.
    pub name: &'static str,
    /// What is being measured.
    pub kind: SloKind,
    /// Allowed ceiling for the measured statistic.
    pub target: Duration,
}

/// Outcome of evaluating a `Slo` against a set of samples.
#[derive(Debug, Clone)]
pub struct Verdict {
    /// The SLO that was evaluated.
    pub name: &'static str,
    /// Kind of measurement.
    pub kind: SloKind,
    /// Measured statistic. `None` when the sample set was empty.
    pub observed: Option<Duration>,
    /// Target ceiling.
    pub target: Duration,
    /// Number of samples considered.
    pub sample_count: usize,
    /// True if observed > target (or samples were empty).
    pub breached: bool,
}

impl Slo {
    /// Evaluate this SLO against a set of **sorted-ascending** duration samples.
    ///
    /// For `LatencyMean`, pass unsorted durations — the function only uses the mean
    /// when `kind == LatencyMean`; otherwise assumes sorted input.
    #[must_use]
    pub fn evaluate(&self, sorted_samples: &[Duration]) -> Verdict {
        let observed = match self.kind {
            SloKind::LatencyMean => mean(sorted_samples),
            k => k.percentile().and_then(|p| nearest_rank(sorted_samples, p)),
        };
        let breached = match observed {
            None => true, // no samples ⇒ treat as breach so harness surfaces it
            Some(v) => v > self.target,
        };
        Verdict {
            name: self.name,
            kind: self.kind,
            observed,
            target: self.target,
            sample_count: sorted_samples.len(),
            breached,
        }
    }
}

fn mean(samples: &[Duration]) -> Option<Duration> {
    if samples.is_empty() {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let avg_ns =
        (samples.iter().map(Duration::as_nanos).sum::<u128>() / samples.len() as u128) as u64;
    Some(Duration::from_nanos(avg_ns))
}

impl Verdict {
    /// Format a compact one-line report.
    #[must_use]
    pub fn summary(&self) -> String {
        match self.observed {
            None => format!(
                "{}: BREACH (no samples; target {:?})",
                self.name, self.target
            ),
            Some(v) => format!(
                "{}: {} observed={:?} target={:?} n={}",
                self.name,
                if self.breached { "BREACH" } else { "ok" },
                v,
                self.target,
                self.sample_count
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(ms: &[u64]) -> Vec<Duration> {
        let mut v: Vec<_> = ms.iter().copied().map(Duration::from_millis).collect();
        v.sort();
        v
    }

    #[test]
    fn empty_samples_breach() {
        let slo = Slo {
            name: "x",
            kind: SloKind::LatencyP95,
            target: Duration::from_millis(100),
        };
        let v = slo.evaluate(&[]);
        assert!(v.breached);
        assert!(v.observed.is_none());
    }

    #[test]
    fn p95_under_target_is_ok() {
        let slo = Slo {
            name: "x",
            kind: SloKind::LatencyP95,
            target: Duration::from_millis(200),
        };
        let samples = sorted(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100]);
        let v = slo.evaluate(&samples);
        assert!(!v.breached, "{}", v.summary());
    }

    #[test]
    fn p95_over_target_breaches() {
        let slo = Slo {
            name: "x",
            kind: SloKind::LatencyP95,
            target: Duration::from_millis(50),
        };
        let samples = sorted(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 500]);
        let v = slo.evaluate(&samples);
        assert!(v.breached, "{}", v.summary());
        assert_eq!(v.observed, Some(Duration::from_millis(500)));
    }

    #[test]
    fn latency_mean_uses_mean() {
        let slo = Slo {
            name: "m",
            kind: SloKind::LatencyMean,
            target: Duration::from_millis(30),
        };
        let v = slo.evaluate(&sorted(&[10, 20, 30, 40, 50]));
        // mean = 30ms; target = 30ms; not breached (observed <= target).
        assert!(!v.breached, "{}", v.summary());
    }
}
