# AI-Core Service Boundary Guidelines (Phase 3 Refactor)

**Version:** 1.0  
**Status:** ACTIVE - Refactoring in Progress  
**Last Updated:** February 19, 2026

## Purpose

This document defines the primary responsibilities of ai-core and identifies transitional patterns that violate the target architecture boundaries.

---

## Primary Responsibilities (What AI-Core SHOULD Own)

### 1. AI Orchestration
- Coordinate AI workflow execution
- Manage model inference requests
- Handle multi-step AI operations via Temporal

### 2. Response Processing
- Reranking (Cohere/other rerankers)
- Synthesis and formatting of AI responses
- Result validation and quality checks

### 3. AI-Specific Integrations
- Azure OpenAI client management
- Cohere integration for reranking
- Letta agent coordination (for memory/learning)
- Model-specific prompt engineering

### 4. Evaluation and Metrics
- AI performance tracking
- Token usage monitoring
- Quality metrics collection

---

## Boundary Violations (What AI-Core Should NOT Own)

### ❌ Document Storage Ownership
**Current Problem:**
- `_vector_store: dict[str, DocumentKnowledge]` in `document_knowledge_pipeline.py`
- In-memory storage of document embeddings and metadata
- Acting as source-of-truth for document state

**Target State:**
- AI-core should NOT own vector storage
- Document embeddings should be managed by data-plane service (org-core retrieval module)
- AI-core should REQUEST retrieval, not STORE documents

**Migration Path:**
1. Replace in-memory `_vector_store` with client calls to org-core retrieval API
2. Use policy-gated retrieval endpoint with proper quota checks
3. Remove document persistence logic from ai-core

---

### ❌ Direct Document Ingestion Routes
**Current Problem:**
- `POST /api/documents/ingest` - Document upload and processing
- `GET /api/documents/list` - Document listing
- `GET /api/documents/{id}` - Document retrieval

**Target State:**
- These routes create ambiguity about document ownership
- Ingestion should flow: `Frontend → Org-core (policy check) → AI-core (processing only)`
- AI-core should provide PROCESSING not STORAGE endpoints

**Migration Path:**
1. Mark `/api/documents/*` routes as DEPRECATED
2. Create new endpoints focused on PROCESSING: `POST /api/ai/process-document`, `POST /api/ai/analyze-document`
3. Document storage/retrieval remains in org-core

---

### ❌ Bypassing Policy Layer for Retrieval
**Current Problem:**
- Direct access to vector storage without quota validation
- No entitlement checks before expensive operations

**Target State:**
- All retrieval operations MUST go through org-core policy gate:
  ```
  AI-core → Org-core policy check → Retrieval → Response
  ```
- Quota enforcement for:
  - Token usage limits
  - Document ingestion limits
  - Retrieval request rate limits

**Migration Path:**
1. Create `PolicyClient` in ai-core to call org-core entitlement API
2. Enforce `check_entitlement()` before any retrieval operation
3. Add circuit breaker for quota exceeded scenarios

---

## Allowed Dependencies

### ✅ Safe to Call (With Policy Checks)
- Org-core retrieval API (via policy layer)
- Org-core entitlement/quota API
- User-service for user context (read-only)
- Temporal for workflow coordination

### ❌ Never Call Directly
- Qdrant (vector database) - must go through org-core
- Auth-service database - use events only
- User-service database - use gRPC/REST API

---

## Event Consumption (Safe)

AI-core SHOULD consume these events for reactive operations:

- `organization.quota_exceeded` → Throttle AI requests
- `organization.entitlement_updated` → Update feature access
- `organization.plan_changed` → Adjust AI capabilities

AI-core should NOT:
- Consume events to build its own copy of document/user state
- Use events as a substitute for API calls

---

## Refactor Checklist

### Phase 3 Immediate Actions

- [ ] Add DEPRECATED comments to `/api/documents/*` routes
- [ ] Create `PolicyClient` for org-core entitlement checks
- [ ] Replace `_vector_store` with `RetrievalClient` (calls org-core)
- [ ] Add boundary violation logging for storage-like operations
- [ ] Document transition plan for existing document routes

### Phase 3 Follow-up

- [ ] Implement new `/api/ai/process-document` endpoint (processing only)
- [ ] Migrate consumers from `/api/documents/ingest` to new endpoint
- [ ] Remove in-memory `_vector_store` after validation
- [ ] Add contract tests for policy-gated retrieval flow

---

## Code Examples

### Before (Violates Boundary)
```python
# WRONG: Direct storage ownership
self._vector_store[doc_id] = knowledge
embeddings = self._vector_store[doc_id].embeddings
```

### After (Follows Boundary)
```python
# CORRECT: Request retrieval through policy layer
retrieval_result = await self.retrieval_client.retrieve(
    org_id=org_id,
    query="relevant query",
    top_k=10,
    # Policy checks happen in org-core before returning results
)
embeddings = retrieval_result.embeddings
```

---

## Success Criteria

Phase 3 is complete when:

1. ✅ No direct vector storage in ai-core codebase
2. ✅ All retrieval operations go through org-core policy layer
3. ✅ Document routes marked DEPRECATED with migration plan
4. ✅ Policy client implemented with quota enforcement
5. ✅ Contract tests validate policy-gated flow

---

## FAQ

**Q: Why can't AI-core own document storage?**  
A: Ownership ambiguity creates data consistency issues. Org-core owns policy/quota enforcement, so it must control the data plane that policy applies to.

**Q: What if AI-core needs fast access to embeddings?**  
A: Use caching layers in org-core retrieval service. AI-core can request cached results via policy-gated API.

**Q: Can AI-core create temporary in-memory state during processing?**  
A: Yes, for in-flight processing only. Persistent state must go through proper storage service (org-core or dedicated document service when created).

---

## Owner

**Team:** AI/ML Platform  
**Reviewers:** Backend Architecture Team  
**Next Review:** After Phase 3 completion
