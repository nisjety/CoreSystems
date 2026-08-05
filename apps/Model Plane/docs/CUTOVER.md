# Cutover Plan: Model Plane v2 to Model Plane

Planning note: this file remains the migration mapping doc for the original v2-to-Model-Plane cutover path.

For the expanded next-goal canon, use:

- `docs/GOAL.md`
- `docs/ROADMAP.md`
- `docs/PLAN.md`
- `docs/REFERENCE-MATRIX.md`

Phase E migration mapping table from old v2 subjects/APIs to new services.

## Current cutover status (2026-04-27)

| Step | Headline | Status |
|---|---|---|
| 1 | model-gateway in front of v2 | ✅ Rust gateway live; HTTP `/v1/invoke` + `/v1/invoke/stream` + JWKS auth + rate limiter + SSE |
| 2 | LLM routing to inference-core | ✅ provider fallback (OpenAI + Anthropic), `Infer` + `InferStream` gRPC live |
| 3 | session-core as thread/checkpoint authority | ✅ thread/run/checkpoint/message/event lifecycle live; transactional outbox green; `orchestration_store.rs` 🟡 module scaffolded — schema migrations + RPC wiring pending |
| 4 | execution-core owns runtime loop | ✅ step loop + hook gates + permission gate + secret scrub (PR-2) + SLO gates green (PR-8) |
| 5 | capability-core owns registry/policy | 🟡 6 RPCs live against in-memory registry; Postgres backing pending; 0 of 8 product registries populated (gap-analysis § 13.2) |
| 6 | orchestrator-core owns long-running workflows | ✅ 5 workflows + 10 activities + Temporal supervision; 🟡 12 orchestration RPC handlers Unimplemented (gap-analysis § 13.1) |
| 7 | Sidecars enabled progressively | 🟡 `sandbox-manager` + `browser-broker` lease/grant validate paths live, create paths Unimplemented pre-codec; `letta-bridge` runs against in-memory `memstore` — real Letta upstream not wired |

## Step 1: model-gateway in front of v2

| Old Path | New Path | Notes |
|----------|----------|-------|
| ai-core :8001 HTTP | model-gateway :8080 HTTP | Proxy through, normalize envelopes |
| ai-core :50051 gRPC | model-gateway :9090 gRPC | New proto contract |

## Step 2: LLM routing to inference-core

| Old Path | New Path | Notes |
|----------|----------|-------|
| llm-worker :8005 | inference-core (future) | Stateless provider routing |
| ai-core ChatService gRPC | model-gateway -> inference-core | New invoke/stream contract |

## Step 3: session-core as thread/checkpoint authority

| Old Path | New Path | Notes |
|----------|----------|-------|
| agent-core-v2 Postgres (runs, sessions) | session-core Postgres | Thread/run/checkpoint tables |
| `verevon.session.{id}.command` | `mp.v1.session.{id}.command` | Bridged by the Go compat adapter during cutover |
| `verevon.agent.run.{id}.event` | `mp.v1.run.{id}.event` | Bridged by the Go compat adapter during cutover |

## Step 4: execution-core owns runtime loop

| Old Path | New Path | Notes |
|----------|----------|-------|
| agent-core-v2 turn loop | execution-core (future) | Hook engine, permission, tool bridge |
| execution-core-v2 task graph | execution-core (future) | Step transitions, artifact writes |

## Step 5: capability-core owns registry/policy

| Old Path | New Path | Notes |
|----------|----------|-------|
| capability-core-v2 :8004 | capability-core :8083/:9092 | Go gRPC, skill registry |

## Step 6: orchestrator-core owns long-running workflows

| Old Path | New Path | Notes |
|----------|----------|-------|
| agent-core-v2 Temporal workflows | orchestrator-core Temporal | New workflow definitions |

## Step 7: Sidecars enabled progressively

| Old Path | New Path | Notes |
|----------|----------|-------|
| N/A | sandbox-manager :8084 | New capability |
| N/A | browser-broker :8085 | New capability |
| N/A | letta-bridge :8086 | Memory block sync |

## NATS Subject Migration

| Legacy Subject | New Subject | Adapter |
|---------------|-------------|---------|
| `verevon.agent.run.*.event` | `mp.v1.run.*.event` | `go/pkg/natsx/compat.go` |
| `verevon.session.*.command` | `mp.v1.session.*.command` | `go/pkg/natsx/compat.go` |
| `aqencia.reasoning.reasoning.started` | `mp.v1.ingress.run_started_compat` | `go/pkg/natsx/compat.go` |
| `aqencia.reasoning.reasoning.completed` | `mp.v1.ingress.run_completed_compat` | `go/pkg/natsx/compat.go` |
| `aqencia.reasoning.usage.recorded` | `mp.v1.ingress.usage` | `go/pkg/natsx/compat.go` |
| `aqencia.reasoning.decision.made` | `mp.v1.ingress.decision` | `go/pkg/natsx/compat.go` |
| `aqencia.reasoning.quota.exceeded` | `mp.v1.ingress.quota_exceeded` | `go/pkg/natsx/compat.go` |

Current runtime note: active legacy subject subscriptions are wired in `go/services/orchestrator-core/cmd/main.go`. Rust currently mirrors the subject constants in `rust/crates/mp-events/src/subjects.rs`, but does not own an active legacy compat subscriber yet.

Feature flags: per service path with canary by org/workspace. Compat adapters remain active until all consumers migrate.

## Phase 9 Staged Cutover — Foundation → Full Shell

