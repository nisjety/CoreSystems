//! The viewer's interface language as a one-sentence framing hint (WS16-S6).
//!
//! `InvokeRequest.locale` is stamped by the BFF gateway from a closed set (`nb-NO` or `en-US`) and
//! is request-scoped: it is never stored, grants nothing, and is `Honored` under Zero Data Retention
//! because it persists nothing. This module is the pure half: it turns that field into the
//! sentence. It matches the closed set again here, so a value that did not come through the
//! gateway's mapping (another route, a hand-built request) produces no sentence rather than text
//! the caller chose. The prompt-assembly call site (`identity_context_message` in `sse.rs`) belongs
//! to the prompt composer and is not edited by the Space slice; until it calls [`hint_sentence`]
//! the field is accepted and ignored.

/// The sentence for a stamped locale, or `None` for an absent or unknown value.
///
/// Last-priority framing: it never overrides what the user wrote.
#[must_use]
#[allow(dead_code, reason = "first caller is the prompt composer's identity block (WS16-S6 open question 1)")]
pub fn hint_sentence(locale: Option<&str>) -> Option<&'static str> {
    match locale.map(str::trim) {
        Some("nb-NO") => Some(
            "The user's interface language is Norwegian Bokmal; answer in the language the user writes in, and use this one when the message does not decide it.",
        ),
        Some("en-US") => Some(
            "The user's interface language is English; answer in the language the user writes in, and use this one when the message does not decide it.",
        ),
        _ => None,
    }
}
