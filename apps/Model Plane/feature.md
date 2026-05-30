# Model Plane Capability Expansion Fit Analysis

Last updated: 2026-04-30

This analysis maps the requested capability expansion plan onto the current
`apps/Model Plane` repository. The goal is to decide where each feature fits,
which pieces are necessary, how to implement them without breaking the existing
Rust hot path and Go durable-control split, and when to adopt external projects
versus building Model Plane-owned versions.

## Executive Decision

Keep the current architecture and finish the missing product shell around it.
Do not add a new top-level agent framework as the runtime owner.

The highest-leverage path is:

1. Finish durable orchestration surfaces: plans, approvals, todos, run-event
   history, cancellation, and run-event streaming.
2. Turn `capability-core` into the durable registry and policy authority for
   tools, skills, MCP, models, routing, memory adapters, safety policies,
   serializers, modality routes, schedules, and operator UI widgets.
3. Keep Temporal as the only durable outer orchestrator.
4. Keep Letta behind an adapter, not as session or run authority.
5. Build Model Plane-owned contracts for memory, artifacts, context rendering,
   wiki, graph, browser traces, and skill packages.
6. Use external projects as adapters, sidecars, design donors, or benchmarks
   unless they match an exact execution lane.

The recurring rule: own the contracts and state model; adopt tools behind those
contracts only when they measurably improve reliability, quality, latency, cost,
or operator experience.

## Current Repo Reality

The repo is not an empty scaffold.

### Live or Mostly Live

- `rust/services/model-gateway`: public HTTP/gRPC/SSE ingress for `/v1/invoke`
  and `/v1/invoke/stream`, JWT/JWKS auth, request normalization, rate limiting,
  and header scrubbing.
- `rust/services/inference-core`: OpenAI and Anthropic provider adapters,
  fallback chain, retries, streaming, and in-memory prompt cache.
- `rust/services/session-core`: thread, message, run, checkpoint, event log,
  context assembly, memory index, and transactional outbox.
- `rust/services/session-core/src/orchestration_grpc.rs`: gRPC read and
  transition surface for plans, todos, approvals, subagent lineage, and
  run-event streaming.
- `rust/services/session-core/migrations/0003_orchestration_tables.sql`:
  Postgres tables for plans, plan steps, todos, approvals, and subagent edges.
- `rust/services/execution-core`: step execution loop, permission gates, hook
  gates, subagent hook, secret scrub, checkpoint recovery, `ExecuteStep`,
  `ResumeRun`, and `CancelRun` proto shape.
- `go/services/orchestrator-core`: Temporal workflows for interactive runs,
  deep tasks, memory consolidation, skill promotion, and wide research.
- `go/services/orchestrator-core/internal/orchestration/handlers.go`: proxies
  orchestration RPCs to `session-core`; no longer a pure `Unimplemented` stub.
- `go/services/capability-core`: capability list/get/policy/skill-promotion
  RPCs, static capability seeds, a Postgres-backed model registry, policy
  engine, and in-code seed catalogs for modalities, artifacts, schedules,
  tasks, and subagent coordination.
- `go/services/sandbox-manager`: lease and snapshot lifecycle against in-memory
  stores.
- `go/services/browser-broker`: browser grant acquire/revoke/validate against
  an in-memory grant store.
- `go/services/letta-bridge`: `MemoryService` backed by an in-memory memstore.
- `python/*-lab-py`: off-hot-path labs for events, provider research, graph
  experiments, and evals.

### Still Missing or Not Productized

- Public `/v1/orchestration/*`, `/v1/runs/*/events`, `/v1/tasks/*`,
  `/v1/cron/*`, `/v1/capabilities/*`, `/v1/ai/*`, `/v1/memory/*`, and
  `/v1/knowledge/*` namespaces.
- Create APIs for plans, todos, approvals, tasks, schedules, graph/wiki
  materialization jobs, and artifact records.
- Durable Postgres backing for all capability registries except the early
  `models` table.
- Durable Redis/Postgres backing for rate limits, browser grants, sandbox
  leases, prompt cache, idempotency, and Letta bridge state.
- Real Letta upstream integration.
- Tool execution bridge beyond deterministic bootstrap behavior in
  `execution-core/src/tool_bridge/mod.rs`.
- Artifact object store provisioning and metadata persistence.
- Multimodal provider traits beyond chat-style inference.
- Context renderer layer for JSON, compact JSON, TOON, markdown tables,
  summaries, and model-specific prompt layouts.
- Human-editable memory, LLM Wiki, graph memory, Logseq export/import, and
  contradiction queues.
- Browser replay bundles, DOM/screenshot/network trace storage, selector
  healing, and scraping evals.
- Operator dashboard for runs, traces, costs, memory diffs, approvals,
  capability health, and replay/fork.

## Architecture Placement Rules

Use these placement rules for all requested features.

