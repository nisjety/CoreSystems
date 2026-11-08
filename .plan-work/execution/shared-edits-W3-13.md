## EP-6-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the capability-core environment block, directly after the line `      CRON_CONTRACT_GUARDS_ENABLED: ${CRON_CONTRACT_GUARDS_ENABLED:-false}` add two lines: `      # EP-6-S2: Run now, a manual schedule fire through the same Control fire authorization. Only the literal `true` enables it.` and `      CRON_RUN_NOW_ENABLED: ${CRON_RUN_NOW_ENABLED:-false}`

## EP-6-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: directly after `  'schedule.fires.list': exempt('no_owner_executor', 'read'),` add `  'schedule.run_now': exempt('agent_cannot_self_govern', 'external_write'),` (action-registry.test.ts asserts this entry)

## EP-6-S2
shared-edit apps/Model Plane/api/contracts/space-surface.v1.json AND apps/Model Plane/rust/services/model-gateway/tests/fixtures/space-surface.v1.json: insert after the row with id `mg:POST:/v1/cron` the row {"id": "mg:POST:/v1/cron/:id/run", "kind": "route", "plane": "model", "service": "model-gateway", "method": "POST", "path": "/v1/cron/:id/run", "handler": "http_routes.rs::run_cron_now_proxy", "decision_type": "token_posture", "min_role": "authenticated", "zdr_class": "refuse", "basis": "guard:proxy_to_capability_core", "idempotency": "request_id", "flag": "CRON_RUN_NOW_ENABLED", "payload_allow_list": [], "reason": "Refuses a ZDR or waived token before it runs (zdr_posture.rs DURABLE_MUTATIONS); capability-core also refuses a ZDR principal and Control refuses ZDR at both fire decisions."}. Match the neighbouring rows' formatting, and if the schema restricts the idempotency/flag values use "unspecified" and null. Then re-pin both space-surface.v1.json.sha256 files. Without this row, model-gateway's zdr_posture `every_non_get_route_is_classified` fails.

## EP-6-S2
shared-edit apps/Model Plane/docs/ZDR.md (LF): in the 'Features refused with a typed code' table row (about line 101), after `Scheduled and unattended runs, browser runs and speech and other modalities are refused too.` add ` Run now (a manual schedule fire, EP-6-S2) is refused at three layers: the model-gateway durable-mutation guard, the capability-core router (non-read request from a ZDR principal, authz.go) and Control's fire and scheduled-run decisions (and the dispatcher's re-check).`

## EP-6-S9
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/config.rs: in `struct SpaceFeatureFlags`, after the `memory_ui` field, add `/// AGENT_VERSION_PINNING (EP-6 S9): a mention turn runs the binding's pinned (else published) agent version and carries its allowlist and cap to the model gateway. Only the literal true turns it on, never by default in development. Off keeps the legacy persona injection. A rollout control, never an authorization input: the allowlist and cap only narrow.` then `pub(crate) agent_version_pinning: bool,`. In `SpaceFeatureFlags::from_env`, after the `memory_ui:` line, add `agent_version_pinning: env::var("AGENT_VERSION_PINNING").is_ok_and(|value| value == "true"),`.

## EP-6-S9
shared-edit apps/Application Plane/convex-core/convex/spaceAgents.ts: in `spaceAgentBindingsForGateway`'s `projected.push({...})`, after `approvalMode: binding.approvalMode,`, add `// EP-6 S9: the agent version this binding pins; absent follows the published version.` then `pinnedVersion: binding.pinnedVersion,`.

## EP-6-S9
shared-edit apps/Frontend Plane/verevonv3/src/shared/i18n/errors.ts ERROR_COPY (otherwise gateway-error-codes.test.ts fails on the new BFF code): add `agent_version_unavailable: { no: 'Agentens versjon kunne ikke leses akkurat nå. Prøv igjen snart.', en: 'This agent\'s version could not be read right now. Try again shortly.' },`. Optionally add the MG stream codes too: `agent_pin_invalid` (no: 'Agentversjonen for denne meldingen kunne ikke leses.' / en: 'The agent version for this turn could not be read.') and `agent_tool_allowlist_do_unsupported` (no: 'Denne agentens verktøyliste kan ikke håndheves i en Gjør-kjøring ennå. Spør den i stedet.' / en: 'This agent\'s tool list cannot be enforced on a Do run yet. Ask it instead.').

## EP-6-S9
shared-edit apps/Model Plane/deploy/docker-compose.yml: model-gateway environment, after the `MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE` line (~429), add `      MODEL_GATEWAY_AGENT_TOOL_ALLOWLIST: ${MODEL_GATEWAY_AGENT_TOOL_ALLOWLIST:-false}`.

## EP-6-S9
shared-edit apps/Frontend Plane/verevonv3/docker-compose.yml: gateway service environment, beside `SPACE_ROOM_FILES_UI` (~280), add `      AGENT_VERSION_PINNING: ${AGENT_VERSION_PINNING:-false}`; add `AGENT_VERSION_PINNING: "false"` beside line 30 of docker-compose.production.yml.

## EP-10-S7
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: replace the two lines `toolPolicies := api.NewToolPoliciesHandler(pool)` / `toolPolicyEnforced := policy.ToolPolicyEnforcedFromLookup(os.Getenv)` with `toolPolicyEnforced := policy.ToolPolicyEnforcedFromLookup(os.Getenv)` then `toolPolicies := api.NewToolPoliciesHandler(pool).WithEnforced(toolPolicyEnforced)` (without it the list always reports enforced=false)

