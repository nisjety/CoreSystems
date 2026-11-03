//! WS25-S3: the org hook rules a Do run carries are enforced on its tool calls, and no rule can
//! widen what the signed capability gate allows.

use execution_core::capability_policy::{CapabilityDecision, CapabilityPolicy};
use execution_core::runtime_loop::{self, StepOutcome};

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

struct DenyPolicy;

#[tonic::async_trait]
impl CapabilityPolicy for DenyPolicy {
    async fn evaluate(
        &self,
        _tool_name: &str,
        _run_id: &str,
        _org_id: &str,
    ) -> Result<CapabilityDecision, tonic::Status> {
        Ok(CapabilityDecision::Deny)
    }
}

static ALLOW_POLICY: AllowPolicy = AllowPolicy;
static DENY_POLICY: DenyPolicy = DenyPolicy;

/// One tool call through the gated path a Do run's dispatch takes, with `hook_context` as the
/// run's org policy. The input is never dispatched by any test here: each assertion is a gate.
async fn step(hook_context: &str, policy: &dyn CapabilityPolicy) -> StepOutcome {
    runtime_loop::execute_step(
        "echo",
        "must-not-dispatch",
        "auto",
        hook_context,
        "",
        "",
        "",
        "",
        "",
        None,
        None,
        None,
        false,
        0,
        None,
        None,
        None,
        policy,
        None,
    )
    .await
}

#[tokio::test]
async fn a_deny_rule_blocks_a_do_run_tool_call_before_it_is_dispatched() {
    let ctx = r#"{"rules":[{"event":"pre_tool_use","tools":"echo","decision":"deny","reason":"no echo"}]}"#;
    let out = step(ctx, &ALLOW_POLICY).await;
    assert_eq!(out.status, "failed");
    assert_eq!(out.error, "blocked by pre-tool hook: no echo");
}

#[tokio::test]
async fn an_allow_rule_never_bypasses_the_signed_gate() {
    let ctx = r#"{"rules":[{"event":"pre_tool_use","tools":"echo","decision":"allow"}]}"#;
    let out = step(ctx, &DENY_POLICY).await;
    assert_eq!(out.status, "permission_denied");
}
