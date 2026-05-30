# Architecture Decision: Cross-Plane Contracts

**Date:** April 8, 2026  
**Decision:** Enforce the pyramid architecture through explicit cross-plane API, event, and state-sharing rules.

---

## Context

CoreSystem uses plane-based microservices, but the operating model is hierarchical:

- Control Plane is the authority root
- Data Plane is the canonical product-data layer
- Ingestion Plane and Model Plane v2 consume lower-plane capabilities
- Application Plane is optional and non-authoritative
- Frontend Plane composes selected planes into end-user applications

Without explicit cross-plane rules, service docs and compose files drift toward direct coupling, mixed ownership, and plane-boundary violations.

---

## Decision

Cross-plane integrations must use **published APIs** and **published events** only. Plane boundaries are authority boundaries, not just deployment folders.

### Allowed API Directions

- Upper planes may call **Control Plane** for auth, user, org, billing, entitlement, quota, and session-domain validation.
- Upper planes may call **Data Plane** for canonical product data storage, retrieval, indexing, and grounding.
- **Application Plane** may call Control Plane, Data Plane, Ingestion Plane, and Model Plane v2 to compose product experiences.
- **Frontend Plane** may call the backend planes needed by a specific frontend application.

### Allowed Event Directions

- Control Plane may publish authority events to all other planes.
- Data Plane may publish document, retrieval, indexing, and knowledge-state events.
- Ingestion Plane may publish crawl, import, sync, and ingestion-status events.
- Model Plane v2 may publish runtime, reasoning, usage, and orchestration events.
- Application Plane may publish UX, collaboration, and notification events.

Events are integration signals. They do **not** transfer canonical ownership.

### Forbidden Shared-State Patterns

- No plane may write directly into another plane's database.
- No plane may depend on another plane's private schema as an integration contract.
- No plane may require another plane's private Docker network as the contract boundary.
- No upper plane may become the source of truth for a lower plane's authority domain.
- Application Plane and Frontend Plane may project or cache state, but may not redefine canonical ownership.

---

## Consequences

### Positive

- Clear authority boundaries
- Independently deployable planes
- Safer refactors and service replacement
- Easier reasoning about ownership and contract surfaces

### Required Follow-Up

- Service-level docs must describe their allowed lower-plane dependencies.
- Compose files and env contracts should gradually standardize around published lower-plane endpoints.
- Cross-plane integrations that still point at the wrong authority surface must be corrected over time.

---

## Immediate Enforcement Standard

When choosing a downstream dependency:

1. If the need is identity, org, billing, quota, entitlement, or session validation, use **Control Plane**.
2. If the need is canonical product data, retrieval, indexing, or grounding, use **Data Plane**.
3. If the need is app-specific collaboration or notification UX, use **Application Plane** only as a projection or composition layer.
4. If an integration requires direct database access across planes, the design is invalid and must be replaced with an API or event contract.