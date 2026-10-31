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
| `MODEL_PLANE_URL` | — (opt-in; empty keeps every Model Plane feature off) | `http://model-gateway:8080` for AI formats + planner |
| `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` | — | `http://browser-broker:9095` for grant validation (the old name `BROWSER_BROKER_GRPC` was never read) |
| `NATS_URL` | — | `nats://nats:4222` for cross-plane events |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | — | OTLP collector for traces |
| `BROWSERLESS_URL` / `BROWSERLESS_TOKEN` | — | Browserless cloud browser |
| `BROWSERBASE_API_KEY` | — | Browserbase cloud browser |
| `KERNEL_URL` / `KERNEL_API_KEY` | — | Kernel cloud browser |
| `BRAVE_SEARCH_KEY` / `SERPER_KEY` / `SEARXNG_URL` | — | SERP search providers |
| `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` | `enforce` | Web policy mode: `enforce`, `observe` (count only) or `off`; see RUNBOOKS "Web policy" |
| `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON` | — | Operator web-policy baseline, a `PolicyLayer` document; unset means no layer |
| `QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM` | `0` | `1` refuses a token without the `web_policy` claim (403); turn on only after auth-core mints the claim |

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

Every model-gateway call a Quarry feature makes (intent classification, autoprompt, the semantic reranker, page titles, AI formats and answer synthesis through `model-invoke`; streamed answers and AI formats through `model-stream`; audio through `model-audio`) is a row of `crates/quarry-runtime/contracts/cross-plane-token-requests.json`, and every such row is `granted: false` (QB-T17). The registry gives quarry-edge no model-gateway audience, and the gateway's `authorize_principal_route` (`apps/Model Plane/rust/services/model-gateway/src/auth.rs`) refuses service principals on `/v1/invoke` and `/v1/invoke/stream`. Flipping a row's `granted` alone therefore does not open a Model Plane path: each call is refused and its feature degrades. The org-bound model lane is not built (build plan, WS16).

**Scrape AI formats (`json`, `summary`, `question`, `highlights`; INNH-1).** `POST /v1/scrape` can answer these inline from the page text through the Model Plane gateway's `POST /v1/ai/chat`. They are off: `QUARRY_EDGE__AI_FORMATS` must be `on`, `1` or `true` (anything else is off), and the committed composes default it to `0`. This is the E1 path, for a development or side-by-side stack only:

| Env | Default | Notes |
|-----|---------|-------|
| `QUARRY_EDGE__AI_FORMATS` | off | The switch. Off, every AI format is `400 feature_disabled` at its index. |
| `QUARRY_EDGE__MODEL_PLANE_URL` | unset | Required. Unset with the switch on: `501 model_plane_unavailable`, decided before the page is fetched. |
| `QUARRY_EDGE__MODEL_PLANE_TOKEN` | unset | The development credential, honoured only under the cross-plane auth dev bypass (`main.rs`). With a URL and no credential: `501 model_lane_not_granted`. |
| `QUARRY_EDGE__MODEL_PLANE_INFERENCE_TOKEN` | unset | Development only: the token for the `inference-core` audience, sent as `x-inference-authorization`. Unset, the one token above goes in both headers. |
| `QUARRY_EDGE__AI_FORMATS_ZDR` | `refuse` | Only `refuse` takes effect (ADR-4): under ZDR every AI format is answered `skipped` / `ZDR_UNAVAILABLE` and no model is called. `allow` is reserved, logged once as ignored. |
| `QUARRY_EDGE__AI_FORMATS_MODEL_JSON`, `QUARRY_EDGE__AI_FORMATS_MODEL_TEXT` | `verevon-balance`, `verevon-budget` | Routing aliases, never vendor model ids. |
| `QUARRY_EDGE__AI_FORMATS_MAX_INPUT_CHARS`, `..._MAX_TOTAL_TOKENS`, `..._MAX_CALLS`, `..._CONCURRENCY` | 60,000; 200,000; 8; 4 | Per request caps (the first can only be lowered, to 1,000 at least; calls 1 to 8; concurrency 1 to 4; 16 calls in flight per process). |

A deployed stack cannot turn this on yet. The Quarry to Model Plane grant for the chat lane (decision X-7: two audiences, `model-gateway` and `inference-core`, retention `tenant`, and the two manifest rows set `granted: true`) and the metered service lane on the gateway (WS16.S1) are owner decisions that are not made; until then Quarry holds no credential for this lane outside the dev bypass. A user token may ask; a service principal needs the scope `extract:ai` (no registry principal holds it); a request labelled `initiated_by: "agent"` is refused. The contract is `docs/CONTRACTS.md`, "Scrape AI formats"; the operator steps are in `docs/RUNBOOKS.md`.

**Answer synthesis (`/v1/answer`, `/v1/answer/stream`, `include_answer`; INNH-8a).** One pipeline (search, read the top pages, ask the Model Plane) serves the three surfaces. It is off: `QUARRY_EDGE__ANSWER_SYNTHESIS` must be `1`, `true` or `on` (compose variable `QUARRY_EDGE_ANSWER_SYNTHESIS`, default `0`), and `MODEL_PLANE_URL` alone no longer turns it on. It also needs a search provider, `QUARRY_EDGE__MODEL_PLANE_URL` and a Model Plane that accepts the edge's service token, which a deployed stack does not have yet (the cross-plane grant is owner decision INNH-8b). When a gate is closed the answer says which, in `reason` (`/v1/answer`: 501 `synthesis_disabled`, `no_search_provider`, `no_model_plane_url` or `model_client_init_failed`; 503 `model_plane_auth_refused`) and in `answer_status` / `answer_reason` on `POST /v1/search`. Operator steps: `docs/RUNBOOKS.md`, "Answer synthesis".

**Page-title enrichment (`page_extracted`).** When a fetched page has no usable `<title>` (missing, or generic such as the host name, "Home", "Forside"), `PageRunner` asks `/v1/invoke` for a clean ≤ 60-char title and a one-sentence summary from a bounded excerpt, and emits them on the `page_extracted` event with `title_source: "model"`. Configuration:

| Env | Default | Notes |
|-----|---------|-------|
| `QUARRY_EDGE__MODEL_PLANE_URL` | — | Enables the hop (and every other Model Plane consumer). Unset → `title_source` is `html`/`host` only. Set, the call is still refused until a model-gateway row is granted (none is, QB-T17). |
| `QUARRY_EDGE__PAGE_TITLE_MODEL` | `verevon-budget` | A Model Plane **routing alias** (`verevon-budget` = cheapest capable model via inference-core's `RoutingPolicy`; also `verevon-balance`, `verevon-genius`). Never a vendor model id. |
| `QUARRY_EDGE__PAGE_TITLE_ENRICH` | `true` when a Model Plane URL is set; the Ingestion compose sets `0` | Set `false` to keep extraction fully local. |
| `QUARRY_BROWSER_SETTLE_MS` | `3500` | Hydration settle budget for JS-shell pages fetched by the browser fallback (see `ONBOARDING_LIVE_CRAWL.md`). |

Guards: 2 s time-box, 3 concurrent calls, 600 calls/hour per edge process, fingerprint-keyed cache, JSON-only reply validation. Any failure degrades to `title_source: "host"`; a page never fails because of the hop. In the shipped configuration that call is refused (above), so titles come from `html` or `host` today.

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
- Skips the page-title Model Plane hop entirely (`page_extracted.title_source` is `html` or `host`), writes nothing to its title cache, and routes `page_extracted` through `emit_for_zdr` so it reaches only live SSE subscribers — never the durable publisher or NATS

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
