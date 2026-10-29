# Multi-Plane Architecture Migration Roadmap

**Last Updated:** February 19, 2026  
**Status:** 🚧 Phase 1 Complete - Planning Remaining Phases

## Executive Summary

CoreSystem is transitioning from a **service-oriented architecture** to a **multi-plane architecture** that separates concerns by operational responsibility across 7 distinct planes:

1. **Control Plane** - Identity, permissions, org metadata
2. **Data Plane** - Document storage, vectors, retrieval
3. **Reasoning Plane** - AI orchestration, agents, synthesis
4. **Ingestion Plane** - Data import, integrations, crawling
5. **Orchestration Plane** - Workflow coordination, sagas
6. **Observability Plane** - Metrics, tracing, audit, evaluation
7. **Security Plane** - Authorization, encryption, compliance

**Timeline:** 20 weeks (5 months)  
**Current Status:** Planning complete, existing services organized

---

## Migration Phases

### ✅ Phase 1: Organization & Planning (Weeks 1-2) - COMPLETE

**Objective:** Create plane structure, organize existing services, document architecture

**Deliverables:**
- [x] Create `/planes/` directory structure
- [x] Symlink existing services to appropriate planes
- [x] Write comprehensive architecture documentation
- [x] Create README for each plane
- [x] Document migration roadmap

**Current State:**
```
planes/
├── control/         → auth-service, user-service, org-service, convex-gateway
├── reasoning/       → ai-core-service (renamed from ai-service)
├── data/           → (empty - to be created)
├── ingestion/      → (empty - to be created)
├── orchestration/  → (empty - to be created)
├── observability/  → (empty - to be created)
└── security/       → (empty - to be created)
```

---

### 🚧 Phase 2: Data Plane Extraction (Weeks 3-7)

**Objective:** Extract document and RAG operations from org-service, create dedicated data plane services

#### Phase 2.1: Setup Infrastructure (Week 3)
- [ ] Deploy MinIO (S3-compatible object storage)
- [ ] Create separate PostgreSQL schema for documents
- [ ] Ensure Qdrant is properly configured
- [ ] Set up NATS streams for data events

#### Phase 2.2: Create document-service (Weeks 3-4)
**Extract from:** org-service (document CRUD operations)

- [ ] Create Go project with Gin framework
- [ ] Implement document upload endpoint (S3 storage)
- [ ] Implement document metadata storage (PostgreSQL)
- [ ] Implement document retrieval endpoints
- [ ] Add versioning support
- [ ] File parsing (PDF, DOCX, TXT, MD, HTML)
- [ ] Publish `document.uploaded`, `document.indexed` events
- [ ] Write comprehensive tests
- [ ] Deploy to staging

**Port:** 3020

#### Phase 2.3: Create embedding-service (Week 4-5)
**Extract from:** ai-service (embedding generation)

- [ ] Create Python FastAPI project
- [ ] Integrate OpenAI embeddings API
- [ ] Integrate Cohere embeddings API
- [ ] Support custom embedding models
- [ ] Implement batching for efficiency
- [ ] Add request caching (Redis)
- [ ] Track costs per org and model
- [ ] Publish `embedding.generated` events
- [ ] Deploy to staging

**Port:** 3022

#### Phase 2.4: Create vector-service (Week 5)
**New service:** Qdrant client wrapper

- [ ] Create Go/Python project
- [ ] Wrap Qdrant SDK with service API
- [ ] Collection management (create, delete, info)
- [ ] Point operations (upsert, delete, search)
- [ ] Faceted filtering
- [ ] Payload indexing
- [ ] Snapshot and backup endpoints
- [ ] Health checks
- [ ] Deploy to staging

**Port:** 3023

#### Phase 2.5: Create rag-service (Week 6)
**Extract from:** org-service (retrieval operations)

- [ ] Create Go/Python project
- [ ] Implement semantic search endpoint
- [ ] Implement hybrid search (keyword + vector)
- [ ] Query expansion and reformulation
- [ ] Result ranking and scoring
- [ ] Integrate with org-service for quota checks
- [ ] Cache frequent queries (Redis)
- [ ] Publish `retrieval.completed` events
- [ ] Deploy to staging

