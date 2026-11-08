# WS21-agent-browser: code-map sweep (F-t3-008)

- Workstream: WS21-agent-browser (Agent browser on real broker grants). Audit item: F-t3-008, split into (a) grant + Do tool (L) and (b) site governance (L).
- Shared with session (b) Quarry/browser: F-t2-001, F-t2-010, F-t2-012, F-t2-013, F-t2-014, F-t2-022, F-t2-024.
- Date: 2026-10-15. Method: static and read-only (Read, Grep, Glob; `git --no-optional-locks` diff, status and log on the live tree). No tests or builds were run.
- Trees: W = C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b (HEAD afb83928, clean). L = C:/dev/CoresSystem (same HEAD plus the ~324 entries in `.plan-work/live-tree-status.txt`).
- Conventions: paths are repo-relative (`apps/...`). `w:` = line in W, `l:` = line in L. "clean" = not listed in live-tree-status.txt. "M" = modified in L (` M`). "??" = exists only in L. "not read" = matched by Grep, line not opened.
- Audit: `C:/dev/verevon-audit-2026-10-14/03-model-plane-chat-and-agents-audit.md` (card L453-467; W2 row L1558; Appx B L1826; Appx C L1981-1989; Appx D L2123; OD13 L100, L1725). Shared cards: `02-quarry-and-browser-audit.md` (L317, 475, 507, 523, 541, 669, 701).

## 1. Summary

- Card claim confirmed. The Do loop advertises `browser.observe` and `browser.act`, but no broker-validated grant reaches dispatch, so every Do-loop browser call fails. Chain (w): `runtime_loop/agent.rs:1919` calls `runtime_loop/mod.rs:400` `execute_step_with_subagent`, which passes `None` for `browser_grant` (w:447, l:459). `mod.rs:593` then calls `tool_bridge/mod.rs:237` `execute_quarry_browser_mcp`, which returns "browser MCP requires a broker-validated grant" (`tool_bridge/mod.rs:247-249`, clean). The only production constructor of `ValidatedBrowserGrant` found is `from_broker_response`, called at `grpc.rs:214` (the ExecuteStep gRPC path, w:184-215 and w:886-948).
- No runtime caller mints grants. `BrowserBroker.AcquireGrant` has no production caller in `apps/Model Plane` (rust or go) or in the Frontend gateway. The only model-side reference is the test double at `browser_run.rs:818` (w); the broker's own tests call its server. The BFF still sends `grant_id = "session:{id}"` (`apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:1235`, clean). The BFF comment at `browser.rs:1258-1262` says real issuance is "still unbuilt".
- The broker grant store is an in-memory map (`apps/Model Plane/go/services/browser-broker/internal/grant/grant.go:62-65`, clean). No browser-grant table exists in the Model Plane migration directories checked.
- Quarry is the strongest enforcer today. With `require_browser_grants` (`quarry-edge/src/state.rs:38`, l), the agent endpoints refuse missing or inactive grants (`agent_routes.rs:323-333, 1377-1397, 2261-2265, 3096-3099, 3283-3287`, clean).
- Site governance is not implemented. There is no per-site table, no Control-owned allow or block list, and no Space narrowing for browser domains. Control Plane has only the `browser-broker` audience (`auth-core/src/auth/plane-token-scopes.ts:32,58`) and a `browser:execute` scope (`config/plane-service-principals.json:154,171`). `CONTROL_PLANE_OWNERSHIP.md` has no browser text.
- ZDR: browser runs are refused when ZDR is effective (`model-gateway/src/browser_run.rs:307` calls `browser_run_zdr_blocked` at w:627-628; l:631 now returns `Result<bool, ApiError>`). Tests at w:1287 and w:1318.
- Plan rows: E1, E2, E4 at `apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:121-123`; E9, E10 at :186-187; E12 at :267. OD13 (plan :138; audit :1725) is still open: broker, Control or Quarry owns the domain fence and per-site consent.

## 2. Citation verification (card F-t3-008)

| card id | cited path:lines | claim | worktree verdict | live status | corrected path:lines |
|---|---|---|---|---|---|
| F-t3-008 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs:1195-1275 | BFF sends grant_id "session:{id}" | MATCH: doc 1195; grant_id 1235; start_ai_run 1263-1279 | clean | same |
| F-t3-008 | apps/Model Plane/rust/services/model-gateway/src/browser_run.rs:138-200 | gateway validates grant with broker, fails closed | MATCH: resolve_browser_grant 138-202; validate_grant call 149-158; fail-closed 160-163 | M (first hunk at 307; 138-202 unchanged) | same: 138-202 |
| F-t3-008 | apps/Model Plane/rust/services/model-gateway/src/browser_run.rs:808-826 | AcquireGrant only in a test double | MATCH: TestBrowserBroker 810-814; acquire_grant stub 818-823 | M (+5 shift) | l:813-831 (acquire_grant l:823) |
| F-t3-008 | apps/Model Plane/go/services/browser-broker/internal/grant/grant.go:62-69 | broker grant store is an in-memory map | MATCH: Store 62-65; NewStore 68-70 | clean | same |
| F-t3-008 | execution-core/src/runtime_loop/agent.rs:4204-4212 (path shortened) | Do loop advertises browser.observe and browser.act | DRIFT: observe def w:4196-4200; act def w:4201-4205; helper `browser_act_parameters_json` w:3472 | M (+7) | l:4204-4212 MATCHES the card (observe name l:4204; act block ends l:4212). Full path: apps/Model Plane/rust/services/execution-core/src/runtime_loop/agent.rs |
| F-t3-008 | execution-core/src/runtime_loop/mod.rs:395-460 (path shortened) | dispatches without a grant | MATCH: doc 390-399; `execute_step_with_subagent` 400-452; passes `None` at 447 | M (fn +8; None +12) | l:398-460 (None at l:459). Full path: apps/Model Plane/rust/services/execution-core/src/runtime_loop/mod.rs |
| F-t3-008 | model-gateway/src/tool_loop.rs:630-646 | inline chat refuses browser_agent | ABSENT at w:630-646. That range is the SNIPPET_USER_SUPPLIED_ARGS doc (w:620-655). Real gate: `inline_tool_allowed` w:615-619, browser_agent denied at w:616; used at w:2489 and by sse.rs:1875 | M (+24 at 345) | l:639-646 MATCHES the card (inline_tool_allowed l:639; browser_agent l:640). Dispatch gate l:2515; sse.rs filter l:1985. Full path: apps/Model Plane/rust/services/model-gateway/src/tool_loop.rs |

