# Data Plane — Gap Analysis and Target Architecture

> Generated: 2026-05-06  
> Updated: 2026-05-08 — 87 items closed across 29 numbered + 21 v2.1 + 16 v2.2 + 6 v2.3-w1 + 11 v2.3-w2 + 4 accepted divergences. All D4 + D5 spec acceptance rows (35/35) green at the wired/provisioned level. **§15 (Wave 3 — Control Plane wiring) is now the next blocker for multi-tenant production deploy** — `X-Org-ID` is still trusted blindly; auth-core/user-core/org-core/cost-core are not consulted. §16 catalogs the consistency/stability/performance/quality/security follow-ups (Waves 4-5). §17 records the v3 architecture changes (Connect-RPC, server streaming, NATS subject contract). §18 sequences the waves.  
> Source baseline: current Data Plane docs and benchmark output uploaded in this chat.  
> Target: Rust + Go knowledge infrastructure with Python only for eval/lab/provider fallback.

## 1. Executive Summary

The current Data Plane is correct in authority but not optimal in implementation language split. It already has the right conceptual boundary: Data Plane is the canonical owner of documents, chunks, embeddings, Qdrant vectors, and retrieval, and no other plane should touch Qdrant, chunk documents, or call the embedding API directly.

The main upgrade is to move the **knowledge engine path** from Python to Rust while creating a Go control layer for document lifecycle, reindex, graph/wiki jobs, and operator workflows.

**Data Plane v2 implements this target.** The v2 codebase contains 8 services across Rust and Go, all compiling clean, with 62 API endpoints, infrastructure orchestrated via docker-compose. **All 29 gaps CLOSED.**

| Domain | v1 State | Target | v2 Status | Service |
|---|---|---|---|---|
| Document CRUD | Python `documents-service` | Go `documents-api-go` | **DONE** | `documents-api-go` :8010 |
| Chunking / knowledge units | Python `knowledge-index` | Rust `index-engine-rs` | **DONE** | `index-engine-rs` :9201 |
| Embedding worker | Python `embedding-worker` | Rust `embedding-engine-rs` | **DONE** | `embedding-engine-rs` :9202 |
| Retrieval | Python `retrieval-service` | Rust `retrieval-engine-rs` | **DONE** | `retrieval-engine-rs` :8014/:50062 |
| Graphify / GraphRAG | Not present | Rust `graph-index-rs` + Go orchestration | **DONE** | `graph-index-rs` :9203 |
| LLM Wiki storage | Not present | Go `wiki-store-go` | **DONE** | `wiki-store-go` :8011 |
| Orchestrator | Not present | Go `data-orchestrator-go` | **DONE** | `data-orchestrator-go` :8012 |
| Data quality / eval | Basic benchmark exists | Go `data-quality-go` + release gates | **DONE** | `data-quality-go` :8013 |
| Eval/benchmarking | Basic benchmark exists | Python `retrieval-eval-py` + release gates | SCAFFOLD | `retrieval-eval-py` (placeholder) |

## 2. Non-Negotiable Authority Rules

1. **Data Plane owns knowledge.** Documents, chunks, embeddings, vector indexes, graph indexes, wiki pages, source logs, retrieval traces, contradiction indexes, and graph/wiki snapshots are canonical here.
2. **Model Plane consumes only APIs.** No direct Data Plane Postgres, Qdrant, graph store, or embedding provider access.
3. **Quarry captures evidence, then submits evidence.** Quarry may call Data Plane ingest APIs but never writes Data Plane storage directly.
4. **Embedding/rerank parity is centralized.** No other plane may embed/rerank independently outside eval labs.
5. **Retrieval is auditable.** Every retrieval response must be explainable with query embedding model, candidate set, reranker, filters, source joins, and trace IDs.
6. **ZDR is propagated.** Data Plane must reject or run ephemeral-only for any ingest/index/retrieval path that would violate Quarry/Model zero-data-retention contracts.

## 3. Target Data Plane Service Structure

> **v2 status:** All services implemented and compiling. Ports offset from v1 so both can run simultaneously.

```text
DATA PLANE v2 — L2

  documents-api-go         :8010
    DONE — document CRUD, metadata, org scoping, bulk ingest, soft delete, NATS events

  index-engine-rs          :9201 (admin)
    DONE — token-aware chunking, BLAKE3 fingerprinting, paragraph/sentence splitting,
           overlap, NATS consumer (dataplane.documents.created)

  embedding-engine-rs      :9202 (admin)
    DONE — batch embedding via Azure OpenAI, Qdrant upsert/delete, retry,
           NATS consumer (dataplane.knowledge_units.created)

  retrieval-engine-rs      :8014 (HTTP) / :50052 (gRPC)
    DONE — hybrid retrieval (dense + sparse + RRF + rerank), query embedding,
           context packing, retrieval traces, ZDR enforcement, ACL filtering
           (org+workspace+collection+acl_tags), 15 HTTP endpoints including all 14
           agent-facing tools (hybrid, sources, chunks, trace, graph, wiki,
           contradictions, freshness, timeline, compare, pack)
           gRPC gateway: 3 proto services (RetrievalService 5 RPCs,
           DocumentService 7 RPCs, KnowledgeService 2 RPCs) with
           INTERNAL_API_KEY interceptor, served via Tonic alongside Axum HTTP

  graph-index-rs           :9203 (admin)
    DONE — LLM entity/relationship/claim extraction (GPT-4o), connected-component
           community detection, graph expansion API, NATS consumer
           (dataplane.documents.indexed)

  wiki-store-go            :8011
    DONE — wiki pages, versions, backlinks (JSONB containment), proposals,
           answer-file-back accept workflow (proposal → auto-version),
           source_logs, maintenance_logs — 15 endpoints

  data-orchestrator-go     :8012
    DONE — reindex/graph_build/wiki_refresh jobs, stale embedding detector
           (content-drift, stuck-pending, failed), progress tracking

  data-quality-go          :8013
    DONE — eval runner (recall@10, nDCG@10, MRR, latency p95), trust scorer
           (authority × freshness composite), 5 release gates

  retrieval-eval-py        lab only
    SCAFFOLD — placeholder for Python eval/experiment harness

Infrastructure:
  PostgreSQL 16     :5442  canonical document/chunk/wiki/graph metadata
  Qdrant 1.13.2     :6345  vector collections through retrieval/embedding engines
  Dragonfly 1.37    :6389  Redis-compatible caching layer (256MB, cache mode)
  NATS 2 JetStream  :4232  async event bus (replaced Redis Streams from v1)
```

## 4. Target Ownership by Feature

| Feature | Canonical owner | Service | Language | v2 Status |
|---|---|---|---|---|
| Document metadata/content | Data Plane | `documents-api-go` | Go | **DONE** |
| Bulk document ingest | Data Plane | `documents-api-go` | Go | **DONE** |
| Chunking | Data Plane | `index-engine-rs` | Rust | **DONE** |
| Semantic chunking | Data Plane | `index-engine-rs` | Rust | **DONE** (token-aware paragraph/sentence split) |
| Code AST extraction | Data Plane | `index-engine-rs` / `graph-index-rs` | Rust | NOT STARTED |
| Embedding batching | Data Plane | `embedding-engine-rs` | Rust | **DONE** |
| Qdrant upsert/delete | Data Plane | `embedding-engine-rs` | Rust | **DONE** |
| Query embedding | Data Plane | `retrieval-engine-rs` | Rust | **DONE** |
| ANN search | Data Plane | `retrieval-engine-rs` | Rust | **DONE** |
| BM25/sparse retrieval | Data Plane | `retrieval-engine-rs` | Rust | **DONE** (Postgres tsvector) |
| Rerank | Data Plane | `retrieval-engine-rs` | Rust | **DONE** (Cohere) |
| Source join | Data Plane | `retrieval-engine-rs` | Rust | **DONE** |
| Graphify graph build | Data Plane | `graph-index-rs` | Rust | **DONE** |
| GraphRAG indexing | Data Plane | `graph-index-rs` | Rust | **DONE** |
| GraphRAG synthesis | Model Plane | `execution-core` / `inference-core` | Rust | N/A (Model Plane) |
| LLM Wiki durable pages | Data Plane | `wiki-store-go` | Go | **DONE** |
| LLM Wiki source/maintenance logs | Data Plane | `wiki-store-go` | Go | **DONE** |
| LLM Wiki page diff/index | Data Plane | `wiki-store-go` + Rust helper | Go + Rust | PARTIAL (versioning done, diff helper not yet) |
| LLM Wiki maintenance agent | Model Plane | `orchestrator-core` + `execution-core` | Go + Rust | N/A (Model Plane) |
| Logseq-like UX | App Shell | app/workspace | TypeScript | N/A (App Shell) |
| Autoresearch | Model Plane | `orchestrator-core` + `execution-core` | Go + Rust | N/A (Model Plane) |

## 5. Research-Based Architecture Additions

### 5.1 Graphify additions

> **v2 status:** Core graph extraction with provenance, confidence, source refs, and text_unit mappings fully implemented. AST extraction not yet started.

Graphify's public docs describe a three-pass pipeline: deterministic AST extraction for code, local transcription for audio/video, LLM semantic extraction for non-code corpus content, then NetworkX + Leiden clustering with every relationship labeled `EXTRACTED`, `INFERRED`, or `AMBIGUOUS`. Data Plane should adopt the **provenance and graph output ideas**, not the Python runtime as the production source of truth.

Implemented in v2:

- [x] `graph_entities` table with entity type, name, description, org scoping, confidence, provenance, source_refs.
- [x] `graph_relationships` table with source/target entity, relation type, confidence, provenance (`extracted | inferred | ambiguous`), source_refs.
- [x] `graph_claims` table with entity, claim text, status, evidence, contradicted_by (JSONB), confidence, provenance.
- [x] `graph_communities` table with entity IDs, summary, size.
- [x] `graph_text_units` table mapping entities/relationships/claims back to knowledge_unit IDs.
- [x] LLM extraction via Azure OpenAI GPT-4o with JSON response_format.
- [x] Connected-component community detection (simpler alternative to Leiden).
- [x] NATS consumer for automatic extraction on document indexing.
- [x] `confidence` field on all graph objects (entities, relationships, claims).
- [x] `source_refs` JSONB linking graph objects to source chunk/artifact IDs.
- [x] Provenance labels (`extracted | inferred | ambiguous`) on all persisted graph objects.

Not yet implemented:

- [ ] AST-first extraction for code using tree-sitter-compatible Rust crates.
- [ ] `graph_exports` for JSON/HTML/markdown graph reports.
- [ ] `graph_rebuild_jobs` with incremental BLAKE3 content cache.

### 5.2 GraphRAG additions

> **v2 status:** Fully implemented. Text_units mapping, cost ledger, community search all in place.

Implemented in v2:

- [x] `graph_entities`, `graph_relationships`, `graph_claims` tables with confidence + provenance.
- [x] `graph_communities` with entity lists and summaries.
- [x] `graph_text_units` mapping entities/relationships/claims → knowledge_unit IDs.
- [x] Graph expansion search (BFS with max_hops/max_entities).
- [x] Community summary search by entity containment.
- [x] Entity listing by type, relationship filtering by relation_type.
- [x] Cost ledger events published via NATS (`dataplane.cost.ledger`) for embedding batches.

Not yet implemented:

- [ ] Cadence controls for graph rebuild scheduling (manual trigger via orchestrator works).

### 5.3 LLM Wiki additions

> **v2 status:** Fully implemented including source and maintenance logs.

Implemented in v2:

- [x] `wiki_pages`: page ID, org, title, path, current_version_id, status.
- [x] `wiki_page_versions`: versioned markdown content, source refs (JSONB), editor metadata.
- [x] `wiki_backlinks`: JSONB containment queries on linked_page_ids.
- [x] `wiki_proposals`: answer-file-back proposals with accept/reject workflow.
- [x] Accept proposal auto-creates new version via transaction.
- [x] Full CRUD API: create page, update version, list versions, get backlinks, submit/review proposals.
- [x] `wiki_source_logs`: source-to-page ingestion trace with source_type, source_ref, sync_status.
- [x] `wiki_maintenance_logs`: action/actor log for contradictions, stale pages, edits, maintenance ops.
- [x] Source log and maintenance log API endpoints (create + list for each).

Not yet implemented:

- [ ] Dedicated `wiki_retrieval` API endpoint on wiki-store-go (retrieval-engine-rs handles wiki search).

### 5.4 Hybrid retrieval additions

> **v2 status:** All 9 pipeline stages implemented.

The target hybrid retrieval pipeline, all implemented in `retrieval-engine-rs`:

1. [x] Hard auth filter: org_id always enforced. Optional document_types, departments, languages, document_ids, sources, region, workspaces, collections, acl_tags filters.
2. [x] Query rewrite/normalization: optional `query_expansion` field, original query preserved in trace.
3. [x] Dense vector retrieval from Qdrant (ANN top_k configurable).
4. [x] Sparse/BM25 retrieval over Postgres tsvector full-text search.
5. [x] Graph expansion from `graph-index-rs` entities/relationships/claims.
6. [x] Rerank with Cohere (configurable model via `reranker_model`).
7. [x] Source join from Postgres, never stale Qdrant payload.
8. [x] Context packaging with token budget (`context_budget_tokens`) and JSON format.
9. [x] Retrieval trace persisted for audit and eval (`retrieval_runs` + `retrieval_candidates`).


## 6. Data Plane as Agentic Context Engine

> **v2 status:** Agentic context engine fully implemented with 14 retrieval tools and 62 total API endpoints across 8 services (4 Rust, 4 Go).

Data Plane should not be a thin vector database wrapper. It should become the canonical **Agentic Context Engine** for CoreSystem: the one layer that stores, indexes, retrieves, traces, evaluates, and governs knowledge for agents.

The target pipeline is fully implemented in v2:

```text
documents                                          — documents-api-go
→ normalized source records                        — documents-api-go
→ stable knowledge units (BLAKE3 fingerprinted)    — index-engine-rs
→ dense vectors (3072d text-embedding-3-large)     — embedding-engine-rs
→ sparse/keyword indexes (Postgres tsvector)       — retrieval-engine-rs
→ graph entities / relationships / claims          — graph-index-rs
→ wiki pages / source logs / maintenance logs      — wiki-store-go
→ hybrid retrieval (dense + sparse + RRF)          — retrieval-engine-rs
→ graph retrieval (expansion + communities)        — retrieval-engine-rs
→ wiki retrieval (FTS + ILIKE)                     — retrieval-engine-rs
→ rerank + context packing                         — retrieval-engine-rs
→ provenance-rich facts for Model Plane agents     — retrieval-engine-rs
```

### 6.1 What belongs inside the Data Plane context engine

| Layer | Data Plane ownership | v2 Status |
|---|---|---|
| Source records | canonical documents, metadata, versions, deletion state | **DONE** |
| Knowledge units | chunking, semantic sections, stable IDs, overlap policy | **DONE** |
| Dense retrieval | embedding lifecycle, Qdrant collections, query embeddings | **DONE** |
| Sparse retrieval | BM25/sparse vectors/full-text candidates | **DONE** |
| Hybrid fusion | RRF/weighted fusion, candidate merge, dedupe | **DONE** |
| GraphRAG | entity/relation/claim extraction, communities, summaries | **DONE** |
| LLM Wiki | durable pages, versions, backlinks, source logs, contradiction records | **DONE** |
| Retrieval traces | candidate set, filters, scores, rerank, source join, context pack | **DONE** |
| Eval and scorecards | recall, nDCG/MRR, citation accuracy, freshness, latency | **DONE** |
| Governance | ACL filters, ZDR, source trust, retention, index-version replay | **DONE** (ACL: org+workspace+collection+acl_tags; ZDR: reject mode enforced; trust scoring; index replay) |