## EP-10-S7
shared-edit apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs (fn with `let connector_offered = connector_reads::offered(`, ~line 8527): change the statement to `let connector_offered = connector_reads::offered(connector_reads::enabled(), zdr, !space_id.is_empty(), connector_turn) && connector_reads::read_policy(state, org_id, user_id, capability_bearer).await.admits_ask_loop();` (short-circuit: the policy read runs only on a turn that already offers the read; state/org_id/user_id/capability_bearer are in scope). Without it read_policy is unused (dead-code warning).

## EP-10-S7
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the `'integrations.fetch_connection_content'` row add `  'integrations.list_connector_policies': exempt('agent_cannot_self_govern', 'read'),` and `  'integrations.set_connector_policy': exempt('agent_cannot_self_govern', 'external_write'),` (MODEL_ACCESS is exhaustive by type; typecheck fails without them)

## EP-1-S1
shared-edit apps/Model Plane/rust/services/execution-core/tests/approval_gate_contract.rs: in fn variant_and_token, before `CauseCode::UnclassifiedGate => ("UnclassifiedGate", "unclassified_gate"),` add `CauseCode::BrowserHumanAssist => ("BrowserHumanAssist", "browser_human_assist"),` (exhaustive match; does not compile otherwise)

## EP-1-S1
shared-edit apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs: in the Feature table after `Feature { key: "browser_tool", label: "browser tools" },` add `Feature { key: "browser_human_assist", label: "browser help requests" },` (feature_not_supported debug_asserts the key, so browser_session test takeover_pauses_with_human_assist_cause_when_switch_on panics without it); add the same row `{"key": "browser_human_assist", "label_en": "browser help requests"}` after browser_tool in apps/Model Plane/api/contracts/zdr-refusals.json, regenerate its .sha256, sync the vendored copies apps/Model Plane/go/pkg/zdrrefusal/zdr-refusals.json(+.sha256) and apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json, add `| 'browser_human_assist'` to the key union and `browser_human_assist: { no: 'hjelpeforespørsler i nettleseren', en: 'browser help requests' }` in apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts, update the row counts in the zdr_refusal_inventory.rs tests, and add one line to apps/Model Plane/docs/ZDR.md (browser human assist is refused for ZDR/waived runs with zdr_feature_not_supported / browser_human_assist; no approval row is written). No zdr-posture-matrix row.

## EP-1-S1
shared-edit apps/Model Plane/rust/services/execution-core/src/browser_events.rs: add `control_epoch: 0,` after the `reason: ...` line in the four BrowserRunPaused/BrowserRunResumed literals (fns run_paused, run_resumed, paused_by_user, resumed_after_takeover); the attached-tab path does not read the epoch yet

## EP-1-S1
shared-edit apps/Model Plane/rust/crates/mp-orchestration/src/proto_shim.rs: in the PbBrowserRunPaused and PbBrowserRunResumed literals (internal->proto, around the `reason,` lines) add `control_epoch: 0,`; the internal run_event enum does not carry the epoch, and the Do loop records the proto directly through RecordOrchestrationEvent

## EP-1-S1
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs: in test orchestration_browser_pause_resume_frames_carry_reason_only_when_set add `control_epoch: 0,` to the BrowserRunPaused and BrowserRunResumed literals

## EP-1-S1
shared-edit apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs: add `control_epoch: 0,` to the three test literals of orchestration_event::BrowserRunPaused/BrowserRunResumed (near the event_run_id assertions and refuse_park_events_while_parking_is_off)

## EP-1-S1
shared-edit apps/Model Plane/api/openapi.yaml: add `- browser_human_assist` before `- unclassified_gate` in the ApprovalCause code enum (around line 1876), then regenerate the Python and TS SDK ApprovalCause enums (api/sdk/python/model_plane_sdk/models/approval_cause.py, api/sdk/typescript/src/models/ApprovalCause.ts)

## EP-3-S2
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block, after the MODEL_GATEWAY_REQUEST_INSPECTOR line, add `      # EP-3 S2: shadow dual-write of artifact revisions into session-core (off|shadow|on; any other value fails startup).` and `      MODEL_GATEWAY_ARTIFACT_REVISIONS: ${MODEL_GATEWAY_ARTIFACT_REVISIONS:-off}`. In the session-core block, after SESSION_CORE_REQUEST_INSPECTOR_READ, add `      # EP-3 S2: the ArtifactRevisionService store. Only the literal true.` and `      SESSION_CORE_ARTIFACT_REVISIONS_ENABLED: ${SESSION_CORE_ARTIFACT_REVISIONS_ENABLED:-false}`.

## EP-3-S2
shared-edit apps/Model Plane/docs/capability-ownership-matrix.md: add a row 'Artifact revision store (artifact_revisions, ArtifactRevisionService) | session-core | writer: model-gateway shadow dual-write (EP-3 S2)'.

## EP-3-S2
shared-edit apps/Model Plane/docs/ZDR.md boundary table: add a row for **Artifact revisions (`artifact_revisions`, session-core, EP-3 S2)**: built dark behind MODEL_GATEWAY_ARTIFACT_REVISIONS (off) and SESSION_CORE_ARTIFACT_REVISIONS_ENABLED (off); no row from a ZDR turn (no artifacts; the RPC refuses ZDR and waived credentials with zdr_durable_mutation_forbidden); a waived turn is skipped by the gateway and a thread holding a waived run is refused (artifact_revisions_waived_thread); erased with the thread (cascade plus delete_thread_rows) and the org (gdpr purge). No posture-matrix row.