Other card claims (no path:lines in the card, checked by Grep or Read):

| claim (card text) | evidence (w unless marked l) | verdict |
|---|---|---|
| browser.act accepts only snapshot, generation and ref | agent.rs:4201-4205 description; `browser_act_parameters_json` w:3472 (l:3479). Dialog, upload and download variants add `approval_grant_id` (w:3490-3494) | MATCH |
| Reuse AcquireGrant and ValidateGrant | broker `server.go:42-107` (AcquireGrant), `:128-155` (ValidateGrant); `authz.go:50-62` | MATCH (exists, no production caller) |
| Reuse resolve_browser_grant | browser_run.rs:138-202 (w and l) | MATCH |
| Reuse ValidatedBrowserGrant "with its risk classes" | `tool_bridge/mod.rs:36-39` holds only `grant_id` and `allowed_domains`. Risk classes are `RiskCategory` in `browser_agent/browser_risk.rs:25` (PersistentCookieUse 56, 74; classify_action_risk 251; classify_deterministic_risk 259) | PARTIAL: wording is loose; cite browser_risk.rs for classes |
| persistent_cookie_use HITL gate | browser_agent.rs:820 (call), :856 (`gate_persistent_cookie_use`), :864 (skips when zdr); tests :1528-1577 | MATCH |
| Quarry navigation, SSRF and network guards | quarry-browser/src/navigation.rs; quarry-browser/tests/ssrf.rs; quarry-runtime/src/dns_guard.rs; quarry-runtime/src/browser_egress_proxy.rs; quarry-runtime/tests/browser_egress_boundary.rs | MATCH (files exist; lines not checked) |
| Browser runs already refused under ZDR | browser_run.rs:307 (call), :627-628 (w; l:631 returns Result); tests :1287, :1318 | MATCH |
| Grants vanish on broker restart | grant.go:62-65 (in-memory map) | MATCH |
| Persistent profile gating | quarry-edge agent_routes.rs:1455-1477 (`persist_profile`; `zdr_forbids_persistent_profile` at 1470) | MATCH (not traced to broker) |
| BrowserBroker bearer under the delegated bearer | `apps/Frontend Plane/verevonv3/apps/gateway/src/audience_tokens.rs:27, 42` (`ModelServiceAudience::BrowserBroker` = "browser-broker"); `:119` get_model_service_token; `:128` require_model_service_token | MATCH (token exists; no grant issuance) |
| Control-owned admin allow and block lists | nothing under apps/Control Plane (grep: browser, domain, allow or block terms) | ABSENT |
| Per-user site table (control, read only, no access) | nothing in code. Audit marks it INF synthesis (Appx C correction 3, L1981-1989) | ABSENT |
| Model-blind logins: persistent profiles plus takeover | profiles at quarry-edge agent_routes.rs:458-466; takeover UI: ChatLiveRunPanel.tsx (clean; audit App D L2128 says the live panel already exists) | PARTIAL |
| Vault with handle substitution | nothing in code (F-t2-024 is Absent, W3) | ABSENT (later item, per card) |
| Interim grant_id is session-scoped and issuance unbuilt | gateway browser.rs:1258-1262 comment | MATCH |

## 3. Audit references (card-adjacent)

- W2 row L1558: XL (was L); deps F-t3-006, OD13, plan E1/E2/E4. Appx B L1826: Priority, partial, XL, Model/Ingestion/Control, W2, reference Antigravity.
- Appx C (L1981-1989) corrections: Comet levels are set by Enterprise admins (no MDM); denylist fail-closed is Antigravity DOC only, and Claude's PreviewBlocklist fails open; the six-step pipeline and per-(org,user) domain table are a Verevon synthesis (INF) to label as recommended design; Antigravity's read/act split is real in source but no `read_url` permission was found; Claude per-site batching is not shown.
- Appx D L2123: L to XL. Sums E2 M + E4 M + E9 M + E10 L + E12 L plus net-new governance and vault work; split into grant + Do tool (L) and site governance (L).
- Ordering: L466 says it depends on F-t3-006 for per-site consent cards. L1699: the computer track is strictly ordered, F-t3-008 before F-t3-026. L1714: F-t3-033 should land before or with F-t3-008.

## 4. Shared items (session b) and ownership reconciliation

