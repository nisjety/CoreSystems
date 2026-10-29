//! Transient-failure retry policy for tool dispatch.
//!
//! Every other harness audited in `claude-hermes-deepseek.md` has some form of
//! this (`DeepSeek`'s `tools/execute` waterfall wrapping timeout/retry/metrics is
//! the most explicit); Model Plane had none, on either dispatch loop. A tool
//! that lost its connection mid-call surfaced to the model as a flat failure,
//! and the model's only recovery was to ask for the same tool again — burning a
//! whole round, and only if it happened to choose to.
//!
//! # The safety rule that shapes this module
//!
//! **A retry is only ever safe when the call could not have already taken
//! effect.** A timeout is *ambiguous*: the request may have reached the
//! provider and succeeded, with only the response lost. Retrying
//! `book_shipment` on a timeout is how one freight order becomes two.
//!
//! So retry is gated on TWO independent conditions, both of which must hold:
//!
//! 1. the failure looks transient ([`is_transient_tool_failure`]), and
//! 2. the call is not side-effecting ([`crate::permission::is_risky_call`] —
//!    the SAME classifier the `ask`-posture approval gate, the leaf/orchestrator
//!    subagent blocklist, and plan-mode enforcement all use, so a tool can never
//!    be "risky enough to gate" and "safe enough to silently retry" at once).
//!
//! Everything here is pure so the policy is unit-testable without a network,
//! matching `model-gateway`'s `compaction::is_context_length_error` — including
//! its check-the-exclusions-first shape, for the same reason: when unsure,
//! answer "not retryable" rather than replaying an action that may have landed.

use std::time::Duration;

/// Maximum dispatch attempts for one tool call, including the first.
///
/// Three, not more: a transient fault that survives two backoffs is not
/// transient, and the round budget this run is spending is shared with every
/// other call in the batch.
pub const MAX_TOOL_ATTEMPTS: u32 = 3;

/// Base backoff, doubled per attempt (100ms, then 200ms).
///
/// Deliberately short. This runs INSIDE a tool round the user is waiting on,
/// not in a background worker — a retry budget measured in seconds would turn
/// a recoverable blip into a visibly hung turn, which is the failure mode the
/// timeout ceilings in `executor.rs` already exist to prevent.
const BASE_BACKOFF_MS: u64 = 100;

/// Substrings that mean "this failed for a reason that may not recur" —
/// transport, timeout, upstream unavailability, or throttling.
///
/// Matched case-insensitively against the tool's own error text, which
/// `execute_step_inner` forwards largely verbatim from the underlying client,
/// so these cover the shapes reqwest/tonic/HTTP upstreams actually produce.
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

/// Wordings that can co-occur with a transient marker but mean the call is
/// **permanently** refused. Checked FIRST, so a policy refusal that happens to
/// mention a timeout, or a bad-argument error quoting an upstream 503 body, is
/// never replayed.
///
/// The failure classes here are all ones where a second identical attempt
/// produces an identical result: the model must change something (its
/// arguments, its tool choice) or a human must act.
const NOT_TRANSIENT_MARKERS: &[&str] = &[
    // Policy / authority refusals — a retry cannot change the decision.
    //
    // These are deliberately the SPECIFIC phrasings this codebase emits, not a
    // bare "refused"/"unavailable": a generic marker here silently swallows
    // `connection refused` and `service unavailable`, the two most canonical
    // transient transport errors there are, and turns the retry feature off
    // for exactly the cases it exists to handle. A unit test pins that
    // collision so the shortcut cannot be reintroduced.
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
    // Prompt-size faults: the same oversized prompt fails identically. The
    // real remedy is compaction, which the caller owns.
    "context length",
    "context_length_exceeded",
    "too long",
];

/// Whether a tool failure looks transient — worth one more attempt.
///
/// Answers `false` whenever unsure. A false positive replays work; a false
/// negative merely reports the failure the model would have seen anyway.
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

/// Whether this specific failed call may be dispatched again.
///
/// Both gates must pass — see the module doc. `is_risky_call` is checked first
/// because it is the one that protects against a *duplicated real-world
/// effect*, which is strictly worse than failing to retry.
#[must_use]
pub fn tool_call_is_retryable(tool_name: &str, tool_input: &str, error: &str) -> bool {
    if crate::permission::is_risky_call(tool_name, tool_input) {
        return false;
    }
    // A delegation is not replayable either, for a reason the risk classifier
    // cannot see: it has already written a durable, IMMUTABLE terminal receipt
    // for its child run.
    //
    // Replaying it re-enters the nested loop under the SAME child run id (the
    // `start_key` is identifiers only, deliberately, so a retried parent step
    // reuses its child run). A second `RecordTerminalOutcome` comes back
    // `already_applied` carrying the FIRST attempt's outcome — so a retry that
    // succeeds hands the parent a good answer while the lineage permanently
    // records the child as failed. The ledger and the answer would disagree, and
    // the ledger is the thing people audit.
    //
    // The replay path in `run_subagent` handles the genuine cross-process case
    // by honouring the recorded outcome rather than re-running; this stops the
    // in-turn retry from creating that disagreement in the first place.
    if crate::subagent::is_subagent_tool(tool_name) {
        return false;
    }
    is_transient_tool_failure(error)
}

