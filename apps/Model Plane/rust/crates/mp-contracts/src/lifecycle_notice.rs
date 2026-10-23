//! The closed vocabulary of the typed run lifecycle notices (build plan WS15 S1).
//!
//! A notice says what the gateway did to a turn or a tool call: it retried a
//! transient failure, compacted the history, or shed context the model would
//! otherwise have seen. Every payload field is a count or a token from one of
//! the closed lists below, never text, so a notice can reach any client and a
//! log without carrying content. A token outside these lists is not a notice
//! this build knows how to show.

/// Master switch for the lifecycle frames. Only the literal `true` enables it,
/// so an unset, empty or mistyped value leaves every stream as it was.
pub const ENABLED_ENV: &str = "MODEL_GATEWAY_LIFECYCLE_EVENTS";

/// Whether `raw` (the value of [`ENABLED_ENV`], if set) is the literal on-word.
#[must_use]
pub fn enabled_from(raw: Option<&str>) -> bool {
    raw == Some("true")
}

/// Whether lifecycle frames are switched on in this process.
#[must_use]
pub fn enabled() -> bool {
    enabled_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// Which part of a run a retry belongs to. Only tool calls are retried today.
pub const RETRY_SCOPES: [&str; 1] = ["tool"];

/// Why a retry was scheduled: the transient class the failure fell into. The
/// error text itself never leaves the process.
pub const RETRY_REASONS: [&str; 5] = [
    "timeout",
    "connection",
    "upstream_unavailable",
    "rate_limited",
    "other",
];

/// Which part of the conversation a compaction touched.
pub const COMPACTION_SCOPES: [&str; 2] = ["history", "tool_results"];

/// What a compaction did. A summary that failed is reported as `truncated`,
/// never as `summarized`.
pub const COMPACTION_OUTCOMES: [&str; 4] = ["summarized", "reused", "truncated", "cleared"];

/// What changed about the context the model was given.
pub const CONTEXT_CHANGES: [&str; 3] = ["history_shed", "skills_truncated", "run_context_skipped"];

/// The keys a `retry_scheduled` payload may carry, beside `request_id`.
pub const RETRY_SCHEDULED_KEYS: [&str; 6] = [
    "scope",
    "tool",
    "attempt",
    "max_attempts",
    "wait_ms",
    "reason",
];

/// The keys a `compaction` payload may carry, beside `request_id`.
pub const COMPACTION_KEYS: [&str; 5] = [
    "scope",
    "outcome",
    "messages_compacted",
    "messages_kept",
    "pinned_kept",
];

/// The keys a `context_change` payload may carry, beside `request_id`.
pub const CONTEXT_CHANGE_KEYS: [&str; 6] = [
    "change",
    "messages_dropped",
    "skills_truncated",
    "skills_dropped",
    "layer",
    "state",
];

/// Whether `token` is a member of `closed`. Exact match only: no trimming, no
/// case folding, so free text is never accepted as a token.
#[must_use]
pub fn is_closed_token(closed: &[&str], token: &str) -> bool {
    closed.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_literal_true_enables_the_switch() {
        assert!(enabled_from(Some("true")));
        for off in [None, Some(""), Some("false"), Some("1"), Some("yes"), Some("TRUE"), Some(" true")] {
            assert!(!enabled_from(off), "{off:?} must leave the switch off");
        }
    }

    #[test]
    fn every_listed_token_is_accepted_and_near_misses_are_not() {
        for (closed, tokens) in [
            (&RETRY_SCOPES[..], vec!["tool"]),
            (&RETRY_REASONS[..], vec!["timeout", "connection", "upstream_unavailable", "rate_limited", "other"]),
            (&COMPACTION_SCOPES[..], vec!["history", "tool_results"]),
            (&COMPACTION_OUTCOMES[..], vec!["summarized", "reused", "truncated", "cleared"]),
            (&CONTEXT_CHANGES[..], vec!["history_shed", "skills_truncated", "run_context_skipped"]),
        ] {
            for token in &tokens {
                assert!(is_closed_token(closed, token), "{token}");
            }
            for bad in ["", " tool", "Tool", "tool ", "tool_results_", "error text"] {
                assert!(!is_closed_token(closed, bad), "{bad:?}");
            }
        }
    }

    #[test]
    fn each_allowlist_names_a_key_once() {
        for keys in [&RETRY_SCHEDULED_KEYS[..], &COMPACTION_KEYS[..], &CONTEXT_CHANGE_KEYS[..]] {
            let unique: std::collections::BTreeSet<_> = keys.iter().collect();
            assert_eq!(unique.len(), keys.len(), "{keys:?}");
        }
    }

    #[test]
    fn no_allowlist_admits_a_content_key() {
        for keys in [&RETRY_SCHEDULED_KEYS[..], &COMPACTION_KEYS[..], &CONTEXT_CHANGE_KEYS[..]] {
            for content in ["summary", "args", "output", "error", "message", "content", "text"] {
                assert!(!keys.contains(&content), "{content} would carry content");
            }
        }
    }
}
