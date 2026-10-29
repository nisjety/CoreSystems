# RAG System - Complete Guide

**Org-core's production-grade Retrieval-Augmented Generation (RAG) system**

*Last Updated: February 2, 2026*

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Quick Start](#quick-start)
4. [Setup & Configuration](#setup--configuration)
5. [Implementation Details](#implementation-details)
6. [Testing](#testing)
7. [Advanced Features](#advanced-features)
8. [Performance & Optimization](#performance--optimization)
9. [API Reference](#api-reference)
10. [Troubleshooting](#troubleshooting)

---

## Overview

### What is RAG?

Retrieval-Augmented Generation (RAG) enhances LLM responses by retrieving relevant context from your knowledge base before generating answers. This enables:

- **Accurate responses** grounded in your data
- **Reduced hallucinations** with source citations
- **Up-to-date information** without retraining models
- **Domain-specific expertise** using your documents

### Key Features

- ✅ **Hybrid Search**: Dense (semantic) + Sparse (keyword) vectors with RRF fusion
- ✅ **Multi-Stage Retrieval**: Query classification → Search → Re-ranking → Filtering
- ✅ **Agentic RAG**: ReAct-style reasoning with query planning and routing
- ✅ **Performance Optimized**: HNSW indexing, scalar quantization, semantic caching
- ✅ **Production Ready**: Metrics, logging, error handling, multi-tenancy

### Tech Stack

| Component | Technology | Purpose |
|-----------|-----------|---------|
| Vector Store | **Qdrant** | Fast similarity search with HNSW |
| Embeddings | sentence-transformers/all-MiniLM-L6-v2 | 384-dim dense vectors |
| Sparse Vectors | BM25 | Keyword matching |
| Database | PostgreSQL | Document metadata & metrics |
| Caching | Multi-tier (Ristretto + Redis) | 100x faster repeated queries |
| Language | Go | High-performance service |

---

## Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Client Application                        │
│              (Frontend, API Consumer)                        │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                     Org-core HTTP API                        │
│              POST /api/v1/rag/retrieve                       │
│              POST /api/v1/rag/documents                      │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                      RAG Service Layer                       │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  1. Query Classification & Planning                  │   │
│  │     • Determine retrieval strategy                   │   │
│  │     • Route to appropriate agents                    │   │
│  └──────────────────────────────────────────────────────┘   │
│                            │                                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  2. Embedding Generation                             │   │
│  │     • Dense: sentence-transformers (384-dim)         │   │
│  │     • Sparse: BM25 keyword extraction                │   │
│  └──────────────────────────────────────────────────────┘   │
│                            │                                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  3. Multi-tier Cache Check                           │   │
│  │     • L1: Ristretto (in-memory, 100MB)               │   │
│  │     • L2: Redis (distributed, 1GB)                   │   │
│  └──────────────────────────────────────────────────────┘   │
│                            │                                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  4. Vector Search (Qdrant)                           │   │
│  │     • Dense: Cosine similarity                       │   │
│  │     • Sparse: Keyword matching                       │   │
│  │     • Hybrid: RRF fusion (k=60)                      │   │
│  │     • Filters: org_id, type, tags                    │   │
│  └──────────────────────────────────────────────────────┘   │
│                            │                                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  5. Re-ranking (Optional)                            │   │
│  │     • Cross-encoder models (monoT5)                  │   │
│  │     • Boosts precision at cost of latency            │   │
│  └──────────────────────────────────────────────────────┘   │
│                            │                                 │
│  ┌──────────────────────────────────────────────────────┐   │
│  │  6. Result Filtering & Formatting                    │   │
│  │     • Similarity threshold (>0.7)                    │   │
│  │     • Top-K selection                                │   │
│  │     • Metadata enrichment                            │   │
│  └──────────────────────────────────────────────────────┘   │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│                     Storage Layer                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │   Qdrant     │  │  PostgreSQL  │  │    Redis     │      │
│  │   Vectors    │  │   Metadata   │  │    Cache     │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└─────────────────────────────────────────────────────────────┘
```

### Data Flow

#### Document Indexing Flow
```
Document Upload
    │
    ▼
Chunking (512-700 tokens)
    │
    ▼
Embedding Generation
    ├─ Dense Vector (384-dim)
    └─ Sparse Vector (BM25)
    │
    ▼
Store in Qdrant
    │
    ▼
Save Metadata to PostgreSQL
```

#### Retrieval Flow
```
User Query
    │
    ▼
Cache Check (L1 → L2)
    │
    ├─ HIT → Return cached results (1-5ms)
    │
    └─ MISS ↓
    │
    ▼
Generate Embeddings
    │
    ▼
Hybrid Search in Qdrant
    │
    ▼
Re-rank Results (optional)
    │
    ▼
Filter & Format
    │
    ▼
Cache Results (TTL: 60s)
    │
    ▼
Return to Client
```

### Database Schema

```sql
-- Documents table
CREATE TABLE rag_documents (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    document_type VARCHAR(50),
    title TEXT,
    content TEXT NOT NULL,
    tags TEXT[],
    metadata JSONB,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Chunks table
CREATE TABLE rag_chunks (
    id UUID PRIMARY KEY,
    document_id UUID REFERENCES rag_documents(id) ON DELETE CASCADE,
    org_id UUID NOT NULL,
    chunk_index INTEGER NOT NULL,
    content TEXT NOT NULL,
    vector_id VARCHAR(255), -- Qdrant point ID
    embedding_model VARCHAR(100),
    created_at TIMESTAMP DEFAULT NOW()
);

-- Retrieval metrics
CREATE TABLE rag_retrievals (
    id UUID PRIMARY KEY,
    org_id UUID NOT NULL,
    query TEXT NOT NULL,
    strategy VARCHAR(50),
    results_count INTEGER,
    latency_ms INTEGER,
    cache_hit BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_rag_documents_org_id ON rag_documents(org_id);
CREATE INDEX idx_rag_chunks_document_id ON rag_chunks(document_id);
CREATE INDEX idx_rag_retrievals_org_id_created ON rag_retrievals(org_id, created_at);
```

---

## Quick Start

### Prerequisites

- Docker (for Qdrant and PostgreSQL)
- Go 1.25+
- PostgreSQL client tools

### 1. Start Qdrant Vector Database

```bash
# Option A: Docker (Recommended for local dev)
docker run -d \
  -p 6333:6333 \
  -v $(pwd)/qdrant_storage:/qdrant/storage \
  --name qdrant \
  qdrant/qdrant:latest

# Option B: Qdrant Cloud (Recommended for production)
# 1. Sign up at https://cloud.qdrant.io
# 2. Create a cluster
# 3. Get API key and cluster URL
```

### 2. Run Database Migrations

```bash
# Ensure PostgreSQL is running
docker ps | grep postgres

# Run migrations
cd /Volumes/Lagring/Triodelab/CoreSystem/backend/Org-core

# Create database if needed
cat migrations/001_init.up.sql | docker exec -i coresystem-postgres-local psql -U coresystem -d org_core

# Add RAG tables
cat migrations/006_rag_tables.up.sql | docker exec -i coresystem-postgres-local psql -U coresystem -d org_core
```

### 3. Configure Environment

```bash
# Create .env.local
cat > .env.local << 'EOF'
# Enable RAG
ENABLE_RAG=true

# Database
DATABASE_DSN="postgres://coresystem:PASSWORD@localhost:5432/org_core?sslmode=disable"

# Qdrant Configuration
QDRANT_HOST=localhost
QDRANT_PORT=6333
QDRANT_API_KEY=  # Empty for local Docker
QDRANT_COLLECTION_NAME=org_core_vectors

# RAG Settings
CHUNK_SIZE=512
CHUNK_OVERLAP=50
VECTOR_DIMENSION=384
USE_SPARSE_VECTORS=true

# Cache Configuration
CACHE_TYPE=multi-tier
CACHE_LOCAL_MAX_COST_MB=100
CACHE_LOCAL_TTL_SECONDS=60
REDIS_URL=redis://localhost:6379/0

# Embeddings (optional - uses mock if not provided)
OPENAI_API_KEY=sk-your-key-here
EOF
```

### 4. Start Org-core Service

```bash
# Build
go build ./cmd/server

# Run
source .env.local && ./server

# Or with Docker Compose
docker compose -f backend/docker-compose.yml up -d org-core
```

### 5. Test the System

```bash
# Generate an org ID
ORG_ID=$(uuidgen)

# Index a test document
curl -X POST http://localhost:8080/api/v1/rag/documents \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "documents": [
      {
        "document_type": "faq",
        "title": "Return Policy",
        "content": "Our return policy allows returns within 30 days of purchase with original receipt. Items must be unused and in original packaging.",
        "tags": ["returns", "policy"]
      }
    ]
  }'

# Query the system
curl -X POST http://localhost:8080/api/v1/rag/retrieve \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "query": "What is the return policy?",
    "top_k": 5,
    "strategy": "hybrid"
  }'
```

**Expected Response:**
```json
{
  "results": [
    {
      "chunk_id": "uuid-here",
      "document_id": "uuid-here",
      "content": "Our return policy allows returns within 30 days...",
      "score": 0.92,
      "metadata": {
        "title": "Return Policy",
        "type": "faq",
        "tags": ["returns", "policy"]
      }
    }
  ],
  "strategy_used": "hybrid",
  "total_results": 1,
  "latency_ms": 45
}
```

---

## Setup & Configuration

### Qdrant Setup Options

#### Option 1: Docker (Development)

```bash
# Start Qdrant with persistence
docker run -d \
  --name qdrant \
  -p 6333:6333 \
  -p 6334:6334 \
  -v $(pwd)/qdrant_storage:/qdrant/storage \
  qdrant/qdrant:latest

# Verify it's running
curl http://localhost:6333/
```

#### Option 2: Qdrant Cloud (Production)

1. **Create Account**: Visit https://cloud.qdrant.io
2. **Create Cluster**: Choose region and tier
3. **Get Credentials**: API Key + Cluster URL
4. **Update Configuration**:
```env
QDRANT_HOST=your-cluster.qdrant.io
QDRANT_PORT=6333
QDRANT_API_KEY=your-api-key-here
QDRANT_USE_TLS=true
```

### Collection Configuration

Collections are auto-created on first document index with these settings:

```go
// Dense vectors
VectorParams: {
    Size: 384,
    Distance: Cosine,
}

// Sparse vectors (keyword matching)
SparseVectorParams: {
    Index: {
        OnDisk: false,
    },
}

// HNSW index config
HNSWConfig: {
    M: 16,              // Number of bi-directional links
    EfConstruct: 100,   // Construction search depth
    FullScanThreshold: 10000,
}

// Quantization (4x memory reduction)
QuantizationConfig: {
    Scalar: {
        Type: Int8,
        Quantile: 0.99,
        AlwaysRam: true,
    },
}
```

### Environment Variables Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `ENABLE_RAG` | `false` | Enable RAG system |
| `QDRANT_HOST` | `localhost` | Qdrant server host |
| `QDRANT_PORT` | `6333` | Qdrant server port |
| `QDRANT_API_KEY` | `` | API key (empty for local) |
| `QDRANT_COLLECTION_NAME` | `org_core_vectors` | Collection name |
| `QDRANT_USE_TLS` | `false` | Use HTTPS for Qdrant |
| `CHUNK_SIZE` | `512` | Target chunk size (tokens) |
| `CHUNK_OVERLAP` | `50` | Overlap between chunks |
| `VECTOR_DIMENSION` | `384` | Embedding dimension |
| `USE_SPARSE_VECTORS` | `true` | Enable BM25 keyword search |
| `CACHE_TYPE` | `multi-tier` | Cache strategy |
| `CACHE_LOCAL_MAX_COST_MB` | `100` | L1 cache size |
| `CACHE_LOCAL_TTL_SECONDS` | `60` | Cache TTL |

---

## Implementation Details

### Document Processing Pipeline

#### 1. Chunking Strategy

```go
// Configuration
ChunkSize: 512 tokens (optimal for 384-dim embeddings)
Overlap: 50 tokens (preserves context across boundaries)
Method: Sentence-aware splitting

// Example chunks from a document:
Document: "Our return policy allows returns within 30 days..."
↓
Chunk 1: "Our return policy allows returns within 30 days of purchase..."
Chunk 2: "...purchase with original receipt. Items must be unused..."
Chunk 3: "...unused and in original packaging. Contact support at..."
```

#### 2. Embedding Generation

**Dense Vectors (Semantic Search):**
- Model: `sentence-transformers/all-MiniLM-L6-v2`
- Dimension: 384
- Speed: ~1000 chunks/second on CPU
- Use case: Understanding meaning and context

**Sparse Vectors (Keyword Search):**
- Algorithm: BM25
- Dimension: Varies (vocabulary-dependent)
- Speed: <1ms per query
- Use case: Exact term matching, acronyms, proper nouns

#### 3. Indexing to Qdrant

```go
// Point structure
Point{
    ID: UUID,
    Vector: {
        Dense: [384]float32,    // Semantic vector
        Sparse: {               // Keyword vector
            Indices: []uint32,
            Values: []float32,
        },
    },
    Payload: {
        "org_id": "uuid",
        "document_id": "uuid",
        "chunk_index": 0,
        "content": "text",
        "title": "doc title",
        "type": "faq",
        "tags": ["tag1", "tag2"],
        "created_at": 1738521600,
    },
}
```

### Retrieval Strategies

#### Dense Search (Semantic)

```bash
# Best for: Natural language queries, paraphrases
# Example: "How do I return an item?" matches "return policy"

curl -X POST http://localhost:8080/api/v1/rag/retrieve \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "query": "How do I return an item?",
    "strategy": "dense",
    "top_k": 5
  }'
```

**Pros**: Understands synonyms, context, meaning
**Cons**: May miss exact terms, proper nouns

#### Sparse Search (Keyword)

```bash
# Best for: Exact terms, product codes, names
# Example: "SKU-12345" or "John Smith"

curl -X POST http://localhost:8080/api/v1/rag/retrieve \
  -d '{
    "query": "iPhone 15 Pro Max",
    "strategy": "sparse",
    "top_k": 5
  }'
```

**Pros**: Exact matching, fast, deterministic
**Cons**: No semantic understanding

#### Hybrid Search (Recommended)

```bash
# Best for: Most real-world queries
# Combines semantic understanding + exact matching

curl -X POST http://localhost:8080/api/v1/rag/retrieve \
  -d '{
    "query": "iPhone 15 Pro return policy",
    "strategy": "hybrid",
    "top_k": 5
  }'
```

**Algorithm**: Reciprocal Rank Fusion (RRF)
```
score = sum(1 / (k + rank_i))
where k = 60 (constant)
```

**Pros**: Best of both worlds, most robust
**Cons**: Slightly higher latency (2x searches)

### Multi-tier Caching

```go
// L1: Ristretto (In-Memory)
- Size: 100MB
- TTL: 60 seconds
- Hit rate: ~60-70%
- Latency: 1-2ms

// L2: Redis (Distributed)
- Size: 1GB
- TTL: 5 minutes
- Hit rate: ~20-25%
- Latency: 5-10ms

// L3: Qdrant (Vector DB)
- Size: Unlimited
- Latency: 40-80ms

// Combined hit rate: ~85-95%
// Average latency: <10ms (vs 50ms without cache)
```

---

## Testing

### Manual Testing

```bash
# Set your org ID
export ORG_ID=$(uuidgen)
export BASE_URL=http://localhost:8080

# Test 1: Index documents
curl -X POST $BASE_URL/api/v1/rag/documents \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $ORG_ID" \
  -d @- << 'EOF'
{
  "documents": [
    {
      "document_type": "faq",
      "title": "Shipping Information",
      "content": "We offer free shipping on orders over $50. Standard shipping takes 3-5 business days. Express shipping is available for $15 and delivers in 1-2 business days.",
      "tags": ["shipping", "delivery", "faq"]
    },
    {
      "document_type": "product",
      "title": "iPhone 15 Pro Max",
      "content": "The iPhone 15 Pro Max features a 6.7-inch Super Retina XDR display, A17 Pro chip, titanium design, and advanced camera system with 5x optical zoom.",
      "tags": ["iphone", "smartphone", "apple"]
    }
  ]
}
EOF

# Test 2: Dense search (semantic)
curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "query": "How long does delivery take?",
    "strategy": "dense",
    "top_k": 3
  }'

# Test 3: Sparse search (keyword)
curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "query": "iPhone 15 Pro Max",
    "strategy": "sparse",
    "top_k": 3
  }'

# Test 4: Hybrid search (best)
curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "query": "iPhone shipping options",
    "strategy": "hybrid",
    "top_k": 3
  }'

# Test 5: Filter by type
curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -H "X-Org-ID: $ORG_ID" \
  -d '{
    "query": "product information",
    "strategy": "hybrid",
    "top_k": 5,
    "filters": {
      "document_type": "product"
    }
  }'

