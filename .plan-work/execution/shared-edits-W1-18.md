## WS03-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment block add `SESSION_CORE_CRASH_PARK_ENABLED: ${SESSION_CORE_CRASH_PARK_ENABLED:-false}` next to SESSION_CORE_PARK_RECOVERY_ENABLED.

## WS07-S2
shared-edit apps/Model Plane/rust/services/model-gateway/src/grpc.rs (mod tests, MockMemoryService, after set_memory_controls which ends at line 3829, before the closing brace at line 3830, 8-space indent inside the impl): async fn reset_memory(&self, _: Request<mp_contracts::model_plane::v1::ResetMemoryRequest>) -> Result<Response<mp_contracts::model_plane::v1::ResetMemoryResponse>, Status> { Err(Status::unimplemented("reset_memory not needed in test")) } (rustfmt layout like set_memory_controls above it).

## WS07-S2
shared-edit apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs (MockMemory, after set_memory_controls which ends at line 11993, before the closing brace at line 11994, 4-space indent): async fn reset_memory(&self, _: TReq<mpv1::ResetMemoryRequest>) -> Result<Response<mpv1::ResetMemoryResponse>, Status> { Err(Status::unimplemented("reset_memory not needed in test")) } (same layout as set_memory_controls). Both files must keep their existing line endings (measure CRLF vs LF first). Then verify with cargo test -q -p model-gateway --lib --no-run.

## WS07-S5
shared-edit apps/Model Plane/rust/services/session-core/src/memory_grpc.rs: in get_memory_controls (about line 807) the service-caller scope passed to authorize_user_memory_preflight is MEMORY_READ_SCOPE ('memory:read'). capability-core now presents only 'session:memory-controls:read'. Change that call's service scope to "session:memory-controls:read" (a new const), keeping user callers on authorize_user, or the proxy's lookup is refused and every proxy verb returns 503. Add a unit test for it.

## WS07-S5
shared-edit apps/Model Plane/deploy/.env.example (comment block near lines 222-226): extend the capability-core line to `capability-core -> aud=session-core: session:read, session:skills:write, session:schedule-prepare, session:memory-controls:read`.

## WS07-S5
shared-edit apps/Control Plane/docker-compose.yml and the generated principal registry: the principals JSON file is updated, but the registry credentials that auth-core actually loads (PLANE_SERVICE_PRINCIPALS_JSON from the launcher; the compose line near 540 is one line, merge by hand) must be refreshed with scripts/run-control-plane.sh BEFORE capability-core is rebuilt, or minting the session-core token with the new scope will be refused and the learning review loses its session-core credential too.

## WS07-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: in the UpdateThreadPresentationRequest literal (around line 9323), add `memory_mode: None,` after `archived: body.archived,`. Without it model-gateway fails to compile (E0063 missing field). The gateway route and body field that actually pass the mode are not part of this slice.

## WS08-S7
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/use-chat-controller.ts: in the launch-consume block (about line 2049, inside the async function) change `const pending = consumePendingChatLaunch()` to `const pending = await consumePendingChatLaunchAsync()` and update the import. Without it a reloaded launch that carries attachments is never consumed (the sync consume leaves it in place).

## WS08-S7
shared-edit apps/Model Plane/deploy/docker-compose.yml: pass CHAT_ATTACHMENT_MAX_TOTAL_BYTES to the inference-core and execution-core service blocks (they read it for their tonic decode limit; unset means the 16 MiB floor). model-gateway should also carry CHAT_STREAM_BODY_LIMIT_BYTES (S2 may already have added it). Add CHAT_EXTRACT_MAX_BYTES to the imports-core service in the Ingestion Plane compose, and CHAT_ATTACHMENT_MAX_TOTAL_BYTES and CHAT_STREAM_BODY_LIMIT_BYTES to the Frontend Plane gateway (BFF) service.

## WS10-S7.code
shared-edit apps/Model Plane/MODEL_PLANE_STATUS.md (append-only, dated, after the live flip, not before): 'WS10-S7 model entitlement flip: preflight/before/after evidence in docs/compliance/evidence/live-reads-model-entitlement-<date>.txt; stages run: <off|shadow|enforce|admin>; shadow soak hours: <n>; would_refuse=<n> service_exempt=<n>; refused-turn row delta 0 on <orgs>; stale-bound and 503 run: <result>.' Code half status: proof script, contract test and runbook landed.

