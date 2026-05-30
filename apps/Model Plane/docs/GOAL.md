# Model Plane — Goal

## North star

Ship a **Rust-and-Go agent platform** that reaches product-surface parity with:

- `apps/Model Plane v2`
- `/Volumes/Lagring/Triodelab/claude-code-fork`

And selectively incorporates the strongest ideas from:

- `graphify`
- `GraphRAG`
- `LLM Wiki`
- `autoresearch`
- `openclaw`
- `hermes-agent`
- `openai/codex`
- `logseq`
- `TOON`
- `caveman`

The target is **parity of capabilities, APIs, workflows, and operator experience**. It is not a language-port or file-for-file clone.

## Current implementation truth (as of this revision)

Foundations that are real and tested:

- Rust `model-gateway` — live HTTP/gRPC invoke paths, JWT/JWKS auth, rate limiter, header scrubbing, SSE for `/v1/invoke/stream`.
- Rust `inference-core` — unary + stream gRPC, provider fallback, OpenAI/Anthropic adapters, router config.
- Rust `session-core` — thread/run/message/checkpoint lifecycle, context assembly, transactional outbox emitting `THREAD_CREATED`/`MESSAGE_APPENDED` in-tx.
- Rust `execution-core` — step loop, permissions, hook gates, subagent hook, compaction-trigger hook, secret-scrub seam, `ExecuteStep`/`ResumeRun` RPCs.
- Go `orchestrator-core` — 5 Temporal workflows + 10 activities across 3 task queues (`mp-session`, `mp-inference`, `mp-execution`); cancel/SAGA compensation; approval durability fixture.
- Go `capability-core` — registry + policy RPCs declared, currently return `Unimplemented` (registries not yet populated).
- Go `sandbox-manager`, `browser-broker`, `letta-bridge` — lease/grant primitives + proxy stubs.
- Cross-cutting — 13 protos, 7 Rust crates (`mp-events`, `mp-eventlog`, `mp-orchestration`, `mp-ids`, `mp-contracts`, `mp-slo`, `mp-telemetry`), 7 Go pkgs (`natsx`, `envelope`, `idempotency`, `jetstream`, `publisher`, `temporalreg`, `cibuf`), `mp-events-py` parity.
- Verification: 18/30 gates green per `VERIFICATION.md` (Phase F closed).

What is **not yet implemented** (the gap this GOAL targets):

- No `/v1/orchestration/*`, `/v1/capabilities/*` (HTTP), `/v1/tasks/*`, `/v1/cron/*`, `/v1/bridge/*`, `/v1/voice/*`, `/v1/channels/*`, `/v1/memory/*`, `/v1/knowledge/*` namespaces.
- No durable backing for plans, todos, approvals, run-event history, subagent lineage. `session-core/src/orchestration_store.rs` (371 LOC) is scaffolded but schema + RPC wiring pending; `orchestrator-core/internal/orchestration/handlers.go` exposes 12 RPCs that all return `Unimplemented` (see gap-analysis.md § 13.1).
- `capability-core` runs against an **in-memory** registry + policy engine; Postgres backing pending; 0 of 8 product registries (tools, commands, skills, plugins, MCP, models, routing, memory adapters, safety) populated.
- `letta-bridge` runs against an **in-memory `memstore`**; no real Letta upstream wired.
- No multimodal breadth beyond chat/completions: images, speech, translate, documents, video, realtime are absent.
- No tasks/cron/coordinator service; no scheduled work or team APIs.
- No operator shell: CLI/TUI, IDE bridge, remote sessions, voice, channel ingress are all missing.
- No knowledge plane (graph memory, wiki memory, provenance, contradiction handling).
- No compact-transport / verbosity-profile layer.
- Multi-scope memory (`run`/`thread`/`workspace`/`user`/`org`/`global`) — only `thread` scope is materialized.
- Infrastructure described in ARCHITECTURE / CONTRACTS not yet integrated: Redis hot cache, MinIO artifact bucket, unified docker compose.

The next goal is to expand the verified foundations into a full product shell that closes every item above. The canonical stub-replacement tracker lives in [gap-analysis.md § 13](gap-analysis.md).

## Non-negotiables

1. **Rust owns hot path.** Public invoke, streaming, session assembly, execution loop, provider routing, multimodal execution, and knowledge extraction stay Rust-first.
2. **Go owns durable control.** Workflows, registries, policy, scheduling, plugin reconciliation, org-scoped configuration, and long-running orchestration stay Go-first.
3. **Parity means product surface.** Match behavior and operator workflows from `Model Plane v2` and `claude-code-fork`; do not recreate their Python or TypeScript layouts as the new source of truth.
4. **Contracts stay additive.** Existing proto and event contracts evolve forward; no silent breaking rewrite.
5. **Provenance beats convenience.** Graph memory, wiki memory, summaries, and compaction artifacts never replace source-bearing records.
6. **Memory is multi-scope.** Run, thread, workspace, user, org, and global scopes are first-class.
7. **Plan mode is explicit.** User-facing planning, approval gating, resume/cancel, and auditability are core shell features.
8. **Token efficiency is a feature, not a hack.** Compact transport and terse operator modes are supported, but correctness and traceability win ties.

