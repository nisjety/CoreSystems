# Docker Compose Duplicate & Conflict Fixes

> **Scope:** Canonical plane stacks, production overrides, and deprecated root overlays.
> **Updated:** 2026-07-12
> **Status:** Source cleanup implemented and statically validated; live rebuild awaits Docker content-store recovery.

## 2026-07-12 implementation update

- The six canonical runtime stacks are now the plane-level Compose files for
  Data v2, Control, Ingestion, Model, Application, and Frontend Velion v3.
- The root monolith is quarantined behind the explicit `legacy-monolith`
  profile. Its floating images and embedded development credentials were
  removed; it is not part of the production bootstrap path.
- Root Zammad and unverified Nohu duplicates were removed. Zammad is owned by
  the Application Plane overlay; active connector ownership is Ingestion
  `integration-corev2`.
- Shared NATS ownership is Frontend Plane Velion v3. The broker and every
  canonical plane-local NATS instance require authentication.
- Production overrides remove internal host publications and harden first-party
  processes with non-root users, dropped capabilities, immutable roots where
  compatible, bounded scratch space, init handling, and graceful shutdowns.
- Velion v3 now has separate `dev` and `production` image targets. Production
  serves a prebuilt SPA through digest-pinned unprivileged nginx; the Rust BFF
  remains internal.
- Compose dependency order is Data → Control → Ingestion → Model → Application
  → Frontend, avoiding Auth/JWKS readiness deadlocks.
- All canonical base and production overlays render with synthetic required
  values. Real promotion still requires operator-provisioned secrets and the
  already-approved Docker Desktop content-store recovery.

The remainder of this document is the historical conflict inventory that led
to the implemented cleanup. Names and port examples below may describe the
pre-cleanup layout.

---

## Executive Summary

The CoreSystem uses **8 docker-compose files** spread across 7 planes plus a legacy root file. Analysis reveals:

- **6 critical host-port conflicts** that prevent simultaneous plane operation
- **2 velion-net internal port collisions** between Data Plane and Model Plane v2
- **4 cross-plane reference issues** (v1-only hardcoded addresses)
- **1 legacy root compose** (`docker-compose.yml`) that predates the plane architecture and uses an incompatible network (`aquatiq-local`)

### Compose File Inventory

| # | File | Project Name | Private Network | Services |
|---|------|-------------|-----------------|----------|
| 1 | `apps/Control Plane/docker-compose.yml` | `control-plane` | controlplane-net | auth-core, user-core, org-core, billing-core, session-core, Lago suite |
| 2 | `apps/Data Plane/docker-compose.yml` | `data-plane` | data-net | documents-service, knowledge-index, embedding-worker, retrieval-service |
| 3 | `apps/Ingestion Plane/docker-compose.yml` | `ingestion-plane` | ingestion-net | quarry-api, quarry-worker, imports-api, integration-api, finspo-api, connector-runtime-engine |
| 4 | `apps/Model Plane/docker-compose.yml` | `model-plane` | model-plane-net | ai-core v1, agent-core v1 |
| 5 | `apps/Model Plane v2/docker-compose.yml` | `model-plane-v2` | reasoning-net | agent-core-v2, execution-core-v2, capability-core-v2, llm-worker, ai-core v2 gateway |
| 6 | `apps/Application Plane/docker-compose.yml` | `application-plane` | app-net | convex-backend, convex-dashboard, convex-gateway, convex-subscriber, notification-core, AFFiNE |
| 7 | `apps/Frontend Plane/velion/docker-compose.yml` | `frontend-plane-agencia` | _(none — velion-net only)_ | frontend |
| 8 | `docker-compose.yml` (root) | _(default)_ | _(none)_ | **LEGACY** — duplicates many plane services on aquatiq-local network |

---

## 1. Network Topology

### Production Networks (Plane Architecture)

```
┌──────────────────────────────────────────────────────────┐
│                    velion-net (external, shared)          │
│  All planes connect here for cross-plane communication   │
└──────────────────────────────────────────────────────────┘
       │            │            │           │          │
  ┌────┴────┐  ┌────┴────┐  ┌───┴───┐  ┌───┴───┐  ┌──┴──┐
  │control- │  │ data-   │  │ingest-│  │model- │  │app- │
  │plane-net│  │  net    │  │ion-net│  │plane- │  │ net │
  │(bridge) │  │(bridge) │  │(bridge│  │net /  │  │(br.)│
  │         │  │         │  │       │  │reason-│  │     │
  │         │  │         │  │       │  │ing-net│  │     │
  └─────────┘  └─────────┘  └───────┘  └───────┘  └─────┘
```