**Port:** 3021

#### Phase 2.6: Integration & Migration (Week 7)
- [ ] End-to-end testing: Upload → Embed → Index → Retrieve
- [ ] Migrate org-service to call data plane services
- [ ] Update frontend to call data plane directly (where appropriate)
- [ ] Remove data operations from org-service
- [ ] org-service becomes control plane only (metadata, quotas, policies)
- [ ] Performance benchmarking
- [ ] Deploy to production with canary rollout

**Success Criteria:**
- ✅ All document operations via document-service
- ✅ All embeddings via embedding-service
- ✅ All vector ops via vector-service
- ✅ All retrievals via rag-service
- ✅ org-service only handles control plane (quotas, entitlements)
- ✅ No data ownership in control plane

---

### 🚧 Phase 3: Reasoning Plane Separation (Weeks 8-12)

**Objective:** Split AI operations into orchestration, agents, reranking, and synthesis

#### Phase 3.1: Refactor ai-core-service (Week 8)
- [ ] Remove document storage logic (moved to data plane)
- [ ] Remove embedding generation (moved to data plane)
- [ ] Remove deprecated endpoints
- [ ] Focus pure LLM orchestration only
- [ ] Improve prompt management
- [ ] Deploy refactored version

#### Phase 3.2: Create agent-service (Weeks 9-10)
**Extract from:** ai-core-service (agent execution)

- [ ] Create Python FastAPI project
- [ ] Integrate LangGraph for workflow orchestration
- [ ] Integrate Letta for agent memory
- [ ] Implement tool registry and execution
- [ ] Build conversation context management
- [ ] Agent lifecycle management (create, execute, monitor)
- [ ] Publish `agent.execution_started`, `agent.execution_completed` events
- [ ] Deploy to staging

**Port:** 3031

#### Phase 3.3: Create rerank-service (Week 10)
**New service:** Result reranking

- [ ] Create Python FastAPI project
- [ ] Integrate Cohere Rerank API
- [ ] Add cross-encoder model support
- [ ] Implement hybrid scoring (keyword + semantic + LLM)
- [ ] Diversity-based reranking
- [ ] Cache rerank results (Redis)
- [ ] Deploy to staging

**Port:** 3032

#### Phase 3.4: Create synthesis-service (Week 11)
**Extract from:** ai-core-service (response generation)

- [ ] Create Python FastAPI project
- [ ] Extract response synthesis logic
- [ ] Implement streaming responses
- [ ] Citation generation and formatting
- [ ] Fact verification pipeline
- [ ] Hallucination detection
- [ ] Publish `synthesis.started`, `synthesis.completed` events
- [ ] Deploy to staging

**Port:** 3033

#### Phase 3.5: Integration & Testing (Week 12)
- [ ] End-to-end agentic RAG flow testing
- [ ] Multi-model orchestration testing
- [ ] Reranking accuracy validation
- [ ] Synthesis quality evaluation
- [ ] Performance benchmarking
- [ ] Deploy to production

**Success Criteria:**
- ✅ AI orchestration separated from agent execution
- ✅ Reranking improves retrieval quality (measured)
- ✅ Synthesis produces factual, cited responses
- ✅ No data ownership in reasoning plane
- ✅ Stateless AI operations

---

### 🚧 Phase 4: Ingestion Plane Creation (Weeks 13-17)

**Objective:** Create services for bulk import, integrations, and crawling

#### Phase 4.1: Create query-service (Week 13)
**New service:** Query preprocessing

- [ ] Create Go/Python project
- [ ] Query validation and sanitization
- [ ] Intent classification
- [ ] Spell correction
- [ ] Synonym expansion
- [ ] Query history tracking
- [ ] Rate limiting (Redis)
- [ ] Deploy to staging

**Port:** 3040

#### Phase 4.2: Create import-service (Weeks 14-15)
**New service:** Bulk document import

