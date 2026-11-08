# WS10-model-entitlements: code map for F-t3-023

Read-only sweep for workstream WS10 (org and role model entitlements). No product code, config or docs were edited. Paths are repo-relative unless marked absolute; the roots are in section 0.

## 0. Roots, status key, verdicts

- **HEAD** = `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b` at afb83928 (clean).
- **LIVE** = `C:/dev/CoresSystem` (HEAD plus about 324 uncommitted or untracked entries, per `.plan-work/live-tree-status.txt`).
- **Status**: `clean` = not listed, so LIVE equals HEAD for that path. `M` = modified in LIVE. `??` = untracked, exists only in LIVE. No ` D` (deleted) entry touches a relevant path.
- **Verdicts**: MATCH (symbol or behaviour at the cited lines), DRIFT (found at other lines; new range given), PARTIAL (only part of the cited range matches), ABSENT.
- **Audit card** (absolute): `C:/Users/ImaFernandesDaCosta/AppData/Roaming/Claude/scratch-workspaces/5e23d905-dd91-4518-8ff4-252efcbe0520/b5ff8b3a-437c-4499-9a7f-469de994f319/scratch-2026-10-12-69a4ee/03-model-plane-chat-and-agents-audit.md`. Card `### F-t3-023` at line 683, body 684-698. Section 9.1 wave row line 1545. Priority table row line 1840. Appendix C entry 2102-2110. Appendix D line 2127.
- **Card header**: Priority; Verevon today Absent; Effort L (was M); planes Control Plane, Model Plane, Frontend Plane; Wave W1; confidence High.

## 1. What the card asks

Give `allowed_models` an owner. org-core holds an entitlement policy (org, role, group, plan) with allow and deny model lists, a max effort, an admin default model and effort per surface, and an explain endpoint. model-gateway pulls the resolved entitlement through a service principal, filters `list_models`, and refuses a disallowed model at admission with a typed reason, failing closed (not the fail-open `org_quota` pattern). inference-core checks again in depth. Needs an ADR on ownership. Quick win: org allow and deny lists, default model and a `list_models` filter on the `org_quota` pull pattern (M).

## 2. Citation verification (step 3)

Card id is F-t3-023 for every row. "Resolved path" is the full path for the short citation.

| # | Cited | Resolved path | Claim | HEAD verdict | LIVE status | Corrected range |
|---|---|---|---|---|---|---|
| C1 | grpc.rs:148-160 | apps/Model Plane/rust/services/model-gateway/src/grpc.rs | allowed_models refused, no owner | MATCH 148-160 (fn 154-160) | M; cited region outside LIVE hunks (hunks start 281) | 148-160 |
| C2 | gateway.proto:1335-1357 | apps/Model Plane/proto/model_plane/v1/gateway.proto | OrgPolicy.allowed_models | MATCH | clean | message 1343-1355; field 1350 |
| C3 | normalize.rs:360-384 | apps/Model Plane/rust/services/model-gateway/src/normalize.rs | ALLOWED_MODELS gate | MATCH (window holds gate 366-378) | M; gate moved +7 | loader 84-91 (both); gate HEAD 366-378, LIVE 373-383 |
| C4 | http_routes.rs:7590-7640 | apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | list_models ignores caller claims | DRIFT: list_models 7490-7529 (7590-7640 is create_document and ThreadMessage) | M; MATCH at LIVE 7603-7642 (`_claims` at 7605) | HEAD 7490-7529 (end inferred from LIVE shift +113); LIVE 7603-7642 |
| C5 | grpc.rs:394-424 | apps/Model Plane/rust/services/inference-core/src/grpc.rs | ListModels reads modality and provider only | DRIFT: fn 388-458 (return at 458); reads req.modality and req.provider at 394-399 and in every extend to 441 | M; fn starts 394 (+6) | HEAD 388-458; LIVE start 394 (end not read) |
| C6 | schema.ts:14-23 | apps/Application Plane/convex-core/convex/schema.ts | Convex mirror allowedModels, defaultModel | MATCH (settings 19-23; defaultModel 21; allowedModels 22) | clean | 19-23 |
| C7 | types.go:266-291 | apps/Control Plane/org-core/internal/org/types.go | org_entitlements table and GET | PARTIAL: 266-271 defaultEntitlements; 273-284 computed ZDR entitlement and plan map. Table is migrations/001_init.up.sql:15-20; GET is internal/http/server.go:113 | clean | defaults 266-271; add 001_init.up.sql:15 and server.go:113 |
| C8 | repository.go:663-667 | apps/Control Plane/org-core/internal/org/repository.go | org_entitlements upsert | MATCH (663-669, provisioning) | clean | 663-669; set and get SQL 815-855 |
| C9 | org_client.go:124-148 | apps/Control Plane/session-core/internal/clients/org_client.go | session-core entitlement client | MATCH (type 124-128; GetEntitlements from 136, read to 157) | clean | 124-157+ |
| C10 | org_quota.rs:1-60 | apps/Model Plane/rust/services/model-gateway/src/org_quota.rs | service-principal pull | PARTIAL: 1-62 is module doc and types. The pull is fetch_org_limits 112-141 | clean | pull 97-141 (credentials 129-136; fail-open doc 106-111) |
| C11 | convex-token.service.ts:150-215 | apps/Control Plane/auth-core/src/auth/convex-token.service.ts | 60 s retentionPosture precedent | MATCH (type 152-162; TTL const 173; issuedTokenZdr 197-210) | clean | 152-217 |
| C12 | sse.rs:1154-1155 | apps/Model Plane/rust/services/model-gateway/src/sse.rs | fail-open org_quota read | DRIFT: HEAD 1086-1087 | M; MATCH at LIVE 1154-1155 (+68) | HEAD 1086-1087; LIVE 1154-1155 |
| C13 | router_policy.rs:1-60 | apps/Frontend Plane/verevonv3/apps/gateway/src/domains/router_policy.rs | forwarded x-user-role is not authority | MATCH (doc 9-10; route 30; upstream URL 47) | clean | same |
| C14 | claims.rs:1-60 | apps/Model Plane/rust/crates/mp-authctx/src/claims.rs | no role in Claims | MATCH (Claims 9-34, no role field) | clean | same. model-gateway's own Claims at HEAD src/auth.rs:42-58 also has no role |
| C15 | CLAUDE.md:23 | CLAUDE.md (repo root) | Control Plane owns entitlements | MATCH | clean | 23 |

