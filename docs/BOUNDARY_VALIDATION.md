# Multi-Plane Boundary Validation

**Last Updated:** February 19, 2026  
**Purpose:** Validate that all planned services have clear, non-overlapping boundaries

## Validation Methodology

For each service, we check:
1. ✅ **Clear Responsibility:** What it DOES and DOES NOT do
2. ✅ **Data Ownership:** What data it owns (if any)
3. ✅ **No Overlaps:** No duplicate responsibilities with other services
4. ✅ **API Contracts:** Well-defined inputs/outputs
5. ✅ **Communication:** Event-driven, no tight coupling

---

## CONTROL PLANE Boundaries ✅

### Principle: "Who can do what?"
Control plane manages **identity and permissions metadata only** - never data content.

### 1. auth-service ✅ CLEAR BOUNDARY
**Owns:**
- User credentials (hashed passwords, OAuth tokens)
- Session data (JWT tokens, session IDs)
- Organization membership (user-org relationships)

**Does:**
- ✅ Authenticate users (login, OAuth)
- ✅ Issue JWT tokens
- ✅ Manage sessions
- ✅ Publish auth events (user.created, session.started)

**Does NOT:**
- ❌ Store user profiles (user-service owns)
- ❌ Store documents (data plane owns)
- ❌ Make authorization decisions (security plane evaluates)
- ❌ Check quotas (org-service provides)

**Boundary Status:** ✅ **CLEAR** - Pure authentication, no data ownership

---

### 2. user-service ✅ CLEAR BOUNDARY
**Owns:**
- User profile data (name, email, preferences)
- API keys metadata
- User preferences

**Does:**
- ✅ Manage user profiles (CRUD)
- ✅ Generate and manage API keys
- ✅ Store user preferences
- ✅ Provide user context to other services

**Does NOT:**
- ❌ Authenticate users (auth-service does)
- ❌ Store organization data (org-service owns)
- ❌ Store documents (data plane owns)
- ❌ Execute AI operations (reasoning plane)

**Boundary Status:** ✅ **CLEAR** - User metadata only, no auth or data

---

### 3. org-service ✅ CLEAR BOUNDARY (After Data Plane Extraction)
**Owns:**
- Organization metadata (name, plan, settings)
- Entitlements (feature flags per org)
- Quota limits (NOT enforcement)

**Does:**
- ✅ Manage organization metadata
- ✅ Define entitlements (what features org has)
- ✅ Set quota limits (retrieval limit, document limit)
- ✅ Provide policy decisions (can this org access feature X?)

**Does NOT:**
- ❌ Store documents (data plane: document-service)
- ❌ Execute retrieval (data plane: rag-service)
- ❌ Enforce quotas (security plane checks)
- ❌ Own vectors (data plane: vector-service)

**⚠️ TRANSITION REQUIRED:**
- Currently owns RAG operations → **Must extract to data plane**
- Currently has Qdrant client → **Must remove, use vector-service**
- Currently has in-memory vectors → **Must delete**

**Boundary Status:** ✅ **CLEAR AFTER PHASE 2** - Control plane only (metadata, quotas, policies)

---

## APPLICATION PLANE Boundaries ✅

### Principle: "Product experience and real-time UI orchestration"
Application plane provides **product-facing experiences and real-time UI orchestration** - never the source of truth, always a reactive composition of backend state tailored for frontend consumption.

### 4. convex-core ✅ CLEAR BOUNDARY
**Owns:**
- Conversation state (chat messages, typing indicators)
- UI projections (reactive mirrors of backend state)
- Live progress updates (crawl progress, job status)
- Session-scoped temporary caches
- Real-time WebSocket connections

**Does:**
- ✅ Validate JWT from auth-service (never trusts frontend orgId)
- ✅ Verify org membership and capabilities
- ✅ Store chat messages and conversation history
- ✅ Sync typing indicators and presence
- ✅ Mirror crawl/job progress for live UI updates
- ✅ Cache UI preferences (temporary, non-authoritative)
- ✅ Provide real-time subscriptions to frontend
- ✅ Accept backend state updates via events

**Does NOT:**
- ❌ Act as source of truth for org ownership
- ❌ Define or enforce plan tiers
- ❌ Set or check quotas (reads from org-core)
- ❌ Enforce capability rules (validates via auth-service)
- ❌ Store canonical billing data
- ❌ Own vector index metadata
- ❌ Define policy rules
- ❌ Accept orgId from frontend as authority
- ❌ Store permanent business data

