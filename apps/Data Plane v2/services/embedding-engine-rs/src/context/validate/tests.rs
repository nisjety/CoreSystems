use super::*;
use crate::context::outcome::ContextOutcome;
use crate::context::testing::{captured_azure, require_capture};

const GOOD: &str = "Fjordfisk AS' årsrapport for 2025, om driftsmarginen på 31 prosent.";

fn accepted(content: &str, stop: &str) -> Accepted {
    validate(content, stop)
        .unwrap_or_else(|rejection| panic!("{content:?} / {stop:?}: {rejection:?}"))
}

fn rejected(content: &str, stop: &str) -> Rejection {
    match validate(content, stop) {
        Ok(kept) => panic!("{content:?} / {stop:?} was kept as {kept:?}"),
        Err(rejection) => rejection,
    }
}

/// The defect: a design that required `end_turn` would have rejected every answer
/// Azure OpenAI ever returned, because inference-core passes the provider's own
/// finish reason through (`stop` for a complete answer). The reasons below are the
/// ones its adapters emit; the real captured answer is checked further down.
#[test]
fn an_azure_answer_that_finished_is_kept() {
    for reason in ["stop", "STOP", " stop\n", "end_turn", "stop_sequence"] {
        assert_eq!(accepted(GOOD, reason).text, GOOD, "{reason:?}");
    }
}

#[test]
fn every_finish_reason_is_classified_and_an_unknown_one_is_not_trusted() {
    let table = [
        ("stop", StopKind::Complete),
        ("end_turn", StopKind::Complete),
        ("stop_sequence", StopKind::Complete),
        ("length", StopKind::Truncated),
        ("max_tokens", StopKind::Truncated),
        ("content_filter", StopKind::ContentFilter),
        ("refusal", StopKind::ContentFilter),
        // The model wanted a tool nobody offered; a new reason nobody has met; none.
        ("tool_calls", StopKind::Unknown),
        ("function_call", StopKind::Unknown),
        ("pause_turn", StopKind::Unknown),
        ("something_new", StopKind::Unknown),
        ("", StopKind::Unknown),
    ];
    for (reason, expected) in table {
        assert_eq!(classify_stop_reason(reason), expected, "{reason:?}");
    }
    // An unknown reason with an otherwise perfect answer is still not kept: the
    // answer might be cut off and nothing says it is not.
    assert_eq!(rejected(GOOD, "tool_calls"), Rejection::UnknownStopReason);
    assert_eq!(rejected(GOOD, ""), Rejection::UnknownStopReason);
}

#[test]
fn a_content_filter_stop_is_permanent_whatever_text_came_with_it() {
    for reason in ["content_filter", "refusal"] {
        let rejection = rejected(GOOD, reason);
        assert_eq!(rejection, Rejection::ContentFilter);
        assert_eq!(rejection.error_class(), ErrorClass::ContentFilter);
        // Never retried: the same document would be refused again and each
        // attempt is paid for.
        assert_eq!(
            rejection.outcome(),
            ContextOutcome::permanent(ErrorClass::ContentFilter)
        );
        assert_eq!(rejection.outcome().retry_after(), None);
    }
}

#[test]
fn every_other_rejection_is_an_explicit_invalid_output_worth_a_later_retry() {
    for rejection in [
        Rejection::Empty,
        Rejection::TooShort,
        Rejection::TooLong,
        Rejection::UnknownStopReason,
        Rejection::TruncatedWithoutSentence,
    ] {
        assert_eq!(rejection.error_class(), ErrorClass::InvalidOutput);
        assert_eq!(
            rejection.outcome(),
            ContextOutcome::transient(ErrorClass::InvalidOutput, None)
        );
    }
}

/// The trap: an empty answer used to be `Ok(None)`, which the caller treated as
/// "no context" without saying so. It is a rejection with a class now.
#[test]
fn an_empty_answer_is_an_explicit_invalid_output_never_a_silent_nothing() {
    for empty in ["", "   ", "\n\t\n", "<p></p>", "<br/>", "\u{200B}\u{FEFF}"] {
        assert_eq!(rejected(empty, "stop"), Rejection::Empty, "{empty:?}");
    }
}

#[test]
fn markup_is_stripped_and_whitespace_collapsed() {
    assert_eq!(
        accepted("<b>Fjordfisk</b> sin <i>årsrapport</i> for 2025.", "stop").text,
        "Fjordfisk sin årsrapport for 2025."
    );
    assert_eq!(
        accepted(
            "<context>\n  Fjordfisk AS,\n\n  årsrapport 2025.\n</context>",
            "stop"
        )
        .text,
        "Fjordfisk AS, årsrapport 2025."
    );
    assert_eq!(
        accepted("Om <!-- skjult --> driftsmarginen i 2025.", "stop").text,
        "Om driftsmarginen i 2025."
    );
    // A bare `<` is text: only something that opens like a tag and closes is removed.
    assert_eq!(
        accepted("Tabellen viser x < 5 og y > 3 for 2025.", "stop").text,
        "Tabellen viser x < 5 og y > 3 for 2025."
    );
    assert_eq!(
        accepted("Om a <b uten slutt i dokumentet 2025.", "stop").text,
        "Om a <b uten slutt i dokumentet 2025."
    );
    // Control and zero-width characters go; an `ø` and a `å` do not.
    assert_eq!(
        accepted("Røyking\u{0007} av\u{200B} laks i Tromsø, 2025.", "stop").text,
        "Røyking av laks i Tromsø, 2025."
    );
}