| Concern | Owner | Reason |
| --- | --- | --- |
| Public ingress, auth, streaming, API shape | `model-gateway` | Single public boundary; no durable authority |
| Session, run, thread, event log, checkpoints, orchestration records | `session-core` | Already authoritative Postgres owner |
| Model calls, provider fallback, token streaming, model hot path | `inference-core` | Rust low-latency path already exists |
| Tool loop, permission gates, hooks, step execution, artifacts | `execution-core` | Owns runtime step semantics |
| Long-running workflows, schedules, approvals, retries, compensation | `orchestrator-core` | Temporal already live here |
| Capability metadata, policy, scope resolution, health, registry scores | `capability-core` | Natural registry and policy authority |
| Browser grants, revocation, session endpoints | `browser-broker` | Existing browser access boundary |
| Sandbox leases, snapshots, resource policy | `sandbox-manager` | Existing sandbox control boundary |
| Memory adapters | `letta-bridge` initially; future `memory-core` | Bridge exists, but product memory needs stronger owner |
| Knowledge graph/wiki materialization | future `memory-core` or `knowledge-core`; Rust workers | Needs separate derived-knowledge lifecycle |
| Evaluation and experiments | `python/eval-lab-py`, `python/graph-lab-py`, Temporal workers | Off hot path, metric-driven |
| Raw web/document ingestion | Ingestion Plane and Data Plane APIs | Model Plane should not own raw corpus ingestion |

## Feature Fit Matrix

### 1. Capability Registry, Skill Registry, MCP Gateway

Necessary: yes. This is the highest-leverage missing feature.

Best fit:

- `capability-core` owns durable metadata, policy, scopes, registry scores,
  health checks, rollout state, and audit logs.
- `execution-core` consumes execution-ready definitions only.
- `model-gateway` exposes HTTP APIs and SSE for registry changes.
- `orchestrator-core` owns promotion/reconciliation workflows.

Current code:

- Static capability seed in `go/services/capability-core/internal/registry`.
- Kind constants already include `tool`, `skill`, `plugin`, `mcp_server`,
  `model`, `routing_policy`, `memory_adapter`, `safety_policy`, and `command`.
- `models` Postgres table exists and is merged into `ListCapabilities`.
- Skill validation and promotion RPCs exist, but only against the in-memory
  registry.
- Seed catalogs for tasks, schedules, modalities, artifacts, and coordination
  exist but are not yet durable or fully mounted as product APIs.

Implement:

- Add `capabilities`, `capability_versions`, `capability_scopes`,
  `capability_examples`, `capability_evals`, `capability_health`,
  `skill_packages`, `skill_resources`, `mcp_servers`, `mcp_oauth_tokens`,
  `plugin_packages`, `routing_policies`, `safety_policies`, and
  `registry_audit_log` tables.
- Replace static registry source with a Postgres source using the existing
  `Source` interface.
- Add scope resolution for `run`, `thread`, `workspace`, `user`, `org`, and
  `global`.
- Store schemas as JSON Schema with generated Go/Rust/Python bindings where
  required.
- Add registry scoring fields: success rate, schema failure rate, p95 latency,
  mean cost, approval rate, incident count, and operator rating.
- Treat skill packages like code: versioned, pinned, testable, reviewable, and
  reversible.
- Add HTTP APIs under `/v1/capabilities/*` after gRPC is durable.

Adopt, adapt, or build:

- Adopt MCP protocol conventions and OAuth/security guidance.
- Adapt OpenAI Apps SDK-style descriptor metadata for tool/UI alignment, but do
  not depend on ChatGPT Apps as the internal product model.
- Adapt Matt Pocock-style composable skills and Codex/Claude progressive
  disclosure patterns.
- Build our own registry and policy engine because scope, tenancy, rollback,
  evals, and audit are Model Plane-specific.

Do not build:

- A marketplace before provenance, scopes, tests, sandboxing, and policy are
  mature.
- Global prompt snippets that bypass registry governance.

Acceptance:

- A new tool, skill, MCP server, model, routing policy, or safety policy can be
  added with descriptor, schemas, scopes, tests, examples, health checks,
  rollout state, and rollback metadata.
- Agents only use capabilities resolved through `capability-core`.
- Operators can pin, disable, canary, quarantine, and roll back any capability.

### 2. Durable Tasks, Cron, and Coordination

Necessary: yes, but do not create a separate service first.

Best fit:

- `orchestrator-core` owns Temporal workflows, schedules, signals, retries, and
  compensation.
- `session-core` owns durable task and coordination records.
- `capability-core` owns task/schedule capability metadata and policy.
- A future `task-core` only makes sense if task volume or API ownership becomes
  too large for `orchestrator-core`.

Current code:

- Temporal is already live in `orchestrator-core`.
- `DeepTaskWorkflow`, `InteractiveRunSupervision`, and `WideResearchWorkflow`
  already model long-running work.
- `go/services/capability-core/internal/tasks` and `internal/schedule` contain
  seed catalogs, not a durable task system.
- `mp-orchestration::Todo` and `orchestration.proto` are not enough for
  long-lived work assignment, retries, external triggers, or cron.

Implement:

- Add `tasks`, `task_events`, `task_assignments`, `task_dependencies`,
  `task_artifacts`, `cron_schedules`, `cron_fires`, and `remote_triggers`
  tables.
- Add `tasks.proto` and `cron.proto` or extend orchestration protos with task
  service boundaries.
- Use Temporal schedules for recurring work instead of rolling a cron runner.
- Use Temporal signals for approvals, pause/resume, cancellation, and external
  trigger delivery.
- Add per-tenant, per-workspace, per-capability, per-model, per-risk, and
  per-budget concurrency controls.
- Materialize subagent coordination into `session-core` lineage tables and
  task records.

Adopt, adapt, or build:

- Adopt Temporal schedules and signals.
- Avoid Cadence because it overlaps Temporal.
- Avoid Airflow for interactive agent tasks; it is useful only for separate
  batch/data DAGs if another team already operates it.
