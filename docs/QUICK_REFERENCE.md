# CoreSystem - Pyramid-over-Planes Quick Reference

## Current State vs Target State

### 📊 Canonical Operating Model

```
┌─────────────────────────────────────────────────────────────────────┐
│                        CURRENT STATE (Phase 1)                       │
│                      5 Services in /services/                        │
└─────────────────────────────────────────────────────────────────────┘

          ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
          │ auth-service │  │ user-service │  │  org-service │
          │   (NestJS)   │  │     (Go)     │  │     (Go)     │
          │   Port 3011  │  │   Port 3012  │  │   Port 3013  │
          └──────────────┘  └──────────────┘  └──────────────┘
                   │                │                │
                   │  Control Plane │  (+ some data) │
                   └────────────────┴────────────────┘

          ┌──────────────┐  ┌──────────────────┐
          │  ai-service  │  │ convex-gateway   │
          │  (Python)    │  │    (Node.js)     │
          │   Port 8000  │  │    Port 3014     │
          └──────────────┘  └──────────────────┘
                   │                │
            Reasoning (mixed)   WebSocket Gateway
```

```text
Frontend Plane
    verevon and future dedicated frontend apps

Application Plane (optional)
    convex, novu, affine, zammad, app-specific projections

Ingestion Plane              Model Plane v2
    imports, integration,      ai-core, agent-core,
    quarry, crawlers           capability/session/runtime services

Data Plane
    documents, retrieval, embedding-worker, knowledge-index

Control Plane
    auth-core, user-core, org-core, billing-core, session-core

Cross-cutting across all layers
    Orchestration Plane, Observability Plane, Security Plane
```

## Authority Rules

- **Control Plane** is the root authority for auth, user, org, billing, entitlement, quota, and session relations.
- **Data Plane** is the canonical store for user- and org-owned product data.
- **Ingestion Plane** writes discovered or imported content into Data Plane.
- **Model Plane v2** reads grounded product data from Data Plane and validates authority through Control Plane.
- **Application Plane** is optional and non-authoritative.
- **Frontend Plane** composes the lower planes into end-user applications.

## Independence Rules

- Each plane keeps its own compose stack.
- Each plane keeps its own env files.
- Each plane keeps its own private runtime network.
- Planes use each other through internal APIs and event contracts, not shared database ownership.
- The pyramid defines dependency direction and authority, not mandatory co-deployment.

---

## 🎯 Migration Timeline

```
Week 1-2:   ✅ Organization & Planning (COMPLETE)
            ↓
Week 3-7:   🚧 Data Plane Extraction
            │  • document-service, rag-service
            │  • embedding-service, vector-service
            ↓
Week 8-12:  🚧 Model Plane v2 Consolidation
            │  • ai-core and agent-core runtime hardening
            │  • capability, session, and workflow surfaces
            ↓
Week 13-17: 🚧 Ingestion + Orchestration (parallel)
            │  • query, import, integration, crawler
            │  • Temporal workers, workflow-service
            ↓
Week 18-20: 🚧 Observability + Security
            │  • Metrics, tracing, audit
            │  • Authorization, encryption, mTLS
            ↓
Week 20:    ✅ COMPLETE - Full Multi-Plane Architecture
```

---

## 📁 Directory Structure

```
CoreSystem/
├── planes/                          ← New multi-plane organization
│   ├── ARCHITECTURE.md              ← Overall architecture docs
│   ├── MIGRATION_ROADMAP.md         ← This migration plan
│   ├── QUICK_REFERENCE.md           ← Quick reference (you are here)
│   │
│   ├── control/                     ✅ Phase 1 Complete
│   │   ├── README.md
│   │   ├── auth-service/            → symlink to /services/auth-service
│   │   ├── user-service/            → symlink to /services/user-service
│   │   ├── org-service/             → symlink to /services/org-service
│   │   └── convex-gateway/          → symlink to /services/convex-gateway
│   │
│   ├── data/                        🚧 Phase 2 (Weeks 3-7)
│   │   ├── README.md
│   │   ├── document-service/        🆕 To be created
│   │   ├── rag-service/             🆕 To be created
│   │   ├── embedding-service/       🆕 To be created
│   │   └── vector-service/          🆕 To be created
│   │
│   ├── model-plane-v2/              ✅ Canonical runtime direction
│   │   ├── README.md
│   │   ├── ai-core/                 → canonical inference facade
│   │   ├── agent-core/              → canonical runtime orchestration
│   │   ├── capability-core/         🆕 policy and routing authority
│   │   └── session-core/            🆕 session authority for model runtime
│   │
│   ├── ingestion/                   🚧 Phase 4 (Weeks 13-17)
│   │   ├── README.md
│   │   ├── query-service/           🆕 To be created
│   │   ├── import-service/          🆕 To be created
│   │   ├── integration-service/     🆕 To be created
│   │   └── crawler-service/         🆕 To be created
│   │
│   ├── orchestration/               🚧 Phase 5 (Weeks 13-17)
│   │   ├── README.md
│   │   ├── temporal/                🆕 Deploy Temporal server
│   │   ├── workers/                 🆕 Temporal workers
│   │   └── workflow-service/        🆕 To be created
│   │
│   ├── observability/               🚧 Phase 6 (Weeks 18-19)
│   │   ├── README.md
│   │   ├── prometheus/              🆕 Deploy
│   │   ├── grafana/                 🆕 Deploy
│   │   ├── jaeger/                  🆕 Deploy
│   │   ├── audit-service/           🆕 To be created
│   │   └── evaluation-service/      🆕 To be created
│   │
│   └── security/                    🚧 Phase 6 (Weeks 19-20)
│       ├── README.md
│       ├── vault/                   🆕 Deploy
│       ├── opa/                     🆕 Deploy
│       ├── authz-service/           🆕 To be created
│       ├── encryption-service/      🆕 To be created
│       └── policies/                🆕 OPA policies
│
├── services/                        ← Original service directory (preserved)
│   ├── auth-service/
│   ├── user-service/
│   ├── org-service/
│   ├── ai-service/
│   └── convex-gateway/
│
├── libraries/                       ← Shared libraries
│   ├── go-common/
│   ├── ts-common/
│   ├── py-common/
│   └── proto-definitions/
│
└── docs/                            ← Documentation
    ├── BACKEND_BOUNDARY_REFACTOR_ROADMAP.md
    ├── SERVICE_BOUNDARY_CHARTER.md
    └── ... (other docs)
```

