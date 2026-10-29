//! Canonical registry of Temporal task queues, workflow names, and activity
//! names for the Model Plane. Rust counterpart to Go `pkg/temporalreg`; both
//! must stay in sync.

use std::collections::BTreeSet;

/// Canonical Temporal task queue names in declaration order.
pub const TASK_QUEUES: &[&str] = &["mp-session", "mp-inference", "mp-execution"];

/// Workflow type names registered on the `mp-session` task queue.
pub const SESSION_WORKFLOWS: &[&str] = &["SessionWorkflow", "RunWorkflow"];
/// Workflow type names registered on the `mp-inference` task queue.
pub const INFERENCE_WORKFLOWS: &[&str] = &["InferenceWorkflow"];
/// Workflow type names registered on the `mp-execution` task queue.
pub const EXECUTION_WORKFLOWS: &[&str] = &["ExecutionWorkflow"];

/// Activity type names registered on the `mp-session` task queue.
pub const SESSION_ACTIVITIES: &[&str] = &[
    "PersistRunStart",
    "PersistRunEnd",
    "EmitEvent",
    "CreateCheckpoint",
];
/// Activity type names registered on the `mp-inference` task queue.
pub const INFERENCE_ACTIVITIES: &[&str] = &["InvokeModel", "RecordUsage", "EmitEvent"];
/// Activity type names registered on the `mp-execution` task queue.
pub const EXECUTION_ACTIVITIES: &[&str] = &["ExecuteStep", "ToolCallActivity", "EmitEvent"];

/// Returns the workflow type names registered on the given task queue.
pub fn workflows_for(task_queue: &str) -> Option<&'static [&'static str]> {
    match task_queue {
        "mp-session" => Some(SESSION_WORKFLOWS),
        "mp-inference" => Some(INFERENCE_WORKFLOWS),
        "mp-execution" => Some(EXECUTION_WORKFLOWS),
        _ => None,
    }
}

/// Returns the activity type names registered on the given task queue.
pub fn activities_for(task_queue: &str) -> Option<&'static [&'static str]> {
    match task_queue {
        "mp-session" => Some(SESSION_ACTIVITIES),
        "mp-inference" => Some(INFERENCE_ACTIVITIES),
        "mp-execution" => Some(EXECUTION_ACTIVITIES),
        _ => None,
    }
}

/// Returns the deduplicated, lexicographically sorted union of every activity
/// name registered across every task queue. Mirrors Go `temporalreg.AllActivities`.
pub fn all_activities() -> Vec<&'static str> {
    let mut set: BTreeSet<&'static str> = BTreeSet::new();
    for q in TASK_QUEUES {
        if let Some(acts) = activities_for(q) {
            for a in acts {
                set.insert(*a);
            }
        }
    }
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_queues_canonical() {
        assert_eq!(TASK_QUEUES, &["mp-session", "mp-inference", "mp-execution"]);
    }

    #[test]
    fn workflows_per_queue() {
        assert_eq!(workflows_for("mp-session"), Some(SESSION_WORKFLOWS));
        assert_eq!(workflows_for("mp-inference"), Some(INFERENCE_WORKFLOWS));
        assert_eq!(workflows_for("mp-execution"), Some(EXECUTION_WORKFLOWS));
        assert_eq!(workflows_for("unknown"), None);
        assert_eq!(SESSION_WORKFLOWS, &["SessionWorkflow", "RunWorkflow"]);
        assert_eq!(INFERENCE_WORKFLOWS, &["InferenceWorkflow"]);
        assert_eq!(EXECUTION_WORKFLOWS, &["ExecutionWorkflow"]);
    }

    #[test]
    fn activities_per_queue() {
        assert_eq!(
            SESSION_ACTIVITIES,
            &[
                "PersistRunStart",
                "PersistRunEnd",
                "EmitEvent",
                "CreateCheckpoint"
            ]
        );
        assert_eq!(
            INFERENCE_ACTIVITIES,
            &["InvokeModel", "RecordUsage", "EmitEvent"]
        );
        assert_eq!(
            EXECUTION_ACTIVITIES,
            &["ExecuteStep", "ToolCallActivity", "EmitEvent"]
        );
        assert_eq!(activities_for("unknown"), None);
    }

    #[test]
    fn all_activities_deduped_sorted() {
        let got = all_activities();
        let expected = vec![
            "CreateCheckpoint",
            "EmitEvent",
            "ExecuteStep",
            "InvokeModel",
            "PersistRunEnd",
            "PersistRunStart",
            "RecordUsage",
            "ToolCallActivity",
        ];
        assert_eq!(got, expected);
        assert_eq!(got.len(), 8);
    }
}