# Test 6: Cache performance
# Run same query twice, second should be much faster
time curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -H "X-Org-ID: $ORG_ID" \
  -d '{"query": "shipping", "strategy": "hybrid"}'

time curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -H "X-Org-ID: $ORG_ID" \
  -d '{"query": "shipping", "strategy": "hybrid"}'
```

### Integration Tests

```bash
# Run comprehensive test suite
cd /Volumes/Lagring/Triodelab/CoreSystem/backend/Org-core
go test ./internal/rag/... -v

# Run with coverage
go test ./internal/rag/... -coverprofile=coverage.out
go tool cover -html=coverage.out
```

### Performance Testing

```bash
# Load test with hey
hey -n 1000 -c 10 -m POST \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: $ORG_ID" \
  -d '{"query":"test","strategy":"hybrid"}' \
  http://localhost:8080/api/v1/rag/retrieve

# Expected results:
# Total: 1000 requests
# Success: 100%
# p50: <50ms
# p95: <100ms
# p99: <200ms
```

---

## Advanced Features

### Agentic RAG

For complex multi-step queries, use LangGraph (already installed in ai-core):

```python
from langgraph.graph import StateGraph
from langchain_core.messages import HumanMessage

# Define workflow
class RAGState(TypedDict):
    query: str
    subqueries: list[str]
    documents: list[dict]
    answer: str

