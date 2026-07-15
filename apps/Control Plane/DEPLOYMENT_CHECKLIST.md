# Control Plane Deployment & Testing Checklist

## ✅ Completed

### Folder Restructuring
- [x] `auth/` → `auth-core/` 
- [x] `user/` → `user-core/`
- [x] `Org-core/` → `org-core/`

### Docker Compose Refactoring (`docker-compose.yml`)
- [x] **Removed all non-control-plane services:**
  - ✅ Deleted: ai-core, Convex, Letta, Temporal (data/reasoning/realtime planes)
- [x] **Added embedded infrastructure:**
  - ✅ PostgreSQL 15 (port 5432) - single shared database
  - ✅ Redis 7 (port 6379) - session/cache store
  - ✅ NATS 2.10 (port 4222) - event streaming
- [x] **Control plane services (HTTP + gRPC + optional extras):**
  - ✅ auth-core: 3011 (HTTP), 50011 (gRPC)
  - ✅ user-core: 3012 (HTTP), 50012 (gRPC)
  - ✅ org-core: 8080 (HTTP), 9090 (gRPC), 9091 (metrics)

### Environment & Secrets
- [x] Each of the six cores owns an independent `.env` and tracked `.env.example`
- [x] Removed the Control Plane root `.env` and `.env.example`
- [x] Added `scripts/run-control-plane.sh` for service-local Compose interpolation
- [x] All hardcoded secrets replaced with `${VAR}` references
- [x] Networks: private `controlplane-net` plus shared external `inter-plane-bus`
- [x] Health checks on all services

### Configuration Standards  
- [x] Service → Service URLs use DNS names: `http://auth-core:3011`, `user-core:50012`
- [x] Database URLs normalized to `controlplane-postgres` by the Docker overrides
- [x] Port standardization per control plane spec
- [x] Removed obsolete `version:` field from compose

---

## 📋 Next Steps: Local Deployment

### 1. Ensure Docker is Healthy
```bash
docker run hello-world  # Verify daemon works
```

### 2. Load Environment
```bash
cd "/Volumes/Lagring/Triodelab/CoreSystem/apps/Control Plane"
# Verify the six service-local contracts and root env removal
bash scripts/control-service-env-contract-test.sh
./scripts/run-control-plane.sh config --quiet
```

### 3. Start Infrastructure Tier
```bash
# Starts Postgres, Dragonfly, and NATS using service-local env files
./scripts/run-control-plane.sh up -d \
  controlplane-postgres \
  controlplane-dragonfly \
  controlplane-nats

# Wait 15 seconds for health checks
sleep 15
./scripts/run-control-plane.sh ps
```

### 4. Start Control Plane Services
```bash
# Services will auto-initialize databases on first connection
./scripts/run-control-plane.sh up -d \
  auth-core \
  user-core \
  org-core

# Wait 45 seconds for startup and migrations
sleep 45
./scripts/run-control-plane.sh ps
```

### 5. Verify All Healthy
```bash
./scripts/run-control-plane.sh ps

# All services should show: STATUS="Up Xs (healthy)"
```

---

## 🧪 Testing the Flow

**See:** `CONTROL_PLANE_TEST_FLOW.md` for comprehensive test scenarios covering:
- Service health checks
- Service-to-service communication (auth → user, org → auth)
- NATS event streams
- API endpoints (sign-up, get user, create org)
- Database schema validation
- Cross-boundary event publishing

Quick start:
```bash
# 1. Health check endpoints
curl http://localhost:3011/api/auth/get-session
curl http://localhost:8080/health

# 2. Database connectivity
psql -h localhost -p 5433 -U aquatiq -d auth_service -c "SELECT version();"

# 3. NATS stream check
docker exec controlplane-nats nats stream list -s nats://localhost:4222
```

---

## 📊 Architecture Summary

```
┌─────────────────────────────────────────────────┐
│          CONTROL PLANE (Isolated Boundary)      │
├─────────────────────────────────────────────────┤
│                                                 │
│  ┌───────────────────────────────────────────┐  │
│  │      INFRASTRUCTURE TIER                  │  │
│  ├───────────────────────────────────────────┤  │
│  │ PostgreSQL (port 5432) - Shared DB        │  │
│  │ Dragonfly (port 6379) - Session/Cache     │  │
│  │ NATS (port 4222) - Event Stream           │  │
│  └───────────────────────────────────────────┘  │
│                      ↑ ↑ ↑                       │
│  ┌───────────┬───────────────────────────┐       │
│  │           │                           │       │
│  ▼           ▼                           ▼       │
│┌─────────┐┌──────────┐ ┌────────────────┐      │
││ auth    ││ user     │ │ org            │      │
││ -core   ││ -core    │ │ -core          │      │
││ 3011    ││ 3012     │ │ 8080/9090/9091│      │
││ 50011   ││ 50012    │ │                │      │
│└─────────┘└──────────┘ └────────────────┘      │
│   Identity  Metadata   Control & Policy        │
│                                                 │
└─────────────────────────────────────────────────┘

No Data Plane | No Reasoning | No Orchestration | No Realtime
```

---

## 🔧 Configuration Reference

### Key Environment Variables
```env
# Database
DB_USER=aquatiq
DB_PASSWORD=<your-secure-password>

# Dragonfly
DRAGONFLY_PASSWORD=<service-local-development-password>

# Scoped NATS credentials are defined per core; do not use a shared token.

# Security
JWT_SECRET=<your-secure-jwt-secret>
BETTER_AUTH_SECRET=<your-secure-auth-secret>
INTERNAL_SERVICE_SECRET=<your-secure-service-secret>
INTERNAL_API_KEY=<same-as-internal-service-secret>
```

### Service DNS Names (Internal)
- auth-core: `http://auth-core:3011` (HTTP), `auth-core:50011` (gRPC)
- user-core: `http://user-core:3012` (HTTP), `user-core:50012` (gRPC)
- org-core: `http://org-core:8080` (HTTP), `org-core:9090` (gRPC)
- postgres: `controlplane-postgres:5432`
- dragonfly: `controlplane-dragonfly:6379`
- nats: `nats://controlplane-nats:4222`

### Ports (External/Host)
| Service | HTTP | gRPC | Metrics |
|---------|------|------|---------|
| auth-core | 3011 | 50011 | - |
| user-core | 3012 | 50012 | - |
| org-core | 8080 | 9090 | 9091 |
| PostgreSQL | - | - | 5432 |
| Dragonfly | - | - | 6379 |
| NATS | - | - | 4222+8222 |

---

## 📝 Boundary Enforcement

**Control Plane = ONLY:**
- Identity (auth)
- User metadata (profiles, api keys)
- Organization metadata (entitlements, quotas)
- Policy decisions (authz checks)

**NOT in Control Plane:**
- ❌ Document storage (data plane)
- ❌ Vector search/retrieval (data plane)
- ❌ LLM calls (reasoning plane)
- ❌ Workflow orchestration (Temporal)
- ❌ Real-time sync (Convex)
- ❌ Agent memory (Letta)

---

## 🚀 Ready to Deploy

Once Docker is healthy on your system:

```bash
cd "/Volumes/Lagring/Triodelab/CoreSystem/apps/Control Plane"
./scripts/run-control-plane.sh up -d
# ... wait 45 seconds
./scripts/run-control-plane.sh ps
```

Then run the test flow from `CONTROL_PLANE_TEST_FLOW.md`.
