# Application Plane Architecture

**Last Updated:** 2025-07-08

## Pyramid Placement

The **Application Plane** sits at **Layer 5** of the CoreSystem pyramid. It is
the collaborative workspace and real-time synchronisation layer. It receives
identity and org context from the Control Plane (L1), retrieves documents from
the Data Plane (L2), consumes AI capabilities from the Model Plane v2 (L4), and
presents interfaces upward to the Frontend Plane (L6). Authority flows strictly
downward — Application Plane **never** writes to Control, Data, Ingestion, or
Model Plane databases.

### Authority Rules

- ✅ **Canonical owner** of collaborative workspaces, real-time document sync,
  and notification delivery.
- ✅ **Canonical owner** of its own databases: `application-postgres`
  (notifications DB) and `application-redis`.
- ✅ **May read** Control Plane identity and org data via NATS subscription and
  REST APIs.
- ✅ **May call** Model Plane v2 AI endpoints for AI-powered workspace features.
- ❌ Does **not** own user identity, org hierarchy, or billing (→ Control Plane).
- ❌ Does **not** own document storage, embeddings, or retrieval (→ Data Plane).
- ❌ Does **not** own raw-content ingestion or import jobs (→ Ingestion Plane).
- ❌ Does **not** own AI reasoning, agent orchestration, or LLM routing (→ Model Plane v2).

> **⚠️ Known Violations:** `convex-backend` currently uses incorrect service
> names — `ORG_CORE_URL=org-core-service:8080` (should be `org-core:8080`) and
> `AUTH_SERVER_URL=auth-service:3011` (should be `auth-core:3011`). These are
> tracked for remediation in Phase C of the architecture alignment plan.

---

## 🏗️ Service Structure

