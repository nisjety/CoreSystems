# Onboarding Guide: Model Plane

## Overview
Model Plane is CoreSystem's agent-execution plane: it owns reasoning, sessions/runs,
inference, tool-execution loops, capabilities, sandboxes, browser grants, and cost.
It's a from-scratch **Rust + Go** rebuild whose explicit north star (`docs/GOAL.md`)
is product-surface parity with a prior Python/TS "Model Plane v2" and with
Anthropic's Claude Code CLI, while selectively porting ideas from Nous Research's
Hermes agent, OpenAI's Codex, and several knowledge/memory projects. It is not a
port — it's a fresh design that reasons in Rust and controls in Go.

## Tech Stack
| Layer | Technology | Notes |
|---|---|---|
| Hot-path services | Rust | `model-gateway`, `inference-core`, `session-core`, `execution-core` |
| Durable-control services | Go | `orchestrator-core`, `capability-core`, `cost-core`, `sandbox-manager`, `browser-broker`, `letta-bridge`, `bridge-core`, `nats-provisioner` |
| Orchestration engine | Temporal | 5 workflows / 10 activities / 3 task queues as of the last audited count |
| Event bus | NATS/JetStream | `mp.v1.*` subjects; transactional outbox pattern from Postgres |
| Database | Postgres | append-only `events`, `checkpoints`, ULID-prefixed keys, single-writer-per-run |
| Contracts | Protobuf + shared crates | `proto/`, `rust/crates/mp-{events,eventlog,orchestration,ids,contracts,slo,telemetry,tokens,toon}`, Go `natsx`/`envelope`/`idempotency`/`jetstream`/`publisher`/`temporalreg` |
| Sandboxing | bubblewrap (`bwrap`) + Landlock/seccomp | Linux-gated; `rust/services/execution-core/src/sandbox.rs` |
| SDKs | Python + TypeScript | `api/sdk/python`, `api/sdk/typescript` |

## Architecture
Two loops exist **on purpose** and must not be merged:
1. **Plain-chat loop** — `model-gateway`'s `dispatch_tool`: an 18-arm read-only tool
   router that refuses anything side-effecting. This backs ordinary chat.
2. **Deployed-agent loop** — `execution-core`'s `execute_step_inner`: a full
   capability-policy → PreToolUse hook → sandboxed execute → PostToolUse hook
   pipeline, fail-closed. This backs autonomous/tool-using runs.

