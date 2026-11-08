## WS15-S3
shared-edit crates/mp-orchestration/src/proto_shim.rs (the match on ProtoEvent, around lines 638-640): add `| ProtoEvent::ToolRetryScheduled(_)` to the arm `ProtoEvent::QuestionAsked(_) | ProtoEvent::QuestionAnswered(_) | ProtoEvent::RunUsageProgress(_) => { return Err(ShimError::UnsupportedEvent); }`. Without this, `cargo check -p mp-orchestration` fails with E0004 (verified) and session-core cannot build. Do this before committing the proto.

## WS15-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml: add `MODEL_GATEWAY_LIFECYCLE_EVENTS: ${MODEL_GATEWAY_LIFECYCLE_EVENTS:-false}` to the execution-core service environment (the Do-loop announcement reads the switch there). The file has no lifecycle switch at all today, so add it to model-gateway as well (WS15-S1's compose line is missing from the tree). Default off.

## WS22-S5
shared-edit apps/Model Plane/proto/model_plane/v1/sandboxes.proto: in the comment on DeleteThreadWorkspace, drop the stale 'refuses every caller until S5' wording (comment-only).

