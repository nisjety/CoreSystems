## EP-9-W3
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: insert the line `pub mod site_policy_client;` immediately before `pub mod skills;` (after `mod session_terminal_auth;`). Keep CRLF.

## EP-9-W3
shared-edit apps/Model Plane/rust/services/execution-core/src/lib.rs: insert the line `pub mod site_policy_client;` immediately before `pub mod skill_mounts;`. Keep CRLF.

## EP-9-W3
shared-edit apps/Model Plane/rust/services/model-gateway/src/org_agent_policy.rs (stamp_web_policy, about L569-572): in the `WebPolicyContext { mode, paid_providers, revision }` literal add `        site_policy_revision: 0,` after `        revision,`. Compile-blocking once the proto field lands.

## EP-9-W3
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs (test helper request_under_web_mode, about L32586): in `pb::WebPolicyContext { ... }` add `                    site_policy_revision: 0,` after `                    revision: 3,`. Compile-blocking in cfg(test).

## EP-9-W3
shared-edit apps/Model Plane/deploy/docker-compose.yml: add `      WEB_DOMAIN_POLICY_MODE: ${WEB_DOMAIN_POLICY_MODE:-off}` to the model-gateway environment next to ORG_AGENT_POLICY_MODE (about L425; model-gateway service from L194), and to the execution-core environment (service from L1491).

## EP-9-W3
shared-edit apps/Model Plane/deploy/.env.example: add `WEB_DOMAIN_POLICY_MODE=off` with a comment: off|shadow|enforce, an unknown value means enforce, EP-9 W3.

## EP-9-T2
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs: line 11653 `fn posture_floor(claims: &crate::auth::Claims) -> crate::profile::ApprovalPosture {` becomes `pub(crate) fn posture_floor(claims: &crate::auth::Claims) -> crate::profile::ApprovalPosture {` (nothing else); required for model-gateway to compile.

## EP-9-T2
shared-edit apps/Model Plane/rust/services/model-gateway/src/lib.rs: insert the line `pub mod policy_effective;` between `pub mod page_reader;` (line 72) and `pub mod pricing;` (line 73); required for model-gateway to compile.

## EP-9-T2
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs: after line 131 `        .merge(domains::agent_policy::router(state.clone()))` insert `        .merge(domains::policy_effective::router(state.clone()))` with the same indentation; without it the BFF route is never mounted.

## EP-9-T2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after line 252 `  'org.preview_effective_policy': exempt('ui_state_only', 'read'),` insert `  'policy.read_effective': exempt('ui_state_only', 'read'),`; MODEL_ACCESS is a Record keyed by every ActionId and admission-contract.test.ts builds from it.

## EP-9-C2
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core service environment, directly after the line `ORG_LEARNING_DEFAULT: ${ORG_LEARNING_DEFAULT:-disabled}`, add:
      # EP-9 C2 retention sweeper: off|dry_run|enforce, default off; any other value is dry_run, never enforce.
      # This build only counts (enforce runs dry). Needs SESSION_ORG_CORE_SERVICE_TOKEN with org:retention-policy:read:any.
      SESSION_RETENTION_SWEEP: ${SESSION_RETENTION_SWEEP:-off}
      SESSION_RETENTION_SWEEP_INTERVAL_SECS: ${SESSION_RETENTION_SWEEP_INTERVAL_SECS:-21600}

## EP-9-C4
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: in delete_thread and in the bulk delete-threads handler, replace `.map_err(|error| session_thread_error("session-core delete_thread failed", &error))` with a mapper that first reads `error.metadata().get("x-mp-error-code").and_then(|v| v.to_str().ok())`: on Some("erasure_blocked_legal_hold") return (StatusCode::CONFLICT, Json(json!({"error":{"code":"erasure_blocked_legal_hold","message":error.message(),"details":{"retryable":false}}}))); on Some("erasure_hold_check_unavailable") return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":{"code":"erasure_hold_check_unavailable","message":"Legal holds could not be checked, so nothing was deleted.","details":{"retryable":true}}}))); else fall through to session_thread_error. Without this, FailedPrecondition is mapped to thread_persistence_not_permitted and the SPA never sees the hold code.