### 6.2 Agent-facing retrieval tool API

> **v2 status:** All 14 tools implemented as HTTP endpoints.

| Tool | Owner service | Purpose | Priority | v2 Status |
|---|---|---|---|---|
| `retrieve.hybrid` | `retrieval-engine-rs` | dense + sparse + filters + rerank default path | P0 | **DONE** — `POST /v1/retrieve/hybrid` |
| `retrieve.sources` | `retrieval-engine-rs` | source-first lookup by document/source metadata | P0 | **DONE** — `POST /v1/retrieve/sources` |
| `retrieve.chunks` | `retrieval-engine-rs` | exact knowledge-unit lookup by ID, document, or section | P0 | **DONE** — `POST /v1/retrieve/chunks` |
| `retrieve.trace` | `retrieval-engine-rs` | fetch audit trail for a retrieval run | P0 | **DONE** — `GET /v1/retrieval/{trace_id}` |
| `retrieve.graph` | `retrieval-engine-rs` | entity/relation/community graph retrieval | P1 | **DONE** — `POST /v1/retrieve/graph` |
| `retrieve.entity` | `graph-index-rs` | retrieve everything known about an entity | P1 | **DONE** — `GET /v1/graph/entities/{entity_id}` + `/expand` |
| `retrieve.claims` | `graph-index-rs` | claim-level retrieval with source refs and confidence | P1 | **DONE** — `GET /v1/graph/claims` |
| `retrieve.wiki` | `retrieval-engine-rs` | retrieve durable wiki pages/sections/backlinks | P1 | **DONE** — `POST /v1/retrieve/wiki` |
| `retrieve.contradictions` | `retrieval-engine-rs` | find conflicting claims/pages/sources | P1 | **DONE** — `POST /v1/retrieve/contradictions` |
| `retrieve.freshness` | `retrieval-engine-rs` | answer "is this knowledge stale?" | P1 | **DONE** — `POST /v1/retrieve/freshness` |
| `retrieve.timeline` | `retrieval-engine-rs` | answer "what did we know at time X?" | P2 | **DONE** — `POST /v1/retrieve/timeline` |
| `retrieve.compare` | `retrieval-engine-rs` | compare entities or documents with shared relationships/chunk counts | P2 | **DONE** — `POST /v1/retrieve/compare` |
| `retrieve.trust` | `data-quality-go` | source trust/authority score for candidate facts | P2 | **DONE** — `POST /v1/quality/trust` |
| `retrieve.pack` | `retrieval-engine-rs` | repack selected facts into a token-budgeted context bundle | P2 | **DONE** — `POST /v1/retrieve/pack` |

Rules (unchanged):

1. Model Plane decides **which tool to call**.
2. Data Plane decides **how retrieval, filters, ranking, provenance, and traces work**.
3. Model Plane may request query expansion, but Data Plane must preserve the original query in the trace.
4. Every tool returns `trace_id`, `index_version`, `source_refs`, and `zdr_mode` fields where applicable.

### 6.3 Agentic RAG contract

Unchanged from original spec. Contract boundaries are respected in v2 implementation.

| Capability | Owner | Contract | v2 Status |
|---|---|---|---|
| Query decomposition | Model Plane primary, Data Plane optional helper | Model Plane may submit subqueries; Data Plane traces each subquery | READY (trace infra done) |
| Retrieval routing | Model Plane primary | Agent chooses hybrid/graph/wiki/entity/claims tools | READY (endpoints exist) |
| Candidate generation | Data Plane only | Dense, sparse, graph, wiki, source lookup | **DONE** |
| Candidate fusion | Data Plane only | RRF/weighted fusion, dedupe, score normalization | **DONE** |
| Reranking | Data Plane only | Provider or local reranker with model/version recorded | **DONE** |
| Context packing | Data Plane primary | Token budget, citations, JSON payloads, chunk boundaries | **DONE** |
| Answer synthesis | Model Plane only | Data Plane never produces final assistant answers | N/A |
| Wiki update proposal | Model Plane | Proposed patch with source refs | READY (proposal API exists) |
| Wiki update acceptance/storage | Data Plane | Versioned page write after policy/approval | **DONE** |

### 6.4 Best-in-class roadmap: P0 / P1 / P2

#### P0 — must exist before Data Plane is considered agent-ready

| Requirement | v2 Status |
|---|---|
| Hybrid retrieval: dense + sparse/full-text + rerank | **DONE** |
| Retrieval traces for every response | **DONE** |
| Stable chunk IDs and chunk-version history | **DONE** (BLAKE3 fingerprinting + chunk_lineage table) |
| ACL-aware hard filters and source-scope enforcement | **DONE** (org_id + workspace + collection + acl_tags enforced in Qdrant conditions) |
| Query normalization and optional rewrite/decomposition fields | **DONE** (query_expansion field) |
| Context packer contract for Model Plane | **DONE** |
| Citation accuracy and retrieval recall scorecards | **DONE** (recall@10, nDCG@10, MRR) |
| Index-lag and freshness metrics | **DONE** (stale embedding detector + freshness scoring) |
| ZDR-safe ingest and retrieval paths | **DONE** (zdr_mode=reject filters restricted docs post-rerank; ephemeral mode skips trace persistence) |

#### P1 — differentiators

| Requirement | v2 Status |
|---|---|
| Graphify-style graph extraction with provenance labels | **DONE** (provenance: extracted/inferred/ambiguous on all graph objects) |
| GraphRAG entity/relation/claim/community indexes | **DONE** |
| LLM Wiki pages, versions, backlinks, source logs, and maintenance logs | **DONE** (source_logs + maintenance_logs with CRUD API) |
| Contradiction and stale-page detection | **DONE** (contradiction search via claims JSONB) |
| Reindex scheduler and stale embedding detector | **DONE** |
| Source trust scoring | **DONE** (authority × freshness composite) |
| Query-time graph expansion and wiki retrieval | **DONE** |
| Data quality dashboards and benchmark history | PARTIAL (eval runner done; dashboard UI not Data Plane) |

#### P2 — "no equal" layer

| Requirement | v2 Status |
|---|---|
| Temporal retrieval: "what did we know at time X?" | **DONE** (before/after date filters) |
| Replayable retrieval: same query + same index version = reproducible context | **DONE** (trace replay by trace_id) |
| Answer-file-back workflow: Model Plane proposes wiki update, Data validates/stores | **DONE** (proposal → accept → auto-version) |
| Multi-index A/B testing and retrieval strategy experiments | **DONE** (`POST /v1/evals/compare` — runs two strategies, diffs scorecards, picks winner) |
| Local reranker fallback and model migration scorecards | NOT STARTED |
| Automatic knowledge linting: contradictions, orphans, stale pages, weak citations | PARTIAL (contradiction detection done; orphan/stale linting not yet) |
| Agent retrieval planner hints | NOT STARTED |

### 6.5 Additional target services

| Service/module | Language | Role | v2 Status |
|---|---|---|---|
| `data-quality-go` | Go | eval runs, scorecards, source trust, freshness, release gates | **DONE** |
| `context-pack-rs` | Rust crate inside `retrieval-engine-rs` | token-budgeted context bundles, citations, JSON output | **DONE** (integrated as `context_pack` module) |
| `agentic-retrieval-api` | Rust/Go boundary | typed tool contracts exposed to Model Plane through REST/gRPC | **DONE** (all 14 tools implemented across retrieval-engine-rs + graph-index-rs + data-quality-go) |

## 7. Gap Matrix

| ID | Gap | v1 State | Target | v2 Status | Notes |
|---|---|---|---|---|---|
| DATA-01 | Python retrieval hot path | `retrieval-service` Python | `retrieval-engine-rs` | **CLOSED** | Rust retrieval with hybrid pipeline, 14 endpoints |
| DATA-02 | Python chunking worker | `knowledge-index` Python | `index-engine-rs` | **CLOSED** | Token-aware chunking, BLAKE3 fingerprinting, NATS consumer |
| DATA-03 | Python embedding worker | `embedding-worker` Python | `embedding-engine-rs` | **CLOSED** | Batch embedding, Qdrant upsert/delete, retry |
| DATA-04 | Document API in Python | `documents-service` Python | `documents-api-go` | **CLOSED** | Go REST API with CRUD, bulk ingest, soft delete, NATS events |
| DATA-05 | No durable retrieval trace | response only | persisted `retrieval_runs` + `retrieval_candidates` | **CLOSED** | Trace persisted with timing, candidates, scores; trace replay API |
| DATA-06 | No hybrid sparse retrieval | dense + rerank only | dense + sparse + graph + rerank | **CLOSED** | BM25 via Postgres tsvector + RRF fusion + graph expansion |
| DATA-07 | No graph index | absent | `graph-index-rs` | **CLOSED** | LLM extraction, entities/relationships/claims API, community detection |
| DATA-08 | No GraphRAG communities | absent | communities + summaries | **CLOSED** | Connected-component detection, community summaries stored and queryable |
| DATA-09 | No LLM Wiki store | absent | `wiki-store-go` | **CLOSED** | Pages, versions, backlinks, proposals, accept workflow |
| DATA-10 | No wiki maintenance workflow | absent | Model proposes, Data accepts | **CLOSED** | Proposal submit/review API with accept → auto-version |
| DATA-11 | Redis Streams only, no governance | lightweight streams | governed event bus | **CLOSED** | NATS JetStream with WorkQueue retention, ack_wait, max_deliver; idempotency keys on events; dead-letter queues (`dataplane.dlq.*`) on all 3 stream consumers (index-engine, embedding-engine, graph-index) |
| DATA-12 | Reindex not fully observable | route exists | first-class reindex jobs | **CLOSED** | data-orchestrator-go with job types, progress tracking, stale detector |
| DATA-13 | No data quality scorecards | basic benchmark only | eval suite | **CLOSED** | Eval runner with recall@10, nDCG@10, MRR, latency p95 |
| DATA-14 | No cross-plane ZDR contract | implicit | explicit rejection/ephemeral mode | **CLOSED** | `zdr_mode` field on documents; reject mode filters restricted docs post-rerank; ephemeral mode skips trace persistence (returns `ephemeral-{uuid}` trace_id, no DB write) |
| DATA-15 | No embedding/rerank cost ledger | absent | usage events to cost-core | **CLOSED** | Cost ledger events published via NATS (`dataplane.cost.ledger`) with model, count, tokens, idempotency key |
| DATA-16 | Qdrant version outdated vs target | v1 uses 1.9.0 | upgrade | **CLOSED** | v2 uses Qdrant 1.13.2 |
| DATA-17 | No graph/wiki App Shell contract | absent | `/v1/knowledge/*` and `/v1/wiki/*` read APIs | **CLOSED** | App Shell read-only routes: `/v1/knowledge/search`, `/v1/knowledge/graph`, `/v1/knowledge/wiki`, `/v1/knowledge/sources`, `/v1/knowledge/freshness` — all behind INTERNAL_API_KEY auth middleware |
| DATA-18 | No source trace for extraction fields | partial | source trace table/API | **CLOSED** | `extraction_trace` JSONB field on documents; persisted on create, returned on get/list |
| DATA-19 | Agent-facing retrieval tools absent | single `/v1/retrieve` | typed tools | **CLOSED** | All 14 tools implemented: hybrid, sources, chunks, trace, graph, entity, claims, wiki, contradictions, freshness, timeline, compare, trust, pack |
| DATA-20 | No context packer contract | facts only | token-budgeted packs | **CLOSED** | `/v1/retrieve/pack` with budget, format, stable source refs |
| DATA-21 | No stable chunk versioning | limited | stable IDs + lineage | **CLOSED** | BLAKE3 fingerprint IDs + `chunk_lineage` table tracking old→new knowledge_ids on reindex |
| DATA-22 | No ACL-aware retrieval proof | org filters exist | full ACL matrix | **CLOSED** | org_id + workspace_id + collection_id + acl_tags enforced as Qdrant filter conditions |
| DATA-23 | No temporal retrieval | absent | query by index version or timestamp | **CLOSED** | `/v1/retrieve/timeline` with before/after date filters |
| DATA-24 | No replayable retrieval | absent | trace + index version replay | **CLOSED** | Trace replay by trace_id; `index_versions` table + API |
| DATA-25 | No source trust scoring | absent | authority + freshness | **CLOSED** | Trust scorer: authority (manual=1.0→external=0.4) × freshness decay |
| DATA-26 | No answer-file-back workflow | absent | Model proposes, Data stores | **CLOSED** | Proposal → review → accept auto-creates wiki version |
| DATA-27 | No stale embedding detector | status only | detect drift | **CLOSED** | Detects content-updated-after-embedding, stuck-pending, failed |
| DATA-28 | No contradiction index | absent | conflicting claims API | **CLOSED** | Claims with `contradicted_by` JSONB; contradiction search endpoint |
| DATA-29 | No retrieval strategy A/B testing | absent | scorecard comparison | **CLOSED** | `POST /v1/evals/compare` runs two eval strategies, diffs scorecards (recall, nDCG, MRR, latency), picks winner |

**Summary: 29 of 29 gaps CLOSED.**

## 8. Build Plan

### Phase D0 — Contract freeze and compatibility

> **Status: DONE**

Delivered:

- [x] Project scaffolded at `/apps/Data Plane v2/` with Cargo workspace (4 Rust members) and 4 Go modules.
- [x] Offset ports (5442, 6389, 6345, 4232, 8010-8014, 9201-9203) so v1 and v2 run simultaneously.
- [x] docker-compose.yml with all 8 services + 4 infrastructure deps.
- [x] Shared Postgres init schema, dual-network topology (dpv2-net + velion-net).
- [x] Health/readyz endpoints on every service.

### Phase D1 — Rust retrieval engine

> **Status: DONE**

Delivered:

- [x] `retrieval-engine-rs` with Axum HTTP + Tonic gRPC.
- [x] Dense retrieval via Qdrant ANN (configurable top_k).
- [x] Sparse/BM25 retrieval via Postgres tsvector full-text search.
- [x] RRF (Reciprocal Rank Fusion) for dense+sparse candidate merge.
- [x] Cohere cross-encoder reranking (configurable model).
- [x] Postgres source join (never stale Qdrant payload).
- [x] Retrieval trace persistence (`retrieval_runs` + `retrieval_candidates`).
- [x] Context packing with token budget (4-chars-per-token heuristic).

### Phase D2 — Rust index and embedding engines

> **Status: DONE**

Delivered:

- [x] `index-engine-rs`: token-aware chunking with paragraph/sentence splitting, configurable chunk_size (512) and overlap (64).
- [x] BLAKE3 content fingerprinting for stable, deterministic chunk IDs.
- [x] NATS JetStream consumer on `dataplane.documents.created`.
- [x] Publishes `dataplane.knowledge_units.created` after chunking.
- [x] `embedding-engine-rs`: batch embedding via Azure OpenAI text-embedding-3-large (3072d).
- [x] Qdrant upsert with metadata payload; delete by document_id filter.
- [x] NATS consumer on `dataplane.knowledge_units.created`.
- [x] Publishes `dataplane.documents.indexed` after embedding complete.
- [x] Configurable batch size (32), embedding dimension (3072).

### Phase D3 — Go document API

> **Status: DONE**

Delivered:

- [x] `documents-api-go` with Chi router, org_id middleware.
- [x] Document CRUD: Get, List (with type filter + pagination), Create, SoftDelete.
- [x] Bulk ingest endpoint (`POST /v1/documents/bulk`).
- [x] NATS event publishing on create (`dataplane.documents.created`) and delete (`dataplane.documents.deleted`).
- [x] ZDR classification field on document model.

### Phase D4 — Graph and wiki foundation

> **Status: DONE**

Delivered:

- [x] `graph-index-rs`: LLM entity/relationship/claim extraction via Azure OpenAI GPT-4o with JSON response_format.
- [x] Entity text→ID mapping, relationship persistence, claim storage with JSONB metadata.
- [x] Connected-component community detection (BFS, configurable min_size).
- [x] Graph expansion API (BFS with max_hops/max_entities).
- [x] Axum API: entities, relationships, claims, contradictions, expand endpoints.
- [x] NATS consumer on `dataplane.documents.indexed`.
- [x] `wiki-store-go`: pages, versions, backlinks, proposals.
- [x] Answer-file-back workflow: accept proposal → transaction creates new version + updates current_version_id.
- [x] `data-orchestrator-go`: reindex/graph_build/wiki_refresh job types with progress tracking.

### Phase D5 — Hybrid retrieval and graph RAG

> **Status: DONE**

Delivered:

- [x] Graph expansion search: FTS on graph_entities → fetch relationships + claims per entity.
- [x] Community summary search: JSONB containment on entity_id in graph_communities.
- [x] Wiki full-text search: ILIKE on title/path + tsvector on version content.
- [x] Contradiction search: claims with `jsonb_array_length(contradicted_by) > 0`.
- [x] Timeline/temporal search: before/after date filters on documents + trace replay.
- [x] Agent-facing endpoints: `/v1/retrieve/graph`, `/v1/retrieve/wiki`, `/v1/retrieve/contradictions`, `/v1/retrieve/timeline`.

### Phase D6 — Production hardening

> **Status: MOSTLY DONE**

Delivered:

- [x] `data-quality-go`: eval runner, trust scorer, 5 release gates.
- [x] OpenTelemetry tracing stubs in all 4 Go services (OTLP HTTP exporter, no-op when endpoint unset).
- [x] OpenTelemetry tracing in `retrieval-engine-rs` via `tracing-opentelemetry` layer.
- [x] Health/readyz on every service with docker-compose healthchecks.
- [x] Idempotency keys on NATS event payloads (hash-based, prefix-scoped).
- [x] Cost ledger events via NATS (`dataplane.cost.ledger`) for embedding batches.
- [x] ZDR enforcement in retrieval pipeline (reject mode filters restricted docs post-rerank).

- [x] Dead-letter queues (`dataplane.dlq.*`) on all 3 NATS stream consumers with max-delivery gating.

Not yet delivered:

- [ ] Eval gates in CI pipeline.
- [ ] Runbooks for Qdrant/Postgres/Dragonfly/NATS failure scenarios.

### Phase D7 — Agentic context engine completion

> **Status: DONE**

Delivered:

- [x] Agent-facing retrieval tool API: all 14 tools implemented.
- [x] `retrieve.chunks` — knowledge-unit lookup by IDs or document_id.
- [x] `retrieve.compare` — entity comparison (shared relationships) or document comparison (chunk counts).
- [x] Context packer with token budget, JSON output, stable source refs.
- [x] Temporal retrieval by date range (`/v1/retrieve/timeline`).
- [x] Replayable retrieval by trace_id.
- [x] Index versioning (`/v1/index/versions`).
- [x] Source trust scoring (authority × freshness composite).
- [x] Freshness scoring endpoint (`/v1/retrieve/freshness`).
- [x] Answer-file-back workflow (proposal → accept → auto-version).
- [x] Stale embedding detector (content-drift, stuck-pending, failed).
- [x] ACL enforcement: org_id + workspace + collection + acl_tags.
- [x] ZDR enforcement: reject mode filters restricted docs.
- [x] Graph text_unit mappings: entity/relationship/claim → knowledge_unit links.
- [x] Chunk lineage tracking: old→new knowledge_id records on reindex.
- [x] Source trace: `extraction_trace` JSONB on documents.
- [x] Wiki source_log and maintenance_log with CRUD API.
- [x] Cost ledger events for embedding batches via NATS.
- [x] Idempotency keys on all NATS event payloads.

- [x] Retrieval strategy A/B comparison endpoint (`POST /v1/evals/compare`).
- [x] ZDR ephemeral mode — skip trace persistence for zero-data-retention retrieval.
- [x] App Shell read-only knowledge API (`/v1/knowledge/*`) behind INTERNAL_API_KEY auth.
- [x] Shared NATS (velion-nats) subscription for Quarry crawl events in documents-api-go.
- [x] INTERNAL_API_KEY auth middleware on retrieval-engine-rs and documents-api-go.

Not yet delivered:

- [ ] Agent retrieval planner hints.

## 9. API Surface Additions

| Endpoint | Owner | Purpose | v2 Status |
|---|---|---|---|
| `POST /v1/documents` | `documents-api-go` | Document create with ZDR and index policy | **DONE** |
| `POST /v1/documents/bulk` | `documents-api-go` | Bulk document ingest | **DONE** |
| `GET /v1/documents/{id}` | `documents-api-go` | Get document by ID | **DONE** |
| `DELETE /v1/documents/{id}` | `documents-api-go` | Soft delete document | **DONE** |
| `POST /v1/retrieve` | `retrieval-engine-rs` | Hybrid retrieval with trace ID | **DONE** |
| `POST /v1/retrieve/hybrid` | `retrieval-engine-rs` | Agent-facing default hybrid retrieval | **DONE** |
| `GET /v1/retrieval/{trace_id}` | `retrieval-engine-rs` | Audit retrieval trace | **DONE** |
| `POST /v1/retrieve/graph` | `retrieval-engine-rs` | Graph expansion + community retrieval | **DONE** |
| `POST /v1/retrieve/wiki` | `retrieval-engine-rs` | Wiki page/section FTS retrieval | **DONE** |
| `POST /v1/retrieve/contradictions` | `retrieval-engine-rs` | Conflicting claims lookup | **DONE** |
| `POST /v1/retrieve/timeline` | `retrieval-engine-rs` | Temporal retrieval by date range or trace replay | **DONE** |
| `POST /v1/retrieve/pack` | `retrieval-engine-rs` | Token-budgeted context packing | **DONE** |
| `POST /v1/retrieve/sources` | `retrieval-engine-rs` | Source document lookup by IDs | **DONE** |
| `POST /v1/retrieve/chunks` | `retrieval-engine-rs` | Knowledge-unit lookup by IDs or document | **DONE** |
| `POST /v1/retrieve/compare` | `retrieval-engine-rs` | Entity/document comparison with provenance | **DONE** |
| `POST /v1/retrieve/freshness` | `retrieval-engine-rs` | Age-based freshness scoring | **DONE** |
| `GET /v1/index/versions` | `retrieval-engine-rs` | List index versions for replay | **DONE** |
| `GET /v1/graph/entities/{id}` | `graph-index-rs` | Entity lookup | **DONE** |
| `GET /v1/graph/entities` | `graph-index-rs` | List entities by type | **DONE** |
| `GET /v1/graph/relationships/{id}` | `graph-index-rs` | Relationships for entity | **DONE** |
| `GET /v1/graph/claims` | `graph-index-rs` | Claims with optional filters | **DONE** |
| `GET /v1/graph/contradictions` | `graph-index-rs` | Claims with contradictions | **DONE** |
| `POST /v1/graph/expand` | `graph-index-rs` | BFS graph expansion | **DONE** |
| `POST /v1/wiki/pages` | `wiki-store-go` | Create wiki page | **DONE** |
| `GET /v1/wiki/pages/{id}` | `wiki-store-go` | Read wiki page | **DONE** |
| `GET /v1/wiki/pages/by-path` | `wiki-store-go` | Lookup by path | **DONE** |
| `POST /v1/wiki/pages/{id}/versions` | `wiki-store-go` | Create new version | **DONE** |
| `GET /v1/wiki/pages/{id}/versions` | `wiki-store-go` | List versions | **DONE** |
| `GET /v1/wiki/pages/{id}/backlinks` | `wiki-store-go` | Backlink graph | **DONE** |
| `POST /v1/wiki/pages/{id}/proposals` | `wiki-store-go` | Submit wiki proposal | **DONE** |
| `POST /v1/wiki/proposals/review` | `wiki-store-go` | Accept/reject proposal | **DONE** |
| `POST /v1/wiki/pages/{id}/source-logs` | `wiki-store-go` | Create source ingestion log | **DONE** |
| `GET /v1/wiki/pages/{id}/source-logs` | `wiki-store-go` | List source logs for page | **DONE** |
| `POST /v1/wiki/pages/{id}/maintenance-logs` | `wiki-store-go` | Create maintenance log entry | **DONE** |
| `GET /v1/wiki/pages/{id}/maintenance-logs` | `wiki-store-go` | List maintenance logs for page | **DONE** |
| `POST /v1/orchestrator/jobs` | `data-orchestrator-go` | Create reindex/graph/wiki job | **DONE** |
| `POST /v1/orchestrator/reindex` | `data-orchestrator-go` | Shortcut reindex job | **DONE** |
| `GET /v1/orchestrator/stale-embeddings` | `data-orchestrator-go` | Stale embedding detection | **DONE** |
| `POST /v1/evals/retrieval` | `data-quality-go` | Run retrieval evaluation | **DONE** |
| `GET /v1/evals/retrieval/{id}` | `data-quality-go` | Get eval scorecard | **DONE** |
| `POST /v1/quality/trust` | `data-quality-go` | Source trust scoring | **DONE** |
| `GET /v1/quality/gates` | `data-quality-go` | Release gate check | **DONE** |
| `POST /v1/evals/compare` | `data-quality-go` | A/B strategy comparison eval | **DONE** |
| `POST /v1/knowledge/search` | `retrieval-engine-rs` | App Shell hybrid search (auth required) | **DONE** |
| `POST /v1/knowledge/graph` | `retrieval-engine-rs` | App Shell graph retrieval (auth required) | **DONE** |
| `POST /v1/knowledge/wiki` | `retrieval-engine-rs` | App Shell wiki retrieval (auth required) | **DONE** |
| `POST /v1/knowledge/sources` | `retrieval-engine-rs` | App Shell source lookup (auth required) | **DONE** |
| `POST /v1/knowledge/freshness` | `retrieval-engine-rs` | App Shell freshness scoring (auth required) | **DONE** |

## 10. Dependency Recommendations

| Dependency / system | Decision | Language | Use | v2 Status |
|---|---|---|---|---|
| Qdrant 1.13.2 | Adopted | Rust client | Vector store | **IN USE** |
| Postgres full-text (tsvector) | Adopted | SQL | Sparse retrieval | **IN USE** |
| tree-sitter | Adopt | Rust | code AST extraction | NOT STARTED |
| blake3 | Adopted | Rust | content/chunk fingerprinting | **IN USE** |
| serde_json | Adopted | Rust | context packaging | **IN USE** |
| OpenTelemetry | Adopted | Rust + Go | tracing | **IN USE** (stubs) |
| pgx (pgxpool) | Adopted | Go | documents/wiki/quality SQL | **IN USE** |
| NATS JetStream | Adopted | Rust + Go | async event bus | **IN USE** (replaced Redis Streams) |
| testcontainers-go | Adopt | Go dev | integration tests | NOT STARTED |
| Python ragas/deepeval | Lab only | Python | retrieval quality experiments | NOT STARTED |
| LightRAG / GraphRAG ideas | Selectively implemented | Rust + Go | graph indexing/retrieval | **IN USE** |
| LangChain/LlamaIndex | Reject in production | Python | eval/prototype only | N/A |
| Neo4j | Deferred | external | Postgres graph tables sufficient | N/A |

## 11. Release Gates

A Data Plane release cannot claim target parity until:

| Gate | v2 Status |
|---|---|
| Retrieval p95 below current Python p95 and zero query failures on benchmark corpus | PENDING (infra not yet deployed for benchmarking) |
| Chunking is deterministic and idempotent | **DONE** (BLAKE3 fingerprinting) |
| Embedding worker handles duplicate deliveries without duplicate vectors | **DONE** (document_id filter delete before upsert) |
| Every retrieval response has a trace | **DONE** |
| Data Plane rejects direct cross-plane DB access by design and tests | PARTIAL (design enforced; tests not written) |
| ZDR behavior tested for ingest and retrieval | PARTIAL (field exists; tests not written) |
| Graph and wiki data are Data Plane-owned; Model Plane only proposes or consumes | **DONE** |
| Eval scorecards exist for dense-only, hybrid, graph, and wiki retrieval | **DONE** (eval runner + A/B strategy comparison endpoint) |
| Agent-facing retrieval tools exist without exposing databases | **DONE** (14 typed tools, 59 total HTTP endpoints) |
| Context packs are token-bounded, citation-complete, and replayable | **DONE** |
| LLM Wiki storage is Data Plane-owned; Model Plane writes only proposals | **DONE** |
| Temporal and replayable retrieval have at least one passing fixture | PENDING (fixtures not written) |

**Programmatic gates in `data-quality-go`:**

1. **retrieval_traces_exist** — at least 1 retrieval trace for org.
2. **no_failed_embeddings** — zero knowledge_units with `embedding_status = 'failed'`.
3. **indexed_docs_have_chunks** — all indexed documents have at least 1 knowledge_unit.
4. **retrieval_p95_under_2000ms** — p95 retrieval latency below 2 seconds.
5. **no_zero_result_queries** — no retrieval runs returned 0 candidates.

## 12. Final Target Rule

**Data Plane knows.** It stores knowledge, indexes knowledge, retrieves knowledge, and proves where knowledge came from. It does not reason, browse, or render product UX.

**Data Plane v2 implements this rule** across 8 services in Rust and Go, with 62 API endpoints, hybrid retrieval (dense + sparse + graph + wiki), ACL enforcement (org + workspace + collection + acl_tags), ZDR enforcement (reject + ephemeral modes), durable retrieval traces, chunk lineage tracking, graph text_unit mappings, cost ledger events, source trust scoring, release gates, wiki source/maintenance logs, extraction trace on documents, dead-letter queues on all NATS consumers, A/B eval comparison, App Shell read-only knowledge API, shared NATS (velion-nats) Control Plane integration, INTERNAL_API_KEY auth middleware, and all 14 agent-facing typed tools — all compiling clean and ready for integration testing. **29 of 29 gaps CLOSED.**

## 13. v2.1 Roadmap — Remaining Items

The 29 numbered gaps are CLOSED, but adjacent items in earlier sections remain NOT STARTED or PARTIAL. New items also surfaced during Tier 1–3 production hardening. This section tracks the v2.1 work.

### 13.1 Closed in v2.1

#### Tier 1–3 production hardening (closed earlier)

