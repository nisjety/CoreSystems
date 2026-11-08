## EP-8-S3
shared-edit apps/Model Plane/rust/services/model-gateway/src/sse.rs (REQUIRED TO COMPILE): in fn authored_layers, the AuthoredLayers literal at about line 8179. After the line `        persona: req.persona_instructions.clone(),` add `        tone: crate::run_context::tone_for_turn(req.persona_tone.as_deref()),`.

## EP-8-S3
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs (REQUIRED TO COMPILE, each anchor occurs once): (a) after line 160 `    ("persona_instructions", Treatment::Honored),` add `    ("persona_tone", Treatment::Honored),`; (b) after line 272 `        persona_instructions: _,` add `        persona_tone: _,`; (c) after line 321 `        "persona_instructions",` add `        "persona_tone",`.

## EP-8-S3
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs (REQUIRED TO COMPILE): in fn make_request (InvokeRequest literal), after line 611 `            persona_instructions: None,` add `            persona_tone: None,`.

## EP-8-S3
shared-edit apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs (REQUIRED, every FIELD_TREATMENTS entry needs a case row): in cases(), after the persona_instructions tuple (about lines 307-311) add the tuple `("persona_tone", with("persona_tone", json!("direct")), None),` in that file's multi-line layout.

## EP-8-S3
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs (REQUIRED: without it settings.set_tone_preset has no backend, which the action-registry test policy forbids, and a tone-only PUT would reset the four AI fields): insert after dispatch_settings_update_setting (ends about line 3685) the function `// Saves the caller's reply tone (EP-8-S3). user-core writes every AI field back on a PUT, so the four existing fields are read first and sent unchanged; a PUT of the tone alone would reset them.` / `pub(super) async fn dispatch_settings_set_tone_preset(state: &AppState, user: &AuthenticatedUser, input: &Value) -> Response {` / `    let tone = ticket_string(input, "tonePreset");` / `    if !crate::domains::persona::is_tone_preset(tone) {` / `        return ticket_bad_request("settings.set_tone_preset requires a tonePreset of default, professional, friendly, direct, warm or neutral");` / `    }` / `    let actor = actor_for_user(user);` / `    let url = format!("{}/api/v1/settings/ai", state.user_core_url);` / `    let (status, Json(mut settings)) = proxy_json(state, Method::GET, &url, None, None, Some(&actor), None).await;` / `    if !status.is_success() {` / `        return owner_json_response_to_envelope("settings.set_tone_preset", user, status, settings);` / `    }` / `    if let Some(object) = settings.as_object_mut() {` / `        object.remove("tonePresetsAvailable");` / `        object.insert("tonePreset".to_owned(), json!(tone));` / `    }` / `    let (status, Json(resp)) = proxy_json(state, Method::PUT, &url, Some(settings), None, Some(&actor), None).await;` / `    owner_json_response_to_envelope("settings.set_tone_preset", user, status, resp)` / `}`

## EP-8-S3
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/handlers.rs (REQUIRED with the previous entry): (a) in the use list of dispatchers (about line 64) add `dispatch_settings_set_tone_preset,` between dispatch_settings_delete_api_key and dispatch_settings_update_me; (b) after the `"settings.update_setting" => {` arm (about lines 274-276) add the arm `"settings.set_tone_preset" => {` / `    dispatch_settings_set_tone_preset(&state, &user, &body.input).await` / `}`.

## EP-8-S3
shared-edit apps/Model Plane/deploy/docker-compose.yml (lead applies; compose is not owned): in service model-gateway environment, after line 420 `      MODEL_GATEWAY_USER_LOCALE_CONTEXT: ${MODEL_GATEWAY_USER_LOCALE_CONTEXT:-false}` add `      MODEL_GATEWAY_TONE_PRESETS: ${MODEL_GATEWAY_TONE_PRESETS:-false}`.

## EP-8-S3
shared-edit apps/Control Plane/docker-compose.yml (lead applies): in service user-core (block lines 378-530, environment lines 397-479), after line 446 `            SPACE_POLICY_ADMIN_WRITES: ${SPACE_POLICY_ADMIN_WRITES:-false}` add `            TONE_PRESETS_VISIBLE: ${TONE_PRESETS_VISIBLE:-false}`.

## EP-8-S6
shared-edit apps/Model Plane/rust/services/session-core/src/main.rs: add the line `mod space_memory;` in the alphabetical mod list right after `mod space;` (line 53). session-core does not compile without it. main.rs is owned by EP-14-S6 in this batch.

## EP-8-S6
shared-edit apps/Model Plane/rust/services/model-gateway/src/memory_provenance.rs: in the tests `fn entry(...)` MemoryEntry literal, after `withheld_reason: String::new(),` add `scope: String::new(),`, `scope_id: String::new(),` and `source_label: String::new(),`. The literal is exhaustive and stops compiling once the new proto fields exist.

## EP-8-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment block add `SESSION_SPACE_MEMORY_ENABLED: ${SESSION_SPACE_MEMORY_ENABLED:-false}`. The owner adds the .env.example line `SESSION_SPACE_MEMORY_ENABLED=false`, with a comment that it stays off until D2-480, D2-481 and D2-461 are closed.

## EP-8-S6
shared-edit apps/Model Plane/ZDR.md: in the store table add the row `agent_memory scope='space' (EP-8 S6, SESSION_SPACE_MEMORY_ENABLED off): no write on a ZDR credential (session:write refused) or a waived credential/thread; never mirrored to Letta; erased by Space deletion in S7`.

