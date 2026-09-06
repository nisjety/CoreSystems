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
//!   * failed tool calls alongside the evidence → slightly lowered,
//!   * the serving model's OWN token-level certainty (provider logprobs) →
//!     raised when the answer was near-certain token by token.
//!
//! v1 treated "grounded" as a single boolean worth a flat +0.15, which meant
//! every grounded, non-hedged answer scored exactly BASE + 0.15 = 0.87 — a
//! constant wearing a percent sign. Users noticed ("why does every answer say
//! 87%?"), and a signal that never moves is worse than no signal. v2 keeps the
//! same explainable shape but grades the bonus on evidence VOLUME and debits
//! failed tools, so the number actually varies with the quality of the turn.
//!
//! v3 adds the signal v2 named as its next upgrade: [`ModelCertainty`], the
//! provider's own per-token logprobs, summarized by inference-core and carried
//! on the contract. Everything before it read the finished TEXT or counted OUR
//! evidence, so a fact the model knows cold and an ungrounded guess were
//! indistinguishable — "Oslo." scored the same BASE as an invented figure and
//! rendered as "uncertain" to the user. Its bands are calibrated against
//! measured completions (see [`certainty_adjustment`]), including the limit of
//! what logprobs can honestly tell us: they identify near-certainty, and they
//! do not identify a fluent fabrication.
//!
//! An eval-lab scorer remains the next upgrade beyond this.

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
const LOW_CONFIDENCE_RETRIEVAL_PENALTY: f64 = 0.20;
const LOW_CONFIDENCE_RETRIEVAL_CEIL: f64 = 0.74;
/// Debit per failed tool call this turn, capped at [`MAX_FAILURE_PENALTY`].
/// Evidence gathered alongside failures is weaker than evidence gathered
/// cleanly — some of what the answer needed never arrived.
const TOOL_FAILURE_PENALTY: f64 = 0.04;
const MAX_FAILURE_PENALTY: f64 = 0.08;

/// Geometric-mean token probability at which the model is, in substance,
/// certain — and the floor such an answer's score may not fall below.
///
/// Measured on the claim statistic, answers only reach this band when the model
/// genuinely knows the thing cold ("… er Oslo.", "… er Lisboa.", "17 ganger 24
/// er 408." — all 0.999–1.000); fabrications land at 0.00–0.22. So in this one
/// band the model's own signal is worth more than the absence of a citation: an
/// uncited answer the model is certain of should not read as
/// barely-better-than-uncertain merely because the org's knowledge base has
/// nothing to say about arithmetic or world capitals.
///
/// The floor does NOT apply to a hedged or truncated answer (see
/// `score_with_certainty`), and it loses to Data Plane's weak-retrieval cap,
/// which is applied afterwards.
const NEAR_CERTAIN_PROBABILITY: f64 = 0.95;
const NEAR_CERTAIN_FLOOR: f64 = 0.90;

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
        self.citable_units()
            .saturating_add(u32::from(self.assembly_grounded))
    }

    /// Evidence a reader could actually follow: citations and successful tool
    /// calls.
    ///
    /// Excludes `assembly_grounded` deliberately, and the distinction is
    /// load-bearing for [`crate::verification`]: assembly supplies prompt
    /// CONTEXT, not a source the answer can be checked against, and it is true
    /// on nearly every turn in a deployment with Data Plane wired up. Treating
    /// it as "this turn already has evidence" would skip verification almost
    /// always — the feature would look enabled and do nothing.
    #[must_use]
    pub fn citable_units(self) -> u32 {
        self.kb_citations
            .saturating_add(self.web_citations)
            .saturating_add(self.tool_successes)
    }
}

/// How sure the serving MODEL was, token by token — the provider's own
/// logprobs, summarized by inference-core and carried on `InferResponse` /
/// the final `InferChunk`.
///
/// Every other input to this scorer reads the finished text or counts our own
/// evidence, so a near-certain fact and a confident-sounding guess were
/// indistinguishable: "Oslo." as the capital of Norway scored exactly the same
/// BASE as an invented figure, because both are short, unhedged and uncited.
/// This is the one signal that separates them.
///
/// `None` means the serving provider does not report logprobs (Anthropic never
/// does) or the model rejects the parameter — unknown, never low, and the score
/// falls back to the text-and-evidence heuristic alone.
#[derive(Debug, Clone, Copy)]
pub struct ModelCertainty {
    token_count: u32,
    mean_logprob: f64,
    claim_token_count: u32,
    claim_mean_logprob: f64,
}