Tally: 10 MATCH, 3 DRIFT, 2 PARTIAL. All three DRIFT rows are HEAD-only: the audit read LIVE, where the cited lines match. Those three files (model-gateway http_routes.rs, sse.rs, inference-core grpc.rs) carry uncommitted ZDR work, so the audit's line numbers will move again when that work is committed.

## 3. Card claims checked without a line cite

- `list_models` takes no org or role input. HEAD `http_routes.rs:7492` binds `Extension(_claims)` and never reads it; the request is `ListModelsRequest::default()` (7499-7502). MATCH.
- ALLOWED_MODELS unset means every model is allowed: `normalize.rs:84-85` (doc) and `86-91` (returns None). MATCH.
- The allowlist refusal is untyped: HEAD `normalize.rs:368-376` returns 400 with `{"error": ...}` and no code. The helper `refusal(message, code)` at `normalize.rs:120-126` can carry a code, but the allowlist gate does not call it. MATCH.
- SetPolicy refuses the allowlist by name: HEAD `grpc.rs:2213-2226` refuses before applying anything, with `Status::unimplemented`. Test at `grpc.rs:2760-2771`. MATCH.
- Convex `allowedModels` and `defaultModel` are "never read for admission": MATCH for admission only. `apps/Application Plane/convex-core/convex/ai.ts:81` reads `org.settings.defaultModel` as the chat-model fallback. The card needs that qualifier.
- org_quota "fails open": `org_quota.rs:106-111` says so in its doc comment; HEAD `sse.rs:1086` repeats it. MATCH.
- `GET /orgs/{id}/entitlements`: `apps/Control Plane/org-core/internal/http/server.go:113` (also `:95` for `/api/v1/organizations/:id/entitlements`; `apps/Control Plane/org-core/README.md:23`). MATCH.
- session-core aggregates entitlements and publishes `app.session.entitlements_changed`: `apps/Control Plane/session-core/internal/service/control_session_service.go:33-42` (composes org-core entitlements), comments at 69 and 208 (publish); `internal/nats/shared_publisher.go:112-118`. MATCH.
- Router-policy admin page as UI neighbour: `apps/Frontend Plane/verevonv3/src/features/router-policy/components/RouterPolicyPage.tsx` (default export at 54), mounted at `src/features/settings/components/WorkspaceSettingsPage.tsx:379-380`. MATCH.
- ADR on entitlement ownership: ABSENT. `docs/architecture/` has three ADR files (agent loop, provider-side privacy tiers, surface-neutral ingress) and none covers entitlements. `apps/Model Plane/docs/decisions/ledger.md` has no OrgPolicy or model-entitlement entry (grep).
- Entitlement action contract: ABSENT. `apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts` has `router_policy.update` (3135-3144) and no entitlement or model-access action.

## 4. Affected-file map (step 4)

Lines are HEAD unless marked LIVE. "Tests" lists named tests seen by grep or read. "not enumerated" = not checked in this sweep.

### 4a. Control Plane (org-core is Go; all clean)

