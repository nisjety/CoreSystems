//! Cross-language proto wire-compat goldens for `OrchestrationEvent`.
//!
//! These hex constants are the **canonical wire bytes** produced by encoding
//! each variant of `OrchestrationEvent` via the prost-generated
//! `model_plane.v1.OrchestrationEvent` message. They MUST stay byte-for-byte
//! identical to the goldens used by the Go and Python sibling tests
//! (`go/gen/model_plane/v1/proto_wire_parity_test.go`,
//! `python/mp-events-py/tests/test_proto_wire_parity.py`).
//!
//! Updating a golden constitutes a wire-level breaking change.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::OrchestrationEvent as ProtoOrchestrationEvent;
use mp_orchestration::{
    ApprovalKind, ApprovalState, OrchestrationEvent, PlanState, SubagentRole, TodoState,
};
use prost::Message;

// ---- canonical timestamp (2023-11-14T22:13:20Z) ----
fn at() -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000, 0).expect("valid epoch")
}

// ---- goldens (filled after first capture; see CONTRIBUTING for refresh flow) ----
const GOLDEN_PLAN_TRANSITIONED: &str = "0a060880e2cfaa0652130a06706c616e2d31120572756e2d3118012002";
const GOLDEN_TODO_TRANSITIONED: &str =
    "0a060880e2cfaa065a160a06746f646f2d3112087468726561642d3118012002";
const GOLDEN_APPROVAL_STATE_CHANGED: &str =
    "0a060880e2cfaa0662250a06617070722d31120572756e2d31180220022a1075736572406578616d706c652e636f6d";
const GOLDEN_SUBAGENT_ATTACHED: &str =
    "0a060880e2cfaa066a190a0a72756e2d706172656e74120972756e2d6368696c641802";
const GOLDEN_SUBAGENT_STOPPED: &str =
    "0a060880e2cfaa0672160a0972756e2d6368696c641209636f6d706c65746564";
const GOLDEN_RUN_PAUSED_FOR_APPROVAL: &str = "0a060880e2cfaa067a0f0a0572756e2d311206617070722d31";
const GOLDEN_RUN_RESUMED_AFTER_APPROVAL: &str =
    "0a060880e2cfaa0682010f0a0572756e2d311206617070722d31";

fn encode_hex(ev: OrchestrationEvent) -> String {
    let proto: ProtoOrchestrationEvent = ev.into();
    hex::encode(proto.encode_to_vec())
}

fn decode_from_hex(h: &str) -> OrchestrationEvent {
    let bytes = hex::decode(h).expect("valid hex");
    let proto = ProtoOrchestrationEvent::decode(bytes.as_slice()).expect("decode");
    OrchestrationEvent::try_from(proto).expect("shim accepts golden")
}

fn assert_roundtrip(ev: &OrchestrationEvent, golden: &str) {
    let actual = encode_hex(ev.clone());
    assert_eq!(
        actual, golden,
        "wire bytes drifted; this is a cross-language breaking change"
    );
    let decoded = decode_from_hex(golden);
    assert_eq!(decoded, *ev, "round-trip lost data");
}

#[test]
fn plan_transitioned_wire_parity() {
    let ev = OrchestrationEvent::PlanTransitioned {
        plan_id: "plan-1".into(),
        run_id: "run-1".into(),
        from: PlanState::Draft,
        to: PlanState::Proposed,
        at: at(),
    };
    assert_roundtrip(&ev, GOLDEN_PLAN_TRANSITIONED);
}

#[test]
fn todo_transitioned_wire_parity() {
    let ev = OrchestrationEvent::TodoTransitioned {
        todo_id: "todo-1".into(),
        thread_id: "thread-1".into(),
        from: TodoState::Pending,
        to: TodoState::InProgress,
        at: at(),
    };
    assert_roundtrip(&ev, GOLDEN_TODO_TRANSITIONED);
}

#[test]
fn approval_state_changed_wire_parity() {
    let ev = OrchestrationEvent::ApprovalStateChanged {
        approval_id: "appr-1".into(),
        run_id: "run-1".into(),
        approval_kind: ApprovalKind::ToolCall,
        to: ApprovalState::Granted,
        decided_by: "user@example.com".into(),
        at: at(),
    };
    assert_roundtrip(&ev, GOLDEN_APPROVAL_STATE_CHANGED);
}

#[test]
fn subagent_attached_wire_parity() {
    let ev = OrchestrationEvent::SubagentAttached {
        parent_run_id: "run-parent".into(),
        child_run_id: "run-child".into(),
        role: SubagentRole::Reviewer,
        at: at(),
    };
    assert_roundtrip(&ev, GOLDEN_SUBAGENT_ATTACHED);
}

#[test]
fn subagent_stopped_wire_parity() {
    let ev = OrchestrationEvent::SubagentStopped {
        child_run_id: "run-child".into(),
        status: "completed".into(),
        at: at(),
    };
    assert_roundtrip(&ev, GOLDEN_SUBAGENT_STOPPED);
}

#[test]
fn run_paused_for_approval_wire_parity() {
    let ev = OrchestrationEvent::RunPausedForApproval {
        run_id: "run-1".into(),
        approval_id: "appr-1".into(),
        at: at(),
    };
    assert_roundtrip(&ev, GOLDEN_RUN_PAUSED_FOR_APPROVAL);
}

#[test]
fn run_resumed_after_approval_wire_parity() {
    let ev = OrchestrationEvent::RunResumedAfterApproval {
        run_id: "run-1".into(),
        approval_id: "appr-1".into(),
        at: at(),
    };
    assert_roundtrip(&ev, GOLDEN_RUN_RESUMED_AFTER_APPROVAL);
}

#[test]
fn truncated_payload_is_rejected() {
    // Use the plan_transitioned golden, lop off the final byte; prost must error.
    let bytes = hex::decode(GOLDEN_PLAN_TRANSITIONED).expect("hex");
    let truncated = &bytes[..bytes.len() - 1];
    let result = ProtoOrchestrationEvent::decode(truncated);
    assert!(result.is_err(), "prost must reject truncated wire bytes");
}
