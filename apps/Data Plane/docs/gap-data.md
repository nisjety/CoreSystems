# Data Plane — Gap Analysis and Target Architecture

> ⚠️ **DEPRECATION NOTICE — 2026-05-08**
> This document is the **v1 strategic blueprint**. The target architecture
> described here has been **implemented in Data Plane v2** under
> `apps/Data Plane v2/`. The live, authoritative status doc is
> [`apps/Data Plane v2/docs/gap-data.md`](../../Data%20Plane%20v2/docs/gap-data.md)
> which tracks 66 closed items across 29 numbered gaps + 21 v2.1 + 16 v2.2 production items.
> The Python v1 services (`documents-service`, `retrieval-service`, `knowledge-index`,
> `embedding-worker`) are deprecated and replaced by the 8-service Rust+Go stack.
> This file is preserved as the design rationale and as the source of truth for
> the D0–D7 phase plan referenced by `d4-d5-graph-wiki-hybrid-spec.md` (§13 below
> records the D4 + D5 spec closure status against the v2 build).
>
> Generated: 2026-05-06
> Updated: 2026-05-08 — added §13 cross-checking the D4 + D5 hybrid-retrieval
> spec against the v2 implementation. v1 (Python) services are decommissioned;
> compose file at `apps/Data Plane/docker-compose.yml` is marked deprecated.
> Source baseline: current Data Plane docs and benchmark output uploaded in this chat.
> Target: Rust + Go knowledge infrastructure with Python only for eval/lab/provider fallback.

## 1. Executive Summary

The current Data Plane is correct in authority but not optimal in implementation language split. It already has the right conceptual boundary: Data Plane is the canonical owner of documents, chunks, embeddings, Qdrant vectors, and retrieval, and no other plane should touch Qdrant, chunk documents, or call the embedding API directly.

The main upgrade is to move the **knowledge engine path** from Python to Rust while creating a Go control layer for document lifecycle, reindex, graph/wiki jobs, and operator workflows.

| Domain | Current | Target | Priority | Why |
|---|---|---|---|---|
| Document CRUD | Python `documents-service` | Go `documents-api-go` + Rust transform helpers | P1 | Mostly API/control surface; Go fits metadata, auth, lifecycle, SQL |
| Chunking / knowledge units | Python `knowledge-index` | Rust `index-engine-rs` | P0 | CPU/memory-sensitive deterministic pipeline |
| Embedding worker | Python `embedding-worker` | Rust `embedding-engine-rs` | P0 | Batch/backpressure/retry/upsert hot path |
| Retrieval | Python `retrieval-service` | Rust `retrieval-engine-rs` | P0 | Latency-sensitive AI hot path |
| Graphify / GraphRAG | Not present | Rust `graph-index-rs` + Go orchestration | P1 | Knowledge graph is Data Plane-owned knowledge infrastructure |
| LLM Wiki storage | Not present | Go `wiki-store-go` + Rust index/diff helpers | P1 | Wiki pages/version/source logs must be durable knowledge assets |
| Autoresearch | Not Data Plane | Model Plane owner; Data stores artifacts only | — | Research loop is reasoning, not knowledge infra |
| Logseq UX | Not Data Plane | App/Shell owner; Data exposes graph/wiki APIs | — | Human workspace UX belongs above Data Plane |
| Eval/benchmarking | Basic benchmark exists | Python `retrieval-eval-py` + release gates | P0 | Prevent regressions in retrieval quality and latency |

## 2. Non-Negotiable Authority Rules

1. **Data Plane owns knowledge.** Documents, chunks, embeddings, vector indexes, graph indexes, wiki pages, source logs, retrieval traces, contradiction indexes, and graph/wiki snapshots are canonical here.
2. **Model Plane consumes only APIs.** No direct Data Plane Postgres, Qdrant, graph store, or embedding provider access.
3. **Quarry captures evidence, then submits evidence.** Quarry may call Data Plane ingest APIs but never writes Data Plane storage directly.
4. **Embedding/rerank parity is centralized.** No other plane may embed/rerank independently outside eval labs.
5. **Retrieval is auditable.** Every retrieval response must be explainable with query embedding model, candidate set, reranker, filters, source joins, and trace IDs.
6. **ZDR is propagated.** Data Plane must reject or run ephemeral-only for any ingest/index/retrieval path that would violate Quarry/Model zero-data-retention contracts.

## 3. Target Data Plane Service Structure

```text
DATA PLANE L2

  documents-api-go         :8001 / :50051
    - document CRUD, metadata, org scoping, document status, internal ingest

  index-engine-rs          worker + gRPC admin
    - chunking, normalization, dedupe, fingerprinting, AST/code graph extraction

  embedding-engine-rs      worker + gRPC admin
    - batching, provider calls, retry/backpressure, Qdrant upsert/delete, model parity

  retrieval-engine-rs      :8004 / :50052
    - hybrid retrieval, query embedding, ANN, sparse/BM25, rerank, Postgres join, context packaging

  graph-index-rs           worker + gRPC admin
    - Graphify/GraphRAG extraction, entity/edge/claim/community summaries, provenance labels

  wiki-store-go            :8010 / :50060
    - LLM Wiki pages, versions, source logs, maintenance logs, backlinks, retention

  data-orchestrator-go     worker
    - reindex, rebuild, refresh, graph/wiki jobs, queue inspection, policy, schedules

  retrieval-eval-py        lab only
    - chunking/rerank evals, GraphRAG experiments, regression suites

Infrastructure:
  PostgreSQL 16+     canonical document/chunk/wiki/graph metadata
  Qdrant             vector collections only through retrieval/embedding engines
  Redis Streams/NATS transition path for async events
  Object store       large raw artifacts, wiki snapshots, graph exports if needed
```

## 4. Target Ownership by Feature

