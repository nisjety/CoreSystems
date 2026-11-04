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

Every model-gateway call a Quarry feature makes (intent classification, autoprompt, page titles, AI formats and answer synthesis through `model-invoke`; streamed answers and AI formats through `model-stream`; audio through `model-audio`) is a row of `crates/quarry-runtime/contracts/cross-plane-token-requests.json`, and every such row is `granted: false` (QB-T17). Since the WS16.S2 registry change quarry-edge holds the `model-gateway` audience (`models:invoke`, tenant) for the two `model-extract` rows, which are granted and have no call site yet; Auth Core cannot tell these three rows from them, so the closed gate is Quarry's own, and the gateway's `authorize_principal_route` (`apps/Model Plane/rust/services/model-gateway/src/auth.rs`) refuses service principals on `/v1/invoke` and `/v1/invoke/stream`. Flipping a row's `granted` alone therefore does not open a Model Plane path: each call is refused and its feature degrades. The org-bound model lane is not built (build plan, WS16).

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

**Structured extract (`POST /v1/extract` with `schema` or `prompt`; INNH-2, INNH-3).** It has no switch of its own, but it is one of the QB-T17 Model Plane features (the table below) and `QUARRY_EDGE__AI_FORMATS` gates it: with the switch off, or refused at boot because no model path exists, a schema or prompt request answers 501 `UNSUPPORTED` ("structured extraction is not available on this edge: QUARRY_EDGE__AI_FORMATS is off") when a Model Plane URL is set. It calls the gateway's `POST /v1/invoke`, which the gateway refuses to a service principal whatever the registry grants (`authorize_principal_route`). The scrape formats already use `POST /v1/ai/chat`; moving this runner to that route is WS16 work and is not built. It needs `QUARRY_EDGE__MODEL_PLANE_URL` and a model credential: the Auth Core minted token (`QUARRY_EDGE__QUARRY_SERVICE_API_KEY`; the `model-invoke` row is not granted, and the gateway refuses that token on this route) or, under the cross-plane auth dev bypass, `QUARRY_EDGE__MODEL_PLANE_TOKEN`. Without them the answer is a 501 `UNSUPPORTED` that says which is missing (three texts, `docs/ERROR_REASONS.md`); a deployed stack always gets one until WS16.S1 to S3 and decision X-7 land. Setting the URL alone is not enough. The operator steps and what works in development are in `docs/RUNBOOKS.md`, "Enable structured extract and AI formats".

**Answer synthesis (`/v1/answer`, `/v1/answer/stream`, `include_answer`; INNH-8a).** One pipeline (search, read the top pages, ask the Model Plane) serves the three surfaces. It is off: `QUARRY_EDGE__ANSWER_SYNTHESIS` must be `1`, `true` or `on` (compose variable `QUARRY_EDGE_ANSWER_SYNTHESIS`, default `0`), and `MODEL_PLANE_URL` alone does not turn it on. It is one of the QB-T17 switches (the table below), so the boot gate also needs a model path for it: a granted `model-invoke` manifest row, or the development bypass credential. A granted row alone does not open that path, because the gateway refuses service principals on `/v1/invoke`, which the answer call uses. A deployed stack has neither (the cross-plane grant is owner decision INNH-8b), so a switch that is on there is refused at boot and `/v1/answer` answers 501 `model_path_refused`. It also needs a search provider and `QUARRY_EDGE__MODEL_PLANE_URL`. When a gate is closed the answer says which, in `reason` (`/v1/answer`: 501 `synthesis_disabled`, `model_path_refused`, `no_search_provider`, `no_model_plane_url` or `model_client_init_failed`; 503 `model_plane_auth_refused` when a wired call is refused) and in `answer_status` / `answer_reason` on `POST /v1/search`. A zero data retention request is refused with a 403 `zdr_unavailable` before any work. Operator steps: `docs/RUNBOOKS.md`, "Answer synthesis".