Frontend Plane has **no private network** — it connects only to velion-net.

### Legacy Network (Root Compose)

```
┌──────────────────────────────────────────────────────┐
│               aquatiq-local (external)               │
│  Used ONLY by root docker-compose.yml (LEGACY)       │
│  Also declares controlplane-network (external)       │
│  INCOMPATIBLE with plane architecture                │
└──────────────────────────────────────────────────────┘
```

**Recommendation:** Deprecate `aquatiq-local` network and migrate root compose services into their respective planes.

---

## 2. Critical Host Port Conflicts

### 2a. Ingestion Plane — Default Port Collisions

🔴 **CRITICAL** — Ingestion Plane binds to system default ports, blocking any host-local PostgreSQL or Redis.

| Service | Current Binding | Conflict With | Fix |
|---------|----------------|---------------|-----|
| `ingestion-postgres` | `5432:5432` | Host PostgreSQL / other planes | → `5435:5432` |
| `ingestion-redis` | `6379:6379` | Host Redis / other planes | → `6382:6379` |

### 2b. Model Plane v1 ↔ v2 — Host Port 8101

🔴 **CRITICAL** — Both planes bind host port 8101 simultaneously.

| Host Port | Model Plane v1 | Model Plane v2 |
|-----------|---------------|----------------|
| `8101` | agent-core v1 (`8101:8001`) | ai-core v2 gateway (`8101:8001`) |

**Fix options (pick one):**
- **Option A:** Move v1 agent-core to `8106:8001` (preferred — v1 is sunset path)
- **Option B:** Move v2 ai-core gateway to `8107:8001`

### 2c. Root Compose ↔ Plane Conflicts

🔴 **CRITICAL** — Root compose duplicates plane services on the SAME host ports but different networks, preventing coexistence.

| Host Port | Plane Service | Root Compose Service |
|-----------|--------------|---------------------|
| `3000` | frontend (Frontend Plane) | frontend (root) |
| `5433` | controlplane-postgres (Control Plane) | org-core-temporal-postgres (root) |
| `7233` | temporal (Ingestion Plane) | org-core-temporal (root) |
| `8080` | org-core (Control Plane) | org-core (root) |
| `3011` | auth-core (Control Plane) | auth-service (root) |
| `3012` | user-core (Control Plane) | user-service (root) |
| `3210`/`3211` | convex-backend (Application Plane) | convex-backend (root) |
| `3140` | notification-core (Application Plane) | notification-core (root) |
| `6791` | convex-dashboard (Application Plane) | convex-dashboard (root) |