- Evaluate Trigger.dev or Hatchet only if a TypeScript or Postgres-backed job
  surface becomes necessary for non-core jobs.
- Build the Model Plane task API because it must integrate with runs,
  approvals, lineage, policies, budgets, and event replay.

Acceptance:

- Scheduled and triggered runs survive worker restarts.
- Operators can inspect inputs, outputs, retries, approvals, skills used,
  capability versions, costs, and artifacts.
- Tasks can be paused, resumed, cancelled, replayed, and forked.

### 3. Agent Runtime and Orchestration Frameworks

Necessary: the runtime is necessary; an additional top-level framework is not.

Best fit:

- Keep `execution-core` as the step loop and tool/permission/hook runtime.
- Keep `session-core` as state authority.
- Keep `orchestrator-core` as durable outer supervisor.
- Use Python frameworks only as sidecars for bounded tasks.

Current code:

- The core runtime split already exists.
- `execution-core/src/tool_bridge/mod.rs` is still deterministic bootstrap
  behavior and needs real registry-backed invocation.
- `orchestrator-core` workflows call downstream gRPC clients.

Implement:

- Make `execution-core` fetch capability definitions from `capability-core`
  before invoking any tool.
- Add structured tool invocation envelope: idempotency key, auth scope,
  risk class, input schema, output schema, timeout, retry policy, trace ID,
  and approval policy.
- Add graph-style control only where a task needs stateful branching beyond
  Temporal workflow code.
- Add structured output repair and retry loops at the `inference-core` and
  sidecar-worker boundary.

Adopt, adapt, or build:

- Keep Temporal as core.
- Use Pydantic AI for typed Python sidecars: extraction, plan proposal drafts,
  memory consolidation, evals, and structured background jobs.
- Use OpenAI Agents SDK only for isolated OpenAI-specific experiments where
  handoffs, guardrails, hosted tools, or tracing provide measurable value.
- Use LangGraph only for contained graph-control experiments with hard kill
  criteria.
- Use LlamaIndex Workflows for retrieval-heavy Python workflows if the
  document/RAG workflow needs it.
- Keep AutoGen, AG2, CrewAI, and smolagents in lab or demo lanes, not core.

Acceptance:

- Same user task can run through the small Rust step path or a Temporal-backed
  workflow without a second state authority.
- Every tool/model/memory/skill call emits traceable events.
- Structured output validation failures retry, fallback, or require review.

### 4. Multimodal API Layer

Necessary: yes, but implement in layers.

Best fit:

- `model-gateway` exposes `/v1/ai/*` and streaming.
- `capability-core` owns modality registry, provider eligibility, route
  policies, and artifact policy.
- `inference-core` owns low-latency model calls for chat, completions, vision,
  speech, translation, realtime, and lightweight document reasoning.
- `execution-core` owns long-running or artifact-heavy modality execution:
  document parsing, video, batch audio, and file normalization.
- Object storage owns bytes; Postgres owns artifact metadata.

Current code:

- `gateway.proto` and `inference.proto` cover text invocation only.
- `capability-core/internal/modalities` already has seed entries for chat,
  completions, images, speech, translation, documents, video, and realtime.
- `execution-core/src/artifact/mod.rs` only builds a simple artifact key.

Implement:

- Add `Artifact` proto and table before adding large multimodal outputs.
- Add modality-specific provider traits: chat, completions, vision, image,
  speech, translation, documents, video, realtime.
- Add `/v1/ai/chat`, `/v1/ai/images`, `/v1/ai/speech`,
  `/v1/ai/documents`, `/v1/ai/video`, and `/v1/ai/realtime` only after
  capability-backed routing exists.
- Normalize files into typed artifacts: raw file, text, layout, tables,
  figures, citations, embeddings refs, graph refs, and wiki refs.
- Send raw document storage and embeddings through Data Plane APIs; do not
  make Model Plane a second document store.

Adopt, adapt, or build:

- Adopt provider APIs behind Model Plane provider traits.
- Use Docling or similar local parsers behind a sandboxed document worker.
- Evaluate RAG-Anything for multimodal document RAG, but keep outputs in Model
  Plane artifact contracts.
- Build our own multimodal contract layer because tenant policy, artifact
  retention, event replay, and provider routing are platform concerns.

Acceptance:

- A run can consume text, images, audio, video, PDFs, Office docs, screenshots,
  and structured JSON through one typed run/artifact model.
- Large outputs are artifact references, not oversized event payloads.
- Operators can inspect modality-specific cost, latency, confidence, and
  extraction failure modes.

### 5. Context Serialization and Token Economy

Necessary: yes, after registry and trace data are available.

Best fit:

- `session-core` owns context assembly and summary boundaries.
- `model-gateway` owns client-facing verbosity profile selection.
- `execution-core` owns tool-output compression before model re-entry.
- `capability-core` registers serializers and policies.
- Python eval labs benchmark fidelity and cost.

Current code:

- `execution-core` has a compaction-trigger hook, but no compactor.
- `session-core` has context assembly SLO tests but no pluggable renderer.
- `docs/phase-8` already defines optional, benchmarked, reversible compaction.

Implement:

- Add `ContextEnvelope` and `ContextRenderer` with canonical JSON, compact JSON,
  TOON, markdown table, plain text, raw excerpt, and model-specific modes.
