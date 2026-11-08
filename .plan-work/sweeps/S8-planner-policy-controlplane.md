# S8 planner-policy-controlplane

Scope: F-t2-010, F-t2-022, F-t2-042, F-t2-043, F-t2-045, F-t2-062. Source of truth: live tree `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted work). Read-only. No product file was touched. Date 2026-10-14.

Tags: [C] committed and unchanged; [M] committed but modified in live tree; [U] untracked (exists only as uncommitted work); [NEW] a file a plan would create. Verdicts: OK | DRIFTED (corrected range given) | FALSE / [CLAIM-FALSE] (what the code does) | MISSING. "Uncommitted" below means the hunk is in the live tree and absent from afb83928.

Cross-item overlaps (merge-conflict hotspots):
1. `execution-core/src/runtime_loop/agent.rs` [M] (13521 lines): 042 (web tool descriptions 4184-4192), 043 (SNIPPET_* 83-170, skill injection 1006-1014), 045 (data_category 314-327, save_memory 4235-4241).
2. `execution-core/src/capability_policy.rs` [C] (1051): 010 and 022 (browser.* to cap.browser.open, 793), 042 (web_search/web_fetch to cap.tool.http, 794), 045 (save_memory to cap.memory.index, 808).
3. `execution-core/src/web_tools.rs` [C] (717): 042 (search_body 254-256, extract_body 259-261) and 062 (x-quarry-org header 92-98, no run id).
4. `model-gateway/src/http_routes.rs` [M] (11681): 010 (suggest-action 1991-2097) and 062 (deep-research waiver refusal 8937-8939).
5. `quarry-edge/src/agent_routes.rs` [C] (3737): 022 (validate_run_grant 314-338, sensitive approvals 144-300, navigate check 2297-2304, ZDR 1441-1445), 043 (driver_capabilities 543-544 and 1658, require_driver_capabilities 1180, call 1305), 062 (agent_router 3659-3735, start_run 1273, step 2228).
6. `quarry-edge/src/routes.rs` [M] (1564) and `quarry-edge/src/main.rs` [M] (1569): 022 (RunPolicy::default at routes.rs 461 and main.rs 1493; ZDR floor routes.rs 416), 062 (route table routes.rs 133-192).
7. `quarry-edge/src/auth.rs` [C] (1242): 022 (Claims and zdr_floor 61-140) and 062 (require_service_route_scope 504-545).
8. `quarry-edge/src/search_routes.rs` [M] (2708): 042 (SearchRequest 50-75; committed `citations` field) and 062 (run_id_str 978, UsageEvent 1006-1012).
9. `model-gateway/src/tool_loop.rs` [M] (13032), `retrieval_metadata.rs` [M] (520), `sse.rs` [M] (13325): 042 (4304-4311 guidance, 33-222 hint helpers), 043 (4359-4361), 045 (4469), 062 (sse 2354-2425).
10. `session-core/migrations/`: next free sequence is 0046 (0044 and 0045 are [U]). Any new migration for 045 (pending or taint state) or 062 (research job record) takes 0046. `session-core/src/grpc.rs` [M] and `memory_grpc.rs` [M] are shared by 022 (ZDR retention posture) and 045.

---

## F-t2-010 Turn on the LLM browser planner by default, with budgets, a browsing procedure prompt and connector-first routing

### 1. Citation check (25 checked; none skipped)

| # | Citation | Tag | Verdict | What the live code says |
|---|---|---|---|---|
| 1 | llm_planner.rs:128-148 | [C] | OK | `from_env` at 134; gate 135-137 accepts 1/true/TRUE/yes; returns None at 138-140 |
| 2 | llm_planner.rs:130-139 | [C] | OK | same function |
| 3 | llm_planner.rs:1-30 | [C] | OK | module doc 9-11 states the `QUARRY_BROWSER_AGENT_LLM=1` gate |
| 4 | llm_planner.rs:53-65 | [C] | OK | ACTION_SCHEMA enum (line 56): navigate, click, type, scroll, wait, extract, done. No observe, press, select, back, wait_for |
| 5 | llm_planner.rs:170-263 | [C] | OK | `planner_infer_request` 200-263; `max_tokens: 512` at 263 |
| 6 | llm_planner.rs:237-241 | [C] | OK | system prompt 236-242 is one paragraph (ref targeting, frame_id, done). No verify, escalation or stop rules |
| 7 | tool_bridge/mod.rs:170-232 | [C] | OK | `LlmPlanner::from_env` at 204; `None` leaves the deterministic planner in charge |
| 8 | tool_bridge/mod.rs:416-460 | [C] | OK | `BrowserMcpRequest::parse` 422-462; browser.observe 429; browser.act 444 |
| 9 | browser_agent.rs:91-99 | [C] | OK | `ActionType`: Goto, Click, Type, Extract, Observe, Scroll, Wait |
| 10 | browser_agent.rs:225-345 | [C] | OK | require_approval trait 221-239; `PlanConfig` 242-; max_steps 249, max_runtime_s 250, stop_criteria 252, max_cost_usd 255, zdr 257, postcondition 291; `check_limits` 320-330; `evaluate_stop_criteria` 348-355 |
| 11 | browser_agent.rs:621 | [C] | OK | `run_browser_agent_loop` |
| 12 | quarry_agent.rs:125-175 | [C] | OK | `AgentAction`: Press 140, Select 146, WaitFor 153, Back 164, GetContent 165 |
| 13 | quarry_agent.rs:340-400 | [C] | OK | `action_to_wire` 346-402; Extract and Observe map to GetContent at 400 |
| 14 | model-gateway http_routes.rs:2015-2100 | [M] | OK, wider than audit | prompt 2013-2021 names press, select, wait_for, screenshot; handler 2023-2097; `BROWSER_ACTION_SCHEMA` 1991-2011 enum includes press, select, wait_for; `max_tokens: 700` at 2065; inference `zdr: true` at 2067 |
| 15 | http_routes.rs:2023-2100 (json) | [M] | OK | same handler |
| 16 | browser_run.rs:296 | [M] | OK | `#[allow(clippy::too_many_lines)]` on `browser_run_start` (297) |
| 17 | browser_run.rs:297-310 | [M] | OK, mechanism uncommitted | ZDR refusal now at 307-313 and calls `browser_run_zdr_blocked(...)?`. Committed version (307) was a bare bool check |
| 18 | browser_run.rs:631-633 (json) | [M] | DRIFTED (minor) | helper is 628-633; returns `Result<bool, ApiError>` via `zdr_posture::zdr_or_refuse`. Uncommitted |
| 19 | gateway domains/browser.rs:1056 | [C] | OK | `suggest_action` at 1056 |
| 20 | verevonv3 src/shared/actions/model-eligibility.ts:137 | [C] | OK (weak) | `isModelExecutableAction` 137-139. grep for "browser" in the file: 0 hits, so no browser.* id is model-executable there |
| 21 | verevonv3 src/shared/actions/agent-tools.ts:113 | [C] | OK | `createVerevonActionToolSpecs` 112-114 |
| 22 | verevonv3 src/features/dashboard/home/browser-loop.ts:1-30 | [C] | OK, stale claim | header 11-26 says durable mode is "pending wiring" and `performBrowserAutoRun` still exists. Live KnowledgeComposer.tsx 1020-1029 says the client loop was replaced and `beginBrowserAiRun` is wired (see Surprises) |
| 23 | tests/browser_agent_e2e.rs:17 | [C] | OK | doc line 17. Both live tests are `#[ignore]` (42, 587) |
| 24 | grep QUARRY_BROWSER_AGENT_LLM across apps | — | OK | code hits only llm_planner.rs 10, 130, 135 and e2e doc 17. Plan docs mention it (CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:662 [C]; the matching json line 3145). No compose or env file sets it |
| 25 | json: "nothing under apps/Model Plane sets it" | — | OK | `apps/Model Plane/deploy/docker-compose.yml` [C] has no QUARRY_BROWSER_AGENT_LLM entry |

[CLAIM-FALSE, partial] Card and json say step, time and token budgets "do not exist". Step and time budgets exist: `max_steps` default 20 and `max_runtime_s` default 120 (`tool_bridge/mod.rs:880-881`), enforced by `check_limits` (`browser_agent.rs:320-330`). Only the per-run token or LLM-spend budget is missing.

[CLAIM-FALSE, partial] Card says press, select and back are "unreachable". They are unreachable from the durable planner (`ACTION_SCHEMA` has no such verbs and `action_to_wire` never emits them). But the suggest-action schema already offers press, select and wait_for (`http_routes.rs:1996`), and browser.act maps `SelectRef` (`tool_bridge/mod.rs:546-556`).

### 2. Affected files