**Security:**
- ✅ **MUST** validate JWT on ALL mutations
- ✅ **MUST** check org membership via auth-service
- ✅ **MUST** verify capabilities via org-core
- ❌ **NEVER** trust orgId from frontend
- ❌ **NEVER** bypass backend validation

**Data Flow:**
```
Backend (Source of Truth) → Event → Convex (Reactive Mirror) → Frontend (Real-time UI)
Frontend → Convex validates JWT → Backend (Authority) → Convex (Updated Mirror)
```

**Dependencies:**
- Calls auth-service (JWT validation, org membership)
- Calls org-core (capability checks)
- Subscribes to backend events (crawl.progress, job.status, etc.)
- Provides WebSocket subscriptions to frontend

**Boundary Status:** ✅ **CLEAR** - Reactive UI state layer, NOT canonical storage

---

### 5. product-bff (Backend for Frontend) ✅ CLEAR BOUNDARY
**Owns:**
- Product-specific API aggregation logic
- UI-optimized response transformations
- Client-specific caching strategies
- GraphQL/REST API composition

**Does:**
- ✅ Aggregate multiple backend service calls for single frontend request
- ✅ Transform backend responses to UI-friendly formats
- ✅ Implement product-specific API contracts (mobile, web, desktop)
- ✅ Cache aggregated responses (Redis, short TTL)
- ✅ Handle pagination, filtering, sorting for UI
- ✅ Validate JWT and enforce client-specific rate limits

**Does NOT:**
- ❌ Store canonical data (reads from backend services)
- ❌ Make business logic decisions (delegates to backend)
- ❌ Execute retrieval or AI operations (calls appropriate plane)
- ❌ Bypass security validation (always validates with auth-service)
- ❌ Accept orgId from frontend without validation

**Dependencies:**
- Calls auth-service (JWT validation)
- Calls org-service (entitlements, quotas)
- Calls data plane services (document-service, rag-service)
- Calls reasoning plane services (ai-core, agent-service)
- Calls ingestion plane services (crawler-service, import-service)

**Boundary Status:** ✅ **CLEAR** - API aggregation and transformation only

---

### 6. session-orchestrator ✅ CLEAR BOUNDARY
**Owns:**
- Active session state (in-memory or Redis)
- Session lifecycle management
- Multi-tab coordination state
- Session-scoped feature flags

**Does:**
- ✅ Manage active user sessions (create, refresh, invalidate)
- ✅ Coordinate state across multiple browser tabs/devices
- ✅ Track session-scoped feature flags and experiments
- ✅ Provide session health checks and heartbeat
- ✅ Implement session timeout and renewal logic
- ✅ Publish session.started, session.ended events

**Does NOT:**
- ❌ Store permanent user data (user-service owns)
- ❌ Authenticate users (auth-service does)
- ❌ Store conversation history permanently (convex-core mirrors)
- ❌ Make authorization decisions (authz-service validates)

**Dependencies:**
- Calls auth-service (JWT validation, token refresh)
- Calls convex-core (sync session state for real-time UI)
- Publishes events → audit-service logs

**Boundary Status:** ✅ **CLEAR** - Session lifecycle only, ephemeral state

---

### 7. feature-composer ✅ CLEAR BOUNDARY
**Owns:**
- Feature flag evaluation logic
- A/B test variant assignment
- Feature composition rules
- Client-side configuration cache

**Does:**
- ✅ Evaluate feature flags per user/org
- ✅ Assign A/B test variants consistently
- ✅ Compose feature sets based on entitlements
- ✅ Provide feature availability API for frontend
- ✅ Cache feature evaluations (Redis, TTL-based)
- ✅ Track feature usage metrics

**Does NOT:**
- ❌ Define entitlements (org-service owns)
- ❌ Store permanent feature definitions (config service owns)
- ❌ Execute business logic (only feature evaluation)
- ❌ Make billing decisions (org-service defines)

**Dependencies:**
- Calls org-service (entitlements, plan tier)
- Calls config-service (feature flag definitions)
- Publishes events → observability plane (feature usage)

**Boundary Status:** ✅ **CLEAR** - Feature evaluation and composition only

---

### 8. realtime-streaming-layer ✅ CLEAR BOUNDARY
**Owns:**
- WebSocket connection pool management
- Stream multiplexing logic
- Server-Sent Events (SSE) channels
- Stream backpressure handling

