# CoreSystem Master Ownership Matrix

> **Runtime note — 2026-07-13:** the ownership rules below remain canonical,
> but they do not certify current availability. Model Plane gateway/inference
> gRPC is absent in the running stack. Authenticated additive source contracts
> and the ordinary invoke caller graph pass tests but are not deployed; the safe
> rebuild gate is documented in
> [Model Plane status](Model%20Plane/MODEL_PLANE_STATUS.md).

> **Application authority note — 2026-07-13:** Control Plane is the source of truth
> for identity and exact organization membership. Application Plane may maintain
> revocable projections, owns the canonical notification request API and application
> delivery ledgers, and must fail closed when Control authority is unavailable.
> Frontend/gateway and Ingestion workers are clients; neither caller-supplied scope
> nor a local projection can grant access. See the dated Application Plane
> [audit](Application%20Plane/docs/core-research/plane-audit-2026-07-13.md).

> Generated: 2026-05-06  
> Scope: Quarry v2, Data Plane, Model Plane, App/Shell, and future reference tooling.  
> Rule: **Quarry captures evidence. Data Plane knows. Model Plane reasons. App Shell presents.**
> Privacy contract: [`GDPR_SUMMARY.md`](./GDPR_SUMMARY.md).

## 0. Decision Rules

| Question | Owner |
|---|---|
| Does it fetch, browse, screenshot, render, crawl, replay, or produce deterministic source evidence? | **Quarry v2** |
| Does it store, chunk, embed, index, retrieve, graph, version, or maintain durable knowledge? | **Data Plane** |
| Does it plan, reason, synthesize, act as an agent, choose tools, write memory/wiki updates, or run research loops? | **Model Plane** |
| Does it expose human-facing graph/wiki/IDE/CLI/canvas/voice UX? | **App Shell** |
| Is it latency-sensitive, CPU/memory-sensitive, parsing-heavy, retrieval-heavy, browser-runtime-heavy, or protocol-heavy? | **Rust** |
| Is it durable workflow, registry, policy, scheduling, resource CRUD, grants, billing, or operator control? | **Go** |
| Is it eval/research/model-lab/provider-specific ML glue only? | **Python** |

## 1. Canonical Plane Matrix

