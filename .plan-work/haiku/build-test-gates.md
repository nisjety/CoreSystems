# Build, test and CI gate map (worker: build-test-gates)

Date: 2026-10-14. Source: live tree `C:/dev/CoresSystem` (read only) and the snapshot files `.plan-work/live-tree-status.txt` and `.plan-work/live-tree-tracked-files.txt`. Rows: 41 services and packages (R01-R41).

Method and limits: static reads of code, Makefiles, compose files and workflows, plus read-only git (`ls-remote`, `config`, `check-ignore`) and read-only `gh api` GETs. No build, test, install, docker command or CI run was executed. Statements about what would fail are reasoned from the files and are marked UNVERIFIED where not confirmed.

State tags. Where a row first cites a file, the file carries a tag in brackets; section 12 lists every cited repo path with its state:
- `[COMMITTED]` tracked at HEAD and not in the status snapshot.
- `[MODIFIED-UNCOMMITTED]` tracked and ` M` in the status snapshot. Line numbers may differ from afb83928.
- `[UNCOMMITTED-NEW]` `??` in the status snapshot, or inside an untracked directory.
- `[MISSING]` not present in the live tree.

Gitignored or generated outputs are described in words and are not cited by path.

Terms: "PR to dev" means a pull request whose base branch is `dev`, the origin default branch (section 1, item 1).

---

## 1. Headline findings

1. **Branch filters name branches that do not exist on origin.** `git ls-remote --heads origin` returns 5 heads, including `dev` and `fix/ui-premium-restore`; there is no `main` and no `develop`. `gh api repos/nisjety/CoreSystems` reports `default_branch: dev`. Every workflow filtered on `branches: [main]` or `branches: [main, develop]` therefore does not run for PRs to `dev`, and its `push` trigger cannot fire until a `main` exists on origin. Section 2 lists them. The planning gitStatus calls `fix/ui-premium-restore` the PR main branch. Both facts are recorded; the branch model is an open question (section 10).
2. **GitHub enforces no gate on dev.** `gh api repos/nisjety/CoreSystems/branches/dev/protection` returns 404 "Branch not protected", and the rulesets count is 0. A "required gate" is a policy statement unless enforced elsewhere (UNVERIFIED for other branches and for organisation settings).
3. **Committed main would be red until the uncommitted set lands together.** Concrete dependencies on uncommitted files:
   - `apps/Model Plane/rust/services/session-core/src/main.rs` `[MODIFIED-UNCOMMITTED]` declares `mod retention_receipt;` at line 35. Its file `apps/Model Plane/rust/services/session-core/src/retention_receipt.rs` `[UNCOMMITTED-NEW]` and the migrations `apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql` and `apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql` `[UNCOMMITTED-NEW]` are not in HEAD.
   - `apps/Model Plane/rust/crates/mp-contracts/src/lib.rs` `[MODIFIED-UNCOMMITTED]` declares `zdr_refusal` and `retention`, whose files `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs` and `apps/Model Plane/rust/crates/mp-contracts/src/retention.rs` are `[UNCOMMITTED-NEW]`.
   - `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]` calls four scripts that are `[UNCOMMITTED-NEW]`: `apps/Data Plane v2/scripts/qb-preflight.sh`, `apps/Data Plane v2/tests/scripts/rerank-input-contract-test.sh`, `apps/Data Plane v2/tests/scripts/web-rerank-compose-contract-test.sh`, `apps/Data Plane v2/tests/scripts/isolated-registry-parity-test.sh`.
   - `.github/workflows/quarry-docs-contract.yml` `[UNCOMMITTED-NEW]` runs `apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs` `[UNCOMMITTED-NEW]`.
   - `.github/workflows/verevonv3-ci.yml` `[MODIFIED-UNCOMMITTED]`, job `demo-product-build`, runs `apps/Frontend Plane/verevonv3/scripts/build-product-demo.mjs` and `apps/Frontend Plane/verevonv3/scripts/probe-product-demo.mjs` (both `[UNCOMMITTED-NEW]`) and diffs `apps/Frontend Plane/verevonv3/apps/verevon-web/public/product-demo/` (`[UNCOMMITTED-NEW]`).
   - `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]` runs the contract check that reads `apps/Model Plane/api/contracts/zdr-refusals.json` and its `.sha256` (both `[UNCOMMITTED-NEW]`), through the pins that `scripts/tests/zdr-contract-sync.test.sh` `[MODIFIED-UNCOMMITTED]` enforces.
   Commit these as one reviewed set, or the gates fail on HEAD.
4. **`buf.yml` is broken as written.** `.github/workflows/buf.yml` `[COMMITTED]` runs `buf lint apps/Model Plane/proto` and `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` with unquoted paths. Bash splits at the space inside `Model Plane`, so buf receives two arguments for each path. The paths exist: `apps/Model Plane/proto/buf.yaml` `[COMMITTED]` and `apps/Model Plane/proto/baseline.binpb` `[COMMITTED]`. Fix: quote the paths. `apps/Model Plane/CLAUDE.md` `[COMMITTED]` also says buf lint carries pre-existing debt.
5. **CI Go pins are below the go.mod directive.**
   - `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]` uses `go-version: "1.23"`. The Data Plane modules declare `go 1.26.5`: `apps/Data Plane v2/services/documents-api-go/go.mod`, `apps/Data Plane v2/tools/migrator/go.mod` `[COMMITTED]`, and the wiki-store, data-orchestrator and data-quality services (section 4).
   - `.github/workflows/user-service.yml` `[COMMITTED]` uses `'1.24'`. `apps/Control Plane/user-core/go.mod` `[COMMITTED]` declares `go 1.26.5`.
   - `.github/workflows/org-service.yml` `[COMMITTED]` uses `'1.24'`. `apps/Control Plane/org-core/go.mod` `[COMMITTED]` declares `go 1.25`.
   - Go's default `GOTOOLCHAIN=auto` downloads the newer toolchain. It may work slowly, or fail on a runner without network access (UNVERIFIED).
6. **The Data Plane docker-smoke readiness loop cannot succeed and never fails.** `.github/workflows/dataplane-v2-ci.yml` polls `http://localhost:8014/health` for 30 iterations and never exits non-zero. Port 8014 is not published by `apps/Data Plane v2/docker-compose.yml` `[MODIFIED-UNCOMMITTED]`; it appears only in `apps/Data Plane v2/infra/prometheus/prometheus.yml` `[COMMITTED]` (scrape target), `apps/Data Plane v2/scripts/smoke-quickwit-retrieval.sh` `[COMMITTED]` and `apps/Data Plane v2/tests/scripts/load-auth-contract-test.sh` `[COMMITTED]`. The real readiness gates are `docker compose up -d --build --wait` and then `apps/Data Plane v2/scripts/smoke-test.sh` `[COMMITTED]` with `--http-only`, whose default transport is `compose` (`DPV2_SMOKE_TRANSPORT`, same file).
7. **Several services have no CI at all.** No workflow references them: Model Plane Go capability-core, sandbox-manager, orchestrator-core, letta-bridge, browser-broker, cost-core, bridge-core and nats-provisioner; Control Plane audit-core; Application Plane convex-core, conversation-ingest-rs, information-core, insight-core and leads-core; Ingestion Plane imports-core and integration-corev2; the Support Plane; and the Playwright end-to-end suite. The Quarry Go services run only in `continue-on-error` jobs in `.github/workflows/quarry-v2-ci.yml` `[COMMITTED]`, so they do not gate.
8. **Gate scripts that no workflow runs.** `apps/Data Plane v2/scripts/check-usage-contract.sh` `[MODIFIED-UNCOMMITTED]` and `scripts/tests/quarry-model-plane-switches.test.mjs` `[UNCOMMITTED-NEW]` are not wired. These Data Plane tests `[COMMITTED]` run only through the local targets `make test-security-scripts` and `make ci`: `apps/Data Plane v2/tests/scripts/smoke-test-test.sh`, `apps/Data Plane v2/tests/scripts/compose-security-contract-test.sh`, `apps/Data Plane v2/tests/scripts/image-provenance-contract-test.sh` and `apps/Data Plane v2/tests/scripts/load-auth-contract-test.sh`.
9. **Some probes prove little.** Model Plane session-core `/healthz` and `/readyz` both return the constant string "ok" (`apps/Model Plane/rust/services/session-core/src/http_health.rs` `[COMMITTED]`). Control user-core `/health` does a database round-trip and is the stronger up-proof.
10. **go.work exists, which contradicts the user memory note.** `apps/Model Plane/go/go.work` `[COMMITTED]` lists 19 module directories under `apps/Model Plane/go/`. The user memory note says "no go.work, build per-service". Treat the note as stale until checked. The Dockerfiles and CI use per-service directories.
11. **Hook and config divergence.** The main checkout's git config sets `core.hooksPath` to the relative value `.githooks`. This worktree's worktree-level config overrides it with the absolute path `C:\dev\CoresSystem\.githooks`, so the worktree runs the main checkout's hook. The hook's gitleaks fallback image is unpinned (`ghcr.io/gitleaks/gitleaks:latest`), while CI pins 8.24.3 (section 6).
12. **Makefiles hard-code a macOS cache root.** `apps/Data Plane v2/Makefile` `[COMMITTED]` and `apps/Ingestion Plane/Quarry-v2/Makefile` `[COMMITTED]` default `CORE_SYSTEM_CACHE_ROOT` to `/Volumes/Applikasjon/Triodelab/CoreSystem-build-cache`, which sets `CARGO_TARGET_DIR`, `GOCACHE` and `GOMODCACHE`. On this Windows host, export the three variables (or override `CORE_SYSTEM_CACHE_ROOT`) before any `make` target.
13. **`convex deploy` targets production.** `apps/Application Plane/convex-core/package.json` `[COMMITTED]` defines `"deploy": "convex deploy --prod"`; `"dev": "convex dev"` is the local watch.
14. **Port mismatch in the Ingestion Makefile.** `apps/Ingestion Plane/Makefile` `[COMMITTED]` target `up` prints "Quarry Control: http://localhost:8081", but `apps/Ingestion Plane/docker-compose.yml` `[MODIFIED-UNCOMMITTED]` publishes host port 28081 for `quarry-control` (container 8081).
15. **Auth-core CI runs Node 20; its image runs Node 22.** `.github/workflows/auth-service.yml` `[COMMITTED]` uses `node-version: '20'` and `pnpm install` without `--frozen-lockfile`. `apps/Control Plane/auth-core/Dockerfile` `[COMMITTED]` uses `node:22.23.1` and pnpm 11.3.0. The verevonv3 CI uses Node 22 with `--frozen-lockfile`.

---

## 2. CI trigger map

| Workflow | PR, any base | PR to dev | push to dev | Gates |
|---|---|---|---|---|
| `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]` | yes (path filter) | yes | no (`push: branches: [main]`) | Rust workspace fmt, check and test; session-core receipts on Postgres 16; contract goldens sync |
| `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]` | yes (path `apps/Data Plane v2/**`) | yes | no (`push: branches: [main]`) | Data Plane Rust, Go, Postgres contracts, contract scripts, docker smoke |
| `.github/workflows/sse-taxonomy.yml` `[COMMITTED]` | yes (path) | yes | yes (path filter, any branch) | SSE taxonomy freshness and drift self-proof |
| `.github/workflows/contract-goldens.yml` `[COMMITTED]` | yes (path) | yes | no (`push: branches: [main]`) | Cross-plane goldens, pins, vendored copies, ZDR loader |
| `.github/workflows/secret-scan.yml` `[COMMITTED]` | yes (all PRs except Dependabot) | yes | no (`push: branches: [main]`) | gitleaks over the commit range |
| `.github/workflows/verevonv3-ci.yml` `[MODIFIED-UNCOMMITTED]` | only if base is `main` | no | no | Fabrication guard; gateway fmt, clippy and test; SPA verify; Docker image; demo bundle |
| `.github/workflows/verevonv3-security.yml` `[COMMITTED]` | only if base is `main` | no | no (weekly cron `0 6 * * 1` runs) | `pnpm audit --audit-level=high` |
| `.github/workflows/quarry-v2-ci.yml` `[COMMITTED]` | only if base is `main` | no | no (weekly cron) | Quarry cargo check and test; cargo-audit (gating); govulncheck and coverage (report only) |
| `.github/workflows/quarry-docs-contract.yml` `[UNCOMMITTED-NEW]` | only if base is `main` | no | no | Quarry docs contract (node test) |
| `.github/workflows/buf.yml` `[COMMITTED]` | base `main` or `develop` | no | no | buf lint and breaking (broken, section 1 item 4) |
| `.github/workflows/user-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | user-core tests on Postgres; build; image push |
| `.github/workflows/org-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | org-core tests on Postgres with lifecycle fixture; build |
| `.github/workflows/auth-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | auth-core lint, build, jest; principal registry parity |
| `.github/workflows/session-core-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | Control Plane Go session-core test and build |
| `.github/workflows/billing-core-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | billing-core tests with lifecycle fixture; build |
| `.github/workflows/conversation-core-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | conversation-core-go test and build |
| `.github/workflows/notification-core-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | notification-core test and build |
| `.github/workflows/social-core-service.yml` `[COMMITTED]` | base `main` or `develop` | no | no | social-core tests on Postgres; build |
| `.github/workflows/eval-lab.yml` `[COMMITTED]` | base `main` or `develop` | no | no | eval-lab offline selftest (at least 10 cases) |
| `.github/workflows/dataplane-v2-eval-nightly.yml` `[COMMITTED]` | schedule `17 3 * * *` and manual only | n/a | n/a | retrieval quality gate on a self-hosted runner labelled `dp2-eval` (paid services) |

Dependabot: `.github/dependabot.yml` `[COMMITTED]` covers the SPA (npm), the gateway (cargo) and GitHub Actions, weekly on Mondays.

Stacked PRs whose base is another feature branch behave like PRs to `dev` for every branch-filtered workflow.

---

## 3. Required gates for Space-touching code

| Touchpoint | Paths (state) | Gate(s) | Runs for a PR to dev? |
|---|---|---|---|
| Control Plane Space authority (user-core) | `apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go` `[COMMITTED]` and the rest of `apps/Control Plane/user-core/internal/spaces/` `[COMMITTED]` | `user-service.yml` test job (Postgres 16, `go test -v ./...`) and build | no |
| Cross-plane token goldens used by Space authz | `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` `[MODIFIED-UNCOMMITTED]` and `.sha256` `[MODIFIED-UNCOMMITTED]`; Quarry copies `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json` `[MODIFIED-UNCOMMITTED]` and `.sha256` `[MODIFIED-UNCOMMITTED]` | `auth-service.yml` (REQUIRE_CONTRACT=1 jest; principal registry parity); `contract-goldens.yml` (path `apps/**/cross-plane-token-requests.json*`); `model-plane-rust-ci.yml` (vendored-copy path) | contract-goldens and model-plane: yes; auth-service: no |
| Model Plane thread, message and run ownership | `apps/Model Plane/rust/services/session-core/src/grpc.rs` `[MODIFIED-UNCOMMITTED]` | `model-plane-rust-ci.yml` job `rust` (fmt, check, all-targets check, `cargo test --workspace --no-fail-fast`) | yes (paths) |
| Model Plane retention receipts (ZDR) | `apps/Model Plane/rust/services/session-core/src/retention_receipt.rs` `[UNCOMMITTED-NEW]` | `model-plane-rust-ci.yml` job `session-core-receipts` (at least 8 ignored `zdr_receipt` tests on Postgres) | yes, once committed |
| Model Plane proto contract | `apps/Model Plane/proto/model_plane/v1/sessions.proto` `[MODIFIED-UNCOMMITTED]` | `model-plane-rust-ci.yml` (proto path; `apps/Model Plane/rust/crates/mp-contracts/build.rs` `[COMMITTED]` compiles protos with protoc 27.x); `buf.yml` (broken) | model-plane: yes; buf: no |
| SSE stream events (chat and agent turns) | `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs` `[MODIFIED-UNCOMMITTED]`; `apps/Model Plane/docs/sse-event-taxonomy.md` `[MODIFIED-UNCOMMITTED]`; `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts` `[MODIFIED-UNCOMMITTED]` | `sse-taxonomy.yml` | yes |
| Gateway Space domain | `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs` `[COMMITTED]` | `verevonv3-ci.yml` jobs `gateway-rust` (fmt --check, clippy `-D warnings`, cargo test) and `fabrication-guard` | no |
| SPA Space feature | `apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.tsx` `[MODIFIED-UNCOMMITTED]` | `verevonv3-ci.yml` job `spa-verify` (`pnpm verify`) | no |
| Convex Space functions and projections | `apps/Application Plane/convex-core/convex/spaces.ts` `[COMMITTED]`; `apps/Application Plane/convex-core/test/space-lifecycle.test.cjs` `[COMMITTED]` | none in CI (`pnpm test` runs locally only) | n/a |
| Space notifications and conversation events | `apps/Application Plane/notification-core/` `[COMMITTED]`; `apps/Application Plane/conversation-core/conversation-core-go/` `[COMMITTED]` | `notification-core-service.yml`; `conversation-core-service.yml` | no |
| Secret scanning for every Space change | `.gitleaks.toml` `[MODIFIED-UNCOMMITTED]`; `.github/workflows/secret-scan.yml` `[COMMITTED]` | `secret-scan.yml` | yes |
| ZDR refusal contract used by Space chat | `apps/Model Plane/api/contracts/zdr-refusals.json` `[UNCOMMITTED-NEW]` and `.sha256` `[UNCOMMITTED-NEW]`; SPA copy `apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts` `[UNCOMMITTED-NEW]` | `contract-goldens.yml` (path `apps/Model Plane/api/contracts/**`, `apps/**/zdr-refusals.json*`) | yes, once committed |

Minimum set that runs on a PR to dev today: `secret-scan`, plus `model-plane-rust-ci`, `dataplane-v2-ci`, `sse-taxonomy` and `contract-goldens` when their paths change. The Control Plane, gateway and SPA gates run only for PRs whose base is `main` or `develop` (Control) or `main` (verevonv3). Changing the branch filters, or basing PRs on `main` once it exists on origin, is a decision for the team (section 10).

---

## 4. Per-service rows

Each row gives: the main path and state, test command with working directory, lint or typecheck, codegen or migrations, CI coverage, compose service with rebuild command, up-proof (health endpoint or log line), and traps.

Command notation: `cd "<dir>" && <command>` runs from `<dir>`; the directory paths are listed in section 12.

### Model Plane

Rust workspace `apps/Model Plane/rust/Cargo.toml` `[COMMITTED]` has 15 members (10 `mp-*` crates, 4 services, 1 client). The CI workspace command is `cargo test --workspace --no-fail-fast`. Compose file `apps/Model Plane/deploy/docker-compose.yml` `[COMMITTED]`. Dev entry point: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build <service>` (`apps/Model Plane/scripts/compose.sh` `[COMMITTED]`). It adds `apps/Model Plane/deploy/docker-compose.override.yml` `[COMMITTED]` unless `MODEL_PLANE_DEV=0`, and requires the gitignored deploy env file. `MODEL_PLANE_PRODUCTION=1` is refused on this path and must go through `apps/Model Plane/scripts/release-artifact.sh` `[COMMITTED]`. In the base compose only infrastructure publishes host ports, on 127.0.0.1: postgres 55434, nats 4228, temporal 7234, minio 9002, dragonfly 6391. Application services are reached with `apps/Model Plane/scripts/compose.sh exec <service> curl ...` or through the override (override ports not read).

