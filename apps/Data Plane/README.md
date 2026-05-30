# Data Plane

The knowledge infrastructure layer of the CoreSystem.

```
Internet / Files / APIs
        ↓
     Documents          ← ground truth, stored in Postgres
        ↓
  Knowledge Index       ← chunking + knowledge unit creation
        ↓
  Embedding Worker      ← Cohere → float vectors
        ↓
      Qdrant            ← semantic memory (vector store)
        ↓
   Retrieval API        ← the only door into knowledge
        ↓
     AI-Core            ← reasons, never stores
```

**Rule: AI reasons. Data Plane knows.**

---

## Services

| Service | Port | Role |
|---|---|---|
| `documents-service` | `8001` (REST) · `50051` (gRPC) | Ingest & manage raw documents |
| `knowledge-index` | `9101` (admin/metrics) | Chunk documents → knowledge units |
| `embedding-worker` | `9102` (admin/metrics) | Cohere embed → Qdrant upsert |
| `retrieval-service` | `8004` (REST) · `50052` (gRPC) | The only interface AI-Core uses |
| `postgres` | `5432` | Documents + knowledge units |
| `redis` | `6379` | Async job streams (Redis Streams) |
| `qdrant` | `6333` (REST) · `6334` (gRPC) | Vector store |

---

## Quick Start

### 1. Configure

```bash
cp .env.example .env
# Edit .env — set COHERE_API_KEY
```

### 2. Start everything

```bash
make up
```

The main compose file now starts the backend stack only. Infra/admin host access is kept out of the default startup path.

`docker build` does not run SQL against Postgres. Schema bootstrap now happens at runtime:
- `postgres` still runs `infra/postgres/init.sql` on first database initialization.
- `postgres-bootstrap` reapplies the same idempotent SQL on `docker compose up` so missing tables, indexes, and triggers are restored for an existing volume.

### 3. Ingest a document

```bash
make dev-ingest
```

### 4. Query knowledge

```bash
make dev-retrieve
```

### 5. Monitor

- Documents API docs: http://localhost:8001/docs
- Retrieval API docs: http://localhost:8004/docs
- Documents readiness: http://localhost:8001/readyz
- Retrieval readiness: http://localhost:8004/readyz
- Documents metrics: http://localhost:8001/metrics
- Retrieval metrics: http://localhost:8004/metrics
- Knowledge Index admin: http://localhost:9101/readyz and http://localhost:9101/metrics
- Embedding Worker admin: http://localhost:9102/readyz and http://localhost:9102/metrics

### 5a. Benchmark

Run a local ingest/index/retrieval benchmark against the live APIs:

```bash
make benchmark
```

The benchmark posts synthetic documents, waits for them to reach `indexed`, runs concurrent retrieval requests, and reports ingest plus retrieval percentiles.

### 6. Optional UI / Admin Access

Development only: this overlay binds infra ports on the host so local admin tools can connect.

```bash
make up-ui
```

This overlay exposes the infra backends to the host only when you need tooling access.

- Qdrant Dashboard: http://localhost:6333/dashboard
- PostgreSQL: `localhost:5432`
- Redis: `localhost:6379`
- Direct compose command: `docker compose -f docker-compose.yml -f docker-compose.ui.yml up -d`

---

## Architecture

### Event Flow (async, Redis Streams)

```
POST /v1/documents
   │
   └─ documents-service ──► stream: dataplane.documents.created
                                       │
                               knowledge-index (worker)
                               [chunk text → knowledge units → Postgres]
                                       │
                                       └─► stream: dataplane.knowledge.units.created
                                                       │
                                               embedding-worker
                                               [Cohere embed → Qdrant upsert]
                                                       │
                                               document.status = "indexed"
```

### Retrieval Flow (sync, REST / gRPC)

```
AI-Core → POST /v1/retrieve { org_id, query, filters }
  or → RetrievalService/Retrieve
       │
       └─ retrieval-service
         1. build Qdrant hard filter  (org_id always enforced)
         2. embed query               (Cohere search_query)
         3. vector search             (Qdrant ANN, top_k=20)
         4. rerank                    (Cohere cross-encoder, top_n=5)
         5. return facts + sources
```

### Deletion (GDPR-safe)

```
DELETE /v1/documents/{id}
   │
   ├─ Postgres: CASCADE delete → knowledge_units
   └─ stream: dataplane.documents.deleted
                   │
           embedding-worker → delete Qdrant vectors by document_id
```

---

## API Reference

### Documents Service (`localhost:8001`)

