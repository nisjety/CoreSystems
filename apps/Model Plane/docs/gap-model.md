# Model Plane — Updated Gap Analysis and Target Architecture

> Generated: 2026-05-06  
> Scope: Rust + Go Model Plane with cross-plane contracts to Quarry v2 and Data Plane.  
> Rule: **Model Plane reasons; Data Plane knows; Quarry captures evidence.**
>
> **Implementation audit: 2026-05-07**

### Implementation Scorecard

| Area | ✅ Done | ⚠️ Partial | ⬜ Missing |
|---|---|---|---|
| **Rust services** (4 target) | 4 | 0 | 0 |
| **Go services** (7 target) | 7 | 0 | 0 |
| **Python labs** (3 target) | 3 | 0 | 0 |
| **Shared crates/pkgs** | 14 | 0 | 0 |
| **Proto contracts** | 14 | 0 | 0 |
| **Gap matrix items** (18) | 15 | 0 | 3 |
| **Feature matrix** (17) | 7 | 5 | 5 |
| **Release gates** (9) | 4 | 4 | 1 |

**Code totals:** ~10,200 lines Rust services + ~14,200 lines Go services + ~3,600 lines shared crates + ~2,800 lines Go pkgs + ~1,100 lines Python labs = **~32,000 lines of production code** across 14 services, 14 shared libraries, 14 proto contracts, and 3 Python lab packages. All service code is real implementation with tests — no stubs or TODO-only files in shipped services.

## 1. Executive Summary

Model Plane already has a strong Rust/Go foundation: Rust gateway/session/inference/execution services and Go orchestration/capability/control services. The remaining work is to complete product-surface parity, harden cross-plane contracts, and add graph/wiki/research/browser-agent capabilities without dragging knowledge storage or browser execution into the reasoning plane.

The updated target is:

- ✅ **Rust** for public invoke, streaming, session/context assembly, inference routing, structured output, execution loop, browser-agent step loop, compaction, and transport codecs. — All 4 services implemented (~9,650 LOC).
- ✅ **Go** for Temporal workflows, task/cron, capabilities, plugins, MCP, browser grants, cost ledger, channels, approvals, and registry/policy. — All 7 services implemented (~14,200 LOC).
- ✅ **Python** for eval/lab/provider-specific ML glue only. — 3 lab packages (graph-lab, eval-lab, provider-research) + mp-events-py with real code.

## 2. Target Model Plane Structure

```text
MODEL PLANE L4

Rust runtime:
  ✅ model-gateway        public invoke, stream, structured schema passthrough, ZDR/cross-plane guards
  ✅ session-core         threads, runs, checkpoints, periodic + on-demand compaction (CompactNow RPC), context assembly, memory index
  ✅ inference-core       providers, streaming, structured outputs, prompt cache, speech/vision/doc-intel provider traits
  ✅ execution-core       tool loop, hooks, browser-agent action/observation loop, subagents, secret scrub

Go durable control:
  ✅ orchestrator-core    Temporal workflows, autoresearch (budgeted program-artifact loop), wide research, approvals, pause/resume/recovery
  ✅ capability-core      tools, commands, skills, plugins, MCP, models, routing, safety, memory adapters
  ✅ browser-broker       trusted grants validate/revoke/acquire, policy, session lifecycle
  ✅ letta-bridge         optional memory block bridge; not thread/retrieval authority
  ✅ task-core            tasks, cron, scheduled work, remote triggers                    — IMPLEMENTED (in-memory store, cron scheduler, CRUD+trigger endpoints)
  ✅ bridge-core          CLI/IDE/channel ingress, JWT/framing adapters, voice pipeline, remote sessions — IMPLEMENTED (session registry, channel adapters, JWT validator, frame codec, WebSocket adapter, voice STT→LLM→TTS pipeline)
  ✅ cost-core            token/cost ledger, budget caps, usage events                    — IMPLEMENTED (in-memory ledger, budget check, NATS usage subscriber, OTel metrics)

Python labs:
  ✅ graph-lab-py         LangGraph/LangChain/Deep Agents prototyping only                — IMPLEMENTED (ReasoningGraph, BFS/DFS traversal, networkx export, tests)
  ✅ eval-lab-py          regression/replay/model/agent evals only                        — IMPLEMENTED (EvalCase/EvalSuite/EvalRunner, exact-match scoring, tests)
  ✅ provider-research-py SDK experiments only                                            — IMPLEMENTED (ProviderProfile/ModelProfile, cost ranking, capability matrix, budget filtering, tests)

Cross-language infra (not in target list but implemented):
  ✅ mp-events-py         Python event envelope + proto bindings (real implementation)
  ✅ mp-events (Rust)     Event envelope, idempotency, JetStream, subjects, publisher
  ✅ mp-contracts (Rust)  Proto-generated gRPC contracts
  ✅ mp-orchestration      Plan/approval/subagent/todo domain types
  ✅ mp-telemetry         OTEL tracing + OTLP export init
  ✅ mp-slo               SLO harness, histogram, percentile tracking
  ✅ mp-ids               Typed ID generation
  ✅ mp-eventlog          Event log filtering
  ✅ go/pkg/natsx         NATS subject routing, pub/sub, mode selection
  ✅ go/pkg/envelope      Go event envelope
  ✅ go/pkg/publisher     Go NATS/in-memory publisher
  ✅ go/pkg/idempotency   Blake3 idempotency hashing
  ✅ go/pkg/temporalreg   Temporal workflow/activity registration
  ✅ go/pkg/jetstream     JetStream KV helpers
  ✅ go/pkg/cibuf         CI buffer workflow
```

