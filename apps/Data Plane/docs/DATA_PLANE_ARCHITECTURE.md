# Data Plane Architecture

**Last Updated:** 2025-07-08

## Pyramid Placement

The Data Plane sits at **Layer 2** of the CoreSystem pyramid. It is the
**canonical authority** for document storage, vector embeddings, knowledge
indexing, and semantic retrieval. No other plane may write directly to its
databases or vector store. Higher layers (Control Plane) issue identity and
org context that the Data Plane trusts; lower layers (Ingestion, Model,
Application, Frontend) consume Data Plane APIs as clients.

### Authority Rules

- ✅ **Canonical owner** of all document content and metadata (PostgreSQL `dataplane` database)
- ✅ **Canonical owner** of all vector embeddings (Qdrant `dataplane_knowledge` collection)
- ✅ **Canonical owner** of the retrieval pipeline (hard-filter → embed → ANN → rerank → join)
- ✅ **Canonical owner** of knowledge-unit chunking and embedding lifecycle
- Does **not** own user identity, sessions, or org membership (Control Plane)
- Does **not** own raw file import or connector orchestration (Ingestion Plane)
- Does **not** own LLM inference, agent execution, or reasoning (Model Plane v2)
- Does **not** own application-level projections or UI composition (Application / Frontend Planes)

## 🏗️ Service Structure

```
┌─────────────────────────────────────────────────────────────────┐
│                         DATA PLANE (L2)                         │
│                                                                 │
│  ┌──────────────────┐  ┌──────────────────┐                    │
│  │ documents-service │  │ retrieval-service │                    │
│  │  REST :8001       │  │  REST :8004       │                    │
│  │  gRPC :50051      │  │  gRPC :50052      │                    │
│  └────────┬─────────┘  └────────┬─────────┘                    │
│           │                      │                               │
│  ┌────────┴─────────┐  ┌────────┴─────────┐                    │
│  │  knowledge-index  │  │ embedding-worker  │                    │
│  │  Worker :9101     │  │  Worker :9102     │                    │
│  └──────────────────┘  └──────────────────┘                    │
│                                                                 │
│  ── Infrastructure ──────────────────────────────────────────   │
│  │ PostgreSQL 16  │  │ Redis 7 (AOF)  │  │ Qdrant v1.9.0  │   │
│  │ :5432 (dataplane)│  │ :6379           │  │ :6333 / :6334  │   │
│  └─────────────────┘  └────────────────┘  └───────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## 📦 Service Responsibilities

### 1. **documents-service** (REST :8001 / gRPC :50051)

**Domain**: Document CRUD and metadata management
**Technology**: Python (asyncpg, grpcio)
**Database**: PostgreSQL 16 — `dataplane` database

**Owns**:
- ✅ Document create, read, update, delete operations
- ✅ Document metadata and content storage
- ✅ 8 REST API routes for document lifecycle
- ✅ gRPC interface for internal service-to-service calls

**Events Published**:
- `documents.created` — emitted to Redis Streams when a new document is stored
- `documents.deleted` — emitted to Redis Streams when a document is removed

**Events Subscribed**: None

**Health Check**: `curl http://localhost:8001/readyz`
**Dependencies**: postgres, redis

---

### 2. **knowledge-index** (Worker :9101)

**Domain**: Document chunking and knowledge-unit creation
**Technology**: Python (worker process)

**Owns**:
- ✅ Chunking pipeline (CHUNK_SIZE=512, OVERLAP=64)
- ✅ Listening for new documents and producing knowledge units
- ✅ gRPC calls to documents-service for content retrieval

**Events Published**:
- `knowledge_units.created` — emitted when chunks are produced for a document

**Events Subscribed**:
- `documents.created` — triggers chunking pipeline

**Dependencies**: postgres, redis, documents-service (gRPC :50051)

---

### 3. **embedding-worker** (Worker :9102)

**Domain**: Vector embedding generation and Qdrant upsert
**Technology**: Python (asyncpg, qdrant-client)

**Owns**:
- ✅ Azure OpenAI `text-embedding-3-large` integration (3072-dimension)
- ✅ Batch embedding (batch size 32)
- ✅ Qdrant upsert into `dataplane_knowledge` collection
- ✅ Metadata join back to PostgreSQL

**Events Published**: None (terminal worker)

**Events Subscribed**:
- `knowledge_units.created` — triggers embedding generation

**Dependencies**: postgres, redis, qdrant

---

### 4. **retrieval-service** (REST :8004 / gRPC :50052)

**Domain**: Semantic search and retrieval-augmented generation support
**Technology**: Python (asyncpg, qdrant-client, cohere)

**Owns**:
- ✅ 5-stage retrieval pipeline:
  1. Hard filter (org_id, collection scope)
  2. Query embedding (Azure OpenAI)
  3. Qdrant ANN search (top_k=20)
  4. Cohere rerank (top_n=5)
  5. PostgreSQL metadata join
- ✅ gRPC interface for Model Plane consumption

**Events Published**: None
**Events Subscribed**: None

**Health Check**: `curl http://localhost:8004/health`
**Dependencies**: postgres, qdrant

---

## 🗄️ Infrastructure

### PostgreSQL 16

| Setting | Value |
|---------|-------|
| Database | `dataplane` |
| shared_buffers | 128MB |
| max_connections | 100 |
| Container memory | 512M |
| Volume | `postgres_data` |

### Redis 7

| Setting | Value |
|---------|-------|
| Persistence | AOF everysec |
| maxmemory | 96mb |
| maxmemory-policy | noeviction |
| Container memory | 128M |
| Volume | `redis_data` |

### Qdrant v1.9.0

| Setting | Value |
|---------|-------|
| REST port | 6335 → 6333 |
| gRPC port | 6336 → 6334 |
| Container memory | 384M |
| Volume | `qdrant_data` |

## Networks

| Network | Type | Purpose |
|---------|------|---------|
| `data-net` | bridge | Internal Data Plane communication |
| `velion-net` | external | Cross-plane connectivity |

## Does NOT Own

| Capability | Canonical Owner |
|------------|-----------------|
| User identity, sessions, org membership | Control Plane (auth-core, user-core, org-core) |
| Raw file import, connector orchestration | Ingestion Plane (Quarry, imports-core) |
| LLM inference, agent execution, reasoning | Model Plane v2 (ai-core, agent-core-v2) |
| Application projections, real-time sync | Application Plane (convex-backend) |
| UI rendering, session-aware routing | Frontend Plane (velion) |

## Cross-Plane Contract Rules

1. **Ingestion → Data**: Ingestion Plane calls `documents-service` REST/gRPC to
   store imported content. It never writes directly to the `dataplane` database.
2. **Model → Data**: Model Plane v2 calls `retrieval-service` gRPC (:50052) for
   RAG context. It never queries Qdrant directly.
3. **Control → Data**: Control Plane provides org_id and user_id context via
   JWT claims. Data Plane trusts these for hard-filter scoping.
4. **Application → Data**: Application Plane reads via `retrieval-service` REST
   (:8004). It never holds a connection to the dataplane database.
5. **Embedding credentials**: Azure OpenAI and Cohere API keys are owned by
   Data Plane configuration. No other plane may embed or rerank independently.
