# Build, test, CI, deploy and evidence inventory (Model Plane + chat build-plan planes)

Date: 2026-10-14. Planes: Model Plane, Control Plane, Application Plane, Frontend Plane verevonv3, Ingestion Plane (Quarry-v2, integration-corev2), CI workflows, `.githooks/pre-commit`, repo evidence rule.

## 0. Conventions and scope

- HEAD = worktree `epic-shaw-4d613b` at afb83928 (clean). LIVE = `C:/dev/CoresSystem` (same HEAD plus the uncommitted/untracked set in `.plan-work/live-tree-status.txt`).
- Paths are relative to the worktree root `C:/dev/CoresSystem/.claude/worktrees/epic-shaw-4d613b/` unless absolute. `file:line` = a line opened in this pass.
- Tags: **[V]** verified at HEAD by reading the file. **[L]** exists or differs only in LIVE (uncommitted/untracked). **[S]** memory hint stale or contradicted by repo source. **[U]** not verifiable from the repo, host-specific, runtime-only, or not executed.
- No build, test, Docker or deploy command was run. Running them would write `target/`, `node_modules/` or images into the tree and touch Docker. Every command below is quoted from a Makefile, package.json, workflow, script or doc, and is unexecuted.
- Evidence pack: only `audit-evidence/README.md` was read. The audit document `03-model-plane-chat-and-agents-audit.md` was not used for this inventory.
- Unprefixed `CLAUDE.md:NN` in section 1 means `apps/Model Plane/CLAUDE.md`. The root `CLAUDE.md` is always cited as "root CLAUDE.md:NN".

## 1. Model Plane (`apps/Model Plane`)

Model Plane has no top-level Makefile or npm task runner. Commands come from `CLAUDE.md`, `scripts/compose.sh`, `scripts/release-artifact.sh`, the root `Makefile` and CI. The only Makefile in the plane is `python/eval-lab-py/Makefile` [V].

### 1.1 Rust workspace (`rust/`)

- Workspace: `rust/Cargo.toml:1-19` (resolver 2; members: crates `mp-contracts, mp-ids, mp-events, mp-telemetry, mp-slo, mp-orchestration, mp-eventlog, mp-toon, mp-tokens, mp-authctx`; services `model-gateway, session-core, inference-core, execution-core`; client `clients/bridge-cli`). Lints: clippy pedantic = warn (`:26-27`), `unsafe_code = forbid` (`:31-32`).
- `-p` package names (`[package] name`, line 2 of each Cargo.toml): `model-gateway`, `session-core`, `inference-core` (lib target `inference_core`, `services/inference-core/Cargo.toml:54`), `execution-core`, `bridge-cli`; crates use their directory name [V].

| Purpose | Command | Source |
|---|---|---|
| Per service, all targets | `cargo test -q -p <service> --all-targets` | `apps/Model Plane/CLAUDE.md:30` [V] |
| Workspace | `cargo test --workspace` (CI: `cargo test --workspace --no-fail-fast`) | `CLAUDE.md:30,39`; `.github/workflows/model-plane-rust-ci.yml:119` [V] |
| CI format | `cargo fmt --all -- --check` | `model-plane-rust-ci.yml:99` [V] |
| CI compile, prod code | `cargo check --workspace` | `model-plane-rust-ci.yml:105` [V] |
| CI compile, all targets | `cargo check --workspace --all-targets` | `model-plane-rust-ci.yml:112` [V] |
| Coverage | `cargo llvm-cov` (risk-based 80% target; not in CI) | `CLAUDE.md:32` [V] |
| Clippy | not in CI; comment says `cargo clippy --workspace --all-targets` exits 0 with ~304 pedantic warnings | `model-plane-rust-ci.yml:122-129` [V] |
| Lib-only per crate | `cargo test -p <crate> --lib` | memory `model-plane-build-and-deploy.md:19` [U] (see 8) |
| Ignored live test | execution-core `browser_agent_e2e` is `#[ignore]` and stays skipped | `model-plane-rust-ci.yml:114-117` [V] |
| Session-core receipts (Postgres, ignored tests) | `cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1` with `DATABASE_URL`, `REQUIRE_CONTRACT=1`; job fails if fewer than 8 pass | LIVE-only job in `model-plane-rust-ci.yml` diff [L] |

Notes: `cargo test --workspace` also runs `services/execution-core/tests/cross_service_loop_contract.rs` (file exists [V]; `CLAUDE.md:23` names it as the pattern for cross-service invariants [V]). The CI job name says "Format + Check + Lib Tests" (`model-plane-rust-ci.yml:70`) but it runs all targets.

### 1.2 Go services (`go/`)

- `go/go.work:1` `go 1.25.1`; `use (...)` at lines 3-23 lists 19 modules: `./gen`; `./pkg/{authctx,cibuf,envelope,idempotency,jetstream,natsx,publisher,quarry,servicetoken,temporalreg}`; `./services/{bridge-core,browser-broker,capability-core,cost-core,letta-bridge,nats-provisioner,orchestrator-core,sandbox-manager}` [V]. `go/go.work.sum` exists [V]. The `use` list has no `.` entry.
- Modules with `go.mod`: the 19 above [V, Glob]. Services that are Docker artifacts: see 1.4.

| Purpose | Command | Source / status |
|---|---|---|
| Per service | `go test ./...` and `go vet ./...` inside each service directory | `CLAUDE.md:31` [V] |
| Root build as written | `cd go && go build ./... && go vet ./...` | `CLAUDE.md:40` [V text]; with no `.` in `use` (`go.work:3-23`) this is expected to fail [U, not run] |
| Scoped build from go root | `go build ./services/<name>/...` | memory `model-plane-build-and-deploy.md:28` [U] (consistent with go.work) |
| New dependency | `GOWORK=off go get` / `go mod tidy` in the consuming service | memory `model-plane-build-and-deploy.md:28` [U] (no repo doc checked) |
| Race detector | `go test -race` needs cgo | memory `model-plane-build-and-deploy.md:26` [U] (host `CGO_ENABLED=0`) |
| Go bindings regen | `buf generate` (writes `../go/gen`) | `proto/buf.gen.go-only.yaml:9,13`, `proto/buf.gen.yaml:14,18` [V]; buf not installed on host [U] |
| CI for these modules | none. No workflow references `apps/Model Plane/go` [V, grep of `.github/workflows`]. The Go workflows that exist run `go test` and `go build` only, no `go vet` or lint (`user-service.yml:67,70`, etc.) [V] |

### 1.3 Compose and deploy

