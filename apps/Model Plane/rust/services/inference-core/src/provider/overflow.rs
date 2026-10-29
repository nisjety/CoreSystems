//! Provider "your prompt is too long" classification.
//!
//! This is the AUTHORITATIVE table. Provider wordings vary and inference-core
//! is the only layer that sees the raw HTTP body, so classification belongs
//! here rather than downstream on a forwarded message string.
//!
//! model-gateway keeps its own `compaction::is_context_length_error` as a
//! **rolling-deploy fallback**: during a deploy where the gateway is new and
//! inference-core is old, no typed signal arrives and the gateway must still
//! recognise an overflow in order to shed history and retry. A contract test
//! pins that the fallback recognises everything this table does, so a marker
//! added here cannot go unrecognised there for the length of a deploy.
//!
//! # Why false positives are the expensive direction
//!
//! A missed overflow costs one hard error the user sees. A *false* overflow
//! makes the gateway shed conversation history and retry a request that was
//! failing for an entirely different reason — so the real error is hidden
//! behind a silently truncated prompt, and the retry cannot succeed. Every
//! marker below is therefore a phrasing that means only this, and the
//! exclusions are checked first.

use super::ProviderError;

/// Substrings that unambiguously mean "this prompt is longer than the model's
/// input limit", across the provider wordings this service fronts.
///
/// Ordered loosely by provider family (OpenAI/Azure, Anthropic, generic) but
/// matching is order-independent.
pub(crate) const LENGTH_MARKERS: &[&str] = &[
    // OpenAI / Azure OpenAI
    "context_length_exceeded",
    "context length exceeded",
    "maximum context length",
    "this model supports at most",
    "reduce the length of the messages",
    "reduce the length of your prompt",
    "reduce your prompt",
    // Anthropic
    "prompt is too long",
    "prompt too long",
    "input length and `max_tokens` exceed context limit",
    "input length and max_tokens exceed context limit",
    "request_too_large",
    "request too large",
    // Generic / other gateways
    "maximum context window",
    "context window exceeded",
    "exceeds the context window",
    "exceeds model context",
    "maximum prompt length",
    "prompt exceeds",
    "input is too long",
    "too many input tokens",
    "too many tokens",
    "token limit exceeded",
    "exceeds the maximum number of tokens",
    "reduce the amount of context",
];

/// Wordings that also mention tokens and limits but mean something else.
/// Checked FIRST so a throttling error is never mistaken for a length error and
/// answered by shrinking a prompt that was never the problem.
///
/// `tokens per` is what keeps the broad `too many tokens` marker safe: every
/// rate-limit phrasing that mentions token counts qualifies them per minute or
/// per day.
pub(crate) const NOT_LENGTH_MARKERS: &[&str] = &[
    "rate limit",
    "rate_limit",
    "per minute",
    "per day",
    "tokens per",
    "quota",
    "insufficient_quota",
];

/// Whether a provider error body means the prompt exceeded the input limit.
#[must_use]
pub fn is_context_overflow(body: &str) -> bool {
    let haystack = body.to_ascii_lowercase();
    if NOT_LENGTH_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
    {
        return false;
    }
    LENGTH_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
}

/// Classify a non-success provider response into a typed [`ProviderError`].
///
/// `detail` keeps the provider's original `"<status>: <body>"` text verbatim on
/// both branches. That is deliberate: downstream string matching (the gateway
/// fallback, log greps, existing tests) keeps working unchanged, so adding the
/// type is purely additive and cannot regress a caller that has not adopted it.
#[must_use]
pub fn classify_http_failure(status: reqwest::StatusCode, body: &str) -> ProviderError {
    let detail = format!("{status}: {body}");
    if is_context_overflow(body) {
        ProviderError::TooLong { detail }
    } else {
        ProviderError::Http(detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overflow(body: &str) -> bool {
        matches!(
            classify_http_failure(reqwest::StatusCode::BAD_REQUEST, body),
            ProviderError::TooLong { .. }
        )
    }

    #[test]
    fn recognises_real_provider_wordings() {
        for body in [
            r#"{"error":{"code":"context_length_exceeded","message":"This model's maximum context length is 8192 tokens."}}"#,
            "prompt is too long: 210000 tokens > 200000 maximum",
            "input length and `max_tokens` exceed context limit: 200000 + 8192 > 200000",
            r#"{"type":"error","error":{"type":"request_too_large"}}"#,
            "Please reduce the length of your prompt.",
            "This model supports at most 4096 completion tokens",
            "token limit exceeded for this deployment",
        ] {
            assert!(overflow(body), "should classify as TooLong: {body}");
        }
    }

    /// The expensive direction. Each of these mentions tokens or limits and is
    /// NOT an overflow; treating one as overflow would shed the user's history
    /// and retry a request that cannot succeed.
    #[test]
    fn does_not_mistake_throttling_or_billing_for_overflow() {
        for body in [
            "Rate limit reached for gpt-4 in organization org-x on tokens per min (TPM): Limit 10000",
            "You exceeded your current quota, please check your plan and billing details",
            "insufficient_quota",
            "429: too many tokens per day for this key",
            "invalid_api_key",
            "The server had an error while processing your request",
        ] {
            assert!(!overflow(body), "must NOT classify as TooLong: {body}");
        }
    }

    /// A rate-limit body that also contains a length phrase must stay a
    /// rate-limit: exclusions are checked first, and this is the ambiguous case
    /// where getting the precedence backwards silently truncates prompts.
    #[test]
    fn exclusions_win_over_length_markers() {
        assert!(!overflow(
            "rate limit exceeded; also note the maximum context length is 8192 tokens"
        ));
    }

    #[test]
    fn preserves_the_provider_text_on_both_branches() {
        let body = "prompt is too long";
        match classify_http_failure(reqwest::StatusCode::BAD_REQUEST, body) {
            ProviderError::TooLong { detail } => assert!(detail.contains(body)),
            other => panic!("expected TooLong, got {other:?}"),
        }
        let body = "some other failure";
        match classify_http_failure(reqwest::StatusCode::BAD_GATEWAY, body) {
            ProviderError::Http(detail) => assert!(detail.contains(body)),
            other => panic!("expected Http, got {other:?}"),
        }
    }

    /// No exclusion may be a substring of a length marker, or it would
    /// unconditionally veto that marker and the table entry would be dead.
    /// (Same property the tool-retry classifier learned the hard way, where
    /// `"refused"` swallowed `"connection refused"`.)
    #[test]
    fn no_exclusion_silently_vetoes_a_length_marker() {
        for marker in LENGTH_MARKERS {
            for exclusion in NOT_LENGTH_MARKERS {
                assert!(
                    !marker.contains(exclusion),
                    "exclusion {exclusion:?} is inside length marker {marker:?}, which \
                     can therefore never match"
                );
            }
        }
    }
}
