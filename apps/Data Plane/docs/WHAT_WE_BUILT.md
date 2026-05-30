# Data Plane — What We Built

> Last updated: 23 February 2026

---

## Overview

The Data Plane is the dedicated **knowledge storage, chunking, embedding, and retrieval** layer for the CoreSystem.  
It is entirely independent of the Reasoning Plane and is consumed by it over REST and gRPC.

No other plane touches Qdrant, no other plane chunks documents, and no other plane calls the embedding API directly.

---

## Services

### 1. `documents-service` — REST :8001 · gRPC :50051

The public intake and management API for documents.

| Route | Method | Purpose |
|---|---|---|
| `/v1/documents` | POST | Ingest a single document |
| `/v1/documents/bulk` | POST | Ingest up to 100 documents in one request |
| `/v1/documents` | GET | List documents for an org |
| `/v1/documents/{id}` | GET | Get document with full content |
| `/v1/documents/{id}/chunks` | GET | Inspect how a document was chunked (knowledge units + embedding status) |
| `/v1/documents/{id}/reindex` | POST | Re-chunk and re-embed without deleting the document record |
| `/v1/documents/{id}` | DELETE | Delete document and trigger vector cleanup |
| `/v1/documents/{id}/status` | PATCH | Internal — workers update processing status |

**Persistence:** PostgreSQL `documents` and `knowledge_units` tables via SQLAlchemy async.  
**Events emitted:** `documents.created` and `documents.deleted` onto Redis Streams.

Documents service now exposes its declared gRPC surface for service-to-service calls.

---

### 2. `knowledge-index` — Background worker (no port)

Listens to the `documents.created` Redis Stream.

**What it does:**
1. Reads document content from `documents-service` over gRPC
2. Splits into `CHUNK_SIZE` (default 512) token chunks with `CHUNK_OVERLAP` (default 64) overlap
3. Writes each chunk as a `knowledge_unit` row in Postgres
4. Publishes each chunk to the `knowledge_units.created` Redis Stream

**Config:** `CHUNK_SIZE`, `CHUNK_OVERLAP` (env / docker-compose).

---

### 3. `embedding-worker` — Background worker (no port)

Listens to the `knowledge_units.created` Redis Stream.

**What it does:**
1. Accumulates chunks in batches (default 32) to minimise Azure OpenAI API calls
2. Calls **Azure OpenAI `text-embedding-3-large`** (3072-dimensional vectors)
3. Upserts each vector into Qdrant collection `dataplane_knowledge`
4. Updates the `knowledge_unit.embedding_status` in Postgres to `indexed`
5. When all chunks for a document are indexed, marks the `document.status` as `indexed`

**Model:** Azure OpenAI `text-embedding-3-large`  
**Why:** 3072 dims → highest retrieval precision in the Azure OpenAI family; single-model parity between indexing and query-time embedding.

---

### 4. `retrieval-service` — REST :8004 · gRPC :50052

The query interface. AI-Core calls this over REST or gRPC; it never talks to Qdrant directly.

**Pipeline (every query):**

```
1. Build hard filter  (org_id + optional: type, department, language, document_ids, region)
2. Embed query        → Azure OpenAI text-embedding-3-large
3. Vector search      → Qdrant ANN, top_k candidates (default 20)
4. Rerank             → Cohere rerank-english-v3.0, top_n results (default 5)
5. Build sources      → fresh JOIN against Postgres documents table (not stale Qdrant payload)
```

**Returns:**
```json
{
  "facts":   [{ "knowledge_id", "document_id", "text", "score", "rerank_score", "metadata" }],
  "sources": [{ "document_id", "title", "source", "type" }],
  "query":   "...",
  "org_id":  "..."
}
```

**Tuneable:** `TOP_K` (candidates before rerank), `TOP_N_AFTER_RERANK` (facts returned).

---

## Infrastructure

