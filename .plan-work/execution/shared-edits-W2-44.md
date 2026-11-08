## WS13-S7
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs: in approval_cause_for_pause (~line 7669) replace approval_cause::for_call(cause, run_id, tool_name, tool_input) with for_call_with_tool(cause, run_id, tool_name, tool_input, <signed effect class>, <signed tool digest>), carrying both from the evaluation through PendingPause.

## WS13-S7
shared-edit apps/Model Plane/rust/services/execution-core/src/grpc.rs: the ExecuteStep pause (~line 1570) replaces approval_cause::for_call with the same for_call_with_tool call.

## WS13-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs: add route_note: None to the InferResponse literal (~line 3376) and the InferChunk literals (~3399, ~3416), all inside mod tests (~line 3117). Then run cargo test -q -p model-gateway --lib approval with CARGO_TARGET_DIR=C:/dev/mp-target-w2 CARGO_INCREMENTAL=0.

## WS13-S7
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx: offer Always for MCP approvals whose cause lists thread; send decideApproval(approvalId, 'approve', '', undefined, 'thread', true).

## WS27-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse_events.rs (apply all of (a)-(e) before compiling model-gateway): (a) in ChatEvent::UsageProgress after line 453 `ceiling: Option<crate::sse_events::run_ceiling::CeilingProgress>,` add a doc comment and `routing: Option<&'static str>,` (8 spaces). (b) payload() UsageProgress arm: add `routing,` (16 spaces) after line 917 `ceiling,`; after line 952 (the 16-space `}` closing `if let Some(ceiling) = ceiling {`) and before line 953 `payload`, insert `if let Some(routing) = routing {` / `payload["routing"] = json!(routing);` / `}` at 16 spaces (body 20). (c) RichEventSink::record_call (line 1568): add `routing: Option<&'static str>,` (8 spaces) after line 1571 `priced: &crate::pricing::Priced,`; in its emit (lines 1588-1592) add `routing,` (16 spaces) after `ceiling,`. (d) test module: replace every `sink.record_call(&call, &priced).await;` (lines 2375, 2376, 2426, 2431) with `sink.record_call(&call, &priced, None).await;`. (e) add `routing: None,` (12 spaces) after `ceiling: None,` at lines 2188, 2265, 2338, 2544; after line 2449 `}),` in the `carried` literal (line 2441, before its closing brace on line 2450); after `ceiling,` at line 3452 in usage_progress_with_ceiling.

## WS27-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs (test module): after line 3390 `provider_timing: None,` (16 spaces) add `route_note: None,` (16 spaces); after lines 3414 and 3431 `provider_timing: None,` (20 spaces) add `route_note: None,` (20 spaces each).

## WS27-S7
shared-edit apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs: after lines 400 and 974 `provider_timing: None,` (12 spaces) add `route_note: None,` (12 spaces); after lines 445 and 462 `provider_timing: None,` (16 spaces) add `route_note: None,` (16 spaces).

## WS27-S7
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts: (a) in ChatUsageProgressEvent after line 32 `warning?: 'run_ceiling_80' | 'run_ceiling_reached'` add a doc comment and `routing?: 'tier_shifted' | 'cheap_fallback'`. (b) after the runCeilingWarningValue function (lines 59-61) add `function routingValue(value: unknown): 'tier_shifted' | 'cheap_fallback' | undefined {` / `  return value === 'tier_shifted' || value === 'cheap_fallback' ? value : undefined` / `}`. (c) in usageProgressFrame after line 78 `warning: runCeilingWarningValue(payload.warning),` add `routing: routingValue(payload.routing),`. Optional: a routing case in the describe('usage_progress (WS27 S3)') block of chat-client.test.ts (line 2329).

## WS27-S7
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/chat-types.ts: in RunMeter after the line `warning?: NonNullable<ChatUsageProgressEvent['warning']>` add a one-line doc comment and `routing?: NonNullable<ChatUsageProgressEvent['routing']>`.

## WS27-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml: under the inference-core environment block, directly after the line INFERENCE_CORE_EFFORT_MODEL_ALIASES (about line 1107), add `      INFERENCE_ROUTE_NOTE: ${INFERENCE_ROUTE_NOTE:-false}` (6 spaces). Carried over from the previous round, still required.

