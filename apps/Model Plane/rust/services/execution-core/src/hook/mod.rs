//! Hook engine — user/org-configurable policy that runs *around* every tool
//! call, mirroring Claude Code's `PreToolUse` / `PostToolUse` model.
//!
//! A hook rule matches a tool (exact name, a `prefix*` glob, or `*` for any) for
//! a given [`HookEvent`] and yields a [`HookDecision`]:
//! - `Allow`  — proceed (defers to the permission engine).
//! - `Deny`   — refuse the call (PreToolUse) or reject the result (PostToolUse).
//! - `Ask`    — pause the run for human approval (routes into the existing HITL
//!              path in `runtime_loop::execute_step`).
//!
//! Rules arrive on the `hook_context` JSON that flows into `ExecuteStep`, so the
//! orchestration layer (or a future durable capability-core hooks registry) can
//! supply them per run without a schema change here. The legacy
//! `block_execution` boolean is preserved: `true` denies every pre-tool call.
//!
//! First matching rule wins (rule order is significant). No rule matching an
//! event => `Allow`, so hooks are purely additive policy and a malformed or
//! empty context never locks tools out.

use serde::Deserialize;

/// The lifecycle point at which a hook fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// Before a tool is dispatched — can block or require approval.
    PreToolUse,
    /// After a tool produced a successful result — can reject the result.
    PostToolUse,
}

impl HookEvent {
    /// The wire token used in a rule's `event` field.
    fn wire(self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "pre_tool_use",
            HookEvent::PostToolUse => "post_tool_use",
        }
    }
}

/// The outcome of evaluating the hook rules for one event + tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    Allow,
    Deny { reason: String },
    Ask { reason: String },
}

#[derive(Debug, Deserialize)]
struct HookRule {
    /// `"pre_tool_use"` | `"post_tool_use"` (case-insensitive; also accepts the
    /// camelCase `PreToolUse`/`PostToolUse` spellings).
    #[serde(default)]
    event: String,
    /// Tool matcher: `"*"` (any), a `"prefix*"` glob, or an exact tool name.
    #[serde(default)]
    tools: String,
    /// `"allow"` | `"deny"` | `"ask"` (case-insensitive; unknown => allow).
    #[serde(default)]
    decision: String,
    /// Human-readable reason surfaced on deny/ask.
    #[serde(default)]
    reason: String,
}

#[derive(Debug, Default, Deserialize)]
struct HookContext {
    /// Legacy blanket pre-tool block, retained for back-compat.
    #[serde(default)]
    block_execution: bool,
    #[serde(default)]
    rules: Vec<HookRule>,
}

fn event_matches(rule_event: &str, event: HookEvent) -> bool {
    let normalized = rule_event.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    match event {
        HookEvent::PreToolUse => normalized == "pre_tool_use" || normalized == "pretooluse",
        HookEvent::PostToolUse => normalized == "post_tool_use" || normalized == "posttooluse",
    }
}

fn tool_matches(matcher: &str, tool_name: &str) -> bool {
    let matcher = matcher.trim();
    if matcher.is_empty() || matcher == "*" {
        return true;
    }
    if let Some(prefix) = matcher.strip_suffix('*') {
        return tool_name.starts_with(prefix);
    }
    matcher == tool_name
}

fn decision_of(rule: &HookRule) -> HookDecision {
    let reason = if rule.reason.trim().is_empty() {
        format!("hook policy for '{}'", rule.tools)
    } else {
        rule.reason.trim().to_owned()
    };
    match rule.decision.trim().to_ascii_lowercase().as_str() {
        "deny" | "block" => HookDecision::Deny { reason },
        "ask" | "approve" | "confirm" => HookDecision::Ask { reason },
        _ => HookDecision::Allow,
    }
}