**Does:**
- ✅ Manage persistent WebSocket connections
- ✅ Multiplex multiple backend streams to single frontend connection
- ✅ Provide SSE endpoints for streaming responses
- ✅ Handle connection lifecycle (connect, disconnect, reconnect)
- ✅ Implement backpressure and flow control
- ✅ Route real-time updates from backend to correct clients

**Does NOT:**
- ❌ Store streaming data permanently (ephemeral only)
- ❌ Generate AI responses (reasoning plane does)
- ❌ Execute retrieval (data plane does)
- ❌ Bypass authentication (validates JWT on connect)

**Dependencies:**
- Receives streams from ai-core-service (LLM streaming)
- Receives updates from convex-core (real-time state)
- Receives progress from crawler-service, import-service
- Validates with auth-service on connection

**Boundary Status:** ✅ **CLEAR** - Stream multiplexing only, no business logic

---

### 9. notification-service ✅ CLEAR BOUNDARY
**Owns:**
- Notification delivery state (sent/pending/failed)
- User notification preferences
- Delivery channel routing (email, push, in-app)
- Notification templates

**Does:**
- ✅ Route notifications to appropriate channels
- ✅ Manage user notification preferences
- ✅ Render notification templates
- ✅ Track delivery status (sent, read, dismissed)
- ✅ Implement rate limiting per user
- ✅ Batch and deduplicate notifications
- ✅ Publish notification.sent, notification.read events

**Does NOT:**
- ❌ Decide WHAT to notify (backend services trigger events)
- ❌ Store application data (only notification metadata)
- ❌ Make business logic decisions
- ❌ Bypass user preferences (always respects opt-out)

**Dependencies:**
- Subscribes to backend events (crawl.completed, import.finished, etc.)
- Calls external services (SendGrid, Twilio, Firebase Push)
- Calls user-service (notification preferences)

**Boundary Status:** ✅ **CLEAR** - Notification delivery orchestration only

---

### 10. usage-projections ✅ CLEAR BOUNDARY
**Owns:**
- Real-time usage counters (Redis)
- Projected quota consumption
- Usage trend calculations
- Client-facing usage dashboards cache

**Does:**
- ✅ Track real-time API usage per org/user
- ✅ Project quota consumption trends
- ✅ Provide usage dashboard data to frontend
- ✅ Calculate cost estimates based on usage
- ✅ Alert when approaching quota limits
- ✅ Cache aggregated usage metrics

**Does NOT:**
- ❌ Define quota limits (org-service owns)
- ❌ Enforce quotas (security plane validates)
- ❌ Store canonical billing data (billing service owns)
- ❌ Make billing decisions (reads only)

**Dependencies:**
- Subscribes to usage events from all planes
- Calls org-service (quota limits)
- Publishes usage.approaching_limit events
- Provides metrics → observability plane

**Boundary Status:** ✅ **CLEAR** - Usage projection and dashboard only

---

## DATA PLANE Boundaries ✅

### Principle: "What data exists?"
Data plane owns **all document content and vectors** - never makes policy or AI decisions.

### 11. document-service ✅ CLEAR BOUNDARY
**Owns:**
- Document content (files in S3/MinIO)
- Document metadata (filename, size, owner, created_at)
- Document versions

**Does:**
- ✅ Store documents in S3/MinIO
- ✅ Store metadata in PostgreSQL
- ✅ Provide document CRUD operations
- ✅ Parse files (PDF, DOCX, TXT, etc.)
- ✅ Publish document.uploaded, document.indexed events

**Does NOT:**
- ❌ Generate embeddings (embedding-service does)
- ❌ Index vectors (vector-service does)
- ❌ Check quotas (asks org-service gRPC)
- ❌ Make authorization decisions (security plane)
- ❌ Execute retrieval (rag-service does)

**Dependencies:**
- Calls org-service (gRPC) to check quota before upload
- Publishes event → embedding-service subscribes

**Boundary Status:** ✅ **CLEAR** - Document storage only, quota-checked

---

### 12. embedding-service ✅ CLEAR BOUNDARY
**Owns:**
- Embedding generation logic
- Model selection strategy
- Embedding cache (Redis)

**Does:**
- ✅ Generate text embeddings (OpenAI, Cohere, custom)
- ✅ Select optimal embedding model
- ✅ Batch embedding requests for efficiency
- ✅ Cache frequently embedded text
- ✅ Publish embedding.generated events

**Does NOT:**
- ❌ Store documents (document-service owns)
- ❌ Index vectors (vector-service does)
- ❌ Execute retrieval (rag-service does)
- ❌ Make business decisions