- [ ] Create Python project
- [ ] File parsing (PDF, DOCX, TXT, MD, HTML, CSV)
- [ ] ZIP file handling
- [ ] Content extraction and cleaning
- [ ] Metadata extraction
- [ ] Batch job management (NATS/Temporal)
- [ ] Progress tracking and notifications (SSE)
- [ ] Resume failed imports
- [ ] Deploy to staging

**Port:** 3041

#### Phase 4.3: Create integration-service (Week 15-16)
**New service:** External API connectors

- [ ] Create Node.js/Python project
- [ ] OAuth flow management
- [ ] **Notion connector:** Sync databases, pages, blocks
- [ ] **Google Drive connector:** Import docs, sheets, slides
- [ ] **Slack connector:** Archive channels, messages
- [ ] Incremental sync (only changes)
- [ ] Webhook support for real-time updates
- [ ] Deploy to staging

**Port:** 3042

#### Phase 4.4: Create crawler-service (Week 16-17)
**New service:** Web crawling

- [ ] Create Python project with Scrapy
- [ ] Sitemap.xml parsing
- [ ] Robots.txt compliance
- [ ] JavaScript rendering (Playwright)
- [ ] Content extraction (article text)
- [ ] Duplicate detection
- [ ] Scheduled recrawling
- [ ] Deploy to staging

**Port:** 3043

#### Phase 4.5: Integration & Testing (Week 17)
- [ ] End-to-end import flow testing
- [ ] Notion integration testing
- [ ] Crawl performance testing
- [ ] Error recovery validation
- [ ] Deploy to production

**Success Criteria:**
- ✅ Bulk import handles 1000s of files
- ✅ Integrations sync incrementally
- ✅ Crawler respects rate limits and robots.txt
- ✅ All ingestion goes through ingestion plane

---

### 🚧 Phase 5: Orchestration Plane Deployment (Weeks 13-17)

**Objective:** Deploy Temporal for workflow coordination

#### Phase 5.1: Infrastructure Setup (Week 13)
- [ ] Deploy Temporal server
- [ ] Configure PostgreSQL persistence
- [ ] Deploy Temporal Web UI
- [ ] Set up monitoring (Prometheus metrics)

#### Phase 5.2: Create Workers (Weeks 14-15)
- [ ] **Document processing worker** (Python)
  - Workflow: Upload → Extract → Embed → Index
- [ ] **Agent execution worker** (Python)
  - Workflow: Retrieve → Rerank → Execute Agent → Synthesize
- [ ] **Import worker** (Go/Python)
  - Workflow: Validate → Parse → Store → Index (batched)
- [ ] Implement activities for each workflow
- [ ] Error handling and compensation (sagas)

#### Phase 5.3: Create workflow-service (Week 15-16)
**New service:** Workflow API

- [ ] Create Go/Node.js project
- [ ] HTTP API for workflow control (start, cancel, query)
- [ ] Workflow listing and search
- [ ] Signal/query support
- [ ] Metrics integration
- [ ] Deploy to staging

**Port:** 3050

#### Phase 5.4: Migrate Operations (Week 16-17)
- [ ] Convert bulk import to Temporal workflow
- [ ] Convert document processing to workflow
- [ ] Convert integration sync to workflow
- [ ] Convert agentic RAG to workflow

#### Phase 5.5: Testing (Week 17)
- [ ] Workflow replay testing
- [ ] Failure recovery testing
- [ ] Load testing (1000s of concurrent workflows)
- [ ] Deploy to production

**Success Criteria:**
- ✅ All long-running operations use Temporal
- ✅ Automatic retries and compensation work
- ✅ Workflow visibility in Temporal UI
- ✅ Durable execution guarantees

---

### 🚧 Phase 6: Cross-Cutting Planes (Weeks 18-20)

**Objective:** Deploy observability and security infrastructure

#### Phase 6.1: Observability Plane (Weeks 18-19)

**Week 18: Metrics & Tracing**
- [ ] Deploy Prometheus server
- [ ] Configure service discovery
- [ ] Deploy Grafana with dashboards
- [ ] Set up AlertManager
- [ ] Deploy Jaeger for distributed tracing
- [ ] Instrument all services with OpenTelemetry