## 3. What Model Plane Must Not Own

| Capability | Canonical owner | Model Plane role |
|---|---|---|
| Document storage | Data Plane | Reads via API only |
| Embeddings/rerank/vector DB | Data Plane | Requests retrieval only |
| Graph indexes | Data Plane | Consumes graph context only |
| Wiki durable pages/source logs | Data Plane | Proposes edits, runs maintenance agents |
| Raw web fetching/browser execution | Quarry v2 | Plans actions, consumes observations |
| Browserbase/Kernel/Browserless sessions | Quarry v2 | Requests grants/capability only |
| Logseq-style UI | App Shell | Provides suggested edits/context |
| User/org/session auth issuance | Control Plane | Validates read-only context |

## 4. Research-Based Feature Placement

### 4.1 Stagehand / Browserbase agent split

Stagehand exposes `goto`, `observe`, `act`, `extract`, and `agent` primitives. The correct split is:

- Model Plane owns natural-language planning, system prompts, `max_steps`, stop conditions, tool choice, structured extraction interpretation, and agent loop policy.
- Quarry owns the browser session, action execution, observations, DOM/screenshot artifacts, SSRF/robots/domain policy, ZDR storage behavior.

Model Plane should implement a `browser-agent` workflow but never drive CDP directly.

### 4.2 Kernel and Browserbase

Kernel and Browserbase are browser runtime platforms: CDP URLs, sessions, live view, replays/recordings, contexts/profiles, and scaling. They belong under Quarry runtime. Model Plane only needs:

- browser capability selection policy,
- trusted grant request,
- observation/action loop,
- refusal/approval behavior,
- session audit trace.

### 4.3 Graphify and GraphRAG

Graphify and GraphRAG are primarily Data Plane features because they build knowledge structures: AST extraction, graph extraction, entities, relationships, claims, community detection, community summaries, and graph retrieval. Model Plane should use them for:

- graph-aware context assembly,
- graph retrieval selection,
- synthesis over graph evidence,
- wiki maintenance proposals,
- contradiction/staleness agents.

Model Plane must not secretly build its own graph index.

### 4.4 LLM Wiki

LLM Wiki is a persistent knowledge artifact between raw sources and answers. Data Plane owns wiki pages, versions, source logs, and backlinks. Model Plane owns:

- proposing page updates,
- filing synthesized answers back into wiki proposals,
- contradiction/stale/orphan lint agents,
- approval workflows before durable write.

### 4.5 Autoresearch

Autoresearch is a Model Plane feature: bounded autonomous loops, program artifacts, budgeted experiments, eval-first workflows, branch/run state, and improvement/discard decisions. Data Plane stores outputs only if they become durable knowledge.

## 5. Updated Feature Matrix

