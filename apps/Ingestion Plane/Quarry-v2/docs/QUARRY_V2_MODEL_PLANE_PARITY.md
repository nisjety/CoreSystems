# Quarry V2 + Model Plane Parity

This document tracks the remaining Quarry V2 output-format parity work that depends on Model Plane or Data Plane capabilities.

## Plane Boundaries

| Plane | Owns | Quarry V2 behavior |
| --- | --- | --- |
| Quarry V2 | Fetching, browser actions, deterministic source artifacts, artifact refs | Store `html`/`raw`/`markdown` first, then request optional enrichments |
| Data Plane | Documents, knowledge units, embeddings, retrieval, RAG facts | Serve retrieval context via `/v1/retrieve`; accept scraped page ingestion through `/internal/v1/documents` |
| Model Plane | LLM inference, agents, synthesis, speech/image/document model providers, browser grants | Generate `summary`, `json`, and `query` answers; later own speech, visual branding, and agentic browsing decisions |

Rule of thumb: Quarry captures evidence; Data Plane knows; Model Plane reasons.

## Current Implementation

| Feature | Status | Current path |
| --- | --- | --- |
| `summary` | Implemented in Quarry V2 bridge | Quarry reads the stored markdown artifact and calls Model Gateway `POST /v1/invoke`; result is stored as `summary` artifact |
| `json` | Implemented in Quarry V2 bridge | String or object format requests call Model Gateway `POST /v1/invoke`; returned JSON is validated and stored as `extract` artifact |
| `query` | Implemented in Quarry V2 bridge | Quarry optionally calls Data Plane `POST /v1/retrieve` using internal auth, then asks Model Plane to answer over the scraped page plus retrieved facts; result is stored as `query` artifact |
| Data Plane retrieval auth | Implemented | `x-internal-key`, `X-Internal-Api-Key`, `x-org-id`, `X-Service-Name: quarry-v2` |
| Zero-data-retention guard | Implemented | AI output formats are rejected when `zeroDataRetention=true` because they send page content to external planes |

## Remaining Model Plane Parity Needs

| Feature | Missing capability | Recommended implementation |
| --- | --- | --- |
| Structured output schema over HTTP | ~~Model Gateway `/v1/invoke` does not expose `structured_output_schema`~~ **Quarry edge now forwards `structured_output_schema` via `ModelPlaneClient::invoke_with_schema` when a json format schema is present.** Model Gateway itself still needs to wire the field through to `InferenceCoreService.Infer`. | Model Gateway: add `structured_output_schema` to HTTP `InvokeRequest` and forward to `InferRequest.structured_output_schema`. Quarry-side contract is complete. |
| `audio` | `/v1/ai/speech` currently returns a queued placeholder and no durable audio artifact | Implement Model Plane speech provider routing, store generated audio in Model Plane or Artifact Plane, then return a JSON/audio artifact reference to Quarry |
| `branding` | No Model Plane visual-analysis or Quarry WASM/static branding extractor is production-ready | Prefer a Quarry-side Rust static extractor for CSS colors/fonts/logos first; add Model Plane visual analysis only for rendered/canvas-heavy pages |
| Agentic browsing | BrowserBroker grant lifecycle exists, but no Quarry V2 contract for agent-driven action planning | Model Plane agent decides next action; Quarry Browser driver executes actions and returns observations; use BrowserBroker only for trusted grants/session lifecycle |
| Data Plane ingest lifecycle | Internal document ingest exists, but immediate retrieval after ingest depends on async indexing | Add an explicit `dataPlaneIngest` option and return the document id/index status; do not assume newly ingested content is retrievable in the same request |

## Transport Recommendation

| Use case | Best transport | Reason |
| --- | --- | --- |
| Current `summary`/`json`/`query` bridge | REST | Existing Model Gateway and Retrieval endpoints already expose REST; easiest to test with mock services |
| Stable high-throughput structured extraction | gRPC | `InferenceCoreService.Infer` already supports `structured_output_schema`; lower overhead and stronger typed contract |
| Long-running speech, document OCR, or agentic browsing jobs | NATS job/event flow | Durable, retryable, observable, and avoids blocking `/v1/scrape` on slow model providers |
| Browser grant lifecycle | gRPC | BrowserBroker already defines typed grant acquire/validate/revoke methods |
| GraphQL | Not recommended for this path | No canonical GraphQL plane contract exists; REST/gRPC/NATS match current service ownership better |

## Language Recommendation

| Component | Language | Why |
| --- | --- | --- |
| Quarry V2 bridge, artifact writes, format parsing | Rust | Hot path, typed contracts, low-latency artifact safety |
| Model Gateway and inference routing | Rust/Go as currently used | Boundary and orchestration services benefit from typed RPC and concurrency |
| Speech provider adapters and visual/document ML sidecars | Python only when provider SDK or model stack requires it | Keep Python out of the hot scrape path; use async jobs for slow model work |
| Static branding extractor | Rust | Deterministic CSS/HTML parsing belongs near Quarry transforms |

## Configuration

Quarry V2 reads these optional environment variables through `QUARRY_EDGE__...`:

| Variable | Purpose |
| --- | --- |
| `QUARRY_EDGE__MODEL_PLANE_BASE_URL` | Model Gateway base URL, for example `http://model-gateway:8080` |
| `QUARRY_EDGE__MODEL_PLANE_BEARER_TOKEN` | Bearer token for Model Gateway; must be real JWT outside local dev bypass |
| `QUARRY_EDGE__MODEL_PLANE_MODEL` | Optional model hint sent to `/v1/invoke` |
| `QUARRY_EDGE__DATA_PLANE_RETRIEVAL_BASE_URL` | Retrieval service base URL, for example `http://retrieval-service:8004` |
| `QUARRY_EDGE__DATA_PLANE_DOCUMENTS_BASE_URL` | Documents service base URL, for example `http://documents-service:8001` |
| `QUARRY_EDGE__DATA_PLANE_INTERNAL_API_KEY` | Internal service key for Data Plane calls; no hardcoded fallback |

## Next Steps

1. Add Model Gateway support for structured JSON schema passthrough to inference-core.
2. Add Quarry-side Rust branding extraction for static CSS/HTML pages.
3. Implement real Model Plane speech provider routing before enabling `audio` as a completed Quarry output.
4. Define the agentic browsing observation/action loop between Model Plane agents and Quarry Browser.
5. Add explicit Data Plane ingest status to Quarry V2 once ingestion is exposed in the public scrape contract.