Phase 9 expands the two-core shell (`ai-core` + `agent-core`) into the full capability surface while holding the foundation invariants: canonical `IdemPrefix = blake3("<service>|<event>|<thread>|<request>")` (fixture hash `fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637`), 4-file package layout, HTTP GET-only with `405 + Allow: GET` on non-GET, `unsafe_code = forbid` in every Rust crate, and legacy `verevon.*` / `aqencia.*` compat adapters remaining live until every consumer has migrated.

Scope ladder applies to every step: **run → thread → workspace → user → org (`triodelab`) → global**. Each step rolls forward one rung at a time with per-org canary flags before the next rung opens.

### Steps

1. **Shadow**. New path mounted behind `mp.cutover.<namespace>=shadow`. Requests still served by the legacy path; the new path is invoked in parallel for observational diff only. No client-visible behavior change.
2. **Canary (org)**. For `org="triodelab"` only, `mp.cutover.<namespace>=canary` routes real traffic through the new path while legacy path runs in shadow for diff. Rollback = flip flag back to `shadow`.
3. **Ramp (workspace → user → thread → run)**. Percentage ramps within `triodelab` across rungs. Compat adapters stay subscribed for every legacy subject during the ramp.
4. **Global canary**. `mp.cutover.<namespace>=canary_global` across all orgs; legacy path remains hot, diff telemetry continues.
5. **Promote**. `mp.cutover.<namespace>=promoted`. New path is authoritative. Compat adapters stay live; legacy HTTP paths emit `Deprecation` + `Sunset` headers.
6. **Decommission**. Only after N (≥ 14) days of zero legacy traffic and zero diff deltas. Legacy HTTP routes return `410 Gone`; compat NATS subscribers unsubscribed last.

Rollback at any step = revert flag to the prior value; no data migration is required because new and legacy paths coexist on the same stores until Step 6.

### HTTP namespace migration

| Old Path | New Path | Notes |
| --- | --- | --- |
| `/v1/orchestration/*` (legacy monolith) | `/v1/orchestration/*` on `orchestrator-core` | Owner: `orchestrator-core` (Go) + `execution-core` (Rust). Shell parity gate covers run/session/plan. GET-only on read endpoints; mutating calls use gRPC/NATS. |
| `/v1/capabilities/*` (legacy directory) | `/v1/capabilities/*` on `capability-core` | Owner: `capability-core`. Phases 0–8 already shipped the foundation; Phase 9 extends to full capability platform parity. |
| `/v1/ai/*` (legacy ai-broker) | `/v1/ai/*` on `ai-core` | Owner: `ai-core`. Multimodal breadth gate covers text/vision/audio/tool-use uniformly via `mp.v1.ingress.*_compat`. |
| `/v1/tasks/*` (legacy task-runner) | `/v1/tasks/*` on `task-core` | Owner: `task-core`. Tasks/cron/coordinator durability gate. Idempotency carried via canonical IdemPrefix. |
| `/v1/cron/*` (legacy scheduler) | `/v1/cron/*` on `task-core` | Owner: `task-core`. Cron schedules persist across failovers; replay via durable NATS consumer. |
| `/v1/bridge/*` (legacy integrations) | `/v1/bridge/*` on `bridge-core` | Owner: `bridge-core`. Bridge/voice/channel surfaces gate covers external integration parity. |
| `/v1/voice/*` (legacy voice-gw) | `/v1/voice/*` on `bridge-core` | Owner: `bridge-core`. Streaming surfaces remain on gRPC; HTTP is GET-only for status. |
| `/v1/channels/*` (legacy channel-hub) | `/v1/channels/*` on `bridge-core` | Owner: `bridge-core`. Channel fan-out parity under bridge/voice/channel gate. |
| `/v1/memory/*` (legacy mem-svc) | `/v1/memory/*` on `memory-core` | Owner: `memory-core`. Graph/wiki memory correctness gate. Writes via NATS only; HTTP GET for reads. |
| `/v1/knowledge/*` (legacy kb) | `/v1/knowledge/*` on `memory-core` | Owner: `memory-core`. Knowledge reads share GET-only contract; writers land on `mp.v1.memory.*`. |

### Proto contract migration

| Old Proto | New Proto | Notes |
| --- | --- | --- |
| `gateway.proto` (legacy) | `proto/model_plane/v1/gateway.proto` | Owner: `ai-core`. Covers ingress ack/route/decision envelopes. Compat gRPC stubs stay live through Step 5. |
| `sessions.proto` (legacy) | `proto/model_plane/v1/sessions.proto` | Owner: `orchestrator-core`. Session lifecycle + checkpointing; compact transport correctness gate asserts binary stability. |
| `execution.proto` (legacy) | `proto/model_plane/v1/execution.proto` | Owner: `execution-core`. Run/step execution with scrubbed checkpoint payloads (Phase 8). |
| `capabilities.proto` (legacy) | `proto/model_plane/v1/capabilities.proto` | Owner: `capability-core`. Capability registry + binding resolution; foundation already live. |

### Cutover invariants (must hold at every step)

- Idempotency: canonical `IdemPrefix` byte-stable; fixture hash unchanged.
- Transport: legacy `verevon.*` and `aqencia.reasoning.*` → `mp.v1.*` mappings from the table above remain active until Step 6.
- HTTP contract: non-GET on any `/v1/*` read endpoint returns `405` with `Allow: GET`.
- Rust safety: `unsafe_code = forbid`, edition 2021, resolver 2 across every crate.
- Observability: each namespace emits shadow-vs-live diff counters under `mp.cutover.<namespace>.diff{kind="…"}` until Step 6.