**R01 session-core (Rust).** Path `apps/Model Plane/rust/services/session-core/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/rust" && cargo test -p session-core`. Receipt contract tests are `#[ignore]` and need `DATABASE_URL` pointing at a scratch Postgres whose database name contains "test": `cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1`. CI requires at least 8 to pass.
- Lint: `cargo fmt --all -- --check` and `cargo check --workspace --all-targets`. No clippy gate (CI comment: 304 pedantic warnings).
- Codegen and migrations: `sqlx::migrate!("./migrations")` embeds the migrations at compile time. No sqlx CLI step and no compile-time `query!` macros under `apps/Model Plane/rust/` (grep), so the build needs no database. `apps/Model Plane/rust/services/session-core/migrations/revert/` `[UNCOMMITTED-NEW]` is a revert directory, not read by the embed (UNVERIFIED).
- CI: `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]`, jobs `rust` and `session-core-receipts` (Postgres 16 service). `.github/workflows/session-core-service.yml` `[COMMITTED]` covers the Control Plane Go service, not this one.
- Compose: service `session-core`, context `apps/Model Plane`, dockerfile `apps/Model Plane/rust/services/session-core/Dockerfile` `[COMMITTED]`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build session-core`.
- Up-proof: container healthcheck `curl -f http://localhost:8081/healthz`. Ports: HTTP 8081, gRPC 9091.
- Traps: `/healthz` and `/readyz` both return constant "ok" in `apps/Model Plane/rust/services/session-core/src/http_health.rs` `[COMMITTED]`, so they prove nothing about the database; check logs or a query instead. User memory note (UNVERIFIED on this host): two cargo targets fail on Windows, so use `--lib`.

**R02 model-gateway (Rust).** Path `apps/Model Plane/rust/services/model-gateway/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/rust" && cargo test -p model-gateway`. Integration files `apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs` and `apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs` are `[COMMITTED]`. The ZDR suites are `[UNCOMMITTED-NEW]`: `apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs`, `apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs`, `apps/Model Plane/rust/services/model-gateway/tests/zdr_refusal_inventory.rs`, plus `apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs`, `apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs`, `apps/Model Plane/rust/services/model-gateway/src/relevance_calibration.rs` and the directory `apps/Model Plane/rust/services/model-gateway/calibration/`.
- Lint: as R01.
- SSE taxonomy: `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` reads `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs` `[MODIFIED-UNCOMMITTED]` and the doc `apps/Model Plane/docs/sse-event-taxonomy.md` `[MODIFIED-UNCOMMITTED]`. The `--check` flag makes it a read-only comparison. To regenerate, run the script without the flag (inferred from its `argparse` setup; not executed).
- CI: `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]`; `.github/workflows/sse-taxonomy.yml` `[COMMITTED]`; `.github/workflows/contract-goldens.yml` `[COMMITTED]` covers `apps/**/zdr-posture-matrix.json*`, whose pins `apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json` and `apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json.sha256` are `[UNCOMMITTED-NEW]`.
- Compose: service `model-gateway`, context `apps/Model Plane`, dockerfile `apps/Model Plane/rust/services/model-gateway/Dockerfile` `[COMMITTED]`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build model-gateway`.
- Up-proof: compose healthcheck `curl -f http://localhost:8080/readyz`; image HEALTHCHECK `curl -fsS http://127.0.0.1:8080/healthz`. Routes are in `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` `[MODIFIED-UNCOMMITTED]` (lines 119-120 and 128-129). gRPC 9090.
- Traps: compose and image use different health paths. The SSE gate fails when the events source changes without the doc.

**R03 execution-core (Rust).** Path `apps/Model Plane/rust/services/execution-core/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/rust" && cargo test -p execution-core`. `browser_agent_e2e` is `#[ignore]` (needs a live stack). ZDR inventory test `apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs` `[UNCOMMITTED-NEW]`.
- Lint: as R01. Dead-code warnings exist (`verification_result_for`, `ExecutionService::new`); CI does not deny warnings for Model Plane.
- CI: `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]`.
- Compose: service `execution-core`, dockerfile `apps/Model Plane/rust/services/execution-core/Dockerfile` `[COMMITTED]`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build execution-core`.
- Up-proof: `curl -f http://localhost:8083/readyz`. Routes `/healthz` and `/readyz` in `apps/Model Plane/rust/services/execution-core/src/http_health.rs` `[COMMITTED]`. HTTP 8083, gRPC 9093.

**R04 inference-core (Rust).** Path `apps/Model Plane/rust/services/inference-core/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/rust" && cargo test -p inference-core`. New tests `apps/Model Plane/rust/services/inference-core/tests/zdr_refusal_inventory.rs` and `apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs` are `[UNCOMMITTED-NEW]`.
- CI: `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]` (workspace).
- Compose: service `inference-core`, dockerfile `apps/Model Plane/rust/services/inference-core/Dockerfile` `[COMMITTED]`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build inference-core`.
- Up-proof: `curl -f http://localhost:8082/readyz`; routes in `apps/Model Plane/rust/services/inference-core/src/http_health.rs` `[COMMITTED]`. HTTP 8082, gRPC 9092.

**R05 mp-contracts (Rust crate).** Path `apps/Model Plane/rust/crates/mp-contracts/` `[COMMITTED]` (`apps/Model Plane/rust/crates/mp-contracts/Cargo.toml` and `apps/Model Plane/rust/crates/mp-contracts/build.rs` `[COMMITTED]`).
- Test: `cd "apps/Model Plane/rust" && cargo test -p mp-contracts`. The new modules `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs`, `apps/Model Plane/rust/crates/mp-contracts/src/retention.rs`, the test `apps/Model Plane/rust/crates/mp-contracts/tests/zdr_retention_matrix.rs` and the directory `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/` are `[UNCOMMITTED-NEW]`; `apps/Model Plane/rust/crates/mp-contracts/src/lib.rs` `[MODIFIED-UNCOMMITTED]` declares them.
- Codegen: `apps/Model Plane/rust/crates/mp-contracts/build.rs` compiles protos from `apps/Model Plane/proto/` through `tonic_build` at build time, so every build needs `protoc`. CI installs `arduino/setup-protoc@v3` version 27.x. `apps/Model Plane/rust/crates/mp-contracts/build.rs` strips the Windows extended path prefix (comments at lines 4-9).
- Contract goldens: `apps/Model Plane/api/contracts/zdr-refusals.json` and `.sha256` `[UNCOMMITTED-NEW]`. Sync: `bash scripts/tests/zdr-contract-sync.test.sh` and `bash scripts/tests/zdr-contract-sync.test.sh --self-test` from the repo root (`scripts/tests/zdr-contract-sync.test.sh` `[MODIFIED-UNCOMMITTED]`). Loader test: `node --test scripts/tests/zdr-contract-schema.test.mjs` `[COMMITTED]`.
- CI: `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]` step "Contract goldens match their pins" (runs before fmt); `.github/workflows/contract-goldens.yml` `[COMMITTED]`.
- Up-proof: not a service. Verification is `cargo test -p mp-contracts` plus the contract scripts.
- Traps: a committed crate root without the two untracked modules does not compile (section 1 item 3).

**R06 capability-core (Go).** Path `apps/Model Plane/go/services/capability-core/` `[COMMITTED]` (`apps/Model Plane/go/services/capability-core/go.mod`, `apps/Model Plane/go/services/capability-core/Dockerfile` and `apps/Model Plane/go/services/capability-core/cmd/main.go` `[COMMITTED]`).
- Test: `cd "apps/Model Plane/go/services/capability-core" && go test ./... && go vet ./...`. The Model Plane README lines 177-178 list this command. 88 `_test.go` files are tracked in the module.
- Lint: `go vet ./...`. Build: the Dockerfile runs `go build -trimpath -ldflags="-s -w" -o /service ./cmd/`.
- Codegen and migrations: Go stubs in `apps/Model Plane/go/gen/` `[COMMITTED]` are generated with `buf generate` and `apps/Model Plane/proto/buf.gen.go-only.yaml` `[COMMITTED]`. `apps/Model Plane/docs/capability-ownership-matrix.md` `[COMMITTED]` (line 145) says the checked-in Go gen is stale against the buf proto-path scheme, and that a repo-wide regeneration must be a controlled migration rather than part of a feature commit. SQL migrations 0001 to 0015 are in `apps/Model Plane/go/services/capability-core/migrations/` `[COMMITTED]`; the Dockerfile copies them to `/app/migrations`. Which code applies them is UNVERIFIED.
- CI: none. `.github/workflows/capability-core.yml` is `[MISSING]`.
- Compose: service `capability-core`, context `apps/Model Plane/go`, dockerfile `apps/Model Plane/go/services/capability-core/Dockerfile`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build capability-core`.
- Up-proof: healthcheck `curl -f http://localhost:8085/healthz`. `apps/Model Plane/go/services/capability-core/cmd/main.go` serves `/healthz` and `/readyz` on `:8085` (line 410) and logs `health server listening` with `addr :8085` (line 415). gRPC `:9097` logs `gRPC listening` (line 457).
- Env read by the service: `ORCHESTRATOR_WORKFLOW_ADDR` (line 643), `SESSION_CORE_ADDR` and `INFERENCE_CORE_ADDR` (lines 822-823), `SANDBOX_MANAGER_GRPC_ADDR` (line 984).
- Traps: `go -race` needs cgo, which the user memory note says is unavailable on this host.

**R07 sandbox-manager (Go).** Path `apps/Model Plane/go/services/sandbox-manager/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/go/services/sandbox-manager" && go test ./... && go vet ./...`. 18 test files tracked.
- Codegen: shared Go stubs as in R06. Migrations: none found in the module (UNVERIFIED).
- CI: none.
- Compose: service `sandbox-manager`, context `apps/Model Plane/go`, dockerfile `apps/Model Plane/go/services/sandbox-manager/Dockerfile`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build sandbox-manager`.
- Up-proof: `curl -f http://localhost:8086/healthz`; `apps/Model Plane/go/services/sandbox-manager/cmd/main.go` serves `/healthz` and `/readyz` at lines 76-80. Log line: UNVERIFIED.
- Traps: capability-core reaches it through `SANDBOX_MANAGER_GRPC_ADDR` (R06). User memory note: signalling the bwrap monitor does not reach the command.

**R08 orchestrator-core (Go).** Path `apps/Model Plane/go/services/orchestrator-core/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/go/services/orchestrator-core" && go test ./... && go vet ./...` (README line 178). 29 test files tracked.
- Build: the Dockerfile builds `apps/Model Plane/go/services/orchestrator-core/cmd/` into `/service`, entrypoint script, CMD `/service`.
- Codegen: shared Go stubs as in R06. No SQL migrations in the module (UNVERIFIED).
- CI: none.
- Compose: service `orchestrator-core`, context `apps/Model Plane/go`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build orchestrator-core`.
- Up-proof: `wget -q -O - http://localhost:8084/healthz` (compose); `apps/Model Plane/go/services/orchestrator-core/cmd/main.go` serves `/healthz` and `/readyz` at lines 87-91.
- Traps: Temporal activities live in `apps/Model Plane/go/services/orchestrator-core/cmd/activities/activities.go` `[COMMITTED]`; the README (dated 2026-07-11) says they are wired.