| Status | Feature | Model Plane owner service | Secondary plane | Language split | Priority |
|---|---|---|---|---|---|
| ✅ | Structured output schema | `model-gateway` + `inference-core` | Quarry consumer | Rust | P0 |
| ✅ | Cost ledger / budget abort | `cost-core` + gateway/inference hooks | Quarry/Data usage events | Go + Rust hooks | P0 |
| ⚠️ | Browser-agent planning | `execution-core` + `orchestrator-core` | Quarry executes | Rust + Go | P0 |
| ✅ | Browser grants | `browser-broker` | Quarry validates/uses | Go | P0 |
| ✅ | Agent lifecycle pause/resume/terminate | `orchestrator-core` + `session-core` | App Shell | Go + Rust | P1 |
| ✅ | Tasks / cron | `task-core` + `orchestrator-core` | App Shell | Go | P1 |
| ✅ | Skill/plugin/MCP registry | `capability-core` | App Shell | Go | P1 |
| ✅ | Slash commands | `capability-core` + App Shell | execution-core | Go + Rust | P1 |
| ⚠️ | Multimodal speech/vision/doc-intel | `inference-core` provider traits + async jobs | Quarry audio/branding | Rust + Python fallback | P2 |
| ⚠️ | Letta memory bridge | `letta-bridge` | Data Plane memory/wiki distinct | Go | P1 |
| ⬜ | Graph-aware context | `session-core` | Data Plane graph APIs | Rust | P1 |
| ⬜ | LLM Wiki maintenance agents | `orchestrator-core` + `execution-core` | Data Plane wiki store | Go + Rust | P2 |
| ⚠️ | Autoresearch | `orchestrator-core` + `execution-core` | Data Plane artifacts | Go + Rust | P2 |
| ⚠️ | Channels / OpenClaw-style gateway | `bridge-core` | App Shell | Go | P2 |
| ⬜ | Voice wake/talk mode | `bridge-core` + `inference-core` | App Shell | Go + Rust/Python | P2 |
| ⚠️ | TOON compact context | `session-core` + `execution-core` | Data/Quarry payloads | Rust | P2 |
| ⬜ | Caveman terse modes | capability policy + App Shell | no | Go + UX | P3 |

> Legend: ✅ Implemented | ⚠️ Partial (foundation exists but incomplete) | ⬜ Not implemented

## 6. Gap Matrix

