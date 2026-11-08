## EP-8-S10
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the session-core environment, right after line `SESSION_SPACE_MEMORY_ENABLED: ${SESSION_SPACE_MEMORY_ENABLED:-false}` (~1129), add `      SESSION_AGENT_MEMORY_ENABLED: ${SESSION_AGENT_MEMORY_ENABLED:-false}`

## EP-8-S10
shared-edit apps/Model Plane/docs/ZDR.md: after the 'Space memory (agent_memory scope=space, EP-8 S6)' row (~line 92), add the row: `| **Agent memory (agent_memory scope='agent', EP-8 S10)** | No agent note written or recalled on a ZDR or waived turn | Built dark behind SESSION_AGENT_MEMORY_ENABLED (off; also needs SESSION_SPACE_MEMORY_ENABLED). Written only by the woken agent's save_memory (topic AGENT_SELF, origin model_tool) on its own verified wake run; recalled only on runs addressed to that agent in that Space; never mirrored to Letta; erased by Space deletion; erasure on binding revocation pending its RPC. |`

## EP-14-S2
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: in the MODEL_ACCESS record, insert after the line `  'chat.set_agent_mode': exempt('agent_cannot_self_govern', 'context_write'),` the line `  'chat.set_transcript_density': exempt('ui_state_only', 'external_write'),`. MODEL_ACCESS is typed Readonly<Record<ActionId, ...>>, so typecheck fails and isModelExecutableAction has no row without it.

## EP-6-S10
shared-edit apps/Model Plane/rust/services/model-gateway/src/run_context.rs: in fn compose, the literal `RunContext { authored_instructions, history, memory_block, status: Some(status), user_profile: ..., }` (around line 796) needs a new line after the user_profile line: `        // Never composed here: the Do launch adds the role catalogue after `prepare` (EP-6 S10).` then `        agent_roles: Vec::new(),`. Without it the build breaks.

## EP-6-S10
shared-edit apps/Model Plane/rust/services/model-gateway/src/normalize.rs: in fn make_request's InvokeRequest literal, after `            ui_locale: None,` (line 616) add `            agent_roles: None,`.

## EP-6-S10
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs: (1) in FIELD_TREATMENTS, after `    ("ui_locale", Treatment::Honored),` add `    ("agent_roles", Treatment::Inert("no tool loop runs on a ZDR turn, so no subagent takes a role; the catalogue is never built")),` (2) in invoke_field_names' destructure, after `        ui_locale: _,` add `        agent_roles: _,` (3) in the returned vec, after `        "ui_locale",` add `        "agent_roles",`.

## EP-6-S10
shared-edit apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs: in cases(), after `        ("ui_locale", with("ui_locale", json!("no")), None),` add `        ("agent_roles", with("agent_roles", json!([{"agent_ref": "a", "version": 1}])), None),`.

## EP-6-S10
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment, next to `MODEL_GATEWAY_USER_PROFILE_PREFIX: ${MODEL_GATEWAY_USER_PROFILE_PREFIX:-false}` add `      MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE: ${MODEL_GATEWAY_SUBAGENT_ROLE_CATALOGUE:-false}`. In the execution-core environment (near EXECUTION_CORE_AUTH_AUDIENCE) add `      EXECUTION_CORE_SUBAGENT_ROLES: ${EXECUTION_CORE_SUBAGENT_ROLES:-false}`.

