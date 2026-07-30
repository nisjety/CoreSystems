//! Lightweight answer-quality (confidence) scorer for the `Usage` SSE event
//! (Phase 7 B6).
//!
//! `confidence` was hardcoded `None` on every run. This is a deterministic
//! signal computed from OBSERVABLE properties of the actual completion — never a
//! fabricated constant:
//!   * an empty answer is a non-answer → low,
//!   * a length-truncated answer (hit the token ceiling mid-thought) → lowered,
//!   * explicit hedging / refusal / "as an AI" disclaimers → lowered,
//!   * evidence (citations, successful tool calls) → raised, GRADUATED by how
//!     much evidence there is,
//!   * failed tool calls alongside the evidence → slightly lowered.
//!
//! v1 treated "grounded" as a single boolean worth a flat +0.15, which meant
//! every grounded, non-hedged answer scored exactly BASE + 0.15 = 0.87 — a
//! constant wearing a percent sign. Users noticed ("why does every answer say
//! 87%?"), and a signal that never moves is worse than no signal. v2 keeps the
//! same explainable shape but grades the bonus on evidence VOLUME and debits
//! failed tools, so the number actually varies with the quality of the turn.
//!
//! It is intentionally modest. A logprob-based or eval-lab scorer is the
//! documented next upgrade; this gives the run stream and the Ops/Quality
//! surface a real, non-null quality signal to render today.

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
    // Norwegian — the product's primary language; without these a hedging
    // Norwegian answer scored the same as a confident one.
    "jeg er usikker",
    "jeg vet ikke",
    "jeg kan ikke",
    "har ikke tilgang",
    "ikke nok informasjon",
    "klarer ikke",
    "finner ikke",
    "beklager",
    "kan ikke svare",
    "vet ikke",
];

const BASE: f64 = 0.72;
const TRUNCATED_PENALTY: f64 = 0.22;
const HEDGING_PENALTY: f64 = 0.18;
const EMPTY_SCORE: f64 = 0.10;
const FLOOR: f64 = 0.05;
const CEIL: f64 = 0.98;
/// Debit per failed tool call this turn, capped at [`MAX_FAILURE_PENALTY`].
/// Evidence gathered alongside failures is weaker than evidence gathered
/// cleanly — some of what the answer needed never arrived.
const TOOL_FAILURE_PENALTY: f64 = 0.04;
const MAX_FAILURE_PENALTY: f64 = 0.08;

/// The evidence a turn actually gathered, counted — not a boolean.
///
/// One `unit` = one independent piece of backing: a knowledge-base citation, a
/// web citation shown in the Sources tab, a successful tool call, or
/// session-core assembly grounding (counted once). The scorer grades on the
/// total, so a five-source answer reads as more confident than a one-source
/// answer instead of both collapsing to the same constant.
#[derive(Debug, Clone, Copy, Default)]
pub struct Evidence {
    /// Knowledge-base / retrieval citations carried by the answer.
    pub kb_citations: u32,
    /// Web citations emitted this turn (forced or model-chosen `web_search`).
    pub web_citations: u32,
    /// Tool calls that returned a non-error result this turn.
    pub tool_successes: u32,
    /// Tool calls that returned an error this turn.
    pub tool_failures: u32,
    /// Session-core's context assembly supplied retrieval/knowledge segments.
    pub assembly_grounded: bool,
}

impl Evidence {
    /// Legacy shim for callers that only know a single grounded-or-not flag
    /// (e.g. execution-core's run summary). One unit of evidence when grounded.
    #[must_use]
    pub fn from_grounded_flag(grounded: bool) -> Self {
        Self {
            tool_successes: u32::from(grounded),
            ..Self::default()
        }
    }

    fn units(self) -> u32 {
        self.kb_citations
            .saturating_add(self.web_citations)
            .saturating_add(self.tool_successes)
            .saturating_add(u32::from(self.assembly_grounded))
    }
}

/// Bonus for evidence volume. Diminishing returns by design: the jump from
/// zero to one source is the meaningful one; the tenth source barely moves it.
fn evidence_bonus(units: u32) -> f64 {
    match units {
        0 => 0.0,
        1 => 0.07,
        2 => 0.10,
        3 => 0.12,
        4..=6 => 0.14,
        _ => 0.16,
    }
}

