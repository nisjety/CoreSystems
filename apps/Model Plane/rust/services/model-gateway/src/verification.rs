//! Verification pass for an answer that scored low.
//!
//! A confidence number that only ever describes a turn is a passive label. The
//! turn that scored 0.72 because nothing backed it is exactly the turn where
//! the system should go and look — the org's own knowledge base first, the web
//! only if this turn was already allowed to reach it — and then say what it
//! found. An answer that verifies gains evidence and a higher score honestly;
//! one that does not keeps its caveat, now meaning "we checked and found
//! nothing" rather than "we did not look".
//!
//! Deliberately POST-hoc. The answer is already on the wire by the time this
//! runs, so verification never rewrites what the user read — it attaches the
//! sources it found and corrects the score. Re-generating the answer would
//! double every low-confidence turn's latency and change text under the
//! reader's eyes.
//!
//! The retrieval this performs is not the one the turn already did. The
//! pre-answer lookup searches the org's knowledge with the USER'S QUESTION;
//! this one searches with the ANSWER'S CLAIMS, which is a different query and
//! the only one that can confirm what was actually said.

/// Scores below this go looking for backing.
///
/// Deliberately the same value the UI uses to render its caveat
/// (`LOW_CONFIDENCE_ANSWER_THRESHOLD`): the set of answers the product calls
/// uncertain and the set it tries to verify should be the same set, or the
/// caveat starts appearing on answers nothing ever checked.
pub const VERIFY_BELOW: f64 = 0.75;

/// Longest answer worth a post-hoc check.
///
/// A single 400-character query cannot represent a 500-word analysis, and an
/// answer that long makes many claims rather than one. Verifying those properly
/// is claim extraction plus per-claim retrieval — a different and far more
/// expensive product. Long answers are grounded the right way instead: by the
/// tool loop that runs BEFORE the answer, whose results this scorer already
/// counts as evidence.
pub const MAX_VERIFIABLE_OUTPUT_TOKENS: u32 = 300;

/// Why a turn was, or was not, verified. Carried rather than reduced to a bool
/// so the decision is loggable: "we did not check" and "we checked and found
/// nothing" are different facts about an answer, and an operator reading a low
/// score needs to know which one happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationDecision {
    /// Low-scored, unevidenced, short enough: go look.
    Verify,
    /// The score already clears the bar.
    SkipScoredHighEnough,
    /// Never scored — see [`decide`].
    SkipUnscored,
    /// The turn already gathered evidence of its own.
    SkipAlreadyEvidenced,
    /// Too long for one query to represent.
    SkipAnswerTooLong,
    /// Turned off by the operator.
    SkipDisabled,
    /// This org has used its hourly allowance; the answer ships unverified
    /// rather than the plane overspending on it.
    SkipBudgetExhausted,
}

impl VerificationDecision {
    #[must_use]
    pub fn should_verify(self) -> bool {
        matches!(self, Self::Verify)
    }

    /// Stable label for logs and metrics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verify => "verify",
            Self::SkipScoredHighEnough => "skip:scored_high_enough",
            Self::SkipUnscored => "skip:unscored",
            Self::SkipAlreadyEvidenced => "skip:already_evidenced",
            Self::SkipAnswerTooLong => "skip:answer_too_long",
            Self::SkipDisabled => "skip:disabled",
            Self::SkipBudgetExhausted => "skip:budget_exhausted",
        }
    }
}

/// Operator kill-switch. `MODEL_GATEWAY_VERIFICATION=off` disables the pass
/// without a rebuild — the same escape hatch the logprobs request parameter
/// has, and for the same reason: this spends time and (for entailment) tokens
/// on someone else's traffic, so it must be stoppable from the outside.
#[must_use]
pub fn verification_enabled() -> bool {
    !matches!(
        std::env::var("MODEL_GATEWAY_VERIFICATION")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no" | "disabled"
    )
}

