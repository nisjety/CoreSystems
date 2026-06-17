//! Live benchmark corpus + scorecard wire shapes.
//!
//! Cycle 28 / cluster #12.
//!
//! ## Goal
//!
//! Validate Quarry's leadership claims against named baselines so a
//! release candidate ships with measured numbers, not vibes:
//!
//! - V1 local (Quarry-v1, Go) — internal regression baseline
//! - Firecrawl self-host / cloud — primary commercial competitor
//! - Trafilatura — best-in-class OSS markdown extractor
//! - Mozilla Readability — browser-native baseline
//!
//! ## Corpus buckets
//!
//! Each suite runs against multiple buckets so a regression in
//! "JS-heavy sites" doesn't hide behind a perfect score on "static
//! HTML". The buckets mirror gap-quarry §10.1 #12:
//!
//! - static-html
//! - js-heavy
//! - bot-sensitive
//! - ecommerce
//! - docs-blog
//! - pdf
//! - login-profile-restore
//! - crawl-with-sitemap
//! - change-tracking (gold set)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One benchmark suite definition. Operators register suites via
/// `/v1/benchmarks` POST (cycle 29 follow-up); the runtime + lab/evals
/// crate consume this shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkSuite {
    pub suite_id: String,
    pub name: String,
    /// One of the corpus buckets — pinned vocab. Free-form for
    /// extensibility but new buckets should land here first.
    pub bucket: String,
    /// URLs (or URL templates) the suite runs against.
    pub corpus: Vec<String>,
    /// Baselines this suite measures against. Producers identified
    /// by `name`; the runner records every named producer's score
    /// per run.
    pub baselines: Vec<BaselineProducer>,
    /// What metric defines "good". Free-form but pinned conventions:
    /// `"markdown_quality"`, `"extraction_completeness"`,
    /// `"latency_p95_ms"`, `"block_rate"`, `"change_detection_f1"`.
    pub primary_metric: String,
    /// Higher / lower / equality target. Some metrics are "smaller
    /// is better" (latency, block-rate); be explicit.
    pub target: MetricTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineProducer {
    pub name: String,
    /// Optional version pin so a scoreboard entry is reproducible
    /// after the baseline ships a new release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Where the producer runs (`"local" | "cloud" | "internal"`).
    pub locale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricTarget {
    HigherBetter,
    LowerBetter,
    /// For metrics where the target is a band (e.g., "within 5%
    /// of the gold set"). The runner uses a separate threshold
    /// configured per-suite.
    InRange,
}

/// One row of the scoreboard — a single producer's score on a
/// single suite at a single point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScorecardEntry {
    pub entry_id: String,
    pub suite_id: String,
    pub producer: String,
    /// Quarry build / version that ran. Useful for regression triage.
    pub quarry_version: Option<String>,
    /// Numerical score for `suite.primary_metric`.
    pub primary_score: f64,
    /// Optional per-bucket sub-scores. Lets the dashboard show
    /// "static-html 0.95 / js-heavy 0.62 → js-heavy regression".
    #[serde(default)]
    pub bucket_scores: Vec<BucketScore>,
    pub ran_at: DateTime<Utc>,
    /// Optional URL of the full run artifact for audit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BucketScore {
    pub bucket: String,
    pub score: f64,
    /// Number of URLs this bucket score is averaged over.
    pub samples: u32,
}