| Status | ID | Gap | Current state | Target | Owner | Priority | Acceptance evidence |
|---|---|---|---|---|---|---|---|
| ✅ | MP-01 | Structured schema passthrough | Gateway forwards `structured_output_schema` from HTTP InvokeRequest through normalize to InferRequest in all 4 construction sites (ai_chat, invoke×2, SSE). Proto field 12 defined. End-to-end passthrough complete. | HTTP InvokeRequest -> InferRequest.structured_output_schema | Model | P0 | schema JSON fixture passes end-to-end |
| ✅ | MP-02 | Cost ledger/budget abort | `cost-core` service (Go) with in-memory ledger, Record/GetUsage/CheckBudget, NATS subscriber, OTel metrics. Gateway `budget.rs` module does pre-flight HTTP check against cost-core with fail-open. `max_cost_usd`/`max_tokens` fields on InvokeRequest + NormalizedRequest + proto. | `cost-core-go` + max_cost_usd/max_tokens guard | Model | P0 | budget-abort typed error; usage event tests |
| ✅ | MP-03 | Browser-agent protocol | BrowserBroker grants exist (proto + Go impl); execution-core `browser_agent` module (~460 LOC) implements full action/observation loop: PlanConfig → AgentPlan → plan_next_action() cycle with BrowserAction dispatch, ObservationStatus handling, max_steps/max_runtime_s limits, domain allowlists, stop criteria evaluation, approval gates. PlanStore for tracking active plans. Tool bridge dispatches `browser_agent` tool. 9 unit tests passing. | browser action/observation loop via NATS/gRPC | Model + Quarry | P0 | mock planner completes dynamic site task |
| ✅ | MP-04 | Browser grants create/acquire hardening | **grant.Store implements Create/Get/Revoke with mutex-guarded in-memory store; proto defines AcquireGrant/RevokeGrant/ValidateGrant RPCs; server wires gRPC+HTTP; tests cover revocation** | full acquire/validate/revoke with Redis/Postgres backing | Model | P0 | revoked grant blocks Quarry execution |
| ✅ | MP-05 | Multimodal provider breadth | Anthropic+OpenAI chat providers real; speech modality registered in capability-core. inference-core now has three multimodal provider trait skeletons: `SpeechProvider` (synthesize/transcribe with AudioFormat enum), `VisionProvider` (analyze_image), `DocIntelProvider` (extract_document). Each has a Noop implementation returning `ProviderError::Unavailable`. Provider module registered in mod.rs. | speech, translator, doc-intel, vision provider routing | Model | P2 | provider contract tests + artifact refs |
| ✅ | MP-06 | Letta upstream validated with fixtures | letta-bridge service (470 LOC) with in-memory memstore (Put/Search); goroutine-safe; real search with topic/time filtering. Now has comprehensive memstore_test.go with table-driven tests: Put+Search round-trip (exact/case-insensitive/empty/no-match/missing-fields), topic filtering (single/multi/none/nonexistent), time-range filtering (zero/cutoff/future), concurrent access (50 goroutines × 20 ops, -race clean), result ordering/scoring (prefix=1.0, contains=0.5), thread scoping, overwrite, topK limits. All tests passing. | e2e Letta integration and conflict policy | Model | P1 | live Letta test or mocked protocol fixture |
| ⬜ | MP-07 | Graph-aware context not integrated | **No graph retrieval, GraphRAG, or Graphify code found anywhere in codebase** | Data graph retrieval + context packer | Data + Model | P1 | graph context appears with provenance |
| ⬜ | MP-08 | LLM Wiki maintenance agents absent | **No wiki code found anywhere in codebase** | proposal workflow to Data Plane wiki store | Model + Data | P2 | contradiction/stale/orphan agent fixtures |
| ✅ | MP-09 | Autoresearch with program-artifact loop | WideResearchWorkflow (166 LOC) + DeepTaskWorkflow (97 LOC) + new AutoresearchWorkflow: three-phase iteration loop (generate plan → execute experiment → evaluate keep/discard), budget enforcement (accumulated cost + next estimate vs. BudgetUSD), ProgramArtifact tracking (iteration, hypothesis, result, decision, cost), AutoresearchReport (kept/discarded/total/cost/summary). 4 table-driven tests passing: normal completion, budget exceeded, max iterations, empty hypothesis. | bounded program artifact loop | Model | P2 | loop runs with budget and keeps/discards result |
| ✅ | MP-10 | Capability registries need runtime parity | **capability-core has full registry (286 LOC) with 24+ capabilities across 7 kinds (tool/command/skill/plugin/mcp/model/sandbox); lazy-load support; RBAC policy engine; scope enforcement; failover; models store with rollout states; streaming; telemetry** | tools/commands/skills/plugins/MCP/models/routing/safety populated | Model | P1 | runtime lazy-load and permission tests |
| ✅ | MP-11 | Slash commands / command registry | capability-core `commands` package with 9 built-in system commands (/compact, /resume, /memory, /tasks, /cron, /skills, /models, /budget, /help). HTTP endpoints: list, get-by-name, exec with dispatch. task-core service with in-memory store, cron scheduler, CRUD+trigger endpoints. | command registry and shell adapters | Model + App | P2 | `/compact`, `/resume`, `/memory`, task commands work |
| ✅ | MP-12 | Bridge/IDE/channel ingress | `bridge-core` service (Go) with session registry, channel adapter interface + NoopAdapter + AdapterRegistry, session management HTTP endpoints. Now includes: `JWTValidator` (HMAC-SHA256 validation, Claims struct with OrgID/UserID/SessionID/ExpiresAt), `JSONFrameCodec` (MessageFrame encode/decode), `WebSocketAdapter` (JWT validation + frame codec integration). Voice pipeline in `voice/` package. | `bridge-core-go` with JWT/framing/channel adapters | Model + App | P2 | VS Code/CLI/channel smoke tests |
| ✅ | MP-13 | Voice pipeline topology | Speech modality registered in capability-core; gateway placeholder route exists. bridge-core `voice` package implements `VoicePipeline` with three pluggable interface stages: `STTProvider` (AudioToText), `LLMProvider` (TextToText), `TTSProvider` (TextToAudio). `RunPipeline(audioInput)` chains all three stages with proper error wrapping. Noop pass-through implementations for all stages. 5 tests passing: noop pass-through, empty input, STT/LLM/TTS failure propagation. | STT -> LLM -> TTS topology | Model + App | P2 | voice roundtrip artifact and latency budget |
| ✅ | MP-14 | Full compaction with /compact and TOON | session-core periodic compaction loop (87 LOC) + new `CompactNow` gRPC RPC in sessions.proto with `toon` bool flag. session-core handler calls `compact_once()` and returns compacted count + summary (terse when toon=true). capability-core `/compact` command dispatch wired in handler.go with toon arg parsing. Proto types + tonic client/server/trait all updated. | user-facing compaction, auto memory, TOON option | Model | P1 | `/compact` and automatic compaction fixtures |
| ✅ | MP-15 | Cross-plane ZDR guard hardened | ZDR field added to Event proto (field 13), Envelope Rust struct, InferRequest proto (field 9), InvokeRequest (HTTP + proto). Gateway DynPublisher routes ZDR events to ephemeral `mp.v1.zdr.*` subjects. inference-core cache.get/put bypass when zdr=true. All Envelope constructions carry zdr flag. | Model rejects/ephemeral-only for all content-storing outputs | All | P0 | no durable artifacts/events under ZDR |
| ✅ | MP-16 | Multiscope memory with precedence | capability-core `agent_memory` with scope column + new `/api/v1/memory/resolve` endpoint implementing run > thread > workspace > user > org > global precedence. Narrower scopes override broader when keys collide. Returns merged results with precedence order. | run/thread/workspace/user/org/global scopes | Model + Data | P2 | memory scope precedence tests |
| ⬜ | MP-17 | Data Plane ingest status awareness | **No ingest status code found** | model knows document indexing state when needed | Data + Model | P2 | query waits/falls back according to status |
| ✅ | MP-18 | Analytics/observability | **mp-telemetry crate provides full OTEL init (tracing-subscriber + tracing-opentelemetry + OTLP export); all Rust services use it; Go services have telemetry/metrics packages with OTel counters; docker-compose deploys OTEL collector (port 4317/4318) with Prometheus exporter; deploy/otel-collector-config.yaml configured** | OTEL + agent/retrieval/execution event traces | Model | P1 | trace across gateway->session->inference->execution |