- Entry script: `scripts/compose.sh` (`ROOT_DIR` = plane root, `:4`).
  - Artifact mode if `artifact-metadata.env` exists (`:24-27`). Then `MODEL_PLANE_PRODUCTION=1` is required and only `config --quiet` and `up -d --wait --no-build` are accepted (`:29-53`), after `release-artifact.sh verify` (`:57`).
  - Dev mode: requires `deploy/.env` (`:96-100`). Exports `SOURCE_REVISION` (git HEAD) and `BUILD_DATE` (UTC now) (`:102-103`). Base file `deploy/docker-compose.yml` (`:105-107`).
  - `MODEL_PLANE_PRODUCTION=1` in dev mode is refused: "production Compose must use scripts/release-artifact.sh compose ..." (`:114-116`).
  - `deploy/docker-compose.override.yml` added unless production or `MODEL_PLANE_DEV=0` (`:118-120`). `MODEL_PLANE_BRIDGES=1` adds `deploy/docker-compose.bridges.yml` (`:122-124`). `MODEL_PLANE_RELEASE_LOCK` adds `deploy/docker-compose.release.yml` (`:146-157`).
  - Env files: `../Control Plane/.env.generated-secrets` first if readable (`:141-144`), then `deploy/.env` (`:145`).
  - On build verbs (`build` or `--build`, not `--no-build`) it runs `make -C <repo root> warm-bases`, non-fatal (`:174-197`).
  - Final call: `exec docker compose <env files> <compose files> "$@"` (`:199`).
- Deploy files present [V]: `deploy/docker-compose.yml`, `.override.yml`, `.production.yml`, `.release.yml`, `.bridges.yml`, `otel-collector-config.yaml`, `nats.conf`, `seccomp-bwrap.json`, `.env.example`.
- Compose service keys in `deploy/docker-compose.yml` [V]: `logging, postgres, nats, nats-provisioner, minio, dragonfly, temporal, temporal-postgres, temporal-ui, otel-collector, model-gateway (:191), session-core (:712), inference-core (:808), execution-core (:1005), orchestrator-core (:1281), capability-core (:1378), sandbox-manager (:1575), browser-broker (:1612), letta-bridge (:1630), agent-memory-redis (:1669), agent-memory-server (:1680), cost-core (:1739), bridge-core (:1816)`; networks `default`, `inter-plane-bus`.

| Purpose | Command | Source / status |
|---|---|---|
| Dev bring-up | `./scripts/compose.sh up -d` (from `apps/Model Plane`) | `CLAUDE.md:37-38` [V] |
| Rebuild one service | `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build <service>` | memory `model-plane-build-and-deploy.md:14` [U] (pass-through is `compose.sh:199` [V]) |
| Bases check, read-only | `make check-bases` (repo root) | `Makefile:44-58` [V] |
| Bases warm | `make warm-bases` (repo root) | `Makefile:60` [V]; called from `compose.sh:195` [V] |
| Production | `scripts/release-artifact.sh compose <artifact> config` or `deploy <artifact>` | `compose.sh:114-116`; `release-artifact.sh:1889-1896` [V] |

### 1.4 Release artifact script (`scripts/release-artifact.sh`)

- `ARTIFACT_FORMAT_VERSION="3"` (`:7`). `SERVICES` (`:9-12`): `model-gateway session-core inference-core execution-core orchestrator-core capability-core sandbox-manager browser-broker letta-bridge cost-core bridge-core`. `nats-provisioner` is a compose service but is NOT in the artifact list [V].
- Infrastructure images (`:30-44`): postgres, nats, minio, dragonfly, temporal, temporal-ui, otel-collector, agent-memory-redis, agent-memory-server.
- Subcommands (dispatch `:1876-1912`; usage `:110-158`):

| Subcommand | Form | Dispatch line |
|---|---|---|
| dry-run / build | `dry-run [dir]`, `build [dir]` | `:1877-1879` |
| verify | `verify <dir>` | `:1880-1884` |
| restore | `restore <dir>` (images only) | `:1885-1888` |
| compose | `compose <dir> config` (only `config` accepted) | `:1889-1892` |
| deploy | `deploy <dir>` | `:1893-1896` |
| validate-deployability | `validate-deployability <dir>` | `:1897-1900` |
| validate-lock | `validate-lock <images.lock.env>` | `:1901-1904` |
| validate-runtime-config | `<runtime.env> <config-policy.tsv> <runtime-public-policy.env> <compatibility-gates.env>` | `:1905-1908` |
| snapshot-runtime-config | same four plus `<destination>` | `:1909-1912` |

- Build/verify gates (usage `:131-141`): `MODEL_PLANE_RELEASE_MODE=1`, `MODEL_PLANE_ARTIFACT_SIGNING_KEY` (build), `MODEL_PLANE_ARTIFACT_VERIFY_KEY` (verify/deploy), `MODEL_PLANE_ROLLBACK_ARTIFACT_DIR` (build/verify/deploy), `MODEL_PLANE_ROLLBACK_RUNTIME_ENV_FILE`, `MODEL_PLANE_ROLLBACK_RUNTIME_EVIDENCE_FILE` (build), `MODEL_PLANE_COMPATIBILITY_GATES_FILE` (build), `MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE` (build), `MODEL_PLANE_RUNTIME_ENV_FILE` (compose/deploy).
- Not referenced by any workflow [V, grep]. Not executed [U].

### 1.5 Other Model Plane gates (HEAD)