## EP-14-S6
shared-edit apps/Model Plane/rust/crates/mp-contracts/build.rs: in the mp_protos list, add the line `"model_plane/v1/run_ledger.proto",` right after `"model_plane/v1/routing_policy.proto",`.

## EP-14-S6
shared-edit apps/Model Plane/rust/services/session-core/src/grpc.rs (serve): (1) in the generated-server import list, next to `routing_policy_server::RoutingPolicyServer,`, add `run_ledger_service_server::RunLedgerServiceServer,`. (2) After `let routing = crate::routing_policy_grpc::RoutingPolicyService::new(pool.clone());`, add `// EP-14 S6: the request snapshot writer (refuses unless SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED).` and then `let run_ledger = crate::run_ledger_grpc::RunLedgerGrpc::new(pool.clone());`. (3) After `.add_service(RoutingPolicyServer::with_interceptor(routing, auth.clone()))`, add `.add_service(RunLedgerServiceServer::with_interceptor(run_ledger, auth.clone()))`. Optionally add "model_plane.v1.RunLedgerService" to HEALTH_SERVICE_NAMES.

## EP-14-S6
shared-edit apps/Control Plane/config/plane-service-principals.json, entry "inference-core": add "session:request-snapshots:write" to "scopes" and to "scopesByAudience"."session-core", and change "retentionByAudience"."session-core" from "zdr" to "persistent". The owner must decide first (depth finding). Then check apps/Control Plane/auth-core/src/auth/plane-service-principals-policy.contract.spec.ts and any pinned copy.

## EP-14-S6
shared-edit apps/Model Plane/deploy/docker-compose.yml: under inference-core environment add `INFERENCE_REQUEST_CAPTURE: ${INFERENCE_REQUEST_CAPTURE:-false}`. Capture also needs SESSION_CORE_URL/ADDR, AUTH_CORE_URL and INFERENCE_ROUTING_SERVICE_API_KEY, which the routing-policy client already uses. Under session-core environment add `SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED: ${SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED:-false}`, `SESSION_CORE_REQUEST_SNAPSHOTS_SWEEP_ENABLED: ${SESSION_CORE_REQUEST_SNAPSHOTS_SWEEP_ENABLED:-false}` and `SESSION_CORE_REQUEST_SNAPSHOT_TTL_DAYS: ${SESSION_CORE_REQUEST_SNAPSHOT_TTL_DAYS:-7}`. The owner adds the same names to .env.example.

## EP-14-S6
shared-edit apps/Model Plane/ZDR.md: add the store run_request_snapshots and run_request_snapshot_blobs (EP-14 S6). Content: prompt text and tool schemas. Never written for ZDR or waived runs: inference-core skips them and session-core refuses with zdr_durable_mutation_forbidden. TTL 7 days (cap 30), cascade on run delete, explicit org purge. User-subject erasure is not covered (Q-ERASE).

## EP-6-S7
shared-edit apps/Application Plane/convex-core/convex/schema/agents.ts: (a) after line 9 `import { v } from "convex/values";` add the line `import { agentDefinitionBodyFields } from "./agentDefinitionVersions";`. (b) Immediately before line 98 `    createdBy: v.optional(v.id("users")),` (inside the agents table) insert these lines: `    // EP-6 S7 (AGENT_DEFINITION_VERSIONS): the version the gateway serves (absent = legacy row,`, `    // served from systemPrompt and tools as before) and the editable draft a publish copies`, `    // into an immutable agentDefinitionVersions row. The draft is not the status "draft".`, `    publishedVersion: v.optional(v.number()),`, `    draft: v.optional(v.object(agentDefinitionBodyFields)),`

## EP-6-S7
shared-edit apps/Application Plane/convex-core/convex/schema/spaces.ts: immediately before line 201 `    projectionVersion: v.number(),` (the spaceAgentBindings table; line 230 is another table, do not touch it) insert: `    // EP-6 S7 (AGENT_DEFINITION_VERSIONS): the agent version this binding pins. Absent means`, `    // follow the agent's published version. Written only by the EP-6 S9 invoke-time pin.`, `    pinnedVersion: v.optional(v.number()),`

## EP-6-S11
shared-edit apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx: (a) add resolvePresetAgent and type ResolvedPresetAgent to the import from '@/shared/actions/preset-agents' (lines 40-44). (b) In runTask replace line 797 `const presetActions = preset ? presetAgentChatActions(preset) : undefined` with: `const resolved = preset ? resolvePresetAgent(preset, new Set<string>()) : undefined`, then `if (resolved && !resolved.available) return`, then `const presetActions = resolved ? presetAgentChatActions(resolved) : undefined`. The empty Set stands in until an admitted-set client exists; with the switch off this is a no-op.

## EP-6-S11
shared-edit apps/Frontend Plane/verevonv3/src/features/agents/components/AgentRunConsole.tsx: in PresetPicker, inside `<For each={presetAgents}>` (line 1274) compute `const available = () => resolvePresetAgent(preset, new Set<string>()).available`, set the button to `disabled={props.disabled || !available()}`, and render `<Show when={!available()}>` under the description span with `i18n.tr('Ikke tilgjengelig i denne organisasjonen', 'Not available in this org')`. Add Show to the solid-js import if it is absent.