- `apps/Model Plane/rust/services/execution-core/src/llm_planner.rs` [C] 1064 lines. `resolve_planner_model` 113-119 (QUARRY_BROWSER_AGENT_MODEL), DEFAULT_MODEL 106, DEFAULT_ADDR 107, `from_env` 134-148, ACTION_SCHEMA 53-66, ACTION_SCHEMA_STRICT 85-100, `next_action` 158, `planner_infer_request` 200-263, `into_browser_action` 304. Callers: `tool_bridge/mod.rs:204` (prod), `tests/browser_agent_e2e.rs:54` (test only).
- `execution-core/src/browser_agent.rs` [C] 1709 lines. ActionType 91-99; PlanConfig 242-; check_limits 320-330; evaluate_stop_criteria 348-355; plan_next_action 511 (check_limits at 519); run_browser_agent_loop 621 (callers: `tool_bridge/mod.rs:213`, e2e:76); gate_persistent_cookie_use 856; gate_risky_action 908.
- `execution-core/src/tool_bridge/mod.rs` [C] 1174 lines. execute_browser_agent 165-227 (QuarryAgentClient::from_env 196, LlmPlanner::from_env 204, BrowserVerificationInputs 208-212); BrowserMcpRequest::parse 422-462; BrowserMcpAction::into_quarry_action 482-556 (ClickRef 492, FrameClickRef 506, TypeRef 521, FrameTypeRef 538, SelectRef 554; arms after 556 not read); judge_browser_procedure call 775; browser_plan_config 863-886 (max_steps default 20 at 880, max_runtime_s default 120 at 881, require_approval default false 886, allowed_domains from broker grant 884).
- `execution-core/src/quarry_agent.rs` [C] 963 lines. AgentAction 44-166; action_to_wire 346-402; `QuarryAgentClient::from_env` 523-535 (QUARRY_BROWSER_AGENT_ENABLED, default off).
- `execution-core/src/postcondition.rs` [C] 2375 lines. `judge_browser_procedure` 1077 (caller tool_bridge/mod.rs:775).
- `execution-core/src/browser_agent/browser_risk.rs` [C] 1288 lines. Risk classifier feeding the HITL gates.
- `model-gateway/src/http_routes.rs` [M] 11681 lines. BROWSER_ACTION_SCHEMA 1991-2011, BROWSER_SUGGEST_SYSTEM_PROMPT 2013-2021 (caller 2054), browser_suggest_action 2023-2097 (route at 223). Uncommitted hunks touch 2032-2036 (ZDR gate now `Route::named`).
- `model-gateway/src/browser_run.rs` [M] 1330 lines. browser_run_start 297 (ZDR refusal 307-313), browser_run_control 663-669, browser_run_zdr_blocked 628-633. Uncommitted.
- `model-gateway/src/zdr_posture.rs` [U] 1259 lines and `mp-contracts/src/zdr_refusal.rs` [U]. The new refusal machinery that browser_run.rs now depends on.
- `apps/Model Plane/proto/model_plane/v1/browser_agent.proto` [C] 236 lines. BrowserAgentInput: max_steps 168, max_runtime_s 171, require_approval 180, allowed_domains 174. No cost or token budget field found by grep (max_cost_usd absent). A per-run token budget needs a new field here (next field number not verified).
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` [C]. suggest_action 1056; start-run body 1233-1246 (require_approval false 1242, max_cost_usd 1243, zdr server-derived 1246).
- `apps/Frontend Plane/verevonv3/src/features/dashboard/home/KnowledgeComposer.tsx` [M]. beginBrowserAiRun 1029; passed as onBrowserAutoRun at 1583.
- `src/features/dashboard/home/BrowserChrome.tsx` [C] (prop 430, call 868); `src/features/chat/components/ChatLiveRunPanel.tsx` [C] (controlBrowserAiRun 451); `src/shared/api/browser-run-client.ts` [C] (controlBrowserAiRun 92); `src/shared/api/browser-client.ts` [C] (suggestBrowserAction 457); `src/features/dashboard/home/browser-loop.ts` [C] (stale header 11-26).
- Org-policy flag home (if persisted): `apps/Control Plane/org-core/internal/org/types.go` [C] 365 lines. Entitlement pattern: defaultEntitlements 266-271, computed `feature.zero_data_retention` 273-284. Migrations: `apps/Control Plane/org-core/migrations/` [C], last 020, next free **021** [NEW].
- Compose: `apps/Model Plane/deploy/docker-compose.yml` [C]. No QUARRY_BROWSER_AGENT_LLM entry. A default-on change would be added here.

### 3. Reusable but unwired

- `ACTION_SCHEMA_STRICT` (llm_planner.rs:85-100): no production caller. grep of `apps/Model Plane/rust/services` finds it only in llm_planner.rs (52 doc, 85, 192, 276, 667, 670, 701, 735, 778, 1055, 1058) and in `tests/browser_agent_e2e.rs` (34, 596, 809). The doc at line 52 says it is unused by `next_action`.
- `AgentAction::Press`, `Select`, `WaitFor`, `Back` (quarry_agent.rs 140-165): wire-only for the durable planner (no BrowserAction maps to them in `action_to_wire`).
- BROWSER_ACTION_SCHEMA press/select/wait_for: wired into the suggest-action route (caller gateway browser.rs:1056).
- `postcondition::judge_browser_procedure` (postcondition.rs:1077): wired (tool_bridge/mod.rs:775). The verification step exists even though the planner prompt has none.
- `check_limits` and PlanConfig budgets (browser_agent.rs:320-330): wired and default-bounded.
- No token-budget or spend-budget reader for the LLM planner was found (grep for budget in planner code returns only the cost-core intent-mode comments in llm_planner.rs:16-22).

### 4. Tests

- execution-core unit tests: llm_planner.rs 31 `#[test]` (navigate_maps_to_goto_with_url 508, click_maps_to_click_with_selector 517, type_maps_to_type_with_value 526, done_is_terminal 538, unknown_action_falls_back_to_observe 545, planner_model_resolution_defaults_to_verevon_intent_mode 619, strict_* 699-875, planner_request_is_on_the_default_route_with_the_given_model_and_schema 1029); browser_agent.rs 34 (max_steps_exhausts_the_plan_without_reporting_completion 1141, max_runtime 1160, stop_criteria_completes_plan 1204, gate_persistent_cookie_use_* 1528-1571, gate_risky_action_records_deterministic_checkout_not_a_planner_login_label 1630); tool_bridge/mod.rs 11; postcondition.rs 59; browser_risk.rs 30.
- Live-only: `tests/browser_agent_e2e.rs` 2 `#[ignore]` (42: quarry-edge plus inference stack; 587: subscription spend). Not run by CI.
- Command: `cd "apps/Model Plane/rust" && cargo test -p execution-core llm_planner` (also `browser_agent`, `tool_bridge`).
- Frontend: `src/features/dashboard/home/browser-loop.test.ts` [C], `src/shared/api/browser-run-client.test.ts` [C]. Command: `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run src/features/dashboard/home/browser-loop.test.ts` [UNVERIFIED runner name; CLAUDE.md says `pnpm test`].
- CI: `.github/workflows/model-plane-rust-ci.yml` [M] job "Run tests" runs `cargo test --workspace --no-fail-fast` (line 119); path filter `apps/Model Plane/rust/**` (30). `.github/workflows/verevonv3-ci.yml` [M] job "SPA — lint + typecheck + test + build" runs `pnpm verify` (144, 193). Both are modified in the live tree (see Surprises).
- model-gateway `http_routes.rs` has 65 `#[test]`; no suggest-action test name found [UNVERIFIED].

### 5. Feature flags and env

- `QUARRY_BROWSER_AGENT_LLM`: read at llm_planner.rs:135-137; default off; accepts 1, true, TRUE, yes.
- `QUARRY_BROWSER_AGENT_MODEL`: llm_planner.rs:114-118; default `DEFAULT_MODEL` = "verevon-balance" (106).
- `INFERENCE_CORE_URL` or `INFERENCE_CORE_ADDR`: llm_planner.rs:144-146; default http://localhost:9092 (107).
- `QUARRY_BROWSER_AGENT_ENABLED` plus `QUARRY_EDGE_URL`: quarry_agent.rs:524-535; default off. When off, the browser_agent loop fails fast (tool_bridge/mod.rs:194-203).
- Budgets: max_steps default 20, max_runtime_s default 120 (tool_bridge/mod.rs:880-881). Caller-overridable.
- Gateway forces `require_approval: false` (browser.rs:1242) and derives zdr server-side (1246).
- Plan text that is NOT in the live compose: CHAT_GAP_CLOSURE_PLAN_2026-10-01.md [C] proposes `QUARRY_BROWSER_AGENT_LLM: ${QUARRY_BROWSER_AGENT_LLM:-1}` near compose line 1062. Live `deploy/docker-compose.yml` has no such line, so the plan is stale or unimplemented.

### 6. Surprises

1. [CLAIM-FALSE, stale doc] browser-loop.ts header says the durable mode is "pending wiring" and the client loop still exists. In the live tree, `KnowledgeComposer.tsx:1020-1029` says `performBrowserAutoRun` was replaced; `performBrowserAutoRun` appears only in comments. `beginBrowserAiRun` is passed to BrowserChrome (KnowledgeComposer.tsx:1583, BrowserChrome.tsx:430, 868) and ChatLiveRunPanel.tsx:451 calls `controlBrowserAiRun`. The durable run is more wired than the audit says.
2. [CLAIM-FALSE, partial] The suggest-action path already exposes press, select, wait_for and screenshot (`http_routes.rs:1996`).
3. [CLAIM-FALSE, partial] Step and time budgets exist (see section 1).
4. The verification step exists outside the prompt: `postcondition.rs:1077`, called at `tool_bridge/mod.rs:775`.
5. The durable run's ZDR refusal is mid-change. Committed `browser_run.rs:307` had a bare bool check; the live tree routes through `zdr_posture.rs` and `zdr_refusal.rs`, both untracked [U]. Line citations still hold, but a default-on change would depend on uncommitted files.
6. The suggest-action path always sends `zdr: true` to inference (`http_routes.rs:2067`). The SPA path never persists, while the durable path is refused under ZDR.
7. "Connector-first" routing has no code. `list_provider_actions` exists (`integration_tools.rs:257`), but the browser sub-planner never sees provider actions.
8. Uncommitted hunks touch `model-gateway/src/http_routes.rs` (2032-2036, 8924-8986), `browser_run.rs` (307-313, 628-632), and `execution-core/src/runtime_loop/mod.rs` (638-652). None touches the planner symbols (llm_planner.rs, ACTION_SCHEMA*, PlanConfig).
9. CI: `model-plane-rust-ci.yml` [M] adds a session-core-receipts job (uncommitted). It does not change the `cargo test --workspace` step.

---

## F-t2-022 Org-pinned, tighten-only web policy: tri-state capability gates delivered as signed claims and enforced by Quarry

### 1. Citation check (25 checked; none skipped)

