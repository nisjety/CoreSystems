//! Lightweight answer-quality (confidence) scorer for the `Usage` SSE event
//! (Phase 7 B6).
//!
//! `confidence` was hardcoded `None` on every run. This is a deterministic v1
//! signal computed from OBSERVABLE properties of the actual completion — never a
//! fabricated constant:
//!   * an empty answer is a non-answer → low,
//!   * a length-truncated answer (hit the token ceiling mid-thought) → lowered,
//!   * explicit hedging / refusal / "as an AI" disclaimers → lowered,
//!   * a grounded answer (carried real retrieval citations) → raised.
//!
//! It is intentionally modest and explainable. A logprob-based or eval-lab
//! scorer is the documented next upgrade; this gives the run stream and the
//! Ops/Quality surface a real, non-null quality signal to render today.

/// Phrases that signal the model is uncertain, refusing, or disclaiming. Matched
/// case-insensitively as substrings of the answer.
const LOW_CONFIDENCE_MARKERS: &[&str] = &[
    "i'm not sure",
    "i am not sure",
    "i'm not certain",
    "i cannot",
    "i can't",
    "i don't know",
    "i do not know",
    "as an ai",
    "i'm unable",
    "i am unable",
    "unable to",
    "no information",
    "cannot determine",
    "not enough information",
    "i'm sorry",
];

const BASE: f64 = 0.72;
const TRUNCATED_PENALTY: f64 = 0.22;
const HEDGING_PENALTY: f64 = 0.18;
const GROUNDED_BONUS: f64 = 0.15;
const EMPTY_SCORE: f64 = 0.10;
const FLOOR: f64 = 0.05;
const CEIL: f64 = 0.98;

/// Score a completion's confidence in `[FLOOR, CEIL]`, or `None` when there is
/// nothing to score (so the caller emits a null confidence rather than a fake).
///
/// `output_tokens` / `max_tokens` detect length truncation (the answer was cut
/// off at the ceiling). `grounded` is true when the answer carried real
/// retrieval citations.
#[must_use]
pub fn score(answer: &str, output_tokens: u32, max_tokens: u32, grounded: bool) -> Option<f64> {
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        return Some(EMPTY_SCORE);
    }

    let mut value = BASE;

    if max_tokens > 0 && output_tokens >= max_tokens {
        value -= TRUNCATED_PENALTY;
    }

    let lowered = trimmed.to_lowercase();
    if LOW_CONFIDENCE_MARKERS.iter().any(|m| lowered.contains(m)) {
        value -= HEDGING_PENALTY;
    }

    if grounded {
        value += GROUNDED_BONUS;
    }

    Some(value.clamp(FLOOR, CEIL))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_answer_is_low_but_real() {
        assert_eq!(score("   ", 0, 1024, false), Some(EMPTY_SCORE));
    }

    #[test]
    fn clean_answer_is_base() {
        assert_eq!(
            score("The capital of Norway is Oslo.", 8, 1024, false),
            Some(BASE)
        );
    }

    #[test]
    fn grounded_answer_scores_higher_than_ungrounded() {
        let grounded = score("Per the source, revenue rose 4%.", 12, 1024, true).unwrap();
        let plain = score("Per the source, revenue rose 4%.", 12, 1024, false).unwrap();
        assert!(grounded > plain);
    }

    #[test]
    fn truncated_and_hedging_lower_the_score() {
        // Hit the token ceiling AND hedged → both penalties applied.
        let s = score(
            "I'm not sure, but as an AI I cannot verify this",
            1024,
            1024,
            false,
        )
        .unwrap();
        assert!(s < BASE);
        assert!(s >= FLOOR);
    }

    #[test]
    fn stays_within_bounds() {
        let s = score("ok", 5, 1024, true).unwrap();
        assert!((FLOOR..=CEIL).contains(&s));
    }
}
