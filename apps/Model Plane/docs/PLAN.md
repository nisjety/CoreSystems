# Model Plane — Detailed Plan

This is the implementation-planning companion to `GOAL.md`, `ROADMAP.md`, and `REFERENCE-MATRIX.md`.

It assumes:

- product-surface parity with `apps/Model Plane v2`
- product-surface parity with `/Volumes/Lagring/Triodelab/claude-code-fork`
- selective enhancement from the external references in `REFERENCE-MATRIX.md`
- Rust hot path / Go durable control remains fixed

## Current baseline

The current codebase is not an empty scaffold. Planning starts from these real foundations.

**Rust hot path** (live, tested):

- `rust/services/model-gateway/src/http_routes.rs` — `/v1/invoke`, `/v1/invoke/stream`, `/healthz`, `/readyz`, `/metrics`. JWT/JWKS auth, rate limiting, header scrubbing, SSE.
- `rust/services/inference-core/src/grpc.rs` — `Infer`, `InferStream` RPCs. Provider fallback chain.
- `rust/services/inference-core/src/provider/{fallback,openai,anthropic}.rs` — provider adapters.
- `rust/services/session-core/src/grpc.rs` — `CreateThread`, `AppendMessage`, context assembly. Transactional outbox emits `THREAD_CREATED` / `MESSAGE_APPENDED` in-tx.
- `rust/services/execution-core/src/runtime_loop/mod.rs` — step loop, hook gates (pre-tool, post-tool, post-step, compaction-trigger), permission gate, subagent hook, secret-scrub seam, `ExecuteStep` + `ResumeRun` RPCs.

**Go control plane** (durable):

- `go/services/orchestrator-core/cmd/activities/activities.go` — 10 Temporal activities (`StartRun`, `ExecuteStepLoop`, `CompleteRun`, `FailRun`, `QueryMemory`, `SummarizeMemory`, `ValidateSkillBundle`, `PromotionGate`, `UpdateRegistry`, …).
- `go/services/orchestrator-core/cmd/workflows/` — 5 workflows (`InteractiveRunSupervision`, `DeepTaskWorkflow`, `MemoryConsolidationWorkflow`, `SkillPromotionWorkflow`, `WideResearchWorkflow`).
- `go/services/capability-core/internal/server/server.go` — RPCs declared, currently return `Unimplemented`.
- `go/services/sandbox-manager`, `go/services/browser-broker` — lease/grant gRPC services.
- `go/services/letta-bridge` — single-adapter HTTP/gRPC proxy stub.

**Cross-cutting** (live):

- 13 protos under `proto/model_plane/v1/` (gateway, sessions, execution, inference, capabilities, runs, browser, sandboxes, eventlog, events, memory, orchestration, ids).
- 7 Rust crates under `rust/crates/` (`mp-events`, `mp-eventlog`, `mp-orchestration` — pure data records for Plans/Todos/Approvals/RunEvents/SubagentLineage, `mp-ids`, `mp-contracts`, `mp-slo`, `mp-telemetry`).
- 7 Go pkgs under `go/pkg/` (`natsx`, `envelope`, `idempotency`, `jetstream`, `publisher`, `temporalreg`, `cibuf`).
- `python/mp-events-py` — pydantic v2 envelope + canonical idempotency hash, parity with Go and Rust.

**Off-hot-path labs**:

- `python/eval-lab-py`, `python/graph-lab-py`, `python/provider-research-py` — research utilities, not production workers.

**Verification status**: 18 / 30 foundation gates green per `VERIFICATION.md`. Phase F closed.

## Core decomposition — recommended structure

The Model Plane should be organized into clear, ownerable "cores". Two acceptable decompositions are provided; prefer the simpler, two-core layout unless there is a strong operational reason to separate concerns at a finer granularity.

Option A — five focused cores (explicit):

- `ai-core` — provider routing, inference primitives, modality routing, streaming, and model governance.
- `agent-core` — runtime and tooling for agents, skill lifecycle, subagent supervision, and agent policies (this includes the Agents Plane items described later).
- `research-core` — experiment orchestration, reproducible runs, artifact/dataset management, and lightweight GPU worker sandboxing.
- `training-core` — long-running training jobs, dataset pipelines, and training infra orchestration (may be colocated with `research-core` initially).
- `evaluating-core` — evaluation harnesses, batch evaluators, metrics collection, and promotion pipelines for candidate models/skills.

