# CoreSystem Audit Backlog

Updated: 2026-07-02

Scope: documentation refresh, validation evidence, and low-risk smoke-script maintenance.

Focused planes:
- Frontend Plane: `apps/Frontend Plane/velionv3`
- Data Plane v2: `apps/Data Plane v2`
- Ingestion Plane: `apps/Ingestion Plane`
- Model Plane: `apps/Model Plane`
- Control Plane: `apps/Control Plane`
- Application Plane: `apps/Application Plane`

Priority key:
- P0: production/blocking boundary issue.
- P1: failing gate or high-risk implementation gap.
- P2: maintainability, test coverage, or onboarding drift.
- P3: cleanup or documentation polish.

Status key:
- Confirmed: observed in source, docs, or a command result in this pass.
- Needs verification: likely from current evidence, but not validated by a targeted command.

## Verification Run

| Area | Command | Result | Notes |
|---|---|---|---|
| Cross-plane runtime map | `docs/CORESYSTEM_CROSS_PLANE_ARCHITECTURE_MAP.md` | Added | Single consolidated map now records authority boundaries, Velion v3 gateway wiring, live proof, and remaining integration gaps. |
| Compose config | `docker compose config --quiet` for Frontend Velion v3, Control, Data v2, Ingestion, Application, and Model | Pass | All six main compose files parse with required variables supplied. |
| Runtime network | `docker network inspect inter-plane-bus` | Pass | Network exists with 50 observed containers. |
| Gateway cross-plane reachability | `docker exec velion-gateway-rs ... curl downstream health/session endpoints` | Pass | Gateway container reached Control, Data, Ingestion, Model, and Application service names over `inter-plane-bus`. |
| Model cross-plane reachability | `docker exec model-plane-model-gateway-1 ... curl downstream health/session endpoints` | Pass | Model gateway reached Control auth session, Data services, Quarry edge, and internal Model services. |
| Data Plane v2 HTTP smoke | `set -a; . ./.env; set +a; bash scripts/smoke-test.sh --http-only` | Pass | 13 passed, 0 failed: health, readiness, document create/get/delete. |
| Data Plane v2 Quickwit smoke | `bash scripts/smoke-quickwit-retrieval.sh` | Pass | Rust fallback test passed; retrieval reports Quickwit-with-Postgres fallback; source object indexed after 30 attempts. |
| Model Plane gRPC smoke | `node scripts/smoke-wave10.js` | Pass | model-gateway gRPC Health, plan mode, team, MCP/plugin, policy, message, task, and trajectory checks passed. |
| Ingestion integration-api smoke | `bash smoke-test-integration-api.sh` | Fail on real boundary | Script now matches current routes and portable parsing; it fails because GitHub webhooks accept missing/invalid signatures with 200. |
| Control integration smoke | `bash test-control-plane-integration.sh` | Pass | Updated script checks current auth session endpoint, service health ports, audit-core 8187, and `controlplane-postgres`; 9 passed, 0 failed. |
| Model durable-layer script | `bash scripts/verify-durable-layer.sh` | Pass | Updated script waits for SQL readiness before migrations; durable-layer assertions pass against throwaway Postgres. |
| Velion auth gateway probe | throwaway signup/signin curl flow | Partial | Signup succeeds through gateway; signin returns `EMAIL_NOT_VERIFIED`; protected `/api/v1/me` and session-context remain 401. |
| Quarry edge onboarding probe | source/runtime curl inspection | Open | Velion onboarding still calls `quarry-control`; edge `/v1/jobs` returns 401 from host while control lists jobs under rollout mode. |
| Data Control consultation probe | source/runtime inspection | Open/partial | retrieval-engine is strict; documents-api `authctx` enforce path still returns 503 because signature verification is not implemented. |
| Velion v3 frontend | `pnpm lint` in `apps/Frontend Plane/velionv3` | Pass | ESLint completed. |
| Velion v3 frontend | `pnpm typecheck` in `apps/Frontend Plane/velionv3` | Pass | `tsc -b` completed. |
| Velion v3 frontend | `pnpm test` in `apps/Frontend Plane/velionv3` | Fail | 46 test files and 202 tests passed, but Vitest failed on 2 unhandled rejections from `loadStudioWorkspace`; also emitted a `--localstorage-file` warning. |
| Velion v3 gateway | `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` | Pass | 146 Rust gateway tests passed. |
| Data Plane v2 Rust | `make check-rs` in `apps/Data Plane v2` | Pass with warning | `retrieval-engine-rs` reports unused `RerankClient::new`. |
| Data Plane v2 Go | `make build-go` in `apps/Data Plane v2` | Pass | `documents-api-go`, `wiki-store-go`, `data-orchestrator-go`, and `data-quality-go` built. |
| Ingestion Quarry-v2 | `cargo test --workspace` in `apps/Ingestion Plane/Quarry-v2` | Fail | Compile failure: `DataPlaneIngestRequest` constructors missing `initiator_user_id` and `visibility`. |
| Model Plane Rust | `cargo test --workspace` in `apps/Model Plane/rust` | Pass | Full Rust workspace passed; several live-DB/browser/doc examples are intentionally ignored. |
| Model Plane Go packages + orchestrator | `go test ./...` across core packages and `services/orchestrator-core` | Fail | Core packages passed; `orchestrator-core/internal/orchestration` test stubs do not implement generated `ListPendingApprovals`. |
| Model Plane Go services | `go test ./...` across capability, browser, sandbox, cost, bridge, and letta services | Fail | capability/browser/sandbox/cost/bridge passed; `letta-bridge/internal/memstore` failed `TestTimeRangeFiltering/cutoff_excludes_old_record`, got 2 hits and wanted 1. |
| Control Plane Go | `go test ./...` in audit, billing, org, session, and user services | Pass | Checked Go services passed. |
| Control Plane auth-core | `pnpm exec jest --runInBand` in `auth-core` | Pass | 1 suite, 4 tests passed; Node emitted the recurring `--localstorage-file` warning. |
| Application Plane Go | `go test ./...` in conversation, information, insight, leads, notification, and social services | Pass | Checked Go services passed. |
| Application Convex | `pnpm typecheck` / `pnpm lint` in `convex-core` | Blocked | pnpm exits before scripts because `esbuild@0.27.0` has ignored build scripts pending approval. |
| Application Convex direct binaries | `./node_modules/.bin/tsc --noEmit` and `./node_modules/.bin/eslint "convex/**/*.ts"` | Pass | TypeScript and ESLint pass when bypassing the pnpm script wrapper. |
| Repo inventory | Pruned manifest scan under `apps` | Pass | Clean inventory requires pruning `node_modules`, `target`, `.next`, `.claude/worktrees`, `.fallow`, and `.playwright-mcp`. |

