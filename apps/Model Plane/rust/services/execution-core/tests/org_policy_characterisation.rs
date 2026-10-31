//! Characterisation goldens for the org hook and permission policy as it stands
//! before WS25 changes it.
//!
//! Every assertion here pins a behaviour that a later WS25 slice reverses on
//! purpose: the Do loop sending an empty hook context (S3), `from_wire` mapping
//! an unknown mode to `auto` (S3), `hook::evaluate` taking the first matching
//! rule with no strictest-wins (S3), and a malformed hook context failing open
//! (S3). The slice that changes one of these changes its golden in the same
//! change and records the decision in the ledger. A golden that starts failing
//! without such a change is a regression, not a stale test.
//!
//! `resolve_mode` and the Do loop's dispatch call are private to
//! `runtime_loop/agent.rs`, so their goldens read that source at run time, the
//! way `zdr_refusal_inventory.rs` reads this crate's `src` tree.

use std::path::Path;

use execution_core::hook::{evaluate, HookDecision, HookEvent};
use execution_core::permission::PermissionMode;

fn execution_core_source(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()))
        // The working copy is CRLF on Windows; the scans compare LF text.
        .replace("\r\n", "\n")
}

/// The text of the function whose signature starts with `signature`, up to its
/// closing brace at column zero.
fn function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` is in the source"));
    let rest = &source[start..];
    let end = rest
        .find("\n}\n")
        .expect("the function closes at column zero");
    &rest[..end]
}

#[test]
fn from_wire_maps_only_the_literal_ask_and_deny_and_everything_else_to_auto() {
    let table = [
        ("ask", PermissionMode::Ask),
        ("deny", PermissionMode::Deny),
        ("auto", PermissionMode::Auto),
        ("", PermissionMode::Auto),
        ("execute", PermissionMode::Auto),
        ("ASK", PermissionMode::Auto),
        ("malformed", PermissionMode::Auto),
    ];
    for (wire, expected) in table {
        assert_eq!(
            PermissionMode::from_wire(wire),
            expected,
            "from_wire({wire:?})"
        );
    }
}

#[test]
fn the_do_loop_resolve_mode_is_fail_closed_to_ask_and_is_applied_to_the_request_mode() {
    let source = execution_core_source("runtime_loop/agent.rs");
    let body = function_body(&source, "fn resolve_mode(wire: &str) -> PermissionMode {");
    let arms: Vec<&str> = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect();
    // Table: "auto" -> Auto, "deny" -> Deny, every other wire value -> Ask. Note
    // that this differs from `from_wire` above for "" and every unknown value.
    assert_eq!(
        arms,
        [
            "fn resolve_mode(wire: &str) -> PermissionMode {",
            "match wire {",
            "\"auto\" => PermissionMode::Auto,",
            "\"deny\" => PermissionMode::Deny,",
            "_ => PermissionMode::Ask,",
            "}",
        ]
    );
    assert!(
        source.contains("resolve_mode(&req.mode)"),
        "the request's mode reaches the loop through resolve_mode"
    );
}

#[test]
fn the_do_loop_dispatches_every_tool_call_with_an_empty_hook_context() {
    let source = execution_core_source("runtime_loop/agent.rs");
    let header = "runtime_loop::execute_step_with_subagent(\n";
    let dispatches: Vec<&str> = source
        .match_indices(header)
        .map(|(at, _)| &source[at + header.len()..])
        .filter(|rest| rest.lines().next().map(str::trim) == Some("&call.name,"))
        .collect();
    assert_eq!(
        dispatches.len(),
        1,
        "the Do loop has exactly one tool dispatch"
    );
    // The fourth argument is `hook_context`. The Do loop passes the literal empty
    // string, so no org hook rule reaches a Do run's tool calls today.
    let arguments: Vec<&str> = dispatches[0].lines().take(4).map(str::trim).collect();
    assert_eq!(
        arguments,
        [
            "&call.name,",
            "&call.arguments_json,",
            "ctx.permission_wire,",
            "\"\",",
        ]
    );
}

#[test]
fn an_empty_or_blank_hook_context_allows_every_call() {
    for raw in ["", "   ", "\n\t"] {
        assert_eq!(
            evaluate(raw, HookEvent::PreToolUse, "shell"),
            HookDecision::Allow,
            "{raw:?}"
        );
        assert_eq!(
            evaluate(raw, HookEvent::PostToolUse, "shell"),
            HookDecision::Allow,
            "{raw:?}"
        );
    }
}

#[test]
fn a_malformed_hook_context_allows_every_call_and_never_locks_a_tool_out() {
    for raw in [
        "not json",
        r#"{"rules":"oops"}"#,
        r#"{"rules":[{"event":"pre_tool_use","tools":"*","decision":"deny""#,
    ] {
        assert_eq!(
            evaluate(raw, HookEvent::PreToolUse, "shell"),
            HookDecision::Allow,
            "{raw}"
        );
    }
}

#[test]
fn the_first_matching_rule_decides_and_a_later_deny_cannot_override_an_earlier_allow() {
    let allow_first = r#"{"rules":[
        {"event":"pre_tool_use","tools":"shell","decision":"allow"},
        {"event":"pre_tool_use","tools":"*","decision":"deny","reason":"default deny"}
    ]}"#;
    assert_eq!(
        evaluate(allow_first, HookEvent::PreToolUse, "shell"),
        HookDecision::Allow
    );

    let deny_first = r#"{"rules":[
        {"event":"pre_tool_use","tools":"*","decision":"deny","reason":"default deny"},
        {"event":"pre_tool_use","tools":"shell","decision":"allow"}
    ]}"#;
    assert_eq!(
        evaluate(deny_first, HookEvent::PreToolUse, "shell"),
        HookDecision::Deny {
            reason: "default deny".to_owned()
        }
    );
}

#[test]
fn an_earlier_ask_outranks_a_later_deny_for_the_same_tool() {
    let ask_then_deny = r#"{"rules":[
        {"event":"pre_tool_use","tools":"shell","decision":"ask","reason":"confirm"},
        {"event":"pre_tool_use","tools":"shell","decision":"deny","reason":"never"}
    ]}"#;
    assert_eq!(
        evaluate(ask_then_deny, HookEvent::PreToolUse, "shell"),
        HookDecision::Ask {
            reason: "confirm".to_owned()
        }
    );
}

#[test]
fn an_unknown_decision_token_is_an_allow() {
    let ctx = r#"{"rules":[{"event":"pre_tool_use","tools":"shell","decision":"quarantine","reason":"x"}]}"#;
    assert_eq!(
        evaluate(ctx, HookEvent::PreToolUse, "shell"),
        HookDecision::Allow
    );
}

#[test]
fn a_rule_without_a_tools_matcher_matches_every_tool() {
    let ctx = r#"{"rules":[{"event":"pre_tool_use","decision":"deny","reason":"all"}]}"#;
    assert_eq!(
        evaluate(ctx, HookEvent::PreToolUse, "web_search"),
        HookDecision::Deny {
            reason: "all".to_owned()
        }
    );
}

#[test]
fn a_prefix_matcher_requires_the_prefix_and_an_exact_matcher_is_not_a_prefix() {
    let ctx = r#"{"rules":[
        {"event":"pre_tool_use","tools":"mcp__*","decision":"deny","reason":"mcp"},
        {"event":"pre_tool_use","tools":"shell","decision":"deny","reason":"shell"}
    ]}"#;
    assert!(matches!(
        evaluate(ctx, HookEvent::PreToolUse, "mcp__github__create_issue"),
        HookDecision::Deny { .. }
    ));
    assert_eq!(
        evaluate(ctx, HookEvent::PreToolUse, "mcp"),
        HookDecision::Allow
    );
    assert_eq!(
        evaluate(ctx, HookEvent::PreToolUse, "shell_exec"),
        HookDecision::Allow
    );
}

#[test]
fn both_event_spellings_match_and_a_rule_never_fires_on_the_other_phase() {
    let camel = r#"{"rules":[{"event":"PreToolUse","tools":"*","decision":"deny","reason":"camel"}]}"#;
    assert!(matches!(
        evaluate(camel, HookEvent::PreToolUse, "shell"),
        HookDecision::Deny { .. }
    ));
    assert_eq!(
        evaluate(camel, HookEvent::PostToolUse, "shell"),
        HookDecision::Allow
    );
}

#[test]
fn legacy_block_execution_denies_only_the_pre_tool_phase_with_a_fixed_reason() {
    let ctx = r#"{"block_execution": true}"#;
    assert_eq!(
        evaluate(ctx, HookEvent::PreToolUse, "shell"),
        HookDecision::Deny {
            reason: "blocked by pre-tool hook".to_owned()
        }
    );
    assert_eq!(
        evaluate(ctx, HookEvent::PostToolUse, "shell"),
        HookDecision::Allow
    );
}
