# S9 ci-and-run-commands

Scope: the verification surface (build, test, CI gates, e2e and smoke scripts, prod-mode config) for Quarry-v2, Model Plane, the Frontend gateway and SPA, Control Plane auth-core and org-core, integration-corev2, and the cross-plane golden gates. No audit item. Read from the live tree `C:/dev/CoresSystem` (commit afb83928 plus uncommitted work). Nothing was built, installed or written there. Four read-only test scripts were run (section 7). Every git call used `--no-optional-locks`.

Tags. Paths: [C] committed and unchanged; [M] committed, modified in live tree; [U] untracked (exists only as uncommitted work); [?] exists in live tree but is neither in afb83928 nor in the status list (git-ignored); [MISSING] cited path does not exist. Evidence: [RUN] executed read-only by S9; [READ] opened and read by S9; [CI] stated by a workflow file, not executed; [MEM] user memory note (outside the repo, not re-verified); [INFER] reasoned from code; [UNVERIFIED] needs a real run.

## Cross-item overlaps (merge-conflict hotspots)

1. `scripts/tests/zdr-contract-sync.test.sh` [M]: GOLDENS, VENDORED and CONSUMERS rows for Quarry, Model Plane, Data Plane, SPA, integration-corev2, auth-core. Run by `contract-goldens.yml` [C] and `model-plane-rust-ci.yml` [M].
2. `apps/Ingestion Plane/docker-compose.yml` [M]: quarry-edge switches, prod env and defaults. Read by `scripts/tests/quarry-model-plane-switches.test.mjs` [U] and `apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs` [U].
3. `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` [M] and `config.rs` [M]: prod checks; both are named by the docs-contract trigger in `quarry-docs-contract.yml` [U].
4. `apps/Control Plane/auth-core/src/auth/fixtures/*` [C; `cross-plane-token-requests.json` and its `.sha256` are M]: vendored into quarry-edge, model-gateway, retrieval-engine, inference-client, embedding-engine, org-core and audit-core; path triggers in `model-plane-rust-ci.yml` [M] and `contract-goldens.yml` [C].
5. `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/contracts/cross-plane-token-requests.json` [M] and `.sha256` [M]: canonical manifest; vendored copy in auth-core fixtures [M]; Rust pin test `src/token_request_contract.rs` [M].
6. `apps/Model Plane/api/contracts/`: `zdr-posture-matrix.json`, `infer-statuses.json`, `connection-zdr-capabilities.json` [C]; `zdr-refusals.json` and `.sha256` [U]. Canonical goldens, vendored into four or more trees.
7. `apps/Control Plane/config/plane-service-principals.json` [C]: trigger in `quarry-docs-contract.yml` [U], `auth-service.yml` [C], and the principal-registry parity test [C].
8. `.github/workflows/model-plane-rust-ci.yml` [M], `verevonv3-ci.yml` [M], `dataplane-v2-ci.yml` [M]: uncommitted job additions on committed bodies.
9. `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml` [M]: path filter in `verevonv3-ci.yml` [C] (SDK generation); read by docs-contract test [U].
10. `apps/Frontend Plane/verevonv3/package.json` [M], `src/shared/api/contracts/zdr-refusals.json` and `.sha256` [U], `scripts/build-product-demo.mjs` [U], `scripts/probe-product-demo.mjs` [U]: SPA gates.

## 0. Headline findings (ordered by severity)

H1. Gate files are not in git. Untracked [U]: `.github/workflows/quarry-docs-contract.yml`; `apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs`; `scripts/tests/quarry-model-plane-switches.test.mjs`; `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/rerank_behavior.rs`; `apps/Frontend Plane/verevonv3/scripts/build-product-demo.mjs`, `probe-product-demo.mjs`, `demo-live-check.mjs`, `scene-preview.mjs`, `capture-demo-poster.mjs`; the product-demo bundle `apps/Frontend Plane/verevonv3/apps/verevon-web/public/product-demo` (55 files, `??` in git status, not ignored); `apps/Model Plane/rust/services/session-core/src/retention_receipt.rs` (declared at `session-core/src/main.rs:35` in the live tree). Plus 28 golden and vendored fixtures (section 7). The passing results in section 7 depend on these files. A push that includes the modified sync script but not these 28 fixtures fails the sync gate on missing files [INFER]. A push of only committed files leaves the docs gate absent and the newer golden rows unchecked.

H2. As committed, a production quarry-edge cannot start [READ + INFER; boot not run]. `ENVIRONMENT=production` forces `require_browser_grants` (`main.rs:183-184`) and bails when no BrowserBroker validator URL is set (`main.rs:197-201`). Neither `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL` nor `..._GRPC_URL` appears in `apps/Ingestion Plane/docker-compose.yml` [M], `docker-compose.production.yml` [C], or the variable names of `apps/Ingestion Plane/Quarry-v2/.env` [?; names read, values not read], which is the only `env_file` of quarry-edge (`docker-compose.yml:10-11`). Separately, `ARTIFACT_BACKEND` defaults to `memory` (`docker-compose.yml:115`, `.env.example:35`) and production bails on memory (`main.rs:202-211`) unless the operator sets `fs`, `filesystem` or `s3`.

H3. Fail-open candidate: dev-bypass parsing mismatch [READ; not run]. The startup refusal matches case-sensitively: `main.rs:133` accepts `"1" | "true" | "TRUE" | "yes"`. The runtime readers accept `"1"` and any case of `"true"`: `auth.rs:258-261` and `change_routes.rs:403-406` (`eq_ignore_ascii_case`). With `ENVIRONMENT=production` and `QUARRY_EDGE_AUTH_DEV_BYPASS=True`, main.rs does not refuse to start, and `auth.rs:450-453` injects a stub identity for bearers that fail JWKS verification. `docker-compose.production.yml:30` pins the flag to `"0"`, so this needs a non-overlay production config. `auth.rs` [C]: the mismatch is in committed code.

H4. Fail-open candidate: artifact backend case mismatch [READ; not run]. `main.rs:369-372` lowercases and trims before computing `artifacts_durable`; `main.rs:373` matches the raw string. `QUARRY_EDGE__ARTIFACT_BACKEND="S3"` (or `"Fs"`, or `" fs"`) passes the durability gate (`durable_ready` includes `artifacts_durable`, `main.rs:694-703`) and selects `InMemoryStore` (`main.rs:386-389`).

H5. Inert CI gate: `.github/workflows/buf.yml` [C] runs `buf lint apps/Model Plane/proto` with an unquoted path, so the shell splits it at the space. Its breaking baseline `apps/Model Plane/proto/baseline.binpb` [C] is 0 bytes. The demo job's first step runs untracked scripts (H1). Both make the gate fail as written [INFER].

H6. Uncommitted default flip [M diff]. Live `docker-compose.yml` [M] defaults `QUARRY_EDGE__AUTOPROMPT` and `QUARRY_EDGE__SEMANTIC_RERANK` to `0` (`:129`, `:152`). The committed compose defaults both to `1`. The untracked switch test asserts `0` and passes (section 7). Committing only the committed compose would enable switches whose header says Auth Core refuses them.

