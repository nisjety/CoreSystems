# Cross-Plane Contract Matrix

> **Authority:** [ARCHITECTURE_DECISION_CROSS_PLANE_CONTRACTS.md](./ARCHITECTURE_DECISION_CROSS_PLANE_CONTRACTS.md)  
> **Purpose:** Per-plane, per-service contract matrix naming allowed API dependencies, published event families, required auth mechanism, and forbidden shared-state patterns.  
> **Role:** Migration checklist and review gate for all later PRs.

---

## Notation

| Symbol | Meaning |
|--------|---------|
| ✅ | Allowed |
| 🚫 | Forbidden |
| ➡️ | Publish direction |
| ⬅️ | Consume direction |

---

## L1 — Control Plane (Source of Truth: Identity, Org, Billing, Entitlement, Quota, Session)

### auth-core (NestJS · REST :3011)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | _None_ — L1 is the apex; does not call any other plane |
| **Published Events** | `user.created`, `user.updated`, `organization.created`, `organization.member.added`, `organization.member.removed` |
| **Auth Mechanism** | JWT issuance, session-token issuance (HTTP-only cookies) |
| **Forbidden Patterns** | 🚫 No other plane writes to auth-core's database; 🚫 No other plane redefines identity/session ownership |

### user-core (Go · REST :3012)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | _None_ — L1 apex |
| **Published Events** | `user.profile.updated`, `user.preferences.changed` |
| **Auth Mechanism** | JWT validation (tokens issued by auth-core) |
| **Forbidden Patterns** | 🚫 No other plane writes to user-core's database; 🚫 No other plane becomes source of truth for user profiles |

### org-core (Go · REST :8080)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | _None_ — L1 apex |
| **Published Events** | `organization.updated`, `organization.plan.changed`, `organization.deleted`, `organization.quota.exceeded`, `organization.feature.enabled` |
| **Auth Mechanism** | JWT validation |
| **Forbidden Patterns** | 🚫 No other plane writes to org-core's database; 🚫 No other plane becomes source of truth for org/entitlement/quota |
| **⚠️ Known Violation** | org-core currently carries Data Plane responsibilities (document storage concerns) — must be extracted in Phase B Step 3 |

### billing-core (Go · REST :3014)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | _None_ — L1 apex |
| **Published Events** | `billing.account_updated`, `billing.quota_exceeded`, `billing.invoice_created`, `billing.plan_changed` |
| **Auth Mechanism** | JWT validation |
| **Forbidden Patterns** | 🚫 No other plane writes to billing-core's database |

### session-core

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | _None_ — L1 apex |
| **Published Events** | Session lifecycle events (create, expire, revoke) |
| **Auth Mechanism** | JWT validation, session-token management |
| **Forbidden Patterns** | 🚫 No other plane manages session state |

---

## L2 — Data Plane (Source of Truth: Documents, Retrieval, Indexing, Knowledge, Embeddings)

### documents-service (Python · REST :8001 · gRPC :50051)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 auth-core → JWT validation (read-only identity claims) |
| **Published Events** | `documents.created`, `documents.deleted` |
| **Auth Mechanism** | JWT validation from Control Plane |
| **Forbidden Patterns** | 🚫 No other plane writes directly to documents-service Postgres; 🚫 No other plane owns document canonical state |

### retrieval-service (Python · REST :8004 · gRPC :50052)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 auth-core → JWT validation |
| **Published Events** | Retrieval result events (internal) |
| **Auth Mechanism** | JWT validation from Control Plane |
| **Forbidden Patterns** | 🚫 No other plane accesses Qdrant directly; 🚫 No other plane bypasses retrieval-service for vector queries |

### knowledge-index (Worker · :9101)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 auth-core → JWT validation (org context) |
| **Published Events** | `knowledge_units.created` |
| **Auth Mechanism** | Internal service-to-service (JWT-derived org claims) |
| **Forbidden Patterns** | 🚫 No other plane writes knowledge indexes directly |

### embedding-worker (Worker · :9102)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 auth-core → JWT validation (org context) |
| **Published Events** | Embedding completion events (internal) |
| **Auth Mechanism** | Internal service-to-service |
| **Forbidden Patterns** | 🚫 Embedding credentials owned exclusively by Data Plane; 🚫 No other plane provisions embedding API keys |

### Data Plane Infrastructure

| Component | Port | Constraint |
|-----------|------|------------|
| PostgreSQL 16 | :5432 | 🚫 Direct access forbidden from L3–L6 |
| Redis 7 | :6379 | 🚫 Direct access forbidden from L3–L6 |
| Qdrant v1.9.0 | :6333/:6334 | 🚫 Direct access forbidden from L3–L6; only retrieval-service and embedding-worker may connect |

---

