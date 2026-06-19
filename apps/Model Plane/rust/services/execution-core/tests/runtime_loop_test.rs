//! Unit tests for execution-core runtime loop decisions.

use execution_core::artifact;
use execution_core::runtime_loop;

#[tokio::test]
async fn deny_mode_blocks_execution() {
    let outcome = runtime_loop::execute_step("echo", "payload", "deny", "", "", None).await;
    assert_eq!(outcome.status, "permission_denied");
    assert!(!outcome.error.is_empty());
}

#[tokio::test]
async fn ask_mode_requires_approval_for_tools() {
    // `ask` mode gates risky/destructive tools behind human approval; benign
    // reads proceed (see permission::evaluate). Use a risky tool name so this
    // exercises the AwaitApproval path.
    let outcome =
        runtime_loop::execute_step("delete_account", "payload", "ask", "", "", None).await;
    assert_eq!(outcome.status, "awaiting_approval");
}

#[tokio::test]
async fn auto_mode_executes_tool() {
    let outcome = runtime_loop::execute_step("echo", "hello", "auto", "", "", None).await;
    assert_eq!(outcome.status, "completed");
    assert_eq!(outcome.output, "hello");
}

#[tokio::test]
async fn hook_can_block_step() {
    let hook_context = r#"{"block_execution": true}"#;
    let outcome = runtime_loop::execute_step("echo", "hello", "auto", hook_context, "", None).await;
    assert_eq!(outcome.status, "failed");
    assert!(outcome.error.contains("hook"));
}

#[tokio::test]
async fn fail_tool_returns_error() {
    let outcome = runtime_loop::execute_step("fail", "payload", "auto", "", "", None).await;
    assert_eq!(outcome.status, "failed");
    assert!(!outcome.error.is_empty());
}

#[tokio::test]
async fn subagent_spawn_appends_summary() {
    let outcome = runtime_loop::execute_step("subagent.xyz", "payload", "auto", "", "", None).await;
    assert_eq!(outcome.status, "completed");
    assert!(outcome.output.contains(" ["));
}

#[tokio::test]
async fn compaction_triggers_when_output_large() {
    let big = "x".repeat(2100);
    let outcome = runtime_loop::execute_step("echo", &big, "auto", "", "", None).await;
    assert_eq!(outcome.status, "completed");
    assert!(outcome.compaction_triggered);
}

#[tokio::test]
async fn reasoning_step_without_tool() {
    let outcome = runtime_loop::execute_step("", "payload", "auto", "", "", None).await;
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