**Dependencies:**
- Subscribes to document.uploaded event
- Calls external APIs (OpenAI, Cohere)
- Publishes event → vector-service subscribes

**Boundary Status:** ✅ **CLEAR** - Pure embedding generation, no data storage

---

### 13. vector-service ✅ CLEAR BOUNDARY
**Owns:**
- Qdrant client wrapper
- Collection management
- Vector index operations

**Does:**
- ✅ Wrap Qdrant SDK
- ✅ Manage collections (create, delete, info)
- ✅ Upsert/delete/search points
- ✅ Apply faceted filters
- ✅ Manage payload indexing

**Does NOT:**
- ❌ Generate embeddings (embedding-service does)
- ❌ Store document content (document-service owns)
- ❌ Execute business logic
- ❌ Make policy decisions

**Dependencies:**
- Subscribes to embedding.generated event
- Wraps Qdrant (infrastructure)

**Boundary Status:** ✅ **CLEAR** - Pure Qdrant wrapper, no business logic

---

### 14. rag-service ✅ CLEAR BOUNDARY
**Owns:**
- Retrieval query logic
- Ranking algorithms
- Query expansion strategies
- Retrieval result cache (Redis)

**Does:**
- ✅ Execute semantic search queries
- ✅ Hybrid search (keyword + vector)
- ✅ Query expansion and reformulation
- ✅ Rank and score results
- ✅ Check quota before retrieval (org-service gRPC)
- ✅ Publish retrieval.completed events

**Does NOT:**
- ❌ Generate embeddings (embedding-service does)
- ❌ Store documents (document-service owns)
- ❌ Own vectors (vector-service manages)
- ❌ Execute AI reasoning (reasoning plane)
- ❌ Set quota limits (org-service defines)

**Dependencies:**
- Calls embedding-service to generate query embedding
- Calls vector-service to search vectors
- Calls document-service to fetch document metadata
- Calls org-service (gRPC) to check retrieval quota

**Boundary Status:** ✅ **CLEAR** - Retrieval orchestration, quota-enforced

---

## REASONING PLANE Boundaries ✅

### Principle: "How do we think?"
Reasoning plane executes **stateless AI operations** - never owns data.

### 15. ai-core-service ✅ CLEAR BOUNDARY (After Refactor)
**Owns:**
- LLM orchestration logic
- Prompt templates
- Model selection strategy
- Response cache (Redis)

**Does:**
- ✅ Orchestrate LLM calls (OpenAI, Anthropic, Cohere)
- ✅ Manage prompt templates
- ✅ Select optimal model for task
- ✅ Stream responses
- ✅ Track token usage and costs

**Does NOT:**
- ❌ Store documents (data plane owns)
- ❌ Generate embeddings (embedding-service does)
- ❌ Execute agents (agent-service does)
- ❌ Rerank results (rerank-service does)
- ❌ Own in-memory vectors ⚠️ **Must remove**

**⚠️ REFACTOR REQUIRED:**
- Remove document storage logic
- Remove embedding generation
- Remove in-memory vector store
- Remove deprecated endpoints

**Boundary Status:** ✅ **CLEAR AFTER PHASE 3** - Pure LLM orchestration

---

### 16. agent-service ✅ CLEAR BOUNDARY
**Owns:**
- Agent execution engine
- Tool registry
- Agent memory (short-term, session-based)
- Conversation context

**Does:**
- ✅ Execute agents with tools
- ✅ Register and call tools
- ✅ Manage agent memory
- ✅ Track conversation context
- ✅ Coordinate multi-agent workflows
- ✅ Publish agent.execution_started, agent.execution_completed

**Does NOT:**
- ❌ Store documents permanently (calls data plane)
- ❌ Generate embeddings (calls embedding-service)
- ❌ Execute retrieval directly (calls rag-service)
- ❌ Synthesize final response (synthesis-service does)

**Dependencies:**
- Calls ai-core-service for LLM operations
- Calls rag-service for retrieval
- Calls synthesis-service for response formatting
- Uses Temporal for long-running agent workflows

**Boundary Status:** ✅ **CLEAR** - Agent orchestration, no permanent data

---

### 17. rerank-service ✅ CLEAR BOUNDARY
**Owns:**
- Reranking algorithms
- Relevance scoring models
- Rerank result cache (Redis)

**Does:**
- ✅ Rerank retrieval results using LLM
- ✅ Cross-encoder scoring
- ✅ Hybrid relevance scoring
- ✅ Diversity-based reranking
- ✅ Cache rerank results

