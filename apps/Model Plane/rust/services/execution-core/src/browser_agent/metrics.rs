//! Browser-run counters (WS11.S5).
//!
//! Four counters, every label from a closed list written down here, so the
//! cardinality is a constant: no org id, run id, URL, grant or error text ever
//! becomes a label.
//!
//! - `execution_browser_runs_total{planner, outcome}`
//! - `execution_browser_planner_tokens_total{direction}`
//! - `execution_browser_failures_total{kind}`
//! - `execution_browser_deterministic_total{reason}`

use super::failure::BrowserFailureKind;
use super::BrowserLoopUsage;
use crate::planner_policy::DeterministicReason;

/// The `planner` label of a recipe run: a saved procedure replayed by `RecipePlanner` (Q4-E4.2-3h).
pub const RECIPE_PLANNER_LABEL: &str = "recipe";
/// The `planner` label.
pub const PLANNER_LABELS: [&str; 3] = ["llm", "deterministic", RECIPE_PLANNER_LABEL];
/// The `outcome` label: how the browser tool call ended.
pub const OUTCOME_LABELS: [&str; 8] = [
    "completed",
    "failed",
    "permission_denied",
    "timed_out",
    "resource_exhausted",
    "cancelled",
    "aborted",
    "awaiting_approval",
];
/// The `direction` label.
pub const DIRECTION_LABELS: [&str; 2] = ["input", "output"];

/// `llm` when a planner ran, `deterministic` when the loop did.
#[must_use]
pub const fn planner_label(llm: bool) -> &'static str {
    if llm {
        PLANNER_LABELS[0]
    } else {
        PLANNER_LABELS[1]
    }
}

/// One finished browser run.
pub fn record_run(planner: &'static str, outcome: &'static str) {
    metrics::counter!("execution_browser_runs_total", "planner" => planner, "outcome" => outcome)
        .increment(1);
}

/// What the planner spent, counts only.
pub fn record_planner_tokens(usage: BrowserLoopUsage) {
    if usage.planner_input_tokens > 0 {
        metrics::counter!("execution_browser_planner_tokens_total", "direction" => "input")
            .increment(usage.planner_input_tokens);
    }
    if usage.planner_output_tokens > 0 {
        metrics::counter!("execution_browser_planner_tokens_total", "direction" => "output")
            .increment(usage.planner_output_tokens);
    }
}

/// A run that stopped on its own judgement.
pub fn record_failure(kind: BrowserFailureKind) {
    metrics::counter!("execution_browser_failures_total", "kind" => kind.as_str()).increment(1);
}

/// A run the policy kept off the planner, by reason.
pub fn record_deterministic(reason: DeterministicReason) {
    metrics::counter!("execution_browser_deterministic_total", "reason" => reason.as_str())
        .increment(1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn all_lowercase_tokens(labels: &[&str]) -> bool {
        labels
            .iter()
            .all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
    }

    #[test]
    fn metrics_labels_are_low_cardinality() {
        // Every label comes from a closed list, and each list is small.
        let kinds = [
            BrowserFailureKind::PlannerUnavailable,
            BrowserFailureKind::RepeatedFailure,
            BrowserFailureKind::NoProgress,
            BrowserFailureKind::Blocked,
            BrowserFailureKind::Incomplete,
        ]
        .map(BrowserFailureKind::as_str);
        let reasons = [
            DeterministicReason::ModeOff,
            DeterministicReason::Zdr,
            DeterministicReason::OrgOff,
            DeterministicReason::NotAllowlisted,
            DeterministicReason::OrgNotOptedIn,
            DeterministicReason::OrgPolicyUnavailable,
        ]
        .map(DeterministicReason::as_str);
        for (name, labels) in [
            ("planner", &PLANNER_LABELS[..]),
            ("outcome", &OUTCOME_LABELS[..]),
            ("direction", &DIRECTION_LABELS[..]),
            ("kind", &kinds[..]),
            ("reason", &reasons[..]),
        ] {
            assert!(labels.len() <= 10, "{name} has {} values", labels.len());
            assert!(all_lowercase_tokens(labels), "{name}: {labels:?}");
            let distinct: BTreeSet<&&str> = labels.iter().collect();
            assert_eq!(distinct.len(), labels.len(), "{name} repeats a value");
        }
        assert_eq!(planner_label(true), "llm");
        assert_eq!(planner_label(false), "deterministic");
        // The recorders accept only these typed inputs; with no recorder
        // installed they are no-ops, so calling them is safe in a test.
        record_run(planner_label(true), OUTCOME_LABELS[0]);
        record_planner_tokens(BrowserLoopUsage {
            planner_calls: 1,
            planner_input_tokens: 10,
            planner_output_tokens: 5,
        });
        record_failure(BrowserFailureKind::NoProgress);
        record_deterministic(DeterministicReason::OrgOff);
    }
}