**Fix:** Deprecate root `docker-compose.yml` entirely. See [Section 6](#6-root-compose-deprecation-plan).

### 2d. Qdrant Port Risk

🟡 **MEDIUM** — Ingestion Plane and Data Plane use different host ports but the ingestion ports conflict with default Qdrant ports if run on bare metal.

| Plane | Host Ports | Container Ports |
|-------|-----------|-----------------|
| Data Plane | `6335:6333`, `6336:6334` | ✅ Safe |
| Ingestion Plane | `6333:6333`, `6334:6334` | ⚠️ Default ports |

**Fix:** Remap Ingestion qdrant to `6337:6333`, `6338:6334`.

---

## 3. Velion-net Internal Port Collisions

When services from different planes join `velion-net`, their **container ports** must be unique across the entire shared network. Two collisions exist:

### 3a. Container Port 8001

| Service | Plane | Container Port | Role |
|---------|-------|---------------|------|
| `documents-service` | Data Plane | `8001` | Document management HTTP |
| `ai-core` v2 gateway | Model Plane v2 | `8001` | AI gateway HTTP |

**Fix:** Change Model Plane v2 `ai-core` gateway container port to `8011` (update both `EXPOSE` and host mapping from `8101:8001` → `8101:8011`).

### 3b. Container Port 8004

| Service | Plane | Container Port | Role |
|---------|-------|---------------|------|
| `retrieval-service` | Data Plane | `8004` | Vector retrieval HTTP |
| `capability-core-v2` | Model Plane v2 | `8004` | Tool catalog / MCP registry |

**Fix:** Change Model Plane v2 `capability-core-v2` container port to `8014` (update both `EXPOSE` and host mapping from `8104:8004` → `8104:8014`).

---

## 4. Cross-Plane Reference Audit

### 4a. Quarry Worker → AI Core (v1 Only)

**File:** `apps/Ingestion Plane/docker-compose.yml`
**Problem:** `quarry-worker` hardcodes `ai-core:50051` and `ai-core:8001` — these resolve to Model Plane v1 only.

```yaml
# Current (v1-only)
AI_CORE_GRPC_URL: "ai-core:50051"
AI_CORE_HTTP_URL: "http://ai-core:8001"
```

**Fix:** Route through `session-core` (Control Plane) for v1/v2 canary routing, or add explicit v2 env vars when `MODEL_PLANE_V2_ROLLOUT_PCT > 0`.

### 4b. Convex Backend → AI Core (v1 Only)

**File:** `apps/Application Plane/docker-compose.yml`
**Problem:** `convex-backend` references `ai-core:8000` — v1 only.

```yaml
AI_CORE_URL: "http://ai-core:8000"
```

**Fix:** Route through `session-core` gateway or update to reference the appropriate ai-core version based on rollout configuration.

### 4c. Root Compose Frontend → Legacy Services

**File:** `docker-compose.yml` (root)
**Problem:** Frontend references services by legacy names on `aquatiq-local` network:

```yaml
AI_CORE_URL: "http://ai-core:8000"           # v1 only
REASONING_CORE_URL: "http://reasoning-core:8000"  # unknown service
DOCS_SERVICE_URL: "http://documents-service:8001"
RETRIEVAL_SERVICE_URL: "http://retrieval-service:8004"
QUARRY_URL: "http://quarry-api:8090"
```

**Fix:** Deprecated with root compose — Frontend Plane's compose already uses proper velion-net references.

### 4d. Root Compose Notification Core → Legacy Infrastructure

**File:** `docker-compose.yml` (root)
**Problem:** `notification-core` references legacy infrastructure names:

```yaml
NATS_URL: "nats://aquatiq-nats-local:4222"
REDIS_URL: "redis://aquatiq-redis-local:6379"
```

**Fix:** The Application Plane version of `notification-core` already uses proper references. Deprecate root compose copy.

---

## 5. Complete Host Port Allocation Matrix

### Recommended Port Ranges per Plane

| Plane | Service Ports | Infrastructure Ports | gRPC Ports |
|-------|--------------|---------------------|------------|
| Control | 3011–3019 | 4223, 5433, 6380, 8223 | 50011–50017 |
| Data | 8001–8004 | 6335–6336 | 50051–50052 |
| Ingestion | 3025–3027, 3126–3130, 8090 | **5435**, **6337–6338**, **6382**, 4224, 7233 | — |
| Model v1 | 8000, **8106** | 4225, 7333–7334, 7574, 7787, 55432, 6389, 8225 | 51051–51052 |
| Model v2 | 8102–8105, 8101 | 4227, 55433, 6390, 8227, 9000–9001 | 50053, 50061 |
| Application | 3005, 3140, 3180, 3210–3211 | 6791, 9540, 6480, 47810 | — |
| Frontend | 3000 | — | — |
| Lago (Control) | 3015–3016 | 5434, 6381 | — |

**Bold** = changed from current values.

### Full Service-to-Port Mapping

<details>
<summary>Click to expand complete port table</summary>

| Host Port | Service | Plane | Protocol |
|-----------|---------|-------|----------|
| 3000 | frontend | Frontend | HTTP |
| 3003 | connector-runtime-engine | Ingestion | HTTP |
| 3005 | convex-gateway | Application | HTTP |
| 3011 | auth-core | Control | HTTP |
| 3012 | user-core | Control | HTTP |
| 3014 | billing-core | Control | HTTP |
| 3015 | lago-front | Control (Lago) | HTTP |
| 3016 | lago-api | Control (Lago) | HTTP |
| 3017 | session-core | Control | HTTP |
| 3025 | imports-api | Ingestion | HTTP |
| 3026 | integration-api | Ingestion | HTTP |
| 3027 | integration-worker | Ingestion (legacy) | HTTP |
| 3126 | integration-engine-go-api | Ingestion (legacy) | HTTP |
| 3127 | integration-engine-go-worker | Ingestion (legacy) | HTTP |
| 3130 | finspo-api | Ingestion | HTTP |
| 3140 | notification-core | Application | HTTP |
| 3180 | affine-core | Application (affine) | HTTP |
| 3210 | convex-backend (http) | Application | HTTP |
| 3211 | convex-backend (site) | Application | HTTP |
| 4223 | controlplane-nats | Control | NATS |
| 4224 | ingestion-nats | Ingestion | NATS |
| 4225 | reasoning-nats (v1) | Model v1 | NATS |
| 4227 | reasoning-v2-nats | Model v2 | NATS |
| 5433 | controlplane-postgres | Control | PG |
| 5434 | lago-db | Control (Lago) | PG |
| **5435** | ingestion-postgres | Ingestion | PG |
| 6335 | data-qdrant (http) | Data | HTTP |
| 6336 | data-qdrant (grpc) | Data | gRPC |
| **6337** | ingestion-qdrant (http) | Ingestion | HTTP |
| **6338** | ingestion-qdrant (grpc) | Ingestion | gRPC |
| 6380 | controlplane-redis | Control | Redis |
| 6381 | lago-redis | Control (Lago) | Redis |
| **6382** | ingestion-redis | Ingestion | Redis |
| 6389 | reasoning-redis (v1) | Model v1 | Redis |
| 6390 | reasoning-v2-redis | Model v2 | Redis |
| 6480 | application-redis | Application | Redis |
| 6791 | convex-dashboard | Application | HTTP |
| 7233 | temporal | Ingestion | Temporal |
| 7333 | reasoning-qdrant (v1 http) | Model v1 | HTTP |
| 7334 | reasoning-qdrant (v1 grpc) | Model v1 | gRPC |
| 7574 | reasoning-neo4j (http) | Model v1 | HTTP |
| 7787 | reasoning-neo4j (bolt) | Model v1 | Bolt |
| 8000 | ai-core v1 | Model v1 | HTTP |
| 8080 | org-core (http) | Control | HTTP |
| 8088 | **DEPRECATED** org-core-temporal-ui | Root (legacy) | HTTP |
| 8090 | quarry-api | Ingestion | HTTP |
| 8101 | ai-core v2 gateway | Model v2 | HTTP |
| 8102 | agent-core-v2 | Model v2 | HTTP |
| 8103 | execution-core-v2 | Model v2 | HTTP |
| 8104 | capability-core-v2 | Model v2 | HTTP |
| 8105 | llm-worker | Model v2 | HTTP |
| **8106** | agent-core v1 | Model v1 | HTTP |
| 8223 | controlplane-nats (monitor) | Control | HTTP |
| 8225 | reasoning-nats (v1 monitor) | Model v1 | HTTP |
| 8227 | reasoning-v2-nats (monitor) | Model v2 | HTTP |
| 9000 | reasoning-v2-minio (api) | Model v2 | HTTP |
| 9001 | reasoning-v2-minio (console) | Model v2 | HTTP |
| 9090 | org-core (grpc) | Control | gRPC |
| 9091 | org-core (debug) | Control | HTTP |
| 9101 | knowledge-index | Data | HTTP |
| 9102 | embedding-worker | Data | HTTP |
| 9540 | application-postgres | Application | PG |
| 39090 | prometheus (v1 monitoring) | Model v1 | HTTP |
| 33000 | grafana (v1 monitoring) | Model v1 | HTTP |
| 47810 | affine-runtime | Application (affine) | HTTP |
| 50011 | auth-core | Control | gRPC |
| 50012 | user-core | Control | gRPC |
| 50013 | billing-core | Control | gRPC |
| 50017 | session-core | Control | gRPC |
| 50051 | documents-service | Data | gRPC |
| 50052 | retrieval-service | Data | gRPC |
| 50053 | agent-core-v2 | Model v2 | gRPC |
| 50061 | ai-core v2 gateway | Model v2 | gRPC |
| 51051 | ai-core v1 | Model v1 | gRPC |
| 51052 | agent-core v1 | Model v1 | gRPC |
| 55432 | reasoning-postgres (v1) | Model v1 | PG |
| 55433 | reasoning-v2-postgres | Model v2 | PG |
| 6060 | user-core (pprof) | Control | HTTP |
| 6061 | org-core (pprof) | Control | HTTP |
| 6062 | billing-core (pprof) | Control | HTTP |

</details>

---

## 6. Root Compose Deprecation Plan

The root `docker-compose.yml` is a **legacy monolith** that predates the plane-based architecture. It uses the `aquatiq-local` network (not `velion-net`) and duplicates services from Control, Application, and Frontend planes with **different port mappings and configurations**.

**2026-07-12 safety update:** every root service is quarantined behind the
explicit `legacy-monolith` profile. A normal root-level `docker compose up`
can no longer start duplicate plane services. The definitions remain available
temporarily for compatibility with `--profile legacy-monolith`; all normal
builds and deployments must use the six canonical plane-level Compose files.

### Unique Services in Root Compose

| Service | Port | Status |
|---------|------|--------|
| `letta-server` | 8283 | ⚠️ Only exists in root compose. Must be assigned to a plane or removed. |
| `org-core-temporal` | 7233 | Duplicates Ingestion Plane temporal. Consolidate to a single Temporal cluster. |
| `org-core-temporal-ui` | 8088 | Admin UI for temporal. Move to Control Plane or remove. |
| `org-core-temporal-postgres` | 5433 | Conflicts with controlplane-postgres. Consolidate. |

### Deprecation Steps

1. **Audit `letta-server`** — Determine if it belongs in Application Plane or Model Plane. Create service definition in the appropriate plane compose file.
2. **Consolidate Temporal** — Merge org-core-temporal into Ingestion Plane's existing temporal instance (or create a shared Temporal in Control Plane).
3. **Verify all root compose services** have plane equivalents with correct configurations.
4. **Rename** root compose to `docker-compose.legacy.yml`.
5. **Delete** `aquatiq-local` network references across all files.

---

## 7. V1 ↔ V2 Model Plane Coexistence Checklist

Both Model Plane v1 and v2 are designed to run simultaneously with canary routing via `session-core`.

| Check | Status | Notes |
|-------|--------|-------|
| Host port 8101 conflict resolved | ❌ | v1 agent-core → 8106 |
| velion-net port 8001 collision resolved | ❌ | v2 ai-core gateway → 8011 |
| velion-net port 8004 collision resolved | ❌ | v2 capability-core → 8014 |
| Separate Postgres instances | ✅ | v1: 55432, v2: 55433 |
| Separate Redis instances | ✅ | v1: 6389, v2: 6390 |
| Separate NATS instances | ✅ | v1: 4225, v2: 4227 |
| Separate Qdrant instances | ✅ | v1: 7333/7334, v2: none (uses MinIO) |
| Canary routing via session-core | ✅ | `MODEL_PLANE_V2_ROLLOUT_PCT` env var |
| Cross-plane refs updated for v2 | ❌ | quarry-worker, convex-backend still hardcode v1 |

---

## 8. Implementation Priority

### Phase 1 — Immediate Fixes (No Service Changes)

```bash
# 1. Ingestion Plane port remapping
# File: apps/Ingestion Plane/docker-compose.yml
# Change:
#   ingestion-postgres: 5432:5432 → 5435:5432
#   ingestion-redis:    6379:6379 → 6382:6379
#   qdrant:             6333:6333 → 6337:6333
#                       6334:6334 → 6338:6334

# 2. Model Plane v1 — resolve host port 8101
# File: apps/Model Plane/docker-compose.yml
# Change:
#   agent-core: 8101:8001 → 8106:8001
```

### Phase 2 — Velion-net Collision Fixes (Requires Container Rebuilds)

```bash
# 3. Model Plane v2 — resolve velion-net port 8001
# File: apps/Model Plane v2/docker-compose.yml
# Change ai-core gateway container port: 8001 → 8011
# Update: host mapping 8101:8001 → 8101:8011
# Update: all internal references to ai-core:8001 → ai-core:8011

# 4. Model Plane v2 — resolve velion-net port 8004
# File: apps/Model Plane v2/docker-compose.yml
# Change capability-core-v2 container port: 8004 → 8014
# Update: host mapping 8104:8004 → 8104:8014
# Update: all internal references to capability-core-v2:8004 → capability-core-v2:8014
```

### Phase 3 — Cross-Plane Reference Updates

```bash
# 5. Update quarry-worker AI Core references to support v2 canary
# 6. Update convex-backend AI Core reference for v2 routing
# 7. Audit all services for hardcoded v1-only addresses
```

### Phase 4 — Root Compose Deprecation

```bash
# 8. Assign letta-server to a plane
# 9. Consolidate Temporal instances
# 10. Rename root compose to docker-compose.legacy.yml
# 11. Remove aquatiq-local network
```

---

## Appendix: Root Compose AI-Core Port Discrepancy

The root compose defines `ai-core` on ports `8040:8000` and `50014:50051` — completely different from both:
- Model Plane v1: `8000:8000`, `51051:50051`
- Model Plane v2: `8101:8001`, `50061:50051`

This confirms the root compose is a separate, outdated configuration that must not be mixed with plane-level deployments.