**Does NOT:**
- ❌ Execute retrieval (rag-service does)
- ❌ Store documents (data plane owns)
- ❌ Generate final response (synthesis-service does)

**Dependencies:**
- Receives candidates from rag-service
- Calls ai-core-service or external APIs (Cohere Rerank)

**Boundary Status:** ✅ **CLEAR** - Pure reranking, no data ownership

---

### 18. synthesis-service ✅ CLEAR BOUNDARY
**Owns:**
- Response generation logic
- Citation formatting
- Fact verification pipeline

**Does:**
- ✅ Generate answers from retrieved documents
- ✅ Format citations
- ✅ Stream responses to frontend
- ✅ Verify facts against sources
- ✅ Detect hallucinations
- ✅ Publish synthesis.started, synthesis.completed

**Does NOT:**
- ❌ Store documents (data plane owns)
- ❌ Execute retrieval (rag-service does)
- ❌ Execute agents (agent-service does)

**Dependencies:**
- Calls ai-core-service for LLM operations
- Receives reranked results from rerank-service

**Boundary Status:** ✅ **CLEAR** - Response synthesis only

---

## INGESTION PLANE Boundaries ✅

### Principle: "How does data arrive?"
Ingestion plane **transforms external data** - never owns permanent storage.

### 19. query-service ✅ CLEAR BOUNDARY
**Owns:**
- Query validation rules
- Spell correction dictionary
- Synonym mappings
- Query history (short-term)

**Does:**
- ✅ Validate and sanitize queries
- ✅ Classify query intent
- ✅ Spell correction
- ✅ Synonym expansion
- ✅ Rate limit queries (Redis)

**Does NOT:**
- ❌ Execute retrieval (rag-service does)
- ❌ Store documents (data plane owns)
- ❌ Execute AI operations (reasoning plane)

**Boundary Status:** ✅ **CLEAR** - Query preprocessing only

---

### 20. import-service ✅ CLEAR BOUNDARY
**Owns:**
- Import job state (PostgreSQL)
- File parsing logic
- Job progress tracking

**Does:**
- ✅ Accept bulk file uploads
- ✅ Parse files (PDF, DOCX, TXT, etc.)
- ✅ Extract content and metadata
- ✅ Manage batch jobs (NATS/Temporal)
- ✅ Track progress and notify (SSE)
- ✅ Publish import.started, import.completed

**Does NOT:**
- ❌ Store documents permanently (calls document-service)
- ❌ Generate embeddings (embedding-service does)
- ❌ Index vectors (vector-service does)
- ❌ Make quota decisions (asks org-service)

**Dependencies:**
- Calls document-service to store each document
- document-service → embedding-service → vector-service (event chain)

**Boundary Status:** ✅ **CLEAR** - Temporary job state only, delegates storage

---

### 21. integration-service ✅ CLEAR BOUNDARY
**Owns:**
- OAuth tokens (encrypted in PostgreSQL)
- Sync cursors (for incremental sync)
- Integration job state

**Does:**
- ✅ Manage OAuth flows
- ✅ Sync external data (Notion, Google Drive, Slack)
- ✅ Incremental sync (only changes)
- ✅ Handle webhooks for real-time updates
- ✅ Transform external formats to internal schema
- ✅ Publish integration.synced events

**Does NOT:**
- ❌ Store documents permanently (calls document-service)
- ❌ Generate embeddings (embedding-service does)
- ❌ Execute AI operations (reasoning plane)

**Dependencies:**
- Calls external APIs (Notion, Google Drive, etc.)
- Calls document-service to store synced content

**Boundary Status:** ✅ **CLEAR** - Sync coordination only, delegates storage

---

### 22. crawler-service ✅ CLEAR BOUNDARY
**Owns:**
- Crawl job state (PostgreSQL)
- Crawled URL tracking (deduplication)
- Crawl rate limiting state

**Does:**
- ✅ Crawl websites and sitemaps
- ✅ Parse robots.txt
- ✅ Render JavaScript (Playwright)
- ✅ Extract main content
- ✅ Detect duplicates
- ✅ Publish crawl.started, crawl.page_crawled

**Does NOT:**
- ❌ Store documents permanently (calls document-service)
- ❌ Generate embeddings (embedding-service does)
- ❌ Execute AI operations (reasoning plane)

**Dependencies:**
- Calls document-service to store crawled pages

**Boundary Status:** ✅ **CLEAR** - Crawl coordination only, delegates storage