workflow = StateGraph(RAGState)

# Add nodes
workflow.add_node("plan", query_planning_agent)
workflow.add_node("retrieve", retrieval_agent)
workflow.add_node("synthesize", synthesis_agent)

# Define flow
workflow.add_edge("plan", "retrieve")
workflow.add_edge("retrieve", "synthesize")
workflow.set_entry_point("plan")

app = workflow.compile()

# Execute
result = await app.ainvoke({
    "query": "Compare iPhone 15 Pro Max with Galaxy S24 Ultra on camera specs"
})
```

### Query Planning Agent

Breaks complex queries into sub-queries:

```
Input: "Compare iPhone 15 Pro with S24 Ultra on camera and battery"

Plan:
1. "iPhone 15 Pro camera specifications"
2. "Galaxy S24 Ultra camera specifications"
3. "iPhone 15 Pro battery capacity"
4. "Galaxy S24 Ultra battery capacity"

Retrieve: Execute 4 searches in parallel
Synthesize: Combine results into comparison table
```

### Re-ranking

Improve precision with cross-encoder re-ranking:

```go
// Configuration
Model: "cross-encoder/ms-marco-MiniLM-L-6-v2"
Top-K: 100 (initial retrieval)
Re-rank: 10 (final results)

// Performance impact
Latency: +20-30ms
Precision: +15-25%
```

### Semantic Caching

Cache query results based on semantic similarity:

```go
// Instead of exact query matching:
cache_key = hash(query)