/// Score a completion's confidence in `[FLOOR, CEIL]`, or `None` when there is
/// nothing to score (so the caller emits a null confidence rather than a fake).
///
/// `output_tokens` / `max_tokens` detect length truncation (the answer was cut
/// off at the ceiling). `evidence` carries what actually backed the answer this
/// turn; a single successful tool call still clears the UI's 0.75
/// low-confidence threshold (BASE + one-unit bonus = 0.79), so correct
/// tool-sourced answers are never flagged "uncertain" — the regression v1 fixed
/// stays fixed.
#[must_use]
pub fn score(
    answer: &str,
    output_tokens: u32,
    max_tokens: u32,
    evidence: Evidence,
) -> Option<f64> {
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

    value += evidence_bonus(evidence.units());
    value -= (f64::from(evidence.tool_failures) * TOOL_FAILURE_PENALTY).min(MAX_FAILURE_PENALTY);

    Some(value.clamp(FLOOR, CEIL))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_answer_is_low_but_real() {
        assert_eq!(score("   ", 0, 1024, Evidence::default()), Some(EMPTY_SCORE));
    }

    #[test]
    fn clean_answer_without_evidence_is_base() {
        assert_eq!(
            score("The capital of Norway is Oslo.", 8, 1024, Evidence::default()),
            Some(BASE)
        );
    }

    #[test]
    fn grounded_answer_scores_higher_than_ungrounded() {
        let grounded = score(
            "Per the source, revenue rose 4%.",
            12,
            1024,
            Evidence::from_grounded_flag(true),
        )
        .unwrap();
        let plain = score("Per the source, revenue rose 4%.", 12, 1024, Evidence::default())
            .unwrap();
        assert!(grounded > plain);
    }

    /// The v1 regression this replaces: "grounded" was a flat +0.15, so every
    /// grounded answer scored exactly 0.87 regardless of whether it stood on
    /// one source or ten. A confidence number that never moves is decoration,
    /// not signal — the score must vary with evidence volume.
    #[test]
    fn confidence_varies_with_evidence_volume_not_a_constant() {
        let answer = "Oslo har 728 714 innbyggere.";
        let one = score(answer, 30, 4096, Evidence { web_citations: 1, ..Evidence::default() })
            .unwrap();
        let five = score(answer, 30, 4096, Evidence { web_citations: 5, ..Evidence::default() })
            .unwrap();
        let ten = score(answer, 30, 4096, Evidence { web_citations: 10, ..Evidence::default() })
            .unwrap();
        assert!(one < five, "more sources must score higher: {one} vs {five}");
        assert!(five < ten, "more sources must score higher: {five} vs {ten}");
    }

    #[test]
    fn failed_tools_alongside_evidence_lower_the_score() {
        let answer = "Delvis skyet, 23 grader.";
        let clean = score(
            answer,
            30,
            4096,
            Evidence { tool_successes: 1, ..Evidence::default() },
        )
        .unwrap();
        let mixed = score(
            answer,
            30,
            4096,
            Evidence { tool_successes: 1, tool_failures: 2, ..Evidence::default() },
        )
        .unwrap();
        assert!(mixed < clean, "failures must debit: {mixed} vs {clean}");
        // ...but the debit is capped so a noisy turn can't nuke the score.
        let noisy = score(
            answer,
            30,
            4096,
            Evidence { tool_successes: 1, tool_failures: 40, ..Evidence::default() },
        )
        .unwrap();
        assert!(noisy >= BASE + 0.07 - MAX_FAILURE_PENALTY - 1e-9);
    }

    #[test]
    fn truncated_and_hedging_lower_the_score() {
        // Hit the token ceiling AND hedged → both penalties applied.
        let s = score(
            "I'm not sure, but as an AI I cannot verify this",
            1024,
            1024,
            Evidence::default(),
        )
        .unwrap();
        assert!(s < BASE);
        assert!(s >= FLOOR);
    }

    #[test]
    fn norwegian_hedging_lowers_the_score() {
        // The product answers in Norwegian; a hedging Norwegian answer must be
        // penalized like its English equivalent, not scored as confident.
        let hedged = score(
            "Jeg er usikker, men jeg vet ikke svaret.",
            12,
            4096,
            Evidence::default(),
        )
        .unwrap();
        assert!(hedged < BASE);
    }

    #[test]
    fn a_grounded_tool_answer_clears_the_ui_low_confidence_threshold() {
        // The v1 bug this must keep fixed: a correct tool-sourced answer scored
        // BASE (0.72) < the UI's 0.75 threshold and was flagged "uncertain"
        // every time. One successful tool call must still clear it.
        let grounded = score(
            "Lageret har 12 tomme varer.",
            30,
            4096,
            Evidence { tool_successes: 1, ..Evidence::default() },
        )
        .unwrap();
        assert!(
            grounded >= 0.75,
            "grounded tool answer should not be flagged: {grounded}"
        );
    }

    #[test]
    fn stays_within_bounds() {
        let s = score(
            "ok",
            5,
            1024,
            Evidence { kb_citations: 50, web_citations: 50, ..Evidence::default() },
        )
        .unwrap();
        assert!((FLOOR..=CEIL).contains(&s));
    }
}
