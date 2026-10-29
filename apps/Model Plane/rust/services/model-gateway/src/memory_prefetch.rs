//! Per-turn memory-prefetch gating (harness-adoption §7.9).
//!
//! Ported from `hermes-agent`'s `is_trivial_prompt` (MIT) with a
//! Norwegian-first extension: Verevon's users write "ja", "takk", "fortsett"
//! exactly as often as "yes", "thanks", "continue", and a trivial-prompt gate
//! that only speaks English would silently pay the recall round-trip — and
//! risk stale user-model context derailing a one-word reply — on precisely the
//! turns it exists to protect, for the product's primary audience.
//!
//! The classification rule mirrors Hermes's regex semantics without a regex:
//! strip trailing punctuation/whitespace, lowercase, and require the WHOLE
//! remaining prompt to be one of the trivial words/phrases. Words that merely
//! START with a trivial word ("okei da, la oss se på ordre 4471") never match,
//! while trailing-punctuation variants ("hi!", "takk :)", "done???") do.

/// Prompts that carry no semantic signal — trivial acknowledgements,
/// greetings, or slash commands. One flat list, English + Norwegian.
const TRIVIAL_PROMPTS: &[&str] = &[
    // English acknowledgements / greetings (Hermes's list, verbatim)
    "yes",
    "no",
    "ok",
    "okay",
    "sure",
    "thanks",
    "thank you",
    "y",
    "n",
    "yep",
    "nope",
    "yeah",
    "nah",
    "hi",
    "hey",
    "hello",
    "yo",
    "sup",
    "continue",
    "go ahead",
    "do it",
    "proceed",
    "got it",
    "cool",
    "nice",
    "great",
    "done",
    "next",
    "lgtm",
    "k",
    // Norwegian additions
    "ja",
    "nei",
    "takk",
    "tusen takk",
    "hei",
    "heisann",
    "hallo",
    "morn",
    "fortsett",
    "kjør",
    "kjør på",
    "gjør det",
    "greit",
    "ok da",
    "okei",
    "flott",
    "supert",
    "bra",
    "ferdig",
    "neste",
    "den er god",
    "skjønner",
];

/// Trailing characters that do not change a prompt's triviality ("hi!",
/// "takk :)", "done???"). Mirrors Hermes's trailing-punctuation class.
const TRAILING_NOISE: &[char] = &[
    ' ', '\t', '\n', '\r', '!', '?', '.', ':', ';', ',', '"', '\'', '~', '\u{2018}', '\u{2019}',
    '\u{201c}', '\u{201d}', '\u{2014}', '\u{2013}', '\u{2026}', '(', ')', '[', ']', '{', '}', '<',
    '>', '*', '&', '^', '%', '$', '#', '@', '+', '=', '`', '\u{a0}',
];

/// True when a user prompt is too trivial to warrant memory recall.
///
/// Callers skip the prefetch round-trip entirely for these turns — the cost
/// saving matters less than the correctness half: injecting recalled context
/// under a bare "ja" invites the model to answer the MEMORY instead of the
/// acknowledgement.
#[must_use]
pub fn is_trivial_prompt(text: &str) -> bool {
    let stripped = text.trim();
    if stripped.is_empty() {
        return true;
    }
    if stripped.starts_with('/') {
        return true;
    }
    let normalized = stripped
        .trim_end_matches(|c: char| TRAILING_NOISE.contains(&c))
        .to_lowercase();
    if normalized.is_empty() {
        return true;
    }
    TRIVIAL_PROMPTS.contains(&normalized.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slash_and_bare_acknowledgements_are_trivial() {
        for prompt in [
            "",
            "   ",
            "/help",
            "yes",
            "ok",
            "Thanks!",
            "done???",
            "hi!",
            "LGTM",
            "thank you.",
            "got it :)",
        ] {
            assert!(is_trivial_prompt(prompt), "should be trivial: {prompt:?}");
        }
    }

    #[test]
    fn norwegian_acknowledgements_are_trivial_too() {
        for prompt in [
            "ja",
            "Nei.",
            "takk!",
            "Tusen takk!",
            "fortsett",
            "kjør på",
            "greit",
            "supert",
        ] {
            assert!(is_trivial_prompt(prompt), "should be trivial: {prompt:?}");
        }
    }

    #[test]
    fn real_questions_are_never_trivial() {
        for prompt in [
            "what is the stock level for SKU 4471?",
            "hva er leveringstiden til Bergen?",
            // Starts with a trivial word but continues — the exact false
            // positive Hermes's anchored regex guards against.
            "ok, la oss se på ordre 4471",
            "yes but what about the invoice from May",
            "k8s deployment failing",
            "note the budget is 42750 NOK",
            "hindsight is 20/20, but why did the shipment fail?",
        ] {
            assert!(
                !is_trivial_prompt(prompt),
                "should NOT be trivial: {prompt:?}"
            );
        }
    }
}

#[cfg(test)]
mod prefetch_observability_tests {
    /// Every early return in `sse::fetch_chat_memory_context` must record an
    /// outcome, or a silently-degraded turn is invisible again.
    ///
    /// Source-read rather than behavioural: the function needs a live memory
    /// client and a verified bearer to call, and the property under test is
    /// "no return path was missed" — a structural claim about the function,
    /// which is exactly what reading it can establish and a single behavioural
    /// test cannot.
    #[test]
    fn every_prefetch_exit_records_an_outcome() {
        let source = include_str!("sse.rs");
        let start = source
            .find("async fn fetch_chat_memory_context")
            .expect("fetch_chat_memory_context not found — re-point this test");
        let body = &source[start..];
        let end = body
            .find("\nasync fn load_recent_thread_messages")
            .expect("could not bound the function");
        let body = &body[..end];

        let returns = body.matches("return (Vec::new(), None);").count();
        let recorded = body.matches("record_memory_prefetch_outcome(").count();
        assert!(
            returns > 0,
            "parsed no early returns — this test broke, not the invariant"
        );
        // Each early return plus the `empty` and `hit` terminal paths.
        assert_eq!(
            recorded,
            returns + 2,
            "an exit from the memory prefetch does not record an outcome. Every \
             path that answers a turn WITHOUT memory must be countable, or a \
             broken prefetch reads as 'nothing to recall'."
        );
    }

    /// The two labels the UI cannot tell apart must stay distinct in telemetry.
    #[test]
    fn empty_and_timeout_are_separate_labels() {
        let source = include_str!("sse.rs");
        for label in [
            "\"empty\"",
            "\"timeout\"",
            "\"no_credential\"",
            "\"error\"",
            "\"hit\"",
        ] {
            assert!(
                source.contains(&format!("record_memory_prefetch_outcome({label})")),
                "outcome label {label} is declared but never recorded"
            );
        }
    }
}
