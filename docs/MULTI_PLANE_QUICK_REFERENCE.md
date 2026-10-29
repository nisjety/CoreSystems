# Multi-Plane Architecture - Quick Reference

**Last Updated:** February 19, 2026  
**Purpose:** One-page overview of all planes and services

---

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────┐
│                     FRONTEND CLIENTS                         │
│              (Web, Mobile, Desktop, CLI)                     │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ↓
┌─────────────────────────────────────────────────────────────┐
│                   APPLICATION PLANE                          │
│   (Product UX, Real-time, API Aggregation)                   │
│   - convex-core (WebSocket state sync)                       │
│   - product-bff (API aggregation)                            │
│   - session-orchestrator (session lifecycle)                 │
│   - feature-composer (feature flags)                         │
│   - realtime-streaming-layer (WebSocket mux)                 │
│   - notification-service (multi-channel)                     │
│   - usage-projections (quota forecasting)                    │
└────────────────────────┬────────────────────────────────────┘
                         │
                         ↓
┌─────────────────────────────────────────────────────────────┐
│                    CONTROL PLANE                             │
│   (Identity, Permissions, Org Management)                    │
│   - auth-service (authentication, JWT)                       │
│   - user-service (user profiles)                             │
│   - org-service (org metadata, quotas)                       │
└────────────────────────┬────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
         ↓               ↓               ↓
┌──────────────┐ ┌──────────────┐ ┌──────────────┐
│ DATA PLANE   │ │ REASONING    │ │ INGESTION    │
│ (Documents,  │ │ PLANE        │ │ PLANE        │
│  Vectors)    │ │ (AI/LLM)     │ │ (Import)     │
│              │ │              │ │              │
│ - document   │ │ - ai-core    │ │ - query      │
│ - embedding  │ │ - agent      │ │ - import     │
│ - vector     │ │ - rerank     │ │ - integration│
│ - rag        │ │ - synthesis  │ │ - crawler    │
└──────────────┘ └──────────────┘ └──────────────┘

┌─────────────────────────────────────────────────────────────┐
│              CROSS-CUTTING CONCERNS                          │
│                                                               │
│  Security Plane: authz, encryption                           │
│  Orchestration Plane: workflow-service, Temporal             │
│  Observability Plane: audit, evaluation, metrics             │
└─────────────────────────────────────────────────────────────┘
```

---

## Service Count by Plane

| Plane | Service Count | Purpose |
|-------|--------------|---------|
| **Control Plane** | 3 | Identity and permissions (auth, user, org) |
| **APPLICATION PLANE** | 7 | Product UX and real-time orchestration |
| **Data Plane** | 4 | Document and vector storage |
| **Reasoning Plane** | 4 | AI operations (LLM, agents) |
| **Ingestion Plane** | 4 | Data import and transformation |
| **Orchestration Plane** | 2 | Workflow coordination |
| **Observability Plane** | 2 | Metrics and auditing |
| **Security Plane** | 2 | Authorization and encryption |
| **Total** | **28** | Microservices |

---

## Control Plane (3 services)

| # | Service | Owns | Port |
|---|---------|------|------|
| 1 | auth-service | User credentials, JWT tokens | 3011 |
| 2 | user-service | User profiles, API keys | 3012 |
| 3 | org-service | Org metadata, quotas, entitlements | 8080 |

**Principle:** "Who can do what?" - Identity and permissions only

---

## APPLICATION PLANE (7 services)

| # | Service | Owns | Technology |
|---|---------|------|------------|
| 4 | convex-core | Chat, UI state, real-time sync | Self-hosted Convex |
| 5 | product-bff | API aggregation, transformations | Node.js GraphQL/REST |
| 6 | session-orchestrator | Session lifecycle, multi-tab state | Dragonfly + Node.js |
| 7 | feature-composer | Feature flags, A/B tests | LaunchDarkly/custom |
| 8 | realtime-streaming-layer | WebSocket mux, SSE | Socket.IO |
| 9 | notification-service | Multi-channel delivery | Bull + SendGrid |
| 10 | usage-projections | Quota forecasting, dashboards | Dragonfly + TimescaleDB |

**Principle:** "Product experience" - Real-time, reactive, UI-optimized

---

## Data Plane (4 services)

| # | Service | Owns | Port |
|---|---------|------|------|
| 11 | document-service | Documents (S3), metadata | TBD |
| 12 | embedding-service | Embedding generation | TBD |
| 13 | vector-service | Qdrant wrapper | TBD |
| 14 | rag-service | Retrieval orchestration | TBD |

**Principle:** "What data exists?" - Document and vector storage

---

## Reasoning Plane (4 services)

| # | Service | Owns | Port |
|---|---------|------|------|
| 15 | ai-core-service | LLM orchestration, prompts | 8040 |
| 16 | agent-service | Agent execution, tools | TBD |
| 17 | rerank-service | Result reranking | TBD |
| 18 | synthesis-service | Answer generation | TBD |

**Principle:** "How do we think?" - Stateless AI operations

---

## Ingestion Plane (4 services)

| # | Service | Owns | Port |
|---|---------|------|------|
| 19 | query-service | Query validation, expansion | TBD |
| 20 | import-service | Bulk file import jobs | TBD |
| 21 | integration-service | External sync (Notion, Slack) | TBD |
| 22 | crawler-service | Web crawling jobs | TBD |

**Principle:** "How does data arrive?" - Transform and delegate

---

## Orchestration Plane (2 services)

| # | Service | Owns | Port |
|---|---------|------|------|
| 23 | workflow-service | Workflow API wrapper | TBD |
| 24 | Temporal Workers | Workflow execution | N/A |

**Principle:** "How do we coordinate?" - Multi-service workflows

---

## Observability Plane (2 services)

| # | Service | Owns | Port |
|---|---------|------|------|
| 25 | audit-service | Audit logs, compliance | TBD |
| 26 | evaluation-service | Quality metrics, feedback | TBD |

**Principle:** "How do we monitor?" - Non-blocking observability

---

## Security Plane (2 services)

| # | Service | Owns | Port |
|---|---------|------|------|
| 27 | authz-service | Policy evaluation (OPA) | TBD |
| 28 | encryption-service | Data encryption, key mgmt | TBD |

**Principle:** "How do we protect?" - Policy enforcement

---

## Key Boundaries

### ✅ CLEAR Separations

1. **Control Plane ≠ Application Plane**
   - Control = authoritative (auth, org, quotas)
   - Application = reactive (BFF, real-time, aggregation)

2. **Application Plane ≠ Data Plane**
   - Application = ephemeral caches, UI optimization
   - Data = canonical storage (documents, vectors)

3. **Reasoning Plane ≠ Data Plane**
   - Reasoning = stateless AI operations
   - Data = document retrieval and storage

4. **All Planes → Control Plane (for validation)**
   - Every request validates JWT with auth-service
   - Every org operation checks quotas with org-service

### ❌ VIOLATIONS (Transitional)

1. **org-service** currently has RAG operations → Extract to Data Plane (Phase 2)
2. **ai-service** currently has embeddings → Extract to embedding-service (Phase 3)

---

## Data Flow Example: RAG Query

```
1. User searches in frontend
   ↓