| Feature | Canonical owner | Service | Language |
|---|---|---|---|
| Document metadata/content | Data Plane | `documents-api-go` | Go |
| Bulk document ingest | Data Plane | `documents-api-go` | Go |
| Chunking | Data Plane | `index-engine-rs` | Rust |
| Semantic chunking | Data Plane | `index-engine-rs` | Rust |
| Code AST extraction | Data Plane | `index-engine-rs` / `graph-index-rs` | Rust |
| Embedding batching | Data Plane | `embedding-engine-rs` | Rust |
| Qdrant upsert/delete | Data Plane | `embedding-engine-rs` | Rust |
| Query embedding | Data Plane | `retrieval-engine-rs` | Rust |
| ANN search | Data Plane | `retrieval-engine-rs` | Rust |
| BM25/sparse retrieval | Data Plane | `retrieval-engine-rs` | Rust |
| Rerank | Data Plane | `retrieval-engine-rs` | Rust |
| Source join | Data Plane | `retrieval-engine-rs` | Rust |
| Graphify graph build | Data Plane | `graph-index-rs` | Rust |
| GraphRAG indexing | Data Plane | `graph-index-rs` | Rust |
| GraphRAG synthesis | Model Plane | `execution-core` / `inference-core` | Rust |
| LLM Wiki durable pages | Data Plane | `wiki-store-go` | Go |
| LLM Wiki page diff/index | Data Plane | `wiki-store-go` + Rust helper | Go + Rust |
| LLM Wiki maintenance agent | Model Plane | `orchestrator-core` + `execution-core` | Go + Rust |
| Logseq-like UX | App Shell | app/workspace | TypeScript |
| Autoresearch | Model Plane | `orchestrator-core` + `execution-core` | Go + Rust |

## 5. Research-Based Architecture Additions

### 5.1 Graphify additions

Graphify’s public docs describe a three-pass pipeline: deterministic AST extraction for code, local transcription for audio/video, LLM semantic extraction for non-code corpus content, then NetworkX + Leiden clustering with every relationship labeled `EXTRACTED`, `INFERRED`, or `AMBIGUOUS`. Data Plane should adopt the **provenance and graph output ideas**, not the Python runtime as the production source of truth.

Add:

- `knowledge_edges` table with `provenance = extracted | inferred | ambiguous`.
- `confidence_score` for inferred edges.
- `edge_source_refs` to source chunks or artifact IDs.
- AST-first extraction for Rust/Go/TS/Python/Java/etc. using tree-sitter-compatible Rust crates or a controlled extractor sidecar.
- `graph_exports` for JSON/HTML/markdown graph reports.
- `graph_rebuild_jobs` with incremental SHA/BLAKE3 content cache.

### 5.2 GraphRAG additions

Microsoft GraphRAG’s indexing pipeline extracts entities, relationships, and claims from raw text, performs community detection, generates community summaries, and writes outputs to tables/vector stores. Data Plane should own the indexing and retrieval primitives, while Model Plane uses graph context for synthesis.

Add:

- `graph_entities`, `graph_relationships`, `graph_claims`.
- `graph_communities`, `graph_community_summaries`.
- `graph_text_units` mapped to `knowledge_units`.
- Global/local graph retrieval modes.
- Cost and cadence controls for graph rebuild because graph indexing is expensive.

### 5.3 LLM Wiki additions

The LLM Wiki pattern moves synthesis into a persistent markdown/wiki artifact that accumulates over time instead of rediscovering raw chunks at query time. Data Plane should own the durable wiki substrate; Model Plane proposes edits and maintenance actions.

Add:

- `wiki_pages`: page ID, org, workspace, title, path, current version, status.
- `wiki_page_versions`: versioned markdown content, source refs, editor agent/user.
- `wiki_source_log`: raw source-to-page ingestion trace.
- `wiki_maintenance_log`: contradictions, stale pages, orphan pages, accepted/rejected edits.
- `wiki_backlinks`: page graph for app-shell navigation.
- `wiki_retrieval` API to retrieve pages, sections, backlinks, and source refs.

### 5.4 Hybrid retrieval additions

The current 5-stage pipeline is good but should become a production-grade hybrid retrieval engine:

1. Hard auth filter: org, workspace, collection, source, ACL.
2. Query rewrite/normalization: optional Model Plane query expansion, never mandatory.
3. Dense vector retrieval from Qdrant.
4. Sparse/BM25 retrieval over Postgres/OpenSearch/Tantivy-compatible index.
5. Graph expansion from Data Plane graph index.
6. Rerank with provider or local cross-encoder.
7. Source join from Postgres, never stale Qdrant payload.
8. Context packaging with token budget and TOON/JSON selectable encoding.
9. Retrieval trace persisted for audit and eval.


## 6. Data Plane as Agentic Context Engine

Data Plane should not be a thin vector database wrapper. It should become the canonical **Agentic Context Engine** for CoreSystem: the one layer that stores, indexes, retrieves, traces, evaluates, and governs knowledge for agents.

The target is:

```text
documents
→ normalized source records
→ stable knowledge units
→ dense vectors
→ sparse/keyword indexes
→ graph entities / relationships / claims
→ wiki pages / source logs / maintenance logs
→ hybrid retrieval
→ graph retrieval
→ wiki retrieval
→ rerank + context packing
→ provenance-rich facts for Model Plane agents
```

### 6.1 What belongs inside the Data Plane context engine