## EP-9-C4
shared-edit apps/Model Plane/rust/services/session-core/src/gdpr_nats.rs: in run()'s `Err(e) =>` branch, replace `msg.ack_with(AckKind::Nak(None))` with `msg.ack_with(AckKind::Nak(e.downcast_ref::<crate::gdpr::ErasureHoldError>().map(|_| Duration::from_secs(300))))` and add the label `counter!("mp_session_gdpr_erasure_total", "result" => "held")` when the downcast matches. Otherwise a held org's erasure event is redelivered at once, in a hot loop, while the switch is on.

## EP-11-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core service environment block and the capability-core service environment block add `SKILL_LINT_MODE: ${SKILL_LINT_MODE:-off}` (values are off|report|enforce, not a boolean, so the default is `off`).

## EP-11-S4
shared-edit apps/Model Plane/proto/model_plane/v1/sessions.proto, UpsertAgentSkillResponse.skip_reason comment (comment only): change `// "owner_required", or "skill_policy:<rule>" when the content policy refused` to `// "owner_required", "origin_requires_person" (origin "user" from a caller that is not a person, D2-509), "skill_lint:<code>" when the skill lint refused the text under enforce (EP-11 S4), or "skill_policy:<rule>" when the content policy refused`. Regenerate bindings only if the guard regenerates on comment changes.

## EP-11-S4
shared-edit apps/Model Plane/api/contracts/README.md table, add the row: `| authored-behaviour-lint.json (+ authored-behaviour-lint-golden.json) | authored (EP-11 S4); pinned by its .sha256 | the one closed lint every skill write passes: rule codes, fields, severities, limits, SKILL_LINT_MODE and the refusal shapes (HTTP 422 skill_lint_failed with details.findings; gRPC skip_reason skill_lint:<code>). Read by mp-contracts skill_lint and capability-core internal/skilllint against the same golden cases |`.

## EP-4-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway service, after line 250 (`      MODEL_GATEWAY_ROLE_ROUTING: ${MODEL_GATEWAY_ROLE_ROUTING:-false}`; the file is LF), insert two lines at the same 6-space indent: `      # EP-4 S2: widget prompt notice, literal true only. VITE_CHAT_INLINE_WIDGETS must also be on in the frontend.` and `      MODEL_GATEWAY_WIDGET_GUIDANCE: ${MODEL_GATEWAY_WIDGET_GUIDANCE:-false}`.

## EP-4-S3
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx: in the import at line ~126 `import { InlineWidget, InlineWidgetSurface, inlineWidgetsEnabled, useInlineWidgetsLive } from './InlineWidget'` add `InlineWidgetAnswer`; in AssistantMessage (~line 867-869) wrap the existing child so it reads `<InlineWidgetSurface value={inlineWidgetsEnabled() ? 'live' : 'inert'}>` / `<InlineWidgetAnswer onSelectFollowUp={props.onSelectFollowUp}>` / `<SettledTurnContent fold={props.settledFold} entries={nodeEntries()} renderEntries={renderNodeEntries} />` / `</InlineWidgetAnswer>` / `</InlineWidgetSurface>`.

## EP-4-S3
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `'chat.goal_auto_toggle': exempt('agent_cannot_self_govern', 'context_write'),` add `'chat.stage_widget_followup': exempt('ui_state_only', 'read'),` and `'chat.open_widget_link': exempt('ui_state_only', 'read'),`. MODEL_ACCESS is Record<ActionId, ...>, so typecheck fails without these.

## EP-4-S6
shared-edit scripts/tests/zdr-contract-sync.test.sh: line 203, replace the 64-hex digest after 'sse-event-payloads.json|' (currently 07670b8fd14fede1fdf149a6045f68b687c97cd539957316360efc6bb267eb59) with the hex from the new .sha256 file. The file is LF.

