//! Permission policy evaluation for tool execution.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Auto,
    Ask,
    Deny,
}

impl PermissionMode {
    #[must_use]
    pub fn from_wire(mode: &str) -> Self {
        match mode {
            "ask" => Self::Ask,
            "deny" => Self::Deny,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    AwaitApproval,
    Deny,
}

#[must_use]
pub fn evaluate(mode: PermissionMode, tool_name: &str) -> PermissionDecision {
    match mode {
        PermissionMode::Deny => PermissionDecision::Deny,
        // `ask` posture (deployed_agent profile) gates risky/destructive tools
        // behind a human approval; benign reads proceed so the bot isn't
        // pausing on every lookup.
        PermissionMode::Ask if is_risky_tool(tool_name) => PermissionDecision::AwaitApproval,
        _ => PermissionDecision::Allow,
    }
}

/// Heuristic for tools with side effects worth a human gate. Matches on
/// destructive/outbound verbs in the tool name.
#[must_use]
pub fn is_risky_tool(tool_name: &str) -> bool {
    const RISKY: &[&str] = &[
        "delete", "remove", "drop", "write", "update", "patch", "create", "send", "email",
        "deploy", "payment", "refund", "charge", "exec", "shell", "post", "purge", "revoke",
        "transfer",
    ];
    let lowered = tool_name.to_ascii_lowercase();
    RISKY.iter().any(|kw| lowered.contains(kw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_mode_always_denies() {
        assert_eq!(
            evaluate(PermissionMode::Deny, "read_doc"),
            PermissionDecision::Deny
        );
    }

    #[test]
    fn ask_gates_risky_allows_benign() {
        assert_eq!(
            evaluate(PermissionMode::Ask, "delete_account"),
            PermissionDecision::AwaitApproval
        );
        assert_eq!(
            evaluate(PermissionMode::Ask, "send_email"),
            PermissionDecision::AwaitApproval
        );
        // Benign read proceeds even under ask.
        assert_eq!(
            evaluate(PermissionMode::Ask, "search_knowledge"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn auto_allows_everything() {
        assert_eq!(
            evaluate(PermissionMode::Auto, "delete_account"),
            PermissionDecision::Allow
        );
    }
}