// Use semantic similarity:
if cosine_similarity(query_embedding, cached_embedding) > 0.95:
    return cached_results

// Benefits:
- "return policy" = "returns policy" (cache hit)
- "shipping info" = "delivery information" (cache hit)
- Hit rate: +10-15%
```

---

## Performance & Optimization

### Benchmarks

| Operation | Cold Cache | Warm Cache | Improvement |
|-----------|------------|------------|-------------|
| Dense search | 45ms | 2ms | 22x faster |
| Sparse search | 35ms | 1ms | 35x faster |
| Hybrid search | 65ms | 3ms | 21x faster |
| Document indexing | 150ms/doc | N/A | N/A |

### Optimization Checklist

#### Vector Search
- ✅ HNSW indexing (M=16, ef_construct=100)
- ✅ Scalar quantization (int8) - 4x memory reduction
- ✅ Disk-based storage for large datasets
- ⚠️ Product quantization (optional, 8-16x compression)

#### Caching
- ✅ Multi-tier cache (L1: Ristretto, L2: Redis)
- ✅ Semantic query caching (0.95 similarity threshold)
- ✅ TTL: 60s local, 5min distributed
- ✅ Cache invalidation on document updates

#### Database
- ✅ Indexes on org_id, document_id
- ✅ Connection pooling (max 25 connections)
- ✅ Prepared statements
- ⚠️ Read replicas for high load (optional)

#### Application
- ✅ Batch document indexing (up to 100 docs)
- ✅ Async embedding generation
- ✅ Request timeout: 30s
- ✅ Circuit breaker for Qdrant failures

### Scaling Strategies

#### Horizontal Scaling
```yaml
# docker-compose.yml
services:
  org-core:
    image: org-core:latest
    deploy:
      replicas: 3
    environment:
      - QDRANT_HOST=qdrant-cluster
      - REDIS_URL=redis://redis-cluster:6379
