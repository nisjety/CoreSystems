# Model Plane — Roadmap

This roadmap tracks the path from the current Rust+Go foundation to product-surface parity with `apps/Model Plane v2` and `claude-code-fork`. Each phase carries an explicit **Current state** and a **Next deliverables** block so the gap is unambiguous.

Sequencing recommendation: see `GOAL.md` § "Recommended sequencing". Phases are numbered for traceability, not strict execution order.

> External-source adoptions per phase (codex/hermes/pi/daytona/claude-code → concrete changes, with license gates) are catalogued in [external-ideas-harvest.md](external-ideas-harvest.md). Phase tags there: P2/P3 (codex traits, plan-mode), P4 (hermes cron, claude-code coordinator), P6 (channels/bridge/TUI), P7 (hermes learning loop), P8 (deferred schema).

Status legend: ✅ done · 🟡 partial · ❌ missing

> **⚠️ 2026-05-30 status reconciliation.** A code-grounded audit found this roadmap badly understated reality — most phases below were marked ❌ but are in fact LIVE (all 8 multimodal groups, orchestration shell, tasks/cron, TOON, graph/wiki routes, MCP/plugin registries). Building off the old ❌ flags is the root cause of duplicate systems. **The authoritative current-state + one-owner-per-capability map is [capability-ownership-matrix.md](capability-ownership-matrix.md).** Phase headers below are corrected; per-phase prose is being reconciled.

---

## Phase 0 — Reconcile truth and canon · 🟡

**Current state**

- Canonical docs exist: `GOAL.md`, `ROADMAP.md`, `PLAN.md`, `REFERENCE-MATRIX.md`.
- Foundation truth captured in `gap-analysis.md` (foundation-gate view, 18/30 green) and `VERIFICATION.md`.

**Remaining**

- Patch `README.md`, `CUTOVER.md`, and `gap-analysis.md` so no doc claims live surfaces are placeholders.
- Add a per-phase product-surface gap section (this roadmap is the start; mirror in `PLAN.md`).

---

## Phase 1 — Orchestration shell parity · ✅ LIVE (corrected — matrix §1)

**Current state**

- Data records exist in `mp-orchestration` (Plans, Todos, Approvals, SubagentLineage, RunEvent) and enums in `orchestration.proto` — pure data, no IO.
- 124 event types in `events.proto` including `TODO_CREATED` / `TODO_TRANSITIONED`.
- `session-core` has thread/message lifecycle + transactional outbox; no plan/approval/todo persistence.
- `execution-core` exposes `ExecuteStep` and `ResumeRun` gRPCs only. No cancel; no run-event SSE for operators.
- `orchestrator-core` Temporal workflows exist (`InteractiveRunSupervision`, `DeepTaskWorkflow`, `MemoryConsolidationWorkflow`, `SkillPromotionWorkflow`, `WideResearchWorkflow`) but no operator-facing API exposes runs, todos, plans, approvals, or event history.

**Next deliverables**

- Choose an owner Go service (extend `model-gateway` Go side or add `orchestration-core`) for the `/v1/orchestration/*` namespace.
- Persist Plans, Todos, Approvals, RunEvents, SubagentLineage in `session-core` Postgres with migrations.
- New protos / RPCs:
  - `orchestration.proto`: `CreatePlan`, `GetPlan`, `ListPlans`, `ApprovePlanStep`, `RejectPlanStep`.
  - `runs.proto`: extend with `CancelRun`, `StreamRunEvents`.
  - new `todos.proto`: full CRUD + `TransitionTodo`.
- Run-event SSE multiplexer in `model-gateway` consuming JetStream.
- Recovery policy for planned-but-unapproved, approval-paused, interrupted, and resumed runs.

**Owner split**

- `session-core` — durability of plans, approvals, todos, run-event history, subagent lineage.
- `execution-core` — step transitions, hook firing, cancel propagation.
- `orchestrator-core` — coordination, recovery, Temporal-driven retries.
- `model-gateway` — public ingress + SSE relay.

---

## Phase 2 — Capability platform parity · 🟡 registries LIVE; consolidation pending (matrix §4.3)

**Current state**

- `capability-core` returns gRPC `Unimplemented` for all RPCs.
- Skill promotion **workflow** runs in `orchestrator-core`, but there is no skill registry to promote into.
- `letta-bridge` is a single memory-adapter proxy stub; not catalogued.
- Inference router config is hardcoded; no model registry.

