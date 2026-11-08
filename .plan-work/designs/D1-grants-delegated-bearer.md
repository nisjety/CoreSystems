# D1 grants-delegated-bearer: production BrowserBroker grants, delegated browser bearer, and a prod-mode contract test

Agent D1 (Sonnet), wave W1, workstream WS1. Live tree `C:/dev/CoresSystem` (HEAD afb83928 + ~324 uncommitted entries). Path tags: `[C]` committed and unchanged, `[M]` committed but modified in the live tree, `[U]` untracked, `[NEW]` created by this plan (verified `MISSING` by `tools/provenance.sh`). Line numbers are live-tree numbers read by D1 unless marked `[S1]`/`[S9]` (taken from the Haiku sweeps and spot-checked). `[UNVERIFIED]` = not run or not read; the sentence says what to check.

## 1. Items resolved and wave

| Item | Title | Wave | Resolved by |
|---|---|---|---|
| F-t2-001 | Production BrowserBroker grants and delegated bearer forwarding (unblocks every browser path) | W1 | Slices 1a, 1b, 1c below |

Three blockers the audit does not name were found while reading the code and are folded into the slices (details in section 2): **G1** Quarry has no credential the broker accepts outside dev bypass; **G5** the broker is not on the `inter-plane-bus` network Quarry lives on; **G6** Quarry's prod validator treats a revoked or expired grant as a 5xx driver error. Without these, issuance would exist and prod validation would still fail.

What F-t2-001 delivers when all three slices are done: a Model-Plane-minted, org- and owner-scoped, Postgres-backed grant that the Verevon gateway obtains for every browser session it opens, a narrower run grant derived inside the Model Plane for every AI run, the delegated `x-browser-authorization` bearer forwarded end to end, a prod-mode Quarry that boots and accepts those grants, and one contract test that fails on any 401/403 across gateway, model-gateway, execution-core and quarry-edge.

## 2. Verified current state

### 2.1 What the code does today (path:line, all read by D1)

**BrowserBroker (Go, Model Plane).**
- Store is a mutex and map: `apps/Model Plane/go/services/browser-broker/internal/grant/grant.go:61-70` [C]. Wired as `bbserver.NewServer(grant.NewStore())` at `cmd/main.go:63` [C]. No database, no migrations directory (`apps/Model Plane/go/services/browser-broker/migrations` is `MISSING`), Dockerfile has no `psql` and no entrypoint (`Dockerfile` [C]).
- `AcquireGrant` is `internal/server/server.go:42-107` [C]. Org comes from the verified principal and the request org must match (44-56). Domains are taken from the request and normalised (61-64, `NormalizeAllowedDomains` at `grant.go:307-330`: 1..32 hosts, no IP, no `localhost`, no single-label, no wildcard). Ordinary grant at 76-80, sensitive child at 81-90 (needs `parent_grant_id`). TTL is the constant at `server.go:22` (15 min). There is no `zdr` input, no grant kind, no derive path, no per-owner cap.
- `RevokeScoped` (`grant.go:189-200`) flips one record; it does not cascade to children and records no reason.
- `ValidateGrant` (`server.go:128-154`) returns `active`, expiry, domains, sensitive scopes, `parent_grant_id`. It does not return owner, kind or zdr. Revoked and expired both map to gRPC `FailedPrecondition`, unknown to `NotFound` (`errors.go`, `mapErr`).
- Authz (`internal/authz/authz.go:50-72` [C]): a **user** principal passes every method inside its own org/owner (65-67); a service principal needs `browser:write` for Acquire/Revoke and `browser:read` for Validate/Health; an unknown method is denied (63). Audience is `browser-broker` only (19, 27-28). `OwnerFilter` pins users to their own grants (85-90).
- Wire contract: `apps/Model Plane/proto/model_plane/v1/browser.proto:25-50` (AcquireGrantRequest fields 1-9), 52-69 (response 1-9), 91-114 (ValidateGrantRequest/Response 1-9) [C]. Quarry vendors a comment-stripped copy `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/proto/browser.proto` [C] (same field numbers; header says it is synced by `tools/sync-protos.sh`). Go bindings are checked in: `apps/Model Plane/go/gen/model_plane/v1/browser.pb.go`, `browser_grpc.pb.go` [C]; Rust bindings are generated at build by `mp-contracts/build.rs:45` [C] (memory note: editing the proto needs no Rust codegen; Go needs `buf generate`, buf is not installed on this host).
- Compose: `browser-broker` service block `apps/Model Plane/deploy/docker-compose.yml:1612-1629` [C] has **no `networks:` key**, so it is only on `model-plane-network`; execution-core and model-gateway reach it by `BROWSER_BROKER_ADDR` on that default network (lines 487, 1113). Quarry-edge runs in compose project `ingestion-plane` on `inter-plane-bus` (`apps/Ingestion Plane/docker-compose.yml:239` etc.). **G5: Quarry cannot resolve the broker.** Precedent for the fix: `inference-core` joins `inter-plane-bus` with aliases (`deploy/docker-compose.yml` ~990-996 [C]).
- CI: no workflow runs the broker's Go tests (`.github/workflows/*`; [S1] G2). `buf.yml` [C] runs `buf lint apps/Model Plane/proto` with an unquoted path and a 0-byte `baseline.binpb` [S9 H5], so a proto change is not actually guarded.

**Callers of AcquireGrant / ValidateGrant (grep proof, D1 re-ran 2026-10-14).**
Command (Grep tool, live tree): `AcquireGrant|acquire_grant` over `apps/**` `*.rs *.go *.ts *.tsx *.py *.proto`, excluding `target`, `node_modules`, `gen`, `_gen`, `.claude`.

| Symbol | Hit | Production caller? |
|---|---|---|
| `AcquireGrant` (server) | `browser-broker/internal/server/server.go:42` | definition |
| `AcquireGrant` (policy) | `browser-broker/internal/authz/authz.go:51,59` | definition |
| `AcquireGrant` (tests) | `server_test.go`, `latency_test.go`, `authz_test.go` | tests |
| `AcquireGrant` (mock) | `model-gateway/src/browser_run.rs:808-826` (inside `#[cfg(test)]` mock broker) | test |
| `AcquireGrant` (proto copies) | `model_plane/v1/browser.proto:13,25,52`; Quarry vendored `browser.proto:12,18,30` | contract |
| `GrpcGrantValidator::acquire` | `quarry-runtime/src/grpc/browser_broker.rs:97-131` (sends empty domains, deny-by-default) | **no caller** (Grep for `GrpcGrantValidator` shows only `main.rs:666` connect, `grpc/mod.rs:29` re-export, the file itself; no `.acquire(` on it) |
| Gateway `ModelServiceAudience::BrowserBroker` | `gateway/src/audience_tokens.rs:27,42` | declared, never invoked (`#[allow(dead_code)]` at 18) |

**Conclusion: nothing outside tests ever issues a grant.** `ValidateGrant` has three production callers, all read-only: (1) model-gateway `browser_run.rs:150` inside `resolve_browser_grant` (user bearer); (2) execution-core `grpc.rs:203` inside `validate_browser_grant` (user bearer, only for `browser_agent`, `browser.observe`, `browser.act`, gated at `grpc.rs:864-873`); (3) Quarry `GrpcGrantValidator::validate` (`browser_broker.rs:165-226`) reached through `state.grant_validator.validate` at `agent_routes.rs:288` (sensitive approval), `319` (`validate_run_grant`, called from `step` at 2258, `execute_ws_action` at 3095, `new_tab` at 3280) and `1386` (`start_run`). A fourth validator, `HttpGrantValidator` (`grant_validator.rs:140-195`), posts to `/v1/broker/grants/{id}/validate`; **no Model Plane service serves that route** (Grep over `apps/Model Plane/rust` for `broker/grants` returns nothing), so the HTTP validator can only ever return errors.

**model-gateway (Rust, Model Plane).**
- `POST /v1/browser/runs` is `browser_run.rs:297-` [M]. It requires five extractors including `VerifiedBrowserBearer` (304). The extractor returns 401 when the extension is absent (`auth.rs:427-439` [M]); the bearer is verified from `x-browser-authorization` with audience `browser-broker`, user principal, same sub/user/org as the ingress token and the same ZDR posture (`auth.rs:942-955` and `verify_delegated_user_bearer` at 972-1002). A service principal that sends the header is rejected 403 (`auth.rs:1286-1301`).
- `resolve_browser_grant` (`browser_run.rs:138-202`): validates the grant with the user's bearer, requires an active grant whose id echoes (160-163), requires a canonical broker policy (164-171), and **rejects any request `allowed_domains` that differs from the broker policy** (172-187; comment 177-179 says "a narrower policy belongs on a newly issued grant"). `start_url` must be inside the policy (188-196). The resolved grant id and domains are the only authority passed on: `tool_input` at 396-412, `x-browser-authorization` forwarded to execution-core at 467-472. ZDR browser runs are refused (307-315, hunk is uncommitted).
- There is **no HTTP route that issues a grant**. `state.browser_client` (`state.rs:132,406,576`) is the existing broker gRPC client.

**execution-core.** `validate_browser_grant` (`grpc.rs:189-218` [M], not in a hunk) resolves the opaque `grant_id` from `tool_input` with the delegated user bearer and refuses a response whose policy is empty or non-canonical (`tool_bridge/mod.rs:36-75`). It then starts its own Quarry lease with `plan.config.grant_id` and an org-scoped service token (`browser_agent.rs:669-690`, `quarry_agent.rs:554-559,574-600`). Domains in tool input are replaced by the broker's (`tool_bridge/mod.rs` test `broker_validated_policy_replaces_caller_domains`). With `QUARRY_BROWSER_AGENT_LLM` unset the loop uses the deterministic planner (Goto `start_url`, then Observe) (`llm_planner.rs:130-137`, `browser_agent.rs:809-848`), which makes the AI leg of a contract test LLM-free. **execution-core needs no code change for WS1.**

