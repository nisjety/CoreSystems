# Model Plane

> **2026-07-20 correction:** the "no Model Plane containers, images, or
> listeners on the audited host" claim in the 2026-07-16 banner directly below
> is stale — it captured an early-in-the-day preflight failure on 2026-07-16
> that the same day's later rollout (see `MODEL_PLANE_STATUS.md`'s "Live local
> reconciliation — 2026-07-16 20:05 CEST") already superseded. A fresh
> `docker ps` on 2026-07-20 reconfirms the stack live and healthy (94 containers
> across CoreSystem, zero unhealthy/restarting), including `model-gateway` and
> `inference-core` with their gRPC listeners reachable. This is a runtime-health
> correction only — it does **not** mean the plane is production-ready; every
> MVP release blocker in `MODEL_PLANE_STATUS.md` (rollback artifact, tenant
> delegation, approval-continuation dispatcher, capability health attestation)
> remains open and unverified in this pass.

> **Current release status — 2026-07-16:** not production-ready and do not
> rebuild/deploy yet. There are no Model Plane containers, images, or listeners
> on the audited host, and no immutable rollback artifact. Source restores
> gateway/inference gRPC listeners with bind-aware readiness, protects cost and
> tool dispatch, has durable approval read-through plus claim/lease/retry/
> terminal primitives, and provides typed multi-step hybrid retrieval plus
> optional non-authoritative Letta tool-definition ranking. These changes are
> not live. Approval continuation still lacks a restartable descriptor,
> authenticated dispatcher, and successful execution receipt. Capability health
> has a global-only scoped attestation path but no configured/attested reporter.
> NATS has distinct named principals and no generic `model-runtime` principal,
> but no TLS/mTLS/workload identity. Signed ZDR cannot be downgraded at
> execution ingress and all unattested inference modalities fail closed before
> provider I/O; interactive all-ZDR identity still has no independently verified
> ZDR provider. Browser terminal statuses now fail/cancel honestly rather than
> falsely completing, but durable managed-run terminalization/reconciliation is
> still a P0 blocker. Artifact v2 requires signed verification, compatibility
> gates, an external allowlisted runtime environment, and a separate verified
> rollback artifact; no artifact exists.
> Read [MODEL_PLANE_STATUS.md](MODEL_PLANE_STATUS.md),
> [plane-audit-2026-07-16.md](docs/core-research/plane-audit-2026-07-16.md), and
> [grpc-safe-rebuild-decision-2026-07-16.md](docs/core-research/grpc-safe-rebuild-decision-2026-07-16.md).
> The 2026-07-11 and 2026-07-13 reports below are historical evidence, not
> current runtime claims.

Rust-first runtime and Go control shell for the CoreSystem AI reasoning layer (Layer 4). Replaces Model Plane v2 through incremental cutover.

> **Historical observation — 2026-07-11, not current runtime evidence:** Live
> audit then found all four Rust services healthy: `model-gateway` `/healthz` 200
> (`/health` 401 = auth active) on host :8080; `session-core`/
> `inference-core`/`execution-core` `/healthz` 200 on host
> :18081/:18082/:18083. The `model-gateway → execution-core` agent/tool loop
> and server-side HITL were real, not mocked. No Visma MCP was wired in source.
> The current 2026-07-16 host has no Model Plane runtime; do not use this
> historical result as a deploy/readiness claim.

## Historical source snapshot (2026-04-16)

The following section is retained for architecture history. Its compile/test
claims are not a substitute for the 2026-07-16 release gates or final candidate
verification.

### Verified now

- Rust workspace compiles: `cargo check --workspace` passes
- Rust tests pass: `cargo test --workspace` passes (0 failures)
- `model-gateway` gRPC invoke/invoke_stream no longer returns stubbed gRPC payloads
- `model-gateway` HTTP invoke delegates real gRPC call to inference-core; response wires input/output tokens and content
- `session-core` context assembly queries messages table; builds thread segments with token estimation
- Strict linting has been cleaned up in the Rust workspace (`clippy -D warnings` brought to green)

### Implemented by service

- `model-gateway`:
  - HTTP health/ready/metrics endpoints
  - Auth middleware with JWKS-backed JWT verification and optional dev bypass
  - Request normalization and rate limiting
  - SSE streaming path with envelope publishing
  - gRPC unary and streaming handlers with deterministic non-stub responses
  - HTTP invoke calls inference-core gRPC; wires real token counts and model response
- `session-core`:
  - gRPC APIs for thread/run/message/checkpoint lifecycle
  - Event replay stream and deterministic ordering strategy
  - Ordered multi-source context assembly from memory topics, recent thread messages, and prompt goal with budget enforcement
- `inference-core`:
  - Provider adapters/routing and fallback chain
  - Prompt cache and streaming bridge
- `execution-core`:
  - Runtime step loop, permission gates, hook checks, tool bridge

### Still using placeholder/stub behavior