## Highest Priority Backlog

| Priority | Status | Plane | Finding | Evidence | Recommended next action |
|---|---|---|---|---|---|
| P0 | Confirmed | Data/Control | Data Plane v2 Control consultation is only partially closed: retrieval-engine is strict, but documents-api still has observe-mode `authctx` with unimplemented signature verification. | `documents-api-go/pkg/authctx/authctx.go` returns 503 for `AUTHCTX_ENFORCE=1`; live documents API is reachable but not proven with verified JWT consultation. | Implement documents-api JWT/JWKS verification and add tenant-isolation integration tests. |
| P0 | Confirmed | Ingestion/Frontend/Control | Quarry-v2/Velion onboarding still posts directly to `quarry-control` `/v1/jobs/`, bypassing `quarry-edge` as the cross-plane entrypoint. | Velion gateway source uses `state.quarry_control_url`; host probe showed edge `/v1/jobs` returns 401 while control `/v1/jobs` lists jobs under rollout mode. | Migrate onboarding crawl handlers to `quarry-edge`; block direct control calls from gateway/frontend domains. |
| P0 | Confirmed | Ingestion | Integration API accepts GitHub webhooks with missing or invalid signatures in the live environment. | Updated `smoke-test-integration-api.sh` fails because `/api/v1/webhooks/github` returns 200 accepted for missing and invalid signature headers. | Fail closed when provider webhook secrets are absent and keep the smoke script as regression coverage. |
| P1 | Confirmed | Frontend | `pnpm test` fails because `loadStudioWorkspace` reads `ctx?.orgs[0]?.id`; optional chaining protects `ctx` but not missing `orgs`. | `apps/Frontend Plane/velionv3/src/features/studio/lib/studio-canvas-model.ts:43`; Vitest reports two unhandled rejections from `StudioPage.test.tsx`. | Change to a safe `ctx?.orgs?.[0]?.id` shape, add a regression test for session context without `orgs`, rerun `pnpm test`. |
| P1 | Confirmed | Ingestion/Data | Quarry-v2 test compilation is behind the Data Plane ingest contract. | `crates/quarry-core/tests/contracts.rs:231` and `crates/quarry-runtime/src/ingest_client.rs:380` construct `DataPlaneIngestRequest` without `initiator_user_id` and `visibility`. | Update constructors and contract tests; decide the correct default visibility and initiator semantics before patching. |
| P1 | Confirmed | Model | Model Plane Go `orchestrator-core` tests no longer compile against the generated orchestration client interface. | `internal/orchestration/handlers_test.go` stubs are missing `ListPendingApprovals`. | Update the test stub or generate/use a compliant fake client, then rerun `go test ./...` in `services/orchestrator-core`. |
| P1 | Confirmed | Model | Model Plane Go `letta-bridge` memstore time-range filtering test fails. | `letta-bridge/internal/memstore` `TestTimeRangeFiltering/cutoff_excludes_old_record` got 2 hits and wanted 1. | Inspect cutoff inclusivity/time source behavior; add a regression test around old-record exclusion. |
| P1 | Confirmed | All | Repository discovery and CodeGraph/static inventories can be polluted by nested `.claude/worktrees`, package caches, and build outputs. | Naive manifest scans surfaced duplicated plane manifests from `.claude/worktrees`; CodeGraph status includes 11,635 indexed files and should be treated as possibly including generated/local artifacts until excludes are audited. | Add explicit index/tooling excludes for `.claude/worktrees`, `node_modules`, `.next`, `target`, `.fallow`, `.playwright-mcp`; regenerate CodeGraph after cleanup. |
| P1 | Confirmed | Ingestion | Top-level Ingestion Makefile still points some commands at legacy `Quarry`, not active `Quarry-v2`. | `apps/Ingestion Plane/Makefile` targets `setup`, `dev-quarry`, `test-quarry`, and docs output reference `Quarry`. | Update Makefile targets/docs to make Quarry-v2 the active path and keep legacy commands clearly marked. |
| P1 | Confirmed | Frontend/All | No authenticated Velion v3 browser journey proves all active planes end to end. | No Playwright dependency/config exists; throwaway gateway signup succeeds but signin is blocked by `EMAIL_NOT_VERIFIED`, leaving protected routes 401. | Add a verified test account/session fixture and authenticated Playwright smoke that touches Control, Data, Ingestion, Model, and Application through the gateway. |

