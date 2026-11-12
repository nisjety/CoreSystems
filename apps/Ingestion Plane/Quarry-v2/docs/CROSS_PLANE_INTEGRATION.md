# Quarry v2 — Cross-Plane Integration

This document shows how Quarry v2 connects to the other CoreSystem planes (Control Plane, Data Plane v2, Model Plane) and the trust/auth boundaries between them.

## Plane responsibilities (canonical)

```
Quarry v2  → captures evidence (fetch, render, observe, fingerprint, diff)
Data Plane → owns durable knowledge (chunks, embeddings, graph, wiki, retrieval)
Model Plane → owns reasoning (planning, agent loops, structured extract, synthesis)
Control Plane → owns identity, org, billing, auth, NATS event spine
App / Shell → human-facing UX (CLI, IDE, channels, dashboards)
```

## Wire diagram

```
┌─────────────┐      AcquireGrant / ValidateGrant (gRPC)       ┌──────────────┐
│ Model Plane │  ◄────────────────────────────────────────►   │  Quarry edge │
│ browser-broker                                                 │              │
│             │                                                  │  /v1/scrape │
│             │  /v1/invoke (HTTP) — planner, structured,        │  /v1/crawl  │
│             │   summary, json, query, ranker                   │  /v1/agent  │
│             │                                                  │  /v1/search │
└─────────────┘                                                  └──────┬───────┘
                                                                        │
                                                                        │ /v1/ingest
                                                                        │  (HTTP or
                                                                        │   gRPC DocumentService)
                                                                        ▼
                                                                ┌──────────────┐
                                                                │ Data Plane v2│
                                                                │ documents-api│
                                                                │ index-engine │
                                                                │ retrieval    │
                                                                └──────┬───────┘
                                                                       │
                                                                       │
                ┌─────────────────────────────────┐                    │
                │ NATS JetStream                  │ ◄──────────────────┘
                │ subjects:                       │ ◄── quarry.run.<id>.<event>
                │   quarry.events.*               │ ◄── quarry.events.<event>
                │   organization.*                │ ◄── (Control Plane)
                │   user.*                        │ ◄── (Control Plane)
                └─────────────┬───────────────────┘
                              │
                              ▼
                ┌─────────────────────────────────┐
                │ Convex / App Plane              │
                │ (read-only projection)          │
                └─────────────────────────────────┘
```

## Auth + identity flow

1. **Org / User → Control Plane**: auth-core + org-core own identity. Quarry never authenticates users directly.
2. **Quarry edge** validates `Authorization: Bearer <token>` against Control Plane (or accepts internal-network tokens).
3. **Quarry → Data Plane**: Bearer-tokened, scoped to the request's `org_id`. Data Plane re-validates org status.
4. **Quarry → Model Plane**: Bearer-tokened, gateway returns an `org_id`-scoped session.
5. **Model Plane → Quarry browser**: the Model Plane browser-broker issues a short-lived grant (`BrowserBroker.AcquireGrant`); Quarry only validates it. Quarry validates each action with `ValidateGrant` before execution. No production code calls `AcquireGrant` yet: the first caller is the Do-mode browser tooling (WS21 S4 and S5a). The `acquire()` helper in `crates/quarry-runtime/src/grpc/browser_broker.rs` has no caller.

### Service-token requests: one manifest

Every Auth Core service token quarry-edge asks for is a row of `crates/quarry-runtime/contracts/cross-plane-token-requests.json`: the registry audience, the mint path, the scopes, the reason, `granted`, the files that request it and the switches that reach it. Production code builds a request only with `ServiceTokenRequest::from_manifest("<row>")` (`service_tokens.rs`); the public constructors from a scope list are gone, so a call site cannot widen its own request. The rows:

| Row | Audience | Scopes | Requested from |
|---|---|---|---|
| `vector-retrieve` | `data-plane` | `data:read` | `vector_index.rs` |
| `ingest` | `data-plane` | `documents:write`, `org:data:read_all` | `ingest_client.rs` |
| `web-rerank` | `data-plane` | `data:rerank` | `rerank.rs` (the Data Plane rerank client, QB-T09) |
| `model-invoke`, `model-stream` | `model-gateway` | `models:invoke` | `mp_client.rs` |
| `model-audio` | `model-gateway` | `models:invoke` | `audio_routes.rs` |
| `model-extract` | `model-gateway` | `models:invoke` | `mp_client.rs` (`lane_token_requests`, WS16.S3, `ModelPlaneClient::chat_for_org`) |
| `model-extract-inference` | `inference-core` | `inference:invoke`, `models:invoke` | `mp_client.rs` (`lane_token_requests`, WS16.S3, `ModelPlaneClient::chat_for_org`) |
| `browser-grant-validate` | `browser-broker` | `browser:read` | `grpc/browser_broker.rs` (`browser_grant_token_request`; the validator that mints it per run organization is WS1.A2 part 2, not yet wired) |
| `mail-otp-read` | `ingestion` | `integration:otp:read` | `otp_ic_client.rs` (E3.1-h, the integration-core `mail.otp.read` client; behind `QUARRY_EDGE__FILL_OTP`, default off) |
| `office-extract` | `ingestion` | `imports:read` | `content_route.rs` (`ManifestImportsBearer::for_office_extract`; the imports-core `POST /api/v1/import/extract` client of E3.6-6; behind `QUARRY_EDGE__OFFICE_VIA_IMPORTS`, default off; ZDR lane, ephemeral, nothing stored) |
| `artifacts-read` | `data-plane` | `data:read` | `data_plane_http.rs` (`DataPlaneArtifactReader::with_manifest_token_request`; the `no_egress` read of a stored page capture, E3.8.8; behind `QUARRY_EDGE__NO_EGRESS`, default off; never requested for a ZDR run) |

- `granted` is a statement about Auth Core's registry, not about the code. quarry-edge holds the `data-plane` and (since principal bundle P2, `browser:read` only) `browser-broker` audiences, and (since R-13 PR 1, WS16.S2) `model-gateway` (`models:invoke`) and `inference-core` (`inference:invoke`, `models:invoke`) at tenant retention, for the two `model-extract` rows, plus `ingestion` (`integration:otp:read` and `imports:read`, ZDR) for the `mail-otp-read` row (E3.1-h) and the `office-extract` row (imports-core admits `imports:read` for exactly `POST /api/v1/import/extract` and nothing else, and only while its `IMPORTS_EXTRACT_READ_SCOPE` is `true`, default off; every other imports-core write still needs `imports:write`). The `artifacts-read` row asks for the same `data-plane` `data:read` grant as `vector-retrieve`. Auth Core cannot tell the three Model Plane rows (`model-invoke`, `model-stream`, `model-audio`) from `model-extract` (same audience, same scope), so they stay `granted: false` as a Quarry-side closed gate, the gateway refuses a service principal on `/v1/invoke`, and their callers (intent classification, autoprompt, page titles, ai-formats, answer synthesis, audio) degrade; their switches default off (QB-T17). On 2026-10-06 a read-only check of the live registry found the five rows other than `web-rerank` in agreement with the manifest.
- Semantic rerank calls only the Data Plane's `web-rerank` row and makes no Model Plane call. `data:rerank` is granted to quarry-edge in the committed Control Plane registry (principal bundle P1, QB-T01). If auth-service did not hold that grant, Auth Core would refuse the mint and the rerank would degrade with reason `token_unavailable`; the read-only check of 2026-10-13 found it live. `bash "apps/Data Plane v2/scripts/qb-preflight.sh" --live --ship-qb-t09` reads the live registry and the Data Plane gate before quarry-edge ships the rerank client.
- Retention posture is never part of a request. Auth Core selects it from the registry and refuses a request that names one, so the manifest has no such field.
- Contract: the tests in `crates/quarry-runtime/src/token_request_contract.rs` read the file as plain JSON (not through the loader) and hold it to its shape and scope rules (retrieval never holds `org:data:read_all`; ingest and rerank are requests of their own; rerank is never a Model Plane request), to its sha256 pin (`cross-plane-token-requests.json.sha256`), to the source tree (no production file builds a request by hand, every `from_manifest("<row>")` names a row from a file that row lists, every row that is not `pending` has its call site) and to the wire: each call path runs against the real `AuthCoreServiceTokenProvider` and a recording Auth Core, and what arrives (path, `x-service-id`, `orgId`, `scopes`, `reason`, nothing else) must be the row. The `/v1/audio` path is covered by a unit test in `audio_routes.rs`, and the loader's own tests are in `service_tokens.rs`. They are unit tests of the two crates, so they add no test binary to the workspace. Quarry builds only in Docker: run them through `deploy/compose/Dockerfile.test` (`cargo test -p quarry-runtime --lib -- service_tokens:: token_request_contract::` and `cargo test -p quarry-edge --lib -- audio_routes::`). They fail rather than skip when a file is missing.
- Changing a row: edit the file, refresh the `.sha256` beside it, update the digest registered for it in `scripts/tests/zdr-contract-sync.test.sh` and re-vendor the copies listed there, then update `EXPECTED_REQUESTS` in `service_tokens.rs`. There is no feature switch and no environment variable: the manifest is compiled in.
- Not claimed: Auth Core's side of the contract (a spec that runs the real `authorizePlaneServicePrincipal` over a byte-for-byte vendored copy: granted rows authorized, `granted: false` rows refused) belongs to CR-T17: it is the block "principal policy against the token requests quarry-edge makes" of `apps/Control Plane/auth-core/src/auth/plane-service-principals-policy.contract.spec.ts`, which reads the vendored copy in `src/auth/fixtures/`, and `zdr-contract-sync.test.sh` lists auth-core as a consumer and compares the copy byte for byte. That spec holds the manifest to the committed policy file (`apps/Control Plane/config/plane-service-principals.json`), not to the live registry.