**R09 letta-bridge (Go).** Path `apps/Model Plane/go/services/letta-bridge/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/go/services/letta-bridge" && go test ./... && go vet ./...` (8 test files).
- CI: none.
- Compose: service `letta-bridge`, context `apps/Model Plane/go`. It depends on `agent-memory-redis` (password required by compose `:?` syntax) and `agent-memory-server` (image `redislabs/agent-memory-server:0.15.1`, port 8000, Python socket healthcheck). Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build letta-bridge`.
- Up-proof: `curl -f http://localhost:8088/healthz`. `apps/Model Plane/go/services/letta-bridge/cmd/main.go` serves `:8088` (line 66) and logs `health server listening` with `addr :8088` (line 68); gRPC `:9096` logs `gRPC listening` (line 97).
- Traps: the Redis password must come from the env file; compose refuses to start the service without it.

**R10 browser-broker (Go).** Path `apps/Model Plane/go/services/browser-broker/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/go/services/browser-broker" && go test ./... && go vet ./...` (7 test files).
- CI: none.
- Compose: service `browser-broker`, context `apps/Model Plane/go`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build browser-broker`.
- Up-proof: `curl -f http://localhost:8087/healthz`; `apps/Model Plane/go/services/browser-broker/cmd/main.go` runs the health server on `:8087` (line 47), logs `health server listening` (line 49); gRPC `:9095` (line 56) logs `gRPC listening` (line 69).
- Traps: per `apps/Model Plane/CLAUDE.md` `[COMMITTED]`, the model may propose browser actions, but Quarry-v2 executes or rejects them. BrowserBroker grants belong to Model Plane.

**R11 cost-core (Go).** Path `apps/Model Plane/go/services/cost-core/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/go/services/cost-core" && go test ./... && go vet ./...` (19 test files).
- Migrations: `apps/Model Plane/go/services/cost-core/migrations/` `[COMMITTED]`, 0001 to 0010 (up and down); the Dockerfile copies them to `/app/migrations`. The applier is UNVERIFIED.
- CI: none.
- Compose: service `cost-core`, context `apps/Model Plane/go`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build cost-core`.
- Up-proof: `curl -f http://localhost:8089/healthz`. `apps/Model Plane/go/services/cost-core/cmd/main.go` runs the HTTP server on `:8089` (line 183) and logs `HTTP server listening` (line 188); gRPC `:9098` (line 198). `/readyz` is in `apps/Model Plane/go/services/cost-core/internal/server/server.go` `[COMMITTED]`.
- Traps: migrations 0008 (erasure replay guard) and 0010 (billing usage outbox) carry ZDR and billing semantics; changes need the Control Plane billing contract (not read).

**R12 bridge-core (Go).** Path `apps/Model Plane/go/services/bridge-core/` `[COMMITTED]`.
- Test: `cd "apps/Model Plane/go/services/bridge-core" && go test ./... && go vet ./...` (9 test files). `apps/Model Plane/go/services/bridge-core/go.mod` line 13 replaces the authctx module with `apps/Model Plane/go/pkg/authctx/`.
- CI: none. Compose: service `bridge-core`, context `apps/Model Plane/go`. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build bridge-core`. Up-proof: `curl -f http://localhost:8091/healthz` (compose); the health route in code is UNVERIFIED.
- Traps: the optional bridge overlay (`MODEL_PLANE_BRIDGES=1`, Model Plane README line 167) brings in bridge services; not read.

**R13 nats-provisioner (Go).** Path `apps/Model Plane/go/services/nats-provisioner/` `[COMMITTED]` (Dockerfile `[COMMITTED]`).
- Test: `cd "apps/Model Plane/go/services/nats-provisioner" && go test ./...` (4 test files). Its `apps/Model Plane/go/services/nats-provisioner/go.mod` has no replace lines, and its Dockerfile sets `GOWORK=off`.
- CI: none. Compose: service `nats-provisioner`, context `apps/Model Plane/go`, one-shot, no healthcheck. Up-proof: container exit status, not HTTP. Rebuild: `cd "apps/Model Plane" && ./scripts/compose.sh up -d --build nats-provisioner`.
- Related: `apps/Model Plane/python/eval-lab-py/Makefile` `[COMMITTED]` is covered by `.github/workflows/eval-lab.yml` `[COMMITTED]` (offline selftest; asserts at least 10 cases ran).

### Control Plane

Compose file `apps/Control Plane/docker-compose.yml` `[COMMITTED]` (4-space indentation). Services: `controlplane-postgres`, `controlplane-dragonfly`, `controlplane-nats`, `control-shared-nats`, `auth-core`, `user-core`, `org-core`, `billing-core`, `session-core`, `audit-nats-provisioner`, `audit-extra-nats-provisioner`, `audit-core`, Lago services, Prometheus and Grafana. Launcher `apps/Control Plane/scripts/run-control-plane.sh` `[COMMITTED]`: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build <service>`. The launcher refuses `docker-compose.production.yml`, requires the eleven service-local env files named at its top (gitignored; not cited), creates the `inter-plane-bus` network on `up`, and keeps generated secrets in a gitignored 0600 store. A bare `docker compose build` exits 0 and builds nothing (user memory note).

**R14 user-core (Go).** Path `apps/Control Plane/user-core/` `[COMMITTED]` (`Makefile`, `apps/Control Plane/user-core/Dockerfile`, `apps/Control Plane/user-core/go.mod`, `apps/Control Plane/user-core/cmd/server/main.go`, `apps/Control Plane/user-core/internal/http/server.go` `[COMMITTED]`).
- Test: `cd "apps/Control Plane/user-core" && make test` (runs `go test -v ./...`). CI runs `go test -v ./...` with `TEST_DATABASE_URL` pointing at a Postgres 16 service (`user_core_test`). Coverage: `make test-coverage`.
- Lint: `make lint` (`golangci-lint run ./...`; not in CI); `make fmt`.
- Build: `make build` (from `apps/Control Plane/user-core/cmd/server/main.go`) and `make build-dual` (from `apps/Control Plane/user-core/cmd/dual/main.go`, which does not exist in the live tree). CI runs `go build -v ./cmd/server`.
- Codegen: `make proto-generate` runs protoc over `apps/Control Plane/user-core/proto/user/v1/` `[COMMITTED]` with the Go plugins. Migrations: not read; the CI comment says the suites provision their own schema.
- CI: `.github/workflows/user-service.yml` `[COMMITTED]`: test, build, and image push to `ghcr.io/triodelab/user-service` on non-PR events. Deploy jobs only echo.
- Compose: service `user-core` (context `apps/Control Plane/user-core`, container `user-service`); ports 3012 (HTTP), 50012 (gRPC), 9091 (metrics). Rebuild: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build user-core`.
- Up-proof: healthcheck `wget --spider http://127.0.0.1:3012/health`. `apps/Control Plane/user-core/internal/http/server.go` registers `/health` at line 157; it round-trips the database and returns 503 on failure.
- Traps: CI Go 1.24 against `go 1.26.5` (section 1 item 5). User memory note: control decision keys are base64url, and a std-base64-only decoder silently disables the decision lane. Relevant to `apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go` `[COMMITTED]`.

**R15 org-core (Go).** Path `apps/Control Plane/org-core/` `[COMMITTED]` (`apps/Control Plane/org-core/Dockerfile`, `apps/Control Plane/org-core/go.mod`, `apps/Control Plane/org-core/internal/http/server.go` `[COMMITTED]`).
- Test: `cd "apps/Control Plane/org-core" && go test -v -p 1 ./...` with `REQUIRE_CONTRACT=1`, `TEST_DATABASE_URL` (database `org_test`) and `CONTROL_LIFECYCLE_TEST_DATABASE_URL` (database `org_lifecycle`). The lifecycle suites refuse to run without a fixture marker row (`testfixture.VerifyLifecycleMarker`). CI creates the role `org_core_app` and the marker with psql.
- Build: `go build -v ./cmd/server`. Lint: none in CI.
- CI: `.github/workflows/org-service.yml` `[COMMITTED]`. `.github/workflows/billing-core-service.yml` `[COMMITTED]` mentions org-core only in a comment.
- Compose: service `org-core` (context `apps/Control Plane/org-core`, container `org-core-service`); ports 18080 to 8080 (HTTP), 19090 to 9090 (gRPC), 19091 to 9091 (metrics). Rebuild: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build org-core`. Cross-plane callers use `http://org-core:8080`, as set in `apps/Application Plane/convex-core/docker-compose.yml` `[COMMITTED]`.
- Up-proof: `GET /health` on 8080 inside the container (`apps/Control Plane/org-core/internal/http/server.go` line 79); the compose healthcheck uses `http://127.0.0.1:8080/health`.
- Traps: CI Go 1.24 against `go 1.25`, while the Dockerfile uses golang 1.26.5 (section 1 item 5).

**R16 audit-core (Go).** Path `apps/Control Plane/audit-core/` `[COMMITTED]` (`apps/Control Plane/audit-core/Dockerfile`, `apps/Control Plane/audit-core/go.mod` `[COMMITTED]`).
- Test: `cd "apps/Control Plane/audit-core" && go test ./...` (inferred; `apps/Control Plane/audit-core/Makefile` is `[MISSING]`). 19 `_test.go` files tracked. `apps/Control Plane/audit-core/internal/testdata/` `[COMMITTED]` is also a path filter of `.github/workflows/model-plane-rust-ci.yml`.
- CI: none. `.github/workflows/audit-core.yml` is `[MISSING]`.
- Build: the Dockerfile builds four binaries into one image: `audit-core`, `nats-provisioner`, `control-event-bridge` and `nats-consumer-migrate`.
- Compose: service `audit-core` (context `apps/Control Plane/audit-core`), host port 8187 (`AUDIT_CORE_HTTP_HOST_PORT`); `audit-nats-provisioner` and `audit-extra-nats-provisioner` are built from the same context. Rebuild: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build audit-core`.
- Up-proof: healthcheck `wget --spider http://127.0.0.1:8187/healthz` (compose). The route in code is UNVERIFIED.

**R17 auth-core (NestJS, TypeScript).** Path `apps/Control Plane/auth-core/` `[COMMITTED]` (`apps/Control Plane/auth-core/package.json`, `apps/Control Plane/auth-core/pnpm-lock.yaml`, `apps/Control Plane/auth-core/Dockerfile` and `apps/Control Plane/auth-core/scripts/lint-ratchet.mjs` `[COMMITTED]`).
- Test: `cd "apps/Control Plane/auth-core" && pnpm install && REQUIRE_CONTRACT=1 pnpm test` (jest). Coverage: `pnpm run test:cov`. Contract spec for the lint ratchet: `pnpm run lint:contract` (not in CI).
- Lint: `pnpm run lint` runs `cd "apps/Control Plane/auth-core" && node scripts/lint-ratchet.mjs`, a ratchet rather than eslint. Typecheck: `pnpm run build` (`nest build`); the Dockerfile asserts that the built entry point exists.
- Codegen: `pnpm run db:generate` and `pnpm run db:push` (drizzle-kit). Not in CI.
- Parity: `bash scripts/tests/principal-registry-parity.test.sh` from the repo root `[COMMITTED]`; it is run by `.github/workflows/auth-service.yml`.
- CI: `.github/workflows/auth-service.yml` `[COMMITTED]`: Node 20, `pnpm install` (no frozen lockfile), lint, build, jest with REQUIRE_CONTRACT=1, registry parity, image push.
- Compose: service `auth-core` (context `apps/Control Plane/auth-core`, container `auth-service`); ports 3011 (HTTP) and 50011 (gRPC). Rebuild: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build auth-core`.
- Up-proof: healthcheck `wget --spider http://127.0.0.1:3011/users/public`, a deliberately public 200 route (the compose comment says the get-session route returns 401 for anonymous calls).
- Traps: the fixtures `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` and `.sha256` are `[MODIFIED-UNCOMMITTED]`. User memory notes (UNVERIFIED in this pass): the full jest suite has 15 pre-existing failures in 3 suites, so diff failing sets against HEAD and do not trust exit codes; worktree files are CRLF, so scripted edits must keep CRLF; the `lint-ratchet` ENOBUFS failure is pre-existing.

**R18 session-core (Control Plane, Go).** Path `apps/Control Plane/session-core/` `[COMMITTED]` (`apps/Control Plane/session-core/Dockerfile`, `apps/Control Plane/session-core/go.mod`, `apps/Control Plane/session-core/internal/http/server.go` `[COMMITTED]`).
- Test: `cd "apps/Control Plane/session-core" && go test -v ./...`; build `go build -v ./cmd/server`. No database is needed (CI comment). `apps/Control Plane/session-core/go.mod` declares `go 1.25`; the Dockerfile uses golang 1.26.5.
- CI: `.github/workflows/session-core-service.yml` `[COMMITTED]` (Go 1.25). This is not the Model Plane Rust session-core (R01).
- Compose: service `session-core` (context `apps/Control Plane/session-core`, container `session-core-service`); ports 3017 (HTTP) and 50017 (gRPC). Rebuild: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build session-core`.
- Up-proof: healthcheck `wget --spider http://127.0.0.1:3017/health`; route at `apps/Control Plane/session-core/internal/http/server.go` line 58.
- Traps: two services are named `session-core` in different compose projects (R01 and R18). Keep them apart in commands and logs.

**R19 billing-core (Go).** Path `apps/Control Plane/billing-core/` `[COMMITTED]` (`apps/Control Plane/billing-core/Dockerfile`, `apps/Control Plane/billing-core/go.mod`, `apps/Control Plane/billing-core/internal/http/server.go`, `apps/Control Plane/billing-core/internal/http/service_auth.go` `[COMMITTED]`).
- Test: `cd "apps/Control Plane/billing-core" && go test -v -p 1 ./...` with `CONTROL_LIFECYCLE_TEST_DATABASE_URL` (database `billing_lifecycle`) and the fixture marker row (CI creates it with psql). Build: `go build -v ./cmd/server`.
- CI: `.github/workflows/billing-core-service.yml` `[COMMITTED]` (Go 1.25, Postgres 16 service).
- Compose: service `billing-core` (container `billing-core-service`); ports 3014 (HTTP) and 50013 (gRPC). Rebuild: `cd "apps/Control Plane" && scripts/run-control-plane.sh up -d --build billing-core`.
- Up-proof: healthcheck `wget --spider http://127.0.0.1:3014/health`; route at `apps/Control Plane/billing-core/internal/http/server.go` line 58.
- Traps: the Nexi webhook path is exempt from service auth (`apps/Control Plane/billing-core/internal/http/service_auth.go` line 442). Lago services in the same compose are vendor services and are not built here.

### Application Plane

Compose file `apps/Application Plane/docker-compose.yml` `[COMMITTED]` (project `application-plane`; 4-space indentation). Services: `convex-backend`, `convex-dashboard`, `convex-gateway`, `convex-subscriber`, `affine-runtime`, `application-postgres`, `application-dragonfly`, `conversation-core-go`, `conversation-ingest-rs`, `social-core`, `insight-core`, `leads-core`, `information-core`, `notification-core`, `app-nats`, `jetstream-provisioner`, Prometheus and Grafana. Local runner: `apps/Application Plane/scripts/run-application-plane.sh` `[COMMITTED]` (its header says it mirrors the Control Plane runner; its subcommands were not read). Direct form: `cd "apps/Application Plane" && docker compose up -d --build <service>`. The Application Plane has no tracked Makefile (`apps/Application Plane/Makefile` is `[MISSING]`). The snapshot shows no uncommitted changes in this plane.