The boundary between them (the plain-chat router's refusal of side effects) **is**
the authority boundary — a 2026-08-14 proposal to unify them (HARN-1/2) was
deliberately withdrawn after measurement showed they share zero tools and unifying
would make an accidental authority cross-call compile. Three cross-service
invariants that were previously "enforced by comment only" (e.g., a deployed agent
must never get a smaller tool-call round budget than plain chat) are now asserted
at runtime in `cross_service_loop_contract.rs` and mutation-tested.

`orchestrator-core`'s Temporal workflows exist and are real, but are **not yet the
production dispatch path** — the live loop is `model-gateway → execution-core`
directly; Temporal workflows run only inside specific supervised flows
(`InteractiveRunSupervision`, `DeepTaskWorkflow`, etc.), not as the default.

## Key Entry Points
- **Public ingress**: `rust/services/model-gateway` — HTTP/gRPC invoke, `/v1/invoke` + `/v1/invoke/stream` (SSE with resume via `Last-Event-Id`), JWT/JWKS auth, rate limiting, budget checks.
- **Provider routing**: `rust/services/inference-core` — Anthropic (direct + Azure AI Foundry), OpenAI, Azure OpenAI; vision/speech/translation/doc-intelligence/video/realtime; `Infer`/`InferStream` gRPC.
- **Session/memory**: `rust/services/session-core` — threads, messages, runs, checkpoints, plans/todos, approvals, subagent lineage, routing policy; background compaction + "dreaming" loops.
- **Tool execution**: `rust/services/execution-core` — the sandboxed step loop, hooks, capability policy gate, bubblewrap sandboxing.
- **Capabilities/registries**: `go/services/capability-core` — models/skills/routing/safety registries live; tools/plugins/MCP registries still stubs as of the last audit.
- **Cost**: `go/services/cost-core` — durable usage ledger, budget checks; one of the most mature services (clean toolchain, no stubs).
- **Orchestration**: `go/services/orchestrator-core` — Temporal workflows/activities.

## Directory Map
```
docs/                  architecture, gap analysis, per-service research dives, ADRs
docs/core-research/    dated, source-grounded audits per service — check the date
proto/                 protobuf contracts (13 as of last count)
rust/services/         the 4 hot-path Rust services
rust/crates/           shared Rust libraries (mp-events, mp-orchestration, mp-toon, ...)
rust/clients/           bridge-cli and other Rust-side clients
go/services/           the 8 durable-control Go services
api/sdk/{python,typescript}/  generated client SDKs
bridges/               cross-plane bridge adapters
deploy/                docker-compose stacks
scripts/               release-artifact tooling, compose helpers
```

## Turn/Loop Lifecycle (deployed-agent path)
1. Request lands at `model-gateway` → auth, rate limit, budget check.
2. `execution-core.ExecuteStep` runs: capability policy check → `PreToolUse` hook → sandboxed execute (bubblewrap on Linux) → `PostToolUse` hook.
3. Side-effecting tool calls are dispatched **sequentially** within a round (no parallel tool-call dispatch yet — a known gap vs. Claude Code / deepseek-harness).
4. HITL-gated tools mint a durable `CreateApproval` and flip the run to `AwaitingApproval` rather than executing inline.
5. Results append to `session-core`'s append-only `events` log inside the same Postgres transaction as the domain write (transactional outbox → NATS).

## Conventions
- Conventional Commits; commit bodies frequently document *why an approach was rejected*, not just what shipped (see git log for `HARN-1/2`, `SKILL-2` dead-RPC removal).
- License-gated external adoption: check `docs/external-ideas-harvest.md` §1 before porting anything from a reference repo (Apache → vendor, MIT → port freely, AGPL → clean-room only, proprietary-leaked → shapes only, zero code).
- One canonical owner per capability — `docs/capability-ownership-matrix.md` is binding; duplication proposals need an explicit ruling there.
- Do not build a multi-backend abstraction for a single implementation — build it when the second real backend exists (see the `letta-bridge` deferral).

## Common Tasks
- **Run the stack**: `./scripts/compose.sh up -d` (from `apps/Model Plane`)
- **Rust tests**: `cd rust && cargo test --workspace` (or `-p <service>` for one)
- **Go tests**: `cd go/services/<service> && go test ./... && go vet ./...`
- **Coverage**: `cargo llvm-cov` (risk-based 80% target on changed critical paths)
- **Proto regen**: `buf generate`
- **Check current truth before trusting docs**: read `MODEL_PLANE_STATUS.md` first — it's dated and explicitly distinguishes source-verified from live-verified from blocked.

## Where to Look
| I want to... | Look at... |
|---|---|
| Add/modify a tool | `rust/services/execution-core/src/runtime_loop/` (agent loop) + `docs/capability-ownership-matrix.md` (check for an existing owner first) |
| Understand current production-readiness | `MODEL_PLANE_STATUS.md`, `MODEL_PLANE_ROADMAP.md` (repo root) |
| Understand what's real vs. stubbed | `docs/gap-analysis.md` §13 (Stub Replacement Inventory) |
| Compare against external agent harnesses | `docs/external-ideas-harvest.md`, and this repo's `../../claude-hermes-deepseek.md` |
| Change sandboxing | `rust/services/execution-core/src/sandbox.rs` (bwrap argv), `go/services/sandbox-manager` |
| Change cost/budget logic | `go/services/cost-core` |
| Change session/memory persistence | `rust/services/session-core/src/grpc.rs` |
| Change proto contracts | `proto/`, then `buf generate`, then update `rust/crates/mp-contracts` |
