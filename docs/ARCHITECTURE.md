# Multi-Plane Architecture

**Last Updated:** April 8, 2026  
**Status:** Active

## Overview

CoreSystem keeps its **plane-based microservice structure**, but the operating model is no longer flat. The system follows a **pyramid architecture layered on top of the plane split**:

1. **Control Plane** is the authority root for identity, user, org, billing, entitlement, quota, and session relations.
2. **Data Plane** is the canonical store for user- and org-owned product data.
3. **Ingestion Plane** and **Model Plane v2** consume Control and Data capabilities to ingest, transform, retrieve, reason, and automate.
4. **Application Plane** hosts optional product-level applications and projections.
5. **Frontend Plane** composes one or more backend planes into end-user applications.

This preserves true microservice independence:

- Each plane owns its own services, infrastructure, databases, env files, and compose stack.
- Planes do **not** require shared databases or co-deployment.
- Planes may still **use** each other through internal APIs and shared event contracts.
- Security, observability, and orchestration remain **cross-cutting planes**, not steps in the pyramid.

## Pyramid Model

```text
Frontend Plane
  |
Application Plane (optional)
  |
Ingestion Plane        Model Plane v2
     \            /
       Data Plane
         |
     Control Plane

Cross-cutting across every layer:
- Security Plane
- Observability Plane
- Orchestration Plane
```

## Core Invariants

### 1. Control Plane is the authority root

The Control Plane governs all identity and relationship concerns for the rest of CoreSystem:

- `auth-core` owns authentication and session issuance
- `user-core` owns user profiles and user metadata
- `org-core` owns org structure, entitlements, quotas, and org metadata
- `billing-core` owns billing state and usage enforcement inputs
- `session-core` is the canonical session authority where that domain is used

No other plane may become the durable owner of those domains.

### 2. Data Plane is the canonical product data layer

The Data Plane stores the user and organization data that powers the system's product capabilities:

- knowledge
- documents
- imports output
- tasks and jobs metadata
- grounding, retrieval, and indexing state

Other planes may cache, mirror, project, or derive from Data Plane state, but they do not become the primary owner of that durable product data.

### 3. Ingestion Plane and Model Plane v2 are capability consumers

The Ingestion Plane and Model Plane v2 sit above Control and Data in the pyramid:

- **Ingestion Plane** authenticates and authorizes through Control Plane, then lands imported or discovered content into Data Plane.
- **Model Plane v2** authenticates and authorizes through Control Plane, then retrieves grounded data from Data Plane for reasoning, orchestration, and agent execution.

They remain independently deployable. "Using" lower planes means contract-based API or event integration, not shared persistence or private schema coupling.

### 4. Application Plane is optional and non-authoritative

The Application Plane hosts product-level systems such as Convex, Novu, AFFiNE, Zammad, and other application-specific surfaces.

These services may orchestrate collaboration, notifications, projections, or specialized workflows, but they do not replace Control Plane authority or Data Plane ownership.

### 5. Frontend Plane is the composition layer

The Frontend Plane assembles the needed planes into end-user applications.

Examples:

- `verevon` can compose Control, Data, Ingestion, Model Plane v2, and Application Plane services.
- A future Quarry-specific frontend can compose only the planes required for Quarry.
- A future MP v2 frontend can compose only Model Plane v2 with the lower-layer dependencies it needs.

## Plane Definitions

### 1. CONTROL PLANE
**Responsibility:** Authority layer for identity, user, org, billing, quota, entitlement, and session relations

**Services:**
- `auth-core` (Port 3011) - Authentication, session issuance, JWT/JWKS
- `user-core` (Port 3012) - User profiles, preferences, API keys
- `org-core` (Port 8080) - Organization metadata, entitlements, quotas, plans
- `billing-core` (Port 3014) - Billing state, plan lifecycle, usage inputs
- `session-core` - Canonical session authority where used

**Characteristics:**
- Canonical authority for identity and relationship domains
- Every other plane validates users, orgs, and capabilities here
- Publishes cross-plane events when control-plane state changes
- Fast, low-latency operations on the critical path
- Does not own product content or retrieval data

**APIs:**
```
GET  /api/v1/users/me
GET  /api/v1/organizations/:id
POST /api/v1/auth/login
GET  /api/v1/organizations/:id/entitlements
```

**Events Published:**
```
user.created, user.updated, user.deleted
organization.created, organization.updated, organization.deleted
session.created, session.ended
```

---

### 2. DATA PLANE
**Responsibility:** Canonical product data storage, indexing, retrieval, and knowledge access

**Services:**
- `documents-service` - Document CRUD, metadata, versioning
- `retrieval-service` - Retrieval operations, semantic search, ranking
- `embedding-worker` - Embedding generation and async indexing support
- `knowledge-index` - Metadata, indexing, collection integrity