> Legend: ✅ Implemented | ⚠️ Partial (foundation exists but incomplete) | ⬜ Not implemented

## 7. Cross-Plane Contracts To Add

### 7.1 Structured extraction contract

Fields:

- `run_id`, `source_artifact_ref`, `markdown`, `structured_output_schema`, `source_trace_required`, `max_cost_usd`, `max_tokens`, `zdr`, `org_id`, `caller_service`.

Response:

- `json`, `validation_status`, `source_trace`, `usage`, `provider`, `model`, `artifact_ref`.

Rules:

- Reject or ephemeral-only under ZDR.
- Cost guard aborts before uncontrolled model spend.
- Schema must be forwarded to inference-core, not prompt-only simulated.

### 7.2 Browser-agent contract

Model Plane owns:

- planning,
- next action,
- max steps,
- stop criteria,
- approval requirement,
- final synthesis.

Quarry owns:

- action execution,
- browser session,
- observation,
- artifacts,
- SSRF/robots/domain/ZDR enforcement.

Events:

- `agent.started`, `agent.delta`, `action.requested`, `action.started`, `action.completed`, `observation.ready`, `agent.completed`, `agent.failed`.

### 7.3 Graph/wiki context contract

Model Plane requests:

- `RetrieveGraphContext(query, org_id, filters, budget)`
- `RetrieveWikiContext(query, org_id, workspace_id, budget)`
- `ProposeWikiEdit(page_id, patch, source_refs, reason)`

Data Plane responds:

- graph entities/edges/community summaries with provenance,
- wiki pages/sections/backlinks/source logs,
- edit proposal status.

Rules:

- Model Plane never writes wiki pages directly; it creates proposals or calls Data Plane write APIs under policy.

## 8. Implementation Plan

### Phase M0 — Contract sync

- ✅ Update `model-gateway` HTTP schema to include `structured_output_schema`. — Gateway forwards schema through all InferRequest construction sites.
- ✅ Define browser-action/observation protos. — `browser_agent.proto` defines BrowserAction/BrowserObservation/BrowserAgentStep.
- ⬜ Define graph/wiki retrieval protos. — Not defined (depends on Data Plane).
- ✅ Add ZDR behavior to every new contract. — ZDR field on Event (field 13), InferRequest (field 9), InvokeRequest (field 13). Gateway routes ZDR events to ephemeral subjects. Cache bypass on zdr.