## Frontend Plane: Velion v3

| Priority | Status | Finding | Evidence | Recommended next action |
|---|---|---|---|---|
| P1 | Confirmed | Vitest fails on unhandled `loadStudioWorkspace` rejection despite all tests passing. | `pnpm test`; `src/features/studio/lib/studio-canvas-model.ts:43`. | Fix missing optional chaining and add regression coverage. |
| P1 | Confirmed | Velion v3 has three frontend/runtime surfaces that need explicit ownership: Solid/Vite root, Rust gateway, nested Next `apps/velion-web`. | `apps/Frontend Plane/velionv3/README.md`, root `package.json`, `apps/gateway/Cargo.toml`, `apps/velion-web/package.json`. | Add a short frontend-plane ownership doc or expand README with deploy/runtime boundaries. |
| P2 | Confirmed | `apps/velion-web` README is the generated Next template, not CoreSystem documentation. | `apps/Frontend Plane/velionv3/apps/velion-web/README.md`. | Replace with project-specific purpose, commands, routes, data boundaries, and deployment notes. |
| P2 | Confirmed | Global CSS is very large and likely hard to maintain. | `apps/Frontend Plane/velionv3/src/styles/global.css` has 30,700 lines. | Split by tokens/layout/features, or document the current generated/manual ownership before further UI work. |
| P2 | Confirmed | Several frontend surfaces intentionally expose fallback/planned/preview data paths. | `src/features/social/lib/social-workspace.ts`, `src/shared/read-data/index.ts`, agent/studio comments and tests. | Inventory fallback views and classify each as acceptable honest empty state, planned state, or needs live owner-plane wiring. |
| P2 | Confirmed | Root docs previously described Velion v2 Server Components/BFF while active target is Velion v3 Solid/Vite plus Rust gateway. | Old `AGENTS.md`, `CLAUDE.md`, and `apps/CODEBASE_INFORMATION_SYSTEM.md` referenced `velionv2`. | Done in this pass; keep future Velion v2 references explicitly historical. |
| P2 | Needs verification | Dev-auth bypass flags need environment hardening review. | `apps/gateway/src/middleware.rs` and config comments mention dev-only fallback and bypass behavior. | Add a focused config/security review to ensure bypass cannot activate in production profiles. |
| P3 | Confirmed | Velion v3 action registry currently has 25 action IDs across knowledge, operating map, security, inbox, tickets, social, agents, and workflows. | `src/shared/actions/action-registry.ts`. | Add a generated action inventory doc or test that action ownerPlane/risk/approval metadata remains complete. |