## L3 — Ingestion Plane (Source of Truth: Crawl, Import, Sync, External Integrations)

### Quarry (Go · REST :8090)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → auth/org/entitlement/quota; ✅ L2 Data Plane → canonical document delivery via REST/gRPC; ✅ L4 Model Plane v2 → extraction pipelines |
| **Published Events** | Crawl lifecycle events (internal) |
| **Auth Mechanism** | JWT validation, CP entitlement checks |
| **Forbidden Patterns** | 🚫 No direct Data Plane DB access (must use REST/gRPC APIs); 🚫 No owning retrieval/vector/grounding state |

### imports-core (Python · REST :3041)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → auth/org/entitlements/quota |
| **Published Events** | `import.completed` |
| **Auth Mechanism** | JWT validation, CP entitlement checks |
| **Forbidden Patterns** | 🚫 No writing to CP databases; 🚫 No treating org-core as document storage |

### integration-core (External Integrations)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → user/org ownership verification |
| **Published Events** | Integration sync events |
| **Auth Mechanism** | JWT validation, CP user/org checks |
| **Forbidden Patterns** | 🚫 No writing to CP databases; 🚫 No owning user/org state |

---

## L4 — Model Plane v2 (Source of Truth: AI Reasoning, Agent Orchestration, Execution, Capabilities)

### ai-core (Python · REST 8101→8001 · gRPC 50061→50051)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation (identity read-only); ✅ L2 Data Plane → retrieval-service gRPC :50052 (never direct Qdrant/PG) |
| **Published Events** | `ai.request.completed`, `ai.pipeline.error` |
| **Auth Mechanism** | JWT validation from CP |
| **Forbidden Patterns** | 🚫 No direct Data Plane DB/Qdrant access; 🚫 No owning identity/auth state |
| **⚠️ Known Violation** | Legacy ai-core v1 overlap may exist — must be cleaned in Phase B Step 4 |

### agent-core-v2 (REST 8102→8002 · gRPC :50053)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation; ✅ L2 Data Plane → retrieval-service API-only |
| **Published Events** | Agent orchestration events |
| **Auth Mechanism** | JWT validation from CP |
| **Forbidden Patterns** | 🚫 No direct Data Plane access; 🚫 No owning identity/auth |

### execution-core-v2 (REST 8103→8003)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation; ✅ L2 Data Plane → API-only |
| **Published Events** | `execution.step.completed`, `execution.failed` |
| **Auth Mechanism** | JWT validation from CP |
| **Forbidden Patterns** | 🚫 No direct Data Plane access |

### capability-core-v2 (REST 8104→8004)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation |
| **Published Events** | `capability.registered`, `capability.updated` |
| **Auth Mechanism** | JWT validation from CP |
| **Forbidden Patterns** | 🚫 No direct lower-plane DB access |

### llm-worker (REST 8105→8005)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation (billing context) |
| **Published Events** | `llm.completion.done`, `usage.{org}.llm` (NATS → billing-core) |
| **Auth Mechanism** | JWT validation from CP, NATS events for billing |
| **Forbidden Patterns** | 🚫 No direct billing DB writes (event-based billing via NATS only) |

### Model Plane v2 Infrastructure

| Component | Port | Constraint |
|-----------|------|------------|
| reasoning-v2-postgres | :55433 | 🚫 Access restricted to Model Plane v2 services only |
| Redis | :6390 | 🚫 Access restricted to Model Plane v2 services only |
| NATS | :4227/:8227 | ✅ Used for cross-plane event transport (billing events to L1) |
| MinIO | :9000/:9001 | 🚫 Artifacts local to Model Plane v2 only |

---

## L5 — Application Plane (Source of Truth: UX Composition, Collaboration, Notifications)

### convex-backend (:3210/:3211)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation (identity read-only); ✅ L2 Data Plane → retrieval-service REST :8004; ✅ L4 Model Plane v2 → AI_CORE_URL API-only |
| **Published Events** | UX state events, collaboration events |
| **Auth Mechanism** | JWT from CP |
| **Forbidden Patterns** | 🚫 No redefining canonical ownership of lower-plane domains; 🚫 No direct lower-plane DB access |
| **⚠️ Known Bug** | `ORG_CORE_URL` and `AUTH_SERVER_URL` use legacy service names — must be fixed in Phase C Step 8 |

### convex-dashboard (:6791)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L5 convex-backend → local API |
| **Published Events** | _None_ (consumer-only within Application Plane) |
| **Auth Mechanism** | JWT from CP (proxied via convex-backend) |
| **Forbidden Patterns** | 🚫 No direct lower-plane access |