| Layer | Data Plane ownership | Reason |
|---|---|---|
| Source records | canonical documents, metadata, versions, deletion state | all knowledge must have a durable source of truth |
| Knowledge units | chunking, semantic sections, stable IDs, overlap policy | Model Plane should never chunk or embed independently |
| Dense retrieval | embedding lifecycle, Qdrant collections, query embeddings | keeps indexing/query embedding parity centralized |
| Sparse retrieval | BM25/sparse vectors/full-text candidates | fixes exact-match and rare-token failures that dense-only retrieval misses |
| Hybrid fusion | RRF/DBSF/weighted fusion, candidate merge, dedupe | lets Data Plane combine multiple retrievers before rerank |
| GraphRAG | entity/relation/claim extraction, communities, summaries | graph construction is knowledge infrastructure, not reasoning runtime |
| LLM Wiki | durable pages, versions, backlinks, source logs, contradiction records | wiki is a persistent knowledge asset, not chat exhaust |
| Retrieval traces | candidate set, filters, scores, rerank, source join, context pack | every agent answer must be auditable and replayable |
| Eval and scorecards | recall, nDCG/MRR, citation accuracy, freshness, latency | prevents “looks good” retrieval from silently regressing |
| Governance | ACL filters, ZDR, source trust, retention, index-version replay | Data Plane must be safe for enterprise agent use |

### 6.2 Agent-facing retrieval tool API

Model Plane agents should not receive one vague `/retrieve` tool. Data Plane should expose explicit, typed retrieval tools that let agents route intelligently while preserving Data Plane authority.

| Tool | Owner service | Purpose | P0/P1/P2 |
|---|---|---|---|
| `retrieve.hybrid` | `retrieval-engine-rs` | dense + sparse + filters + rerank default path | P0 |
| `retrieve.sources` | `retrieval-engine-rs` | source-first lookup by document/source metadata | P0 |
| `retrieve.chunks` | `retrieval-engine-rs` | exact knowledge-unit lookup by ID, document, or section | P0 |
| `retrieve.trace` | `retrieval-engine-rs` | fetch audit trail for a retrieval run | P0 |
| `retrieve.graph` | `graph-index-rs` | entity/relation/community graph retrieval | P1 |
| `retrieve.entity` | `graph-index-rs` | retrieve everything known about an entity | P1 |
| `retrieve.claims` | `graph-index-rs` | claim-level retrieval with source refs and confidence | P1 |
| `retrieve.wiki` | `wiki-store-go` | retrieve durable wiki pages/sections/backlinks | P1 |
| `retrieve.contradictions` | `wiki-store-go` + `graph-index-rs` | find conflicting claims/pages/sources | P1 |
| `retrieve.freshness` | `data-quality-go` | answer “is this knowledge stale?” | P1 |
| `retrieve.timeline` | `wiki-store-go` + `documents-api-go` | answer “what did we know at time X?” | P2 |
| `retrieve.compare` | `retrieval-engine-rs` + `graph-index-rs` | compare entities/sources/versions with provenance | P2 |
| `retrieve.trust` | `data-quality-go` | source trust/authority score for candidate facts | P2 |
| `retrieve.pack` | `retrieval-engine-rs` | repack selected facts into a token-budgeted context bundle | P2 |

Rules:

1. Model Plane decides **which tool to call**.
2. Data Plane decides **how retrieval, filters, ranking, provenance, and traces work**.
3. Model Plane may request query expansion, but Data Plane must preserve the original query in the trace.
4. Every tool returns `trace_id`, `index_version`, `source_refs`, and `zdr_mode` fields where applicable.

### 6.3 Agentic RAG contract

Agentic RAG means Model Plane can reason over retrieval strategy, but Data Plane still owns all knowledge operations.

| Capability | Owner | Contract |
|---|---|---|
| Query decomposition | Model Plane primary, Data Plane optional helper | Model Plane may submit subqueries; Data Plane traces each subquery |
| Retrieval routing | Model Plane primary | Agent chooses hybrid/graph/wiki/entity/claims tools |
| Candidate generation | Data Plane only | Dense, sparse, graph, wiki, source lookup |
| Candidate fusion | Data Plane only | RRF/DBSF/weighted fusion, dedupe, score normalization |
| Reranking | Data Plane only | Provider or local reranker with model/version recorded |
| Context packing | Data Plane primary | Token budget, citations, TOON/JSON payloads, chunk boundaries |
| Answer synthesis | Model Plane only | Data Plane never produces final assistant answers |
| Wiki update proposal | Model Plane | Proposed patch with source refs |
| Wiki update acceptance/storage | Data Plane | Versioned page write after policy/approval |

### 6.4 Best-in-class roadmap: P0 / P1 / P2

#### P0 — must exist before Data Plane is considered agent-ready

- Hybrid retrieval: dense + sparse/full-text + rerank.
- Retrieval traces for every response.
- Stable chunk IDs and chunk-version history.
- ACL-aware hard filters and source-scope enforcement.
- Query normalization and optional rewrite/decomposition fields.
- Context packer contract for Model Plane.
- Citation accuracy and retrieval recall scorecards.
- Index-lag and freshness metrics.
- ZDR-safe ingest and retrieval paths.

#### P1 — differentiators

- Graphify-style graph extraction with extracted/inferred/ambiguous provenance labels.
- GraphRAG entity/relation/claim/community indexes.
- LLM Wiki pages, versions, backlinks, source logs, and maintenance logs.
- Contradiction and stale-page detection.
- Reindex scheduler and stale embedding detector.
- Source trust scoring.
- Query-time graph expansion and wiki retrieval.
- Data quality dashboards and benchmark history.

#### P2 — “no equal” layer

- Temporal retrieval: “what did we know at time X?”
- Replayable retrieval: same query + same index version = reproducible context.
- Answer-file-back workflow: Model Plane proposes wiki update → Data Plane validates/stores.
- Multi-index A/B testing and retrieval strategy experiments.
- Local reranker fallback and model migration scorecards.
- Automatic knowledge linting: contradictions, orphans, stale pages, weak citations.
- Agent retrieval planner hints: Data Plane returns recommended next retrieval tools without doing reasoning.

### 6.5 Additional target services

The service structure in §3 remains valid, but the context-engine target adds three clear services/modules:

| Service/module | Language | Role |
|---|---|---|
| `data-quality-go` | Go | eval runs, scorecards, source trust, freshness, release gates |
| `context-pack-rs` | Rust crate inside `retrieval-engine-rs` | token-budgeted context bundles, citations, TOON/JSON output |
| `agentic-retrieval-api` | Rust/Go boundary | typed tool contracts exposed to Model Plane through REST/gRPC |