- Keep JSON as storage and API truth.
- Use TOON only at prompt boundaries for uniform arrays, tool outputs,
  registry listings, retrieval results, and graph neighborhoods.
- Add round-trip tests for any reversible compact format.
- Add token/cost telemetry by prompt section, artifact type, tool output,
  retrieval source, memory source, serializer, and skill resource.
- Add verbosity profiles: brief, normal, detailed, audit.

Adopt, adapt, or build:

- Adapt TOON as a renderer pattern, not as a canonical format.
- Adapt Caveman-style brevity as a profile, not a default brand/persona.
- Build our own renderer because it must preserve provenance, source anchors,
  security-relevant details, errors, diffs, and auditability.

Acceptance:

- 25-40 percent token reduction on structured context benchmarks without lower
  task success, schema accuracy, or auditability.
- Every compacted payload links back to canonical raw artifacts.
- Operators can disable compaction per workspace, run, or risk class.

### 6. Memory Product

Necessary: yes. This is a product, not just a vector search feature.

Best fit:

- `session-core` remains authoritative for run/session memory index metadata.
- `letta-bridge` becomes a real adapter to Letta, not a hidden source of truth.
- Add `memory-core` when memory scope, provenance, deletion, review state,
  wiki, graph, and notebook APIs outgrow `letta-bridge`.
- Use Data Plane for enterprise document retrieval and embeddings.
- Use Model Plane memory for agent/session/user/project/org reasoning memory,
  wiki pages, graph facts, glossary/ADR memory, and memory policy.

Current code:

- `memory.proto` only supports thread-scoped `SearchMemory` and `IndexMemory`.
- `letta-bridge` is in-memory only.
- `session-core` has `memory_index` metadata, but not a full memory product.
- `docs/phase-7` defines derived graph/wiki concepts, but current docs say
  they are additive only and mostly read-only.

Implement:

- Extend memory envelopes with scope, provenance, confidence, owner,
  created/updated timestamps, expiry/staleness, deletion state, review state,
  classification, and source links.
- Materialize scopes: run, thread, session, user, project/workspace, org, and
  global.
- Add memory write classification: preference, fact, instruction, project
  decision, entity relation, artifact summary, graph edge, wiki block,
  glossary term, ADR, issue state, task, annotation, or policy.
- Add inspect/edit/pin/expire/export/delete APIs.
- Wire real Letta upstream for memory blocks and archival memory, with bounded
  retrieval on the live path.
- Add `memory-core` tables before LLM Wiki and graph memory become product
  features.

Adopt, adapt, or build:

- Adopt Letta concepts for stateful agents, memory blocks, archival memory,
  messages, runs, and steps, but keep `session-core` as run/session authority.
- Evaluate Mem0 for user preference extraction and personalization.
- Evaluate Zep/Graphiti for temporal facts if it saves time, but keep
  provenance and scope in Model Plane contracts.
- Evaluate Cognee for graph-vector bootstrapping.
- Build Model Plane memory envelopes and review/deletion semantics ourselves.

Acceptance:

- Agents recall relevant cross-session facts without dumping full history into
  context.
- Contradictions are flagged, not silently merged.
- Users and operators can inspect, edit, export, delete, and review memory.
- Sensitive memory is permissioned, auditable, and revocable.

### 7. LLM Wiki, Logseq-Compatible Memory, and Shared-Language Files

Necessary: yes, but after the memory envelope and provenance model.

Best fit:

- Future `memory-core` or `knowledge-core` owns wiki/page/block APIs.
- Rust workers produce candidate page edits and lint outputs.
- Go workflows own review gates, scheduling, and retention.
- Git-backed storage is appropriate for project/workspace wiki exports.

Current code:

- `docs/phase-7/wiki-memory.md` defines derived wiki memory, but not
  human-editable product memory.
- There is no Git-backed wiki store, Logseq export/import, page diff, or lint
  queue.

Implement:

- Store immutable raw sources separately from synthesized wiki pages.
- Add wiki page templates for entity, topic, comparison, decision, glossary,
  ADR, issue state, and research log pages.
- Add Git branch/diff workflow for LLM-written page changes.
- Add lint checks for unsupported claims, stale pages, duplicate entities,
  broken links, missing provenance, contradiction, glossary drift, and ADR
  conflicts.
- Add Logseq-compatible Markdown/Org export/import for selected wiki pages,
  tasks, glossary pages, ADRs, and memory entries.
- Ingest human edits back as reviewed memory diffs with provenance.

Adopt, adapt, or build:

- Implement the Karpathy LLM Wiki pattern directly.
- Use Logseq as UX/data-model inspiration and optional compatibility target.
- Do not copy Logseq AGPL code into proprietary core services unless licensing
  is intentionally accepted.
- Adapt Matt Pocock-style `CONTEXT.md`, glossary, ADR, TDD/debug, interview,
  and issue-slicing skill templates into governed skills.

Acceptance:

- Wiki changes show diffs, sources, lint results, and rollback.
- The same memory can be viewed through API, workbench UI, and exported
  Markdown/Org-compatible files.
- Shared-language memory reduces repeated explanation without creating
  unreviewed jargon drift.

### 8. Graphify-Style Corpus Graph and GraphRAG

Necessary: yes for project/corpus intelligence, but do not make it the only
memory system.

Best fit:

- Ingestion Plane and Data Plane own raw corpus acquisition, file storage, and
  document indexes.
