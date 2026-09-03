# Quarry v2 — Self-Host Guide

This guide walks through running Quarry v2 in a self-hosted setup, integrated with Control Plane (auth/org/user), Data Plane v2 (documents/index/retrieval), and Model Plane (gateway/inference/browser-broker).

## Prerequisites

- Rust 1.85+ (`rustup install stable`)
- Go 1.22+
- protoc 3.20+ (`brew install protobuf` / `apt install protobuf-compiler`)
- Postgres 14+ (control + orchestrator state)
- Redis 7+ (cache admission)
- NATS 2.10+ with JetStream enabled (cross-plane events)
- Optional: Browserless / Browserbase / Kernel API keys for cloud browser drivers

## 1. Bootstrap services

```bash
# Control Plane: org/auth/user services + NATS + Postgres
cd /path/to/CoreSystem/apps/Control\ Plane
docker compose up -d

# Verify NATS JetStream
nats stream ls
```

## 2. Build Quarry v2

```bash
cd /path/to/CoreSystem/apps/Ingestion\ Plane/Quarry-v2
cargo build --workspace --release
go build ./services/quarry-control/cmd/control
go build ./services/quarry-orchestrator/cmd/orchestrator
```

For cross-plane gRPC (browser-broker, data-plane documents):

```bash
cargo build -p quarry-runtime --features grpc
```

## 3. Configure Quarry-edge

Set the following environment variables:

| Variable | Default | Purpose |
|---|---|---|
| `QUARRY_EDGE_ADDR` | `:8082` | HTTP listen address |
| `QUARRY_EDGE_REDIS_URL` | — | Redis cache; disable cache when unset |
| `DATA_PLANE_URL` | — | `http://documents-api-go:9001` for ingest path |
| `QUARRY_EDGE__AUTH_CORE_URL` | `http://auth-core:3011` | Auth Core service-token issuer |
| `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` | — | Durable credential registered under fixed service id `quarry-edge`; required when a plane URL is set |
| `QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS` | `false` | Local-only static-token compatibility; refused in production |
| `MODEL_PLANE_URL` | — | `http://model-gateway:8080` for AI formats + planner |
| `BROWSER_BROKER_GRPC` | — | `model-plane-broker:9090` for grant validation |
| `NATS_URL` | — | `nats://nats:4222` for cross-plane events |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | — | OTLP collector for traces |
| `BROWSERLESS_URL` / `BROWSERLESS_TOKEN` | — | Browserless cloud browser |
| `BROWSERBASE_API_KEY` | — | Browserbase cloud browser |
| `KERNEL_URL` / `KERNEL_API_KEY` | — | Kernel cloud browser |
| `BRAVE_SEARCH_KEY` / `SERPER_KEY` / `SEARXNG_URL` | — | SERP search providers |

## 4. Run Quarry

```bash
# Hot-path edge (Rust)
./target/release/quarry-edge-rs

# Durable orchestrator (Go, Temporal worker)
./orchestrator

# Control plane (Go, REST API)
./control
```

## 5. Smoke test

```bash
curl -X POST http://localhost:8082/v1/scrape \
  -H 'content-type: application/json' \
  -d '{"url": "https://example.com", "formats": ["markdown", "links"]}'
```

You should see a JSON response with `markdown` (artifact ref), `links`, and a `fingerprint`.

## 6. Cross-plane flows

### Quarry → Data Plane v2 ingest

When a scrape request includes `"ingest": true` and an `org_id`, Quarry posts to Data Plane's `/v1/ingest` after the page is fetched. Failures don't fail the scrape — they're logged.

### Quarry → Model Plane (planner / structured extract / AI formats)

The agentic browser loop calls Model Plane's `/v1/invoke` for each step decision. Structured extract uses the same gateway with a JSON Schema in the prompt. AI formats (`summary`, `json`, `query`) all flow through `/v1/invoke`.

### Model Plane → Quarry (browser grants)

When Model Plane wants Quarry to execute browser actions, it issues a *grant* via `BrowserBroker.AcquireGrant`. Enable Quarry Edge's `grpc` feature and set `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` to validate each privileged action over gRPC. `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL` remains the HTTP-shim compatibility path; when neither is configured, only development may fall back to `NoopGrantValidator`.

### NATS event flow

Quarry publishes `quarry.run.<run_id>.<event_type>` and an aggregate `quarry.events.<event_type>`. Control Plane and Model Plane subscribe to the aggregate streams for cross-plane workflows.

## 7. ZDR (zero data retention)

Set `zdr: true` on any scrape, agent, or structured extract request. Quarry:

- Skips Redis cache writes
- Skips artifact persistence
- Refuses Data Plane ingest with payload (returns 403 Forbidden)
- Sends source markdown to Model Plane ephemerally only (Model Plane is contracted to not persist)

ZDR violations bubble up as typed `Forbidden` errors. Audit them via the `agent.failed` and `page.blocked` events.

Set `zdr: true` on `/v1/search`, `/v1/answer`, or `/v1/answer/stream` and, in
addition to the cache/event skips above, `SmartSearchRouter` never invokes
Brave or Serper for that request — only in-infra providers (Tantivy /
Stract / SearXNG + Data Plane) are queried, so the query text itself never
egresses to a third party. This is a hard per-request override: it applies
even when `QUARRY_EDGE__ZERO_SAAS_SEARCH` is left at its default (`0`), which
only controls whether Brave/Serper are registered for *non-ZDR* traffic. See
`SearchOptions::zdr` in `crates/quarry-runtime/src/serp.rs`, and the gate itself in
`crates/quarry-runtime/src/smart_router.rs`.

## 8. Operations

- See [RUNBOOKS.md](./RUNBOOKS.md) for incident playbooks.
- See [DRIVER_MATRIX.md](./DRIVER_MATRIX.md) for browser provider selection rules.
- See [gap-quarry.md](./gap-quarry.md) for the implementation status matrix.
