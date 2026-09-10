//! Labelled samples for refitting the confidence bands.
//!
//! Every threshold in [`crate::confidence`] was placed by hand from a probe of
//! eight answers whose truth this author happened to know. That was enough to
//! find the shape of the signal — fabrications collapse on the claim statistic
//! while correct content does not — and it is nowhere near enough to pin the
//! edges. The bands are honest guesses at where two measured clusters stop.
//!
//! What turns a guess into a fit is labelled production data, and the label
//! already exists: users rate turns. This module keeps each scored turn's
//! INPUTS next to its request id until a rating names it, then emits one
//! structured line joining the two. That line is the training row.
//!
//! Deliberately a log line rather than a store. The row is small, the volume is
//! one per rated turn, and the analysis is offline (see
//! `scripts/calibration_report.py`) — standing up a database for a
//! calibration exercise would cost more than the exercise. Nothing here is on
//! the answer path: recording is a map insert, and emission happens on the
//! rating request.
//!
//! No answer or question text is retained. A row is numbers plus a rating, so
//! it can be read, shipped and kept without carrying tenant content.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

/// How long a scored turn stays joinable to a rating. Ratings arrive while the
/// user is still looking at the answer; past this the row is dropped rather
/// than held.
const RETENTION: Duration = Duration::from_hours(2);

/// Ceiling on retained rows, so a busy plane cannot grow this without bound.
const MAX_ENTRIES: usize = 20_000;

/// The scoring inputs for one answer — everything the bands are fitted over.
#[derive(Debug, Clone)]
pub struct AnswerScoring {
    /// Model that served the answer; bands are per-model until proven otherwise.
    pub model: String,
    /// Final score the user was shown.
    pub confidence: f64,
    /// Geometric-mean probability over the answer's CLAIM tokens, when the
    /// provider reported logprobs. The statistic the bands grade on.
    pub claim_probability: Option<f64>,
    /// The same over every token, framing included — kept so a refit can test
    /// whether grading on claims was the right call rather than assuming it.
    pub whole_probability: Option<f64>,
    pub claim_tokens: u32,
    pub output_tokens: u32,
    pub kb_citations: u32,
    pub web_citations: u32,
    pub tool_successes: u32,
    pub assembly_grounded: bool,
    /// What the verification pass concluded, when it ran.
    pub verdict: Option<&'static str>,
    /// Whether re-sampling reproduced the answer, when it was tried.
    pub reproduced: Option<bool>,
}

struct Entry {
    scoring: AnswerScoring,
    at: Instant,
}

/// `request_id` → the scoring that produced that turn's number.
#[derive(Clone, Default)]
pub struct CalibrationRegistry {
    inner: Arc<DashMap<String, Entry>>,
}

impl CalibrationRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember how a turn was scored. Best-effort and bounded: at capacity the
    /// oldest expired rows are dropped, and if none have expired the sample is
    /// skipped rather than allowed to grow the map.
    pub fn record(&self, request_id: &str, scoring: AnswerScoring) {
        let request_id = request_id.trim();
        if request_id.is_empty() {
            return;
        }
        if self.inner.len() >= MAX_ENTRIES {
            self.evict_expired();
            if self.inner.len() >= MAX_ENTRIES {
                return;
            }
        }
        self.inner.insert(
            request_id.to_owned(),
            Entry {
                scoring,
                at: Instant::now(),
            },
        );
    }

    /// Take the scoring for a rated turn, if it is still held.
    #[must_use]
    pub fn take(&self, request_id: &str) -> Option<AnswerScoring> {
        let request_id = request_id.trim();
        if request_id.is_empty() {
            return None;
        }
        let (_, entry) = self.inner.remove(request_id)?;
        if entry.at.elapsed() > RETENTION {
            return None;
        }
        Some(entry.scoring)
    }

    fn evict_expired(&self) {
        self.inner.retain(|_, entry| entry.at.elapsed() <= RETENTION);
    }

    #[must_use]
    pub fn tracked(&self) -> usize {
        self.inner.len()
    }
}

/// The process-wide registry.
pub fn global() -> &'static CalibrationRegistry {
    static GLOBAL: std::sync::OnceLock<CalibrationRegistry> = std::sync::OnceLock::new();
    GLOBAL.get_or_init(CalibrationRegistry::new)
}

/// Remember a turn's scoring against its request id.
pub fn record(request_id: &str, scoring: AnswerScoring) {
    global().record(request_id, scoring);
}

/// Emit the labelled row for a rated turn, if its scoring is still held.
///
/// One line, one turn, stable field names — `calibration_sample` is the token
/// the analysis script greps for. A rating for a turn this process did not
/// score (another replica, or one older than [`RETENTION`]) emits nothing:
/// half a row is not a training row.
pub fn emit_sample(request_id: &str, rating: &str) {
    let Some(scoring) = global().take(request_id) else {
        return;
    };
    tracing::info!(
        sample = "calibration_sample",
        rating,
        model = %scoring.model,
        confidence = scoring.confidence,
        claim_probability = scoring.claim_probability,
        whole_probability = scoring.whole_probability,
        claim_tokens = scoring.claim_tokens,
        output_tokens = scoring.output_tokens,
        kb_citations = scoring.kb_citations,
        web_citations = scoring.web_citations,
        tool_successes = scoring.tool_successes,
        assembly_grounded = scoring.assembly_grounded,
        verdict = scoring.verdict,
        reproduced = scoring.reproduced,
        "calibration sample"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoring() -> AnswerScoring {
        AnswerScoring {
            model: "gpt-4o-mini".to_owned(),
            confidence: 0.72,
            claim_probability: Some(0.11),
            whole_probability: Some(0.63),
            claim_tokens: 2,
            output_tokens: 14,
            kb_citations: 0,
            web_citations: 0,
            tool_successes: 0,
            assembly_grounded: true,
            verdict: Some("unrelated"),
            reproduced: Some(false),
        }
    }

    #[test]
    fn a_scored_turn_is_joinable_to_its_rating() {
        let registry = CalibrationRegistry::new();
        registry.record("req-1", scoring());
        let taken = registry.take("req-1").expect("held");
        assert!((taken.claim_probability.unwrap() - 0.11).abs() < 1e-9);
        assert_eq!(taken.verdict, Some("unrelated"));
    }

    /// One row per rating. A second rating of the same turn must not produce a
    /// duplicate sample, which would weight that turn twice in the fit.
    #[test]
    fn a_turn_yields_at_most_one_row() {
        let registry = CalibrationRegistry::new();
        registry.record("req-1", scoring());
        assert!(registry.take("req-1").is_some());
        assert!(registry.take("req-1").is_none());
    }

    #[test]
    fn an_unknown_or_blank_request_yields_nothing() {
        let registry = CalibrationRegistry::new();
        assert!(registry.take("never-seen").is_none());
        assert!(registry.take("").is_none());
        registry.record("   ", scoring());
        assert_eq!(registry.tracked(), 0, "a blank id must not be stored");
    }

    /// The map is bounded: an unrated plane must not accumulate rows forever.
    #[test]
    fn the_registry_refuses_to_grow_past_its_ceiling() {
        let registry = CalibrationRegistry::new();
        for index in 0..MAX_ENTRIES {
            registry.record(&format!("req-{index}"), scoring());
        }
        assert_eq!(registry.tracked(), MAX_ENTRIES);
        registry.record("one-too-many", scoring());
        assert_eq!(registry.tracked(), MAX_ENTRIES);
        assert!(registry.take("one-too-many").is_none());
    }
}
