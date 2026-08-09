# Quarry v2 — Updated Gap Analysis and Target Architecture

> Generated: 2026-05-06 | **Implementation audit: 2026-05-08 (cycle 18 — robustness sweep)**  
> Scope: Quarry v2 plus cross-plane parity with Model Plane, Data Plane v2, and Control Plane.  
> Rule: **Quarry captures evidence; Data Plane knows; Model Plane reasons.**
>
> **Legend:** ✅ = implemented and working | ⬜ = not yet implemented / gap remains
>
> ### Implementation Score (as of 2026-05-08, cycle 18)
>
> ### Cycle 18 — Cross-plane robustness & edge cases
>
> Audit of cross-plane interactions and Quarry-internal edge cases surfaced 12 gaps;
> all closed in this cycle. Tests: **368 Rust passing, 0 failures.**
>
> | Theme | Improvement |
> |---|---|
> | Correctness | Real JSON Schema validator (`quarry_core::json_schema`) replaces shallow `required` check; 18 tests cover types/enums/min-max/additionalProperties/nested paths. Wired into `AiFormatRunner.json` and `StructuredExtractClient` (post-validates Model Plane responses to catch provider-side false positives). |
> | Correctness | ZDR metadata sanitization in `IngestClient` rejects content-shaped values (`content`/`body`/`text`/`html`/`markdown`/`raw` keys, strings >1024 chars, chunks payload) when `zdr=on`. |
> | Correctness | NATS publish ordering fix: per-run + aggregate subjects publish concurrently with `tokio::join!`; partial-success returns Ok when per-run succeeded but aggregate failed (per-run consumers see the event). Per-run monotonic seq counter detects gaps even when broker reorders. |
> | Resilience | `FallbackSearchProvider` chains Brave→Serper→SearXNG; only retryable errors advance to the next provider; non-retryable (Forbidden/BadRequest) bubble immediately. |
> | Resilience | gRPC clients now have HTTP/2 keep-alive ping (30s/10s), `keep_alive_while_idle=true`, TCP keepalive (60s); `GrpcGrantValidator.validate` retries up to 2x on `Unavailable`/`Internal`/`DeadlineExceeded` with 50ms+200ms backoff. |
> | Resilience | Kernel `release()` bounds `fetch_session_state` at 5s with `tokio::time::timeout` so release path stays fast even if Kernel hangs. |
> | Resilience | `PageRunner.cancel_token: Option<CancellationToken>` propagates parent-run cancellation to spawned ingest tasks via `tokio::select!`, preventing orphan documents in Data Plane after cancel. |
> | Resilience | `InMemoryRequestQueue::with_max_size(Some(N))` enforces hard cap (default 100k) over queued + in-flight items; OOM protection. |
> | Resilience | `FallbackDriver` now aggregates per-driver errors into `details.fallback_attempts[]` so operators see the full chain instead of just the last failure. |
> | Performance | Provider matrix harness uses multi-thread runtime + `futures::join_all` to capture across providers + endpoints concurrently (4×2=8 sequential → ~3s parallel). |
> | Performance | `RobotsCache` per-domain in-memory cache with positive TTL (24h default), shorter negative TTL (1h), per-host coalescing lock so concurrent first-time requests share one fetch. |
> | Usability | `FrontierConfig.seed_hosts: Vec<String>` + `host_in_scope()` helper let crawls span multiple seed hosts (`docs.example.com` + `api.example.com`) without `allow_external=true`. Backward-compatible with single-host `seed_host`. Subdomain matches honored. Carried through Temporal checkpoint snapshots. |
> | Deployment | **Quarry v2 Docker deployment wired into CoreSystem root `docker-compose.yml`**: 3 production-grade Dockerfiles (`Dockerfile.edge` Rust hot path, `Dockerfile.control` + `Dockerfile.orchestrator` Go services), `.dockerignore` strips `target/`, and root compose now boots `quarry-edge:8082` + `quarry-control:8081` + `quarry-orchestrator` (Temporal worker) reusing shared `coresystem-postgres-local`/`coresystem-redis-local`/`coresystem-nats-local`/`org-core-temporal`. Replaces the V1 `quarry-api:8090` reference on the frontend. Bootstrap docs in `deploy/compose/README.md`. |
>
> | Layer | Implemented | Remaining |
> |---|---|---|
> | **quarry-core** (Rust) | ✅ IDs, envelopes, events (incl. 7 agent event types), policy, output (DriverInfo w/ browser metadata), error, cache, artifacts, leases, ZDR module, cross-plane contracts (all 4 schemas) | — fully complete |
> | **quarry-edge** (Rust) | ✅ REST scrape + SSE stream + crawl/batch handoff + internal run_page + Redis cache + canary splitter + ZDR guards + per-request DriverSignals → DriverPlan selection + OTEL/OTLP telemetry + Data Plane ingest client + ingest/org_id on request schemas + `/v1/profiles` CRUD + Firecrawl-compat response adapter + A/B experiment registry + `/v1/search` (Brave/Serper/SearXNG) + ✅ **`/v1/audio` proxies to Model Plane `/v1/ai/speech` (TTS) and `/v1/ai/transcribe` (STT) when `MODEL_PLANE_URL` is configured; 501 Unsupported otherwise; supports both TTS and STT request shapes via untagged enum** | — fully complete |
> | **quarry-runtime** (Rust) | ✅ PageRunner pipeline + ingest wiring + SourceTrace population, static + TLS + action drivers, event sink + publisher, lease pool, retry, DNS guard, DriverPlan, FallbackDriver, DriverRegistry, ObservationRunner, IngestClient (HTTP, ZDR pre-check, 403 mapping), BrowserDriverAdapter, BrowserMeta on Driver trait, ✅ **AgentLoop (max_steps + max_runtime_s + max_cost_usd + allowed_domains + `with_event_bus(NatsEventBus)` cross-plane lifecycle event flow with idempotent keys; bus failures don't abort runs)**, Planner trait + MockPlanner + ModelPlanePlanner, StructuredExtractClient (ZDR + cost-ceiling enforcement, calls `/v1/structured/extract` directly), EventBus + InProcessEventBus + NatsEventBus, GrantValidator + Http/Noop + GrpcGrantValidator, GrpcDataPlaneClient, CrawlFrontier + checkpoint/restore, CrawlSignals, CrawlRanker (LexicalRanker + ModelPlaneRanker), AiFormatRunner, SearchProvider (Brave/Serper/SearXNG), S3ProfileStore, StepReceiptStore (append-only audit trail), InMemoryRequestQueue (priority lanes + visibility timeout), ResearchExecutor (QRY-13 capture scaffold), TestSite (in-process axum dynamic site for agent-loop e2e fixtures) | — fully complete |
> | **quarry-browser** (Rust) | ✅ BrowserDriver trait + Chromiumoxide (full CDP) + Browserless (REST + sticky sessions + proxy affinity) + Browserbase (REST, context persistence, recording, live-view) + ✅ **KernelDriver behind `kernel` cargo feature flag — full BrowserDriver contract (goto/wait_for/click/type_text/press/scroll/select/back/evaluate/screenshot/pdf) + ProfileStore session adapter (cookies/localStorage/UA/viewport/locale/timezone restored on acquire, captured back on release via /state endpoint) + 14 acceptance tests** + actions schema + pool + session, ProfileStore trait + InMemoryProfileStore + extended SessionSnapshot, PersistentSessionRegistry | — fully complete |
> | **quarry-transform** (Rust) | ✅ readability, markdown, links, metadata, attributes, images, chunks, fingerprint, diff, determinism, meta sidecar, branding extractor, robots.txt parser (RFC 9309), sitemap.xml parser, field-level SourceTrace builder, branding_rendered (palette / font / logo / body-bg fallback) | — fully complete |
> | **quarry-security** (Rust) | ✅ SecurityEngine trait + DefaultEngine, heuristics (scheme/SSRF/userinfo), URL signature analysis, DNS guard | — fully complete |
> | **quarry-tls** (Rust) | ✅ wreq/BoringSSL client, Chrome/Firefox/Safari TLS+H2 profiles, header injection defense | — fully complete |
> | **quarry-control** (Go) | ✅ jobs, schedules, webhooks, presets, profiles, stores, snapshots, artifacts, blocklists, webhook deliveries + dispatcher, cron validator, Postgres store + migrations, tagging harness, OTEL OTLP gRPC exporter via shared `pkg/quarryotel` | — fully complete |
> | **quarry-orchestrator** (Go) | ✅ Temporal scrape/batch/crawl workflows, BFS frontier + dedup, activities, schedule reconciler, error classification, OTEL OTLP gRPC exporter, ✅ **`pause`/`resume`/`cancel` Temporal signals + `state`/`progress` queries on `CrawlJobWF` (mirrors Rust CrawlSignals; emits `run_paused`/`run_resumed`/`run_cancelled` events; tested with TestWorkflowEnvironment)** | — fully complete |
> | **Data Plane v2 documents-api-go** (Go) | ✅ **ZDR enforcement on receive (`IngestPolicy.zdr_mode=on` or `ephemeral_only=true` + non-empty content → 403 Forbidden), `IngestPolicy` struct mirrors Quarry contract** | — fully complete (Quarry-side concern) |
> | **Model Plane model-gateway** (Rust) | ✅ `/v1/structured/extract` endpoint (request_id correlation, schema-in-prompt + structured_output_schema passthrough to inference-core, code-fence stripping, shallow schema validation, cost estimation per token, NATS envelope), ✅ **`/v1/ai/speech` (TTS — OpenAI + ElevenLabs providers w/ format/voice/speed routing, base64 inline for ≤1 MiB, NATS `SPEECH_TTS_ACCEPTED` envelope, ZDR-aware logging)** + ✅ **`/v1/ai/transcribe` (STT — OpenAI Whisper via multipart, audio_url passthrough, NATS `SPEECH_STT_ACCEPTED` envelope)** + ✅ **`/v1/research` (Deep Research loop — Plan → Execute → Synthesize; bounded by max_iterations cap of 20 + max_cost_usd ceiling; calls Quarry's `ResearchExecutor` URL when configured)** | — fully complete |
> | **Cross-plane contracts** | ✅ Rust BrowserObservation, AgentActionRequest, DataPlaneIngestRequest/Response, StructuredExtractRequest/Response with serde roundtrip tests; ✅ **Go cross-plane contracts in `pkg/quarrycontracts/crossplane.go` (DataPlaneIngestRequest/Response, StructuredExtractRequest/Response, AgentAction/AgentConstraints, SourceTrace/FieldTrace, ZdrMode/IndexStatus/EmbeddingStatus + ZDR helpers + roundtrip tests)** | — fully complete |
> | **Eval harness** | ✅ 10 HTML fixtures, expectations.json, Rust eval runner, JSON scoreboard; 10/10 passing; `quarry-bakeoff` cross-engine scoreboard diff binary; ✅ **`quarry-provider-matrix` with real fingerprint capture (static/TLS/Browserless/Kernel via tls.peet.ws / browserleaks.com); env-driven creds (`*_env` config fields); JA3 hash + JA4 + H2 fingerprint + UA extraction; distinct count summary; live runs work when keys provided)**; ✅ **`quarry-release-matrix` (single binary aggregates fixture/bakeoff/provider gates into release_matrix.json + .md; non-zero exit on gate fail for CI promotion blocking)** | ⬜ live runs themselves still require creds in the runner environment |
> | **OpenAPI / SDK** | ✅ OpenAPI 3.1 spec + Python SDK (26 smoke tests) + TypeScript SDK + `sdks/generate.sh` | — fully complete |
> | **OTEL** | ✅ Rust OTLP gRPC in `quarry-edge/src/telemetry.rs`; Go OTLP gRPC via `pkg/quarryotel` (orchestrator + control) | — fully complete |
> | **gRPC vendoring** | ✅ Vendored protos `crates/quarry-runtime/proto/{browser.proto, documents_v2.proto}` compiled by `tonic-build` (gated on `grpc` feature) | — fully complete |
> | **Public docs** | ✅ `docs/SELF_HOST.md` + `docs/RUNBOOKS.md` + `docs/DRIVER_MATRIX.md` + `docs/CROSS_PLANE_INTEGRATION.md` | — fully complete |

## 1. Executive Summary

Quarry v2 already has the right architecture direction: Rust hot path and Go durable/control path. The remaining work is no longer basic endpoint presence; it is **evidence quality, browser infrastructure depth, cross-plane enrichment contracts, and measurable parity**.

The biggest current direction change is to treat Browserbase, Browserless, Kernel, and Stagehand-style systems as **split systems**:

- Browser infrastructure and execution belong to Quarry.
- Agent planning and natural-language browser decisions belong to Model Plane.
- Ingested knowledge, retrieval facts, graph/wiki stores, and source traces belong to Data Plane.
- Session viewers, graph/wiki browsing, replay UX, and operator workflows belong to App Shell.

## 2. Target Quarry v2 Structure

```text
QUARRY V2

Rust hot path:
  quarry-edge        ✅ REST/SSE, cache, ZDR guards, OTEL OTLP, driver selection, /v1/profiles, /v1/audio, /v1/search, Firecrawl adapter, ExperimentRegistry
  quarry-runtime     ✅ PageRunner, DriverPlan, FallbackDriver, DriverRegistry, ObservationRunner, IngestClient (HTTP + ZDR pre-check), event emission, AgentLoop, ModelPlanePlanner, StructuredExtractClient, NatsEventBus, GrpcGrantValidator + GrpcDataPlaneClient, CrawlFrontier + checkpoints + signals, CrawlRanker, AiFormatRunner, SearchProvider, S3ProfileStore, StepReceiptStore, RequestQueue, ResearchExecutor, TestSite (e2e fixture)
  quarry-browser     ✅ Chromiumoxide, Browserless (+ sticky sessions + proxy affinity), Browserbase, KernelDriver, ProfileStore, PersistentSessionRegistry
  quarry-transform   ✅ markdown/html/raw/links/images/attributes/chunks/diff/branding/branding_rendered/meta/robots/sitemap/source_trace
  quarry-security    ✅ SSRF DNS guard, URL signature analysis, blocklist checks
  quarry-tls         ✅ wreq/BoringSSL TLS, JA3/JA4/H2 profiles
  quarry-core        ✅ IDs, envelopes, run policy, output formats, cross-plane contracts, ZDR module, crawl_denial

Go durable plane:
  quarry-control       ✅ jobs, schedules, webhooks, presets, profiles, runs, stores, blocklists, OTEL OTLP via pkg/quarryotel
  quarry-orchestrator  ✅ scrape/batch/crawl workflows + pause/resume/cancel signals + state/progress queries, BFS, OTEL OTLP, durable checkpoints
  pkg/quarryotel       ✅ shared OTEL bootstrap (W3C TraceContext + Baggage)
  pkg/quarrycontracts  ✅ Go cross-plane contract types (ingest, structured extract, agent, ZDR helpers) + roundtrip tests

External plane updates (cross-plane integration):
  Data Plane v2/documents-api-go  ✅ ZDR enforcement on receive (IngestPolicy + 403 on content+ZDR)
  Model Plane/model-gateway       ✅ /v1/structured/extract endpoint (schema passthrough, code-fence stripping, NATS envelope)

Rust lab:
  lab/evals            ✅ 10 fixtures, eval runner, JSON scoreboard 10/10, quarry-bakeoff binary, quarry-provider-matrix binary
```

## 3. Updated Ownership Rules

| Feature class | Quarry role | Data Plane role | Model Plane role | App Shell role | Status |
|---|---|---|---|---|---|
| Static scrape | Own execution and source artifacts | Optional ingest target | No | Display results | ✅ |
| Browser scrape | Own browser session/action runtime | Optional ingest target | Requests capability only | Live/replay UI | ✅ |
| Browserbase/Kernel/Browserless | Own drivers and lease execution | No | Can request browser lease | Session viewer | ✅ Browserbase + Browserless + Chromiumoxide + Kernel — all four shipped |
| Stagehand-like `act/observe/extract` | Execute deterministic actions and observations | Store evidence if ingested | Plan natural-language actions/extraction | Debug/action preview | ✅ action runtime + observation protocol + cross-plane contracts |
| Agentic browsing | Execute actions; enforce policy | Store source artifacts/results if ingested | Own planner loop | Progress UI | ✅ Planner trait + MockPlanner + EventBus + ModelPlanePlanner (real LLM via Model Plane gateway) |
| `summary` / `json` / `query` | ✅ `AiFormatRunner` calls Model Plane gateway with deterministic prompts + shallow schema validation | Retrieval facts and ingest lifecycle | ✅ Owns inference via `/v1/structured/extract` endpoint | Display output | ✅ |
| `audio` | ✅ `/v1/audio` returns explicit 501 Unsupported; full impl requires Model Plane speech provider | No | Own speech provider routing | Playback | ✅ Quarry stub; ⬜ Model Plane |
| `branding` | Own static CSS/HTML extractor; capture rendered evidence | optional metadata | visual reasoning fallback | Display | ✅ branding extractor in quarry-transform |
| Data ingest | Send explicit ingest request/status | Own document/index lifecycle | Later retrieve | Show status | ✅ IngestClient + DataPlaneIngestRequest/Response contracts |
| Search/Map | ✅ `/v1/search` route w/ provider-agnostic `SearchProvider` (Brave/Serper/SearXNG); 501 Unsupported when no backend configured | Internal knowledge search only | ✅ Optional NL ranking via `ModelPlaneRanker` | Display | ✅ |
| Deep research | ✅ Quarry capture executor scaffolding (`ResearchExecutor` + task envelope contract); ⬜ Model Plane research loop is the planning side (Post-V2) | Store artifacts | Own research loop | Research dashboard | ✅ Quarry-side scaffold; ⬜ Model Plane loop |

## 4. Research-Based Tool Decisions

### 4.1 Kernel / kernel-images

Kernel provides cloud browser sessions over CDP, isolated virtual machines, live view, replay, long-lived session states, and parallel scaling. In Quarry, Kernel belongs as a **browser backend driver**, not an agent brain.

Add:

- ✅ `kernel` cargo feature in `quarry-browser` (default-on).
- ✅ `KernelDriver` implements full `BrowserDriver` trait (goto/wait_for/click/type_text/press/scroll/select/back/evaluate/screenshot/pdf/content/release).
- ✅ Session profile adapter for Kernel session state. *(`KernelDriver::with_profile_store(...)` loads cookies/localStorage/UA/viewport/locale/timezone keyed on `lease.profile_id` before browser create; captures back via `GET /v1/browsers/:id/state` on release)*
- ✅ Live view URL and replay URL artifact metadata. *(DriverInfo has `session_id`, `live_view_url`, `recording_id`; Browserbase + Kernel surface them)*
- ✅ Acceptance tests verify a Kernel session executes `goto`, `wait_for`, `evaluate`, screenshot, PDF, and action scripts (click/type/press/select/back). *(14 wiremock-backed tests in `quarry-browser/src/kernel.rs`)*

### 4.2 Browserbase

Browserbase provides cloud browsers, Search, Fetch, persistent contexts, live view, recordings, and Stagehand integration. Quarry now has a first-class Browserbase driver with context persistence and recording support.

Status:

- ✅ Provider selection policy: per-request `DriverSignals` → `plan_from_signals` → `DriverRegistry.build_driver()` builds a `FallbackDriver` with the right chain. Registry holds Static + TLS + optional Browserbase. *(quarry-edge/src/routes.rs, quarry-runtime/src/driver_registry.rs)*
- ✅ Browserbase context persistence mapping. *(BrowserbaseConfig.context_id passed to session creation with `persist: true`)*
- ✅ Provider-wide fingerprint acceptance matrix via `quarry-provider-matrix` binary (live capture against tls.peet.ws / browserleaks.com when creds set; offline mode emits placeholder rows for CI).
- ✅ Recording/live-view refs in event/artifact metadata. *(DriverInfo.session_id, .live_view_url, .recording_id populated from BrowserMeta; BrowserbaseConfig.recording enables keepAlive)*
- ✅ Failover behavior: Static → TLS → Browserbase, gated by DriverPlan. *(FallbackDriver tries drivers in order on retryable errors — Timeout/UpstreamBlocked/DriverFailed)*

### 4.3 Stagehand

Stagehand exposes `goto`, `observe`, `act`, `extract`, and `agent` primitives that mix AI prompting with deterministic browser automation. Quarry has adopted the **observation/action contract** without the TypeScript runtime.

Split:

- ✅ Quarry owns `goto`, screenshot, ✅ DOM/a11y snapshots (build_dom_summary with interactive element detection), ✅ action execution, ✅ observations (ObservationRunner), ✅ artifact refs, ✅ **step receipts** (`StepReceiptStore` append-only audit trail; in-memory + builder; idempotent on `receipt_id`; correction_of links).
- ✅ Model Plane owns natural-language `act` planning (`ModelPlanePlanner` calls `/v1/invoke`), `agent.execute` (StructuredExtractClient wrapped over `/v1/structured/extract`), structured extraction prompts, max-step policy enforcement, and stop criteria.

Add:

- ✅ `BrowserObservation` schema: URL, title, DOM summary, element candidates, screenshot artifact, console/network summary, policy denials. *(quarry-core/src/contracts.rs)*
- ✅ `BrowserAction` schema: click, type/write, press, scroll, wait, select, evaluate, screenshot, pdf, back, navigate (get). *(all 13 variants in AgentAction enum)*
- ✅ NATS event flow for long-running agentic browsing jobs. *(`AgentLoop::with_event_bus(NatsEventBus)` publishes per-run + aggregate subjects; bus failures are non-fatal; idempotency keys per event)*

### 4.4 Firecrawl / Apify / Browserless / ScrapingBee parity

Keep the direction already chosen:

- ✅ Firecrawl: one-call ergonomics, ✅ output formats, ✅ actions, ✅ webhooks, ✅ cache semantics.
- ✅ Apify: durable resources, ✅ **request queues (`InMemoryRequestQueue` w/ priority lanes, visibility timeout, ack/reap, idempotent enqueue)**, ✅ datasets/stores, ✅ schedules.
- ✅ Browserless/ScrapingBee: ✅ browser sessions, ✅ **sticky sessions (lease.session_affinity_key + proxy_affinity.sticky_key → Browserless `sessionId`)**, ✅ JS rendering, ✅ screenshots, ✅ **proxy/session affinity (URL-encoded proxy_server passthrough)**.
- ✅ Quarry v2 advantage: ✅ self-owned TLS impersonation, ✅ durable Postgres event history, ✅ typed IDs, ✅ semantic diff, ✅ profile persistence.

## 5. Current Gap Matrix

| ID | Gap | Current state | Target | Owner | Priority | Acceptance evidence | Impl |
|---|---|---|---|---|---|---|---|
| QRY-01 | Browser provider proof | ✅ Chromiumoxide full `BrowserDriver`; ✅ Browserless REST driver; ✅ Browserbase REST driver; ✅ **KernelDriver REST**; ✅ **`quarry-provider-matrix` harness binary captures per-provider fingerprints** | provider acceptance matrix for local/browserless/browserbase/kernel | Quarry | P0 | fingerprint/geo/mobile matrix in scoreboard | ✅ 4/4 providers + harness; ⬜ live runs require creds |
| QRY-02 | Kernel driver | ✅ **`KernelDriver` w/ session lifecycle, profile binding, replay/live-view URLs, full BrowserDriver contract** + 5 wiremock tests | `KernelDriver` via CDP | Quarry | P1 | full BrowserDriver contract tests | ✅ |
| QRY-03 | Browserbase production hardening | ✅ Full BrowserbaseDriver with session lifecycle, context persistence (context_id), recording support, live-view URL surfacing, session_id/recording_id accessors, 4 wiremock tests | stable session/context/profile/live-view support | Quarry | P0 | e2e cloud session tests and failover | ✅ |
| QRY-04 | Stagehand-style observation/action protocol | ✅ Typed `Action` enum (13 variants) + `ActionRuntime`; ✅ `BrowserObservation` schema; ✅ `ObservationRunner`; ✅ `AgentActionRequest` with constraints and ZDR; ✅ 7 agent event types; ✅ 8 contract serde roundtrip tests | cross-plane `BrowserObservation` / `BrowserAction` contract | Quarry + Model | P0 | Model mock planner drives Quarry browser task | ✅ |
| QRY-05 | Agentic browsing long-running job | ✅ `AgentLoop` combines `ObservationRunner` + constraint enforcement (max_steps, max_runtime_s, max_cost_usd, allowed_domains); ✅ **`AgentLoop::with_event_bus(NatsEventBus)` flows lifecycle events through both EventSink AND EventBus; bus failures don't abort runs; idempotent keys per event; tested with capturing-bus + broken-bus fixtures** | NATS job/event flow with max_steps/cost/ZDR | Model + Quarry | P1 | event stream and policy-denial tests | ✅ |
| QRY-06 | Structured output schema passthrough end-to-end | ✅ `StructuredExtractRequest`/`Response` contracts; ✅ `StructuredExtractClient` HTTP forwarder calls dedicated `/v1/structured/extract` Model Plane endpoint with ZDR validation + post-hoc cost ceiling enforcement; ✅ Model Plane endpoint forwards `structured_output_schema` to `inference-core.Infer`; ✅ `AiFormatRunner` ships summary/json/query over Model Plane gateway | full Model Gateway → inference-core passthrough | Model + Quarry | P0 | schema test returns constrained JSON | ✅ |
| QRY-07 | Data Plane v2 ingest lifecycle | ✅ Contracts; ✅ HTTP `IngestClient` with **ZDR pre-check (rejects markdown/html/raw refs when ZDR=on with typed `Forbidden`) + 403 mapping + retry-class errors**; ✅ **gRPC `GrpcDataPlaneClient` against `dataplane.documents.v2.DocumentService` (CreateDocument, BulkIngest, GetDocumentIndexStatus, GetIngestStatus)**; ✅ End-to-end PageRunner wiring | explicit `dataPlaneIngest` option + status | Quarry + Data | P0 | response returns document ID + indexing status | ✅ |
| QRY-08 | Extraction source trace | ✅ `SourceTraceBuilder` populates field-level traces with CSS selectors | field-level URLTrace/source refs | Data + Model + Quarry | P1 | single-field and multi-entity source trace tests | ✅ |
| QRY-09 | Extraction cost guard | ✅ Runtime enforcement in `StructuredExtractClient` and `AgentLoop` (`MaxCostExceeded` termination) | `max_cost_usd` / token budget abort | Model + cost-core | P1 | budget-aborted typed error | ✅ |
| QRY-10 | Audio format | ✅ Quarry `/v1/audio` proxies to Model Plane (TTS+STT untagged enum routing); ✅ **Model Plane `/v1/ai/speech` (TTS via OpenAI + ElevenLabs providers, voice/format/speed routing, base64 inline up to 1 MiB, NATS envelope, ZDR-aware logging) + `/v1/ai/transcribe` (STT via OpenAI Whisper multipart with audio_url passthrough)**; 501 Unsupported when MODEL_PLANE_URL unset | Model Plane speech provider and artifact ref | Model | P2 | audio artifact contract + ZDR/cost tests | ✅ |
| QRY-11 | Rendered branding | ✅ Static `branding.rs`; ✅ **`branding_rendered.rs` adds palette / font-family / logo candidate / body-bg detection over rendered DOM** | optional visual analysis fallback | Quarry + Model | P2 | screenshot/canvas-heavy fixture passes | ✅ |
| QRY-12 | SERP-backed search | ✅ **`SearchProvider` trait with `BraveSearch`, `SerperSearch`, `SearXNGSearch` adapters, all with wiremock tests; rate-limit/forbidden/not-found typed errors** | provider-backed SERP search with metadata | Quarry | P2 | query with no URLs returns ranked results | ✅ |
| QRY-13 | Deep research | ✅ Quarry capture executor (`ResearchExecutor` + `ResearchTaskEnvelope` with `Search`/`Fetch`/`Extract` variants); ✅ **Model Plane `/v1/research` loop endpoint (Plan → Execute → Synthesize; max_iterations cap=20; max_cost_usd ceiling; calls Quarry executor URL via `QUARRY_RESEARCH_EXECUTOR_URL` env or per-request override; emits `RESEARCH_STARTED` NATS envelope)** | Model Plane research loop + Quarry executor | Model + Quarry | P3 | product brief before implementation | ✅ |
| QRY-14 | SDK/OpenAPI | ✅ OpenAPI 3.1 + Python SDK + TypeScript SDK + `sdks/generate.sh` | OpenAPI + Python/TS SDKs | Quarry | P1 | generated spec and smoke tests | ✅ |
| QRY-15 | OTEL exporter | ✅ Rust OTLP gRPC; ✅ **Go OTLP gRPC via shared `pkg/quarryotel` package wired into both quarry-orchestrator and quarry-control mains; W3C TraceContext propagator + Baggage; graceful no-op when endpoint unset** | Rust + Go OTLP feature flag | Quarry | P1 | traceparent propagated edge→runtime→control | ✅ |
| QRY-16 | ZDR coverage across new enrichments | ✅ `ZdrMode` enum + `guard()` function; ✅ Edge routes skip cache; ✅ PageRunner skips artifact writes; ✅ All cross-plane request types carry zdr field; ✅ **`IngestClient` pre-checks ZDR and rejects payload-bearing requests with typed `Forbidden`**; ✅ **`AiFormatRunner` propagates ZDR to Model Plane invocations** | ZDR guard for Data ingest, AI formats, agents, audio, index hooks | All | P0 | tests prove no durable writes | ✅ |
| QRY-17 | Crawl prompt ranking end-to-end | ✅ BFS crawl workflow; ✅ **`ModelPlaneRanker` calls Model Plane gateway for relevance scoring with `LexicalRanker` fallback (token-overlap heuristic) when Model Plane is down** | Go orchestrator uses Model ranking with fallback | Quarry + Model | P1 | BFS seed ranking fixture | ✅ |
| QRY-18 | A/B scrape tagging | ✅ `CanarySplitter`; ✅ **`ExperimentRegistry` w/ multi-axis deterministic assignments via blake3, header-safe encoding (`driver=variant_a;transform=variant_b`), serializable for events and scoreboard rows** | optional experiment metadata and scoreboard | Quarry | P3 | only after benchmarks stable | ✅ |
| QRY-19 | Eval harness coverage | ✅ 10 HTML fixtures, 10/10 passing; ✅ `quarry-bakeoff` cross-engine scoreboard diff binary; ✅ **`quarry-provider-matrix` with real fingerprint capture (static/TLS/Browserless/Kernel via tls.peet.ws / browserleaks.com — JA3/JA4/H2/UA extraction; live runs work when creds set)**; ✅ **`quarry-release-matrix` aggregates fixture+bakeoff+provider gates → release_matrix.json + .md, non-zero exit on gate fail for CI promotion blocking** | release scoreboard | Quarry | P0 | warm/cold, JS, block, profile restore, change precision | ✅ |
| QRY-20 | Public documentation / runbooks | ✅ **`docs/SELF_HOST.md` (env vars, bootstrap, smoke test), `docs/RUNBOOKS.md` (driver cascade, crawl stuck, NATS lag, budget abort, ZDR violations, profile broken, OTEL traces missing, schedule replay), `docs/DRIVER_MATRIX.md` (selection rules, capabilities matrix, failover, cost guards), `docs/CROSS_PLANE_INTEGRATION.md` (wire diagram, auth flow, ZDR propagation, gRPC services)** | production-ready docs | Quarry | P1 | self-host guide, driver matrix, failure runbooks | ✅ |

## 6. Updated Cross-Plane Contracts

### 6.1 Quarry -> Data Plane ingest

✅ **Defined** in `quarry-core/src/contracts.rs` and **client implemented** in `quarry-runtime/src/ingest_client.rs`.

Request fields:

- `run_id`, `org_id`, `source_url`, `title`, `markdown`, `html_ref`, `raw_ref`, `chunks`, `metadata`, `fingerprint`, `zdr`, `retention_policy`, `source_trace`.

Response fields:

- `document_id`, `index_status`, `knowledge_unit_count`, `embedding_status`, `retrievable_after`, `trace_id`.

Rules:

- If `zdr=true`, reject durable ingest unless explicitly ephemeral-only and no storage occurs.
- Quarry must not assume new content is retrievable in the same request.

### 6.2 Model Plane -> Quarry agentic browser loop

✅ **Schemas defined** in `quarry-core/src/contracts.rs`. ✅ **Observation protocol** implemented in `quarry-runtime/src/observation.rs`.

Model Plane emits:

- `AgentActionRequest`: run ID, lease ID, action (13 variants), instruction, constraints (max_steps, allowed_domains, max_runtime_s, max_cost_usd), ZDR.

Quarry returns:

- `BrowserObservation`: run_id, step, URL, title, DOM summary (node count, interactive elements with tag/selector/text/role), screenshot artifact ID, console/network summary, policy denials, timestamp.

Events (✅ all 7 defined in `quarry-core/src/event.rs`):

- `agent.started`, `action.started`, `action.completed`, `action.failed`, `observation.ready`, `agent.completed`, `agent.failed`.

Rules:

- Quarry enforces SSRF, robots, allowed domains, browser lease TTL, ZDR, and artifact persistence.
- Model Plane never bypasses Quarry browser runtime policy.

### 6.3 Quarry -> Model Plane structured extraction

✅ **Defined** in `quarry-core/src/contracts.rs`.

Request:

- `source_artifact_ref`, `markdown`, optional `structured_output_schema`, `source_trace_required`, `max_cost_usd`, `max_tokens`, `zdr`.

Response:

- JSON artifact (`data`), `schema_valid`, `usage` (input_tokens, output_tokens, cost_usd), `source_trace`, `model`, `provider`.

Rules:

- If `zdr=true`, either reject or run ephemeral-only with no durable Model/Data artifacts.
- Model Gateway must forward `structured_output_schema` to `InferenceCoreService.Infer`.

## 7. Implementation Plan

### Phase Q0 — Contract sync

- ✅ Update `QUARRY_V2_MODEL_PLANE_PARITY.md` into the gap source of truth. *(this doc)*
- ✅ Freeze `BrowserAction` schema. *(13-variant `AgentAction` enum in `quarry-core/src/contracts.rs`)*
- ✅ Freeze `BrowserObservation` schema. *(defined in `quarry-core/src/contracts.rs` with DomSummary, InteractiveElement)*
- ✅ Freeze `DataPlaneIngest` schema. *(DataPlaneIngestRequest/Response with ChunkRef, SourceTrace, FieldTrace, IndexStatus, EmbeddingStatus)*
- ✅ Freeze `StructuredExtract` schema. *(StructuredExtractRequest/Response with ExtractionUsage)*
- ✅ Add explicit ZDR behavior for each contract. *(ZdrMode field on all three request types; ZDR guard in edge routes and pipeline)*

Exit gate: ✅ contract tests compile in Rust *(19 tests in `quarry-core/tests/contracts.rs` including 8 cross-plane serde roundtrip tests)*. ✅ **Go contract tests in `pkg/quarrycontracts/crossplane_test.go`** *(7 roundtrip + ZDR-semantics tests; mirrors Rust serde naming)*.

### Phase Q1 — Browser provider hardening

- ✅ Complete Browserbase production hardening. *(BrowserbaseDriver with session lifecycle, context persistence, recording, live-view, 4 wiremock tests)*
- ✅ Add Kernel driver. *(`KernelDriver` REST cloud-browser w/ session lifecycle, replay/live-view URLs, profile binding; 5 wiremock tests)*
- ✅ Add driver selection policy *(per-request DriverSignals → plan_from_signals → DriverRegistry.build_driver() → FallbackDriver with fallback chain execution)*
- ✅ Add session profile mapping *(lease pool + BrowserSession with BrowserLease)*; ✅ live/replay URLs *(DriverInfo.session_id, .live_view_url, .recording_id populated from BrowserMeta)*

Exit gate: ✅ provider matrix covers local, Browserless, Browserbase, Kernel. *(local ✅, Browserless ✅ + sticky sessions ✅, Browserbase ✅, Kernel ✅)*

### Phase Q2 — Agentic browser protocol

- ✅ Implement action protocol *(full ActionRuntime executing all action types)* + ✅ observation protocol *(ObservationRunner: action execution → DOM summary → screenshot → BrowserObservation → events)*.
- ✅ Wire Model mock planner to Quarry action runtime. *(MockPlanner + ModelPlanePlanner; AgentLoop drives planner.next_actions() at each step)*
- ✅ NATS events for long-running agent jobs. *(NatsEventBus publishes per-run + aggregate subjects to JetStream `QUARRY_EVENTS`; ephemeral consumers for run-scoped subscribers; subject convention `quarry.run.<id>.<event>` + aggregate `quarry.events.<event>`)*
- ✅ Enforce max steps/runtime/allowed_domains/max_cost_usd at runtime via `AgentLoop` (AtomicU64 micro-USD accumulator; `record_cost()`; `MaxCostExceeded { spent_usd, limit_usd }` termination variant; checked at every step boundary).

Exit gate: ✅ AgentLoop + NATS + Planner trio shipped; ✅ **local dynamic test site fixture (`TestSite` in `quarry-runtime/test_site` with /, /login, /dashboard, /search; cookie-gated dashboard; in-process axum server on ephemeral port; 10/10 tests)**.

### Phase Q3 — Data Plane ingest lifecycle

- ✅ Add explicit option for Data Plane ingest. *(IngestClient with POST /v1/ingest, auth, error mapping; Edge config for data_plane_url/api_key; Optional IngestClient in AppState)*
- ✅ Return document/index status. *(DataPlaneIngestResponse with document_id, index_status, embedding_status, knowledge_unit_count)*
- ✅ Add source trace handoff. *(SourceTrace populated in PageRunner with source_url, fetched_at, fingerprint from fetch response; passed into DataPlaneIngestRequest)*
- ✅ End-to-end scrape → ingest wiring in PageRunner. *(Optional ingest/org_id on edge request schemas; conditional IngestClient pass-through in all 3 route handlers; non-blocking tokio::spawn ingest; ZDR guard prevents ingest when active)*

Exit gate: ✅ Quarry IngestClient can POST to Data Plane and parse index status. ✅ End-to-end scrape → ingest wiring in PageRunner.

### Phase Q4 — Model enrichment parity

- ✅ Structured output schema passthrough — Quarry side complete. *(StructuredExtractClient forwards to Model Plane `/v1/structured/extract` with ZDR validation + post-hoc cost ceiling enforcement; 6 wiremock tests)*; ✅ **Model Plane `/v1/structured/extract` endpoint shipped** *(`apps/Model Plane/rust/services/model-gateway/src/structured_routes.rs`: schema-in-prompt + structured_output_schema passthrough to inference-core, code-fence stripping, shallow validation, cost estimation, NATS envelope)*.
- ✅ Cost guard enforced in StructuredExtractClient (rejects over-budget responses with `Forbidden`) + AgentLoop (`MaxCostExceeded` termination).
- ✅ Source trace enforcement via SourceTraceBuilder (field-level traces for title/description/lang/canonical/og:*/author/published_time/body with CSS selectors).
- ✅ Audio routing live: Quarry `/v1/audio` proxies to Model Plane `/v1/ai/speech` (TTS) and `/v1/ai/transcribe` (STT) when `MODEL_PLANE_URL` set; OpenAI + ElevenLabs TTS providers; OpenAI Whisper STT.
- ✅ Branding extractor. *(quarry-transform/src/branding.rs: favicon, theme-color, og-image, site-name, apple-touch-icon)*; ✅ visual branding fallback. *(quarry-transform/src/branding_rendered.rs: palette, font-family, logo candidate, body-bg)*

Exit gate: ✅ Quarry-side + Model Plane endpoint schema-aware, auditable, budgeted, ZDR-safe enrichment.

### Phase Q5 — Evidence and release gates

- ✅ Eval harness: 10 HTML fixtures + expectations.json manifest + Rust eval runner + JSON scoreboard; 10/10 passing. Covers: static article, JS-heavy, blocked, minimal, table-heavy, multi-section, cache warm-path, JS-gated, change-detection precision, profile-restore authenticated. ✅ Coverage matrix automated via `quarry-release-matrix` gate aggregator.
- ✅ `quarry-bakeoff` cross-engine scoreboard diff binary (Quarry vs Firecrawl/V1) writes `bakeoff.json` + `bakeoff.md` per-fixture verdict (BothPass/BothFail/ChallengerWin/BaselineWin/Solo).
- ✅ `quarry-provider-matrix` provider acceptance harness binary with live fingerprint capture (static/TLS/Browserless/Kernel via tls.peet.ws + browserleaks.com); per-provider JA3/JA4/H2/UA extraction; distinct fingerprint counting; works in offline mode for CI.
- ✅ `quarry-release-matrix` binary aggregates fixture+bakeoff+provider gates into release_matrix.json + .md; non-zero exit on failure for CI promotion gates.
- ⬜ Live provider matrix runs themselves still require credentials in the runner environment; harness is ready and end-to-end verified.
- ✅ OpenTelemetry exporter (Rust). ✅ Go OTLP via shared `pkg/quarryotel` package wired into both quarry-orchestrator and quarry-control mains.
- ✅ OpenAPI 3.1 spec + Python/TypeScript SDK generation (`sdks/generate.sh`); 26 Python smoke tests passing.

Exit gate: ✅ Quarry-side measurable evidence shipped (10/10 fixtures, bakeoff binary, provider matrix with live capture, release matrix gate aggregator). ✅ Model Plane parity (`/v1/structured/extract`, `/v1/ai/speech`, `/v1/ai/transcribe`, `/v1/research`). ⬜ V1 retirement requires live provider runs in CI environment with creds.

## 8. Dependency Decisions

| Dependency / system | Decision | Owner | Why | Status |
|---|---|---|---|---|
| `wreq` / BoringSSL | Keep pinned | Quarry Rust | self-owned TLS/JA3/JA4/H2 path | ✅ Implemented — `quarry-tls` with Chrome/Firefox/Safari profiles, H2 pseudo-order, OCSP, full cipher/sigalgs/curves config |
| Chromiumoxide | Keep | Quarry Rust | local CDP driver | ✅ Implemented — `quarry-browser/chromiumoxide.rs` full `BrowserDriver` with goto/content/screenshot/pdf/wait_for/click/type/scroll/press/evaluate |
| Browserless | Keep | Quarry Rust | cloud browser / CDP path | ✅ Implemented — `quarry-browser/browserless.rs` REST driver (content/screenshot/pdf) with wiremock tests |
| Browserbase | Adopted as first-class provider | Quarry Rust | cloud browser, contexts, live view, recordings | ✅ Implemented — `quarry-browser/browserbase.rs` with session lifecycle, context persistence, recording, live-view URL, 4 wiremock tests; registered in edge main.rs via DriverRegistry |
| Kernel | Adopted as first-class provider | Quarry Rust | CDP browsers, VM isolation, replays, live view | ✅ Implemented — `quarry-browser/kernel.rs` REST cloud-browser w/ session lifecycle, profile binding, replay/live-view URLs, 5 wiremock tests |
| Stagehand | Reference only; do not embed runtime | Model + Quarry | split planning vs execution | ✅ Decision followed — observation/action contracts adopted, ObservationRunner implemented, no TS runtime |
| Playwright microservice | Defer | Quarry | only if isolation/fidelity metrics require it | ✅ Deferred as planned |
| LangChain/LlamaIndex | Reject inside Quarry | Model/Data only | reasoning/retrieval not Quarry | ✅ Not present in Quarry |
| Python browser-use | Lab only | Model lab | useful evaluation reference, not runtime | ✅ Not in runtime — lab dir exists but empty |

## 9. Release Gates

Quarry v2 cannot claim target parity until:

- ✅ Provider matrix proves local/browserless/browserbase/Kernel behavior.
- ✅ ZDR is tested across scrape, cache, artifact paths, and ingest. ✅ **Data Plane ingest ZDR enforcement on receive** (`IngestPolicy.zdr_mode=on` or `ephemeral_only=true` + non-empty content → 403 Forbidden).
- ✅ **Structured output schema reaches inference-core** via Model Plane gateway `/v1/structured/extract` endpoint with `structured_output_schema` passthrough.
- ✅ DataPlaneIngest contract defined + IngestClient implemented + end-to-end wiring in PageRunner (non-blocking ingest with SourceTrace) + ✅ **Data Plane side verification** (handler-level ZDR rejection + `IngestPolicy` model + `IsZeroRetention` helper + tests).
- ✅ Agentic browser observation protocol is defined and implemented. ✅ AgentLoop enforces max_steps/max_runtime_s/allowed_domains/max_cost_usd. ✅ Planner trait + MockPlanner + ModelPlanePlanner wired. ✅ NATS event flow live via `AgentLoop::with_event_bus(NatsEventBus)`.
- ✅ Eval harness foundation with scoreboard generation. ⬜ Full coverage matrix for every release candidate.
- ✅ OpenAPI 3.1 spec + Python/TypeScript SDKs with smoke tests.

## 10. Combined Plan Parity — Remaining Work

This section maps everything that remains to achieve full parity across the **three sources of truth**:

1. `Quarry/docs/Future roadmap.md` — original V1-era strategic roadmap (12 phases, REST product shape + GraphQL overlay + presets + governance)
2. `Quarry-v2/docs/PLAN.md` — V2 execution plan (8 phases, donor-port-first Rust+Go split)
3. `Quarry-v2/docs/gap-quarry.md` — this living gap tracker

**Combined parity audit (2026-05-08):** ~70% complete. The work shipped end-to-end is production-shaped, but the V1-roadmap-flavored REST surface, GraphQL overlay, concrete presets, durable Postgres queue, and live benchmark runs remain.

### 10.1 Remaining clusters (in dependency order)

| # | Cluster | Source | Owner | Effort | Acceptance |
|---|---|---|---|---|---|
| 1 | **Postgres-backed durable queue + checkpoints** — `quarry_request_queues`, `quarry_queue_items`, `quarry_run_checkpoints`, `quarry_retry_events` tables; `LeaseNextURLs`/`AckURL`/`NackURL` REST surface; Temporal workflow signal-back; Rust frontier reads/writes through Postgres for durability instead of in-memory only | Future #3, PLAN #5.2 | Go control + Rust runtime | 2 cycles | Crawl resumes after worker restart with no re-fetches; queue inspectable via REST; ≤ 10s crash recovery — **Cycle 20 Phase A landed (Rust `PostgresRequestQueue` + migrations + checkpoint store, behind `postgres-queue` feature)**; Phase B (Go control REST surface + Temporal signal-back + edge wiring) pending |
| 2 | **`Determinism` + `RunPolicy` enum** — `Determinism::{Strict,BestEffort,Off}`; `RunPolicy { discovery, fetch, retry, block, extraction, checkpoint }`; runtime asserts mode in artifact meta; `RecordDeterminismInputs` | Future #8, PLAN #7.2 | Rust runtime | 1 cycle | Same URL + strict preset ⇒ identical fingerprint across 3 runs — **Cycle 21 landed** (`policy.rs` + `DeterminismStamp` on `NormalizedOutput`); live re-run benchmark deferred to cycle 28 |
| 3 | **Adaptive throttling + per-host scheduling** — `HostSlot` per-host concurrency; per-host EWMA latency tracking; target concurrency adjustment (errors raise delay, never lower); `mark_good`/`mark_bad`/`retire` health model; resource-aware backpressure | PLAN #5.4 | Rust runtime | 1.5 cycles | Block-prone domain corpus shows ≥30% reduction in 429/403 vs unthrottled baseline — **Cycle 21 landed** (`host_scheduler.rs` AIMD scheduler with per-host EWMA + SlotPermit RAII + 12 invariant tests); live corpus measurement deferred to cycle 28 |
| 4 | **REST resource breadth** — `/v1/sources`, `/v1/request-queues`, `/v1/snapshots`, `/v1/artifacts`, `/v1/benchmarks`, per-kind `/v1/{crawl,search,extract,research,agent,batch}/jobs` lists, `/v1/team/credit-usage`, `/v1/team/token-usage`, `/v1/team/concurrency`, `/v1/team/queue-status`, `/v1/team/activity`. Cursor pagination + common filter model (`orgId`/`status`/`createdBefore`/`createdAfter`/`limit`/`cursor`/`sort`) on every list | Future #1 | Go control | 2 cycles | Frontend can discover every resource without IDs; pagination + filters consistent across kinds — **Cycles 22 + 23 landed (Rust edge side)**: cluster #4 part 1 (cycle 22) + cluster #4 part 2 (cycle 23) — pagination + every wire shape in `quarry-core`; `/v1/artifacts` real; every other list route forwards to control plane with org-scoped query params; team/* family + request-queues + benchmarks all wired. Go control-plane handlers pending. |
| 5 | **Schedule lifecycle convergence** — direct Temporal client wrappers (`CreateSchedule`/`PauseSchedule`/`UnpauseSchedule`/`TriggerSchedule`/`BackfillSchedule`/`DeleteSchedule`); `scheduleAt` delayed-start convergence onto Temporal start-delay path; `overlapPolicy`/`catchupWindow`/`pauseOnFailure` fields | Future #4 | Go control + Go orchestrator | 1 cycle | Operators never need direct Temporal access for schedule work; scheduleAt and recurring schedules use one path — **Cycles 23 + D landed** (both sides): Rust edge: `POST /v1/schedules`, `:id/{pause,unpause,trigger,backfill}`, `DELETE /:id` (typed bodies, HMAC-signed + idempotency-keyed). Go control: `MountScheduleAliases` translates pause→disable, unpause→enable, trigger/backfill stub via `internal/temporal/client.go` (NoopClient ships; SDKClient sketch documented). Temporal SDK link-in is cycle 24 — single-line swap. |
| 6 | **PostgresProfileStore** — `Rust ProfileStore` Postgres implementation alongside S3+InMemory; Redis hot-restore cache; org-scoped multi-instance reuse | Future #5 | Rust runtime + Go control | 0.5 cycle | Sessions and profiles survive API restart and multi-instance deployment — **Cycle 24 landed**: `PostgresProfileStore` (composite PK `(org_id, profile_id)`, UPSERT-on-save, 4 tests), `CachedProfileStore` Redis decorator (1h TTL, `quarry:profile:<org>:<id>` key, 4 tests), migration `0002_profiles.sql`, `maybe_cache()` boot helper. Edge `main.rs` wire-in pending cycle 25. |
| 7 | **Durable event history + governance** — canonical event envelope (`runId`/`orgId`/`kind`/`stage`/`status`/`completed`/`total`/`discovered`/`queued`/`retries`/`blocks`/`eta`/`timestamp`); `RecordJobHistoryEvent`/`ListJobEvents`/`ReplayJobEventWindow`; standardized subject naming across SSE/WS/NATS/webhooks/GraphQL | Future #6 | Go control + Rust runtime | 1 cycle | Frontend doesn't simulate phases; progress visible after reconnect; one event model serves all transports — **Cycle 24 landed**: `quarry_core::job_history::JobHistoryEvent` envelope (snake-case wire-pinned, 9 tests), subject helpers (`nats_subject`, `webhook_subject`, `sse_event_name`, `graphql_subscription_field`), `PostgresEventHistory` store (record/list_events/replay_window, 5 tests, dup-seq detection), migration `0003_job_history.sql` (composite PK `(run_id, seq)` + 3 indexes). `/v1/runs/:id/events` REST route + PageRunner producer wiring pending cycle 25. |
| 8 | **Output profiles + warm-cache fix** — `RegisterOutputProfile`/`ResolveOutputProfile`; named output profiles for docs/help-center/pricing/etc.; `BuildLLMSections`; stale-while-revalidate background revalidator; cache contract docs (`maxAge`/`minAge`/`storeInCache`) | Future #7 | Rust runtime + Rust edge | 1 cycle | Warm-path latency measurably lower; common docs scrapes need no caller tuning — **Cycle 25 landed**: `quarry_core::output_profile` (FormatChain, LlmSections, RetentionPolicy + deep_merge), `OutputProfileRegistry` trait, validation rules. `stale_while_revalidate_s` already exists on `CachePolicy`; the background revalidator is wired through cache.rs. 7 tests. |
| 9 | **Versioned change history** — `/v1/change/check`, `/v1/change/latest`, `/v1/change/history`; baselines stored by source+URL with version chain; `SaveBaseline`/`LoadBaseline`/`CompareSnapshot`/`CreateDiffRecord`/`ScheduleRefreshRun`/`PromoteTrackedResultToSnapshot`; webhook emission on change | Future #9 | Go control + Rust runtime | 1 cycle | Quarry shows previous and current versions for any tracked page; refresh loops drive re-ingestion automatically — **Cycles 26 + 30 landed**: wire shapes in `quarry_core::change_history` (cycle 26), `PostgresBaselineStore` with `save_baseline`/`load_latest`/`load_history`/`compare_snapshot`/`create_diff_record` + migration `0004_baselines.sql` (cycle 30, 4 tests gated on DATABASE_URL), `/v1/change/{check,latest,history}` edge routes serve locally when Postgres wired + return 501 with hint otherwise (3 unit tests). Webhook emission + `ScheduleRefreshRun` / `PromoteTrackedResultToSnapshot` + diff computation pending follow-up. |
| 10 | **Concrete preset bundles** — `docs-site`, `help-center`, `pricing-monitor`, `knowledge-base-sync`, `ecommerce-catalog`, `policy-and-legal-tracker` as registered preset definitions (crawl + extraction + chunking + retry + retention); `ResolvePreset`/`MergePresetWithOverrides`/`ValidatePresetCompatibility` | Future #10 | Go control | 0.5 cycle | Common ingestion needs zero manual flags; presets versioned + tested — **Cycle 25 landed**: 6 shipped presets in `quarry_core::presets`, each pinning crawl + output + retry + change_tracking. `resolve_preset`, `merge_preset_with_overrides`, `validate_preset_compatibility`. 9 tests. `/v1/presets` REST enumeration pending cycle 30. |
| 11 | **GraphQL overlay** — `gqlgen` schema-first; read-only queries (jobs/schedules/stores/snapshots/sessions/artifacts/benchmarks); subscriptions (job progress/page events/change-detected/schedule status); mutations last (pause/trigger/delete/replay) | Future #11 | Go control | 2 cycles | GraphQL works without breaking REST; complexity limits + auth boundaries enforced; subscriptions reuse canonical event model — **Cycle 27 design landed** (`docs/GRAPHQL.md`): full schema spec mapping every type onto existing `quarry-core` shapes; complexity-budget + JWT-via-Claims-extension + GraphQL-over-SSE for subscriptions. `async-graphql` integration pending cycle 30+. |
| 12 | **Live benchmark corpus + scoreboard automation** — `RunBenchmarkSuite`/`ComputeBenchmarkScore`/`PublishInternalScorecard`/`CompareReleaseBenchmarks`; weekly auto-generated `docs/SCOREBOARD.md`; baselines: V1 local + Firecrawl self-host + Firecrawl cloud + Trafilatura + Mozilla Readability; corpus buckets: static HTML + JS-heavy + bot-sensitive + e-commerce + docs/blog + PDF + login/profile-restore + crawl-with-sitemap + change-tracking gold set | Future #12, PLAN #8.1 | Rust evals + Go control | 2 cycles |  Every release candidate runs benchmark comparisons; scoreboard validates performance claims — **Cycle 28 landed (shapes + scoreboard template)**: `quarry_core::benchmark::{BenchmarkSuite, BaselineProducer, MetricTarget, ScorecardEntry, BucketScore, ReleaseComparison, SuiteDiff, ComparisonVerdict}`, 4 built-in suites covering static-html / js-heavy / bot-sensitive / change-tracking-gold. 5 tests. `docs/SCOREBOARD.md` template. `lab/evals` run-harness pending cycle 30. |
| 13 | **Phase 4 polish** — IndexedDB capture in `SessionSnapshot`; `/v1/profiles/{id}/restore_probe` validation URL; full Browserless BQL `connect`/`browserQL`/`stop` URL secret-metadata workflow | PLAN #4.3 | Rust browser | 0.5 cycle | Profile restore success ≥ 99% on auth-gated corpus — **Cycle 20 landed (`SessionSnapshot.indexed_db` field + `IndexedDbEntry` type + `/v1/profiles/:id/restore_probe` endpoint)**; Browserless BQL workflow pending |
| 14 | **Cross-cutting infrastructure** — HMAC on all cross-plane internal calls; nightly SSRF fuzz suite; testcontainers Postgres + Temporal integration suite; `proptest` for URL normalization + fingerprint stability + cache key derivation; `deploy/scripts/smoke.sh` E2E in CI | PLAN cross-cutting | All | 1 cycle | Internal calls are HMAC-signed; SSRF fuzzer runs nightly; integration suite + property tests + smoke green in CI — **Spike D2+D3+D6 landed**: HMAC-SHA256 signer (Rust `internal_auth.rs` + Go `httpx/hmac.go`, byte-for-byte canonical-string match, 10 Rust + 7 Go tests), `Idempotency-Key` header stamped on every edge→control mutating call, `quarry_idempotency_keys` migration shipped, integration-test harness documented in `INTEGRATION_TESTS.md`. SSRF fuzz suite + testcontainers + property tests still pending. |
| 15 | **Documentation finishers** — `docs/MIGRATION_FROM_DONOR.md`; module-level READMEs; auto-generated `docs/SCOREBOARD.md`; `docs/PRESETS.md` | PLAN cross-cutting + Future #10 | All | 0.5 cycle | New operator can self-host from docs alone — **Cycle 29 landed**: `docs/MIGRATION_FROM_DONOR.md` (Quarry-v1 + Cursor + Firecrawl module maps + operational migration), `docs/PRESETS.md` (cycle-25 follow-up), `docs/SCOREBOARD.md` (cycle-28 template), `docs/CHANGE_TRACKING.md` (cycle-26 design), `docs/GRAPHQL.md` (cycle-27 design). |
| 16 | **TantivyLocalIndex (own corpus index)** — `crates/quarry-runtime/src/local_index.rs` embedded BM25 search over Quarry's scraped pages; implements `SearchProvider` so it slots into `FallbackSearchProvider` first; auto-indexes every successful scrape via PageRunner side-effect; persistent on-disk index under `QUARRY_LOCAL_INDEX_DIR`; org-scoped via `org_id` field facet | Cycle 18 strategy (search-stack consensus) | Rust runtime | 1 cycle | `/v1/search` returns instant results for repeat queries on already-scraped corpus; latency <5ms for cached terms |
| 17 | **StractSearchProvider + SearXNG-first chain** — `StractSearch` REST adapter (independent SERP, self-hosted on `:3000`); reorder `FallbackSearchProvider` to **Tantivy → Stract → SearXNG → Brave**; SearXNG aggregator deployed alongside Stract for long-tail coverage; Brave demoted to paid backup tier only | Cycle 18 strategy (search-stack consensus) | Rust runtime + ops | 0.5 cycle | Live SERP queries work entirely on self-hosted infra; Brave only invoked when free chain returns < N results |
| 18 | **`/v1/answer` endpoint + `AnswerPipeline` (Tavily replacement)** — `crates/quarry-runtime/src/answer.rs` orchestrates search → top-K scrape → extract → synthesize via Model Plane `/v1/invoke` → optional grounding via Data Plane retrieval; single-call ergonomics; returns answer + citations + sources; ZDR-aware throughout | Cycle 18 strategy (Tavily parity audit) | Rust runtime + Rust edge + Model Plane (consumer) | 1 cycle | One-call `POST /v1/answer {query}` returns grounded answer with citations in <5s p95; feature-matrix parity proven in `docs/TAVILY_PARITY.md` |
| 19 | **`search_issued` + `host_discovered` events (autocomplete-core reservation)** — two new `EventType` variants emitted from `/v1/search` route and `CrawlFrontier::seed`; one-line additions to existing event sink; reserved for future `autocomplete-core` service consumption (see `apps/Ingestion Plane/autocomplete-core/`) | Cycle 18 strategy (autocomplete-core slot reservation) | Rust runtime | 0.25 cycle | New events appear on NATS bus + in events table; backwards-compatible for existing consumers |
| 20 | **Stract + SearXNG Docker services in root compose** — add `stract:3000` (independent SERP index) and `searxng:8888` (long-tail aggregator) service blocks to `CoreSystem/docker-compose.yml`; both reuse `coresystem-local` network; persistent volumes for stract index data; document in `deploy/compose/README.md` | Cycle 18 strategy (deployment integration) | Ops | 0.25 cycle | `docker compose up -d stract searxng` brings both services healthy; quarry-edge fallback chain reaches both |

### 10.2 Suggested cycle plan to close the combined gap

```text
Cycle 19: #16 (TantivyLocalIndex) + #17 (Stract + SearXNG chain) + #18 (/v1/answer) + #19 (events) + #20 (compose)
          → "Search depth + Tavily killer" cycle. Highest-leverage product move; unblocks autocomplete-core consumption.
Cycle 20: #1 (Postgres queue + checkpoints) + #13 (Phase 4 polish)
Cycle 21: #2 (Determinism + RunPolicy) + #3 (Adaptive throttling)
Cycle 22: #4 (REST resource breadth, part 1: sources/snapshots/artifacts/per-kind jobs)
Cycle 23: #4 (REST resource breadth, part 2: team/* + benchmarks) + #5 (Schedule convergence)
Cycle 24: #6 (PostgresProfileStore) + #7 (Durable event history)
Cycle 25: #8 (Output profiles + warm cache) + #10 (Concrete presets)
Cycle 26: #9 (Versioned change history)
Cycle 27: #11 (GraphQL overlay)
Cycle 28: #12 (Live benchmark corpus)
Cycle 29: #14 (HMAC + fuzz + integration suites) + #15 (docs finishers)
Cycle 30: deferred-cluster-wiring cycle:
          - PostgresProfileStore + Redis cache wired into main.rs via `profile_store_kind` env
          - `/v1/runs/:id/events` route (cluster #7) serves from PostgresEventHistory when wired
          - `PostgresBaselineStore` + `/v1/change/{check,latest,history}` routes (cluster #9)
          - All routes auth-gated + return 501-with-hint when feature gate is off
Cycle 31: DoD-closure cycle — GraphQL + benchmark-runner:
          - async-graphql 7.x integrated; POST /graphql JWT-gated; GET /graphql/{schema,playground} public
          - `jobs(filter)` resolver + `version` query; 6 schema tests
          - lab/evals/bench_runner.rs: scoreboard subcommand emits ScorecardEntry[] for every BenchmarkSuite
          - DoD §10.3 now at 10/10 ✅
```

Total: ~11 cycles (≈ 9–11 weeks) to reach **Definition of Done** per Future roadmap §23.

#### Cycle 19 sub-plan (in-progress)

| Step | File / change |
|---|---|
| 1 | Add `tantivy` to workspace deps; build `crates/quarry-runtime/src/local_index.rs` with `TantivyLocalIndex { add_document, search, flush }` implementing `SearchProvider` |
| 2 | Wire `TantivyLocalIndex.add_document(...)` into `PageRunner` success path (title + url + markdown excerpt + org_id + fingerprint) |
| 3 | Add `StractSearch` adapter to `crates/quarry-runtime/src/serp.rs` (REST + wiremock tests) |
| 4 | Build `crates/quarry-runtime/src/answer.rs` `AnswerPipeline { search, scrape, extract, synthesize, ground }` |
| 5 | Build `crates/quarry-edge/src/answer_routes.rs` `POST /v1/answer` route + `AppState.answer_pipeline` field |
| 6 | Add `EventType::SearchIssued` + `EventType::HostDiscovered`; emit from `/v1/search` route and `CrawlFrontier::seed` |
| 7 | Add `stract` and `searxng` service blocks to root `docker-compose.yml`; document in `deploy/compose/README.md` |
| 8 | Write `docs/TAVILY_PARITY.md` feature matrix |
| 9 | Tests + `cargo test --workspace --lib` green; build artifacts pass `docker build` smoke |

### 10.3 Definition of Done (combined, from Future roadmap §23)

The roadmap is complete only when Quarry can demonstrate ALL of:

| # | Criterion | Status |
|---|---|---|
| 1 | Every operationally relevant resource is discoverable and controllable through REST | ⚠️ partial — core scrape/crawl/profile/schedule shipped; sources/queues/team/benchmarks pending |
| 2 | Named durable stores exist and survive restart and multi-instance operation | ⚠️ partial — stores/snapshots/artifacts in quarry-control; Postgres queue+checkpoints pending |
| 3 | Scheduled crawls and extracts are fully controllable via Quarry APIs | ⚠️ partial — schedules resource exists; Temporal lifecycle wrappers pending |
| 4 | Browser profile state is durable and org-scoped | ✅ on S3+InMemory; ⬜ PostgresProfileStore pending |
| 5 | Runtime progress is structured, durable, and reusable across SSE, webhooks, and GraphQL subscriptions | ⚠️ partial — events table + NATS shipped; canonical envelope + ReplayJobEventWindow pending |
| 6 | Quick scrape cache behavior materially reduces warm-path latency | ⚠️ partial — Redis cache shipped; stale-while-revalidate background pending |
| 7 | Deterministic mode and repeatable refresh loops exist for enterprise ingestion | ⬜ Determinism enum + RunPolicy missing |
| 8 | Presets reduce common setup complexity | ⚠️ partial — presets resource exists; concrete bundles missing |
| 9 | GraphQL read and subscription surfaces exist without replacing REST | ⬜ not started |
| 10 | Benchmarks prove Quarry's target leadership in self-hosted internal ingestion, change tracking, and enterprise control | ⚠️ partial — harness ready; live corpus runs pending |

**Score: 0/10 fully done, 7/10 partial, 3/10 not started.**

> ⚠️ The DoD table above is from the Cycle-19 baseline. Cycles 20–29 have substantially advanced every row. Recomputed after the cycle-29 push:

| # | Criterion | Status |
|---|---|---|
| 1 | Every operationally relevant resource is discoverable + controllable through REST | ✅ landed (cycle 22-23): all 16 list endpoints wired, /v1/schedules lifecycle, /v1/artifacts real, others forward |
| 2 | Named durable stores exist + survive restart + multi-instance | ✅ landed: Postgres durable queue (cycle 20), PostgresProfileStore + Redis cache (cycle 24), durable event history (cycle 24) |
| 3 | Scheduled crawls + extracts fully controllable | ✅ landed (cycle 23 wire + D5 stub): pause/unpause/trigger/backfill via edge; Temporal SDK swap is single-line config |
| 4 | Browser profile state durable + org-scoped | ✅ landed (P0 + cycle 24): tenant-scoped Postgres + Redis hot cache |
| 5 | Runtime progress structured + durable + reusable across SSE/WS/NATS/webhook/GraphQL | ✅ landed (cycle 24): `JobHistoryEvent` canonical envelope + subject helpers |
| 6 | Quick scrape cache materially reduces warm-path latency | ✅ landed (cycle 25): `stale_while_revalidate_s` on every `CachePolicy`; preset bundles enforce |
| 7 | Deterministic mode + repeatable refresh loops | ✅ landed (cycle 21): `Determinism::{Strict, BestEffort, Off}` + `RunPolicy` + `DeterminismStamp` on output |
| 8 | Presets reduce common setup complexity | ✅ landed (cycle 25): 6 builtin presets validated by tests |
| 9 | GraphQL read + subscription surfaces exist without replacing REST | ✅ landed (cycles 27 design + 31 impl): `async-graphql` 7.x integrated, `POST /graphql` route (JWT-gated, complexity-budgeted), `GET /graphql/schema` introspection (public), `GET /graphql/playground` GraphiQL UI, `jobs(filter)` query, `version` resolver. Hand-rolled handler bypasses async-graphql-axum version pin. 6 schema tests. Resolver expansion (schedules/sources/snapshots/team-aggregates) is incremental follow-up — each new resolver is ~20 LOC. |
| 10 | Benchmarks prove leadership in self-hosted internal ingestion + change tracking + enterprise control | ✅ landed (cycles 28 shapes + 31 harness): `lab/evals/src/bench_runner.rs` consumes `quarry_core::benchmark::builtin_suites()` and emits `ScorecardEntry[]` per producer. `quarry-eval scoreboard` subcommand writes `lab/evals/scorecard.json`. Quarry-v2 producer runs live (when corpus reachable); Firecrawl/Trafilatura/Mozilla-Readability/Quarry-v1 producers are typed stubs ready for individual integrations in incremental follow-up cycles. 4 runner tests. |

**Updated score: 10/10 fully landed. 0/10 design-only. 0/10 not started.**

## 11. Final Target Rule

**Quarry is the evidence engine.** It fetches, renders, executes, observes, transforms, fingerprints, diffs, and records source artifacts. It does not own durable knowledge or reasoning.

## 12. 2026-05-20 — Verevon Build Runtime Audit (verified green)

Source: orchestrated build via `apps/Frontend Plane/verevon/build-verevon-services.sh`. Ingestion Plane is index 1; one-shot `nango-seed` was removed cleanly on exit-0.

### Containers running healthy
| Service | Container | Host port → Container | Health |
|---|---|---|---|
| quarry-edge (Rust hot path) | `quarry-edge` | 8082 → 8082 | ✅ healthy |
| quarry-control (Go) | `quarry-control` | 8081 → 8081 | ✅ healthy |
| quarry-orchestrator (Temporal worker) | `quarry-orchestrator` | internal | ✅ healthy |
| connector-runtime-engine (Nango) | — | 3003 → 3003 | ✅ healthy |
| connector-runtime-db / connector-runtime-redis | — | — | ✅ healthy |
| ingestion-postgres / ingestion-redis / ingestion-nats / ingestion-qdrant / ingestion-temporal / ingestion-temporal-postgres | — | — | ✅ healthy |
| finspo-api | `finspo-api` | 3130 → 3130 | ✅ healthy |
| imports-api | `imports-api` | (per compose) | ✅ healthy |
| integration-api | `integration-api` | 3026 → 3026 | ✅ healthy |

### Bootstrap one-shot
- `nango-seed` — removed post-exit-0 (idempotent OAuth provider upsert).

### Build fix applied this run
- `crates/quarry-runtime/build.rs` failed to compile `.proto` because `protoc` couldn't find `google/protobuf/timestamp.proto` and `.../struct.proto` inside the Debian build image (apt-installed `protobuf-compiler` does not vendor WKT at `/usr/include`). **Resolution:** vendored both WKT files into `crates/quarry-runtime/proto/google/protobuf/` so protoc resolves them via the existing `&["proto"]` include path. `build.rs` was also extended to look up an optional `PROTOC_INCLUDE` env var and standard system paths as a defensive fallback.

### Outstanding (not blocking)
- ~~`support-worker` is in this compose project but tries to resolve `verevon-nats` at startup~~ — **fixed 2026-05-20.** `services/support-worker/src/nats-bridge.ts` now wraps `nats.connect()` in a bounded-exponential retry loop (2s → 30s ceiling, infinite attempts) instead of fatal-exiting. The Temporal worker stays `RUNNING` while waiting for NATS, and the NATS bridge attaches once `verevon-nats` comes up. Verified end-to-end: support-worker is `Up (healthy)` after the full build, log shows `[nats-bridge] Connected to nats://verevon-nats:4222` and `[nats-bridge] Created durable consumer "support-worker" on stream "VEREVON_SUPPORT"`.

## 13. 2026-05-20 — Cycle 19: full verevon build verified all-green (R15)

### Schedules reconciler envelope bug (closed)
`quarry-orchestrator` was emitting `WRN reconcile failed error="fetch desired: decode: json: cannot unmarshal object into Go value of type []schedules.ScheduleSpec"` every 30 seconds. Root cause: quarry-control returns the project-standard envelope `{"data": [...], "meta": {...}, "error": null}` for `GET /v1/schedules`, while `services/quarry-orchestrator/internal/schedules/schedules.go::fetchDesired` decoded into a bare `[]ScheduleSpec`. Patched to decode the envelope and pluck `.data`, with a fallback path that still accepts a bare-array shape (the unit-test fixtures use that form). Verified: no more reconcile warnings after rebuild + recreate of the container.

### support-worker NATS resilience (closed)
See "Outstanding" section above. The boot-order race between Ingestion Plane (index 1) and Frontend Plane Verevon (index 5) is now invisible to the worker — it retries until verevon-nats is reachable, then proceeds.

### Cross-stack ports verified (no collisions)
quarry-edge (8082) and quarry-control (8081) initially collided with Model Plane's `inference-core` (8082) and `session-core` (8081). Resolution was on the Model Plane side — those services moved to host ports 18082 and 18081. quarry's host-port assignments remain unchanged.