**R20 convex-core (Convex, TypeScript).** Path `apps/Application Plane/convex-core/` `[COMMITTED]` (`apps/Application Plane/convex-core/package.json`, `apps/Application Plane/convex-core/convex.json`, `apps/Application Plane/convex-core/Dockerfile`, `apps/Application Plane/convex-core/docker-compose.yml` `[COMMITTED]`; functions in `apps/Application Plane/convex-core/convex/` `[COMMITTED]`).
- Test: `cd "apps/Application Plane/convex-core" && pnpm test`, which is `tsx --test test/*.test.cjs`. The 13 test files under `apps/Application Plane/convex-core/test/` `[COMMITTED]` include the space lifecycle, presence, read-marker and agent-registry suites.
- Lint and typecheck: `pnpm typecheck` (`tsc --noEmit`) and `pnpm lint` (`eslint "convex/**/*.ts"`). The 2026-07-11 audit `apps/Application Plane/docs/core-research/plane-audit-2026-07-11.md` `[COMMITTED]` reports both blocked by pnpm's ignored-build policy for `esbuild@0.27.0` (`ERR_PNPM_IGNORED_BUILDS`). Current status UNVERIFIED.
- Codegen: `pnpm run generate` (`convex codegen`). Generated output under `apps/Application Plane/convex-core/convex/_generated` is expected to be tracked (per the compose comment; UNVERIFIED for this snapshot).
- Deploy: `pnpm run deploy` is `convex deploy --prod`, a production deployment. `pnpm run dev` is `convex dev`.
- CI: none. `apps/Application Plane/convex-core/Makefile` is `[MISSING]`; `.github/workflows/convex-core.yml` is `[MISSING]`.
- Compose: `convex-backend` (ports 3210 API and 3211 HTTP actions; healthcheck `curl -f http://127.0.0.1:3210/version`); `convex-gateway` (builds `apps/Application Plane/convex-core`, runs `npx convex dev` through `apps/Application Plane/convex-core/startup.sh`, healthcheck `pgrep -f '[c]onvex dev'`, mounts `apps/Application Plane/convex-core/convex` read-write at `/app/convex`); `convex-subscriber` (healthcheck `pgrep -f 'nats-subscriber'`). `apps/Application Plane/convex-core/docker-compose.yml` `[COMMITTED]` duplicates the `convex-gateway` block and says both must be kept in step.
- Rebuild or push: edits under `apps/Application Plane/convex-core/convex/` reach the backend through the bind mount. Restart with `docker restart convex-gateway`, or `docker compose restart convex-gateway` (`apps/Application Plane/convex-core/CONVEX_INTEGRATION.md` `[COMMITTED]`, line 313). Wait for "Convex functions ready!" (user memory note).
- Up-proof: `curl -f http://127.0.0.1:3210/version` for the backend. The gateway has no HTTP surface (`apps/Application Plane/docs/core-research/convex-core.md` `[COMMITTED]`, line 23).
- Traps: `apps/Application Plane/convex-core/CONVEX_INTEGRATION.md` (line 410) says `deploy_convex_functions` runs `npx convex deploy` against `convex-backend:3210`. Do not use `pnpm run deploy` for local work.

**R21 notification-core (Go).** Path `apps/Application Plane/notification-core/` `[COMMITTED]` (`apps/Application Plane/notification-core/go.mod`, `apps/Application Plane/notification-core/Dockerfile`, `apps/Application Plane/notification-core/internal/http/server.go`, `apps/Application Plane/notification-core/internal/database/migrate_test.go` `[COMMITTED]`).
- Test: `cd "apps/Application Plane/notification-core" && go test -v ./...`; build `go build -v ./cmd/server`. The status doc reports `go test -race` elsewhere; the user memory note says cgo is unavailable on this host, so `-race` is not available here.
- CI: `.github/workflows/notification-core-service.yml` `[COMMITTED]`.
- Compose: service `notification-core` (context `apps/Application Plane/notification-core`), port 3140. Rebuild: `cd "apps/Application Plane" && docker compose up -d --build notification-core`.
- Up-proof: healthcheck `wget --no-verbose --tries=1 -O /dev/null http://localhost:3140/health`; route in `apps/Application Plane/notification-core/internal/http/server.go` line 59.
- Traps: migration tests use fakes, so the real migration path is not exercised by CI.

**R22 conversation-core-go (Go).** Path `apps/Application Plane/conversation-core/conversation-core-go/` `[COMMITTED]` (`apps/Application Plane/conversation-core/conversation-core-go/go.mod`, `apps/Application Plane/conversation-core/conversation-core-go/Dockerfile` `[COMMITTED]`).
- Test: `cd "apps/Application Plane/conversation-core/conversation-core-go" && go test -v ./...`; build `go build -v ./cmd/server`. 44 test files tracked.
- CI: `.github/workflows/conversation-core-service.yml` `[COMMITTED]` (Go 1.25).
- Compose: service `conversation-core-go` (context `apps/Application Plane/conversation-core/conversation-core-go`), host port 3160. Rebuild: `cd "apps/Application Plane" && docker compose up -d --build conversation-core-go`.
- Up-proof: healthcheck `wget -qO- http://localhost:3160/health`; route at `apps/Application Plane/conversation-core/conversation-core-go/internal/http/server.go` line 53.
- Traps: the migrations contract test asserts against migration SQL text on disk, not a live database.

**R23 conversation-ingest-rs (Rust).** Path `apps/Application Plane/conversation-core/conversation-ingest-rs/` `[COMMITTED]` (`apps/Application Plane/conversation-core/conversation-ingest-rs/Cargo.toml`, `apps/Application Plane/conversation-core/conversation-ingest-rs/Dockerfile`, `apps/Application Plane/conversation-core/conversation-ingest-rs/src/lib.rs` `[COMMITTED]`).
- Test: `cd "apps/Application Plane/conversation-core/conversation-ingest-rs" && cargo test`. It is a standalone crate with its own lock file, and it is not a member of the Data Plane or Model Plane workspaces. It has one `#[cfg(test)]` module in `apps/Application Plane/conversation-core/conversation-ingest-rs/src/lib.rs` and no `apps/Application Plane/conversation-core/conversation-ingest-rs/tests/` directory.
- CI: none.
- Compose: service `conversation-ingest-rs` (context `apps/Application Plane/conversation-core/conversation-ingest-rs`), host port 3161. Rebuild: `cd "apps/Application Plane" && docker compose up -d --build conversation-ingest-rs`.
- Up-proof: healthcheck `wget -qO- http://localhost:3161/health`; route in `apps/Application Plane/conversation-core/conversation-ingest-rs/src/lib.rs` line 158.
- Traps: user memory note: the ingest bridge returns 401 when tokens mismatch; never re-add Control Plane tokens to `apps/Ingestion Plane/integration-corev2/.env`.

**R24 social-core (Go).** Path `apps/Application Plane/social-core/` `[COMMITTED]` (`apps/Application Plane/social-core/go.mod`, `apps/Application Plane/social-core/internal/http/server.go`, `apps/Application Plane/social-core/internal/social/repository_postgres_test.go` `[COMMITTED]`).
- Test: `cd "apps/Application Plane/social-core" && go test -v -p 1 ./...` with `SOCIAL_CORE_TEST_DATABASE_URL` pointing at the CI Postgres 16 service. The org-scoping suite self-applies migrations.
- CI: `.github/workflows/social-core-service.yml` `[COMMITTED]` (Go 1.25, Postgres 16).
- Compose: service `social-core` (context `apps/Application Plane/social-core`), port 3162. Rebuild: `cd "apps/Application Plane" && docker compose up -d --build social-core`.
- Up-proof: healthcheck `wget -qO- http://localhost:3162/health`; route at `apps/Application Plane/social-core/internal/http/server.go` line 50.

### Data Plane v2

Makefile `apps/Data Plane v2/Makefile` `[COMMITTED]` (section 5). Compose file `apps/Data Plane v2/docker-compose.yml` `[MODIFIED-UNCOMMITTED]`: application services are private to `dpv2-net` and publish no host ports. Bring-up: `cd "apps/Data Plane v2" && make up` (creates `.env` from `apps/Data Plane v2/.env.example` `[MODIFIED-UNCOMMITTED]` when absent, then `docker compose up -d` with SOURCE_REVISION and BUILD_DATE). One service: `docker compose build <service> && docker compose up -d <service>`. Health from inside a container: `docker compose exec <service> curl http://127.0.0.1:<port>/readyz`. Cross-plane overlay: `make cross-plane-up` (needs the `inter-plane-bus` network provisioned by Control Plane). User memory note: the base compose uses a private bus, and without the overlay documents-api crash-loops and GDPR erasure fails silently.

**R25 retrieval-engine-rs (Rust).** Path `apps/Data Plane v2/services/retrieval-engine-rs/` `[COMMITTED]` (`apps/Data Plane v2/services/retrieval-engine-rs/Dockerfile` `[COMMITTED]`; `apps/Data Plane v2/services/retrieval-engine-rs/Cargo.toml` `[MODIFIED-UNCOMMITTED]`).
- Test: `cd "apps/Data Plane v2" && cargo test --workspace --tests` (CI "Unit tests, whole workspace"). Integration: `TEST_DATABASE_URL=<postgres 16 dataplane_test> cargo test -p retrieval-engine-rs --tests -- --nocapture` (CI `rust-integration-tests`). `make test-integration` runs only `--test grpc_integration`. New tests `apps/Data Plane v2/services/retrieval-engine-rs/tests/rerank_contract.rs`, `apps/Data Plane v2/services/retrieval-engine-rs/tests/zdr_dense_egress.rs`, `apps/Data Plane v2/services/retrieval-engine-rs/tests/chunk_context_join.rs` and `apps/Data Plane v2/services/retrieval-engine-rs/src/egress_attestation.rs` are `[UNCOMMITTED-NEW]`; `apps/Data Plane v2/services/retrieval-engine-rs/tests/grpc_integration.rs` is `[MODIFIED-UNCOMMITTED]`.
- Lint: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo check --workspace`. CI sets `RUSTFLAGS="-D warnings"` for all cargo jobs. `make clippy-rs` omits `--all-targets`.
- Codegen: `make proto` runs `apps/Data Plane v2/scripts/gen-clients.sh` `[COMMITTED]`.
- CI: `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]`, jobs `rust-check`, `rust-integration-tests` and `docker-smoke`.
- Compose: service `retrieval-engine` (context `apps/Data Plane v2`, dockerfile `apps/Data Plane v2/services/retrieval-engine-rs/Dockerfile`). Rebuild: `cd "apps/Data Plane v2" && docker compose build retrieval-engine && docker compose up -d retrieval-engine`.
- Up-proof: `docker compose exec retrieval-engine curl -sf http://127.0.0.1:8004/readyz` (the compose healthcheck; the CI `--wait` uses it). The service listens on 8014 inside the container (Prometheus target `retrieval-engine:8014`).
- Traps: the CI `curl` to `localhost:8014/health` cannot reach it (section 1 item 6). The Makefile cache root (section 1 item 12).

**R26 documents-api-go (Go).** Path `apps/Data Plane v2/services/documents-api-go/` `[COMMITTED]` (`apps/Data Plane v2/services/documents-api-go/go.mod` declares `go 1.26.5`; `apps/Data Plane v2/services/documents-api-go/Dockerfile` `[COMMITTED]`).
- Test: `cd "apps/Data Plane v2/services/documents-api-go" && go build ./... && go vet ./... && go test ./...` (CI job `go-build`, which pins Go 1.23). Integration, Postgres-gated and behind a build tag: `TEST_DATABASE_URL=<postgres 16 documents_test> go test -tags=integration -v ./internal/repo/...` (CI job `documents-api-postgres-tests`, working directory is the module).
- Lint: `go vet ./...`. Codegen: none local. `make go-tidy` tidies the four Go modules.
- Migrations: `make migrate-up` runs `docker compose run --rm migrate up` (R30).
- CI: `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]`.
- Compose: service `documents-api` (context `apps/Data Plane v2/services/documents-api-go`). Rebuild: `cd "apps/Data Plane v2" && docker compose build documents-api && docker compose up -d documents-api`.
- Up-proof: healthcheck `curl -sf http://127.0.0.1:8010/readyz`.
- Traps: the compose service name is `documents-api`; the directory is `documents-api-go`.

**R27 wiki-store-go (Go).** Path `apps/Data Plane v2/services/wiki-store-go/` `[COMMITTED]` (`apps/Data Plane v2/services/wiki-store-go/Dockerfile` `[COMMITTED]`).
- Test: `cd "apps/Data Plane v2/services/wiki-store-go" && go build ./... && go vet ./... && go test ./...` (CI `go-build`, 53 tests per the CI comment). Whether it uses a workspace file is UNVERIFIED.
- CI: `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]` (go-build only; not in the Postgres contract list).
- Compose: service `wiki-store`, context `apps/Data Plane v2` (the Data Plane root), dockerfile `apps/Data Plane v2/services/wiki-store-go/Dockerfile`. Rebuild: `cd "apps/Data Plane v2" && docker compose build wiki-store && docker compose up -d wiki-store`.
- Up-proof: healthcheck `curl -sf http://127.0.0.1:8011/readyz`.
- Traps: the build context is the plane root, so the paths the Dockerfile copies need checking (UNVERIFIED; not read).

**R28 data-orchestrator-go (Go).** Path `apps/Data Plane v2/services/data-orchestrator-go/` `[COMMITTED]` (`apps/Data Plane v2/services/data-orchestrator-go/Dockerfile` `[COMMITTED]`).
- Test: `cd "apps/Data Plane v2/services/data-orchestrator-go" && go build ./... && go vet ./... && go test ./...` (CI `go-build`; 77 tests per the CI comment).
- CI: `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]`.
- Compose: service `data-orchestrator` (context `apps/Data Plane v2/services/data-orchestrator-go`). Rebuild: `cd "apps/Data Plane v2" && docker compose build data-orchestrator && docker compose up -d data-orchestrator`.
- Up-proof: healthcheck `curl -sf http://127.0.0.1:8012/readyz`.

**R29 data-quality-go (Go).** Path `apps/Data Plane v2/services/data-quality-go/` `[COMMITTED]` (`apps/Data Plane v2/services/data-quality-go/Dockerfile` `[COMMITTED]`).
- Test: `cd "apps/Data Plane v2/services/data-quality-go" && go build ./... && go vet ./... && go test ./...` (CI `go-build`; 49 tests per the CI comment). It is also a gated suite in `postgres-contract-tests` (REQUIRE_CONTRACT=1).
- CI: `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]`. The keyword-index parity contract is checked by `apps/Data Plane v2/scripts/check-parity-contract.sh` `[COMMITTED]`.
- Compose: service `data-quality` (context `apps/Data Plane v2/services/data-quality-go`). Rebuild: `cd "apps/Data Plane v2" && docker compose build data-quality && docker compose up -d data-quality`.
- Up-proof: healthcheck `curl -sf http://127.0.0.1:8013/readyz`.

