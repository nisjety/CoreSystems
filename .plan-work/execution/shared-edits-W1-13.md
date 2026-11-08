## WS04-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/browser_run.rs:3486 and apps/Model Plane/rust/services/model-gateway/src/grpc.rs:3774: change `AppendMessageResponse { sequence: 1 }` to `AppendMessageResponse { sequence: 1, ..Default::default() }` (the model-gateway LIB TESTS do not compile without these two). Same one-token fix in apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs:1250, tests/usage_meter_contract.rs:279 (`mpv1::AppendMessageResponse`) and apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:9458 (`pb::AppendMessageResponse`).

## WS04-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs ~6907-6931 (the round-boundary drain): `let drained = state.queued_inputs.drain_receipted(request_id); let queued: Vec<String> = drained.iter().map(|m| m.text.clone()).collect();` (the rest of the block keeps using `queued`), and at the push: `events.push(if crate::queued_input::send_receipts_enabled() { ChatEvent::QueuedInputReceipted { messages: queued, ids: drained.iter().map(|m| m.client_message_id.clone()).collect() } } else { ChatEvent::QueuedInput { messages: queued } }).await;`. Without it the delivery event never carries ids and the SPA falls back to matching by text.

## WS04-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs ~1396: call `crate::session_flow::prepare_managed_run_authenticated_with_receipt(` instead of `prepare_managed_run_authenticated(` and add the last argument `crate::session_flow::send_receipt_id(crate::queued_input::send_receipts_enabled(), effective_zdr, req.regenerated, req.edited_resubmit, req.idempotency_key.as_deref())`; update the source-text test at sse.rs ~19876 (`the_execute_gate_runs_before_any_run_exists`) to search `prepare_managed_run_authenticated_with_receipt(`. Without it the streaming send path (the SPA's main path) names no message to the transcript.

## WS04-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml (model-gateway environment): `MODEL_GATEWAY_SEND_RECEIPTS: ${MODEL_GATEWAY_SEND_RECEIPTS:-false}`; and the same default-off line in the .env.example that lists the other MODEL_GATEWAY_* switches. Deploy order: session-core (migration 0052) first, then model-gateway, then the BFF and frontend, then the switch.