| Gate | Command / check | Source |
|---|---|---|
| Proto lint + breaking | `buf lint apps/Model Plane/proto`; `buf breaking apps/Model Plane/proto --against apps/Model Plane/proto/baseline.binpb` | `.github/workflows/buf.yml:21,23` [V]; `proto/baseline.binpb` exists [V]; `proto/buf.yaml` v2 [V] |
| SSE taxonomy freshness | `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (no deps); drift-proof removes one documented row and expects failure | `.github/workflows/sse-taxonomy.yml:45`, `:49-62` [V]; triggers `:16-30` include `sse_events.rs`, `docs/sse-event-taxonomy.md`, `verevonv3/src/shared/api/chat-client.ts` [V] |
| Contract goldens / ZDR | `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`; `node --test scripts/tests/zdr-contract-schema.test.mjs` | `contract-goldens.yml:66,71,74` [V]; also `model-plane-rust-ci.yml:95-96` [V] |
| ZDR posture golden (HEAD) | `apps/Model Plane/api/contracts/zdr-posture-matrix.json` (+ `.sha256`) | Glob [V]; referenced by `scripts/tests/zdr-contract-sync.test.sh` [V, grep] |
| ZDR posture Rust matrix | `rust/services/model-gateway/tests/zdr_posture_matrix.rs` and fixtures | LIVE-only, untracked [L] |
| Eval lab (Python) | `make venv`, `make selftest` (`pytest tests/test_runner.py -q`), `make eval` (`EVAL_LIVE=1 pytest tests/test_live_suite.py -q -rs`) | `python/eval-lab-py/Makefile:11-21` [V]; CI `eval-lab.yml:56` (`pip install -e ".[dev]"`), `:64` (`pytest tests/ -q --junit-xml=...`), `:73` (asserts at least 10 offline cases ran) [V] |
| Cross-service invariant test | `services/execution-core/tests/cross_service_loop_contract.rs` (run by `cargo test --workspace`) | file [V]; CLAUDE.md:23 [V] |
| Size or i18n gate | none in workflows or package.json [V, grep]. Frontend i18n tests run only under `vitest` (see 4) | |

Other Model Plane scripts (`verify-durable-layer.sh`, `verify-sandbox-isolation.sh`, `eval-evidence.sh`, `smoke-*.js`, `seed_tool_knowledge.*`, `calibration_report.py`): exist [V, Glob]; no workflow references them [V, grep]. `scripts/tests/*proof*.sh` (for example `capability-health-proof.sh`, `coresystem-qm-proof.sh`, `tickets-create-proof.sh`) are under `apps/Model Plane/scripts/tests/` [V, Glob]; cited as evidence runners in `docs/MODEL_PLANE_RELEASE_EVIDENCE.md:32-44` [V]; not in CI [V].

## 2. Control Plane (`apps/Control Plane`)

- Dev runner: `scripts/run-control-plane.sh`. Header `:4-11`: local-development runner; generated credentials in a gitignored `.env.generated-secrets`; "must not be used with the production overlay". Refuses `docker-compose.production.yml` (`:16-19`, exit 2). Loads env files `auth-core`, `user-core`, `org-core`, `audit-core`, `billing-core`, `session-core` (`.env` / `.env.docker`) (`:21-33`); errors when one is missing (`:34-36`). Final call `exec docker compose ... -f docker-compose.yml "$@"` (`:513`) [V].
- Compose files present [V]: `docker-compose.yml`, `docker-compose.production.yml`, `docker-compose.legacy-bridge.yml`, `docker-compose.recovery.override.yml`.
- `docker-compose.yml`: `user-service` container name at `:381`, port `3012` at `:450`, health `http://127.0.0.1:3012/health` at `:491` [V]. Other planes address it as `http://user-core:3012` (`apps/Ingestion Plane/docker-compose.yml:435`; `apps/Application Plane/convex-core/docker-compose.yml:268`) [V]; the DNS alias source was not determined [U].

| Service | Test | Build / run | CI |
|---|---|---|---|
| auth-core (NestJS, pnpm) | `pnpm test` = `jest` (`package.json:17`); `pnpm run test:cov` (`:19`); `pnpm run lint:contract` = `node --test scripts/lint-ratchet.test.mjs` (`:16`) | `pnpm run build` = `nest build` (`:9`); `pnpm run start:dev` (`:12`) | `auth-service.yml:60` install, `:63` lint, `:66` build (labelled type check), `:74` test, `:81` `bash scripts/tests/principal-registry-parity.test.sh` (repo root) [V] |
| auth-core e2e / smoke | `pnpm run test:e2e` / `test:smoke` = jest on `apps/auth/test/http-smoke.e2e-spec.ts` (`:21,23`) | | not in CI [V] |
| auth-core lint | `pnpm run lint` = `node scripts/lint-ratchet.mjs` (`:15`) | | the only "ratchet" in the repo; not a size ratchet [V] |
| user-core (Go) | `make test` = `go test -v ./...` (`Makefile:44-46`); `make test-coverage` (`:48-51`) | `make build` (`:19-22`); `make docker-build` = `docker build -t coresystem-user-service:latest .` (`:99-101`) | `user-service.yml:67` (`go test -v ./...`), `:70` (`go build -v ./cmd/server`); job `build` `:72`; deploy jobs `:110` staging, `:120` production [V] |
| org-core (Go) | `go test -v -p 1 ./...` (`org-service.yml:103`); Postgres bootstrap step (`:74-93`) | `go build -v ./cmd/server` (`:106`) | `org-service.yml`, working dir `apps/Control Plane/org-core` (`:36`); deploy jobs `:146`, `:156` [V]; no Makefile [V] |
| billing-core (Go) | `go test -v -p 1 ./...` (`billing-core-service.yml:84`) | build `:87` | working dir `:32` [V] |
| session-core (Go, Control Plane) | `go test -v ./...` (`session-core-service.yml:43`) | build `:46` | working dir `apps/Control Plane/session-core` (`:29`) [V] |
| audit-core (Go) | no source-defined command (no Makefile; `go.mod` present) | | no workflow of its own; only path-filter mentions in `model-plane-rust-ci.yml:36,49` [V] |

Memory hint verification for Control Plane is in 8.

## 3. Application Plane (`apps/Application Plane`)

No Makefile in the plane [V, Glob]. Go modules with `go.mod` [V]: `conversation-core/conversation-core-go`, `information-core`, `insight-core`, `leads-core`, `notification-core`, `social-core`. Node: `convex-core/package.json`, `zammad-foundation/bootstrap/package.json`.

| Service | Test | Build | CI |
|---|---|---|---|
| notification-core | `go test -v ./...` (`notification-core-service.yml:42`) | `go build -v ./cmd/server` (`:45`) | working dir `:28`; triggers `:13-20` [V] |
| conversation-core-go | `go test -v ./...` (`conversation-core-service.yml:41`) | `go build -v ./cmd/server` (`:44`) | working dir `apps/Application Plane/conversation-core/conversation-core-go` (`:27`) [V] |
| social-core | `go test -v -p 1 ./...` (`social-core-service.yml:65`) | `go build -v ./cmd/server` (`:68`) | working dir `:31` [V] |
| information-core, insight-core, leads-core, zammad-foundation | none | none | no workflow references them [V, grep] |
| convex-core (Convex, npm) | `npm run test` = `tsx --test test/*.test.cjs` (`package.json:12`; 13 test files [V]); `typecheck` = `tsc --noEmit` (`:9`); `lint` = `eslint "convex/**/*.ts"` (`:10`) | `dev` = `convex dev` (`:7`); `generate` = `convex codegen` (`:11`); `deploy` = `convex deploy --prod` (`:8`) | no workflow references convex-core [V, grep] |

- Convex bring-up: `convex-core/startup.sh` runs `npx convex env set` (`:87`), `npx convex deploy --yes` (`:127`), then `npx convex dev` (`:132`) [V]. Compose: `convex-gateway` service at `docker-compose.yml:117` with `container_name: convex-gateway` at `:122`, and again at `convex-core/docker-compose.yml:109,113` [V]. Which file the memory means is not determined.
- Root: `cd "apps/Application Plane" && docker compose up -d --build` (root CLAUDE.md:96) [V].