Option B — two top-level cores (recommended default):

- `ai-core` — contains the responsibilities from `ai-core`, `research-core`, `training-core`, and `evaluating-core` where appropriate, focused on model/experiment lifecycle and provider plumbing.
- `agent-core` — contains the Agents Plane runtime, AutoAgent tooling, skill registry integrations, and subagent orchestration.

Rationale: Option B reduces cross-service coordination, concentrates model lifecycle concerns under a single owner for faster iteration, and keeps agent runtime/behavior responsibilities clear and policy-driven under `agent-core`.

Reference: mirror `Model Plane v2` where topology and ownership were validated in practice: `/Volumes/Lagring/Triodelab/CoreSystem/apps/Model Plane v2`

### Option B mapping — language responsibilities (recommended default)

When using Option B (`ai-core` + `agent-core`) the following language responsibilities are recommended so each core uses the best tool for the job while preserving operational consistency:

- Go (primary): the canonical language for Model Plane control surfaces and durable services — public ingress (`model-gateway`), capability registry, orchestration, scheduling, API gateways, durable metadata services, and job coordinators. Prefer Go for ownerable services that require low-ops runtime and consistent observability.
- Rust (hot-paths): use for performance- and latency-critical components (inference hot paths, embedding generation, realtime extraction, memory/graph engines, runtime loops). Rust gives safe concurrency and low-latency guarantees for components that must be fast and memory-efficient.
- Python (ML & research): use for model training, experiment code, evaluation harnesses, dataprep pipelines, and research notebooks. Keep Python workloads as containerized, scheduled workers or batch jobs with clearly versioned artifacts and APIs so production control plane remains Go/Rust.

Mapping by responsibility (Option B):

- `ai-core`: implement control plane, routing, and durable APIs in Go; place high-throughput inference/hot-paths in Rust when needed; orchestrate training/experiment runs from Go but execute compute in Python workers.
- `agent-core`: implement agent runtime, supervisor, and orchestration in Go; use Rust for low-latency runtime primitives when necessary; AutoAgent/editor tooling and model-generation helpers can be Python (or TypeScript for UI) and run as isolated services.

Guidelines:

- Keep service contracts language-agnostic (gRPC/HTTP + events) so components can be implemented in the language best suited for the job.
- Run Python training/eval in containerized workers with artifact storage (object store + metadata) and expose results via the Go/Rust control plane.
- Use the event bus for cross-language communication and replayable audit trails.


## Phase 0 — Canonical truth · 🟡 in progress

**Scope**

- turn docs into a trustworthy source of truth
- separate "implemented now" from "target parity"

**Current state**

- Canonical docs in place: `GOAL.md`, `ROADMAP.md`, `PLAN.md`, `REFERENCE-MATRIX.md`.
- Foundation truth tracked in `gap-analysis.md` (foundation-gate view) and `VERIFICATION.md` (18/30 green).
- This `PLAN.md` now carries per-phase current-state and gap blocks.

**Remaining**

- Patch `README.md` to remove the outdated "orchestrator activities still placeholder" claim — `activities.go` has real downstream gRPC dispatch.
- Cross-link the stub-replacement inventory in [gap-analysis.md § 13](gap-analysis.md) from each phase.
- Keep `CUTOVER.md` step-status table in sync with reality.

**Deliverables**

- `GOAL.md`, `ROADMAP.md`, `PLAN.md`, `REFERENCE-MATRIX.md`
- patched `README.md`, `gap-analysis.md`, `CUTOVER.md`, and `VERIFICATION.md`

**Acceptance**

- no doc claims real implemented surfaces are placeholders when they are live
- current state and next target are clearly separated

## Phase 1 — Orchestration shell parity · ❌

**Current state**

- Data shapes only: `mp-orchestration` Rust crate defines pure-data records for `Plan`, `Todo`, `Approval`, `RunEvent`, `SubagentLineage`. No service backs them.
- `orchestration.proto` declares enums (`PlanState`, `PlanStepState`, `ApprovalKind`, `ApprovalState`, `TodoState`, `TodoPriority`) — no service RPCs for these domains.
- `runs.proto` exposes `GetRun`, `ListRuns`, `CancelRun` shape but `CancelRun` is not implemented end-to-end.
- `events.proto` includes `TODO_CREATED` / `TODO_TRANSITIONED` among 124 event types.
- `session-core` persists threads + messages with transactional outbox; does not persist plans/approvals/todos/run-events.
- `execution-core` exposes `ExecuteStep`, `ResumeRun`; has no run-event SSE for operators.
- `orchestrator-core` runs Temporal workflows but exposes no operator-facing API for runs/plans/approvals.
- `model-gateway` SSE only multiplexes streaming inference, not run events.