```
┌─────────────────────────────────────────────────────────────────────┐
│                   APPLICATION PLANE  (Layer 5)                      │
│                                                                     │
│  ┌───────────────┐  ┌────────────────┐  ┌───────────────────┐      │
│  │ convex-backend│  │convex-dashboard│  │  convex-gateway   │      │
│  │ :3210/:3211   │  │    :6791       │  │   :3005 → :3000   │      │
│  │   (Rust)      │  │  (dashboard)   │  │   (Node.js GW)    │      │
│  └───────────────┘  └────────────────┘  └───────────────────┘      │
│                                                                     │
│  ┌──────────────────┐  ┌──────────────┐  ┌──────────────────┐      │
│  │convex-subscriber │  │  affine-core │  │ affine-runtime   │      │
│  │(nats-subscriber) │  │    :3180     │  │  :47810 → :3010  │      │
│  └──────────────────┘  └──────────────┘  └──────────────────┘      │
│                                                                     │
│  ┌──────────────────┐                                               │
│  │notification-core │                                               │
│  │      :3140       │                                               │
│  └──────────────────┘                                               │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐     │
│  │                    Infrastructure                          │     │
│  │  application-postgres :9540 → :5432  (notifications DB)   │     │
│  │  application-redis    :6480 → :6379  (256 MB)             │     │
│  └────────────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 📦 Service Responsibilities

### 1. **convex-backend** (Port 3210, HTTP 3211)

| Attribute | Value |
|-----------|-------|
| **Domain** | Collaborative workspace backend — real-time data sync |
| **Technology** | Rust |
| **Database** | SQLite (embedded) |
| **Memory** | 512 MB |

**Owns:**
- ✅ Real-time collaborative document state
- ✅ Workspace data functions and mutations
- ✅ SQLite-backed per-workspace persistence

**Key Environment:**
- `AI_CORE_URL` — calls Model Plane v2 for AI features
- `ORG_CORE_URL=org-core-service:8080` ⚠️ should be `org-core:8080`
- `AUTH_SERVER_URL=auth-service:3011` ⚠️ should be `auth-core:3011`

---

### 2. **convex-dashboard** (Port 6791)

| Attribute | Value |
|-----------|-------|
| **Domain** | Admin dashboard for Convex workspace management |
| **Technology** | Web UI |

**Owns:**
- ✅ Workspace administration interface

---

### 3. **convex-gateway** (Port 3005 → 3000)

| Attribute | Value |
|-----------|-------|
| **Domain** | HTTP/WebSocket gateway for real-time client connections |
| **Technology** | Node.js |

**Owns:**
- ✅ Client connection management and WebSocket routing
- ✅ Request proxying to convex-backend

---

### 4. **convex-subscriber** (nats-subscriber.js)

| Attribute | Value |
|-----------|-------|
| **Domain** | NATS event bridge — syncs Control Plane state into Application Plane |
| **Technology** | Node.js |

**Owns:**
- ✅ NATS subscription to Control Plane events (org, user changes)
- ✅ Local state synchronisation from Control Plane into Convex

**Events Subscribed:** Control Plane NATS subjects (org/user lifecycle events)

---

### 5. **affine-core** (Port 3180)

| Attribute | Value |
|-----------|-------|
| **Domain** | AFFiNE collaborative editor backend |
| **Technology** | Node.js |

**Owns:**
- ✅ Collaborative document editing (block-based editor)
- ✅ Workspace and page management

---

### 6. **affine-runtime** (Port 47810 → 3010)

| Attribute | Value |
|-----------|-------|
| **Domain** | AFFiNE runtime and migration engine |
| **Technology** | Node.js |

**Owns:**
- ✅ Runtime process management for AFFiNE
- ✅ Schema migrations for AFFiNE data

---

### 7. **notification-core** (Port 3140)

| Attribute | Value |
|-----------|-------|
| **Domain** | Notification delivery and management |
| **Technology** | Node.js |
| **Database** | application-postgres (notifications DB) |

**Owns:**
- ✅ Notification lifecycle (create, deliver, mark-read, archive)
- ✅ Notification preferences and channel routing

**Events Subscribed:** Notification triggers from other planes via NATS

---

## 🔧 Infrastructure

| Component | Image / Version | Port(s) | Purpose |
|-----------|----------------|---------|---------|
| application-postgres | postgres:16 | 9540 → 5432 | Persistent store for notification-core |
| application-redis | redis:7-alpine | 6480 → 6379 | Shared cache (256 MB maxmemory) |

### Networks

| Network | Purpose |
|---------|---------|
| `application-network` | Internal mesh for all Application Plane services |
| `control-plane-network` | Read-only access to Control Plane NATS and auth APIs |
| `dataplane-network` | Read-only access to Data Plane document APIs |
| `reasoning-v2-network` | Read-only access to Model Plane v2 AI endpoints |

---

## Does NOT Own

| Capability | Canonical Owner | How Application Plane Accesses |
|---|---|---|
| User identity & sessions | Control Plane (auth-core, session-core) | Validates JWT; convex-subscriber syncs state |
| Org hierarchy & billing | Control Plane (org-core, billing-core) | REST API calls (⚠️ wrong service name) |
| Document storage & embeddings | Data Plane (documents-service) | REST / gRPC calls |
| Retrieval & vector search | Data Plane (retrieval-service) | REST / gRPC calls |
| Raw content ingestion | Ingestion Plane (Quarry, imports-core) | Does not access directly |
| AI reasoning & agents | Model Plane v2 (ai-core, agent-core-v2) | REST calls via AI_CORE_URL |
| UI rendering | Frontend Plane (velion) | Serves API responses to frontend |

---

## Cross-Plane Contract Rules

1. **Identity is read-only.** Application Plane validates Control Plane JWT
   tokens but never issues, rotates, or stores them.
2. **Org state is synced via NATS.** `convex-subscriber` receives org/user
   lifecycle events from Control Plane NATS — it never writes to Control Plane
   databases.
3. **AI access is API-only.** All AI capabilities are consumed via Model Plane v2
   REST endpoints through `AI_CORE_URL`.
4. **Notification data is local.** `application-postgres` is owned exclusively by
   the Application Plane for notification storage.
5. **Service name alignment required.** `ORG_CORE_URL` and `AUTH_SERVER_URL` in
   `convex-backend` use legacy service names and must be updated to use
   canonical Control Plane service names (`org-core`, `auth-core`).

---

## 2026-05-20 — Velion Build Runtime Audit

Build orchestrator: `apps/Frontend Plane/velion/build-velion-services.sh`. Application Plane is index 4 in the build order; bootstrap one-shot `affine-runtime-migration` is removed on exit-0; post-build hook `deploy_convex_functions` runs `npx convex deploy` against `convex-backend:3210`.

### Observed compose roster (this run)
- Compose project: `application-plane`
- Networks: `app-net` (private) + `inter-plane-bus` (shared)
- Services on inter-plane-bus: `convex-backend`, `convex-subscriber`, `affine-runtime`, `notification-core`, `application-postgres`, `application-redis`
- Services on app-net only (NOT reachable from velion by name): `convex-dashboard:6791`, `convex-gateway:3000` (host 3005)

### Build status
- Not yet reached in the 2026-05-20 run. Build halted upstream in Model Plane (`capability-core` go.sum, `orchestrator-core` go.mod replace-path context).
- `docker compose -f apps/Application\ Plane/docker-compose.yml config --quiet` validates clean.

### Stale env defaults (still present, surface bug 5 in §Authority Rules above)
- `convex-backend` env: `ORG_CORE_URL=http://org-core-service:8080` → should be `org-core:8080`
- `convex-backend` env: `AUTH_SERVER_URL=http://auth-service:3011` → should be `auth-core:3011`
- `convex-backend` env: `AI_CORE_URL=http://ai-core:8000` → Model Plane gateway lives at `model-gateway:8080` on `model-plane-network` (not inter-plane-bus).

