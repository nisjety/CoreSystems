# Ingestion Plane - Direct Port Mapping

**Status:** ✅ Runtime ports in base compose, optional UI/admin ports in overlay  
**Networks:** `ingestion-net` for local infra, `velion-net` for cross-plane traffic  
**Shared NATS:** `velion-nats`

---

## Published Ports

Every published service now uses the same host and container port. That removes the old Docker-side rerouting layer and makes the exposed endpoint match the process that is actually listening.

| Service | Published Port(s) | Protocol | Notes |
|---------|-------------------|----------|-------|
| **Quarry API** | `8090` | HTTP | Web scraping API |
| **Imports-Core API** | `3025` | HTTP | File import API |
| **Integration-Core API** | `3026` | HTTP | OAuth and provider discovery |
| **Integration Engine Go API** | `3126` | HTTP | Go replacement path |
| **Connector Runtime API** | `3003` | HTTP | Nango runtime API |
| **PostgreSQL** | `5432` | PostgreSQL | Ingestion plane database |
| **Redis** | `6379` | Redis | Cache and queue state |
| **Temporal Frontend** | `7233` | gRPC | Workflow frontend |
| **NATS Client Port** | `4222` | NATS | Client connections |
| **Qdrant HTTP** | `6333` | HTTP | Vector DB API |
| **Qdrant gRPC** | `6334` | gRPC | Vector DB gRPC endpoint |

## Optional UI/Admin Overlay

These endpoints live in [docker-compose.ui.yml](/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion%20Plane/docker-compose.ui.yml) and are only published when you start the overlay together with the base stack.

| Service | Published Port(s) | Protocol | Notes |
|---------|-------------------|----------|-------|
| **Connector Runtime UI** | `3009` | HTTP | Nango connect UI |
| **Temporal UI** | `8081` | HTTP | Workflow web UI |
| **NATS Monitoring** | `8222` | HTTP | Monitoring endpoint |

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
velion-nats:4222
```

Cross-plane calls should use `velion-net` plus service names such as `ai-core:50051` and `velion-nats:4222`.

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
# Quarry API
curl http://localhost:8090/health

# Imports-Core API
curl http://localhost:3025/health

# Integration-Core API
curl http://localhost:3026/health

# PostgreSQL
psql -h localhost -p 5432 -U ingestion_user

# Redis
redis-cli -h localhost -p 6379 -a ingestion_redis_password_2026

# Qdrant HTTP
curl http://localhost:6333/health
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
netstat -an | grep 8090
netstat -an | grep 3025
netstat -an | grep 3026
netstat -an | grep 3126
netstat -an | grep 5432
netstat -an | grep 6379
netstat -an | grep 7233
netstat -an | grep 6334
netstat -an | grep 4222
```

---

**Updated:** March 30, 2026