- Model Plane owns graph extraction jobs, graph metadata, graph queries,
  graph-to-context rendering, and run-time graph retrieval.
- Rust workers own deterministic AST extraction and high-throughput graph
  processing.
- Python `graph-lab-py` owns experiments and benchmarks.
- `capability-core` registers graph extractors, graph stores, graph routes,
  and graph policies.

Current code:

- `python/graph-lab-py` is a lab only.
- `docs/phase-7/graph-memory.md` exists, but there is no graph store,
  extractor, graph report, or query API.

Implement:

- Add `CorpusGraphBuilder` as a Model Plane-owned worker contract.
- Output `graph.json`, `graph.html`, `GRAPH_REPORT.md`, graph statistics,
  community summaries, and query APIs.
- Use deterministic AST extraction first, semantic extraction second.
- Track every edge as extracted, inferred, ambiguous, or contradicted.
- Cache by content hash and prevent silent graph shrink on incremental updates.
- Add graph neighborhoods to `ContextRenderer` with compact JSON/TOON when
  uniform enough.

Adopt, adapt, or build:

- Adapt Graphify patterns: AST-first extraction, multimodal semantic pass,
  caching, NetworkX-style analysis, graph reports, and explicit provenance.
- Optionally run Graphify itself as a benchmark baseline or plugin.
- Use Microsoft GraphRAG for global corpus questions and community summaries
  where its indexing cost is justified.
- Use LightRAG/LazyGraphRAG patterns when lower cost and incremental updates
  matter more than full GraphRAG richness.
- Build the production graph contracts ourselves.

Acceptance:

- Graph retrieval improves multi-hop project-navigation tasks over raw search
  on benchmark corpora.
- Every graph answer cites source file/span, wiki page, graph edge, or artifact
  lineage.
- Incremental updates do not drop known-good graph edges without detection.

### 9. Browser Automation and Scraping Reliability

Necessary: yes, but split agent browser actions from raw ingestion.

Best fit:

- `browser-broker` owns browser grants, endpoint selection, revocation, and
  authenticated-session boundaries.
- `execution-core` owns browser action execution, trace capture, and policy
  checks.
- `orchestrator-core` owns long-running browser workflows and retries.
- `capability-core` owns browser-action capability metadata and risk policy.
- Ingestion Plane/Quarry owns large-scale crawling and scraping ingestion.

Current code:

- `browser-broker` grant lifecycle is in-memory.
- `execution-core` has no browser trace or replay bundle model.
- Quarry v2 in Ingestion Plane already owns much of the crawling/scraping lane.

Implement:

- Persist browser grants and revocations in Redis/Postgres.
- Add browser action artifacts: screenshot, DOM snapshot, accessibility tree,
  network log, console log, cookies/storage metadata, extracted JSON, and trace
  bundle.
- Add action states: navigate, observe, extract, act, verify, retry, escalate.
- Add selector healing, visual anchors, content anchors, schema validation,
  delta detection, and canary pages.
- Escalate CAPTCHA, login, payment, and destructive actions to humans.
- Add URL/file safety checks before fetch/download/parser handoff.

Adopt, adapt, or build:

- Use Playwright as the deterministic base.
- Use Crawlee in Ingestion Plane for crawling queues, retries, and scale.
- Use Stagehand-style AI fallback on top of deterministic Playwright
  primitives, not as the core reliability layer.
- Benchmark browser-use/OpenManus-style loops before adopting.
- Build Model Plane browser trace and policy contracts.

Acceptance:

- Browser runs are replayable with screenshots, DOM snapshots, actions,
  extracted outputs, and policy decisions.
- Nightly scraping/browser evals run against golden pages and schema fixtures.
- Unsafe URLs/files are rejected before any fetch or parser execution.

### 10. Operator Control Plane

Necessary: yes. Without this, powerful agents are not operable.

Best fit:

- `model-gateway` exposes public APIs and streaming.
- `session-core`, `orchestrator-core`, `capability-core`, `memory-core`, and
  `execution-core` expose inspection APIs.
- Frontend Plane implements the dashboard/workbench UI.
- OpenTelemetry remains the trace substrate.

Current code:

- OTel primitives exist.
- `capability-core` exposes implementation status only.
- No runs dashboard, trace viewer, replay/fork UI, approval queue, cost view,
  prompt/version manager, memory inspector, or knowledge workbench exists.

Implement:

- Runs dashboard: active, scheduled, failed, waiting approval, cost anomalies,
  latency anomalies.
- Trace viewer: model calls, tool calls, retrievals, memory reads/writes,
  browser actions, prompts, context renderers, compressed payloads, outputs,
  approvals, loaded skills, skipped skills, and resource files.
- Replay and fork from any step with changed model, prompt, tool, memory,
  capability version, serializer, or skill version.
- Approval queue with risk class, policy reason, scope, and proposed action.
- Knowledge workbench: wiki browser, graph explorer, memory inspector,
  contradiction queue, ADR/glossary editor, source viewer, and task list.

Adopt, adapt, or build:

- Adopt OpenTelemetry GenAI semantic conventions where they fit.
- Evaluate Langfuse or Phoenix for traces, prompt management, datasets, and
  evals.
- Build operator-specific APIs and workbench UX because they must map to Model
  Plane events, capabilities, memory, approvals, and replay semantics.

Acceptance:

- Operator can answer what happened, why, how much it cost, what data was
  touched, which versions ran, what memory changed, and how to replay it.