| id | audit row (tier, status, effort, plane, wave) | covers | overlap with F-t3-008 | proposed split |
|---|---|---|---|---|
| F-t2-001 | Priority, Partial, L; Model Plane BrowserBroker + Frontend gateway; Quarry validates only; W1 (02 audit L321) | durable Postgres grant store; issuance path the gateway can reach; gateway forwards `x-browser-authorization`; prod-mode contract test | plan E1/E2 grant half; F-t3-008(a) depends on it | Session (b): broker persistence, issuance endpoint, BFF forwarding, contract test. WS21: model-side per-run derivation and Do-loop dispatch. One owner per AcquireGrant call site. |
| F-t2-010 | Priority, Partial, M, Model Plane, W2 (L479) | LLM browser planner on by default, budgets, connector-first routing | plan E4; code: execution-core llm_planner.rs; model-gateway `/v1/browser/suggest-action` (http_routes.rs:223) | Not determined which session owns it. Proposed: WS21 (Do browser subagent needs it); session (b) to confirm. |
| F-t2-012 | Priority, Partial, L, Frontend Plane, W2 (L511) | dockable browser panel bound to the chat run | plan E9 ("Open in browser", /browse) | Frontend. Proposed: session (b) owns the panel; WS21 supplies grant and consent cards only. |
| F-t2-013 | Priority, Partial, L; Ingestion + Model + Frontend; W2, phase 1 in W1 (L527) | co-browsing; takeover enforced inside Quarry | plan E10; F-t3-026 | Proposed: session (b) owns Quarry-side takeover enforcement; WS21 owns the model-blind login hand-off (persistent profile plus takeover request). |
| F-t2-014 | Priority, Partial, L, Medium; Model Plane trace contract, Ingestion and Frontend halves; W2 (L545) | one canonical run trace: typed events, receipts, live view, correlation envelope | browser_* run events exist (sse.rs w:9859-9866); F-t3-050 replay | Not determined. WS21 emits browser events on the shared envelope; trace owner decision needed. |
| F-t2-022 | Priority, Partial, L; Control policy of record + Model grants + Ingestion enforcement; W2 (L673) | org-pinned, tighten-only web policy delivered as signed claims, enforced by Quarry | site governance (Control admin lists, Space narrowing); F-t3-055 | Proposed: session (b) owns the signed-claim pipeline and Quarry enforcement. WS21 site governance consumes it and adds no second Control policy store. OD13 decides the fence owner. |
| F-t2-024 | Priority, Absent, L, Medium; Ingestion + Model approval; W3 (L705) | credential handoff; executor-side fill_secret; email OTP via integration-corev2 | card: "vault with handle substitution in Quarry is a separate later item" | Proposed: session (b) owns fill_secret and the vault (W3). WS21 owns persistent profiles and takeover for logins (W2). |

Reconciliation proposal (for the merge step):
- Grant path: F-t2-001 (session b, W1) is an upstream blocker. WS21 grant + Do tool (F-t3-008a) starts after it. The model side derives a narrower per-run grant and passes it to execution-core; it never reads the broker store directly.
- Policy path: F-t2-022 (signed claims, session b) is the substrate. WS21 site governance (F-t3-008b) adds the per-user table and the Space narrowing only. Do not create a second Control policy store.
- Credential path: F-t2-024 (vault) stays with session b as a later item. WS21 keeps persistent profiles plus takeover for W2.

## 5. Affected-file map

Columns: path | role | approx lines (w / l) | live status | existing tests

### 5.1 Model Plane, Rust (apps/Model Plane/rust/services unless noted)

