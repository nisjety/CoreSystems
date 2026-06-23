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
///
/// External MCP tools (`mcp__<server>__<tool>`) are gated unconditionally: the
/// server is third-party and the remote tool's side effects are unverifiable
/// from the name alone, so under the `ask` (`deployed_agent`) posture a human
/// approves each external call by default. Under `auto` (chat) posture nothing
/// is gated, so this does not change the default chat experience.
#[must_use]
pub fn is_risky_tool(tool_name: &str) -> bool {
    const RISKY: &[&str] = &[
        "delete", "remove", "drop", "write", "update", "patch", "create", "send", "email",
        "deploy", "payment", "refund", "charge", "exec", "shell", "post", "purge", "revoke",
        "transfer",
    ];
    if tool_name.starts_with("mcp__") {
        return true;
    }
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

    #[test]
    fn external_mcp_tools_are_gated_under_ask() {
        // Even a read-ish remote name (no risky verb) is gated under `ask`,
        // because the external server's behavior is unverifiable.
        assert!(is_risky_tool("mcp__fakemcp__echo"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "mcp__fakemcp__echo"),
            PermissionDecision::AwaitApproval
        );
        // ...but not gated under the default `auto` (chat) posture.
        assert_eq!(
            evaluate(PermissionMode::Auto, "mcp__fakemcp__echo"),
            PermissionDecision::Allow
        );
    }
}