## ZDR propagation

ZDR (zero data retention) flows down every contract:

| Boundary | ZDR field | Effect when on |
|---|---|---|
| Quarry edge route | `request.zdr: true` | Skip Redis cache write |
| `PageRunner` | `pipeline.zdr` | Skip artifact persistence |
| `IngestClient` | `DataPlaneIngestRequest.zdr` | Reject with `Forbidden` when payload present |
| `AgentActionRequest` | `request.zdr` | AgentLoop skips persistent profile snapshots |

Bottom line: a single `zdr: true` at the boundary propagates through every cross-plane call so no plane accidentally persists.

### The token `zdr` claim is a floor, for user principals only

Auth Core stamps `zdr` on every plane token. quarry-edge (`auth.rs`: `Claims::zdr_floor`, `Claims::effective_zdr`, `Claims::effective_zdr_mode`) lifts a request's ZDR posture from it, and every JWT-authenticated route that makes a ZDR decision (`/v1/search`, `/v1/search/similar`, `/v1/answer`, `/v1/answer/stream`, `/v1/scrape`, `/v1/scrape/stream`, `/v1/extract`, `/v1/agent/runs`, `/v1/audio`, and the durable `/v1/crawl` and `/v1/batch` guard) takes it from that one helper: effective ZDR = body `zdr` OR (principal is a user AND claim `zdr` is true). The old `effective_zdr(requested)` in `search_routes.rs`, which only echoed the body flag, is gone.