/// Evaluate the hook rules in `raw_context` for `event` against `tool_name`.
/// Returns the first matching rule's decision, else `Allow`.
#[must_use]
pub fn evaluate(raw_context: &str, event: HookEvent, tool_name: &str) -> HookDecision {
    if raw_context.trim().is_empty() {
        return HookDecision::Allow;
    }
    let Ok(ctx) = serde_json::from_str::<HookContext>(raw_context) else {
        // A malformed context is never allowed to block tools (fail-open policy
        // layer): the permission engine remains the authoritative gate.
        return HookDecision::Allow;
    };
    // Legacy blanket block applies only to the pre-tool phase.
    if ctx.block_execution && event == HookEvent::PreToolUse {
        return HookDecision::Deny {
            reason: "blocked by pre-tool hook".to_owned(),
        };
    }
    for rule in &ctx.rules {
        if event_matches(&rule.event, event) && tool_matches(&rule.tools, tool_name) {
            return decision_of(rule);
        }
    }
    let _ = event.wire(); // keep wire() referenced for the (documented) token names
    HookDecision::Allow
}

/// Back-compat: whether the pre-tool phase denies `tool_name` outright. Kept so
/// existing callers keep compiling; new code should use [`evaluate`] directly.
#[must_use]
pub fn is_blocked(raw_context: &str) -> bool {
    // A pre-existing caller that passes no tool name checks the blanket block.
    matches!(
        evaluate(raw_context, HookEvent::PreToolUse, "*"),
        HookDecision::Deny { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_context_allows() {
        assert_eq!(evaluate("", HookEvent::PreToolUse, "shell"), HookDecision::Allow);
    }

    #[test]
    fn legacy_block_execution_denies_pre_tool_only() {
        let ctx = r#"{"block_execution": true}"#;
        assert!(matches!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Deny { .. }
        ));
        // PostToolUse is unaffected by the legacy pre-tool block.
        assert_eq!(evaluate(ctx, HookEvent::PostToolUse, "shell"), HookDecision::Allow);
        assert!(is_blocked(ctx));
    }

    #[test]
    fn deny_rule_matches_exact_tool() {
        let ctx = r#"{"rules":[{"event":"pre_tool_use","tools":"shell","decision":"deny","reason":"no shell"}]}"#;
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Deny { reason: "no shell".to_owned() }
        );
        // A different tool is unaffected.
        assert_eq!(evaluate(ctx, HookEvent::PreToolUse, "web_search"), HookDecision::Allow);
    }

    #[test]
    fn ask_rule_with_prefix_matcher_and_camelcase_event() {
        let ctx = r#"{"rules":[{"event":"PreToolUse","tools":"mcp__*","decision":"ask","reason":"review MCP"}]}"#;
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "mcp__github__create_issue"),
            HookDecision::Ask { reason: "review MCP".to_owned() }
        );
        assert_eq!(evaluate(ctx, HookEvent::PreToolUse, "shell"), HookDecision::Allow);
    }

    #[test]
    fn wildcard_and_first_match_wins() {
        let ctx = r#"{"rules":[
            {"event":"pre_tool_use","tools":"web_search","decision":"allow"},
            {"event":"pre_tool_use","tools":"*","decision":"deny","reason":"default deny"}
        ]}"#;
        // Explicit allow for web_search wins over the later wildcard deny.
        assert_eq!(evaluate(ctx, HookEvent::PreToolUse, "web_search"), HookDecision::Allow);
        // Everything else hits the wildcard deny.
        assert!(matches!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Deny { .. }
        ));
    }

    #[test]
    fn post_tool_use_can_reject_result() {
        let ctx = r#"{"rules":[{"event":"post_tool_use","tools":"*","decision":"deny","reason":"output policy"}]}"#;
        assert!(matches!(
            evaluate(ctx, HookEvent::PostToolUse, "web_fetch"),
            HookDecision::Deny { .. }
        ));
        // Pre-tool phase is unaffected by a post-tool rule.
        assert_eq!(evaluate(ctx, HookEvent::PreToolUse, "web_fetch"), HookDecision::Allow);
    }

    #[test]
    fn malformed_context_fails_open() {
        assert_eq!(evaluate("not json", HookEvent::PreToolUse, "shell"), HookDecision::Allow);
    }
}
