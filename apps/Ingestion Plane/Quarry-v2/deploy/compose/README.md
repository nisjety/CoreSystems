# Quarry v2 — Docker Compose deployment

This directory holds the three Dockerfiles that build Quarry v2's microservices,
plus a self-contained `docker-compose.yml` for running Quarry v2 standalone
(separate from the CoreSystem stack).

## Files

| File | Purpose |
|---|---|
| `Dockerfile.edge` | Rust hot path — REST/SSE/cache/transform pipeline (port 8082) |
| `Dockerfile.control` | Go durable plane — jobs/schedules/profiles/webhooks (port 8081) |
| `Dockerfile.orchestrator` | Go Temporal worker — scrape/batch/crawl workflows (no port) |
| `docker-compose.yml` | Standalone stack with bundled Postgres + Redis + Temporal |

## Production deployment (CoreSystem stack)

The three services are also wired into `/Volumes/Lagring/Triodelab/CoreSystem/docker-compose.yml`
which reuses the shared `aquatiq-postgres-local`, `aquatiq-redis-local`,
`aquatiq-nats-local`, and `org-core-temporal` infrastructure.

### Bootstrap (first-time setup)

```bash
# 1. Create the quarry_v2 database in the shared Postgres
docker exec aquatiq-postgres-local psql -U "${DB_USER:-aquatiq}" \
    -c "CREATE DATABASE quarry_v2 OWNER \"${DB_USER:-aquatiq}\";"

# 2. Build + start Quarry v2 services
cd /Volumes/Lagring/Triodelab/CoreSystem
docker compose up -d --build quarry-control quarry-edge quarry-orchestrator

# 3. Verify
curl http://localhost:8082/health   # quarry-edge
curl http://localhost:8081/health   # quarry-control
docker logs quarry-orchestrator --tail 20
```

### Smoke test

```bash
curl -X POST http://localhost:8082/v1/scrape \
    -H 'content-type: application/json' \
    -d '{"url": "https://example.com", "formats": ["markdown", "links"]}'
```

## Standalone deployment (this directory)

For dev runs without the CoreSystem stack:

```bash
cd /Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion\ Plane/Quarry-v2
docker compose -f deploy/compose/docker-compose.yml up -d --build
```

This brings up its own Postgres, Redis, and Temporal containers in addition
to the three Quarry services.

## Build details

### `Dockerfile.edge` (Rust)
- Base: `rust:1.85-slim-bookworm`
- System deps: `pkg-config`, `libssl-dev`, `protobuf-compiler` (for `tonic-build`),
  `cmake`, `perl`, `clang`, `build-essential` (for `boring-sys2` in `wreq`)
- Cargo workspace: builds `-p quarry-edge --bin quarry-edge-rs` only (other crates
  compile as transitive deps)
- Runtime: `debian:bookworm-slim` with `wget` for healthcheck; non-root `quarry` user
- Final image: ~120 MB

### `Dockerfile.control` (Go)
- Base: `golang:1.25-bookworm` (workspace requires Go 1.25 for `quarry-control`)
- Local deps copied: `pkg/quarrycontracts`, `pkg/quarryotel`
- Build: `CGO_ENABLED=0 -ldflags="-s -w -extldflags=-static"`
- Runtime: `debian:bookworm-slim` with `wget` for healthcheck; non-root `quarry` user
- Final image: ~50 MB

### `Dockerfile.orchestrator` (Go)
- Same base + build pattern as `Dockerfile.control`
- Runtime: `debian:bookworm-slim` with `procps` for `pgrep`-based healthcheck
- Final image: ~55 MB

## Environment variables (root compose)

| Variable | Default | Purpose |
|---|---|---|
| `QUARRY_CONTROL_API_KEY` | `dev-quarry-control-key` | Internal token control accepts on event ingestion |
| `DATA_PLANE_URL` | (unset) | Optional Data Plane v2 ingest endpoint |
| `DATA_PLANE_API_KEY` | (unset) | Bearer token for Data Plane |
| `MODEL_PLANE_URL` | `http://ai-core:8040` | Model Plane gateway URL (for /v1/audio + AI formats) |
| `MODEL_PLANE_TOKEN` | (unset) | Bearer token for Model Plane |
| `BROWSER_BROKER_GRPC` | (unset) | gRPC endpoint for grant validation |
| `BROWSERLESS_URL` / `BROWSERLESS_TOKEN` | (unset) | Browserless cloud browser |
| `BROWSERBASE_API_KEY` | (unset) | Browserbase cloud browser |
| `KERNEL_URL` / `KERNEL_API_KEY` | (unset) | Kernel cloud browser |
| `BRAVE_SEARCH_KEY` / `SERPER_KEY` / `SEARXNG_URL` | (unset) | SERP search providers |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | (unset) | OTLP collector for traces |
| `RUST_LOG` | `info` | Edge service log level |

All cross-plane integrations are optional — Quarry v2 degrades gracefully when
they're unset (e.g. `/v1/audio` returns `501 Unsupported`, ingest is skipped).

## Migration from V1

The frontend's `QUARRY_URL` env var was changed from
`http://quarry-api:8090` (V1 monolith) to `http://quarry-edge:8082` (V2 edge).
V1's separate compose file (`apps/Ingestion Plane/Quarry/docker-compose.yml`) is
no longer brought up alongside the CoreSystem stack.