H7. Port mismatch in scripts [INFER from published ports; not run]. `apps/Ingestion Plane/Quarry-v2/deploy/scripts/smoke.sh` [C] line 4 and `scripts/start-stack.sh` [C] line 32 default the edge to `http://localhost:8080`. The standalone compose publishes the edge at `127.0.0.1:38080` (`deploy/compose/docker-compose.yml` [C] line 113). The Ingestion compose publishes it at `127.0.0.1:8082` (`docker-compose.yml` [M] line 229). `start-stack.sh` waits 90 s on :8080 and exits 1. `start-stack.sh` line 33 defaults the control token to `devsecret`, while the standalone compose requires `QUARRY_CONTROL_API_KEY` (line 153).

H8. No CI gate for large parts of the surface [READ]. Not in any workflow: Model Plane Go services (`browser-broker` 7, `capability-core` 88, `cost-core` 19 `_test.go` files); integration-corev2 (112 `_test.go` files plus `services/webhook-normalizer-rs`); the 27 Playwright specs in the SPA; quarry-vision-sidecar pytest; the live `#[ignore]` tests; six auth-core `*.postgres.spec.ts` files; auth-core `lint:contract`. Nested workflow directories that GitHub never runs [C]: `apps/Infra Plane/.github/workflows/{build-and-deploy,deploy,fresh-install}.yml`; `apps/Model Plane/api/sdk/python/.github/workflows/python.yml`; `apps/Ingestion Plane/Quarry-v2/.claude/worktrees/vigilant-banach-6933aa/.github/workflows/security.yml`.

H9. Memory notes are partly stale (section 10): `go.work` exists; the inference-core Windows break is cfg-gated; the e2e test literal now uses `..Default::default()`.

## 1. Gate map (what CI runs)

| Gate | Workflow | Triggers | Commands (line) | Status |
|---|---|---|---|---|
| Quarry-v2 CI | `quarry-v2-ci.yml` [C] | PR and push to main, `apps/Ingestion Plane/Quarry-v2/**` | `cargo check --workspace`, `cargo test --workspace` (:201-205); `cargo check/test -p quarry-browser --features chromiumoxide` (:207-211); `cargo audit` with 4 ignores (:231-237); govulncheck (:239-263), rust-coverage `--fail-under-lines 80` (:265-304), go-coverage 80% awk floor (:306-333), all `continue-on-error` | Hard: cargo-check, cargo-audit. Report-only: rest |
| Quarry docs contract | `quarry-docs-contract.yml` [U] | PR and push to main | `node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"` (:53) | Not on main. Live: 17/17 pass [RUN] |
| Model Plane Rust | `model-plane-rust-ci.yml` [M] | PR (any base), push to main | `cargo fmt --all -- --check` (:99); `cargo check --workspace` (:105); `--all-targets` (:112); `cargo test --workspace --no-fail-fast` (:119); golden drift `zdr-contract-sync.test.sh` and `--self-test` (:95-96). Live adds job `session-core-receipts` (:127-169) | Committed body; receipts job uncommitted |
| Contract goldens | `contract-goldens.yml` [C] | PR (any base), push to main, golden paths | `zdr-contract-sync.test.sh` (:66); `--self-test` (:71); `node --test scripts/tests/zdr-contract-schema.test.mjs` (:74). `REQUIRE_CONTRACT` deliberately unset (:13-15) | Live: sync 90 ok / 0 failed / 4 pending; schema 65/65 [RUN] |
| Verevon v3 CI | `verevonv3-ci.yml` [M] | PR to main, push to main; SPA paths, Quarry `openapi.yaml`, `sdks/generate.sh` | fabrication guard (:39-106); gateway-rust: fmt (:133), clippy `--all-targets -D warnings` (:138), `cargo test` (:141); spa-verify: SDK build, `pnpm install --frozen-lockfile`, `pnpm verify` (:171-193); docker-image (:195-223); demo-product-build (:228-278, uncommitted) | Committed jobs fine; demo job cannot pass until its files are committed |
| Verevon v3 security | `verevonv3-security.yml` [C] | push and PR to main, weekly cron | `pnpm install --frozen-lockfile`; `pnpm audit --audit-level=high` (:51) | [CI] |
| Auth service | `auth-service.yml` [C] | push and PR to main/develop; auth-core, config, `run-control-plane.sh`, parity script | `pnpm install` (:60, not frozen); `pnpm run lint` (:63); `pnpm run build` (:66, labelled "Type check"); `REQUIRE_CONTRACT=1 pnpm test` (:71-74); `bash scripts/tests/principal-registry-parity.test.sh` (:81); docker build (not on PR) | [CI]; Node 20 |
| Org service | `org-service.yml` [C] | push and PR to main/develop | Postgres 16 service (:37-50); role and fixture bootstrap (:74-93); `go test -v -p 1 ./...` with `REQUIRE_CONTRACT=1`, `TEST_DATABASE_URL`, `CONTROL_LIFECYCLE_TEST_DATABASE_URL` (:95-103); `go build -v ./cmd/server` (:106) | [CI]; setup-go `1.24` (:57) vs `go.mod` `go 1.25` |
| Buf proto | `buf.yml` [C] | push and PR to main/develop, `apps/Model Plane/proto/**` | `buf lint apps/Model Plane/proto`; `buf breaking ... --against apps/Model Plane/proto/baseline.binpb` | Broken as written (H5) |
| SSE taxonomy | `sse-taxonomy.yml` [C] | model-gateway `sse_events.rs`, SPA `chat-client.ts`, generator | `python3 "apps/Model Plane/scripts/gen-sse-event-taxonomy.py" --check` (:45) | [CI] |
| Eval lab | `eval-lab.yml` [C] | `apps/Model Plane/python/eval-lab-py/**` | `python -m pytest tests/ -q --junit-xml=eval-selftest.xml` (:63-64) | [CI]; live tests need `EVAL_LIVE=1` |

Not gated: Quarry `make lint` (`cargo clippy --workspace --all-targets -D warnings`, Quarry-v2 `Makefile` lint target); Model Plane clippy (the workflow comment says 304 pedantic warnings, model-plane-rust-ci.yml:174-184); all Go `go vet`; the SPA Playwright suite; the Model Plane and integration-corev2 Go tests.

## 2. Quarry-v2 (Rust workspace and Go services)

### 2.1 Commands

