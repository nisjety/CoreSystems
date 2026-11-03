//! Hook engine — user/org-configurable policy that runs *around* every tool
//! call, mirroring Claude Code's `PreToolUse` / `PostToolUse` model.
//!
//! A hook rule matches a tool (exact name, a `prefix*` glob, or `*` for any) for
//! a given [`HookEvent`] and yields a [`HookDecision`]:
//! - `deny`  — refuse the call (`PreToolUse`) or reject the result (`PostToolUse`).
//! - `ask`   — pause the run for human approval (routes into the existing HITL
//!   path in `runtime_loop::execute_step`).
//! - `allow` — no effect. It never overrides a deny, so the gateway does not store it.
//!
//! Rules arrive on the `hook_context` JSON that flows into `ExecuteStep`, and on
//! `RunAgentRequest.org_policy` for a Do run, so the orchestration layer can supply
//! them per run without a schema change here. The legacy `block_execution` boolean
//! is preserved: `true` denies every pre-tool call.
//!
//! Strictest wins (DEC-15): every matching rule is consulted and the verdict is
//! deny over ask over allow, so no rule order lets an allow hide a later deny. A
//! decision token the engine does not know is an ask, never an allow. A malformed
//! context allows every call, unless `HOOKS_FAIL_CLOSED=true` makes it a deny. No
//! matching rule gives `Allow`, so hooks stay additive policy.

use serde::Deserialize;

/// The reason a malformed, non-empty context is denied while `HOOKS_FAIL_CLOSED` is on.
const MALFORMED_CONTEXT_REASON: &str = "hook_context_malformed";

/// The lifecycle point at which a hook fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// Before a tool is dispatched — can block or require approval.
    PreToolUse,
    /// After a tool produced a successful result — can reject the result.
    PostToolUse,
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
    /// `"allow"` | `"deny"` | `"ask"` (case-insensitive). Anything else is `"ask"`.
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
    /// Set for `HOOKS_DO_RUNS=shadow`: verdicts are computed and logged, and nothing is enforced.
    #[serde(default)]
    observe_only: bool,
    #[serde(default)]
    rules: Vec<HookRule>,
}

fn event_matches(rule_event: &str, event: HookEvent) -> bool {
    let normalized = rule_event
        .trim()
        .to_ascii_lowercase()
        .replace(['-', ' '], "_");
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
        "allow" => HookDecision::Allow,
        "deny" | "block" => HookDecision::Deny { reason },
        // "ask", "approve" and "confirm" ask, and so does anything unrecognised: a typo
        // in a rule must not open a tool.
        _ => HookDecision::Ask { reason },
    }
}

/// The verdict of every rule that matches `event` and `tool_name`. A deny is returned as soon as
/// one matches; otherwise the first ask stands; otherwise the call is allowed.
fn strictest_verdict(rules: &[HookRule], event: HookEvent, tool_name: &str) -> HookDecision {
    let mut verdict = HookDecision::Allow;
    for rule in rules {
        if !(event_matches(&rule.event, event) && tool_matches(&rule.tools, tool_name)) {
            continue;
        }
        match decision_of(rule) {
            deny @ HookDecision::Deny { .. } => return deny,
            ask @ HookDecision::Ask { .. } if verdict == HookDecision::Allow => verdict = ask,
            _ => {}
        }
    }
    verdict
}

/// Evaluate the hook rules in `raw_context` for `event` against `tool_name`.
/// The `HOOKS_FAIL_CLOSED` switch is read once per process.
#[must_use]
pub fn evaluate(raw_context: &str, event: HookEvent, tool_name: &str) -> HookDecision {
    evaluate_with(raw_context, event, tool_name, fail_closed_switch())
}

/// [`evaluate`] with the fail-closed switch passed in, so a caller and a test can pin it.
#[must_use]
pub fn evaluate_with(
    raw_context: &str,
    event: HookEvent,
    tool_name: &str,
    fail_closed: bool,
) -> HookDecision {
    if raw_context.trim().is_empty() {
        return HookDecision::Allow;
    }
    let Ok(ctx) = serde_json::from_str::<HookContext>(raw_context) else {
        // With the switch off a malformed context allows every call, as the engine always
        // did. The permission engine stays the authoritative gate either way.
        return if fail_closed {
            HookDecision::Deny {
                reason: MALFORMED_CONTEXT_REASON.to_owned(),
            }
        } else {
            HookDecision::Allow
        };
    };
    // The legacy blanket block applies only to the pre-tool phase.
    let verdict = if ctx.block_execution && event == HookEvent::PreToolUse {
        HookDecision::Deny {
            reason: "blocked by pre-tool hook".to_owned(),
        }
    } else {
        strictest_verdict(&ctx.rules, event, tool_name)
    };
    if ctx.observe_only {
        if verdict != HookDecision::Allow {
            tracing::info!(
                tool = tool_name,
                ?event,
                ?verdict,
                "hook rule would have decided; observe only, nothing enforced"
            );
        }
        return HookDecision::Allow;
    }
    verdict
}