| # | Citation | Tag | Verdict | What the live code says |
|---|---|---|---|---|
| 1 | quarry-core/src/policy.rs:1-17 | [C] | OK | `RunPolicy` struct at 6 (derive 5), fields concurrency, delay, retry, proxy, robots, ordering, block, checkpoint, determinism |
| 2 | quarry-runtime/src/policy.rs:60-78 and 55-80 (json "second RunPolicy") | [C] | DRIFTED / [CLAIM-FALSE] citation | 55-80 is `DiscoveryPolicy` (respect_robots etc., Default 71-79). The second `RunPolicy` is at **215-230** (Default to best_effort 225-229) |
| 3 | quarry-edge/src/routes.rs:413-417 | [M] | OK | 414-416: `claims.effective_zdr_mode(req.zdr)` (ZDR floor applied to body flag) |
| 4 | routes.rs:461 | [M] | OK | `policy: quarry_runtime::RunPolicy::default()` |
| 5 | main.rs:1485-1493 | [M] | OK, with comment | line 1493 `RunPolicy::default()`; comment 1489-1492 documents `QUARRY_EDGE__RUN_POLICY=strict` as "future config wiring". No reader found |
| 6 | quarry-runtime/src/observation.rs:542-543 | [C] | OK | `AgentAction::Evaluate` calls `self.browser.evaluate(session, script)` with no policy or approval check in that arm |
| 7 | quarry-edge/src/auth.rs:82-140 and 56-140 | [C] | OK | `Claims` 61-91 (zdr 79-90); `zdr_floor` 121-123 is `!is_service() && zdr == Some(true)`; `effective_zdr` 129-131 |
| 8 | quarry-edge/src/agent_routes.rs:1435-1441 and 1440-1445 | [C] | OK | resume ZDR floor at 1440-1441; start ZDR at 1443 |
| 9 | agent_routes.rs:2296-2304 | [C] | OK | step path: `AgentAction::Navigate` checked against `entry.constraints.allowed_domains` (2297-2304), 403 on failure |
| 10 | agent_routes.rs:314-343 | [C] | OK | `validate_run_grant` 314-338. Broker-domain equality is enforced only when `state.require_browser_grants` (326) |
| 11 | agent_routes.rs:144-300 (json "run policy") | [C] | DRIFTED / wrong purpose | 144 is `sensitive_approval_for_action` (RespondDialog 149, UploadRef 184, FrameUploadRef 191). 284 is `validate_sensitive_approval`. No Evaluate arm |
| 12 | presets.go:45-80 and 55-75 | [C] | OK | `ResolvePreset` 47-54; `defaultPresets` 56; "fast" 59-75 with `Robots: RobotsIgnore` (69) |
| 13 | quarry-control resources.go (production caller of ResolvePreset) | [C] | [CLAIM-FALSE] "never consumed" | `resources.go:474` calls `ResolvePreset` inside `resolveJobPolicy` (469-481), called from `resources.go:408` (job creation). The edge never reads presets |
| 14 | blocklists.go:1-10 | [C] | OK | `MountBlocklists` 8-10 mounts `/v1/blocklists` via mountSimple |
| 15 | store.go:397-402 | [C] | OK | `BlocklistEntry` {ID, Pattern, IsRegex, CreatedAt}. No org column (gap confirmed) |
| 16 | quarry-edge/src/config.rs (not in audit) | [M] | NEW | `security_snapshot_path` 34-37: URLhaus/PhishTank host blocklist file. Read at main.rs:348. This is the edge's only host blocklist source; /v1/blocklists is not read |
| 17 | org-core/internal/org/types.go:255-315 and 266-315 | [C] | OK | defaultEntitlements 266-271; zeroDataRetentionPlans 281-284; `RetentionPosture` 286-316 (ZDRIntent, Entitled, `Effective()` 314-316) |
| 18 | execution-core/src/capability_policy.rs:793-794 | [C] | OK | `"browser_agent" \| "browser.observe" \| "browser.act" => "cap.browser.open"` (793) |
| 19 | capability-core/internal/policy/engine.go:27-60 and 158-215 | [C] | OK | ScopeResolver 38-40; `IsSupportedScope` 47-55 accepts only org and global; `denyOnScope` 158-177; `denyOnGrant` 199-216 |
| 20 | browser-broker/internal/grant/grant.go:30-36 | [C] | OK | `allowedSensitiveActions` 32-36 = respond_dialog, upload_ref, download_ref. No evaluate. Grant struct 39-59 has AllowedDomains 47 and AllowedActions 50 |
| 21 | org-core/internal/http/retention_posture_handler.go (mount) | [C] | OK | route asserted at retention_posture_handler_test.go:73 |
| 22 | auth-core/src/auth/plane-token.controller.ts (claim minting) | [C] | OK | `resolveInteractiveRetentionPosture` 256; service retentionPosture 343-349; `zdr` claim 383-384; subject token zdr 498-502 |
| 23 | auth-core plane-service-principals-policy.contract.spec.ts:143-150 | [C] | OK | `webToolScopes` = extract:read, search:read (150); no wildcard |
| 24 | browser-broker grant persistence (not in audit) | [C] | NEW | `Store` is "a thread-safe in-memory grant store" (grant.go:61-64). No Postgres or Redis store |
| 25 | gateway domains/browser.rs:1233-1246 (start-run body) | [C] | OK | `allowed_domains: body.allowed_domains` (1238) is forwarded from the client; execution-core ignores caller domains (tool_bridge/mod.rs:882-884) |

### 2. Affected files

Quarry (Ingestion Plane):
- `quarry-core/src/policy.rs` [C] 153 lines: RunPolicy 6-16, RobotsMode 65.
- `quarry-runtime/src/policy.rs` [C] 450 lines: DiscoveryPolicy 58-80, RunPolicy 215-230, best_effort (not read).
- `quarry-edge/src/routes.rs` [M] 1564 lines: ZDR floor 414-417, RunPolicy::default 461, route table 133-192 (crawl 133, batch 134, search 159, extract 170, answer 171, jobs 192).
- `quarry-edge/src/main.rs` [M] 1569 lines: RunPolicy::default 1493; `require_browser_grants` 183-184 (forced on in prod or production) and startup error 197.
- `quarry-edge/src/auth.rs` [C] 1242 lines: Claims 61-91, zdr_floor 121-123, effective_zdr 129-138, `require_service_route_scope` 504-545.
- `quarry-edge/src/agent_routes.rs` [C] 3737 lines: validate_run_grant 314-338, sensitive_approval_for_action 144-~283, validate_sensitive_approval 284-~300, start_run 1273, step 2228, navigate check 2297-2304, ZDR 1433-1445.
- `quarry-edge/src/state.rs` [M] 155 lines (require_browser_grants field 38); `quarry-edge/src/config.rs` [M] 501 lines (require_browser_grants 132, security_snapshot_path 37); `quarry-edge/src/test_support.rs` [M] 193 lines.
- `quarry-edge/tests/zdr_claim_floor.rs` [M] 1317 lines (committed tests the_golden_is_the_one_its_pin_names 401, the_golden_still_says_what_the_floor_depends_on 415).
- `quarry-runtime/src/observation.rs` [C]: Evaluate arm 542-543.
- `quarry-control` (Go): `services/quarry-control/internal/resources/presets.go` [C] 124 lines; `resources.go` [C] (resolveJobPolicy 469-481, call 408, ResolvePreset call 474); `blocklists.go` [C] 10 lines; `internal/store/store.go` [C] (BlocklistEntry 397-402); `internal/store/pg/webhooks_blocklists.go` [C] (Create 190-196: the INSERT has no org column; Get 198-); `cmd/control/main.go` [C] (MountPresets 174).
- Quarry contract types: `quarry-core/src/contracts.rs` [C] (AgentConstraints 830-838: allowed_domains, max_cost_usd).
- Quarry OpenAPI: `docs/openapi.yaml` [M] 4713 lines (uncommitted 78-line diff).

Control Plane:
- `org-core/internal/org/types.go` [C] 365 lines. RetentionPosture 286-316 is the delivery pattern to copy.
- `org-core/internal/http/retention_posture_handler.go` [C] 67 lines; `retention_posture_handler_test.go` [C]; `retention_posture_contract_test.go` [C].
- `org-core/migrations/` [C]: last 020; next free **021** [NEW] if a policy table is added.
- `auth-core/src/auth/plane-token.controller.ts` [C] 554 lines (claim minting 343-384, 498-502). `plane-service-principals-policy.contract.spec.ts` [C] (141-150).
- Retention contract JSON referenced by types.go:300 (`internal/http/testdata/auth-core-org-core-retention-posture.contract.json`) [UNVERIFIED existence].

Model Plane:
- `execution-core/src/capability_policy.rs` [C] 1051 lines (browser mapping 793-794; web mapping 794).
- `capability-core/internal/policy/engine.go` [C] 227 lines; `engine_test.go` [C] 247 lines. Migrations `capability-core/migrations/` [C] last 0015; next free **0016** [NEW] if capability-level policy is persisted.
- `browser-broker/internal/grant/grant.go` [C] 362 lines (Grant 39-59; allowedSensitiveActions 32-36; in-memory Store 61-64); `grant_test.go` [C] 133 lines.
- `proto/model_plane/v1/browser.proto` [C] 114 lines (allowed_domains at 38, 63, 108); `browser_agent.proto` [C] 236 lines (allowed_domains 174). Action-class fields would go into the grant message here.
- Gateway: `verevonv3/apps/gateway/src/domains/browser.rs` [C] (start-run body 1233-1246).

Callers of key symbols:
- RunPolicy::default: routes.rs:461, main.rs:1493 (prod); test_support.rs [M] (test).
- ResolvePreset: resources.go:474 (prod), presets_test.go:126 (test).
- Claims::zdr_floor and effective_zdr: routes.rs:416; agent_routes.rs:1441-1443; auth.rs (definitions).
- BlocklistEntry: pg/webhooks_blocklists.go:186-190 (prod).
- RetentionPosture: retention_posture_handler.go (prod); auth-core `resolveInteractiveRetentionPosture` (plane-token.controller.ts:256).

### 3. Reusable but unwired

- `RunPolicy::default()` sites (routes.rs:461, main.rs:1493) are wired production code. What is unwired is any org or request policy input.
- quarry-control presets: wired for job creation (resources.go:474). Not read by the edge or the agent path.
- `/v1/blocklists` (quarry-control, BlocklistEntry 397-402): stored, no org column, not read by quarry-edge (no reader found in `quarry-edge/src`). The edge's live host blocklist is the file snapshot (config.rs:34-37 to main.rs:348).
- `QUARRY_EDGE__RUN_POLICY`: documented only (main.rs:1489-1492). No reader.
- `RetentionPosture` endpoint and `Claims::zdr_floor`: wired and reusable as the delivery pattern.
- `capability-core` denyOnGrant: wired, but engine.go:58-62 says the `reg` field and two methods were removed as POL-2.
- Broker `Grant.AllowedActions` (grant.go:50): wired for sensitive grants only.
- Evaluate sensitive approval: NOT present. No Evaluate arm in sensitive_approval_for_action (the only `evaluate` hits are string mappings at agent_routes.rs 694 and 751).