**Week 19: Audit & Evaluation**
- [ ] Create audit-service (Port 3060)
  - Audit log storage (PostgreSQL with TimescaleDB)
  - Compliance query endpoints
  - Tamper-proof logging
- [ ] Create evaluation-service (Port 3061)
  - Retrieval metrics (precision@k, recall@k)
  - Generation metrics (faithfulness, relevance)
  - Human feedback collection
  - Quality dashboards

**Deliverables:**
- ✅ Comprehensive dashboards (service health, AI quality, compliance)
- ✅ Alerting for critical issues
- ✅ Distributed tracing across all requests
- ✅ Audit logs for compliance (GDPR, SOC2)

#### Phase 6.2: Security Plane (Weeks 19-20)

**Week 19: Authorization & Encryption**
- [ ] Deploy HashiCorp Vault for secrets management
- [ ] Migrate all secrets from .env to Vault
- [ ] Deploy Open Policy Agent (OPA)
- [ ] Create authz-service (Port 3070)
  - Fine-grained authorization
  - Policy evaluation
  - RBAC and ABAC support
- [ ] Create encryption-service (Port 3071)
  - Envelope encryption
  - Field-level encryption for PII
  - Key rotation

**Week 20: mTLS & Compliance**
- [ ] Set up certificate authority
- [ ] Issue certificates to all services
- [ ] Deploy Envoy/Istio for mTLS
- [ ] Enable service-to-service mTLS
- [ ] Implement GDPR right to be forgotten
- [ ] Automate SOC2 compliance checks

**Deliverables:**
- ✅ All secrets in Vault (no .env files in production)
- ✅ Fine-grained authorization on all endpoints
- ✅ Data encrypted at rest and in transit
- ✅ mTLS between all services
- ✅ GDPR compliance automation

---

## Timeline Summary

| **Phase** | **Duration** | **Weeks** | **Status** |
|-----------|-------------|----------|-----------|
| Phase 1: Organization & Planning | 2 weeks | 1-2 | ✅ Complete |
| Phase 2: Data Plane Extraction | 5 weeks | 3-7 | 🚧 Planned |
| Phase 3: Reasoning Plane Separation | 5 weeks | 8-12 | 🚧 Planned |
| Phase 4: Ingestion Plane Creation | 5 weeks | 13-17 | 🚧 Planned |
| Phase 5: Orchestration Plane | 5 weeks | 13-17 | 🚧 Planned (parallel) |
| Phase 6: Cross-Cutting Planes | 3 weeks | 18-20 | 🚧 Planned |
| **Total** | **20 weeks** | | **5 months** |

*Note: Phases 4 and 5 run in parallel (both weeks 13-17)*

---

## Service Creation Summary

### New Services to Create (15 Total)

**Data Plane (4 services):**
1. document-service (Port 3020) - Week 3-4
2. rag-service (Port 3021) - Week 6
3. embedding-service (Port 3022) - Week 4-5
4. vector-service (Port 3023) - Week 5

**Reasoning Plane (3 services):**
5. agent-service (Port 3031) - Week 9-10
6. rerank-service (Port 3032) - Week 10
7. synthesis-service (Port 3033) - Week 11

**Ingestion Plane (4 services):**
8. query-service (Port 3040) - Week 13
9. import-service (Port 3041) - Week 14-15
10. integration-service (Port 3042) - Week 15-16
11. crawler-service (Port 3043) - Week 16-17

**Orchestration Plane (1 service):**
12. workflow-service (Port 3050) - Week 15-16

**Observability Plane (2 services):**
13. audit-service (Port 3060) - Week 19
14. evaluation-service (Port 3061) - Week 19

**Security Plane (2 services):**
15. authz-service (Port 3070) - Week 19
16. encryption-service (Port 3071) - Week 19

---

## Deployment Strategy

### Staging Deployment (Each Phase)
1. Deploy new service to staging
2. Run integration tests
3. Validate with smoke tests
4. Monitor for 24 hours

