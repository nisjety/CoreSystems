# Phase 3: Intelligence Features

**Date**: February 1, 2026  
**Status**: In Progress

## Overview

Phase 3 focuses on advanced AI capabilities including workflow orchestration, enhanced RAG strategies, and intelligent context management.

---

## Features

### 1. Workflow Service (1-2 weeks) 🔥

**Purpose**: Multi-step AI workflow orchestration with state management

**Core Capabilities**:
- Sequential and parallel step execution
- Conditional branching and loops
- State persistence across steps
- Error handling and retry logic
- Workflow templates and composition
- Async execution with job queue integration

**Architecture**:
```
Workflow Definition (JSON/YAML)
    ↓
Workflow Engine (Execution)
    ↓
Step Executors (AI Core, RAG, External APIs)
    ↓
State Manager (Redis/PostgreSQL)
    ↓
Result Aggregator
```

**Use Cases**:
- Document processing pipeline: extract → classify → summarize → index
- Multi-agent workflows: research → analyze → synthesize → review
- Content generation: outline → draft → refine → publish
- Data enrichment: fetch → transform → validate → store

**Files to Create**:
```
backend/Org-core/internal/workflows/
  ├── engine.go          (300 lines) - Core execution engine
  ├── definition.go      (150 lines) - Workflow schema and validation
  ├── executor.go        (200 lines) - Step execution logic
  ├── state.go           (180 lines) - State management
  └── templates.go       (120 lines) - Pre-built workflow templates

backend/Org-core/internal/http/handlers/
  └── workflow_handler.go (250 lines) - HTTP API endpoints

backend/Org-core/migrations/
  └── 00X_workflows.up.sql (100 lines) - Database schema
```

**API Endpoints**:
```
POST   /api/v1/workflows              - Create workflow definition
GET    /api/v1/workflows              - List workflows
GET    /api/v1/workflows/:id          - Get workflow details
PUT    /api/v1/workflows/:id          - Update workflow
DELETE /api/v1/workflows/:id          - Delete workflow

POST   /api/v1/workflows/:id/execute  - Execute workflow
GET    /api/v1/workflows/:id/runs     - List execution runs
GET    /api/v1/workflows/runs/:run_id - Get run status
POST   /api/v1/workflows/runs/:run_id/cancel - Cancel run
```

**Workflow Definition Example**:
```json
{
  "name": "Document Processing Pipeline",
  "version": "1.0",
  "steps": [
    {
      "id": "extract",
      "type": "ai_core",
      "action": "document.analyze",
      "input": {"file": "{{input.document}}"},
      "output": "extracted_text"
    },
    {
      "id": "classify",
      "type": "ai_core",
      "action": "chat.completion",
      "input": {
        "prompt": "Classify this document: {{steps.extract.output}}",
        "model": "gpt-4o-mini"
      },
      "output": "category"
    },
    {
      "id": "index",
      "type": "rag",
      "action": "index.documents",
      "input": {
        "text": "{{steps.extract.output}}",
        "metadata": {"category": "{{steps.classify.output}}"}
      }
    }
  ]
}
```

---

### 2. Advanced RAG Strategies (1 week)

**Purpose**: Improve retrieval quality and relevance

**Enhancements**:
- **Hybrid Search**: Combine dense (vector) + sparse (BM25) retrieval
- **Reranking**: Cross-encoder model to reorder results
- **Query Expansion**: Generate multiple query variations
- **Chunk Optimization**: Smart chunking with context preservation
- **Metadata Filtering**: Enhanced filtering on structured metadata

**Files to Create**:
```
backend/Org-core/internal/rag/
  ├── reranker.go        (200 lines) - Reranking logic
  ├── query_expander.go  (150 lines) - Query expansion
  ├── hybrid_search.go   (180 lines) - Hybrid retrieval
  └── chunker_v2.go      (220 lines) - Advanced chunking

backend/Org-core/internal/rag/strategies/
  ├── strategy.go        (100 lines) - Strategy interface
  ├── default.go         (120 lines) - Default strategy
  └── hybrid.go          (150 lines) - Hybrid strategy
```

