## WS07-S8
shared-edit apps/Model Plane/rust/crates/mp-orchestration/src/proto_shim.rs: line 294, add the missing ApprovalStateChanged fields (cause_code, decision_source, rule_id and the 2 others) or ..Default::default(); nothing in session-core compiles until then

## WS08-S6
shared-edit apps/Model Plane/rust/services/execution-core/src/browser_events.rs: add `..Default::default()` (or decision_source/scope) to the two pb::DecideApprovalRequest literals at ~:355 and ~:436, and apps/Model Plane/rust/services/model-gateway/src/approvals.rs ~:135; the workspace does not compile without it (owned by the slice that changed sessions.proto)

## WS08-S6
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs ~:893: `let ro_inputs = crate::run_inputs::global().get(org_id, run_id);` must return None for a delegated child (thread depth or a root-only flag from run_rounds into the dispatch) so children_do_not_inherit_attachments also holds for code_interpreter files

## WS08-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/attachment_policy.rs: add `modes.do_attachments` (true when CHAT_DO_ATTACHMENTS_ENABLED is the literal true) to the declared policy JSON; apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts parseAttachmentPolicy: read it; then switch the composer from VITE_CHAT_DO_ATTACHMENTS_ENABLED to it

## WS08-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml model-gateway block: pass-through line `CHAT_DO_ATTACHMENTS_ENABLED: ${CHAT_DO_ATTACHMENTS_ENABLED:-false}`