/// The full gate. Every condition here is a reason NOT to spend the round.
///
/// `already_evidenced` is the one that keeps complex work cheap: a turn that
/// ran tools or carried citations has real backing, scores well above the
/// threshold on that alone, and would never reach this gate — but when a
/// tool-heavy turn DOES score low (failed tools, say), re-querying will not
/// help, because the evidence problem was not a missing lookup.
#[must_use]
pub fn decide(
    confidence: Option<f64>,
    evidence: crate::confidence::Evidence,
    output_tokens: u32,
) -> VerificationDecision {
    if !verification_enabled() {
        return VerificationDecision::SkipDisabled;
    }
    let Some(confidence) = confidence else {
        return VerificationDecision::SkipUnscored;
    };
    if confidence >= VERIFY_BELOW {
        return VerificationDecision::SkipScoredHighEnough;
    }
    if evidence.citable_units() > 0 {
        return VerificationDecision::SkipAlreadyEvidenced;
    }
    if output_tokens > MAX_VERIFIABLE_OUTPUT_TOKENS {
        return VerificationDecision::SkipAnswerTooLong;
    }
    VerificationDecision::Verify
}

/// Longest query sent to retrieval. A whole essay embeds to a vague centroid
/// that matches everything weakly; the opening claims carry the answer's
/// substance.
const MAX_QUERY_CHARS: usize = 400;

/// Below this an answer is too short to be its own search query ("4 😊",
/// "Oslo."): there is nothing to match on, so the user's question is the better
/// probe of whether the org's documents speak to the topic at all.
const MIN_ANSWER_QUERY_CHARS: usize = 24;

/// Whether an answer scored low enough to verify. `None` (unscored) does not
/// verify: an answer with no score is one the pipeline could not assess, and
/// spending a retrieval round on it would be guessing about a guess.
#[must_use]
pub fn should_verify(confidence: Option<f64>) -> bool {
    confidence.is_some_and(|value| value < VERIFY_BELOW)
}

/// The query that asks "do our documents support what was just said?".
///
/// Returns `None` when there is nothing worth asking — an empty answer, or one
/// so short that neither it nor the question carries a searchable claim.
#[must_use]
pub fn verification_query(question: &str, answer: &str) -> Option<String> {
    let answer = answer.trim();
    let question = question.trim();
    let source = if answer.chars().count() >= MIN_ANSWER_QUERY_CHARS {
        answer
    } else if !question.is_empty() {
        question
    } else if answer.is_empty() {
        return None;
    } else {
        answer
    };
    let truncated: String = source.chars().take(MAX_QUERY_CHARS).collect();
    let truncated = truncated.trim();
    if truncated.is_empty() {
        return None;
    }
    Some(truncated.to_owned())
}

/// Whether the verification pass may escalate past the knowledge base.
///
/// The web is only reached when THIS turn already had `web_search` available —
/// the user's own Search toggle. A low score is a reason to look harder in what
/// the org already owns; it is not consent to send the answer's contents to an
/// external search engine on a turn where the user did not ask for that.
#[must_use]
pub fn may_escalate_to_web(tool_names: impl IntoIterator<Item = impl AsRef<str>>) -> bool {
    tool_names
        .into_iter()
        .any(|name| name.as_ref() == "web_search")
}

/// What the retrieved sources actually say about the answer.
///
/// The reason this exists: counting citations without reading them lets
/// verification confirm anything. Searching for an invented capital returns
/// pages — pages explaining that the country is fictional — and by volume alone
/// those lifted the score exactly like real support would. "Found sources" and
/// "confirmed" are different claims, and only one of them belongs in a number
/// the user is asked to trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceVerdict {
    /// The sources back what the answer said.
    Supports,
    /// The sources say something materially different.
    Contradicts,
    /// The sources are about something else; they neither back nor refute it.
    Unrelated,
}

impl SourceVerdict {
    /// Whether the sources may be counted as evidence FOR the answer.
    #[must_use]
    pub fn is_support(self) -> bool {
        matches!(self, Self::Supports)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::Contradicts => "contradicts",
            Self::Unrelated => "unrelated",
        }
    }
}

/// Score debit when the sources disagree with the answer.
///
/// Contradicted is strictly worse than unverified: we looked, we found relevant
/// material, and it says otherwise. Sized to push a typical unbacked answer
/// (0.72) well clear of the caveat threshold rather than merely under it.
pub const CONTRADICTED_PENALTY: f64 = 0.20;