| path | role | approx lines (w / l) | live | tests |
|---|---|---|---|---|
| model-gateway/src/browser_run.rs | browser run start and control; `resolve_browser_grant`; ZDR block; test double | routes 51-52; resolve 138-202; start 307; ZDR fn 627-628 (l:631); test double 808-831 (l) | M (+29) | in-file: issuer_zdr_blocks_all_browser_controls_before_execution_forwarding (w:1287), issuer_zdr_cannot_be_downgraded_by_browser_run_body (w:1318) |
| model-gateway/src/http_routes.rs | mounts `/v1/browser/suggest-action` (223) and merges `browser_run::router()` (227); browser-event projection read (w:1276-1280) | w:223-280 | M (+237) | not read |
| model-gateway/src/tool_loop.rs | inline tool gate; legacy `browser_agent` HTTP arm (BROWSER_AGENT_URL) | `inline_tool_allowed` w:615-619 (l:639-643); dispatch gate w:2489 (l:2515); browser_agent arm w:3071-3146 (l:3062) | M (+425) | asserts w:9107-9110 (l:9222); MCP denials w:9433-9436 |
| model-gateway/src/sse.rs | inline tool-advertisement filter; run-event names incl. browser_* | filter w:1875 (l:1985); names w:9859-9866 (l:10221-10228) | M (+883) | browser_action_approval_required_maps_to_step_update (w:11042); browser_action_decided_maps_to_step_update (w:11089) |
| model-gateway/src/state.rs | AppState; `browser_client` used at browser_run.rs:148 | — | M (+6) | — |
| model-gateway/src/auth.rs | model-gateway service auth; matched browser and grant names | not read | M (+415) | fixtures in L only: tests/fixtures/auth-core/ (??) |
| model-gateway/tests/e2e_invoke_chain_test.rs; tests/orchestration_http_test.rs | e2e tests that mention browser | not read | M (+288; +102) | self |
| model-gateway/src/zdr_posture.rs | ZDR posture matrix; lists `browser_run_zdr_blocked` as a gate | l:778-793 | ?? (live-only) | self |
| execution-core/src/runtime_loop/agent.rs | Do loop: browser tool defs; per-call dispatch through `execute_step_with_subagent` | defs w:4196-4205 (l:4204-4212); dispatch w:1919 (l:1923); helper w:3472 (l:3479) | M (+207) | in-file tests; no grant-carrying browser test found |
| execution-core/src/runtime_loop/mod.rs | step entry points; browser_grant plumbing; QUARRY_MCP_BROWSER_* routing | consts 45-46; `execute_step_with_browser_grant` w:336 (l:339); `execute_step_with_subagent` w:400-452 (l:408-460); `execute_step_inner` w:455, browser_grant param w:480 (l:~493); BROWSER_AGENT arm w:567-583; quarry routing w:589-594 | M (+18) | forged-grant test w:2680 |
| execution-core/src/grpc.rs | ExecuteStep gRPC: broker grant validation; calls `execute_step_with_browser_grant` | `validate_browser_grant` w:184-215 (grant id from tool_input w:189; from_broker_response w:214); ExecuteStep w:828; browser tool gate w:848; grant resolved w:886-890; call w:921-948 (l:~939) | M (+98) | not read |
| execution-core/src/tool_bridge/mod.rs | `ValidatedBrowserGrant`; Quarry browser MCP executor | struct w:36-39; from_broker_response w:45-63; execute_quarry_browser_mcp w:237-249 (no-grant error w:248); unavailable msg w:258-260; parse w:416-462; observe/act arms w:429, 444 | clean | not read |
| execution-core/src/browser_agent.rs | browser_agent loop; grant_id required; persistent-cookie HITL gate | grant_id required w:364; gate call w:820; gate fn w:856; zdr skip w:864 | clean | gate_persistent_cookie_use_skips_when_zdr (w:1528); _skips_when_no_profile (1541); _asks_once_and_proceeds_on_grant (1551); _aborts_the_run_on_denial (1571) |
| execution-core/src/browser_agent/browser_risk.rs | risk classes: RiskCategory and classifiers | enum w:25; PersistentCookieUse w:56, 74; classify_action_risk w:251; classify_deterministic_risk w:259 | clean | classify_* tests w:556-620+ |
| execution-core/src/capability_policy.rs | browser tools map to cap.browser.open | w:793 | clean | not read |
| execution-core/src/permission/mod.rs | permission list includes browser.act | w:271 | clean | not read |
| execution-core/src/llm_planner.rs | browser planner: model JSON to BrowserAction | parse w:186-409; into_browser_action w:304 | clean | tests w:562-781 |
| execution-core/src/browser_events.rs | OrchestrationEventSink for browser progress events | used at grpc.rs w:902-906 | clean | not read |
| execution-core/src/auth.rs | execution-side browser grant revalidation for callers | doc w:179, w:441 | M (+631) | not read |
| execution-core/src/approval_delivery_worker.rs | durable approval delivery (F-t3-006 dependency) | not read | clean | not read |
| execution-core/src/quarry_agent.rs | Quarry agent client; from_env gate | called from tool_bridge/mod.rs w:255 | clean | not read |
| execution-core/tests/browser_agent_e2e.rs | live-stack browser agent e2e | — | clean | self (CI note model-plane-rust-ci.yml:116) |
| mp-crates: crates/mp-orchestration/src/run_event.rs | run-event types incl. browser events | not read | clean | not read |

### 5.2 Model Plane, Go and proto

| path | role | approx lines | live | tests |
|---|---|---|---|---|
| go/services/browser-broker/internal/grant/grant.go | in-memory grant store: Store 62-65; Create 74; CreateWithSensitiveScopes 89; CreateScopedSensitiveApproval 104; GetScoped 172; RevokeScoped 189; NormalizeAllowedDomains 307 | w:39-360 | clean | grant_test.go |
| go/services/browser-broker/internal/server/server.go | BrowserBroker gRPC: AcquireGrant 42-107; RevokeGrant 110-124; ValidateGrant 128-155; Health 157; Register 166 | w:25-170 | clean | server_test.go (AcquireGrant cases at 69, 102, 123, 160, 193-195, 211, 236, 275, 299, 309); latency_test.go; errors_test.go; headers_test.go; ratelimit_test.go |
| go/services/browser-broker/internal/authz/authz.go | audience "browser-broker" (18, 28); Authorize: Acquire and Revoke vs Validate and Health (50-62) | w:18-85 | clean | authz_test.go (line 32) |
| go/services/browser-broker/internal/server/types.go, errors.go, ratelimit.go, headers.go; internal/telemetry/metrics.go | type aliases; error map (errors.go:21); rate limit (ratelimit.go:60); header scrub; metrics | — | clean | per-file tests |
| go/services/browser-broker/cmd/main.go | health :8087 (w:47); gRPC :9095 (w:69) | — | clean | — |
| go/services/browser-broker/go.mod | module github.com/triodelab/model-plane/services/browser-broker; go 1.25.1 | l.1, l.3 | clean | — |
| go/go.work | workspace; use block includes ./services/browser-broker | l.16 | clean | — |
| go/services/orchestrator-core (config.go:84; internal/grpcclient/clients.go:23; cmd/main.go:76) | Go client to BrowserBroker; BROWSER_BROKER_ADDR default browser-broker:9095 | — | clean | call sites not determined |
| go/services/capability-core/migrations/0008_execution_dispatch_capabilities.up.sql; internal/roadmap/data.go | mention browser (not read) | — | clean | catalog_test.go |
| proto/model_plane/v1/browser.proto | service BrowserBroker 11-22; AcquireGrantRequest 25-49 (parent_grant_id 49); AcquireGrantResponse 52-68; ValidateGrantRequest 91; ValidateGrantResponse 96-113 (active 101) | w | clean | — |
| proto/model_plane/v1/browser_agent.proto | BrowserAgentService 11-15 (PlanAction, ReportObservation); ActionType 22; ObservationStatus 49; BrowserAction 91-116 (grant at 96); BrowserObservation 117 | w | clean | — |
| proto/model_plane/v1/execution.proto | ExecuteStep rpc l.15; no browser-grant field (grant mentions are comments) | — | clean | — |
| proto/model_plane/v1/sessions.proto | managed run source enums: EXECUTION_BROWSER = 3 (w:462); GATEWAY_BROWSER = 5 (w:469) | w:461-469 (live shifted; not re-read) | M (+42) | — |
| go/gen/model_plane/v1/browser*.pb.go; browser_agent*.pb.go; python/mp-events-py/.../browser_pb2.py, browser_agent_pb2.py | generated code | — | clean | — |