#[test]
fn the_length_bounds_count_characters_not_bytes() {
    let min = "ø".repeat(MIN_CHARS);
    let max = "ø".repeat(MAX_CHARS);
    assert_eq!(accepted(&min, "stop").text.chars().count(), MIN_CHARS);
    assert_eq!(accepted(&max, "stop").text.chars().count(), MAX_CHARS);
    assert!(
        max.len() > MAX_CHARS,
        "the test is only meaningful with multi-byte characters"
    );
    assert_eq!(
        rejected(&"ø".repeat(MIN_CHARS - 1), "stop"),
        Rejection::TooShort
    );
    assert_eq!(
        rejected(&"ø".repeat(MAX_CHARS + 1), "stop"),
        Rejection::TooLong
    );
    // The bounds apply to the normalized text, not the raw one.
    assert_eq!(rejected("N/A", "stop"), Rejection::TooShort);
    assert_eq!(
        rejected(&format!("<p>{}</p>", "ø".repeat(MIN_CHARS - 1)), "stop"),
        Rejection::TooShort
    );
}

#[test]
fn an_answer_cut_off_by_the_output_cap_keeps_its_last_full_sentence() {
    let cut = accepted(
        "Fjordfisk AS rapporterte om driftsmarginen i 2025. Neste setning ble avbrutt midt i et ord som kj",
        "length",
    );
    assert_eq!(
        cut.text,
        "Fjordfisk AS rapporterte om driftsmarginen i 2025."
    );
    assert!(cut.cut_at_sentence);
    // Both vocabularies say the same thing.
    assert_eq!(
        accepted(
            "Første hele setning her i teksten! Så en som blir avbr",
            "max_tokens"
        )
        .text,
        "Første hele setning her i teksten!"
    );
    assert_eq!(
        accepted("Første hele setning her i teksten… og så en halv", "length").text,
        "Første hele setning her i teksten…"
    );
    // Cut off exactly at a sentence end: nothing to shorten.
    let whole = accepted(
        "Fjordfisk AS rapporterte om driftsmarginen i 2025.",
        "length",
    );
    assert!(!whole.cut_at_sentence);
    // A complete answer is never shortened, whatever it holds.
    let complete = accepted(
        "Første setning her i teksten. Andre setning uten punktum",
        "stop",
    );
    assert_eq!(
        complete.text,
        "Første setning her i teksten. Andre setning uten punktum"
    );
    assert!(!complete.cut_at_sentence);
}

#[test]
fn a_cut_off_answer_with_no_usable_sentence_is_invalid_output() {
    // No terminator at all.
    assert_eq!(
        rejected("Fjordfisk AS rapporterte om driftsmarginen i", "length"),
        Rejection::TruncatedWithoutSentence
    );
    // The only sentence is under twenty characters.
    assert_eq!(
        rejected("Kort her. Så avbr", "length"),
        Rejection::TruncatedWithoutSentence
    );
    // A decimal and an abbreviation inside a word are not sentence ends.
    assert_eq!(
        rejected("Marginen var 3.5 prosent og stigende i", "max_tokens"),
        Rejection::TruncatedWithoutSentence
    );
    // Only after the cut is the minimum length checked: a nineteen-character
    // sentence is not kept even though the whole answer was long.
    assert_eq!(
        rejected("Nitten tegn i alt.. og så videre uten", "length"),
        Rejection::TruncatedWithoutSentence
    );
}

#[test]
fn meta_openings_are_counted_not_rejected() {
    for opening in [
        "This chunk describes the margin of Fjordfisk AS in 2025.",
        "the chunk is about the margin of Fjordfisk AS in 2025.",
        "This section covers the margin of Fjordfisk AS in 2025.",
        "Denne delen handler om driftsmarginen til Fjordfisk AS i 2025.",
        "Dette avsnittet gjelder driftsmarginen til Fjordfisk AS i 2025.",
    ] {
        let kept = accepted(opening, "stop");
        assert!(kept.meta_opening, "{opening}");
        assert_eq!(kept.text, opening);
    }
    for clean in [
        GOOD,
        "Fjordfisk AS, årsrapport 2025: om driftsmarginen.",
        "In the chunk-sized pieces of the report the margin is discussed.",
    ] {
        assert!(!accepted(clean, "stop").meta_opening, "{clean}");
    }
}

/// The captured real answer, once the live proof has recorded it. Until then this
/// does not pass by not looking: under `REQUIRE_CONTRACT` it fails and names what is
/// missing. See `tests/fixtures/contracts/README.md`.
#[test]
fn the_captured_real_azure_answers_are_accepted_or_cut_the_way_their_reason_says() {
    let Some(capture) = captured_azure() else {
        assert!(
            !require_capture(),
            "tests/fixtures/contracts/infer-response-azure-live.json has not been captured yet: run the live proof \
             (provider::contextualize::live_proof, see docs/contextual-retrieval-runbook.md) and commit the file"
        );
        eprintln!("pending: the real Azure answer has not been captured yet (REQUIRE_CONTRACT=1 makes this a failure)");
        return;
    };
    for case in capture.cases {
        let stop = case.stop_reason.as_str();
        let verdict = validate(&case.content, stop);
        match classify_stop_reason(stop) {
            StopKind::Complete => {
                let kept = verdict.unwrap_or_else(|rejection| {
                    panic!(
                        "the real answer `{}` ({stop}) was rejected: {rejection:?}",
                        case.name
                    )
                });
                assert!(!kept.cut_at_sentence, "{}", case.name);
            }
            StopKind::Truncated => match verdict {
                Ok(kept) => assert!(
                    kept.text.chars().count() >= MIN_TRUNCATED_CHARS,
                    "{}",
                    case.name
                ),
                Err(rejection) => assert_eq!(
                    rejection,
                    Rejection::TruncatedWithoutSentence,
                    "{}",
                    case.name
                ),
            },
            other => panic!(
                "the capture `{}` carries a reason of kind {other:?}",
                case.name
            ),
        }
    }
}