/// Diff between two scorecards — used to gate release candidates.
/// Computed by `compare_release_benchmarks` (cycle 29 helper).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseComparison {
    pub from_version: String,
    pub to_version: String,
    pub suite_diffs: Vec<SuiteDiff>,
    /// Overall verdict — pass / warn / fail. Driven by the per-suite
    /// regression thresholds (default 5% drop on a HigherBetter
    /// metric is a `Warn`; 15% is `Fail`).
    pub verdict: ComparisonVerdict,
    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteDiff {
    pub suite_id: String,
    pub from_score: f64,
    pub to_score: f64,
    pub delta_pct: f64,
    pub verdict: ComparisonVerdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonVerdict {
    Pass,
    Warn,
    Fail,
}

/// Default regression thresholds (percent drop) for release/baseline gating.
pub const WARN_PCT: f64 = 5.0;
pub const FAIL_PCT: f64 = 10.0;

/// Verdict for a `candidate` score vs a `baseline`, honoring the metric's
/// direction (`MetricTarget`). `warn_pct`/`fail_pct` are regression magnitudes
/// in percent (e.g. 5.0, 10.0). A zero baseline yields `Pass` (nothing to
/// regress against). This is the scoring primitive behind release comparisons
/// and producer bake-offs (OSS-parity P2 3E).
pub fn compare_score(
    candidate: f64,
    baseline: f64,
    target: MetricTarget,
    warn_pct: f64,
    fail_pct: f64,
) -> ComparisonVerdict {
    if baseline.abs() < f64::EPSILON {
        return ComparisonVerdict::Pass;
    }
    // improvement_pct: positive ⇒ candidate is better than baseline.
    let improvement_pct = match target {
        MetricTarget::HigherBetter => (candidate - baseline) / baseline * 100.0,
        MetricTarget::LowerBetter => (baseline - candidate) / baseline * 100.0,
        MetricTarget::InRange => -((candidate - baseline).abs() / baseline * 100.0),
    };
    if improvement_pct < -fail_pct {
        ComparisonVerdict::Fail
    } else if improvement_pct < -warn_pct {
        ComparisonVerdict::Warn
    } else {
        ComparisonVerdict::Pass
    }
}

#[cfg(test)]
mod compare_tests {
    use super::*;

    #[test]
    fn higher_better_regression_grades() {
        // 12% drop → Fail, 7% drop → Warn, improvement → Pass.
        assert_eq!(
            compare_score(88.0, 100.0, MetricTarget::HigherBetter, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Fail
        );
        assert_eq!(
            compare_score(93.0, 100.0, MetricTarget::HigherBetter, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Warn
        );
        assert_eq!(
            compare_score(105.0, 100.0, MetricTarget::HigherBetter, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Pass
        );
    }

    #[test]
    fn lower_better_inverts() {
        // latency: lower candidate beats baseline → Pass; much higher → Fail.
        assert_eq!(
            compare_score(80.0, 100.0, MetricTarget::LowerBetter, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Pass
        );
        assert_eq!(
            compare_score(120.0, 100.0, MetricTarget::LowerBetter, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Fail
        );
    }

    #[test]
    fn in_range_grades_by_distance() {
        assert_eq!(
            compare_score(102.0, 100.0, MetricTarget::InRange, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Pass
        );
        assert_eq!(
            compare_score(120.0, 100.0, MetricTarget::InRange, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Fail
        );
    }

    #[test]
    fn zero_baseline_passes() {
        assert_eq!(
            compare_score(50.0, 0.0, MetricTarget::HigherBetter, WARN_PCT, FAIL_PCT),
            ComparisonVerdict::Pass
        );
    }
}

/// Built-in suites Quarry ships. Cycle 28 covers shape + registration;
/// actual run harness lives in `lab/evals`.
pub fn builtin_suites() -> Vec<BenchmarkSuite> {
    vec![
        BenchmarkSuite {
            suite_id: "bench-static-html".into(),
            name: "Static HTML extraction quality".into(),
            bucket: "static-html".into(),
            corpus: vec![
                "https://en.wikipedia.org/wiki/Rust_(programming_language)".into(),
                "https://www.mozilla.org/en-US/firefox/new/".into(),
            ],
            baselines: vec![
                BaselineProducer {
                    name: "trafilatura".into(),
                    version: Some("1.12".into()),
                    locale: "local".into(),
                },
                BaselineProducer {
                    name: "mozilla-readability".into(),
                    version: None,
                    locale: "local".into(),
                },
                BaselineProducer {
                    name: "quarry-v1".into(),
                    version: None,
                    locale: "local".into(),
                },
            ],
            primary_metric: "markdown_quality".into(),
            target: MetricTarget::HigherBetter,
        },
        BenchmarkSuite {
            suite_id: "bench-js-heavy".into(),
            name: "JS-heavy rendering completeness".into(),
            bucket: "js-heavy".into(),
            corpus: vec![
                "https://react.dev/learn".into(),
                "https://nextjs.org/docs".into(),
            ],
            baselines: vec![
                BaselineProducer {
                    name: "firecrawl-self-host".into(),
                    version: None,
                    locale: "local".into(),
                },
                BaselineProducer {
                    name: "firecrawl-cloud".into(),
                    version: None,
                    locale: "cloud".into(),
                },
            ],
            primary_metric: "extraction_completeness".into(),
            target: MetricTarget::HigherBetter,
        },
        BenchmarkSuite {
            suite_id: "bench-bot-sensitive".into(),
            name: "Bot-sensitive site bypass rate".into(),
            bucket: "bot-sensitive".into(),
            corpus: vec![
                // Operators MAY populate this with internal sites
                // they own; the public defaults are illustrative.
                "https://news.ycombinator.com/".into(),
            ],
            baselines: vec![BaselineProducer {
                name: "quarry-v1".into(),
                version: None,
                locale: "local".into(),
            }],
            primary_metric: "block_rate".into(),
            target: MetricTarget::LowerBetter,
        },
        BenchmarkSuite {
            suite_id: "bench-change-tracking-gold".into(),
            name: "Change-tracking accuracy (gold set)".into(),
            bucket: "change-tracking".into(),
            corpus: vec![],
            baselines: vec![],
            primary_metric: "change_detection_f1".into(),
            target: MetricTarget::HigherBetter,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_target_json_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&MetricTarget::HigherBetter).unwrap(),
            "\"higher_better\""
        );
        assert_eq!(
            serde_json::to_string(&MetricTarget::LowerBetter).unwrap(),
            "\"lower_better\""
        );
    }

    #[test]
    fn comparison_verdict_json_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&ComparisonVerdict::Warn).unwrap(),
            "\"warn\""
        );
    }

    #[test]
    fn builtin_suites_have_unique_ids() {
        let ids: Vec<_> = builtin_suites()
            .iter()
            .map(|s| s.suite_id.clone())
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len(), "duplicate suite_ids: {ids:?}");
    }

    #[test]
    fn bot_sensitive_uses_lower_better_metric() {
        let s = builtin_suites()
            .into_iter()
            .find(|s| s.suite_id == "bench-bot-sensitive")
            .unwrap();
        assert_eq!(s.target, MetricTarget::LowerBetter);
        assert_eq!(s.primary_metric, "block_rate");
    }

    #[test]
    fn scorecard_entry_roundtrips_through_json() {
        let now = Utc::now();
        let entry = ScorecardEntry {
            entry_id: "score_1".into(),
            suite_id: "bench-static-html".into(),
            producer: "quarry-v2".into(),
            quarry_version: Some("0.2.0".into()),
            primary_score: 0.91,
            bucket_scores: vec![BucketScore {
                bucket: "static-html".into(),
                score: 0.91,
                samples: 12,
            }],
            ran_at: now,
            artifact_url: None,
        };
        let s = serde_json::to_string(&entry).unwrap();
        let back: ScorecardEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(back.primary_score, 0.91);
        assert_eq!(back.bucket_scores[0].samples, 12);
    }
}