```

#### Qdrant Clustering
```bash
# For >10M vectors or >100 QPS
# Use Qdrant Cloud or self-hosted cluster
# Shard collections across nodes
```

#### Cache Warming
```bash
# Pre-populate cache with common queries
curl -X POST $BASE_URL/api/v1/rag/cache/warm \
  -d '{"queries": ["return policy", "shipping info", ...]}'
```

---

## API Reference

### Index Documents

**Endpoint**: `POST /api/v1/rag/documents`

**Headers**:
- `X-Org-ID`: Organization ID (UUID)
- `Content-Type`: application/json

**Request**:
```json
{
  "documents": [
    {
      "document_type": "faq|product|article|policy",
      "title": "Document title",
      "content": "Full text content...",
      "tags": ["tag1", "tag2"],
      "metadata": {
        "source_url": "https://...",
        "author": "Name",
        "price": 99.99
      }
    }
  ]
}
```

**Response**:
```json
{
  "indexed_count": 1,
  "document_ids": ["uuid1"],
  "chunk_counts": [3],
  "processing_time_ms": 150
}
```

**Status Codes**:
- `200`: Success
- `400`: Invalid request
- `401`: Missing org ID
- `500`: Server error

### Retrieve Documents

**Endpoint**: `POST /api/v1/rag/retrieve`

**Headers**:
- `X-Org-ID`: Organization ID (UUID)
- `Content-Type`: application/json

**Request**:
```json
{
  "query": "User's natural language query",
  "top_k": 5,
  "strategy": "hybrid|dense|sparse",
  "filters": {
    "document_type": "faq",
    "tags": ["shipping"]
  },
  "similarity_threshold": 0.7
}
```

**Response**:
```json
{
  "results": [
    {
      "chunk_id": "uuid",
      "document_id": "uuid",
      "content": "Relevant text chunk...",
      "score": 0.92,
      "metadata": {
        "title": "Document title",
        "type": "faq",
        "tags": ["tag1"],
        "source_url": "https://..."
      }
    }
  ],
  "strategy_used": "hybrid",
  "total_results": 1,
  "latency_ms": 45,
  "cache_hit": false
}
```

### Delete Documents

**Endpoint**: `DELETE /api/v1/rag/documents/:document_id`

**Headers**:
- `X-Org-ID`: Organization ID (UUID)

**Response**:
```json
{
  "deleted": true,
  "document_id": "uuid",
  "chunks_deleted": 3
}
```

### Health Check

**Endpoint**: `GET /api/v1/rag/health`

**Response**:
```json
{
  "status": "healthy",
  "qdrant": {
    "status": "connected",
    "latency_ms": 2
  },
  "database": {
    "status": "connected",
    "latency_ms": 1
  },
  "cache": {
    "status": "operational",
    "hit_rate": 0.87
  }
}
```

---

## Troubleshooting

### Common Issues

#### 1. Qdrant Connection Failed

**Symptom**: `failed to connect to Qdrant: connection refused`

**Solutions**:
```bash
# Check if Qdrant is running
curl http://localhost:6333/