**Next deliverables — eight registries, all owned by `capability-core`**

| Registry | Storage | Scope dimensions | Notes |
|---|---|---|---|
| Tools | Postgres | agent / workspace / user / org / global | Mirror `Model Plane v2` `catalog/`; reference: `claude-code-fork/src/tools/` (43 tools) |
| Commands | Postgres | same | Slash-command registry; reference: fork `src/commands/` (50+) |
| Skills | Postgres + artifact ref | same | Receives output of `SkillPromotionWorkflow` |
| Plugins | Postgres + manifest | org / global | Install, enable, reconcile |
| MCP servers | Postgres + OAuth tokens table | agent / workspace / user / org / global | Stdio / HTTP / SSE transports |
| Models | Postgres | org / global | Per-org defaults, routing-policy targets |
| Routing policies | Postgres | org / global | Cost / latency / capability strategies |
| Memory adapters | Postgres | org / global | Catalog of adapters (letta, vector stores, etc.) |
| Safety policies | Postgres | agent / workspace / user / org / global | Tool-call allowlists, output filters |

- Scope-resolution helper exposed via gRPC (`ResolveCapability(scope, capability_id)`) and HTTP `/v1/capabilities/*`.
- Promotion + reconciliation flows wired to event bus (`skill.registered`, `plugin.installed`, `mcp.server.enabled`, `routing.policy.updated`).

**Owner split** — `capability-core` owns metadata, policy, scope, enablement, discovery. `execution-core` consumes execution-ready definitions only.

---

## Phase 3 — Plan mode, approvals, hooks, recovery · 🟡 LIVE in-memory at gateway; durable wiring pending (matrix §4.1)

**Current state**

