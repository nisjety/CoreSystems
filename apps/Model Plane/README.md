# Model Plane

> **Current release status — 2026-07-13:** not production-ready. The running `model-gateway` and `inference-core` have green HTTP health but no required gRPC listeners, so chat/inference/query embedding are down. Live cost/session/capability boundaries remain unauthenticated, semantic memory is degraded, and no Visma runtime integration exists. Authenticated/tenant-scoped source restoration, exact audience issuance/callers, terminal-safe approval replay, and the ordinary invoke graph pass source tests but are not deployed. Approval outbox/cache recovery, durable browser ownership, background callers, capability dispatch authority, release-database proof, a verified ZDR provider route, and rollback/live gates remain incomplete. Read [MODEL_PLANE_STATUS.md](MODEL_PLANE_STATUS.md), [plane-audit-2026-07-13.md](docs/core-research/plane-audit-2026-07-13.md), and [grpc-safe-rebuild-decision-2026-07-13.md](docs/core-research/grpc-safe-rebuild-decision-2026-07-13.md) before using the historical 2026-07-11 claims below.

Rust-first runtime and Go control shell for the CoreSystem AI reasoning layer (Layer 4). Replaces Model Plane v2 through incremental cutover.

> **Verified 2026-07-11** — Live audit against running services and current source. All four Rust services healthy: `model-gateway` `/healthz` 200 (`/health` 401 = auth active) on host :8080; `session-core`/`inference-core`/`execution-core` `/healthz` 200 on host :18081/:18082/:18083 (host-published from container :8081/:8082/:8083 to dodge Ingestion-Plane/Expo collisions). The `model-gateway → execution-core` agent/tool loop is real and non-mocked: `execution-core` dispatches real tools (e.g. `shipping_tools.rs` → shipping-core `:3156`, live 200), and MCP tools are proxied through the gateway's registry (`ListMcpTools`/`ProxyMcpTool`). HITL is enforced server-side, not decorative (runtime_loop blocks provider writes without a durable approval reference; `book_shipment` is in `permission::is_risky_tool`). No Visma MCP is wired in source — see [MODEL_PLANE_DEEP_DIVE.md](MODEL_PLANE_DEEP_DIVE.md) and `docs/core-research/*` for the full MCP/Visma finding.

## Current Stack Status (2026-04-16)

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
# Start infrastructure + services
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
files and passes `--env-file .env`, so provider credentials and local service
overrides are loaded consistently. Direct `docker compose` from `deploy/` only
works on machines that have the ignored local `deploy/.env -> ../.env` symlink.

- **PostgreSQL 16** — session-core metadata (threads, runs, checkpoints, events, memory_index)
- **NATS JetStream** — event bus (mp.v1.* subjects), compat adapter for v2 velion.* subjects
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