### Production Deployment
1. **Canary Rollout:** 5% → 25% → 50% → 100%
2. **Feature Flags:** Enable new services gradually
3. **Dual Running:** Old and new services run in parallel
4. **Gradual Migration:** Shift traffic over 1-2 weeks
5. **Rollback Plan:** Keep old services for 4 weeks

---

## Risk Mitigation

### Technical Risks
| **Risk** | **Impact** | **Mitigation** |
|----------|-----------|---------------|
| Service dependencies break | High | Comprehensive integration testing, contract testing |
| Database schema conflicts | Medium | Use separate schemas per plane |
| Event ordering issues | Medium | NATS JetStream ordered delivery |
| Data consistency | High | Temporal workflows with sagas |
| Performance degradation | High | Load testing, gradual rollout |

### Organizational Risks
| **Risk** | **Impact** | **Mitigation** |
|----------|-----------|---------------|
| Team overwhelmed | High | Parallel development, hire contractors |
| Knowledge gaps | Medium | Documentation, training sessions |
| Scope creep | Medium | Stick to roadmap, defer non-critical features |
| Timeline slippage | Medium | Weekly check-ins, adjust scope if needed |

---

## Success Metrics

### Phase 2 (Data Plane)
- ✅ Document upload latency < 2s (95th percentile)
- ✅ Retrieval latency < 500ms (95th percentile)
- ✅ Embedding generation throughput > 100 docs/sec
- ✅ Zero data loss during migration

### Phase 3 (Reasoning Plane)
- ✅ Agent execution success rate > 95%
- ✅ Reranking improves precision@5 by > 20%
- ✅ Synthesis hallucination rate < 5%
- ✅ AI cost reduction > 30% (via caching, model selection)

### Phase 4 (Ingestion Plane)
- ✅ Bulk import handles > 10,000 files/hour
- ✅ Integration sync latency < 5 minutes
- ✅ Crawler throughput > 100 pages/minute
- ✅ Import error recovery rate > 99%

### Phase 5 (Orchestration)
- ✅ Workflow success rate > 99.9%
- ✅ Automatic retry resolves > 95% of transient failures
- ✅ Workflow visibility for all long-running operations

### Phase 6 (Observability & Security)
- ✅ Distributed tracing coverage: 100% of requests
- ✅ Alert false positive rate < 10%
- ✅ Secrets management: Zero secrets in code or configs
- ✅ Authorization enforcement: 100% of endpoints

---

## Communication Plan

### Weekly Updates
- **Audience:** Stakeholders, management
- **Format:** Email with progress summary
- **Content:** Completed tasks, risks, blockers

### Bi-Weekly Demos
- **Audience:** Team, stakeholders
- **Format:** Live demo of new capabilities
- **Content:** Show new services, explain benefits

### Documentation
- **Continuous:** Update architecture docs
- **Per Phase:** Write migration guides
- **Post-Migration:** Update all service READMEs

---

## Next Steps (Immediate)

### Week 3 Actions
1. **Set up infrastructure:**
   - Deploy MinIO for S3-compatible object storage
   - Create `documents` PostgreSQL schema
   - Configure NATS streams for data events

2. **Start document-service:**
   - Create Go project with Gin
   - Implement document upload endpoint
   - Implement S3 storage client

3. **Team coordination:**
   - Assign engineers to data plane services
   - Schedule daily standups
   - Set up project tracking (Linear/Jira)

---

## Related Documentation

- [Multi-Plane Architecture Overview](./ARCHITECTURE.md)
- Plane-Specific READMEs:
  - [Control Plane](./control/README.md)
  - [Data Plane](./data/README.md)
  - [Reasoning Plane](./reasoning/README.md)
  - [Ingestion Plane](./ingestion/README.md)
  - [Orchestration Plane](./orchestration/README.md)
  - [Observability Plane](./observability/README.md)
  - [Security Plane](./security/README.md)

**Migration Owner:** Engineering Team  
**Last Review:** February 19, 2026  
**Next Review:** March 1, 2026 (after Phase 2.1 complete)