- No run-mode field on any current run record.
- Approval enums exist (`ApprovalKind`, `ApprovalState`); no API to request/grant/deny.
- `execution-core` has hook gates (pre-tool, post-tool, post-step seams, compaction-trigger hook); no hook registry per scope; no `pre-plan` hook (plan phase doesn't exist yet).
- Recovery is implicit in `InteractiveRunSupervision`; not documented per state.

**Next deliverables**

- Run modes: `execute`, `plan`, `reactive`, `research` — modeled in `runs.proto` + persisted with the run record.
- Approval API + state machine: `RequestApproval` / `GrantApproval` / `DenyApproval`, durable in `session-core`.
- Hook registry in `capability-core` (Phase 2 dependency) with scope and lifecycle: `pre-plan`, `pre-tool`, `post-tool`, `post-step`, `pre-compact`, `post-compact`.
- Documented recovery matrix:
  - planned-but-unapproved → resumes from latest plan checkpoint.
  - approval-paused → blocks until approval event.
  - interrupted → resumes from latest durable step.
  - resumed → re-runs from the last persisted RunEvent.

---

## Phase 4 — Tasks, cron, coordination · 🟡 tasks/cron LIVE; task-core↔session-core store overlap (matrix §4.2)

**Current state**

- No durable task service. The closest primitive is `mp-orchestration::Todo`, which is not the same thing.
- No cron / scheduled-work surface.
- `execution-core` has a subagent **hook**; there is no subagent **coordinator** (no spawn/message/summarize/reconcile API).
- Skill-promotion workflow already exists, but there is no AutoAgent / Hermes runtime / AgentSummary pipeline.

**Next deliverables**

- New `tasks.proto`: `CreateTask`, `UpdateTask`, `AssignTask`, `BlockTask`, `CompleteTask`, `ListTasks`. Owner: `orchestrator-core` (durable) backed by `session-core` storage.
- New `cron.proto`: `ScheduleCron`, `ListCron`, `FireCron`, `CancelCron`. Owner: `orchestrator-core`. Reference: fork `ScheduleCronTool`, `RemoteTriggerTool`.
- Subagent coordinator API on `execution-core`: `SpawnSubagent`, `SendMessage`, `SummarizeSubagent`, `ReconcileSubagent`. Lineage materialised into `session-core` from existing `SubagentLineage` record type.
- Agents Plane sub-deliverables (Hermes runtime, AutoAgent creator, AgentSummary analytics) — see `PLAN.md` Phase 4 detail.
- New event types: `task.created`, `task.assigned`, `task.completed`, `cron.fired`, `subagent.spawned`, `subagent.reconciled`, `agent.run.completed`.

---

## Phase 5 — Multimodal AI breadth · ✅ LIVE — all 8 modality groups routed (corrected — matrix §1)

**Current state**

- `model-gateway` exposes only `POST /v1/invoke` and `POST /v1/invoke/stream`.
- `inference-core::provider::fallback` knows chat/completions; nothing else.

**Next deliverables — eight modality groups under `/v1/ai/*`**

| Modality | Endpoint | inference-core work | Notes |
|---|---|---|---|
| chat | `/v1/ai/chat/*` | ✅ (rename / namespace from `/v1/invoke`) | Keep `/v1/invoke` as additive alias |
| completions | `/v1/ai/completions/*` | ✅ | Same path |
| images | `/v1/ai/images/*` | new provider trait | DALL-E / SDXL / equivalent |
| speech | `/v1/ai/speech/*` | new provider trait | TTS + STT |
| translation | `/v1/ai/translate/*` | new provider trait | |
| documents | `/v1/ai/documents/*` | new provider trait | OCR / parse / summarize |
| video | `/v1/ai/video/*` | new provider trait | Transcription, summarization |
| realtime | `/v1/ai/realtime/*` | WebSocket transport | Streaming bidirectional |

- Per-modality routing + fallback in `inference-core`, sourced from the model registry in `capability-core`.
- Artifact / result-store contract for large outputs (likely MinIO-backed; see `Model Plane v2` topology).

**Sub-phase: Model/Research plane** — `autoresearch` pilot, GPU worker sandbox template, experiment scheduler, artifact + dataset store, `experiment.completed` event.

---

## Phase 6 — Product clients and operator shell · ❌

**Current state** _(reconciled 2026-05-30 — the prior "zero surfaces" line was stale; see capability-ownership-matrix "P6 reconciliation")_

- **Bridge surface IMPLEMENTED in `go/services/bridge-core`** (builds; voice tests pass): remote-**session registry** + HTTP CRUD/ingest (`POST/GET/DELETE /api/v1/sessions`, `.../{id}/ingest`), **channel adapters + framing + JWT** (`internal/channel/`), and a **voice STT→LLM→TTS pipeline** (`internal/voice/`). The namespace is `/api/v1/sessions` + channel adapters rather than the originally-envisioned `/v1/bridge/*`+`/v1/remote/*`, but the capability (remote sessions, channel ingress, JWT/framing, voice) exists.
- **Only the operator-facing CLI/TUI client is unbuilt** (no `ratatui`/`crossterm` surface) — a net-new terminal-UI **product surface** awaiting a UX brief; it would consume bridge-core's session API, not add backend.

**Next deliverables**

- CLI/TUI shell — ✅ **CLI BUILT** (`rust/clients/bridge-cli`): a Rust REPL over bridge-core's `/api/v1/sessions` API (new/list/get/send/close), parser + base64 ingest unit-tested. A richer `ratatui` TUI (panels, slash-commands sourced from `capability-core`'s registry) is optional future polish layered on the same `client` module — not parity-required. Reference: `claude-code-fork/src/commands/`.
- ~~IDE bridge / Remote sessions / Voice ingress~~ — **DONE in `bridge-core`** (session registry, channel adapters + JWT + framing, voice pipeline). Original `/v1/bridge/*`,`/v1/remote/*`,`/v1/voice/*` namespace plans superseded by bridge-core's actual surface; do not rebuild (would duplicate).
- Channel ingress — `/v1/channels/*` for Slack, email, webhooks, GitHub. Reference: v2 `agent-core/app/messaging/`.
- Settings sync / MDM — for remote-managed defaults; reference: fork `services/settingsSync/`, `remoteManagedSettings/`.

---

## Phase 7 — Knowledge plane · 🟡 graph/wiki routes LIVE; extraction service pending

**Current state**

- No graph memory, no wiki memory, no provenance/contradiction handling.
- `letta-bridge` is a single proxy adapter; not a knowledge layer.

**Next deliverables**