## 4. Frontend Plane verevonv3 (`apps/Frontend Plane/verevonv3`)

### 4.1 package.json (HEAD)

- `dev` = `vite` (`package.json:8`); `build` = `pnpm typecheck && vite build` (`:9`); `lint` = `eslint .` (`:11`); `test` = `node --test apps/gateway/scripts/{security-coverage-gate,action-surface-contract,product-truth-contract,zdr-seed-test-org}.test.mjs && vitest run` (`:13`); `typecheck` = `tsc -b` (`:17`); `verify` = `pnpm lint && pnpm typecheck && pnpm test && pnpm build` (`:18`).
- E2E: `test:e2e` = `playwright test` (`:15`); `test:e2e:install` = `playwright install chromium` (`:16`) [V]. Not run in any workflow [V, grep of `verevonv3-ci.yml` and `verevonv3-security.yml`].
- Dependencies: `@quarry/client` = `file:../../Ingestion Plane/Quarry-v2/sdks/typescript` (`:23`) [V]; `@verevon/remote-core` = `workspace:*` (`:29`) [V]. `typescript-eslint ^8.69.0` (`:50`); `typescript` aliased to `@typescript/typescript6@^6.0.2` (`:49`); `@typescript/native` = `typescript@^7.0.2` (`:45`) [V].
- LIVE-only: `demo:build` = `node scripts/build-product-demo.mjs` and `demo:probe` = `node scripts/probe-product-demo.mjs` (`package.json` +2 lines) [L]; both scripts are untracked [L].

### 4.2 Playwright

- `playwright.config.ts`: `testDir: 'tests/e2e'` (`:68`), `timeout: 60_000` (`:69`). `BASE_URL = process.env.E2E_BASE_URL || 'http://localhost:5199'` (`:44`); `LOCAL_BASE_URL = process.env.LOCAL_E2E_BASE_URL || 'http://localhost:5173'` (`:46`). CI-only: `forbidOnly` (`:73`), `retries: 1` in CI (`:74`), github reporter (`:76`). Projects: `setup` (`:88`), `e2e` (`:90`), `local-setup` (`:97`), `local` (`:102`), `chat` (`:108`), `product-recording` (`:114`), `product-readiness` (`:123`) [V].
- No `webServer` block [V, grep]: a dev server must already be running at the base URL.
- 27 `*.spec.ts` files under `tests/e2e/` [V, Glob].

### 4.3 Gateway (Rust, `apps/gateway`)

- Crate `verevon-gateway-rs` (`apps/gateway/Cargo.toml:2`); `apps/gateway/Dockerfile` exists [V].
- Commands: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (root CLAUDE.md:65) [V]. CI `gateway-rust` job (`verevonv3-ci.yml:108-141`, working dir `:113`): `cargo fmt --check` (`:133`), `cargo clippy --all-targets -- -D warnings` (`:138`), `cargo test` (`:141`) [V].

### 4.4 Dev server and containers

- `apps/Frontend Plane/verevonv3/Dockerfile`: `FROM ... AS dev` (`:4`); `CMD ["pnpm", "dev", "--host", "0.0.0.0"]` (`:46`); `AS build` (`:48`); `AS production` nginx (`:71`) [V].
- `docker-compose.yml`: project `name: frontend-plane-verevonv3` (`:1`); `frontend` service with `target: dev` (`:219-223`); host port `${VEREVONV3_HOST_PORT:-5173}` bound to `${VEREVON_BIND_ADDRESS:-127.0.0.1}` (`:234`); `- .:/app` bind mount (`:242`); `VITE_HMR_PORT: 5173` (`:302`); health check on `127.0.0.1:5173` (`:318`) [V]. `gateway` builds `./apps/gateway` with `container_name: verevon-gateway-rs` (`:61-66`) [V].
- Local dev port 5174 variant: `pnpm dev --port 5174 --strictPort` passes args to `vite` (`package.json:8`) [V-consistent, not run].

### 4.5 Quarry SDK prerequisite

- `apps/Ingestion Plane/Quarry-v2/sdks/generate.sh` exists [V]; `sdks/typescript/package.json` is absent at HEAD [V, Glob]. The `file:` dependency therefore cannot resolve until `generate.sh` and `pnpm run build` run in `sdks/typescript` [V]. CI does this at `verevonv3-ci.yml:171-177` and `:204-211`.

### 4.6 Install, verify and security in CI

- `spa-verify` job (`verevonv3-ci.yml:143-193`): SDK generation (`:171-177`), `pnpm install --frozen-lockfile` (`:180`), `pnpm --filter @verevon/remote-core run build` (`:190`), `pnpm verify` (`:193`) [V].
- `docker-image` job (`:195-223`): `docker buildx build --target production --build-context 'quarry-client=../../Ingestion Plane/Quarry-v2/sdks/typescript' --tag verevonv3-ci:${{ github.sha }} --load .` (`:215-223`) [V].
- `fabrication-guard` job (`:39-106`): grep-based allowlist, no `org_id_from_headers`, no `demo_accounts|demo_posts|SocialStore`, no `Bearer dev-bypass` outside the two API clients, no `region default` (`:44-104`) [V].
- Security workflow: `pnpm install --frozen-lockfile` (`verevonv3-security.yml:48`), `pnpm audit --audit-level=high` (`:51`) [V].
- i18n: `src/shared/i18n/errors.test.ts` exists [V]; no i18n gate in workflows or package.json [V]. Runs only under `vitest run`.
- LIVE-only job `demo-product-build` (`verevonv3-ci.yml` +55 lines): `node scripts/build-product-demo.mjs --verify`, rebuild with a `git status` check, `pnpm exec playwright install --with-deps chromium`, `node scripts/probe-product-demo.mjs` [L].

## 5. Ingestion Plane (`apps/Ingestion Plane`)

### 5.1 Root Makefile