| Tool / feature | Quarry v2 | Data Plane | Model Plane | App Shell | Rust | Go | Python |
|---|---|---|---|---|---|---|---|
| Static fetch / TLS impersonation | **Owner** | — | — | — | `quarry-tls`, `quarry-runtime` | — | lab benchmark only |
| Browser actions | **Owner** | — | Consumer via agent contract | UI inspector | `quarry-browser`, `ActionRuntime` | browser lease admin | browser-use lab only |
| Browserbase sessions | **Owner** | — | Requests browser capability | session viewer link | Browserbase CDP driver | profile/grant lifecycle | no |
| Kernel browsers / kernel-images | **Owner** | — | Requests runtime lease | live view / replay links | CDP adapter / VM session driver | lease, quota, policy | no |
| Browserless | **Owner** | — | Requests browser capability | session viewer link | CDP-over-WS driver | config/admin | no |
| Stagehand `goto/observe/act/extract` low-level browser operations | **Owner for execution + observations** | optional source artifact ingest | **Owner for natural-language planning** | debug/playback UI | action/observation protocol | grant/job lifecycle | reference SDK only |
| Stagehand Agent / autonomous workflow | Executes browser steps only | stores resulting evidence if ingested | **Owner** | task UI | browser observation execution | Temporal orchestration | prototype only |
| Browser Use / computer-use screenshot agents | Executes screenshots/actions | stores evidence artifacts only | **Owner** for VLM/action decisions | live view | screenshot/action runtime | grants/quotas | prototype only |
| Firecrawl-like scrape formats | **Owner** for html/raw/markdown/links/images/screenshot/pdf/chunks/change/meta | optional ingest | summary/json/query generation only | API docs | transforms | job/control | no |
| AI `summary` / `json` / `query` formats | Captures source + stores artifacts | retrieval facts | **Owner** | display | bridge/output validation | jobs/cost | provider-specific only |
| `audio` format | evidence/reference only | optional artifact metadata | **Owner** speech provider routing | playback UX | artifact handling | async job/event flow | possible SDK glue |
| Static branding extraction | **Owner** | optional metadata storage | visual fallback only | display | CSS/HTML parser | catalog/policy | no |
| Rendered/visual branding | captures screenshot/DOM | stores metadata if promoted | **Owner** visual interpretation | display | screenshot capture | async job | vision lab/provider |
| Data Plane document ingest from Quarry | Sends markdown/chunks/meta | **Owner** | consumes retrieval later | status display | payload shaping | lifecycle orchestration | no |
| Documents service | writes via API only | **Owner** | reads only via API | doc admin UI | heavy transforms only | **documents-api-go** | legacy/prototype |
| Chunking / knowledge-unit creation | source producer only | **Owner** | consumer only | inspection UI | **index-engine-rs** | reindex orchestration | eval only |
| Embedding generation | no | **Owner** | no independent embedding | status UI | **embedding-engine-rs** | batch control | provider SDK fallback only |
| Vector retrieval / rerank / source join | no | **Owner** | consumer only | search UI | **retrieval-engine-rs** | control/rebuild | eval only |
| Hybrid retrieval (BM25 + dense + rerank) | no | **Owner** | chooses retrieval mode | search UI | retrieval engine | policy/config | eval only |
| Graphify-style corpus graph | captures raw sources | **Owner** graph extraction/storage | graph-aware context use | graph viewer | AST/graph extraction | graph registry/jobs | graphify reference/lab |
| GraphRAG indexing | no | **Owner** | consumes graph context | operator rebuild UI | graph extraction/retrieval | rebuild schedules | prototype/reference |
| GraphRAG query synthesis | no | returns graph context | **Owner** synthesis | answer UI | context packing | task policy | eval |
| LLM Wiki durable pages/versioning/source log | source capture only | **Owner** wiki storage/index | proposes updates | wiki UX | page diff/index | lifecycle/policy | lab |
| LLM Wiki maintenance agents | no | stores accepted pages | **Owner** contradiction/stale/orphan agents | review UI | synthesis helpers | workflows/approval | eval |
| Logseq-like graph/page UX | no | backs page/graph APIs | provides suggested edits | **Owner** | local transforms if needed | app API control | no |
| Autoresearch loops | no | stores experiment artifacts/results if needed | **Owner** | research dashboard | execution steps | Temporal loops/budgets | experiment runtime |
| OpenClaw gateway/channels | no | no | routing decisions | **Owner for UX** | runtime execution | channel/gateway control | no |
| Hermes tools/toolsets/MCP/cron | no | memory persistence only | **Owner** | CLI/channel UX | runtime tool execution | capability/orchestrator | no |
| Codex/Claude Code-style local coding shell | no | source/project knowledge only | **Owner** coding agent | **Owner** CLI/IDE | execution core | bridge/control | no |
| TOON compact transport | can encode artifacts/observations | encode retrieval facts/wiki tables | encode tool/context payloads | display/debug | **codec-rs** | registry policy | no |
| Caveman terse mode | no | no | response-format policy | **UX owner** | formatting runtime | config policy | no |
| MCP registry | no | no | **Owner** capability use | shell integrations | runtime adapter | **capability-core-go** | no |
| Plugin registry | no | no | **Owner** runtime policy | shell/admin UI | plugin execution boundary | **capability-core-go** | no |
| Tasks / cron | no | reindex/refresh jobs only | **Owner** for agent tasks | task UI | step execution | **orchestrator/task-core-go** | no |
| Cost ledger / budgets | ZDR/cost metadata only | embed/retrieval cost | **Owner** inference/agent cost | billing display | metering hooks | **cost-core-go** | no |
| Evals / scoreboards | capture benchmarks | retrieval benchmarks | agent/model benchmarks | dashboards | benchmark targets | CI/go test harness | **lab/evals only** |

## 2. Service Target Structures

### Quarry v2 target

| Service / crate | Language | Owner scope |
|---|---|---|
| `quarry-edge` | Rust | Public REST/SSE, request normalization, ZDR guards, cache admission, Model/Data bridge client |
| `quarry-runtime` | Rust | PageRunner, DriverPlan waterfall, security policy, deterministic execution |
| `quarry-browser` | Rust | Chromiumoxide, Browserless, Browserbase, Kernel CDP adapters, action runtime, observations |
| `quarry-transform` | Rust | Markdown/html/links/images/attributes/chunks/diff/branding/static outputs |
| `quarry-security` | Rust | DNS guard, URL signatures, blocklist hints, SSRF enforcement |
| `quarry-control` | Go | Jobs, runs, schedules, profiles, presets, webhooks, resource CRUD |
| `quarry-orchestrator` | Go | Durable crawl/batch/scrape workflows, BFS, checkpoints, pause/resume/backfill |
| `lab/evals` | Python | Firecrawl/Browserbase/Kernel benchmark adapters only |

### Data Plane target

| Service | Language | Owner scope |
|---|---|---|
| `documents-api-go` | Go | Document CRUD, metadata, org-scoped lifecycle, status, internal ingest |
| `index-engine-rs` | Rust | Chunking, normalization, AST/code graph extraction, dedupe/fingerprint, knowledge units |
| `embedding-engine-rs` | Rust | Batch embedding, backpressure, retries, provider parity, Qdrant upsert/delete |
| `retrieval-engine-rs` | Rust | Hybrid retrieval, query embedding, ANN, BM25/sparse, rerank, source join, context packaging |
| `graph-index-rs` | Rust | Graphify/GraphRAG extraction, entity/edge/community summaries, provenance labels |
| `wiki-store-go` | Go | Wiki pages, versions, source log, maintenance log, permissions, retention |
| `data-orchestrator-go` | Go | Reindex, rebuild, refresh, compaction, graph/wiki jobs, queue inspection |
| `retrieval-eval-py` | Python | RAG evals, chunking/rerank experiments, offline scoring only |

