//! Runtime loop orchestration for one execution step.

use crate::hook;
use crate::permission::{self, PermissionDecision, PermissionMode};
use crate::subagent;
use crate::tool_bridge;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub status: String,
    pub output: String,
    pub error: String,
    pub compaction_triggered: bool,
}

impl StepOutcome {
    fn completed(output: String) -> Self {
        Self {
            status: "completed".to_owned(),
            output,
            error: String::new(),
            compaction_triggered: false,
        }
    }

    fn failed(error: &str) -> Self {
        Self {
            status: "failed".to_owned(),
            output: String::new(),
            error: error.to_owned(),
            compaction_triggered: false,
        }
    }

    fn awaiting_approval() -> Self {
        Self {
            status: "awaiting_approval".to_owned(),
            output: String::new(),
            error: String::new(),
            compaction_triggered: false,
        }
    }

    fn permission_denied() -> Self {
        Self {
            status: "permission_denied".to_owned(),
            output: String::new(),
            error: "permission denied by policy".to_owned(),
            compaction_triggered: false,
        }
    }
}

pub fn execute_step(
    tool_name: &str,
    tool_input: &str,
    permission_mode: &str,
    hook_context: &str,
) -> StepOutcome {
    if hook::is_blocked(hook_context) {
        return StepOutcome::failed("blocked by pre-tool hook");
    }

    let mode = PermissionMode::from_wire(permission_mode);
    match permission::evaluate(mode, tool_name) {
        PermissionDecision::Deny => return StepOutcome::permission_denied(),
        PermissionDecision::AwaitApproval => return StepOutcome::awaiting_approval(),
        PermissionDecision::Allow => {}
    }

    let subagent_note = subagent::maybe_spawn(tool_name)
        .map(|entry| format!(" [{}]", entry.summary))
        .unwrap_or_default();

    let exec = tool_bridge::execute(tool_name, tool_input);
    if let Some(error) = exec.error {
        return StepOutcome::failed(&error);
    }

    let output = format!("{}{}", exec.output, subagent_note);
    let mut outcome = StepOutcome::completed(output.clone());
    outcome.compaction_triggered = output.len() > 2048;
    outcome
}