## What parity means here

### 1. Agent shell parity

- Runs, todos, plans, approvals, resume/cancel, control actions, and event streams.
- Team/subagent coordination, messaging, task management, and cron/scheduled work.
- Hooks, permissions, checkpoints, recovery, and session continuity.

### 2. Capability platform parity

- Tool registry
- Command registry
- Skill registry and promotion
- Plugin system and marketplace-style reconciliation
- MCP server registry and scoped enablement
- Model registry and routing policy
- Safety and memory adapter surfaces

### 3. AI runtime parity

- Chat
- Completions
- Images
- Speech
- Translation
- Documents
- Video
- Realtime

### 4. Product shell parity

- CLI/TUI/Web operator surfaces
- IDE bridge
- Remote sessions
- Voice interactions
- Channel/messaging ingress
- First-class automation surfaces

### 5. Knowledge and memory parity

- Graph-backed memory
- Persistent wiki-style knowledge artifacts
- Provenance-aware extraction vs inference
- Contradiction tracking
- Search, synthesis, and compaction loops that improve over time

## Ecosystem-derived enhancements

### Knowledge systems

- `graphify`: multimodal knowledge graph extraction, extracted-vs-inferred provenance, graph exports, codebase understanding.
- `GraphRAG`: graph-based indexing and retrieval for long-lived knowledge spaces.
- `LLM Wiki`: persistent writable wiki between raw sources and query-time answers.
- `logseq`: graph-oriented private knowledge UX and operator-facing knowledge workspace concepts.

### Agent product shell

- `claude-code-fork`: command/tool/plugin/skill/memory/task/voice/bridge/coordinator breadth.
- `Model Plane v2`: orchestration shell, approvals, hooks, MCP, skills, plugins, tasks, cron, broad capability API, multimodal AI breadth.
- `openclaw`: gateway, channels, voice, live canvas, multi-agent routing.
- `hermes-agent`: shared command model across CLI and messaging, toolsets, MCP, memory, scheduling.
- `openai/codex`: local-first coding-agent shell, context-file model, lightweight operator UX.
- `autoresearch`: bounded autonomous research loops and program-driven experimentation.

### Token-efficiency layer

- `TOON`: compact structured transport for large JSON-like payloads.
- `caveman`: operator-facing terse response modes and token-aware output profiles.

## Success criteria

### Product surface

- Canonical docs define owner service, API namespace, and acceptance gate for every missing parity feature.
- `Model Plane` has a single coherent shell story across orchestration, capability, runtime, memory, and client surfaces.

### Architecture

- No planned feature violates Rust hot path / Go durable control ownership.
- No new Python or TypeScript monolith becomes a required runtime source of truth.

### Knowledge plane

- Graph memory and wiki memory are documented as first-class layers with provenance and contradiction handling.
- Context assembly is documented as able to consume graph/wiki/memory artifacts without replacing source-backed records.

### Operator experience

- CLI, bridge, voice, channels, tasks, and cron are documented as product surfaces, not afterthoughts.
- Plan mode, approvals, and recovery are explicit and user-facing.

### Documentation quality

- `GOAL.md`, `ROADMAP.md`, `PLAN.md`, and `REFERENCE-MATRIX.md` become the canonical planning set.
- Existing `README.md`, `gap-analysis.md`, `CUTOVER.md`, and `VERIFICATION.md` no longer contradict current implementation truth or the next-goal direction.

## Recommended sequencing (highest leverage first)

The phases in `ROADMAP.md` and `PLAN.md` are numbered for clarity, not strictly for execution order. The ordering with the highest unblock factor is:

1. **Capability platform (Phase 2) first.** Nothing else can be enabled per-scope, registered, promoted, or routed without it.
2. **Orchestration shell (Phase 1)** on top of capability — durable plans/approvals/todos/run events backed by `session-core` and exposed by a Go orchestration API (extend `model-gateway` or add a new orchestration service). The data records already exist in `mp-orchestration`; they need an owner and an API.
3. **Tasks / cron / coordination (Phase 4)** — rides on the orchestration record types and capability scoping.
4. **Multimodal breadth (Phase 5)** — mechanical once `capability-core` owns the model registry; can run in parallel with shell work.
5. **Plan mode + approvals (Phase 3)** — thin behavioral layer on top of the orchestration store.
6. **Operator shell (Phase 6)** — bridge/voice/channel ingress; can start in parallel with 1–2 once gateway contracts stabilise.
7. **Knowledge plane (Phase 7)** — consumes events emitted by 1–5.
8. **Token efficiency (Phase 8)** — last; reversible optimisation layer.
9. **Verification & cutover (Phase 9)** — parity gates added incrementally as each phase lands.