---

## 🔑 Key Concepts

### What is a "Plane"?
A **plane** is a horizontal service boundary with its own runtime and ownership rules. The pyramid sits on top of those plane boundaries and defines which planes are authoritative for which domains.

- **Control Plane:** Who can do what, and under which user/org/billing/session relation?
- **Data Plane:** What product data exists and how is it retrieved?
- **Model Plane v2:** How do we reason, orchestrate, and run model-backed workflows?
- **Ingestion Plane:** How does data arrive? (Import, crawl, integrate)
- **Application Plane:** Which optional app-level systems enrich the product experience?
- **Orchestration Plane:** How do we coordinate? (cross-cutting)
- **Observability Plane:** How do we monitor? (Metrics, traces, audit)
- **Security Plane:** How do we protect? (AuthZ, encryption, compliance)

### Benefits
✅ **Clear Ownership:** Each plane has a single responsibility  
✅ **Independent Scaling:** Scale data plane ≠ scale control plane  
✅ **Technology Flexibility:** Different tools per plane  
✅ **Team Organization:** Teams own planes, not just services  
✅ **Security Boundaries:** Stricter isolation between concerns  

---

## 🚀 Quick Start (For Developers)

### Explore the Architecture
```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/planes

# Read overall architecture
cat ARCHITECTURE.md

# Check control plane services
ls -la control/

# Read migration plan
cat MIGRATION_ROADMAP.md
```

### Work on a Plane
```bash
# Control plane (existing services)
cd planes/control
ls -la  # Shows symlinks to actual services

# Data plane (future services)
cd planes/data
cat README.md  # Read about planned services
```

### Run Services (Current State)
```bash
# Still use the original docker-compose
cd /Volumes/Lagring/Triodelab/CoreSystem
docker-compose -f docker-compose.services.yml up
```

### Run Services (Future - Per Plane)
```bash
# Run only control plane
docker-compose -f planes/control/docker-compose.control.yml up

# Run only data plane
docker-compose -f planes/data/docker-compose.data.yml up

# Run all planes
docker-compose -f planes/docker-compose.all.yml up
```

---

## 📊 Service Count

| **Category** | **Current** | **Target** | **Change** |
|-------------|-------------|------------|------------|
| Control Plane | 4 | 4 | No change |
| Data Plane | 0 (in org-service) | 4 | +4 new services |
| Model Plane v2 | 1 legacy donor + v2 runtime | Canonical runtime stack | Consolidate onto v2 authority |
| Ingestion Plane | 0 | 4 | +4 new services |
| Orchestration | 0 | 1 + workers | +Temporal |
| Observability | 0 | 4 + infra | +Monitoring stack |
| Security | 0 | 4 + infra | +Security stack |
| **Total Services** | **5** | **21** | **+16 services** |

---

## 🎯 Success Criteria (Final)

### Technical
- ✅ All 21 services deployed and operational
- ✅ No cross-plane data ownership
- ✅ Clear API contracts between planes
- ✅ Independent scaling per plane
- ✅ Observability across all planes
- ✅ Security enforced at plane boundaries

### Performance
- ✅ Document upload < 2s (P95)
- ✅ Retrieval < 500ms (P95)
- ✅ Agent success rate > 95%
- ✅ Workflow success rate > 99.9%
- ✅ Zero data loss during migration

### Operational
- ✅ 100% distributed tracing coverage
- ✅ All secrets in Vault
- ✅ Fine-grained authorization on all endpoints
- ✅ GDPR compliance automation
- ✅ SOC2 compliance verification

---

## 📚 Documentation Links

### Overview
- [Multi-Plane Architecture](./ARCHITECTURE.md)
- [Migration Roadmap](./MIGRATION_ROADMAP.md)
- **[Quick Reference](./QUICK_REFERENCE.md)** ← You are here

### Plane-Specific
- [Control Plane README](./control/README.md)
- [Data Plane README](./data/README.md)
- [Model Plane v2 Freeze](./ARCHITECTURE_FREEZE.md)
- [Ingestion Plane README](./ingestion/README.md)
- [Orchestration Plane README](./orchestration/README.md)
- [Observability Plane README](./observability/README.md)
- [Security Plane README](./security/README.md)

### Historical
- [Backend Boundary Refactor Roadmap](../docs/BACKEND_BOUNDARY_REFACTOR_ROADMAP.md)
- [Service Boundary Charter](../docs/SERVICE_BOUNDARY_CHARTER.md)
- [Physical Split Complete](../docs/PHYSICAL_SPLIT_COMPLETE.md)

---

## 🤝 Getting Help

- **Questions:** Check plane-specific READMEs
- **Issues:** #architecture channel (Slack)
- **Updates:** Weekly migration email
- **Demos:** Bi-weekly stakeholder demos

---

**Last Updated:** February 19, 2026  
**Current Phase:** Phase 1 Complete ✅  
**Next Milestone:** Phase 2.1 - Deploy MinIO and create document-service (Week 3)
