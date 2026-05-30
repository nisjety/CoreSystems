# Data Plane D4 + D5 — Graph + Wiki + Hybrid retrieval — Build specification

> **Owner reference**: `apps/master-ownership-matrix.md` §0 — Data Plane owns documents, chunks, embeddings, indexes, graph, wiki, retrieval. No other plane touches Postgres knowledge tables, Qdrant, or the graph store directly.
>
> **Source contract**: `apps/Data Plane/docs/gap-data.md` §5.2 (Graphify additions), §5.3 (LLM Wiki additions), §5.4 (Hybrid retrieval), §6 (Agentic Context Engine), §8 Phase D4 + D5, §9 (API surface).
>
> **Generated**: 2026-05-17. Tracks Wave 11.C of the velion knowledge-base overhaul.

This document is the **building blueprint** for two paired Data Plane waves. App Shell (velion) consumes these via the HTTP/gRPC contracts in §3 below — no mocks, the velion side starts wiring against these contracts as soon as the OpenAPI/proto handshake is published from Phase D4-1.

---

## 1 · Service inventory

| Service | Language | Owner of | Status |
|---|---|---|---|
| `graph-index-rs` | Rust (axum + sqlx + tree-sitter) | Knowledge graph: nodes, edges, communities, rebuilds, graph retrieval | **NEW** in D4 |
| `wiki-store-go` | Go (chi + pgx + Logseq-format storage layer) | Wiki pages, versions, source-log, backlinks, maintenance-log, contradictions | **NEW** in D4 |
| `retrieval-engine-rs` | Rust (existing, extend) | Add hybrid (BM25 + dense + graph + wiki blend), `/v1/retrieve/hybrid`, `/v1/knowledge/search` | EXTEND in D5 |
| `data-orchestrator-go` | Go (existing, extend) | Graph rebuild jobs, wiki maintenance jobs, contradiction sweeps | EXTEND in D4 + D5 |
| `index-engine-rs` | Rust (existing, extend) | Entity-extraction hooks during chunking (feeds the graph) | EXTEND in D4 |

All five live under `apps/Data Plane/services/` (mirror of the existing layout). No Python in the request path — only `retrieval-eval-py` for offline regression suites.

---

## 2 · Storage layout (canonical)

### 2.1 PostgreSQL 16+ schema additions (single `data_plane` database)

#### Graph (D4)

```sql
CREATE TABLE knowledge_nodes (
  id              UUID PRIMARY KEY,
  org_id          TEXT NOT NULL,
  workspace_id    TEXT,
  kind            TEXT NOT NULL,                 -- person | org | product | concept | doc_section | code_symbol
  label           TEXT NOT NULL,
  canonical_uri   TEXT,                          -- e.g. doc://… , wiki://… , code://…
  attributes      JSONB NOT NULL DEFAULT '{}',
  content_hash    BYTEA NOT NULL,                -- BLAKE3 of canonical attribute payload
  source_refs     UUID[] NOT NULL DEFAULT '{}',  -- chunk/artifact IDs
  community_id    UUID,                          -- Leiden/Louvain partition
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON knowledge_nodes (org_id, kind);
CREATE INDEX ON knowledge_nodes USING GIN (attributes);

CREATE TABLE knowledge_edges (
  id              UUID PRIMARY KEY,
  org_id          TEXT NOT NULL,
  src_node_id     UUID NOT NULL REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
  dst_node_id     UUID NOT NULL REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
  relation        TEXT NOT NULL,                 -- mentions | implements | reports_to | depends_on | …
  provenance      TEXT NOT NULL CHECK (provenance IN ('extracted','inferred','ambiguous')),
  confidence      REAL NOT NULL DEFAULT 1.0,
  weight          REAL NOT NULL DEFAULT 1.0,
  attributes      JSONB NOT NULL DEFAULT '{}',
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON knowledge_edges (org_id, src_node_id);
CREATE INDEX ON knowledge_edges (org_id, dst_node_id);

CREATE TABLE edge_source_refs (
  edge_id         UUID NOT NULL REFERENCES knowledge_edges(id) ON DELETE CASCADE,
  source_kind     TEXT NOT NULL,                 -- chunk | artifact | wiki_version
  source_id       UUID NOT NULL,
  span            JSONB,                         -- {start, end} for chunk-level provenance
  PRIMARY KEY (edge_id, source_kind, source_id)
);

CREATE TABLE graph_rebuild_jobs (
  id              UUID PRIMARY KEY,
  org_id          TEXT NOT NULL,
  scope           JSONB NOT NULL,                -- {documentIds?: [...], full?: true}
  status          TEXT NOT NULL,                 -- queued | running | succeeded | failed
  started_at      TIMESTAMPTZ,
  finished_at     TIMESTAMPTZ,
  stats           JSONB,                         -- {nodes_added, edges_added, ...}
  error           TEXT
);

CREATE TABLE graph_exports (
  id              UUID PRIMARY KEY,
  org_id          TEXT NOT NULL,
  format          TEXT NOT NULL,                 -- json | html | markdown | graphml
  uri             TEXT NOT NULL,                 -- object-store URI
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

#### Wiki (D4)

```sql
CREATE TABLE wiki_pages (
  id              UUID PRIMARY KEY,
  org_id          TEXT NOT NULL,
  workspace_id    TEXT NOT NULL,
  path            TEXT NOT NULL,                 -- "/products/foo", Logseq-style
  title           TEXT NOT NULL,
  current_version BIGINT NOT NULL,
  status          TEXT NOT NULL,                 -- draft | published | deprecated
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  UNIQUE (org_id, workspace_id, path)
);