- `COMPOSE := docker compose --env-file ../../.env --env-file .env -f docker-compose.yml` (`Makefile:1`) [V]. Only two env files; the 10-file chain in memory is not what `make up` uses [V].
- `up` = `$(COMPOSE) up -d --build --wait --wait-timeout 300` (`:66-68`); `down` (`:84-86`); `clean` = `down -v` (`:89-91`); `build` (`:95-97`); `dev-imports` = uvicorn on `imports-core` (`:101-103`); `dev-quarry` = `cargo run -p quarry-edge` (`:106-108`).
- `test-imports` = `pytest -v tests/` in `imports-core` venv (`:123-124`); `test-quarry` = `cargo test --workspace` in `Quarry-v2` (`:127-128`); `test-endpoints` (`:131-151`): curl health on 8082, 3025 `/ready`, 3026, 3130, 3156 `/healthz`, `pg_isready`, Dragonfly `redis-cli ping`, `temporal operator cluster health` [V]; `ci-test` = `test-imports test-quarry test-endpoints` (`:234`) [V]; `warm-bases` (`:274`, `build: warm-bases` `:277`) [V].
- Compose project name `name: ingestion-plane` (`docker-compose.yml:1`) [V].
- Root CLAUDE.md:81-82 lists `make up` and `make test-endpoints` [V].

### 5.2 Quarry-v2 Makefile

- `build` = `build-rust build-go` (`Makefile:20`); `build-rust` = `cargo build --workspace` (`:23`); `build-go` = `go build ./...` in `services/quarry-control` and `services/quarry-orchestrator` (`:26-27`).
- `test` = `test-rust test-go` (`:29`); `test-rust` = `cargo test --workspace` (`:32`); `test-go` = `go test ./...` in the same two services (`:35-36`); `test-vision` = `python -m pytest` in `services/quarry-vision-sidecar` (`:39`); `bench-browser` (`:42`); `fmt` = `cargo fmt` plus `gofmt -w` (`:45-47`) [V].

### 5.3 Quarry-v2 CI (`quarry-v2-ci.yml`, working dir `apps/Ingestion Plane/Quarry-v2`)

- `cargo check --workspace` (`:202`), `cargo test --workspace` (`:205`), `cargo check -p quarry-browser --features chromiumoxide` (`:208`), `cargo test -p quarry-browser --features chromiumoxide` (`:211`) [V].
- `cargo audit --deny warnings` (`:233`) [V]; `govulncheck ./...` per module (`:263`) [V]; `cargo llvm-cov --workspace --all-features --fail-under-lines 80` (`:304`) [V]; `go test -coverprofile=coverage.out ./...` with 80% floor (`:329`) [V].
- `govulncheck` (`:242`), `rust-coverage` (`:268`) and `go-coverage` (`:309`) are `continue-on-error: true`, i.e. report-only [V]. Schedule `cron: '0 6 * * 1'` (`:136`) runs only cargo-audit and govulncheck (`:147`, `:267`, `:308` `if:` gates) [V].

### 5.4 integration-corev2 (Go)

- No Makefile and no CI workflow [V]. `go.mod` present [V]. README: `go test ./...` and `go run ./cmd/api` (`apps/Ingestion Plane/integration-corev2/README.md:173-174`) [V].
- `imports-core`: `pytest -v tests/` via Makefile `:124` [V].

### 5.5 Quarry Rust build prerequisites (host note)

- `wreq = "=6.0.0-rc.28"` (`Quarry-v2/Cargo.toml:45`) [V]; `deploy/compose/README.md:66-67` lists `pkg-config libssl-dev protobuf-compiler cmake perl clang build-essential` for `boring-sys2` and `tonic-build` [V]; `deploy/compose/Dockerfile.edge:9-10`, `:25-27`, `:40-42` list the same packages [V]. No `FROM rust` line in any Quarry-v2 Dockerfile [V, grep].

## 6. CI workflows

### 6.1 Inventory (HEAD, 19 files in `.github/workflows`)

Plane key: MP = Model Plane, CP = Control Plane, AP = Application Plane, FE = Frontend verevonv3, IP = Ingestion Plane, DP = Data Plane v2 (out of scope), X = repo-wide.

| File | Name | Triggers | Jobs | Commands / gates | Plane |
|---|---|---|---|---|---|
| `buf.yml` | Buf Proto CI | push and PR on main/develop, path `apps/Model Plane/proto/**` (`:3-13`) | `lint` (`:15`) | `buf lint`, `buf breaking ... --against baseline.binpb` (`:21,23`) | MP |
| `contract-goldens.yml` | Contract goldens | PR and push main, goldens paths incl. `apps/**/inference-service-tokens.golden.json*` (`:17-44`) | `goldens` (`:55`) | `zdr-contract-sync.test.sh` (`:66`), `--self-test` (`:71`), `node --test zdr-contract-schema.test.mjs` (`:74`) | MP, CP, DP (ZDR goldens) |
| `model-plane-rust-ci.yml` | Model Plane Rust CI | PR (paths `:27-40`), push main (`:41-53`) | `rust` (`:68`) | `zdr-contract-sync` (`:95-96`), `cargo fmt --all -- --check` (`:99`), `cargo check --workspace` (`:105`), `cargo check --workspace --all-targets` (`:112`), `cargo test --workspace --no-fail-fast` (`:119`) | MP (path filters also cover CP auth-core fixtures, CP org-core/audit-core testdata, DP, Ingestion Quarry fixtures) |
| `sse-taxonomy.yml` | SSE taxonomy freshness | push and PR paths (`:16-30`) | `check` (`:33`) | `gen-sse-event-taxonomy.py --check` (`:45`); drift proof (`:49-62`) | MP, FE |
| `eval-lab.yml` | Eval Lab (Python) CI | push and PR main/develop, path `python/eval-lab-py/**` (`:21-31`) | `selftest` (`:37`) | `pip install -e ".[dev]"` (`:56`), `pytest tests/ -q --junit-xml` (`:64`), >=10 offline cases (`:73`) | MP |
| `dataplane-v2-ci.yml` | Data Plane v2 CI | PR and push main, path-filtered (`:8-17`) | `rust-check` (`:26`), `rust-integration-tests` (`:73`), `go-build` (`:124`), `documents-api-postgres-tests` (`:157`), `postgres-contract-wiring` (`:203`), `postgres-contract-tests` (`:230`), `subject-contract` (`:292`), `graph-index-no-azure-contract` (`:302`), `contextual-retrieval-config-contract` (`:315`), `background-budget-contract` (`:330`), `eval-harness` (`:343`), `docker-smoke` (`:359`) | `cargo fmt`, `cargo check --workspace`, `cargo test --workspace --tests` (`:46,52,71`); not read further | DP |
| `dataplane-v2-eval-nightly.yml` | Data Plane v2 Nightly Retrieval Eval | `schedule` (`:32`), `workflow_dispatch` (`:37`) | `eval` (`:53`) | not read | DP |
| `quarry-v2-ci.yml` | Quarry-v2 CI | PR and push main, `apps/Ingestion Plane/Quarry-v2/**` (`:120-130`); weekly `0 6 * * 1` (`:131-136`) | `cargo-check` (`:145`), `cargo-audit` (`:213`), `govulncheck` (`:239`), `rust-coverage` (`:265`), `go-coverage` (`:306`) | see 5.3 | IP |
| `verevonv3-ci.yml` | Verevon v3 CI | PR and push main, `apps/Frontend Plane/verevonv3/**` plus Quarry `openapi.yaml` and `sdks/generate.sh` (`:10-28`) | `fabrication-guard` (`:39`), `gateway-rust` (`:108`), `spa-verify` (`:143`), `docker-image` (`:195`) | see 4.3, 4.6 | FE, IP (SDK) |
| `verevonv3-security.yml` | Verevon v3 Frontend Security | push and PR main with paths (`:8-18`), `schedule` (`:19`) | `audit` (`:26`) | `pnpm audit --audit-level=high` (`:51`) | FE |
| `secret-scan.yml` | secret-scan | PR, push main, `workflow_dispatch` (`:9-13`) | `gitleaks` (`:32`) | `gitleaks/gitleaks-action@v3` with `GITLEAKS_CONFIG: .gitleaks.toml` (`:58,68`) | X |
| `auth-service.yml` | Auth Service CI/CD | push and PR main/develop with paths (`:13-36`) | `test` (`:44`), `build` (`:83`), `deploy-staging` (`:121`), `deploy-production` (`:132`) | see 2 (`:60-81`) | CP |
| `user-service.yml` | User Service CI/CD | push and PR main/develop, `apps/Control Plane/user-core/**` (`:10-18`) | `test` (`:27`), `build` (`:72`), `deploy-staging` (`:110`), `deploy-production` (`:120`) | see 2 (`:67,70`) | CP |
| `org-service.yml` | Org Service CI/CD | push and PR main/develop (`:13-22`) | `test` (`:32`), `build` (`:108`), `deploy-staging` (`:146`), `deploy-production` (`:156`) | see 2 | CP |
| `session-core-service.yml` | Session Core (Go) CI | push and PR main/develop (`:13-21`) | `test` (`:25`) | `go test -v ./...` (`:43`), build (`:46`) | CP |
| `billing-core-service.yml` | Billing Core CI | push and PR main/develop (`:16-24`) | `test` (`:28`) | `go test -v -p 1 ./...` (`:84`), build (`:87`) | CP |
| `notification-core-service.yml` | Notification Core CI | push and PR main/develop (`:12-20`) | `test` (`:24`) | see 3 | AP |
| `conversation-core-service.yml` | Conversation Core (Go) CI | push and PR main/develop (`:11-19`) | `test` (`:23`) | see 3 | AP |
| `social-core-service.yml` | Social Core Service CI/CD | push and PR main/develop (`:12-20`) | `test` (`:27`) | see 3 | AP |