**Gap vs `Model Plane v2`**

- v2 ships full `/agent-runs`, `/agent-runs/{id}/stream`, plan-mode, approvals, hooks, coordinator; Model Plane has none of these endpoints.

**Gap vs `claude-code-fork`**

- fork ships `TodoWriteTool`, `EnterPlanModeTool`/`ExitPlanModeTool`, `Task{Create,Update,List,Get,Output,Stop}Tool`, `Team{Create,Delete}Tool`, `ScheduleCronTool`, `RemoteTriggerTool`, `AskUserQuestionTool`, `SendMessageTool`, `EnterWorktreeTool`/`ExitWorktreeTool`. Model Plane exposes none of the corresponding APIs.

**Scope**

- define full orchestration API surface comparable to `Model Plane v2`

**Deliverables**

- canonical endpoint groups for:
- runs
- todos
- plans
- approvals
- event history
- SSE event stream
- resume/cancel
- control actions
- team/subagent lifecycle
- documented durable records for:
- plan state
- approval state
- run event history
- subagent lineage

**Owner split**

- `session-core`: run/session/plan/approval/event durability
- `execution-core`: step execution status and runtime transitions
- `orchestrator-core`: long-running coordination and recovery

**Acceptance**

- docs specify owner service, state model, and verification gate for each orchestration surface

**Cross-cutting infra — Event Sourcing & Consensus (Model Plane / Orchestration)**

**Deliverables**

- Authoritative append-only event log (Postgres append-only table or event store) and streaming fanout (NATS/JetStream or Kafka) for low-latency consumers.
- Consensus/write-authoritative layer for critical writes (Raft-backed service, etcd, or equivalent).
- Event schema versioning, idempotency keys, retention and replay tests, and standard event names (for example: `wiki.page.created`, `agent.run.completed`, `skill.registered`, `experiment.completed`).

**Owner split**

- `session-core` / `orchestrator-core`: write canonical run and plan events.
- `model-gateway`: emit provider-level events and ingress events.
- Knowledge Plane and analytics: subscribe and index (consumer role).

**Acceptance**

- Producers write durable, versioned events; consumers can replay reliably; wiki and agent actions are indexed incrementally; approval/plan gating can pause/resume runs via event-driven hooks.

## Phase 2 — Capability platform parity · ❌ (highest-leverage gap)

**Current state**

- `capability-core` returns gRPC `Unimplemented` for all RPCs ([go/services/capability-core/internal/server/server.go](../go/services/capability-core/internal/server/server.go)).
- `capabilities.proto` defines `ListCapabilities` + `GetCapability` only — no per-registry RPCs.
- `letta-bridge` exists as a single, un-catalogued memory adapter.
- `inference-core::router::config` is hardcoded — no model registry, no routing-policy table.
- `SkillPromotionWorkflow` runs in `orchestrator-core` but has no skill registry to promote into.

**Registry coverage today: 0 of 8.** All eight required registries (tools, commands, skills, plugins, MCP, models, routing policies, memory adapters, safety policies) are absent.

**Gap vs `Model Plane v2`**

- v2 `capability-core/app/{catalog,plugins,mcp,routing,safety,memory}/` plus `agent-core/app/{commands,tools,skills}/` cover the eight registries with tables in v2 migrations (`mcp_servers`, `mcp_oauth_tokens`, `tools`, `plugin_catalog`, `routing_policies`, `memory_adapter_catalog`, `model_configs`, `capability_audit_log`).

**Gap vs `claude-code-fork`**

- fork `src/tools/` (43 tools), `src/commands/` (50+ commands), `src/skills/`, `src/plugins/`, `src/services/mcp/`, `MCPTool`, `McpAuthTool`, `ListMcpResourcesTool`, `ReadMcpResourceTool` — operator-discoverable today; Model Plane has no equivalents.

**Scope**

- expand `capability-core` from registry/policy RPCs to full product-level capability shell

**Deliverables**