- Rust (CI-equivalent), from `apps/Ingestion Plane/Quarry-v2` [C]: `cargo check --workspace`; `cargo test --workspace`; `cargo check -p quarry-browser --features chromiumoxide`; `cargo test -p quarry-browser --features chromiumoxide`. CI needs protoc 27.x (`quarry-v2-ci.yml:174-176`) and `CARGO_PROFILE_TEST_DEBUG=0` (:164). Workspace members (`Cargo.toml` [C] :4-12): quarry-core, quarry-security, quarry-transform, quarry-browser, quarry-tls, quarry-runtime, quarry-edge, lab/evals. `workspace.package.rust-version = "1.85"` (Cargo.toml [C] :17).
- Audit (CI): `cargo audit --deny warnings --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2025-0134 --ignore RUSTSEC-2026-0192 --ignore RUSTSEC-2026-0253` (`quarry-v2-ci.yml:231-237`). `scripts/rust-security-audit.sh` [C] uses one ignore plus a `cargo tree` check and is not wired anywhere (two policies).
- Coverage (CI, report-only): `cargo llvm-cov --workspace --all-features --fail-under-lines 80` (:304). `scripts/check_go_coverage.sh` [C] (merged profile over three modules) is not wired.
- Go (per module): `cd services/quarry-control && go test ./...`; `cd services/quarry-orchestrator && go test ./...`; `cd pkg/quarrycontracts && go test ./...`. Go directives: control and orchestrator `go 1.25.0`; quarrycontracts `go 1.22` [C]. The Makefile `test-go` [C] covers only control and orchestrator. Host runs of these Go commands are not verified by S9 [UNVERIFIED]; no cgo requirement is documented for Quarry Go.
- Makefile [C]: `build`, `test` (`cargo test --workspace` plus `test-go`), `lint`, `bench-browser` (`cargo run -p quarry-evals --bin quarry-browser-benchmark`, package name matches lab/evals/Cargo.toml:2 and :41-42). `test-vision` runs pytest in `services/quarry-vision-sidecar` [C]; no CI.
- Windows host: the Rust crates do not build on the host (boring-sys2 needs cmake, clang, perl, go, protoc) [MEM quarry-rust-tests-in-docker]. Use a `rust:1` container with `quarry-cargo`, `quarry-go` and `quarry-target` volumes and `bash -c` (not `bash -lc`); a full `cargo test -p quarry-core -p quarry-runtime -p quarry-edge` runs more than 10 minutes [MEM].
- macOS defaults: `apps/Ingestion Plane/Quarry-v2/Makefile` [C] lines 4-8 set `CORE_SYSTEM_CACHE_ROOT ?= /Volumes/Applikasjon/...`; `apps/Ingestion Plane/Makefile` [C] line 5 sets `CARGO_TARGET_DIR ?= /Volumes/Applikasjon/...`. Override both on Windows [READ].

### 2.2 Test inventory