/// Backoff before `attempt` (1-indexed: the wait BEFORE attempt 2 is
/// `backoff_before_attempt(2)`). Exponential from [`BASE_BACKOFF_MS`].
#[must_use]
pub fn backoff_before_attempt(attempt: u32) -> Duration {
    let exponent = attempt.saturating_sub(2);
    Duration::from_millis(BASE_BACKOFF_MS.saturating_mul(1u64 << exponent.min(10)))
}

/// Whether an inference response stopped because it hit the output token
/// ceiling rather than because the model was finished.
///
/// Kept byte-identical to `model-gateway`'s `output_hit_token_ceiling`
/// (`tool_loop.rs`) — the vocabulary is the provider's, so the two loops must
/// read it the same way or the same truncated round would be caught on one
/// surface and dispatched on the other.
#[must_use]
pub fn output_hit_token_ceiling(stop_reason: &str) -> bool {
    matches!(
        stop_reason.trim().to_ascii_lowercase().as_str(),
        "max_tokens" | "length"
    )
}

/// Index of the tool call whose arguments are truncated, given a round's
/// `stop_reason` and how many calls it requested.
///
/// # Why only the LAST call, not all of them
///
/// pi's version of this guard fails *every* tool call in a length-stopped
/// message. That is safe but wasteful: providers emit content blocks in
/// order, so when the ceiling falls mid-message only the FINAL `tool_use`
/// block can be half-written — every earlier call in the same round is
/// complete and still worth running. `model-gateway`'s inline loop already
/// reasoned this out (the bug it fixed was a truncated `create_artifact`
/// reporting "requires non-empty 'content'" while the real cause, truncation,
/// was reported nowhere). This mirrors that decision so both loops behave
/// identically.
///
/// `None` when the round ended normally or requested no tools.
#[must_use]
pub fn truncated_tool_call_index(stop_reason: &str, tool_call_count: usize) -> Option<usize> {
    if tool_call_count == 0 || !output_hit_token_ceiling(stop_reason) {
        return None;
    }
    Some(tool_call_count - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_real_transient_transport_and_throttle_failures() {
        for error in [
            "quarry web_search request failed: operation timed out",
            "transport error: connection reset by peer",
            "upstream returned 503 Service Unavailable",
            "provider returned 429 Too Many Requests",
            "shipping-core unavailable: connection refused",
            "stream ended: unexpected EOF",
            "dns error: failed to lookup address information",
        ] {
            assert!(
                is_transient_tool_failure(error),
                "should be transient: {error}"
            );
        }
    }

    #[test]
    fn deterministic_and_policy_failures_are_never_transient() {
        for error in [
            "",
            "tool 'delete_records' is not in this agent's allowed scope",
            "tool 'shell' requires human approval, which a delegated subagent cannot request",
            "this run is in plan mode and may only investigate, never act",
            "invalid json: expected value at line 1 column 1",
            "failed to parse arguments",
            "unknown tool 'frobnicate'",
            "context_length_exceeded",
        ] {
            assert!(
                !is_transient_tool_failure(error),
                "should NOT be transient: {error}"
            );
        }
    }

    #[test]
    fn no_exclusion_marker_swallows_a_transient_marker() {
        // Regression guard for a real bug caught by the test above on first
        // run: a bare "refused" exclusion matched INSIDE "connection refused",
        // silently disabling retry for the single most common transient
        // transport failure. Any exclusion marker that is a substring of a
        // transient marker has that same effect, so assert the property
        // directly rather than re-listing individual cases.
        for exclusion in NOT_TRANSIENT_MARKERS {
            for transient in TRANSIENT_MARKERS {
                assert!(
                    !transient.contains(exclusion),
                    "exclusion {exclusion:?} is a substring of transient marker \
                     {transient:?} — it would suppress retry for every failure \
                     carrying that transient text"
                );
            }
        }
    }

    #[test]
    fn the_real_refusal_texts_this_codebase_emits_are_still_excluded() {
        // The specific strings the exclusions were narrowed to match, taken
        // verbatim from their emitting call sites — proof that narrowing them
        // to dodge the substring collision did not stop them working.
        for refusal in [
            "delegation refused: a subagent may not spawn another subagent (nesting depth limit is 1)",
            "requested sandbox isolation is unavailable; refusing unsandboxed execution",
            "AllowDomains egress policy requires an egress proxy (HTTPS_PROXY/ALL_PROXY); none is \
             configured, refusing to grant unrestricted network egress",
        ] {
            assert!(
                !is_transient_tool_failure(refusal),
                "a policy refusal must never be retried: {refusal}"
            );
        }
    }

    #[test]
    fn an_exclusion_wins_even_when_a_transient_word_is_also_present() {
        // The exact false positive the ordering exists to prevent: a refusal
        // whose text quotes an upstream timeout, or a bad-input error that
        // mentions a 503 body. Replaying either accomplishes nothing.
        assert!(!is_transient_tool_failure(
            "permission denied after the upstream connection reset"
        ));
        assert!(!is_transient_tool_failure(
            "invalid argument: 'deadline exceeded' is not a valid status filter"
        ));
    }

    #[test]
    fn a_side_effecting_tool_is_never_retried_however_transient_the_failure() {
        // The double-booking hazard: a timeout is ambiguous, so a write that
        // MAY have landed upstream must never be replayed automatically.
        for tool in [
            "book_shipment",
            "execute_provider_action",
            "delete_records",
            "publish_social_post",
            "shell",
            "browser_agent",
        ] {
            assert!(
                !tool_call_is_retryable(tool, "{}", "operation timed out"),
                "{tool} is side-effecting and must never auto-retry"
            );
        }
    }

    #[test]
    fn a_read_only_tool_retries_only_on_a_transient_failure() {
        assert!(tool_call_is_retryable(
            "knowledge_search",
            r#"{"query":"q"}"#,
            "transport error: connection reset"
        ));
        assert!(!tool_call_is_retryable(
            "knowledge_search",
            r#"{"query":"q"}"#,
            "invalid json: expected value"
        ));
    }

    #[test]
    fn a_read_classified_provider_action_still_never_retries() {
        // `execute_provider_action` is operation-aware: a READ operation is
        // not "risky" for approval purposes. It is still a provider round
        // trip whose timeout is ambiguous, and `is_risky_call` fails safe to
        // risky when arguments are absent/unknown — assert the read case
        // explicitly so a future relaxation of that classifier is a visible,
        // deliberate change here rather than a silent new replay path.
        let read = r#"{"operation":"crm.contacts.list"}"#;
        assert!(!tool_call_is_retryable(
            "execute_provider_action",
            read,
            "operation timed out"
        ));
    }

    #[test]
    fn the_token_ceiling_vocabulary_matches_the_providers() {
        for reason in ["max_tokens", "length", "  MAX_TOKENS  ", "Length"] {
            assert!(output_hit_token_ceiling(reason), "{reason:?}");
        }
        for reason in [
            "end_turn",
            "stop",
            "tool_use",
            "stop_sequence",
            "",
            "stream_incomplete",
        ] {
            assert!(!output_hit_token_ceiling(reason), "{reason:?}");
        }
    }

    #[test]
    fn only_the_last_call_of_a_truncated_round_is_refused() {
        // Providers emit content blocks in order, so a ceiling that falls
        // mid-message can only have cut the FINAL tool_use block. Failing all
        // of them (pi's approach) would discard complete, valid calls.
        assert_eq!(truncated_tool_call_index("length", 3), Some(2));
        assert_eq!(truncated_tool_call_index("max_tokens", 1), Some(0));
        // A normal round refuses nothing, however many calls it made.
        assert_eq!(truncated_tool_call_index("end_turn", 3), None);
        assert_eq!(truncated_tool_call_index("tool_use", 3), None);
        // No calls means nothing to refuse — never an underflow.
        assert_eq!(truncated_tool_call_index("length", 0), None);
    }

    /// A delegation is not replayable, for a reason the risk classifier cannot
    /// see: it has already written an IMMUTABLE terminal receipt for its child
    /// run. Replaying re-enters the nested loop under the same child run id, and
    /// the second receipt comes back `already_applied` carrying the FIRST
    /// outcome — so a retry that succeeds hands the parent a good answer while
    /// the lineage permanently records the child as failed.
    ///
    /// The error text here is deliberately one the classifier calls transient:
    /// without the delegation exclusion this returns `true`, which is what
    /// produced the disagreement.
    #[test]
    fn a_delegation_is_never_retried_even_when_its_failure_looks_transient() {
        let transient = "connection reset by peer";
        assert!(
            is_transient_tool_failure(transient),
            "the premise: this error IS the kind the classifier retries"
        );
        assert!(
            !tool_call_is_retryable("subagent.task", r#"{"goal":"look it up"}"#, transient),
            "replaying a delegation forks the answer from the receipt its child run already carries"
        );
        assert!(
            !tool_call_is_retryable("subagent.research", "{}", transient),
            "every subagent.* label, not just the one the tool table happens to offer"
        );
        // And the exclusion is specific: an ordinary read with the same
        // transient error is still retried, or this change would have quietly
        // disabled retry for everything.
        assert!(
            tool_call_is_retryable("knowledge_search", r#"{"query":"x"}"#, transient),
            "a plain read stays retryable"
        );
        // The bare prefix is not a subagent tool, and must not be caught by a
        // sloppy `starts_with` — it would be an ordinary unknown tool name.
        assert!(tool_call_is_retryable("subagent.", "{}", transient));
    }

    #[test]
    fn backoff_grows_and_stays_bounded() {
        assert_eq!(backoff_before_attempt(2), Duration::from_millis(100));
        assert_eq!(backoff_before_attempt(3), Duration::from_millis(200));
        // Never panics or overflows on an absurd attempt number.
        assert!(backoff_before_attempt(u32::MAX) <= Duration::from_millis(100 * 1024));
    }
}
