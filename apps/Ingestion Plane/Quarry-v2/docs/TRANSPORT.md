# Transport Matrix — Quarry v2

How Quarry talks to its neighbors. Updated after P2 (gRPC enabled in
production image).

## Quarry inbound (clients → quarry-edge)

| Route                  | Transport        | Why                                                                       |
| ---------------------- | ---------------- | ------------------------------------------------------------------------- |
| `POST /v1/scrape`      | REST (axum)      | Browser-friendly; one-shot                                                |
| `POST /v1/scrape/stream` | REST + **SSE**  | Streaming progress events to a browser                                     |
| `POST /v1/crawl`       | REST             | Async handoff to orchestrator                                              |
| `POST /v1/batch`       | REST             | Async handoff to orchestrator                                              |
| `POST /v1/search`      | REST             | Single-shot SERP-style results                                             |
| `POST /v1/answer`      | REST             | Tavily-shape Q&A with citations                                            |
| `* /v1/profiles[/:id]` | REST             | CRUD over session snapshots                                                |
| `POST /v1/audio`       | REST             | Proxies to Model Plane                                                     |
| `GET /health`, `/ready`| REST             | LB / k8s probes — **no auth**                                              |
| All `/v1/internal/*`   | REST             | Orchestrator → edge service-to-service                                     |

REST chosen over gRPC for inbound because clients are heterogeneous
(browsers, CLIs, Cursor, n8n) and OpenAPI generation matters more than
wire efficiency. SSE handles streaming.

## Quarry outbound

### Data Plane v2 (ingest) — **dual transport, P2 / cluster #grpc**

| Mode | Implementation               | Wire                | When to use                                    |
| ---- | ---------------------------- | ------------------- | ---------------------------------------------- |
| http | `IngestClient`               | HTTP/1.1 + JSON     | Default. Simpler, debuggable, full field set.  |
| grpc | `GrpcIngestAdapter`          | HTTP/2 + protobuf   | Production. Lower latency + smaller wire size. |

Both impls satisfy `quarry_runtime::ingest_client::DataPlaneIngest`.
`PageRunner.ingest` is `Option<Arc<dyn DataPlaneIngest>>` so the choice is
runtime-configurable without recompilation (provided the binary has
`--features grpc`):

```
QUARRY_EDGE__DATA_PLANE_TRANSPORT=http   # default
QUARRY_EDGE__DATA_PLANE_TRANSPORT=grpc   # production
```

ZDR enforcement is identical on both paths — `IngestClient::pre_check_zdr`
runs before any wire I/O, and the gRPC adapter sets
`zdr_classification=ephemeral` + `IngestPolicy::ephemeral()` when ZDR is
on so the Data Plane side can't accidentally persist.

**Trade-off documented in code:** `CreateDocumentResponse` (proto) has
fewer fields than `DataPlaneIngestResponse` (HTTP envelope) — the
adapter defaults `knowledge_unit_count`, `embedding_status`,
`retrievable_after`. Callers needing those fields must use HTTP until
the proto grows them.

### Model Plane (planner + intent + answer synth) — REST

`ModelPlaneClient` → `POST {MP_URL}/v1/invoke` (HTTP/1.1 + JSON).
The Model Plane gateway exposes a stable REST contract; keeping JSON
here avoids generating a second proto SDK in every consumer.

### Control Plane (event durable log) — REST

`EventPublisher` posts mpsc-buffered events to control-plane HTTP. The
events flow is durable on control's side, so HTTP/JSON is sufficient.
Cross-plane fan-out happens in parallel via NATS (see below).

### Cross-plane events — **NATS JetStream**

P1 / cluster #nats. `EventSink::emit` writes to:

1. **mpsc** → `EventPublisher` → control-plane HTTP (authoritative durable).
2. **NATS JetStream** subject `quarry.run.<run_id>.<type>` +
   `quarry.events.<type>` (best-effort cross-plane).

JetStream is the right fit because the consumers are heterogeneous
(autocomplete-core, org-core, ai-core, billing-core) and want pull
semantics with at-least-once delivery, replay, and stream retention.
See `docs/TENANCY.md` for subject layout.

### Auth-Core (JWT verification) — REST + JWKS

P0 / cluster #auth+tenancy. `require_auth` middleware fetches the JWKS
document from `AUTH_CORE_JWKS_URL` over HTTP and caches it locally.
RS256 verification happens in-process — no per-request round-trip to
auth-core. Token rotation is handled by JWKS refresh.

## Why not gRPC everywhere?

| Concern                              | Why we kept REST                                                         |
| ------------------------------------ | ------------------------------------------------------------------------ |
| Inbound (`/v1/*`)                    | Clients heterogeneous; OpenAPI generation > wire efficiency.              |
| Model Plane invoke                   | MP gateway exposes a stable REST contract today.                          |
| Control Plane event publish          | Already buffered through mpsc → HTTP; switching cost > benefit.           |
| Auth                                 | JWKS is HTTP-shaped; no streaming need.                                   |
| Data Plane ingest (until P2)         | Was REST-only. **P2 added gRPC opt-in for production deployments.**       |

## Why not GraphQL?

Listed as cluster #11 future overlay in `gap-quarry.md`. The use case is
admin/dashboard query composition (e.g. "give me all runs for org X in
the last 24h with their cost units"). Not on the request hot path. Will
be added when a dashboard frontend needs it.

## Feature-flag matrix (build-time)

| Cargo feature | Effect                                                        |
| ------------- | ------------------------------------------------------------- |
| (default)     | HTTP-only transport. Smaller binary (~15 MB).                  |
| `grpc`        | Compiles `GrpcDataPlaneClient` + `GrpcIngestAdapter` + `GrpcGrantValidator`. Adds ~3 MB and a build-time dep on `tonic-build` / `protoc`. |
| `http3`       | Compiles the optional reqwest/quinn HTTP/3 driver. Per-request `prefer_http3` tries QUIC first and falls back to the planned HTTP/1.1+2/TLS driver on retryable transport failures such as blocked UDP egress. |

`Dockerfile.edge` enables `grpc` and `http3` for the production image.
Local dev builds use the default for faster cycles. On Docker Desktop
for macOS, outbound QUIC can fail because container UDP egress is proxied
through Docker's userspace networking path; `prefer_http3` now degrades to
the normal planned driver instead of surfacing that host limitation as a
scrape failure.