---

## ORCHESTRATION PLANE Boundaries ✅

### Principle: "How do we coordinate?"
Orchestration plane **coordinates workflows** - never executes business logic directly.

### 23. workflow-service ✅ CLEAR BOUNDARY
**Owns:**
- Workflow API endpoints
- Workflow trigger logic

**Does:**
- ✅ Provide HTTP API to start workflows
- ✅ Query workflow status
- ✅ Send signals to workflows
- ✅ Cancel/terminate workflows
- ✅ List workflow executions

**Does NOT:**
- ❌ Execute business logic (workflows call other services)
- ❌ Store application data (only workflow state in Temporal)
- ❌ Make policy decisions

**Dependencies:**
- Wraps Temporal SDK

**Boundary Status:** ✅ **CLEAR** - Workflow API wrapper only

---

### 24. Temporal Workers ✅ CLEAR BOUNDARY
**Owns:**
- Workflow definitions (code)
- Activity implementations

**Does:**
- ✅ Execute workflow steps
- ✅ Coordinate multi-service operations
- ✅ Handle retries and compensation
- ✅ Manage saga patterns

**Does NOT:**
- ❌ Own business data (calls services for data operations)
- ❌ Execute AI directly (calls reasoning plane)
- ❌ Make policy decisions (asks control plane)

**Dependencies:**
- Calls all planes as needed (document-service, rag-service, agent-service, etc.)

**Boundary Status:** ✅ **CLEAR** - Coordination only, no business logic

---

## OBSERVABILITY PLANE Boundaries ✅

### Principle: "How do we monitor?"
Observability plane is **read-only with async writes** - never blocks requests.

### 25. audit-service ✅ CLEAR BOUNDARY
**Owns:**
- Audit logs (PostgreSQL with TimescaleDB)
- Compliance reports

**Does:**
- ✅ Store all sensitive operations
- ✅ Provide compliance queries (GDPR, SOC2)
- ✅ Track data lineage
- ✅ Generate audit reports
- ✅ Ensure tamper-proof logging

**Does NOT:**
- ❌ Make authorization decisions (security plane does)
- ❌ Store application data (data plane owns)
- ❌ Block requests (async write-only)

**Boundary Status:** ✅ **CLEAR** - Audit logging only, non-blocking

---

### 26. evaluation-service ✅ CLEAR BOUNDARY
**Owns:**
- Evaluation metrics (PostgreSQL)
- Human feedback (thumbs up/down, ratings)
- Benchmark results

**Does:**
- ✅ Evaluate retrieval quality (precision@k, recall@k)
- ✅ Evaluate generation quality (faithfulness, relevance)
- ✅ Collect human feedback
- ✅ Run automated evaluations
- ✅ Track quality over time

**Does NOT:**
- ❌ Execute retrieval (rag-service does)
- ❌ Execute AI operations (reasoning plane)
- ❌ Make real-time decisions (evaluation is post-hoc)

**Boundary Status:** ✅ **CLEAR** - Evaluation only, read-only analysis

---

## SECURITY PLANE Boundaries ✅

### Principle: "How do we protect?"
Security plane **enforces policies** - never makes policy decisions (control plane does).

### 27. authz-service ✅ CLEAR BOUNDARY
**Owns:**
- Authorization policy engine (OPA)
- Policy evaluation cache (Redis)

**Does:**
- ✅ Evaluate authorization policies
- ✅ Check resource-level permissions
- ✅ Support RBAC and ABAC
- ✅ Cache policy decisions
- ✅ Publish authz.denied events (for audit)

**Does NOT:**
- ❌ Authenticate users (auth-service does)
- ❌ Define policies (org-service provides entitlements)
- ❌ Store application data
- ❌ Execute business logic

**Dependencies:**
- Receives user context from auth-service
- Receives entitlements from org-service
- Publishes events → audit-service logs

**Boundary Status:** ✅ **CLEAR** - Policy evaluation only, no decisions

---

### 28. encryption-service ✅ CLEAR BOUNDARY
**Owns:**
- Encryption/decryption logic
- Key management (DEK/KEK hierarchy)

**Does:**
- ✅ Encrypt data at rest
- ✅ Decrypt data for authorized services
- ✅ Manage encryption keys (with Vault)
- ✅ Rotate keys
- ✅ Envelope encryption (DEK encrypted by KEK)

**Does NOT:**
- ❌ Store application data (data plane owns)
- ❌ Make authorization decisions (authz-service does)
- ❌ Authenticate users

