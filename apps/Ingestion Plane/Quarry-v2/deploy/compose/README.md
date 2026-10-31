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

The canonical production wiring lives in
`apps/Ingestion Plane/docker-compose.yml` plus
`apps/Ingestion Plane/docker-compose.production.yml`. The repository-root
Compose file is a quarantined legacy compatibility stack and must not be used
for production.

### Bootstrap (first-time setup)

```bash
# 1. Provision the required secrets and build metadata described in the
#    Ingestion Plane `.env.example`.

# 2. Render, build, migrate, and start from the owning plane directory.
cd "/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane"
docker compose -f docker-compose.yml -f docker-compose.production.yml config -q
docker compose -f docker-compose.yml -f docker-compose.production.yml up -d --build \
  postgres dragonfly temporal-postgres temporal quarry-control quarry-edge quarry-orchestrator

# 3. Verify
docker compose ps quarry-edge quarry-control quarry-orchestrator
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
cp deploy/compose/.env.example deploy/compose/.env
# Replace every placeholder, then:
deploy/scripts/dev-up.sh -d
```

This brings up its own Postgres, Redis, and Temporal containers in addition
to the three Quarry services.

## Build details

### `Dockerfile.edge` (Rust)
- Base: `rust:1.91-slim-bookworm`
- System deps: `pkg-config`, `libssl-dev`, `protobuf-compiler` (for `tonic-build`),
  `cmake`, `perl`, `clang`, `build-essential` (for `boring-sys2` in `wreq`)
- Cargo workspace: builds `-p quarry-edge --bin quarry-edge-rs` only (other crates
  compile as transitive deps)
- Runtime: date-pinned Debian Bookworm slim with `wget`; non-root `quarry` user
- Final image: ~120 MB

### `Dockerfile.control` (Go)
- Base: `golang:1.26.5-bookworm`
- Local deps copied: `pkg/quarrycontracts`, `pkg/quarryotel`
- Build: `CGO_ENABLED=0 -ldflags="-s -w -extldflags=-static"`
- Runtime: date-pinned Debian Bookworm slim with `wget`; non-root `quarry` user
- Final image: ~50 MB

### `Dockerfile.orchestrator` (Go)
- Same base + build pattern as `Dockerfile.control`
- Runtime: `debian:bookworm-slim` with `procps` for `pgrep`-based healthcheck
- Final image: ~55 MB

## Environment variables (Ingestion Plane compose)

| Variable | Default | Purpose |
|---|---|---|
| `QUARRY_CONTROL_API_KEY` | required | Internal token control accepts on event ingestion |
| `DATA_PLANE_URL` | (unset) | Optional Data Plane v2 ingest endpoint |
| `AUTH_CORE_URL` | `http://auth-core:3011` | Auth Core service-token issuer |
| `QUARRY_SERVICE_API_KEY` | required with a plane URL | Durable credential registered as Auth Core service id `quarry-edge` |
| `QUARRY_CROSS_PLANE_AUTH_DEV_BYPASS` | `0` | Explicit local-only legacy static-token escape hatch; production refuses it |
| `MODEL_PLANE_URL` | (unset: opt-in) | Model Plane gateway URL. Unset keeps every Model Plane feature off (answer synthesis, AI formats, audio, search suggest, autoprompt, rerank, intent, page titles). Set it together with `QUARRY_SERVICE_API_KEY`. |
| `QUARRY_EDGE_AUTOPROMPT`, `QUARRY_EDGE_SEMANTIC_RERANK`, `QUARRY_EDGE_LLM_CLASSIFY_INTENT`, `QUARRY_EDGE_PAGE_TITLE_ENRICH` | `0` | Model Plane switches (decision 3.12 of 2026-10-27). Each one also needs `MODEL_PLANE_URL`. |
| `QUARRY_EDGE_AI_FORMATS` | `0` | AI formats of `POST /v1/scrape` (`json`, `summary`, `question`, `highlights`; INNH-1). `on`, `1` or `true` enables; anything else is off. Needs `MODEL_PLANE_URL` and, today, the development credential `MODEL_PLANE_TOKEN` (and optionally `MODEL_PLANE_INFERENCE_TOKEN`); a deployed stack has no Model Plane credential for this lane yet (decision X-7). Refused under ZDR. See `docs/RUNBOOKS.md`, "Scrape AI formats". |
| `QUARRY_EDGE_AI_FORMATS_ZDR` | `refuse` | Only `refuse` takes effect in v1 (ADR-4); `allow` is reserved and ignored. |
| `QUARRY_EDGE_SCRAPE_ACTIONS` | `off` | Scrape-time actions (ADR-6): `off`, `user` (dev and evidence stacks only) or `on` (gated; see `docs/RUNBOOKS.md`). |
| `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` | (unset) | BrowserBroker gRPC endpoint for grant validation (`http://browser-broker:9095` on inter-plane-bus). Needs the `grpc` feature. (Corrected 2026-10-16: the old name `BROWSER_BROKER_GRPC` was never read by any code.) |
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

## Chromium sandbox

Chromium runs with its own sandbox. `quarry-edge` is started with `seccomp-chromium.json` (allow-by-default plus a deny-list of escape syscalls, scoped to this service), which is what lets Chromium create the user and PID namespaces its sandbox needs; `QUARRY_BROWSER_NO_SANDBOX` defaults to `0`. Setting it to `1` is an explicit local opt-out that production refuses. See `docs/RUNBOOKS.md` for symptoms and for the production overlay's scratch mounts.
