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
    // Name-only evaluation. Kept for callers without the tool arguments; it
    // delegates to the call-aware path with empty input, so operation-nuanced
    // tools (e.g. `execute_provider_action`) fail safe (treated as risky) when
    // their arguments are unavailable.
    evaluate_call(mode, tool_name, "")
}

/// Operation-aware permission evaluation for a specific tool CALL. Identical to
/// [`evaluate`] but inspects `tool_input` so a tool whose risk depends on its
/// arguments is classified on the actual operation, not the tool name alone.
#[must_use]
pub fn evaluate_call(
    mode: PermissionMode,
    tool_name: &str,
    tool_input: &str,
) -> PermissionDecision {
    match mode {
        PermissionMode::Deny => PermissionDecision::Deny,
        // `ask` posture (deployed_agent profile) gates risky/destructive tools
        // behind a human approval; benign reads proceed so the bot isn't
        // pausing on every lookup.
        PermissionMode::Ask if is_risky_call(tool_name, tool_input) => {
            PermissionDecision::AwaitApproval
        }
        _ => PermissionDecision::Allow,
    }
}

/// Tool name for the provider-action bridge (`integration_tools`). Its risk is
/// operation-dependent, so it is classified by argument, not by name.
const EXECUTE_PROVIDER_ACTION_TOOL: &str = "execute_provider_action";

/// Operation-aware risk classification for a specific tool call. Extends
/// [`is_risky_tool`] (name-based) with argument inspection for tools whose risk
/// depends on their input.
///
/// `execute_provider_action` is classified by the `operation` embedded in the
/// input JSON, read-vs-write via the frozen actions-surface catalog
/// (`integration_tools::operation_is_write`): a READ proceeds under `ask`; a
/// WRITE — or a missing/unknown/uncatalogued operation (fail safe) — is risky.
/// Every other tool falls back to the name-based [`is_risky_tool`].
#[must_use]
pub fn is_risky_call(tool_name: &str, tool_input: &str) -> bool {
    if tool_name == EXECUTE_PROVIDER_ACTION_TOOL {
        return provider_action_is_risky(tool_input);
    }
    is_risky_tool(tool_name)
}

/// Classify an `execute_provider_action` call by its `operation`. A read is not
/// risky; a write, an unknown operation, or malformed input is risky (fail safe
/// — integration-corev2 leaves unmapped operations ungated, so exec-core must
/// gate anything it cannot positively classify as a read).
fn provider_action_is_risky(tool_input: &str) -> bool {
    let operation = serde_json::from_str::<serde_json::Value>(tool_input)
        .ok()
        .and_then(|v| {
            v.get("operation")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        });
    match operation {
        Some(op) => crate::integration_tools::operation_is_write(&op).unwrap_or(true),
        None => true,
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
        "delete",
        "remove",
        "drop",
        "write",
        "update",
        "patch",
        "create",
        "send",
        "email",
        "deploy",
        "payment",
        "refund",
        "charge",
        "exec",
        "shell",
        "post",
        "purge",
        "revoke",
        // book_shipment places a real freight order (money + a truck arriving)
        // — explicitly gated since no generic keyword above catches it.
        "transfer",
        "book_shipment",
        // execute_provider_action runs an arbitrary provider write (publish a
        // Page/Instagram post, send a WhatsApp/Messenger message, create an ad
        // campaign …). The "exec" keyword above already matches it, but list
        // it explicitly so the gate is intent-visible and survives a rename.
        // NOTE: per-operation nuance (read vs write) is applied by
        // `is_risky_call`/`provider_action_is_risky`, which run BEFORE this
        // name match; this entry is the fail-safe when arguments are absent.
        "execute_provider_action",
        // browser_agent drives a live agentic browser loop (navigate + click +
        // type + form submit) with real side effects on external sites — at
        // least as risky as any other write-capable tool. No generic keyword
        // above matches it, so gate it explicitly; under `ask` it pauses for a
        // human, under `auto` (chat) it runs as before.
        "browser_agent",
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
    fn provider_action_write_is_gated_discovery_is_not() {
        // execute_provider_action runs arbitrary provider writes → gated.
        assert!(is_risky_tool("execute_provider_action"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "execute_provider_action"),
            PermissionDecision::AwaitApproval
        );
        // list_provider_actions is read-only discovery → not gated.
        assert!(!is_risky_tool("list_provider_actions"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "list_provider_actions"),
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

    #[test]
    fn browser_agent_is_gated_under_ask() {
        // browser_agent drives real page side effects → treated as write-risky.
        assert!(is_risky_tool("browser_agent"));
        assert_eq!(
            evaluate(PermissionMode::Ask, "browser_agent"),
            PermissionDecision::AwaitApproval
        );
        // ...but not under the default `auto` (chat) posture.
        assert_eq!(
            evaluate(PermissionMode::Auto, "browser_agent"),
            PermissionDecision::Allow
        );
    }

    #[test]
    fn provider_action_gating_is_operation_aware() {
        let read = r#"{"connection_id":"c1","operation":"pages.list"}"#;
        let write = r#"{"connection_id":"c1","operation":"whatsapp.messages.send"}"#;
        let unknown = r#"{"connection_id":"c1","operation":"totally.unknown"}"#;

        // A READ operation proceeds under `ask` — no needless approval pause.
        assert!(!is_risky_call(EXECUTE_PROVIDER_ACTION_TOOL, read));
        assert_eq!(
            evaluate_call(PermissionMode::Ask, EXECUTE_PROVIDER_ACTION_TOOL, read),
            PermissionDecision::Allow
        );

        // A WRITE operation is gated under `ask`.
        assert!(is_risky_call(EXECUTE_PROVIDER_ACTION_TOOL, write));
        assert_eq!(
            evaluate_call(PermissionMode::Ask, EXECUTE_PROVIDER_ACTION_TOOL, write),
            PermissionDecision::AwaitApproval
        );

        // Unknown / uncatalogued operation → gated (fail safe).
        assert_eq!(
            evaluate_call(PermissionMode::Ask, EXECUTE_PROVIDER_ACTION_TOOL, unknown),
            PermissionDecision::AwaitApproval
        );
        // Malformed input → gated (fail safe).
        assert_eq!(
            evaluate_call(PermissionMode::Ask, EXECUTE_PROVIDER_ACTION_TOOL, "not json"),
            PermissionDecision::AwaitApproval
        );

        // Under `auto` nothing gates, read or write.
        assert_eq!(
            evaluate_call(PermissionMode::Auto, EXECUTE_PROVIDER_ACTION_TOOL, write),
            PermissionDecision::Allow
        );
    }
}