| Path | Role | Lines | Status | Tests |
|---|---|---|---|---|
| apps/Control Plane/org-core/internal/org/types.go | defaultEntitlements; computed ZDR entitlement; plan map; RetentionPosture | 266-291 | clean | org/retention_posture_test.go:28 (plan-lapse case) |
| apps/Control Plane/org-core/internal/org/repository.go | org_entitlements upsert at provisioning; set and get entitlement SQL | 663-669; 815-855 | clean | not enumerated |
| apps/Control Plane/org-core/internal/org/service_enhanced.go | entitlement load; 15 min cache (const 23); computed ZDR; cache invalidation | 23; 133-170; 393 | clean | org/retention_posture_postgres_test.go |
| apps/Control Plane/org-core/internal/http/handlers.go | getEntitlements handler | 379-390 | clean | internal/http/retention_posture_contract_test.go:302 |
| apps/Control Plane/org-core/internal/http/server.go | routes for entitlements (v1 and service path) | 95; 113 | clean | internal/http/service_auth_test.go:181, 195 |
| apps/Control Plane/org-core/internal/http/service_auth.go | machine scopes for GET entitlements and quotas: org:read:self or org:read:any | 290-291; 317-318 | clean | internal/http/service_auth_test.go:86-104 (registry validation cases) |
| apps/Control Plane/org-core/migrations/ | org_entitlements (001_init 15-20); org_quotas (002 line 2); RLS (008 78-82; 009 75; 013 12); GDPR delete (003 21-24) | per file | clean | none named |
| apps/Control Plane/session-core/internal/clients/org_client.go | Entitlement type; GetEntitlements to org-core | 124-157+ | clean | not enumerated |
| apps/Control Plane/session-core/internal/service/control_session_service.go | composes entitlements into session; publishes entitlements_changed | 33-42; 69; 208 | clean | not enumerated |
| apps/Control Plane/session-core/internal/nats/shared_publisher.go | publishes app.session.entitlements_changed | 112-118 | clean | not enumerated |
| apps/Control Plane/auth-core/src/auth/convex-token.service.ts | retentionPosture claim; 60 s TTL cap | 152-217 | clean | auth/tenant-retention-posture.spec.ts (exists; not read) |
| apps/Control Plane/auth-core/src/auth/model-plane-token.controller.ts | mints Model Plane audience tokens | not read | clean | model-plane-token.controller.spec.ts (not read) |
| apps/Control Plane/config/plane-service-principals.json | principal registry; model-gateway audiences session-core and quarry only | 289-307 | clean | auth/plane-service-principals-policy.contract.spec.ts (not read) |
| apps/Control Plane/CONTROL_PLANE_OWNERSHIP.md | owner rows; defaultModel in Convex mirror snippet | 33; 340; 241 | clean | n/a |

### 4b. Model Plane

