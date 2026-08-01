//! Detects that a user was unhappy with an answer, without them saying so.
//!
//! Nothing in the system noticed re-asking, regenerating, editing-and-resubmitting
//! or "det er feil". The only negative signal was an explicit thumbs-down, which
//! almost nobody clicks — so the promotion loop learned from a tiny, self-selected
//! slice of turns.
//!
//! # Evidence, not truth
//!
//! Every signal here is WEAK. A regenerate can mean "give me another style"; a
//! near-duplicate question can mean the user thought of a better phrasing; "nei"
//! can be answering a question the assistant asked. So this module reports a
//! [`Signal`] with a KIND and a STRENGTH and never a verdict, and the consumer
//! weights implicit evidence far below an explicit rating (see
//! orchestrator-core's feedback scoring).
//!
//! # Why precision matters more than recall
//!
//! A false positive teaches the promotion loop that a good skill is bad, and it
//! does so silently. A false negative merely leaves the loop as blind as it was
//! before this module existed. So every rule here errs toward not firing:
//! correction phrases must be standalone rather than merely present, and
//! near-duplicate detection needs both a short window and a high similarity.
//!
//! The Norwegian substring trap is the specific thing being avoided. `feil`
//! appears inside `feilmelding` (error message) — a user asking *about* an error
//! is not saying the answer is wrong — and `nei` appears inside `neide`,
//! `neitakk`, and the surname `Neis`. Word-boundary matching with an inflection
//! allowlist, the same approach the web-search staleness gate uses, is the
//! minimum bar.

use std::time::Duration;

/// How the user signalled dissatisfaction.
///
/// Kept as distinct variants rather than one boolean because the consumer weights
/// them differently: an explicit "det er feil" is much stronger evidence than a
/// regenerate, which is a normal way to ask for variety.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    /// The user asked for the same answer again.
    Regenerate,
    /// The user edited the previous question and resubmitted it.
    EditResubmit,
    /// The user re-asked something very close to the previous question, soon.
    NearDuplicate,
    /// The user said the answer was wrong.
    Correction,
}

impl SignalKind {
    /// Stable token for the wire and for metrics. Never derived from the enum
    /// name, so renaming a variant cannot silently change a published value.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Regenerate => "regenerate",
            Self::EditResubmit => "edit_resubmit",
            Self::NearDuplicate => "near_duplicate",
            Self::Correction => "correction",
        }
    }

    /// Relative evidence weight, 0..=1, within the implicit family.
    ///
    /// A stated correction is near-certain; a regenerate is barely evidence at
    /// all. These are multiplied by the consumer's own implicit-vs-explicit
    /// weight, so the absolute penalty stays small even for a correction.
    #[must_use]
    pub fn strength(self) -> f32 {
        match self {
            Self::Correction => 1.0,
            Self::EditResubmit => 0.6,
            Self::NearDuplicate => 0.5,
            Self::Regenerate => 0.3,
        }
    }
}

/// One detected signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signal {
    pub kind: SignalKind,
    /// [`SignalKind::strength`], carried so a consumer never has to re-derive it.
    pub strength: f32,
}

impl Signal {
    fn of(kind: SignalKind) -> Self {
        Self { kind, strength: kind.strength() }
    }
}

/// How soon a re-asked question still counts as dissatisfaction with the previous
/// answer.
///
/// Three minutes: long enough to cover reading an answer and deciding it missed,
/// short enough that returning to a topic later in a working session is not read
/// as a complaint. A user who re-asks something an hour later is usually
/// continuing work, not objecting.
pub const NEAR_DUPLICATE_WINDOW: Duration = Duration::from_secs(180);

/// Token-overlap ratio above which two questions count as near-duplicates.
///
/// 0.8 is deliberately high. At 0.6 two different questions about the same
/// invoice look identical; the cost of that is teaching the loop that a correct
/// answer was wrong.
const NEAR_DUPLICATE_SIMILARITY: f32 = 0.8;