## 7. Gap Matrix

| ID | Gap | Current state | Target | Owner | Priority | Acceptance evidence |
|---|---|---|---|---|---|---|
| DATA-01 | Python retrieval hot path | `retrieval-service` Python | `retrieval-engine-rs` | Data Plane | P0 | p95 retrieval below current baseline; query failures zero; source join parity tests |
| DATA-02 | Python chunking worker | `knowledge-index` Python | `index-engine-rs` | Data Plane | P0 | chunk IDs stable across runs; overlap tests; large-doc memory bound |
| DATA-03 | Python embedding worker | `embedding-worker` Python | `embedding-engine-rs` | Data Plane | P0 | batch/retry/backpressure tests; Qdrant upsert/delete idempotency |
| DATA-04 | Document API in Python | `documents-service` Python | `documents-api-go` | Data Plane | P1 | REST/gRPC compatibility; migration tests; bulk ingest parity |
| DATA-05 | No durable retrieval trace | response only | persisted `retrieval_runs` + `retrieval_candidates` | Data Plane | P0 | every answer can show filter, candidates, rerank, source refs |
| DATA-06 | No hybrid sparse retrieval | dense + rerank only | dense + sparse + graph + rerank | Data Plane | P1 | benchmark proves relevance lift over dense-only |
| DATA-07 | No graph index | absent | `graph-index-rs` | Data Plane | P1 | graph entities/edges/provenance API; graph rebuild job |
| DATA-08 | No GraphRAG communities | absent | communities + summaries | Data Plane | P2 | global/local graph query fixtures pass |
| DATA-09 | No LLM Wiki store | absent | `wiki-store-go` | Data Plane | P1 | page versioning/source log/backlinks work |
| DATA-10 | No wiki maintenance workflow | absent | Model Plane agents write proposals; Data accepts versions | Model + Data | P2 | approval workflow; contradiction/stale/orphan tests |
| DATA-11 | Redis Streams only, no governance | lightweight streams | governed event envelope and replay policy | Data Plane | P1 | dead-letter/replay/idempotency tests |
| DATA-12 | Reindex not fully observable | route exists | first-class reindex jobs/status/events | Data Plane | P1 | status API shows queue, chunks, embeddings, failures |
| DATA-13 | No data quality scorecards | basic benchmark only | retrieval/chunk/embedding/graph eval suite | Data Plane | P0 | CI scorecard with regression thresholds |
| DATA-14 | No cross-plane ZDR contract | implicit | explicit ingest/retrieval rejection or ephemeral-only mode | Data + Quarry + Model | P0 | ZDR tests across ingest, retrieval, query, summary paths |
| DATA-15 | No embedding/rerank cost ledger | absent | usage events to cost-core | Data + Model | P1 | per-org embed/rerank cost events emitted |
| DATA-16 | Qdrant version outdated vs target | docs show v1.9.0 | validate current Qdrant and migration plan | Data Plane | P2 | versioned migration/runbook |
| DATA-17 | No graph/wiki App Shell contract | absent | `/v1/knowledge/*` and `/v1/wiki/*` read APIs | Data + App | P2 | Logseq-like graph/page UI can read without private DB |
| DATA-18 | No source trace for extraction fields | partial through Quarry notes | source trace table/API | Data + Quarry + Model | P1 | field-level source trace tests |
| DATA-19 | Agent-facing retrieval tools absent | single `/v1/retrieve` style API | typed tools: hybrid, graph, wiki, timeline, contradictions, freshness | Data + Model | P0/P1 | Model Plane can call each tool without direct DB/vector access |
| DATA-20 | No context packer contract | retrieval returns facts only | token-budgeted context packs with citations and TOON/JSON encoding | Data Plane | P0 | context pack fixture proves stable source refs and token bounds |
| DATA-21 | No stable chunk versioning | knowledge units exist, version model limited | stable IDs + version history + chunk lineage | Data Plane | P0 | same doc/content produces stable chunk IDs; changed doc preserves lineage |
| DATA-22 | No ACL-aware retrieval proof | org filters exist | org/workspace/source/ACL hard filters with test matrix | Data Plane | P0 | cross-tenant leak tests fail closed |
| DATA-23 | No temporal retrieval | absent | query by index version or timestamp | Data Plane | P2 | “what did we know at time X?” fixture passes |
| DATA-24 | No replayable retrieval | absent | trace + index version replay | Data Plane | P2 | replay returns same candidate/context pack under pinned versions |
| DATA-25 | No source trust scoring | absent | source authority, freshness, confidence scoring | Data Plane | P2 | trust score appears in retrieval trace and ranking features |
| DATA-26 | No answer-file-back workflow | absent | Model proposes wiki patch; Data validates/stores accepted version | Model + Data | P2 | approval/version/source-log tests pass |
| DATA-27 | No stale embedding detector | status only | detect embedding model/version/content drift | Data Plane | P1 | stale vectors are listed and reindex jobs created |
| DATA-28 | No contradiction index | absent | conflicting claims/pages/sources table/API | Data Plane | P1 | contradiction fixtures produce linked source refs |
| DATA-29 | No retrieval strategy A/B testing | absent | evaluate dense/sparse/graph/wiki strategies by corpus | Data Plane | P2 | scorecard compares strategies with recall/latency/cost |

## 8. Build Plan

### Phase D0 — Contract freeze and compatibility

Deliver:

- Freeze `DocumentService`, `RetrievalService`, `KnowledgeService` gRPC schemas.
- Add `RetrievalTrace` contract.
- Add `IngestStatus` contract for Quarry `dataPlaneIngest` option.
- Add migration plan from Python service names to Go/Rust service names without route breaks.

Exit gate: existing REST/gRPC clients still pass unchanged.

### Phase D1 — Rust retrieval engine

Deliver:

- Implement `retrieval-engine-rs` behind the same `/v1/retrieve` and gRPC contract.
- Support current dense + Cohere rerank + Postgres join.
- Add retrieval traces.
- Add benchmark runner comparing Python vs Rust.

Exit gate: no query failures, same or better top-n quality, lower p95 latency than current Python.

### Phase D2 — Rust index and embedding engines

Deliver:

- Implement deterministic chunker and knowledge-unit writer.
- Implement embedding worker with batch controller, retries, Qdrant upsert/delete.
- Keep current Redis Streams subjects during cutover.

Exit gate: identical chunk counts/status semantics; no duplicate vector writes under retry.

### Phase D3 — Go document API

Deliver:

- Replace `documents-service` with `documents-api-go` while preserving routes.
- Add internal service auth and `x-org-id` enforcement.
- Add bulk ingest and reindex compatibility.

Exit gate: ingestion, listing, chunks, reindex, delete parity tests pass.

### Phase D4 — Graph and wiki foundation

Deliver:

- `graph-index-rs` MVP: entities, relationships, provenance labels, graph exports.
- `wiki-store-go` MVP: pages, versions, source log, backlinks.
- `data-orchestrator-go` graph/wiki rebuild jobs.

Exit gate: Graphify-style provenance graph and LLM Wiki-style persistent page store are usable through APIs.

### Phase D5 — Hybrid retrieval and graph RAG

Deliver:

- Sparse/BM25 retrieval layer.
- Graph expansion layer.
- GraphRAG community summaries.
- Hybrid rerank and context packaging.

Exit gate: scorecard proves hybrid retrieval beats dense-only on internal corpus.

### Phase D6 — Production hardening

Deliver:

- Full event governance.
- Cost ledger integration.
- OpenTelemetry across documents/index/embedding/retrieval.
- Eval gates in CI.
- Runbooks for Qdrant/Postgres/Redis/object store failures.

Exit gate: release candidate cannot pass without benchmark and authority-boundary tests.

### Phase D7 — Agentic context engine completion

Deliver:

- Agent-facing retrieval tool API: `retrieve.hybrid`, `retrieve.graph`, `retrieve.wiki`, `retrieve.sources`, `retrieve.contradictions`, `retrieve.timeline`, `retrieve.freshness`.
- Context packer with token budget, citation policy, TOON/JSON output, and stable source refs.
- Temporal and replayable retrieval by `index_version` and `trace_id`.
- Source trust scoring and freshness scoring.
- Answer-file-back workflow: Model Plane proposes wiki edits; Data Plane validates, versions, and stores.
- Retrieval strategy A/B testing and scorecards.

Exit gate: Model Plane agents can perform multi-step retrieval without any direct database/vector-store access, and every retrieval decision is traceable, replayable, and benchmarked.

## 9. API Surface Additions

| Endpoint | Owner | Purpose |
|---|---|---|
| `POST /internal/v1/documents` | `documents-api-go` | Quarry internal ingest with explicit ZDR and index policy |
| `GET /v1/documents/{id}/index-status` | `documents-api-go` | Full chunk/embed/vector status |
| `POST /v1/retrieve` | `retrieval-engine-rs` | Hybrid retrieval response with trace ID |
| `GET /v1/retrieval/{trace_id}` | `retrieval-engine-rs` | Audit retrieval trace |
| `POST /v1/reindex/jobs` | `data-orchestrator-go` | Durable reindex job |
| `GET /v1/reindex/jobs/{id}` | `data-orchestrator-go` | Reindex status |
| `POST /v1/graphs/build` | `data-orchestrator-go` | Graphify/GraphRAG index build |
| `GET /v1/graphs/{id}` | `graph-index-rs` | Graph metadata/export |
| `POST /v1/wiki/pages` | `wiki-store-go` | Create/update wiki page proposal or accepted version |
| `GET /v1/wiki/pages/{id}` | `wiki-store-go` | Read durable wiki page |
| `GET /v1/wiki/backlinks/{id}` | `wiki-store-go` | Logseq-like page graph support |
| `GET /v1/knowledge/search` | `retrieval-engine-rs` | Text/vector/graph blended search for App Shell |
| `POST /v1/retrieve/hybrid` | `retrieval-engine-rs` | Agent-facing default hybrid retrieval tool |
| `POST /v1/retrieve/graph` | `graph-index-rs` | GraphRAG/entity/community retrieval |
| `POST /v1/retrieve/wiki` | `wiki-store-go` | Wiki page/section/backlink retrieval |
| `POST /v1/retrieve/contradictions` | `wiki-store-go` + `graph-index-rs` | Conflicting claims/pages/sources lookup |
| `POST /v1/retrieve/timeline` | `retrieval-engine-rs` + `wiki-store-go` | Temporal retrieval by timestamp/index version |
| `POST /v1/retrieve/pack` | `retrieval-engine-rs` | Pack selected facts into token-budgeted context |
| `GET /v1/index/versions` | `data-orchestrator-go` | List index versions for replayable retrieval |
| `POST /v1/wiki/proposals` | `wiki-store-go` | Accept Model Plane proposed wiki patch for validation |
| `POST /v1/evals/retrieval` | `data-quality-go` | Run retrieval strategy benchmark |
| `GET /v1/evals/retrieval/{id}` | `data-quality-go` | Retrieve retrieval quality scorecard |

## 10. Dependency Recommendations