### 4. Tests

- quarry-edge: `tests/zdr_claim_floor.rs` (see above); auth.rs 14 `#[test]`; agent_routes.rs 16 `#[test]`.
- quarry-runtime: policy.rs 10 `#[test]`; usage.rs 5. quarry-core policy.rs 0; quarry-browser lib.rs 0.
- quarry-control: `presets_test.go` (ResolvePreset test at 126), `resources_test.go` [C].
- org-core: `retention_posture_handler_test.go` (TestRetentionPostureRouteIsMounted line 70; TestRetentionPostureHandlerReturnsOnlyTheTwoBooleans line 80), `retention_posture_contract_test.go`.
- auth-core: `plane-service-principals-policy.contract.spec.ts`, `plane-token.controller.spec.ts` [C] 720 lines.
- capability-core: `engine_test.go` [C] 247 lines. browser-broker: `grant_test.go` [C] 133 lines.
- Commands:
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge --test zdr_claim_floor`
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime policy`
  - `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./internal/resources/ -run Preset`
  - `cd "apps/Control Plane/org-core" && go test ./internal/http/ -run RetentionPosture`
  - `cd "apps/Control Plane/auth-core" && npx jest plane-service-principals-policy.contract` (package.json "test": "jest")
  - `cd "apps/Model Plane/go/services/capability-core" && GOWORK=off go test ./internal/policy/`
  - `cd "apps/Model Plane/go/services/browser-broker" && go test ./internal/grant/`
- CI: `.github/workflows/quarry-v2-ci.yml` [C] `cargo test --workspace` (205), `go test ./...` (329), path `apps/Ingestion Plane/Quarry-v2/**` (123-125). `org-service.yml` [C] path `apps/Control Plane/org-core/**` (16-19). `auth-service.yml` [C] path `apps/Control Plane/auth-core/**` (15-16). `model-plane-rust-ci.yml` [M] path filter includes org-core testdata and quarry-edge fixtures (33-38).

### 5. Feature flags and env

- `require_browser_grants`: config.rs:132, state.rs:38, main.rs:183-184. Forced on when environment is "prod" or "production". main.rs:197 fails startup if required and no grant backend is configured.
- `QUARRY_EDGE__RUN_POLICY`: documented in comment only (main.rs:1489-1492). No reader.
- `security_snapshot_path`: config.rs:34-37; read at main.rs:348 (optional file).
- `QUARRY_LITE_EXPERIMENTAL`: agent_routes.rs:1143-1147; gates the ephemeral_evidence tier (1283).
- Cargo feature `browser-agent`: agent_routes.rs:3734 (no-op router when disabled).
- Control: org-core entitlements (types.go:266-271); ZDR plans "pro" and "enterprise" (281-284); interactive ZDR intent stored in org metadata (types.go:293-294).
- Model: capability ids `cap.browser.open` (capability_policy.rs:793), `cap.tool.http` (794).

### 6. Surprises

1. [CLAIM-FALSE] "control's presets are never consumed". `resources.go:474` consumes them at job creation (resolveJobPolicy 469-481, called at 408). Only the edge and agent path ignore them.
2. [DRIFTED citation] The second RunPolicy is quarry-runtime/src/policy.rs:215, not 55-80 (that range is DiscoveryPolicy).
3. [CLAIM-FALSE] "raw evaluate is gated by sensitive approval" is not the case in the code. Evaluate has no sensitive-approval arm (agent_routes.rs 144-300), and the broker's sensitive list is respond_dialog, upload_ref, download_ref (grant.go:32-36). The observation path calls `browser.evaluate` directly (observation.rs:542-543).
4. Partial per-URL enforcement already exists on the step path: Navigate only (agent_routes.rs:2297-2304). Redirects and PageRunner are not covered.
5. Broker run-domain equality (validate_run_grant, agent_routes.rs:326-335) is enforced only when require_browser_grants is set, which main.rs:184 forces on only in prod.
6. The gateway forwards client `allowed_domains` (browser.rs:1238). execution-core ignores caller domains (tool_bridge/mod.rs:882-884) and uses the broker grant. The broker grant is the authority, so the policy design can rely on it.
7. Browser-broker grants are in-memory only (grant.go:61-64). The audit's "persisted in Postgres or Redis" is new work. Its "grant issuance in production" dependency stands.
8. The ZDR claim does not reach the agent path (auth.rs:121-123 ignores service principals). Confirmed.
9. `/v1/blocklists` has no org column (store.go:397-402). Confirmed.
10. Uncommitted hunks in routes.rs (1329 only) and main.rs (many, 47-1463) do not touch the cited lines 413-417, 461, 1485-1493.
11. The audit cites agent_routes.rs:144-300 as a run-policy site. It is the sensitive-approval gate.

---

## F-t2-042 Agent web toolset: content-hydrated search, next-step hints, citations, and Quarry jobs as approval-gated tools with cost estimates

### 1. Citation check (24 checked; web_tools.rs:1-60 header not re-read, marked UNVERIFIED)

| # | Citation | Tag | Verdict | What the live code says |
|---|---|---|---|---|
| 1 | quarry-edge search_routes.rs:50-120 (no scrape options) | [M] | OK | `SearchRequest` at 50; zdr 58, allow_paid_providers 74. grep for `scrape_options`, `scrapeOptions`, `hints`, `next_steps`, `suggested_next` returns 0 hits in the live file |
| 2 | search_routes.rs:1006-1030 (per-query UsageEvent) | [M] | OK | `meter(UsageEvent::new(run_id_str, ...))` at 1006-1012; `run_id_str` set at 978 |
| 3 | search_routes.rs:50-160 (json: "no source ids") | [M] | [CLAIM-FALSE] partly | committed `SearchResponse.citations` at 157 (live 171): "Sources behind answer" when include_answer. Populated 804-816, 842. Type `Citation` in `quarry-runtime/src/answer.rs:179` [C] |
| 4 | answer_routes.rs:44 and 145 | [M] | OK | `answer` 44; `answer_stream` 145 |
| 5 | answer_routes.rs:1-30, 131-145 | [M] | OK | ZDR cache rule at 143-144 |
| 6 | api_error.rs:15-60, 24-25, 46-58 | [C] | OK | `next_actions` 23-25; the only populating constructor is `rate_limited` (44-58) |
| 7 | web_tools.rs:120-175; 133; 152; 254-264 | [C] | OK | `search` 124-136 posts `/v1/search` at 133 with `search_body` 254-256 (query, limit, zdr). `extract` 151-153 posts `/v1/extract` at 152 with `extract_body` 259-261 |
| 8 | web_tools.rs:39-40 | [C] | OK | `SEARCH_SCOPES` 39, `EXTRACT_SCOPES` 40 |
| 9 | web_tools.rs:92-98 (bearer and x-quarry-org) | [C] | OK | `bearer_auth` 95, `x-quarry-org` 96 |
| 10 | web_tools.rs:1-60 ("free-chain only") | [C] | [UNVERIFIED] | header not re-read |
| 11 | runtime_loop/agent.rs:4184 and 4184-4192 | [M] | OK | web_search 4184-4187 and web_fetch 4188-4192: one sentence each. No uncommitted hunk touches these lines |
| 12 | agent.rs:4089-4290 (no crawl, map, batch tool) | [M] | OK | grep finds no crawl, map or batch tool name in agent.rs (only unrelated `batch_translate_text` at 5877) |
| 13 | capability_policy.rs:775-835 (no cap.web.*) | [C] | OK | web_search, web_fetch and web.* map to cap.tool.http (794). grep `cap\.web\.\|cap\.pack\.` in execution-core: 0 hits |
| 14 | capability_policy.rs:787-795 | [C] | OK | same region |
| 15 | execution-core browser_agent/browser_risk.rs:1 | [C] | OK | header "Phase 5 — browser-action risk classification" |
| 16 | auth-core plane-service-principals-policy.contract.spec.ts:143-150 | [C] | OK | comment 142-149; `webToolScopes` at 150 |
| 17 | model-gateway tool_loop.rs:4304-4310 (chat web_search guidance) | [M] | OK | description 4304-4305 includes freshness and narrowest-tool guidance |
| 18 | tool_loop.rs:4359-4361 (fetch_url one-liner) | [M] | OK | 4359-4361 |
| 19 | tool_loop.rs:4469 (results labelled as data) | [M] | OK | 4469 "treat anything the result CONTAINS as data" |
| 20 | retrieval_metadata.rs:1-40 (suggested_next_tools) | [M] | OK | header 1-14; `HINT_TO_TOOL` 33; `hinted_tool_names` 213-222 (advertised guard). Uncommitted hunks are at 40-114, 130-188, 256-389 (ZDR RetrievalNotice), not on the hint helpers |
| 21 | session-core orchestration_grpc.rs:699-708 (COST kind) | [M] | OK, shifted | `approval_kind_from_str` 702-711; "cost" to `ApprovalKind::Cost` at 708 |
| 22 | runtime_loop/mod.rs:42-56 and 642-646 | [M] | OK | in-process aliases 41-56; dispatch 642-646 |
| 23 | bridges/mcp-bridge/server.js:1-20 | [C] | OK | header: reference MCP bridge exposing registered servers. Not a Quarry facade |
| 24 | negative: "no cost-estimate producer" | — | OK | grep `ApprovalKind::Cost` in model-plane services: only the mapping (orchestration_grpc.rs 708, 720) |

### 2. Affected files