/// Snippets shown to the judge. Enough to decide, few enough to stay cheap.
const MAX_JUDGED_SNIPPETS: usize = 5;
const MAX_SNIPPET_CHARS: usize = 500;

/// Ask a small model whether the sources back the answer.
///
/// One word out, so the call is bounded and the parse is trivial. The framing
/// is deliberately narrow — "do these sources support this claim" rather than
/// "is this true" — because the second question invites the judge to answer
/// from its own knowledge, which is the thing being checked.
#[must_use]
pub fn entailment_prompt(question: &str, answer: &str, snippets: &[String]) -> String {
    let mut prompt = String::from(
        "You judge whether SOURCES support a claim. Do NOT use your own knowledge; \
         judge only from the sources below.\n\n\
         Answer with exactly one word:\n\
         SUPPORTS - the sources state or clearly imply the claim\n\
         CONTRADICTS - the sources state something materially different\n\
         UNRELATED - the sources do not address the claim\n\n",
    );
    prompt.push_str("QUESTION: ");
    prompt.push_str(question.chars().take(500).collect::<String>().trim());
    prompt.push_str("\n\nCLAIM: ");
    prompt.push_str(answer.chars().take(1500).collect::<String>().trim());
    prompt.push_str("\n\nSOURCES:\n");
    for (index, snippet) in snippets.iter().take(MAX_JUDGED_SNIPPETS).enumerate() {
        use std::fmt::Write as _;
        let _ = writeln!(
            prompt,
            "[{}] {}",
            index + 1,
            snippet
                .chars()
                .take(MAX_SNIPPET_CHARS)
                .collect::<String>()
                .trim()
        );
    }
    prompt.push_str("\nOne word:");
    prompt
}

/// Read the judge's answer. Anything unrecognized is [`SourceVerdict::Unrelated`]
/// — the neutral verdict — so a confused judge can never lift a score, and a
/// failed call is indistinguishable from "did not confirm".
#[must_use]
pub fn parse_verdict(raw: &str) -> SourceVerdict {
    let normalized = raw.trim().to_ascii_lowercase();
    // Take the first recognizable word: models like to add a period, quotes, or
    // a short justification despite the instruction.
    let head = normalized
        .split(|c: char| !c.is_ascii_alphabetic())
        .find(|word| !word.is_empty())
        .unwrap_or("");
    match head {
        "supports" | "support" | "supported" => SourceVerdict::Supports,
        "contradicts" | "contradict" | "contradicted" => SourceVerdict::Contradicts,
        _ => SourceVerdict::Unrelated,
    }
}

/// Longest answer worth re-sampling. Two extra samples of "Lisboa." cost
/// nothing; two extra samples of a 300-token analysis triple the turn, and
/// prose legitimately varies in wording anyway, so agreement would be
/// meaningless there.
pub const MAX_RESAMPLED_OUTPUT_TOKENS: u32 = 30;

/// How many extra samples to draw. Two is enough to see instability — a
/// fabricated year came back 2000, then 2020, then 2005 across draws — and
/// keeps the cost at two short completions.
pub const SELF_CONSISTENCY_SAMPLES: usize = 2;

/// Debit for an answer the model does not reproduce.
///
/// Truth is stable across samples and invention is not, so disagreement is
/// evidence about the answer that no amount of retrieval can supply — it is the
/// signal available exactly when the knowledge base and the web had nothing to
/// say.
pub const INCONSISTENT_PENALTY: f64 = 0.12;

/// Self-consistency is the last resort, not the first. It only runs when the
/// sources came back with nothing (`Unrelated`) — a confirmed or contradicted
/// answer has already been judged on evidence, which is better — and only on
/// answers short enough for agreement to mean anything.
#[must_use]
pub fn self_consistency_applies(output_tokens: u32, verdict: SourceVerdict) -> bool {
    self_consistency_enabled()
        && verdict == SourceVerdict::Unrelated
        && output_tokens > 0
        && output_tokens <= MAX_RESAMPLED_OUTPUT_TOKENS
}

