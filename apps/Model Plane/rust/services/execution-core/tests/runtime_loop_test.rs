//! Unit tests for execution-core runtime loop decisions.

use execution_core::artifact;
use execution_core::runtime_loop;

#[test]
fn deny_mode_blocks_execution() {
    let outcome = runtime_loop::execute_step("echo", "payload", "deny", "");
    assert_eq!(outcome.status, "permission_denied");
    assert!(!outcome.error.is_empty());
}

#[test]
fn ask_mode_requires_approval_for_tools() {
    let outcome = runtime_loop::execute_step("echo", "payload", "ask", "");
    assert_eq!(outcome.status, "awaiting_approval");
}

#[test]
fn auto_mode_executes_tool() {
    let outcome = runtime_loop::execute_step("echo", "hello", "auto", "");
    assert_eq!(outcome.status, "completed");
    assert_eq!(outcome.output, "hello");
}

#[test]
fn hook_can_block_step() {
    let hook_context = r#"{"block_execution": true}"#;
    let outcome = runtime_loop::execute_step("echo", "hello", "auto", hook_context);
    assert_eq!(outcome.status, "failed");
    assert!(outcome.error.contains("hook"));
}

#[test]
fn fail_tool_returns_error() {
    let outcome = runtime_loop::execute_step("fail", "payload", "auto", "");
    assert_eq!(outcome.status, "failed");
    assert!(!outcome.error.is_empty());
}

#[test]
fn subagent_spawn_appends_summary() {
    let outcome = runtime_loop::execute_step("subagent.xyz", "payload", "auto", "");
    assert_eq!(outcome.status, "completed");
    assert!(outcome.output.contains(" ["));
}

#[test]
fn compaction_triggers_when_output_large() {
    let big = "x".repeat(2100);
    let outcome = runtime_loop::execute_step("echo", &big, "auto", "");
    assert_eq!(outcome.status, "completed");
    assert!(outcome.compaction_triggered);
}

#[test]
fn reasoning_step_without_tool() {
    let outcome = runtime_loop::execute_step("", "payload", "auto", "");
    assert_eq!(outcome.status, "completed");
    assert_eq!(outcome.output, "reasoning_step_completed");
}

#[test]
fn artifact_key_format() {
    assert_eq!(
        artifact::build_artifact_key("r", "s"),
        "runs/r/steps/s.json"
    );
}