- _(Verified 2026-07-11: none outstanding.)_ The orchestrator memory-consolidation and skill-promotion activities in [go/services/orchestrator-core/cmd/activities/activities.go](go/services/orchestrator-core/cmd/activities/activities.go) are now wired — `summarizeMemoryEntries` performs real per-thread consolidation, `QueryMemoryEntriesActivity`/`WriteConsolidatedMemoryActivity` call letta-bridge, and `RunPromotionGateActivity`/`UpdateRegistryActivity` make real gRPC calls into capability-core (`CheckSkillPromotion`/`PromoteSkill`). No `placeholder`/`TODO`/`stub` markers remain in that file.

## What Is Left In The Plan

Highest-impact remaining work before cutover: production hardening, integration validation, auth hardening.

1. Wire and validate cross-service contracts end-to-end (`model-gateway -> session-core -> inference-core -> execution-core`)
2. Replace orchestrator placeholder activities with real memory consolidation and skill promotion integrations
3. Execute and check off all verification gates in [docs/VERIFICATION.md](docs/VERIFICATION.md)
4. Validate migration and compat behavior (`v2` subject translation and dual-read/dual-write checks)

## Improvement Priorities To Consider

1. Auth and security hardening
- Verify JWT issuer/audience and key rotation via JWKS
- Add explicit claim validation (`org_id`, `user_id`, expiry, scopes)
- Add defense-in-depth tests for malformed/forged tokens and header abuse

2. Observability quality
- Add consistent request/run correlation IDs across HTTP and gRPC boundaries
- Ensure envelopes include enough diagnostics to debug replay and fallback decisions
- Add alert thresholds for stream latency and replay backlog

3. Deterministic behavior and idempotency
- Add stronger replay offset tests (`after_event_id`) across mixed event types
- Add duplicate-request/idempotency tests across retries and partial failures

4. Gateway behavior parity
- Keep HTTP and gRPC invoke semantics aligned (validation, model defaulting, token accounting)
- Add golden tests to assert parity between transports

5. Performance and scaling
- Stress-test SSE and gRPC streaming with concurrent tenants
- Validate rate limiter fairness and retry-after quality under burst load

## Edge Cases To Explicitly Test

### Request validation edge cases

- Empty or whitespace-only prompt content
- Extremely large content payloads near/over limits
- Missing model/provider hints and default model fallback behavior
- Invalid metadata payload shapes in gRPC requests

### Auth and tenancy edge cases

- Missing `Authorization` header
- Invalid Bearer format and expired token
- Valid token with missing `org_id`/`user_id` claims
- Cross-tenant replay or retrieval attempts

### Streaming edge cases

- Client disconnects mid-stream
- Final done chunk is missing/duplicated
- Partial chunk delivery then provider timeout
- Retry behavior after stream failure

### Replay and ordering edge cases

- Multiple events with identical timestamps, ordering by `event_id`
- Replay from `after_event_id` around boundary events
- Corrupted or missing payload fields during replay

### Failure-mode edge cases

- NATS unavailable during ingress/usage envelope publication
- Postgres transient failures during run/checkpoint writes
- Provider rate-limit responses and fallback exhaustion
- Hook/permission gate denial paths with correct status transitions

## Quickstart

```bash
# Development-only: start infrastructure + services. This is not a production
# or rollback command.
./scripts/compose.sh up -d

# Optional: enable MCP/LSP bridge overlay
MODEL_PLANE_BRIDGES=1 ./scripts/compose.sh up -d

# Regenerate proto stubs (requires buf CLI)
cd proto && buf generate

# Run Rust tests
cd rust && cargo test --workspace

# Run Go tests (per-module)
cd go
for d in pkg/envelope pkg/natsx pkg/idempotency; do (cd "$d" && go test ./...); done
for d in services/orchestrator-core services/capability-core; do (cd "$d" && go test ./...); done
```

Production or rollback traffic must never use the mutable workspace Compose
path. After all gates in the dated decision record pass, the only release entry
point is the signed artifact runner, for example:

```bash
MODEL_PLANE_PRODUCTION=1 \
MODEL_PLANE_RUNTIME_ENV_FILE=/managed/model-plane/runtime.env \
MODEL_PLANE_ARTIFACT_VERIFY_KEY=/managed/model-plane/artifact-public.pem \
./scripts/release-artifact.sh compose /accepted/artifact-v2 up -d
```

That command is intentionally blocked today: no accepted artifact or separate
rollback artifact exists. Artifact creation additionally requires release mode,
a managed signing key, the trusted verification key, and a non-secret
compatibility-gates evidence file. Runtime credentials stay in the external
runtime-env file and are validated against the artifact's key policy; they are
not bundled into the archive.

## Service Topology

