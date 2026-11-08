## EP-11-S8
shared-edit apps/Model Plane/go/services/capability-core/cmd/main.go: directly after the line `api.NewSkillBundlesHandler(pool, api.SkillBundlesAPIFromLookup(os.Getenv)).Register(protectedMux)` add `api.NewSkillImportHandler(pool, api.SkillImportFromLookup(os.Getenv)).WithPublisher(recPub).Register(protectedMux)`

## EP-11-S8
shared-edit apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go: (1) in `type SkillsHandler struct` after the `bundleRetention bool` field add `\t// importEnabled is SKILL_IMPORT_ENABLED (EP-11 S8, with bundles and drafts): on, the listing says so\n\t// and the Skills page offers an import; off, every body is as it was.\n\timportEnabled bool`; (2) in `NewSkillsHandler` add `importEnabled: SkillImportFromLookup(os.Getenv)` to the struct literal; (3) in listOrCreate GET, after the `if h.draftPublish { body["drafts_enabled"] = true }` block add `\t\t// EP-11 S8: the Skills page offers an import only when the listing says it is on.\n\t\tif h.importEnabled {\n\t\t\tbody["import_enabled"] = true\n\t\t}`

## EP-11-S8
shared-edit apps/Model Plane/rust/services/model-gateway/src/http_routes.rs: in the Skills route block, directly BEFORE `.route("/v1/skills/:id", get(get_skill_proxy)...)`, add `.route("/v1/skills/import", post(import_skill_proxy).layer(DefaultBodyLimit::max(SKILL_IMPORT_BODY_LIMIT_BYTES)))`; add near the other skill proxies: `/// EP-11 S8: a skill imported from a SKILL.md or a zip, sent as base64 inside JSON; capability-core\n/// answers 404 while SKILL_IMPORT_ENABLED is off. 12 MiB carries the largest bundle in base64.\nconst SKILL_IMPORT_BODY_LIMIT_BYTES: usize = 12 * 1024 * 1024;\nasync fn import_skill_proxy(State(s): State<AppState>, Extension(c): Extension<Claims>, bearer: VerifiedCapabilityBearer, Json(b): Json<Value>) -> Result<Json<Value>, HttpJsonError> {\n    proxy_to_capability_core(&s, &c, &bearer, "skills/import", "POST", Some(&b)).await\n}` (do NOT inject org_id; capability-core ignores it anyway). If a durable-mutation inventory test lists proxy routes, add this one.

## EP-11-S8
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after `'skills.version.restore': exempt('agent_cannot_self_govern', 'context_write'),` add `'skills.import': exempt('agent_cannot_self_govern', 'context_write'),`

## EP-11-S8
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.test.ts: bump `expect(ids).toHaveLength(316)` by one (to whatever the batch total is +1) and add the comment line `// +1 after EP-11-S8 registered skills.import, the Skills page's import from a file.` above it

## EP-11-S8
shared-edit apps/Model Plane/deploy/docker-compose.yml: capability-core environment add `SKILL_IMPORT_ENABLED: ${SKILL_IMPORT_ENABLED:-false}`

## EP-11-S8
shared-edit apps/Model Plane/docs/ZDR.md: after the '/learn (EP-11 S7)' table row add `| **Skill import (EP-11 S8)** | Refused for ZDR principals | capability-core POST /api/v1/skills/import refuses a ZDR or posture-less principal (authz.go:81, and the handler answers zdr_feature_not_supported with feature skill_authoring) before reading the file; model-gateway refuses a ZDR token before forwarding (no posture-matrix row). Imported text and files are org configuration (D9), stored as a switched-off draft and draft bundle, erased with the skill and the org. Built dark behind SKILL_IMPORT_ENABLED (off; needs SKILL_BUNDLES_ENABLED and SKILL_DRAFT_PUBLISH). |`

## EP-4-S10
shared-edit apps/Frontend Plane/verevonv3/apps/gateway/src/domains/chat/shared.rs: change `async fn proxy_model_json_with_delegations(` to `pub(crate) async fn proxy_model_json_with_delegations(` (the BFF tool-call handler in domains/mcp.rs needs capability + session + execution + data-plane + inference on one request; no other helper sends all five)

## EP-4-S10
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: after the line `  'chat.stage_mcp_app_message': exempt('ui_state_only', 'read'),` add `  'chat.mcp_app_tool_call': exempt('agent_cannot_self_govern', 'read'),`

## EP-4-S10
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/McpAppFrame.tsx (wiring, needed before VITE_MCP_APP_TOOL_CALLS does anything): (1) import `createMcpAppToolCaller` from '../lib/mcp-app-bridge'; (2) add `runId?: string` to the props of McpAppsForAnswer, McpAppFrame and McpAppDocument and pass `runId={props.runId}` down at `<McpAppFrame ...>` and `<McpAppDocument ...>`; (3) in McpAppDocument's `createMcpAppBridgeHost({...})` add `toolCaller: createMcpAppToolCaller({ runId: props.runId, serverId: props.serverId }),`

## EP-4-S10
shared-edit apps/Frontend Plane/verevonv3/src/features/chat/components/ChatMessages.tsx: in the EP-4 S9 block `<McpAppsForAnswer calls=... />` add the prop `runId={props.message.runId}`

## EP-12-S5
shared-edit apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs: in no_raw_retention_claim_is_read_outside_the_named_inventory's RAW_READ_INVENTORY, right after the ("auth.rs", "verify_delegated_api_key_browser_bearer", ...) row, add: (
                "auth.rs",
                "verify_delegated_api_key_session_bearer",
                "posture equality between an api key's ingress and session bearers, not a decision",
            ), (without it that lint fails on the new function's claims.zdr comparison)

## EP-12-S5
shared-edit apps/Model Plane/deploy/docker-compose.yml: in the model-gateway environment block, after the MODEL_GATEWAY_THREAD_EXPORT line, add:
      # EP-12 S5: the Agent API read and control routes (/v1/agent/*) for API keys with agent:runs. Only the literal true.
      MODEL_GATEWAY_AGENT_API: ${MODEL_GATEWAY_AGENT_API:-false}

## EP-12-S5
shared-edit apps/Model Plane/docs/ZDR.md: in the feature table after the 'session-core api_key principal (EP-12 S4)' row, add: | **Agent API read and control routes (EP-12 S5)** | Refused for ZDR api_key tokens | model-gateway refuses every /v1/agent/* route with 412 zdr_feature_not_supported (details.feature agent_api) before any upstream call, for any posture that is not persistent; no posture-matrix row (pinned by agent_api_routes::tests::zdr_key_is_refused_before_upstream). Built dark behind MODEL_GATEWAY_AGENT_API (off). |

## EP-12-S11
shared-edit apps/Frontend Plane/verevonv3/src/shared/actions/model-access.ts: insert the row `  'thread.export': exempt('no_owner_executor', 'read'),` directly after the line `  'chat.inspect_run_request': exempt('no_owner_executor', 'read'),` (currently line 345) inside the MODEL_ACCESS object. Required: the registry is `as const`, so ActionId is a literal union and MODEL_ACCESS (Readonly<Record<ActionId, ...>>) fails typecheck without it; the thread.export test also reads this row.

