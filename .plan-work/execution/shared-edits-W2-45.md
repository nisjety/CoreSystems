## WS13-S8
shared-edit apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs (WS13-S1 and S3 owners): in create_approval, line ~3411, `let (metadata, cause_code) = new_approval_metadata(&req, caller.zdr());` must also drop the cause for a waiver (caller.zdr() || caller.zdr_waiver().is_some()); in decide_approval, line ~3786, refuse a waived caller next to `retention_receipt::refuse_zdr_continuation(&caller)?;` before store::decide_approval_recorded. Add a unit test beside a_zero_retention_caller_stores_no_cause.

## WS13-S8
shared-edit apps/Model Plane/rust/services/session-core/src/orchestration_store.rs: decide_approval_recorded (line ~1169, `if updated && record.audit`) must not write approval_decided for a non-persistent run; the handler fix in the previous item does not cover a waived store caller.