/// Minimum tokens before similarity is even considered.
///
/// Short messages collide trivially — "og?" vs "hva?" share little, but "takk"
/// vs "takk!" share everything — and a two-word follow-up is normal conversation
/// rather than a re-ask.
const MIN_TOKENS_FOR_SIMILARITY: usize = 4;

/// Standalone correction phrases, Norwegian first.
///
/// Matched only at the START of a message (after trimming), never anywhere
/// inside it. "det er feil" as the opening of a turn is a correction; the same
/// words inside "hvorfor sier den at det er feil i fakturaen?" are a question
/// about the data. Anchoring is what separates the two.
const CORRECTION_OPENERS: &[&str] = &[
    // Norwegian
    "det er feil",
    "det er galt",
    "dette er feil",
    "dette er galt",
    "nei det er feil",
    "feil svar",
    "det stemmer ikke",
    "dette stemmer ikke",
    "ikke riktig",
    "du tar feil",
    // English
    "that's wrong",
    "thats wrong",
    "that is wrong",
    "this is wrong",
    "wrong answer",
    "that's incorrect",
    "that is incorrect",
    "not correct",
    "you're wrong",
    "youre wrong",
];

/// Bare negations that only count as a correction when they are the WHOLE
/// message.
///
/// "nei" alone, right after an answer, is a rejection. "nei, men kan du også …"
/// is conversation, and "nei takk" is politeness. Requiring the entire message
/// keeps the first and excludes the rest without guessing.
const BARE_NEGATIONS: &[&str] = &["nei", "no", "feil", "galt", "wrong", "nope", "nei.", "no."];

/// What the caller knows about the turn being classified.
#[derive(Debug, Clone, Default)]
pub struct TurnContext<'a> {
    /// The message the user just sent.
    pub message: &'a str,
    /// The user's PREVIOUS message in the same thread, if any.
    pub previous_message: Option<&'a str>,
    /// How long ago that previous message was sent.
    pub since_previous: Option<Duration>,
    /// True when the client asked to regenerate the last answer rather than
    /// sending a new message.
    pub regenerated: bool,
    /// True when the client resubmitted an EDITED version of the previous
    /// question (the composer's edit-and-resend path), which it knows and the
    /// server cannot infer.
    pub edited_resubmit: bool,
}

/// Classify one turn, returning every signal it carries.
///
/// Returns all matches rather than the strongest, so a turn that both regenerates
/// and says "det er feil" is recorded as both — the consumer decides how to
/// combine them, and collapsing here would throw away the distinction between
/// "unhappy once" and "unhappy in two ways".
#[must_use]
pub fn classify(context: &TurnContext<'_>) -> Vec<Signal> {
    let mut signals = Vec::new();

    // A client-declared action supersedes the inference of the same thing. Both
    // a regenerate and an edited resubmit send back a near-identical question,
    // so `is_near_duplicate` fires on them by construction — counting both
    // would charge one click twice (0.3 + 0.5 instead of 0.3), and at
    // ImplicitWeight 0.25 that is 0.2 of a weighted sample against a demotion
    // budget of 3.0, so ~15 regenerates could quarantine a skill instead of ~40.
    //
    // The near-duplicate detector exists to catch the user who re-asks by
    // retyping, which is exactly the case where no explicit flag arrives.
    let declared = context.regenerated || context.edited_resubmit;
    if context.regenerated {
        signals.push(Signal::of(SignalKind::Regenerate));
    }
    if context.edited_resubmit {
        signals.push(Signal::of(SignalKind::EditResubmit));
    }
    if is_correction(context.message) {
        signals.push(Signal::of(SignalKind::Correction));
    }
    if !declared && is_near_duplicate(context) {
        signals.push(Signal::of(SignalKind::NearDuplicate));
    }
    signals
}

/// Whether `message` states that the previous answer was wrong.
#[must_use]
pub fn is_correction(message: &str) -> bool {
    let normalized = normalize(message);
    if normalized.is_empty() {
        return false;
    }
    // The whole message is a bare rejection.
    if BARE_NEGATIONS.contains(&normalized.as_str()) {
        return true;
    }
    // Or it OPENS with a correction phrase. Anchored, so the same words inside a
    // question about an error do not fire.
    CORRECTION_OPENERS.iter().any(|opener| {
        normalized.strip_prefix(opener).is_some_and(|rest| {
            // A phrase boundary, so "ikke riktig" does not match "ikke riktigt
            // nok av dette" — wait, that IS a correction. The boundary check is
            // about not matching a longer WORD: "feil svar" must not fire on
            // "feil svarprosent".
            rest.is_empty() || rest.starts_with(|c: char| !c.is_alphanumeric())
        })
    })
}