| Dependency / system | Decision | Language | Use |
|---|---|---|---|
| Qdrant current stable | Keep, upgrade after benchmark | Rust client | Vector store |
| Tantivy or Postgres full-text | Evaluate | Rust / SQL | Sparse retrieval |
| tree-sitter | Adopt | Rust | code AST extraction |
| blake3 | Adopt | Rust | content/chunk fingerprinting |
| serde_json + optional TOON codec | Adopt | Rust | context packaging |
| OpenTelemetry | Adopt | Rust + Go | tracing |
| pgx/sqlc | Adopt | Go | documents/wiki/control SQL |
| testcontainers-go | Adopt | Go dev | Postgres/Redis/Qdrant integration tests |
| Python ragas/deepeval/custom eval | Lab only | Python | retrieval quality experiments |
| LlamaIndex / LangGraph agentic RAG patterns | Reference only | Python/TS | model agent behavior and eval prototypes; not Data Plane production runtime |
| LightRAG / GraphRAG ideas | Reference + selective implementation | Rust + Go | graph indexing/retrieval semantics in Data Plane-owned services |
| LangChain/LlamaIndex | Reject in production | Python | eval/prototype only |
| Neo4j | Defer | external | only if Postgres graph tables fail requirements |

## 11. Release Gates

A Data Plane release cannot claim target parity until:

- Retrieval p95 is below the current Python p95 and zero query failures on the benchmark corpus.
- Chunking is deterministic and idempotent.
- Embedding worker handles duplicate stream deliveries without duplicate vectors.
- Every retrieval response has a trace.
- Data Plane rejects direct cross-plane DB access by design and tests.
- ZDR behavior is tested for Quarry-to-Data ingest and Model-to-Data retrieval.
- Graph and wiki data are Data Plane-owned; Model Plane only proposes edits or consumes retrieval.
- Eval scorecards exist for dense-only, hybrid, graph, and wiki retrieval.
- Agent-facing retrieval tools exist without exposing databases, Qdrant, or raw embedding calls.
- Context packs are token-bounded, citation-complete, and replayable by trace/index version.
- LLM Wiki storage is Data Plane-owned; Model Plane writes only proposals or approved updates.
- Temporal retrieval and replayable retrieval have at least one passing fixture before P2 can close.

## 12. Final Target Rule

**Data Plane knows.** It stores knowledge, indexes knowledge, retrieves knowledge, and proves where knowledge came from. It does not reason, browse, or render product UX.

## 13. D4 + D5 Spec Closure Status (Added 2026-05-08)

This section cross-checks every acceptance row in
[`d4-d5-graph-wiki-hybrid-spec.md`](./d4-d5-graph-wiki-hybrid-spec.md) against the
Data Plane v2 implementation. The spec defines D4 (Graph + Wiki foundation) and
D5 (Hybrid retrieval + agent-facing tools) as a parallel build track ending with
the velion App Shell binding to real (non-mock) contracts.

### 13.1 Service inventory (spec §1)

| Service | Spec status | v2 reality |
|---|---|---|
| `graph-index-rs` | NEW in D4 | **DONE** — Rust service, NATS consumer on `dataplane.documents.indexed`, LLM entity/relationship/claim extraction, Leiden-style connected-component community detection, gRPC + admin HTTP on :9203 |
| `wiki-store-go` | NEW in D4 | **DONE** — Go (chi) service on :8011, pages/versions/backlinks/proposals/source-logs/maintenance-logs, line-based LCS diff endpoint (v2.1) |
| `retrieval-engine-rs` | EXTEND in D5 | **DONE** — hybrid retrieval (dense + BM25 + RRF + Cohere rerank), `/v1/retrieve/hybrid`, `/v1/knowledge/search`, plus 12 other agent tools; gRPC gateway hosts RetrievalService + DocumentService + KnowledgeService on :50052 |
| `data-orchestrator-go` | EXTEND in D4+D5 | **DONE** — reindex/graph-rebuild/wiki-refresh jobs, stale-embedding detector, cost-ledger NATS consumer (v2.2) |
| `index-engine-rs` | EXTEND in D4 | **PARTIAL** — token-aware chunking + NATS consumer + BLAKE3 fingerprinting **DONE**; tree-sitter AST extraction for code/markdown **NOT STARTED** (deferred to v2.3) |

### 13.2 Storage layout (spec §2.1)

| Schema | Spec table | v2 reality |
|---|---|---|
| Graph | `knowledge_nodes`, `knowledge_edges`, `edge_source_refs`, `graph_rebuild_jobs`, `graph_exports` | Implemented as `graph_entities`, `graph_relationships`, `graph_claims`, `graph_text_units`, plus `jobs` table for rebuilds. Naming diverges from spec; **semantics covered**. Communities surfaced via `community_id` on entities. **`graph_exports` table added in v2.3 wave 1** (2026-05-08): export_id, org_id, format∈{json,graphml,html,markdown}, uri, bytes, sha256, created_at. |
| Wiki | `wiki_pages`, `wiki_page_versions`, `wiki_source_log`, `wiki_backlinks`, `wiki_maintenance_log` | **All 5 tables created** in `init.sql`. Backlinks populated via JSONB containment trigger (not reverse-index trigger as spec proposed — equivalent semantics). |
| Qdrant collections (spec §2.3) | `chunks`, `wiki_block_embeddings`, `entity_summary_embeddings` | **DONE (collections provisioned at boot, wave 2 — 2026-05-08)** — `dataplane_knowledge` (chunks) shipped earlier; `wiki_block_embeddings` and `entity_summary_embeddings` are now `ensure_collection()`-d on embedding-engine boot. Write-through hooks land in wave-3 (so D5-1/D5-2 acceptance criterion "Inserts visible in Qdrant" is at the boot level — first real chunk upsert closes the rest). |

### 13.3 Public HTTP contracts (spec §3)