impl ModelCertainty {
    /// Build from the contract's `TokenConfidence` fields. `None` when the
    /// summary covers no tokens or carries a nonsensical mean, so a malformed
    /// value can never be read as a confident answer. Claim fields that fail
    /// the same checks are dropped individually — the whole-answer mean still
    /// stands on its own.
    #[must_use]
    pub fn new(
        token_count: u32,
        mean_logprob: f64,
        claim_token_count: u32,
        claim_mean_logprob: f64,
    ) -> Option<Self> {
        if token_count == 0 || !mean_logprob.is_finite() || mean_logprob > 0.0 {
            return None;
        }
        let claim_usable =
            claim_token_count > 0 && claim_mean_logprob.is_finite() && claim_mean_logprob <= 0.0;
        Some(Self {
            token_count,
            mean_logprob,
            claim_token_count: if claim_usable { claim_token_count } else { 0 },
            claim_mean_logprob: if claim_usable { claim_mean_logprob } else { 0.0 },
        })
    }

    /// The certainty the scorer grades on: the answer's CLAIM tokens when the
    /// provider identified any, otherwise the whole answer.
    ///
    /// Preferring the claim tokens is what makes this signal usable at all.
    /// Over the same measured answers the whole-answer mean put fabrications
    /// (0.63–0.78) inside the range of correct prose (0.69) — no threshold
    /// could separate them — while the claim mean put those fabrications at
    /// 0.00–0.22 against 0.60–1.00 for correct content.
    #[must_use]
    pub fn per_token_probability(self) -> f64 {
        if self.claim_token_count > 0 {
            self.claim_mean_logprob.exp()
        } else {
            self.mean_logprob.exp()
        }
    }

    /// The whole-answer statistic, framing included. Kept for reporting; the
    /// scorer does not grade on it.
    #[must_use]
    pub fn whole_answer_probability(self) -> f64 {
        self.mean_logprob.exp()
    }

    /// Answer tokens the summary covers.
    #[must_use]
    pub fn token_count(self) -> u32 {
        self.token_count
    }

    /// Tokens that carried a claim; 0 when the answer was entirely framing.
    #[must_use]
    pub fn claim_token_count(self) -> u32 {
        self.claim_token_count
    }
}