- `crates/quarry-edge/tests/` [C unless noted]: cache_policy, change_webhook_proptest, profile_roundtrip, properties, sse_stream, zdr_behavior; `rerank_behavior.rs` [U] (not in git, so CI's `cargo test --workspace` does not run it); `zdr_claim_floor.rs` [M]; plus `fixtures/` and `support/`.
- `crates/quarry-runtime/tests/`: browser_egress_boundary [C], dom_summary_proptest [C], integration_runtime [C], publisher_integration [C], dp_retrieve_contract.rs [U], dp_rerank_contract.rs [U]. The two `dp_*` files read `tests/fixtures/dp-contracts/*` [U] (section 7).
- `crates/quarry-core/tests/` (change_history_proptest, contracts, properties), `crates/quarry-security/tests/security.rs`, `crates/quarry-transform/tests/transform.rs` [C].
- `crates/quarry-browser/tests/ssrf.rs` [C]: three `#[ignore]` tests at lines 38, 73, 101, each "Requires chromiumoxide + browser binary". CI line 211 runs with the feature but skips them [INFER]. One more `#[ignore]` line in `crates/quarry-runtime/src/browser_egress_proxy.rs` [C].
- `services/quarry-control`: 32 `_test.go` files [C].
- `services/quarry-orchestrator`: 6 test files (`cmd/orchestrator/main_test.go`; `internal/errs`; `internal/fleet`; `internal/schedules` x2; `internal/workflows`). `internal/jobs`, `internal/activities` and `internal/controlauth` each contain one `.go` file and no test file [READ]; this matches the CI comment (quarry-v2-ci.yml:97-104).
- `lab/evals` (package `quarry-evals`): binaries quarry-eval, quarry-bakeoff, quarry-provider-matrix, quarry-release-matrix, quarry-browser-benchmark, quarry-web-rerank-gate [M].

### 2.3 Prod-mode configuration (QUARRY_* and raw env)

| Setting | Read at (live) | Effect in prod | Committed value |
|---|---|---|---|
| `ENVIRONMENT` or `APP_ENV` (raw env, not EdgeConfig) | `main.rs:128-131`, `157-160` | `prod` or `production` enables the refusals below | Base compose `ENVIRONMENT: ${ENVIRONMENT:-dev}` (`docker-compose.yml:111`); production overlay `ENVIRONMENT: production` (`docker-compose.production.yml` [C] :29) |
| `QUARRY_EDGE_AUTH_DEV_BYPASS` (single underscore, raw env) | refusal `main.rs:132-144`; reader `auth.rs:258-261`; `change_routes.rs:403-406` | refuses start in prod (case-sensitive, H3) | base `${...:-0}` (`docker-compose.yml:98`); overlay `"0"` (:30) |
| `QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS` (EdgeConfig) | `main.rs:174-178` | refuses start in prod | base `${QUARRY_CROSS_PLANE_AUTH_DEV_BYPASS:-0}` (:108); overlay `"0"` (:36) |
| `QUARRY_EDGE__INTERNAL_SECRET` | `main.rs:161-172` (`InternalSigner::new`) | required in prod | base requires `QUARRY_INTERNAL_SECRET` (:68) |
| `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL` / `..._GRPC_URL` (`config.rs` [M] :124, :129) | `main.rs:183-196` (grpc needs the `grpc` feature), `197-201` bail | prod forces `require_browser_grants` (`config.rs` :132 field, main.rs:183-184) and needs one URL | not set anywhere committed (H2) |
| `QUARRY_EDGE__REQUIRE_DURABLE_RUNTIME` (raw) | `main.rs:179-182` | durability forced on (`1`, `true`, `TRUE`, `yes`) | not set |
| `QUARRY_EDGE__ARTIFACT_BACKEND` (from `QUARRY_EDGE_ARTIFACT_BACKEND`) | `main.rs:202-211`; match `main.rs:373-390` | prod bails on memory, `in_memory` or empty; accepts `fs`, `filesystem`, `s3` (`main.rs:371-378`); `s3` needs `QUARRY_EDGE__S3_BUCKET` | base default `memory` (`docker-compose.yml:115`); `.env.example:35` `memory`; overlay does not set it (H4 case bug) |
| `QUARRY_EDGE__DATABASE_URL` | `main.rs:212-215`, `durable_ready` `694-703` | required for durable history | base `:53` |
| Durable gate | `main.rs:694-708` (artifacts, profiles, receipts, frontier if `postgres-queue`, durable history, database_url); bail text at `:706` | prod bails when any backend is ephemeral | derived |
| EdgeConfig loader | `config.rs:334-346`; prefix `QUARRY_EDGE`, separator `__` (:336); on deserialize error falls back to `Self::defaults()` with an error log only (:338-341) | silent default fallback | committed at `config.rs` line 277 (message); live :339 |

Model Plane switches (`QUARRY_EDGE__LLM_CLASSIFY_INTENT`, `AUTOPROMPT`, `PAGE_TITLE_ENRICH`, `ANSWER_SYNTHESIS`, `AI_FORMATS`, `SEARCH_SUGGEST`, `MODEL_AUDIO`, `SEMANTIC_RERANK`): live defaults `0` (`docker-compose.yml` [M] :129-152); committed defaults `1` for AUTOPROMPT and SEMANTIC_RERANK (H6). Other switches read by config: `QUARRY_EDGE__RERANK_DEADLINE_MS` (live `config.rs:327-332`, uncommitted), `QUARRY_EDGE__RERANK_SHADOW`, `QUARRY_EDGE__HTTP3_ENABLED` (main.rs:1257 parser).

The edge env_file is `apps/Ingestion Plane/Quarry-v2/.env` [?]. Its variable names (values not read): `QUARRY_INTERNAL_SECRET`, `QUARRY_CONTROL_API_KEY`, `QUARRY_RUNTIME_AUTH_TOKEN`, `QUARRY_EDGE_AUTH_DEV_BYPASS`, `QUARRY_SERVICE_API_KEY`.

Standalone compose [C] (`deploy/compose/docker-compose.yml`): `ENVIRONMENT: "development"` (:75); `QUARRY_EDGE__ARTIFACT_BACKEND: "filesystem"` (:81); `QUARRY_EDGE_AUTH_DEV_BYPASS: ${...:-0}` (:96); `QUARRY_EDGE__INTERNAL_SECRET` required (:100); edge published `127.0.0.1:38080:8082` (:113).

### 2.4 Stack run

- Ingestion Plane: `cd "apps/Ingestion Plane" && make up` runs `docker compose --env-file ../../.env --env-file .env -f docker-compose.yml up -d --build --wait --wait-timeout 300` [C Makefile]. `make test-endpoints` checks edge `127.0.0.1:8082/health`, imports `:3025/ready`, integration `:3026/health`, finspo `:3130/health`, shipping `:3156/healthz`, plus Postgres, Dragonfly and Temporal. The memory recipe [MEM ingestion-plane-deploy-recipe] passes a longer env chain (Control and Application `.env.generated-secrets`, `Quarry-v2/.env`, the core `.env` files, and `apps/Ingestion Plane/.env.generated-secrets` last). The Makefile passes only two files, so required interpolations such as `QUARRY_CONTROL_API_KEY` may be unset [UNVERIFIED].
- Standalone: `deploy/scripts/dev-up.sh` [C] requires `deploy/compose/.env` [?, exists; not read]. Edge is then at `127.0.0.1:38080`; run `EDGE=http://127.0.0.1:38080 deploy/scripts/smoke.sh` (the default :8080 is wrong, H7).
- Prod config check without starting anything: `docker compose -f docker-compose.yml -f docker-compose.production.yml config -q` from `apps/Ingestion Plane` (command given in `Quarry-v2/deploy/compose/README.md` [C]; the README paths are Mac paths). `config -q` renders only; use it for H2 checks.

## 3. Model Plane (Rust workspace; Go browser-broker, capability-core, cost-core)

### 3.1 Commands

- Rust CI (from `apps/Model Plane/rust` [C]): `cargo fmt --all -- --check`; `cargo check --workspace`; `cargo check --workspace --all-targets`; `cargo test --workspace --no-fail-fast` (model-plane-rust-ci.yml:98-119). Workspace has 15 members (`rust/Cargo.toml` [C]: 10 `crates/mp-*`, 4 `services/*`, `clients/bridge-cli`).
- Golden drift (repo root): `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test` (model-plane-rust-ci.yml:95-96).
- Receipts job (uncommitted [M] :127-169): `DATABASE_URL=postgres://session_core:session_core@localhost:5432/session_core_test REQUIRE_CONTRACT=1 cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1`, Postgres 16 service, fails below 8 passing tests.
- Windows local: `cargo test -p <crate> --lib` [MEM model-plane-build-and-deploy].
- Go: `apps/Model Plane/go/go.work` [C] lists `services/browser-broker`, `services/capability-core`, `services/cost-core` (and others). Build and test per service from `apps/Model Plane/go`: `go build ./services/<name>/...` and `go test ./services/<name>/...` [MEM correction 2026-09-11]. `-race` needs cgo; the host has `CGO_ENABLED=0` [MEM]. No CI.
- Stack: `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build <service>` [MEM]. `compose.sh` [C] requires `MODEL_PLANE_RUNTIME_ENV_FILE` to be an absolute, canonical, regular, non-symlink file (function `canonical_runtime_file` in `compose.sh` [C]). Compose files: `apps/Model Plane/deploy/docker-compose{,.override,.production,.release,.bridges}.yml` [C].
- Python: `cd "apps/Model Plane/python/eval-lab-py" && python -m pytest tests/ -q --junit-xml=eval-selftest.xml` [CI eval-lab.yml:63-64]; `tests/` exists [READ].
- Proto: `apps/Model Plane/proto/buf.yaml` [C], `buf.gen.yaml` [C]; `buf.gen.yaml` regenerates Go bindings under `go/gen/` [MEM]. buf is not installed on the host [MEM].

### 3.2 Tests and ignored tests

- `#[ignore]` lines in the Rust workspace: 103 (grep count; some may be doc text). session-core: about 93 (`grpc.rs` 40 [M], `orchestration_grpc.rs` 16 [M], `retention_receipt.rs` 11 [U], `dreaming.rs` 7 [M], `run_service_grpc.rs` 5, `compaction_tests.rs` 4, `audit_publisher.rs` 3, `space_membership_nats.rs` 2, `memory_grpc.rs` 2, `memory_dedup.rs` 1, `memory_control.rs` 1, `approval_delivery.rs` 1; the rest [C] as far as checked). model-gateway: 7 (`stream_buffer.rs` 3; `tests/source_review_live.rs` [M], `tests/source_recheck_live.rs`, `tests/computed_evidence_local.rs`, `src/source_validation.rs` 1 each). execution-core: `tests/browser_agent_e2e.rs` [C] 3 (the live browser-agent e2e; CI comment says it stays skipped, model-plane-rust-ci.yml:114-117).
- `services/model-gateway/tests/e2e_invoke_chain_test.rs` [M, 288 changed lines uncommitted]: the `SessionMessage` literal at lines 1338-1343 uses `..Default::default()`, so `metadata` is not required [READ]. The CI comment claims the full suite is green at 2070 passed and 24 ignored (model-plane-rust-ci.yml:190-193) [CI comment, not re-run].
- Go: browser-broker 7, capability-core 88, cost-core 19 `_test.go` files [C]. No CI.

### 3.3 Surprises

- The Go services, which hold browser grants and cost, have no CI at all (H8).
- `apps/Model Plane/api/sdk/python/.github/workflows/python.yml` [C] never runs (nested, H8).
- `apps/Model Plane/rust/services/session-core/src/main.rs` [M] declares `mod retention_receipt;` (live :35), but the committed `session-core/src/main.rs` does not reference `retention_receipt` [C grep; session-core has no `lib.rs` at all, MISSING]. The committed tree therefore lacks the receipts code its job would run [INFER].

## 4. Frontend (verevonv3 SPA and apps/gateway)

### 4.1 Commands

- Hazard [MEM verevonv3-worktree-verification]: any `pnpm` command inside a git worktree prunes `pnpm-lock.yaml`; restore it after the last pnpm call. S9 ran no pnpm, and sweeps should not run pnpm in the live tree.
- SDK prerequisite (CI, [C] steps in verevonv3-ci.yml:171-177): `cd "apps/Ingestion Plane/Quarry-v2/sdks" && ./generate.sh && cd typescript && pnpm install --ignore-scripts && pnpm run build`. Then `pnpm --filter @verevon/remote-core run build` (:190).
- Gate (CI): `pnpm install --frozen-lockfile` then `pnpm verify` = `pnpm lint && pnpm typecheck && pnpm test && pnpm build` (`package.json` [M] :20; verevonv3-ci.yml:192-193). `lint` = `eslint .`; `typecheck` = `tsc -b`; `build` = `pnpm typecheck && vite build`; `test` = four `node --test` scripts under `apps/gateway/scripts` plus `vitest run` (`package.json` :11-13, :17-18).
- Vitest [C] `vite.config.ts` :77-82: jsdom, `setupFiles: ['./src/test/setup.ts']`; the comment at :82 says e2e specs must never be collected by vitest (exclusion line not read). 325 `*.test.ts(x)` files under `src` [count].
- Playwright (not in CI): `pnpm test:e2e` (`playwright test`). `playwright.config.ts` [C]: `testDir: 'tests/e2e'` (:68); `BASE_URL = E2E_BASE_URL || http://localhost:5199` (:44); `LOCAL_BASE_URL = LOCAL_E2E_BASE_URL || http://localhost:5173` (:46); projects `setup`, `e2e`, `local-setup`, `local`, `chat`, `product-recording`, `product-readiness` (:82-128). 27 spec files in `tests/e2e`.
- Gateway (CI): `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` (verevonv3-ci.yml:132-141). CLAUDE.md says `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` from the SPA root; CI omits `--all-targets` [INFER]. Gateway tests: `tests/ssrf_forward_not_fetch.rs`, `tests/sse_relay_no_allowlist.rs` [C].
- Security coverage gate: `apps/gateway/scripts/run-security-coverage.sh` [C] exits 2 unless `cargo-llvm-cov` is installed (:8-10). Not in CI; the `.test.mjs` files of the same folder run in `pnpm test`.
- Fabrication guard (CI, committed): greps for `fn org_id_from_headers`, `demo_accounts|demo_posts|SocialStore`, `Bearer dev-bypass` outside `src/shared/api/(http|sse)(.test).ts`, `x-org-id` and `x-verevon-org-id` in `STRIPPED_HEADERS` (middleware.rs [C]), domain reads of org headers, and `region default` (verevonv3-ci.yml:44-104).
- Security (CI): `verevonv3-security.yml` [C] runs `pnpm audit --audit-level=high` (:51) on push, PR and weekly.

### 4.2 Gateway prod-mode configuration [C] (`apps/gateway/src/config.rs`)

- `APP_ENV`: `dev_flags_allowed(APP_ENV)` gates dev escape hatches (:169 for the public origin); `dev_only_flag` refuses a dev flag when `dev_flags_allowed(APP_ENV)` is false (its doc comment names `production` and `prod`) and logs the refusal (:419-426); the predicate `dev_flags_allowed` itself was not read.
- `CONTROL_PLANE_ENFORCEMENT`: default `off`; accepts `off`, `permissive`, `strict` (:160-165).
- `VEREVON_PUBLIC_ORIGIN`: default `http://localhost:5173` (:166-168). `VEREVON_ALLOWED_ORIGINS`, `MODEL_GATEWAY_DEV_BEARER` (:205), `INTERNAL_API_KEY`, `AUTH_CORE_URL`, `SESSION_CORE_URL` are read in the same file.
- Model-token dev bypass: `Bearer dev-bypass` accepted only with the explicit gate (`domains/chat/shared.rs` [C] :363-371; tests :1400-1417).

### 4.3 Surprises

- The Playwright suite, including `browser-live.spec.ts` and `browser-workspace-{hitl,live-takeover,profile,session,zdr}.spec.ts` [C], has no CI gate (H8). It needs a live stack and storage state from `auth.setup.ts`.
- The demo job (uncommitted, H1) cannot run its first step as committed.
- [MEM verevonv3-worktree-verification, 2026-09-06] `pnpm typecheck` gave 21 errors and `pnpm lint` died (typescript-eslint 8.67 vs TS 7). The live `package.json` [M] now pins `typescript` to `npm:@typescript/typescript6@^6.0.2` and `typescript-eslint ^8.69.0`, so the memory is probably stale [UNVERIFIED].
- [MEM, same note] Vitest baseline on a pristine tree: 12 failed files, 30 failed tests, 8 errors. Not re-measured.

## 5. Control Plane (auth-core, org-core)

### 5.1 auth-core [C] (`apps/Control Plane/auth-core`)

- Commands (CI-equivalent): `pnpm install`; `pnpm run lint` (`node scripts/lint-ratchet.mjs`); `pnpm run build` (`nest build`); `REQUIRE_CONTRACT=1 pnpm test` (jest, rootDir `src`, `*.spec.ts`; 59 spec files). `package.json` [C] :8-21; auth-service.yml :59-74.
- Not in CI: `pnpm run lint:contract` (`node --test scripts/lint-ratchet.test.mjs`, package.json:16; file present [C]); the six `*.postgres.spec.ts` specs, gated by `CONTROL_LIFECYCLE_TEST_DATABASE_URL`. auth-service.yml lines 5-11 say these are "tracked as a follow-up".
- Coverage: `pnpm run test:cov` (`jest --coverage`, :19).
- Broken script: `test:e2e` and `test:smoke` (package.json :21, :23) point at `apps/auth/test/http-smoke.e2e-spec.ts`. That path is [MISSING] (provenance). The real files are `test/http-smoke.e2e-spec.ts` and `test/jest-e2e.json` [C]. A jest path pattern that matches no file cannot pass [INFER].
- Parity: `bash scripts/tests/principal-registry-parity.test.sh` from repo root [C] (auth-service.yml:79-81; hermetic sandboxes per its comment; not run by S9).
- Local Windows (MEM auth-core-local-testing, 2026-09-11): `pnpm install` once, then `npx jest <file>` and `npx tsc --noEmit -p tsconfig.json`. Baseline on HEAD: 15 failing tests in 3 suites (`deployment-contract.spec.ts` 8, `plane-service-principal.spec.ts` 6, `model-plane-token.controller.spec.ts` 1). Compare failing sets, not exit codes. `pnpm lint` dies with ENOBUFS in batch 1/8 [MEM].
- Fixtures [C] (`src/auth/fixtures/`): `inference-service-tokens.golden.json`, `quarry-token-claims.golden.json` (pin `.golden.sha256`), `data-plane-authorization-decision.golden.json`, `data-plane-service-token-claims.golden.json`, `auth-core-org-core-retention-posture.contract.json`, `cross-plane-token-requests.json` [M].

### 5.2 org-core [C] (`apps/Control Plane/org-core`)

- CI (org-service.yml): Postgres 16 service; `CREATE ROLE org_core_app NOLOGIN NOSUPERUSER NOBYPASSRLS ...`; `createdb org_test` and `org_lifecycle`; a `control_lifecycle_fixture` marker row (:74-93); `go test -v -p 1 ./...` with `REQUIRE_CONTRACT=1`, `TEST_DATABASE_URL`, `CONTROL_LIFECYCLE_TEST_DATABASE_URL` (:95-103); `go build -v ./cmd/server` (:106).
- Local: needs a Postgres. Without `TEST_DATABASE_URL` the RLS tests (`internal/database`) skip [READ comment :63-73]; with `REQUIRE_CONTRACT` unset, a missing vendored file skips silently. A local run can mirror CI: a Postgres 16 container with user and password `org_core` on port 5432, then the bootstrap in org-service.yml:74-93 and the `go test` env in :95-103 [CI-derived; not run].
- `go.mod` [C]: `module github.com/I-Dacosta/AquatiqCMS/apps/org-core`, `go 1.25`. CI uses setup-go `1.24` (org-service.yml:57). Toolchain behavior [UNVERIFIED].
- 36 `_test.go` files. No Makefile at org-core root. The user-core Makefile has `test-coverage` (`apps/Control Plane/user-core/Makefile` [C] :48-52).
- Root contains `index.html` (about 1.46 MB), `script.js`, `styles.css` [READ listing], `.idea/` and `.env*` files (not read).

### 5.3 Control stack [C]

- `bash scripts/run-control-plane.sh build <svc>` then `up -d --no-deps --build <svc>` from `apps/Control Plane` [MEM control-plane-build-and-deploy]. The script refuses the production overlay (`scripts/run-control-plane.sh` [C] :16-17). A bare `docker compose build` exits 0 and builds nothing [MEM].

## 6. integration-corev2 ([C] `apps/Ingestion Plane/integration-corev2`)

- Commands: `cd "apps/Ingestion Plane/integration-corev2" && go test ./...` (README.md [M] :173); `cargo test` for `services/webhook-normalizer-rs` (README.md :181). `go.mod` [C]: `module github.com/triodelab/integration-corev2`, `go 1.25.0`.
- Tests: 112 `_test.go` files [count]. No Makefile; a `Dockerfile` [C] and `deploy/codex/` [C].
- CI: no workflow in `.github` references `integration-corev2` (git grep, none). Its Go and Rust tests have no gate.
- Indirect gate only: `contract-goldens.yml` checks its embedded copy `internal/zdr/testdata/connection-zdr-capabilities.json` [C] and `.sha256` [C] (byte-identical, pass [RUN]), and lists it as a pending consumer of `zdr-posture-matrix.json` (Z-13) [RUN].
- [MEM integration-core-env-drift] Never re-add Control Plane service tokens to `integration-corev2/.env`.

## 7. Cross-plane golden gates

Mechanism ([READ] `scripts/tests/zdr-contract-sync.test.sh` [M], lines 48-419): (a) every GOLDENS row digest matches its pin and any sibling `.sha256` agrees; (b) every VENDORED copy is byte-identical to its golden; (c) the strict loader `scripts/tests/zdr-contract-schema.mjs` [M] accepts the ZDR goldens and the refusal catalogue; (d) every CONSUMERS row has a vendored copy in its tree, else "pending" (fails only under `REQUIRE_CONTRACT`). Rust-side pins: quarry-runtime `token_request_contract.rs` [M] and `dp_retrieve_contract.rs` [U] / `dp_rerank_contract.rs` [U] (named in the script's comments, lines 113-116, 223-231).

Run results on the live tree ([RUN], read-only, cwd `C:/dev/CoresSystem`):

- `node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"`: exit 0; tests 17, pass 17.
- `node --test scripts/tests/quarry-model-plane-switches.test.mjs`: exit 0; tests 8, pass 8.
- `bash scripts/tests/zdr-contract-sync.test.sh`: exit 0; "90 passed, 0 failed". Pending (non-REQUIRE mode): auth-core (Z-14), bff-gateway (Z-03, Z-19), conversation-core (Z-03), integration-corev2 (Z-13), all for `zdr-posture-matrix.json`.
- `node --test scripts/tests/zdr-contract-schema.test.mjs`: exit 0; tests 65, pass 65.

Not run (write or heavy): `zdr-contract-sync.test.sh --self-test` (copies to scratch repeatedly), `principal-registry-parity.test.sh`, every cargo, go, pnpm and docker command.

Untracked fixture set on which the sync gate depends (28 paths [U]):
- Model Plane: `apps/Model Plane/api/contracts/zdr-refusals.json` and `.sha256`; `apps/Model Plane/rust/crates/mp-contracts/tests/fixtures/zdr-posture-matrix.json` and `.sha256`; `apps/Model Plane/rust/services/model-gateway/tests/fixtures/auth-core/quarry-token-claims.golden.json` and `.sha256`; `apps/Model Plane/rust/services/model-gateway/tests/fixtures/zdr-posture-matrix.json` and `.sha256` (8).
- SPA: `apps/Frontend Plane/verevonv3/src/shared/api/contracts/zdr-refusals.json` and `.sha256` (2).
- Quarry-runtime: `crates/quarry-runtime/tests/fixtures/dp-contracts/retrieve-v1/` (SHA256SUMS, request, response, zdr-modes) and `.../rerank-v1/` (SHA256SUMS, ok, skipped-zdr, degraded-timeout) (8).
- Quarry lab: `apps/Ingestion Plane/Quarry-v2/lab/evals/fixtures/web-rerank/contracts/rerank-v1/` (SHA256SUMS, ok, skipped-zdr, degraded-timeout) (4).
- Data Plane retrieval-engine-rs: `tests/fixtures/contracts/rerank-v1/` (SHA256SUMS, ok, skipped-zdr, degraded-timeout) (4); `tests/fixtures/contracts/usage-ledger-v2/usage-envelopes.v2.json` and `.sha256` (2).

Also modified [M] and needed by the sync gate: the Quarry canonical manifest `crates/quarry-runtime/contracts/cross-plane-token-requests.json` and `.sha256`; the auth-core copy `src/auth/fixtures/cross-plane-token-requests.json` and `.sha256`. Canonical goldens `zdr-posture-matrix.json`, `infer-statuses.json` and `connection-zdr-capabilities.json` are [C].

Gap by design: `contract-goldens.yml` leaves `REQUIRE_CONTRACT` unset (:13-15), so the four pending posture-matrix consumers pass CI. Under `REQUIRE_CONTRACT=1` they would fail.

## 8. E2E and smoke inventory (in-app browser, scrape, crawl)

| Script or spec | Tag | Exercises | Runs | In CI |
|---|---|---|---|---|
| `apps/Frontend Plane/verevonv3/tests/e2e/browser-live.spec.ts` | [C] | In-app browser, live | `pnpm test:e2e`, live stack | No |
| `tests/e2e/browser-workspace-{hitl,live-takeover,profile,session,zdr}.spec.ts` | [C] | Browser workspace: approvals, takeover, profile, session, ZDR | same | No |
| `tests/e2e/cross-plane-smoke.spec.ts` | [C] | Cross-plane | same | No |
| `tests/e2e/product-*.spec.ts` (13 files), `agentic-subscription`, `chat-voice-mode`, `chat-workspace`, `inbox-ticketing-ai-first`, `real-authority-knowledge`, `space-shell`, `zdr-org-switch-gate` | [C] | Product flows | same | No |
| `apps/Frontend Plane/verevonv3/scripts/probe-product-demo.mjs` | [U] | Privacy and smoke probe of the demo, headless Chromium | demo job step (uncommitted) | Uncommitted only |
| `scripts/capture-product.mjs`, `verify-insights-refresh.mjs` [C]; `demo-live-check.mjs`, `scene-preview.mjs`, `capture-demo-poster.mjs` [U] | Manual capture and checks | manual | No |
| `apps/Ingestion Plane/Quarry-v2/scripts/start-stack.sh` | [C] | Crawl smoke of `https://example.com` through control, orchestrator, Temporal and edge | manual | No; :8080 default (H7) |
| `apps/Ingestion Plane/Quarry-v2/deploy/scripts/smoke.sh` | [C] | `POST /v1/scrape` example.com; lists jobs | manual | No; :8080 default (H7) |
| `apps/Ingestion Plane/smoke-test-integration-api.sh` | [C] | 12 integration-api checks | `make smoke-integration` | No |
| `apps/Ingestion Plane/Quarry-v2/crates/quarry-browser/tests/ssrf.rs` | [C] | SSRF against a browser (`chromiumoxide`) | ignored unless browser binary | No |
| `apps/Model Plane/rust/services/execution-core/tests/browser_agent_e2e.rs` | [C] | Live browser agent e2e | ignored | No (CI comment, model-plane-rust-ci.yml:114-117) |
| `apps/Model Plane/rust/services/model-gateway/tests/source_review_live.rs`, `source_recheck_live.rs` | [C] | Live source review and recheck | ignored | No |
| `apps/Model Plane/scripts/smoke-inference-core-metrics.js`, `smoke-tts.js`, `smoke-wave10.js` | [C] | Smoke | manual | No |
| `apps/Control Plane/auth-core/test/http-smoke.e2e-spec.ts` | [C] | HTTP smoke | `test:smoke` (stale path, section 5.1) | No |
| `apps/Ingestion Plane/Quarry-v2/lab/evals` (`quarry-browser-benchmark`, `quarry-web-rerank-gate`) | [M] | Browser benchmark; web-rerank gate | `make bench-browser` | No |
| `apps/Data Plane v2` docker smoke `scripts/smoke-test.sh --http-only` | [C] | Data Plane HTTP | `dataplane-v2-ci.yml` docker-smoke (:384-408; file [M]) | Yes (Data Plane; out of focus) |

## 9. Known pre-existing failures (source-tagged; not invented)

- Quarry: govulncheck reports stdlib CVEs fixed in Go 1.26.6, with the toolchain at 1.26.5 per the CI comment (the job uses `go-version: stable`; quarry-v2-ci.yml:41-48) [CI comment]; go coverage 41.6% (quarry-control) and 37.2% (orchestrator) against an 80% floor, 2026-09-08 [CI comment, :97-104]; rust-coverage never measured on a clean build [CI comment :113]; four advisories ignored in cargo-audit (:19-29).
- Model Plane: 304 pedantic clippy warnings and live dead_code warnings, so neither clippy nor `-D warnings` is gated (model-plane-rust-ci.yml:174-184) [CI comment]. Full suite claimed green at 2070 passed, 24 ignored (:190-193) [CI comment, not re-run].
- Model Plane Windows breaks (MEM model-plane-build-and-deploy): `inference-core/src/main.rs` `tokio::signal::unix`; `model-gateway/tests/e2e_invoke_chain_test.rs` missing `metadata`. Code now shows both fixed: `inference-core/src/main.rs` [C] has `#[cfg(unix)]` (line 109) and `#[cfg(not(unix))]` (line 120) variants; the e2e literal uses `..Default::default()` (section 3.2). Not re-run.
- auth-core: 15 failing tests in 3 suites on HEAD (MEM, 2026-09-11, Windows). `pnpm lint` ENOBUFS (MEM).
- SPA: pnpm typecheck and vitest baselines (MEM 2026-09-06, worktree-specific). Not re-measured on live tree.
- Windows host: Quarry Rust does not build natively (MEM); Go `-race` needs cgo (MEM).

## 10. Stale doc and memory claims

- MEMORY index line "no go.work" (model-plane-build-and-deploy) is stale: `apps/Model Plane/go/go.work` [C] exists. The note's own body already corrects it.
- MEMORY "Model Plane Windows breaks" is stale (section 9).
- MEMORY bootstrap-secrets-ledger index says the plane `.env` files were renamed and the script refuses to run; the body was reconciled on 2026-09-04 (ledgers restored). Live: `apps/Ingestion Plane/.env` exists [?, existence only].
- `apps/Ingestion Plane/Quarry-v2/deploy/compose/README.md` [C] line 95 and `docs/SELF_HOST.md` [M] line 60 and `docs/RUNBOOKS.md` [M] line 84 name `BROWSER_BROKER_GRPC`. No code reads that name (rust grep over the Ingestion tree). The code reads `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` (config.rs:129; main.rs:189-195). `SELF_HOST.md` line 130 names the right variable [CLAIM-FALSE for the first three].
- Quarry README and deploy docs use `/Volumes/Lagring/Triodelab/...` paths [C]; `deploy/compose/README.md` says the repo-root compose is quarantined legacy (root `docker-compose.yml` [C] exists).
- `CLAUDE.md` gateway command (`--all-targets`) differs from CI (section 4.1).
- `auth-service.yml` header (lines 5-11) says the wrong-path bug was fixed; `package.json` [C] still has the stale `apps/auth/...` test path (section 5.1).
- `scripts/tests/ownership-docs-contract.test.mjs` [C], `scripts/tests/coresystem-conformance-test.sh` [C], `scripts/tests/bootstrap_runtime_environment.test.sh` [C] and `scripts/tests/quarry-model-plane-switches.test.mjs` [U] are referenced by docs but by no workflow. The `ownership` test is cited in a plan file as "passes 18/18" [READ]. Run manually only.

## 11. Uncommitted in-flight changes on the verification surface

`git diff --stat` (live against afb83928), 18 files, 1089 insertions and 129 deletions:
- `scripts/tests/zdr-contract-sync.test.sh` [M] (95 changed lines): rows for QB-T09, QB-T10, usage-ledger and the SPA refusal catalogue. `scripts/tests/zdr-contract-schema.mjs` [M] (24 changed lines).
- `apps/Ingestion Plane/docker-compose.yml` [M] (50 changed lines): switch defaults 1 to 0 (H6); new switch lines; rerank deadline and shadow lines.
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs` [M] (188 changed lines): hunks at main.rs:47, 244-253 and 984-1463 (other features). The prod-check region (120-215) and the durable gate (683-708) are unchanged relative to committed (committed strings found at committed lines 137, 175, 198, 208, 218) [READ].
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/config.rs` [M] (187 changed lines): `rerank_deadline()` (:327-332), `defaults()` made `pub` (:348), tests added around :424. `from_env` body unchanged.
- `.github/workflows/model-plane-rust-ci.yml` [M] (50 insertions): `session-core-receipts` job (:127-169).
- `.github/workflows/verevonv3-ci.yml` [M] (55 insertions): `demo-product-build` job (:228-278).
- `.github/workflows/dataplane-v2-ci.yml` [M] (25 insertions): rerank contract and web-rerank jobs.
- `apps/Model Plane/rust/services/model-gateway/tests/e2e_invoke_chain_test.rs` [M] (288 changed lines).
- `apps/Ingestion Plane/Quarry-v2/docs/RUNBOOKS.md` [M] (90 insertions), `docs/SELF_HOST.md` [M] (24 changed lines), `docs/openapi.yaml` [M] (78 changed lines).
- `apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/token_request_contract.rs` [M] (17 changed lines); `crates/quarry-runtime/contracts/cross-plane-token-requests.json` [M] (5 changed lines); `crates/quarry-edge/tests/zdr_claim_floor.rs` [M] (10 insertions); `lab/evals/Cargo.toml` [M] (4 insertions).
- `apps/Ingestion Plane/integration-corev2/README.md` [M] (26 insertions); `apps/Model Plane/api/contracts/README.md` [M]; `apps/Frontend Plane/verevonv3/package.json` [M] (2 insertions).
Untracked in this area: the 28 fixtures and 6 gate files in sections 1 and 7.

## 12. Citation check

Scope: every path:line cited above. The cap of 25 applies per item and S9 has no item; 33 anchors were opened and read.

| # | Cited | Verdict | Note |
|---|---|---|---|
| 1 | quarry-v2-ci.yml:231-237 cargo-audit ignores | OK [C] | |
| 2 | quarry-v2-ci.yml:304, 326-332 coverage floors | OK [C] | report-only (continue-on-error) |
| 3 | quarry-docs-contract.yml:53 | OK [U] | not on main |
| 4 | model-plane-rust-ci.yml:95-96 golden sync | OK [C] (also in live [M]) | |
| 5 | model-plane-rust-ci.yml:127-169 receipts job | OK live [M]; absent in committed | uncommitted |
| 6 | contract-goldens.yml:66, 71, 74 | OK [C] | |
| 7 | verevonv3-ci.yml:141 `cargo test` | OK [M] (job committed) | CLAUDE.md differs (`--all-targets`) |
| 8 | verevonv3-ci.yml:228-278 demo job | UNCOMMITTED [M]; scripts [U] | cannot pass as committed |
| 9 | auth-service.yml:60, 63, 66, 71-74, 79-81 | OK [C] | |
| 10 | org-service.yml:57 setup-go 1.24 | OK [C]; mismatch with org-core go.mod `go 1.25` [C] | [UNVERIFIED] toolchain |
| 11 | buf.yml run line, baseline.binpb | FALSE as written [C]; baseline 0 bytes [C] | H5 |
| 12 | auth-core/package.json:21, 23 `apps/auth/...` | MISSING target | path [MISSING] in provenance |
| 13 | auth-core/package.json:16 lint:contract | OK [C]; not in CI | |
| 14 | verevonv3/package.json:20 `verify` | OK [M] | |
| 15 | playwright.config.ts:44, 46, 68 | OK [C] | |
| 16 | vite.config.ts:77-82 | OK [C] | |
| 17 | zdr-contract-sync.test.sh:48, 146, 253, 561-571 | OK live [M]; committed version smaller | section 11 |
| 18 | main.rs:128-134 dev-bypass refusal | OK live [M] (committed region shifted +1) | H3 |
| 19 | main.rs:183-201 grant gate | Audit "~182-200" DRIFTED to 183-201 | H2 |
| 20 | main.rs:694-708 durable_ready/bail | Audit "690-704" DRIFTED to 694-708 | |
| 21 | main.rs:369-373, 386-389 artifact match | OK [M] (region identical to committed) | H4 |
| 22 | config.rs:124, 129, 132 grant and require fields | OK [M] | |
| 23 | config.rs:334-341 from_env fallback | OK [M]; committed message at line 277 | |
| 24 | auth.rs:258-261 dev-bypass reader | OK [C] | H3 mismatch is in committed code |
| 25 | change_routes.rs:403-406 internal bypass | OK [C] | same semantics as auth.rs |
| 26 | docker-compose.yml:98, 108, 111, 115, 129, 152 | OK live [M]; defaults 129/152 DIFFER from committed (1) | H6 |
| 27 | docker-compose.production.yml:29-36 | OK [C] | no grant var, no artifact var |
| 28 | SELF_HOST.md:60, RUNBOOKS.md:84, deploy README:95 `BROWSER_BROKER_GRPC` | CLAIM-FALSE (no code reads it) | section 10 |
| 29 | smoke.sh:4; start-stack.sh:32-33 | FALSE vs published ports (38080 / 8082) | H7 |
| 30 | e2e_invoke_chain_test.rs:1338-1343 | OK [M]; memory "missing metadata" STALE | section 3.2 |
| 31 | inference-core/src/main.rs:109-121 cfg gating | OK [C]; memory "fails on Windows" STALE | section 9 |
| 32 | gateway config.rs:160-169, 419-426 | OK [C] | |
| 33 | quarry-browser ssrf.rs:38, 73, 101 `#[ignore]` | OK [C] | |

Also checked and correct: `apps/Ingestion Plane/Quarry-v2/Makefile` [C] lines 4-8 (macOS cache root); `apps/Ingestion Plane/Makefile` [C] line 5; `apps/Model Plane/go/go.work` [C]; `apps/Model Plane/scripts/compose.sh` [C]; `scripts/run-control-plane.sh` [C] :16-17. Corrected: the audit's proto/`openapi` paths: `apps/Frontend Plane/verevonv3/docs/openapi.yaml` is [MISSING]; the Quarry spec is `apps/Ingestion Plane/Quarry-v2/docs/openapi.yaml` [M]. The wrong base `apps/verevon-web/public/product-demo` is [MISSING]; the real bundle is `apps/Frontend Plane/verevonv3/apps/verevon-web/public/product-demo` [U].

Surfaces not covered: `apps/Data Plane v2` CI detail (out of focus); secret-scan.yml contents (not read); the content of any `.env` file (never read; names only where stated).

<!-- COMPLETE -->