**Characteristics:**
- Owns user- and org-owned product data used by the rest of the platform
- Receives imported and generated data from other planes
- Serves as the canonical retrieval and grounding layer
- High throughput and horizontally scalable
- Never owns identity, billing, or org authority

**APIs:**
```
POST /api/v1/documents
GET  /api/v1/documents/:id
POST /api/v1/rag/retrieve
POST /api/v1/embeddings/generate
POST /api/v1/vectors/index
```

**Events Published:**
```
document.indexed, document.updated, document.deleted
retrieval.completed
embedding.generated
```

---

### 3. MODEL PLANE V2
**Responsibility:** AI orchestration, agent execution, reasoning, synthesis, and model-facing runtime

**Services:**
- `ai-core` - Inference facade, model normalization, safety, inference APIs
- `agent-core` - Planning, orchestration, tool calling, workflows, reflection
- Supporting v2 services such as `capability-core`, `session-core`, `llm-worker`, and future runtime components

**Characteristics:**
- Consumes Control Plane for auth, org, quota, and policy inputs
- Consumes Data Plane for retrieval and grounding
- Does not become the owner of canonical product data
- Can be deployed independently and scaled for runtime workloads
- Model Plane v2 is the canonical runtime; legacy Model Plane remains frozen except for critical fixes

**APIs:**
```
POST /api/v1/ai/complete
POST /api/v1/agents/execute
POST /api/v1/rerank
POST /api/v1/synthesis/generate
```

**Events Consumed:**
```
retrieval.completed → Trigger reranking
document.indexed → Update agent context
```

---

### 4. INGESTION PLANE
**Responsibility:** Data import, external integrations, crawling, batch processing

**Services:**
- `query-service` (Port 3040) - Query processor, request validation
- `import-service` (Port 3041) - Bulk document import, file parsing
- `integration-service` (Port 3042) - External API connectors (Notion, Google Drive, etc.)
- `crawler-service` (Port 3043) - Web crawling, sitemap processing

**Characteristics:**
- Authenticates users and org context through Control Plane
- Pushes imported and discovered content into Data Plane
- Async, batch-oriented, and durable by design
- Long-running jobs use workflow and messaging infrastructure
- Does not become the authority for identities or durable product truth

**APIs:**
```
POST /api/v1/query
POST /api/v1/import/bulk
POST /api/v1/integrations/notion/sync
POST /api/v1/crawl/sitemap
```

**Events Published:**
```
import.started, import.completed, import.failed
crawl.started, crawl.page_found, crawl.completed
integration.synced
```

---

### 5. APPLICATION PLANE
**Responsibility:** Optional product-level applications, projections, notifications, collaboration, and app-specific workflows

**Services:**
- `convex-core`
- `notification-service` / Novu bridge
- `affine-core`
- `zammad` and similar product-facing application services

**Characteristics:**
- Optional layer above backend authority planes
- Can host app-specific projections, notifications, collaboration, and workflows
- May aggregate or react to lower-plane events
- Does not become the canonical owner of auth, org, billing, or product data

---

### 6. ORCHESTRATION PLANE (Cross-Cutting)
**Responsibility:** Workflow coordination, saga orchestration, long-running processes

**Services:**
- `temporal-workers` - Temporal workflow workers
- `workflow-service` (Port 3050) - Workflow definitions, triggers

**Characteristics:**
- Manages distributed transactions
- Handles retries, timeouts, compensation
- Durable execution guarantees
- Visibility into workflow state

**Workflows:**
```
DocumentProcessingWorkflow:
  1. Upload to document-service
  2. Generate embeddings (embedding-service)
  3. Index in vectors (vector-service)
  4. Update metadata (document-service)
  5. Notify completion

AgenticRAGWorkflow:
  1. Retrieve candidates (rag-service)
  2. Rerank results (rerank-service)
  3. Execute agent (agent-service)
  4. Synthesize response (synthesis-service)
```

---

### 7. OBSERVABILITY PLANE (Cross-Cutting)
**Responsibility:** Metrics, tracing, audit logs, evaluation, monitoring

**Components:**
- `metrics-service` - Prometheus metrics, Grafana dashboards
- `tracing-service` - Jaeger/Tempo distributed tracing
- `audit-service` (Port 3060) - Audit log storage, compliance queries
- `evaluation-service` (Port 3061) - AI response quality, benchmarking

**Characteristics:**
- Non-blocking (async writes)
- High write throughput
- Long retention for compliance
- Queryable for debugging and analysis

**Metrics Tracked:**
```
- Request rate, latency, error rate per service
- Token usage per org, user, model
- Retrieval quality (precision, recall)
- Agent execution success rate
- Cost per request
```

**Trace Context:**
```json
{
  "traceId": "abc123",
  "spanId": "def456",
  "parentSpanId": "ghi789",
  "service": "rag-service",
  "operation": "retrieve",
  "orgId": "org_123",
  "userId": "user_456"
}
```