### convex-gateway (:3005→3000)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L5 convex-backend → gateway routing |
| **Published Events** | _None_ (routing layer) |
| **Auth Mechanism** | JWT pass-through |
| **Forbidden Patterns** | 🚫 No direct lower-plane access |

### convex-subscriber (NATS listener)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → org state events via NATS |
| **Published Events** | Org sync completion (internal) |
| **Auth Mechanism** | NATS subscription (org events from CP) |
| **Forbidden Patterns** | 🚫 No writing back to CP; projection/cache only |

### affine-core (:3180)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation; ✅ L2 Data Plane → API-only |
| **Published Events** | Collaboration events |
| **Auth Mechanism** | JWT from CP |
| **Forbidden Patterns** | 🚫 No redefining canonical ownership; 🚫 No direct lower-plane DB access |

### affine-runtime (:47810→3010)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L5 affine-core → local API |
| **Published Events** | Runtime lifecycle events (internal) |
| **Auth Mechanism** | JWT from CP (proxied via affine-core) |
| **Forbidden Patterns** | 🚫 No direct lower-plane access |

### notification-core (:3140)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L1 Control Plane → JWT validation (user context) |
| **Published Events** | `notification.sent`, `notification.read` |
| **Auth Mechanism** | JWT from CP |
| **Forbidden Patterns** | 🚫 Notification data is local to Application Plane; 🚫 No redefining identity/org ownership |

### Application Plane Infrastructure

| Component | Port | Constraint |
|-----------|------|------------|
| application-postgres | :9540→5432 | 🚫 Access restricted to Application Plane services only |
| application-redis | :6480→6379 | 🚫 Access restricted to Application Plane services only |

---

## L6 — Frontend Plane (Consumer-Only Layer)

### velion (Next.js · :3000)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L5 Application Plane → REST/WebSocket APIs; ✅ L1 auth-core → HTTP-only auth cookies (login/logout flows) |
| **Published Events** | _None_ — consumer-only layer |
| **Auth Mechanism** | HTTP-only auth cookies from auth-core; environment-driven URLs (`NEXT_PUBLIC_*`) |
| **Forbidden Patterns** | 🚫 No direct DB access; 🚫 No bypassing Application Plane to call L2–L4 directly; 🚫 L5 is the ceiling (all backend data must flow through Application Plane) |

### triodelab-web (Static)

| Aspect | Detail |
|--------|--------|
| **Allowed Dependencies** | ✅ L5 Application Plane → REST APIs |
| **Published Events** | _None_ — static consumer |
| **Auth Mechanism** | HTTP-only auth cookies from auth-core |
| **Forbidden Patterns** | 🚫 Same as velion — no direct DB, no bypassing L5 |

---

## Plane-to-Plane Interaction Summary

| Caller ↓ / Target → | L1 Control | L2 Data | L3 Ingestion | L4 Model v2 | L5 Application | L6 Frontend |
|----------------------|------------|---------|--------------|-------------|-----------------|-------------|
| **L1 Control** | — | 🚫 | 🚫 | 🚫 | 🚫 | 🚫 |
| **L2 Data** | ✅ JWT validation | — | 🚫 | 🚫 | 🚫 | 🚫 |
| **L3 Ingestion** | ✅ Auth/Org/Quota | ✅ REST/gRPC | — | ✅ Extraction | 🚫 | 🚫 |
| **L4 Model v2** | ✅ JWT + Billing events | ✅ Retrieval API | 🚫 | — | 🚫 | 🚫 |
| **L5 Application** | ✅ JWT + Org events | ✅ Retrieval API | 🚫 | ✅ AI API | — | 🚫 |
| **L6 Frontend** | ✅ Auth cookies | 🚫 | 🚫 | 🚫 | ✅ REST/WS | — |

**Rules governing this matrix:**
1. Authority flows DOWN only (upper calls lower)
2. Events are integration signals, NOT canonical ownership transfers
3. No cross-plane database writes — ever
4. No private Docker network as a contract boundary
5. Application and Frontend may project/cache but NOT redefine canonical ownership

---

## Known Violations (Migration Backlog)

| # | Violation | Location | Fix Phase |
|---|-----------|----------|-----------|
| 1 | org-core carries Data Plane responsibilities | L1 org-core | Phase B Step 3 |
| 2 | Legacy ai-core v1 overlap with Model Plane v2 | L4 ai-core | Phase B Step 4 |
| 3 | Root docker-compose duplicates plane-level definitions | `/docker-compose.yml` | Phase C Step 8 |
| 4 | Application Plane uses legacy service hostnames (`ORG_CORE_URL`, `AUTH_SERVER_URL`) | L5 convex-backend env | Phase C Step 8 |

---

_This matrix is the migration checklist and review gate for all later PRs. Any PR that introduces a cross-plane dependency MUST be validated against this matrix before merge._