| Method | Path | Description |
|---|---|---|
| `POST` | `/v1/documents` | Ingest a document |
| `GET` | `/v1/documents?org_id=` | List documents |
| `GET` | `/v1/documents/{id}?org_id=` | Get document with content |
| `DELETE` | `/v1/documents/{id}?org_id=` | Delete + clean up vectors |
| `PATCH` | `/v1/documents/{id}/status` | Internal status update |

**Ingest payload:**
```json
{
  "org_id":   "org_abc",
  "source":   "sharepoint",
  "type":     "policy",
  "title":    "Vacation Policy",
  "content":  "Full text content here...",
  "metadata": { "department": "HR", "language": "en" }
}
```

### Retrieval Service (`localhost:8004`)

| Method | Path | Description |
|---|---|---|
| `POST` | `/v1/retrieve` | Query knowledge for an org |

**Request:**
```json
{
  "org_id": "org_abc",
  "query":  "How many vacation days do employees get?",
  "filters": {
    "document_types": ["policy"],
    "departments":    ["HR"],
    "languages":      ["en"]
  },
  "top_k": 20,
  "top_n": 5
}
```

**Response:**
```json
{
  "facts": [
    {
      "knowledge_id": "kn_456",
      "document_id":  "doc_123",
      "text":         "Employees receive 25 vacation days per year.",
      "score":        0.91,
      "rerank_score": 0.97,
      "metadata":     { "department": "HR", "type": "policy" }
    }
  ],
  "sources": [
    {
      "document_id": "doc_123",
      "title":       "Vacation Policy",
      "source":      "sharepoint",
      "type":        "policy"
    }
  ],
  "query":  "How many vacation days do employees get?",
  "org_id": "org_abc"
}
```

---

## Proto / gRPC

Service-to-service gRPC contracts are defined in `proto/`:

| File | Service | Purpose |
|---|---|---|
| `documents.proto` | `DocumentService` | Knowledge Index fetches documents over gRPC |
| `knowledge.proto` | `KnowledgeService` | Permission checks + unit lookup |
| `retrieval.proto` | `RetrievalService` | AI-Core gRPC interface (alternative to REST) |

Generate stubs:
```bash
pip install grpcio-tools
make proto
```

---

## Environment Variables

| Variable | Default | Description |
|---|---|---|
| `AZURE_OPENAI_API_KEY_EMBEDDING` | — | **Required** for query and chunk embeddings |
| `AZURE_OPENAI_ENDPOINT_EMBEDDING` | `https://core-ai-rg.cognitiveservices.azure.com` | Azure OpenAI endpoint |
| `AZURE_OPENAI_DEPLOYMENT_EMBEDDING` | `text-embedding-3-large` | Azure embedding deployment |
| `AZURE_OPENAI_API_VERSION_EMBEDDING` | `2024-12-01-preview` | Azure OpenAI API version |
| `COHERE_API_KEY` | — | **Required** for reranking |
| `COHERE_RERANK_MODEL` | `rerank-english-v3.0` | Reranking model |
| `CHUNK_SIZE` | `512` | Max chars per knowledge unit |
| `CHUNK_OVERLAP` | `64` | Overlap between chunks |
| `EMBEDDING_BATCH_SIZE` | `32` | Units per embedding batch |
| `KNOWLEDGE_INDEX_PENDING_MIN_IDLE_MS` | `30000` | Minimum idle time before reclaiming pending knowledge-index jobs |
| `KNOWLEDGE_INDEX_MAX_DELIVERY_ATTEMPTS` | `5` | Delivery attempts before a knowledge-index message is sent to the DLQ |
| `KNOWLEDGE_INDEX_ADMIN_PORT` | `9101` | Admin/metrics port for the knowledge-index worker |
| `EMBEDDING_PENDING_MIN_IDLE_MS` | `30000` | Minimum idle time before reclaiming pending embedding jobs |
| `EMBEDDING_MAX_DELIVERY_ATTEMPTS` | `5` | Delivery attempts before an embedding message is sent to the DLQ |
| `EMBEDDING_ADMIN_PORT` | `9102` | Admin/metrics port for the embedding worker |
| `RETRIEVAL_TOP_K` | `20` | Vector search candidates |
| `RETRIEVAL_TOP_N` | `5` | Facts returned after rerank |
| `QDRANT_COLLECTION` | `dataplane_knowledge` | Collection name |

---

## Design Principles

1. **Documents are ground truth** — Postgres is the source of truth, Qdrant is an index.
2. **Embedding is async** — expensive, batchable, model-replaceable.
3. **org_id is always enforced** — hard filter on every vector search, no exceptions.
4. **AI gets facts, not documents** — the retrieval response shape is fixed and minimal.
5. **Deletion is complete** — deleting a document cascades through Postgres and Qdrant.
6. **Model-agnostic** — swap Cohere for OpenAI / BGE by changing the worker config.