**Dependencies:**
- Uses Vault for KEK storage
- Called by document-service before S3 upload

**Boundary Status:** ✅ **CLEAR** - Encryption only, no data ownership

---

## Cross-Cutting Concerns ✅

### Infrastructure Services (Not Application Logic)
- **Prometheus:** Metrics collection (cross-cutting)
- **Grafana:** Dashboards (cross-cutting)
- **Jaeger:** Distributed tracing (cross-cutting)
- **Vault:** Secrets management (security plane infra)
- **OPA:** Policy engine (security plane infra)
- **Temporal:** Workflow engine (orchestration plane infra)

**Boundary Status:** ✅ **CLEAR** - Infrastructure only, no business logic

---

## Boundary Violation Analysis

### ❌ VIOLATIONS FOUND: 2

#### 1. org-service Currently Violates Data Plane ⚠️
**Problem:** org-service currently owns:
- Document storage and retrieval
- RAG operations
- Qdrant vector operations
- In-memory vector store

**Solution (Phase 2):**
- Extract to document-service
- Extract to rag-service
- Extract to vector-service
- org-service becomes control plane only (quotas, entitlements)

**Status:** ⚠️ **MUST FIX IN PHASE 2** (Weeks 3-7)

#### 2. ai-service Currently Violates Data Plane ⚠️
**Problem:** ai-service currently:
- Generates embeddings (should be embedding-service)
- Has in-memory vector store (should not exist)
- Has deprecated document endpoints

**Solution (Phase 3):**
- Extract embedding to embedding-service
- Delete in-memory vector store
- Remove deprecated endpoints
- ai-core-service becomes stateless LLM orchestration only

**Status:** ⚠️ **MUST FIX IN PHASE 3** (Weeks 8-12)

---

## Dependency Graph Validation

### ✅ NO CIRCULAR DEPENDENCIES DETECTED

```
Control Plane (auth, user, org)
    ↓
    ├─→ Security Plane (authz, encryption) [policy enforcement]
    ↓
Application Plane (Product Layer)
    ├─→ convex-core (real-time state sync)
    ├─→ product-bff (API aggregation for frontend)
    ├─→ session-orchestrator (session lifecycle)
    ├─→ feature-composer (feature flag evaluation)
    ├─→ realtime-streaming-layer (WebSocket multiplexing)
    ├─→ notification-service (multi-channel delivery)
    └─→ usage-projections (quota trend forecasting)
    │
    ├─→ Validates with Control Plane (auth, org)
    ├─→ Aggregates from Data/Reasoning/Ingestion Planes
    └─→ Provides optimized APIs to Frontend
    ↓
Data Plane (document, rag, embedding, vector)
    ↓
    ├─→ Reasoning Plane (ai-core, agent, rerank, synthesis)
    ↓
Ingestion Plane (query, import, integration, crawler)
    ↓
    └─→ Data Plane [stores imported data]

Orchestration Plane (Temporal, workflows)
    └─→ Coordinates all planes

Observability Plane (metrics, tracing, audit, evaluation)
    └─→ Observes all planes (non-blocking)
```

### Dependency Rules
✅ Control Plane → No dependencies on other application planes  
✅ Application Plane → Validates with Control Plane, aggregates from all backend planes  
✅ Data Plane → Can ask Control Plane for quotas (gRPC)  
✅ Reasoning Plane → Calls Data Plane for retrieval  
✅ Ingestion Plane → Calls Data Plane to store  
✅ Orchestration → Coordinates all planes  
✅ Observability → Observes all planes (async)  
✅ Security → Enforces policies from Control Plane  

---

## Data Ownership Matrix

| **Data Type** | **Owner** | **Boundary Status** |
|---------------|-----------|---------------------|
| User credentials | auth-service | ✅ Clear |
| User profiles | user-service | ✅ Clear |
| Org metadata | org-service | ✅ Clear (after Phase 2) |
| Documents | document-service | ✅ Clear |
| Vectors | vector-service | ✅ Clear |
| Embeddings | (ephemeral) embedding-service | ✅ Clear |
| Audit logs | audit-service | ✅ Clear |
| Evaluation data | evaluation-service | ✅ Clear |
| OAuth tokens | integration-service | ✅ Clear (encrypted) |
| Encryption keys | Vault | ✅ Clear |
| Workflow state | Temporal | ✅ Clear |

### ✅ NO OVERLAPPING DATA OWNERSHIP

---

## API Contract Validation