**R30 tools/migrator (Go).** Path `apps/Data Plane v2/tools/migrator/` `[COMMITTED]` (`apps/Data Plane v2/tools/migrator/go.mod` declares `go 1.26.5`; `apps/Data Plane v2/tools/migrator/Dockerfile` `[COMMITTED]`).
- Build: `make migrate-build` builds the migrator from `apps/Data Plane v2/tools/migrator/` and writes the binary outside the repo tree.
- Run: `make migrate-status` and `make migrate-up` (`docker compose run --rm migrate status|up`). Compose service `migrate` (context `apps/Data Plane v2/tools/migrator`).
- CI: `postgres-contract-tests` in `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]` uses `go-version-file: apps/Data Plane v2/tools/migrator/go.mod` and applies the init script and every migration through this tool to a disposable Postgres 16.
- Traps: the job takes its Go version from `apps/Data Plane v2/tools/migrator/go.mod` (1.26.5), not from the 1.23 pin used by other jobs. Whether the runner fetches the toolchain is UNVERIFIED.

### Ingestion Plane

Makefile `apps/Ingestion Plane/Makefile` `[COMMITTED]` (section 5). Compose file `apps/Ingestion Plane/docker-compose.yml` `[MODIFIED-UNCOMMITTED]`. Its Makefile variable `COMPOSE` passes two env files, the repo-root env file and the plane env file, so both must exist (both gitignored). Bring-up: `cd "apps/Ingestion Plane" && make up` (runs `up -d --build --wait --wait-timeout 300`).

**R31 imports-core (Python).** Path `apps/Ingestion Plane/imports-core/` `[COMMITTED]` (`apps/Ingestion Plane/imports-core/requirements.txt`, `apps/Ingestion Plane/imports-core/Dockerfile`, `apps/Ingestion Plane/imports-core/tests/`, `apps/Ingestion Plane/imports-core/migrations/001_init.sql` `[COMMITTED]`).
- Test: `cd "apps/Ingestion Plane/imports-core" && python -m venv venv && . venv/bin/activate && pip install -r requirements.txt && pytest -v tests/` (Makefile targets `setup` and `test-imports`).
- Lint or typecheck: none found.
- Migrations: `cd "apps/Ingestion Plane" && make migrate` pipes `apps/Ingestion Plane/imports-core/migrations/001_init.sql` into database `imports` through `psql` in the `postgres` service.
- CI: none (no workflow references imports-core).
- Compose: service `imports-api` (context `apps/Ingestion Plane/imports-core`, Python 3.13 image), port 3025. Rebuild: `cd "apps/Ingestion Plane" && make up`.
- Up-proof: image HEALTHCHECK and compose healthcheck `http://localhost:3025/ready`; `make test-endpoints` checks `http://127.0.0.1:3025/ready`.
- Traps: `make ci-test` (imports, quarry, endpoints) is not run by any workflow.

**R32 integration-corev2 (Go).** Path `apps/Ingestion Plane/integration-corev2/` `[COMMITTED]` (`apps/Ingestion Plane/integration-corev2/go.mod` `[COMMITTED]`, `apps/Ingestion Plane/integration-corev2/Dockerfile` `[COMMITTED]`, `apps/Ingestion Plane/integration-corev2/services/webhook-normalizer-rs/` `[COMMITTED]`).
- Test: `cd "apps/Ingestion Plane/integration-corev2" && go test ./...` (inferred; `apps/Ingestion Plane/integration-corev2/Makefile` is `[MISSING]`). Build: `go build ./cmd/api`; the Dockerfile builds `apps/Ingestion Plane/integration-corev2/cmd/api`, `apps/Ingestion Plane/integration-corev2/cmd/finspo-worker` and `apps/Ingestion Plane/integration-corev2/cmd/email-worker`.
- CI: none.
- Compose: service `integration-api` (context `apps/Ingestion Plane/integration-corev2`, port 3026, healthcheck `wget -q -O /dev/null http://127.0.0.1:3026/health`); workers `integration-finspo-worker` and `integration-email-worker` (no healthcheck); `integration-webhook-normalizer` (context `apps/Ingestion Plane/integration-corev2/services/webhook-normalizer-rs`, host 13036 to container 3036, healthcheck `/health`). Rebuild: `cd "apps/Ingestion Plane" && make up`.
- Traps: user memory note: never re-add Control Plane tokens to `apps/Ingestion Plane/integration-corev2/.env`; a 401 on the ingest bridge means a token mismatch with `apps/Application Plane/conversation-core/conversation-ingest-rs/`. `apps/Ingestion Plane/integration-corev2/internal/api/zdr_acknowledgement.go` is `[UNCOMMITTED-NEW]`.

**R33 quarry-edge (Rust).** Path `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/` (`apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/Cargo.toml` `[COMMITTED]`; `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` `[MODIFIED-UNCOMMITTED]`).
- Test: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test --workspace` (CI job `cargo-check`; `make test-rust`). New files `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/model_plane_gate.rs` and `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/rerank_behavior.rs` are `[UNCOMMITTED-NEW]`.
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` (`make lint`; not in CI). CI runs cargo-audit with `--deny warnings` and four ignored advisories.
- Codegen: an OpenAPI-generated SDK from `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml` `[MODIFIED-UNCOMMITTED]`, via `apps/Ingestion Plane/Quarry-v2/sdks/generate.sh` `[COMMITTED]` (runs the generator in Docker). The generated TypeScript SDK is gitignored output that the verevonv3 CI generates and builds before install.
- CI: `.github/workflows/quarry-v2-ci.yml` `[COMMITTED]` (base `main` only).
- Compose: service `quarry-edge` (build context `apps/Ingestion Plane/Quarry-v2`, dockerfile `apps/Ingestion Plane/Quarry-v2/deploy/compose/Dockerfile.edge` `[COMMITTED]`). Rebuild: `cd "apps/Ingestion Plane" && make up`.
- Up-proof: `curl http://127.0.0.1:8082/health` (`make test-endpoints`); compose healthcheck `wget --quiet --tries=1 -O /dev/null http://127.0.0.1:8082/health`.
- Traps: user memory note: Quarry Rust tests in Docker need a `rust:1` container with cache volumes, `bash -c` rather than `-lc`, and more than 10 minutes. The Makefile cache root (section 1 item 12).

**R34 quarry-runtime (Rust).** Path `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/` (`apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/Cargo.toml` `[COMMITTED]`).
- Test: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime` (inside `cargo test --workspace`). `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/build.rs` needs `protoc`; CI installs 27.x. CI comments say the cache-cleanup step must not delete the protoc install directory.
- Lint: as R33.
- New files `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/data_plane_http.rs`, `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_rerank_contract.rs`, `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_retrieve_contract.rs` and the directory `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/` are `[UNCOMMITTED-NEW]`. The contract directory `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/` is `[MODIFIED-UNCOMMITTED]` in part (its two files are `[MODIFIED-UNCOMMITTED]`).
- CI: `.github/workflows/quarry-v2-ci.yml` `[COMMITTED]`. The docs contract reads `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/` through `.github/workflows/quarry-docs-contract.yml` `[UNCOMMITTED-NEW]`.
- Compose: no separate service; the library is used by the quarry-edge and quarry-orchestrator images.
- Traps: `publisher_integration` and `dom_summary_proptest` link very large binaries. CI sets `CARGO_PROFILE_TEST_DEBUG=0` on the cargo-check job to avoid linker crashes from a full disk (CI comment).

**R35 quarry-browser (Rust).** Path `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/` (`apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/Cargo.toml` `[COMMITTED]`).
- Test: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-browser --features chromiumoxide` (CI runs this explicitly, because the feature is not default). Check: `cargo check -p quarry-browser --features chromiumoxide`.
- Lint: as R33. The Model Plane proposes browser actions, and Quarry executes or rejects them (`apps/Model Plane/CLAUDE.md`; repo root `CLAUDE.md`).
- CI: `.github/workflows/quarry-v2-ci.yml` `[COMMITTED]`.
- Compose: no separate service in the Ingestion compose; the browser is part of the edge or runtime images (UNVERIFIED).
- Traps: the rerank files `apps/Ingestion Plane/Quarry-v2/lab/evals/web-rerank-baseline.json` `[UNCOMMITTED-NEW]` and `apps/Ingestion Plane/Quarry-v2/docs/WEB_RERANK_GATE_RUNBOOK.md` `[UNCOMMITTED-NEW]` belong to the rerank gate, not to the browser. Keep them in the rerank commit.

**R36 quarry-control (Go).** Path `apps/Ingestion Plane/Quarry-v2/services/quarry-control/` (`apps/Ingestion Plane/Quarry-v2/services/quarry-control/go.mod` `[COMMITTED]`; declares `go 1.25.0`).
- Test: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...` (Makefile `test-go`). Build: `go build ./...`. Vet: `go vet ./...` (Makefile `lint`).
- CI: `.github/workflows/quarry-v2-ci.yml` `[COMMITTED]`, job `go-coverage` only, `continue-on-error: true`, 80 percent floor; the CI comment reports 41.6 percent (2026-09-08). Not gating.
- Compose: service `quarry-control` (dockerfile `apps/Ingestion Plane/Quarry-v2/deploy/compose/Dockerfile.control` `[COMMITTED]`), host port 28081, container 8081; healthcheck `wget --quiet --tries=1 -O /dev/null http://127.0.0.1:8081/health`. Rebuild: `cd "apps/Ingestion Plane" && make up`.
- Traps: the `up` target text says 8081 (section 1 item 14).