/// Adjustment for the model's own certainty.
///
/// Calibrated against measured answers rather than intuition (gpt-4o-mini via
/// Azure `OpenAI`, geometric-mean token probability after the sentinel clamp).
/// Both statistics shown, because the difference between the columns is the
/// whole reason [`ModelCertainty::per_token_probability`] grades on claims:
///
/// | answer                                  | whole | claim | true? |
/// |-----------------------------------------|-------|-------|-------|
/// | "Hovedstaden i Norge er Oslo."          | 0.998 | 1.000 | yes   |
/// | "Hovedstaden i Portugal er Lisboa."     | 1.000 | 0.999 | yes   |
/// | "17 ganger 24 er 408."                  | 1.000 | 1.000 | yes   |
/// | "Hovedstaden i Finland er Helsingfors"  | 0.938 | 0.728 | yes   |
/// | ordinary explanatory prose (120 tokens) | 0.685 | 0.600 | yes   |
/// | invented capital of a fictional country | 0.633 | 0.111 | NO    |
/// | fabricated founding year                | 0.776 | 0.220 | NO    |
/// | fabricated head-count                   | 0.106 | 0.000 | NO    |
///
/// In the `whole` column the fabrications (0.63–0.78) sit inside the range of
/// correct content (0.69–1.00) and no threshold separates them. In the `claim`
/// column they collapse to 0.00–0.22 while correct content holds 0.60–1.00,
/// leaving a wide empty gap — so the bands are placed in it: a real bonus only
/// where measured true answers live, debits only below where any of them
/// landed, and a deliberately generous neutral band in between for prose,
/// which legitimately sits mid-range.
fn certainty_adjustment(certainty: ModelCertainty) -> f64 {
    let probability = certainty.per_token_probability();
    if probability >= 0.95 {
        0.16
    } else if probability >= 0.80 {
        0.08
    } else if probability >= 0.45 {
        0.0
    } else if probability >= 0.25 {
        -0.06
    } else {
        -0.12
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
pub fn score(answer: &str, output_tokens: u32, max_tokens: u32, evidence: Evidence) -> Option<f64> {
    score_with_certainty(answer, output_tokens, max_tokens, evidence, None)
}

/// [`score`] plus the serving model's own token-level certainty, when the
/// provider reported it. See [`ModelCertainty`].
#[must_use]
pub fn score_with_certainty(
    answer: &str,
    output_tokens: u32,
    max_tokens: u32,
    evidence: Evidence,
    certainty: Option<ModelCertainty>,
) -> Option<f64> {
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        return Some(EMPTY_SCORE);
    }

    let mut value = BASE;

    let truncated = max_tokens > 0 && output_tokens >= max_tokens;
    if truncated {
        value -= TRUNCATED_PENALTY;
    }

    let lowered = trimmed.to_lowercase();
    let hedged = LOW_CONFIDENCE_MARKERS.iter().any(|m| lowered.contains(m));
    if hedged {
        value -= HEDGING_PENALTY;
    }

    value += evidence_bonus(evidence.units());
    value -= (f64::from(evidence.tool_failures) * TOOL_FAILURE_PENALTY).min(MAX_FAILURE_PENALTY);

    // Token certainty measures how predictable the wording was, not whether
    // the claim is true — and a model that has decided to say "jeg vet ikke"
    // says it very fluently. So when the CONTENT itself signals doubt (a hedge)
    // or was cut off mid-thought, certainty may only lower the score, never
    // raise it: high-probability tokens are no argument against the answer's
    // own disclaimer.
    let adjustment = certainty.map_or(0.0, certainty_adjustment);
    let doubts_itself = hedged || truncated;
    value += if doubts_itself {
        adjustment.min(0.0)
    } else {
        adjustment
    };

    // A near-certain answer floors above the "uncertain" threshold even with no
    // evidence at all — see NEAR_CERTAIN_PROBABILITY. Same suppression rule as
    // the bonus: an answer that hedges or was cut off cannot be floored by how
    // fluently it said so.
    //
    // The floor additionally REQUIRES measured claim tokens. It is the
    // strongest statement this scorer makes — 0.90 with no evidence whatsoever
    // — and without that condition it rests on the whole-answer mean, which is
    // dominated by framing that restates the question. An answer whose claim
    // could not be isolated would then be floored for sounding fluent, which is
    // the precise failure grading on claim tokens exists to prevent.
    if !doubts_itself
        && certainty.is_some_and(|c| {
            c.claim_token_count() > 0 && c.per_token_probability() >= NEAR_CERTAIN_PROBABILITY
        })
    {
        value = value.max(NEAR_CERTAIN_FLOOR);
    }

    Some(value.clamp(FLOOR, CEIL))
}

/// Apply a post-scoring debit, keeping the result on this module's scale.
///
/// Used when something learned AFTER scoring contradicts the answer — sources
/// that say otherwise (see [`crate::verification`]). Kept here so the clamp
/// bounds live with the constants that define them.
#[must_use]
pub fn debit(score: f64, penalty: f64) -> f64 {
    (score - penalty).clamp(FLOOR, CEIL)
}

/// Score a completion while preserving Data Plane's retrieval-confidence
/// verdict. Weak retrieval evidence must not receive the normal citation bonus
/// and then render above the UI's "uncertain" threshold.
///
/// The verdict only applies when the answer actually STANDS ON retrieval
/// (`evidence.kb_citations > 0`). `retrieval::build_grounding` also flags a
/// fact-less result `low_confidence`, so before this gate every turn the
/// knowledge base simply had nothing about — "what is the capital of Norway"
/// against a company KB — was debited 0.20 and capped at 0.74: a correct
/// general-knowledge answer rendered as "52% — uncertain", and a tool- or
/// web-backed answer never cleared the UI's 0.75 threshold. A weak or empty
/// match the answer did not use is not evidence against the answer; the
/// no-sources case is already communicated by the zero citation count.
///
/// When it does apply, the cap outranks [`ModelCertainty`] on purpose: our own
/// retrieval saying "this match is weak" is a statement about the evidence,
/// while token certainty is a statement about the wording, and fluent wording
/// must not talk the score back over the threshold that a weak citation put it
/// under.
#[must_use]
pub fn score_with_retrieval_confidence(
    answer: &str,
    output_tokens: u32,
    max_tokens: u32,
    evidence: Evidence,
    retrieval_low_confidence: bool,
    certainty: Option<ModelCertainty>,
) -> Option<f64> {
    let score = score_with_certainty(answer, output_tokens, max_tokens, evidence, certainty)?;
    if retrieval_low_confidence && evidence.kb_citations > 0 {
        Some((score - LOW_CONFIDENCE_RETRIEVAL_PENALTY).clamp(FLOOR, LOW_CONFIDENCE_RETRIEVAL_CEIL))
    } else {
        Some(score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_answer_is_low_but_real() {
        assert_eq!(
            score("   ", 0, 1024, Evidence::default()),
            Some(EMPTY_SCORE)
        );
    }

    #[test]
    fn clean_answer_without_evidence_is_base() {
        assert_eq!(
            score(
                "The capital of Norway is Oslo.",
                8,
                1024,
                Evidence::default()
            ),
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
        let plain = score(
            "Per the source, revenue rose 4%.",
            12,
            1024,
            Evidence::default(),
        )
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
        let one = score(
            answer,
            30,
            4096,
            Evidence {
                web_citations: 1,
                ..Evidence::default()
            },
        )
        .unwrap();
        let five = score(
            answer,
            30,
            4096,
            Evidence {
                web_citations: 5,
                ..Evidence::default()
            },
        )
        .unwrap();
        let ten = score(
            answer,
            30,
            4096,
            Evidence {
                web_citations: 10,
                ..Evidence::default()
            },
        )
        .unwrap();
        assert!(
            one < five,
            "more sources must score higher: {one} vs {five}"
        );
        assert!(
            five < ten,
            "more sources must score higher: {five} vs {ten}"
        );
    }

    #[test]
    fn failed_tools_alongside_evidence_lower_the_score() {
        let answer = "Delvis skyet, 23 grader.";
        let clean = score(
            answer,
            30,
            4096,
            Evidence {
                tool_successes: 1,
                ..Evidence::default()
            },
        )
        .unwrap();
        let mixed = score(
            answer,
            30,
            4096,
            Evidence {
                tool_successes: 1,
                tool_failures: 2,
                ..Evidence::default()
            },
        )
        .unwrap();
        assert!(mixed < clean, "failures must debit: {mixed} vs {clean}");
        // ...but the debit is capped so a noisy turn can't nuke the score.
        let noisy = score(
            answer,
            30,
            4096,
            Evidence {
                tool_successes: 1,
                tool_failures: 40,
                ..Evidence::default()
            },
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
            Evidence {
                tool_successes: 1,
                ..Evidence::default()
            },
        )
        .unwrap();
        assert!(
            grounded >= 0.75,
            "grounded tool answer should not be flagged: {grounded}"
        );
    }

    #[test]
    fn data_plane_low_confidence_cannot_render_as_high_confidence() {
        let evidence = Evidence {
            kb_citations: 5,
            ..Evidence::default()
        };
        let score = score_with_retrieval_confidence(
            "The policy says refunds are accepted.",
            30,
            4096,
            evidence,
            true,
            None,
        )
        .unwrap();
        assert!(score <= LOW_CONFIDENCE_RETRIEVAL_CEIL);
        assert!(score < 0.75);
    }

    /// The "never past 60%" regression: `build_grounding` marks an EMPTY
    /// knowledge-base result `low_confidence`, and the retrieval debit used to
    /// apply regardless of whether the answer used any retrieval. A clean
    /// general-knowledge answer must score BASE, not BASE - 0.20.
    #[test]
    fn empty_retrieval_does_not_debit_an_answer_that_used_no_retrieval() {
        let score = score_with_retrieval_confidence(
            "Oslo.",
            3,
            4096,
            Evidence::default(),
            true,
            None,
        )
        .unwrap();
        assert!((score - BASE).abs() < 1e-9, "unused empty retrieval debited: {score}");
    }

    /// Same gate for evidence that came from elsewhere: a weather answer backed
    /// by a successful tool call must clear the UI threshold even when the KB
    /// lookup that ran alongside it found nothing.
    #[test]
    fn tool_backed_answer_clears_threshold_despite_empty_retrieval() {
        let score = score_with_retrieval_confidence(
            "Delvis skyet, 21 grader i Oslo.",
            20,
            4096,
            Evidence {
                tool_successes: 1,
                ..Evidence::default()
            },
            true,
            None,
        )
        .unwrap();
        assert!(score >= 0.75, "tool-backed answer flagged uncertain: {score}");
    }

    /// Helper: a certainty summary for a `token_count`-token answer whose
    /// geometric-mean token probability is `probability`.
    fn certainty(token_count: u32, probability: f64) -> Option<ModelCertainty> {
        // Claim tokens carry the signal the scorer grades on; the whole-answer
        // mean is set deliberately HIGHER so a test that accidentally graded on
        // it would fail loudly rather than pass for the wrong reason.
        //
        // Clamped the way inference-core clamps (MIN_TOKEN_LOGPROB): a measured
        // probability of 0 arrives as a very negative but FINITE logprob, never
        // `-inf`, so the helper cannot manufacture a malformed value that
        // production would never send.
        ModelCertainty::new(
            token_count,
            (0.99_f64).ln(),
            token_count,
            probability.max(f64::MIN_POSITIVE).ln().max(-20.0),
        )
    }

    /// The point of the whole logprob path: a fact the model knows cold must
    /// not read as a coin flip just because our knowledge base has nothing
    /// about it. "Oslo." with no evidence scored BASE (0.72) — under the UI's
    /// 0.75 threshold, so it rendered "Usikkert svar". Near-certain tokens
    /// carry it over.
    #[test]
    fn a_fact_the_model_knows_cold_clears_the_threshold_without_evidence() {
        let score = score_with_certainty(
            "Oslo.",
            3,
            4096,
            Evidence::default(),
            certainty(3, 0.985),
        )
        .unwrap();
        assert!(score >= 0.75, "near-certain answer still flagged: {score}");
        assert!(score > BASE);
    }

    /// A fact the model knows cold floors at 0.90 even with nothing citable
    /// behind it: in the near-certain band the model's own signal outweighs the
    /// absence of a citation, and reading 72% on "17 ganger 24 er 408." teaches
    /// users to ignore the number.
    #[test]
    fn a_near_certain_answer_floors_above_the_threshold() {
        let score = score_with_certainty(
            "17 ganger 24 er 408.",
            10,
            4096,
            Evidence::default(),
            certainty(10, 0.999),
        )
        .unwrap();
        assert!(score >= NEAR_CERTAIN_FLOOR, "score = {score}");

        // Just below the band, the ordinary ladder applies and nothing floors.
        let below = score_with_certainty(
            "17 ganger 24 er 408.",
            10,
            4096,
            Evidence::default(),
            certainty(10, 0.90),
        )
        .unwrap();
        assert!(below < NEAR_CERTAIN_FLOOR, "score = {below}");

        // A hedged answer is not floored by how fluently it hedged, and Data
        // Plane's weak-retrieval verdict still outranks the floor.
        let hedged = score_with_certainty(
            "Jeg vet ikke.",
            8,
            4096,
            Evidence::default(),
            certainty(8, 0.999),
        )
        .unwrap();
        assert!(hedged < 0.75, "hedged answer floored to {hedged}");
        let weak_retrieval = score_with_retrieval_confidence(
            "Retningslinjen sier at refusjon aksepteres.",
            30,
            4096,
            Evidence {
                kb_citations: 3,
                ..Evidence::default()
            },
            true,
            certainty(30, 0.999),
        )
        .unwrap();
        assert!(weak_retrieval <= LOW_CONFIDENCE_RETRIEVAL_CEIL);
    }

    /// The opposite case, and the reason this is worth carrying: an answer the
    /// model produced token by token with real alternatives at every step is
    /// LESS trustworthy than its confident phrasing suggests, and nothing in
    /// the text says so.
    #[test]
    fn a_token_by_token_guess_is_debited_below_base() {
        let guess = score_with_certainty(
            "Omsetningen var 4,2 millioner kroner.",
            30,
            4096,
            Evidence::default(),
            certainty(30, 0.15),
        )
        .unwrap();
        assert!(guess < BASE, "guessed answer not debited: {guess}");
        assert!(guess < 0.75);
    }

    /// The calibration guard, pinned to measured fabrications: an invented
    /// capital (claim 0.111), a fabricated founding year (0.220) and a
    /// fabricated head-count (0.000). Each must be debited BELOW an ordinary
    /// unbacked answer, not merely left at it — the whole reason to grade on
    /// claim tokens is that these used to read 0.63–0.78 and pass for prose.
    #[test]
    fn measured_fabrications_are_debited_below_an_unbacked_answer() {
        for probability in [0.000_f64, 0.111, 0.220] {
            let score = score_with_certainty(
                "Aquatiq AS ble grunnlagt i 2005.",
                12,
                4096,
                Evidence::default(),
                certainty(12, probability),
            )
            .unwrap();
            assert!(
                score < BASE,
                "fabrication at claim p={probability} scored {score}, not below BASE"
            );
            assert!(score < 0.75);
        }
    }

    /// The other side of the same gap: correct content sits at 0.60–1.00 on the
    /// claim statistic and must never be debited for it. Ordinary prose (0.600)
    /// and a correct-but-less-canonical answer (0.728, "Helsingfors") stay
    /// neutral; a known fact (0.999) earns the floor.
    #[test]
    fn measured_correct_answers_are_never_debited() {
        for probability in [0.600_f64, 0.728] {
            let score = score_with_certainty(
                "Sporing av pakker gir kunden oversikt.",
                60,
                4096,
                Evidence::default(),
                certainty(60, probability),
            )
            .unwrap();
            assert!(
                (score - BASE).abs() < 1e-9,
                "correct content at claim p={probability} was moved to {score}"
            );
        }
        let known = score_with_certainty(
            "Hovedstaden i Portugal er Lisboa.",
            9,
            4096,
            Evidence::default(),
            certainty(9, 0.999),
        )
        .unwrap();
        assert!(known >= NEAR_CERTAIN_FLOOR, "known fact scored {known}");
    }

    /// Mean token probability falls with length and stylistic freedom, so
    /// ordinary well-sourced prose sits in a wide neutral band: the model's
    /// certainty must not quietly penalize the answers that carry the most
    /// evidence.
    #[test]
    fn ordinary_prose_certainty_is_neutral() {
        let evidence = Evidence {
            kb_citations: 4,
            ..Evidence::default()
        };
        let with = score_with_certainty(
            "Retningslinjen sier at refusjon gis innen 30 dager.",
            60,
            4096,
            evidence,
            certainty(60, 0.5),
        )
        .unwrap();
        let without = score(
            "Retningslinjen sier at refusjon gis innen 30 dager.",
            60,
            4096,
            evidence,
        )
        .unwrap();
        assert!(
            (with - without).abs() < 1e-9,
            "mid-range certainty must not move the score: {with} vs {without}"
        );
    }

    /// A model that has decided to say "I don't know" says it very fluently.
    /// Certainty may lower a hedged or truncated answer, never raise it.
    #[test]
    fn certainty_never_rescues_an_answer_that_doubts_itself() {
        let hedged = score_with_certainty(
            "Jeg vet ikke hva omsetningen var.",
            12,
            4096,
            Evidence::default(),
            certainty(12, 0.97),
        )
        .unwrap();
        assert!(
            hedged < 0.75,
            "fluent hedging must stay flagged as uncertain: {hedged}"
        );
        assert!((hedged - (BASE - HEDGING_PENALTY)).abs() < 1e-9);

        // Truncated mid-thought, same rule.
        let truncated =
            score_with_certainty("Punkt 1 er at", 1024, 1024, Evidence::default(), certainty(1024, 0.97))
                .unwrap();
        assert!((truncated - (BASE - TRUNCATED_PENALTY)).abs() < 1e-9);

        // But a genuinely uncertain hedged answer is still debited twice.
        let both = score_with_certainty(
            "Jeg vet ikke.",
            8,
            4096,
            Evidence::default(),
            certainty(8, 0.10),
        )
        .unwrap();
        assert!(both < hedged, "low certainty must still apply: {both}");
    }

    /// Providers that report no logprobs (Anthropic) must score exactly as
    /// they did before this signal existed.
    #[test]
    fn absent_certainty_leaves_the_heuristic_untouched() {
        let evidence = Evidence {
            tool_successes: 2,
            ..Evidence::default()
        };
        let answer = "Lageret har 12 tomme varer.";
        assert_eq!(
            score_with_certainty(answer, 30, 4096, evidence, None),
            score(answer, 30, 4096, evidence)
        );
    }

    /// A malformed or empty summary is "unknown", never "certain" — a zero
    /// token count or a positive mean logprob is not a probability we can read.
    #[test]
    fn malformed_certainty_is_rejected_at_construction() {
        assert!(ModelCertainty::new(0, -0.01, 0, 0.0).is_none());
        assert!(ModelCertainty::new(5, 0.5, 0, 0.0).is_none());
        assert!(ModelCertainty::new(5, f64::NAN, 0, 0.0).is_none());
        assert!(ModelCertainty::new(5, f64::NEG_INFINITY, 0, 0.0).is_none());
        let ok = ModelCertainty::new(5, -0.02, 2, -0.03).unwrap();
        assert_eq!(ok.token_count(), 5);
        assert_eq!(ok.claim_token_count(), 2);
        assert!(ok.per_token_probability() > 0.96);
    }

    /// The scorer grades on the claim tokens; the whole-answer mean is only a
    /// fallback for an answer that was entirely framing. Getting this backwards
    /// is precisely the failure the claim statistic exists to prevent, so it is
    /// asserted rather than assumed.
    #[test]
    fn the_claim_statistic_is_what_the_scorer_grades_on() {
        // Fluent framing (0.99 whole) around an invented claim (0.11).
        let fabrication =
            ModelCertainty::new(9, (0.99_f64).ln(), 2, (0.11_f64).ln()).unwrap();
        assert!((fabrication.per_token_probability() - 0.11).abs() < 1e-9);
        assert!((fabrication.whole_answer_probability() - 0.99).abs() < 1e-9);
        let scored = score_with_certainty(
            "Hovedstaden i Zubrowka er Krokowa.",
            12,
            4096,
            Evidence::default(),
            Some(fabrication),
        )
        .unwrap();
        assert!(scored < BASE, "an invented claim must be debited: {scored}");

        // No claim tokens (the answer only echoed the question): fall back to
        // the whole-answer mean rather than reading 0 claims as 0 certainty.
        let framing_only = ModelCertainty::new(4, (0.98_f64).ln(), 0, 0.0).unwrap();
        assert!((framing_only.per_token_probability() - 0.98).abs() < 1e-9);
        // Malformed claim fields degrade the same way.
        let broken = ModelCertainty::new(4, (0.98_f64).ln(), 3, f64::NAN).unwrap();
        assert_eq!(broken.claim_token_count(), 0);
        assert!((broken.per_token_probability() - 0.98).abs() < 1e-9);

        // ...but that fallback may not reach the 0.90 floor. The floor is the
        // scorer's strongest statement, and without a measured claim it would
        // rest entirely on framing — which restates the question and is
        // near-certain whatever the answer asserts.
        let floored = score_with_certainty(
            "Oslo.",
            4,
            4096,
            Evidence::default(),
            Some(framing_only),
        )
        .unwrap();
        assert!(
            floored < NEAR_CERTAIN_FLOOR,
            "framing alone must not floor an answer: {floored}"
        );
    }

    /// Our own retrieval verdict outranks the model's fluency: a weak cited
    /// match stays capped however certain the wording was.
    #[test]
    fn weak_retrieval_cap_outranks_model_certainty() {
        let score = score_with_retrieval_confidence(
            "Retningslinjen sier at refusjon aksepteres.",
            30,
            4096,
            Evidence {
                kb_citations: 5,
                ..Evidence::default()
            },
            true,
            certainty(30, 0.99),
        )
        .unwrap();
        assert!(score <= LOW_CONFIDENCE_RETRIEVAL_CEIL);
        assert!(score < 0.75);
    }

    #[test]
    fn stays_within_bounds() {
        let s = score(
            "ok",
            5,
            1024,
            Evidence {
                kb_citations: 50,
                web_citations: 50,
                ..Evidence::default()
            },
        )
        .unwrap();
        assert!((FLOOR..=CEIL).contains(&s));
    }
}