## EP-4-S4
shared-edit apps/Model Plane/rust/services/execution-core/src/mcp_gateway.rs: in fn proxy_request, inside the pb::ProxyMcpToolRequest literal, add the line `        allow_elicitation: false,` right after `        expected_digest: expected_digest.to_owned(),` (keep the file's CRLF). Without it execution-core does not compile against the new proto field. EP-4-S7 later sets it from GovernedMcp.

## EP-4-S4
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block, right after the `HOOKS_DURABLE: ${HOOKS_DURABLE:-false}` line, add: `      # EP-4 S4: MCP elicitation (a server's form held in the gateway while the owner answers). Only the literal true.`, then `      MCP_ELICITATION_ENABLED: ${MCP_ELICITATION_ENABLED:-false}`, then `      # Hold window of an unanswered form, seconds (default 600, clamped 60-1800).`, then `      MCP_ELICITATION_HOLD_SECONDS: ${MCP_ELICITATION_HOLD_SECONDS:-600}`.

## EP-4-S5
shared-edit apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs: in execute_ask_user, the `pb::CreateRunQuestionRequest { ... }` literal (after `idempotency_key: format!("{run_id}:{step_id}"),`) add `origin: String::new(),` and `origin_label: String::new(),` (it has no `..Default::default()`, so it will not compile without them).

## EP-4-S5
shared-edit apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs: in fn scripted_question, the `RunQuestion { ... }` literal, after `answered_message_id: String::new(),` add `origin: String::new(),` and `origin_label: String::new(),`.

## EP-12-S2
shared-edit apps/Control Plane/scripts/plane-principal-registry.jq: (1) in known_row_keys add "apiKeyExchange" to the list (alphabetical: put it first, before "allowAnyOrg"); (2) in check_maps, after the tokenExchange block, add `| (if ($row | has("apiKeyExchange")) then $row.apiKeyExchange as $l | if ($l | non_empty_strings | not) then fail($name; "\"apiKeyExchange\" must be a non-empty array of non-empty strings") elif ([$l[] | IN("model-gateway", "session-core", "inference-core", "quarry", "browser-broker")] | all | not) then fail($name; "\"apiKeyExchange\" audiences must be model-gateway, session-core, inference-core, quarry or browser-broker") else . end else . end)`; (3) in the reduce, after `+ optional($row; "tokenExchange")` add `+ optional($row; "apiKeyExchange")`. Without this the assembler refuses any policy row carrying the field.

## EP-12-S2
shared-edit apps/Control Plane/config/plane-service-principals.json (only after the jq edit lands): on the "verevon-gateway" row add `"apiKeyExchange": ["model-gateway", "session-core", "inference-core"]`. It stays inert while API_KEY_AGENT_EXCHANGE_ENABLED is off. Then re-run scripts/tests/principal-registry-parity.test.sh and plane-service-principals-policy.contract.spec.ts (its toMatchObject assertions on that row tolerate the extra key).

## EP-12-S2
shared-edit apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md: in the auth-core token-issuance section add the line: `POST /api/:audience/api-key-exchange` (EP-12 S2 / INNH-10 CP-1): the only path that mints an `api_key` principal token (sub apikey:<key_id>, acting_user_id attribution only, TTL <= 120 s, jti); callers opted in per audience by registry `apiKeyExchange`; switch API_KEY_AGENT_EXCHANGE_ENABLED (on top of API_KEY_ENABLED), default off; durable audit api_key_token_exchanged / api_key_token_denied, fail-closed.

## EP-12-S3
shared-edit apps/Model Plane/api/contracts/agent-api-v1.json.sha256: create the file with one line: 0bd0c4974e6be687e548dddfe37c8b9f8d93d72e4946f6e1f3411dae1146dabb  agent-api-v1.json (sha256 of the golden as written, LF). The parity test include_str!s this pin, so the batch does not compile until it exists. Recompute with sha256sum if the golden changes.

## EP-12-S3
shared-edit scripts/tests/zdr-contract-sync.test.sh line 118: in the GOLDENS row 'apps/Model Plane/api/contracts/zdr-refusals.json|<digest>' replace 5a14940f482bd0af7b546a0b620d1357c9c5a292518cd70f826fe49fe878caaf with the regenerated digest.

## EP-12-S3
shared-edit apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts: (a) after line 63 '  | 'user_profile'' add the union members '  | 'agent_api'', '  | 'conversation_export'', '  | 'conversation_share'', '  | 'conversation_import''. (b) after line 192 '  user_profile: { no: 'den faste profilen', en: 'the standing profile' },' add '  agent_api: { no: 'agent-API', en: 'the Agent API' },', '  conversation_export: { no: 'eksport av samtaler', en: 'conversation export and replay' },', '  conversation_share: { no: 'deling av samtaler', en: 'conversation sharing' },', '  conversation_import: { no: 'import av samtaler', en: 'conversation import' },'. Without these, zdr-refusals.test.ts (keys must equal the golden) and the Record<ZdrFeatureKey, Copy> typecheck fail. The Norwegian wording is a proposal for owner review.