**Page-title enrichment (`page_extracted`).** When a fetched page has no usable `<title>` (missing, or generic such as the host name, "Home", "Forside"), `PageRunner` asks `/v1/invoke` for a clean ≤ 60-char title and a one-sentence summary from a bounded excerpt, and emits them on the `page_extracted` event with `title_source: "model"`. Configuration:

| Env | Default | Notes |
|-----|---------|-------|
| `QUARRY_EDGE__MODEL_PLANE_URL` | — | Enables the hop (and every other Model Plane consumer). Unset → `title_source` is `html`/`host` only. Set, the call is still refused until a model-gateway row is granted (none is, QB-T17). |
| `QUARRY_EDGE__PAGE_TITLE_MODEL` | `verevon-budget` | A Model Plane **routing alias** (`verevon-budget` = cheapest capable model via inference-core's `RoutingPolicy`; also `verevon-balance`, `verevon-genius`). Never a vendor model id. |
| `QUARRY_EDGE__PAGE_TITLE_ENRICH` | `false`; the Ingestion compose also sets `0` (decision 3.12) | Off by default (QB-T17): the hop needs a Model Plane path that the registry grants, and none exists yet. Set `true` only with that path; without it boot logs an error and extraction stays local. See the Model Plane switches below. |
| `QUARRY_BROWSER_SETTLE_MS` | `3500` | Hydration settle budget for JS-shell pages fetched by the browser fallback (see `ONBOARDING_LIVE_CRAWL.md`). |

Guards: 2 s time-box, 3 concurrent calls, 600 calls/hour per edge process, fingerprint-keyed cache, JSON-only reply validation. Any failure degrades to `title_source: "host"`; a page never fails because of the hop. In the shipped configuration that call is refused (above), so titles come from `html` or `host` today.

### Model Plane switches (QB-T17)

Every Quarry feature that calls the Model Plane is OFF by default. The switches below use the manifest rows `model-invoke` (and `model-audio` for audio), which are `granted: false`, so Auth Core refuses each call with a 403. The two AI-format rows, `model-extract` and `model-extract-inference`, have been granted since WS16.S2, but the gate does not check them yet. A switch set to `1` without a granted model path stays off, and boot logs an `ERROR` that names it. Its routes then answer unavailable (`501`, or an empty `200` for suggestions) and make no Model Plane call. A granted row does not turn a switch on: every prerequisite below must hold for the lane a switch uses. The org-bound, metered design for these features is `DEF-QB-T17-PART2`, now decision DR-15.1 in the build plan (proposed; its owner, decision X-7, is not named yet).

**Prerequisites for a deployed path (DR-15.1).** (1) A route the gateway admits to a service principal: `POST /v1/ai/chat` with `models:invoke`, metered and bounded under WS16.S1, which is not built. `/v1/invoke` and `/v1/invoke/stream` stay closed to services. (2) Two registry audiences for quarry-edge: `model-gateway` (`models:invoke`) and `inference-core` (`inference:invoke`, `models:invoke`), granted by the `model-extract` and `model-extract-inference` rows. (3) Tenant retention on both rows, so the organisation's own zero-retention posture is resolved at every mint. (4) The second bearer: an `inference-core` token sent as `x-inference-authorization` beside the first. Items (2) and (3) are in the registry. Item (1) is incomplete until WS16.S1 lands, and the scrape formats, which already call `POST /v1/ai/chat`, stay off on a deployed stack because the boot gate still checks `model-invoke` for them.

| Env | Default | Feature |
|-----|---------|---------|
| `QUARRY_EDGE__AUTOPROMPT` | `0` | Rewrites verbose Research/Comparative queries before fan-out. |
| `QUARRY_EDGE__LLM_CLASSIFY_INTENT` | `0` | LLM intent classification of queries. |
| `QUARRY_EDGE__PAGE_TITLE_ENRICH` | `0` | Page titles (see above). |
| `QUARRY_EDGE__ANSWER_SYNTHESIS` | `0` | `POST /v1/answer`, `/v1/answer/stream` and `include_answer` on search. Also needs a SearchProvider. A ZDR request is refused (403). |
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