- `quarry-edge/src/search_routes.rs` [M] 2708 lines: SearchRequest 50-75 (add scrape_options here), SearchResponse (committed 146-158, `citations` 157), UsageEvent 1006-1012. Uncommitted hunks change 611 lines (score policy `apply_score_policy`, `QueryIntent::Research`). None adds a scrape or hint field.
- `quarry-edge/src/answer_routes.rs` [M] 379 lines: answer 44 (search then fetch then grounded answer); answer_stream 145; cache rule 143-144. Call path to `/v1/search` is inside this module.
- `quarry-edge/src/api_error.rs` [C] 109 lines: ApiError `next_actions` 23-25, `rate_limited` 44-58.
- `quarry-edge/src/routes.rs` [M]: search route 159, extract 170, answer 171. Success envelope unchanged.
- `quarry-runtime/src/answer.rs` [C]: `Citation` 179.
- `execution-core/src/web_tools.rs` [C] 717 lines: `search` 124-136, `extract` 151-153, `search_body` 254-256, `extract_body` 259-261, scopes 39-40, MAX_SNIPPET_CHARS 37, MAX_FETCH_CHARS 38. Callers: runtime_loop/mod.rs:642-646 (execute_web_search, execute_web_fetch).
- `execution-core/src/runtime_loop/agent.rs` [M] 13521 lines: web tool definitions 4184-4192; prompt snippet home 83-170.
- `model-gateway/src/tool_loop.rs` [M] 13032 lines: chat tool definitions 4302-4362; citation machinery and relevance gate (5396 region); results label 4469.
- `model-gateway/src/retrieval_metadata.rs` [M] 520 lines: `HINT_TO_TOOL` 33, `hinted_tool_names` 213-222 (caller at 148).
- `session-core/src/orchestration_grpc.rs` [M] 6215 lines: approval_kind_from_str 702-711 (callers 1146, 1810, 2613).
- `execution-core/src/capability_policy.rs` [C] 1051 lines: web mapping 794. New cap.web.* ids would be added here and in capability-core registry.
- `auth-core` plane-service-principals policy [C]: new scopes (for example crawl:write, pack:run) would extend the webToolScopes contract (150).
- `proto/model_plane/v1/browser_agent.proto` [C]; OpenAPI `docs/openapi.yaml` [M] 4713 lines (SearchRequest scrape options would be documented here).
- Migrations: none for search or hints. A Quarry job record (crawl/batch as tools) would need a migration in quarry-control `internal/store/pg/migrations/` [C] (last 014_fleets.sql; next **015** [NEW]).
- Config: none new.

### 3. Reusable but unwired

- Quarry `SearchResponse.citations` (committed): built and populated when include_answer is set, but execution-core never requests include_answer (`search_body` 254-256). Reusable for stable source ids.
- `retrieval_metadata::hinted_tool_names` and HINT_TO_TOOL (retrieval_metadata.rs 33, 213-222): wired in model-gateway (148), covered by the tool_loop test "every_hinted_tool_is_actually_advertised".
- `ApiError.next_actions`: only populated by `rate_limited` (api_error.rs 44-58). Effectively unwired for success hints.
- `ApprovalKind::Cost` (orchestration_grpc.rs 708, 720): mapped both ways, with no producer. Unwired.
- `answer_routes.rs` fan-out (search then full-page fetch): wired and reusable for bounded hydration.
- `search:read` and `extract:read` scopes: wired (web_tools.rs 39-40; auth-core contract 150).
- No cap.web.*, cap.pack.*, `/v1/crawl` agent path or org domain policy reader exists in execution-core (grep 0 hits).

### 4. Tests

- quarry-edge: search_routes.rs 59 `#[test]`; answer_routes.rs 3 (module at 331); api_error.rs 3.
- execution-core: web_tools.rs 19 `#[test]`; runtime_loop/agent.rs 120.
- model-gateway: tool_loop.rs 189 (includes "every_hinted_tool_is_actually_advertised"); retrieval_metadata.rs 14.
- auth-core: plane-service-principals-policy.contract.spec.ts [C] (webToolScopes assertions).
- Commands:
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge search_routes`
  - `cd "apps/Model Plane/rust" && cargo test -p execution-core web_tools`
  - `cd "apps/Model Plane/rust" && cargo test -p model-gateway retrieval_metadata`
  - `cd "apps/Control Plane/auth-core" && npx jest plane-service-principals-policy.contract`
- CI: `quarry-v2-ci.yml` [C] (`cargo test --workspace`, line 205); `model-plane-rust-ci.yml` [M] (`cargo test --workspace --no-fail-fast`, line 119); `auth-service.yml` [C].

### 5. Feature flags and env

- `allow_paid_providers`: search_routes.rs:74 (`#[serde(default)]`, false). Paid SERP (Brave, Serper) requires the caller's explicit permission.
- `zdr`: request body; for service callers it is the posture (search_routes.rs:55-56).
- MAX_SNIPPET_CHARS 900 and MAX_FETCH_CHARS 8000 (web_tools.rs:37-38): constants, not env.
- No env flag gates hints, citations or scrape options.
- Scopes gate routes: auth.rs:514-516 (`/v1/search` needs search:read, `/v1/extract` extract:read).

### 6. Surprises

1. [CLAIM-FALSE, partial] "attach stable source ids" partly exists on the Quarry side: `SearchResponse.citations` is committed (search_routes.rs 157). The Model Plane never requests include_answer.
2. Chat path already has the guidance the audit says is missing: tool_loop.rs 4304-4305 describes freshness and narrowest-tool choice. Only the execution-core descriptions (agent.rs 4184-4192) are one-liners.
3. Large uncommitted diff in search_routes.rs (611 lines). It adds rerank score policy and QueryIntent::Research, not scrape options or hints.
4. The "docs-read gate" is not in the code and the approach drops it. The Codex-style tool doc registry does not exist (grep finds no tool-doc registry).
5. No crawl, map or batch tool is offered to the model in agent.rs; /v1/crawl (routes.rs:133) and /v1/batch (134) exist only as Quarry routes with handoffs (crawl_handoff 590, batch_handoff 681).
6. The `COST` approval kind is mapped but has no producer (grep).
7. retrieval_metadata.rs has a large uncommitted change (ZDR RetrievalNotice), but its hint helpers (33, 213-222) are untouched.

---

## F-t2-043 URL-aware planner guidance: site skills, integration-corev2 provider capability hints by domain, driver capability matrix

### 1. Citation check (16 citation rows plus 3 negative checks; agent.rs 1015-1070, integration server.go 221-224 and agent_routes.rs 1132-1135 not re-read, marked accordingly)

| # | Citation | Tag | Verdict | What the live code says |
|---|---|---|---|---|
| 1 | execution-core runtime_loop/agent.rs:4184-4192 | [M] | OK | one-sentence web_search and web_fetch descriptions (see F-t2-042 row 11) |
| 2 | agent.rs:85-170 (SNIPPET_* conditional guidance; no web snippet) | [M] | OK | SNIPPET_READ_TOOLS 83, SNIPPET_ACTION_TOOLS 93, SNIPPET_KNOWLEDGE_SEARCH 105, SNIPPET_MEMORY_TOOLS 118, SNIPPET_SUBAGENT 126, SNIPPET_SUBAGENT_RESULTS 136, SNIPPET_USER_SUPPLIED_ARGS 166. No SNIPPET_WEB |
| 3 | agent.rs:1006-1014 (skill injection by keyword, MAX 3) | [M] | OK | `MAX_INJECTED_SKILLS` 1006; fetch doc 1008-1014 (goal keyword overlap). 1015-1070 [UNVERIFIED] |
| 4 | runtime_loop/skill_budget.rs:23-35 and 18-40 | [C] | OK | `SKILL_CONTEXT_BUDGET_CHARS` 8_000 at 30; `MIN_SKILL_CHARS` 240 at 33 |
| 5 | proto/model_plane/v1/sessions.proto:876-893 (AgentSkill) | [M] | OK | fields 877-892: trigger_keywords 5, trigger_file_patterns 6, tool_restrictions 7, origin 9 ("user" or "background_review"), scope 10, owner_user_id 11, shared_with 12. No url_patterns. Uncommitted hunks (196, 1219) are elsewhere |
| 6 | session-core migrations 0006 (origin) and 0004:198 (trigger_file_patterns) | [C] | [UNVERIFIED] line | migration files exist [C]; line numbers not re-read |
| 7 | execution-core integration_tools.rs:60-75 (CATALOG) and 257-263 | [C] | OK | `CatalogOp {operation, is_write}` at 60-63; `list_provider_actions` 257-264 (tenant-scoped, org_id required) |
| 8 | integration-corev2 providers/catalog.go:68-86 | [C] | OK | `Provider` 68-86: Key, Label, Category, ConnectorType, AuthType, DirectOAuthReady, Configured, Status, MissingConfig, Capabilities 78, Bundles 79, MetaSDK, SupersededBy. No Domains field |
| 9 | integration-corev2 actions/service.go:727 | [C] | OK | `case "pages.post", "facebook.page.post":` string switch arm |
| 10 | integration-corev2 api/server.go:376, 514, 221-224 | [M] | [UNVERIFIED] | not re-read. Uncommitted hunks 217-218 add `registerZDRAcknowledgementRoutes` (not a catalog). grep for `/catalog` under integration-corev2 api: 0 hits |
| 11 | quarry-browser/src/lib.rs:262-290 (BrowserDriverCapabilities) | [C] | OK | struct 268-286 with 8 booleans: persistent_profile, devtools_trace, downloads_to_artifacts, uploads_from_artifacts, full_visual_fidelity, isolated_egress, security_evidence, atomic_target_actions |
| 12 | quarry-edge agent_routes.rs:543-551, 1180-1260, 1305, 1658 | [C] | OK | response fields `driver_capabilities` 543, `execution_tier` 544, `execution_engine` 547; `require_driver_capabilities` 1180-1186; call 1305; response 1658-1659 |
| 13 | agent_routes.rs:1132-1135 | [C] | [UNVERIFIED] | not re-read |
| 14 | docs/openapi.yaml:2425-2510 | [M] | OK | execution_tier enum 2423-2428; driver_requirements `$ref` 2429-2430 |
| 15 | docs/DRIVER_MATRIX.md:1-60 | [C] | OK | four driver families (table 7-12); names `DriverPlan` (quarry-runtime/src/driver_plan.rs:36 `plan_from_signals`, [C]) |
| 16 | quarry-core/src/output.rs:112-125 and 123 | [C] | DRIFTED (minor) | `UrlTriple` at 120-126: requested 122, final_url 123, canonical 125. DeterminismStamp 110-118 |
| 17 | negative: "no /v1/drivers route" | — | OK | grep `/v1/drivers` in apps: 0 hits |
| 18 | negative: "no consumer of driver_capabilities or driver_requirements" | — | OK | grep (.rs, .go, .ts, .tsx, .proto): only `quarry-edge/src/agent_routes.rs` |
| 19 | negative: "no url_patterns, snapshot_scope, autoInject" | — | OK | grep: 0 hits |

