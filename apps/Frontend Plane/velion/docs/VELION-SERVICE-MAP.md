# Verevon service map — ground truth + drift audit

> Generated: 2026-05-19. Reflects live `docker ps` output against verevon's
> `.env` and `.env.local`. The drift table below was the root cause of
> "X-Org-ID required", "No crawls yet", and Graph/Wiki 404s observed
> during Wave 11.A/B/C bring-up.

## 1 · Canonical service map (live containers)

| Plane | Service | Container name | Host port | Container port | Notes |
|---|---|---|---|---|---|
| **Control** | auth (Better Auth + Convex token issuer) | `auth-service` | 3011 | 3011 | also gRPC :50011 |
| **Control** | user-core | `user-service` | 3012 | 3012 | also :6060, gRPC :50012 |
| **Control** | org-core | `org-core-service` | 8080 | 8080 | also :6061, :9090–9091 |
| **Control** | billing-core | `billing-core-service` | 3014 | 3014 | gRPC :50013 |
| **Application** | Convex backend | `convex-backend` | 3210/3211 | 3210/3211 | |
| **Application** | Convex dashboard | `convex-dashboard` | 6791 | 6791 | |
| **Application** | Convex gateway | `convex-gateway` | 3005 | 3000 | verevon-proxied auth |
| **Application** | Convex subscriber | `convex-subscriber` | — | 3000 | NATS mirror; internal-only |
| **Model** | model-gateway (v1) | `model-plane-model-gateway-1` | 18080 | 8080 | gRPC :19090 |
| **Model** | inference-core | `model-plane-inference-core-1` | 18082 | 8082 | gRPC :19092 |
| **Model** | execution-core | `model-plane-execution-core-1` | 18083 | 8083 | gRPC :19093 |
| **Model** | orchestrator-core | `model-plane-orchestrator-core-1` | 18084 | 8084 | |
| **Model** | sandbox-manager | `model-plane-sandbox-manager-1` | 18086 | 8086 | gRPC :19094 |
| **Model** | letta-bridge | `model-plane-letta-bridge-1` | 18088 | 8088 | gRPC :19096 |
| **Model** | letta (memory) | `model-plane-letta` | 8283 | 8283 | |
| **Model** | NATS | `model-plane-nats-1` | 4228 | 4222 | |
| **Ingestion** | Quarry control (v2) | `quarry-control` | **none — internal-only** | 8081 | ⚠️ HOST CAN'T REACH IT |
| **Ingestion** | Quarry edge | `quarry-edge` | 8082 | 8082 | |
| **Ingestion** | Quarry orchestrator | `quarry-orchestrator` | none | n/a | Temporal worker |
| **Ingestion** | integration-api (Nango bridge) | `integration-api` | 3026 | 3026 | |
| **Ingestion** | integration-engine-go-api | _check_ | _check_ | 3126 | |
| **Data v2** | documents-api | `dpv2-documents-api` | 8010 | 8010 | |
| **Data v2** | wiki-store | `dpv2-wiki-store` | 8011 | 8011 | |
| **Data v2** | data-orchestrator | `dpv2-data-orchestrator` | 8012 | 8012 | |
| **Data v2** | data-quality | `dpv2-data-quality` | 8013 | 8013 | |
| **Data v2** | retrieval-engine | `dpv2-retrieval-engine` | 8014 | 8004 | gRPC :50062 → :50052 |
| **Data v2** | graph-index | `dpv2-graph-index` | 9201 | 9201 (verify) | admin port |
| **Data v2** | index-engine | `dpv2-index-engine` | 9202 (admin) | 9202 | |
| **Data v2** | embedding-engine | `dpv2-embedding-engine` | 9203 (admin) | 9203 | |
| **Frontend** | verevon (prod build) | `frontend-plane-verevon-frontend-1` | 3000 | 3000 | ⚠️ stale image; use `pnpm dev` instead |

## 2 · Drift caught in audit (and fixed below)

| File | Variable | Stale value | Correct value | Source of truth |
|---|---|---|---|---|
| `.env` | `AUTH_SERVICE_URL` | `http://auth-core:3011` | `http://auth-service:3011` | container roster |
| `.env` | `USER_SERVICE_URL` | `http://user-core:3012` | `http://user-service:3012` | container roster |
| `.env` | `ORG_SERVICE_URL` | `http://org-core:8080` | `http://org-core-service:8080` | container roster |
| `.env` | _missing_ | — | `GRAPH_SERVICE_URL`, `WIKI_SERVICE_URL`, `RETRIEVAL_SERVICE_URL` (container DNS) | Wave 11.C-a routes |
| `.env.local` | `QUARRY_API_URL` | `http://localhost:9090` | `http://localhost:8081` ← **needs host port mapping added to compose** | quarry-control listens on 8081 only |
| `.env.local` | `DOCUMENTS_SERVICE_URL` | `http://dpv2-documents-api:8010` | `http://localhost:8010` | container has host mapping |
| `.env.local` | _missing_ | — | `DATA_ORCHESTRATOR_URL=http://localhost:8012`, `DATA_QUALITY_URL=http://localhost:8013`, `RETRIEVAL_SERVICE_URL=http://localhost:8014` | Wave 11 routes |

## 3 · `quarry-control` host-port gap — action required

The container exposes `8081/tcp` inside the network but does **not** publish a host port. Verevon's dev server (running on the host) can't reach it. Two fixes:

### A) Add host mapping in compose

In `apps/Ingestion Plane/docker-compose.yml`, add to the `quarry-control` service:

```yaml
ports:
  - "8081:8081"
```

### B) Run verevon inside the same docker network

Drop `pnpm dev` on the host and use the `frontend-plane-verevon-frontend-1` container, which is on `verevon-net` and can reach `quarry-control:8081` via container DNS.

**Recommendation**: A (host mapping). The frontend container is currently a stale build; using `pnpm dev` against properly-exposed services gives both HMR and live Quarry access.

## 4 · Rebuild from scratch — runbook

```bash
# 1. Prune EVERYTHING (the new --prune flag)
cd "/Volumes/Lagring/Triodelab/CoreSystem/apps/Frontend Plane/verevon"
./build-verevon-services.sh --prune

# 2. Validate compose files compile (dry-run)
./build-verevon-services.sh --dry-run

# 3. Full rebuild — Data Plane v2 first, then Ingestion, Model, Control, Application, Frontend
./build-verevon-services.sh

# 4. Verify per-plane health
./build-verevon-services.sh --status

# 5. Run verevon against the live stack
pnpm dev                  # OR `docker logs frontend-plane-verevon-frontend-1` if you trust the image
```

## 5 · Snake-case / hostname convention

- **Container DNS**: kebab-case service name (`auth-service`, `dpv2-documents-api`). Hyphens, **never** underscores. Compose project prefix may be present on some (`model-plane-*-1`).
- **Compose service id ≠ container name**: the container is named with `container_name:` directive when set. Always grep `docker ps` for truth — never assume.
- **Host-mapped ports**: explicit `:host->container` in `docker ps`. Verevon's `.env.local` must use the **host port**.
- **Container-network ports**: implicit single `:container/tcp` (no `host->` arrow). Verevon's `.env` (inside the container) must use the **container port**.