/// Operator kill-switch, separate from the verification one: this is the only
/// part of the pass that spends completion tokens on the ANSWER's model rather
/// than on a small judge, so an operator may want it off while keeping the
/// rest.
#[must_use]
pub fn self_consistency_enabled() -> bool {
    !matches!(
        std::env::var("MODEL_GATEWAY_SELF_CONSISTENCY")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no" | "disabled"
    )
}

/// The words an answer contributes that the question did not — the same idea as
/// the claim tokens the certainty statistic grades on, at word granularity.
///
/// Comparing whole strings would call "Oslo." and "Hovedstaden i Norge er
/// Oslo." different answers; comparing what each ASSERTS gets both to {oslo}.
#[must_use]
pub fn claim_words(text: &str, question: &str) -> std::collections::BTreeSet<String> {
    let question = question.to_lowercase();
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| word.len() > 1)
        .filter(|word| !question.contains(*word))
        .map(str::to_owned)
        .collect()
}

/// Whether re-sampling reproduced the same claim.
///
/// Agreement is an OVERLAP of claim words, not equality: the model rephrases
/// freely and that is not disagreement. When either side asserts nothing the
/// comparison cannot decide, and undecided means consistent — this may only
/// debit on positive evidence of instability.
#[must_use]
pub fn samples_agree(original: &str, samples: &[String], question: &str) -> bool {
    let original_words = claim_words(original, question);
    if original_words.is_empty() {
        return true;
    }
    samples.iter().all(|sample| {
        let sample_words = claim_words(sample, question);
        sample_words.is_empty() || sample_words.intersection(&original_words).next().is_some()
    })
}

/// Per-org ceiling on verification passes, in a rolling hour.
///
/// The pass spends a retrieval round and (for entailment) model tokens on
/// traffic the operator did not individually approve. Without a cap, one busy
/// org's low-scoring turns become everyone's latency and bill. Over the cap the
/// turn degrades to no verification — the answer still ships with its honest
/// unverified score — rather than to an overrun.
pub struct VerificationBudget {
    per_org_hourly: u32,
    windows: std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, u32)>>,
}

/// Default ceiling. Generous for an interactive org (a verified turn is a
/// low-scoring one, not the common case) and small enough that a runaway
/// integration cannot spend unboundedly.
const DEFAULT_PER_ORG_HOURLY: u32 = 120;

impl VerificationBudget {
    #[must_use]
    pub fn new(per_org_hourly: u32) -> Self {
        Self {
            per_org_hourly,
            windows: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Build from `MODEL_GATEWAY_VERIFICATION_MAX_PER_ORG_HOURLY`.
    #[must_use]
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("MODEL_GATEWAY_VERIFICATION_MAX_PER_ORG_HOURLY")
                .ok()
                .and_then(|value| value.trim().parse::<u32>().ok())
                .unwrap_or(DEFAULT_PER_ORG_HOURLY),
        )
    }