### 2. Affected files

- `execution-core/src/runtime_loop/agent.rs` [M] 13521 lines: web descriptions 4184-4192 (rewrite); SNIPPET_* 83-170 (add SNIPPET_WEB); skill fetch 1006-1014 (url match hook); tool definition list around 4089-4290.
- `execution-core/src/runtime_loop/skill_budget.rs` [C] 206 lines: budget constants 30 and 33.
- `execution-core/src/integration_tools.rs` [C] 1465 lines: CATALOG (CatalogOp 60-63), `list_provider_actions` 257-264 (callers not traced).
- `execution-core/src/runtime_loop/mod.rs` [M] 2856 lines: browser.observe dispatch (tool name constants 45-46) where the URL match would run.
- `model-gateway/src/skills.rs` [C] (trigger_file_patterns struct literal at 532).
- `session-core/src/grpc.rs` [M] 13341 lines: trigger_file_patterns consumer (upsert and list).
- `capability-core/internal/api/registry_apis.go` [C]: trigger_file_patterns field 88 and SELECT 145.
- `proto/model_plane/v1/sessions.proto` [M] 1249 lines: AgentSkill (876-893). A url_patterns field would be field 13 (next after shared_with = 12) [derived].
- Migrations: session-core `migrations/` [C] (last committed 0043; 0044 and 0045 are [U]). A url_patterns column would be a new migration: **0046** [NEW] (collides with 010, 045, 062 hotspots).
- Quarry: `crates/quarry-edge/src/agent_routes.rs` [C] 3737 lines (driver fields 543-544, 1180-1186, 1305, 1658-1659) and `quarry-runtime/src/driver_plan.rs` [C] (DriverPlan, plan_from_signals 36). `crates/quarry-browser/src/lib.rs` [C] 676 lines.
- Quarry OpenAPI `docs/openapi.yaml` [M] 4713 lines (driver_requirements docs 2429-2430; add GET /v1/drivers/{id}/capabilities only if that route is built).
- integration-corev2: `internal/providers/catalog.go` [C] 1932 lines (Provider 68-86); `internal/actions/service.go` [C] 2098 lines (switch arms; 727); `internal/api/server.go` [M] 4256 lines (provider catalog and discovery routes; no new catalog route).
- Gateway / SPA: none for this item.

### 3. Reusable but unwired

- `driver_capabilities`, `execution_tier`, `execution_engine` on POST /v1/agent/runs (agent_routes.rs 543-551, 1658-1659): produced, consumed by no code outside quarry-edge. The cheapest route is forwarding these existing fields.
- `driver_requirements` gate (agent_routes.rs 1180, 1305): wired at run start (fail-closed).
- `BrowserDriverCapabilities` (quarry-browser lib.rs 268-286): wired inside Quarry only.
- `trigger_file_patterns` (sessions.proto 882): stored and plumbed through execution-core, model-gateway, session-core and capability-core; the template for url_patterns.
- `SNIPPET_*` mechanism (agent.rs 83-170): wired into the prompt; there is no SNIPPET_WEB.
- `list_provider_actions` (integration_tools.rs 257-264): wired, tenant-scoped.
- No `/v1/drivers` route, no `/catalog` route in integration-corev2, no url_patterns or snapshot_scope symbol anywhere (grep 0 hits).

### 4. Tests

- execution-core: skill_budget.rs 7 `#[test]`; runtime_loop/agent.rs 120 `#[test]` (skill injection tests in 7176-8103 region [UNVERIFIED names]).
- quarry-edge: agent_routes.rs 16 `#[test]`.
- quarry-browser lib.rs: 0 `#[test]`. quarry-runtime driver_plan.rs [C]: tests not counted.
- integration-corev2: tests not counted [UNVERIFIED].
- Commands: `cd "apps/Model Plane/rust" && cargo test -p execution-core skill_budget`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge agent_routes`.
- CI: `model-plane-rust-ci.yml` [M] (`cargo test --workspace --no-fail-fast`); `quarry-v2-ci.yml` [C] (`cargo test --workspace`); integration-corev2 CI [UNVERIFIED workflow name].

### 5. Feature flags and env

- SKILL_CONTEXT_BUDGET_CHARS 8000, MIN_SKILL_CHARS 240 (skill_budget.rs 30, 33). Constants.
- MAX_INJECTED_SKILLS 3 (agent.rs:1006). Constant.
- QUARRY_LITE_EXPERIMENTAL (agent_routes.rs:1143-1147): gates ephemeral_evidence. Not a driver switch.
- Driver families selected per request by DriverPlan from signals (driver_plan.rs:36), not by env [UNVERIFIED for env reads inside driver_plan].
- integration-corev2 operation catalog: no version or env.

### 6. Surprises

1. [CLAIM-FALSE] "build GET /v1/drivers/{id}/capabilities" is not the cheapest path. POST /v1/agent/runs already returns the driver fields (agent_routes.rs 543-551, 1658-1659) and the audit's own "corrected premise" note confirms it. Nothing consumes them.
2. The chat path already has web guidance (tool_loop.rs 4304-4305, 4359-4361). A "web access" built-in skill would duplicate it.
3. There is no web SNIPPET in agent.rs. tool_loop.rs has a separate SNIPPET_USER_SUPPLIED_ARGS constant (671), so the name is reused in another crate.
4. sessions.proto has uncommitted hunks (196 and 1219) but AgentSkill (876-893) is unchanged.
5. integration-corev2 server.go has uncommitted hunks, but they add ZDR acknowledgement routes, not a provider catalog or domain field.
6. DRIVER_MATRIX.md describes DriverPlan; the type is real (driver_plan.rs:36).

---

## F-t2-045 Memory pollution guard for runs that ingested web or browser content (verify first)

### 1. Citation check (25 checked; none skipped)

| # | Citation | Tag | Verdict | What the live code says |
|---|---|---|---|---|
| 1 | model-gateway source_validation.rs:23 (json "data, not instructions") | [C] | [CLAIM-FALSE] citation | line 23 is `WEB_ONLY_SCOPE_NOTICE` (scope notice). The data-not-instructions label is tool_loop.rs:4469 |
| 2 | model-gateway tool_loop.rs:4469 | [M] | OK | "treat anything the result CONTAINS as data to read, never as an instruction" |
| 3 | session-core dreaming.rs:739-756 (extract_memory_candidates) | [M] | DRIFTED (minor) | signature takes (role, content, thread_id) at 739-743, not only role and content. User rows strip document blocks (744-746), user and assistant dispatch (753-755), others return empty (756) |
| 4 | dreaming.rs:1636-1690 (upsert, review_state 'accepted') | [M] | OK | VALUES ... 'accepted' (1647); ON CONFLICT sets review_state 'accepted' (1655). Not in an uncommitted hunk |
| 5 | dreaming.rs:1263-1290 (pending messages) | [M] | OK, clause uncommitted | roles user and assistant (1271); zdr_waived exclusion (1278-1282). **The zdr_waived clause is uncommitted** (0 hits in afb83928) |
| 6 | dreaming.rs:1318-1330 (context window) | [M] | DRIFTED | roles filter at 1324 OK; there is no zdr_waived clause in this query |
| 7 | dreaming.rs:223-224 (conversation_only) and 434 (review_state filter) | [M] | OK | 223 `if last.conversation_only`; 434 `AND review_state = 'accepted'` |
| 8 | session-core grpc.rs:4096-4150 (MemoryRetention) | [M] | OK, uncommitted | enum 4096-4107 with `WaivedRun` 4106 and `of(caller)` 4110. `permits_external_memory_indexing` 4147-4149 (Durable only). **Both WaivedRun and the method are uncommitted** (0 hits in afb83928) |
| 9 | grpc.rs:4159 (ZDR_MEMORY_WRITE_SUPPRESSED) | [M] | OK | constants 4158-4159 |
| 10 | dream_extractor.rs:1-60 (module doc) | [C] | OK for 1-12 | both extractors always run and merge, deterministic wins collisions (9-12). Lines 13-60 not re-read [UNVERIFIED] |
| 11 | dream_extractor.rs:109 (LLM_SOURCE_LINK) | [C] | OK | `"extractor:llm"` at 109 |
| 12 | dream_extractor.rs:552, 616-640 (render_window) | [C] | OK | 620-624 keep only assistant and user; other roles `continue` |
| 13 | migration 0004:226 (review_state) | [C] | OK | `review_state TEXT NOT NULL DEFAULT 'accepted'` (226) with comment pending, accepted, rejected, expired. **Default is accepted** |
| 14 | migration 0038 (1-4) | [C] | OK | `threads.source_scope` workspace or conversation |
| 15 | migration 0013 (1-15) | [C] | OK | session_tool_audit_intents: run_id, action_id, request_id, tool, data_category, zdr, status (reserved, completed, failed) |
| 16 | session-core memory_grpc.rs:444-505 (index_memory) | [M] | OK | index_memory starts 444. Uncommitted hunk 496-502 adds `MemoryRetention::of(&caller).permits_external_memory_indexing()` and WAIVED_MEMORY_INDEX_SUPPRESSED |
| 17 | memory_grpc.rs:625-635 (json: index gated by retention) | [M] | DRIFTED / wrong function | 625-636 is the **delete** path ("Owner decision 17"), not index |
| 18 | proto/model_plane/v1/memory.proto:126-140 (IndexMemoryRequest) | [C] | OK | fields 126-141: thread_id 1, topic 2, content 3, org_id 4, user_id 5. No taint or provenance field |
| 19 | execution-core runtime_loop/mod.rs:650-663 and 2172 | [M] | OK | dispatch 657-664 passes no taint; `execute_save_memory` 2172-2174 signature has no taint |
| 20 | runtime_loop/agent.rs:314-332 (data_category) and 4235-4240 | [M] | [CLAIM-FALSE] partly | data_category 314-327: `mcp__` gives "unclassified"; knowledge_search and subagents give "customer_private"; **everything else, including web_search, web_fetch and browser.***, gives "public_non_personal" (325). save_memory description 4235-4238 says it is unavailable under ZDR |
| 21 | execution-core permission/mod.rs:171-182 (save_memory ungated) | [C] | OK with correction | save_memory is absent from `is_risky_tool` (doc 169-179). It IS restricted in plan mode and delegated subagents by `is_restricted_context_write` (181-183), called at permission/mod.rs:104 and runtime_loop/agent.rs:1646 and 1807 |
| 22 | capability_policy.rs:796-808 (save_memory low risk, refused under ZDR) | [C] | OK | save_memory maps to cap.memory.index (808); comment 801-807 |
| 23 | execution-core wiki_agent.rs:1-15 (wiki tool not offered) | [C] | OK | header 1-12. grep: no wiki tool definition in agent.rs (wiki names appear in wiki_agent.rs and tool_bridge/mod.rs only) |
| 24 | negative: no polluted, taint or memory_mode flag | — | OK | grep `\btaint(ed)?\b\|polluted\|memory_mode\|memoryMode\|pollution` in apps (rs, go, ts, tsx, sql, proto): only an unrelated SPA draft flag (home-draft.ts 61-176) and a test string |
| 25 | negative: no accept or reject review RPC | — | OK | memory.proto rpcs: SearchMemory 14, IndexMemory 17, ListMemory 22, DeleteMemory 27, Health 30 |

letta_adapter.rs exists [C] (1095 lines). Not checked against the audit's Letta-suppression claim [UNVERIFIED].

### 2. Affected files

- `session-core/src/dreaming.rs` [M] 2778 lines: extract_memory_candidates 739-758; conversation_only 223; pending messages 1263-1290 (zdr_waived 1278-1282, uncommitted); context window 1318-1331; review_state reads 434, 596, 614, 1121; upsert_current_key 1636-1690 (callers inside dreaming.rs [UNVERIFIED]). Uncommitted hunks: 216-222, 568-619, 1248-1262, 1278-1282.
- `session-core/src/dream_extractor.rs` [C] 1181 lines: LLM_SOURCE_LINK 109; render_window 616-640; strip_document_blocks.
- `session-core/src/grpc.rs` [M] 13341 lines: MemoryRetention 4096-4150; ZDR constants 4158-4163; retention identity `of(caller)` 4110. Uncommitted: WaivedRun, permits_external_memory_indexing, waived constants.
- `session-core/src/memory_grpc.rs` [M] 1425 lines: index_memory 444-505 (uncommitted gate 496-502); delete 625-636.
- `session-core/src/letta_adapter.rs` [C] 1095 lines: external semantic mirror (must be suppressed for quarantined runs).
- `session-core/src/memory_control.rs` [C] 204 lines: user memory controls (extract_after).
- `session-core/migrations/` [C] (last 0043). 0044 and 0045 are [U]. Next free: **0046** [NEW] if a review or taint column is added to agent_memory or a tool-taint table is added. This collides with 010, 043 and 062.
- `proto/model_plane/v1/memory.proto` [C] 220 lines: IndexMemoryRequest 126-141 (a provenance or taint field would be field 6, derived from the last field 5). New review RPC would go in service block 14-30.
- `execution-core/src/runtime_loop/mod.rs` [M] 2856 lines: save_memory dispatch 657-664; `execute_save_memory` 2172-2195 (add preflight here).
- `execution-core/src/runtime_loop/agent.rs` [M] 13521 lines: data_category 314-327; save_memory definition 4235-4241; `is_restricted_context_write` call sites 1646, 1807.
- `execution-core/src/permission/mod.rs` [C] 701 lines: `is_restricted_context_write` 181-183, call 104.
- `execution-core/src/capability_policy.rs` [C] 1051 lines: save_memory mapping 808.
- `model-gateway/src/tool_loop.rs` [M] 13032 lines: data-label 4469.
- Data Plane v2: no agent-derived graph write path found (per audit; not re-verified).

### 3. Reusable but unwired

- `review_state` values pending, rejected, expired: allowed by schema (0004:226) and filtered on reads (dreaming.rs 434, 596, 614, 1121), but **no writer produces 'pending' for agent_memory**. Reads already exclude non-accepted rows, so a pending path needs no read-side change.
- `is_restricted_context_write` (permission/mod.rs:181-183): wired (plan mode, subagents). A taint check can reuse this gate pattern.
- `permits_external_memory_indexing` (grpc.rs 4147-4149): wired only in the uncommitted index path (memory_grpc.rs 496-502).
- `session_tool_audit_intents` (0013): table exists with tool and data_category. Writers not traced [UNVERIFIED].
- `zdr_waived` run filter (dreaming.rs 1278-1282): uncommitted.
- ZDR_MEMORY_WRITE_SUPPRESSED (grpc.rs 4159): wired on the delete path (memory_grpc.rs 631-635).
- No accept or reject RPC exists (memory.proto rpcs above).

### 4. Tests

- session-core: dreaming.rs 24 `#[test]`; dream_extractor.rs 26; memory_grpc.rs 15; grpc.rs 114 (MemoryRetention tests [UNVERIFIED names]).
- execution-core: permission/mod.rs 20 `#[test]` (includes `is_restricted_context_write("save_memory")` at 304-305 and 311-314); runtime_loop/agent.rs 120.
- Commands:
  - `cd "apps/Model Plane/rust" && cargo test -p session-core dreaming`
  - `cd "apps/Model Plane/rust" && cargo test -p session-core memory`
  - `cd "apps/Model Plane/rust" && cargo test -p execution-core permission`