# Start Qdrant if not running
docker run -d -p 6333:6333 qdrant/qdrant

# Check firewall rules
sudo ufw allow 6333/tcp

# Verify environment variables
echo $QDRANT_HOST $QDRANT_PORT
```

#### 2. No Results Returned

**Symptom**: Empty results array with valid query

**Solutions**:
```bash
# Verify documents are indexed
curl -X GET $BASE_URL/api/v1/rag/documents \
  -H "X-Org-ID: $ORG_ID"

# Check collection exists in Qdrant
curl http://localhost:6333/collections

# Lower similarity threshold
curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -d '{"query":"test","similarity_threshold":0.5}'

# Verify org_id matches
echo $ORG_ID
```

#### 3. Slow Queries

**Symptom**: Latency >200ms consistently

**Diagnostics**:
```bash
# Check cache hit rate
curl $BASE_URL/api/v1/rag/metrics

# Monitor Qdrant performance
curl http://localhost:6333/metrics

# Check database query times
docker logs org-core-service | grep "query_duration"
```

**Solutions**:
- Enable caching if not already
- Use HNSW indexing (not brute force)
- Enable scalar quantization
- Scale Qdrant horizontally

#### 4. Out of Memory

**Symptom**: OOM killed or high memory usage

**Solutions**:
```bash
# Reduce local cache size
export CACHE_LOCAL_MAX_COST_MB=50