- documented registries for:
- tools
- commands
- skills
- plugins
- MCP servers
- models
- routing policies
- memory adapters
- safety policies
- scoped enablement rules:
- agent
- workspace
- user
- org
- global
- promotion and reconciliation flows for skills and plugins

**Owner split**

- `capability-core` owns metadata, policy, scope, enablement, and discovery
- `execution-core` consumes execution-ready definitions

**Acceptance**

- every user-visible capability surface maps to `capability-core`

## Phase 3 — Plan mode, approvals, hooks, recovery · ❌

**Current state**

- No run-mode field on any current run record. Planning is not a phase.
- `Approval` data record + `ApprovalKind` / `ApprovalState` enums exist; no API to request, grant, or deny.
- `execution-core` has hook *gates* (pre-tool, post-tool, post-step, compaction-trigger) but no hook *registry*, no scope, and no `pre-plan` hook.
- Recovery is implicit in `InteractiveRunSupervision`; no per-state policy is documented for planned-but-unapproved, approval-paused, interrupted, or resumed runs.

**Gap vs `Model Plane v2`**

- v2 ships `agent-core/app/plan_mode.py`, `approvals/`, `hooks/`, `session_recovery.py`, `recovery.py` — all absent in Model Plane.

**Gap vs `claude-code-fork`**

- fork ships `EnterPlanModeTool`, `ExitPlanModeTool`, `src/hooks/`, plan-mode-aware permission gating; absent here.

**Scope**

- make planning and approval gating explicit at the product level

**Deliverables**

- documented run modes:
- execute
- plan
- reactive
- research
- approval checkpoints for risky actions and tool calls
- hook lifecycle definition:
- pre-plan
- pre-tool
- post-tool
- compaction/post-step
- recovery model for:
- planned but unapproved runs
- approval-paused runs
- interrupted runs
- resumed runs from latest durable state

**Reference inputs**

- `Model Plane v2` plan mode + approvals
- `claude-code-fork` permission and plan gating patterns

**Acceptance**

- docs make plan mode and approval behavior user-facing, not internal-only

## Phase 4 — Tasks, cron, and coordination · ❌

**Current state**

- No durable task service. Closest primitive is `mp-orchestration::Todo`, which is not a long-running task.
- No cron / scheduled-work surface. No `RemoteTrigger` equivalent.
- `execution-core` has a subagent **hook** (`subagent.rs`); no **coordinator** (no spawn / message / summarize / reconcile API).
- `SubagentLineage` data record exists; not materialised into any store.
- No Hermes runtime, no AutoAgent creator, no AgentSummary analytics pipeline.

**Gap vs `Model Plane v2`**

- v2 `agent-core/app/{tasks,cron,coordinator,messaging,trajectory,analytics}/` + tables `agent_tasks`, `agent_cron_tasks` — absent here.

**Gap vs `claude-code-fork`**

- fork `Task{Create,Update,List,Get,Output,Stop}Tool`, `Team{Create,Delete}Tool`, `ScheduleCronTool`, `RemoteTriggerTool`, `SendMessageTool`, `services/AgentSummary/`, `services/autoDream/`, `tasks/{RemoteAgentTask,InProcessTeammateTask,DreamTask,LocalShellTask,LocalAgentTask}/` — none of these surfaces exist in Model Plane.

**Scope**

- durable work management and team/subagent coordination

**Deliverables**

- task model:
- create
- update
- assign
- block
- complete
- scheduled work model:
- cron
- remote triggers
- recurring workflows
- team/subagent coordination model:
- spawn
- message
- summarize
- reconcile
- attach results back to parent run

### Agents Plane (part of `agent-core`) — runtime & tooling

**Scope**

- runtime and tooling for agent execution, skill creation, and feedback loops.

**Deliverables**

- Hermes runtime (agent supervisor + learning loop) for subagent orchestration and scheduled automations.
- AutoAgent (creator/editor) for zero-code agent/workflow creation and CLI/editor tooling.
- `AgentSummary` + analytics pipeline (skill/run metrics, success rates, cost) and dashboards.
- Integration points: Agents read/write the `LLM Wiki` and the Knowledge Graph; agents publish skills into `capability-core`.

**Owner split**

- `execution-core`: runtime/isolation, subagent spawn/management.
- `capability-core`: skill registry, policy, enablement.
- `orchestrator-core`: coordination, recovery, and event-driven control.
- `analytics` / `session-core`: AgentSummary aggregation, metrics, and audit logs.

