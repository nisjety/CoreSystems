## WS13-S3
shared-edit apps/Model Plane/rust/crates/mp-orchestration/src/proto_shim.rs: in the ProtoEvent::ApprovalStateChanged(PbApprovalStateChanged { ... }) literal (lines 294-300) add `..Default::default(),` on a new line after `decided_by,` (line 299). Required before any session-core, execution-core or model-gateway test can compile.

## WS13-S3
shared-edit apps/Model Plane/rust/services/execution-core/src/browser_events.rs: in the DecideApprovalRequest literal with decided_by "system:user-cancelled" (line 355), add `decision_source: pb::ApprovalDecisionSource::System as i32,` and `..Default::default(),` after `org_id`. In the literal with decided_by "system:approval-timeout" (line 436), add `decision_source: pb::ApprovalDecisionSource::Timeout as i32,` and `..Default::default(),` after `org_id`. The pb alias is already imported as `self as pb`.

## WS13-S3
shared-edit apps/Model Plane/rust/services/model-gateway/src/approvals.rs: in to_decide_approval_request (line 135), add `decision_source: ApprovalDecisionSource::User as i32,` and `..Default::default(),` after `org_id`, and add `ApprovalDecisionSource` to the mp_contracts::model_plane::v1 use block (lines 29-34).

