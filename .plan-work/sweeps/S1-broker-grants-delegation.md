# S1 broker-grants-delegation

Scope: F-t2-001, F-t2-013, F-t2-011, F-t2-024, F-t2-032, F-t2-036. Source of truth is the live tree `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted work). Paths are repo-relative to that root. Line numbers are live-tree numbers. Tags from `tools/provenance.sh`: [C] committed and unchanged, [M] committed but modified in the live tree, [U] untracked (exists only as uncommitted work), [NEW] would be created by the plan. Verdicts: OK, DRIFTED (corrected range given), FALSE, MISSING, UNVERIFIED. Sizes are `wc -l` of the live file.

## Cross-item overlaps (merge-conflict hotspots)
1. `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs` [C, 6317]: 001, 011, 013, 024, 032, 036.
2. `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs` [C, 3737]: 001, 011, 013, 024, 032, 036.
3. `apps/Model Plane/go/services/browser-broker/internal/grant/grant.go` [C, 362]: 001, 013, 024, 032, 036.
4. `.../browser-broker/internal/server/server.go` [C, 179], `apps/Model Plane/proto/model_plane/v1/browser.proto` [C, 114] and `apps/Model Plane/go/gen/model_plane/v1/browser*.pb.go` [C]: 001, 032, 036.
5. `apps/Model Plane/rust/services/execution-core/src/grpc.rs` [M, 2121] and `.../execution-core/src/browser_agent.rs` [C, 1709]: 001 (validate_browser_grant), 013 (start_run), 032 (run policy), 036 (events).
6. `.../execution-core/src/browser_agent/browser_risk.rs` [C, 1288] and `.../execution-core/src/browser_events.rs` [C, 857]: 024 (Login gate), 032 (ApprovalOutcome), 036 (decided event).
7. `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs` [M, 1330] and `.../model-gateway/src/auth.rs` [M, 3304]: 001 (bearer, resolve_browser_grant), 032 (run-policy equality), 013 (any attach route).
8. `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` [M, 1569], `.../quarry-edge/src/config.rs` [M, 501] and `.../quarry-edge/src/state.rs` [M, 155]: 001 (validator wiring and flags), 011 (flag defaults), 032 (denial plumbing).
9. `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx` [C, 2172]: 011 (omnibox, reload), 013 (takeover chip), 024 and 032 (approval card, lines 1804-1815).
10. `apps/Model Plane/rust/services/session-core/src/orchestration_grpc.rs` [M, 6215], `.../session-core/src/orchestration_store.rs` [C, 1673], `.../session-core/src/audit_publisher.rs` [C, 1945], `.../session-core/src/retention_receipt.rs` [U, 1735]: 032, 036. Also `apps/Ingestion Plane/integration-corev2/internal/api/server.go` [M, 4256] and `.../internal/config/config.go` [M, 1247] (024 only; both carry uncommitted ZDR and GDPR work).

## Global notes (affect several items)
- G1 (not in audit): the Quarry-to-BrowserBroker validator sends no bearer outside dev bypass. `GrpcGrantValidator.auth_token` is set only by `with_bearer_token`, which `main.rs:667-672` calls only under `cross_plane_auth_dev_bypass`. `apply_auth` (`grpc/browser_broker.rs:87-94`) adds no Authorization header otherwise, and the broker rejects bearer-less calls (`authz_test.go:123` expects Unauthenticated). Code-verified; runtime not tested.
- G2 (not in audit): no CI workflow runs the browser-broker Go tests. Grep of `.github/workflows` for `browser-broker`, `go/services` and `services/browser` returns nothing; `buf.yml` covers the proto only. `integration-corev2` matches no workflow. `audit-core` appears only in path filters (`model-plane-rust-ci.yml:36, 49`, testdata); no job runs audit-core Go tests.
- G3: Quarry's own tests run with `NoopGrantValidator` and `require_browser_grants: false` (`quarry-edge/src/test_support.rs:102-103` [M], `tests/support/mod.rs:47-48`). No test sets `require_browser_grants: true` (grep, no hits).
- G4: `apps/Model Plane/docs/decisions/ledger.md` [C] (69 entries) has no entry on BrowserBroker grant issuance or browser delegation (grep for BrowserBroker, browser grant, browser-broker, BrowserGrant, x-browser-authorization: 0 hits). Nearest precedent: line 3043, "Sandbox lease identity: a ninth delegated user bearer, not a service token or token exchange (2026-09-11)".

---

## F-t2-001 · Production BrowserBroker grants and delegated bearer forwarding

### 1. CITATION CHECK
| Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|
| gateway `browser.rs:483-506` (card, json) | C | OK | `create_session` body: `max_steps` 12 (485), `allowed_domains` = first host (486), `max_runtime_s` 120 (487). No `grant_id` key. POST `/v1/agent/runs` at 506. |
| `browser.rs:1235` (card: "start_ai_run sends session:{id}") | C | OK, misattributed | The string is built in `build_ai_run_request` (fn 1212-1248), which `start_ai_run` (fn 1263) calls. |
| `browser.rs:1258-1262` (json) | C | OK | Doc comment: real grant issuance "still unbuilt". |
| `browser.rs:1318-1360` (json) | C | OK | start_ai_run forwards model, data-plane, inference, execution and session tokens (1321-1325) into `proxy_model_json_with_data_plane` (1350). `x-browser-authorization` appears nowhere in browser.rs. |
| `chat/shared.rs:629-690` (card, json) | C | OK | `proxy_model_json_with_delegations` sets x-data-plane (673), x-inference (679), x-execution (682), x-session (688). No browser header. `proxy_model_json_with_data_plane` (405-435) passes `None` in placeholder slots (425, 430, 431). |
| `audience_tokens.rs:27-42` (card, json) | C | OK | BrowserBroker variant (27), claim "browser-broker" (42). No reference outside this file (grep). |
| auth-core `convex-token.service.ts:446-455` (json) | C | OK | `browser-broker` audience via `BROWSER_BROKER_AUTH_AUDIENCE`, TTL env default 300. |
| `grant.go:61-71` (card), `:62-64`, `:62-69` (json) | C | OK | `Store` is a mutex and map (62-65); `NewStore` 68-70. |
| broker `cmd/main.go:62-64` (card), `:63` (json) | C | OK | Line 63: `bbserver.NewServer(grant.NewStore())`. |
| `server.go:22` (json) | C | OK | `defaultGrantTTL = 15 * time.Minute`. |
| `server.go:41-103` (json) | C | OK, short | AcquireGrant is 42-107. Org and owner come from the verified principal (44-55). Sensitive branch 81-90 needs `parent_grant_id`. |
| `server.go:106-150` (json) | C | OK, loose | RevokeGrant 110-125 (calls RevokeScoped at 119). ValidateGrant 128-155. |
| `browser.proto:12-60` (json) | C | OK | Service at 11, RPCs 13-22, AcquireGrantRequest 25-49, response 52-69. |
| model-gateway `browser_run.rs:138-190` (json) | M | OK | `resolve_browser_grant` 138-187 calls ValidateGrant. Requires canonical broker policy (164-171) and rejects a request policy that differs (180-186). Committed content; uncommitted hunks are 307-313 and 627-666. |
| `browser_run.rs:297-310` (json) | M | OK | `browser_run_start` 297; `browser_bearer: VerifiedBrowserBearer` at 304; ZDR refusal at 307 (inside an uncommitted hunk). |
| model-gateway `auth.rs:427-437` (card) | M | OK | `VerifiedBrowserBearer` extractor 427-439 returns UNAUTHORIZED when the extension is missing (437-438). Not in an uncommitted hunk. |
| `auth.rs:943-955` (json) | M | OK, in flight | `verify_delegated_browser_bearer` 942-955 reads `x-browser-authorization` (948) and `BROWSER_BROKER_AUTH_AUDIENCE` (949). Uncommitted change at 945 alters its error type from StatusCode to AuthRejection. |
| execution-core `grpc.rs:189-215` (json) | M | OK | `validate_browser_grant` 189-218. 3 s timeout; Unavailable or DeadlineExceeded maps to unavailable, else permission_denied. Not in an uncommitted hunk. |
| quarry-edge `main.rs:182-200` (json) | M | OK | Prod forces `require_browser_grants` (183-184). Boot bails without a validator URL (197-201). Uncommitted hunks start at 47 and 244; grant wiring unchanged. |
| `agent_routes.rs:1377-1412` (card, json) | C | OK | 1377-1381: "browser grant_id is required" (403) when required. 1397-1404: broker domains overwrite client domains. |
| `agent_routes.rs:314-339` (card, json) | C | OK | `validate_run_grant` 314-338: usable check 320-325 (SecurityBlocked); exact equality 326-335. |
| `agent_routes.rs:2257-2266` (json) | C | OK | step() 2228 validates the grant (2257-2260); 403 when the grant is missing under the requirement (2261-2266). |
| quarry-runtime `grpc/browser_broker.rs:97-130` (json) | C | OK | `acquire()` 97-131 sends empty `allowed_domains` (deny-by-default). No caller (see section 3). |
| execution-core `browser_agent.rs:669-690` (json) | C | OK | `client.start_run(...)` 673-681 passes `plan.config.grant_id` (679) and `profile_id`. |
| `docs/BROWSER_WORKSPACE_PLAN.md:7` (json) | C | OK as quote, stale | Line 7 reads "Program status: COMPLETE (2026-07-09)". Contradicted by `browser.rs:1258-1262`. |
| `agent_routes.rs:1446` (json) | C | OK | `ttl_s = constraints.max_runtime_s.unwrap_or(120)`. |

Skipped (checked OK, not tabled): `grant.go:62` (folded into row 8), `browser.rs:1255-1263` (doc block, folded into row 4).

### 2. AFFECTED FILES
- `apps/Model Plane/go/services/browser-broker/internal/grant/grant.go` [C, 362]: `Store` 62-70 (in-memory, to become Postgres); `Create` 74; `CreateWithSensitiveScopes` 89; `CreateScopedSensitiveApproval` 104; `GetScoped` 172; `RevokeScoped` 189; `NormalizeAllowedDomains` 307. Callers: server.go:77, 85, 110-119.
- `.../browser-broker/internal/server/server.go` [C, 179]: AcquireGrant 42-107 (add run-grant derivation; principal pinning 44-55); RevokeGrant 110-125; ValidateGrant 128-155; `scopeURLForMode` 170-179.
- `.../browser-broker/cmd/main.go` [C, 79]: `grant.NewStore()` at 63; gRPC on :9095 (56). Needs a DB handle.
- `.../browser-broker/internal/authz/authz.go` [C, 90]: audience check 27-28; AcquireGrant and RevokeGrant special case 51-59. `authz_test.go:32` shows user principals reach AcquireGrant with org pinning; service principals need ScopeWrite (`authz_test.go:127`).
- `apps/Model Plane/proto/model_plane/v1/browser.proto` [C, 114]: AcquireGrantRequest 25-49 (no run-mode or action-class fields); ValidateGrantRequest 91-94. `buf lint` and `buf breaking` run in `.github/workflows/buf.yml` against `proto/baseline.binpb`. Generated code: `apps/Model Plane/go/gen/model_plane/v1/browser.pb.go` [C] (plus `browser_grpc.pb.go`, `browser_agent*.pb.go`).
- `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs` [M, 1330]: `resolve_browser_grant` 138-187; `browser_run_start` 297. The broker client is `state.browser_client`. Natural home for an HTTP issue endpoint (audit prefers this over gateway gRPC).
- `.../model-gateway/src/auth.rs` [M, 3304]: `VerifiedBrowserBearer` 427-439; `verify_delegated_browser_bearer` 942-955; tests 1899, 1999.
- `apps/Model Plane/rust/services/execution-core/src/grpc.rs` [M, 2121]: `validate_browser_grant` 189-218 (call at 202); `BROWSER_BROKER_URL` and `_ADDR` 1360-1361; browser channel 1454.
- `.../execution-core/src/browser_agent.rs` [C, 1709]: `PlanConfig` 243-292 (`grant_id` 245); start_run call 673-681.
- `.../execution-core/src/quarry_agent.rs` [C, 963]: `AGENT_SCOPES = ["browser:execute"]` (34); `token()` 554-559; `start_run` 574.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/grpc/browser_broker.rs` [C, 296]: `GrpcGrantValidator` struct 24-27 (`auth_token: Option`); `connect` 45; `with_bearer_token` 78; `apply_auth` 87-94; `impl GrantValidator` 157; `validate` 165-; legacy `acquire()` 97-131.
- `.../quarry-runtime/src/grant_validator.rs` [C, 371]: `GrantValidator` trait 114; `NoopGrantValidator` 121.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` [M, 1569]: validator selection 659-672 (bearer only under dev bypass, 667-672); HTTP fallback 1551-1557; prod gate 183-201.
- `.../quarry-edge/src/config.rs` [M, 501]: `browser_grant_validator_url` 124; `browser_grant_validator_grpc_url` 129; `require_browser_grants` 132.
- `.../quarry-edge/src/state.rs` [M, 155]: `require_browser_grants` field 38.
- `.../quarry-edge/src/test_support.rs` [M]: NoopGrantValidator with `require_browser_grants: false` at 102-103 (the uncommitted change is one unrelated line).
- `.../quarry-edge/src/agent_routes.rs` [C, 3737]: start_run 1273-1421; step 2228-2300; validate_run_grant 314-339.
- gateway `browser.rs` [C, 6317]: create_session 451-506 (add grant issuance and `grant_id` on the body); build_ai_run_request 1212-1248 (`grant_id` 1235); start_ai_run 1263-1388 (bearers 1321-1325); control_ai_run 1389 (second proxy call 1432).
- gateway `chat/shared.rs` [C, 1492]: `proxy_model_json_with_delegations` 629-690 (add a browser-bearer parameter and header); `proxy_model_json_with_data_plane` 405-435.
- gateway `audience_tokens.rs` [C, 376]: BrowserBroker variant 27; claim 42.
- `apps/Control Plane/auth-core/src/auth/convex-token.service.ts` [C, 1101]: browser-broker audience 446-455.
- Cross-plane manifest: `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` and `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json` (both [M], 79 lines in auth-core), with their `.sha256` files (also [M]). Both need a quarry-edge to browser-broker row, which does not exist. Refresh the sha256 and the digest registered in `scripts/tests/zdr-contract-sync.test.sh`, as the file's own description says.
- New Postgres store: `apps/Model Plane/go/services/browser-broker/migrations/` [NEW dir; does not exist]. Use `0001_browser_grants.sql`. The broker module is `github.com/triodelab/model-plane/services/browser-broker`. Next free number for another Model Plane store: `apps/Model Plane/rust/services/session-core/migrations/` [C], latest `0045_zdr_waived_runs_org_index.sql`, so `0046_*.sql`.

### 3. REUSABLE BUT UNWIRED
- `grant.Store.CreateScopedSensitiveApproval` (grant.go:104-122): the only caller is AcquireGrant (server.go:85), which has no production caller. Proof: `grep -rn "AcquireGrant\|acquire_grant\|\.acquire(" "apps/Model Plane" "apps/Ingestion Plane/Quarry-v2" "apps/Frontend Plane/verevonv3/apps/gateway" --include=*.rs --include=*.go --exclude-dir=.claude` returns only the server and authz definitions, the RPC name in authz.go, tests (server_test.go, latency_test.go, authz_test.go) and quarry-runtime's own `acquire` definition.
- Quarry `GrpcGrantValidator::acquire` (browser_broker.rs:97-131): no caller; `\.acquire(` has no hits outside its definition.
- Gateway `ModelServiceAudience::BrowserBroker` (audience_tokens.rs:27): no reference outside its file.
- Wired, for contrast: model-gateway `resolve_browser_grant` (called from browser_run_start), Quarry `validate_run_grant` (called from step at 2258 and new_tab at 3279-3282), `GrpcGrantValidator` (main.rs:666).

### 4. TESTS
- Go broker: `cd "apps/Model Plane/go/services/browser-broker" && GOWORK=off go test ./...` (memory: no go.work; -race needs cgo, unavailable). Files: `internal/grant/grant_test.go` (4 tests: TestStoreScopesCopiesAndRevokesGrants, TestStoreReportsExpiredAndUnknownGrants, TestNormalizeAllowedDomainsRejectsUnboundedOrNonHostPolicy, TestStoreCopiesCanonicalAllowedDomains); `internal/server/server_test.go` (15, including TestAcquireGrant_Success, TestRevokeGrant_Success, TestValidateGrant_RevokedReturnsFailedPrecondition, TestValidateGrantReturnsBrokerOwnedNormalizedDomainPolicy); `internal/server/latency_test.go` (TestAcquireGrant_P95Latency); `internal/authz/authz_test.go` (TestAuthorizationPinsTenantAndPrincipalContext, TestInterceptorFailsClosedAndOnlyStandardHealthIsPublic). CI: none.
- model-gateway: `cd "apps/Model Plane/rust" && cargo test -p model-gateway browser` (memory: two cargo targets fail pre-existing on Windows; use `--lib`). Tests: auth.rs:1899 `verified_browser_bearer_debug_output_never_exposes_the_token`; auth.rs:1999 `delegated_browser_bearer_requires_exact_audience_and_matching_identity`; browser_run.rs has 15 test attributes (e.g. 1325 `issuer_zdr_cannot_be_downgraded_by_browser_run_body`). CI: `.github/workflows/model-plane-rust-ci.yml:119` (`cargo test --workspace --no-fail-fast`).
- execution-core: `cd "apps/Model Plane/rust" && cargo test -p execution-core validate_browser_grant`. Tests live in grpc.rs `auth_tests` (from about 1796, uncommitted hunks there). CI: model-plane-rust-ci.yml:119. The live-stack test `browser_agent_e2e` is at :116.
- Gateway: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets build_ai_run_request` (crate `verevon-gateway-rs`; 65 test attributes in browser.rs). CI: verevonv3-ci.yml, gateway-rust job, `cargo test` at :141.
- Quarry: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge` (memory: run Quarry Rust tests in the rust:1 Docker container, more than 10 minutes). agent_routes.rs has 16 test attributes, e.g. 2029 `browser_session_projection_requires_exact_tenant_and_actor`, 2051 `allowed_domains_accept_exact_and_subdomains_only`. No prod-mode grant test. CI: quarry-v2-ci.yml:205 (`cargo test --workspace`).
- Cross-service contract test (gateway, model-gateway, execution-core, quarry-edge): does not exist.
- auth-core: `cd "apps/Control Plane/auth-core" && pnpm test` (memory: 15 pre-existing failures in 3 suites; diff failing sets against HEAD, do not trust exit codes). CI: auth-service.yml:48; contract-goldens.yml triggers on `cross-plane-token-requests.json*`; quarry-docs-contract.yml reads the manifest.

### 5. FEATURE FLAGS / ENV
| Name | Read at | Default | Note |
|---|---|---|---|
| QUARRY_EDGE__REQUIRE_BROWSER_GRANTS | config.rs:131-132 (`require_browser_grants`) | false; prod forces true (main.rs:183-184) | Env name follows the crate convention; the sibling name is confirmed at main.rs:195 |
| QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL | config.rs:128-129; main.rs:661-666 | unset | Preferred path; needs the `grpc` feature (main.rs:193-195) |
| QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL | config.rs:123-124; main.rs:1551-1557 | unset | HTTP shim; unset falls back to NoopGrantValidator outside prod |
| QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS | main.rs:174, 667-668 | off | Only switch that attaches the model-plane token to the broker validator; forbidden in prod (174) |
| ENVIRONMENT (prod or production) | main.rs:179-184 | n/a | Forces durable runtime and browser grants |
| BROWSER_BROKER_BASE_URL | server.go:34 | empty | Grant endpoint base |
| BROWSER_BROKER_AUTH_AUDIENCE | authz.go:27; model-gateway auth.rs:949; convex-token.service.ts:447-450 | browser-broker | Must match everywhere |
| AUTH_CORE_ISSUER, AUTH_CORE_JWKS_URL | authz.go:32-33 | n/a | Token verifier |
| PLANE_TOKEN_TTL_BROWSER_BROKER_SECONDS | convex-token.service.ts:452 | 300 | Audience token TTL |
| BROWSER_BROKER_URL, BROWSER_BROKER_ADDR | execution-core grpc.rs:1360-1361; model-gateway state.rs:577-578 | n/a | Client endpoints |
| Hard-coded | server.go:22 (15 min grant TTL); grant.go:29 (32 domains); grant.go:32-36 (3 sensitive actions); main.go:56 (:9095); browser_broker.rs:174 (3 validate attempts) | n/a | Not env-configurable |

### 6. SURPRISES
1. G1 above: once issuance exists, prod validation still fails, because the validator sends no bearer. This is a second blocker the audit does not name.
2. Manifest gap (not in audit): `grep -ci browser` on both cross-plane-token-requests.json copies returns 0. Rows cover data-plane and model-gateway only. In the live tree both JSON files and their .sha256 files are uncommitted. The committed sha256 (0849784e…) matches the committed JSON; the live sha256 (40bd3586…) matches the live JSON. The live edit fills `used_by` for a rerank row (QB-T09 work), so this manifest is in flux.
3. The card says start_ai_run sends `session:{id}`. The string is built in `build_ai_run_request` (browser.rs:1212-1248). Same effect, different function.
4. Gateway test `build_ai_run_request_never_forwards_client_requested_require_approval` (browser.rs:5191-5232) pins `grant_id == "session:sess_1"` at 5228. A correct fix breaks it on purpose.
5. Prod mode is untested in Quarry (G3). Both `test_support.rs:102-103` and `tests/support/mod.rs:47-48` use Noop.
6. In-flight edits sit beside the path: auth.rs:945 changes the return type of `verify_delegated_browser_bearer`; browser_run.rs:307-313 changes ZDR refusal lines. Neither touches `resolve_browser_grant` (138-187) or the extractor (427-439).
7. Stale docs: `docs/BROWSER_WORKSPACE_PLAN.md:7` says COMPLETE. `apps/Ingestion Plane/Quarry-v2/docs/SELF_HOST.md:130` says Model Plane "issues a grant via BrowserBroker.AcquireGrant", but nothing calls it. SELF_HOST.md is [M], yet line 130 is outside its uncommitted diff.
8. G4 above: no ledger entry covers grant issuance. Use ledger.md:3043 (ninth delegated bearer) as the precedent for `x-browser-authorization`. An ADR is needed (audit: adrNeeded true).
9. The broker store is in memory (main.go:63). A broker restart invalidates every grant, and Quarry's next ValidateGrant fails. [INF]

---

## F-t2-013 · Co-browsing: the agent works in the user's visible session, takeover enforced in Quarry

### 1. CITATION CHECK
| Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|
| `agent_routes.rs:1733-1785` (card, json) | C | OK | set_browser_session_control 1733-1771: `entry.control_mode = body.mode` (1760); Control timeline event with `initiated_by: Human` hard-coded (1766). |
| `step_receipts.rs:55-90` (card), `:67-70` (json) | C | OK | `control_mode` field on the run record (70). Enum `BrowserControlMode` 86-90, `#[default] AgentControl` (87-88). |
| `agent_routes.rs:2227-2294` (card) | C | OK | step() is 2228-2300. No `control_mode` read in step() (2228-2300), `execute_ws_action` (3086-3195) or `new_tab` (3240-3290). |
| gateway `browser.rs:696-723` (card) | C | OK | run_action (fn 663) blocks actor == Agent during HumanTakeover (705-711). Human sets takeover (715). |
| `browser.rs:2065-2075` (card) | C | OK | prepare_client_ws_message (fn 2037): actor string "agent" blocked during takeover (2071-2075). |
| `browser.rs:1235` (card) | C | OK | `session:{id}` built in build_ai_run_request (1212-1248). |
| execution-core `browser_agent.rs:674` (card, json) | C | OK | `client.start_run(...)` 673-681 leases its own session with `profile_id` and `grant_id`. PlanConfig 243-292 has no attach parameter. |
| quarry-edge `auth.rs:60-139` (card) | C | OK | Claims 62; `is_service` 101-103; `actor_id` 144-160. |
| `grant.go:44-130` (card), `:30-130` (json) | C | OK | SessionKey 43; ParentGrantID 56; CreateScopedSensitiveApproval 104-122 requires a matching session key and no actions on the parent. |
| `agent_routes.rs:474-482` (card) | C | OK | caller_owns_browser_run: org equals claims org and actor_id equals `claims.actor_id()`. |
| `agent_routes.rs:493-495` (card) | C | OK | record_browser_timeline_event returns early under ZDR. |
| `agent_routes.rs:2250-2300` (json) | C | OK | step() owner gate 2249-2255: non-owner gets 404. Budget 2294; domain FORBIDDEN 2299-2300. |
| `agent_routes.rs:1725-1800` (json) | C | OK | Set control (1733-1770); timeline (1787). |
| `step_receipts.rs:185-200` (json) | C | OK | StepReceipt `actor_id` 192, `action` 198. No initiator field. |
| `BrowserChrome.tsx:497-624` (json) | C | OK | humanTakeoverActive 497; model actions disabled 559-560; toggle 624 (`agent_control` and `human_takeover`). |
| `BrowserChrome.tsx:1225-1245` (card), `:1215-1250` (json) | C | OK | AI/Manual control chip 1228-1237; aria-label 1230. |
| `ChatLiveRunPanel.tsx:608` (card), `:39`, `:598-612` (json) | C | OK | Import 38-40; `<BrowserChrome compact session={model()} />` at 608. The audit's "read-only, pause/resume" claim is UNVERIFIED. |
| execution-core `quarry_agent.rs:488-567` (json) | C | DRIFTED | The org-scoped service token is `token()` 554-559 (scopes at 34). start_run is 574. Cite 554-559 and 574. |
| gateway `browser.rs:663-732`, `:761-796` (json) | C | OK | run_action 663; set_control_mode 761 POSTs `/v1/agent/runs/{id}/browser-session/control` (786-787). |
| `chromiumoxide.rs:2447-2590`, `:969-1005` (json) | C | OK, names only | list_tabs 2447; install_popup_bridge 969. Bodies not re-read (UNVERIFIED). |

Skipped (checked OK, not tabled): `agent_routes.rs:1335, 1715, 1824` (other caller_owns uses); `browser.rs:1212` (build_ai_run_request signature).

### 2. AFFECTED FILES
- `agent_routes.rs` [C, 3737]: RunEntry `control_mode` 92; checkpoint 470; set_browser_session_control 1733-1771; record_browser_timeline_event 488-510; step gate 2228-2300 (add the control check after the owner check 2249-2255); `execute_ws_action` 3086-3195 and `new_tab` 3240-3290 need the same gate; receipt append 2352-2364 (add initiator); route 3673.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/step_receipts.rs` [C, 818]: StepReceipt 183-200+ (add initiator and control epoch); BrowserControlMode 86-90; BrowserTimelineInitiator 161.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/auth.rs` [C] (size not measured): `is_service` 101; `actor_id` 144.
- gateway `browser.rs` [C, 6317]: run_action 663-760 (UX pre-check stays); prepare_client_ws_message 2037-2105; set_control_mode 761-800; ActionBody 246-251 (`actor` at 250).
- execution-core `browser_agent.rs` [C, 1709]: start_run 673-681; PlanConfig 243-292 (add `attach_session_id`). No USER_TOOK_OVER mapping exists.
- execution-core `quarry_agent.rs` [C, 963]: start_run 574; token 554-559 (on-behalf-of design needed).
- Broker `grant.go` [C, 362] and `server.go` [C, 179]: session-bound child grant (ParentGrantID 56; CreateScopedSensitiveApproval 104).
- `BrowserChrome.tsx` [C, 2172]: control chip 1228-1237; takeover toggle 624-629; "Ask AI on this tab" does not exist.
- `ChatLiveRunPanel.tsx` [C, 713]: embed 598-609.
- Migrations: none needed for Phase 1, since control mode is already persisted on the run record.

### 3. REUSABLE BUT UNWIRED
- `BrowserTimelineInitiator::Agent` is never constructed. The only constructions are `Human`, at agent_routes.rs:1766 and step_receipts.rs:798. Proof: `grep -rn "BrowserTimelineInitiator::" "apps/Ingestion Plane/Quarry-v2" --include=*.rs --exclude-dir=.claude`.
- `BrowserControlMode` is persisted and projected (agent_routes.rs:470, 629) but not enforced on any step path. Built, not enforced.
- Broker `ParentGrantID` and SessionKey (grant.go:43, 56): reachable only through the unwired AcquireGrant.
- Wired: `Claims::is_service()` (auth.rs:101, used by the least-privilege gate at 508). Suitable for deriving the initiator. Gateway takeover pre-checks are wired (UX only).

### 4. TESTS
- Quarry: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge`. Only browser_session_projection_requires_exact_tenant_and_actor (agent_routes.rs:2029) touches ownership. No Quarry test asserts that step() refuses during HumanTakeover.
- Gateway takeover tests: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" websocket_`. Names: browser.rs:5571 websocket_human_action_updates_control_replay; 5631 websocket_agent_action_is_rejected_during_human_takeover; 5686 websocket_control_release_updates_state_and_returns_session.
- Frontend: `cd "apps/Frontend Plane/verevonv3" && pnpm test -- browser-session browser-loop` (both test files exist in `src/features/dashboard/home/`). Takeover coverage UNVERIFIED.
- CI: verevonv3-ci.yml (gateway-rust cargo test :141; `pnpm verify` :193); quarry-v2-ci.yml:205.

### 5. FEATURE FLAGS / ENV
- No env switch for takeover. Default mode is `BrowserControlMode::AgentControl` (step_receipts.rs:87-88, `#[default]`).
- ZDR is per run (timeline writes skipped at agent_routes.rs:493-495); no env switch.

### 6. SURPRISES
1. Gateway actor is a request field that defaults to Human (`ActionBody.actor`, browser.rs:249-250; `BrowserActionActor` default Human at 255-258). A caller that omits `actor` is treated as Human, so the Agent checks at 705-711 and 2071-2075 never fire. Not in the audit.
2. The WebSocket action path (`execute_ws_action` 3086-3195) and `new_tab` (3240-3290) also skip `control_mode` in Quarry. The audit names only step(). Not in the audit.
3. step()'s owner gate (2249-2255) is the real Phase 2 blocker, as the audit says.
4. One-shot sensitive approvals live in a process-local HashSet (agent_routes.rs:98-101). A Quarry restart resets them, and resumed runs get fresh authority. Include in the race analysis.
5. No in-flight edits in agent_routes.rs, step_receipts.rs, browser.rs, browser_agent.rs or quarry_agent.rs (all [C]). Phase 1 has a clean base.

---

## F-t2-011 · Let users browse freely: human-session policy and omnibox

### 1. CITATION CHECK
| Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|
| gateway `browser.rs:483-495` (card, json) | C | OK | `allowed_domains` = first host (486); `max_steps` 12 (485); `max_runtime_s` 120 (487). |
| `agent_routes.rs:1446` (json) | C | OK | `ttl_s = max_runtime_s.unwrap_or(120)`. |
| `agent_routes.rs:314-339`, `:326-345` (json) | C | OK | Exact equality check 326-335 (SecurityBlocked). |
| quarry-browser `lib.rs:55-77` (json), `:59-77` (card) | C | OK, body unread | `canonical_broker_domains` at 59. Empty or more-than-32 rule UNVERIFIED (body not read). |
| `browser-omnibox.ts:14` (card, json) | M | OK in live tree | The Google fallback is at 14 because an uncommitted VITE_DEMO guard sits at 12-13. HEAD has it at 12. |
| `BrowserChrome.tsx:1143-1215` (card, json) | C | OK | Toolbar 1146-1215: back 1150, forward 1159, reload "Last nettlesersiden på nytt" 1168. No hard reload control. |
| `BrowserChrome.tsx:1146` (json) | C | OK | Toolbar aria-label; omnibox input 1194. |
| `server.go:22` (json, broker) | C | OK | defaultGrantTTL 15 min. |
| `persistent_session.rs:55-110` (json) | C | DRIFTED | Registry and `reap_idle` are at 77-150 (`reap_idle` 139-150). Correct range 77-150. |
| `web-history.ts:1-60` (json) | C | OK | Header 10-12: browser-only memory of web searches and pages. Not an omnibox history. |
| `search.rs:36-70` (json) | C | OK | Router 37-49; search_web 51. |
| `search.rs:100-140` (json) | C | FALSE as stated | Lines 100-140 are inside search_web (51 to about 298). The suggest routes are router lines 41 and 44, with handlers search_suggest 406 and search_suggestions 524. |
| `agent_routes.rs:446-451` (json) | C | OK | enforce_step_budget. |
| `agent_routes.rs:2258-2300` (json) | C | OK | Domain validation 2258-2262; budget 2294-2300. |
| `agent_routes.rs:1735-1770` (json) | C | OK | Control set; Human initiator at 1766. |
| `browser.rs:255-300` (json) | C | OK | BrowserActionActor 255-259; BrowserControlMode 263-285. |
| `browser.rs:690-790` (json) | C | OK | run_action takeover check 700-723; set_control_mode 761-800. |
| `BrowserChrome.tsx:846-852` (json) | C | OK | navigateFromAddress calls `browserOmniboxTarget(address(), session().url)`. |
| `browser-omnibox.ts:1-15` (json) | M | OK | Function 4-16 in live tree. VITE_DEMO guard 12-13 is uncommitted. |
| `agent_routes.rs:1397-1412` (json) | C | OK | Broker domains override; mismatch error 1403-1404. |
| quarry-browser `lib.rs:342-352` (closest to "private-IP blocks", not cited) | C | PARTIAL | Only a loopback check on the pinned egress proxy endpoint. Private-target blocking is tested at agent_routes.rs:1967 and browser.rs:4993. |
| execution-core `browser_risk.rs:25-45`, `:277` (json reuse) | C | OK | CrossDomainNavigation 41; classification 277. |

Skipped (checked OK): `browser.rs:4986-5030` (sanitize tests, in section 4); `search.rs:717-731` (URL detection, in section 2).

### 2. AFFECTED FILES
- gateway `browser.rs` [C, 6317]: create_session 451-506 (domain and budget literals 485-487 are the change site); run_action 663-760; set_control_mode 761-800; sanitize_action 3702-3760 (navigate normalization; tests 4986-5030); `suggest_action` 1056 (AI action suggester, not the omnibox).
- `agent_routes.rs` [C, 3737]: start_run 1273-1421 (ttl 1446; policy 1397-1412); step 2228-2300; enforce_step_budget 446-451; domain_allowed 442-445; validate_run_grant 314-339; RunEntry 91-101.
- quarry-browser `lib.rs` [C, 676]: canonical_broker_domains 59; BrowserEgressPolicy 33; configure_egress_policy 445-451; loopback check 342-352.
- quarry-browser `persistent_session.rs` [C, 239]: PersistentSessionRegistry 77-150; reap_idle 139-150. The pattern to copy.
- quarry-browser `chromiumoxide.rs` [C, 3551]: CDP Fetch guard 700-775 (`domain_grant_blocked` 760); `redact_egress_url` 1053.
- model-gateway `browser_run.rs` [M, 1330]: run-policy equality 180-186. A human-browse grant kind must pass here.
- gateway `search.rs` [C, 1078]: router 37-49; search_web 51; is_url_input 717; normalize_url 731; search_suggest 406; search_suggestions 524.
- Frontend: `browser-omnibox.ts` [M, 15]; `browser-omnibox.test.ts` [C, 25]; `BrowserChrome.tsx` [C, 2172] (navigateFromAddress 846-852; toolbar 1146-1215); `src/shared/api/search-client.ts` [C, 345] (loadSearchSuggestions 78-80; suggest 176-180); `web-history.ts` [C].
- Config: quarry-edge `config.rs` [M, 501] and `state.rs` [M, 155]. No human-session field exists; a new session kind needs one.
- Migrations: none expected. No browser-session table was found.

### 3. REUSABLE BUT UNWIRED
- `PersistentSessionRegistry::reap_idle` (persistent_session.rs:139-150): no production caller. Proof: `grep -rn "PersistentSessionRegistry" "apps/Ingestion Plane/Quarry-v2" --include=*.rs --exclude-dir=.claude` returns only the re-export (`quarry-browser/src/lib.rs:168`) and the file's own unit test (220-225). The registry is never constructed in production.
- Suggest routes `/api/v1/search/suggest` and `/suggestions` (search.rs:41, 44) are wired to dashboard search (search-client.ts:78-80, 176-180; DashboardSearchPanel and its tests). They are not wired to the omnibox: browser-omnibox.ts has no import from search-client.
- Wired and reusable: `browserOmniboxTarget` (browser-omnibox.ts:4; used at BrowserChrome.tsx:81, 847); CrossDomainNavigation HITL (browser_risk.rs:41, 277).

### 4. TESTS
- Frontend: `cd "apps/Frontend Plane/verevonv3" && pnpm test -- browser-omnibox`. Four tests: explicit http URLs; host-like input; "falls back to Google search for plain queries" (line 16, pins google.com); empty input uses the current URL. Also `DashboardSearchPanel.web-history.test.tsx` and `DashboardHome.web-history.test.tsx`.
- Quarry: `cargo test -p quarry-edge agent_` with agent_navigation_blocks_private_targets_at_edge_boundary (1967), allowed_domains_accept_exact_and_subdomains_only (2051), step_budget_is_fail_closed_once_limit_is_reached (2063).
- Gateway: `cargo test --manifest-path ... sanitize_action` (browser.rs 4986-5030: sanitize_action_normalizes_navigation_urls, sanitize_action_blocks_private_navigation_targets at 4993, sanitize_action_denies_raw_script_evaluation at 5000). search.rs tests at 855-871: detects_urls_and_bare_domains, rejects_keyword_queries, normalizes_bare_domains_to_https.
- CI: verevonv3-ci.yml (`pnpm verify`; gateway `cargo test`); quarry-v2-ci.yml:205.

### 5. FEATURE FLAGS / ENV
- `VITE_DEMO=1` makes `browserOmniboxTarget` return null instead of a Google URL (browser-omnibox.ts:13; uncommitted).
- `QUARRY_EDGE__SEARCH_SUGGEST` (config.rs:166, doc comment). Default UNVERIFIED.
- `require_browser_grants` and the grant TTL (see F-t2-001). The 120 s lease (agent_routes.rs:1446) and `max_steps` 12 (browser.rs:485) are literals, not env.

### 6. SURPRISES
1. browser-omnibox.test.ts:16 pins the Google fallback. The change must rewrite that test.
2. Line 14 is correct only for the live tree (uncommitted VITE_DEMO guard). Citations for this item shift if that edit lands.
3. The omnibox never calls the suggest routes the audit says to reuse. That wiring is new work.
4. `browser.rs:1056 suggest_action` is an AI action suggester. The name collides with the search suggest routes.
5. The audit's "BrowserEgressPolicy private-IP and SSRF blocks" is imprecise. lib.rs:342-352 is a loopback check on the pinned proxy. Private-target blocks are in sanitize_action and the agent navigation tests.
6. Decision conflict: the audit's human-browse approach is "open public web minus a broker or org denylist". Open decision OD13 (`apps/Frontend Plane/verevonv3/apps/verevon-web/plans/system-audits/CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:477-479`, [C]) recommends a hard fence of the start host plus at most 8 user-listed domains, with the broker as authority. Settle OD13 first.
7. No hard-reload control exists in BrowserChrome.tsx. The single reload button (1168) navigates to the session URL.

---

## F-t2-024 · Credential handoff: executor-side fill_secret and email OTP via integration-corev2

### 1. CITATION CHECK
| Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|
| quarry-core `contracts.rs:636-700` (card), `:639-720` (json), `:640-827` (json) | C | OK, range short | AgentAction enum starts at 640: Type 675, TypeRef 679, FrameTypeRef 686, TypeSemantic 693. Sensitive variants: RespondDialog 765, UploadRef 780, DownloadRef 803, Evaluate 821. Use 640-830. |
| quarry-runtime `step_receipts.rs:185-200` (json), `:198` (card) | C | OK | StepReceipt `org_id` 188, `actor_id` 192, `action` 198. No redaction. |
| `agent_routes.rs:2346-2362` (json) | C | OK | Receipt appended when not ZDR (2360-2364). Typed text persisted verbatim for non-ZDR runs. |
| `agent_routes.rs:196-312` (json) | C | OK | sensitive_approval_for_action 144-283; validate_sensitive_approval 284-313; single-use set 101 and 2284. |
| `agent_routes.rs:2222-2290` (json) | C | OK | step() 2228; approval check 2276-2290. |
| quarry-browser `chromiumoxide.rs:1049-1060` (json) | C | OK | `redact_egress_url` 1053. "redact" hits are URL and egress sites only (1053, 1520, 1553, 2388). No value-level redaction. |
| broker `grant.go:20-130` (card), `:32-36` (json) | C | OK | allowedSensitiveActions = respond_dialog, upload_ref, download_ref. |
| `grant.go:62-69` (json) | C | OK | In-memory Store. |
| `grant.go:96-130` (json) | C | OK | CreateScopedSensitiveApproval 104-122 (parent binding). |
| IC `crypto/vault.go:1-40` (json) | C | OK, partial | 58 lines. Imports crypto/aes and crypto/cipher (4-5). GCM mode UNVERIFIED (body not read). |
| IC `api/server.go:1315-1345` (json) | M | OK | `/internal/connectors/token` at 1315; consumer checks 1321-1324. Uncommitted hunks are at 59, 213-218, 1368 and 2987-3028, all outside this route. |
| IC `config.go:335-350` (json), `:347` | M | OK | TokenLeaseConsumers default at 347 excludes quarry. Uncommitted +2 lines at 349. |
| IC `server.go:170-177` (json) | M | OK | internalAuth 170-173 uses `cfg.Config.InternalAPIKey`; codex model-plane auth 174-177. One shared key. |
| IC `actions/mail_read.go:100-175` (json) | C | OK | Query parsing (126-190). No OTP action. Case-insensitive "otp" grep in IC internal/actions: 0 hits. |
| IC `providers/catalog.go:254-256`, `:431-434` (json) | C | OK | mail.read at 254; gmail.read at 431-434. |
| execution-core `browser_agent/browser_risk.rs:25-45` (card) | C | OK | RiskCategory 25-41; Login 27; CrossDomainNavigation 41. |
| gateway `browser.rs:1190-1210` (card, json) | C | OK, comments only | 1197-1210 are comments describing the generic orchestration approvals routes. No approval code at those lines. |
| `BrowserChrome.tsx:1225-1245` (card) | C | OK for the chip only | That is the AI/Manual chip. Approval buttons: Godkjenn 1804, Avslå 1815. Evidence drawer "Godkjenninger" 2040-2043. |
| execution-core `grpc.rs:190-215` (json) | M | OK | `validate_browser_grant` 189-218. Not in an uncommitted hunk. |
| model-gateway `browser_run.rs:136-160` (json) | M | DRIFTED | resolve_browser_grant is 138-187. |
| quarry-runtime `agent_memory.rs:14` (json) | C | CLAIM-FALSE as evidence | `AgentScratchpad`, a bounded note buffer. Not a secret store. Remove from evidence. |
| `QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026_OWNERSHIP_RECONCILED.md:1106-1127` (json, DOC) | C | OK | Contains the fill_secret example at 1114-1116 (`kind` fill_secret, `secretRef` vault://connection/123/password). |

### 2. AFFECTED FILES
- `quarry-core/src/contracts.rs` [C, 973]: AgentAction 640-830 (add a fill_otp variant). Go mirror path UNVERIFIED (audit names pkg/quarrycontracts).
- quarry-edge `agent_routes.rs` [C, 3737]: sensitive_approval_for_action 144-283 (catch-all `_ => Ok(None)` at 280; a fill_otp arm goes above it); validate_sensitive_approval 284-313; step 2228-2300 (approval consume 2276-2290); receipt 2346-2364 (boolean only for OTP).
- quarry-runtime `step_receipts.rs` [C, 818]: StepReceipt.action 198 (redact before persisting, non-ZDR).
- quarry-browser `chromiumoxide.rs` [C, 3551]: execution of Type variants (not re-read); Fetch guard 700-775; redaction 1053.
- execution-core `browser_agent/browser_risk.rs` [C, 1288]: RiskCategory 25-41; deterministic classification 259-290 (Login at 288); risk_reason 466-471.
- execution-core `runtime_loop/agent.rs` [M, 13521]: browser.act schema 3479-3508 (closed set; add fill_otp only with an opaque reference); description 4209-4210.
- session-core `orchestration_store.rs` [C, 1673]: DECIDE_APPROVAL_SQL at 826- (writes decided_by, the approval card's decision).
- session-core `orchestration_grpc.rs` [M, 6215]: decided_by from the caller principal (314); validation 753-759.
- broker `grant.go` [C, 362]: allowedSensitiveActions 32-36 (add fill_otp); CreateScopedSensitiveApproval 104-122. Broker `server.go` [C, 179]: sensitive branch 81-90 (unwired).
- IC `internal/api/server.go` [M, 4256]: new executor-only route with its own identity (not the shared internalAuth key). `internal/config/config.go` [M, 1247]: TokenLeaseConsumers 347 (do not add quarry). `internal/actions/mail_read.go` [C, 765]: reuse query parsing 126-190. `internal/providers/catalog.go` [C, 1932]: capabilities at 254, 292, 298, 431-434, 486, 492. `internal/crypto/vault.go` [C, 58].
- `BrowserChrome.tsx` [C, 2172]: approval card 1804-1815; evidence drawer 2040-2043.
- Migrations: none for OTP-first (no new table). If approvals need a field, session-core next free is `0046_*.sql`.

### 3. REUSABLE BUT UNWIRED
- Quarry sensitive-approval machinery (agent_routes.rs:144-313) is wired in code, but its child grants come only from the unwired broker AcquireGrant path. In production it is unreachable until F-t2-001 lands.
- `grant.CreateScopedSensitiveApproval` (grant.go:104): unwired (see F-t2-001).
- IC mail read capabilities (Gmail and Microsoft, catalog.go 254 and 431): wired. No OTP action built.
- Model Plane `RiskCategory::Login` gate (browser_risk.rs:288): wired.
- IC `/internal/connectors/token` lease route (server.go:1315): wired; its allow-list excludes quarry.
- Approval card (BrowserChrome.tsx:1804-1815): wired and reusable.

### 4. TESTS
- quarry-core: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-core --test contracts` (tests/contracts.rs mentions grants). CI: quarry-v2-ci.yml:205.
- execution-core: `cd "apps/Model Plane/rust" && cargo test -p execution-core browser_risk` (browser_risk.rs tests 556-600, including classify_detects_login_by_selector at 595). CI: model-plane-rust-ci.yml:119.
- Quarry sensitive approvals: no test name in quarry-edge contains "sensitive" (grep). Gap.
- IC: `cd "apps/Ingestion Plane/integration-corev2" && go test ./...`. Test files UNVERIFIED. No CI workflow matched integration-corev2.
- Frontend: no BrowserChrome approval test found in `src/features/dashboard/home/` (only browser-*.test.ts). UNVERIFIED.

### 5. FEATURE FLAGS / ENV
- IC `INTEGRATION_TOKEN_LEASE_CONSUMERS` (config.go:347): default excludes quarry.
- IC `ZDR_ACKNOWLEDGEMENT_ENABLED` (config.go:349, uncommitted, default false) sits in the same file.
- IC `INTEGRATION_ALLOW_IN_MEMORY_STORE` (config.go:334, default false). IC `NATS_TOKEN` and `VEREVON_NATS_TOKEN` (config.go:342).
- IC internal key: `cfg.Config.InternalAPIKey` and header `cfg.Config.InternalAPIKeyHeader` (server.go:171-172). Env names UNVERIFIED.
- Quarry: ZDR per run. Non-ZDR receipts persist typed text (agent_routes.rs:2360).

### 6. SURPRISES
1. agent_memory.rs:14 is not evidence for this item (see section 1).
2. IC server.go (+30 lines) and config.go (+2) carry uncommitted ZDR acknowledgement and GDPR purge work. An OTP route would land in the same files: merge hotspot.
3. Quarry-side sensitive gating is unreachable in production until F-t2-001. That is a hard ordering dependency, and the audit places this item in W3 without naming it.
4. Codex "Cloud Browser" re-attribution in the card cannot be checked here; the reference evidence is outside the repo.
5. The approver identity is available: decided_by is set from the caller principal (orchestration_grpc.rs:314).

---

## F-t2-032 · Per-origin agent consent and action-class grants: typed denials, run modes, org origin policy, settings hub

### 1. CITATION CHECK
| Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|
| `agent_routes.rs:2296` (json), `:2294-2400` (json) | C | OK | Budget 2294-2295; domain-policy FORBIDDEN 2299-2300. |
| `agent_routes.rs:1106-1122` (card) | C | OK with correction | status_err (1106-1114) keeps the HTTP status (403 stays 403) and sets the typed error code to BadRequest (1111). driver_err 1116-1122. |
| `agent_routes.rs:1377-1414`, `:1397` (json) | C | OK | Missing-grant 403 at 1377-1381; broker domains override at 1397-1404. |
| `agent_routes.rs:284-340` (json) | C | OK | validate_sensitive_approval 284-313; validate_run_grant 314-339. |
| `agent_routes.rs:391-400` (json) | C | OK | BrowserExecutionTier {Chromium (default), EphemeralEvidence} 391-395; helper 397-400. |
| `agent_routes.rs:1153-1178` (json) | C | OK | require_ephemeral_evidence_action 1153. |
| `agent_routes.rs:144-300` (json) | C | OK | sensitive_approval_for_action 144-283; catch-all `_ => Ok(None)` at 280. |
| `agent_routes.rs:2276-2306` (json) | C | OK | Approval 2279; reuse CONFLICT 2287 after the set insert 2284; FORBIDDEN 2300. |
| `agent_routes.rs:98-102` (json) | C | OK | used_sensitive_approval_grants (101), process-local per the comment at 98-100. |
| `agent_routes.rs:1131-1139`, `:1396-1411` (json) | C | OK | require_isolated_agent_egress 1131; policy mismatch 1403-1411. |
| `browser_run.rs:190` (json) | M | DRIFTED | Exact equality is at 180-186. Line 190 is start_url filtering. |
| `browser_run.rs:170-195` (json) | M | OK | Equality 180-186; start_url 188-195. |
| `grant.go:32-34` (card), `:56`, `:104-119` (json) | C | OK | Allowlist 32-36 (three actions); ParentGrantID 56; CreateScopedSensitiveApproval 104-122. |
| `grant.go:307-345` (json) | C | OK | NormalizeAllowedDomains 307-331; isCanonicalDomain 332 (its rules UNVERIFIED). |
| `main.go:63` (json, broker) | C | OK | NewStore (in memory). |
| `server.go:78-95`, `:170-179` (json) | C | OK | Sensitive branch 81-90 (parent_grant_id required, 82-84); scopeURLForMode 170-179 (local or cloud). |
| `browser.proto:25-49` (json) | C | OK | No run-mode or action-class fields. |
| execution-core `browser_risk.rs:25-110` (json) | C | OK | RiskCategory 25-41; ApprovalOutcome 103-110; no remembered-approval variant. |
| `browser_events.rs:148` (json) | C | OK | `decided_by: String::new()`. |
| `llm_planner.rs:237-241` (json) | C | PARTIAL | A format! template with the goal and system_prompt (236-242). Not a static string. |
| gateway `browser.rs:263-285`, `:695-715` (json) | C | OK | BrowserControlMode 263-285; takeover check 695-715. |
| gateway `browser.rs:3702-3760` (json) | C | OK | sanitize_action 3702; evaluate denial asserted by test at 5000. |
| `BrowserChrome.tsx:1775-1830` (json) | C | OK | Godkjenn 1804; Avslå 1815 (per action only). |
| `src/features/settings/components` (json) | C | OK | No browser section. Patterns: McpServersSection.tsx and settings-ui.tsx. |
| `user-core/internal/users/types.go:136-143` (json) | C | OK | UserSettings is a category row with no revision field. |
| `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:479` (json) | C | OK | OD13 recommendation. |

Skipped (checked OK, not tabled): `quarry-core/src/error.rs:10-72` (ErrorCode 10; retryable 38-45; QuarryError.retryable 70; no decision_source); `quarry-core/src/crawl_denial.rs:12-80` (CrawlDenialReason 12, code 39, message 56); `quarry-browser/src/lib.rs:25-125` and `:445-451` (BrowserEgressPolicy 33; configure_egress_policy 445); `chromiumoxide.rs:706-775` (Fetch listener; domain_grant_blocked 760).

### 2. AFFECTED FILES
- broker `grant.go` [C, 362]: allowlist 32-36; Grant 39-59; CreateScopedSensitiveApproval 104-122; NormalizeSensitiveScopes 223-270; NormalizeAllowedDomains 307-331; isCanonicalDomain 332.
- `browser.proto` [C, 114]: AcquireGrantRequest 25-49 (add run-mode and action-class fields).
- broker `server.go` [C, 179]: AcquireGrant 42-107 (mode and parent checks 57-89).
- quarry-edge `agent_routes.rs` [C, 3737]: `status_err` 1106-1114 and `driver_err` 1116-1122 (the typed-denial choke point); validate_run_grant 314-339 (SecurityBlocked at 322, 329, 332); step 2294-2300 (FORBIDDEN domain denial); sensitive_approval_for_action 144-283; BrowserExecutionTier 391-400; require_ephemeral_evidence_action 1153-1178.
- quarry-core `error.rs` [C, 94]: ErrorCode and QuarryError (add decision source or a DenialReason).
- quarry-core `crawl_denial.rs` [C, 141]: enum template with `code()` and `message()`.
- model-gateway `browser_run.rs` [M, 1330]: run-policy equality 180-186.
- execution-core `browser_risk.rs` [C, 1288]; `permission/mod.rs` [C]; `browser_events.rs` [C, 857] (decided_by at 148); `runtime_loop/agent.rs` [M, 13521] (browser.act schema 3479-3508 and description 4209-4210); `llm_planner.rs` [C] (prompt 236-242).
- session-core `orchestration_store.rs` [C, 1673]: decision write at 826-.
- gateway `browser.rs` [C, 6317]: BrowserControlMode 263-285; control parse 2112-2117; run_action 695-715; sanitize_action 3702-3760.
- `BrowserChrome.tsx` [C, 2172]: approval buttons 1804-1815.
- Frontend settings: `src/features/settings/components/` [C]. A new Browser section is needed (none exists).
- Control Plane: `user-core/internal/users/types.go` [C, 296] (UserSettings 136-143). Migrations: `apps/Control Plane/user-core/migrations/` [C], latest `029_mail_task_addresses.up.sql`, so next `030_*.up.sql`.
- Org origin-policy object: none found by a light grep (browser_policy, allowed_sites, blocked_sites). UNVERIFIED across Control Plane.

### 3. REUSABLE BUT UNWIRED
- `CrawlDenialReason` (crawl_denial.rs:12): wired only to the crawl frontier (grep: `quarry-core/src/crawl_denial.rs` and `quarry-runtime/src/crawl_frontier.rs`). Reusable template for browser actions; unwired there.
- `QuarryError.retryable` (error.rs:70): already exists and is wired.
- `ErrorCode::SecurityBlocked`: already the code for run-grant denials (agent_routes.rs:322, 329, 332).
- `UserSettings` (user-core types.go:136-143): wired generic category rows. Reusable for per-user site rules.
- Broker `CreateScopedSensitiveApproval`: unwired (see F-t2-001).

### 4. TESTS
- quarry-core: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-core crawl_denial` (crawl_denial.rs:90 codes_are_stable_and_unique, 119 messages_include_operator_context, 131 serde_round_trip). Template tests.
- quarry-edge: `cargo test -p quarry-edge allowed_domains` (agent_routes.rs:2051) and `step_budget` (2063). No typed-denial or origin-consent test exists.
- execution-core: `cd "apps/Model Plane/rust" && cargo test -p execution-core browser_risk` (classify tests at 556-600).
- Broker: `cd "apps/Model Plane/go/services/browser-broker" && GOWORK=off go test ./internal/grant/...` (grant_test.go:80 TestNormalizeAllowedDomainsRejectsUnboundedOrNonHostPolicy). CI: none.
- Gateway: `cargo test --manifest-path ... sanitize_action` (browser.rs:5000 sanitize_action_denies_raw_script_evaluation).
- CI: quarry-v2-ci.yml:205; model-plane-rust-ci.yml:119; verevonv3-ci.yml:141.

### 5. FEATURE FLAGS / ENV
- BrowserExecutionTier is a serde enum with default Chromium (agent_routes.rs:391-395, `#[default]`). How a caller selects it is UNVERIFIED.
- `require_isolated_agent_egress` (agent_routes.rs:1131-1139) is the fail-closed egress gate. Env name UNVERIFIED.
- No org origin-policy config found (grep config.rs for origin, allow, deny: none).

### 6. SURPRISES
1. status_err keeps the HTTP status. Only the typed error code is BadRequest. The audit's "wraps a 403 as BAD_REQUEST" is accurate only for the code.
2. A typed policy code already exists (ErrorCode::SecurityBlocked) and `retryable` already exists on QuarryError (error.rs:70). The gap is decision_source and a denial catalogue, not retryable.
3. The step-level domain denial (2299-2300) is FORBIDDEN with a free-text message. The audit is right about that branch.
4. browser_run.rs:190 is the wrong cite for equality (use 180-186).
5. Uncommitted runtime_loop/agent.rs (+200, -7, mostly tests) shares the file with the browser.act schema. The schema lines (3479-3508) have no uncommitted hunk.
6. The approach's per-origin consent needs the child-grant shape to change. CreateScopedSensitiveApproval accepts only the three sensitive actions (grant.go:32-36), and validate_run_grant requires run domains to equal the grant (agent_routes.rs:326-335).
7. OD13 is still open (plan lines 477-479). The audit's "at most 8 domains" matches its recommendation.
8. The settings hub is greenfield: no browser section exists under settings/components.

---

## F-t2-036 · Durable, tenant-scoped authority and one audit record for every permission and approval decision

### 1. CITATION CHECK
| Citation (source) | Tag | Verdict | Finding |
|---|---|---|---|
| broker `grant.go:32-74` (json) | C | OK | Allowlist 32-36; Grant 39-59 (no decision fields); Store 62-70. |
| `grant.go:104-125` (json) | C | OK | CreateScopedSensitiveApproval 104-122 is the only inheritance primitive. |
| `server.go:41-107` (json) | C | OK | AcquireGrant 42-107 mints into the in-memory store (77-89). No audit emission. |
| quarry-runtime `grpc/browser_broker.rs:96-135` (json) | C | OK | acquire() 97-131; no caller. |
| gateway `browser.rs:1235`, `:1258-1263` (json) | C | OK | Interim string and its doc comment. |
| gateway `browser.rs:3702-3760` (json) | C | OK | sanitize_action; evaluate denial asserted by test at 5000. |
| `agent_routes.rs:2257-2266` (json) | C | OK | step() grant validation. |
| `agent_routes.rs:694` (json) | C | OK | browser_action_name maps Evaluate to "evaluate" (naming only). |
| `agent_routes.rs:98-102`, `:144-280` (json) | C | OK | Used-set 101 is process-local. sensitive_approval_for_action's `_ => Ok(None)` at 280 means Evaluate has no sensitive gating. Confirmed. |
| `browser_events.rs:129-151`, `:144-148` (json) | C | OK | decided() 126-152 publishes with `decided_by` empty (148). |
| `browser_events.rs:328-432` (json) | C | UNVERIFIED | Not re-read. The claim is that system:* deciders are written to the durable approval. |
| session-core `audit_publisher.rs:34` (json) | C | OK | SUBJECT_MODEL_TOOL_ACTION at 34. |
| `audit_publisher.rs:1-60`, `:604-632` (json) | C | DRIFTED | Event names are at 37 (tool_action) and 40 (tool_action_reserved). 604-632 is outbox subject-class code. Correct cite: 30-40 and 608-634. |
| session-core `orchestration_grpc.rs:753-760` (json) | M | OK | validate_approval_decision_context 753-759. Not in an uncommitted hunk. |
| session-core `retention_receipt.rs:28-34` (json) | U | OK, untracked | RUN_STARTED_ZDR_WAIVED_EVENT at 34. The file exists only as uncommitted work. |
| audit-core `subscriber.go:27-29`, `:287-295` (json) | C | OK | Subjects 27-28 (verevon.audit.v2.>, usage.v2.>). eventAuthorityMatches 287-295: a six-token subject must match payload plane, producer and event. |
| quarry-control `store.go:376-383` (json) | C | OK | Webhook has no org_id. |
| `store/pg/migrations/002_webhooks_blocklist.sql:6-12` (json) | C | OK | webhooks table has no org column. |
| `resources/webhooks.go:9` (json) | C | OK | mountSimple "/v1/webhooks" on quarry-control. |
| `dispatcher.go:25-34` (json) | C | OK | Backoff 1s, 5s, 30s, 5m, 1h (25-31); maxAttempts 5 (33). |
| `dispatcher.go:111-123` (json) | C | OK | HMAC-SHA256 over "<ts>.<body>", sent as X-Quarry-Signature (123). No dispatch-id header. |
| execution-core `runtime_loop/agent.rs:3479-3508` (json) | M | OK | browser_act_parameters_json; additionalProperties false; no URL, selector or JavaScript fields. |
| `agent.rs:4209-4212` (json) | M | OK | browser.act description at 4209-4210. |

Skipped (checked OK, not tabled): `agent_routes.rs:670-780` (browser_action_name region); `browser.rs:1258-1263` (duplicate of F-t2-001 row).

### 2. AFFECTED FILES
- broker `grant.go` [C, 362]: Store 62-70 (becomes durable); CreateScopedSensitiveApproval 104-122 (parent binding); GetScoped 172-188; RevokeScoped 189-200.
- broker `server.go` [C, 179]: AcquireGrant 42-107; RevokeGrant 110-125; ValidateGrant 128-155 (audit point for denials only, per ZDR rules).
- broker `cmd/main.go` [C, 79]: NewStore at 63 (wire the DB).
- broker `authz.go` [C, 90]: principal used as the actor on audit events.
- execution-core `browser_events.rs` [C, 857]: decided() 126-152 (fill `decided_by` from the durable record, line 148).
- session-core `audit_publisher.rs` [C, 1945]: event names 34-40; outbox subject classes 608-634. Add an AuthzDecision name.
- session-core `orchestration_grpc.rs` [M, 6215] and `orchestration_store.rs` [C, 1673]: decision write 826-; validation 753-759.
- session-core `retention_receipt.rs` [U, 1735]: ZDR event name at 34.
- Control Plane `audit-core/internal/subscriber/subscriber.go` [C, 361]: subject family checks 287-295. A new subject must be verevon.audit.v2.<plane>.<producer>.<event>.
- Quarry: no audit publisher exists. Grep `verevon.audit` in `apps/Ingestion Plane/Quarry-v2` (excluding .claude) returns 0 hits, for `.rs` and `.go`. A new publisher is [NEW]. Candidate homes: quarry-runtime `grant_validator.rs` [C, 371] and `grpc/browser_broker.rs` [C, 296].
- quarry-control `store.go` [C, 923]: Webhook 376-383 (add org_id). `resources/webhooks.go` [C, 10] (mount at line 9). `dispatcher.go` [C, 165] (backoff 25-33; signature 111-123; add a dispatch-id header [NEW]).
- Migrations: quarry-control `services/quarry-control/internal/store/pg/migrations/` latest `014_fleets.sql`, so next `015_webhooks_org_id.sql` [NEW]. Session-core `apps/Model Plane/rust/services/session-core/migrations/` latest `0045`, so next `0046_*.sql` if approvals need fields. Broker: `apps/Model Plane/go/services/browser-broker/migrations/` [NEW dir].

### 3. REUSABLE BUT UNWIRED
- Durable approvals table with `decided_by` and `decision_reason` (orchestration_store.rs:826-): wired.
- Session Core audit outbox (audit_publisher.rs 34-40; retention_receipt.rs:34): wired for tool_action, tool_action_reserved and run_started_zdr_waived. Reusable.
- Quarry-control dispatcher HMAC, backoff and DLQ (dispatcher.go:25-33, 111-123): present. Whether `dispatcher.Run` is started in main is UNVERIFIED.
- audit-core `eventAuthorityMatches` (287-295): wired in the subscriber.
- Quarry legacy `acquire()` (browser_broker.rs:97-131): unwired (no callers).
- Broker AcquireGrant: unwired in production (see F-t2-001).

### 4. TESTS
- Broker: `cd "apps/Model Plane/go/services/browser-broker" && GOWORK=off go test ./internal/grant/... ./internal/server/...`. grant_test.go TestStoreScopesCopiesAndRevokesGrants; server_test.go TestRevokeGrant_Success (158) and TestValidateGrant_RevokedReturnsFailedPrecondition (234). CI: none.
- session-core: `cd "apps/Model Plane/rust" && cargo test -p session-core`. The Postgres receipt tests run with `zdr_receipt -- --ignored --test-threads=1` (memory: needs a database). CI: model-plane-rust-ci.yml:119; the uncommitted session-core-receipts Postgres job (model-plane-rust-ci.yml, [M]) is not yet committed.
- audit-core: `cd "apps/Control Plane/audit-core" && go test ./...`. UNVERIFIED for local runs. No CI job runs it; model-plane-rust-ci.yml:36, 49 list only its testdata as path filters.
- quarry-control: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`. CI: quarry-v2-ci.yml:247 and :315 list services/quarry-control.
- user-core (for settings rows): user-service.yml covers user-core.

### 5. FEATURE FLAGS / ENV
- `BROWSER_BROKER_BASE_URL` (server.go:34); `AUTH_CORE_ISSUER` and `AUTH_CORE_JWKS_URL` (authz.go:32-33).
- ZDR tags on audit events are a data rule, not an env switch.
- quarry-control webhook secrets are per row (store.go:379). No env.
- IC NATS tokens `NATS_TOKEN` / `VEREVON_NATS_TOKEN` (IC config.go:342) apply to integration-corev2. Session Core NATS env names UNVERIFIED.

### 6. SURPRISES
1. Quarry publishes no audit events at all. The Quarry half of AuthzDecision is entirely new.
2. The audit cites retention_receipt.rs as evidence, but the file is untracked [U]. The committed tree cannot prove the event name.
3. audit_publisher.rs:604-632 is outbox classification, not an event list. The cite is imprecise.
4. The durable approval record already has `decided_by`. The "decided_by empty" gap is in the browser decided event (browser_events.rs:148), not in storage.
5. The webhook dispatch-id header and test endpoint are absent. dispatcher.go:122-123 sets only Content-Type and X-Quarry-Signature.
6. The Model Plane CI change in flight (uncommitted Postgres job in model-plane-rust-ci.yml) would give a test vehicle for session-core audit work once committed.
7. Evaluate is not sensitive-gated at the edge (catch-all at agent_routes.rs:280), and the model schema excludes JavaScript (agent.rs:3479-3508). The evaluate change is defense in depth, as the audit already says.
8. Source of the audit's "Quarry webhooks" is quarry-control, a separate Go service. Quarry-edge does not mount /v1/webhooks (webhooks.go:9 mounts on quarry-control only).

<!-- COMPLETE -->