---

### 8. SECURITY PLANE (Cross-Cutting)
**Responsibility:** Authorization, encryption, mTLS, compliance, secrets management

**Components:**
- `authz-service` - Fine-grained authorization (OPA, Zanzibar)
- `encryption-service` - Data encryption at rest and in transit
- `mtls-gateway` - Mutual TLS for service-to-service
- `compliance-service` - GDPR, SOC2, audit trail

**Characteristics:**
- Every request passes through security checks
- Transparent to application code (middleware)
- Zero-trust architecture
- Secrets never touch application logs

**Security Layers:**
```
1. Authentication (Control Plane: auth-service)
2. Authorization (Security Plane: authz-service)
3. Quota Enforcement (Control Plane: org-service)
4. Data Encryption (Security Plane: encryption-service)
5. Audit Logging (Observability Plane: audit-service)
```

---

## Communication Model

### Internal APIs

- Planes may call lower planes through explicit internal APIs
- Internal API authentication must come from Control Plane-issued or Control Plane-governed credentials
- No plane should depend on another plane's private database schema or private Docker network to function

### Events and NATS

- Each plane may keep its own plane-local NATS broker for internal choreography
- Shared application or cross-plane NATS can exist for contract-based event exchange
- Event contracts are integration surfaces, not ownership transfer

### Independence Rule

Each plane must remain independently startable and operable:

- own compose stack
- own env files
- own state stores
- own internal network

The pyramid defines **authority and dependency direction**, not forced runtime co-location.

## Service-to-Plane Mapping

### Current Services → Planes

| Current Service | Plane | New Location |
|----------------|-------|--------------|
| auth-core | Control | `planes/control/auth-core/` |
| user-core | Control | `planes/control/user-core/` |
| org-core | Control | `planes/control/org-core/` | 
| ai-core | Reasoning | `planes/reasoning/ai-core/` |
| convex-gateway | Control | `planes/control/convex-core/` |

### New Services to Create

| New Service | Plane | Purpose |
|-------------|-------|---------|
| document-service | Data | Document storage, metadata |
| rag-service | Data | Retrieval operations |
| embedding-service | Data | Embedding generation |
| vector-service | Data | Qdrant client wrapper |
| agent-service | Reasoning | Agent execution |
| rerank-service | Reasoning | Result reranking |
| synthesis-service | Reasoning | Response generation |
| query-service | Ingestion | Query processing |
| import-service | Ingestion | Bulk import |
| integration-service | Ingestion | External APIs |
| crawler-service | Ingestion | Web crawling |
| workflow-service | Orchestration | Temporal workflows |

---

## Request Flow Examples

### Example 1: Simple RAG Query
```
1. Frontend → api-gateway → authz-service (Security Plane)
2. api-gateway → query-service (Ingestion Plane) - Validate query
3. query-service → rag-service (Data Plane) - Retrieve documents
4. rag-service → vector-service (Data Plane) - Semantic search
5. rag-service → rerank-service (Reasoning Plane) - Rerank results
6. rerank-service → synthesis-service (Reasoning Plane) - Generate response
7. synthesis-service → Frontend (via api-gateway)

Cross-cutting:
- tracing-service: Track trace through all steps
- audit-service: Log query for compliance
- metrics-service: Track latency, cost
```

### Example 2: Agentic Workflow
```
1. Frontend → api-gateway → agent-service (Reasoning Plane)
2. agent-service → workflow-service (Orchestration Plane) - Start workflow
3. workflow-service coordinates:
   a. rag-service (Data) - Retrieve context
   b. ai-core-service (Reasoning) - Run agent with tools
   c. document-service (Data) - Create new documents if needed
   d. synthesis-service (Reasoning) - Format final response
4. workflow-service → Frontend (streaming response)

Cross-cutting:
- evaluation-service: Track agent quality
- audit-service: Log tool executions
- encryption-service: Encrypt sensitive data
```

### Example 3: Bulk Document Import
```
1. Frontend → api-gateway → import-service (Ingestion Plane)
2. import-service → workflow-service (Orchestration Plane) - Start import workflow
3. workflow-service coordinates:
   a. document-service (Data) - Store documents
   b. embedding-service (Data) - Generate embeddings (batched)
   c. vector-service (Data) - Index vectors (batched)
   d. audit-service (Observability) - Log import
4. workflow-service → Frontend (progress updates via SSE)

Cross-cutting:
- org-service (Control): Check quotas before each document
- metrics-service: Track import rate, failures
```

---

## Infrastructure Layout