| Component | Image | Port(s) | Purpose |
|---|---|---|---|
| PostgreSQL 16 | `postgres:16-alpine` | 5432 (internal, optional host exposure via `docker-compose.ui.yml`) | Document + knowledge unit store |
| Redis 7 | `redis:7-alpine` | 6379 (internal, optional host exposure via `docker-compose.ui.yml`) | Event bus (Streams) between services |
| Qdrant 1.9.0 | `qdrant/qdrant:v1.9.0` | 6333 (REST), 6334 (gRPC) via `docker-compose.ui.yml` | Vector store |

Schema lives in `infra/postgres/init.sql`.
It is applied on first Postgres initialization and re-applied idempotently by `postgres-bootstrap` during `docker compose up`.

UI and admin-level host access is split out of the main compose file into `docker-compose.ui.yml`.

---

## Pipeline Flow

```
Caller (Reasoning Plane)
        │
        ▼  POST /v1/documents
  documents-service (:8001)
        │  writes row        ┐
        │  Postgres          │
        │  publishes →       │  Redis Stream: documents.created
        ▼                    ▼
  knowledge-index       (event consumer)
        │  chunks doc
        │  writes knowledge_units → Postgres
        │  publishes → Redis Stream: knowledge_units.created
        ▼
  embedding-worker
        │  batches chunks
        │  Azure OpenAI text-embedding-3-large
        │  upserts → Qdrant
        │  updates status → Postgres (indexed)
        ▼
     [indexed]

Caller (Reasoning Plane)
        │
        ▼  POST /v1/retrieve (query, org_id, filters)
  retrieval-service (:8004)
        │  embed query  → Azure OpenAI
        │  ANN search   → Qdrant (top_k=20)
        │  rerank       → Cohere rerank-english-v3.0 (top_n=5)
        │  join sources → Postgres
        ▼
    { facts, sources }
```

---

## Key Design Decisions

| Decision | Rationale |
|---|---|
| Azure OpenAI `text-embedding-3-large` (3072 dim) | Best-in-class Azure OpenAI model; single model for both indexing and retrieval guarantees embedding space parity |
| Cohere `rerank-english-v3.0` for reranking | Cross-encoder rerankers are architecturally superior to bi-encoders for precision at small `top_n`; Cohere's model is best-in-class |
| Redis Streams as event bus | Lightweight, ordered, at-least-once — sufficient for the ingest pipeline without a full message broker |
| Sources joined from Postgres at retrieval time | Qdrant payload is written at index time and becomes stale after re-index. Postgres is authoritative. |
| Re-index via event replay | `POST /{id}/reindex` deletes KUs, emits `documents.deleted` (Qdrant cleanup), resets status, re-emits `documents.created`. Reuses all existing handlers with no bespoke logic. |
| Bulk ingest endpoint | Allows seeding from SharePoint exports, database dumps, or any batch source in a single HTTP call |
| gRPC for server-to-server paths | `documents-service` now exposes `50051`, `retrieval-service` now exposes `50052`, and `knowledge-index` fetches documents over gRPC. |

---

## File Map

```
Data Plane/
├── docker-compose.yml
├── infra/
│   └── postgres/init.sql
├── .env.example
└── services/
    ├── documents/
    │   └── app/
    │       ├── api/v1/documents.py     ← all HTTP routes
    │       ├── db/postgres.py          ← all SQL queries
    │       ├── events/publisher.py     ← Redis Stream publisher
    │       ├── models/document.py      ← Pydantic models (incl. bulk + chunk models)
    │       └── config.py
    ├── knowledge-index/
    │   └── worker/
    │       ├── main.py                 ← Redis consumer loop
    │       ├── chunker.py
    │       └── knowledge_builder.py
    ├── embedding-worker/
    │   └── worker/
    │       ├── main.py                 ← Redis consumer + batch controller
    │       ├── openai_embed_client.py  ← Azure OpenAI calls
    │       └── qdrant_writer.py        ← Qdrant upsert
    └── retrieval/
        └── app/
            ├── api/                    ← FastAPI routes
            ├── retrieval/
            │   ├── pipeline.py         ← orchestrates steps 1-5
            │   ├── vector_search.py
            │   ├── rerank.py
            │   └── filters.py
            ├── db.py                   ← Postgres source metadata lookup
            └── config.py
```