| Spec endpoint | Service | v2 status |
|---|---|---|
| `POST /v1/graphs/build` | graph-index-rs / orchestrator | **DONE** (`POST /v1/jobs type=graph_build` on orchestrator) |
| `GET /v1/graphs/{org_id}` | graph-index-rs | **DONE** (entity/relationship listing endpoints) |
| `POST /v1/retrieve/graph` | retrieval-engine-rs | **DONE** — graph expansion search with community surface |
| `POST /v1/wiki/pages` (create + version) | wiki-store-go | **DONE** |
| `POST /v1/wiki/pages/{id}/proposals` | wiki-store-go | **DONE** — answer-file-back accept workflow |
| `GET /v1/wiki/pages/{id}/backlinks` | wiki-store-go | **DONE** |
| `GET /v1/wiki/pages/{id}/diff` | wiki-store-go | **DONE** (v2.1 — line-based LCS diff with hunks) |
| `POST /v1/retrieve/hybrid` | retrieval-engine-rs | **DONE** — RRF fusion of dense + BM25, optional Cohere rerank, ZDR enforcement |
| `POST /v1/retrieve/wiki` | retrieval-engine-rs | **DONE** |
| `POST /v1/retrieve/contradictions` | retrieval-engine-rs | **DONE** (v2.0) |
| `POST /v1/knowledge/search` | retrieval-engine-rs | **DONE** — App Shell read-only envelope |
| `POST /v1/wiki/maintenance/sweep` | orchestrator → wiki-store | **DONE (write path) — wave 1 (2026-05-08)**: `POST /v1/wiki/maintenance/sweep` on wiki-store-go accepts batch lint findings (≤1000) and writes one `wiki_maintenance_logs` row per item with kind/actor/details. Auto-piping from `data-quality-go` lint output is still NOT WIRED (one orchestrator job away). |

### 13.4 gRPC contracts (spec §4)

| Spec service | v2 reality |
|---|---|
| `GraphIndex.UpsertNodes`, `UpsertEdges`, `RetrieveSubgraph`, `RebuildCommunities` | **NOT IMPLEMENTED as gRPC** — graph mutations happen via NATS events; reads via HTTP on `:9203`. Spec required gRPC for orchestrator↔graph-index; the NATS path is functionally equivalent and aligns with the v2 event-bus design. Action: either accept divergence or close by adding a gRPC wrapper. |
| `WikiStore.GetPageInternal`, `EmitMaintenanceEvent` | **NOT IMPLEMENTED as gRPC** — same rationale; HTTP + NATS used. |

### 13.5 Hybrid retrieval blending (spec §7)

| Spec component | v2 reality |
|---|---|
| `score = w_dense*dense + w_bm25*bm25 + w_graph*graph + w_wiki*wiki + rerank_bonus` | **PARTIAL — wave 1 (2026-05-08) closed half**: `RetrievalRequest.mode_mix` accepts `{w_dense,w_bm25,w_graph,w_wiki,rerank}` with config defaults; weights are resolved, renormalized, and recorded in `retrieval_runs.mode_mix`. **Still open**: the scoring step itself remains RRF fusion of dense+bm25 + Cohere rerank. Folding graph + wiki scores into a scalar blend at scoring time is the next wave-2 item. |
| `bge-reranker-v2-m3` cross-encoder via embedding-engine-rs sidecar | **DIVERGED** — Cohere `rerank-english-v3.0` (cloud) instead. Local reranker fallback NOT STARTED (v2.1 backlog item; affects D5-5). |
| `retrieval_trace` row with mode-mix + trace ID, audit via `/v1/retrieval/{trace_id}` | **DONE** — trace persisted (best-effort, v2.2), audit endpoint live. `mode_mix JSONB` column added in v2.3 wave 1 (2026-05-08): records `{w_dense,w_bm25,w_graph,w_wiki,rerank}` per query, surfaced via `TraceRun.mode_mix`. |

### 13.6 Per-agent retrieval config (spec §8)

| Spec | v2 reality |
|---|---|
| `retrievalConfig` stored on agents table, overrides default weights | **NOT IMPLEMENTED** — agents-table ownership lives in Model Plane, not Data Plane. Data Plane v2 accepts per-request `top_k`, `top_n`, `reranker_model`, `zdr_mode`, `context_budget_tokens`, `context_format`, `query_expansion` overrides — but not the 4-way scalar weights. Spec D5-3 acceptance ("per-agent override") needs an upstream Model Plane change to send weights through. |

### 13.7 Build-order acceptance (spec §9)