**Acceptance**

- Hermes runs in dev and can spawn subagents.
- AutoAgent can create a registered skill that passes a baseline eval and is discoverable via `capability-core`.
- Agent actions and skill lifecycle events are emitted to the event bus for indexing and audit.

**Preferred languages & placement**

- `agent-core` primary runtime and control surfaces: **Go** (orchestration, durable APIs, scheduling, capability integration).
- Low-latency runtime primitives and embedding/extraction engines: **Rust** (where performance and memory-safety are required).
- Agent tooling, model-generation helpers, and rapid prototyping: **Python** (containerized workers) or TypeScript for UI/editor components.

**Reference inputs**

- `Model Plane v2` tasks and cron
- `claude-code-fork` coordinator and team tools
- `openclaw` webhook/cron/channel routing
- `hermes-agent` scheduling and toolset ergonomics
- `autoresearch` bounded autonomous loops

**Acceptance**

- docs describe a durable owner for tasks, cron, and multi-agent coordination

## Phase 5 — Multimodal AI breadth · ❌ (chat/completions only)

**Current state**

- `model-gateway` exposes `POST /v1/invoke` and `POST /v1/invoke/stream` only.
- `inference-core::provider::fallback` knows chat/completions; no other modality.
- No `/v1/ai/*` namespace; no per-modality routing; no artifact / result-store contract for large outputs.

**Gap vs `Model Plane v2`**

- v2 `ai-core` exposes 9 modality groups (`/api/v1/{chat,completions,images,speech,translate,language,documents,video,realtime}` + analyzer pipeline). Model Plane covers chat/completions only.

**Gap vs `claude-code-fork`**

- the fork is operator-side and does not itself host modalities, but its bridge protocol expects multimodal artifacts to be addressable.

**Scope**

- expand current invoke/inference foundations into a broad AI API surface

**Deliverables**

- documented public/runtime surfaces for:
- chat
- completions
- images
- speech
- translation
- documents
- video
- realtime
- runtime routing and provider fallback strategy per modality
- artifact and result-store expectations for large outputs

**Owner split**

- `model-gateway`: public ingress
- `inference-core`: provider routing, fallback, and streaming
- `execution-core`: multimodal task orchestration when tied to agent runs

**Acceptance**

- docs explain how broad modality support is achieved without restoring a Python gateway monolith

### Model/Research Plane — automated experiments & training workers

**Scope**

- Controlled experiment and training infrastructure for reproducible model exploration and small-scale automated research jobs.

**Deliverables**

- `autoresearch` pilot and a GPU worker sandbox (containerized worker template).
- Experiment scheduler, artifact & dataset store, experiment tracking (reproducible runs, metrics).
- Integration: publish experiment lifecycle events (e.g., `experiment.completed`) to the event store; optionally surface high-quality artifacts to the Knowledge Plane.

**Owner split**

- `model-gateway`: job ingress and auth.
- `execution-core` / `research-core`: job orchestration, scheduling, and sandboxing.
- `inference-core`: provider routing for large-scale evals and fallback logic.

**Acceptance**

- Launch a reproducible experiment job, capture artifacts and metrics, and emit `experiment.completed` events consumable by analytics and the Knowledge Plane.

**Preferred languages & placement**

- `ai-core` control & durable APIs: **Go** (model-gateway, schedulers, artifact registries).
- Experiment/training code and evaluation harnesses: **Python** (containerized workers, notebooks, training scripts).
- High-throughput inference and dataplane runtimes: **Rust** (hot paths, embedding pipelines, graph engines).


## Phase 6 — Product clients and operator shell · ❌

**Current state**

- Zero operator-facing client surfaces. No CLI/TUI, no IDE bridge, no remote-session manager, no voice ingress, no channel ingress.
- `/v1/bridge/*`, `/v1/voice/*`, `/v1/channels/*`, `/v1/remote/*` namespaces do not exist.

**Gap vs `claude-code-fork` (primary reference)**

- `src/bridge/` (31 files, JWT, framing protocol, IDE↔CLI), `src/remote/` (WebSocket sessions), `src/server/` (direct-connect bridge), `src/voice/`, `src/coordinator/`, `src/skills/`, `src/plugins/`, `src/hooks/`, `src/screens/`, `src/components/` (Ink + React TUI), `src/commands/` (50+ slash commands), `services/{settingsSync,remoteManagedSettings,oauth}` — Model Plane has zero equivalents.