    /// The process-wide budget.
    pub fn global() -> &'static Self {
        static GLOBAL: std::sync::OnceLock<VerificationBudget> = std::sync::OnceLock::new();
        GLOBAL.get_or_init(Self::from_env)
    }

    /// Claim one verification for `org_id`, or refuse when the org is over its
    /// ceiling. Claiming is what counts — a pass that then finds nothing still
    /// spent the round.
    pub fn try_claim(&self, org_id: &str) -> bool {
        self.try_claim_at(org_id, std::time::Instant::now())
    }

    fn try_claim_at(&self, org_id: &str, now: std::time::Instant) -> bool {
        if self.per_org_hourly == 0 {
            return false;
        }
        let Ok(mut windows) = self.windows.lock() else {
            // A poisoned lock must not become an unbounded spend.
            return false;
        };
        let hour = std::time::Duration::from_hours(1);
        let entry = windows
            .entry(org_id.to_owned())
            .or_insert((now, 0));
        if now.duration_since(entry.0) >= hour {
            *entry = (now, 0);
        }
        if entry.1 >= self.per_org_hourly {
            return false;
        }
        entry.1 += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_judge_is_asked_about_the_sources_not_about_the_world() {
        let prompt = entailment_prompt(
            "Når ble selskapet grunnlagt?",
            "Aquatiq AS ble grunnlagt i 2005.",
            &["Aquatiq ble etablert i 1992.".to_owned()],
        );
        assert!(prompt.contains("Do NOT use your own knowledge"));
        assert!(prompt.contains("grunnlagt i 2005"), "claim missing");
        assert!(prompt.contains("etablert i 1992"), "source missing");
    }

    #[test]
    fn only_the_first_five_snippets_are_judged() {
        let snippets: Vec<String> = (0..9).map(|i| format!("kilde-nummer-{i}")).collect();
        let prompt = entailment_prompt("q", "a", &snippets);
        assert!(prompt.contains("kilde-nummer-4"));
        assert!(!prompt.contains("kilde-nummer-5"), "snippet cap not applied");
    }

    #[test]
    fn verdicts_parse_through_the_usual_model_noise() {
        assert_eq!(parse_verdict("SUPPORTS"), SourceVerdict::Supports);
        assert_eq!(parse_verdict(" supports.\n"), SourceVerdict::Supports);
        assert_eq!(parse_verdict("\"Supported\""), SourceVerdict::Supports);
        assert_eq!(parse_verdict("CONTRADICTS"), SourceVerdict::Contradicts);
        assert_eq!(
            parse_verdict("Contradicts — the sources say 1992."),
            SourceVerdict::Contradicts
        );
        assert_eq!(parse_verdict("UNRELATED"), SourceVerdict::Unrelated);
    }

    /// A judge that answers something unexpected must never be able to lift a
    /// score. Unrecognized is neutral, never support.
    #[test]
    fn an_unrecognizable_verdict_is_neutral() {
        for raw in ["", "   ", "maybe?", "ja", "42", "I think it is correct"] {
            let verdict = parse_verdict(raw);
            assert_eq!(verdict, SourceVerdict::Unrelated, "{raw:?}");
            assert!(!verdict.is_support());
        }
    }

    #[test]
    fn only_supporting_sources_count_as_evidence() {
        assert!(SourceVerdict::Supports.is_support());
        assert!(!SourceVerdict::Contradicts.is_support());
        assert!(!SourceVerdict::Unrelated.is_support());
    }

    /// The measured instability this exists to catch: the same question
    /// produced 2000, then 2020, then 2005 across draws.
    #[test]
    fn a_fabricated_fact_does_not_reproduce() {
        let question = "Naar ble Aquatiq AS grunnlagt?";
        assert!(!samples_agree(
            "Aquatiq AS ble grunnlagt i 2005.",
            &[
                "Aquatiq AS ble grunnlagt i 2000.".to_owned(),
                "Selskapet ble grunnlagt i 2020.".to_owned(),
            ],
            question
        ));
    }

    /// Rephrasing is not disagreement. A real fact survives being said
    /// differently, and calling that instability would debit correct answers.
    #[test]
    fn rephrasing_the_same_fact_still_agrees() {
        let question = "Hva er hovedstaden i Norge?";
        assert!(samples_agree(
            "Oslo.",
            &[
                "Hovedstaden i Norge er Oslo.".to_owned(),
                "Det er Oslo!".to_owned(),
            ],
            question
        ));
    }

    #[test]
    fn claim_words_drop_the_question_and_the_noise() {
        let words = claim_words(
            "Hovedstaden i Norge er Oslo.",
            "Hva er hovedstaden i Norge? Svar kort.",
        );
        assert_eq!(words.into_iter().collect::<Vec<_>>(), vec!["oslo"]);
    }

    /// An answer that asserts nothing beyond the question cannot be judged by
    /// resampling; undecided must mean consistent, never a debit.
    #[test]
    fn an_answer_with_no_claim_words_is_never_debited() {
        // Everything the answer says is already in the question.
        assert!(samples_agree(
            "Oslo.",
            &["Ja, Oslo.".to_owned()],
            "Er hovedstaden i Norge Oslo?"
        ));
        // A sample that came back empty decides nothing either.
        assert!(samples_agree("Lisboa.", &[String::new()], "Hovedstaden?"));
    }

    /// ...but a model that flips its answer across draws IS unstable, and that
    /// is the case worth debiting even though each answer is a single word.
    #[test]
    fn flipping_the_answer_across_draws_is_disagreement() {
        assert!(!samples_agree(
            "Ja.",
            &["Nei.".to_owned()],
            "Leverer selskapet kjemi?"
        ));
    }

    /// Last resort, tightly gated: only when the sources found nothing, and
    /// only on answers short enough for agreement to mean anything.
    #[test]
    fn resampling_is_gated_to_short_unresolved_answers() {
        assert!(self_consistency_applies(12, SourceVerdict::Unrelated));
        assert!(!self_consistency_applies(12, SourceVerdict::Supports));
        assert!(!self_consistency_applies(12, SourceVerdict::Contradicts));
        assert!(!self_consistency_applies(
            MAX_RESAMPLED_OUTPUT_TOKENS + 1,
            SourceVerdict::Unrelated
        ));
        assert!(!self_consistency_applies(0, SourceVerdict::Unrelated));
        temp_env("MODEL_GATEWAY_SELF_CONSISTENCY", Some("off"), || {
            assert!(!self_consistency_applies(12, SourceVerdict::Unrelated));
        });
    }

    #[test]
    fn an_org_is_cut_off_at_its_hourly_ceiling() {
        let budget = VerificationBudget::new(3);
        let start = std::time::Instant::now();
        for attempt in 0..3 {
            assert!(budget.try_claim_at("org-1", start), "claim {attempt} refused");
        }
        assert!(!budget.try_claim_at("org-1", start), "ceiling not enforced");
        // A different org has its own ceiling — one busy tenant does not
        // silence verification for everyone.
        assert!(budget.try_claim_at("org-2", start));
        // ...and the window rolls.
        let next_hour = start + std::time::Duration::from_secs(3601);
        assert!(budget.try_claim_at("org-1", next_hour));
    }

    #[test]
    fn a_zero_ceiling_disables_verification_entirely() {
        let budget = VerificationBudget::new(0);
        assert!(!budget.try_claim_at("org-1", std::time::Instant::now()));
    }

    #[test]
    fn only_low_scored_answers_verify() {
        assert!(should_verify(Some(0.72)));
        assert!(should_verify(Some(0.10)));
        assert!(!should_verify(Some(VERIFY_BELOW)));
        assert!(!should_verify(Some(0.88)));
        // Unscored is not low: there is nothing to act on.
        assert!(!should_verify(None));
    }

    fn unevidenced() -> crate::confidence::Evidence {
        crate::confidence::Evidence::default()
    }

    #[test]
    fn a_low_scored_short_unevidenced_answer_is_verified() {
        assert_eq!(decide(Some(0.72), unevidenced(), 20), VerificationDecision::Verify);
        assert!(decide(Some(0.72), unevidenced(), 20).should_verify());
    }

    /// The guardrail that keeps complex work cheap. A turn that ran tools or
    /// carried citations has real backing; re-querying cannot add anything the
    /// turn did not already try, so it never pays for the round.
    #[test]
    fn a_turn_that_already_gathered_evidence_is_never_verified() {
        for evidence in [
            crate::confidence::Evidence { tool_successes: 1, ..unevidenced() },
            crate::confidence::Evidence { kb_citations: 2, ..unevidenced() },
            crate::confidence::Evidence { web_citations: 1, ..unevidenced() },
            // Failed tools alongside a success still count: the turn reached
            // for evidence and got some.
            crate::confidence::Evidence { tool_successes: 1, tool_failures: 3, ..unevidenced() },
        ] {
            assert_eq!(
                decide(Some(0.40), evidence, 20),
                VerificationDecision::SkipAlreadyEvidenced,
                "{evidence:?} should have skipped"
            );
        }
    }

    /// ...but assembly grounding is NOT evidence for this purpose. It is prompt
    /// context, true on nearly every turn, and counting it here would silently
    /// disable the whole pass.
    #[test]
    fn assembly_grounding_alone_does_not_count_as_evidence() {
        let assembly_only = crate::confidence::Evidence {
            assembly_grounded: true,
            ..unevidenced()
        };
        assert_eq!(
            decide(Some(0.61), assembly_only, 20),
            VerificationDecision::Verify
        );
    }

    /// One 400-character query cannot represent a long analysis, and a long
    /// answer makes many claims rather than one. Those are grounded by the tool
    /// loop before the answer, not by a post-hoc check.
    #[test]
    fn a_long_answer_is_not_verified_post_hoc() {
        assert_eq!(
            decide(Some(0.72), unevidenced(), MAX_VERIFIABLE_OUTPUT_TOKENS),
            VerificationDecision::Verify
        );
        assert_eq!(
            decide(Some(0.72), unevidenced(), MAX_VERIFIABLE_OUTPUT_TOKENS + 1),
            VerificationDecision::SkipAnswerTooLong
        );
    }

    #[test]
    fn a_good_score_and_an_unscored_turn_both_skip() {
        assert_eq!(
            decide(Some(0.75), unevidenced(), 20),
            VerificationDecision::SkipScoredHighEnough
        );
        assert_eq!(
            decide(None, unevidenced(), 20),
            VerificationDecision::SkipUnscored
        );
    }

    /// Ordering matters for the log: an operator who turned the pass off should
    /// see "disabled", not "scored high enough".
    #[test]
    fn the_kill_switch_outranks_every_other_reason() {
        temp_env("MODEL_GATEWAY_VERIFICATION", Some("off"), || {
            assert_eq!(
                decide(Some(0.72), unevidenced(), 20),
                VerificationDecision::SkipDisabled
            );
            assert!(!verification_enabled());
        });
        for value in ["", "1", "on", "true", "anything-else"] {
            temp_env("MODEL_GATEWAY_VERIFICATION", Some(value), || {
                assert!(verification_enabled(), "{value} should leave it enabled");
            });
        }
    }

    /// `set_var`/`remove_var` are process-global; keep the mutation inside one
    /// helper so a test cannot leak a value into its neighbours.
    fn temp_env(key: &str, value: Option<&str>, body: impl FnOnce()) {
        let previous = std::env::var(key).ok();
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        body();
        match previous {
            Some(previous) => std::env::set_var(key, previous),
            None => std::env::remove_var(key),
        }
    }

    /// The whole point of the pass: search with what was ASSERTED, not with
    /// what was asked. The pre-answer lookup already tried the question and
    /// came back empty — that is why the score was low.
    #[test]
    fn the_query_is_built_from_the_answers_claims() {
        let query = verification_query(
            "Når ble selskapet grunnlagt?",
            "Aquatiq AS ble grunnlagt i 2000 og holder til i Oslo.",
        )
        .unwrap();
        assert!(query.contains("grunnlagt i 2000"), "query = {query}");
        assert!(!query.contains("Når ble selskapet"), "query = {query}");
    }

    /// A two-word answer is not a search query. Fall back to the question,
    /// which at least probes whether the org has documents on the topic.
    #[test]
    fn a_too_short_answer_falls_back_to_the_question() {
        let query = verification_query("Hva er hovedstaden i Norge?", "Oslo.").unwrap();
        assert_eq!(query, "Hva er hovedstaden i Norge?");
        assert_eq!(
            verification_query("Hvor mange ansatte?", "4 😊").unwrap(),
            "Hvor mange ansatte?"
        );
    }

    #[test]
    fn nothing_to_search_yields_no_query() {
        assert!(verification_query("", "").is_none());
        assert!(verification_query("   ", "  \n ").is_none());
        // A short answer with no question still beats searching nothing.
        assert_eq!(verification_query("", "Oslo.").unwrap(), "Oslo.");
    }

    /// Truncation must not split a multi-byte character, and must not run away
    /// on a long answer.
    #[test]
    fn a_long_answer_is_truncated_on_a_character_boundary() {
        let answer = "æøå ".repeat(400);
        let query = verification_query("q", &answer).unwrap();
        assert!(query.chars().count() <= MAX_QUERY_CHARS);
        assert!(query.starts_with("æøå"));
    }

    #[test]
    fn web_escalation_needs_the_turn_to_already_have_web_search() {
        assert!(may_escalate_to_web(["knowledge_search", "web_search"]));
        assert!(!may_escalate_to_web(["knowledge_search", "fetch_url"]));
        assert!(!may_escalate_to_web(Vec::<String>::new()));
    }
}