### 5.3 Ingestion Plane (apps/Ingestion Plane/Quarry-v2)

| path | role | approx lines | live | tests |
|---|---|---|---|---|
| crates/quarry-edge/src/agent_routes.rs | Phase B agent endpoint; grant checks | routes 3661-3671 (POST /v1/agent/runs, /step, /interact, /browser-session); require_browser_grants 326; grant errors 157, 202, 249, 310-333, 364, 1377-1397, 2261-2265, 3096-3099, 3283-3287; persist and ZDR 520-532, 1455-1477 | clean | zdr_behavior.rs, profile_roundtrip.rs (quarry-edge/tests) |
| crates/quarry-edge/src/state.rs | `require_browser_grants` (l:38); `agent_driver` (l:142) | — | M (+21) | tests/support/mod.rs (M, +1) |
| crates/quarry-edge/src/config.rs; src/main.rs | env and config wiring for browser agent (not traced) | — | M (+187; +188) | tests/zdr_claim_floor.rs (M, +10) |
| crates/quarry-edge/src/model_plane_gate.rs | cross-plane model-gateway gate (no browser refs) | — | ?? (live-only) | — |
| crates/quarry-browser/src/navigation.rs; tests/ssrf.rs | navigation and SSRF guard | not read | clean | ssrf.rs |
| crates/quarry-browser/src/kernel.rs; browserless.rs; browserbase.rs | CDP driver adapters | not read | clean | — |
| crates/quarry-runtime/src/dns_guard.rs; browser_egress_proxy.rs; tests/browser_egress_boundary.rs | DNS pin and egress proxy | not read | clean | browser_egress_boundary.rs |
| crates/quarry-core/tests/contracts.rs | contract test mentioning browser | not read | clean | self |
| docs/openapi.yaml | Quarry agent API: /v1/agent/runs 255; step 293; interact 332; receipts 365; egress-receipts 386; procedure 428; run 512; grant_id 2407; approval_grant_id 3099, 3108 | docs | M (+78) | — |
| docs/CROSS_PLANE_INTEGRATION.md | l.60 describes AcquireGrant as the live flow (no caller); l.137 BrowserBroker :9090 GrpcGrantValidator; l.217 health | docs | M (+5) | doc drift (section 10) |

### 5.4 Control Plane (apps/Control Plane)