Notes: `quarry-docs-contract.yml` is LIVE-only (untracked) [L]: PR and push, runs `node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"` (`:12,24,53`). No workflow references `information-core`, `insight-core`, `leads-core`, `zammad-foundation`, `convex-core`, `imports-core`, `finspo-core`, `shipping-core`, `autocomplete-core`, `Quarry-v2/services` or `integration-corev2` [V, grep]. Deploy jobs in CP workflows were not read in detail.

### 6.2 Gates named in the task

| Gate | Where | Status |
|---|---|---|
| buf breaking | `buf.yml:23` | HEAD. The LIVE `sessions.proto` is modified (+42 lines) and `baseline.binpb` is not; the live outcome is not determined [L]. |
| SSE taxonomy gate | `sse-taxonomy.yml:45` | HEAD. `sse_events.rs` and `docs/sse-event-taxonomy.md` are modified in LIVE; outcome not determined [L]. |
| cross_service_loop_contract | `execution-core/tests/cross_service_loop_contract.rs`, run by `model-plane-rust-ci.yml:119` | HEAD [V]. |
| ZDR golden matrix | `contract-goldens.yml:66,71,74`; `zdr-contract-sync.test.sh`; `api/contracts/zdr-posture-matrix.json` | HEAD [V]. The Rust-side matrix test and fixture are LIVE-only [L]. |
| Size ratchet | none. The only ratchet is auth-core `lint-ratchet.mjs` (`package.json:15`, run by `auth-service.yml:63`). A LIVE demo budget exists in an untracked script [L]. | |
| i18n gates | none in CI or package.json [V]. `errors.test.ts` runs under `vitest` [V]. | |

### 6.3 `.githooks/pre-commit`

Opt-in per clone with `git config core.hooksPath .githooks` (hook header `:4-5`). It scans only the staged diff (`git diff --cached`) with gitleaks `detect --pipe --redact --no-banner` (`:35`, `:67`, `:81-83`), using a local `gitleaks` binary if present, otherwise `docker run ghcr.io/gitleaks/gitleaks:latest` (`:64-83`), and points at `.gitleaks.toml` when present (`:65`, `:70-73`). It removes lockfiles from the diff (Cargo.lock, go.sum, go.work.sum, pnpm-lock.yaml, package-lock.json, yarn.lock; `:49-56`). An empty diff exits 0 (`:60-62`). If neither tool is available it warns and allows the commit (`:84-88`). Exit 1 from gitleaks blocks the commit with a "do not use --no-verify" message (`:96-101`). Exit 125-127 (container cannot start) skips with a warning (`:102-106`). Any other non-zero status fails closed (`:107-114`). LIVE adds one lockfile-style exclude for `**/verevon-web/public/product-demo/**` (`:56-57`, +2 lines) [L].

## 7. Repo evidence rule (how the repo counts a "done" or deployment claim)

