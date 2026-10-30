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
| `QUARRY_EDGE__SEMANTIC_RERANK` | `false` (the Ingestion compose sets `0`; set `1` after the QB-T09 ship check passes) | Semantic rerank (option B): the Data Plane's `POST /v1/rerank` reorders the top results by a paid cross-encoder score. Needs `DATA_PLANE_URL` and the cross-plane tokens. Read the deploy order in `RUNBOOKS.md` (Semantic rerank) before it is on. |
| `QUARRY_EDGE__SEMANTIC_RERANK_TOP_N` | `10` | Candidates reranked per search, 1 to 100. The rest of the results keep their order. |
| `QUARRY_EDGE__RERANK_DEADLINE_MS` | `2000` | One rerank call's budget in milliseconds, clamped to 100 to 8000. A call that misses it keeps the search in its input order. |
| `QUARRY_EDGE__RERANK_SHADOW` | `false` (the Ingestion compose sets `1` for the first deploy's smoke run) | The Data Plane is asked and its order measured, but results keep their order and carry no score. The smoke run ends with the post-proof flip to `0`. |
| `QUARRY_EDGE__VECTOR_RETRIEVAL` | `false` | Kill switch for the Data Plane vector leg (`POST /v1/retrieve`) in hybrid search and find-similar. Off: search is lexical-only and find-similar answers 501. Turn it on only after the Data Plane has stamped and backfilled the quarry corpus (QB-M1b). Needs `DATA_PLANE_URL` and a service identity. |
| `QUARRY_EDGE__VECTOR_RETRIEVAL_PER_ORG_HOURLY_BUDGET` | `600` | Vector calls one org may spend per hour, counted per process. Each call is one paid query embedding. `QUARRY_EDGE__ZERO_SAAS_SEARCH=1` (sovereign) turns the leg off, and a zero-retention run never sends it. Replaced by usage-ledger metering (DEF-QB-VECTOR-METERING). |
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

Every model-gateway call a Quarry feature makes (intent classification, autoprompt, page titles, AI formats and answer synthesis through `model-invoke`; streamed answers and AI formats through `model-stream`; audio through `model-audio`) is a row of `crates/quarry-runtime/contracts/cross-plane-token-requests.json`, and every such row is `granted: false` (QB-T17). The registry gives quarry-edge no model-gateway audience, and the gateway's `authorize_principal_route` (`apps/Model Plane/rust/services/model-gateway/src/auth.rs`) refuses service principals on `/v1/invoke` and `/v1/invoke/stream`. Flipping a row's `granted` alone therefore does not open a Model Plane path: each call is refused and its feature degrades. The org-bound model lane is not built (build plan, WS16).

**Page-title enrichment (`page_extracted`).** When a fetched page has no usable `<title>` (missing, or generic such as the host name, "Home", "Forside"), `PageRunner` asks `/v1/invoke` for a clean ≤ 60-char title and a one-sentence summary from a bounded excerpt, and emits them on the `page_extracted` event with `title_source: "model"`. Configuration:

| Env | Default | Notes |
|-----|---------|-------|
| `QUARRY_EDGE__MODEL_PLANE_URL` | — | Enables the hop (and every other Model Plane consumer). Unset → `title_source` is `html`/`host` only. Set, the call is still refused until a model-gateway row is granted (none is, QB-T17). |
| `QUARRY_EDGE__PAGE_TITLE_MODEL` | `verevon-budget` | A Model Plane **routing alias** (`verevon-budget` = cheapest capable model via inference-core's `RoutingPolicy`; also `verevon-balance`, `verevon-genius`). Never a vendor model id. |
| `QUARRY_EDGE__PAGE_TITLE_ENRICH` | `false`; the Ingestion compose also sets `0` (decision 3.12) | Off by default (QB-T17): the hop needs a Model Plane path that the registry grants, and none exists yet. Set `true` only with that path; without it boot logs an error and extraction stays local. See the Model Plane switches below. |
| `QUARRY_BROWSER_SETTLE_MS` | `3500` | Hydration settle budget for JS-shell pages fetched by the browser fallback (see `ONBOARDING_LIVE_CRAWL.md`). |

Guards: 2 s time-box, 3 concurrent calls, 600 calls/hour per edge process, fingerprint-keyed cache, JSON-only reply validation. Any failure degrades to `title_source: "host"`; a page never fails because of the hop. In the shipped configuration that call is refused (above), so titles come from `html` or `host` today.

### Model Plane switches (QB-T17)

Every Quarry feature that calls the Model Plane is OFF by default. The cross-plane registry grants quarry-edge no model-gateway request yet: the manifest rows `model-invoke` and `model-audio` are `granted: false`, so Auth Core refuses each call with a 403. A switch set to `1` without a granted model path stays off, and boot logs an `ERROR` that names it. Its routes then answer unavailable (`501`, or an empty `200` for suggestions) and make no Model Plane call. Flip a switch only together with the registry grant. The org-bound, metered design for these features is `DEF-QB-T17-PART2` (owner and date not yet assigned).

| Env | Default | Feature |
|-----|---------|---------|
| `QUARRY_EDGE__AUTOPROMPT` | `0` | Rewrites verbose Research/Comparative queries before fan-out. |
| `QUARRY_EDGE__LLM_CLASSIFY_INTENT` | `0` | LLM intent classification of queries. |
| `QUARRY_EDGE__PAGE_TITLE_ENRICH` | `0` | Page titles (see above). |
| `QUARRY_EDGE__ANSWER_SYNTHESIS` | `0` | `POST /v1/answer` and `include_answer` on search. Also needs a SearchProvider. |
| `QUARRY_EDGE__AI_FORMATS` | `0` | Structured extraction: `schema` on `POST /v1/extract`. |
| `QUARRY_EDGE__SEARCH_SUGGEST` | `0` | `POST /v1/search/suggest`. |
| `QUARRY_EDGE__MODEL_AUDIO` | `0` | `POST /v1/audio`. Needs the `model-audio` grant, not `model-invoke`. |

The compose file reads each switch from the host environment as `QUARRY_EDGE_` followed by the name after `QUARRY_EDGE__` (for example `QUARRY_EDGE_ANSWER_SYNTHESIS`) and defaults it to `0`.

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