## Data Plane v2

| Priority | Status | Finding | Evidence | Recommended next action |
|---|---|---|---|---|
| P0 | Confirmed | Multi-tenant production readiness is blocked by incomplete Control Plane consultation. | retrieval-engine is strict, but documents-api `authctx` still has unimplemented signature verification and observe-mode header trust. | Implement documents-api JWT/JWKS verification and add cross-tenant denial tests. |
| P1 | Confirmed | Data ingest contract changes have not propagated to Quarry-v2 tests/runtime test constructors. | Quarry-v2 compile failure references `DataPlaneIngestRequest` missing `initiator_user_id` and `visibility`. | Patch Ingestion Plane contract call sites and decide migration/default semantics. |
| P2 | Confirmed | Rust check passes but reports unused rerank constructor. | `make check-rs`; `services/retrieval-engine-rs/src/search/rerank.rs:46`. | Remove if dead or wire through the intended provider configuration path. |
| P2 | Confirmed | Some docs still describe scaffold/lab placeholders in otherwise complete architecture. | `docs/gap-data.md` marks `retrieval-eval-py` as scaffold and lists remaining graph/export/rebuild follow-ups. | Convert remaining unchecked items into service-owned issues with owners and gates. |

## Ingestion Plane

| Priority | Status | Finding | Evidence | Recommended next action |
|---|---|---|---|---|
| P0 | Confirmed | Direct `quarry-control` onboarding path violates the documented "edge is the only public/cross-plane entrypoint" rule. | `Quarry-v2/docs/ARCHITECTURE.md`. | Migrate Velion v3 onboarding crawl handlers to edge. |
| P0 | Confirmed | Integration API webhook signature enforcement fails open when the GitHub webhook secret is absent or invalid. | Updated live smoke returns 200 for `/api/v1/webhooks/github` with no signature and with `sha256=invalid`. | Require configured secrets for enabled webhook providers or reject unsigned provider webhooks. |
| P1 | Confirmed | Quarry-v2 workspace tests fail to compile after ingest contract expansion. | `cargo test --workspace` in `Quarry-v2`. | Update constructors in `quarry-core` tests and `quarry-runtime` tests; rerun workspace tests. |
| P1 | Confirmed | Top-level Makefile still targets legacy `Quarry`. | `apps/Ingestion Plane/Makefile`. | Update commands to Quarry-v2 or explicitly prefix legacy commands. |
| P2 | Needs verification | Control service degrades to "trust the network" when internal HMAC secret is unset during rollout. | `Quarry-v2/docs/ARCHITECTURE.md`. | Verify deployment envs require HMAC where cross-plane trust matters. |
| P2 | Needs verification | Quarry-v2 contract tests should become a required gate for Data Plane contract changes. | Compile failure shows contract drift was not caught before this pass. | Add CI ownership for Quarry/Data contract compatibility. |