- Product teams can roll back prompts, serializers, skills, capabilities, and
  memory policies without unsafe production edits.

### 11. Policy, Permissions, and Security

Necessary: yes, cross-cutting and immediate.

Best fit:

- `capability-core` owns risk classes, allowlists, scope policies, rollout
  state, and policy decisions.
- `execution-core` enforces per-step permission decisions.
- `model-gateway` enforces public auth and request-level policy.
- `browser-broker` and `sandbox-manager` enforce grant/lease boundaries.
- Future `memory-core` enforces memory/wiki edit policy.

Current code:

- Policy engine exists but is simple.
- Sandbox policy contract exists in `capability-core/internal/sandbox`.
- Secret scrub exists in `execution-core`.
- Browser grants and sandbox leases are in-memory.

Implement:

- Add risk classes: read-only, write-low-risk, external-send, financial,
  destructive, code-exec, credential-access, browser-authenticated,
  memory-write, wiki-edit, graph-inference, skill-load, external-fetch, and
  high-cost.
- Add scoped credentials and per-capability OAuth/API scopes.
- Add prompt-injection defenses for web pages, documents, email, MCP tool
  outputs, memory entries, wiki pages, glossary pages, ADRs, and skill
  resources.
- Treat MCP servers and skill packages as untrusted code until pinned, scoped,
  sandboxed, and tested.
- Persist every external side effect and every memory/wiki/skill/capability
  change to the event log.

Adopt, adapt, or build:

- Adopt MCP auth/security guidance.
- Build Model Plane policy evaluation and audit because tenancy, approvals,
  memory, tool execution, and rollback are platform-specific.

Acceptance:

- No external write action happens without policy evaluation.
- Credential use is scoped, visible, and revocable.
- High-risk memory, wiki, glossary, ADR, or skill edits require review and can
  be reverted.

### 12. Benchmark and Evaluation Suite

Necessary: yes. This is the only honest way to claim superiority.

Best fit:

- `python/eval-lab-py` owns benchmark harnesses and reports.
- `python/graph-lab-py` owns graph and retrieval experiments.
- `orchestrator-core` runs scheduled eval workflows.
- `capability-core` stores capability/skill eval metadata and promotion gates.
- `session-core` and object storage preserve traces and artifacts.

Current code:

- Labs exist but are not production eval workers.
- Skill promotion workflow exists but has only basic registry validation.

Implement:

- Build datasets from real failed tasks and golden fixtures.
- Add promotion gates for capabilities, skills, serializers, memory policies,
  graph extractors, browser actions, and provider routes.
- Add scorecards tied to capability and skill versions.
- Make eval traces and artifacts inspectable in the operator dashboard.

Benchmark categories:

- Registry: tool selection accuracy, wrong-tool rate, schema failures, latency,
  cost, rollback time.
- Skills: trigger precision/recall, skill loading cost, task success lift,
  resource overloading, cross-model portability.
- Tasks/cron: completion after worker crashes, retry correctness, schedule
  accuracy, stuck-run detection.
- Multimodal: PDF tables, chart QA, screenshot QA, audio transcription, video
  QA, citation accuracy.
- Token economy: JSON vs compact JSON vs TOON vs markdown, schema parse
  accuracy, answer quality, retries, latency, auditability.
- Memory/wiki/graph: temporal recall, contradiction handling, deletion
  compliance, editability, source coverage, graph edge precision/recall.
- Browser/scraping: task success, page-change robustness, extraction F1,
  replay completeness, escalation correctness.
- Operator tooling: mean time to diagnose failed run, prompt rollback time,
  approval latency, memory-diff review time.

Acceptance:

- Every claimed advantage has a baseline, benchmark, and regression test.
- New capabilities and skills cannot ship without evals, traces, and rollback.

### 13. Autonomous Research and Self-Improvement

Necessary: yes, later. It should not touch production code or policy early.

Best fit:

- `orchestrator-core` owns durable experiment workflows.
- `python/eval-lab-py` owns metric harnesses.
- `python/provider-research-py` owns provider/model experiments.
- `python/graph-lab-py` owns graph/retrieval experiments.
- Object storage stores datasets, traces, and artifacts.
- Git branches store proposed prompt/skill/config/template changes.

Current code:

- `WideResearchWorkflow` exists.
- There is no `ExperimentSpec`, branch isolation, fixed-budget experiment
  worker, metric gate, or proposal workflow.

Implement:

- Add `ExperimentSpec`: hypothesis, editable surface, allowed files/configs,
  fixed budget, primary metric, guardrails, dataset, rollback rule, branch,
  reviewer, and stop condition.
- Allow early experiments only on prompts, skill triggers/resources,
  serializers, extractors, retrieval thresholds, memory ingestion prompts,
  wiki templates, and browser selectors.
- Require before/after metrics and review before promotion.

Adopt, adapt, or build:

- Adapt Karpathy Autoresearch: one editable target, one metric, fixed budget,
  keep/discard loop, human-readable program file, and rollback.
- Build Model Plane experiment governance because production safety and
  capability promotion are platform-specific.

Acceptance:

- Experiments are reproducible, reviewable, and reversible.
- Failed experiments leave no production side effects.
- Accepted experiments include diff, metric delta, trace links, and rollback.

## External Framework Decisions