CREATE TABLE wiki_page_versions (
  page_id         UUID NOT NULL REFERENCES wiki_pages(id) ON DELETE CASCADE,
  version         BIGINT NOT NULL,
  content_md      TEXT NOT NULL,                 -- Logseq-format block-outline markdown
  blocks          JSONB NOT NULL,                -- normalized outline tree for fast block ops
  editor_kind     TEXT NOT NULL,                 -- user | agent | system
  editor_id       TEXT,
  source_refs     JSONB NOT NULL DEFAULT '[]',   -- chunks/artifacts/edges that produced this
  reason          TEXT,                          -- "regenerate from sources" | "manual edit" | …
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
  PRIMARY KEY (page_id, version)
);

CREATE TABLE wiki_source_log (
  id              UUID PRIMARY KEY,
  page_id         UUID NOT NULL REFERENCES wiki_pages(id) ON DELETE CASCADE,
  version         BIGINT NOT NULL,
  source_kind     TEXT NOT NULL,                 -- chunk | artifact | url | edge
  source_id       TEXT NOT NULL,
  contribution    REAL,                          -- 0..1 — how much this source shaped the page
  created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE wiki_backlinks (
  src_page_id     UUID NOT NULL REFERENCES wiki_pages(id) ON DELETE CASCADE,
  dst_page_id     UUID NOT NULL REFERENCES wiki_pages(id) ON DELETE CASCADE,
  anchor          TEXT,
  PRIMARY KEY (src_page_id, dst_page_id, anchor)
);

CREATE TABLE wiki_maintenance_log (
  id              UUID PRIMARY KEY,
  page_id         UUID REFERENCES wiki_pages(id) ON DELETE CASCADE,
  kind            TEXT NOT NULL,                 -- contradiction | stale | orphan | accepted_edit | rejected_edit
  detected_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
  resolved_at     TIMESTAMPTZ,
  details         JSONB NOT NULL
);
```

### 2.2 Object store (D4 — only when needed)

- `graph_exports`: rebuild artifacts > 1 MB → stored as object-store blobs; `uri` column points at them.
- `wiki_page_versions.blocks`: kept in Postgres unless a single version exceeds 256 KB, then offloaded.

### 2.3 Qdrant (D5 only — touched via `retrieval-engine-rs`)

- Existing `chunks` collection unchanged.
- Add `wiki_block_embeddings` collection with payload: `{page_id, version, block_path, org_id}`. Used by `/v1/retrieve/wiki` for semantic recall within a page.
- Add `entity_summary_embeddings` collection: per-node summary embedding for `graph` retrieval re-ranking. Payload: `{node_id, org_id, kind, community_id}`.

---

## 3 · Public HTTP contracts (consumed by App Shell)

All endpoints require `X-Org-ID` header + JWT (auth-core JWKS). Errors follow RFC-7807 problem+json.

### 3.1 Graph build / inspect — `graph-index-rs` + `data-orchestrator-go`

```http
POST /v1/graphs/build               # data-orchestrator-go
Body: { "scope": { "documentIds": ["…"] } | { "full": true } }
→ 202 { "jobId": "uuid", "statusHref": "/v1/graphs/jobs/uuid" }

GET  /v1/graphs/jobs/{id}            # data-orchestrator-go
→ 200 { "id","status","stats":{ "nodesAdded":42,"edgesAdded":118 } }

GET  /v1/graphs/{org_id}             # graph-index-rs
?kind=person,org&community=&limit=500
→ 200 {
  "nodes": [{ "id","kind","label","attributes","community":"uuid","sourceRefs":[…] }],
  "edges": [{ "id","src","dst","relation","provenance","confidence","weight" }]
}

GET  /v1/graphs/{org_id}/node/{node_id}
→ 200 { "node": {…}, "incoming": [{edge}], "outgoing": [{edge}], "summary": "…" }

POST /v1/retrieve/graph              # graph-index-rs
Body: { "orgId":"…", "query":"…", "anchorNodeIds":[…]?, "hops":2, "k":20, "kinds":[…]? }
→ 200 {
  "traceId": "uuid",
  "subgraph": { "nodes":[…], "edges":[…] },
  "communitySummaries": ["…"],
  "rankedNodeIds": ["uuid", "uuid", …]
}
```

### 3.2 Wiki — `wiki-store-go`

```http
GET  /v1/wiki/pages?orgId=&workspaceId=&pathPrefix=&limit=
→ 200 { "pages": [{ "id","path","title","currentVersion","status","updatedAt" }] }

POST /v1/wiki/pages
Body: { "orgId","workspaceId","path","title","contentMd","blocks","editor":{ "kind","id" },"sourceRefs":[…],"reason":"…" }
→ 201 { "id","version" }

GET  /v1/wiki/pages/{id}?version=
→ 200 { "id","path","title","status","currentVersion","content":{ "markdown","blocks" },"sourceLog":[…] }

GET  /v1/wiki/backlinks/{id}
→ 200 { "backlinks": [{ "pageId","title","path","anchor" }] }

POST /v1/retrieve/wiki
Body: { "orgId","query","k":10,"includeSections":true }
→ 200 { "traceId","pages":[{ "id","title","matchedBlocks":[…],"score" }] }

POST /v1/retrieve/contradictions     # wiki-store-go + graph-index-rs
Body: { "orgId","topic":"…","k":5 }
→ 200 { "conflicts": [{ "claimA":{…source},"claimB":{…source},"confidence" }] }
```

### 3.3 Hybrid (the headline of D5) — `retrieval-engine-rs`

```http
POST /v1/retrieve/hybrid
Body: {
  "orgId":"…",
  "agentId":"…",
  "query":"…",
  "k":10,
  "filters":{ "documentIds":[…]?, "wikiPaths":[…]?, "graphCommunityIds":[…]? },
  "weights":{ "dense":0.5, "bm25":0.2, "graph":0.2, "wiki":0.1 },
  "rerank":true
}
→ 200 {
  "traceId":"uuid",
  "items":[{
    "kind":"chunk"|"wiki"|"graph_node",
    "id":"…",
    "score":0.87,
    "title":"…",
    "snippet":"…",
    "sourceRefs":[{ "kind":"document","id":"…","span":{…} }]
  }],
  "tokenBudgetUsed":1820,
  "modeMix":{ "dense":6,"bm25":2,"graph":1,"wiki":1 }
}

GET  /v1/knowledge/search?orgId=&q=&kinds=docs,wiki,nodes&limit=
→ same envelope as /v1/retrieve/hybrid — used by App Shell global search bar
```

### 3.4 Internal — orchestrator-driven jobs

```http
POST /v1/wiki/maintenance/sweep            # data-orchestrator-go → wiki-store-go
Body: { "orgId","kinds":["contradiction","stale","orphan"] }
→ 202 { "jobId" }
```

All endpoints must emit OpenTelemetry traces with `org_id`, `query_id`, `trace_id`, `mode_mix` attributes.

---

## 4 · gRPC contracts (intra-Data-Plane only)

Defined in `apps/Data Plane/proto/d4d5.proto` (NEW). Used by `data-orchestrator-go` ↔ `graph-index-rs`, and by `retrieval-engine-rs` ↔ both. App Shell never speaks gRPC.

```proto
service GraphIndex {
  rpc UpsertNodes(UpsertNodesRequest) returns (UpsertResponse);
  rpc UpsertEdges(UpsertEdgesRequest) returns (UpsertResponse);
  rpc RetrieveSubgraph(SubgraphRequest) returns (SubgraphResponse);
  rpc RebuildCommunities(RebuildRequest) returns (RebuildResponse);
}
service WikiStore {
  rpc GetPageInternal(GetPageRequest) returns (Page);
  rpc EmitMaintenanceEvent(MaintenanceEvent) returns (Ack);
}
```

---

## 5 · Internal data flow

```
Quarry v2 artifact ──┐
                     ▼
            documents-api-go            ◄─ existing (D3)
                     │  emits   doc.ingested  via NATS
                     ▼
            index-engine-rs   ─── chunks ───►  retrieval-engine-rs
                     │
                     │ extracts entities (tree-sitter + LLM-light)
                     ▼
            graph-index-rs   ◄────  edge proposals (provenance=extracted)
                     │
                     │ post-build job (Leiden) → communities
                     ▼
            data-orchestrator-go
                     │
                     │ schedules wiki sync (LLM-light summarizes communities → wiki proposal)
                     ▼
            wiki-store-go    (page version v_n+1 created, status=draft)
                     │
                     │ App Shell operator reviews → accept / reject
                     ▼
            wiki_page_versions.status='published'
```

**Critical rule:** Quarry never writes to graph or wiki. It produces artifacts; data-orchestrator-go is the only writer.

---

## 6 · Entity & edge extraction strategy (D4)

Inside `index-engine-rs`:

| Source type | Extractor | Confidence |
|---|---|---|
| Code (`.rs`, `.go`, `.ts`, `.py`, `.java`, …) | `tree-sitter-<lang>` AST walk → declarations + import edges | 0.95 |
| Markdown / structured docs | `tree-sitter-markdown` headings + link parser | 0.85 |
| Free-text chunks | LLM-light pass (model-gateway `/v1/invoke` with structured output) | 0.6 (`inferred`) |
| Tables (CSV/HTML) | Header-as-attribute extractor | 0.9 |

LLM-extracted edges land with `provenance='inferred'`. A nightly job in `data-orchestrator-go` promotes edges to `extracted` once they're observed from ≥ 2 independent sources (`edge_source_refs` count).

Community detection uses **Leiden** (via `rustworkx`-equivalent in Rust: `petgraph` + custom Leiden — there's no first-party crate, so vendor a small implementation; reference: Leiden algorithm paper, Traag 2019). Communities are recomputed when ≥ 5 % of edges change.

---

## 7 · Hybrid retrieval blending (D5)

Inside `retrieval-engine-rs::hybrid`:

```
score(item) = w_dense * dense_score
           + w_bm25  * bm25_score
           + w_graph * graph_score
           + w_wiki  * wiki_score
           + rerank_bonus    # only if rerank=true; uses bge-reranker-v2-m3 via embedding-engine-rs sidecar
```

- **dense**: existing Qdrant ANN.
- **bm25**: pg_trgm + `tsvector` over chunk text (cheap; runs in Postgres).
- **graph**: anchor query into graph (entities mentioned in query), expand `hops≤2`, score by edge weight × confidence × hops-decay.
- **wiki**: ANN over `wiki_block_embeddings` + backlink-density boost.
- **rerank**: cross-encoder over top-50 items, narrow to k.

Defaults (overridable per-agent via `retrievalConfig` stored in agents table — see D5-3): `dense=0.5 bm25=0.2 graph=0.2 wiki=0.1`.

Every call emits a `retrieval_trace` row with mode mix + trace ID. Reusable by `/v1/retrieval/{trace_id}` audit.

---

## 8 · Per-agent retrieval config (D5)

Two new columns on the existing `agents` table (Convex side — not Data Plane, but contract-shared):

```ts
retrievalConfig: v.optional(v.object({
  weights: v.object({ dense: v.number(), bm25: v.number(), graph: v.number(), wiki: v.number() }),
  chunkSize: v.number(),     // overrides at retrieval time only (re-chunking is a separate orchestrator job)
  topK: v.number(),
  rerank: v.boolean(),
  graphHops: v.number(),
  filters: v.optional(v.object({ documentIds: v.array(v.string()), wikiPaths: v.array(v.string()) }))
})),
```

App Shell exposes ElevenLabs-style `Configure RAG` per agent (Wave 11.B Phase 6.5).

---

## 9 · Build order (read these phases in sequence)

| Phase | Task | Owner | Days | Done-when |
|---|---|---|---|---|
| D4-1 | Contract freeze: write `proto/d4d5.proto` + OpenAPI for all §3 endpoints; publish to `apps/Data Plane/proto/` and `apps/Frontend Plane/velion/src/types/data-plane/` (generated TS) | this spec | 0.5 | velion TS types compile against the OpenAPI |
| D4-2 | Postgres migrations for §2.1 tables; rollback scripts | `data-orchestrator-go` | 0.5 | `make migrate` green on a fresh DB |
| D4-3 | `wiki-store-go` skeleton: routes from §3.2, pgx repo, OTel + JWT middleware | new service | 2 | All §3.2 endpoints respond 200/201 against an empty DB |
| D4-4 | `graph-index-rs` skeleton: routes from §3.1, sqlx repo, OTel + JWT middleware | new service | 2 | `GET /v1/graphs/{org_id}` returns `{nodes:[], edges:[]}` 200 |
| D4-5 | Entity-extraction hook in `index-engine-rs`: tree-sitter for code + markdown; emit edge proposals via gRPC to graph-index-rs | extend | 2 | New docs land → edges appear |
| D4-6 | Wiki Logseq-format read/write: block-outline parser/serializer + version chaining + backlink reverse-index trigger | wiki-store-go | 1.5 | Create→update→backlinks round-trip works; `/v1/wiki/backlinks/{id}` returns links |
| D4-7 | Graph rebuild job in `data-orchestrator-go`: schedule, status, Leiden community pass | extend | 1 | `POST /v1/graphs/build` → completes, communities populated |
| D5-1 | `wiki_block_embeddings` Qdrant collection + write-through from wiki-store-go on `published` | extend | 0.5 | Inserts visible in Qdrant |
| D5-2 | `entity_summary_embeddings` Qdrant collection + nightly summarizer job | extend | 0.5 | Embeddings flow on new community |
| D5-3 | `/v1/retrieve/graph` (graph-index-rs): query-entity-extract → expand → score | new | 1 | Returns ranked subgraph for a real query |
| D5-4 | `/v1/retrieve/wiki` (wiki-store-go): ANN + backlink density | new | 0.5 | Returns ranked page hits |
| D5-5 | `/v1/retrieve/hybrid` (retrieval-engine-rs): blend per §7 | extend | 1.5 | Trace ID stored; mode_mix returned; cross-encoder rerank wired |
| D5-6 | `/v1/retrieve/contradictions` (wiki+graph): conflict detector | new | 1 | Two contradictory pages → one conflict row |
| D5-7 | `/v1/knowledge/search` envelope wrapper + global search | extend | 0.5 | Velion global search bar binds |
| D5-8 | Retrieval trace audit endpoint + Grafana dashboards | extend | 0.5 | Mode-mix histogram visible |
| D5-9 | Eval harness in `retrieval-eval-py`: nDCG / Recall@k / MRR on a 100-question golden set | new | 1 | First eval run completes |
| D5-10 | Release gates per gap-data.md §11: latency p95 < 800ms hybrid; zero plane-mixing in audit | check | 0.5 | Pre-flight passes |

**Total D4 + D5 ≈ 17 focused days.** Parallelize: D4-3 / D4-4 / D4-5 are independent; D5-3 / D5-4 are independent.

---

## 10 · Hard constraints

1. **No mocks.** This is a parallel real-build track; the velion UI binds against the OpenAPI from D4-1 day one. If a section's data isn't ready yet, the endpoint returns honest `200 { items: [] }` — not synthetic fixtures.
2. **No plane crossings.** Velion never imports a Postgres client. Quarry never writes to graph/wiki. Model Plane never touches Qdrant or tables. Audit by tagging every table CREATE with `OWNER=data-plane`; reject merges that violate.
3. **Org-scoped everywhere.** Every query is `WHERE org_id = $1`. Postgres RLS optional but recommended (turn on after D4-7).
4. **OpenTelemetry mandatory.** Every endpoint emits a span with `org_id`, `agent_id?`, `trace_id`. Service-to-service propagation via `traceparent`.
5. **JWT mandatory.** auth-core JWKS, RS256. No bypass except in `make dev-loopback` profile.
6. **Idempotent writes.** Re-running `POST /v1/wiki/pages` with same `(orgId, workspaceId, path, version-number)` is a no-op, not a duplicate.

---

## 11 · Cross-references

- App Shell consumer: `apps/Frontend Plane/velion/docs/prompts/wave11-knowledge.md` (sections 6 + 11.C).
- Master rules: `apps/master-ownership-matrix.md`.
- LLM-Wiki product spec: `apps/Ingestion Plane/Quarry-v2/docs/LLM-Wiki.md`.
- Existing retrieval contract: `apps/Data Plane/services/retrieval/app/main.py` (legacy) ⇄ `apps/Data Plane/services/retrieval/proto/retrieval.proto`.