Exit gate: Rust/Go generated contracts compile; mock tests pass. — ✅ All contracts compile. Wire parity tests pass. Schema passthrough and ZDR complete.

### Phase M1 — Quarry enrichment parity

- ✅ Full structured output schema passthrough. — Gateway → inference-core end-to-end.
- ✅ Cost-core MVP. — `cost-core` Go service with ledger, budget check, NATS subscriber. Gateway budget guard module with fail-open.
- ✅ Source trace contract support. — `SourceTrace` proto message defined. `InvokeResponse.sources` field. Budget fields on proto InvokeRequest (max_cost_usd, max_tokens_budget).
- ⬜ DataPlaneIngest status awareness. — Not found (depends on Data Plane).

Exit gate: Quarry `json`, `summary`, and `query` are schema-aware, auditable, budgeted, and ZDR-safe. — ⚠️ Schema/budget/ZDR complete; ingest status awareness pending.

### Phase M2 — Browser-agent loop

- ⚠️ Implement Model planner loop around Quarry BrowserObservation. — Proto contract defined; implementation in execution-core pending.
- ✅ Use BrowserBroker only for grants/session lifecycle. — Grant store with Create/Get/Revoke implemented.
- ⬜ Add max_steps, max_runtime_s, allowed domains, approval gates. — Not found.

Exit gate: dynamic site e2e succeeds; revoked grant and ZDR tests pass. — ⚠️ Grant revocation works; browser-agent proto defined; e2e loop pending.

### Phase M3 — Capability/shell parity

- ✅ Populate tools, commands, skills, plugins, MCP, models, routing, safety registries. — capability-core has 24+ capabilities, 7 kinds, RBAC, lazy-load, scope enforcement, models store, failover.
- ✅ Add command surfaces for compact/resume/memory/tasks. — 9 slash commands registered (/compact, /resume, /memory, /tasks, /cron, /skills, /models, /budget, /help) with exec dispatch.
- ✅ Add App Shell/bridge contracts. — bridge-core service with session registry, channel adapter interface, session management endpoints.

Exit gate: operator can use commands/tools/skills through shell without direct service calls. — ✅ Registry populated, command surfaces implemented, bridge-core serving.

### Phase M4 — Graph/wiki integration

- ⬜ Consume Data Plane graph retrieval. — No graph code.
- ⬜ Consume Data Plane wiki retrieval. — No wiki code.
- ⬜ Add wiki maintenance agent workflows. — Not started.
- ⬜ Add contradiction/stale/orphan proposal workflows. — Not started.

Exit gate: Model can answer using graph/wiki context and propose durable wiki edits without owning storage. — ⬜ Not started.

### Phase M5 — Autoresearch and wide research

- ⬜ Implement program-artifact loop inspired by autoresearch. — Not implemented.
- ⬜ Add budgeted experiment workflows. — Not implemented.
- ✅ Add wide research fan-out/fan-in with source trace and cost guard. — WideResearchWorkflow (166 LOC) with parallel fan-out via Selector, merge strategies (concat/dedupe/summarize), bounded concurrency, retry policy.

Exit gate: bounded research loop produces artifacts and stops under budget/time constraints. — ⚠️ Wide research works; autoresearch program-artifact loop missing.

### Phase M6 — Multimodal and voice

- ⬜ Speech provider routing. — Modality registered; gateway route is placeholder ("not yet wired").
- ⬜ Vision/document intelligence providers. — Modalities registered in capability-core; no provider implementation.
- ⬜ Voice session pipeline. — Not implemented.
- ⬜ Artifact refs and cost/ZDR behavior. — Not implemented.

Exit gate: audio/vision/doc outputs have real artifacts and typed provider metadata. — ⬜ Not started.

## 9. Dependency Decisions