### Sample Cross-Plane Flow: RAG Query
```
1. Frontend → query-service (Ingestion Plane)
   └─→ Validates query, expands synonyms
   
2. query-service → authz-service (Security Plane)
   └─→ Check: Can user access RAG?
   
3. authz-service → org-service (Control Plane)
   └─→ Get user entitlements
   
4. If authorized → rag-service (Data Plane)
   └─→ Execute retrieval
   
5. rag-service → org-service (Control Plane)
   └─→ Check quota before retrieval
   
6. rag-service → embedding-service (Data Plane)
   └─→ Generate query embedding
   
7. rag-service → vector-service (Data Plane)
   └─→ Semantic search
   
8. rag-service → rerank-service (Reasoning Plane)
   └─→ Rerank results
   
9. rerank-service → synthesis-service (Reasoning Plane)
   └─→ Generate response with citations
   
10. Response → Frontend
    └─→ Stream synthesized answer
    
11. Backend → convex-core (Application Plane)
    └─→ Publish query event for real-time chat history (async)
    
12. convex-core → Frontend (WebSocket)
    └─→ Real-time UI update (conversation state)
    
13. audit-service (Observability Plane)
    └─→ Log query for compliance (async)
```

### Sample Cross-Plane Flow: Live Crawl Progress
```
1. User starts crawl → crawler-service (Ingestion Plane)
   └─→ Creates job in PostgreSQL (canonical state)
   
2. crawler-service → convex-core (Application Plane)
   └─→ Publishes crawl.started event
   
3. convex-core validates:
   └─→ JWT from frontend
   └─→ org membership via auth-service
   └─→ Stores initial state (reactive mirror)
   
4. Crawler processes pages → emits crawl.page_crawled events
   └─→ Backend updates PostgreSQL (source of truth)
   
5. Backend → convex-core (Application Plane)
   └─→ Publishes progress updates
   
6. convex-core → Frontend (WebSocket)
   └─→ Real-time progress bar updates
   
7. Crawl completes → Backend
   └─→ Updates PostgreSQL (canonical state)
   
8. Backend → convex-core
   └─→ Publishes crawl.completed
   
9. convex-core → Frontend
   └─→ Real-time UI transition to "completed"
```

### ✅ All calls have well-defined contracts (REST/gRPC/Events)

---

## Event-Driven Communication Validation

### ✅ Event Flows are Acyclic

```
document.uploaded (document-service)
    ↓
embedding.generated (embedding-service)
    ↓
vector.indexed (vector-service)
    ↓
document.indexed (document-service - status update)
    ↓
audit.logged (audit-service)
```

**No circular event dependencies detected.**

---

## Final Validation Summary

### ✅ Boundaries Are Clear

| **Criterion** | **Status** | **Notes** |
|--------------|-----------|-----------|
| Clear responsibilities | ✅ PASS | All services have DOES/DOES NOT |
| No data overlap | ✅ PASS | Each data type has single owner |
| No circular deps | ✅ PASS | Dependency graph is acyclic |
| Well-defined APIs | ✅ PASS | REST/gRPC/Events documented |
| Event-driven | ✅ PASS | Async communication via NATS |
| Separation of concerns | ✅ PASS | Planes have distinct responsibilities |

### ⚠️ 2 Violations to Fix

1. **org-service** - Extract data operations (Phase 2)
2. **ai-service** - Remove data ownership (Phase 3)

### ✅ Overall Verdict: **ARCHITECTURE IS SOUND**

The multi-plane architecture has clear boundaries. The 2 violations are:
1. **Known** (already documented in roadmap)
2. **Planned to fix** (Phases 2 and 3)
3. **Transitional** (migration from old architecture)

Once Phases 2 and 3 are complete, the system will have **zero boundary violations**.

---

## Recommendations

### 1. Enforce Boundaries During Development
- [ ] Add linting rules to prevent cross-plane data access
- [ ] Code review checklist for boundary violations
- [ ] Integration tests that verify boundaries

### 2. Monitor Boundaries in Production
- [ ] Alert if service calls wrong plane
- [ ] Track event flow to detect cycles
- [ ] Measure cross-plane latency

### 3. Document Boundary Changes
- [ ] Any boundary change requires architecture review
- [ ] Update this document when boundaries change
- [ ] Version plane contracts

---

**Validation Date:** February 19, 2026  
**Next Review:** After Phase 2 Complete (Week 7)  
**Validated By:** Architecture Review  
**Status:** ✅ **APPROVED FOR IMPLEMENTATION**