**Configuration**:
```yaml
rag:
  strategy: hybrid
  reranker:
    enabled: true
    model: cross-encoder/ms-marco-MiniLM-L-6-v2
    top_k: 100  # Initial retrieval
    final_k: 10 # After reranking
  
  query_expansion:
    enabled: true
    method: llm  # llm, synonyms, or paraphrase
    num_variations: 3
  
  chunking:
    method: semantic  # fixed, recursive, or semantic
    size: 512
    overlap: 50
    preserve_sentences: true
```

---

### 3. Context Window Optimization (3-4 days)

**Purpose**: Intelligent context management for long conversations

**Features**:
- **Summarization**: Compress old messages while preserving key information
- **Sliding Window**: Keep recent messages + summarized history
- **Semantic Pruning**: Remove low-relevance messages
- **Token Counting**: Accurate token estimation per model
- **Context Budget**: Allocate tokens across system/user/assistant/rag

**Files to Create**:
```
backend/Org-core/internal/sessions/
  ├── context_optimizer.go  (250 lines) - Core optimization logic
  ├── summarizer.go         (180 lines) - Message summarization
  ├── token_counter.go      (120 lines) - Token counting
  └── pruner.go             (150 lines) - Message pruning

backend/Org-core/internal/sessions/strategies/
  ├── sliding_window.go     (140 lines) - Sliding window strategy
  ├── semantic_prune.go     (160 lines) - Semantic pruning
  └── summarize_old.go      (170 lines) - Summarization strategy
```

**Strategy Example**:
```go
type ContextStrategy interface {
    Optimize(session *Session, tokenBudget int) (*OptimizedContext, error)
}

type OptimizedContext struct {
    SystemPrompt    string
    Messages        []Message
    RAGContext      string
    TokensUsed      int
    TokensAvailable int
    Summary         string  // Compressed history
}
```

**Configuration**:
```yaml
context_optimization:
  enabled: true
  strategy: sliding_window  # sliding_window, semantic_prune, summarize_old
  
  token_budget:
    gpt-4o: 128000
    gpt-4o-mini: 32000
    gpt-4-turbo: 128000
  
  allocation:
    system: 0.05      # 5% for system prompt
    rag: 0.20         # 20% for RAG context
    history: 0.65     # 65% for conversation
    completion: 0.10  # 10% for response
  
  sliding_window:
    keep_recent: 10        # Always keep last N messages
    summarize_every: 20    # Summarize every N messages
  
  semantic_prune:
    relevance_threshold: 0.5
    min_messages: 5
```

---

## Implementation Plan

### Week 1-2: Workflow Service
1. **Days 1-2**: Schema design, database migrations, core engine
2. **Days 3-4**: Step executors, state management
3. **Days 5-6**: HTTP API, workflow templates
4. **Days 7-8**: Testing, integration with job queue
5. **Days 9-10**: Documentation, examples

### Week 3: Advanced RAG
1. **Days 1-2**: Hybrid search implementation
2. **Days 3-4**: Reranking and query expansion
3. **Day 5**: Testing and benchmarks

### Week 4: Context Optimization
1. **Days 1-2**: Token counting, optimization strategies
2. **Days 3-4**: Integration with session manager, testing

---

## Success Metrics

### Workflow Service
- ✅ Execute 3-step workflow end-to-end
- ✅ Handle parallel step execution
- ✅ Persist state across service restarts
- ✅ Error handling with retries
- ✅ 5+ pre-built workflow templates

### Advanced RAG
- ✅ 20% improvement in retrieval relevance (nDCG@10)
- ✅ Hybrid search outperforms vector-only
- ✅ Reranking improves top-3 results
- ✅ Sub-200ms query latency (p95)

### Context Optimization
- ✅ Maintain 100+ message conversations
- ✅ Stay within token budgets (GPT-4o: 128K)
- ✅ Summarization preserves key information (human eval)
- ✅ No degradation in response quality

---

## Dependencies

### Workflow Service
- Job Queue System (Phase 2) ✅
- State storage (Redis + PostgreSQL)
- AI Core API integration

### Advanced RAG
- Qdrant (Phase 1) ✅
- Reranker model (HuggingFace)
- Query expansion LLM

### Context Optimization
- Session Manager (Phase 2) ✅
- Tokenization library (tiktoken)
- Summarization model (GPT-4o-mini)

---

## Notes

- Prompt Templates from Phase 2 can be reused in workflows
- Workflow executions leverage existing job queue infrastructure
- RAG improvements benefit all existing search/retrieval features
- Context optimization reduces LLM costs significantly (30-50%)