- No single sentence defines "rebuilt local images" as deployment evidence. The rule is spread across the register, the status file and the status claims below.
- `apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md:6-7`: "A claim is not release-ready merely because source code or a unit test exists."
- Same file `:9-21`: evidence classes `source`, `test`, `integration` ("passed against release-shaped dependencies"), `staging` ("observed in a deployed, immutable candidate"), `candidate` (signed artifact and external runtime policy verified), `rollback`, `production`. `:21`: "Only `candidate` and `rollback` evidence can support a production promotion."
- Same file `:50-54`: "Release status remains blocked: candidate/rollback artifacts are absent, capabilities are `source_only`, ..."
- `apps/Model Plane/MODEL_PLANE_STATUS.md:7-8`: "The running stack is evidence for this dirty source aggregate only, not a production-release authorization."
- `MODEL_PLANE_STATUS.md:90-96` (table "Evidence states"): **Source-verified** = "Source and repeatable local tests pass; this alone is not deployment evidence." **Live-verified** = "... reproduced on 2026-07-16; not immutable production evidence." **Blocked** = "Required implementation, operator input, or live proof is absent."
- `MODEL_PLANE_STATUS.md:41`: "Source hardening has advanced materially, but it is not deployment evidence:"
- `MODEL_PLANE_STATUS.md:258-259`: "The running Auth container ... has not been rebuilt from that source." (the image-rebuild rule in practice)
- `MODEL_PLANE_STATUS.md:154-157`: "no external Letta, provider, Postgres release database, NATS deployment, or end-to-end Model Plane is represented as real production evidence."
- `MODEL_PLANE_STATUS.md:11-12` names the binding decision `docs/core-research/grpc-safe-rebuild-decision-2026-07-16.md`. Not read here.
- `apps/Model Plane/CLAUDE.md:33`: read `MODEL_PLANE_STATUS.md` "before trusting any 'done' claim — it tracks source-verified vs. live-verified vs. blocked with dates."
- `apps/Model Plane/CLAUDE.md:57`: "Never claim a feature is 'done' without source + test evidence".
- Root `SYSTEM_STATUS.md:3-9` (2026-07-16 correction): "This is not a production release: the eleven app images are unsigned dirty-tree builds, there is no accepted candidate/rollback artifact ..."
- Root `SYSTEM_STATUS.md:48-49`: "verified via build/test; most NOT yet deployed — Docker rebuild gated" and "fixed + live-verified (this one deployed)". The repo separates build/test verification, deployment and live verification.
- `apps/Model Plane/docs/CAPABILITY_PROMOTION_LEDGER.tsv`: header `capability_id state source_revision candidate_ref live_evidence_ref rollback_ref expires_at reviewer notes` (`:1`); 28 rows (`:2-29`), every row `state = source_only`, with `candidate_ref`, `live_evidence_ref` and `rollback_ref` set to `-` [V, awk count].
- Root `CLAUDE.md` and `AGENTS.md`: no evidence or "done" rule found (grep) [V]. The rule lives in the Model Plane `CLAUDE.md`.

## 8. Memory hints (`C:/Users/ImaFernandesDaCosta/.claude/projects/C--dev-CoresSystem/memory/`) checked against the repo

| Memory file (age) | Claim | Status |
|---|---|---|
| model-plane-build-and-deploy (33d) | `bash apps/Model Plane/scripts/compose.sh up -d --no-deps --build <service>` | [V] pass-through (`compose.sh:199`); not run |
| | plain `docker compose` in that dir fails | [U]; `compose.sh:102-103` supplies `SOURCE_REVISION`/`BUILD_DATE` |
| | `inference-core/src/main.rs` uses `tokio::signal::unix` and fails on Windows | [S] `main.rs:108-121` has `#[cfg(unix)]` and `#[cfg(not(unix))]` (Ctrl-C fallback, "Without this the binary does not compile there") |
| | `e2e_invoke_chain_test.rs` misses field `metadata` in `SessionMessage` | [S] literal at `:1338-1344` uses `..Default::default()`; LIVE file is modified (+288) [L] |
| | `normalize.rs` test literal assigns `Vec<String>` to `tools` (fixed, uncommitted) | [S] corrected at HEAD: `normalize.rs:528-530`; LIVE file modified [L] |
| | `crates/mp-contracts/build.rs` generates protobuf at build time | [V] file exists; CI comment `model-plane-rust-ci.yml:24` |
| | Go bindings under `go/gen/` are checked in; need `buf generate`; buf not installed | [V] `go/gen` present; `buf.gen.go-only.yaml:9,13`; buf install [U] |
| | `go.work` exists, lists 8 services incl. sandbox-manager | [V] `go/go.work:15-22` |
| | `go build ./...` from `go/` fails; per-service works | [U] consistent with go.work (no `.` entry); not run |
| | `-race` needs cgo; host `CGO_ENABLED=0` | [U] host |
| | `GOWORK=off` when adding deps | [U] not in repo docs checked |
| control-plane-build-and-deploy (37d) | use `scripts/run-control-plane.sh <compose args>` from `apps/Control Plane` | [V] `run-control-plane.sh:513` |
| | merges per-service `.env` and `.env.docker`; supplies interpolation values | [V] `:4-11`, `:21-33` |
| | bare `docker compose build user-core` fails and exits 0 | [U] not run |
| | `bash scripts/run-control-plane.sh build user-core` / `up -d --no-deps user-core` | [U] pass-through verified; `user-core` as the base-compose service key not determined (`docker-compose.yml:381` has `container_name: user-service`; the production overlay has a `user-core:` key at `docker-compose.production.yml:128`) |
| | container is `user-service`; gateway reaches `http://user-core:3012` | [V] `docker-compose.yml:381`; consumers use `user-core:3012` (`apps/Ingestion Plane/docker-compose.yml:435`); DNS alias not determined |
| | never pass the production overlay | [V] `run-control-plane.sh:16-19` refuses |
| verevonv3-worktree-verification (37d) | `:5173` serves the main checkout (bind mount of `C:\dev\CoresSystem\apps\Frontend Plane\verevonv3`) | [V] `docker-compose.yml:242` (`- .:/app`); the mount points at the checkout that holds the compose file, so the worktree is not served |
| | run a second dev server with `pnpm dev --port 5174 --strictPort` | [V-consistent] `package.json:8` |
| | `node_modules` absent in a fresh worktree; `pnpm install` takes ~35s | [U] host; `node_modules` is present in this worktree (Glob hits) |
| | every `pnpm` command prunes `pnpm-lock.yaml` | [U] not run |
| | `pnpm typecheck`: 21 errors from missing `@quarry/client` package.json | [V] dependency `file:` at `package.json:23`; `sdks/typescript/package.json` absent at HEAD; error count [U] |
| | `pnpm lint` dies: typescript-eslint 8.67 refuses TS 7.0 | [S] `package.json:45-50` now pins typescript-eslint `^8.69.0` and aliases `typescript` to TS6; not run |
| | full vitest suite: 12 failed files / 30 tests / 8 errors baseline | [U] not run |
| quarry-rust-tests-in-docker (40d) | Quarry crates do not build on Windows; `boring-sys2` via `wreq` needs cmake, clang, perl, go, protoc | [V] deps (`Quarry-v2/Cargo.toml:45`, `deploy/compose/README.md:66-67`, `Dockerfile.edge:9-10,25-27`); Windows failure [U] |
| | run in a `rust:1` container with `quarry-cargo`, `quarry-go`, `quarry-target` volumes; `apt-get install cmake clang perl pkg-config libssl-dev git golang-go protobuf-compiler` | [U] host recipe; no `FROM rust` in Quarry Dockerfiles; apt list matches `Dockerfile.edge:40-42` in part |
| | `bash -c`, not `bash -lc` | [U] host |
| | full test run takes >10 minutes | [U] |
| auth-core-local-testing (32d) | jest and tsc run after `pnpm install` in `apps/Control Plane/auth-core` | [V] scripts (`package.json:17`); not run |
| | full suite: 15 failing tests in 3 suites on HEAD (`deployment-contract.spec.ts`, `plane-service-principal.spec.ts`, `model-plane-token.controller.spec.ts`) | [V] three spec files exist (`src/auth/*.spec.ts`); failure count [U] |
| | `pnpm lint` (`scripts/lint-ratchet.mjs`) dies with ENOBUFS | [U] not run; script exists [V] |
| | CRLF worktree files; `prettier --end-of-line auto`; `Delete ␍` noise | [U] host/git config |
| | deploy with `run-control-plane.sh up -d --no-deps --build auth-core` | [U] `auth-core` service key not verified |
| | pre-commit runs gitleaks on the staged diff | [V] `.githooks/pre-commit` |
| bash-heredoc-limit (40d) | long heredocs fail in the Bash tool; write with Write, then run | [U] tool behaviour, not a repo command |
| integration-core-env-drift (40d) | CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN: owner in Application Plane, mirror in Ingestion | [V] `scripts/bootstrap_runtime_environment.sh:847-848` (ensure from `$APPLICATION_ENV`, sync to `$INGESTION_ENV`) |
| | bootstrap script treats Application as owner (line ~806) | [S] direction [V]; the line is now 847-848 |
| | never re-add Control Plane service tokens to integration-corev2 `.env` | [U] `.env` is host state (not in repo) |
| | 3 dormant copies; 21 vs 62 pending changes; ledgers renamed `.env.bootstrap-orphan`, script refuses every run | [U] host state |
| convex-core-functions-deploy (36d) | functions under `apps/Application Plane/convex-core/convex/*`; pushed by `convex-gateway` running `npx convex dev` | [V] `startup.sh:132`; `convex-gateway` in two compose files (`docker-compose.yml:117`, `convex-core/docker-compose.yml:109`); `convex/` folder implied by lint glob (`package.json:10`); bind mount [U] |
| | `node:test` suites in `test/*.test.cjs` | [V] `package.json:12`; 13 files |
| | success line "Convex functions ready!" | [U] string not found under `convex-core/` (grep) |
| | `docker restart convex-gateway` triggers a re-push | [U] runtime |
| ingestion-plane-deploy-recipe (40d, extra) | compose project `ingestion-plane` | [V] `docker-compose.yml:1` |
| | 10-file `--env-file` chain, then `docker compose -p ingestion-plane ... up -d --no-deps --build` | [S] the Makefile uses two env files (`Makefile:1`); chain and swap script `C:/dev/tools/swap_gateway_image.py` are host-side [U] |
| | `docker restart frontend-plane-verevonv3-frontend-1` after frontend edits | [V-consistent] verevonv3 compose `frontend` service (`apps/Frontend Plane/verevonv3/docker-compose.yml:219`); default container naming [U] |
| verevonv3-vite-needs-restart (36d, extra) | container runs `pnpm dev --host 0.0.0.0` over a bind mount | [V] `Dockerfile:46`; `docker-compose.yml:242`; HMR and browser-cache behaviour [U] |