**Gap vs `Model Plane v2`**

- v2 `agent-core/app/{voice,messaging,lsp}/` covers voice + Slack/email/webhook ingress + LSP bridging.

**Scope**

- document operator-facing shell surfaces beyond bare HTTP/gRPC

**Deliverables**

- CLI/TUI/Web shell targets
- IDE bridge target
- remote session target
- voice target
- channel/gateway target
- session transfer and remote-control expectations

**Reference inputs**

- `claude-code-fork`
- `openai/codex`
- `openclaw`
- `hermes-agent`

**Acceptance**

- client surfaces are documented as first-class product features with clear backend ownership

## Phase 7 — Knowledge plane · ❌

**Current state**

- No graph memory, no wiki memory, no provenance/contradiction handling, no knowledge lint.
- `letta-bridge` is a single proxy adapter; not a knowledge layer.
- Multi-scope memory non-negotiable from `GOAL.md` §6 unmet: only `thread` scope materialised; `run`/`workspace`/`user`/`org`/`global` absent.

**Gap vs `Model Plane v2`**

- v2 `agent-memory` table + agent-core memory consolidation; not a graph/wiki layer per se but production memory storage exists. Model Plane has no equivalent.

**Gap vs `claude-code-fork`**

- fork `src/memdir/` (8 files), `services/SessionMemory/`, `services/extractMemories/`, `services/teamMemorySync/` — all absent here.

**External references** — `graphify`, `GraphRAG`, `LLM Wiki`, `logseq` (see `REFERENCE-MATRIX.md`).

**Scope**

- add long-lived graph and wiki knowledge layers on top of raw events and memory records

**Deliverables**

- graph memory layer:
- extracted entities
- relationships
- provenance labels
- confidence labels
- wiki memory layer:
- topic pages
- concept/entity pages
- summaries
- knowledge logs
- contradiction handling:
- reinforce
- weaken
- qualify
- contradict
- create
- knowledge lint passes:
- orphan pages
- stale claims
- missing cross-links
- missing pages

**Reference inputs**

- `graphify`
- `GraphRAG`
- `LLM Wiki`
- `logseq`

**Owner split**

- Rust: extraction, synthesis, graph/wiki materialization, context consumption
- Go: scheduling, policy, retention, scope control, durable metadata

**Acceptance**

- docs define graph/wiki layers as additive knowledge artifacts, never replacements for source-bearing truth

## Phase 8 — Token efficiency and ergonomics · ❌

**Current state**

- `execution-core` has a compaction-trigger hook gate; no compactor implementation.
- No compact transport; no operator verbosity profiles; no response-style profiles.

**Gap vs `Model Plane v2`**

- v2 `prompt_cache.py`, `token_budget.py` — absent here.

**Gap vs `claude-code-fork`**

- fork `services/compact/`, `outputStyles/`, caveman-family skills — absent here.

**Scope**

- improve operator and runtime token efficiency without hiding system state

**Deliverables**

- optional compact transport for large structured outputs
- optional compact memory and context summaries
- operator-facing verbosity profiles
- response-style profiles distinct from runtime logic

**Reference inputs**

- `TOON`
- `caveman`

**Acceptance**

- compact formats are documented as optional, benchmarked, and reversible

## Phase 9 — Verification and cutover · 🟡 partial

**Current state**

- 18 / 30 foundation gates green per `VERIFICATION.md`. Phase F closed (cancel propagation, SAGA compensation, approval-durability fixture).
- Foundation gap items (PR-2 secret scrub, PR-3 browser-broker post-revoke, PR-8 context-assembly SLO) tracked in `gap-analysis.md`.
- No parity gates yet exist for orchestration shell, capability platform, multimodal breadth, knowledge correctness, tasks/cron/coordinator durability, bridge/voice/channel surfaces, or compact transport.

**Gap**

- `VERIFICATION.md` and `CUTOVER.md` cover the foundation only; the new shell gates listed in Deliverables below need definition and wiring.

**Scope**

- extend verification and cutover docs beyond current foundation gates

**Deliverables**

- new verification gates for:
- orchestration shell parity
- capability platform parity
- multimodal breadth
- graph/wiki memory correctness
- tasks/cron/coordinator durability
- bridge/voice/channel surfaces
- compact transport correctness
- staged cutover plan from current foundation services to full shell