### Docker Compose Organization
```yaml
# planes/control/docker-compose.control.yml
services:
  auth-service:
  user-service:
  org-service:
  convex-gateway:

# planes/data/docker-compose.data.yml
services:
  postgres:  # Document metadata
  qdrant:    # Vector storage
  s3:        # Document storage (MinIO locally)
  document-service:
  rag-service:
  embedding-service:
  vector-service:

# planes/reasoning/docker-compose.reasoning.yml
services:
  ai-core-service:
  agent-service:
  rerank-service:
  synthesis-service:

# planes/ingestion/docker-compose.ingestion.yml
services:
  query-service:
  import-service:
  integration-service:
  crawler-service:

# planes/orchestration/docker-compose.orchestration.yml
services:
  temporal:
  temporal-workers:
  workflow-service:

# planes/observability/docker-compose.observability.yml
services:
  prometheus:
  grafana:
  jaeger:
  audit-service:
  evaluation-service:

# planes/security/docker-compose.security.yml
services:
  authz-service:  # OPA or custom
  vault:          # Secrets management
  mtls-proxy:     # Envoy or similar
```

### Kubernetes Organization
```
planes/
├── control/
│   └── k8s/
│       ├── auth-service.yaml
│       ├── user-service.yaml
│       └── org-service.yaml
├── data/
│   └── k8s/
│       ├── document-service.yaml
│       ├── rag-service.yaml
│       ├── embedding-service.yaml
│       └── vector-service.yaml
└── ... (similar for other planes)
```

---

## Migration Strategy

### Phase 1: Organize Existing Services (Current) ✅
- Create plane directories
- Move/symlink existing services to appropriate planes
- Update documentation

### Phase 2: Extract Data Plane (Weeks 1-4)
- Create document-service (extract from org-service)
- Create rag-service (extract from org-service)
- Create embedding-service (extract from ai-service)
- Create vector-service (Qdrant client)
- Migrate org-service to only control plane operations

### Phase 3: Extract Reasoning Plane (Weeks 5-8)
- Rename ai-service to ai-core-service
- Create agent-service (extract from ai-core)
- Create rerank-service
- Create synthesis-service
- Clean separation: reasoning vs data

### Phase 4: Create Ingestion Plane (Weeks 9-12)
- Create query-service
- Create import-service
- Create integration-service (Notion, Google Drive, etc.)
- Create crawler-service

### Phase 5: Add Orchestration Plane (Weeks 13-16)
- Set up Temporal
- Create workflow-service
- Migrate long-running operations to workflows
- Add saga patterns for distributed transactions

### Phase 6: Enhance Cross-Cutting Planes (Weeks 17-20)
- Deploy Prometheus + Grafana (Observability)
- Deploy Jaeger/Tempo (Tracing)
- Create audit-service (Compliance)
- Create evaluation-service (AI quality)
- Add authz-service (Fine-grained permissions)
- Set up mTLS between services

---

## Benefits of Plane Architecture

### Clear Separation of Concerns
- **Control Plane:** Who can do what?
- **Data Plane:** What data exists?
- **Reasoning Plane:** How do we think about it?
- **Ingestion Plane:** How does data arrive?
- **Orchestration:** How do we coordinate?

### Independent Scaling
- Scale data plane for storage capacity
- Scale reasoning plane for AI workload
- Scale ingestion plane for batch jobs
- Control plane stays small and fast

### Technology Flexibility
- Data plane can use different databases per service
- Reasoning plane can deploy to GPU nodes
- Ingestion plane can use different queuing systems
- Observability can swap monitoring tools

### Team Ownership
- **Platform Team:** Control, Security, Observability planes
- **Data Team:** Data plane services
- **AI/ML Team:** Reasoning plane services
- **Integration Team:** Ingestion plane services
- **DevOps Team:** Orchestration plane

### Security Boundaries
- Control plane has strictest access controls
- Data plane enforces quotas and encryption
- Reasoning plane is stateless (no PII)
- Security plane is isolated from application logic

---

## Success Criteria

### Phase 1 (Organization) ✅
- ✅ Plane directories created
- ⏳ Existing services organized by plane
- ⏳ Documentation complete

### Phase 2-6 (New Services)
- ⏳ All plane services deployed
- ⏳ No cross-plane data ownership
- ⏳ Clear API contracts between planes
- ⏳ Observability across all planes
- ⏳ Security enforced at plane boundaries

### Production Readiness
- ⏳ All services independently deployable
- ⏳ Horizontal scaling verified per plane
- ⏳ Chaos testing passed
- ⏳ Disaster recovery tested
- ⏳ Compliance requirements met

---

## Related Documentation

- [Service Boundary Charter](../docs/SERVICE_BOUNDARY_CHARTER.md)
- [Backend Refactor Roadmap](../docs/BACKEND_BOUNDARY_REFACTOR_ROADMAP.md)
- [Physical Split Complete](../docs/PHYSICAL_SPLIT_COMPLETE.md)

**Next Steps:** Organize existing services into plane structure, then begin extracting data plane services