/// `HOOKS_FAIL_CLOSED` (WS25-S3), read once per process. Only the literal `true` turns it on. It
/// defaults off, so a deploy cannot start denying calls before the S3 proof is recorded.
fn fail_closed_switch() -> bool {
    static SWITCH: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SWITCH.get_or_init(|| matches!(std::env::var("HOOKS_FAIL_CLOSED").as_deref(), Ok("true")))
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
        assert_eq!(
            evaluate("", HookEvent::PreToolUse, "shell"),
            HookDecision::Allow
        );
    }

    #[test]
    fn legacy_block_execution_denies_pre_tool_only() {
        let ctx = r#"{"block_execution": true}"#;
        assert!(matches!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Deny { .. }
        ));
        // PostToolUse is unaffected by the legacy pre-tool block.
        assert_eq!(
            evaluate(ctx, HookEvent::PostToolUse, "shell"),
            HookDecision::Allow
        );
        assert!(is_blocked(ctx));
    }

    #[test]
    fn deny_rule_matches_exact_tool() {
        let ctx = r#"{"rules":[{"event":"pre_tool_use","tools":"shell","decision":"deny","reason":"no shell"}]}"#;
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Deny {
                reason: "no shell".to_owned()
            }
        );
        // A different tool is unaffected.
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "web_search"),
            HookDecision::Allow
        );
    }

    #[test]
    fn ask_rule_with_prefix_matcher_and_camelcase_event() {
        let ctx = r#"{"rules":[{"event":"PreToolUse","tools":"mcp__*","decision":"ask","reason":"review MCP"}]}"#;
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "mcp__github__create_issue"),
            HookDecision::Ask {
                reason: "review MCP".to_owned()
            }
        );
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Allow
        );
    }

    #[test]
    fn strictest_rule_wins_whatever_the_order() {
        // A deny beats an earlier allow, and an ask beats an earlier allow.
        let allow_then_deny = r#"{"rules":[
            {"event":"pre_tool_use","tools":"web_search","decision":"allow"},
            {"event":"pre_tool_use","tools":"*","decision":"deny","reason":"default deny"}
        ]}"#;
        assert_eq!(
            evaluate(allow_then_deny, HookEvent::PreToolUse, "web_search"),
            HookDecision::Deny {
                reason: "default deny".to_owned()
            }
        );
        // A deny beats an earlier ask, and the ask does not take its place.
        let ask_then_deny = r#"{"rules":[
            {"event":"pre_tool_use","tools":"shell","decision":"ask","reason":"confirm"},
            {"event":"pre_tool_use","tools":"shell","decision":"deny","reason":"no shell"}
        ]}"#;
        assert_eq!(
            evaluate(ask_then_deny, HookEvent::PreToolUse, "shell"),
            HookDecision::Deny {
                reason: "no shell".to_owned()
            }
        );
        let ask_then_allow = r#"{"rules":[
            {"event":"pre_tool_use","tools":"shell","decision":"ask","reason":"confirm"},
            {"event":"pre_tool_use","tools":"shell","decision":"allow"}
        ]}"#;
        assert_eq!(
            evaluate(ask_then_allow, HookEvent::PreToolUse, "shell"),
            HookDecision::Ask {
                reason: "confirm".to_owned()
            }
        );
    }

    #[test]
    fn allow_never_overrides_a_later_deny() {
        let ctx = r#"{"rules":[
            {"event":"pre_tool_use","tools":"shell","decision":"allow"},
            {"event":"pre_tool_use","tools":"shell","decision":"deny","reason":"no shell"}
        ]}"#;
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Deny {
                reason: "no shell".to_owned()
            }
        );
        // With no deny to override, an allow is simply an allow.
        let alone = r#"{"rules":[{"event":"pre_tool_use","tools":"shell","decision":"allow"}]}"#;
        assert_eq!(
            evaluate(alone, HookEvent::PreToolUse, "shell"),
            HookDecision::Allow
        );
    }

    #[test]
    fn an_unknown_decision_token_asks_and_an_empty_one_does_too() {
        let unknown = r#"{"rules":[{"event":"pre_tool_use","tools":"shell","decision":"quarantine","reason":"review"}]}"#;
        assert_eq!(
            evaluate(unknown, HookEvent::PreToolUse, "shell"),
            HookDecision::Ask {
                reason: "review".to_owned()
            }
        );
        let empty = r#"{"rules":[{"event":"pre_tool_use","tools":"shell"}]}"#;
        assert!(matches!(
            evaluate(empty, HookEvent::PreToolUse, "shell"),
            HookDecision::Ask { .. }
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
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "web_fetch"),
            HookDecision::Allow
        );
    }

    #[test]
    fn malformed_context_fails_closed_when_the_switch_is_on() {
        for raw in ["not json", r#"{"rules":"oops"}"#] {
            assert_eq!(
                evaluate_with(raw, HookEvent::PreToolUse, "shell", true),
                HookDecision::Deny {
                    reason: MALFORMED_CONTEXT_REASON.to_owned()
                },
                "{raw}"
            );
        }
    }

    #[test]
    fn malformed_context_allows_while_the_switch_is_off() {
        assert_eq!(
            evaluate_with("not json", HookEvent::PreToolUse, "shell", false),
            HookDecision::Allow
        );
    }

    #[test]
    fn observe_only_enforces_nothing() {
        let ctx = r#"{"observe_only":true,"rules":[{"event":"pre_tool_use","tools":"shell","decision":"deny","reason":"no shell"}]}"#;
        assert_eq!(
            evaluate(ctx, HookEvent::PreToolUse, "shell"),
            HookDecision::Allow
        );
    }
}