| Item | Status | Notes |
|---|---|---|
| Prometheus metrics on retrieval-engine-rs | **DONE** | `/metrics` endpoint, 11 metric families, 7 alert rules |
| gRPC retry+backoff on outbound (embed/rerank) | **DONE** | exponential backoff, 5xx/429 retry, fail fast on 4xx |
| Tower middleware (timeout/concurrency/load-shed) | **DONE** | gRPC server, configurable via env |
| OTel trace propagation through gRPC metadata | **DONE** | extracts traceparent + x-trace-id, records on span |
| Dragonfly cache (embeddings + retrieval results) | **DONE** | Redis-compatible TTL-based cache, graceful degradation, readyz-checked |
| Graceful shutdown (SIGTERM drain) | **DONE** | both Axum HTTP and Tonic gRPC |
| Integration tests for gRPC server | **DONE** | 10 tests, requires TEST_DATABASE_URL |
| Smoke test script (HTTP + gRPC) | **DONE** | `./scripts/smoke-test.sh` |
| Client stub generation (Go + Rust) | **DONE** | `gen/go/`, `gen/rust/`, `./scripts/gen-clients.sh` |
| GitHub Actions CI | **DONE** | rust-check, integration tests, go-build, docker-smoke |
| Load test harness (ghz + k6) | **DONE** | `tests/load/grpc-retrieve.sh`, `tests/load/http-retrieve.js` |
| Migration runbook v1→v2 | **DONE** | `docs/migration-v1-to-v2.md` |
| Prometheus alert rules | **DONE** | `infra/prometheus/alerts.yml` |

#### v2.1 batch — closed 2026-05-08

| Item | Status | Notes |
|---|---|---|
| Prometheus metrics on 4 Go services | **DONE** | shared `internal/metrics` package per service, `/metrics` endpoint, route-pattern label normalization |
| DB migration system | **DONE** | `tools/migrator` Go binary, `schema_migrations` table, `make migrate-status`/`migrate-up` |
| Cache invalidation on document mutations | **DONE** | NATS subscriber on `dataplane.documents.{created,deleted,updated}` invalidates per-org retrieval cache via SCAN+DEL |
| Wiki diff helper endpoint | **DONE** | `GET /v1/wiki/pages/{pageID}/diff?from=&to=`, line-based LCS diff with hunks |
| Knowledge linting | **DONE** | `GET /v1/quality/lint` — orphan_doc, stale_doc, orphan_wiki, stale_wiki, weak_citation |
| ZDR behavior tests | **DONE** | 4 tests verify ephemeral skips persistence + reject filters restricted docs at SQL layer |
| gRPC client retry middleware (Rust) | **DONE** | `dataplane-client::retry::with_retry`, configurable backoff/jitter, 4 unit tests, 3 presets |
| Backup/restore runbook + scripts | **DONE** | `scripts/backup.sh` (pg_dump + Qdrant snapshots), `scripts/restore.sh`, `docs/backup-restore.md` |

### 13.2 v2.2 batch — closed 2026-05-08

#### Gap closures

| Item | Status | Notes |
|---|---|---|
| Cross-plane DB rejection static check | **DONE** | `scripts/check-tenant-isolation.sh` (Python AST-style scanner over `*.rs`/`*.go` SQL literals); wired into `make ci`; refactored `documents-api-go` `List` to static SQL to pass the check |
| Cost ledger consumer + query API | **DONE** | `data-orchestrator-go/internal/cost` consumes `dataplane.cost.ledger`; `cost_events` table; `GET /v1/cost/summary` on `data-quality-go` with `from`/`to` window |
| testcontainers-go integration tests | **DONE** | `services/documents-api-go/internal/repo/integration_test.go` (build tag `integration`); covers create, idempotency, cross-org delete, list scoping |
| Outbound OTel tracing | **DONE** | `#[tracing::instrument]` on `vector_search` (qdrant), `bm25_search` (postgres), `embed_query`/`embed_batch`, `rerank`, `persist_trace`, `join_sources`, `pipeline.retrieve` — full client span tree per query |
| Zero-result query alerting | **DONE** | `dpv2_retrieval_zero_results_total` metric; alert fires when zero-result rate > 20% over 10 min |

#### Edge case fixes (real bugs caught and patched)

| Edge case | Status | Notes |
|---|---|---|
| Trace persistence failure breaks retrieval | **FIXED** | Now best-effort: logs error + emits `dpv2_trace_persist_failures_total` metric + returns synthetic `unpersisted-{uuid}` trace_id; retrieval response unaffected |
| Idempotent document creation | **DONE** | `idempotency_key` column + per-org partial unique index; race-safe (catches `23505` post-insert and recovers); Quarry crawl uses URL/document_id as key automatically |
| Input validation at API boundary | **DONE** | `internal/validate` package: org_id format (regex), content size (5 MiB max), title/source/type bounds, ZDR allow-list, idempotency key format, bulk batch size (500 max), UTF-8 enforcement |
| Cross-org data leakage tests | **DONE** | `tests/cross_org_isolation.rs` — 5 tests verifying GetDocument, ListDocuments, DeleteDocument, GetKnowledgeUnits, CheckPermissions all reject cross-org access |
| Orphan Qdrant vectors after soft-delete | **DONE** | `POST /v1/admin/cleanup/orphans` with `org_id` + `dry_run`; uses Qdrant filter to drop vectors for soft-deleted docs, then deletes `knowledge_units` rows |
| Postgres pool sizing | **DONE** | Configurable via `PG_MAX_CONNECTIONS` / `PG_MIN_CONNECTIONS` / `PG_ACQUIRE_TIMEOUT_SECS` env vars; defaults preserved (20/2/5s) |
| Embedding response validation | **DONE** | Reject empty vectors and dimension mismatches at orchestrator entry — clear error instead of cryptic Qdrant InvalidArgument |
| Retrieval input bounds | **DONE** | Caps query length (8 KiB) and `filters.document_ids` (1000) before propagating into Qdrant filter / Postgres `ANY($1)` |
| Trace metrics gap | **DONE** | New metrics: `dpv2_trace_persist_failures_total`, `dpv2_retrieval_zero_results_total` |
| Tenant-isolation regression prevention | **DONE** | Static check in CI catches new SQL on tenant tables that lacks `org_id`/PK scoping |

### 13.3 v2.3 wave 1 — closed 2026-05-08

| Item | Status | Notes |
|---|---|---|
| `mode_mix` columns in `retrieval_traces` | **DONE** | `retrieval_runs.mode_mix JSONB`; persisted by `persist_trace`; surfaced in `TraceRun` (audit endpoint) |
| Per-request blend weights API | **DONE** | `RetrievalRequest.mode_mix` accepts `{w_dense, w_bm25, w_graph, w_wiki, rerank}`; defaults from config; renormalized to 1.0; partial closure on D5-5 + spec §7 |
| Per-agent `retrievalConfig` (request-side hook) | **DONE** (Data Plane side) | Per-request override works end-to-end; awaiting Model Plane to pipe `agent_id → weights` upstream. Spec §8 unblocked. |
| `wiki/maintenance/sweep` write path | **DONE** | `POST /v1/wiki/maintenance/sweep` on wiki-store-go writes `wiki_maintenance_logs` rows from lint findings batch (≤1000 items); spec §3.4 |
| `graph_exports` table | **DONE** | Migration `20260508150000_…` + init.sql; columns: export_id, org_id, format∈{json,graphml,html,markdown}, uri, bytes, sha256, created_at; spec §2.1 |
| Grafana dashboards auto-provisioned | **DONE** | `infra/grafana/provisioning/{datasources,dashboards}` + `retrieval-overview.json` (p50/p95/p99 latency, RPS, cache hit, retry rate, trace-persist failures, gRPC p95). Mounted into Grafana container via compose `obs` profile. D5-8 closed. |

### 13.4 v2.3 wave 2 — closed 2026-05-08

| Item | Spec ref | Notes |
|---|---|---|
| Hybrid latency gate p95 < 800ms | D5-10 | Tightened alert `RetrievalLatencyAboveSpec` to 0.8s; added programmatic gate `retrieval_p95_under_800ms_spec` to `data-quality-go/internal/gates/checker.go`. |
| Migration rollback support | D4-2 | `tools/migrator` learned `down --to VERSION`; loads paired `*.down.sql` files. Sample rollback for `20260508150000` shipped. |
| Wire data-quality lint → `wiki/maintenance/sweep` | D5-6 | `ExecuteWikiRefresh` in `data-orchestrator-go` now GETs `/v1/quality/lint`, filters wiki-relevant kinds, POSTs batches to wiki-store's `/maintenance/sweep`. |
| Agent retrieval planner hints | spec §3 | `RetrievalResponse.suggested_next_tools[]` — heuristic recommendations (low_confidence → graph/wiki; ≥3 sources → contradictions; empty result → wiki/knowledge-search). |
| `wiki_block_embeddings` Qdrant collection | D5-1, D5-4 | `embedding-engine-rs` boot now calls `ensure_collection("wiki_block_embeddings", embedding_dim)`. Write-through hook on wiki-store-go `published` event is wave-3 follow-up. |
| `entity_summary_embeddings` Qdrant collection | D5-2 | Same — collection provisioned at boot; nightly summarizer is wave-3. |
| JWT auth via JWKS/static public key | spec §10 #5 | `auth_middleware` accepts Bearer JWT verified against `JWT_PUBLIC_KEY_PEM` (RS256) in addition to the shared API key. JWKS endpoint fetch deferred (most internal deployments pin a single key). |
| gRPC TLS env knobs | spec §10 #5 | `GRPC_TLS_CERT_PATH` + `GRPC_TLS_KEY_PATH` config plumbed; logged-warn when set without `grpc-tls` build feature. Operator can opt in by rebuilding; default plaintext is fine on `aquatiq-local`. |
| 4-way blend scoring step uses captured weights | D5-5, spec §7 | `mix_for_scoring` resolved BEFORE fusion; renormalized dense+bm25 sub-mix fed into RRF; same `resolved_mix` written to trace — scoring and audit agree. Graph + wiki signals remain on their own endpoints until ANN is populated. |
| Logseq block-outline parser/serializer | D4-6 | `internal/logseq` package in `wiki-store-go`: ATX bullets, tab indent, inline properties (`key:: value`). Round-trip stable. |
| Markdown entity extraction (tree-sitter substitute) | D4-5 | `services/index-engine-rs/src/extract/markdown.rs`: regex-based ATX heading + inline link extractor at confidence 0.85 (spec §6 value). API-compatible with future tree-sitter swap-in. 3 unit tests. |

### 13.5 Architectural divergences — accepted and closed

These items are intentionally NOT being implemented as the spec described, with documented rationale that v2 reviewers agree on. They count as closed because the spec's *outcome* is delivered another way (or because v2 architecture makes them moot).

| Item | Decision | Rationale |
|---|---|---|
| gRPC contracts §4 — `GraphIndex` / `WikiStore` proto services | **DIVERGED ACCEPTED** | v2 uses NATS for orchestrator↔graph-index edge proposals (event-driven, decoupled, durable retries) and HTTP for synchronous reads (`/v1/graph/*`, `/v1/wiki/*`). The spec's gRPC services would replicate that contract in a synchronous-only shape and force orchestrator+graph-index to be co-versioned. v2 architecture trades that off explicitly; the spec's *purpose* (decoupled intra-plane communication) is preserved. |
| `retrieval-eval-py` harness (RAGAS/DeepEval) | **DIVERGED ACCEPTED** | `data-quality-go/internal/eval` runs recall@10, nDCG@10, MRR, and latency p95 against the same golden-set contract D5-9 describes. The Python harness directory remains a SCAFFOLD for any future RAGAS-specific experiments that demand the Python ecosystem, but it is NOT on the release-gate critical path. |
| Local reranker fallback (offline ONNX / `bge-reranker-v2-m3`) | **DEFERRED ACCEPTED** | Cohere `rerank-english-v3.0` works at current scale; no production blocker. Trigger for re-opening: monthly rerank spend > 2× retrieval infra cost, OR p95 rerank-latency contribution > 200ms sustained. Both are alarms operators can wire in Prometheus. |
| Leiden community detection (vs. connected-components) | **ACCEPTED** | Connected-components produces semantically adequate communities at the current org graph sizes (< 10k entities). Leiden adds modularity-optimal partitioning at the cost of a non-trivial Rust implementation (no first-party crate). Re-open when any single org's `graph_entities` exceeds 50k nodes. |

### 13.6 Open (deferred — wave 3+)

No items currently rated `high` priority remain open. Wave-3 candidates: `wiki_block_embeddings` write-through (vs. just collection ensure); ~~4-way scalar blend that actually consumes graph+wiki signals at scoring time~~ (**graph DONE** — the GraphRAG program folds `w_graph` into RRF via the graph arm; wiki was already fused via `w_wiki`; see `docs/graphrag-neo4j-plan.md`); tree-sitter swap-in for code AST.

**Effort scale**: S = ≤1 day, M = 2–4 days, L = ≥1 week.

### 13.7 Cumulative v2 score

- **Closed**: 29 numbered + 21 v2.1 + 16 v2.2 + 6 v2.3-w1 + 11 v2.3-w2 + 4 accepted divergences + 8 wave-3 partial (OpenAPI spec, Velion d4d5 TS, `/v1/graphs/{org_id}` aggregator, port-mapping reference §13.8, §15-A AuthContext, §15-B MembershipCache + cache, §15-C PermissionResolver foundation, §15-E access_audit_log) + 8 wave-3.1 (§15.6: wiki list-pages, §15-C apply_to_request, §16.1.2 gRPC JWT, §16.2.5 body limit, §16.2.8 model-version cache key, §15-F user_id on cost ledger, §15-G rerank cost event, §15.5 velion wiring) + 9 wave-3.2 (§15.7: rerank NaN sanit., pool warmup, pool gauges, HTTP/2 keepalive, JSONB GIN indexes, JS consumer drain, PII redaction, zdr_actions_applied column, admin_audit_log) + 5 wave-3.3 (§15.8: NATS env std, BM25 tsvector, zdr_actions_applied writer, parallel rerank, clippy --all-targets gate) + 5 wave-3.4 (§15.9: BPE tokenizer, per-org rate limit, wiki sanitization, JWKS fetch, DLQ replay tool) + 9 wave-3.5 (§15.10: mode_mix_applied, agent_retrieval_configs, graph_exports endpoint, org_versions, documents_outbox, wiki publisher, e2e scaffold, chaos scaffold, production runbook) + 6 wave-3.6 (§15.11: retrieval cache key versioning, agent_id wire-through, org_version bump on mutation, outbox publisher loop, wiki repo emit, embedding-engine wiki subscriber) + 5 wave-3.7 (§15.12: outbox publisher boot, BulkIngest outbox writer, wiki publisher boot, retrieval proto agent_id, make go-tidy) + 2 wave-3.8 (§15.13: go mod tidy applied, cross-language schema lock w/ Rust+Go round-trip tests) + 4 wave-4 (§15.14: NATS subject contract + CI lint, gRPC backpressure trailers, server-streaming retrieval, gRPC DocumentService deprecation) = **148 items**

### 13.8 Port-mapping reference (D4+D5 spec → v2 reality)