**Acceptance**

- `VERIFICATION.md` and `CUTOVER.md` cover the new shell, not just the original migration foundation work

## Cross-cutting gaps (independent of phase)

These gaps cut across phases and should be tracked at the top level so they are not forgotten while phase work proceeds.

1. **HTTP namespace coverage.** Of the planned `/v1/orchestration/*`, `/v1/capabilities/*`, `/v1/ai/*`, `/v1/tasks/*`, `/v1/cron/*`, `/v1/bridge/*`, `/v1/voice/*`, `/v1/channels/*`, `/v1/memory/*`, `/v1/knowledge/*` — only `/v1/invoke` + `/v1/invoke/stream` are live (1 of 10 namespace groups).
2. **Proto evolution debt.** `gateway.proto`, `sessions.proto`, `execution.proto`, `capabilities.proto` need additive evolution. New protos required for tasks, cron, plans-as-services, approvals-as-services, bridge, voice, channels, knowledge.
3. **Scope model propagation.** `run / thread / workspace / user / org / global` is documented but not propagated through capability and policy lookups. Today only `thread` is materialised end-to-end.
4. **Event schema gaps.** `events.proto` has 124 event types but is missing named events for capability/task/cron/subagent/wiki/graph/experiment domains: `skill.registered`, `experiment.completed`, `wiki.page.created`, `agent.run.completed`, `mcp.server.enabled`, `plugin.installed`, `cron.fired`, `task.assigned`, `subagent.spawned`, `graph.entity.extracted`, `knowledge.contradiction.detected`.
5. **Cost / analytics.** `Model Plane v2` ships a dedicated `cost-core` (run_costs, analytics_events, NATS `verevon.cost.*`); fork ships `services/analytics/`. Model Plane has neither. `model-gateway` does not emit `usage.{org}.llm` events.
6. **Multi-scope memory.** `GOAL.md` non-negotiable §6 makes six scopes first-class. Only `thread` scope is materialised.
7. **Python research/training plane.** Existing `python/` projects are off-hot-path labs; not the autoresearch + experiment-tracking workers Phase 5b describes (no scheduler, no artifact registry integration, no `experiment.completed` emission).

## Recommended execution sequencing

Phase numbers above are for traceability. The ordering with the highest unblock factor is:

1. **Phase 2 — Capability platform** first. Nothing else can be enabled per-scope, registered, promoted, or routed without it.
2. **Phase 1 — Orchestration shell** on top of Phase 2. Storage in `session-core` (plans/approvals/todos/run-events), API in a Go service (extend `model-gateway` Go side or add `orchestration-core`), Temporal-driven recovery in `orchestrator-core`.
3. **Phase 4 — Tasks / cron / coordination** rides on Phase 1 + Phase 2.
4. **Phase 5 — Multimodal breadth** can run in parallel with shell work once Phase 2 owns the model registry.
5. **Phase 3 — Plan mode + approvals** is a thin behavioural layer once Phase 1 stores plans + approvals.
6. **Phase 6 — Operator shell** can start in parallel with 1–2 because the bridge/voice/channel ingress just needs gateway-stable contracts.
7. **Phase 7 — Knowledge plane** consumes events emitted by 1–5.
8. **Phase 8 — Token efficiency** last; reversible optimisation layer.
9. **Phase 9 — Verification & cutover** gates land incrementally as each phase ships.

## Contract and interface additions to document

### Proto evolution

- `gateway.proto`
- `sessions.proto`
- `execution.proto`
- `capabilities.proto`

### Future HTTP namespaces

- `/v1/orchestration/*`
- `/v1/capabilities/*`
- `/v1/ai/*`
- `/v1/tasks/*`
- `/v1/cron/*`
- `/v1/bridge/*`
- `/v1/voice/*`
- `/v1/channels/*`
- `/v1/memory/*`
- `/v1/knowledge/*`

### Scope model

- run
- thread
- workspace
- user
- org
- global

## Cross-cutting acceptance rules

### Documentation

- new canonical docs become the entry point for planning
- existing docs link back to canonical docs

### Ownership

- every planned feature has one clear owner service
- every client-facing feature has one backing contract story

### Architecture

- no planned feature collapses Rust and Go responsibilities
- no reference repo becomes the new architecture by imitation

### Safety

- planning, approvals, permissions, hooks, and memory provenance remain explicit in docs