| Path | Role | Lines | Status | Tests |
|---|---|---|---|---|
| apps/Model Plane/proto/model_plane/v1/gateway.proto | OrgPolicy; GetPolicy and SetPolicy RPCs and messages | 1343-1355; 165-166; 1357-1375 | clean | n/a |
| apps/Model Plane/proto/model_plane/v1/inference.proto | ListModels RPC; ListModelsRequest (modality, provider only); ModelInfo; response | 27; 484-490; 493; 523 | clean | n/a |
| apps/Model Plane/proto/model_plane/v1/routing_policy.proto | RoutingPolicy service; singleton JSON config | 23-32; 34-43 | clean | n/a |
| apps/Model Plane/rust/services/model-gateway/src/grpc.rs | unowned_policy_fields; set_policy refusal; get_policy calls fetch_org_limits | 148-160; 2167; 2177; 2202-2226 | M (LIVE hunks at HEAD 281-2513; cited regions outside them) | grpc.rs:2760 policy_fields_with_no_owning_service_are_named_not_silently_banked |
| apps/Model Plane/rust/services/model-gateway/src/normalize.rs | ALLOWED_MODELS loader; untyped gate; normalize (197) and normalize_stream (216) both call normalize_with (223); refusal helper 120-126 | loader 84-91; gate 366-378 (LIVE 373-383) | M | none asserts the refusal text (grep) |
| apps/Model Plane/rust/services/model-gateway/src/http_routes.rs | /v1/models (route 195) to list_models; /v1/ai/models (416) to ai_models (3577); /v1/ai/realtime/models (450, fn 4849); /v1/ai/video/models (457, fn 5002); fetch_org_limits in invoke (8964) | list_models 7490-7529 (LIVE 7603-7642) | M | mock list_models at 11082 only |
| apps/Model Plane/rust/services/model-gateway/src/org_quota.rs | org ceilings: service-credential pull; limits_from_rows; fail-open | 1-62; 79-95; 97-141 | clean | 163-200: micro_dollars_round_trip_without_truncation; both_run_ceilings_are_read_from_the_payload; a_zero_or_negative_ceiling_reads_as_unset_not_as_block_everything; a_missing_absent_or_malformed_payload_yields_no_ceilings |
| apps/Model Plane/rust/services/model-gateway/src/sse.rs | invoke stream: fail-open org_quota call; ZDR refusal streaming | HEAD 1086-1087 (LIVE 1154-1155) | M | not enumerated |
| apps/Model Plane/rust/services/model-gateway/src/state.rs | org-core base URL, service id and token | 155-164; 413-415; 605-609 | M | n/a |
| apps/Model Plane/rust/services/model-gateway/src/auth.rs | model-gateway Claims: zdr bool, no role | 42-58 | M | not enumerated |
| apps/Model Plane/rust/services/model-gateway/src/grpc_auth.rs | issuer ZDR view; effective_zdr at HEAD | not located in this sweep | M | not enumerated |
| apps/Model Plane/rust/services/model-gateway/src/budget.rs | consumer of OrgQuotaLimits; request cap beats org default | 45; 54; 300 | clean | not enumerated |
| apps/Model Plane/rust/services/model-gateway/src/zdr_precheck.rs | HEAD ZDR refusal struct and subscription refusal | 59; 68; 109; 119; 133 | M | not enumerated |
| apps/Model Plane/rust/services/model-gateway/src/rate_limit.rs | comment: OrgPolicy belongs at the edge | 30 | clean | n/a |
| apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs | ALLOWED_MODELS cleanup in tests | HEAD: 53 remove_var, 0 set_var | M | itself |
| apps/Model Plane/deploy/docker-compose.override.yml | sets ALLOWED_MODELS to empty | 12 | clean | n/a |
| apps/Model Plane/rust/services/inference-core/src/grpc.rs | ListModels; reject_unattested_zdr_modality (per-modality ZDR gate); min_privacy_tier wiring | HEAD 179; 388-458; 1102 (LIVE ListModels 394) | M | src/grpc/real_rules_tests.rs:773 (clean) |
| apps/Model Plane/rust/services/inference-core/src/auth.rs | inference principal claims: org_zdr, zdr_waiver | not located in this sweep | M | not enumerated |
| apps/Model Plane/rust/services/inference-core/src/provider/fallback.rs | provider chain; list_models over the chain | 1893 | clean | tests/fallback_test.rs:503 list_models_filters_by_modality_and_provider |
| apps/Model Plane/rust/services/inference-core/src/provider/mod.rs | residency floor doc; skip rule under zdr | 140-143 | clean | n/a |
| apps/Model Plane/rust/crates/mp-authctx/src/claims.rs | canonical Claims (user_id, org_id, scopes); no role | 9-34 | clean | not enumerated |
| apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs | typed ZDR refusal catalogue | LIVE 1-45 read | ?? | mp-contracts/tests/zdr_retention_matrix.rs (??) |
| apps/Model Plane/rust/crates/mp-contracts/src/lib.rs | module exports | 2 lines added | M | n/a |
| apps/Model Plane/rust/services/session-core/src/auth.rs | ZdrWaiver on persistent tokens | not located in this sweep | M | not enumerated |
| apps/Model Plane/go/services/capability-core/migrations/0003_capabilities_registry.up.sql | routing_policies (org_id default 'global'; model_ids TEXT[]; priority; enabled) | 262-286 | clean | n/a |
| apps/Model Plane/go/services/capability-core/internal/api/registry_apis.go | RoutingHandler CRUD on routing_policies | 1434; 1490; 1525; 1550; 1585; 1597 | clean | not enumerated |
| apps/Model Plane/docs/GATEWAY_DEDUPLICATION_PLAN.md | OrgPolicy decomposition; allowlist "still homeless"; fix direction | 225; 241; 388 | clean | n/a |
| apps/Model Plane/docs/capability-ownership-matrix.md | capability-core registry system of record; routing.policy.updated | 42; 106 | clean | n/a |
| apps/Model Plane/docs/decisions/ledger.md | no OrgPolicy or model-entitlement entry | n/a | clean | n/a |
| apps/Model Plane/go/gen/model_plane/v1/gateway.pb.go; apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/gateway_pb2.py | generated OrgPolicy bindings | n/a | clean | n/a |

### 4c. Application Plane and Frontend Plane