| Service | Lang | HTTP | gRPC | Responsibility |
|---------|------|------|------|----------------|
| model-gateway | Rust | :8080 | :9090 | Public boundary — auth, normalization, rate limiting, SSE streaming, usage envelopes |
| session-core | Rust | :8081 | :9091 | Thread/run/checkpoint authority — context assembly, compaction, memory index, event replay |
| inference-core | Rust | :8082 | :9092 | Provider routing — LLM calls, retries, fallback chains, prompt cache, structured output |
| execution-core | Rust | :8083 | :9093 | Runtime loop — tool planning, permission gates, hooks, artifacts, subagents, step transitions |
| orchestrator-core | Go | :8084 | — | Temporal workflows — interactive run, deep task, wide research, memory consolidation |
| capability-core | Go | :8085 | :9097 | Skill registry — tool metadata, policy engine, model eligibility, budget checks |
| sandbox-manager | Go | :8086 | :9094 | Sandbox lifecycle — leases, TTL, snapshots, cleanup, quota |
| browser-broker | Go | :8087 | :9095 | Browser grants — local/cloud mode, per-session revocation, action audit |
| letta-bridge | Go | :8088 | :9096 | Memory bridge — block sync, retrieval tooling, graceful degradation |
| cost-core | Go | :8089 | :9098 | Cost/budget authority — token pricing, per-org budgets, spend accounting (verified 2026-07-11) |
| bridge-core | Go | :8091 | — | Client bridge — WebSocket transport for MCP/LSP bridge overlay (`MODEL_PLANE_BRIDGES=1`; see `bridges/mcp-bridge`, `bridges/lsp-bridge`) (verified 2026-07-11) |

> Rust HTTP ports above are container-internal; the compose stack host-publishes their health endpoints at :18081/:18082/:18083 (`/healthz`). MCP server registration/discovery/proxy is owned by **model-gateway** (`ListMcpTools`/`ProxyMcpTool`); `execution-core` only proxies through it. `sandbox-manager` and `browser-broker` also run today.

## Authority Rules

- **session-core** — single writer for threads, messages, runs, checkpoints, event log, memory index
- **capability-core** — single writer for agent definitions, tool metadata, skill bundles, routing policies
- **execution-core** — owns runtime step loop, delegates durable writes to session-core
- **orchestrator-core** — owns Temporal workflows, never executes per-step logic
- **inference-core** — stateless except prompt cache, no tool loop, no session authority
- **No Python on the hot path** — Python labs (graph-lab, eval-lab) are offline only

## Structure

```
proto/          Protobuf contracts (buf-managed)
rust/           Rust workspace (crates + services)
  crates/       mp-contracts, mp-ids, mp-events, mp-telemetry
  services/     model-gateway, session-core, inference-core, execution-core
go/             Go workspace (pkg + services)
  pkg/          envelope, natsx, idempotency
  services/     orchestrator-core, capability-core, sandbox-manager, browser-broker, letta-bridge
python/         Offline labs (graph-lab-py, eval-lab-py, provider-research-py)
deploy/         Docker Compose + OTEL collector config
docs/           Architecture, contracts, cutover, verification
```

## Infrastructure (docker compose)

Use `./scripts/compose.sh` from the Model Plane root. It pins the compose
files and passes the canonical ignored `deploy/.env`, so provider credentials
and local service overrides are loaded consistently without sourcing or echoing
them. Direct `docker compose` is unsupported because it can select a different
environment file based on the caller's working directory.

Immutable application images are produced only through release-artifact format
v2 from an isolated reviewed revision. The artifact snapshots application image
archives, digest-only image locks, Compose/NATS/OTEL/seccomp inputs, migrations,
cross-plane revision records, and a root checksum manifest. In release mode it
requires a managed signing key at build time and a trusted verification key at
verify/deploy time, plus a non-secret compatibility-gates file that attests
protocol, migration, live-authorization, approval-continuation, ZDR, and
separate rollback evidence. The artifact never copies runtime credentials:
deployment supplies a separately managed runtime-env file which must pass the
artifact's key allowlist. `verify` and `restore` do not rebuild images; the
artifact-contained `compose`/`deploy` path verifies the signed snapshot before
calling Docker. Direct workspace production Compose and mutable workspace locks
are refused.

No artifact was produced during the 2026-07-16 audit: the workflow correctly
refuses the current dirty source tree. A separately accepted rollback artifact
must exist and be restore-rehearsed before the first current deployment; the
candidate cannot be its own rollback.

- **PostgreSQL 16** — session-core metadata (threads, runs, checkpoints, events, memory_index)
- **NATS JetStream** — event bus (mp.v1.* subjects), compat adapter for v2 verevon.* subjects
- **MinIO** — artifact storage (org/{org_id}/thread/{thread_id}/run/{run_id}/artifact/{artifact_id})
- **Redis 7** — hot cache, idempotency dedup, rate limiter state
- **Temporal** — workflow orchestration (separate Postgres instance)
- **OTEL Collector** — distributed tracing aggregation

## Docs

- [ARCHITECTURE.md](docs/ARCHITECTURE.md) — service boundaries and authority rules
- [CONTRACTS.md](docs/CONTRACTS.md) — IDs, events, NATS subjects, storage conventions
- [CUTOVER.md](docs/CUTOVER.md) — v2 to new migration mapping
- [MODEL_PLANE_V2_PARITY.md](docs/MODEL_PLANE_V2_PARITY.md) — explicit v2 donor capability parity matrix
- [VERIFICATION.md](docs/VERIFICATION.md) — Phase F gate checklist