- Rust extraction service — entities, relationships, provenance (`extracted` vs `inferred`), confidence. References: `graphify`, `GraphRAG`.
- Wiki layer — topic / concept / entity pages, summaries, knowledge logs. References: `LLM Wiki`, `logseq`.
- Contradiction handling state machine — reinforce / weaken / qualify / contradict / create.
- Knowledge lint — orphans, stale claims, missing cross-links, missing pages.
- Go scheduling, retention, and scope policy for knowledge artifacts.
- Context assembly in `session-core` extended to consume graph + wiki artifacts without replacing source records.
- New event types: `wiki.page.created`, `wiki.page.updated`, `graph.entity.extracted`, `graph.relationship.added`, `knowledge.contradiction.detected`.

---

## Phase 8 — Token efficiency and ergonomics · 🟡 TOON route + compaction LIVE; benchmark/deferred-schema pending

**Current state**

- `execution-core` has a compaction-trigger hook gate; no compactor.
- No compact transport, no verbosity profiles, no operator-style profiles.

**Next deliverables**

- TOON-style compact transport for large structured payloads, optional + benchmarked + reversible.
- Operator-facing verbosity profiles distinct from runtime logic. Reference: fork `outputStyles/`, `caveman` skill family.
- Compact memory + context summaries at the `session-core` boundary, never replacing source records.

---

## Phase 9 — Verification, cutover, retirement · 🟡

**Current state**

- 18/30 foundation gates green per `VERIFICATION.md`. Phase F closed (cancel/SAGA/approval-durability fixture).

**Next deliverables — new parity gates**

- Orchestration shell parity gate (`/v1/orchestration/*` end-to-end).
- Capability platform parity gate (8 registries populated and scope-resolved).
- Multimodal breadth gate (8 modality groups end-to-end with fallback).
- Knowledge correctness gate (provenance preserved, contradictions surfaced).
- Tasks / cron / coordinator durability gate.
- Bridge / voice / channel surface gates.
- Compact transport correctness gate (round-trip, benchmark, reversibility).
- Staged cutover plan from foundation services to full shell.
- Retire stale planning narratives once new canon and gates are in place.

---

## Cross-cutting backlog (not phase-specific)

These gaps span phases and should be tracked at the top-level. The canonical stub-replacement inventory is [gap-analysis.md § 13](gap-analysis.md).

- **In-memory backings that must move to durable stores.** `capability-core` registry + policy engine, `letta-bridge` `memstore`, `model-gateway` rate-limit + idempotency state, `inference-core` prompt cache. Phase 9 stub-replacement gates in `VERIFICATION.md` track each.
- **12 orchestration RPC handlers Unimplemented.** `go/services/orchestrator-core/internal/orchestration/handlers.go` blocks Phase 1 cutover until replaced. See gap-analysis § 13.1.
- **HTTP namespaces.** None of the planned `/v1/orchestration/*`, `/v1/capabilities/*`, `/v1/ai/*`, `/v1/tasks/*`, `/v1/cron/*`, `/v1/bridge/*`, `/v1/voice/*`, `/v1/channels/*`, `/v1/memory/*`, `/v1/knowledge/*` namespaces exist; only `/v1/invoke` + `/v1/invoke/stream` are live.
- **Proto evolution.** `gateway.proto`, `sessions.proto`, `execution.proto`, `capabilities.proto` need additive evolution. New protos required for tasks, cron, plans-as-services, approvals-as-services, bridge, voice, channels, knowledge.
- **Scope model propagation.** `run / thread / workspace / user / org / global` is documented but not propagated through capability and policy lookups.
- **Event schema gaps.** Add named events for capability/task/cron/subagent/wiki/graph/experiment domains (see Phase 1 cross-cutting infra).
- **Cost / analytics core.** `Model Plane v2` ships a dedicated `cost-core`; Model Plane has none. Add per-run cost tracking + `usage.{org}.llm` emission from `model-gateway`.
- **Multi-scope memory.** Only `thread` scope exists today; `run`, `workspace`, `user`, `org`, `global` scopes are not materialised. (GOAL.md non-negotiable §6.)
- **Python research/training plane.** Existing `python/` projects are off-hot-path labs (`eval-lab-py`, `graph-lab-py`, `provider-research-py`, `mp-events-py`); they are not the autoresearch + experiment-tracking workers PLAN Phase 5b describes.

---

## Non-goals

- Rebuilding `Model Plane v2` as a Python monolith.
- Rebuilding `claude-code-fork` as a TypeScript clone.
- Replacing source records with summary-only memory.
- Shipping feature breadth without owner-service clarity.
- Letting any reference repo become the new architecture by imitation.
