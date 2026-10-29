//! Transient-failure retry policy for the inline chat tool loop.
//!
//! The sibling of `execution-core`'s `runtime_loop::retry`, deliberately kept
//! as its own copy rather than shared through a crate: the two dispatch loops
//! stay separate on purpose (see `CLAUDE.md`, and
//! `docs/postmortem/0001-harn-1-2-tool-dispatch-unification.md` for the
//! measurement that settled it). What must NOT drift is the failure
//! vocabulary, so `tests/tool_retry_contract.rs` asserts both marker lists
//! agree — the same source-of-record technique
//! `execution-core/tests/cross_service_loop_contract.rs` already uses for the
//! round-budget constants, and for the same reason (the alternative is a
//! build dependency heavier than the invariant it protects).
//!
//! # Why this loop needs no side-effect gate
//!
//! `execution-core`'s copy gates retry on `permission::is_risky_call`, because
//! that loop dispatches real writes and a timeout is ambiguous — the call may
//! have landed. This loop cannot: `inline_tool_allowed` refuses every
//! side-effecting tool before dispatch, and the `match` arms fall through to
//! an explicit unknown-tool error. Every tool reachable here is a read, so a
//! replay costs latency, never a duplicated effect. The transient-vs-permanent
//! judgement is therefore the ONLY judgement this copy has to make — which is
//! precisely why the two copies must still agree on it.

use std::time::Duration;

/// Maximum dispatch attempts for one chat tool call, including the first.
/// Matches `execution-core`'s `MAX_TOOL_ATTEMPTS`; the contract test pins it.
pub const MAX_TOOL_ATTEMPTS: u32 = 3;

/// Base backoff in milliseconds, doubled per attempt.
const BASE_BACKOFF_MS: u64 = 100;

/// See `execution-core`'s `runtime_loop::retry::TRANSIENT_MARKERS`. Kept
/// byte-identical by `tests/tool_retry_contract.rs`.
const TRANSIENT_MARKERS: &[&str] = &[
    // Timeouts / deadlines
    "timed out",
    "timeout",
    "deadline exceeded",
    // Connection-level faults
    "connection refused",
    "connection reset",
    "connection closed",
    "connection error",
    "broken pipe",
    "transport error",
    "no route to host",
    "dns error",
    "unexpected eof",
    // Upstream saying "not now"
    "service unavailable",
    "temporarily unavailable",
    "502 bad gateway",
    "503",
    "504",
    "too many requests",
    "429",
    "rate limit",
    "rate_limit",
];

/// See `execution-core`'s `runtime_loop::retry::NOT_TRANSIENT_MARKERS`. Kept
/// byte-identical by `tests/tool_retry_contract.rs`.
///
/// Note the deliberately specific refusal phrasings: a bare `"refused"` here
/// would match inside `connection refused` and silently disable retry for the
/// most common transient failure there is.
const NOT_TRANSIENT_MARKERS: &[&str] = &[
    // Policy / authority refusals — a retry cannot change the decision.
    "requires human approval",
    "not in this agent's allowed scope",
    "permission denied",
    "is not permitted",
    "plan mode",
    "may never run",
    "delegation refused",
    "refusing unsandboxed",
    "refusing to grant",
    "denied by",
    "unauthorized",
    "forbidden",
    // Deterministic input faults — identical input, identical failure.
    "invalid argument",
    "invalid json",
    "malformed",
    "failed to parse",
    "parse error",
    "unknown tool",
    "no executor",
    "unsupported",
    // Prompt-size faults: the same oversized prompt fails identically.
    "context length",
    "context_length_exceeded",
    "too long",
];

/// Whether a tool failure looks transient — worth one more attempt.
/// Answers `false` whenever unsure.
#[must_use]
pub fn is_transient_tool_failure(error: &str) -> bool {
    let haystack = error.to_ascii_lowercase();
    if NOT_TRANSIENT_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
    {
        return false;
    }
    TRANSIENT_MARKERS
        .iter()
        .any(|marker| haystack.contains(marker))
}

/// Backoff before `attempt` (1-indexed; the wait BEFORE attempt 2 is
/// `backoff_before_attempt(2)`).
#[must_use]
pub fn backoff_before_attempt(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(2);
    Duration::from_millis(BASE_BACKOFF_MS.saturating_mul(1u64 << exponent.min(10)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_transport_failures_are_retryable_here_too() {
        for error in [
            "web_search failed: operation timed out",
            "transport error: connection reset by peer",
            "knowledge_search: connection refused",
            "upstream returned 503 Service Unavailable",
            "provider returned 429 Too Many Requests",
        ] {
            assert!(is_transient_tool_failure(error), "should retry: {error}");
        }
    }

    #[test]
    fn deterministic_failures_are_not_retryable() {
        for error in [
            "",
            "unknown tool 'frobnicate'",
            "invalid json: expected value at line 1 column 1",
            "tool 'save_memory' is not in this agent's allowed scope",
        ] {
            assert!(
                !is_transient_tool_failure(error),
                "should NOT retry: {error}"
            );
        }
    }

    #[test]
    fn no_exclusion_marker_swallows_a_transient_marker() {
        // Same property the execution-core copy asserts — a bare exclusion
        // that is a substring of a transient marker disables retry for every
        // failure carrying that text.
        for exclusion in NOT_TRANSIENT_MARKERS {
            for transient in TRANSIENT_MARKERS {
                assert!(
                    !transient.contains(exclusion),
                    "exclusion {exclusion:?} is a substring of transient marker {transient:?}"
                );
            }
        }
    }
}