**Verevon gateway (Rust, Frontend Plane).**
- `create_session` is `domains/browser.rs:451-565` [C]. It mints a quarry-audience token (476), sets `allowed_domains` to the first host only (477, 486), `max_steps: 12`, `max_runtime_s: 120` (485-487) and POSTs `/v1/agent/runs` with **no `grant_id`** (483-511). It is the only place the gateway starts a Quarry agent run (Grep `agent/runs` over `gateway/src`).
- `build_ai_run_request` (`browser.rs:1212-1248`) sets `"grant_id": format!("session:{session_id}")` (1235) and forwards the client's `allowed_domains` (1238). `start_ai_run` (1263-1382) mints data-plane, inference, execution and session tokens (1320-1326) and calls `proxy_model_json_with_data_plane` (1350-1362); there is no browser token and `x-browser-authorization` appears nowhere in `browser.rs` ([S1], re-confirmed). The doc comment at 1258-1262 says grant issuance is unbuilt.
- `proxy_model_json_with_delegations` (`domains/chat/shared.rs:629-690` [C]) has 14 positional parameters and sets exactly the headers for data-plane, capability, inference, execution, cost, session and sandbox bearers (672-692). There is no browser parameter. Eight wrappers pass `None` positionally (375-626).
- `close_session` (`browser.rs:1023-1054`) deletes the Quarry run only.
- Quarry's owner projection carries no grant id (`agent_routes.rs:555-650`, struct fields at 555-567); the gateway deliberately keeps no process-local run state ("never inserted into `BrowserRunStore`", `browser.rs:215-235, 2709`). The SPA-facing mapping is an allowlist (`browser_response_from_owner_projection`, 2589-), so a new internal field would not leak unless mapped.
- AppState config pattern: `enforcement_mode` is parsed in `config.rs:160-165` from `CONTROL_PLANE_ENFORCEMENT` (`off|permissive|strict`, default `off`); dev escape hatches are refused when `APP_ENV` is `production|prod` (`dev_only_flag`, 419-426; `dev_flags_allowed`, 429-431) [C]. Memory note: upstream config lives on AppState, not read from env inside handlers (a handler's `std::env::var` raced under parallel tests).

**Quarry (Rust, Ingestion Plane).**
- Prod gate `quarry-edge/src/main.rs:182-201` [M]: `ENVIRONMENT=production|prod` forces `require_browser_grants`, and boot fails with no BrowserBroker endpoint configured. `start_run` returns 403 `browser grant_id is required` (`agent_routes.rs:1377-1383`), validates the grant (1384-1396), and **overwrites** client domains with the broker's canonical list only when `require_browser_grants` (1397-1412). `validate_run_grant` (314-338) requires an active grant and exact equality between the run's domains and the broker's. These are the "Quarry validates only" strengths in audit section 8; WS1 must not weaken them.
- Validator wiring `main.rs:659-672` [M]: the gRPC validator gets a bearer **only** `if cfg.cross_plane_auth_dev_bypass` (667-672); the HTTP fallback likewise (1551-1557). `apply_auth` (`browser_broker.rs:87-94`) adds no `authorization` header otherwise, and the broker rejects bearer-less calls (`authctx` interceptor; `authz_test.go:123` expects Unauthenticated). **G1: in production Quarry cannot validate any grant.**
- The registry gives quarry-edge only the `data-plane` audience: `apps/Control Plane/config/plane-service-principals.json` entry `quarry-edge` [C] (audiences `["data-plane"]`, consumerEnv `QUARRY_SERVICE_API_KEY`). Quarry's token-request manifest has six rows (vector-retrieve, ingest, web-rerank, model-invoke, model-stream, model-audio), none for browser-broker (`crates/quarry-runtime/contracts/cross-plane-token-requests.json` [M]). Quarry already has the machinery to mint short-lived per-org service tokens: `ServiceTokenAudience` (`service_tokens.rs:40-66`: DataPlane, ModelPlane), `ServiceTokenRequest::from_manifest` (89), `token_for_org` (`ServiceTokenProvider`, 326-333), and `cross_plane_tokens` is built whenever `QUARRY_SERVICE_API_KEY` is set (`main.rs:221-234`).
- auth-core can already mint `aud=browser-broker` tokens: interactive route `GET /api/browser-broker/token` (`plane-token.controller.ts:203-260`, audience registered at `convex-token.service.ts:446-455`, TTL 300 s, env `PLANE_TOKEN_TTL_BROWSER_BROKER_SECONDS`) with an empty scope list for users (`plane-token-scopes.ts:51-66`), and `POST /api/:audience/internal-token` for registered service principals, which a **directory-tenancy** principal can mint per organisation (`plane-token.controller.ts:281-360`, `assertServiceOrganizationBinding`). The broker accepts a user token with no scopes (`authz.go:65-67`).
- Gap G6: `GrpcGrantValidator::validate` maps `NotFound` to `active:false` (`browser_broker.rs:198-210`) but a revoked or expired grant arrives as `FailedPrecondition` and falls to `grpc_status_to_error` (220, 241-254) which yields `ErrorCode::DriverFailed` (a 5xx class), not a `SecurityBlocked` 403.
- HTTP fallback: `HttpGrantValidator` has no server (see above); in prod a deployment that configures only `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL` passes the boot gate (`main.rs:197-201`) and then fails every call.
- Prod stack readiness [S9 H2/H4]: `apps/Ingestion Plane/docker-compose.yml` [M] and `docker-compose.production.yml` [C] set neither `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` nor `..._URL`, and `QUARRY_EDGE__ARTIFACT_BACKEND` defaults to `memory` (`docker-compose.yml:115`), which prod bails on (`main.rs:202-211`). **As committed, a production quarry-edge cannot start.** (Case-mismatch bug at `main.rs:369-373` means `ARTIFACT_BACKEND=S3` passes the gate and selects the in-memory store; out of WS1 scope, flagged to the Ingestion owner.)

### 2.2 Audit claims checked

| Audit claim | Verdict |
|---|---|
| "no production code calls AcquireGrant" | **TRUE** (proof above). |
| "broker store is in memory" `grant.go:61-71` | TRUE. |
| "create_session posts with no grant_id; prod Quarry rejects with 403" `browser.rs:483-506`, `agent_routes.rs:1377-1412` | TRUE. Note the 403 only fires in prod mode; in dev (`require_browser_grants=false`, Noop validator) the session works today. |
| "start_ai_run sends `session:{id}` (browser.rs:1235)" | [DRIFT] it is built in `build_ai_run_request` (1212-1248), which `start_ai_run` calls. Same effect. |
| "…without the browser bearer that model-gateway's extractor requires" `auth.rs:427-437` | TRUE; the **first** failure is 401 at the extractor, the **second** (if a bearer were added) is 403 because `ValidateGrant("session:sess_1")` returns NotFound, mapped to `browser_grant_forbidden` (`browser_run.rs:204-223`). Both are [INF-by-code]; reproduce them first (slice 1a does exactly that). |
| "Only x-browser-authorization is missing from the bearer set" | TRUE for the AI path. The json item's claim that the gateway should also start forwarding `x-inference-authorization` is [CLAIM-FALSE]: it is already forwarded (`browser.rs:1323,1357`; `shared.rs:678-680`). |
| "Quarry is unchanged except for tests" | [CLAIM-FALSE]. Quarry needs a credential for the broker (G1), a validator that distinguishes inactive from error (G6), org-aware validation, and (hardening) an owner/zdr/projection field. Quarry's *enforcement logic* (exact domain equality, per-step validation) stays as is. |
| "Prefer a model-gateway HTTP endpoint wrapping AcquireGrant" | AGREED (section 3, D1). model-gateway already holds `browser_client`, the verified bearer and the org-from-claims binding. |
| "15-minute TTL caps session length until F-t2-011 adds renewal" | TRUE in effect, but note the gateway sets a 120 s lease TTL (`browser.rs:487`, `agent_routes.rs:1446`) and a lease reaper does not exist ([S1] F-t2-006); until it does, the grant TTL is the only enforced cap. |
| "user-session issuance must not bypass the CrossDomainNavigation approval gate" | Refined. That gate is a **run-loop** gate in execution-core (`browser_agent/browser_risk.rs:36-41, 262-280`): a `Goto` to a host different from the last observed host raises `CrossDomainNavigation` *even when the target is inside the grant*. It does not read the grant, so issuing a session grant cannot bypass it. The real risk is a future wide human-browse grant being inherited by a run grant, which removes the allow-list backstop; D1 prevents that by construction (section 3, D3). |
| `docs/BROWSER_WORKSPACE_PLAN.md:7` "COMPLETE" | [DOC-STALE] contradicted by `browser.rs:1258-1262`. Fixed in slice 1a DoD. |
| `apps/Ingestion Plane/Quarry-v2/docs/SELF_HOST.md:130` "Model Plane issues a grant via BrowserBroker.AcquireGrant" | [DOC-STALE] nothing calls it. SELF_HOST.md:60, RUNBOOKS.md:84 and `deploy/compose/README.md:95` name `BROWSER_BROKER_GRPC`, which no code reads [CLAIM-FALSE, S9]; the real variable is `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` (`config.rs:128-129`). |

### 2.3 Uncommitted ([M]/[U]) files in the area and conflicts with in-flight diffs

Diff hunks from `git --no-optional-locks diff -U0` against afb83928 (live tree) intersected with the ranges this plan edits.

| File | Tag | In-flight hunks (live line numbers) | Edited by D1 at | Conflict |
|---|---|---|---|---|
| `model-gateway/src/browser_run.rs` | [M] | 307-313 (ZDR refusal), 628-632 (finalization), 663-669 (control), tests 995, 1328 | struct 86-126, `resolve_browser_grant` 138-202, call 337, key hoist 348, `tool_input` 396-412, tests | None textual; adjacent to 307-313 and 337. Rebase risk low. |
| `model-gateway/src/auth.rs` | [M] | 29-37, 71-85 (Claims), 566-571, 771-854 (delegated verifiers now return `AuthRejection`) | not edited | None. D1 relies on the extractor and `verify_delegated_browser_bearer` as they are. |
| `model-gateway/src/lib.rs`, `http_routes.rs` | [M] | lib.rs 63; http_routes.rs 824+ | lib.rs top (`pub mod browser_grants;`), http_routes.rs ~227 (`.merge(...)`) | None. |
| `model-gateway/src/state.rs`, `main.rs` | [M] | state.rs 716-723; main.rs 30 | not edited | None. |
| `execution-core/src/grpc.rs` | [M] | 111-115, 229-234, 660-670, 749-766, 834-837, 899, 961-964, 1099 | not edited | None. Sibling (b) may edit it. |
| `quarry-edge/src/main.rs` | [M] | 47, 244-253, 984-1463 | 182-201 (prod gate), 659-672 (validator), 1551-1557 | None textual. |
| `quarry-edge/src/config.rs`, `state.rs` | [M] | many (rerank switches) | not edited (no new config field is needed) | None. |
| `quarry-edge/src/test_support.rs` | [M] | 126 | ~100-126 (new `test_state_with_grants`) | Adjacent hunk. |
| `quarry-runtime/contracts/cross-plane-token-requests.json` (+ `.sha256`) | [M] | 35-38 (rerank row `used_by`) | append a row at the end | **Hard hotspot**: both `.sha256` files and the auth-core vendored copy are also [M]; land after the QB-T09 commit or rebase the pins. |
| `auth-core/src/auth/fixtures/cross-plane-token-requests.json` (+ `.sha256`) | [M] | 35-38 | same row | same |
| `quarry-runtime/src/token_request_contract.rs` | [M] | 747-762 | one new row test | adjacent |
| `scripts/tests/zdr-contract-sync.test.sh` | [M] | 105-117, 198-245, 275-287, 380 | digest registered for the manifest | **Hotspot**; change the single digest line only. |
| `apps/Ingestion Plane/docker-compose.yml` | [M] | 121-154 (switch defaults 1 to 0) | quarry-edge env block ~95-135 | Adjacent; rebase. |
| `.github/workflows/model-plane-rust-ci.yml`, `verevonv3-ci.yml` | [M] | new jobs | not edited (D1 adds new workflow files instead) | None. |
| gateway `browser.rs`, `chat/shared.rs`, `config.rs`, `audience_tokens.rs` | [C] | none | all WS1 gateway edits | None in-flight; conflicts only with other workstreams (section 5). |
| broker Go tree, `proto/browser.proto`, `go/gen/*`, `deploy/docker-compose.yml`, `docker-compose.override.yml`, `docker-compose.production.yml` (Model Plane) | [C] | none | all WS1 broker edits | None in-flight. |
| `auth-core/.../plane-service-principals-policy.contract.spec.ts`, `config/plane-service-principals.json` | [C] | none | one principal, one test bundle | None. |

Untracked gate files from [S9 H1] that WS1 touches indirectly: `.github/workflows/quarry-docs-contract.yml` [U], `scripts/tests/quarry-model-plane-switches.test.mjs` [U], `apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs` [U]. The new contract tests in this plan follow their pattern but live in new files so they do not depend on those uncommitted gates.

## 3. Design decisions

Tags: **ADR-GATED** = a user or ledger decision is needed before the dependent code merges (the audit topic number from section 9.3 is named; where no audit topic fits, "new ledger entry" is said). The Model Plane ledger `apps/Model Plane/docs/decisions/ledger.md` [C] has no entry on browser grants or browser delegation ([S1] G4); the nearest precedent for a new delegated bearer is the entry at line 3043, "Sandbox lease identity: a ninth delegated user bearer, not a service token or token exchange (2026-09-11)". WS1 adds one ledger entry covering D1, D2, D5 and D6.

### D1. Issuance transport: a model-gateway HTTP endpoint wrapping the broker, not a gateway gRPC client
- Options: (a) teach the Verevon gateway the BrowserBroker gRPC contract (new tonic dependency, `BROWSER_BROKER_URL` on the gateway, proto vendoring); (b) model-gateway exposes `POST /v1/browser/grants` and `DELETE /v1/browser/grants/{grant_id}` that call `BrowserBroker.AcquireGrant/RevokeGrant` with the caller's own delegated bearer; (c) the broker grows its own HTTP front.
- Recommendation: **(b)**.
- Rationale: model-gateway already holds `state.browser_client` (`state.rs:132`), the `VerifiedBrowserBearer` extractor (`auth.rs:427-439`), the rule "org and user come from verified claims, never the body" (`browser_run.rs:328-332`) and the SSE/typed-error conventions; the gateway already speaks to model-gateway with a fixed header set and gets one more bearer (`chat/shared.rs:629-690`). The gateway gains no authority: it presents a user token that Auth Core minted for `browser-broker`, so org and owner derive from the verified principal inside the broker (`server.go:44-56`). Tag: not ADR-gated (the audit recommends it); recorded in the ledger entry.

### D2. Who mints authority: the broker alone; who derives a run grant: model-gateway inside `POST /v1/browser/runs`
- The Model Plane BrowserBroker is the only component that creates a grant record. model-gateway is a pass-through that binds `org_id` from verified claims and never from the body. The Verevon gateway never mints; it can only (i) ask model-gateway for a **session** grant with its own user's bearer and (ii) hand the session grant id back when starting an AI run.
- Options for run-grant derivation: (a) the gateway calls an issue endpoint twice (session grant, then run grant) and chooses the run's domains; (b) **model-gateway derives the run grant itself during `browser_run_start`** from the session grant the caller names, using the new broker RPC `DeriveRunGrant`; (c) execution-core derives at `ExecuteStep`.
- Recommendation: **(b)**. Rationale: the run grant must exist before the durable run is created (`browser_run.rs:334-337` validates the grant "before any thread/run write"); the gateway then never sees or chooses run authority; execution-core stays unchanged (it validates whatever grant id arrives, with the user bearer, as today). (c) would put minting authority into the executor.
- Compatibility rule (server-decided, never client-declared): if the validated grant's kind is `session`, model-gateway derives; if `run` or empty (an older broker or a direct caller), it keeps today's exact-equality path (`browser_run.rs:172-187`); if `sensitive_approval`, 400.
- Tag: ledger entry; not ADR-gated.

### D3. Grant taxonomy and parent_grant semantics (the invariant part of audit ADR topic 3)
- Three kinds, stored and returned as strings: `session` (root, issued by AcquireGrant for a user session), `run` (derived, narrower, issued by DeriveRunGrant), `sensitive_approval` (single-action child, issued by the existing AcquireGrant sensitive branch, unchanged). No `human_browse` kind is added in WS1.
- Parent rules (enforced in the broker, tested): a `run` grant's parent must be an **active `session`** grant owned by the verified principal; a `run` grant cannot be a parent of another `run` grant; a `sensitive_approval`'s parent stays "an ordinary grant with matching session key and no actions" (`grant.go:110-116`), which now means `session` or `run`; children never outlive their parent (`expires_at = min(requested, parent.expires_at)`); revoking a parent revokes its descendants (new cascade).
- A run grant's domains are a **subset** of the parent's (a child host is allowed iff it equals a parent host or is a dot-suffix of one, the same rule Quarry and execution-core already apply: `agent_routes.rs:442-445`, `browser_run.rs:265-281`). Requesting nothing inherits the parent's list. A derived grant can only narrow; there is no code path that widens.
- The wide-human-browse hazard: when WS8 (F-t2-011) later introduces a broad human-session policy, a naive derive would hand the AI that breadth. WS1 therefore fixes the rule now: **`DeriveRunGrant` with `purpose="ai_run"` rejects an inherited policy larger than `BROWSER_BROKER_MAX_RUN_DOMAINS` (default 8) unless the caller names an explicit subset.** The CrossDomainNavigation approval remains an independent execution-core gate and is never made conditional on grant contents.
- ADR-GATED, audit topic 3 ("shared human+agent sessions"): WS1 implements only the kinds above and a separate lease for AI runs (today's behaviour). Whether an AI run attaches to the *user's* session (F-t2-013 phase 2) or whether a human-browse kind exists is **not decided here**; the schema (`kind` text, `parent_grant_id`, `absolute_expires_at`) leaves room without a migration.

### D4. Where domains come from
- Rule: downstream enforcement never trusts a caller's domain list. Quarry overwrites client domains with the broker's canonical list (`agent_routes.rs:1397-1412`), execution-core replaces tool-input domains with the broker's (`tool_bridge` tests), model-gateway rejects a differing list (`browser_run.rs:180-186`).
- At issuance the request must carry *some* hosts, because the broker cannot guess what the user opened. The gateway sends exactly one host, derived server-side from the URL it already validated with `normalize_public_http_url` (`browser.rs:457-462, 477`), never a client-supplied list. The broker then normalises (`NormalizeAllowedDomains`), caps the count for `session` grants (`BROWSER_BROKER_MAX_SESSION_DOMAINS`, default 8, reflecting OD13's "start host plus at most 8" from `CHAT_GAP_CLOSURE_PLAN_2026-10-01.md:477-479` [C]), and applies a policy hook `PolicyResolver.Resolve(org, owner, requested) -> allowed` whose default implementation is a static deny-list (`.internal`, `.local`, `.localhost` and cloud metadata names). IP literals and single-label hosts are already refused by `isCanonicalDomain` (`grant.go:332-354`); a session on a public IP-literal URL will therefore get a typed `browser_host_not_grantable` 400 in `strict` mode (documented regression, previously it only failed in prod).
- The SPA-supplied `allowed_domains` on `POST .../ai-runs` (`StartAiRunBody`, `browser.rs:1238`) stays accepted but its meaning changes to a **narrowing request** passed to `DeriveRunGrant`; the broker rejects anything outside the parent.
- ADR-GATED, audit topic 1 ("web policy of record"): the `PolicyResolver` interface is the owner-neutral hook. Which plane owns org web policy (org-core claims vs capability-core grant fields) is not decided here; WS1 ships only the static default.

### D5. How Quarry authenticates to the broker in production (G1)
- Options: (a) register `quarry-edge` as a service principal for audience `browser-broker` with scope `browser:read` and mint a per-org token for every validation, using the existing manifest-driven token machinery; (b) forward the caller's user bearer (`x-browser-authorization`) to Quarry; (c) make grants self-contained signed tokens that Quarry verifies offline.
- Recommendation: **(a)**. Rationale: (b) fails for the AI path (execution-core calls Quarry with its own org-scoped service token, and the user bearer lives 300 s while a run and a grant live up to 15 min); (c) loses revocation and is a different security model. (a) reuses `ServiceTokenAudience`/`from_manifest`/`token_for_org` (`service_tokens.rs`), keeps the "a token request that is not a manifest row cannot be built" rule, and gives the broker a read-only caller: `browser:read` only validates, it cannot acquire, derive or revoke. The registry row follows the existing P1 bundle pattern (`data:rerank` for quarry-edge, `plane-service-principals-policy.contract.spec.ts:1496-1560`).
- Registry delta: `quarry-edge.audiences += "browser-broker"`, `scopesByAudience["browser-broker"] = ["browser:read"]`, `retentionByAudience["browser-broker"] = "zdr"` (the token carries no retained content; precedent: execution-core's `quarry` lane is `zdr`), top-level `scopes += "browser:read"`. **No service principal receives `browser:write` on `browser-broker`** (a contract-spec assertion), so only a verified user bearer can create or revoke a grant.
- Owner binding as defence in depth: because the service token is org-scoped but not owner-scoped, `ValidateGrantResponse` gains `owner_id` and Quarry checks `grant.owner_id == claims.actor_id()` when the caller is a **user** principal (the gateway's session path). Service callers (execution-core acting for the user) are bound by org plus the owner-pinned validation execution-core already performed with the user's bearer at `ExecuteStep`.
- ADR-GATED (new ledger entry; closest audit topic 3, because it defines how a grant binds across actors) and **needs Control Plane owner sign-off** because it edits the principal registry. It is the only decision in WS1 that blocks merging code (the Quarry credential), see section 8.

### D6. ZDR: grants are never persisted; single-replica consequence
- Rule: a grant is ZDR if the request says `zdr=true` **or** the verified token's `zdr` claim is true (`authctx.Principal.ZeroDataRetention`, `authctx.go:36,157-159`). OR-only; a body can never lower it. ZDR grants live in the broker's process memory only; their ids carry the prefix `z_` so `ValidateGrant`/`Revoke` route by prefix to the memory repository and a forged `z_` id can never reach Postgres. Postgres has **no zdr column** (a contract test asserts this).
- Derivation: a run grant's ZDR equals its parent's. If the principal's token says ZDR but the parent is non-ZDR (the org flipped posture mid-session), derive fails with `FailedPrecondition` "zdr posture changed" and the user reopens the session.
- Consequence to record: a ZDR grant is lost when the broker restarts, so the next Quarry step sees `NotFound` and the session ends; and the broker must run as a **single replica** while ZDR grants exist (a second replica would not see them). Today it is one container. model-gateway already refuses ZDR AI runs (`browser_run.rs:307-315`), so only ZDR *sessions* (gateway create_session with `zdr:true`) rely on this.
- Logging rule (ZDR telemetry, F-t2-004): broker, model-gateway and gateway never log domain lists or URLs for grant calls; grant ids may be logged. Broker metrics keep attribute `outcome` only (`telemetry.GrantsIssuedTotal`, `server.go:92,95`).
- ADR-GATED, audit topic 4 ("ZDR semantics per feature"): consistent with the existing "refused or ephemeral" strictness, so it proceeds; the choice "ephemeral-only, single replica" is written into the ledger entry.

### D7. Postgres store placement and migration mechanics
- Follow sandbox-manager (S3.2/S3.3, the newest Model Plane Go store): migrations are `NNNN_name.up.sql` / `.down.sql` files under `<service>/migrations/` (`sandbox-manager/migrations/0001..0004` [C]), applied by a `docker-entrypoint.sh` psql loop on every start (so every statement must be re-runnable: `IF NOT EXISTS`), a memory fallback only when `*_ALLOW_EPHEMERAL_DEVELOPMENT=true`, otherwise refuse to start without `DATABASE_URL` (`sandbox-manager/cmd/main.go:42-47`), and integration tests behind `//go:build integration` with testcontainers (`lease_integration_test.go`). Correction to [S1]: use `0001_browser_grants.up.sql` and `.down.sql`, not a single `0001_browser_grants.sql`.
- Database: the shared `session_core` Postgres with the `postgres` role, like cost-core, letta-bridge and sandbox-manager (`deploy/docker-compose.yml` `DATABASE_URL: postgresql://postgres:...@postgres:5432/session_core`). Tables are prefixed `browser_` and nothing else reads them; this is Model Plane inside Model Plane, so no cross-plane database access. A dedicated database plus least-privilege role is a hardening follow-up, listed in section 9 only as a preference.

### D8. Gateway fail-mode flag and dev default
- `BROWSER_GRANT_ENFORCEMENT` on `AppState` (not read in handlers): `off | permissive | strict`. Default: **`permissive` when `APP_ENV` is not production/prod, `strict` when it is**; an explicit `off` in production is refused and treated as `strict` (log line in the style of `dev_only_flag`, `config.rs:419-426`). `strict`: no grant means no session (503 `browser_grant_unavailable`). `permissive`: try, and on broker/model-gateway failure continue without a grant (warn). `off`: today's behaviour, for stacks without a broker.
- Rationale: dev keeps working against a Quarry without `require_browser_grants`; prod can never silently run grant-less.

### D9. Revocation and lifetime
- Session close revokes the session grant (cascade to run grants). Stopping an AI run does **not** revoke its run grant: the run grant is TTL-bounded (<= parent) and the user bearer used to mint it is gone by then; adding revoke-on-stop needs a run-to-grant map that does not exist (candidate owner: execution-core `StateStore` or session-core run metadata; left to F-t2-036). Documented limitation, not a regression.
- TTL stays 15 min but becomes `BROWSER_BROKER_GRANT_TTL_SECONDS` (default 900, clamp 30..3600) so the contract test can exercise expiry in seconds.
- **Renewal plug-in point for WS8 (F-t2-011):** WS1 adds the column `absolute_expires_at` and the repository method `Renew(ctx, id, orgID, ownerID, newExpiry)` signature (memory and Postgres implement it, unexported from the RPC surface). WS8 adds `rpc RenewGrant(RenewGrantRequest) returns (RenewGrantResponse)` (names reserved here: request `grant_id=1`, response `expires_at=1`), a model-gateway route `POST /v1/browser/grants/{grant_id}/renew`, and a gateway keep-alive at half TTL while the session is active. Rules reserved for it: renew only `session` grants, only for the owner, never past `absolute_expires_at` (default 8 h), never for ZDR beyond process lifetime, run grants are re-derived from the renewed parent rather than renewed.

### D10. Contract-test shape
- A live cross-service test cannot be in-process: gateway, model-gateway/execution-core, quarry-edge and the Go broker are three Cargo workspaces plus a Go module, and auth-core issues the tokens. Decision: **two layers**. (L1) a hermetic, CI-gated source/manifest contract (`node --test`) that pins the wire facts each hop depends on (headers, audiences, scopes, registry rows, proto field numbers, compose posture, "no zdr column"); (L2) a live prod-mode stack gate (Playwright API spec plus a log-scan wrapper) that is the deployment evidence the repo requires and runs pre-merge on a developer host or a self-hosted runner. Details in slice 1a.

### D11. Proto compatibility and field-number allocation
- All proto changes are additive. Allocation proposed to the integrating architect so WS1 does not collide with F-t2-032 (action-class grants) and F-t2-008 (budgets carried in the grant): numbers 10-19 are WS1 (zdr, kind, owner_id), 20-39 reserved for F-t2-032, 40-49 for F-t2-008 budgets. `reserved` statements are not added (they block everyone); the allocation is a convention recorded in the ledger entry.
- Go bindings are regenerated with `buf generate` through the `bufbuild/buf` Docker image because buf is not installed on the host (memory note); Rust bindings regenerate at build. Quarry's vendored copy needs no update for new RPCs; sync it for the three new `ValidateGrantResponse` fields so Quarry can read them.

## 4. Slices (PR-sized, in build order)

Order: **1a** (contract harness + issuance on the in-memory store + Quarry credential, network and registry) then **1b** (derive API, gateway wiring, bearer forwarding) then **1c** (Postgres). 1a and 1b each fit one to two PRs per plane; the plane split inside a slice is given so two engineers can work in parallel. Slice names below use "PR-a/b/c" for the suggested PR cuts.

---

### Slice 1a: prod-mode contract harness, issuance on the in-memory store, Quarry-to-broker credential

**Goal.** Make a production-mode Quarry accept a broker-issued grant end to end for the first time, prove it with a test that fails on any 401/403, and reproduce (not assume) the AI-path failure the audit inferred. Issuance here uses the existing in-memory store; nothing in this slice needs Postgres or the gateway.

**Why it comes first.** Audit section 10.3 says every "fails in production" statement is inference. 1a turns it into a recorded failing test (legs B and C marked expected-failure) before 1b fixes it.

#### 1a.1 Change list

Broker (Go, Model Plane) `apps/Model Plane/go/services/browser-broker`
| Path | Tag | Symbol / change |
|---|---|---|
| `internal/grant/grant.go` | [C] | `Grant` gains `Kind string`, `Zdr bool`, `AbsoluteExpiresAt time.Time` (zero = unset, reserved for WS8). Constants `KindSession="session"`, `KindRun="run"`, `KindSensitiveApproval="sensitive_approval"`. New `Repository` interface (ctx-first): `CreateSession`, `CreateScopedSensitiveApproval`, `Derive` (signature only; implemented in 1b), `GetScoped`, `RevokeScoped(ctx,id,org,owner,reason)` with **cascade to descendants**, `CountActive(ctx,org,owner)`, `Renew` (reserved, WS8). `Store` (memory) implements it; the current no-ctx methods stay as thin wrappers so `grant_test.go` and `server_test.go` compile unchanged in PR-a, then migrate. `newID(zdr)` returns `z_`+32 hex for ZDR, bare 32 hex otherwise. `NormalizeAllowedDomains` unchanged (`grant.go:307-330`). |
| `internal/grant/policy.go` | [NEW] | `PolicyResolver` interface and `StaticPolicy` (max session domains from `BROWSER_BROKER_MAX_SESSION_DOMAINS`, default 8; deny-list `.internal`, `.local`, `.localhost` and cloud metadata hostnames). The hook ADR topic 1 will later implement. |
| `internal/server/server.go` | [C] | `Server.grants` becomes `grant.Repository`. `AcquireGrant` (42-107): reads `req.GetZdr()`, `zdr := req.Zdr \|\| principal.ZeroDataRetention`; calls the policy; per-owner active cap `BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER` (default 20) returning `ResourceExhausted`; sets `Kind` (`session` when no actions, `sensitive_approval` otherwise); TTL from `BROWSER_BROKER_GRANT_TTL_SECONDS` (default 900, clamp 30..3600) replacing the constant at 22. `ValidateGrant` (128-154) now returns `Kind`, `OwnerId`, `Zdr`. `RevokeGrant` (110-125) passes `req.GetReason()` and cascades. |
| `internal/server/errors.go` | [C] | `ErrTooManyGrants` to `ResourceExhausted`; `ErrPolicyDenied` already maps to `PermissionDenied`; (1b adds `ErrDeriveWidensParent`, `ErrZdrPostureChanged`). Fix `mapErr` so `grant.ErrInvalidDomainPolicy` maps to `InvalidArgument` instead of falling to `Internal`. |
| `internal/authz/authz.go` | [C] | No change in 1a. (1b adds the DeriveRunGrant method case.) |
| `cmd/main.go` | [C] | Read the new env vars; log the effective TTL and caps (no domains). |

Contract (proto) `apps/Model Plane/proto/model_plane/v1/browser.proto` [C]; regenerate `go/gen/model_plane/v1/browser.pb.go`, `browser_grpc.pb.go` [C]
| Message | New field | Number |
|---|---|---|
| `AcquireGrantRequest` | `bool zdr` | 10 |
| `AcquireGrantResponse` | `string kind`, `bool zdr` | 10, 11 |
| `ValidateGrantResponse` | `string kind`, `string owner_id`, `bool zdr` | 10, 11, 12 |

Additive only. Quarry's vendored copy `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/proto/browser.proto` [C] gets the three `ValidateGrantResponse` fields in the same PR (Quarry needs `kind`/`owner_id`/`zdr` in 1b). Fix the CI guard in the same PR: `.github/workflows/buf.yml` [C] quote the path and commit a real `baseline.binpb` (`buf build -o baseline.binpb` from main before the proto edit) because the 0-byte baseline makes `buf breaking` inert ([S9] H5).

model-gateway (Rust) `apps/Model Plane/rust/services/model-gateway/src`
| Path | Tag | Symbol / change |
|---|---|---|
| `browser_grants.rs` | [NEW] | `router()` with `POST /v1/browser/grants` and `DELETE /v1/browser/grants/:grant_id`. Extractors: `Extension<Claims>`, `VerifiedBrowserBearer`. `IssueBrowserGrantRequest { allowed_domains: Vec<String>, mode: Option<String>, zdr: bool }`; `org_id` is taken from `claims.org_id` only; `session_key = format!("ui-{}", new_ulid())`. Calls `state.browser_client.acquire_grant` through `authenticated_browser_request` (the helper at `browser_run.rs:745-762`, made `pub(crate)`). Response `{ "grant_id", "session_key", "kind", "expires_at", "allowed_domains", "zdr" }`. Broker code mapping: InvalidArgument 400 `browser_host_not_grantable`, PermissionDenied 403, ResourceExhausted 429, Unavailable/DeadlineExceeded 503, else 502. DELETE calls `revoke_grant` with reason `session_closed`; broker NotFound returns 200 `{ "revoked": false }` (idempotent). Never logs `allowed_domains`. Allowed under ZDR (it persists nothing). |
| `lib.rs` | [M] | `pub mod browser_grants;` near `pub mod browser_run;` (line 8). |
| `http_routes.rs` | [M] | `.merge(crate::browser_grants::router())` beside `.merge(crate::browser_run::router())` (~227). |
| `browser_run.rs` | [M] | Make `authenticated_browser_request` and `browser_grant_status_to_http` `pub(crate)`. No behaviour change in 1a. |

Quarry (Rust, Ingestion Plane) `apps/Ingestion Plane/Quarry-v2/crates`
| Path | Tag | Symbol / change |
|---|---|---|
| `quarry-runtime/src/service_tokens.rs` | [C] | `ServiceTokenAudience::BrowserBroker` (endpoint `/api/browser-broker/internal-token`, registry name `browser-broker`), added to `from_registry_audience` (line 62-65). |
| `quarry-runtime/contracts/cross-plane-token-requests.json` | [M] | New row `browser-grant-validate`: audience `browser-broker`, `mint_path` `/api/browser-broker/internal-token`, scopes `["browser:read"]`, reason `validate broker browser grants for run authority`, `granted: true`, `used_by ["crates/quarry-runtime/src/grpc/browser_broker.rs"]`, `feature_flags ["QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL"]`. Refresh `cross-plane-token-requests.json.sha256`, the auth-core vendored copy and its `.sha256`, and the digest registered in `scripts/tests/zdr-contract-sync.test.sh` (the file's own description lists these steps). |
| `quarry-runtime/src/grant_validator.rs` | [C] | `GrantValidator` gains `async fn validate_for_org(&self, org_id: &str, grant_id: &str)` with a default body calling `validate` (Noop and Http keep working). `GrantValidation` gains `kind: String`, `owner_id: String`, `zdr: bool` (all `#[serde(default)]`). |
| `quarry-runtime/src/grpc/browser_broker.rs` | [C] | `GrpcGrantValidator` auth becomes an enum `GrantAuth { None, Static(String), ServiceTokens(SharedServiceTokenProvider) }`; `with_service_tokens(provider)`; `validate_for_org` mints `token_for_org(org_id, &ServiceTokenRequest::from_manifest("browser-grant-validate")?, false)` per call (the provider caches and refreshes), retries once on a broker `Unauthenticated` with `force=true`. **G6 fix:** in the `match` at 182-221 treat `tonic::Code::FailedPrecondition` like `NotFound` (return `active:false`) so a revoked or expired grant becomes `SecurityBlocked` 403, not a 5xx. Map the three new response fields. |
| `quarry-edge/src/main.rs` | [M] | Validator wiring (659-672): `with_service_tokens(cross_plane_tokens.clone())` when `cross_plane_tokens` is `Some` and not dev bypass; keep the dev-bypass static bearer. Prod gate (182-201): bail if `require_browser_grants` and the gRPC validator is configured but `cross_plane_tokens` is `None`; **bail if only the HTTP validator URL is configured in production** (`HttpGrantValidator` has no server, section 2.1). |
| `quarry-edge/src/agent_routes.rs` | [C] | Pass the org to validation: `validate_run_grant(state, org_id, grant_id, run_domains)` (314-338) and the calls at 2258, 3095, 3280; `validate_sensitive_approval` (288) and `start_run` (1386) call `validate_for_org(&claims.org_id, ..)`. No enforcement logic changes. |
| `quarry-edge/src/test_support.rs` | [M] | `test_state_with_grants(validator: Arc<dyn GrantValidator>, require: bool)` beside the existing Noop state (lines ~100-126; adjacent in-flight hunk at 126). |
| `quarry-edge/tests/browser_grants_prod.rs` | [NEW] | First tests ever with `require_browser_grants: true` (gap G3, [S1]): see tests below. |
| `quarry-runtime/src/token_request_contract.rs` | [M] | Row test for `browser-grant-validate` (call-site scan finds the `from_manifest("browser-grant-validate")` literal). |

Control Plane
| Path | Tag | Symbol / change |
|---|---|---|
| `apps/Control Plane/config/plane-service-principals.json` | [C] | `quarry-edge`: audiences `["data-plane","browser-broker"]`; `scopesByAudience["browser-broker"]=["browser:read"]`; `retentionByAudience["browser-broker"]="zdr"`; top-level `scopes` adds `browser:read`. Requires the launcher to re-emit the registry (`scripts/run-control-plane.sh`, `plane_principals_registry`), then an auth-core rebuild and restart. |
| `apps/Control Plane/auth-core/src/auth/plane-service-principals-policy.contract.spec.ts` | [C] | New describe "principal bundle P2: browser:read for quarry-edge" in the shape of P1 (lines 1496-1560): held by quarry-edge alone, on browser-broker alone; mints exactly that scope as `principalType: 'service'` for a directory org; refused on every other audience and to every other principal; **no principal holds `browser:write` on `browser-broker`**. |

Deploy
| Path | Tag | Symbol / change |
|---|---|---|
| `apps/Model Plane/deploy/docker-compose.yml` | [C] | `browser-broker` (1612-1629): add `networks: { default: {}, inter-plane-bus: { aliases: [model-plane-browser-broker-1, browser-broker] } }` (pattern of inference-core, ~990-996) and `BROWSER_BROKER_GRANT_TTL_SECONDS: ${BROWSER_BROKER_GRANT_TTL_SECONDS:-900}`. **G5 fix.** |
| `apps/Ingestion Plane/docker-compose.yml` | [M] | quarry-edge env: `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: ${QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL:-}` (empty keeps dev unchanged). In-flight hunks at 121-154: rebase. |
| `apps/Ingestion Plane/docker-compose.production.yml` | [C] | require it: `${QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL:?...}` (fixes [S9] H2 for the grant half; the artifact-backend half is the Ingestion owner's). |
| `apps/Ingestion Plane/docker-compose.grant-contract.yml` | [NEW] | Test-only overlay applied after `docker-compose.yml` (not the production overlay, which hides ports with `!reset` and needs release metadata): `ENVIRONMENT: production`, `QUARRY_EDGE_AUTH_DEV_BYPASS: "0"`, `QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS: "0"`, `QUARRY_EDGE__ARTIFACT_BACKEND: filesystem` with a volume on `/app/data/artifacts`, `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: http://browser-broker:9095`. |

Docs: correct `docs/BROWSER_WORKSPACE_PLAN.md:7` (status line), `apps/Ingestion Plane/Quarry-v2/docs/SELF_HOST.md:60,130`, `docs/RUNBOOKS.md:84`, `deploy/compose/README.md:95` (`BROWSER_BROKER_GRPC` is not a real variable) and register them in `apps/STALE_DOC_DELETION_REGISTER.md` per audit open question 15.

CI (new files only, so the in-flight workflow edits are not touched)
| Path | Tag | Content |
|---|---|---|
| `.github/workflows/browser-broker-ci.yml` | [NEW] | `cd apps/Model Plane/go/services/browser-broker && GOWORK=off go vet ./... && go test ./...` on changes under `apps/Model Plane/go/services/browser-broker/**`, `proto/model_plane/v1/browser.proto`, `go/gen/**`, `go/pkg/authctx/**` (closes gap G2 for this service). 1c adds the Postgres integration job here. |
| `.github/workflows/browser-grant-contract.yml` | [NEW] | `node --test scripts/tests/browser-grant-contract.test.mjs` plus `bash scripts/tests/zdr-contract-sync.test.sh`; triggers on the registry, manifest, proto, gateway, compose and test paths. |

#### 1a.2 Contract test (decision: where, how, what)

**Where it lives.** Two files plus a wrapper, deliberately outside any single plane because it asserts a cross-plane contract (the repo's own precedent: `scripts/tests/zdr-contract-sync.test.sh`, `quarry-model-plane-switches.test.mjs`):
1. L1 hermetic, CI-gated: `scripts/tests/browser-grant-contract.test.mjs` [NEW] (node:test, reads files, no network).
2. L2 live prod-mode: `apps/Frontend Plane/verevonv3/tests/e2e/browser-grants-prod-contract.spec.ts` [NEW] (Playwright API `request` fixture, no browser rendering; reuses `tests/e2e/auth.setup.ts` storage state and the `local` project of `playwright.config.ts`, `testDir: tests/e2e`) and the wrapper `scripts/tests/browser-grant-prod-contract.sh` [NEW].

**How it boots a prod-mode Quarry (no dev bypass).** The wrapper documents and, with `--boot`, runs the repo's own recipes, in order:
1. Control Plane: `cd "apps/Control Plane" && bash scripts/run-control-plane.sh up -d --no-deps --build auth-core` (registry change; memory note: never bare `docker compose build`).
2. Model Plane: `BROWSER_BROKER_GRANT_TTL_SECONDS=45 bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build browser-broker model-gateway` (execution-core is unchanged). The short TTL is for the expiry assertion only.
3. Ingestion Plane, with the memory-note env-file chain (`C:/dev/CoresSystem/.env`, Control and Application `.env.generated-secrets`, `Quarry-v2/.env`, the core `.env` files, `apps/Ingestion Plane/.env.generated-secrets` last): `docker compose -p ingestion-plane <chain> -f "apps/Ingestion Plane/docker-compose.yml" -f "apps/Ingestion Plane/docker-compose.grant-contract.yml" up -d --no-deps --build quarry-edge`. `make up` is not used: it passes only two env files and leaves required secrets empty ([S9] 2.4).
4. Gateway (slice 1b onward): image build and `python3 C:/dev/tools/swap_gateway_image.py` (memory note).
5. Posture is **proved, not read**: the spec asserts that a user-token `POST /v1/agent/runs` straight to Quarry without `grant_id` returns 403 `browser grant_id is required` (`agent_routes.rs:1377-1383`). If that returns 200, the stack is not in prod mode and the whole run fails with "not prod mode".
First task of the slice is a 0.5-day **boot spike** (section 9 risk): start quarry-edge with the overlay and record every boot bail (`main.rs:132-215`, durable gate 694-708) because [S9] H2/H4 predict failures that no one has run.

**What it asserts** (every HTTP status is checked; any 401 or 403 outside the labelled negative controls fails the run):

*Leg A: hop contracts (green at the end of 1a).* Tokens are minted from auth-core with the test user's session cookie plus the internal key from the environment (never logged).
- A1 `GET /api/browser-broker/token` and `/api/model-plane/token` return 200.
- A2 model-gateway `POST /v1/browser/grants {allowed_domains:["example.com"]}` returns 200; `grant_id` is 32 hex; `kind=="session"`; `allowed_domains==["example.com"]`; `0 < expires_at-now <= TTL`.
- A3 Quarry `POST /v1/agent/runs {grant_id, constraints:{allowed_domains:["evil.example"]}}` with the aud=quarry user token returns 200 and a `run_id`.
- A4 Quarry `/step` navigate `https://example.com` returns 200 (observation url host `example.com`). Navigate `https://evil.example` returns the domain FORBIDDEN class: **domains came from the broker, not the client**.
- A5 *(neg)* Quarry start without `grant_id` returns 403 `browser grant_id is required`; forged id returns 403; revoke via model-gateway `DELETE` then `/step` returns 403 (G6 fix: not 5xx); after the 45 s TTL `/step` returns 403.
- A6 *(neg, if `E2E_SECOND_USER_STORAGE` is set)* another user's grant id returns 403/404.
- A7 broker log scan: no domain strings in broker, model-gateway or gateway logs for the window (ZDR telemetry rule).

*Leg B: gateway session (red/expected-fail in 1a, green in 1b).* `POST /api/v1/browser/sessions {url:"https://example.com"}` returns 200 with no `grantId` in the body; `GET /api/v1/browser/sessions/{id}` 200; `DELETE` 200 and a later Quarry `/step` with that grant returns 403 (session close revoked it).

*Leg C: AI run through the gateway (red in 1a, green in 1b).* `POST /api/v1/browser/sessions/{id}/ai-runs {goal, max_steps:3}` returns 200 with a `run_id`; the run-event replay `GET /api/v1/runs/{run_id}/events/replay` (or the SSE tail) contains `BrowserActionDispatched` and `BrowserObservationReceived` (execution-core reached Quarry with the derived grant); no event reason matches `401|403|PermissionDenied|Unauthenticated|not authorized`. The deterministic planner (no `QUARRY_BROWSER_AGENT_LLM`) makes this LLM-free. Negative: `allowed_domains:["evil.example"]` in the request returns 4xx from model-gateway (not a subset).

*Per-hop log scan.* After the happy-path legs, before the negative controls: `docker logs --since <T0> <container>` for gateway, model-gateway, execution-core, quarry-edge, browser-broker is grepped for `\b40[13]\b`, `PermissionDenied`, `Unauthenticated`, `browser grant is not authorized`, `browser grant_id is required`. Any hit fails the run and names the hop. Negative controls run last so they cannot pollute the window.

In 1a, legs B and C are written with Playwright `test.fail()` annotations and a comment holding the expected failure text (B: forwarded 403 from Quarry; C: 401 at model-gateway's `VerifiedBrowserBearer`, then 403 on `session:{id}` once a bearer is added). A `test.fail()` that unexpectedly passes fails the suite, so 1b cannot forget to remove it. Because gateway `ai-runs` only needs a Quarry run the user owns, leg C in 1a uses the run created in leg A.

*Constraint to state plainly:* the broker requires dotted, non-IP hostnames and Quarry's SSRF guard blocks private targets, so a local fixture site cannot be used; the live gate needs outbound HTTPS to `example.com` (an IANA-reserved host). L2 is therefore a pre-merge evidence gate on a developer or self-hosted host, not a GitHub-hosted CI job. L1 is the CI gate.

**L1 `browser-grant-contract.test.mjs` assertions** (each is a failing-test-first rule): (1) registry: `quarry-edge` holds `browser-broker`/`browser:read` and **no principal holds `browser:write` on `browser-broker`**; (2) manifest row `browser-grant-validate` is `granted:true` and byte-identical in auth-core's vendored copy (sha256 match); (3) proto field numbers: `AcquireGrantRequest` 1-10, `ValidateGrantResponse` 1-12, `DeriveRunGrantRequest` 1-6 (after 1b), no number reused or renamed; (4) gateway: `chat/shared.rs` contains the literal `x-browser-authorization`; `browser.rs` calls `required_browser_token` (1b) and contains no `format!("session:` (1b); (5) **single minting path**: the only non-test source hits for `acquire_grant(` in `apps/Model Plane/rust`, `apps/Frontend Plane/verevonv3/apps/gateway/src` and `apps/Ingestion Plane/Quarry-v2/crates` are `model-gateway/src/browser_grants.rs` and, until slice 1b deletes it, the legacy `GrpcGrantValidator::acquire` definition (zero callers); the gateway has zero; (6) compose: broker block lists `inter-plane-bus`; production overlay requires the validator URL; the contract overlay sets prod flags; (7) (1c) the grants migration has no `zdr` column.

#### 1a.3 API / proto / data-model / migration changes
Proto fields above; model-gateway routes above; Auth Core registry row above; manifest row above; **no SQL in 1a**.

#### 1a.4 Feature flags, rollout, rollback
- No behaviour flag in the broker for issuance itself; new env vars `BROWSER_BROKER_GRANT_TTL_SECONDS`, `BROWSER_BROKER_MAX_SESSION_DOMAINS`, `BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER` have safe defaults.
- Quarry: `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` empty in dev (Noop, as today); set in prod and in the contract overlay.
- Deploy order: broker, model-gateway, auth-core registry (**the validator cannot mint until auth-core serves the new registry**; Quarry's manifest comment for the rerank row describes the same ordering and `apps/Data Plane v2/scripts/qb-preflight.sh` is the precedent for a live preflight), then quarry-edge. Every change is additive; rollback is redeploying the previous image of each service; the registry row can be removed without effect on other audiences.
- Dev mode is unaffected: nothing in 1a changes the gateway or the non-prod Quarry path.

#### 1a.5 Tests
Unit (host):
- Broker: `cd "apps/Model Plane/go/services/browser-broker" && GOWORK=off go test ./...` (memory notes: per-service, `-race` needs cgo, unavailable). New: `TestAcquireGrant_ZdrFromRequestOrToken_UsesEphemeralIdPrefix`, `TestAcquireGrant_ZdrCannotBeLoweredByRequest`, `TestAcquireGrant_PerOwnerActiveCap`, `TestAcquireGrant_PolicyDeniesInternalSuffix`, `TestValidateGrant_ReturnsKindOwnerAndZdr`, `TestRevokeGrant_CascadesToSensitiveChildren`, `TestMapErr_InvalidDomainPolicyIsInvalidArgument`. Existing: `TestAcquireGrant_Success`, `TestValidateGrant_RevokedReturnsFailedPrecondition`, `TestAuthorizationPinsTenantAndPrincipalContext`.
- model-gateway: `cd "apps/Model Plane/rust" && cargo test -p model-gateway --lib browser_grants` (memory: two cargo targets fail on Windows, use `--lib`): `issue_requires_browser_bearer`, `issue_binds_org_from_claims_not_body`, `issue_rejects_service_principal_with_browser_header`, `issue_maps_broker_status_codes`, `revoke_is_idempotent_on_not_found`, `issue_response_and_logs_never_contain_domains_when_zdr`. They use a tonic mock broker in the style of `browser_run.rs:804-881`.
- auth-core: `cd "apps/Control Plane/auth-core" && npx jest plane-service-principals-policy.contract` (memory: 15 pre-existing failures in 3 suites; diff failing sets against HEAD, do not trust exit codes).
- Repo: `node --test scripts/tests/browser-grant-contract.test.mjs`; `bash scripts/tests/zdr-contract-sync.test.sh` (digest and vendored-copy rows); `bash scripts/tests/principal-registry-parity.test.sh` (several minutes on Windows).
Quarry (Docker `rust:1` recipe from memory `quarry-rust-tests-in-docker`, `bash -c` not `-lc`, more than 10 minutes, run in the background): `cargo test -p quarry-runtime grpc::browser_broker` and `cargo test -p quarry-edge --test browser_grants_prod` with: `prod_start_run_without_grant_is_403`, `prod_start_run_with_active_grant_overrides_client_domains`, `prod_step_with_revoked_grant_is_403_not_5xx`, `prod_validator_mints_org_scoped_browser_broker_token` (wiremock Auth Core `/api/browser-broker/internal-token`, header `x-service-api-key`, body `orgId` and `scopes:["browser:read"]`), `prod_boot_refuses_http_only_validator`, `prod_boot_refuses_grpc_validator_without_service_tokens`, `token_request_contract` row test.
Integration/e2e: the L2 spec, leg A green, legs B and C expected-fail.

#### 1a.6 Definition of done
All unit tests green on the diffed baseline; L1 green in CI; L2 leg A green on a rebuilt stack; legs B/C fail exactly with the recorded messages; docs corrected; `browser-broker-ci.yml` running; ledger entry merged.

#### 1a.7 Deployment evidence required
Rebuilt local images for `browser-broker`, `model-gateway`, `auth-core`, `quarry-edge` (prod overlay) with image ids and `SOURCE_REVISION` noted; running stack `docker ps` showing quarry-edge `ENVIRONMENT=production` healthy; the wrapper transcript showing A1-A7 pass, the five-hop log scan clean, and B/C failing with the 401/403 texts above (the reproduction of the audit's inferred failure). Observable assertion for the flow: *user token -> model-gateway `/v1/browser/grants` -> broker -> Quarry `/v1/agent/runs` (prod) -> `/step` navigate returns 200 and a navigate to a non-granted host is refused*.

#### 1a.8 Effort and risks
**M, about 10 engineer-days** (harness and boot spike 4, broker 2, model-gateway 1.5, Quarry validator, manifest and tests 2, registry and compose 0.5), splittable between a Go/Model Plane engineer and a Rust/Ingestion engineer.
| Risk | Mitigation |
|---|---|
| Prod boot of quarry-edge fails for reasons beyond the grant (artifact backend, durability, secrets) | 0.5-day boot spike first; overlay owns the fixes; escalate findings to the Ingestion owner. |
| Manifest and sha256 pins churn with the in-flight QB-T09 work | Land after that commit or rebase pins; touch only the appended row and one digest line. |
| Auth Core refuses the mint until the registry is live | Deploy order above; a preflight in the wrapper mints a `browser-broker` internal token for the test org before running. |
| Broker reachable from every container on `inter-plane-bus` | Auth is a verified JWT with audience `browser-broker`; only `/grpc.health.v1.Health/` is public (`authz.go:37-46`). Same exposure class as inference-core. |
| `example.com` dependency makes L2 flaky | L2 is an evidence gate, not CI; L1 carries the regression guard. |

---

### Slice 1b: derive API, gateway wiring, delegated browser bearer

**Goal.** The gateway obtains a session grant when it opens a browser session and sends its id on `POST /v1/agent/runs`; AI runs run on a **narrower run grant derived inside the Model Plane**; the delegated `x-browser-authorization` bearer is forwarded; legs B and C go green.

#### 1b.1 Change list

Contract `apps/Model Plane/proto/model_plane/v1/browser.proto` [C] (+ regenerated `go/gen/model_plane/v1/browser*.pb.go` [C])
```proto
// In service BrowserBroker:
//   Derive a narrower, shorter-lived run grant from an active session grant.
rpc DeriveRunGrant(DeriveRunGrantRequest) returns (DeriveRunGrantResponse);

message DeriveRunGrantRequest {
  string parent_grant_id = 1;           // active kind=session grant owned by the verified principal
  string session_key = 2;               // run-scoped idempotency key; (parent_grant_id, session_key) is unique
  string org_id = 3;                    // must equal the verified organisation (same rule as AcquireGrant)
  repeated string allowed_domains = 4;  // optional narrowing; non-empty must be a subset of the parent; empty = inherit
  string purpose = 5;                   // allow-list: "ai_run"
  int32 ttl_seconds = 6;                // optional; clamped to min(parent remaining, broker run cap 900)
}
message DeriveRunGrantResponse {
  string grant_id = 1;
  string endpoint = 2;
  google.protobuf.Timestamp expires_at = 3;
  repeated string allowed_domains = 4;  // canonical, broker-owned
  string parent_grant_id = 5;
  string kind = 6;                      // "run"
  bool zdr = 7;
}
```
Authz audience rules: audience stays `browser-broker` only. `internal/authz/authz.go` [C]: add `"/model_plane.v1.BrowserBroker/DeriveRunGrant"` to the method switch (51-64, required scope `browser:write` for services; users pass at 65-67); extend the org pre-check at 51-56 to `*mpv1.DeriveRunGrantRequest`. A user principal derives only from its own parent (`GetScoped(parent, principal.OrganizationID, OwnerFilter(principal))`, so a cross-owner parent is `NotFound`). **No service principal is granted `browser:write`** (spec assertion in 1a), so in production only a verified user bearer can derive. The unknown-method default stays deny (63).

Broker `internal/grant/grant.go` [C] `Derive(ctx, DeriveParams)`:
1. `parent := GetScoped(...)`; require `parent.Kind == KindSession` (else `ErrParentNotDerivable`, FailedPrecondition) and `len(parent.AllowedActions)==0`.
2. ZDR: `child.Zdr = parent.Zdr`; if `principalZdr && !parent.Zdr` return `ErrZdrPostureChanged` (FailedPrecondition).
3. Domains: empty request -> inherit; otherwise `NormalizeAllowedDomains(requested)` and every host must equal or dot-suffix a parent host else `ErrDeriveWidensParent` (PermissionDenied). If the resulting list exceeds `BROWSER_BROKER_MAX_RUN_DOMAINS` (default 8) and the request was empty, reject (the wide-human-browse guard, D3).
4. Expiry: `min(now+ttl, parent.ExpiresAt)`; reject if under 30 s remain.
5. Idempotent on `(parent, session_key)`: an active existing run grant with identical domains is returned; with different domains `AlreadyExists`.
6. `purpose` must be `"ai_run"` (anything else `InvalidArgument`).
`internal/server/server.go` [C]: `DeriveRunGrant` handler (principal, org equality, telemetry `GrantsIssuedTotal` outcome `derived`), `errors.go` [C]: `ErrParentNotDerivable`, `ErrDeriveWidensParent`, `ErrZdrPostureChanged` mapped as stated. `CreateScopedSensitiveApproval` keeps `parent.SessionKey == sessionKey && no actions`, now satisfied by `session` or `run` parents (so F-t2-024/032/039 can issue approvals under a run grant).

model-gateway `apps/Model Plane/rust/services/model-gateway/src/browser_run.rs` [M]
- Hoist `let run_key = format!("browser-{}", new_ulid());` from the `prepare_managed_run_authenticated` call (348) to before `resolve_browser_grant` (337) and pass it in.
- `ResolvedBrowserGrant` gains `parent_grant_id: Option<String>`; `resolve_browser_grant(state, request, bearer, org_id, run_key)`: `ValidateGrant` first (existing 148-171); read `response.kind`; **kind `session`:** call `derive_run_grant` with `parent_grant_id=grant_id`, `session_key=run_key`, `allowed_domains=request.allowed_domains` (None = inherit), `purpose="ai_run"`, `org_id`; the returned grant id and canonical domains replace the resolved values; **kind `run` or empty:** today's path (172-187) unchanged; **`sensitive_approval`:** 400. `start_url` check (188-196) runs against the derived policy.
- `BrowserRunStartRequest.allowed_domains` (107-108) is documented as a *narrowing request*; the "must exactly match" rule applies only to the legacy path. Map `PermissionDenied` from derive to 403 `browser grant is not authorized`, `FailedPrecondition` (parent expired, revoked, zdr) to 409 `browser_grant_not_derivable`.
- `tool_input` (396-412) already carries `grant_id` and `allowed_domains` from `ResolvedBrowserGrant`; unchanged. execution-core validates the derived run grant with the same user bearer (owner-pinned: the child owner equals the parent owner).
- In-flight hunks at 307-313, 628-632, 663-669: no overlap; resolve conflicts on rebase.

Verevon gateway `apps/Frontend Plane/verevonv3/apps/gateway/src`
| Path | Tag | Symbol / change |
|---|---|---|
| `config.rs` | [C] | `AppState.browser_grant_mode: BrowserGrantMode {Off,Permissive,Strict}` parsed from `BROWSER_GRANT_ENFORCEMENT` next to `enforcement_mode` (160-165); default `Permissive` outside production, `Strict` in production, explicit `Off` refused in production (D8; test beside `dev_flags_allowed_outside_production`, 588). |
| `domains/chat/shared.rs` | [C] | `proxy_model_json_with_delegations` (629-690) gains `browser_bearer: Option<&str>` and sets `x-browser-authorization` via `data_plane_authorization_value` (same `Bearer` normalisation as the other headers, 672-692). The eight wrappers (375-626) pass `None`. New wrappers `proxy_model_json_with_browser_run(.., data_plane, inference, execution, session, browser, ..)` and `proxy_model_json_with_browser(state, method, url, body, bearer_token, browser_bearer, user)`; new `required_browser_token` (pattern of `required_execution_token`) over `require_model_service_token(.., ModelServiceAudience::BrowserBroker)`. |
| `audience_tokens.rs` | [C] | The `BrowserBroker` variant (27, 42) becomes live; no code change besides an updated comment at 16-18. |
| `domains/browser.rs` | [C] | **`create_session` (451-565):** after `allowed_domain = hostname(&target)` (477) and unless mode is `Off`, `issue_session_grant(&state,&user,&headers,host,zdr)` mints the model-gateway token (`model_token`) and `required_browser_token` and POSTs model-gateway `/v1/browser/grants {allowed_domains:[host], zdr}`; on success set `start_body["grant_id"]` (top-level, matching `StartRunBody.grant_id`, `agent_routes.rs:367`) before the Quarry call at 503-511. Failures: `Strict` returns 503 `browser_grant_unavailable` (or 400 `browser_host_not_grantable`, 403 `browser_grant_denied`) in the typed envelope `error(code,message)`; `Permissive` logs a warning and continues without a grant. If the Quarry start or the first navigate step fails after issuance (the cleanup path at 544-555 and the start failure at 512-514), revoke the grant best effort. **`build_ai_run_request` (1212-1248):** new parameter `grant_id: &str`, replaces `format!("session:{session_id}")` (1235); `allowed_domains` documented narrow-only. **`start_ai_run` (1263-1382):** read `grantId` from the owner projection (internal field, below); `Strict` with none returns 409 `browser_session_grant_missing` (reopen the session), `Permissive` also returns 409 (the legacy `session:` string can never validate); add `required_browser_token` to the `tokio::join!` (1320-1326) with the same `delegated_auth_unavailable` handling; call `proxy_model_json_with_browser_run` (1350-1362). **`close_session` (1023-1054):** read `grantId` from the projection before deleting the Quarry run, then best-effort `DELETE /v1/browser/grants/{id}` (never fails the close). Remove the "unbuilt" doc comment at 1258-1262. The SPA mapping (`browser_response_from_owner_projection`, 2589-) is an allowlist and must keep **not** exposing `grantId` (test below). |
| SPA `apps/Frontend Plane/verevonv3/src` | [C] | **No change.** No new user operation is added, so no `src/shared/actions` contract is needed; `browser-client.ts` keeps its shape and never sees `grantId`. Stated explicitly to satisfy the "action contract before wiring a UI operation" rule. |

Quarry `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/agent_routes.rs` [C]
- Projection: `BrowserSessionProjectionData` (struct at ~548-567) gains `grant_id: Option<String>` (`#[serde(rename_all = "camelCase")]`, skipped when `None`); filled from `entry.grant_id` in `live_browser_session_projection` (~615) and `checkpoint.grant_id` in `checkpoint_browser_session_projection` (~644). Served only through the existing owner gate (`caller_owns_browser_run`, 474-482).
- Owner and posture binding: in `start_run` after validation (1384-1396), when the caller is a **user** principal require `grant.owner_id == claims.actor_id()` (403 `browser grant belongs to another user`), and require `claims.effective_zdr_mode(...)` to be at least the grant's `zdr` (a ZDR grant cannot back a non-ZDR run). Service callers are exempt from the owner equality (section D5).
- Domain override: change the condition at 1397 from `if state.require_browser_grants` to `if state.require_browser_grants || !grant.allowed_domains.is_empty()`, so a validated grant's policy replaces client domains in every mode; the empty-domain Noop validator is unaffected.
- No change to `validate_run_grant` equality (314-338), the sensitive-approval machinery (144-313) or `step`.
- `quarry-runtime/src/grpc/browser_broker.rs` [C]: delete the legacy `GrpcGrantValidator::acquire` (97-131) and its `AcquiredGrant` type. It has zero callers and it is the only code in Quarry that can call `AcquireGrant`; removing it makes the rule "Model Plane owns issuance, Quarry only validates" structural. Keep `revoke` (134-146) only if a caller appears.

Contract test: remove the `test.fail()` on legs B and C; add the L1 assertions marked (1b) above; extend with the widening negative.

Docs: `docs/BROWSER_WORKSPACE_PLAN.md` status line; ledger entry updated to "implemented".

#### 1b.2 API / proto / data-model changes
`DeriveRunGrant` RPC and messages above; gateway env `BROWSER_GRANT_ENFORCEMENT`; Quarry projection field `grantId` (internal, camelCase like the rest of the projection); no SQL.

#### 1b.3 Feature flags, rollout, rollback
- `BROWSER_GRANT_ENFORCEMENT` (gateway AppState): `off|permissive|strict` as D8.
- Rollout: (1) broker + model-gateway (additive RPC, additive route) -> (2) Quarry image (projection, owner check, override) -> (3) gateway image with the default mode (`permissive` outside prod) -> run L2 -> (4) production gateway starts at `strict` by default; confirm with L2 on the prod-overlay stack.
- Rollback: set `BROWSER_GRANT_ENFORCEMENT=off` on the gateway (instant) or swap the previous gateway image (`swap_gateway_image.py`); the other services are backward compatible with an old gateway (an old gateway simply has no grants, which only works against a non-prod Quarry, exactly today's state). A broker rollback to the 1a image leaves model-gateway's derive call returning `Unimplemented`, mapped to 502; revert model-gateway with it.
- Dev mode keeps working: a dev stack with the broker gets grants end to end; a dev stack without the broker sets `off`.

#### 1b.4 Tests
- Broker: `GOWORK=off go test ./...` in the service directory. New: `TestDeriveRunGrant_NarrowsDomains`, `..._RejectsWidening` (`ErrDeriveWidensParent`), `..._RejectsRunOrSensitiveParent`, `..._ClampsExpiryToParent`, `..._IdempotentPerParentAndSessionKey`, `..._ZdrFollowsParentAndRefusesPostureChange`, `..._OtherOwnersParentIsNotFound`, `..._EmptyRequestInheritsButRespectsRunDomainCap`, `TestRevokeParentCascadesToRunGrants`, `TestSensitiveApprovalUnderRunGrant`, `TestAuthorizeDeriveRunGrantRequiresWriteScopeForServicesAndAllowsUsers`, `TestAuthorizeRejectsCrossOrgDeriveRequest`.
- model-gateway: `cd "apps/Model Plane/rust" && cargo test -p model-gateway --lib browser_run` (+ `browser_grants`). New: `session_kind_grant_is_derived_before_any_run_write`, `run_kind_grant_keeps_exact_equality_path`, `derive_failure_creates_no_thread_or_run`, `requested_domains_must_be_subset_of_session_grant`, `derive_precondition_maps_to_409`, `sensitive_kind_grant_is_rejected`; the existing `issuer_zdr_cannot_be_downgraded_by_browser_run_body` stays.
- Gateway: `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets browser` (CLAUDE.md form; CI at `verevonv3-ci.yml:132-141` runs `fmt --check`, `clippy --all-targets -D warnings`, `cargo test`, so run all three). New wiremock tests: `create_session_issues_grant_then_starts_quarry_run_with_grant_id`, `create_session_strict_returns_503_when_grant_unavailable`, `create_session_permissive_continues_without_grant`, `create_session_off_never_calls_model_gateway`, `create_session_revokes_grant_when_first_step_fails`, `start_ai_run_forwards_browser_bearer_and_session_grant_id`, `start_ai_run_without_grant_is_409`, `browser_session_response_never_contains_grant_id`, `close_session_revokes_grant_best_effort`, `grant_mode_defaults_strict_in_production_and_refuses_off`, `proxy_with_delegations_sets_x_browser_authorization_only_when_present`. **Update** `build_ai_run_request_never_forwards_client_requested_require_approval` (`browser.rs:5192-5232`): it pins `grant_id == "session:sess_1"` at 5228 and must now pin the passed id; this break is intended.
- Quarry (Docker recipe): `cargo test -p quarry-edge` with `projection_includes_grant_id_only_for_owner`, `user_cannot_use_another_users_grant`, `zdr_grant_cannot_back_a_non_zdr_run`, `validated_grant_overrides_client_domains_in_every_mode`.
- L1 and L2 as above, now fully green.

#### 1b.5 Definition of done
Every listed test green; L2 legs A, B, C green on the prod-overlay stack; the gateway has zero `AcquireGrant` references (L1); the three prod-mode negative controls fail closed; `browser.rs` carries no `session:` grant string.

#### 1b.6 Deployment evidence required
Rebuilt images for `browser-broker`, `model-gateway`, `quarry-edge`, and the gateway (`frontend-plane-verevonv3-gateway:latest` swapped in); stack running with quarry-edge in prod mode and the gateway at the default mode for that environment; L2 transcript with the observable flow: *SPA-equivalent `POST /api/v1/browser/sessions` -> 200 -> `POST .../ai-runs` -> 200 `run_id` -> run events contain `BrowserActionDispatched` and `BrowserObservationReceived` -> broker log shows one `AcquireGrant` and one `DeriveRunGrant` for the run -> `DELETE` the session -> a later `/step` with the derived grant returns 403*, plus a clean per-hop log scan.

#### 1b.7 Effort and risks
**L, about 10 engineer-days** (broker derive 2.5, model-gateway 1.5, gateway wiring and tests 3.5, Quarry hardening 1.5, contract legs and docs 1).
| Risk | Mitigation |
|---|---|
| Gateway latency from three extra round trips on session create (two token mints, one model-gateway call) | Mint the two tokens with `tokio::join!`; budget about 100-150 ms; measure in L2 and record. |
| A BFF compromise could request grants for hosts of its choosing for users it holds sessions for | Same trust boundary as today (the gateway already chooses Quarry domains, `browser.rs:486`); broker policy hook, per-owner cap, org/owner pinning and 15-min TTL bound it; the gateway cannot derive or widen. Stated honestly in section 6. |
| Derive semantics collide with WS8 (human-browse) and F-t2-013 (attach) | D3: only invariant kinds ship; `parent_grant_id`, `kind` and `absolute_expires_at` leave room; reconcile at the ADR (section 9). |
| Sessions opened before rollout have no grant | `strict` returns 409 `browser_session_grant_missing`; sessions are short-lived (120 s lease) so impact is minutes. |
| IP-literal public hosts now fail in strict mode | Documented `browser_host_not_grantable`; no allow-list widening. |

---

### Slice 1c: Postgres grant store for the broker

**Goal.** Grants survive a broker restart (except ZDR grants, by design), the broker refuses to run non-ephemerally without a database, and the repository boundary introduced in 1a gets its durable implementation. No new RPC and no gateway change.

#### 1c.1 Change list

Broker `apps/Model Plane/go/services/browser-broker`
| Path | Tag | Symbol / change |
|---|---|---|
| `migrations/0001_browser_grants.up.sql` | [NEW] (directory is new) | Schema below. Every statement is re-runnable (`IF NOT EXISTS`) because the entrypoint re-applies all `*.up.sql` on every start (sandbox-manager and cost-core precedent). |
| `migrations/0001_browser_grants.down.sql` | [NEW] | Drops the indexes then `browser_grants`. |
| `internal/grant/postgres.go` | [NEW] | `PgStore` implementing `Repository` over `pgxpool.Pool`: `CreateSession`, `CreateScopedSensitiveApproval`, `Derive` (transaction, `SELECT ... FOR SHARE` on the parent so a concurrent revoke cannot be lost, `INSERT ... ON CONFLICT (parent_grant_id, session_key) WHERE kind='run' DO NOTHING` then read the existing row for idempotency), `GetScoped`, `RevokeScoped` (single `WITH RECURSIVE` statement that revokes the whole subtree and reports `NotFound` when the root is outside the verified org/owner scope), `CountActive`, `Renew`, and `PurgeExpired(ctx, olderThan)`. The per-owner cap is made exact with `pg_advisory_xact_lock(hashtext(org_id||':'||owner_id))` inside the create transaction. |
| `internal/grant/router.go` | [NEW] | `Routed{persistent, ephemeral Repository}`: creates route by the ZDR decision; `GetScoped`, `RevokeScoped` and `Derive` route by the id prefix (`z_` goes to the memory repository, everything else to Postgres). A `z_` id is never sent to Postgres. |
| `internal/grant/grant.go` | [C] | Memory `Store` kept as the ephemeral repository (ZDR grants and `ALLOW_EPHEMERAL_DEVELOPMENT`). |
| `cmd/main.go` | [C] | Read `DATABASE_URL`; if empty and `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT` is not `true`, log and exit 1 (copy of `sandbox-manager/cmd/main.go:42-47`); open the pool; wire `Routed`; start the purge loop (every 10 min, `DELETE ... WHERE expires_at < now() - interval '24 hours'`; ZDR rows never exist); make `/readyz` (currently unconditional, 42-45) ping the database; log which store is active (no domains). |
| `internal/telemetry/metrics.go` | [C] | Add `GrantStoreErrorsTotal{op}` counter next to `GrantsIssuedTotal`; no domain attributes. |
| `docker-entrypoint.sh` | [NEW] | Copy of `sandbox-manager/docker-entrypoint.sh` (psql loop over `/app/migrations/*.up.sql`, then `exec "$@"`), service name changed. |
| `Dockerfile` | [C] | `apk add ... postgresql-client`; `COPY services/browser-broker/migrations /app/migrations`; copy and `chmod` the entrypoint; `ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]` with `CMD ["service"]` (build context is `go/`, per the Dockerfile header). |
| `go.mod` / `go.sum` | [C] | `GOWORK=off go get github.com/jackc/pgx/v5@v5.9.2` (the version siblings use) and, for tests, `github.com/testcontainers/testcontainers-go@v0.42.0` plus `.../modules/postgres@v0.42.0` (sandbox-manager's versions). `GOWORK=off` because workspace MVS can otherwise satisfy the import silently without writing the require line (memory note). |
| `internal/grant/postgres_integration_test.go` | [NEW] | `//go:build integration`, testcontainers, migrations applied from `../../migrations` (pattern of `sandbox-manager/internal/lease/lease_integration_test.go`). |

Deploy
| Path | Tag | Change |
|---|---|---|
| `apps/Model Plane/deploy/docker-compose.yml` | [C] | `browser-broker`: `DATABASE_URL: postgresql://postgres:${MODEL_POSTGRES_PASSWORD:?MODEL_POSTGRES_PASSWORD is required}@postgres:5432/session_core` and `depends_on: postgres: condition: service_healthy` (same shape as cost-core and letta-bridge). |
| `apps/Model Plane/deploy/docker-compose.production.yml` | [C] | `browser-broker` (113-115) already uses `*hardened-service` (read-only root, `tmpfs /tmp`); the entrypoint's `/tmp` log write is covered. No other change. |
| `.github/workflows/browser-broker-ci.yml` | [NEW from 1a] | Add job `integration`: `go test -tags integration ./internal/grant/...` (Docker is available on GitHub-hosted runners for testcontainers). |

#### 1c.2 Schema (SQL `0001_browser_grants.up.sql`)
```sql
CREATE TABLE IF NOT EXISTS browser_grants (
    id                   TEXT PRIMARY KEY,
    org_id               TEXT NOT NULL,
    owner_id             TEXT NOT NULL,
    kind                 TEXT NOT NULL CHECK (kind IN ('session','run','sensitive_approval')),
    session_key          TEXT NOT NULL,
    scope_url            TEXT NOT NULL,
    allowed_domains      TEXT[] NOT NULL CHECK (cardinality(allowed_domains) BETWEEN 1 AND 32),
    allowed_actions      TEXT[] NOT NULL DEFAULT '{}',
    allowed_frame_ids    TEXT[] NOT NULL DEFAULT '{}',
    allowed_dialog_ids   TEXT[] NOT NULL DEFAULT '{}',
    allowed_artifact_ids TEXT[] NOT NULL DEFAULT '{}',
    parent_grant_id      TEXT REFERENCES browser_grants(id) ON DELETE CASCADE,
    expires_at           TIMESTAMPTZ NOT NULL,
    absolute_expires_at  TIMESTAMPTZ,            -- renewal ceiling, unset until WS8 (F-t2-011)
    revoked_at           TIMESTAMPTZ,
    revoked_reason       TEXT NOT NULL DEFAULT '',
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((kind = 'session') = (parent_grant_id IS NULL)),
    CHECK (id NOT LIKE 'z\_%')                   -- a ZDR id can never be stored
);
CREATE INDEX IF NOT EXISTS browser_grants_owner_active_idx
    ON browser_grants (org_id, owner_id, expires_at) WHERE revoked_at IS NULL;
CREATE INDEX IF NOT EXISTS browser_grants_parent_idx
    ON browser_grants (parent_grant_id) WHERE parent_grant_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS browser_grants_run_idempotency_idx
    ON browser_grants (parent_grant_id, session_key) WHERE kind = 'run';
CREATE INDEX IF NOT EXISTS browser_grants_expiry_idx ON browser_grants (expires_at);
```
There is deliberately **no `zdr` column**: ZDR grants never reach Postgres; the CHECK on the id prefix and the L1 assertion enforce it twice. No domain value is ever indexed or logged. Next free number: this is the first migration for this service (`apps/Model Plane/go/services/browser-broker/migrations` does not exist); the other Model Plane migration series are separate (`session-core/migrations/0045_*` is the latest there, `sandbox-manager/migrations/0004_*`).

#### 1c.3 Restart and failure semantics for in-flight sessions
| Event | Non-ZDR grant | ZDR grant | Effect on an open session / AI run |
|---|---|---|---|
| Broker restart | Survives (Postgres) | **Lost** (memory) | Non-ZDR: next Quarry `/step` validates normally. ZDR: next `/step` sees NotFound, 403 inactive, session ends; the user reopens. |
| Broker unavailable | `Unavailable` | same | Quarry retries 3 times (0/50/200 ms, `browser_broker.rs:174`) then a 5xx driver error; model-gateway returns 503 `browser grant validation unavailable`; the gateway in `strict` shows `browser_grant_unavailable`. No fail-open. |
| Postgres unavailable | broker `Unavailable` | unaffected | same as broker unavailable for non-ZDR; `/readyz` fails. |
| Quarry restart | intact | intact | Durable runtime resumes from the checkpoint's `grant_id` and revalidates (`agent_routes.rs:1372-1376`). |
| model-gateway or gateway restart | intact | intact | Run grants are independent of both; execution-core keeps running a durable run. |
| TTL expiry (default 15 min) | step returns 403 | same | Session ends. Renewal is WS8 (D9). |
| Rollback to memory (`DATABASE_URL` unset, ephemeral flag on) | outstanding Postgres grants become unreachable | n/a | Sessions end; rows stay for inspection; `down.sql` is not run automatically. |

TTL is 15 min (`BROWSER_BROKER_GRANT_TTL_SECONDS`, D9). **Renewal plugs in** at three places that 1c already provides: the `absolute_expires_at` column, the `Renew` repository method, and the cap/expiry index. WS8 adds the RPC, the model-gateway route and the gateway keep-alive.

#### 1c.4 Feature flags, rollout, rollback
`DATABASE_URL` and `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT` (name chosen to mirror `SANDBOX_MANAGER_ALLOW_EPHEMERAL_DEVELOPMENT`). Rollout: deploy the broker image with `DATABASE_URL` set (compose default); the migration runs at container start; verify with the restart experiment below. Rollback: unset `DATABASE_URL` and set the ephemeral flag (dev only; production refuses to start without a database, which is the intended fail-closed behaviour).

#### 1c.5 Tests
- Unit (no Docker): `cd "apps/Model Plane/go/services/browser-broker" && GOWORK=off go test ./...`. New: `TestRouted_ZdrNeverTouchesPersistentRepo`, `TestRouted_ZPrefixIdNeverReachesPostgres`, `TestRouted_DeriveRoutesByParentPrefix`.
- Integration (Docker): `cd "apps/Model Plane/go/services/browser-broker" && GOWORK=off go test -tags integration ./internal/grant/...` (no `-race`: needs cgo, unavailable on this host). Names: `TestPg_CreateGetExpireRevoke`, `TestPg_RevokeCascadesWholeSubtree`, `TestPg_DeriveIsIdempotentUnderConcurrency`, `TestPg_DeriveRejectsRevokedParentRace` (revoke and derive in parallel, never an active child of a revoked parent), `TestPg_PerOwnerCapIsExactUnderConcurrency`, `TestPg_OwnerAndOrgScopingReturnNotFound`, `TestPg_ZdrCreateWritesZeroRows`, `TestPg_MigrationIsRerunnable` (apply twice), `TestPg_SurvivesNewRepositoryInstanceOnSameDatabase` (restart simulation), `TestPg_PurgeRemovesExpiredAndCascadesChildren`.
- L1: add assertion (7): the migration has no `zdr` column and keeps the `z\_%` CHECK.
- L2 evidence: see 1c.6.

#### 1c.6 Definition of done and deployment evidence
Done: unit and integration tests green in `browser-broker-ci.yml`; compose broker healthy with the database; L1 green.
Evidence (rebuilt `browser-broker` image, running stack, verified flow): (1) open a non-ZDR session through the gateway and note the grant id from the broker log; (2) `docker restart model-plane-browser-broker-1`; (3) `/step` on the same session returns 200 (**grant survived**); `psql` shows the row with the expected kind and `expires_at`; (4) repeat with a ZDR session (`zdr:true`): after the restart `/step` returns 403 inactive, and `SELECT count(*) FROM browser_grants` did not increase for it (**ZDR never persisted**); (5) run L2 legs A, B, C again against the Postgres-backed broker.

#### 1c.7 Effort and risks
**M, about 6 engineer-days** (store and router 2.5, main/entrypoint/Dockerfile/compose 1.5, integration tests 1.5, CI and evidence 0.5).
| Risk | Mitigation |
|---|---|
| Shared `session_core` database and the `postgres` superuser (precedent, not ideal) | Prefix `browser_`; nothing else reads the table; dedicated database and role is a listed follow-up (section 9, non-blocking). |
| Migration re-applied every start | Idempotent DDL only; `TestPg_MigrationIsRerunnable`; later migrations must use `ADD COLUMN IF NOT EXISTS`. |
| ZDR grants lost on restart and per-replica | Single replica is documented in the ledger entry; L2 asserts the restart behaviour; a second replica is out of scope until a sticky or shared-ephemeral design exists. |
| `ON DELETE CASCADE` purge surprising operators | 24 h grace and a purge test; children expire no later than their parent. |
| Run grants outlive a stopped AI run (no revoke-on-stop) | Bounded by TTL; revoke-on-stop deferred to F-t2-036 with a run-to-grant map (D9). |

---

## 5. Dependencies and ordering

**Hard, inbound (WS1 needs):**
- Control Plane owner sign-off for the `quarry-edge` to `browser-broker` `browser:read` principal and an auth-core rebuild with the new registry (1a). Without it prod Quarry cannot validate (G1).
- The Ingestion owner's confirmation that the prod-overlay quarry-edge boots (boot spike, 1a). [S9] H2/H4 predict artifact-backend and durability bails.
- Model Plane compose network change (1a, G5).
**Soft:** F-t2-006 (lease reaper and caps): the gateway lease TTL is 120 s and unreaped, so the grant TTL is the only live cap until it lands; F-t2-022 (policy owner, ADR topic 1) plugs into `PolicyResolver`; F-t2-008 (budgets in the grant) uses field numbers 40-49 (D11); F-t2-004/029 telemetry rules (no domains in logs); F-t2-013 phase 1 (initiator on receipts) is independent.

**Build order:** 1a, then 1b, then 1c. 1c can start after PR-a of 1a lands the `Repository` interface (the `Derive` signature is in the interface from 1a), so Go work on `PgStore` can overlap 1b; its `Derive` SQL merges after 1b's memory implementation fixes the semantics. Ship order in production: broker, model-gateway, auth-core registry, quarry-edge, gateway (permissive), L2, gateway strict.

**Hard, outbound (blocked until WS1):** F-t2-010 planner on, F-t2-011, F-t2-012, F-t2-013 phase 2, F-t2-024, F-t2-032, F-t2-033, F-t2-039, F-t2-063 (audit 9.4 item 1); F-t2-061/062/076 reuse run-grant derivation; F-t2-036 builds on 1c's Postgres rows.

**Merge-conflict hotspots (files and ranges shared with other workstreams):**
| File | Range WS1 edits | Shared with |
|---|---|---|
| gateway `domains/browser.rs` [C] | `create_session` 451-565, `close_session` 1023-1054, `build_ai_run_request` 1212-1248, `start_ai_run` 1263-1382 | F-t2-011 (domains and budgets at 483-495), F-t2-013 (`run_action` 663-760, `prepare_client_ws_message` 2037-2105: not touched by WS1), F-t2-024/032/036 |
| gateway `domains/chat/shared.rs` [C] | `proxy_model_json_with_delegations` 629-690 and wrappers 375-626 | sibling (b) chat bearer plumbing, sibling (a) sandbox bearer |
| gateway `config.rs` [C] | `AppState` field and parse next to 160-165 | any workstream adding gateway flags |
| `browser.proto` [C], `go/gen/model_plane/v1/browser*.pb.go` [C] | fields 10-12, `DeriveRunGrant` | F-t2-013, F-t2-024 (`fill_otp`), F-t2-032 (fields 20-39), F-t2-036 |
| broker `grant.go`, `server.go`, `authz.go` [C] | whole files (Repository refactor) | F-t2-013, F-t2-024 (`allowedSensitiveActions` 32-36), F-t2-032, F-t2-036: **the largest conflict surface; WS1 should merge first** |
| model-gateway `browser_run.rs` [M] | 86-126, 138-202, 337-348, 396-412 | F-t2-032 (equality at 180-186), F-t2-013, sibling (b) |
| quarry-edge `agent_routes.rs` [C] | 288, 314-338, 548-650, 1372-1413, 2258, 3095, 3280 | F-t2-006, 011, 013, 024, 032, 036 |
| quarry-edge `main.rs` [M] | 182-201, 659-672 | F-t2-011 (flag defaults), F-t2-032 |
| manifest `cross-plane-token-requests.json` + both `.sha256` + auth-core copy + `zdr-contract-sync.test.sh` [M] | one appended row, one digest | QB-T09/T17 work, F-t2-019/037 ("model-gateway grant to quarry-edge") |
| `plane-service-principals.json` [C] `quarry-edge` object | audiences and scopes | F-t2-019/037 add a model-gateway audience to the **same** principal; coordinate in one registry PR |
| `deploy/docker-compose.yml` (Model Plane) [C] 1612-1629; `docker-compose.yml` (Ingestion) [M] ~95-135 | env and networks | any workstream touching broker or quarry-edge env |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

- **Control Plane owns identity, sessions, quotas.** WS1 mints no identity: it consumes Auth Core user tokens for `browser-broker` and one registry principal. The registry file is Control Plane's; the change needs its owner (D5). Not at risk.
- **Data Plane v2.** Untouched.
- **Ingestion Plane (Quarry) captures and executes; validates only.** Quarry gains a credential, org-aware validation, an internal projection field and a hardening check; it never issues authority. The legacy Quarry `acquire` (the only Quarry code that can call AcquireGrant, no callers) is deleted in 1b so the rule is structural, and L1 asserts a single minting path. Quarry persists nothing new (the checkpoint already stores `grant_id`; ZDR runs are never checkpointed, `agent_routes.rs:1350`).
- **Model Plane owns browser grants.** All authority is minted by the broker; model-gateway binds org and user from verified claims; execution-core is unchanged. Verified by L1 (single minting path) and by the registry assertion that no service principal holds `browser:write` on `browser-broker`.
- **Frontend Plane owns UI and the same-origin gateway.** The gateway only forwards the user's own bearer and a session grant id; it cannot mint, derive or widen; responses stay in typed envelopes; the SPA is unchanged and never sees `grantId`.
- **Application Plane, Support Plane, Channel Plane.** Untouched.
- **No direct database crossing.** The only new database object is a Model Plane table read only by the broker; Quarry, model-gateway and the gateway reach grants over gRPC/HTTP.
- **A Space must never widen a grant (sibling (a)).** Structural: neither `AcquireGrantRequest` nor `DeriveRunGrantRequest` has a `space_id`, org and owner come from the verified principal, a run grant is a subset of a session grant owned by the same user, and a Space member has no path to a grant they do not own. Any future Space-aware grant field must be a Control-signed decision (the sandbox-lease precedent) and must only narrow. Test: `TestDeriveRunGrant_OtherOwnersParentIsNotFound` plus the L2 negative A6.
- **User-session issuance must not bypass the CrossDomainNavigation gate.** The gate is an execution-core run-loop check on the target host versus the last observed host (`browser_risk.rs:262-280`); it does not read the grant, so it fires even for hosts inside a grant. WS1 does not touch it, session grants are single-host fences, and derived run grants cannot inherit a wide policy (D3). Test: add `cross_domain_goto_is_flagged_even_when_both_hosts_are_in_the_grant` to `browser_risk.rs` tests (exists in spirit at 654, 688; assert with a two-host grant) in 1b.
- **Model proposes, Quarry executes or rejects.** Unchanged and strengthened: the AI never chooses domains (the run grant is derived server-side from the user's session grant; the optional request list can only narrow); every action still goes through execution-core's allow-list, the HITL risk gates and Quarry's per-step `ValidateGrant` and domain check; issuance never executes anything.
- **ZDR propagation.** (1) ZDR is OR-ed from the request and the verified token and can never be lowered; (2) ZDR grants are memory-only, `z_`-prefixed, and cannot be stored (CHECK constraint, no column); (3) a run grant inherits its parent's ZDR and a posture change fails closed; (4) no domain lists in broker, model-gateway or gateway logs or metric attributes; (5) Quarry refuses a non-ZDR run under a ZDR grant (1b); (6) model-gateway keeps refusing ZDR AI runs (`browser_run.rs:307-315`). Residual: ZDR grants are lost on broker restart and require a single replica (D6).
- **Rules at risk, stated plainly.** (i) The gateway (BFF) chooses the start host for a session grant; a compromised BFF could request grants for hosts of its choosing for users whose bearers it holds. This is the same trust boundary as today (it already chooses Quarry's domain, `browser.rs:486`); the broker's policy hook, per-owner cap, org/owner pinning, 15-minute TTL and the absence of any derive or widen path from the gateway bound it. The real fix is org policy (ADR topic 1). (ii) Giving quarry-edge `browser:read` lets a compromised Quarry validate (never create) grants in any directory org it can name; it cannot create, derive or revoke.

## 7. Cross-session interfaces

Sibling sessions: (a) Verevon Space, (b) Model Plane + chat/agents. Overlap flagged: BrowserBroker grants (F-t2-001 is in both scopes), agent browser path (F-t2-010/013), delegated bearer forwarding, approvals (F-t2-033), agent PC/browser in Spaces, ZDR-safe telemetry.

| Shared piece | Proposed owner | Interface / what the other session does | Reconcile |
|---|---|---|---|
| Broker store, `Repository`, Postgres, `DeriveRunGrant`, authz, proto | **WS1 (this plan)** | (b) consumes the RPCs; (b) must not design a second grant API | The user must name one owner for the broker. This plan proposes itself because the surface is small and the conflict surface (section 5) is largest here. |
| model-gateway `/v1/browser/grants` and the derive step in `resolve_browser_grant` | **WS1** | (b) may add routes to `browser_grants.rs`, e.g. `POST /v1/browser/grants/{parent}/approvals` for sensitive approvals | L1's single-minting-path test must be updated by whoever adds the second route. |
| Where a chat turn gets its browser grant (execution-core `browser.observe`/`browser.act` take `grant_id` from tool input, `runtime_loop/agent.rs:3484,4206`) | **(b)** | Proposal for (b): obtain a run grant by calling `DeriveRunGrant` from the user's session grant (or a Space-scoped one); extend the `purpose` allow-list with `chat_turn` (one line in `Derive`) | Reconcile the `purpose` list and who holds the parent session grant for a chat that never opened a UI session. |
| `x-browser-authorization` on **chat** routes | WS1 adds the parameter and wrappers in `chat/shared.rs`; **(b)** wires chat call sites | Pass `required_browser_token` (fail) or a best-effort variant | Agree the failure mode per route. |
| Sensitive-approval issuance (F-t2-033/024/039 call `AcquireGrant` with actions) | **(b)** | Works under a `session` or `run` parent after 1b | Same L1 update as above. |
| execution-core `grpc.rs` `validate_browser_grant` | **(b)** if anything | WS1 needs no change | None. |
| Agent PC/browser inside Spaces | **(a)** owns Space UI and membership; **WS1** owns the grant | The Space-hosted agent derives its run grant from the acting user's session grant; no `space_id` in grant APIs; a Space never widens (section 6) | Confirm no Space feature plans a shared (non-owner) grant. |
| ZDR-safe telemetry (F-t2-004/029/014) | **(a)/(b)** own export; WS1 supplies the rules | Grant calls never log domains; broker metrics use `outcome` only | Add the rule to the telemetry lint if one is built. |
| Attach-to-user-session and a human-browse kind (ADR topic 3; F-t2-013 phase 2, F-t2-011) | **WS8 with (b)** | WS1 leaves `kind`, `parent_grant_id`, `absolute_expires_at` ready | One joint ADR; WS1 does not decide it. |
| Proto field numbers | integrating architect | 10-19 WS1, 20-39 F-t2-032, 40-49 F-t2-008 (D11) | Confirm the allocation. |

## 8. What can proceed WITHOUT an ADR or decision, and what is blocked

**Proceed now (no decision needed):** the broker `Repository` refactor, kinds, ZDR routing, per-owner cap and `PolicyResolver` hook on the memory store; the model-gateway grant routes; the L1 contract test and both new CI workflows; the `buf.yml` fix and baseline; Quarry's G6 fix, org-aware validation, boot gates, legacy `acquire` deletion and projection/owner/override hardening; the Model Plane compose network; the L2 spec and wrapper (leg A can be developed first against a dev-bypass Quarry); `DeriveRunGrant`; gateway wiring in `off` and `permissive`; the whole Postgres slice; doc corrections.

**Blocked (and by what):**
- Merging the `quarry-edge` registry principal and enabling the service-token validator in production: **D5, Control Plane owner sign-off**, new ledger entry (closest audit topic 3).
- Production default `strict` for the gateway: not an ADR, an **evidence gate**: L2 green on the prod-overlay stack.
- A human-browse kind and AI attach to the user's session: **audit ADR topic 3**; WS1 does not need them.
- Real org web policy behind `PolicyResolver`: **audit ADR topic 1**; WS1 ships the hook and a static default.
- Multi-replica ZDR grants: **audit ADR topic 4** extension; not needed while the broker is one container.

## 9. Open questions for the user (real blockers only)

1. **Blocks slice 1a merge.** Approve registering `quarry-edge` as a `browser-broker` principal with scope `browser:read` only (Control Plane owner). If refused, a production Quarry can never validate a grant; the only alternative is offline-verifiable signed grants, which loses revocation and is a different security model.
2. **Blocks slice 1c production rollout.** Confirm that ZDR browser sessions may rely on memory-only grants on a single broker replica (grant lost on broker restart, session ends), or decide to refuse ZDR browser sessions where the broker runs more than one replica.
3. **Blocks "L2 as a gate".** The prod-mode live test cannot run on a GitHub-hosted runner (it needs the full compose stack and outbound HTTPS to `example.com`). Decide: a self-hosted runner, or a manual pre-merge evidence run recorded in the PR (this plan assumes the latter, with L1 in CI).
Non-blocking preferences (defaults are applied unless overridden): dedicated Postgres database and least-privilege role for the broker instead of the shared `session_core` database; `BROWSER_BROKER_MAX_SESSION_DOMAINS` default 8 (OD13); whether ledger entry numbering should split into one entry per D1/D2/D5/D6.

## 10. INTERFACE SUMMARY

**Files touched by plane.** *Model Plane:* Go `browser-broker` (`grant/grant.go`, `grant/policy.go` NEW, `grant/postgres.go` NEW, `grant/router.go` NEW, `server/server.go`, `server/errors.go`, `authz/authz.go`, `cmd/main.go`, migrations `0001_browser_grants.{up,down}.sql` NEW, `docker-entrypoint.sh` NEW, `Dockerfile`, `go.mod`); `proto/model_plane/v1/browser.proto` + `go/gen/*`; Rust `model-gateway` (`browser_grants.rs` NEW, `browser_run.rs`, `lib.rs`, `http_routes.rs`); `deploy/docker-compose.yml`. execution-core: no change. *Control Plane:* `config/plane-service-principals.json`, auth-core policy spec. *Ingestion:* quarry-runtime (`service_tokens.rs`, `grant_validator.rs`, `grpc/browser_broker.rs`, manifest + pins), quarry-edge (`main.rs`, `agent_routes.rs`, `test_support.rs`, new `tests/browser_grants_prod.rs`), two compose files plus a contract overlay NEW. *Frontend:* gateway `browser.rs`, `chat/shared.rs`, `config.rs`; Playwright spec NEW; SPA unchanged. *Repo:* `scripts/tests/browser-grant-contract.test.mjs`, `browser-grant-prod-contract.sh` NEW; workflows `browser-broker-ci.yml`, `browser-grant-contract.yml` NEW; `buf.yml` fix.

**New contracts.** Proto: `AcquireGrantRequest.zdr=10`; `AcquireGrantResponse.kind=10,zdr=11`; `ValidateGrantResponse.kind=10,owner_id=11,zdr=12`; `rpc DeriveRunGrant(DeriveRunGrantRequest 1-6) returns (DeriveRunGrantResponse 1-7)`. HTTP: model-gateway `POST /v1/browser/grants`, `DELETE /v1/browser/grants/{id}`. Registry: `quarry-edge` on `browser-broker`, `browser:read`; manifest row `browser-grant-validate`. SQL: `browser_grants` (no zdr column). Flags/env: gateway `BROWSER_GRANT_ENFORCEMENT` (off/permissive/strict; permissive dev, strict prod); broker `DATABASE_URL`, `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT`, `..._GRANT_TTL_SECONDS` (900), `..._MAX_SESSION_DOMAINS` (8), `..._MAX_RUN_DOMAINS` (8), `..._MAX_ACTIVE_GRANTS_PER_OWNER` (20); Quarry `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL`.

**Dependencies.** Hard in: Control Plane sign-off (D5), Quarry prod-boot spike, broker network. Hard out: F-t2-010/011/012/013/024/032/033/039/063; F-t2-036 builds on 1c. Soft: F-t2-006, 008, 022.

**Effort.** 1a M (about 10 engineer-days), 1b L (about 10), 1c M (about 6); total about 26 engineer-days, 16-18 working days with two engineers. Audit "L" stands; XL only if the Quarry prod-boot spike finds further blockers.

**ADR gates.** D5 (new ledger entry, Control sign-off; closest topic 3); D3 (topic 3, kinds beyond session/run); D4 (topic 1, policy owner); D6 (topic 4, ZDR ephemeral-only).

**Parallelisable with:** F-t2-002/003/004/005/007/009 (disjoint files). **Conflicts with:** F-t2-013, 024, 032, 036 (broker, proto, `agent_routes.rs`); F-t2-011 (`browser.rs` `create_session`, `main.rs` flags); F-t2-019/037 (registry `quarry-edge` object, manifest pins); sibling (b) (`chat/shared.rs`, `browser_run.rs`). Merge WS1 broker work first.

<!-- COMPLETE -->