| path | role | approx lines | live | tests |
|---|---|---|---|---|
| auth-core/src/auth/plane-token-scopes.ts | browser-broker audience | l.32, l.58 | clean | plane-token-scopes.spec.ts (matched; not read) |
| config/plane-service-principals.json | principal scopes incl. browser:execute | l.154, l.171 | clean | plane-service-principals-policy.contract.spec.ts (matched; not read) |
| auth-core/src/auth/fixtures/cross-plane-token-requests.json (+ .sha256) | cross-plane token request fixture; browser rows not checked | — | M (+5) | — |
| user-core/internal/spaces/*: authority.go, owner_grant_decision.go, personal_thread_decision.go, model_action_view.go | Space decisions (audit: Space narrowing point); mention browser (not read) | — | clean | — |
| user-core/migrations/017_space_authority.up.sql | Space authority migration; browser only in comment (l.50) | l.50 | clean | — |
| CONTROL_PLANE_OWNERSHIP.md | ownership doc; no browser text | — | clean | — |

### 5.5 Frontend Plane (apps/Frontend Plane/verevonv3 unless noted)

| path | role | approx lines | live | tests |
|---|---|---|---|---|
| apps/gateway/src/domains/browser.rs | BFF browser routes; AI-run start; grant_id string | routes 371-445 (ai-runs 405); build_ai_run_request 1212-1248 (grant_id 1235); start_ai_run 1263-1279; zdr forwarding 1244-1246 | clean | not read |
| apps/gateway/src/audience_tokens.rs | ModelServiceAudience::BrowserBroker; get and require model-service token | enum 27, 42, 247; cache 81; get 119; require 128 | clean | test 237-256 |
| src/shared/actions/action-registry.ts | browser action zod contracts | 1094-1149 (browserRunStartInput 1098; browserRunControlInput 1108; browserProfileScopeEnum 1113; create and close session 1115, 1127; create, select and close tab 1131, 1136, 1141) | clean | — |
| src/shared/actions/agent-tools.ts; preset-agents.ts; chat-organization.ts; action-client.ts | mention browser (not read) | — | clean | — |
| src/shared/api/chat-client.ts; src/shared/api/http.ts; src/shared/api/sse.ts | chat, HTTP and SSE clients with browser mentions | not read | M (+53; +24; +4) | chat-client.test.ts (M) |
| src/shared/api/run-console-client.ts; src/shared/api/orchestration-client.ts | Do run and orchestration clients (browser mentions) | not read | clean | run-console-client.test.ts; orchestration-client.test.ts |
| src/features/agents/components/AgentRunConsole.tsx | agent run console (Do) | not read | M (+11) | AgentRunConsole.test.tsx (M, +40) |
| src/features/chat/components/ChatLiveRunPanel.tsx | live run panel (takeover surface per audit) | not read | clean | ChatLiveRunPanel.test.tsx |
| src/features/chat/components/ChatMessages.tsx; chat-normalizers.ts; chat-types.ts; use-chat-controller.ts | chat render and state for browser and agent events | not read | M (+229; +39; +7; +4) | ChatMessages.failure-actions.test.tsx (M) |
| src/features/dashboard/home/browser-omnibox.ts | browser omnibox | — | M (+2) | — |
| apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/browser_pb.ts; browser_connect.ts | generated browser clients (non-v3 app) | — | clean | — |

### 5.6 Docs and plans

| path | role | lines | live |
|---|---|---|---|
| docs/BROWSER_WORKSPACE_PLAN.md (repo root) | plan cited by browser.rs:1262; last commit e528b8f6 | not read | clean |
| apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md | plan rows E1-E12 and OD13 | 121-124, 138, 168, 186-187, 213, 263-267, 343 | clean |
| apps/master-ownership-matrix.md | owner table | 54, 128, 131, 159, 163 | clean |
| apps/Model Plane/docs/capability-ownership-matrix.md | one-owner rulings | 73 | clean |
| apps/Model Plane/docs/sse-event-taxonomy.md | SSE taxonomy (no browser text in W or L) | — | M (+4, not browser) |
| apps/Model Plane/docs/core-research/model-gateway.md; core-research/browser-broker.md; decisions/ledger.md | research and decision docs (browser mentions; not read) | — | M (+9); clean; clean |

## 6. Related contracts

- Proto: `browser.proto` (BrowserBroker: AcquireGrant, RevokeGrant, ValidateGrant, Health). AcquireGrantRequest fields: session_key=1, mode=2, org_id=3, allowed_domains=4, allowed_actions=5, allowed_frame_ids=6, allowed_dialog_ids=7, allowed_artifact_ids=8, parent_grant_id=9. `browser_agent.proto` (BrowserAgentService PlanAction and ReportObservation; BrowserAction carries grant at l.96). `execution.proto` ExecuteStep (l.15) has no grant field: the grant is derived inside the handler from the broker (grpc.rs w:886-890). `sessions.proto` managed-run source enums (M).
- SQL migrations: no browser-grant table. Latest session-core in W: `0043_audit_outbox_requeue.sql`. In L, untracked (??) additions: `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, and `revert/`. Browser-mentioning session-core migrations are only `0004_tasks_cron_hooks_skills_memory.sql`, `0015_managed_run_terminalization.sql`, `0033_dispatch_unreachable_failure_code.sql`, not browser tables. Latest capability-core: `0015_background_process_capability.up.sql` / `.down.sql`. Latest user-core: `017_space_authority.up.sql`. A Postgres grant store (F-t2-001) would need a new migration; owner not determined.
- Config and env: `BROWSER_BROKER_ADDR` (orchestrator-core config.go:84, default `browser-broker:9095`; deploy/docker-compose.yml:487, :1113). `BROWSER_BROKER_AUTH_AUDIENCE` (compose :488, :1624, default `browser-broker`; authz.go:26-28 requires exactly that value). `QUARRY_BROWSER_AGENT_ENABLED` (compose :1189, default 1). `QUARRY_EDGE_URL` (tool_bridge/mod.rs w:258-260). `BROWSER_AGENT_URL` (tool_loop.rs w:3080). `require_browser_grants` (quarry-edge state.rs l:38; source in config.rs not traced). Broker health :8087 and gRPC :9095 (main.go w:47, w:69).
- Run events (SSE): orchestration event names `browser_action_dispatched`, `browser_observation_received`, `browser_run_paused`, `browser_run_resumed`, `browser_action_approval_required`, `browser_action_decided` (model-gateway sse.rs w:9859-9866, l:10221-10228). Source: orchestration.proto (not line-checked). Approval routes: `/api/v1/orchestration/runs/:run_id/approvals` (doc at gateway browser.rs:1209-1210).
- Gateway and service routes: BFF `/api/v1/browser/*` (gateway browser.rs:371-445, including `sessions/{session_id}/ai-runs` at 405). Model-gateway: `POST /v1/browser/runs` (browser_run.rs:51), `POST /v1/browser/suggest-action` (http_routes.rs:223). Quarry: `/v1/agent/runs` and sub-routes (agent_routes.rs:3661-3671; openapi.yaml 255-512). Broker: gRPC `model_plane.v1.BrowserBroker` on :9095.
- Action contracts: `src/shared/actions/action-registry.ts:1094-1149` (browser zod schemas). Repo rule: add or update the action contract before wiring a meaningful UI operation.
- Zero Data Retention: gateway forwards server-derived zdr (browser.rs:1244-1246); model-gateway helper at browser_run.rs:627-631; posture matrix at zdr_posture.rs (??, l:778-793).

## 7. Test and build commands

- Model Plane Rust, workspace: `cd "apps/Model Plane/rust" && cargo test --workspace` (apps/Model Plane/CLAUDE.md, Build and Run). Per crate: `cargo test -q -p model-gateway --all-targets` and `cargo test -q -p execution-core --all-targets` (crate names from rust/services/model-gateway/Cargo.toml:2 and rust/services/execution-core/Cargo.toml:2; Model Plane CLAUDE.md, Testing). CI: `cargo test --workspace --no-fail-fast` (.github/workflows/model-plane-rust-ci.yml:119). Live-stack test: execution-core/tests/browser_agent_e2e.rs (CI comment at :116).
- Go: `cd "apps/Model Plane/go/services/browser-broker" && go test ./... && go vet ./...` (Model Plane CLAUDE.md, Testing). Module path from go.mod:1. go.work exists (go/go.work:16).
- Gateway: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets` (repo CLAUDE.md, Frontend block; crate verevon-gateway-rs, Cargo.toml:2).
- Frontend (apps/Frontend Plane/verevonv3): `pnpm typecheck` (tsc -b), `pnpm test` (node --test on gateway scripts, then `vitest run`), `pnpm lint` (eslint .), `pnpm build` (typecheck, then vite build). Source: package.json lines 9-17. Single file: `pnpm vitest run <path>`.
- Quarry: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test --workspace` (repo CLAUDE.md). CI quarry-v2-ci.yml:208-211: `cargo check -p quarry-browser --features chromiumoxide` and `cargo test -p quarry-browser --features chromiumoxide`. Crate quarry-browser (crates/quarry-browser/Cargo.toml:2).
- Control Plane: auth-core `test:cov` and user-core `make test-coverage` (repo CLAUDE.md, Testing). Package scripts not read.
- Stack: `apps/Model Plane/scripts/compose.sh up -d` (Model Plane CLAUDE.md). Broker service: deploy/docker-compose.yml:1612-1624.
- User memory caveats (not repo docs): two Model Plane cargo targets fail pre-existing on Windows, so use `--lib`; auth-core full suite has 15 pre-existing failures in 3 suites, so diff the failing set against HEAD.

## 8. Owners

- Repo CLAUDE.md (Architecture Rules): Model Plane owns browser grants and execution loops; Model proposes browser actions and Quarry-v2 executes or rejects them; Control owns identity, users, orgs, billing, sessions, audit, quotas and entitlements (site lists not named: owner open); Frontend owns the UI and BFF normalization; no direct database crossing between planes; ZDR must propagate.
- Model Plane CLAUDE.md: keep model-gateway `dispatch_tool` and execution-core `execute_step_inner` as separate loops; one canonical owner per capability per `docs/capability-ownership-matrix.md`.
- `apps/master-ownership-matrix.md`: l.54 Browser actions: owner Quarry v2 (quarry-browser, ActionRuntime); Model Plane is a consumer via the agent contract. l.128 execution-core: browser-agent observation and action protocol. l.131 browser-broker (Go): trusted browser grants, validate and revoke, session lease permissions. l.159 rule 3: no agent bypass around Quarry policy. l.163 rule 7: browser-agent planning lives in Model Plane.
- `apps/Model Plane/docs/capability-ownership-matrix.md` l.73: Browser grants owned by browser-broker (Go), storage "in-mem + Redis target" (Redis not found in code).
- By affected area:
  - Grant store, issuance, validation, revocation: Model Plane / browser-broker (Go).
  - Per-run derivation, Do-loop dispatch, browser subagent: Model Plane / execution-core and model-gateway.
  - Executor-side enforcement and consent gating: Ingestion Plane / Quarry-v2 (quarry-browser, quarry-edge).
  - Broker audience and service principals: Control Plane / auth-core.
  - Domain fence and per-site read or act consent: OD13 open (broker, Control or Quarry). Admin lists: audit says Control Plane; not in the master matrix. Space narrowing: Control Plane / user-core (audit).
  - BFF routes and bearer forwarding: Frontend Plane / verevonv3 gateway.
  - UI (consent cards, site table, chat panel): Frontend Plane / verevonv3; action contracts in src/shared/actions first.
  - Run events and trace contract: Model Plane (orchestration events), per the F-t2-014 card.

## 9. Uncommitted-only (live tree, from live-tree-status.txt and git diff --stat)

Model Plane (M unless marked):
- `rust/services/model-gateway/src/browser_run.rs` (+29, ~): ZDR helper now returns Result; test lines added.
- `rust/services/model-gateway/src/tool_loop.rs` (+425): +24 lines at 345 and dispatch edits in dispatch_tool_with_cost; `inline_tool_allowed` content unchanged (shift only).
- `rust/services/model-gateway/src/sse.rs` (+883): large run-event and stream changes; browser event names present (shifted to l:10221).
- `rust/services/model-gateway/src/http_routes.rs` (+237): route changes; browser routes at w:223 and w:227 not diffed.
- `rust/services/model-gateway/src/auth.rs` (+415), `state.rs` (+6), `session_flow.rs` (+5), `chat_turn_registry.rs` (+36): not browser-specific on review.
- `rust/services/model-gateway/tests/e2e_invoke_chain_test.rs` (+288, ~), `tests/orchestration_http_test.rs` (+102).
- `rust/services/execution-core/src/runtime_loop/agent.rs` (+207): +7 shift before the tool defs, test additions, continuation_descriptor hunk near 4566.
- `rust/services/execution-core/src/runtime_loop/mod.rs` (+18): new lines in execute_step (319), execute_step_with_browser_grant (356, 385), execute_step_with_subagent (425, 453), execute_step_inner (484, 652), knowledge search (1208, 1244).
- `rust/services/execution-core/src/grpc.rs` (+98): ExecuteStep changes (persistence-free enforcement, durable approval, scheduled auth); browser call site shifted to l:~939.
- `rust/services/execution-core/src/auth.rs` (+631): not traced to browser paths.
- `proto/model_plane/v1/sessions.proto` (+42): managed run source enums (browser values at w:461-469; live positions not re-read).
- `docs/core-research/model-gateway.md` (+9); `docs/sse-event-taxonomy.md` (+4, no browser text).
- ?? (live-only): `rust/services/model-gateway/src/zdr_posture.rs` (lists browser_run_zdr_blocked); `rust/services/session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `revert/`.

Ingestion Plane (Quarry-v2):
- `crates/quarry-edge/src/state.rs` (+21, ~; `require_browser_grants` at l:38); `src/config.rs` (+187, ~); `src/main.rs` (+188, ~); `src/routes.rs` (+1); `src/test_support.rs` (+1); `tests/support/mod.rs` (+1); `tests/zdr_claim_floor.rs` (+10).
- `docs/openapi.yaml` (+78, ~); `docs/CROSS_PLANE_INTEGRATION.md` (+5, ~); `docs/RUNBOOKS.md` (M, not stat'd).
- ?? (live-only): `crates/quarry-edge/src/model_plane_gate.rs` (no browser refs).

Control Plane:
- `auth-core/src/auth/fixtures/cross-plane-token-requests.json` (+5, ~) and `.sha256`.

Frontend Plane (verevonv3):
- `src/shared/api/chat-client.ts` (+53), `http.ts` (+24, ~), `sse.ts` (+4, ~).
- `src/features/agents/components/AgentRunConsole.tsx` (+11, ~) and `AgentRunConsole.test.tsx` (+40).
- `src/features/chat/components/ChatMessages.tsx` (+229, ~), `chat-normalizers.ts` (+39), `chat-types.ts` (+7, ~), `use-chat-controller.ts` (+4); `ChatMessages.failure-actions.test.tsx` (M).
- `src/features/dashboard/home/browser-omnibox.ts` (+2).

Clean (no live edits) and relevant: all browser-broker Go files, proto `browser.proto` and `browser_agent.proto`, execution-core `tool_bridge/mod.rs`, `browser_agent.rs`, `browser_agent/browser_risk.rs`, `capability_policy.rs`, `permission/mod.rs`, `llm_planner.rs`, `quarry_agent.rs`; quarry-browser crate; quarry-runtime egress and DNS guards; `agent_routes.rs`; gateway `browser.rs` and `audience_tokens.rs`; `action-registry.ts`; `plane-token-scopes.ts`; `plane-service-principals.json`; `deploy/docker-compose.yml`; `docs/BROWSER_WORKSPACE_PLAN.md`; both ownership matrices; `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md`.

## 10. Doc drift noticed (for the merge)

- `quarry-v2/docs/CROSS_PLANE_INTEGRATION.md:60` describes `BrowserBroker.AcquireGrant` as the live Model Plane to Quarry flow. No runtime caller exists (only the test double at browser_run.rs:818 and broker tests).
- `apps/Model Plane/go/services/orchestrator-core/README.md:252` gives the default `BROWSER_BROKER_ADDR` as `localhost:50055`. `config.go:84` gives `browser-broker:9095`.
- Card F-t3-008 says `ValidatedBrowserGrant` carries "its risk classes". The struct has only `grant_id` and `allowed_domains` (tool_bridge/mod.rs:36-39).
- Card cite `tool_loop.rs:630-646` is stale in W. Correct W cite is 615-616; the live cite 639-646 matches.
- Card cite `agent.rs:4204-4212` matches L, not W (W is 4196-4205).
- User auto-memory says Model Plane has "no go.work". `apps/Model Plane/go/go.work` exists (W and L, clean).
- Gateway comment `browser.rs:1258-1262` (issuance unbuilt) is accurate.

## 11. NOT DETERMINED

- Whether any runtime path issues `AcquireGrant`. Grep found only the test double (browser_run.rs:818), broker tests and the broker's own server. `orchestrator-core` holds a BrowserBroker connection (clients.go:23) but no call site was found.
- Which caller sends ExecuteStep with `browser_bearer` metadata (grpc.rs:886), and whether any BFF or orchestrator path does so today.
- Whether the orchestrator path that calls ExecuteStep produces a valid `ValidatedBrowserGrant` end to end (not traced past grpc.rs:886-948).
- Whether the legacy chat `browser_agent` HTTP arm (tool_loop.rs w:3071-3146, BROWSER_AGENT_URL) is deployed.
- Quarry `require_browser_grants` default and its env source (config.rs is M and not traced).
- Whether Quarry's per-action checks match the broker's `allowed_domains` semantics.
- Effect of the uncommitted changes on browser paths. Nothing was run.
- Whether any Control Plane service holds a site table outside apps/Control Plane (grep covered that tree only).
- OD13 owner: no repo doc records a decision.
- Redis-backed grant store: the capability matrix names it as a target; no Redis code was searched for or found.
- Which Frontend app owns `apps/Frontend Plane/verevon` (non-v3): not determined.
- Test pass or fail status: not run.