| Project | Decision | Fit |
| --- | --- | --- |
| Temporal | Adopt as core | Already live in `orchestrator-core`; best owner for durable workflows, schedules, retries, signals, and compensation |
| Letta | Adapt behind bridge | Good for memory blocks and archival memory; must not own run/session state |
| Pydantic AI | Add as bounded Python sidecar | Typed structured outputs, extraction, evals, memory consolidation, plan drafts |
| OpenAI Agents SDK | Lab only | Useful for OpenAI-specific handoffs/guardrails/tracing experiments; duplicates core runtime if adopted broadly |
| LangGraph | Prototype only | Useful for one graph-control experiment; dangerous as second durable state brain |
| LlamaIndex Workflows | Retrieval sidecar only | Good for event-driven retrieval/RAG workflows, not the core shell |
| AutoGen / AG2 | Lab only | Good for multi-agent research, too much overlap with Temporal/NATS/gRPC contracts |
| CrewAI | Avoid for core | Fast demos, weaker fit for replayable audited runtime |
| smolagents | Learn from | Minimal sandbox/code-agent patterns, not main runtime |
| Cadence | Avoid | Redundant with Temporal |
| Airflow | Avoid for agent runtime | Only for separate batch/data DAGs if already operated |
| MCP | Adopt protocol, build gateway | Standard tool/resource/prompt protocol, but requires strict pinning, scopes, sandboxing, and audit |
| OpenAI Apps SDK descriptors | Adapt | Useful UI/tool metadata conventions; not internal registry authority |
| Graphify | Adapt and benchmark | Good corpus graph design donor and optional plugin, not enterprise memory backend |
| GraphRAG / LightRAG / LazyGraphRAG | Use selectively | GraphRAG for global corpus synthesis; Light/Lazy for incremental or cheaper graph retrieval |
| Logseq | Compatibility target | Good UX/data model for local-first notebook memory; avoid AGPL core dependency unless planned |
| Karpathy LLM Wiki | Implement directly | Best pattern for inspectable Markdown memory over immutable raw sources |
| Karpathy Autoresearch | Adapt | Good safe self-improvement loop pattern |
| TOON | Renderer only | Useful for prompt-boundary compact structured data, not canonical storage/API |
| Caveman | Style policy inspiration | Good for brevity profiles and token budgets, not product persona |
| Matt Pocock Skills | Adapt | Good composable skill templates and shared-language patterns |
| Playwright | Adopt | Deterministic browser automation base |
| Crawlee | Use in Ingestion Plane | Better fit for crawling/scraping queues, retries, and large-scale ingestion |
| Stagehand | Optional fallback | Natural-language browser actions on top of deterministic Playwright only |
| Docling / RAG-Anything | Evaluate sidecars | Useful document/multimodal extraction if wrapped by Model Plane artifact contracts |
| Langfuse / Phoenix | Evaluate | Observability/eval accelerators if they fit OTel and self-hosting constraints |

## Build Order

### Slice A - Truth Sync and Product Surface Inventory

Priority: immediate.

- Update docs that still say orchestration handlers are `Unimplemented`.
- Add a generated or in-code status endpoint for current feature readiness that
  distinguishes seed catalog, gRPC implementation, durable backing, public HTTP
  API, and production readiness.
- Add parity gates for each feature group before large implementation starts.

### Slice B - Orchestration API Closure

Priority: immediate.

- Add create/write APIs for plans, todos, approvals, and run events.
- Expose `/v1/orchestration/*` and `/v1/runs/{run_id}/events`.
- Wire run cancel/resume through `model-gateway`, `orchestrator-core`,
  `execution-core`, and `session-core`.
- Persist and stream run events from the same durable source.

### Slice C - Durable Capability Registry

Priority: immediate.

- Move static capability seeds to Postgres.
- Add registry tables for tools, commands, skills, plugins, MCP servers,
  models, routing policies, memory adapters, safety policies, serializers,
  modalities, artifacts, schedules, and UI widgets.
- Add scope resolution, policy evaluation, rollout, health, scoring, audit,
  and rollback.

### Slice D - Tool Invocation and Sandbox Runtime

Priority: high.

- Replace deterministic `tool_bridge` with registry-backed invocation.
- Persist sandbox leases and browser grants.
- Add tool schemas, timeouts, retries, idempotency, output schemas, artifact
  refs, and approval gates.
- Keep terminal-first coding UX out of scope, but keep sandboxed workers for
  workflows, parsing, scraping, evals, graph building, and automation.

### Slice E - Tasks, Cron, and Subagent Coordination

Priority: high after C.

- Implement task and cron records with Temporal schedules/signals.
- Add task assignment, dependencies, blocking, completion, and artifact links.
- Add subagent spawn/message/summarize/reconcile APIs and materialize lineage.

### Slice F - Artifact Store and Multimodal Contracts

Priority: high after C/D.

- Add artifact table and object-store provisioning.
- Expand modality provider traits.
- Add `/v1/ai/*` namespace progressively, starting with chat namespace
  compatibility and document parsing.
- Add modality evals before production claims.

### Slice G - Memory-Core MVP

Priority: medium-high.

- Wire real Letta upstream.
- Extend memory scopes and envelopes.
- Add inspect/edit/delete/export APIs.
- Add memory review, contradiction, and deletion compliance.

### Slice H - Context Renderer and Token Economy

Priority: medium.

- Add renderer abstraction at session/context boundary.
- Implement compact JSON and TOON renderers behind flags.
- Add benchmark suite before enabling by default.
- Add verbosity profiles and trace-visible compression decisions.