## 9. LIVE-tree deltas that change gates or commands

| Path | Live state | Effect |
|---|---|---|
| `.github/workflows/model-plane-rust-ci.yml` | +50 lines: job `session-core-receipts` (Postgres 16 service; `zdr_receipt` ignored tests; >=8 required) [L] | New gate, HEAD has none. Job comment places the `zdr_receipt*` tests in `session-core` `retention_receipt::db_tests`; `retention_receipt.rs` and migrations `0044`/`0045` are untracked [L] |
| `.github/workflows/verevonv3-ci.yml` | +55 lines: job `demo-product-build` [L] | Depends on untracked `scripts/build-product-demo.mjs` and `probe-product-demo.mjs` [L] |
| `.github/workflows/dataplane-v2-ci.yml` | +25 lines: rerank contract steps and job `web-rerank-contract` [L] | Data Plane (out of scope) |
| `.github/workflows/quarry-docs-contract.yml` | new, untracked [L] | Ingestion docs contract |
| `.githooks/pre-commit`, `.gitleaks.toml` | +2 and +9 lines: allowlists for `verevon-web/public/product-demo/**` and `scripts/eval-corpus/repo/` [L] | Secret-scan exceptions |
| `apps/Frontend Plane/verevonv3/package.json` | +2 scripts: `demo:build`, `demo:probe` [L] | |
| `apps/Model Plane/rust/services/model-gateway/src/normalize.rs`, `.../tests/e2e_invoke_chain_test.rs` | modified (e2e +288) [L] | Test outcome not determined |
| `apps/Model Plane/proto/model_plane/v1/sessions.proto` | modified (+42) [L] | `buf breaking` outcome not determined |
| `apps/Model Plane/rust/services/model-gateway/src/sse_events.rs`, `docs/sse-event-taxonomy.md` | modified [L] | SSE gate outcome not determined |
| `apps/Model Plane/rust/services/model-gateway/tests/zdr_posture_matrix.rs` (+ fixtures), `zdr_refusal_inventory.rs`, `retention.rs`, `zdr_refusal.rs` | untracked [L] | Live-only ZDR tests |
| `apps/Ingestion Plane/docker-compose.yml` | modified (+50/-) [L] | |
| `apps/Control Plane/auth-core/src/auth/fixtures/cross-plane-token-requests.json` (+ `.sha256`) | modified [L] | Cross-plane fixture drift check (`model-plane-rust-ci.yml:34`) |
| `inference-core/src/main.rs`, `Control Plane/scripts`, `Control Plane/docker-compose*.yml`, `Application Plane/`, `playwright.config.ts`, `Ingestion Plane/Makefile`, `Quarry-v2/Makefile`, `go/`, `scripts/`, `deploy/` | not modified in LIVE [V] | HEAD commands are current for these |

## 10. Not determined, and why

- Whether any HEAD or LIVE cargo, go, pnpm, Docker or Playwright command passes. None were run.
- The Model Plane Windows trap count (memory says two targets fail; source says none fail at HEAD). A compile on this host would settle it.
- Which `convex-gateway` compose file is authoritative, and the bind mount for `convex-core`.
- The DNS alias that makes `user-core:3012` resolve in the base Control Plane compose.
- The Ingestion Plane env-file chain used by the orchestrator (outside the repo, or in `.env.generated-secrets`).
- Outcome of `buf breaking`, the SSE gate and `e2e_invoke_chain_test.rs` in LIVE (files modified, not run).