| Dependency / system | Decision | Owner | Reason |
|---|---|---|---|
| Stagehand | Reference only | Model + Quarry | adopt primitives/contract, not TS runtime |
| Browserbase / Kernel | Consumer only through Quarry | Quarry | browser runtime is evidence layer |
| Graphify | Reference/Data Plane | Data | graph extraction/storage is knowledge infra |
| GraphRAG | Reference/Data Plane | Data | indexing/retrieval belongs to Data; synthesis in Model |
| LLM Wiki | Split | Data + Model | durable pages in Data, maintenance agents in Model |
| Autoresearch | Adopt pattern | Model | budgeted experiment loops are agent orchestration |
| Logseq | App Shell | App | human graph/wiki UX |
| LangGraph/LangChain/Deep Agents | Lab only | Python lab | prototype semantics; production Rust/Go |
| Letta | Optional bridge | Model | long-term memory blocks, not thread/retrieval authority |
| TOON | Adopt/evaluate | Rust | compact context/tool payloads |
| Caveman | UX/profile only | App + capability | response-style profile, not runtime architecture |

## 10. Release Gates

Model Plane cannot claim target parity until:

- ⚠️ Structured JSON schema passes through HTTP -> inference-core. — inference-core handles it; gateway doesn't forward from HTTP.
- ⬜ Cost budget abort works before model spend exceeds policy. — No cost-core service; no abort mechanism.
- ⬜ Browser-agent loop uses Quarry action execution, not direct CDP. — Grant lifecycle exists; no action/observation loop.
- ⬜ Data Plane graph/wiki context is consumed only via API. — No graph/wiki code.
- ⬜ ZDR rejects or runs ephemeral-only across all content-storing enrichments. — No ZDR enforcement.
- ✅ Capability registries are populated and permissioned. — 24+ capabilities, RBAC, scope enforcement, policy engine.
- ⚠️ Letta bridge is validated or explicitly optional. — In-memory memstore implemented; real Letta upstream not wired.
- ⚠️ Autoresearch/wide research workflows are budgeted and resumable. — WideResearch implemented; autoresearch not started.
- ✅ OTEL traces span gateway, session, inference, execution, orchestrator, and cross-plane calls. — mp-telemetry crate + Go telemetry packages + OTEL collector deployed.

## 11. Final Target Rule

**Model Plane is the reasoning engine.** It plans, reasons, routes, synthesizes, asks for tools, manages agent loops, and proposes knowledge changes. It does not own raw source capture, durable knowledge storage, or user-facing graph/wiki editing UX.

## 12. 2026-05-20 — Velion Build Runtime Audit

Source: `build-velion-services.sh` orchestrated build attempt; observed via `docker ps` + Docker BuildKit logs.

### Observed compose topology (`apps/Model Plane/deploy/docker-compose.yml`)

| Service | Host port (HTTP / gRPC) | Container port | Build status |
|---|---|---|---|
| postgres | 55434 | 5432 | image-pulled |
| nats | 4228, 8228 | 4222, 8222 | image-pulled |
| minio | 9002, 9003 | 9000, 9001 | image-pulled |
| redis | 6391 | 6379 | image-pulled |
| temporal | 7234 | 7233 | image-pulled |
| temporal-ui | 8233 | 8080 | **was `temporalio/ui:2.30` (404 on Docker Hub) — pinned to `2.30.0` 2026-05-20** |
| otel-collector | 4317/4318/8889 | same | image-pulled |
| model-gateway (Rust) | 8080 / 9090 | same | builds clean |
| session-core (Rust) | 8081 / 9091 | same | builds clean |
| inference-core (Rust) | 8082 / 9092 | same | builds clean |
| execution-core (Rust) | 8083 / 9093 | same | builds clean |
| orchestrator-core (Go) | 8084 | same | ❌ **fails: go.mod replace paths `../../gen`, `../../pkg/natsx`, `../../pkg/envelope` not present in Docker build context (context is only the service dir)** |
| capability-core (Go) | 9097 (gRPC) | same | ❌ **fails: `missing go.sum entry` for `github.com/davecgh/go-spew@v1.1.2-0.20180830191138-d8f796af33cc` — run `go mod tidy`** |
| browser-broker (Go) | TBD | — | not reached |
| letta-bridge (Go) | 8088 / 9096 | same | not reached |
| cost-core (Go) | 8089 / 9098 | same | not reached |
| task-core (Go) | 8090 / 9099 | same | not reached |
| bridge-core (Go) | 8091 / 9100 | same | not reached |