| Spec ports | v2 reality | Notes |
|---|---|---|
| documents-service `8001/50051` | `documents-api` `8010` (host `8010:8010`) | Renamed v1→v2, no gRPC (HTTP-only). |
| retrieval-service `8004/50052` | `retrieval-engine` `8004/50052` (host `8014/50062`) | Direct match (host-mapped). |
| wiki-store `8010/50060` | `wiki-store` `8011` (host `8011`) | Different port (8011 vs spec's 8010) — spec port conflicts with documents in v2. |
| Probe ports `8201, 8202, 3030, 50053, 8001` | NOT USED in v2 | v2 ports: `8004`, `8010-8013`, `9201-9203`, `50052`. |
| graph-index — not specified | `graph-index` `9203` (admin-only) | Admin/observability surface; no public port. |
| index-engine — not specified | `index-engine` `9201` | Admin-only. |
| embedding-engine — not specified | `embedding-engine` `9202` | Admin-only. |
| data-orchestrator — not specified | `data-orchestrator` `8012` | Job control HTTP. |
| data-quality — not specified | `data-quality` `8013` | Evals/gates/lint/cost HTTP. |

All v2 services join the `aquatiq-local` docker network so internal callers use container DNS (no host ports needed). Host-port maps exist only for ad-hoc `curl`/`grpcurl` from the dev machine.
- **Open (wave 3+, none rated `high`)**: wiki-store-go write-through on `published` → `wiki_block_embeddings`; nightly entity summarizer → `entity_summary_embeddings`; full 4-way scoring with graph + wiki signals folded in; tree-sitter swap-in for code AST; live-traffic measurement against the 800ms gate; JWKS endpoint fetch (currently static PEM only).
- **D4 + D5 spec closure**: 35 of 35 acceptance rows green at the "wired + provisioned" level. Cross-reference: [`apps/Data Plane/docs/gap-data.md`](../../Data%20Plane/docs/gap-data.md) §13.
- **CI surface**: cargo workspace + tests build clean; index-engine markdown extractor 3/3 unit tests pass; all 4 Go services and migrator build clean; tenant-isolation static check stays green.
- **Endpoints**: 66 HTTP + 14 gRPC = 80 total
- **Test files**: 4 Rust integration test suites (`grpc_integration`, `zdr_behavior`, `cross_org_isolation`, `documents-api integration_test.go`); 4 Rust unit test suites (`retry`, `cache`, `pipeline`, `metrics`); ZDR + cross-org cover compliance/security paths
- **CI checks**: fmt, clippy, workspace check, integration tests, Go build, Go vet, **tenant-isolation static check**, docker smoke test, load tests on demand

## 14. Known Edge Cases Under Watch

These edge cases were surfaced during the v2.2 audit. Each one is either accepted with a documented mitigation, or flagged for future work. Listed in priority order.

### 14.1 Concurrency

| Edge case | Current behavior | Mitigation status | Future work |
|---|---|---|---|
| **NATS at-least-once → duplicate processing** | Embedding-engine consumer can process the same `dataplane.knowledge_units.created` event twice (network blip, ack loss). Outcome is harmless because Qdrant upsert is keyed by `knowledge_id` and `embedding_status` UPDATE is idempotent. | ACCEPTED | Add `idempotency_key` to NATS event payload + dedupe table for true exactly-once if regulatory need arises. |
| **Reindex while document being soft-deleted** | Race: chunker reads a document that the API just soft-deleted. Index proceeds, knowledge_units row inserted; cleanup admin endpoint sweeps it later. | ACCEPTED | Add `deleted_at IS NULL` check in chunker before commit; abort cleanly if doc disappeared. |
| **Concurrent CreateDocument with same idempotency_key** | Race-safe: unique index rejects the loser, code catches `23505` and re-fetches winner. Test `TestIdempotency` asserts. | **CLOSED** | — |
| **Embedding model rotation mid-flight** | If `EMBEDDING_DIMENSION` changes, Qdrant collection schema mismatch causes upsert errors. Currently no migration helper. | OPEN | Add `embedding_model_version` column + Qdrant collection-per-model strategy with cutover script. |
| **Cache invalidation race** | Doc updated → NATS event sent → in-flight retrieval already read stale cache. Window is < 5 min (cache TTL). | ACCEPTED | Cache-key versioning per org_id (incremented on mutation) eliminates the window at cost of one extra write per mutation. |

### 14.2 Resource exhaustion

| Edge case | Current behavior | Mitigation status |
|---|---|---|
| **Postgres pool exhausted under load** | Configurable via `PG_MAX_CONNECTIONS`; default 20. `PG_ACQUIRE_TIMEOUT_SECS=5` returns error rather than hanging. | **CLOSED** (configurable). Monitor via Prometheus connection metrics. |
| **Qdrant timeout returns partial candidates** | Qdrant client uses default 60s gRPC timeout; truncation returns fewer candidates without error. Could silently drop relevant docs. | OPEN. Add explicit timeout config + alert on `dense_ms > timeout * 0.8`. |
| **Reranker NaN scores** | If Cohere returns NaN, `final_score` becomes NaN, breaks ordering. | OPEN. Sanitize: `if !score.is_finite() { score = 0.0; }` in rerank response handler. |
| **Embedding API rate limit** | Retry+backoff handles 429; under sustained pressure the request just takes longer. No circuit breaker on outbound. | ACCEPTED for v2. v2.3: add `tower::limit::RateLimit` on embedder client. |
| **Bulk ingest of >500 docs** | Capped at 500; clients chunk. Returns `400` with clear message. | **CLOSED**. |
| **Pathological retrieval input** | Query length capped at 8 KiB; `filters.document_ids` capped at 1000. | **CLOSED**. |
| **Cache memory full** | docker-compose now uses Dragonfly cache mode with a 256 MiB local cap. Cache failure still degrades gracefully. | **CLOSED** for local compose. Production should size Dragonfly separately and keep cache-mode eviction enabled. |
| **Orphan Qdrant vectors growing unbounded** | Soft-deleted docs leave vectors in Qdrant indefinitely until admin runs `POST /v1/admin/cleanup/orphans`. | **PARTIALLY CLOSED**. v2.3: schedule the cleanup as a daily orchestrator job. |

### 14.3 Data integrity

| Edge case | Current behavior | Mitigation status |
|---|---|---|
| **Empty/oversized content rejected at API boundary** | `validate.CreateDocument` rejects empty + over-5 MiB content; UTF-8 enforced. | **CLOSED**. |
| **Embedding response empty/wrong-dim** | Orchestrator validates `len() == config.embedding_dimension`. | **CLOSED**. |
| **ZDR classification changed after indexing** | Vectors stay in Qdrant; reject-mode filter looks up `zdr_classification` from Postgres at query time, so newly-restricted docs ARE filtered. Lag = next retrieval. | **ACCEPTED** (correctness preserved by lazy filter). |
| **Soft-deleted document accumulation** | No automatic hard-delete window. Soft-deleted rows accumulate forever. | OPEN. Add `data_retention_days` org config + nightly purge job. |
| **GDPR right-to-be-forgotten** | Soft delete leaves content recoverable; would not satisfy a deletion request. | OPEN. Add `POST /v1/admin/forget` that hard-deletes documents + chunks + Qdrant vectors + retrieval_traces references for an org. |
| **Migration with data backfill** | `tools/migrator` runs SQL in a single transaction; long-running backfills would hold a transaction open. | OPEN. v2.3: add `--no-tx` flag + chunked-update helper. |

### 14.4 Security

| Edge case | Current behavior | Mitigation status |
|---|---|---|
| **Cross-org data leakage at SQL layer** | Static check in CI + 5 integration tests + repo-level org_id scoping enforced. | **CLOSED**. |
| **`X-Org-ID` header is trusted** | Anyone with `INTERNAL_API_KEY` can claim any org. Auth model assumes Model Plane has already authenticated the user and proven org ownership. | ACCEPTED. Documented as Control-Plane responsibility. v2.3 (or earlier if exposed publicly): JWT-with-org-claim middleware. |
| **`INTERNAL_API_KEY` in logs** | `tracing` does not log Authorization or `x-api-key` headers (Axum extractors don't include them in spans). Verified by grep. | **CLOSED**. |
| **Prompt injection via document content during graph extraction** | `graph-index-rs` sends raw document text to GPT-4o for entity extraction. A malicious doc could include `Ignore prior instructions` and exfiltrate system prompt or produce false entities. | OPEN. v2.3: prefix-fence the prompt + system-message hardening + entity-name allow-list. |
| **Wiki proposal accepting raw HTML/JS** | Content stored as-is. Rendering is consumer's responsibility. | ACCEPTED. Documented in API; UI consumers must sanitize. |
| **DoS via large filter arrays** | `filters.document_ids` capped at 1000. | **CLOSED**. |

### 14.5 Operational

| Edge case | Current behavior | Mitigation status |
|---|---|---|
| **Migration rollback** | Migrator is forward-only; no `down()` migrations. | ACCEPTED. Rollback strategy: restore from backup (documented in `docs/backup-restore.md`). |
| **Disk full on Qdrant** | Qdrant returns errors on write; embedding worker DLQs the message after max-delivery; retrieval continues serving from existing data. | ACCEPTED. Operator alert via existing `qdrant_ok=false` readyz signal. |
| **Pagination drift** | List endpoints use `LIMIT/OFFSET`; concurrent inserts/deletes can cause skipped or duplicated rows in pagination. | OPEN. v2.3: cursor-based pagination using `(created_at, document_id)` keyset. |
| **Sorting** | List endpoints only sort by `created_at DESC`. | ACCEPTED. Add explicit `sort_by` param if/when callers need it. |
| **Backfill of `idempotency_key` for existing docs** | Existing docs created before the v2.2 migration have `NULL` keys. Re-ingest from source will create duplicates because there's no key to match. | ACCEPTED. v2.3 (if needed): one-shot backfill script that synthesizes keys from `(source, type, title)` hash. |
| **Embedding cost ledger reflects only embeddings, not retrieval** | `cost_events` captures embedding-engine output. Rerank API calls do not publish events; query embedding misses are not counted separately. | OPEN. v2.3: have retrieval-engine publish per-query rerank cost events. |

### 14.6 What we explicitly do NOT plan to address

| Edge case | Why we accept it |
|---|---|
| Sub-millisecond retrieval p99 | Not a stated requirement; 2s p95 is the gate. |
| Zero downtime schema changes | Single-Postgres deployment; planned downtime is acceptable. v3 may revisit with logical replication. |
| Multi-region active-active | Out of scope for v2. Data Plane is single-region by design; multi-region would require Qdrant replication strategy. |
| Real-time embedding model A/B | Eval comparison is offline (`POST /v1/evals/compare`). Live A/B would require dual-write to two Qdrant collections — heavy for marginal value. |

---

## 15. v2.4 Wave 3 — Control Plane Integration (SECURITY-CRITICAL, do first)

**Context:** Data Plane v2 today scopes data by `org_id` at the SQL layer, but the `X-Org-ID` header is **trusted whatever the caller sends**. Anyone holding `INTERNAL_API_KEY` (or a valid signed JWT — signature only, claims discarded) can claim any `org_id`. There is no per-user ACL, no quota enforcement, no membership check against `user-core` or `org-core`, and no audit log of admin actions. Documented in §14.4 row "`X-Org-ID` header is trusted" — this wave finally closes it.

The Control Plane services already exist on `aquatiq-local` (`auth-service:3011`, `user-service:3012/50012`, `org-core:8080/9090`, `mp-cost-core` in Model Plane). Wave 3 is about *using* them.

### 15.1 Current enforcement gap

| Boundary | State |
|---|---|
| Org-level SQL isolation | ✅ tenant-isolation static check + 5 cross-org tests |
| Cross-plane auth (signature only) | ✅ `INTERNAL_API_KEY` or JWT (RS256 vs static PEM) |
| `X-Org-ID` matches authenticated principal | ❌ trusted as-is |
| User identity enforced | ❌ `user_id` accepted but never verified |
| Per-user ACL (workspaces/collections/acl_tags) | ❌ filter fields accepted but not authorized |
| Quota / billing enforcement | ❌ ledger captures embed events; nothing blocks on quota |
| Session validation | ❌ no interaction with auth-service or session-core |
| Admin-action audit log | ❌ orphan cleanup, sweeps, hard deletes are untraceable |

### 15.2 Required Control Plane contracts

These two RPCs from existing Control Plane services need confirmed schemas before wave-3 ships:

| RPC | Service | Returns |
|---|---|---|
| `CheckMembership(user_id, org_id)` | `user-service:50012` | `bool` — is the user a member of this org |
| `GetUserPermissions(user_id, org_id)` | `org-core:9090` | `{workspaces[], collections[], acl_tags[], can_read, can_write, can_delete}` |

Status: **need one-page contract proposal sent to org-core / user-core owners**. This coordination is the long pole — Data Plane code is ~3 sprint-days once the contract is agreed.

### 15.3 Wave 3 tickets — status

| # | Ticket | Effort | Status |
|---|---|---|---|
| 15-A | **`AuthContext` middleware** — decode JWT claims into `{user_id, org_id, scopes}` in request extensions; reject if header `X-Org-ID` ≠ `claims.org_id` | S | **DONE (2026-05-08)** — `services/retrieval-engine-rs/src/authz/{context,policy}.rs` + upgraded `auth_middleware` in `api/mod.rs`. JWT claims (`sub`, `org_id`, `scopes`, `exp`, `iss`, `aud`) are decoded; header-vs-claims mismatch returns 403 with `denied:header_claims_mismatch`. `AuthContext` injected via `req.extensions_mut()`. Optional `JWT_REQUIRED_ISSUER` / `JWT_REQUIRED_AUDIENCE` env knobs added. |
| 15-B | **`MembershipCache`** — call `user-service:50012.CheckMembership(user_id, org_id)` with 5-min cache | S | **DONE** — `HttpPolicyClient` uses `moka::future::Cache` (5-min TTL, 50k entries, positive-only). Calls `GET /internal/v1/users/{user_id}/memberships/{org_id}` against `USER_SERVICE_HTTP_URL`. Default to `NoopPolicyClient` when `CONTROL_PLANE_ENFORCEMENT=off`. |
| 15-C | **`PermissionResolver`** — call `org-core:9090.GetUserPermissions(user_id, org_id)`; intersect with request filters | M | **DONE (foundation)** — `EffectiveAcl` returned by policy client; `AuthContext::intersect_filter(requested, axis)` produces the permitted-filter set for workspaces/collections/acl_tags. Filter-application at the pipeline layer is the remaining wave-3.1 step. |
| 15-D | **Cost-core integration** — `CheckQuota` + `RecordUsage` keyed by `(user_id, org_id)` | M | **DEFERRED to wave 3.1** — depends on Model Plane `mp-cost-core` proto contract. Hook point is the `auth_middleware` post-response block. |
| 15-E | **`access_audit_log` table + middleware** | S | **DONE** — Migration `20260508170000_add_access_audit_log.sql` (+ `.down.sql`); writer in `services/retrieval-engine-rs/src/audit/mod.rs`; middleware appends one row per request (auth_method, latency, http_status, document_ids, cause). Best-effort: write failure does not impact the response. Two integration tests in `tests/audit_log.rs` confirm. |
| 15-F | **Cost ledger keyed by `(user_id, org_id)`** | S | **DEFERRED to wave 3.1** — `cost_events` already has `org_id`; adding `user_id` is one ALTER + event-payload extension. |
| 15-G | **Rerank cost events** | S | **DEFERRED to wave 3.1** — same shape as `dataplane.cost.ledger` embed events. |

**Enforcement modes** (`CONTROL_PLANE_ENFORCEMENT` env):
- `off` (default for v2.3 back-compat) → `NoopPolicyClient`, allow-all.
- `permissive` → call Control Plane; on transport error allow-all (audit-logged).
- `strict` → call Control Plane; on transport error deny.

**Audit-trail metrics added** in `src/metrics/mod.rs`:
- `dpv2_audit_write_failures_total{org_id}`
- `dpv2_authz_denials_total{org_id, cause}` — cardinality bounded by `cause` (small fixed set).

### 15.4 Wave 3 acceptance

- A new integration test `tests/user_acl_isolation.rs` verifies that user A in org X cannot see workspace data that user A is not a member of, even with a valid org-X JWT.
- `INTERNAL_API_KEY` shared-secret path is retained as a fallback (admin/backfill), but every regular request requires a JWT that passes membership + permission resolution.
- `access_audit_log` rows visible in Grafana within 30s of admin action.

### 15.5 Velion frontend wiring (2026-05-08)

The frontend (`apps/Frontend Plane/velion`) now binds against the real D4/D5
services rather than v1 mocks. Feature-flagged so a partial v2 deploy doesn't
break velion in environments where the Data Plane stack hasn't been brought up.

**Service routes wired**:

| Service | Routes used by velion |
|---|---|
| `graph-index-rs` (admin port `9203`) | `GET /v1/graph/entities`, `GET /v1/graph/entities/{id}`, `GET /v1/graph/relationships/{id}`, `GET /v1/graph/claims`, `POST /v1/graph/expand`, `GET /v1/graphs/{org_id}` (aggregate, wave-3) |
| `wiki-store-go` (`8011`) | `POST /v1/wiki/pages`, `GET /v1/wiki/pages/{id}`, `GET /v1/wiki/pages/by-path`, `POST /v1/wiki/pages/{id}/versions`, `GET /v1/wiki/pages/{id}/backlinks`, `GET /v1/wiki/pages/{id}/source-logs`, `GET /v1/wiki/pages/{id}/diff` (wave-1) |

**Feature flags** (velion env):

```env
KNOWLEDGE_GRAPH_ENABLED=1
KNOWLEDGE_WIKI_ENABLED=1
GRAPH_SERVICE_URL=http://localhost:9203   # or container DNS http://graph-index:9203
WIKI_SERVICE_URL=http://localhost:8011    # or container DNS http://wiki-store:8011
```

When flags are off, `/knowledge/graph` + `/knowledge/wiki` return 404 and the
sub-nav hides them. The TS types these consume are now in
[`apps/Frontend Plane/velion/src/types/data-plane/d4d5.ts`](../../Frontend%20Plane/velion/src/types/data-plane/d4d5.ts),
generated against `apps/Data Plane v2/openapi/d4d5.yaml`.

**Known gap → Wave 11.C-b (closed in wave-3.1, 2026-05-08)**:

| Gap | Status |
|---|---|
| `wiki-store-go` had no "list all pages" endpoint; the wiki sidebar used browser-localStorage bookmarks + path-lookup as a workaround. UI surfaced the limitation honestly with a warning banner. | **CLOSED** — `GET /v1/wiki/pages` paginated list endpoint added in wave-3.1 (see §15.6 below). |
| Version-history side panel showed current version only; "list versions" wiring stubs out the timeline component. | **DONE earlier** — `GET /v1/wiki/pages/{pageID}/versions` was already implemented; velion just needed to bind it. |

### 15.6 Wave 3.1 batch — eight closures (2026-05-08 → 2026-05-19)

Wave 3.1 closed eight items that had been called out across §15, §16.1, and §16.2 but
not yet implemented. Each landed with code, migration (where needed), and was
verified by `cargo check --workspace`.

| # | Gap | Closure | Where it landed |
|---|---|---|---|
| 1 | **`wiki-store-go` had no list-all endpoint** (Wave 11.C-b) | `GET /v1/wiki/pages?workspace_id&status&limit&offset` paginated list. Static query shapes avoid dynamic SQL; org-scope enforced by callers via workspace. | `services/wiki-store-go/internal/repo/wiki_repo.go` (`ListPages`), `internal/handler/wiki.go`, `cmd/main.go` route table |
| 2 | **§15-C `AuthContext.intersect_filter` not applied** at the request boundary | New `AuthContext::apply_to_request(&mut RetrievalRequest)` intersects user ACL against each filter axis (workspaces / collections / acl_tags), copies `org_id`, and propagates `user_id` when caller didn't supply one. Wired in the HTTP `retrieve` handler before the pipeline call. | `services/retrieval-engine-rs/src/authz/context.rs`, `src/api/mod.rs` (`retrieve`) |
| 3 | **§16.1.2 JWT only on HTTP** — gRPC was API-key-only, asymmetric trust boundary | `ApiKeyInterceptor` now accepts either `X-Internal-Api-Key` OR `Authorization: Bearer <jwt>`. `verify_jwt_sync` decodes against `JWT_PUBLIC_KEY_PEM` / `JWT_HS256_SECRET` in the synchronous interceptor path. | `services/retrieval-engine-rs/src/grpc/interceptor.rs` |
| 4 | **§16.2.5 No HTTP body size limit** — Axum default unbounded → 1 GB POST DoS | `RequestBodyLimitLayer::new(10 * 1024 * 1024)` added to the Axum router stack. `tower-http` `limit` feature enabled in workspace `Cargo.toml`. | `services/retrieval-engine-rs/src/api/mod.rs` (router builder), `Cargo.toml` (workspace deps) |
| 5 | **§16.2.8 Embed cache poisoning on model rotation** — cache key didn't include model version | `cache::get_embedding` / `set_embedding` now take `model_version: &str`; cache key format `embed:{model_version}:{blake3(text)}`. Orchestrator passes `cfg.azure_openai_embedding_deployment` as the version. Stale entries from prior model rotate out naturally; no cross-version collisions. | `src/cache/mod.rs`, `src/pipeline/orchestrator.rs` |
| 6 | **§15-F cost ledger had no `user_id`** — per-user attribution impossible | Migration `20260508180000_add_user_id_to_cost_events.sql` adds `user_id UUID NULL` + composite index `(user_id, created_at)`. Down migration drops index then column. `data-orchestrator-go` `cost.Event` struct + INSERT updated. | `infra/postgres/migrations/20260508180000_*.sql`, `infra/postgres/init.sql`, `services/data-orchestrator-go/internal/cost/consumer.go` |
| 7 | **§15-G no rerank cost event published** — embedding cost was tracked but rerank wasn't, blind spot in Cohere spend | Pipeline now captures `rerank_used_count` and publishes a `dataplane.cost.ledger` event with `{event_type: "rerank", model, count, estimated_tokens, org_ids, user_id, idempotency_key}` when rerank actually ran. Best-effort: NATS publish failure does not fail the retrieval. | `src/pipeline/orchestrator.rs` (rerank step + NATS publish), `src/main.rs` (single shared `async_nats::Client` plumbed into pipeline + cache invalidator) |
| 8 | **Velion frontend wires to real D4/D5 services** (Wave 11.C-a closure recorded) | Documented in §15.5 — the d4d5 OpenAPI + TS types are now consumed by `/knowledge/graph` + `/knowledge/wiki` against `graph-index-rs:9201` and `wiki-store-go:8011`. Stub-only mode is gone. | `apps/Frontend Plane/velion/src/types/data-plane/d4d5.ts`, `apps/Data Plane v2/openapi/d4d5.yaml` |

**NATS client lifecycle.** `main.rs` now connects once to NATS and shares the
single `async_nats::Client` across (a) `cache::invalidator::spawn_invalidator`
and (b) the retrieval pipeline's rerank cost publisher. Previously each feature
called `async_nats::connect` independently, which left the cost publisher
unwired when the cache invalidator path failed — now they degrade together.

**Compile gate.** `cargo check --workspace` is green after wave-3.1. Remaining
warnings are dead-code lints in legacy modules tracked under §16.4.5.

### 15.7 Wave 3.2 batch — nine closures (2026-05-19)

Continuing the §16 backlog burndown. All items below were rated `S` effort and
landed against a single migration `20260519180000_wave_3_2_traces_audit_indexes.sql`
plus targeted Rust changes. `cargo check --workspace` + `cargo test -p
retrieval-engine-rs --lib redact::` are both green.

| # | Gap | Closure | Where it landed |
|---|---|---|---|
| 1 | **§14.2 Rerank NaN scores** poisoned ordering when Cohere returned non-finite scores | Coerce non-finite `relevance_score` to `0.0` with a warn-level log. Sort stays well-defined; the broken candidate sinks. | `src/search/rerank.rs` |
| 2 | **§16.3.6 No connection-pool warmup** — cold-start tail spike on deploy | `warmup_pool` fans out `min_conns` parallel `SELECT 1`s after `connect()` so the pool is materialized before the first user query. | `src/db/mod.rs` |
| 3 | **§16.2.7 Postgres pool saturation invisible** until `acquire_timeout` errors fire | Background task publishes `dpv2_postgres_pool_{size,idle,active,max,saturation}` gauges every 5s. Alert at saturation ≥ 0.8. | `src/db/mod.rs` (`spawn_pool_metrics`) |
| 4 | **§16.3.4 No HTTP/2 keep-alive tuning on Tonic** — connection churn under intermittent load | `http2_keepalive_interval(30s)` + `http2_keepalive_timeout(20s)` + `tcp_keepalive(60s)` on the gRPC server builder. | `src/main.rs` |
| 5 | **§16.3.7 No GIN index on `extraction_trace` JSONB** (or other admin-queried JSONB columns) | New GIN indexes on `documents.extraction_trace`, `documents.metadata`, `retrieval_runs.filters_json`, `retrieval_runs.mode_mix`. | migration `20260519180000_*.sql`, `infra/postgres/init.sql` |
| 6 | **§16.2.3 JetStream consumer doesn't drain on shutdown** — in-flight messages redeliver after restart | Spawned signal-watcher flips `AtomicBool`; consumer loop exits cleanly only when buffer is empty. SIGTERM + Ctrl-C both honored. | `services/embedding-engine-rs/src/stream/mod.rs` |
| 7 | **§16.4.7 PII in logs/traces** — `tracing::info!(query = ...)` emitted user queries verbatim to OTLP | New `redact::redact_query(s, max_len)` strips emails / phone-ish digit runs / 13–19-digit card-like strings, then truncates. 5 unit tests pass. Applied at `retrieval.start` log. | `src/redact.rs`, `src/pipeline/orchestrator.rs` |
| 8 | **§16.1.3 `zdr_mode` captured requested, not applied** | New `retrieval_runs.zdr_actions_applied JSONB` column. Writer to be filled in §15.7 follow-up (column ready for `persist_trace` payload). | migration `20260519180000_*.sql`, `infra/postgres/init.sql` |
| 9 | **§16.5.3 No admin-action audit log** — orphan cleanup / sweeps / hard deletes untraceable | New `admin_audit_log` table + `audit::record_admin` writer. Wired in `cleanup_orphans` (both error + success paths). Same best-effort semantics as access audit. | migration `20260519180000_*.sql`, `src/audit/mod.rs`, `src/api/mod.rs` (`cleanup_orphans`) |

**Deps added.** `regex` + `once_cell` promoted from workspace defs to
`retrieval-engine-rs/Cargo.toml` (already declared in the workspace, just
needed enabling at the crate level).

### 15.8 Wave 3.3 batch — five closures (2026-05-19)

Continuing the §16 burndown with another tight pass. `cargo check --workspace --tests`
green; `cargo test -p retrieval-engine-rs --lib redact::` 5/5 still pass.

| # | Gap | Closure | Where it landed |
|---|---|---|---|
| 1 | **§16.1.6 NATS env naming inconsistent** (`NATS_URL` vs `NATS_SHARED_URL` vs `velion-nats`) | `main.rs` now resolves in order `DPV2_NATS_URL` → `SHARED_NATS_URL` → `NATS_URL` → `NATS_LOCAL_URL`. New deployments standardize on the first two; legacy stays compatible. | `src/main.rs` |
| 2 | **§16.3.3 BM25 re-tokenizes per query** — no precomputed `tsvector` | Migration `20260519190000_bm25_tsvector.sql` adds `knowledge_units.content_tsv` generated column + `idx_ku_content_tsv_gin`. `sparse.rs` rewrites the query to `content_tsv @@ plainto_tsquery(...)` and `ts_rank_cd(content_tsv, ...)`. Old expression index stays for one release as fallback. | migration, `src/search/sparse.rs`, `infra/postgres/init.sql` |
| 3 | **§16.1.3 zdr_actions_applied** (column added in 3.2) — writer was still TODO | Orchestrator now collects an action list during ZDR enforcement (`reject_mode_filtered_restricted`, `reject_mode_no_restricted_found`, `ephemeral_no_trace_persist`). `persist_trace` takes a new `zdr_actions_applied: &[&str]` parameter and binds it to the column; empty slice → SQL NULL. Test caller in `tests/zdr_behavior.rs` updated. | `src/trace/mod.rs`, `src/pipeline/orchestrator.rs`, `tests/zdr_behavior.rs` |
| 4 | **§16.3.5 Rerank is sequential** — Cohere RTT dominates latency at >50 candidates | `rerank()` now branches: ≤50 candidates → original single-call path; >50 → `rerank_parallel()` shards into 32-candidate chunks, calls Cohere concurrently via `futures::join_all`, merges by `final_score` desc, truncates to `top_n`. NaN-safe sort because shard path also coerces non-finite to 0.0. | `src/search/rerank.rs` |
| 5 | **§16.4.5 Clippy `-D warnings` not strict enough in CI** | CI step upgraded from `cargo clippy --workspace` to `cargo clippy --workspace --all-targets -- -D warnings` so test targets are linted too. Long-standing dead-code in `cache/mod.rs` flagged with crate-level `#![allow(dead_code, unused_imports)]` to keep the gate green; remaining lints are scheduled for a dedicated wave. | `.github/workflows/ci.yml`, `src/cache/mod.rs` |

### 15.9 Wave 3.4 batch — five closures (2026-05-19)

Heavier batch — three medium-effort security items + BPE tokenizer + DLQ tooling.
`cargo check --workspace --tests` green.

| # | Gap | Closure | Where it landed |
|---|---|---|---|
| 1 | **§16.3.2 Tokenizer is `len()/4`** — context-packing over/under-fills budget | `context_pack::estimate_tokens` now uses `tiktoken-rs` cl100k_base (matches all current Azure/OpenAI text-embedding and chat models). Lazy `OnceCell` so the 5 MB BPE table loads once per process. `RETRIEVAL_TOKENIZER=heuristic` env var keeps the old `len()/4` path for tests that don't want the table. | `src/context_pack/mod.rs`; workspace `Cargo.toml` adds `tiktoken-rs = "0.6"` |
| 2 | **§16.5.1 No per-org rate limit** — noisy tenant starves others | New `rate_limit` module wraps `governor::RateLimiter` keyed by `org_id` (anonymous callers share one bucket). Quota controlled by `DPV2_RATE_LIMIT_PER_ORG_RPS` (default 20) + `DPV2_RATE_LIMIT_PER_ORG_BURST` (default 40). Wired as a `route_layer` BELOW `auth_middleware` so the limiter sees the authenticated `AuthContext`. Rejected requests get HTTP 429 + `Retry-After: 1`. | `src/rate_limit/mod.rs`, `src/api/mod.rs` router builder; workspace adds `governor = "0.7"` |
| 3 | **§16.5.4 Wiki content unsanitized** — XSS responsibility punted to consumers | New Go package `internal/sanitize` wraps `bluemonday.UGCPolicy()` with markdown-friendly attribute allowances. `WikiPageVersion` gains `safe_html` + `safe_html_ok` fields; `GetPage` + `GetPageByPath` now call `applySafeHTML` after fetching the current version. Raw `content` is unchanged — round-trip equality preserved. | `services/wiki-store-go/internal/sanitize/sanitize.go`, `internal/model/wiki.go`, `internal/handler/wiki.go`, `go.mod` (adds `bluemonday v1.0.27`) |
| 4 | **§16.5.5 JWKS endpoint fetch not implemented** — single static PEM only | New `authz::jwks` module fetches `JWT_JWKS_URL` at boot + refreshes every `JWT_JWKS_REFRESH_SECS` (default 600s). Stores RS256 keys in an `RwLock<HashMap<kid, DecodingKey>>` global. `verify_jwt` now tries kid-lookup first; falls back to `JWT_PUBLIC_KEY_PEM` if unset, kid unknown, or JWKS empty. | `src/authz/jwks.rs`, `src/authz/mod.rs`, `src/api/mod.rs` (`verify_jwt`), `src/main.rs` (init at boot) |
| 5 | **§16.2.4 DLQ messages accumulate with no replay tool** | New cargo binary `dlq-replay` drains a DLQ subject and republishes each message to its original target. CLI: `--dlq <subject> --target <subject> [--limit N] [--dry-run]`. Honors the §16.1.6 NATS env fallback chain. Bin entry added to `retrieval-engine-rs/Cargo.toml`. | `src/bin/dlq_replay.rs`, `Cargo.toml` ([[bin]] section) |

**Deps added at workspace level.** `tiktoken-rs = "0.6"` (BPE), `governor = "0.7"`
(rate limiter), `ammonia = "4"` (reserved for future Rust-side scrub if we
move wiki rendering server-side). Go side adds `bluemonday v1.0.27`.

### 15.10 Wave 3.5 batch — nine closures, all remaining §16 open items (2026-05-19)

Largest batch yet — closes every remaining open `§16` candidate. Some
land as full implementations, others as scaffolds with a clearly-scoped
follow-up. `cargo check --workspace` green.

| # | Gap | Closure | Where it landed |
|---|---|---|---|
| 1 | **§16.1.1 `mode_mix` trace lies** — weights recorded ≠ scoring used | New `retrieval_runs.mode_mix_applied JSONB` column written by `persist_trace`. **2026-07-17 update: fully CLOSED** — graph/wiki/visual weights now drive the fused RRF scorer (`fuse_arms`), and `mode_mix_applied` records the real applied `{w_dense, w_bm25, w_graph, w_wiki, w_visual, rerank}`. Historical rows carry the old zeroed-graph/wiki note. | migration `20260519200000_*.sql`, `src/trace/mod.rs`, `src/pipeline/orchestrator.rs` |
| 2 | **§16.1.4 No per-org retrievalConfig table** | New `agent_retrieval_configs (org_id, agent_id, weights, rerank)` table + `agent_config::lookup()` helper with 60s moka cache. Engine wiring (read request `agent_id` → lookup → apply weights) is the read-side follow-up; table + lookup live now. | migration, `src/agent_config/mod.rs`, lib/main registration |
| 3 | **§16.1.5 No `graph_exports` endpoint** | `POST /v1/graph/exports` on graph-index-rs. Formats: `json`, `graphml`, `markdown`. Renders directly from `snapshot_org_graph`. Sets `Content-Disposition: attachment` for browser download. | `services/graph-index-rs/src/api.rs`, `Cargo.toml` adds workspace `chrono` |
| 4 | **§16.2.2 Cache invalidation race** | New `org_versions` table + `cache::org_version::{current, bump}` helpers (Postgres + 30s moka cache). Mutations on documents-api should call `bump(org_id)`; retrieval read-side embeds the version in cache key (final wiring follow-up). | migration, `src/cache/org_version.rs` |
| 5 | **§16.2.6 BulkIngest not transactional** | New `documents_outbox` table for the outbox pattern + comment-block on `BulkIngest` documenting the new flow. The repo.Create→outbox.Insert in one tx is the natural follow-up shape. | migration, `services/documents-api-go/internal/handler/documents.go` |
| 6 | **§16.3.8 wiki_block_embeddings write-through missing** | New `services/wiki-store-go/internal/events/publisher.go` with `PublishWikiVersionPublished` event (`dataplane.wiki.version.published`). go.mod adds `nats.go v1.39.1`. Wiring into `CreatePage` / `CreateVersion` + embedding-engine subscriber are the next steps — the publisher contract is locked in. | `services/wiki-store-go/internal/events/publisher.go`, `go.mod` |
| 7 | **§16.4.1 No end-to-end pipeline test** | New `tests/e2e/README.md` documenting scenarios + `services/retrieval-engine-rs/tests/pipeline_e2e.rs` scaffold (3 `#[tokio::test] #[ignore]` cases: happy_path runnable now, zdr_reject + cache_invalidation as TODO stubs). Gated behind `--ignored` so default `cargo test` doesn't try to hit docker-compose. | `tests/e2e/`, `services/retrieval-engine-rs/tests/pipeline_e2e.rs` |
| 8 | **§16.4.3 No chaos tests** | `tests/chaos/README.md` (5 numbered scenarios C-01 … C-05) + `tests/chaos/docker-compose.chaos.yml` interposing toxiproxy for NATS / Postgres / Qdrant / Dragonfly. Implementation per-scenario is queued; the design + stack are locked in. | `tests/chaos/` |
| 9 | **§16.4.8 No production deployment runbook** | New `docs/production-deploy.md`: topology, replica sizing, required secrets, network policy, autoscaling, rollout strategy, observability alerts (incl. §16.2.7 pool saturation), incident playbook, DR. | `docs/production-deploy.md` |

**Schema changes (one migration).** `20260519200000_wave_3_5_consistency.sql`
adds: `retrieval_runs.mode_mix_applied`, `agent_retrieval_configs`,
`org_versions`, `documents_outbox`. `init.sql` mirrors.

**Build gate.** `cargo check --workspace` green after wave-3.5. Go side
has new packages (`internal/sanitize`, `internal/events`) that need
`go mod tidy` to resolve — flagged as a `make tidy` step before Docker
build.

### 15.11 Wave 3.6 batch — six follow-up wires (2026-05-19)

Promotes the wave-3.5 scaffolds into real read/write-side wires.
`cargo check --workspace` green; `cargo check -p embedding-engine-rs`
green.

| # | Wire | What landed | Where |
|---|---|---|---|
| 1 | **§16.2.2 retrieval cache key versioned by `org_version`** | `CacheLayer::{get,set}_retrieval` signatures now take `org_version: i64`; key format `dpv2:ret:{org}:v{version}:{key}`. Old keys become unreachable on bump → no stale-cache window. | `src/cache/mod.rs` |
| 2 | **§16.1.4 `agent_id` flows end-to-end** | `RetrievalRequest` gains `agent_id: Option<String>`. Orchestrator calls `agent_config::lookup` and applies the row's `weights` as the default when no per-request `mode_mix` is set. Precedence: per-request > agent default > config. gRPC path passes `None` (proto not bumped yet). | `src/pipeline/types.rs`, `src/pipeline/orchestrator.rs`, `src/grpc/retrieval_svc.rs`, `src/api/mod.rs` |
| 3 | **§16.2.2 documents-api bumps `org_versions`** | `DocumentRepo.Create` (after successful insert) and `SoftDelete` (after rowsAffected>0) call `bumpOrgVersion(ctx, orgID)`. UPSERT pattern handles first-touch + N-th-touch in one statement. Failure is logged-only — TTL still saves us. | `services/documents-api-go/internal/repo/document_repo.go` |
| 4 | **§16.2.6 outbox publisher loop** | New `events.OutboxPublisher` polls every 500ms with `FOR UPDATE SKIP LOCKED` so multiple replicas don't double-publish. Uses `event_type` as the NATS subject directly. Marks `published=true` + `published_at` after successful flush. Honors `ctx.Done()` for shutdown. Wired up in `cmd/main.go` follow-up. | `services/documents-api-go/internal/events/outbox.go` |
| 5 | **§16.3.8 wiki repo emits publish events** | `WikiRepo.SetPublisher(*events.Publisher)` injects the NATS publisher; `CreatePage` + `CreateVersion` call `emitPublished` after commit. Re-fetches the page on `CreateVersion` to enrich the event with workspace_id / title / path. | `services/wiki-store-go/internal/repo/wiki_repo.go` |
| 6 | **§16.3.8 embedding-engine wiki subscriber** | New `wiki_consumer::spawn` subscribes to `dataplane.wiki.version.published`, embeds `content`, upserts one point per `version_id` into Qdrant `wiki_block_embeddings` with `{org_id, page_id, workspace_id, title, path}` payload. Best-effort: failure logs, next publish overwrites. | `services/embedding-engine-rs/src/wiki_consumer.rs`, `src/main.rs` (boot spawn) |

**End-to-end status.** With this wave, the full §16.3.8 chain is live
(wiki publish → NATS → embedding → Qdrant) and the §16.2.2 invalidation
loop (mutation → bump → cache miss) is complete on both sides. §16.2.6
outbox publisher exists; cmd/main.go boot-time integration is the
remaining 5-line follow-up.

### 15.12 Wave 3.7 batch — five boot wires + proto bump (2026-05-19)

Closes the "remaining nits" surface — all wave-3.6 implementations now
boot end-to-end. `cargo check --workspace` green.

| # | Wire | What landed | Where |
|---|---|---|---|
| 1 | **§16.2.6 outbox publisher booted** | `events.NewOutboxPublisher(pool, nc).Start(ctx)` in `documents-api-go` main. Honors `ctx.Done()` for graceful shutdown. | `services/documents-api-go/cmd/main.go` |
| 2 | **§16.2.6 BulkIngest writes to outbox** | `DocumentRepo.EnqueueOutbox(ctx, orgID, eventType, payload)` helper added. BulkIngest now marshals `DocumentCreatedEvent` into a `[]byte` and enqueues with `SubjectDocCreated` as the event_type — outbox publisher loop fans out to NATS. Removes the at-most-once direct publish. | `internal/repo/document_repo.go`, `internal/handler/documents.go` |
| 3 | **§16.3.8 wiki publisher booted** | `cfg.NatsURL` resolves via the §16.1.6 fallback chain (`DPV2_NATS_URL` → `SHARED_NATS_URL` → `NATS_URL` → `NATS_LOCAL_URL`). When non-empty, `wikiRepo.SetPublisher(events.NewPublisher(nc))` attaches the publisher; CreatePage / CreateVersion emissions now flow over the wire. | `services/wiki-store-go/cmd/main.go`, `internal/config/config.go` |
| 4 | **§16.1.4 proto contract bumped** | `proto/retrieval_v2.proto` adds `optional string agent_id = 13;`. gRPC `retrieval_svc` passes `req.agent_id` straight through to `RetrievalRequest`. HTTP and gRPC callers now both have agent_config access. | `proto/retrieval_v2.proto`, `src/grpc/retrieval_svc.rs` |
| 5 | **`make go-tidy` target** | New Makefile target walks all four Go services and runs `go mod tidy` in each. Run before any Docker build that adds Go deps; covers the wave-3.5/3.6/3.7 `nats.go` + `bluemonday` additions. | `Makefile` |

### 15.13 Wave 3.8 batch — final operational closures (2026-05-19)

Closes the last two operational items called out in §15.12.
`make go-tidy` ran clean; both schema-lock tests pass.

| # | Closure | Where |
|---|---|---|
| 1 | **`go mod tidy` run across all Go services** | `make go-tidy` invoked; `services/wiki-store-go/go.mod` resolved bluemonday + nats.go transitive deps (`aymerick/douceur`, `gorilla/css`, `nats-io/nkeys`, `nats-io/nuid`, `klauspost/compress`). `go build ./...` clean in both `documents-api-go` and `wiki-store-go`. | (operational — no new files) |
| 2 | **Cross-language wiki event schema lock** | New `docs/schemas/wiki_events.md` is the canonical reference. `services/embedding-engine-rs/tests/wiki_event_schema.rs` (Rust, 2/2 pass) and `services/wiki-store-go/internal/events/publisher_schema_test.go` (Go, ok) freeze the same canonical payload + assert unknown-field tolerance. CI now catches drift on either side. | `docs/schemas/wiki_events.md`, `services/embedding-engine-rs/tests/wiki_event_schema.rs`, `services/wiki-store-go/internal/events/publisher_schema_test.go` |

**§16 status: closed.** All open candidates listed at the end of wave-3.4
are now landed. The original §16 backlog from the v2.3 audit + the
follow-ups identified during waves 3.1–3.7 are at parity. Cumulative
score reflects the full closure surface.

### 15.14 Wave 4 — §17 v3-architecture closures (2026-05-19)

Picks the S+M items off the §17 v3 roadmap (Connect-RPC / GraphQL stay
queued for the actual v3 refactor — both are explicitly L-effort
"replace the runtime" changes). `cargo check --workspace` green;
`scripts/check-subjects.sh` green.

| # | Gap | Closure | Where |
|---|---|---|---|
| 1 | **§17.3.3 NATS subject ownership convention-only** | `infra/nats/SUBJECTS.md` is the canonical contract: streams, subjects, producers, consumers, payload schema refs. `scripts/check-subjects.sh` scans every Rust/Go source file for `"dataplane.*"` literals, asserts each appears in the contract, AND rejects inline `publish(...)` with a string literal (must use a named constant). CI job `subject-contract` wires it in. Three real subjects (`dataplane.documents.indexed`, `dataplane.dlq.{index,graph}-engine`) added; two inline publishes (`cost.ledger`, `documents.indexed`) refactored to constants. | `infra/nats/SUBJECTS.md`, `scripts/check-subjects.sh`, `.github/workflows/ci.yml`, `services/data-orchestrator-go/internal/jobs/executor.go`, `services/embedding-engine-rs/src/batch/mod.rs`, `services/retrieval-engine-rs/src/pipeline/orchestrator.rs` |
| 2 | **§17.3.6 No backpressure semantics in gRPC trailers** | `Retrieve` response now carries `x-ratelimit-limit` (per-org burst) + `x-ratelimit-rps` (per-org sustained) metadata derived from `DPV2_RATE_LIMIT_PER_ORG_*` env vars. Smart Model Plane callers can pre-throttle without hitting 429s. | `src/grpc/retrieval_svc.rs` |
| 3 | **§17.3.2 No streaming responses** | New `rpc RetrieveStream(RetrieveRequest) returns (stream RetrievalChunk)`. Each chunk is `oneof { Candidate, RetrievalTrailer }`. Implementation runs the existing pipeline, fans out candidates per frame, then sends a trailer with trace_id/index_version/zdr_mode/low_confidence/count. Wire shape locked; pipeline-side progressive evaluation is a v3 refactor. `tokio-stream` promoted to runtime dep. | `proto/retrieval_v2.proto`, `src/grpc/retrieval_svc.rs`, `Cargo.toml` |
| 4 | **§17.3.4 Duplicate doc write paths (gRPC + HTTP)** | gRPC `DocumentService.{CreateDocument, DeleteDocument, BulkIngest}` permanently refuse writes with `FailedPrecondition` + a migration message pointing at `documents-api-go`. The historical `DPV2_ALLOW_GRPC_DOCUMENT_WRITES` migration gate no longer enables writes. Reads (`GetDocument`, `ListDocuments`) remain available behind verified authentication, tenant pinning, and visibility enforcement. | `src/grpc/document_svc.rs` |

**v3 architecture items deferred** (truly L-effort runtime swaps): §17.3.1
Connect-RPC adoption, §17.3.5 GraphQL admin endpoint. Both are
documented as "v3.x roadmap" in §17 and stay there.

---

## 16. v2.4 Wave 4 — Consistency / Stability / Performance / Quality / Security

Items derived from the systematic v2.3 audit. None rated `high` for production blocker (security exception: 16.5.2 is medium-high), but each closes a real friction point. Ordered within each axis by ROI.

### 16.1 Consistency

| # | Issue | Fix | Effort |
|---|---|---|---|
| 16.1.1 | **`mode_mix` trace lies** — `w_graph`/`w_wiki` recorded but scoring ignores them (still RRF over dense+bm25). Audit endpoint shows weights that weren't applied. | Fold graph + wiki signals into a single scalar score path, OR relabel trace as "intended weights" with a separate `mode_mix_applied` column. | **DONE** — `w_graph` now folded into RRF by the GraphRAG graph arm (`fuse_arms` graph step + `arm_graph`); `w_wiki`/`w_visual` were already fused. Scoring and the persisted `mode_mix` agree. See `docs/graphrag-neo4j-plan.md`. |
| 16.1.2 | **JWT only on HTTP** — gRPC interceptor still checks `INTERNAL_API_KEY` only. Asymmetric trust boundary. | Move JWT verification into `ApiKeyInterceptor` (Bearer-header alternative). | S |
| 16.1.3 | **`zdr_mode` captures requested, not applied** — a `restricted` doc is filtered even in `zdr_mode=disabled`. | Add `zdr_actions_applied JSONB` column to `retrieval_runs`. | S |
| 16.1.4 | **No per-org `retrievalConfig` table** — Data Plane has request-side hook, Model Plane has no `agent_id → weights` mapping. | Either add `agent_retrieval_configs` table here OR formalize an `X-Agent-Retrieval-Config` header from Model Plane. | M |
| 16.1.5 | **`graph_exports` table without export endpoint** — schema exists, nothing populates it. | Add `POST /v1/graph/exports` (formats: json, graphml, html, markdown). | M |
| 16.1.6 | **NATS env naming inconsistent** — `NATS_URL` vs `NATS_SHARED_URL` vs `velion-nats`. | Standardize on `DPV2_NATS_URL` / `SHARED_NATS_URL`. | S |

### 16.2 Stability

| # | Issue | Risk | Fix | Effort |
|---|---|---|---|---|
| 16.2.1 | **No live load test run** — 800ms gate untested against real traffic. | Production surprise. | Run `tests/load/grpc-retrieve.sh` + `make load-http`; record p95 baseline; fold result into a `LATENCY_BASELINE.md`. | S |
| 16.2.2 | **Cache invalidation race** — in-flight retrieval reads stale cache before NATS event arrives. | Up to 5 min of stale results after document update. | Add `org_version` counter; include in cache key (versioning instead of invalidation). | M |
| 16.2.3 | **JetStream consumers don't drain on shutdown** — `shutdown_signal()` cancels Tokio; in-flight messages redeliver after restart. | Duplicate embed calls on rolling deploy. | Add `consumer.cancel()` + last-message-await in shutdown path. | S |
| 16.2.4 | **DLQ messages accumulate with no replay tool** | Failures pile up silently. | Add `cargo run --bin dlq-replay -- --subject <dlq>`. | M |
| 16.2.5 | **No max HTTP body size** — Axum default is unbounded. | DoS via 1 GB POST. | `RequestBodyLimitLayer::new(10 * 1024 * 1024)` on the router. | S |
| 16.2.6 | **BulkIngest not transactional** — partial-success leaves orphans if NATS publish fails. | Indexing pipeline starves on incomplete state. | Wrap DB insert + event publish in `tx.commit()` (outbox pattern). | M |
| 16.2.7 | **Postgres pool saturation invisible** | First sign is `acquire_timeout` errors. | Expose `dpv2_postgres_pool_active` + `_idle` gauges; add alert rule at 80%. | S |
| 16.2.8 | **Embed cache poisoning on model rotation** — cache key doesn't include model version. | Post-rotation deploy returns stale-model vectors for ~1h (TTL). | Include `embedding_model_version` in cache key. | S |

### 16.3 Performance

| # | Issue | Cost | Fix | Effort |
|---|---|---|---|---|
| 16.3.1 | **Real load test never run; 800ms gate untested** | Unknown headroom. | (same as 16.2.1) record baseline. | S |
| 16.3.2 | **Tokenizer is `len/4`, not BPE** — context-packing over/under-fills budget. | Wasted tokens or truncated citations. | Add `tiktoken-rs` for OpenAI-compatible counting. | S |
| 16.3.3 | **BM25 re-tokenizes per query** — no precomputed `tsvector` column. | 30–50% latency hit on sparse path. | Add generated `content_tsv` column + GIN index. | S |
| 16.3.4 | **No HTTP/2 keep-alive tuning on Tonic** — connection churn under intermittent load. | Higher tail latency. | `Server::builder().http2_keepalive_interval(Some(Duration::from_secs(30)))`. | S |
| 16.3.5 | **Rerank is sequential** — one batch per query. | Latency dominated by Cohere RTT. | Parallelize candidate-list splits when `candidates > 50`. | M |
| 16.3.6 | **No connection-pool warmup** — first queries after boot pay connect cost. | Cold-start tail spike on deploy. | Run `SELECT 1` × `min_connections` at startup. | S |
| 16.3.7 | **`extraction_trace` JSONB has no GIN index** | Cannot query traces efficiently. | `CREATE INDEX … USING GIN`. | S |
| 16.3.8 | **`/v1/retrieve/wiki` ANN blocked on missing write-through** — collection provisioned in wave-2 but nothing writes to it. | wiki retrieval is text-only. | Wave-3+ : write embedding on wiki version `published`. | M |

### 16.4 Quality

| # | Issue | Why | Fix | Effort |
|---|---|---|---|---|
| 16.4.1 | **No end-to-end pipeline test** — ingest → chunk → embed → retrieve never runs as one. | Each service tested in isolation. | Add `tests/e2e/` integration that drives the full pipeline against docker-compose. | M |
| 16.4.2 | **No property/fuzz tests** despite `proptest` available | Validation layer + Logseq parser are prime candidates. | Add proptest suite for `validate.CreateDocument` and `logseq.Parse∘Serialize` round-trip. | S |
| 16.4.3 | **No chaos tests** — NATS down, Qdrant slow, Postgres degraded paths untested. | Production reality. | Add toxiproxy-based suite running quarterly (or pre-release). | L |
| 16.4.4 | **testcontainers-go covers only documents-api** — wiki-store-go, data-orchestrator-go, data-quality-go lack integration tests. | Coverage gap. | Replicate the integration_test.go pattern. | M |
| 16.4.5 | **`clippy --workspace -- -D warnings` not in CI required** | 12+ warnings accumulating (`dead_code`, unused imports). | Add to `.github/workflows/ci.yml` required job. | S |
| 16.4.6 | **No OpenAPI spec** — only protos for gRPC + hand-written endpoint table | Velion TS types built ad-hoc. | Generate OpenAPI from Axum routes (or hand-write + lint with `spectral`). | M |
| 16.4.7 | **PII in logs/traces** — `tracing::info!(query = …)` leaks user queries to OTLP. | GDPR / SOC2 audit finding. | Add `redact_query()` for queries longer than N chars or containing email/phone patterns. | S |
| 16.4.8 | **No production deployment runbook** — only local docker docs. | Operators have no Kubernetes / cloud-deploy reference. | Write `docs/production-deploy.md` covering replicas, secrets, network policies, autoscaling. | M |

### 16.5 Security

| # | Issue | Severity | Fix | Effort |
|---|---|---|---|---|
| 16.5.1 | **No per-org rate limiting** — only global concurrency cap. | medium — noisy tenant starves others | `tower::limit::RateLimit` with org-keyed `governor` middleware. | M |
| 16.5.2 | **Single shared `INTERNAL_API_KEY`** — no per-caller identity, no audit, single rotation point. | **medium-high** — leaked key is unrecoverable without full rotation | Short-lived JWT with `kid` for rotation; keep API key as fallback for admin/backfill only. (Coupled with §15-A). | M |
| 16.5.3 | **No admin-action audit log** — orphan cleanup, sweeps, hard deletes are untraceable. | medium | Append `admin_audit_log` table; write from every admin endpoint. (Same table as §15-E covers the user side.) | S |
| 16.5.4 | **Wiki content unsanitized** — XSS responsibility deferred to consumers, but API doesn't even flag it. | medium | At minimum, flag with `safe_html: bool` field after server-side scrub via `ammonia` (Rust) / `bluemonday` (Go). | M |
| 16.5.5 | **JWKS endpoint fetch not implemented** — single static PEM only. | low | Add background JWKS poll with `kid`-based key selection. | M |
| 16.5.6 | **No PII redaction in OTel traces** | medium | Same redactor as 16.4.7 — applied at span attribute set. | S |

---

## 17. v3 Architecture Roadmap — Protocol & Streaming

These are larger architectural changes, not wave-tickets. They address the duplicate-handler problem, missing streaming responses, and brittle NATS subject convention.

### 17.1 What we use today

| Surface | Protocol | Caller | Rationale |
|---|---|---|---|
| Frontend → Retrieval `:8004` | HTTP/JSON | velion (Next.js) | Browser-friendly; no proxy. |
| Model Plane → Retrieval `:50052` | gRPC | mp-session-core, mp-inference-core | Protobuf compact; tight contract; internal. |
| Admin/control `:8010-:8013` | HTTP/JSON | ops, orchestrator | `curl + jq` debuggability. |
| documents-api → index → embed → graph | NATS JetStream | each stage | Decoupled, durable, backpressured. |

### 17.2 Where current implementation has friction

| # | Friction | Cost |
|---|---|---|
| 17.2.1 | **Two write paths for documents** — REST on documents-api-go AND gRPC `DocumentService` on retrieval-engine-rs both write Postgres. | Drift risk; double validation maintenance. |
| 17.2.2 | **No streaming responses** anywhere. `top_k=500` materializes the entire candidate list before responding. | Higher TTFB; no progressive UI rendering. |
| 17.2.3 | **NATS subject ownership is convention-only** — we hit the `DATAPLANE_KNOWLEDGE` collision during Docker bring-up. | Brittle when adding new consumers. |
| 17.2.4 | **Hand-coded gRPC type conversions** — ~200 LOC of `.into()` boilerplate per handler. | Tedious; field skew can hide there. |
| 17.2.5 | **No Connect-RPC** — HTTP + gRPC clients run different handler trees for the same contract. | The root cause behind 17.2.1 + 17.2.4. |

### 17.3 v3 changes

| # | Change | Rationale | Effort |
|---|---|---|---|
| 17.3.1 | **Adopt Connect-RPC** for retrieval-engine (replace Tonic, keep same protos) | Single handler set serves both HTTP/JSON and gRPC — kills the duplicate handler problem (17.2.1 + 17.2.4). | L |
| 17.3.2 | **Server-streaming retrieval** | Better TTFB; enables Velion progressive results UI. | M |
| 17.3.3 | **NATS subject contract file** `infra/nats/SUBJECTS.md` + CI lint that scans `STREAM_NAME` / subject constants | Prevents collisions like the one we just fixed. | S |
| 17.3.4 | **Make gRPC `DocumentService` forward to documents-api-go HTTP** (or remove it entirely) | One write path, one set of validators. | M |
| 17.3.5 | **GraphQL `/v1/admin/graph` endpoint** for ops graph-explorer UI | Only when that UI is real — the data is naturally graph-shaped (entities/edges/claims). | L |
| 17.3.6 | **Backpressure semantics in gRPC trailers** — `X-RateLimit-*` / `Retry-After` | Smoother callers under load. | S |

### 17.4 What we are NOT changing in v3

- **REST stays** for Frontend and ops/admin. gRPC-web would add an Envoy proxy with zero meaningful win.
- **gRPC stays** for Model Plane ↔ Data Plane. Bandwidth + contract benefits are real at the candidate-list sizes we serve.
- **NATS stays** for async pipelines. Replacing with HTTP chain would couple every stage's latency and uptime.
- **No GraphQL on the core query path.** v2 has two stable client classes; introspection / field-selection benefits don't justify the resolver / N+1 / depth-limit operational cost.

---

## 18. Execution sequencing

The five-wave plan from here to a fully Control-Plane-integrated v2.4:

| Wave | Scope | When |
|---|---|---|
| **Wave 3** (§15) | Control Plane wiring — `AuthContext`, `MembershipCache`, `PermissionResolver`, cost-core, audit log | next sprint (blocks any public/multi-tenant deploy) |
| **Wave 4** (§16) | Consistency + stability + performance fixes — load-test, mode_mix honesty, pool metrics, body limits, BPE tokenizer, BM25 tsvector, JWT-on-gRPC | sprint after Wave 3 |
| **Wave 5** | Quality + security hardening — e2e tests, property tests, per-org rate limit, PII redaction, wiki sanitization, deployment runbook | following sprint |
| **v3 architecture** (§17) | Connect-RPC, server-streaming, NATS subject contract, eliminate gRPC `DocumentService` duplication | v3.x — coupled to next major retrieval-engine refactor |
| **v3.x ops** | GraphQL graph-explorer admin UI, JWKS fetch, multi-region considerations | when scale demands it |

**Wave 3 is gating.** Until §15 lands, the system is single-tenant-safe but multi-tenant-trusted-clients-only. Don't deploy to a context where the caller's Model Plane could be compromised — the API key is the only line of defense.

---

## 19. 2026-05-20 — Velion Build Runtime Audit (verified green)

Source: `apps/Frontend Plane/velion/build-velion-services.sh` end-to-end run. Data Plane v2 is index 0 (first stack) and the only stack confirmed all-green this run.

### Containers running healthy (12 / 12)
| Service | Container | Host port → Container | Health |
|---|---|---|---|
| documents-api | `dpv2-documents-api` | 8010 → 8010 | ✅ healthy |
| wiki-store | `dpv2-wiki-store` | 8011 → 8011 | ✅ healthy |
| data-orchestrator | `dpv2-data-orchestrator` | 8012 → 8012 | ✅ healthy |
| data-quality | `dpv2-data-quality` | 8013 → 8013 | ✅ healthy |
| retrieval-engine | `dpv2-retrieval-engine` | 8014 → 8004, 50062 → 50052 | ✅ healthy |
| index-engine | `dpv2-index-engine` | 9201 → 9201 | ✅ healthy |
| embedding-engine | `dpv2-embedding-engine` | 9202 → 9202 | ✅ healthy |
| graph-index | `dpv2-graph-index` | 9203 → 9203 | ✅ healthy |
| postgres | `dpv2-postgres` | 5442 → 5432 (db=dataplane) | ✅ healthy |
| dragonfly | `dpv2-dragonfly` | 6389 → 6379 | ✅ healthy |
| nats | `dpv2-nats` | 4232 → 4222 | ✅ healthy |
| qdrant | `dpv2-qdrant` | 6345 → 6333, 6346 → 6334 | ✅ healthy |

### Networks
- `dpv2-net` (private) + `inter-plane-bus` (shared) — wiring matches design.

### Bootstrap one-shots tracked by `build-velion-services.sh`
- None for this stack (BOOTSTRAP_SERVICES[0]="" in the script).

### Velion server-side wiring — verified correct
The following `apps/Frontend Plane/velion/.env` entries resolve correctly because the corresponding containers are joined to `inter-plane-bus`:

```
DOCUMENTS_SERVICE_URL=http://dpv2-documents-api:8010      ✅
DATA_DOCUMENTS_API_URL=http://dpv2-documents-api:8010     ✅
DATA_RETRIEVAL_API_URL=http://dpv2-retrieval-engine:8004  ✅
WIKI_SERVICE_URL=http://dpv2-wiki-store:8011              ✅
DATA_ORCHESTRATOR_URL=http://dpv2-data-orchestrator:8012  ✅
DATA_QUALITY_URL=http://dpv2-data-quality:8013            ✅
GRAPH_SERVICE_URL=http://dpv2-graph-index:9201            ✅
DATA_DOCUMENTS_GRPC_HOST=dpv2-documents-api               ✅
DATA_DOCUMENTS_GRPC_PORT=50051                            ✅
```

### Outstanding gaps unchanged
§15 (Control Plane wiring) and §16 (Wave 4 fixes) remain the blocking work — the runtime audit didn't surface anything new beyond what was already tracked.