## Model Plane

| Priority | Status | Finding | Evidence | Recommended next action |
|---|---|---|---|---|
| P1 | Confirmed | Orchestrator memory consolidation and skill promotion activities are still documented as placeholder implementations. | `apps/Model Plane/README.md`. | Replace placeholders with real memory/skill integrations or gate features that depend on them. |
| P1 | Confirmed | `orchestrator-core/internal/orchestration` tests fail to compile after the generated orchestration client interface changed. | `go test ./...` in `services/orchestrator-core`; stubs miss `ListPendingApprovals`. | Update fake client/test stubs or use generated mocks. |
| P1 | Confirmed | `letta-bridge/internal/memstore` time-range filtering currently includes an old record that the test expects to exclude. | `go test ./...` in `services/letta-bridge`; `TestTimeRangeFiltering/cutoff_excludes_old_record`. | Clarify cutoff semantics and fix the filter or test fixture. |
| P1 | Confirmed | Rust Model Plane workspace passed, but Go Model Plane has failing gates. | `cargo test --workspace` passed in `apps/Model Plane/rust`; Go failures above. | Treat Rust as green for this pass; fix Go failures before declaring Model Plane green. |
| P1 | Needs verification | Full cross-service contract validation remains on the cutover plan. | `apps/Model Plane/README.md` lists `model-gateway -> session-core -> inference-core -> execution-core` validation as remaining work. | Run/extend the verification plan in `docs/VERIFICATION.md`, especially across live service boundaries. |
| P2 | Needs verification | Auth hardening work remains explicit: issuer/audience, key rotation, org/user claims, malformed token tests. | `apps/Model Plane/README.md`. | Add security tests before exposing broader gateway traffic. |
| P2 | Needs verification | Streaming/replay/idempotency edge cases are documented but not proven in this pass. | `apps/Model Plane/README.md`. | Convert edge-case list into automated tests or mark covered tests in docs. |

## Control Plane

| Priority | Status | Finding | Evidence | Recommended next action |
|---|---|---|---|---|
| P0 | Confirmed | Data Plane production readiness depends on Control Plane consultation and trust decisions. | Data Plane `docs/gap-data.md`. | Publish a small contract for org/user/session/quota checks consumed by Data Plane and gateway clients. |
| P1 | Needs verification | Gateway and plane services need a single source of truth for active org/user scoping headers. | Velion gateway strips forged headers; Data Plane still documents `X-Org-ID` trust risk. | Audit header names and trust boundaries across Control, gateway, Data, and Ingestion. |
| P2 | Confirmed | Checked Control Plane Go services and auth-core Jest suite pass in this worktree. | `go test ./...` in audit/billing/org/session/user; `pnpm exec jest --runInBand` in `auth-core`. | Keep these as baseline gates for the first remediation batch. |
| P2 | Needs verification | Control Plane has broad pre-existing local changes in this worktree. | `git status` showed modified auth/org/user/billing/session/audit adjacent files before docs work. | Stabilize or review local changes before using this worktree for release evidence. |