/// Whether the user re-asked essentially the previous question, soon enough that
/// it reads as dissatisfaction rather than a new task.
#[must_use]
pub fn is_near_duplicate(context: &TurnContext<'_>) -> bool {
    let Some(previous) = context.previous_message else {
        return false;
    };
    // No timestamp means we cannot establish the window, and a re-ask outside it
    // is ordinary work. Fail closed.
    let Some(elapsed) = context.since_previous else {
        return false;
    };
    if elapsed > NEAR_DUPLICATE_WINDOW {
        return false;
    }
    let current = tokenize(context.message);
    let earlier = tokenize(previous);
    if current.len() < MIN_TOKENS_FOR_SIMILARITY || earlier.len() < MIN_TOKENS_FOR_SIMILARITY {
        return false;
    }
    similarity(&current, &earlier) >= NEAR_DUPLICATE_SIMILARITY
}

/// Lowercase, trim, and collapse whitespace. No character stripping: removing
/// punctuation would turn "nei, men …" into "nei men …" and lose the boundary the
/// bare-negation rule depends on.
fn normalize(message: &str) -> String {
    message.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split into comparable word tokens, dropping punctuation and one-character
/// noise.
fn tokenize(message: &str) -> Vec<String> {
    message
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.chars().count() > 1)
        .map(str::to_owned)
        .collect()
}

/// Jaccard similarity over token SETS.
///
/// Sets rather than sequences: word order changes constantly between a question
/// and its re-ask ("hva er saldoen for Aquatiq" / "for Aquatiq, hva er saldoen"),
/// and treating those as different would miss the case this exists to catch.
fn similarity(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut union: Vec<&String> = Vec::new();
    let mut intersection = 0usize;
    for token in a.iter().chain(b.iter()) {
        if !union.contains(&token) {
            union.push(token);
            if a.contains(token) && b.contains(token) {
                intersection += 1;
            }
        }
    }
    if union.is_empty() {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)] // token counts are far below f32 precision limits
    let ratio = intersection as f32 / union.len() as f32;
    ratio
}

#[cfg(test)]
mod tests {

    /// One regenerate click must be ONE sample. A regenerate resends the same
    /// question, so the near-duplicate detector fires on it too; counting both
    /// charged a single click 0.3 + 0.5, and at ImplicitWeight 0.25 that is 0.2
    /// of a weighted sample against a demotion budget of 3.0 — making ~15 clicks
    /// enough to quarantine a skill the policy budgeted ~40 for.
    #[test]
    fn a_declared_regenerate_is_not_also_counted_as_a_near_duplicate() {
        let context = TurnContext {
            message: "hva er saldoen paa konto 1920",
            previous_message: Some("hva er saldoen paa konto 1920"),
            since_previous: Some(Duration::from_secs(5)),
            regenerated: true,
            edited_resubmit: false,
        };
        let kinds: Vec<SignalKind> = classify(&context).iter().map(|s| s.kind).collect();
        assert_eq!(kinds, vec![SignalKind::Regenerate], "{kinds:?}");
    }

    /// Same suppression for an edited resubmit, near-identical by definition.
    #[test]
    fn a_declared_edit_resubmit_is_not_also_counted_as_a_near_duplicate() {
        let context = TurnContext {
            message: "hva er saldoen paa konto 1920 i dag",
            previous_message: Some("hva er saldoen paa konto 1920"),
            since_previous: Some(Duration::from_secs(5)),
            regenerated: false,
            edited_resubmit: true,
        };
        let kinds: Vec<SignalKind> = classify(&context).iter().map(|s| s.kind).collect();
        assert_eq!(kinds, vec![SignalKind::EditResubmit], "{kinds:?}");
    }

