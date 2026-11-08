## WS25-S1
shared-edit apps/Model Plane/deploy/docker-compose.yml: model-gateway service environment: add LOOP_GUARD_MODE: ${LOOP_GUARD_MODE:-} (unset means off). Leave the default off in this batch. Carried from the prior round.

## WS16-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts: after the chat.delete_thread entry insert { id: 'chat.mark_thread_seen', label: 'Mark chat thread seen', description: 'Mark one of the caller's threads read. Owner-only; refused for zero-retention and temporary chats, which keep read state on the client. Never model-executable.', ownerPlane: 'model', risk: 'low', requiresApproval: false, reversible: true, inputSchema: chatMarkThreadSeenInput, outputSchema: envelopeStatusOutput }, and beside chatDeleteThreadInput add const chatMarkThreadSeenInput = z.object({ threadId: z.string().trim().min(1) }).

## WS16-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: add row 'chat.mark_thread_seen': exempt('ui_state_only', 'context_write') (recommended reason; 'ui_state_only' is an existing ModelExemptReason).

## WS16-S2
shared-edit golden: VEREVON_UPDATE_GOLDEN=1 node node_modules/vitest/vitest.mjs run src/shared/actions/admission-contract.test.ts; refresh src/shared/api/contracts/agent-action-admission.json.sha256; re-vendor to apps/Model Plane/api/contracts/agent-action-admission.json and its .sha256; run scripts/tests/zdr-contract-sync.test.sh.

## WS16-S2
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs and handlers.rs: add dispatch_chat_mark_thread_seen (POST {model_gateway_url}/v1/threads/{id}/seen with the session token, the same route attention.rs calls) and a 'chat.mark_thread_seen' arm; then action-surface-contract.test.mjs covers the id.

## WS16-S4
shared-edit apps/Application Plane/notification-core/internal/http/handlers.go: the capability-core block of isNotificationTypeAuthorized gained three lines (modelplane.needs_approval, modelplane.needs_input, modelplane.plan_ready). The Space session shares this allowlist, so whichever lands second rebases onto the other.

## WS13-S6
shared-edit apps/Model Plane/rust/services/session-core/src/grpc.rs (about line 9923, beside the OrchestrationCoreServiceServer add_service): register ApprovalScopeGrantServiceServer::with_interceptor with the same OrchestrationGrpc and interceptor. Until then the grant call is Unimplemented and grantable pauses stay paused.

## WS13-S6
shared-edit apps/Model Plane/rust/services/session-core/src/main.rs (startup): make scope_grants_require_audit (orchestration_grpc.rs) pub(crate) and refuse boot when APPROVAL_SCOPE_GRANTS_ENABLED=true without APPROVAL_AUDIT_EVENTS_ENABLED=true (K3-2).

## WS13-S6
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs (pause_for_approval, about line 7343) and apps/Model Plane/rust/services/execution-core/src/grpc.rs (about line 1519): replace approval_cause::for_call with for_call_with_class, passing the signed effect class of the call (StepOutcome and PendingPause carry it; mod.rs has signed_class.class). Until then every creator states no class, so only once is offered and no grant is created.

## WS13-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs (N7 test every_durable_mutation_refuses_a_zdr_or_waived_token_before_it_runs, about line 826): add UseApprovalScopeGrant and RevokeApprovalScopeGrant to the durable-mutation inventory.