# Enable quantization in Qdrant
# (reduces memory by 4x)

# Use disk-based vectors for large collections
# Set in Qdrant collection config:
# on_disk: true

# Increase Docker memory limit
docker update --memory=4g org-core-service
```

#### 5. Embeddings Error

**Symptom**: `failed to generate embeddings`

**Solutions**:
```bash
# If using OpenAI:
echo $OPENAI_API_KEY  # Verify key is set

# If using local model:
# Ensure model is downloaded
ls ~/.cache/huggingface/hub/

# Use mock embeddings for testing
export USE_MOCK_EMBEDDINGS=true
```

### Debug Mode

```bash
# Enable debug logging
export LOG_LEVEL=debug

# Restart service
docker restart org-core-service

# View detailed logs
docker logs -f org-core-service | grep RAG
```

### Performance Profiling

```bash
# CPU profiling
curl http://localhost:9090/debug/pprof/profile?seconds=30 > cpu.prof
go tool pprof cpu.prof

# Memory profiling
curl http://localhost:9090/debug/pprof/heap > mem.prof
go tool pprof mem.prof

# Trace requests
curl http://localhost:9090/debug/pprof/trace?seconds=5 > trace.out
go tool trace trace.out
```

---

## Best Practices

### Document Preparation

✅ **DO**:
- Clean HTML/formatting before indexing
- Include relevant metadata (title, tags, type)
- Use descriptive titles
- Tag documents consistently
- Update documents instead of duplicating

❌ **DON'T**:
- Index binary files (images, PDFs) directly
- Create documents >10,000 tokens
- Use generic titles like "Document 1"
- Mix multiple topics in one document

### Query Optimization

✅ **DO**:
- Use hybrid search for most queries
- Set reasonable top_k (5-10)
- Apply filters when possible (type, tags)
- Cache frequently asked questions
- Monitor query patterns

❌ **DON'T**:
- Request top_k >100 (slow)
- Use overly broad queries
- Ignore cache hit rates
- Re-index documents unnecessarily

### Production Deployment

✅ **DO**:
- Use Qdrant Cloud or HA cluster
- Enable multi-tier caching
- Monitor metrics (latency, hit rate)
- Set up alerting
- Regular backups of Qdrant data

❌ **DON'T**:
- Run single Qdrant instance in production
- Ignore cache configuration
- Skip monitoring setup
- Forget disaster recovery plan

---

## Migration & Maintenance

### Upgrading Qdrant

```bash
# 1. Backup current data
docker exec qdrant \
  tar czf /qdrant/storage/backup.tar.gz /qdrant/storage

