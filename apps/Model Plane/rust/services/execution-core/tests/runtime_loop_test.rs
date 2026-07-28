//! Unit tests for execution-core runtime loop decisions.

use execution_core::artifact;
use execution_core::capability_policy::{CapabilityDecision, CapabilityPolicy};
use execution_core::runtime_loop;

struct AllowPolicy;

#[tonic::async_trait]
impl CapabilityPolicy for AllowPolicy {
    async fn evaluate(
        &self,
        _tool_name: &str,
        _run_id: &str,
        _org_id: &str,
    ) -> Result<CapabilityDecision, tonic::Status> {
        Ok(CapabilityDecision::Allow)
    }
}

static ALLOW_POLICY: AllowPolicy = AllowPolicy;

#[tokio::test]
async fn deny_mode_blocks_execution() {
    let outcome = runtime_loop::execute_step(
        "echo",
        "payload",
        "deny",
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
    assert_eq!(outcome.status, "permission_denied");
    assert!(!outcome.error.is_empty());
}

#[tokio::test]
async fn ask_mode_requires_approval_for_tools() {
    // `ask` mode gates risky/destructive tools behind human approval; benign
    // reads proceed (see permission::evaluate). Use a risky tool name so this
    // exercises the AwaitApproval path.
    let outcome = runtime_loop::execute_step(
        "delete_account",
        "payload",
        "ask",
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
    assert_eq!(outcome.status, "awaiting_approval");
}

#[tokio::test]
async fn auto_mode_executes_tool() {
    let outcome = runtime_loop::execute_step(
        "echo",
        "hello",
        "auto",
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
    assert_eq!(outcome.status, "completed");
    assert_eq!(outcome.output, "hello");
}

#[tokio::test]
async fn hook_can_block_step() {
    let hook_context = r#"{"block_execution": true}"#;
    let outcome = runtime_loop::execute_step(
        "echo",
        "hello",
        "auto",
        hook_context,
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
    assert_eq!(outcome.status, "failed");
    assert!(outcome.error.contains("hook"));
}

#[tokio::test]
async fn fail_tool_returns_error() {
    let outcome = runtime_loop::execute_step(
        "fail",
        "payload",
        "auto",
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
    assert_eq!(outcome.status, "failed");
    assert!(!outcome.error.is_empty());
}

#[tokio::test]
async fn subagent_without_a_loop_to_delegate_into_fails_closed() {
    // This entry point is the SINGLE-STEP path: it holds no inference channel,
    // tool allowlist, or round budget, so it cannot run a delegated agent. It
    // used to answer `completed` with a "spawned subagent.xyz" note appended,
    // which meant a model asking for delegated work got a fabricated success.
    let outcome = runtime_loop::execute_step(
        "subagent.xyz",
        r#"{"goal":"do the thing"}"#,
        "auto",
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
    assert_eq!(outcome.status, "failed");
    assert!(
        outcome.error.contains("subagent.xyz") && outcome.error.contains("agent run"),
        "the error must name the tool and why it cannot run: {}",
        outcome.error
    );
    assert!(
        outcome.output.is_empty(),
        "a refused delegation must not produce tool output: {}",
        outcome.output
    );
    assert!(
        !outcome.error.contains("spawned"),
        "never claim anything was spawned: {}",
        outcome.error
    );
}

#[tokio::test]
async fn compaction_triggers_when_output_large() {
    let big = "x".repeat(2100);
    let outcome = runtime_loop::execute_step(
        "echo",
        &big,
        "auto",
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
    assert_eq!(outcome.status, "completed");
    assert!(outcome.compaction_triggered);
}

#[tokio::test]
async fn reasoning_step_without_tool() {
    let outcome = runtime_loop::execute_step(
        "",
        "payload",
        "auto",
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        None,
        None,
        None,
        &ALLOW_POLICY,
    )
    .await;
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