- User tokens: the claim is the organisation's standing posture (`zdrIntent && entitled`, resolved by Auth Core from org-core), so a caller in a ZDR org cannot send `zdr: false` to lower it. A body `zdr: true` still raises a non-ZDR org's request (monotonic). The claim is a snapshot taken at mint time, so a change of the org's setting reaches Quarry when the user's token is next minted.
- Service tokens: the claim is NOT read. It is a registry property (`retentionByAudience`): model-gateway and execution-core hold `quarry=zdr`, so their token says `zdr: true` for every org on every call, and reading it would force ZDR on every chat web search. For a service caller the run's posture is the body flag, set by the caller from the run's resolved posture (false on a waived run).
- A token without the claim is accepted and adds no floor (the field is `Option<bool>` with a serde default; a mismatching type is a decode failure, as for every other claim).
- What the posture switches off is the existing ZDR behaviour of each route: paid search providers, the search/answer/page caches (read and write), durable page and search events, artifact and corpus persistence, ingest, and persistent browser profiles. `/v1/answer/stream` used to read and write its org-keyed answer cache whatever the request said; it now skips it under ZDR.
- Durable `/v1/crawl` and `/v1/batch` have no ZDR support (the orchestrator does not forward a posture to `/v1/internal/run_page`), so a request that carries an active floor or body `zdr: true` is refused with 403 instead of being run as non-ZDR. `/v1/internal/run_page` authenticates with the runtime token, not a JWT, and reads only its body. Lifting this needs the orchestrator to carry the posture from the handoff to `run_page`.
- Not covered by the floor, and not claimed: schedules, change tracking, resources and browser profiles are durable by design and are not gated on the org's posture; Quarry's own calls to the Model Plane (answer synthesis, structured extraction) carry no posture field, so what the Model Plane retains for them is decided by the Model Plane's token, not by this flag; `/v1/search/similar` takes the floor for its URL fetch, but that fetcher does not act on a posture (it is a plain GET that retains nothing), and the seed then goes to the Data Plane without a posture, so find-similar is not ZDR-gated until the Data Plane query path accepts one; `/v1/search/suggest` sends the search query to the Model Plane with no posture.
- There is no feature switch and nothing to configure: the floor has no off setting and no new environment variable. With no ZDR org (or tokens minted before the claim existed) nothing changes.
- Contract: `crates/quarry-edge/tests/zdr_claim_floor.rs` signs the real Auth Core claim sets (`tests/fixtures/auth-core/quarry-token-claims.golden.json`, regenerated and compared by auth-core's `quarry-token-zdr-claims.golden.spec.ts`, which mints with the real token controller over the real principal registry) through the real `require_auth` and `require_service_route_scope` and drives the real router. Its cache test needs a Redis (`REDIS_URL`) and fails rather than skips under `REQUIRE_CONTRACT=1` (it skips otherwise, so a job without a Redis stays green; `CI` alone is not a trigger because GitHub sets it on every job). The `cargo-check` job of `quarry-v2-ci.yml` provides a Redis service and sets `REQUIRE_CONTRACT=1` in its page-cache gate step, so the cache half runs in CI.

## Event flow

### Quarry → NATS JetStream

Stream: `QUARRY_EVENTS`
Subjects: `quarry.run.<run_id>.<event_type>` (per-run scope) AND `quarry.events.<event_type>` (aggregate).

Event types:

| Event | When | Payload |
|---|---|---|
| `run_started` | Run accepted | run_id, request hash |
| `page_fetched` | Page fetched OK | url, status, fingerprint, driver, latency_ms |
| `page_failed` | Page fetch failed | url, error_code, attempt count |
| `artifact_written` | Artifact stored | artifact_id, format, bytes |
| `change_detected` / `change_unchanged` | Diff vs previous | prev_fingerprint, new_fingerprint |
| `store_record_written` | Data Plane ingest acknowledged | document_id, index_status |
| `agent_started` / `agent_completed` / `agent_failed` | Agent loop lifecycle | run_id, step_count, termination |
| `action_started` / `action_completed` / `action_failed` | Per-step in agent loop | action variant, observation snapshot |
| `lease_acquired` / `lease_released` | Browser lease lifecycle | lease_id, profile_id |
| `profile_captured` / `profile_restored` | Profile lifecycle | profile_id, viewport |

### Control Plane → Quarry

Subjects: `organization.*`, `user.*`. Quarry does NOT re-publish these — it consumes them when an org changes plan or is deleted (to gate or revoke active runs).

## gRPC service map

| Service | Owner | Endpoint | Quarry caller |
|---|---|---|---|
| `BrowserBroker` | Model Plane | `:9095` | `GrpcGrantValidator` |
| `DocumentService` (v2) | Data Plane | `:9001` | `GrpcDataPlaneClient` |
| `InferenceCoreService` | Model Plane | `:9092` | (via gateway HTTP) |

When the `grpc` feature is enabled:

```bash
cargo build -p quarry-runtime --features grpc
```

The protos live vendored at `crates/quarry-runtime/proto/`. Update via `tools/sync-protos.sh` if the upstream contracts change.

## Data Plane ingest contract

Request (`DataPlaneIngestRequest`):

```json
{
  "run_id": "run_01HX...",
  "org_id": "org_acme",
  "source_url": "https://example.com/page",
  "title": "Example",
  "markdown": "# Hello\n...",
  "html_ref": "artifact_01HX...",
  "raw_ref": "artifact_01HY...",
  "chunks": [...],
  "metadata": {...},
  "fingerprint": "blake3:abc...",
  "zdr": false,
  "retention_policy": null,
  "source_trace": {
    "fields": {
      "title": {"selector": "head > title", "extracted_at": "..."},
      "body": {"selector": "article", "extracted_at": "..."}
    }
  }
}
```

Response (`DataPlaneIngestResponse`):

```json
{
  "document_id": "doc_01HX...",
  "index_status": "indexed",
  "knowledge_unit_count": 24,
  "embedding_status": "embedded",
  "retrievable_after": "2026-05-08T10:00:00Z",
  "trace_id": "trace_..."
}
```

When ZDR=on, the request must omit `markdown`, `html_ref`, `raw_ref`. Data Plane returns `index_status: "skipped"`, `embedding_status: "skipped"`.

## Operational checks

| Check | Expectation |
|---|---|
| Quarry edge healthy | `GET http://edge:8082/health` → 200 |
| NATS reachable | `nats stream info QUARRY_EVENTS` succeeds |
| Browser broker reachable | `grpcurl model-plane-broker:9095 model_plane.v1.BrowserBroker/Health` |
| Data Plane ingest reachable | `curl http://documents-api-go:9001/health` |
| Model Plane gateway reachable | `curl http://model-gateway:8080/health` |

## Failure boundaries

- Data Plane ingest failure → scrape still succeeds; retried later via Temporal workflow.
- Model Plane invoke failure → AgentLoop falls back to `LexicalRanker` for ranking; surfaces `agent.failed` for active loops.
- NATS unavailable → events buffered in process up to channel capacity, then dropped (logged at WARN).
- Browser broker unavailable → Quarry refuses agent action with `Forbidden`. Production deploys never set the validator to `Noop`.