### Model Plane target

| Service | Language | Owner scope |
|---|---|---|
| `model-gateway` | Rust | Public invoke, stream, structured output schema passthrough, ZDR/cross-plane guards |
| `session-core` | Rust | Threads, runs, checkpoints, memory index, compaction, context assembly |
| `inference-core` | Rust | Provider routing, streaming, structured outputs, prompt cache, speech/vision/doc provider traits where native |
| `execution-core` | Rust | Tool loop, hooks, browser-agent observation/action protocol, secret scrub, subagents |
| `orchestrator-core` | Go | Temporal workflows, autoresearch, wide research, tasks/cron, approvals, recovery |
| `capability-core` | Go | Tools, commands, skills, plugins, MCP, model routing, safety, memory adapters |
| `browser-broker` | Go | Trusted browser grants, validate/revoke, session lease permissions |
| `letta-bridge` | Go | Optional long-term memory block bridge; not thread or retrieval source of truth |
| `cost-core` | Go | Token/cost ledger, budgets, usage events |
| `graph-lab-py` / `eval-lab-py` | Python | LangGraph/LangChain/Deep Agents/eval prototyping; no production hot path |

## 3. Non-negotiable Cross-Plane Contracts

1. **No direct database crossing.** Model and Quarry never connect to Data Plane Postgres/Qdrant directly.
2. **No independent embeddings.** Model Plane and Quarry never embed/rerank independently except in isolated eval labs.
3. **No agent bypass around Quarry policy.** Model Plane can propose browser actions; Quarry executes or rejects them.
4. **ZDR propagates across planes.** If `zeroDataRetention=true`, Quarry rejects or keeps ephemeral-only any Data/Model enrichment that would persist content.
5. **GDPR policy metadata travels with data.** Durable records and processing jobs carry or link to purpose, lawful basis, retention, residency, privacy class, third-party processing allowance, and deletion scope.
6. **Knowledge assets are Data Plane-owned.** Graphs, wiki pages, embeddings, chunks, source logs, contradiction indexes live in Data Plane.
7. **Reasoning is Model Plane-owned.** Autoresearch, graph-aware synthesis, wiki maintenance agents, browser-agent planning live in Model Plane.
8. **UX is App/Shell-owned.** Logseq-like page/graph editing, CLI/TUI, IDE bridge, channel inbox, voice/canvas surfaces live above the core planes.
9. **Identity and membership are Control Plane-owned.** Sensitive callers resolve the exact user/organization decision through the canonical Control authority; active-organization headers, session hints, and Application projections are not grants.
10. **Application projections only narrow access.** Convex, notification, conversation, and other Application services may cache or mirror membership for availability, but a denial/removal must revoke the projection and an authority outage must never create or widen access.
11. **Notification intake is Application Plane-owned.** The canonical contract is `POST /api/v1/notification-requests`; Ingestion support workers and the Frontend gateway own their client calls and must propagate authentication, tenant, retention, and delivery failures honestly.
12. **Provider execution never invents approval authority.** Conversation owns its durable human intent or approved-AI action; Ingestion Integration owns provider execution and the single-use receipt. An effectful call requires a tenant-bound service bearer plus a short-lived signature over the exact durable authorization, actor, tenant, provider effect, payload digest, and idempotency key. Every other issuer, including Model Plane, fails closed until it implements an equivalent durable contract.

## 4. Research Notes Used

- Kernel: cloud browser sessions expose CDP URLs, VM isolation, live view, replay, long-lived session states, and scaling; treat as browser runtime substrate for Quarry.
- Browserbase: Search/Fetch/Browsers/Contexts plus live view and recordings; treat as cloud-browser backend for Quarry and browser-agent substrate for Model Plane.
- Stagehand: primitives `goto`, `observe`, `act`, `extract`, and `agent`; split low-level execution to Quarry and natural-language planning to Model Plane.
- Graphify: multimodal folder-to-knowledge-graph pipeline with AST pass, LLM extraction, NetworkX/Leiden clustering, provenance labels; canonical Data Plane graph extraction reference.
- GraphRAG: LLM extraction of entities/relationships/claims, community detection/summaries, vector store outputs; Data Plane indexing and Model Plane synthesis consumer.
- LLM Wiki: persistent markdown wiki between raw sources and answers; Data Plane owns the durable wiki, Model Plane owns maintenance agents.
- Autoresearch: program-file-driven bounded experiment loop; Model Plane orchestration pattern.
- Logseq: graph/page human knowledge UX; App Shell pattern, not a core plane owner.
- TOON: compact, schema-aware JSON-equivalent prompt encoding; Rust codec in runtime/context layers.