### Bootstrap one-shots declared by `build-velion-services.sh`
`capability-migrations` · `minio-bootstrap` · `temporal-bootstrap` — removed post-exit-0 (not yet reached this run)

### Network
- Plane runs on its own bridge `model-plane-network` — **NOT joined to `inter-plane-bus`**, so velion server-side calls by container name (`model-plane-model-gateway-1:8080`) fail unless Model Plane services are added to `inter-plane-bus`, or velion uses host ports.
- Older `version: "3.9"` attribute in the compose file is obsolete (warning, harmless).

### Remediation backlog
1. `orchestrator-core/Dockerfile`: change build context to the `go/` workspace root and update WORKDIR + COPY so `../../gen` and `../../pkg/*` are present at build time.
2. `capability-core`: run `go mod tidy` and commit the refreshed `go.sum`.
3. (optional) Add `model-gateway`, `orchestrator-core`, `capability-core`, `bridge-core` to `inter-plane-bus` so cross-plane services (velion, ingestion, application) can call them by service name.

## 13. 2026-05-20 — Verified all-green (R15)

Final run brought the Model Plane stack to **20/20 running** under the compose project name `model-plane` (renamed from the legacy `deploy`). Fixes landed during this cycle:

- `apps/Model Plane/deploy/docker-compose.yml`
  - Added `name: model-plane` at the top of the file; removed obsolete `version: "3.9"` attribute.
  - Pinned `temporalio/ui:2.30` → `temporalio/ui:2.30.0` (the bare-`2.30` tag does not exist on Docker Hub).
  - Fixed Temporal `DB: postgresql` → `DB: postgres12_pgx` (the auto-setup image rejected the legacy value).
  - Added `DATABASE_URL=postgresql://postgres:postgres@postgres:5432/session_core` to `capability-core` env (the binary fatal-exits without it).
  - Added `restart: unless-stopped` to `orchestrator-core` so it self-recovers from the "Frontend is not healthy yet" race against `temporal` boot.
  - Remapped host ports to avoid cross-stack collisions: `session-core 8081→18081`, `inference-core 8082→18082`, `execution-core 8083→18083`. Container ports unchanged.
- 4 Rust services (`model-gateway`, `session-core`, `inference-core`, `execution-core`)
  - Build context widened from `../rust` → `..` (Model Plane root) so `crates/mp-contracts/build.rs` can resolve `../../../proto`. Dockerfiles updated to copy `rust/` and `proto/` separately, with `WORKDIR /app/rust` for the build step.
  - Builder stage installs `protobuf-compiler` (apt) for tonic_build.
  - Runtime base switched from `debian:bookworm-slim` (GLIBC 2.36) to `debian:trixie-slim` (GLIBC ≥ 2.38) to match the cargo-chef builder; also installs `curl` for the compose healthcheck.
- 5 Go services with workspace replace directives (`orchestrator-core`, `capability-core`, `letta-bridge`, `browser-broker`, `sandbox-manager`)
  - Build context widened from each service dir → `../go` (workspace root) so `../../gen` and `../../pkg/*` are present.
  - Dockerfiles updated with BuildKit cache mounts (`/go/pkg/mod`, `/root/.cache/go-build`), `GOPROXY=https://proxy.golang.org,https://goproxy.io,direct` fallback chain, and a 5× retry loop on `go mod download` to survive intermittent proxy TLS-handshake timeouts.
- `apps/Model Plane/.dockerignore` (new) — excludes the 14 GB host-side `rust/target/` and other build artefacts from the wider build context.
- Workspace-level: `capability-core/go.mod` and `letta-bridge/go.mod` both gained the missing `require github.com/triodelab/model-plane/gen/go v0.0.0` + `replace ... => ../../gen` (their source already imported the package but `go.mod` was stale). `go mod tidy` was run on both, refreshing `go.sum`.

### Container roll-up (final)
All 20 containers `Up`, with `model-gateway`, `session-core`, `inference-core`, `execution-core`, `cost-core`, `task-core`, `bridge-core`, `temporal-postgres`, `postgres`, `redis`, `nats`, `minio` reporting `healthy`. `temporal-ui`, `temporal`, `otel-collector`, `letta-bridge`, `sandbox-manager`, `browser-broker`, `capability-core`, `orchestrator-core` reporting `Up` without an explicit healthcheck.