| Path | Role | Lines | Status | Tests |
|---|---|---|---|---|
| apps/Application Plane/convex-core/convex/schema.ts | organizations.settings: maxTokens, defaultModel, allowedModels | 19-23 | clean | convex-core test/*.test.cjs |
| apps/Application Plane/convex-core/convex/ai.ts | defaultModel as chat fallback (not admission) | 81 | clean | not enumerated |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/router_policy.rs | BFF proxy for GET and PUT router policy | 1-60 (route 30; upstream 47) | clean | apps/gateway/src/main.rs:1204-1237 |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/orgs.rs | BFF GET /api/v1/orgs/{id}/entitlements | 33-34 | clean | apps/gateway/src/main.rs:1026 |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/dispatchers.rs | dispatch_router_policy_update | 5383-5407 | clean | not enumerated |
| apps/Frontend Plane/verevonv3/apps/gateway/src/domains/actions/handlers.rs | routes router_policy.update | 368 | clean | n/a |
| apps/Frontend Plane/verevonv3/src/shared/actions/action-registry.ts | router_policy.update descriptor | 3135-3144 | clean | action-surface-contract.test.mjs (named in package.json test script; not read) |
| apps/Frontend Plane/verevonv3/src/shared/api/router-policy-client.ts | client for router policy | whole file | clean | router-policy-client.test.ts |
| apps/Frontend Plane/verevonv3/src/features/router-policy/components/RouterPolicyPage.tsx | admin page (UI neighbour) | 54; 60; 95 | clean | not enumerated |
| apps/Frontend Plane/verevonv3/src/features/settings/components/WorkspaceSettingsPage.tsx | mounts router-policy section | 2; 219; 379-380 | clean | WorkspaceSettingsPage.zdr-gate.test.tsx:68-71 (mocks /api/v1/orgs/org_1/entitlements) |
| apps/Frontend Plane/verevonv3/src/features/core/lib/sidebar-navigation.ts | nav entry | 380 | clean | n/a |
| apps/Frontend Plane/verevonv3/src/app/App.tsx | references RequireWorkspaceAdmin (definition not located) | not located | M | n/a |
| apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/gateway_pb.ts | generated OrgPolicy TS type (old `verevon` app, not verevonv3) | 3850-3898 | clean | n/a |

### 4d. Root and docs

| Path | Role | Lines | Status |
|---|---|---|---|
| CLAUDE.md (repo root) | Control Plane owns quotas and entitlements | 23 | clean |
| apps/master-ownership-matrix.md | owner table and rules | 127; 130; 160; 165 | clean |
| docs/architecture/adr-provider-side-orchestration-privacy-tiers.md | existing ADR on privacy tiers (no entitlement text) | 13 | clean |

## 5. Related contracts (step 5)

- **Protos**: `gateway.proto` OrgPolicy (`allowed_models = 4` at 1350) and GetPolicy and SetPolicy RPCs (165-166). `inference.proto` ListModels (27) and ListModelsRequest (484-490). `routing_policy.proto` RoutingPolicy (23-43; comment 13-17 says session-core owns the durable store). Generated bindings: `apps/Model Plane/go/gen/model_plane/v1/gateway.pb.go`, `apps/Model Plane/python/mp-events-py/src/mp_events/_gen/model_plane/v1/gateway_pb2.py`, `apps/Frontend Plane/verevon/src/lib/model-plane/gen/model_plane/v1/gateway_pb.ts` (3850-3898). Regenerate with `buf generate`.
- **SQL migrations** (directory listings in both trees):
  - org-core `apps/Control Plane/org-core/migrations/`: 37 files; latest up file `020_interactive_retention_outbox_org_id_check.up.sql`. HEAD equals LIVE. No migration holds `allowed_models` (grep across apps).
  - session-core (Model Plane) `apps/Model Plane/rust/services/session-core/migrations/`: HEAD latest `0043_audit_outbox_requeue.sql` (43 entries). LIVE adds `0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql` and `revert/` (all ??; 46 entries).
  - capability-core `apps/Model Plane/go/services/capability-core/migrations/`: latest `0015_background_process_capability.up.sql`. `routing_policies` is in `0003_capabilities_registry.up.sql`.
- **Config and env**:
  - `ALLOWED_MODELS` (env read at `normalize.rs:86-87`). `apps/Model Plane/deploy/docker-compose.override.yml:12` sets it to empty.
  - model-gateway org-core identity: `MODEL_GATEWAY_SERVICE_ID` (`state.rs:605`; default `model-gateway` at 414); `MODEL_GATEWAY_ORG_CORE_SERVICE_TOKEN` (607-608); base URL default `http://localhost:3010` (413), read at 609. Org-core side: `ORG_CORE_SERVICE_CREDENTIALS` (named in the `state.rs:159` doc).
  - Control Plane principal registry: `apps/Control Plane/config/plane-service-principals.json` (model-gateway 289-307; consumerEnv `MODEL_GATEWAY_SERVICE_API_KEY` at 295).
- **Error and SSE contracts**:
  - `error` control event: `apps/Model Plane/docs/sse-event-taxonomy.md:24-25` (ChatEvent::Error, ChatEvent::SubscriptionError).
  - LIVE-only typed ZDR error frame `{code, message, retryable, action, feature?}`: `mp-contracts/src/zdr_refusal.rs:15-16` (??).
  - gRPC refusal at HEAD: `Status::unimplemented` in `model-gateway/src/grpc.rs` (2222).
  - NATS `app.session.entitlements_changed`: published by session-core (`nats/shared_publisher.go:118`); consumed by notification-core (`apps/Application Plane/notification-core/internal/consumers/control_session.go:25`).
- **Gateway and HTTP routes**:
  - Model listing, all proxying inference-core ListModels: model-gateway `/v1/models` (http_routes.rs 195), `/v1/ai/models` (416), `/v1/ai/realtime/models` (450), `/v1/ai/video/models` (457).
  - org-core: `GET /api/v1/organizations/:id/entitlements` (server.go:95); `GET /orgs/:id/entitlements` (113). Quota pull `GET /api/v1/organizations/{org}/quotas` (org_quota.rs:129-131; route registration not located).
  - BFF: `GET /api/v1/orgs/{id}/entitlements` (`orgs.rs:33-34`); `GET|PUT /api/v1/router-policy` (`router_policy.rs:30`), proxied to inference-core `/internal/v1/router-policy` (47).
- **Action contracts**: `router_policy.update` only (`action-registry.ts` 3135-3144; ownerPlane model; risk high; requiresApproval true; description at 3137 says the owner enforces no admin gate). No entitlement or model-access action.
- **Envelope rules**: root CLAUDE.md:46 `{ error: { code, message, details } }`. The LIVE ZDR catalogue uses `{code, message, retryable, action, feature?}` (`zdr_refusal.rs:15-16`).

## 6. Test and build commands (step 6)

- **Rust, Model Plane workspace** (`apps/Model Plane/rust`): CI runs `cargo test --workspace --no-fail-fast` (`.github/workflows/model-plane-rust-ci.yml:57` working dir, `:119` command). Per crate: `cargo test -q -p model-gateway --all-targets` and `cargo test -q -p inference-core --all-targets` (Model Plane CLAUDE.md, Testing). Packages: `model-gateway` (`services/model-gateway/Cargo.toml:2`), `inference-core` (`services/inference-core/Cargo.toml:2`; lib `inference_core` at :54), `mp-contracts` (`crates/mp-contracts/Cargo.toml:2`), `mp-authctx` (`crates/mp-authctx/Cargo.toml:2`). session-core package name not read; LIVE CI adds `cargo test -p session-core --bin session-core zdr_receipt -- --ignored`. User memory: two cargo targets fail pre-existing on Windows; use `--lib`.
- **Go**:
  - org-core: module `github.com/I-Dacosta/AquatiqCMS/apps/org-core` (`apps/Control Plane/org-core/go.mod:1`). CI `go test -v -p 1 ./...` in `apps/Control Plane/org-core` (`.github/workflows/org-service.yml:36, 103`). No Makefile.
  - session-core (Control Plane): module `github.com/I-Dacosta/CoreSystem/apps/session-core` (`go.mod:1`). CI `go test -v ./...` (`.github/workflows/session-core-service.yml:29, 43`).
  - capability-core: module `github.com/triodelab/model-plane/services/capability-core` (`go.mod:1`). `go build ./... && go vet ./... && go test ./...` (Model Plane CLAUDE.md). No go.work; build per service.
- **TypeScript and gateway**:
  - verevonv3 `package.json` scripts: `lint` (eslint .), `typecheck` (tsc -b), `test` (node --test over four gateway scripts, then vitest run). Root CLAUDE.md: `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`.
  - gateway crate: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (root CLAUDE.md, Build and Run).
  - convex-core: `apps/Application Plane/convex-core/package.json:12` test script `tsx --test test/*.test.cjs`.
  - auth-core: `test:cov` (root CLAUDE.md, Testing). User memory: full suite has 15 pre-existing failures in 3 suites; compare failing sets with HEAD and do not trust exit codes.
- **Proto**: `buf generate` (Model Plane CLAUDE.md). `buf lint` has pre-existing debt.

## 7. Owners (step 7)

- **Entitlement storage and decision: Control Plane.** CLAUDE.md:23 ("quotas, and entitlements"). CONTROL_PLANE_OWNERSHIP.md:33 (org_entitlements) and :340 (org-core stores the plan entitlement; auth-core resolves and signs). Membership: master-ownership-matrix.md:165 (rule 9).
- **Admission enforcement: Model Plane.** CLAUDE.md:27 (inference, capabilities, cost). Model Plane CLAUDE.md Code Style: Rust owns the request hot path; Go owns durable control.
- **Model routing: overlap not resolved in code.** master-ownership-matrix.md:127 (inference-core: provider routing) and :130 (capability-core: "model routing"). capability-ownership-matrix.md:106 (capability-core is the registry system of record). capability-core `routing_policies` (0003:262-286) has `model_ids`. routing_policy.proto is session-core's store with inference-core's live view. GATEWAY_DEDUPLICATION_PLAN.md:388 proposes moving the allowlist to capability-core routing_policies or the inference-core router-policy endpoint. The card proposes a third home (org-core). Model Plane CLAUDE.md: "One canonical owner per capability."
- **Admin UI and action contract: Frontend Plane.** CLAUDE.md:29; action contracts in `src/shared/actions` (CLAUDE.md:43); gateway domains in `apps/gateway/src/domains/*`.
- **ZDR propagation across content-persisting boundaries:** master-ownership-matrix.md:160 (rule 4); CLAUDE.md:35.

## 8. Uncommitted-only (step 8)

### 8a. LIVE-only (??) files relevant to the design

- `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs`: typed refusal catalogue. Three transports (doc 13-20). Codes include the frozen `zdr_unavailable` (line 43). Named refusals used by LIVE sse.rs and normalize.rs include `CONNECTION_NOT_SUPPORTED`, `INFERENCE_FAILED`, `UNAVAILABLE` and `WAIVER_ROUTE_MISMATCH`. Strongest existing precedent for a typed model refusal.
- `apps/Model Plane/rust/crates/mp-contracts/src/retention.rs`; `tests/zdr_retention_matrix.rs`; `tests/fixtures/`.
- `apps/Model Plane/api/contracts/zdr-refusals.json` (+ `.sha256`): code catalogue keyed by the SPA (Z-05b).
- `apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs` and `zdr_features.rs`: posture resolver (zdr_or_refuse, continuation_gate, decide_for, Route), named in the LIVE diffs of http_routes.rs and auth.rs.
- `apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs`, `zdr_feature_table.rs`, `zdr_refusal_inventory.rs`; `tests/fixtures/zdr-posture-matrix.json` (+ `.sha256`); `tests/fixtures/auth-core/`.
- `apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs`; `apps/Model Plane/rust/services/inference-core/tests/zdr_refusal_inventory.rs`; `apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs`.
- `apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `migrations/revert/`; `src/retention_receipt.rs`.
- `apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts` (+ `zdr-refusals.test.ts`); `src/shared/api/contracts/`; `src/shared/api/chat-client.zdr-refusal.test.ts`; `src/shared/api/http.zdr-refusal.test.ts`; `src/shared/i18n/errors.zdr-refusal.test.ts`; `src/features/chat/components/chat-normalizers.zdr-refusal.test.ts`; `src/features/chat/components/use-chat-controller.zdr-refusal.test.ts`.

### 8b. LIVE-modified (M) files relevant to the design

Stats from `git --no-optional-locks diff --stat` in `C:/dev/CoresSystem`. Paths are relative to `apps/Model Plane/rust/services/` unless shown.

| File | Stat | What changed in LIVE |
|---|---|---|
| model-gateway/src/normalize.rs | 17 +- | +8 lines: ZDR subscription refusal before the ALLOWED_MODELS gate (after HEAD 295). Gate body unchanged; now 373-383 |
| model-gateway/src/grpc.rs | 47 +- | ZDR wording and gating edits in 13 HEAD hunks, 281-2513. unowned_policy_fields (148-160) and set_policy refusal (2202-2226) are outside them |
| model-gateway/src/http_routes.rs | 237 ++- | ZDR gating moved to crate::zdr_posture (zdr_or_refuse, continuation_gate, decide_for) across handlers. list_models body unchanged; shifted +113 |
| model-gateway/src/sse.rs | 883 ++- | adds zdr_error_stream, prepared_zdr_refusal_stream and a zdr_refusal_stream closure (900). fetch_org_limits call shifted +68 |
| model-gateway/src/auth.rs | 415 ++ | adds ZdrWaiverClaim and org_zdr; effective_zdr removed |
| model-gateway/src/grpc_auth.rs | 199 ++ | adds issuer_org_zdr and a posture view via zdr_posture; effective_zdr removed |
| model-gateway/src/zdr_precheck.rs | 284 +- | local ZdrRefusal and refuse_subscription_under_token_zdr replaced by mp_contracts Refusal |
| model-gateway/tests/e2e_invoke_chain_test.rs | 288 ++ | M; not itemised (HEAD: 53 ALLOWED_MODELS cleanups) |
| model-gateway/src/state.rs | not stat'd | M; not itemised |
| inference-core/src/grpc.rs | 131 ++- | adds enforce_non_chat_zdr_posture and enforce_zdr_route calls and a zdr_refusal_catalogue test module. list_models body unchanged; fn now at 394 (+6) |
| inference-core/src/auth.rs | 404 ++ | adds org_zdr and zdr_waiver (ZdrWaiver) fields; imports Refusal |
| session-core/src/auth.rs | 396 +++ | adds ZdrWaiver accessor and a waiver field on persistent tokens |
| mp-contracts/src/lib.rs | 2 + | pub mod retention; pub mod zdr_refusal |
| mp-contracts/src/dataplane_posture.rs | not stat'd | M; not itemised |
| proto/model_plane/v1/sessions.proto | 42 + | M; not itemised; not a model-entitlement contract |
| docs/ZDR.md | 64 +- | M; not read |
| docs/sse-event-taxonomy.md | 4 + | M; not read |
| docs/core-research/model-gateway.md | 9 + | M; not read |
| api/contracts/README.md | 1 + | M; not read |
| .github/workflows/model-plane-rust-ci.yml | 50 ++ | adds a session-core retention-receipt Postgres job; no entitlement test |
| apps/Frontend Plane/verevonv3/package.json | 2 + | adds demo:build and demo:probe scripts only |
| apps/Frontend Plane/verevonv3/src/app/App.tsx | not stat'd | M; not read |
| verevonv3 src/shared/i18n/errors.ts; src/shared/api/http.ts | not stat'd | M; refusal text and HTTP error handling; not read |

Clean (LIVE equals HEAD) for: all org-core and Control Plane session-core files; auth-core `convex-token.service.ts`; `plane-service-principals.json`; `CONTROL_PLANE_OWNERSHIP.md`; convex-core; the gateway, inference and routing_policy protos; capability-core migration 0003 and `registry_apis.go`; the BFF router-policy, orgs, dispatchers and handlers files; `action-registry.ts`; `RouterPolicyPage.tsx`; `router-policy-client.ts`; `WorkspaceSettingsPage.tsx`; `master-ownership-matrix.md`; `capability-ownership-matrix.md`; `ledger.md`; `GATEWAY_DEDUPLICATION_PLAN.md`; root `CLAUDE.md`.

## 9. Findings that bear on the card (from the code)

1. **Admission today is a deployment-wide env list, not an org policy.** `normalize.rs:86-91` reads ALLOWED_MODELS; unset allows all. The refusal is an untyped 400 (HEAD 368-376), even though `refusal()` (120-126) takes a code. No test asserts the refusal text.
2. **Listing has no identity.** list_models binds `_claims` and sends no org or model filter. Four routes proxy the same ListModels: `/v1/models`, `/v1/ai/models` (ai_models 3577 takes only the query modality and provider), `/v1/ai/realtime/models`, `/v1/ai/video/models`. All four need the filter.
3. **The only org ceiling pull fails open.** `org_quota.rs:106-111` documents it. Call sites: sse.rs (HEAD 1086-1087), http_routes.rs (HEAD 8964), grpc.rs get_policy (HEAD 2177). The card's fail-closed requirement needs a different read path (last-known-good with a short TTL, or refuse).
4. **No service principal can read entitlements yet.** model-gateway's org-core identity is a service id and token (`state.rs:160-164`). Its Control Plane registry entry (`plane-service-principals.json:289-307`) has no org-core audience. org-core's entitlements and quotas GET accept only `org:read:self` or `org:read:any` (`service_auth.go:290-291`). `docker-compose.yml:534` describes `org:read:any` as the whole organization record and says nobody else holds it. A narrow entitlement scope does not exist.
5. **Convex mirror:** `allowedModels` is never read. `defaultModel` is read as a chat-model fallback (`ai.ts:81`). "Never read" holds only for admission.
6. **Owner overlap already exists.** capability-core `routing_policies` has `model_ids` (0003:262-286), and master-ownership-matrix.md:130 gives capability-core "model routing". GATEWAY_DEDUPLICATION_PLAN.md:388 names that as the allowlist home. The ADR must pick between that and org-core.
7. **The typed-refusal precedent is LIVE-only.** `mp-contracts/src/zdr_refusal.rs` is untracked. HEAD has the older ZdrRefusal struct at `zdr_precheck.rs:68`. Model refusals should join one catalogue; the envelope rule is CLAUDE.md:46.
8. **inference-core already has a per-modality ZDR gate at HEAD.** `reject_unattested_zdr_modality` (`inference-core/src/grpc.rs:179`) is called from speech, translation, language, image and document handlers. LIVE adds enforce_zdr_route and enforce_non_chat_zdr_posture. This is the defence-in-depth precedent for the card's inference-core check.
9. **Closest existing entitlement pattern:** org-core's computed `feature.zero_data_retention` is plan-gated (`types.go:273-284`). No model-level entitlement key exists.
10. **Line numbers drift.** Three of 15 citations (sse.rs +68, http_routes.rs +113, inference-core grpc.rs +6) and the normalize gate (+7) move with uncommitted ZDR work. A plan meant to survive the next commit should cite HEAD line numbers.
11. **Frontend has no entitlement action.** The router_policy.update descriptor says the owner enforces no admin gate (action-registry.ts:3137). An admin UI would start with a new action contract (CLAUDE.md:43).
12. **No ADR exists for entitlement ownership.** Existing homes are `docs/architecture/adr-*.md` (three files) and the Model Plane `docs/decisions/ledger.md` (no OrgPolicy entry).

## 10. NOT DETERMINED (step 9)

- Whether model-gateway's org-core credential is deployed anywhere. Not in `apps/Control Plane/docker-compose.yml` (grep). Other env files and the `ORG_CORE_SERVICE_CREDENTIALS` source were not searched.
- Whether a narrower entitlement scope is needed, or whether model-gateway may hold `org:read:any`. This is a design decision; no such scope is defined.
- Which inference-core function selects the provider chain under a privacy tier. Only `list_models` (`provider/fallback.rs:1893`) and `min_privacy_tier` (`grpc.rs:1102`) were located.
- Whether any runtime path reads capability-core `routing_policies`. Not traced.
- Whether ALLOWED_MODELS is set in any deployed environment beyond `deploy/docker-compose.override.yml`. Root-level env files and other compose files were not searched.
- Test status. No tests were run (read-only sweep). Pre-existing failures in auth-core and verevonv3 (user memory) are unverified here.
- Content of unread tests and specs: `auth/tenant-retention-posture.spec.ts`, `auth/plane-service-principals-policy.contract.spec.ts`, `model-plane-token.controller.spec.ts`; any org-core or session-core GetEntitlements unit tests (none named by grep).
- Definition of `RequireWorkspaceAdmin` (only the App.tsx reference was seen, and App.tsx is M).
- Body of `info::get_org_entitlements` (BFF) and the context of `middleware.rs:813`.
- org-core route registration for `/api/v1/organizations/:id/quotas` (not in the server.go lines read; the service_auth.go scope table lists it).
- LIVE end of inference-core `list_models` (HEAD verified 388-458; LIVE end not read) and the full LIVE `Refusal` constant list (only names seen in grep).
- Exact live location of `grpc_auth.rs` and `inference-core/src/auth.rs` symbols named in section 8b (not located).
- Whether the audit's external claims (ChatGPT, Claude, Codex, T3 Code) hold. These are DOC or SRC claims, not code-checkable in this sweep.
- Where the new ADR should live: `docs/architecture/adr-*.md` or the Model Plane ledger. Not decided here.