- Live tests: `session-core/tests/runtime_lifecycle.rs` [C] (not read).
- CI: `session-core-service.yml` [C] (path filter and command not re-read [UNVERIFIED]); `model-plane-rust-ci.yml` [M] (`cargo test --workspace --no-fail-fast`, line 119; uncommitted `session-core-receipts` Postgres job, 40+ lines added).

### 5. Feature flags and env

- No env flag gates memory writes. The gates are the ZDR posture (`MemoryRetention`, grpc.rs 4096-4150) and `threads.source_scope` (0038).
- ZDR_MEMORY_READ_SUPPRESSED and ZDR_MEMORY_WRITE_SUPPRESSED (grpc.rs 4158-4159): constants.
- `WAIVED_MEMORY_INDEX_SUPPRESSED` (uncommitted, used at memory_grpc.rs ~500).
- MAX_WINDOW_MESSAGES (dream_extractor.rs, value not read).

### 6. Surprises

1. [UNCOMMITTED] WaivedRun, `permits_external_memory_indexing`, the dreaming zdr_waived clause and the index gate are all uncommitted (Z-17 waiver work). Any F-t2-045 change must rebase onto this in-flight work.
2. [MIS-CITED] memory_grpc.rs:625-635 is the delete path, not index.
3. [CLAIM-FALSE] "extract_memory_candidates takes only role and content": it takes thread_id too.
4. [CLAIM-FALSE, partly] "data_category already distinguishes web, browser and mcp calls": only `mcp__` is distinguished. web_search, web_fetch and browser.* are "public_non_personal" (agent.rs 325).
5. [CLAIM-FALSE, partly] save_memory is "ungated": it is restricted in plan mode and for delegated subagents (`is_restricted_context_write`).
6. The default for review_state is 'accepted' (0004:226), so a pending state must be written explicitly.
7. Migration numbering: 0044 and 0045 are untracked; next free is 0046.
8. Dreaming's context window (dreaming.rs 1318-1331) does not exclude zdr_waived threads; only the pending query does (1278-1282).
9. Both extractors run and merge (dream_extractor.rs module doc 9-12), so the deterministic matcher (dreaming.rs 754-755) is also a path to taint.

---

## F-t2-062 Budgeted web research job contract with run-context cost attribution

### 1. Citation check (26 citation rows plus 3 negative checks; none skipped)