# 2. Stop old container
docker stop qdrant
docker rm qdrant

# 3. Start new version
docker run -d \
  --name qdrant \
  -p 6333:6333 \
  -v $(pwd)/qdrant_storage:/qdrant/storage \
  qdrant/qdrant:v1.12.0  # New version

# 4. Verify collections
curl http://localhost:6333/collections
```

### Re-indexing Documents

```bash
# When changing:
# - Embedding model
# - Chunk size/overlap
# - Vector dimension

# 1. Delete old collection
curl -X DELETE http://localhost:6333/collections/org_core_vectors

# 2. Re-index all documents
curl -X POST $BASE_URL/api/v1/rag/reindex \
  -H "X-Org-ID: $ORG_ID"

# 3. Verify results
curl -X POST $BASE_URL/api/v1/rag/retrieve \
  -H "X-Org-ID: $ORG_ID" \
  -d '{"query":"test","strategy":"hybrid"}'
```

### Database Maintenance

```bash
# Vacuum to reclaim space
docker exec coresystem-postgres-local \
  psql -U coresystem -d org_core -c "VACUUM ANALYZE rag_documents;"

# Re-index for performance
docker exec coresystem-postgres-local \
  psql -U coresystem -d org_core -c "REINDEX TABLE rag_retrievals;"

# Archive old metrics (>90 days)
docker exec coresystem-postgres-local \
  psql -U coresystem -d org_core -c \
  "DELETE FROM rag_retrievals WHERE created_at < NOW() - INTERVAL '90 days';"
```

---

## Additional Resources

### Documentation
- [Qdrant Documentation](https://qdrant.tech/documentation/)
- [LangGraph (Agentic RAG)](https://github.com/langchain-ai/langgraph)
- [Sentence Transformers](https://www.sbert.net/)

### Research Papers
- [RAG Survey 2024](https://arxiv.org/abs/2312.10997)
- [Reciprocal Rank Fusion](https://plg.uwaterloo.ca/~gvcormac/cormacksigir09-rrf.pdf)
- [Agentic RAG Patterns](https://arxiv.org/abs/2401.00812)

### Internal Docs
- [Multi-tier Cache Guide](../docs/CACHE_IMPLEMENTATION.md)
- [Linear Integration](../ai-core/docs/LINEAR_INTEGRATION.md)
- [Deployment Guide](../docs/DEPLOYMENT_GUIDE.md)

---

**Document Version**: 2.0  
**Last Updated**: February 2, 2026  
**Maintained By**: Org-core Team

For questions or issues, consult the troubleshooting section or check recent logs.