### Cross-plane NATS subjects observed in compose (`convex-subscriber`)
- Primary bus: `nats://velion-nats:4222` (velion-side bus on inter-plane-bus)
- Secondary bus: `nats://model-plane-nats-1:4222` (Model Plane isolated cluster) — dual-connect added for W4-2 so `mp.v1.run.*.event` reaches the agent run lifecycle mirror.

### Remediation
1. Fix `convex-backend` env defaults to match live container names — Control Plane uses `container_name: auth-service / org-core-service` overrides, so the defaults already match. No action needed.
2. Decide whether `convex-dashboard` / `convex-gateway` need to be on `inter-plane-bus`; if yes, extend their `networks:` block. Velion server-side currently uses host-port URLs for the dashboard, so this can stay deferred.
3. Resolve Model Plane network isolation (see `Model Plane/docs/gap-model.md` §12 remediation #3) — still relevant; convex-subscriber's fallback to the velion-nats cluster works for now.

## 2026-05-20 — Verified all-green (R15)

Final run brought the Application Plane stack to **8/8 running** under compose project `application-plane`. The post-build hook `deploy_convex_functions` succeeded (idempotent `npx convex deploy` against convex-backend:3210).

### Container roll-up (final)
| Container | Status |
|---|---|
| convex-backend (3210/3211) | Up healthy |
| convex-dashboard (6791) | Up |
| convex-gateway (3005→3000) | Up healthy |
| convex-subscriber | Up healthy (self-healed after velion-nats came up; logs show retry-then-success cycle) |
| affine-runtime (47810→3010) | Up |
| notification-core (3140) | Up |
| application-postgres (9540→5432) | Up healthy |
| application-redis (6480→6379) | Up healthy |
| affine-runtime-migration (one-shot) | Exited 0, removed |

`convex-subscriber` confirmed subscribing to: `velion.controlplane.org.{created,updated,deleted}`, `velion.controlplane.org.member.{added,removed}`, `velion.ingestion.import.completed`, `velion.ingestion.crawl.{started,progress,completed,failed,indexed}`, plus `mp.v1.run.*.event` via the velion-nats fallback (the Model Plane NATS cluster lives on `model-plane-network` and is not reachable from `app-net` + `inter-plane-bus`, which is the documented W4-2 behaviour).