2. product-bff (APPLICATION PLANE)
   - Validates JWT with auth-service
   - Checks quota with org-service
   ↓
3. rag-service (DATA PLANE)
   - Generates embedding
   - Searches vectors
   ↓
4. rerank-service → synthesis-service (REASONING PLANE)
   - Reranks results
   - Generates answer
   ↓
5. product-bff (APPLICATION PLANE)
   - Transforms response for UI
   - Caches result
   ↓
6. convex-core (APPLICATION PLANE)
   - Stores query in chat history
   - Pushes to frontend via WebSocket
   ↓
7. Frontend displays answer in real-time
```

---

## Communication Patterns

| Pattern | Example | Protocol |
|---------|---------|----------|
| Synchronous | product-bff → rag-service | gRPC/REST |
| Asynchronous | crawler-service → convex-core | NATS events |
| Real-time | convex-core → frontend | WebSocket |
| Streaming | ai-core → realtime-streaming-layer | SSE |
| Background | notification-service → SendGrid | Job queue |

---

## Security Rules (ALL Planes)

### MUST ✅
1. Validate JWT on every request
2. Check org membership for org-scoped data
3. Verify capabilities via org-service
4. Log sensitive operations to audit-service
5. Respect rate limits per user/org

### MUST NOT ❌
1. Trust orgId from frontend without validation
2. Bypass backend validation for performance
3. Store passwords in plaintext
4. Expose internal service URLs to frontend
5. Return stack traces to clients

---

## Next Steps

### Phase 2 (Weeks 3-7): Data Plane Extraction
- [ ] Extract RAG from org-service to rag-service
- [ ] Extract vectors from org-service to vector-service
- [ ] Implement document-service
- [ ] Implement embedding-service

### Phase 3 (Weeks 8-12): Reasoning Plane
- [ ] Refactor ai-core-service (remove embeddings)
- [ ] Implement agent-service
- [ ] Implement rerank-service
- [ ] Implement synthesis-service

### Phase 4 (Weeks 13-16): Application Plane
- [ ] Implement product-bff (GraphQL gateway)
- [ ] Implement session-orchestrator
- [ ] Implement feature-composer
- [ ] Implement realtime-streaming-layer
- [ ] Implement notification-service
- [ ] Implement usage-projections

---

## Documentation Map

- [BOUNDARY_VALIDATION.md](./BOUNDARY_VALIDATION.md) - Full service boundaries
- [APPLICATION_PLANE_OVERVIEW.md](./APPLICATION_PLANE_OVERVIEW.md) - Application Plane details
- [ARCHITECTURE.md](./ARCHITECTURE.md) - System architecture
- [CONTROL_REASONING_PLANES_QUICK_REF.md](./CONTROL_REASONING_PLANES_QUICK_REF.md) - Control/Reasoning
- [CONVEX_CORE_MIGRATION.md](./CONVEX_CORE_MIGRATION.md) - Convex Core setup

---

**Total Services:** 28  
**Longest Dependency Chain:** 5 hops (Frontend → BFF → RAG → Synthesis → AI-Core)  
**Circular Dependencies:** 0 ✅  
**Boundary Violations:** 2 (transitional, planned to fix)  

**Architecture Status:** ✅ **APPROVED FOR IMPLEMENTATION**