    /// The detector must still catch the user who re-asks by RETYPING — the case
    /// it exists for, where no flag arrives.
    #[test]
    fn an_undeclared_retype_is_still_a_near_duplicate() {
        let kinds: Vec<SignalKind> =
            classify(&reask("hva er saldoen paa konto 1920", "hva er saldoen paa konto 1920", 5))
                .iter()
                .map(|s| s.kind)
                .collect();
        assert!(kinds.contains(&SignalKind::NearDuplicate), "{kinds:?}");
    }
    use super::*;

    fn ctx<'a>(message: &'a str) -> TurnContext<'a> {
        TurnContext { message, ..TurnContext::default() }
    }

    // ── The Norwegian substring traps ───────────────────────────────────────
    //
    // These are the false positives that would teach the promotion loop a good
    // skill is bad. Each one is a real thing a user types.

    #[test]
    fn a_question_about_an_error_is_not_a_correction() {
        for message in [
            "hvorfor sier den at det er feil i fakturaen?",
            "jeg får en feilmelding, hva betyr den?",
            "kan du forklare feilkoden 500?",
            "hva er feil med denne ordren?",
        ] {
            assert!(!is_correction(message), "{message:?} must not read as a correction");
        }
    }

    #[test]
    fn polite_and_conversational_negations_are_not_corrections() {
        for message in [
            "nei takk",
            "nei, men kan du også sjekke lageret?",
            "no thanks",
            "no, i meant the other one — can you check both?",
        ] {
            assert!(!is_correction(message), "{message:?} must not read as a correction");
        }
    }

    #[test]
    fn a_bare_rejection_is_a_correction() {
        for message in ["nei", "Nei", "  nei  ", "feil", "wrong", "nope", "nei."] {
            assert!(is_correction(message), "{message:?} should read as a correction");
        }
    }

    #[test]
    fn a_stated_correction_is_detected_in_both_languages() {
        for message in [
            "det er feil",
            "Det er feil, tallet skal være 42",
            "dette stemmer ikke",
            "du tar feil her",
            "that's wrong",
            "That is incorrect — the invoice is from March",
            "feil svar",
        ] {
            assert!(is_correction(message), "{message:?} should read as a correction");
        }
    }

    /// An opener must not match a longer word: "feil svarprosent" is a topic, not
    /// a complaint about the answer.
    #[test]
    fn an_opener_does_not_match_a_longer_word() {
        assert!(!is_correction("feil svarprosent i rapporten"));
    }

    #[test]
    fn an_empty_message_is_not_a_signal() {
        assert!(!is_correction(""));
        assert!(!is_correction("   "));
        assert!(classify(&ctx("")).is_empty());
    }

    // ── Near-duplicate ──────────────────────────────────────────────────────

    fn reask<'a>(message: &'a str, previous: &'a str, secs: u64) -> TurnContext<'a> {
        TurnContext {
            message,
            previous_message: Some(previous),
            since_previous: Some(Duration::from_secs(secs)),
            ..TurnContext::default()
        }
    }

    #[test]
    fn an_immediate_reask_of_the_same_question_is_a_signal() {
        let context = reask(
            "hva er saldoen for kunde Aquatiq nå",
            "hva er saldoen for kunde Aquatiq",
            30,
        );
        assert!(is_near_duplicate(&context));
    }

    /// Word order changes constantly between a question and its re-ask, which is
    /// why similarity is over token sets rather than sequences.
    #[test]
    fn a_reordered_reask_still_matches() {
        let context = reask(
            "for kunde Aquatiq hva er saldoen",
            "hva er saldoen for kunde Aquatiq",
            20,
        );
        assert!(is_near_duplicate(&context));
    }

    #[test]
    fn the_same_question_much_later_is_ordinary_work() {
        let context = reask(
            "hva er saldoen for kunde Aquatiq",
            "hva er saldoen for kunde Aquatiq",
            NEAR_DUPLICATE_WINDOW.as_secs() + 1,
        );
        assert!(!is_near_duplicate(&context), "outside the window it is not a complaint");
    }

    #[test]
    fn a_different_question_about_the_same_topic_is_not_a_reask() {
        let context = reask(
            "hvem er kontaktpersonen hos kunde Aquatiq",
            "hva er saldoen for kunde Aquatiq",
            20,
        );
        assert!(!is_near_duplicate(&context), "same subject, different question");
    }

    #[test]
    fn short_messages_never_trigger_similarity() {
        assert!(!is_near_duplicate(&reask("takk!", "takk", 5)));
        assert!(!is_near_duplicate(&reask("og?", "og", 5)));
    }

    /// Without a timestamp the window cannot be established, so it fails closed.
    #[test]
    fn a_missing_timestamp_fails_closed() {
        let context = TurnContext {
            message: "hva er saldoen for kunde Aquatiq",
            previous_message: Some("hva er saldoen for kunde Aquatiq"),
            since_previous: None,
            ..TurnContext::default()
        };
        assert!(!is_near_duplicate(&context));
    }

    #[test]
    fn no_previous_message_is_not_a_reask() {
        assert!(!is_near_duplicate(&ctx("hva er saldoen for kunde Aquatiq")));
    }

    // ── Client-declared signals + combination ───────────────────────────────

    #[test]
    fn regenerate_and_edit_resubmit_come_from_the_client() {
        let signals = classify(&TurnContext {
            message: "hva er saldoen",
            regenerated: true,
            edited_resubmit: true,
            ..TurnContext::default()
        });
        let kinds: Vec<_> = signals.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SignalKind::Regenerate));
        assert!(kinds.contains(&SignalKind::EditResubmit));
    }

    /// Both signals are reported, not just the strongest: "unhappy in two ways"
    /// is different evidence from "unhappy once".
    #[test]
    fn a_turn_can_carry_several_signals() {
        let signals = classify(&TurnContext {
            message: "det er feil",
            regenerated: true,
            ..TurnContext::default()
        });
        assert_eq!(signals.len(), 2, "expected both regenerate and correction: {signals:?}");
    }

    #[test]
    fn an_ordinary_turn_carries_no_signal() {
        assert!(classify(&ctx("kan du finne fakturaen for mars?")).is_empty());
    }

    // ── Weighting ───────────────────────────────────────────────────────────

    /// A stated correction must outweigh a regenerate, which is a normal way to
    /// ask for variety rather than a complaint.
    #[test]
    fn a_correction_outweighs_a_regenerate() {
        assert!(SignalKind::Correction.strength() > SignalKind::Regenerate.strength());
        assert!(SignalKind::EditResubmit.strength() > SignalKind::Regenerate.strength());
    }

    #[test]
    fn every_strength_is_a_ratio() {
        for kind in [
            SignalKind::Regenerate,
            SignalKind::EditResubmit,
            SignalKind::NearDuplicate,
            SignalKind::Correction,
        ] {
            let strength = kind.strength();
            assert!(strength > 0.0 && strength <= 1.0, "{kind:?} strength {strength}");
            assert_eq!(Signal::of(kind).strength, strength);
        }
    }

    /// Wire tokens are stable and independent of the variant names, so a rename
    /// cannot silently change what is published.
    #[test]
    fn wire_tokens_are_stable() {
        assert_eq!(SignalKind::Regenerate.as_str(), "regenerate");
        assert_eq!(SignalKind::EditResubmit.as_str(), "edit_resubmit");
        assert_eq!(SignalKind::NearDuplicate.as_str(), "near_duplicate");
        assert_eq!(SignalKind::Correction.as_str(), "correction");
    }

    #[test]
    fn similarity_is_bounded_and_symmetric() {
        let a = tokenize("hva er saldoen for kunde Aquatiq");
        let b = tokenize("for kunde Aquatiq hva er saldoen");
        let forward = similarity(&a, &b);
        assert!((forward - similarity(&b, &a)).abs() < f32::EPSILON);
        assert!((0.0..=1.0).contains(&forward));
        assert!((similarity(&a, &a) - 1.0).abs() < f32::EPSILON);
        assert_eq!(similarity(&[], &b), 0.0);
    }
}