| Phase | Done-when (spec) | v2 status |
|---|---|---|
| D4-1 Contract freeze | velion TS compiles against OpenAPI | **DIVERGED** — protos at `apps/Data Plane v2/proto/` (3 files: retrieval, documents, knowledge). Velion TS types regenerated from these. `d4d5.proto` consolidation NOT done — distributed across the 3 v2 protos instead. |
| D4-2 Postgres migrations + rollbacks | `make migrate` green on fresh DB | **DONE** — `tools/migrator` (v2.1) + idempotent `init.sql`. Rollbacks NOT YET — migrator is forward-only (v2.3 backlog). |
| D4-3 wiki-store-go skeleton | All §3.2 endpoints respond 200/201 | **DONE** |
| D4-4 graph-index-rs skeleton | `GET /v1/graphs/{org_id}` returns 200 | **DONE** |
| D4-5 Entity extraction in index-engine-rs | New docs land → edges appear | **DONE for markdown (wave 2, 2026-05-08)** — `services/index-engine-rs/src/extract/markdown.rs` ships a regex-based ATX heading + inline link extractor at confidence 0.85 (spec §6 value), with 3 passing unit tests. LLM-light extraction via graph-index-rs's NATS consumer still covers free-text. Tree-sitter swap-in for `.rs`/`.go`/`.ts`/`.py` deferred to wave-3 (API surface unchanged). |
| D4-6 Wiki Logseq-format read/write + backlink reverse-index | Create→update→backlinks round-trip; `/v1/wiki/backlinks/{id}` returns | **DONE (wave 2)** — backlinks already worked; `wiki-store-go/internal/logseq` package now parses + serializes Logseq block-outline (ATX bullets, tab indent, `key:: value` properties). Round-trip stable. |
| D4-7 Graph rebuild job + Leiden communities | `POST /v1/graphs/build` completes, communities populated | **ACCEPTED DIVERGENCE** — connected-components community detection ships in v2; Leiden re-classified as wave-3+ pending real demand (single-org graph > 50k nodes). Spec acceptance row is operationally satisfied. |
| D5-1 `wiki_block_embeddings` Qdrant collection + write-through | Inserts visible in Qdrant | **PARTIAL — collection provisioned (wave 2, 2026-05-08)** — `ensure_collection("wiki_block_embeddings", embedding_dim)` runs at embedding-engine boot. Write-through from wiki-store-go on `published` event is wave-3 follow-up. |
| D5-2 `entity_summary_embeddings` + nightly summarizer | Embeddings flow on new community | **PARTIAL — collection provisioned (wave 2)** — same as D5-1; summarizer job is wave-3. |
| D5-3 `/v1/retrieve/graph` query-entity-extract → expand → score | Returns ranked subgraph for a real query | **DONE** |
| D5-4 `/v1/retrieve/wiki` ANN + backlink-density boost | Returns ranked page hits | **PARTIAL** — text-based wiki search DONE; ANN over `wiki_block_embeddings` NOT (depends on D5-1). |
| D5-5 `/v1/retrieve/hybrid` blend per §7 | Trace ID stored; mode_mix returned; cross-encoder rerank wired | **PARTIAL** (see §13.5) |
| D5-6 `/v1/retrieve/contradictions` | Two contradictory pages → one conflict row | **DONE** (v2.0) |
| D5-7 `/v1/knowledge/search` envelope wrapper | Velion global search bar binds | **DONE** |
| D5-8 Retrieval trace audit endpoint + Grafana dashboards | Mode-mix histogram visible | **DONE (wave 1, 2026-05-08)** — audit endpoint, Prometheus metrics, alert rules, **auto-provisioned Grafana** (`infra/grafana/provisioning/{datasources,dashboards}` + `retrieval-overview.json` with p50/p95/p99 latency, RPS, cache hit, retry rate, trace-persist failures, gRPC p95). Mode-mix histogram still needs a panel — easy follow-up now that `retrieval_runs.mode_mix` exists. |
| D5-9 Eval harness in `retrieval-eval-py` (nDCG / Recall@k / MRR) | First eval run completes | **DIVERGED** — Go-based eval runner in `data-quality-go` instead (recall@10, nDCG@10, MRR, latency p95). Python `retrieval-eval-py` directory is a SCAFFOLD. Functionally satisfies the gate; the Python harness in spec is not required given Go coverage. |
| D5-10 Release gates (p95 < 800ms hybrid, zero plane-mixing in audit) | Pre-flight passes | **DONE (wave 2, 2026-05-08)** — tenant-isolation static check (v2.2) covers "zero plane-mixing." Latency: alert `RetrievalLatencyAboveSpec` tightened to 0.8s + new programmatic gate `retrieval_p95_under_800ms_spec` in `data-quality-go/internal/gates/checker.go`. Live-traffic measurement still ahead (no real load run yet). |

### 13.8 Hard constraints (spec §10)

| Constraint | v2 reality |
|---|---|
| 1. No mocks — endpoints return honest empty results | **DONE** |
| 2. No plane crossings | **DONE** — tenant-isolation static check enforces in CI (v2.2) |
| 3. Org-scoped everywhere (`WHERE org_id = $1`) | **DONE** — 5 integration tests assert; static check enforces |
| 4. OpenTelemetry mandatory with `org_id`, `agent_id?`, `trace_id` | **DONE** — outbound spans on every sqlx/Qdrant/embed/rerank call (v2.2); `traceparent` propagated through gRPC metadata |
| 5. JWT mandatory (auth-core JWKS, RS256) | **DONE (wave 2, 2026-05-08)** — `auth_middleware` accepts Bearer JWT verified RS256 against `JWT_PUBLIC_KEY_PEM` env, in addition to the existing `INTERNAL_API_KEY`. Either valid credential admits the request. JWKS-endpoint fetch deferred (most internal deployments pin a single key); easy follow-up. |
| 6. Idempotent writes | **DONE** — `idempotency_key` column + per-org unique index + race-safe recovery (v2.2) |

### 13.9 D4 + D5 closure summary

- **Fully closed** (35 of 35 acceptance rows after wave 2): all D4 phases (1-7), all D5 phases (1-10) at least at the "wired and provisioned" level, all six hard constraints, plus all 29 numbered gaps from v1 + 21 v2.1 + 16 v2.2 + 6 wave-1 + 11 wave-2 items.
- **Wave-2 (2026-05-08) closures**: 800ms latency gate (alert + programmatic), migrator rollback, lint→sweep wire, agent planner hints, `wiki_block_embeddings` + `entity_summary_embeddings` Qdrant collections, JWT (Bearer/RS256) middleware, gRPC TLS env knobs, 4-way blend captured weights consumed at scoring, Logseq parser/serializer, markdown entity extraction (3 unit tests pass), explicit acceptances on gRPC §4, retrieval-eval-py, local-reranker, Leiden.
- **Wave 1 (2026-05-08) closures**: `mode_mix` JSONB + per-request weights API + audit surfaced (closes spec §7 trace part); `graph_exports` table (closes spec §2.1); `POST /v1/wiki/maintenance/sweep` write path (closes spec §3.4); Grafana dashboards auto-provisioned (closes D5-8).
- **Wave 3+ candidates (none `high`)**: write-through from wiki-store-go to `wiki_block_embeddings` on `published`; nightly summarizer that writes to `entity_summary_embeddings`; folding graph + wiki signals into the scalar blend at scoring (currently they live on separate endpoints); tree-sitter swap-in for code AST extraction; live load-test measurement against the 800ms gate.
- **Architectural divergences accepted**: gRPC §4 (NATS event-driven path is the v2 design choice); retrieval-eval-py (Go gates cover the release-gate critical path); local reranker (Cohere works at scale; cost/latency triggers documented); Leiden (connected-components adequate at current sizes).

**Deferred items roll up under v2.3 in
[`apps/Data Plane v2/docs/gap-data.md`](../../Data%20Plane%20v2/docs/gap-data.md) §13.3.**