**R37 quarry-orchestrator (Go).** Path `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/` (`apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/go.mod` `[COMMITTED]`; declares `go 1.25.0`).
- Test: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./...`. CI job `go-coverage` only (report only). The CI comment reports 37.2 percent, with `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/jobs`, `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/activities` and `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/controlauth` at 0 percent.
- Compose: service `quarry-orchestrator` (dockerfile `apps/Ingestion Plane/Quarry-v2/deploy/compose/Dockerfile.orchestrator` `[COMMITTED]`); no healthcheck in the Ingestion compose.
- Up-proof: container status only (health route UNVERIFIED).

### Frontend Plane (verevonv3) and Support Plane

Compose file `apps/Frontend Plane/verevonv3/docker-compose.yml` `[COMMITTED]`: `nats` (container `verevon-nats`), `gateway` (context `apps/Frontend Plane/verevonv3/apps/gateway`, host `127.0.0.1:3185`) and `frontend` (target `dev`, host port `VEREVONV3_HOST_PORT`, default 5173). Production image: the `production` stage of `apps/Frontend Plane/verevonv3/Dockerfile` `[COMMITTED]` (nginx-unprivileged on 8080, HEALTHCHECK on `/health`). `apps/Frontend Plane/verevonv3/apps/gateway/Dockerfile` `[COMMITTED]` builds the gateway with cargo-chef on rust 1.89.

**R38 verevonv3 SPA (SolidJS, Vite).** Path `apps/Frontend Plane/verevonv3/` `[COMMITTED]` (`apps/Frontend Plane/verevonv3/package.json` `[MODIFIED-UNCOMMITTED]`; `apps/Frontend Plane/verevonv3/pnpm-lock.yaml` `[COMMITTED]`; `apps/Frontend Plane/verevonv3/vite.config.ts` `[COMMITTED]`; `apps/Frontend Plane/verevonv3/vite.demo.config.ts` `[UNCOMMITTED-NEW]`).
- Test: `cd "apps/Frontend Plane/verevonv3" && pnpm test`. That runs `node --test` on four gateway scripts under `apps/Frontend Plane/verevonv3/apps/gateway/scripts/` and then `vitest run` (jsdom, setup file `apps/Frontend Plane/verevonv3/src/test/setup.ts` `[COMMITTED]`; `apps/Frontend Plane/verevonv3/tests/e2e/` is excluded from Vitest).
- Lint: `pnpm lint` (`eslint .`). Typecheck: `pnpm typecheck` (`tsc -b`). Build: `pnpm build` (`pnpm typecheck && vite build`). Full gate: `pnpm verify` (lint, typecheck, test, build). `pnpm audit` runs in `.github/workflows/verevonv3-security.yml` `[COMMITTED]`.
- Workspace: `apps/Frontend Plane/verevonv3/pnpm-workspace.yaml` `[COMMITTED]` lists the root, `packages/*` and a `remote-dev` entry under `apps/`, which resolves to `apps/Frontend Plane/verevonv3/apps/remote-dev/`. The gateway at `apps/Frontend Plane/verevonv3/apps/gateway/` is not a pnpm package; it is a separate Cargo crate.
- Prerequisites in CI order: build the generated Quarry SDK, then `pnpm install --frozen-lockfile`, then `pnpm --filter @verevon/remote-core run build`. The SDK steps are `cd "apps/Ingestion Plane/Quarry-v2/sdks" && ./generate.sh`, then `cd typescript && pnpm install --ignore-scripts && pnpm run build` (the typescript output is gitignored).
- Demo bundle: `pnpm demo:build` (`cd "apps/Frontend Plane/verevonv3" && node scripts/build-product-demo.mjs`) and `pnpm demo:probe` (`cd "apps/Frontend Plane/verevonv3" && node scripts/probe-product-demo.mjs`). Both scripts, `apps/Frontend Plane/verevonv3/scripts/build-product-demo.mjs` and `apps/Frontend Plane/verevonv3/scripts/probe-product-demo.mjs`, are `[UNCOMMITTED-NEW]`, and so is the committed bundle directory `apps/Frontend Plane/verevonv3/apps/verevon-web/public/product-demo/`. CI job `demo-product-build` rebuilds the bundle and diffs it.
- Codegen: the Quarry SDK only. No proto or GraphQL codegen in this package.
- CI: `.github/workflows/verevonv3-ci.yml` `[MODIFIED-UNCOMMITTED]`: jobs `fabrication-guard`, `gateway-rust`, `spa-verify`, `docker-image` (`docker buildx build --target production --build-context 'quarry-client=../../Ingestion Plane/Quarry-v2/sdks/typescript' ... --load .`) and `demo-product-build`. All branch-filtered to `main`.
- Compose: service `frontend` (target `dev`). Rebuild: `cd "apps/Frontend Plane/verevonv3" && docker compose up -d --build frontend`. Local alternative: `pnpm dev`.
- Up-proof: the `frontend` healthcheck fetches `http://127.0.0.1:5173/health`. `apps/Frontend Plane/verevonv3/vite.config.ts` line 71 has a `'/health'` entry (probably a proxy; UNVERIFIED). Production: `GET /health` on 8080.
- Traps: user memory notes (UNVERIFIED in this pass): `:5173` is a container over a Windows bind mount, so HMR does not see host edits and the browser caches the global stylesheet across a restart; use a second dev server to verify a worktree; pre-existing typecheck, lint and vitest failures are recorded in memory; the remote-core Docker dev container writes Linux symlinks into workspace `node_modules` (repair with rmdir and a PowerShell `pnpm install`); `typescript` must resolve to the root's TS6 alias, or lint fails.

**R39 verevon gateway (Rust, axum BFF).** Path `apps/Frontend Plane/verevonv3/apps/gateway/` (`apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml` and `apps/Frontend Plane/verevonv3/apps/gateway/Cargo.lock` `[COMMITTED]`; `apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs`, `apps/Frontend Plane/verevonv3/apps/gateway/src/middleware.rs`, `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs` `[COMMITTED]`; integration tests in `apps/Frontend Plane/verevonv3/apps/gateway/tests/` `[COMMITTED]`).
- Test: `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo test`. Root form (CLAUDE.md): `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" --all-targets`.
- Lint: `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` (CI `gateway-rust`).
- Codegen: none.
- Gates on gateway code: `fabrication-guard` in `.github/workflows/verevonv3-ci.yml` fails on `fn org_id_from_headers`, `demo_accounts`, `SocialStore`, `Bearer dev-bypass` outside `apps/Frontend Plane/verevonv3/src/shared/api/http.ts` `[MODIFIED-UNCOMMITTED]` and the SSE client `apps/Frontend Plane/verevonv3/src/shared/api/sse.ts`, `region default`, a missing `x-org-id` or `x-verevon-org-id` entry in STRIPPED_HEADERS in the gateway middleware, and org-header reads in domain files before `#[cfg(test)]`.
- Compose: service `gateway` (container `verevon-gateway-rs`, `VEREVON_BIND_ADDRESS:3185`). Rebuild: `cd "apps/Frontend Plane/verevonv3" && docker compose up -d --build gateway`.
- Up-proof: `curl -sf http://127.0.0.1:3185/health` (compose healthcheck); route at `apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs` line 91. The listening port comes from the `PORT` env var (line 76).
- Traps: user memory note: upstream configuration lives on AppState, not env, and a handler reading `std::env::var` raced under parallel tests. `apps/Frontend Plane/verevonv3/apps/gateway/scripts/run-security-coverage.sh` `[COMMITTED]` is not referenced by any workflow.

**R40 SPA end-to-end (Playwright).** Config `apps/Frontend Plane/verevonv3/playwright.config.ts` `[COMMITTED]`; specs under `apps/Frontend Plane/verevonv3/tests/e2e/` `[COMMITTED]`.
- Run: `cd "apps/Frontend Plane/verevonv3" && pnpm test:e2e` (`playwright test`); browser install: `pnpm test:e2e:install`. Base URLs: `E2E_BASE_URL` (default `http://localhost:5199`) and `LOCAL_E2E_BASE_URL` (default `http://localhost:5173`). The config has no `webServer` block, so the stack must already run. A second config, `apps/Frontend Plane/verevonv3/playwright.real-authority.config.ts` `[COMMITTED]`, exists.
- CI: none. The verevonv3 CI runs `pnpm verify`, which does not include e2e.
- Traps: needs the gateway and the frontend running (R38, R39) and an authenticated storage state; not reproducible from the repo alone (UNVERIFIED).

**R41 remote-core package (Support Plane client).** Path `apps/Frontend Plane/verevonv3/packages/remote-core/` (`apps/Frontend Plane/verevonv3/packages/remote-core/package.json` `[COMMITTED]`; `build` is `tsc -p tsconfig.build.json`, `test` is `vitest run`, `lint` is `eslint .`, `typecheck` is `tsc -p tsconfig.json`).
- Test: `cd "apps/Frontend Plane/verevonv3" && pnpm --filter @verevon/remote-core run test`. Build: `pnpm --filter @verevon/remote-core run build` (a CI `spa-verify` prerequisite; its build output is gitignored).
- Support Plane backend: `apps/Support Plane/docker-compose.yml` `[COMMITTED]`, `apps/Support Plane/README.md` `[COMMITTED]`. No Makefile (`apps/Support Plane/Makefile` is `[MISSING]`). No CI for the backend. Its test commands and hbbs/hbbr health are UNVERIFIED (not read).

---

## 5. Root and per-plane Makefiles

Root `Makefile` `[COMMITTED]` has no build, test or deploy targets:
- `help`: prints the target list.
- `check-bases`: derives the digest-pinned `FROM` lines from every `Dockerfile*` under `apps/` (pruning `node_modules`, `.git`, `target`, `.claude`, `dist`, `.venv` and `vendor`), checks each with `docker image inspect`, and exits 1 if any are missing. This is the strict gate.
- `warm-bases`: pulls missing pinned bases. It is non-fatal inside `apps/Model Plane/scripts/compose.sh`.
- Both need Docker. Commands: `make check-bases`, `make warm-bases`.

Per-plane Makefiles that exist:
- `apps/Data Plane v2/Makefile` `[COMMITTED]`: `up` (cp-env, `docker compose up -d`), `standalone-up`, `cross-plane-up` (checks the environment and requires the Control-provisioned `inter-plane-bus`), `down`, `build`, `logs`, `status`, `docker-build`, `docker-build-clean`, `docker-up-obs` (Prometheus and Grafana profile), `docker-test`, `check-rs` (`cargo check --workspace`), `test-rs` (`cargo test --workspace`), `fmt-rs`, `clippy-rs` (`-D warnings`, no `--all-targets`), `build-go` and `vet-go` (four Go services), `test-integration` (`cargo test -p retrieval-engine-rs --test grpc_integration -- --nocapture`, needs `TEST_DATABASE_URL`), `smoke`, `smoke-health`, `auth-matrix`, `test-security-scripts` (seven scripts, including the graph-index and background-budget contracts), `ci` (fmt-rs, clippy-rs, check-rs, build-go, vet-go, tenant-static-check, test-security-scripts), `tenant-check`, `proto` (`apps/Data Plane v2/scripts/gen-clients.sh`), `migrate-build`, `migrate-status`, `migrate-up`, `backup`, `restore`, `load-grpc`, `load-http` (k6), the paid eval targets (`eval-retrieval`, `eval-gate`, `eval-check`, `eval-variant`, `eval-harness-test`; not CI), `go-tidy`, and `clean` (`docker compose down -v --remove-orphans`, destructive).
- `apps/Ingestion Plane/Makefile` `[COMMITTED]`: `setup` (venv and pip for imports-core, `cargo fetch`), `up`, `down`, `clean` (`down -v`), `build`, `dev-imports` (uvicorn on 3025), `dev-quarry` (`cargo run -p quarry-edge`), `logs`, `shell-imports`, `shell-postgres`, `test-imports` (pytest), `test-quarry` (`cargo test --workspace`), `test-endpoints` (readiness checks: quarry-edge 8082, imports-api 3025 `/ready`, integration-api 3026 `/health`, finspo 3130, shipping-core 3156 `/healthz`, postgres, dragonfly and temporal), `test-load` (ab), `migrate` (psql with `apps/Ingestion Plane/imports-core/migrations/001_init.sql`), `seed`, `db-reset`, `metrics`, `stats`, `temporal-health`, `push-images`, `deploy-k8s`, `docs`, `version`, `ci-test` (imports, quarry, endpoints; not invoked by any workflow), `smoke-integration`, `ci-build`, `ci-push`, `warm-bases`.
- `apps/Ingestion Plane/Quarry-v2/Makefile` `[COMMITTED]`: `bootstrap`, `build` (`build-rust` = `cargo build --workspace`; `build-go`), `test` (`test-rust` = `cargo test --workspace`; `test-go` runs go test in quarry-control and quarry-orchestrator), `test-vision` (pytest in `apps/Ingestion Plane/Quarry-v2/services/quarry-vision-sidecar`), `bench-browser`, `fmt` (cargo fmt, gofmt), `lint` (clippy `-D warnings`, go vet), `dev`, `down`, `clean`, `tidy`.
- `apps/Control Plane/user-core/Makefile` `[COMMITTED]`: `proto-generate`, `build`, `build-dual`, `build-all`, `run`, `run-dual`, `test`, `test-coverage`, `tidy`, `clean`, `deps`, `fmt`, `lint` (golangci-lint), `dev` (air).
- `apps/Model Plane/python/eval-lab-py/Makefile` `[COMMITTED]`: `make selftest` is referenced by the workflow comment; the Makefile was not read.

Per-plane Makefiles that do not exist (all `[MISSING]`): `apps/Model Plane/Makefile`, `apps/Model Plane/go/Makefile`, `apps/Control Plane/Makefile`, `apps/Control Plane/audit-core/Makefile`, `apps/Application Plane/Makefile`, `apps/Application Plane/convex-core/Makefile`, `apps/Frontend Plane/verevonv3/Makefile`, `apps/Ingestion Plane/integration-corev2/Makefile`, `apps/Support Plane/Makefile`. For those planes, the commands are in the per-service rows and in the plane READMEs.

---

## 6. Pre-commit hook and gitleaks

`.githooks/pre-commit` `[MODIFIED-UNCOMMITTED]` is the only file in `.githooks/`. Activation: `git config core.hooksPath .githooks` (per its header). It runs one check: gitleaks on the staged diff.
- Scope: `git diff --cached` with lockfile excludes (`Cargo.lock`, `go.sum`, `go.work.sum`, `pnpm-lock.yaml`, `package-lock.json`, `yarn.lock`) and `**/verevon-web/public/product-demo/**`, which `apps/Frontend Plane/verevonv3/scripts/build-product-demo.mjs` scans instead.
- Engine: a local `gitleaks` binary if present; else `docker run ... ghcr.io/gitleaks/gitleaks:latest` (unpinned); else it warns and allows the commit, since CI is the backstop.
- Arguments: `detect --pipe --redact --no-banner`, plus `--config .gitleaks.toml` when that file exists at the top level.
- Exit handling: 0 passes; 1 blocks with the message "Do not use --no-verify"; 125, 126 and 127 (the container did not start) pass with a warning; any other code blocks (fails closed).
- The hook runs no lint, test, typecheck or contract check.

`.gitleaks.toml` `[MODIFIED-UNCOMMITTED]`: title "Verevon CoreSystem secret-scan"; `[extend] useDefault = true`; one `[allowlist]` table (singular) with a `paths` list. Its header (lines 9-19) says CI pins gitleaks 8.24.3 through `gitleaks-action@v3`, that this version reads only the singular `[allowlist]`, and that a plural `[[allowlists]]` array is silently ignored. The hook's `latest` image is a different version and mode, so the file header says the hook cannot catch this.

`.github/workflows/secret-scan.yml` `[COMMITTED]`: `gitleaks/gitleaks-action@v3`, `GITLEAKS_CONFIG: ${{ github.workspace }}/.gitleaks.toml`, `GITHUB_TOKEN` set (required for PR scans), full-history checkout (`fetch-depth: 0`), `pull-requests: read` permission, and a skip for Dependabot PR runs. `GITLEAKS_LICENSE` is commented out and is needed only for organisation-owned repositories (UNVERIFIED whether this repository is organisation-owned; the origin owner appears to be a user account).

---

## 7. Contract sync, wire compatibility and proto gates

| Gate | File (state) | Runs in | Checks |
|---|---|---|---|
| SSE taxonomy freshness | `apps/Model Plane/scripts/gen-sse-event-taxonomy.py` `[COMMITTED]` with `--check` | `.github/workflows/sse-taxonomy.yml` `[COMMITTED]` (PRs and pushes touching the watched paths) | Doc `apps/Model Plane/docs/sse-event-taxonomy.md` `[MODIFIED-UNCOMMITTED]` must match `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs` `[MODIFIED-UNCOMMITTED]`. The job also removes the `memory_recall` row and expects the check to fail (drift self-proof). |
| Cross-plane goldens and ZDR pins | `scripts/tests/zdr-contract-sync.test.sh` `[MODIFIED-UNCOMMITTED]` (and `--self-test`) | `.github/workflows/model-plane-rust-ci.yml` `[MODIFIED-UNCOMMITTED]` (step before fmt); `.github/workflows/contract-goldens.yml` `[COMMITTED]` (no `REQUIRE_CONTRACT`) | Goldens under `apps/Model Plane/api/contracts/`, their vendored copies, sha256 pins and the CONSUMERS table. Pending consumers are reported, and fail only with `REQUIRE_CONTRACT`. |
| ZDR loader | `scripts/tests/zdr-contract-schema.mjs` `[MODIFIED-UNCOMMITTED]`; `scripts/tests/zdr-contract-schema.test.mjs` `[COMMITTED]` | `.github/workflows/contract-goldens.yml` (`node --test`) | Strict loader for the ZDR goldens. |
| Proto lint and breaking | `apps/Model Plane/proto/buf.yaml` `[COMMITTED]`, `apps/Model Plane/proto/buf.gen.yaml` `[COMMITTED]`, `apps/Model Plane/proto/baseline.binpb` `[COMMITTED]` | `.github/workflows/buf.yml` `[COMMITTED]` | Broken by unquoted paths (section 1 item 4). Even when fixed, `apps/Model Plane/CLAUDE.md` says lint carries pre-existing debt. |
| Principal registry parity | `scripts/tests/principal-registry-parity.test.sh` `[COMMITTED]`; `apps/Control Plane/scripts/plane-principal-registry.jq` `[COMMITTED]`; `apps/Control Plane/config/plane-service-principals.json` `[COMMITTED]` | `.github/workflows/auth-service.yml` `[COMMITTED]` | The policy file, launcher and bootstrap produce the same registry, in hermetic sandboxes. |
| Quarry docs contract | `apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs` `[UNCOMMITTED-NEW]` | `.github/workflows/quarry-docs-contract.yml` `[UNCOMMITTED-NEW]` | Compose, config, docs, OpenAPI, token manifest and registry agree; removed rerank identifiers stay removed. |
| Cross-plane token copies (Quarry) | `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json` `[MODIFIED-UNCOMMITTED]` and `.sha256` `[MODIFIED-UNCOMMITTED]` | `.github/workflows/contract-goldens.yml` path `apps/**/cross-plane-token-requests.json*` | Vendored copy must match the auth-core golden pin. |
| Data Plane contract scripts | `apps/Data Plane v2/scripts/check-parity-contract.sh`, `apps/Data Plane v2/scripts/check-eval-contract.sh`, `apps/Data Plane v2/scripts/check-subjects.sh`, `apps/Data Plane v2/scripts/eval-ingest-corpus.py` (all `[COMMITTED]`) | `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]` | Keyword-index parity; eval evidence contract; NATS subject lint; frozen corpus manifest. Each has a `--self-test` mode where the workflow calls it. |
| Data Plane Postgres contracts | `apps/Data Plane v2/tests/scripts/run-postgres-contract-tests.sh` `[COMMITTED]` (`--self-test`, `--lint`) | `.github/workflows/dataplane-v2-ci.yml` jobs `postgres-contract-wiring` and `postgres-contract-tests` | Every Postgres-gated test is registered; migrations apply through `apps/Data Plane v2/tools/migrator/`; `REQUIRE_CONTRACT=1` fails on skipped gated tests. |
| Data Plane config contracts | `apps/Data Plane v2/tests/scripts/contextual-retrieval-config-contract-test.sh`, `apps/Data Plane v2/tests/scripts/background-budget-contract-test.sh`, `apps/Data Plane v2/tests/scripts/graph-index-no-azure-contract-test.sh`, `apps/Data Plane v2/tests/scripts/eval-harness-contract-test.sh` (all `[COMMITTED]`); `apps/Data Plane v2/tests/scripts/rerank-input-contract-test.sh`, `apps/Data Plane v2/tests/scripts/web-rerank-compose-contract-test.sh`, `apps/Data Plane v2/tests/scripts/isolated-registry-parity-test.sh` (all `[UNCOMMITTED-NEW]`) | `.github/workflows/dataplane-v2-ci.yml` `[MODIFIED-UNCOMMITTED]` | Compose defaults, `apps/Data Plane v2/.env.example`, code defaults, OpenAPI and runbook agree. |
| Data Plane read-only pre-flight | `apps/Data Plane v2/scripts/qb-preflight.sh` `[UNCOMMITTED-NEW]` (`--self-test`) | `.github/workflows/dataplane-v2-ci.yml` | No mutating command is issued; no credential is echoed. |
| Data Plane auth matrix | `apps/Data Plane v2/tests/scripts/auth-regression-matrix-test.sh` `[MODIFIED-UNCOMMITTED]` (test); runner `apps/Data Plane v2/scripts/auth-regression-matrix.sh` `[MODIFIED-UNCOMMITTED]` | `.github/workflows/dataplane-v2-ci.yml` job `web-rerank-contract` (test script); `make auth-matrix` (runner); `make test-security-scripts` (test script) | Auth regression matrix rows. |
| Data Plane usage contract | `apps/Data Plane v2/scripts/check-usage-contract.sh` `[MODIFIED-UNCOMMITTED]` | none (not wired) | Usage-ledger contract; not run in CI. |
| Quarry model-plane switches | `scripts/tests/quarry-model-plane-switches.test.mjs` `[UNCOMMITTED-NEW]` | none (not wired) | Not run by any workflow. |
| Tenant isolation static check | `apps/Data Plane v2/scripts/check-tenant-isolation.sh` `[COMMITTED]` | `make tenant-static-check` and `make ci` (local only) | Static tenant checks. |

Other repo-level contract script: `scripts/bootstrap_runtime_environment.sh` `[COMMITTED]` appears in the `auth-service.yml` path filter and is not run in CI. The Verevon fabrication guard, inline in `.github/workflows/verevonv3-ci.yml`, is a grep-based contract over gateway and SPA sources.

---

## 8. Codegen, migrations and deploy commands (summary)

- Protobuf in Rust: compiled in `build.rs` at build time (`apps/Model Plane/rust/crates/mp-contracts/build.rs` `[COMMITTED]`, `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/` `[COMMITTED]`). Requires `protoc` 27.x (CI installs it).
- Protobuf in Go (Model Plane): `buf generate` with `apps/Model Plane/proto/buf.gen.go-only.yaml` `[COMMITTED]` (remote plugins `buf.build/protocolbuffers/go` and `buf.build/grpc/go`; needs network). Checked-in output `apps/Model Plane/go/gen/` `[COMMITTED]` is documented as stale (`apps/Model Plane/docs/capability-ownership-matrix.md` line 145). Use scoped regeneration only.
- Protobuf, Control Plane user-core: `make proto-generate` (protoc with the Go plugins).
- Protobuf, Data Plane: `make proto` runs `apps/Data Plane v2/scripts/gen-clients.sh`.
- sqlx: `sqlx::migrate!` in session-core (compile-time embed; no CLI; no offline data needed). No compile-time `query!` macros were found under `apps/Model Plane/rust/`.
- SQL migrations: Data Plane `apps/Data Plane v2/tools/migrator/` through `make migrate-up` or the CI postgres-contract job; Ingestion `make migrate` (psql with `apps/Ingestion Plane/imports-core/migrations/001_init.sql`); Model Plane Go services ship migration directories in their images (the applier is UNVERIFIED); Control Plane Go services apply migrations through their own test or runtime code (not read); auth-core `pnpm run db:generate` and `pnpm run db:push` (drizzle-kit, not in CI).
- Convex: `pnpm run generate` (`convex codegen`), `pnpm run dev` (`convex dev`), `pnpm run deploy` (`convex deploy --prod`, production). Runtime push through `convex-gateway`.
- OpenAPI SDK: `apps/Ingestion Plane/Quarry-v2/sdks/generate.sh` `[COMMITTED]` (Docker, `openapitools/openapi-generator-cli`). The TypeScript output is gitignored and built in CI.
- Docker bases: every Dockerfile pins its base by digest; the root `make check-bases` is the gate. Most infrastructure images in the compose files are digest-pinned too.
- Deploy: production Model Plane goes through `apps/Model Plane/scripts/release-artifact.sh` (not read). The Control Plane launcher refuses the production overlay. The `deploy-*` jobs in the CI service workflows only echo; nothing is deployed from this repo.

---

## 9. Gaps: services and gates with no CI

- Model Plane Go: capability-core, sandbox-manager, orchestrator-core, letta-bridge, browser-broker, cost-core, bridge-core and nats-provisioner. Per-module `go test ./...` runs only locally.
- Control Plane: audit-core has tests and no CI. session-core, billing-core, user-core, org-core and auth-core have CI (section 2).
- Application Plane: convex-core has 13 test files and no CI; conversation-ingest-rs has no tests directory (`apps/Application Plane/conversation-core/conversation-ingest-rs/tests/` is `[MISSING]`) and no CI; information-core, insight-core and leads-core are Go with no CI (status docs report their tests passing).
- Ingestion Plane: imports-core (pytest) and integration-corev2 have no CI. The Quarry Go services run in report-only jobs.
- Frontend: the Playwright suite has no CI. The demo probe runs in CI only with uncommitted files.
- Support Plane: no CI for the backend. Only the remote-core build step in `.github/workflows/verevonv3-ci.yml`.
- Model Plane: clippy is not gated, and `-D warnings` is not set (CI comment: 304 pedantic warnings).
- Data Plane: several contract scripts run only through `make ci` and `make test-security-scripts` (section 7).

---

## 10. Open questions and UNVERIFIED items

1. Which base branch the team uses for PRs. Origin's default is `dev`; the planning gitStatus says `fix/ui-premium-restore`; origin has no `main` and no `develop`. This decides whether 14 workflows run for PRs at all.
2. Whether any gate is enforced outside the repo (organisation rulesets, merge queue). The API showed none on `dev`.
3. Whether `apps/Model Plane/go/go.work` (section 1 item 10) supersedes the user memory note.
4. Whether CI runners fetch Go 1.26.5 through `GOTOOLCHAIN=auto` (section 1 item 5).
5. Current status of the convex-core `pnpm typecheck` and `pnpm lint` block (2026-07-11 audit).
6. User memory notes not verified in this pass: 15 pre-existing auth-core test failures; Windows cargo target failures; the `lint-ratchet` ENOBUFS failure; pre-existing verevonv3 typecheck, lint and vitest failures; remote-core symlink damage.
7. Migration appliers for the Model Plane Go services and the Control Plane services (not read).
8. Health route in code for audit-core, bridge-core, sandbox-manager (log line), quarry-control and quarry-orchestrator; whether the `/health` entry in `apps/Frontend Plane/verevonv3/vite.config.ts` is a proxy.
9. The gateway script `apps/Frontend Plane/verevonv3/apps/gateway/scripts/run-security-coverage.sh` is not referenced by any workflow; its purpose was not read.
10. Support Plane test commands, hbbs and hbbr health, and its compose structure (not read beyond existence).
11. `apps/Model Plane/deploy/docker-compose.override.yml` ports and environment (not read; only its existence and inclusion rule).
12. Whether the Data Plane `wiki-store` Dockerfile copies paths from the plane root (build context `.`).

---

## 11. Sources

Snapshot files: `.plan-work/live-tree-status.txt` and `.plan-work/live-tree-tracked-files.txt` (planning worktree). Live reads, all read only, under `C:/dev/CoresSystem`: the root `Makefile`, `.githooks/pre-commit`, `.gitleaks.toml`, the workflows under `.github/workflows/`, `.github/dependabot.yml`, the plane Makefiles and compose files named above, `go.mod`, `Cargo.toml`, `package.json` and `pnpm-workspace.yaml` files, the Dockerfiles, the scripts and tests named above, `apps/Model Plane/CLAUDE.md`, `apps/Model Plane/README.md`, `apps/Model Plane/docs/capability-ownership-matrix.md`, and `apps/Application Plane/docs/core-research/plane-audit-2026-07-11.md`. Git, read only: `ls-remote --heads origin`, `config` (both checkouts), `check-ignore`. GitHub, read only: `gh api` GET calls for repository metadata, branch protection and rulesets on `nisjety/CoreSystems`.

---

## 12. Path state index

Generated from every repo path cited above, with its state from the two snapshot files. Directories end with `/`. A directory marked MIXED contains both tracked and untracked entries; cite its files individually.

| Path | State |
|---|---|
| `.githooks/pre-commit` | MODIFIED-UNCOMMITTED |
| `.github/dependabot.yml` | COMMITTED |
| `.github/workflows/audit-core.yml` | MISSING |
| `.github/workflows/auth-service.yml` | COMMITTED |
| `.github/workflows/billing-core-service.yml` | COMMITTED |
| `.github/workflows/buf.yml` | COMMITTED |
| `.github/workflows/capability-core.yml` | MISSING |
| `.github/workflows/contract-goldens.yml` | COMMITTED |
| `.github/workflows/conversation-core-service.yml` | COMMITTED |
| `.github/workflows/convex-core.yml` | MISSING |
| `.github/workflows/dataplane-v2-ci.yml` | MODIFIED-UNCOMMITTED |
| `.github/workflows/dataplane-v2-eval-nightly.yml` | COMMITTED |
| `.github/workflows/eval-lab.yml` | COMMITTED |
| `.github/workflows/model-plane-rust-ci.yml` | MODIFIED-UNCOMMITTED |
| `.github/workflows/notification-core-service.yml` | COMMITTED |
| `.github/workflows/org-service.yml` | COMMITTED |
| `.github/workflows/quarry-docs-contract.yml` | UNCOMMITTED-NEW |
| `.github/workflows/quarry-v2-ci.yml` | COMMITTED |
| `.github/workflows/secret-scan.yml` | COMMITTED |
| `.github/workflows/session-core-service.yml` | COMMITTED |
| `.github/workflows/social-core-service.yml` | COMMITTED |
| `.github/workflows/sse-taxonomy.yml` | COMMITTED |
| `.github/workflows/user-service.yml` | COMMITTED |
| `.github/workflows/verevonv3-ci.yml` | MODIFIED-UNCOMMITTED |
| `.github/workflows/verevonv3-security.yml` | COMMITTED |
| `.gitleaks.toml` | MODIFIED-UNCOMMITTED |
| `apps/Application Plane/` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-core-go/` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-core-go/Dockerfile` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-core-go/go.mod` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-core-go/internal/http/server.go` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-ingest-rs/` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-ingest-rs/Cargo.toml` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-ingest-rs/Dockerfile` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-ingest-rs/src/lib.rs` | COMMITTED |
| `apps/Application Plane/conversation-core/conversation-ingest-rs/tests` | MISSING |
| `apps/Application Plane/convex-core/` | COMMITTED |
| `apps/Application Plane/convex-core/CONVEX_INTEGRATION.md` | COMMITTED |
| `apps/Application Plane/convex-core/convex.json` | COMMITTED |
| `apps/Application Plane/convex-core/convex/` | COMMITTED |
| `apps/Application Plane/convex-core/convex/_generated/` | COMMITTED |
| `apps/Application Plane/convex-core/convex/spaces.ts` | COMMITTED |
| `apps/Application Plane/convex-core/docker-compose.yml` | COMMITTED |
| `apps/Application Plane/convex-core/Dockerfile` | COMMITTED |
| `apps/Application Plane/convex-core/Makefile` | MISSING |
| `apps/Application Plane/convex-core/package.json` | COMMITTED |
| `apps/Application Plane/convex-core/startup.sh` | COMMITTED |
| `apps/Application Plane/convex-core/test/` | COMMITTED |
| `apps/Application Plane/convex-core/test/space-lifecycle.test.cjs` | COMMITTED |
| `apps/Application Plane/docker-compose.yml` | COMMITTED |
| `apps/Application Plane/docs/core-research/convex-core.md` | COMMITTED |
| `apps/Application Plane/docs/core-research/plane-audit-2026-07-11.md` | COMMITTED |
| `apps/Application Plane/Makefile` | MISSING |
| `apps/Application Plane/notification-core/` | COMMITTED |
| `apps/Application Plane/notification-core/Dockerfile` | COMMITTED |
| `apps/Application Plane/notification-core/go.mod` | COMMITTED |
| `apps/Application Plane/notification-core/internal/database/migrate_test.go` | COMMITTED |
| `apps/Application Plane/notification-core/internal/http/server.go` | COMMITTED |
| `apps/Application Plane/scripts/run-application-plane.sh` | COMMITTED |
| `apps/Application Plane/social-core/` | COMMITTED |
| `apps/Application Plane/social-core/go.mod` | COMMITTED |
| `apps/Application Plane/social-core/internal/http/server.go` | COMMITTED |
| `apps/Application Plane/social-core/internal/social/repository_postgres_test.go` | COMMITTED |
| `apps/Control Plane/audit-core/` | COMMITTED |
| `apps/Control Plane/audit-core/Dockerfile` | COMMITTED |
| `apps/Control Plane/audit-core/go.mod` | COMMITTED |
| `apps/Control Plane/audit-core/internal/testdata/` | COMMITTED |
| `apps/Control Plane/audit-core/Makefile` | MISSING |
| `apps/Control Plane/auth-core/Dockerfile` | COMMITTED |
| `apps/Control Plane/auth-core/package.json` | COMMITTED |
| `apps/Control Plane/auth-core/pnpm-lock.yaml` | COMMITTED |
| `apps/Control Plane/auth-core/scripts/lint-ratchet.mjs` | COMMITTED |
| `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` | MODIFIED-UNCOMMITTED |
| `apps/Control Plane/billing-core/` | COMMITTED |
| `apps/Control Plane/billing-core/Dockerfile` | COMMITTED |
| `apps/Control Plane/billing-core/go.mod` | COMMITTED |
| `apps/Control Plane/billing-core/internal/http/server.go` | COMMITTED |
| `apps/Control Plane/billing-core/internal/http/service_auth.go` | COMMITTED |
| `apps/Control Plane/config/plane-service-principals.json` | COMMITTED |
| `apps/Control Plane/docker-compose.yml` | COMMITTED |
| `apps/Control Plane/Makefile` | MISSING |
| `apps/Control Plane/org-core/` | COMMITTED |
| `apps/Control Plane/org-core/Dockerfile` | COMMITTED |
| `apps/Control Plane/org-core/go.mod` | COMMITTED |
| `apps/Control Plane/org-core/internal/http/server.go` | COMMITTED |
| `apps/Control Plane/scripts/plane-principal-registry.jq` | COMMITTED |
| `apps/Control Plane/scripts/run-control-plane.sh` | COMMITTED |
| `apps/Control Plane/session-core/` | COMMITTED |
| `apps/Control Plane/session-core/Dockerfile` | COMMITTED |
| `apps/Control Plane/session-core/go.mod` | COMMITTED |
| `apps/Control Plane/session-core/internal/http/server.go` | COMMITTED |
| `apps/Control Plane/user-core/` | COMMITTED |
| `apps/Control Plane/user-core/cmd/dual/main.go` | MISSING |
| `apps/Control Plane/user-core/cmd/server/main.go` | COMMITTED |
| `apps/Control Plane/user-core/Dockerfile` | COMMITTED |
| `apps/Control Plane/user-core/go.mod` | COMMITTED |
| `apps/Control Plane/user-core/internal/http/server.go` | COMMITTED |
| `apps/Control Plane/user-core/internal/spaces/` | COMMITTED |
| `apps/Control Plane/user-core/internal/spaces/personal_thread_decision.go` | COMMITTED |
| `apps/Control Plane/user-core/Makefile` | COMMITTED |
| `apps/Control Plane/user-core/proto/user/v1/` | COMMITTED |
| `apps/Data Plane v2/.env.example` | MODIFIED-UNCOMMITTED |
| `apps/Data Plane v2/docker-compose.yml` | MODIFIED-UNCOMMITTED |
| `apps/Data Plane v2/infra/prometheus/prometheus.yml` | COMMITTED |
| `apps/Data Plane v2/Makefile` | COMMITTED |
| `apps/Data Plane v2/scripts/auth-regression-matrix.sh` | MODIFIED-UNCOMMITTED |
| `apps/Data Plane v2/scripts/check-eval-contract.sh` | COMMITTED |
| `apps/Data Plane v2/scripts/check-parity-contract.sh` | COMMITTED |
| `apps/Data Plane v2/scripts/check-subjects.sh` | COMMITTED |
| `apps/Data Plane v2/scripts/check-tenant-isolation.sh` | COMMITTED |
| `apps/Data Plane v2/scripts/check-usage-contract.sh` | MODIFIED-UNCOMMITTED |
| `apps/Data Plane v2/scripts/eval-ingest-corpus.py` | COMMITTED |
| `apps/Data Plane v2/scripts/gen-clients.sh` | COMMITTED |
| `apps/Data Plane v2/scripts/qb-preflight.sh` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/scripts/smoke-quickwit-retrieval.sh` | COMMITTED |
| `apps/Data Plane v2/scripts/smoke-test.sh` | COMMITTED |
| `apps/Data Plane v2/services/data-orchestrator-go/` | COMMITTED |
| `apps/Data Plane v2/services/data-orchestrator-go/Dockerfile` | COMMITTED |
| `apps/Data Plane v2/services/data-quality-go/` | COMMITTED |
| `apps/Data Plane v2/services/data-quality-go/Dockerfile` | COMMITTED |
| `apps/Data Plane v2/services/documents-api-go/` | COMMITTED |
| `apps/Data Plane v2/services/documents-api-go/Dockerfile` | COMMITTED |
| `apps/Data Plane v2/services/documents-api-go/go.mod` | COMMITTED |
| `apps/Data Plane v2/services/retrieval-engine-rs/Cargo.toml` | MODIFIED-UNCOMMITTED |
| `apps/Data Plane v2/services/retrieval-engine-rs/Dockerfile` | COMMITTED |
| `apps/Data Plane v2/services/retrieval-engine-rs/src/egress_attestation.rs` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/services/retrieval-engine-rs/tests/chunk_context_join.rs` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/services/retrieval-engine-rs/tests/grpc_integration.rs` | MODIFIED-UNCOMMITTED |
| `apps/Data Plane v2/services/retrieval-engine-rs/tests/rerank_contract.rs` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/services/retrieval-engine-rs/tests/zdr_dense_egress.rs` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/services/wiki-store-go/` | COMMITTED |
| `apps/Data Plane v2/services/wiki-store-go/Dockerfile` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/auth-regression-matrix-test.sh` | MODIFIED-UNCOMMITTED |
| `apps/Data Plane v2/tests/scripts/background-budget-contract-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/compose-security-contract-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/contextual-retrieval-config-contract-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/eval-harness-contract-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/graph-index-no-azure-contract-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/image-provenance-contract-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/isolated-registry-parity-test.sh` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/tests/scripts/load-auth-contract-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/rerank-input-contract-test.sh` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/tests/scripts/run-postgres-contract-tests.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/smoke-test-test.sh` | COMMITTED |
| `apps/Data Plane v2/tests/scripts/web-rerank-compose-contract-test.sh` | UNCOMMITTED-NEW |
| `apps/Data Plane v2/tools/migrator/` | COMMITTED |
| `apps/Data Plane v2/tools/migrator/Dockerfile` | COMMITTED |
| `apps/Data Plane v2/tools/migrator/go.mod` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/Cargo.lock` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/Dockerfile` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/scripts/` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/scripts/run-security-coverage.sh` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/spaces.rs` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/src/main.rs` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/src/middleware.rs` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/gateway/tests/` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/remote-dev/` | COMMITTED |
| `apps/Frontend Plane/verevonv3/apps/verevon-web/public/product-demo/` | UNCOMMITTED-NEW |
| `apps/Frontend Plane/verevonv3/docker-compose.yml` | COMMITTED |
| `apps/Frontend Plane/verevonv3/Dockerfile` | COMMITTED |
| `apps/Frontend Plane/verevonv3/Makefile` | MISSING |
| `apps/Frontend Plane/verevonv3/package.json` | MODIFIED-UNCOMMITTED |
| `apps/Frontend Plane/verevonv3/packages/remote-core/` | COMMITTED |
| `apps/Frontend Plane/verevonv3/packages/remote-core/package.json` | COMMITTED |
| `apps/Frontend Plane/verevonv3/playwright.config.ts` | COMMITTED |
| `apps/Frontend Plane/verevonv3/playwright.real-authority.config.ts` | COMMITTED |
| `apps/Frontend Plane/verevonv3/pnpm-lock.yaml` | COMMITTED |
| `apps/Frontend Plane/verevonv3/pnpm-workspace.yaml` | COMMITTED |
| `apps/Frontend Plane/verevonv3/scripts/build-product-demo.mjs` | UNCOMMITTED-NEW |
| `apps/Frontend Plane/verevonv3/scripts/probe-product-demo.mjs` | UNCOMMITTED-NEW |
| `apps/Frontend Plane/verevonv3/src/features/spaces/components/SpaceRoomComposer.tsx` | MODIFIED-UNCOMMITTED |
| `apps/Frontend Plane/verevonv3/src/shared/api/chat-client.ts` | MODIFIED-UNCOMMITTED |
| `apps/Frontend Plane/verevonv3/src/shared/api/http.ts` | MODIFIED-UNCOMMITTED |
| `apps/Frontend Plane/verevonv3/src/shared/api/sse.ts` | MODIFIED-UNCOMMITTED |
| `apps/Frontend Plane/verevonv3/src/shared/api/zdr-refusals.ts` | UNCOMMITTED-NEW |
| `apps/Frontend Plane/verevonv3/src/test/setup.ts` | COMMITTED |
| `apps/Frontend Plane/verevonv3/tests/e2e/` | COMMITTED |
| `apps/Frontend Plane/verevonv3/vite.config.ts` | COMMITTED |
| `apps/Frontend Plane/verevonv3/vite.demo.config.ts` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/docker-compose.yml` | MODIFIED-UNCOMMITTED |
| `apps/Ingestion Plane/imports-core/` | COMMITTED |
| `apps/Ingestion Plane/imports-core/Dockerfile` | COMMITTED |
| `apps/Ingestion Plane/imports-core/migrations/001_init.sql` | COMMITTED |
| `apps/Ingestion Plane/imports-core/requirements.txt` | COMMITTED |
| `apps/Ingestion Plane/imports-core/tests/` | COMMITTED |
| `apps/Ingestion Plane/integration-corev2/cmd/email-worker/` | COMMITTED |
| `apps/Ingestion Plane/integration-corev2/cmd/finspo-worker/` | COMMITTED |
| `apps/Ingestion Plane/integration-corev2/Dockerfile` | COMMITTED |
| `apps/Ingestion Plane/integration-corev2/go.mod` | COMMITTED |
| `apps/Ingestion Plane/integration-corev2/internal/api/zdr_acknowledgement.go` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/integration-corev2/Makefile` | MISSING |
| `apps/Ingestion Plane/integration-corev2/services/webhook-normalizer-rs/` | COMMITTED |
| `apps/Ingestion Plane/Makefile` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/Cargo.toml` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/Cargo.toml` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` | MODIFIED-UNCOMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/model_plane_gate.rs` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/rerank_behavior.rs` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/build.rs` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/Cargo.toml` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json` | MODIFIED-UNCOMMITTED |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/data_plane_http.rs` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_rerank_contract.rs` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/dp_retrieve_contract.rs` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/tests/fixtures/` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/deploy/compose/Dockerfile.control` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/deploy/compose/Dockerfile.edge` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/deploy/compose/Dockerfile.orchestrator` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml` | MODIFIED-UNCOMMITTED |
| `apps/Ingestion Plane/Quarry-v2/docs/WEB_RERANK_GATE_RUNBOOK.md` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/lab/evals/web-rerank-baseline.json` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/Makefile` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs` | UNCOMMITTED-NEW |
| `apps/Ingestion Plane/Quarry-v2/sdks/` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/sdks/generate.sh` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-control/` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-control/go.mod` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/go.mod` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/activities/` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/controlauth/` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator/internal/jobs/` | COMMITTED |
| `apps/Ingestion Plane/Quarry-v2/services/quarry-vision-sidecar/` | COMMITTED |
| `apps/Model Plane/api/contracts/zdr-refusals.json` | UNCOMMITTED-NEW |
| `apps/Model Plane/CLAUDE.md` | COMMITTED |
| `apps/Model Plane/deploy/docker-compose.override.yml` | COMMITTED |
| `apps/Model Plane/deploy/docker-compose.yml` | COMMITTED |
| `apps/Model Plane/docs/capability-ownership-matrix.md` | COMMITTED |
| `apps/Model Plane/docs/sse-event-taxonomy.md` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/go/` | COMMITTED |
| `apps/Model Plane/go/gen/` | COMMITTED |
| `apps/Model Plane/go/go.work` | COMMITTED |
| `apps/Model Plane/go/Makefile` | MISSING |
| `apps/Model Plane/go/pkg/authctx/` | COMMITTED |
| `apps/Model Plane/go/services/bridge-core/` | COMMITTED |
| `apps/Model Plane/go/services/bridge-core/go.mod` | COMMITTED |
| `apps/Model Plane/go/services/browser-broker/` | COMMITTED |
| `apps/Model Plane/go/services/browser-broker/cmd/main.go` | COMMITTED |
| `apps/Model Plane/go/services/capability-core/` | COMMITTED |
| `apps/Model Plane/go/services/capability-core/cmd/main.go` | COMMITTED |
| `apps/Model Plane/go/services/capability-core/Dockerfile` | COMMITTED |
| `apps/Model Plane/go/services/capability-core/go.mod` | COMMITTED |
| `apps/Model Plane/go/services/capability-core/migrations/` | COMMITTED |
| `apps/Model Plane/go/services/cost-core/` | COMMITTED |
| `apps/Model Plane/go/services/cost-core/cmd/main.go` | COMMITTED |
| `apps/Model Plane/go/services/cost-core/internal/server/server.go` | COMMITTED |
| `apps/Model Plane/go/services/cost-core/migrations/` | COMMITTED |
| `apps/Model Plane/go/services/letta-bridge/` | COMMITTED |
| `apps/Model Plane/go/services/letta-bridge/cmd/main.go` | COMMITTED |
| `apps/Model Plane/go/services/nats-provisioner/` | COMMITTED |
| `apps/Model Plane/go/services/nats-provisioner/go.mod` | COMMITTED |
| `apps/Model Plane/go/services/orchestrator-core/` | COMMITTED |
| `apps/Model Plane/go/services/orchestrator-core/cmd/` | COMMITTED |
| `apps/Model Plane/go/services/orchestrator-core/cmd/activities/activities.go` | COMMITTED |
| `apps/Model Plane/go/services/orchestrator-core/cmd/main.go` | COMMITTED |
| `apps/Model Plane/go/services/sandbox-manager/` | COMMITTED |
| `apps/Model Plane/go/services/sandbox-manager/cmd/main.go` | COMMITTED |
| `apps/Model Plane/go/services/sandbox-manager/Dockerfile` | COMMITTED |
| `apps/Model Plane/Makefile` | MISSING |
| `apps/Model Plane/proto/baseline.binpb` | COMMITTED |
| `apps/Model Plane/proto/buf.gen.go-only.yaml` | COMMITTED |
| `apps/Model Plane/proto/buf.gen.yaml` | COMMITTED |
| `apps/Model Plane/proto/buf.yaml` | COMMITTED |
| `apps/Model Plane/proto/model_plane/v1/sessions.proto` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/python/eval-lab-py/Makefile` | COMMITTED |
| `apps/Model Plane/README.md` | COMMITTED |
| `apps/Model Plane/rust/Cargo.toml` | COMMITTED |
| `apps/Model Plane/rust/crates/mp-contracts/build.rs` | COMMITTED |
| `apps/Model Plane/rust/crates/mp-contracts/Cargo.toml` | COMMITTED |
| `apps/Model Plane/rust/crates/mp-contracts/src/lib.rs` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/rust/crates/mp-contracts/src/retention.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/crates/mp-contracts/src/zdr_refusal.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/crates/mp-contracts/tests/zdr_retention_matrix.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/execution-core/Dockerfile` | COMMITTED |
| `apps/Model Plane/rust/services/execution-core/src/http_health.rs` | COMMITTED |
| `apps/Model Plane/rust/services/execution-core/tests/zdr_refusal_inventory.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/inference-core/Dockerfile` | COMMITTED |
| `apps/Model Plane/rust/services/inference-core/src/grpc/zdr_waiver_route_tests.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/inference-core/src/http_health.rs` | COMMITTED |
| `apps/Model Plane/rust/services/inference-core/tests/zdr_refusal_inventory.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/calibration/` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/Dockerfile` | COMMITTED |
| `apps/Model Plane/rust/services/model-gateway/src/http_routes.rs` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/rust/services/model-gateway/src/relevance_calibration.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/rust/services/model-gateway/src/zdr_features.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/src/zdr_posture.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json.sha256` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/tests/orchestration_http_test.rs` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/rust/services/model-gateway/tests/zdr_feature_table.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/model-gateway/tests/zdr_refusal_inventory.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/session-core/Dockerfile` | COMMITTED |
| `apps/Model Plane/rust/services/session-core/migrations/0044_run_retention_receipt.sql` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/session-core/migrations/0045_zdr_waived_runs_org_index.sql` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/session-core/migrations/revert/` | UNCOMMITTED-NEW |
| `apps/Model Plane/rust/services/session-core/src/grpc.rs` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/rust/services/session-core/src/http_health.rs` | COMMITTED |
| `apps/Model Plane/rust/services/session-core/src/main.rs` | MODIFIED-UNCOMMITTED |
| `apps/Model Plane/rust/services/session-core/src/retention_receipt.rs` | UNCOMMITTED-NEW |
| `apps/Model Plane/scripts/compose.sh` | COMMITTED |
| `apps/Model Plane/scripts/gen-sse-event-taxonomy.py` | COMMITTED |
| `apps/Model Plane/scripts/release-artifact.sh` | COMMITTED |
| `apps/Support Plane/docker-compose.yml` | COMMITTED |
| `apps/Support Plane/Makefile` | MISSING |
| `apps/Support Plane/README.md` | COMMITTED |
| `CLAUDE.md` | COMMITTED |
| `Makefile` | COMMITTED |
| `scripts/bootstrap_runtime_environment.sh` | COMMITTED |
| `scripts/tests/principal-registry-parity.test.sh` | COMMITTED |
| `scripts/tests/quarry-model-plane-switches.test.mjs` | UNCOMMITTED-NEW |
| `scripts/tests/zdr-contract-schema.mjs` | MODIFIED-UNCOMMITTED |
| `scripts/tests/zdr-contract-schema.test.mjs` | COMMITTED |
| `scripts/tests/zdr-contract-sync.test.sh` | MODIFIED-UNCOMMITTED |

Counts over the 319 paths above: MODIFIED-UNCOMMITTED 33; COMMITTED 227; MISSING 14; UNCOMMITTED-NEW 45.
