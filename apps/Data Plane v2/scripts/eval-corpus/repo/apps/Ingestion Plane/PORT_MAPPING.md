# Ingestion Plane - Direct Port Mapping

**Status:** ✅ Runtime ports in base compose, optional UI/admin ports in overlay  
**Networks:** `ingestion-net` for local infra, `verevon-net`/`inter-plane-bus` for cross-plane traffic
**Shared NATS:** `verevon-nats`

> **Verified 2026-07-11** against `docker-compose.yml`, `docker-compose.ui.yml`, live `docker ps`, and host `curl`. The prior March-30 table was materially wrong: there is **no `:8090` Quarry API** (Quarry-v2 is `quarry-edge` on `127.0.0.1:8082` + `quarry-control` on `:8081`), the NATS host port is **`4224`** (not `4222`), Dragonfly is **not** host-published, and `shipping-core`/`finspo-api`/`webhook-normalizer` were missing entirely. Corrected below. Ports/host-bindings can still drift — re-check against the compose files before relying on any single row.

---

## Published Ports (base stack)

Host and container ports usually match, but **not always** — `shipping-core` maps `3156→8080`, `nats` maps `4224→4222`, and `searxng` maps `8888→8080`. Several infra services bind to `127.0.0.1` only. Source of truth is `docker-compose.yml`.

| Service | Host → Container | Protocol | Live (curl) 2026-07-11 | Notes |
|---------|------------------|----------|------------------------|-------|
| **quarry-edge** | `127.0.0.1:8082 → 8082` | HTTP | ✅ 200 `/health` | Quarry-v2 scrape/search edge (loopback only) |
| **quarry-control** | `8081 → 8081` | HTTP | ✅ 200 `/health` | Quarry-v2 control/jobs API (**host-published on all interfaces**) |
| **imports-api** | `3025 → 3025` | HTTP | ✅ 200 `/health` | File import API |
| **integration-api** | `3026 → 3026` | HTTP | ✅ 200 `/health` | OAuth and provider discovery |
| **shipping-core** | `3156 → 8080` | HTTP | ✅ 200 `/healthz` | Health path is `/healthz` — `/health` 404s |
| **finspo-api** | `3130 → 3130` | HTTP | ✅ 200 `/health` | SharePoint/Graph delta-sync connector API |
| **integration-webhook-normalizer** | `3036 → 3036` | HTTP | ✅ 200 `/health` | Inbound webhook normalizer |
| **integration-engine-go-api** | `3126 → 3126` | HTTP | ❌ no listener | Mapped in compose but not up in current stack |
| **connector-runtime-engine** | `3003 → 3003`, `3009 → 3009` | HTTP | ❌ no listener | Nango runtime; gated behind `legacy-nango` profile |
| **ingestion-postgres** | `127.0.0.1:5432 → 5432` | PostgreSQL | — | Ingestion plane database (loopback only) |
| **ingestion-dragonfly** | *not host-published* | Redis wire | — | Redis→Dragonfly migration; `6379/tcp` container-internal only |
| **ingestion-temporal** | `127.0.0.1:7233 → 7233` | gRPC | — | Workflow frontend (loopback only) |
| **ingestion-nats** | `4224 → 4222` | NATS | — | Client port is host **4224** (container 4222) |
| **ingestion-qdrant** | `6333 → 6333`, `6334 → 6334` | HTTP / gRPC | — | Vector DB HTTP + gRPC |
| **ingestion-searxng** | `127.0.0.1:8888 → 8080` | HTTP | — | Metasearch backend (loopback only) |

Workers with **no host port** (internal only): `quarry-orchestrator`, `integration-email-worker`, `integration-finspo-worker`, `integration-worker`, `integration-engine-go-worker`, `support-worker`, `nango-seed`, `tika`, `gotenberg`, `stirling-pdf`, `temporal-postgres`.

## Optional UI/Admin Overlay

These endpoints live in [docker-compose.ui.yml](/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion%20Plane/docker-compose.ui.yml) and are only published when you start the overlay together with the base stack.

| Service | Published Port(s) | Protocol | Notes |
|---------|-------------------|----------|-------|
| **Temporal UI** | `8081` | HTTP | Workflow web UI — ⚠️ **collides with base-stack `quarry-control` on `8081`**; both cannot bind at once |
| **Connector Runtime UI** | `3009` | HTTP | Nango connect UI (`legacy-nango` profile) |
| **NATS Monitoring** | `8222` | HTTP | Monitoring endpoint |

> ⚠️ **Port collision:** the base stack already publishes `quarry-control` on `8081`, so the overlay's Temporal UI (also `8081`) will fail to bind unless `TEMPORAL_UI_PORT` / the overlay mapping is changed. Fix the overlay before relying on Temporal UI.

Start them when needed with:

```bash
docker compose -f docker-compose.yml -f docker-compose.ui.yml up -d
```

---

## Service-to-Service Addresses

Internal traffic still uses Docker service discovery and the same port numbers:

```text
postgres:5432
redis:6379
nats:4222
temporal:7233
qdrant:6333
qdrant:6334
connector-runtime-engine:3003
verevon-nats:4222
```

Cross-plane calls should use `verevon-net` plus service names such as `ai-core:50051` and `verevon-nats:4222`.

---

## gRPC Exposure

The ingestion plane itself is primarily HTTP plus gRPC clients. The gRPC servers that must be published from this stack are:

| Service | Port | Purpose |
|---------|------|---------|
| **Temporal Frontend** | `7233` | Workflow orchestration gRPC |
| **Qdrant** | `6334` | Vector database gRPC |

Quarry consumes `ai-core` over gRPC at `ai-core:50051`, but does not host a gRPC server inside this stack.

---

## Host Access Examples

```bash
# Quarry-v2 edge (loopback only)
curl http://127.0.0.1:8082/health

# Quarry-v2 control
curl http://localhost:8081/health

# Imports-Core API
curl http://localhost:3025/health

# Integration-Core API
curl http://localhost:3026/health

# Shipping-Core (health path is /healthz, not /health)
curl http://localhost:3156/healthz

# Finspo-Core API
curl http://localhost:3130/health

# PostgreSQL (loopback only)
psql -h 127.0.0.1 -p 5432 -U ingestion_user

# Dragonfly (Redis wire) is NOT host-published — exec into the container or
# reach it in-network as dragonfly:6379

# Qdrant HTTP
curl http://localhost:6333/healthz
```

Optional UI/admin endpoints:

```bash
open http://localhost:8081
open http://localhost:3009
open http://localhost:8222
```

---

## Verification Checklist

```bash
netstat -an | grep 8082   # quarry-edge (loopback)
netstat -an | grep 8081   # quarry-control
netstat -an | grep 3025   # imports-api
netstat -an | grep 3026   # integration-api
netstat -an | grep 3156   # shipping-core
netstat -an | grep 3130   # finspo-api
netstat -an | grep 3036   # integration-webhook-normalizer
netstat -an | grep 5432   # postgres (loopback)
netstat -an | grep 7233   # temporal (loopback)
netstat -an | grep 6333   # qdrant http
netstat -an | grep 6334   # qdrant grpc
netstat -an | grep 4224   # nats client (host 4224 -> container 4222)
```

---

**Updated:** March 30, 2026 · **Verified & corrected:** July 11, 2026
