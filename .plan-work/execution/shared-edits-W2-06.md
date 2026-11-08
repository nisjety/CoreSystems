## WS17-S1
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat.rs:25: change `put(history::save_thread).delete(history::delete_thread)` to `put(history::save_thread).get(history::get_thread).delete(history::delete_thread)`. Without it getChatThread gets 405 and history::get_thread is dead code.

## WS17-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs:3973: `ListThreadsResponse { threads: vec![] }` add `next_cursor: String::new()` (or `..Default::default()`).

## WS17-S1
shared-edit apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs:1415: same literal fix.

## WS17-S1
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs:9659: `pb::ListThreadsResponse { threads: vec![] }` add `next_cursor: String::new()` (or `..Default::default()`).

## WS25-S4
shared-edit services/model-gateway/src/grpc.rs:3973: ListThreadsResponse literal needs next_cursor (owned by WS17-S1, which changed sessions.proto); the lead or WS17-S1 must fix it before any model-gateway test target builds.

## WS25-S4
shared-edit services/model-gateway/src/sse_events.rs:2478: ProviderSummary literal needs failed_attempts_ms and retry_wait_ms (owned by WS15-S4, which changed turn_timing.rs); the lead or WS15-S4 must fix it.

## WS15-S4
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse_events.rs: in the test literal `ProviderSummary { ... }` at line 2478, after `attempts: 1,` (line 2482), add `failed_attempts_ms: 0,` and `retry_wait_ms: 0,`. This file is CRLF (2798/2798), so keep CRLF.

## WS15-S4
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts: (a) in `export type TurnTimingProvider` (line 422) add `failedAttemptsMs?: number` and `retryWaitMs?: number`; (b) in `turnTimingProvider()` return object, after `attempts: countValue(raw.attempts),` (line 516), add `...(millis(raw.failed_attempts_ms) !== undefined ? { failedAttemptsMs: millis(raw.failed_attempts_ms) } : {}),` and `...(millis(raw.retry_wait_ms) !== undefined ? { retryWaitMs: millis(raw.retry_wait_ms) } : {}),`. This clears the 3 slice-file typecheck errors.

## WS23-S2
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs: line 9659, change `pb::ListThreadsResponse { threads: vec![] }` to `pb::ListThreadsResponse { threads: vec![], next_cursor: String::new() }`. The owner of agent.rs or the lead applies it.