## Application Plane

| Priority | Status | Finding | Evidence | Recommended next action |
|---|---|---|---|---|
| P2 | Confirmed | No top-level Application Plane README/architecture doc was found in the first pass; docs are service-local. | Manifest/doc scan found service READMEs such as `convex-core/README.md`, `information-core/README.md`, `notification-core/README.md`, `zammad-foundation/README.md`. | Add or designate a top-level Application Plane orientation doc if this plane is actively onboarded by agents. |
| P2 | Needs verification | Application projections must stay clearly separate from Control/Data/Ingestion/Model ownership. | `apps/master-ownership-matrix.md` and system rules. | Add integration tests or docs for projected state vs canonical authority for inbox/social/conversation surfaces. |
| P2 | Confirmed | Convex-core pnpm script gates are blocked by ignored build approval for `esbuild@0.27.0`. | `pnpm typecheck` and `pnpm lint` exit before scripts with `ERR_PNPM_IGNORED_BUILDS`. | Approve or document build-script policy, then rerun scripts through pnpm. |
| P3 | Confirmed | Checked Application Plane Go services and Convex direct TypeScript/ESLint binaries pass. | `go test ./...` in conversation/information/insight/leads/notification/social; direct `tsc --noEmit` and ESLint in `convex-core`. | Keep these as baseline gates; add Convex runtime/integration checks later. |

## Cross-Plane / Repository Hygiene

| Priority | Status | Finding | Evidence | Recommended next action |
|---|---|---|---|---|
| P1 | Confirmed | Static inventory can double-count nested agent worktrees. | Naive `find` surfaced many manifests under `.claude/worktrees/...`. | Exclude `.claude/worktrees` from CodeGraph and local inventory scripts. |
| P1 | Confirmed | Large pre-existing dirty worktree reduces audit certainty. | `git status` before this docs work showed many modified/deleted/untracked files across planes. | Before remediation, split user changes from audit fixes or work in a clean branch/worktree. |
| P2 | Confirmed | Generated/build/dependency directories are present inside plane trees. | Scans had to prune `node_modules`, `target`, `.next`, `.fallow`, `.playwright-mcp`. | Review `.gitignore`, CodeGraph excludes, and artifact cleanup policy. |
| P2 | Confirmed | Root docs and system map were stale to Velion v2. | `rg velionv2 AGENTS.md CLAUDE.md apps/CODEBASE_INFORMATION_SYSTEM.md` before this pass. | Done in this pass; keep stale references from reappearing by linking new docs in future PRs. |
| P3 | Needs verification | CodeGraph counts may include local generated/duplicated content until excludes are audited. | CodeGraph status reports 11,635 indexed files; discovery pollution was separately confirmed. | Regenerate index after exclude cleanup and record fresh counts. |

## Suggested Remediation Order

1. Fix the failing gates: Velion v3 `loadStudioWorkspace`, Quarry-v2 `DataPlaneIngestRequest`, Model Go `orchestrator-core`, Model Go `letta-bridge`, and Convex pnpm ignored-build approval.
2. Close the cross-plane trust blockers: Data Plane Control consultation and Quarry-v2 onboarding edge migration.
3. Clean repository/index hygiene so future onboarding facts are not polluted by nested worktrees or generated artifacts.
4. Replace stale/default docs: `apps/velion-web` README and top-level Ingestion Makefile/help output.
5. Inventory preview/fallback surfaces in Velion v3 and promote live owner-plane wiring where the product claims are no longer preview-only.
6. Run broader live-stack verification: Ingestion endpoint tests, Model Plane cross-service verification, Control coverage gates, Convex runtime tests, and Application realtime integration checks.