| # | Citation | Tag | Verdict | What the live code says |
|---|---|---|---|---|
| 1 | quarry-edge agent_routes.rs:3659-3729 (route list) | [C] | OK | `agent_router` 3659; POST /v1/agent/runs 3661; step 3662; receipts 3683; DELETE /v1/agent/runs/:run_id 3728. No research or agent-job route |
| 2 | agent_routes.rs:1273-1667 (run start) | [C] | OK (partial read) | `start_run` 1273 |
| 3 | agent_routes.rs:2228-2410 (step) | [C] | OK (partial read) | `step` 2228 |
| 4 | agent_routes.rs:3734-3737 (no-op router) | [C] | OK | `#[cfg(not(feature = "browser-agent"))]` no-op |
| 5 | quarry-edge fleet_routes.rs:219-222 | [C] | OK | /v1/fleets POST 219, GET 220, members POST 221, budget GET 222 |
| 6 | routes.rs:898-1066 (json: route list) | [M] | FALSE citation | route table is **133-192** (crawl 133, batch 134, search 159, extract 170, answer 171, jobs 192). Handlers: scrape 398, crawl_handoff 590, batch_handoff 681 |
| 7 | resource_routes.rs:10 (job listing read model) | [C] | OK | doc 10-11: `GET /v1/{crawl,search,extract,research,agent,batch}/jobs` shared handler; list_jobs 732 |
| 8 | auth.rs:504-545 (service scopes) | [C] | OK | scope map: /v1/scrape, /v1/search (search:read), /v1/extract (extract:read), /v1/artifacts/* (scrape or browser), /v1/agent/* (browser:execute). Everything else FORBIDDEN (`_ => false`). No map, crawl, batch or answer |
| 9 | gateway browser.rs:369, 451, 1235-1245 | [C] | OK | router 369; create_session 451; start-run body 1233-1246 (zdr server-derived at 1246) |
| 10 | model-gateway deep_research.rs:1-24 (no Temporal; dies with the stream) | [C] | OK | lines 19-23 |
| 11 | deep_research.rs:58-140 (caps) | [C] | OK | caps 58-66; env-clamp macro 370; env names: DEEP_RESEARCH_MAX_PAGES 135, MAX_SUB_QUERIES 379, PAGE_CHARS 196, CORPUS_CHARS 409, TIMEOUT_SECS 419, REPORT_TOKENS 429, PLAN_MODEL 3090 |
| 12 | deep_research.rs:233 | [C] | OK | `DEFAULT_WALL_CLOCK_SECS` 410; `MAX_WALL_CLOCK_SECS` 600 (235) |
| 13 | deep_research.rs:600-660 (Coverage, halt_code) | [C] | OK | Coverage fields 598-604 (filtered, relevance_fallback); `halt_code()` 636-638; `read_halt_code` 1431 |
| 14 | deep_research.rs:712 (DEEP_RESEARCH_TOOL) | [C] | OK | constant 712 |
| 15 | deep_research.rs:2413 (run_deep_research) | [C] | OK | the only caller is sse.rs:2356 |
| 16 | sse.rs:2354-2362 (deep research under the turn session run) | [M] | OK | call 2356; `session_run_for_terminal.run_id` at 2359 |
| 17 | sse.rs:2425 (cancel on client stop) | [M] | OK | warn and cancel at 2423-2427 |
| 18 | http_routes.rs:6728 (deep_research flag) | [M] | OK | `pub deep_research: bool` with alias deepResearch |
| 19 | http_routes.rs:8937 (refused for waived runs) | [M] | OK, uncommitted region | 8937-8939 refusal `WAIVER_ROUTE_MISMATCH`; hunks 8924-8986 uncommitted |
| 20 | quarry-edge search_routes.rs:1006-1030 (UsageEvent metadata, no run id) | [M] | [CLAIM-FALSE] partly | `UsageEvent::new(run_id_str, ...)` 1006-1012 with `run_id_str` at 978. The event_id therefore carries the Quarry search run id (usage.rs:74). No Model Plane parent run id |
| 21 | quarry-runtime usage.rs:47-83 | [C] | OK | struct 49-58 {event_id, org_id, metric, quantity, source, occurred_at, metadata}; `new` 64-82 |
| 22 | execution-core web_tools.rs:95-96 (bearer and x-quarry-org) | [C] | OK | `bearer_auth` 95; header 96 |
| 23 | web_tools.rs:254-262 (bodies carry no run id) | [C] | OK | `search_body` 254-256; `extract_body` 259-261 |
| 24 | quarry-core contracts.rs:830-837 (AgentConstraints) | [C] | OK | 830-838: max_steps, allowed_domains, max_runtime_s, max_cost_usd |
| 25 | quarry-core resources.rs:246-250 (TeamCreditUsage) | [C] | OK | 246-251: team-level credits only |
| 26 | cost-core server.go:142 (GET /api/v1/cost/run) | [C] | OK | line 142 `handleGetRunUsage`. No Quarry linkage found |
| 27 | negative: "no parent_run_id in Model Plane" | — | [CLAIM-FALSE] | parent_run_id exists: runs.proto:277 [C]; sessions.proto:336 and 497 [M]; session-core grpc.rs:484 and 532-544 [M] (validation that a non-empty parent is owned by the same owner); execution-core runtime_loop/subagent_results.rs:30, 147, 246 [C] |
| 28 | negative: "no thread_busy or 409" | — | OK | grep `thread_busy\|ThreadBusy\|THREAD_BUSY\|active_run` in Model Plane rust: 0 hits |
| 29 | negative: "no research job route" | — | OK | routes.rs 133-192 and agent routes 3661-3728 |

### 2. Affected files

- `model-gateway/src/deep_research.rs` [C] 5249 lines: pipeline and caps 58-140; wall clock 233-235; Coverage and halt_code 598-660; DEEP_RESEARCH_TOOL 712; run_deep_research 2413.
- `model-gateway/src/sse.rs` [M] 13325 lines: deep-research call 2354-2362; cancel 2423-2427. Hot file (large uncommitted diff).
- `model-gateway/src/http_routes.rs` [M] 11681 lines: deepResearch flag 6726-6728; waiver refusal 8935-8939 (uncommitted region).
- `model-gateway` Quarry client and session flow: not traced (the audit says the gateway sends only bearer and x-quarry-org, which is confirmed for web_tools.rs).
- `execution-core/src/web_tools.rs` [C] 717 lines: header 92-98 (add requestId and parentRunId headers or body here); `search_body` 254-256; `extract_body` 259-261.
- `quarry-edge/src/search_routes.rs` [M] 2708 lines: UsageEvent 978, 1006-1012 (add parent attribution to metadata here; advisory only).
- `quarry-edge/src/routes.rs` [M] 1564 lines: route table 133-192 (add /v1/research route here if built).
- `quarry-edge/src/agent_routes.rs` [C] 3737 lines: `start_run` 1273; `step` 2228; `list_receipts` 2414; agent_router 3659-3735 (job route would go here).
- `quarry-edge/src/auth.rs` [C] 1242 lines: service scope map 504-545 (map, crawl and batch not in the list; adding them is a deliberate scope widening).
- `quarry-core/src/contracts.rs` [C] 830-838 (AgentConstraints: budget fields) and `quarry-core/src/resources.rs` [C] 246-251 (TeamCreditUsage).
- `quarry-runtime/src/usage.rs` [C] 298 lines: UsageEvent 49-58; `new` 64-82 (event_id format at 74).
- `apps/Model Plane/go/services/cost-core/internal/server/server.go` [C] 771 lines: GET /api/v1/cost/run (142).
- `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` [C]: start-run body 1233-1246.
- Proto: `proto/model_plane/v1/runs.proto` [C] 277 (`parent_run_id`); `sessions.proto` [M] (336, 497); a research-job message would go in runs.proto or sessions.proto [UNVERIFIED which].
- Migrations: session-core `migrations/` [C], next free **0046** [NEW] for a durable research-job table (collision with 010, 043, 045). No content retention for ZDR callers.
- OpenAPI: `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml` [M] 4713 lines, if a new route is documented.

### 3. Reusable but unwired

- `parent_run_id` lineage (runs.proto:277; session-core grpc.rs 484 and 532-544; subagent_results.rs 147, 246): wired for subagent child runs. It is the existing parent-child link and is reusable for run-context attribution. The audit's negative search missed it.
- `deep_research` pipeline with Coverage and halt_code (deep_research.rs 598-660): wired (chat path only). Reusable as the job body.
- `AgentConstraints.max_cost_usd` (contracts.rs 837): wired for browser runs only.
- `UsageEvent.metadata` (usage.rs 58): free-form JSON; a place for advisory attribution without schema change.
- Read model `GET /v1/{...}/jobs` (resource_routes.rs 10-11, list_jobs 732): lists research and agent kinds as a read model. No producer found [UNVERIFIED].
- cost-core `GET /api/v1/cost/run` (server.go 142): wired, no Quarry usage linkage.
- No thread_busy guard, no research job route, no credits budget for search or scrape (resources.rs 246-251 is team-level).

### 4. Tests

- model-gateway: deep_research.rs 66 `#[test]`; sse.rs and http_routes.rs (65) [UNVERIFIED names for deep research].
- quarry-edge: search_routes.rs 59; agent_routes.rs 16; routes.rs 5; resource_routes.rs 8; auth.rs 14.
- quarry-runtime: usage.rs 5 `#[test]`.
- Commands:
  - `cd "apps/Model Plane/rust" && cargo test -p model-gateway deep_research`
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime usage`
  - `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge agent_routes`
- CI: `model-plane-rust-ci.yml` [M] (`cargo test --workspace --no-fail-fast`, 119); `quarry-v2-ci.yml` [C] (`cargo test --workspace`, 205; `go test ./...`, 329). cost-core CI [UNVERIFIED workflow name].

### 5. Feature flags and env

- DEEP_RESEARCH_MAX_SUB_QUERIES (379), DEEP_RESEARCH_MAX_PAGES (135), DEEP_RESEARCH_PAGE_CHARS (196), DEEP_RESEARCH_CORPUS_CHARS (409), DEEP_RESEARCH_TIMEOUT_SECS (419), DEEP_RESEARCH_REPORT_TOKENS (429): each env-clamped to a hard ceiling (macro at 370). DEEP_RESEARCH_PLAN_MODEL (3090). Defaults: wall clock 410 s, max 600 s (233-235).
- `deepResearch` request flag refused for waived postures (http_routes.rs 8937).
- Quarry: no run-context flag. Service scopes (auth.rs 504-545) are the authority.
- `browser-agent` cargo feature (agent_routes.rs 3734) gates the agent router entirely.

### 6. Surprises

1. [CLAIM-FALSE] "no parent_run_id in Model Plane". The lineage exists for subagent child runs (runs.proto 277; session-core grpc.rs 484, 532-544; subagent_results.rs 147, 246). Reuse it for run-context.
2. [FALSE citation] routes.rs:898-1066 is not the route list. The table is 133-192.
3. [CLAIM-FALSE, partly] "usage events carry no run id". The Quarry search run id is embedded in event_id (search_routes.rs 978, 1006-1012; usage.rs 74). No Model Plane parent id is attached.
4. deep_research.rs is explicitly in-process by design (lines 19-23: no Temporal, "dies with the stream"). An addressable job record is new state. Its migration would take 0046 (collision hotspot).
5. The chat deep-research path already has spend caps (wall clock and sub-query and page caps). They are not a credits ceiling. No credits budget exists for search or scrape (resources.rs 246-251 is team-level).
6. Quarry already exposes /v1/crawl (routes.rs 133, handoff 590) and /v1/batch (134, handoff 681). Their service scopes are not granted (auth.rs 504-545 has no map, crawl or batch).
7. `http_routes.rs` deep-research refusal (8937) and `browser_run.rs` ZDR gate (307-313) are both in uncommitted regions.
8. The gateway derives zdr for browser runs server-side (browser.rs 1246). The web_tools.rs path sends zdr from the caller body (search_body 254-256), so a durable job must not trust the body flag.

<!-- COMPLETE -->