### Slice I - Browser Reliability and Scraping Bridge

Priority: medium.

- Persist browser traces and replay bundles.
- Add Playwright action worker and Stagehand-style fallback only after
  deterministic actions are traced.
- Integrate with Quarry/Ingestion Plane for bulk crawling rather than
  duplicating crawler infrastructure.

### Slice J - Knowledge Graph, LLM Wiki, and Workbench

Priority: medium after G/H/I.

- Build corpus graph worker contract and benchmark against Graphify.
- Add LLM Wiki storage, diffs, lint, contradiction queue, and Logseq export.
- Add retrieval router for lexical/vector/hybrid/graph/wiki/code/ADR/glossary.

### Slice K - Operator Dashboard and Evals

Priority: continuous.

- Start with run and capability inspection, then add replay/fork, memory diffs,
  graph/wiki workbench, cost views, and eval scorecards.
- Make every release change tie back to evals and rollback.

### Slice L - Autoresearch Loop

Priority: later.

- Implement `ExperimentSpec` and fixed-budget workers.
- Start with prompts, skill triggers, serializers, retrieval thresholds,
  memory ingestion prompts, graph extraction prompts, and browser selectors.
- Disallow production code, auth, policy, credential, or billing edits until
  the governance path is proven.

## What We Should Build Ourselves

Build these as Model Plane-owned systems:

- Capability registry and policy engine.
- Skill package schema and promotion workflow.
- Tool invocation envelope.
- Artifact envelope and retention model.
- Context envelope and renderer selection policy.
- Memory envelope, scopes, provenance, review, deletion, and export.
- LLM Wiki page/diff/lint workflow.
- Corpus graph contracts, graph edge provenance, and graph-to-context renderer.
- Operator run/trace/replay/memory/capability workbench APIs.
- Benchmark harnesses tied to capability and skill versions.

Reason: these are platform identity, tenancy, policy, audit, replay, and
operator-experience concerns. External frameworks can plug in, but they should
not define the internal truth.

## What We Should Adopt Directly

Adopt these where they already match the lane:

- Temporal for durable execution, workflows, schedules, signals, retries, and
  compensation.
- OpenTelemetry for traces and metrics.
- MCP protocol concepts for tool/resource/prompt interoperability.
- Playwright for deterministic browser automation.
- Postgres as system of record.
- Object storage for large artifacts.
- JSON Schema/Pydantic/Zod-style schemas at tool and structured-output
  boundaries.

## What We Should Adapt or Wrap

Adapt these behind Model Plane contracts:

- Letta for memory blocks and archival memory.
- Pydantic AI for typed Python sidecars.
- Graphify for corpus graph ideas and benchmark baselines.
- GraphRAG/LightRAG/LazyGraphRAG for specific retrieval workloads.
- Logseq for export/import and UX compatibility.
- TOON for prompt-boundary structured rendering.
- Stagehand for AI fallback on deterministic browser actions.
- Docling/RAG-Anything for document/multimodal parsing.
- Langfuse/Phoenix for observability/eval acceleration.

## What We Should Avoid in Core

Avoid these as core runtime decisions:

- Cadence next to Temporal.
- Airflow for interactive agent runtime.
- CrewAI, AutoGen, AG2, LangGraph, LlamaIndex Workflows, or OpenAI Agents SDK
  as the main orchestration brain.
- Letta as session/run authority.
- Graphify as sole memory backend.
- Logseq core code as a proprietary dependency without explicit license
  acceptance.
- TOON as canonical API or database format.
- Terminal-first Claude Code parity work.

## Open Architecture Questions

Resolve these before implementation PRs:

1. Should `memory-core` and `knowledge-core` be one service or two? Default:
   start with `memory-core` and split `knowledge-core` only if graph/wiki
   materialization needs independent scaling.
2. Should task records live in `session-core` or a new Go-owned store? Default:
   store durable task/run linkage in `session-core`; let `orchestrator-core`
   own Temporal execution.
3. Should artifact metadata live in `session-core` or `capability-core`?
   Default: `session-core` owns run-produced artifact metadata; `capability-core`
   owns artifact capability and retention policy.
4. Should graph storage be Neo4j, Kuzu, Postgres, or object-store snapshots?
   Default: start with Postgres/object-store graph artifacts plus Kuzu/Neo4j
   benchmark before choosing.
5. Should UI widgets be first-class capabilities? Default: yes, but only after
   tool/capability registry durability is done.

## Source Notes

External references checked for this analysis:

- Temporal durable execution and schedules: https://temporal.io/
- Pydantic AI durable execution: https://ai.pydantic.dev/durable_execution/overview/
- OpenAI Agents SDK guardrails and handoffs: https://openai.github.io/openai-agents-python/
- LangGraph durable execution: https://docs.langchain.com/oss/python/langgraph/durable-execution
- Letta stateful agents and archival memory: https://docs.letta.com/
- LlamaIndex Workflows: https://docs.llamaindex.ai/en/stable/module_guides/workflow/
- MCP authorization/security: https://modelcontextprotocol.io/specification/
- Stagehand browser automation: https://docs.stagehand.dev/
- Graphify: https://graphify.net/ and https://github.com/safishamsi/graphify
- Logseq: https://github.com/logseq/logseq
- Karpathy Autoresearch: https://github.com/karpathy/autoresearch
- TOON: https://github.com/toon-format/toon

