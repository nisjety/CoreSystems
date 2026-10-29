# Quarry v2 — Gap Analysis

> **Generated:** 2026-05-03  
> **Updated:** 2026-05-07  
> **Scope:** Three-way comparison of Quarry v2, Quarry v1 (Go), and Firecrawl OSS.  
> Sources: live codebase review of all three repos.

---

## 1. Executive Summary

| System | Strength | Primary Weakness |
|--------|----------|------------------|
| **Quarry v2** | TLS impersonation, durable Postgres contracts, change tracking, typed IDs, self-owned security | Remaining gaps are parity-depth work: reduced `/v1/search`/`/v1/extract`/`/v1/agent`, SDKs/OpenAPI, audio, and evidence-heavy browser hardening |
| **Quarry v1** | Full product surface (agent, search, map, extract, crawl, batch, change), Temporal scheduling, measured performance baselines | Hot path in Go (detectable fingerprint), memory-only sessions, weak type safety, ~8 600 ms cold scrape |
| **Firecrawl** | Broadest format surface, Fire Engine advanced drivers (managed), SDK breadth (6 languages), LLM extract, deep research, Wikipedia integration | Hosted/proprietary advanced drivers, no cron scheduling, no persistent browser profiles, no event history, Redis state (lost on restart) |

Gap closure now follows the phase system in Section 14. The `Owner Phase` column is execution ownership: a gap is not closed until its phase exit gate and evidence requirements are met.

---

## 2. Scraping Engine Gaps

### 2.1 V2 vs V1

| Capability | V1 Status | V2 Status | Gap |
|------------|-----------|-----------|-----|
| Static HTTP fetch | ✅ Go net/http | ✅ Rust + wreq | V2 ahead (TLS impersonation) |
| TLS/JA3/JA4/H2 impersonation | ❌ detectable | ✅ BoringSSL via `wreq` | **V1 gap** |
| Browser (headless) | ✅ Rod (CDP) | ✅ chromiumoxide CDP | Equivalent |
| Browserbase cloud driver | ❌ | ✅ Feature-gated Browserbase WSS driver (`browserbase`) | V2 ahead (optional cloud browser backend) |
| Browser stealth mode | ✅ (JS injection evasions) | 🟡 Baseline CDP stealth init script + emulation overrides are wired in chromiumoxide and Browserbase; fingerprint benchmark still pending | **V2 partial gap** |
| PDF rendering / fetch | ✅ Rod print-to-PDF | ✅ CDP `printToPdf` | Equivalent |
| PDF parsing (ingest PDF as source) | ❌ | ✅ Self-hosted `pdf-extract` text extraction in transform pipeline | V2 ahead for text PDFs; OCR/layout parsing still optional |
| Document/binary fetch | ✅ | ✅ Text/PDF/binary classification with raw/markdown output routing | Equivalent |
| Engine waterfall (fallback tiers) | ✅ (manual) | ✅ `DriverPlan` typed waterfall | V2 better structured |
| Challenge / CAPTCHA detection | ✅ heuristics in driver | ✅ `urlsig::analyze` + `SecurityEngine` | V2 more principled |
| Mobile emulation | ✅ Rod + DevMetrics | 🟡 Static/mobile UA, Browserless viewport, and chromiumoxide + Browserbase device metrics/touch are wired; full fingerprint benchmarking pending | **V2 partial gap** |
| Geo / locale targeting | ✅ | 🟡 Locale is threaded edge → runtime → static/browser paths; chromiumoxide country proxy routing is wired, but provider-wide geo enforcement and benchmark evidence are still uneven | **V2 partial gap** |
| Ad blocking | ✅ Rod HijackRequests | ✅ CDP Fetch domain intercept: ad/tracker URLs matched against block list and blocked at network level | Equivalent |
| `waitFor` (JS render delay) | ✅ | ✅ | Equivalent |
| Custom request headers | ✅ | ✅ | Equivalent |
| `onlyMainContent` / tag filters | ✅ | ✅ (readability) | V2 ahead (scored readability) |

### 2.2 V2 vs Firecrawl

| Capability | Firecrawl | V2 | Gap |
|------------|-----------|-----|-----|
| Static TLS impersonation | Fire Engine (hosted) | ✅ self-owned BoringSSL | **V2 structural advantage** |
| Browser CDP (headless) | Fire Engine + Playwright | ✅ chromiumoxide | Equivalent |
| Cloud browser session backend | ✅ Managed Fire Engine sessions | ✅ Browserbase WSS driver (feature-gated) | Equivalent (different ops model) |
| Stealth / anti-detect | Fire Engine stealth tier | 🟡 Baseline CDP stealth script + device/locale overrides are shipped across local/cloud browser paths | **V2 partial gap in hardening/measurement** |
| `fetch` fallback | ✅ | ✅ | Equivalent |
| PDF ingest (LlamaParse) | ✅ | 🟡 Self-hosted text-PDF extraction; no hosted OCR/layout parser | **V2 partial gap for OCR/layout fidelity** |
| Wikipedia Enterprise | ✅ | ✅ Configurable Wikipedia source adapter with enterprise base URL/token support | Equivalent |
| `mobile` emulation | ✅ | 🟡 Static UA + Browserless viewport + chromiumoxide/Browserbase metrics/touch wired; fingerprint benchmark pending | **V2 partial gap** |
| `blockAds` | ✅ | ✅ CDP Fetch domain intercept implemented; ad/tracker patterns blocked at network level | Equivalent |
| Geo / `location.country` | ✅ | 🟡 Locale threading is implemented and chromiumoxide country proxy routing exists, but provider-wide geo proof is still pending | **V2 partial gap** |
| `storeInCache` per-request | ✅ | ✅ (cache layer in edge) | Equivalent |
| `zeroDataRetention` | ✅ | ✅ Scrape fast path bypasses cache, artifacts, and durable event publishing while preserving live SSE fanout | Equivalent for `/v1/scrape`; future async/indexing features must preserve it |
| `maxAge` cache control | ✅ | ✅ | Equivalent |

### 2.3 Progress Update — 2026-05-03

- Edge `/v1/scrape` now accepts request-scoped `mobile`, `locale`, `location.country`, `blockAds`, and custom `headers`.
- `FetchContext` is threaded edge → runtime → driver boundary; static fetch now applies mobile user-agent, `Accept-Language`, and validated custom headers.
- Browser path now has a typed request-configuration hook; Browserless receives mobile viewport, locale, extra headers, and `blockAds` intent in its request body.
- **2026-05-03 (update):** geo-backed proxy/session routing (`--proxy-server`) and ad/tracker blocking (CDP Fetch domain intercept) are now implemented. Remaining proof gaps: fingerprint benchmarking and provider-wide geo/device parity.

### 2.4 Progress Update — 2026-05-04

- `quarry-browser` now includes a feature-gated Browserbase cloud driver (`browserbase`) with WSS session connection via `BROWSERBASE_API_KEY` + `BROWSERBASE_SESSION_ID`.

### 2.5 Progress Update — 2026-05-06

- Quarry edge now ships public handlers for `POST /v1/search`, `POST /v1/extract`, `POST /v1/agent`, `POST /v1/change/check`, and `GET /v1/change/latest`, plus a compatibility shim for `POST /v1/security/blocklist` and proxied run-event access at `GET/POST /v1/runs/:id/events`.
- Browserbase request configuration is now wired for mobile device metrics, locale/headers, touch emulation, ad blocking, PDF output, and fail-fast empty environment validation.
- Remaining browser parity work is now evidence-oriented: provider-wide geo/fingerprint benchmarking and acceptance-matrix proof rather than missing request-path wiring.
- Browserbase support is wired through module exports and implements the full `BrowserDriver` contract.
- Browser action runtime now supports action-level retry (`on_error=retry`) with a single retry attempt per failed action.
- `zeroDataRetention` request flag is now wired through edge → runtime, with cache/artifact persistence and durable event publishing bypass on this path.
- Action script schema now accepts `screenshotAfterEach` and `write` (alias of `type`), and runtime captures a screenshot after each successful non-screenshot action.
- Runtime events now use an ephemeral-only path for ZDR requests, so live subscribers still receive progress while durable publisher channels stay empty.
- Static/TLS drivers now fail fast on browser actions, while the browser adapter executes public action scripts before content extraction.
- PDF, text, and binary source routing is implemented in the pipeline; PDF text extraction uses a self-hosted parser and text documents preserve raw content.
- Wikipedia pages can be fetched through a configurable source adapter, including enterprise-style base URL and bearer token configuration.
- Chromiumoxide now applies a baseline stealth init script plus request-scoped mobile metrics, touch emulation, locale override, timezone hint, headers, ad blocking, and country proxy selection.

---

## 3. Browser Actions Gaps

### 3.1 V2 vs V1

| Action | V1 | V2 | Gap |
|--------|----|----|-----|
| `click` | ✅ | ✅ | Equivalent |
| `wait` | ✅ | ✅ | Equivalent |
| `screenshot` | ✅ | ✅ | Equivalent |
| `scroll` | ✅ Rod | ✅ Public action schema + browser driver execution | Equivalent |
| `write` / `type` | ✅ | ✅ `write` alias maps to `type`; public action schema validated | Equivalent |
| `press` | ✅ | ✅ Public action schema + browser driver execution | Equivalent |
| `executeJavascript` | ✅ (returns `Output`) | ✅ Public action schema + `action.started/completed` events | Equivalent |
| `generatePDF` via action | ✅ | ✅ CDP `printToPdf` | Equivalent |
| Screenshot after each action | ✅ | ✅ `screenshotAfterEach` accepted by public schema and enforced by `ActionRuntime` | Equivalent |
| Retry on action failure | ✅ | ✅ Action-level retry via `ActionScript.on_error=retry` in `ActionRuntime` (single retry) | Equivalent |

### 3.2 V2 vs Firecrawl

| Action | Firecrawl | V2 | Gap |
|--------|-----------|----|-----|
| `click` | ✅ | ✅ | Equivalent |
| `wait` | ✅ | ✅ | Equivalent |
| `screenshot` | ✅ | ✅ | Equivalent |
| `scroll` | ✅ | ✅ | Equivalent |
| `write` | ✅ | ✅ (`write` alias for `type`) | Equivalent |
| `press` | ✅ | ✅ | Equivalent |
| `executeJavascript` | ✅ | ✅ | Equivalent |
| `generatePDF` via action | ✅ | ✅ | Equivalent |

---

## 4. Output Format Gaps

### 4.1 V2 vs V1

| Format | V1 | V2 | Gap |
|--------|----|----|-----|
| `markdown` | ✅ | ✅ (scored readability) | V2 better quality |
| `html` / `rawHtml` | ✅ | ✅ | Equivalent |
| `json` (structured) | ✅ AI-backed | ✅ Model Plane bridge validates JSON into `extract` artifact | Closed for sync bridge; schema passthrough tracked in `QUARRY_V2_MODEL_PLANE_PARITY.md` |
| `links` | ✅ | ✅ | Equivalent |
| `images` | ✅ | ✅ (lazy-load resolved) | V2 better |
| `attributes` / Dublin Core | ✅ (partial) | ✅ Open Graph + Dublin Core | V2 better |
| `screenshot` | ✅ | ✅ | Equivalent |
| `pdf` | ✅ | ✅ | Equivalent |
| `chunks` | ❌ | ✅ | **V1 gap** |
| `change` (diff) | ✅ Redis hash (basic) | ✅ SemanticDiff + Blake3 fingerprint | V2 ahead |
| `meta` sidecar | ❌ | ✅ | **V1 gap** |
| `summary` (AI) | ✅ via ai-core | ✅ Model Plane bridge artifact | Closed for sync bridge |
| `branding` | ❌ | ✅ Static HTML/meta/CSS branding artifact (`site_name`, `logo_url`, `theme_colors`, `font_families`) | **V2 ahead vs V1; rendered branding remains optional parity work** |
| `audio` | ❌ | ❌ | **Shared gap vs Firecrawl** |
| `query` (LLM Q&A) | ✅ via ai-core | ✅ Model Plane + Data Plane retrieval bridge artifact | Closed for sync bridge; agentic browsing parity remains separate |

### 4.2 V2 vs Firecrawl

| Format | Firecrawl | V2 | Gap |
|--------|-----------|----|-----|
| `markdown` | ✅ | ✅ | Equivalent |
| `html` | ✅ | ✅ | Equivalent |
| `rawHtml` | ✅ | ✅ | Equivalent |
| `links` | ✅ | ✅ | Equivalent |
| `images` | ✅ | ✅ | Equivalent |
| `attributes` | ✅ | ✅ Dublin Core + OG | V2 richer metadata model |
| `screenshot` | ✅ | ✅ | Equivalent |
| `pdf` | ✅ | ✅ | Equivalent |
| `chunks` | ❌ | ✅ | **Firecrawl gap** |
| `change` (diff) | ✅ flag only | ✅ full SemanticDiff + fingerprint | **V2 ahead** |
| `summary` (LLM) | ✅ | ✅ Model Plane bridge artifact | Closed for sync bridge |
| `query` (LLM Q&A) | ✅ | ✅ Model Plane + Data Plane retrieval bridge artifact | Closed for sync bridge |
| `json` (LLM extract) | ✅ | ✅ Model Plane bridge validates JSON into `extract` artifact | Closed for sync bridge; structured schema passthrough remains Model Plane parity work |
| `branding` (fonts/colors) | ✅ | ⚠ Static extractor shipped; browser-rendered / visual-heavy branding parity still open | **V2 partial gap** |
| `audio` | ✅ (gated) | ❌ | **V2 gap** (non-critical) | but wanted

---

## 5. Crawl Gaps

### 5.1 V2 vs V1

| Feature | V1 | V2 | Gap |
|---------|----|----|-----|
| Async crawl with job ID | ✅ | ✅ | Equivalent |
| Depth control | ✅ | ✅ BFS in Go orchestrator | Equivalent |
| Page limit | ✅ | ✅ | Equivalent |
| Include / exclude paths | ✅ (regex + glob) | ✅ Go orchestrator filters sitemap + discovered URLs with path glob, path regex (`re:`), and full-URL glob/regex (`re_url:`) rules | Closed |
| Sitemap-guided crawl | ✅ | ✅ Go orchestrator seeds crawl frontier via runtime-backed sitemap activity; targeted workflow coverage added | Closed |
| Robots.txt respect | ✅ | ✅ Go orchestrator enforces runtime-backed robots checks and emits `page.blocked` on denial | Closed |
| Allow external links | ✅ | ✅ Go orchestrator blocks cross-host links by default and admits them when `allow_external_links=true` | Closed |
| Allow subdomains | ✅ | ✅ Go orchestrator admits sibling subdomains when `allow_subdomains=true` | Closed |
| Backward / whole-domain path scope | ✅ | ✅ `crawl_entire_domain` is canonical and `allow_backward_crawling` is accepted as a compatibility alias | Closed |
| Cancel crawl | ✅ | ✅ Temporal cancellation | Equivalent |
| Crawl error report | ✅ | ✅ event log | Equivalent |
| Webhook on completion | ✅ Temporal | ✅ HMAC webhook dispatcher | Equivalent |
| Scheduled crawl (cron) | ✅ Temporal | ✅ Temporal-style schedule | Equivalent |
| SSE streaming updates | ✅ | ✅ | Equivalent |
| Crawl BFS dedupe | ✅ | ✅ (50-page checkpoints) | V2 more durable |
| Natural language prompt crawl | ⚠ preview | ✅ `prompt` field on `POST /v1/crawl`; forwarded to orchestrator payload for pre-BFS URL ranking | Closed (edge contract) |

### 5.2 V2 vs Firecrawl

| Feature | Firecrawl | V2 | Gap |
|---------|-----------|----|-----|
| Async crawl + job ID | ✅ | ✅ | Equivalent |
| Cancel crawl | ✅ | ✅ | Equivalent |
| Crawl errors endpoint | ✅ | ✅ event log | Equivalent |
| Webhook | ✅ | ✅ | Equivalent |
| Scheduled / cron crawl | ❌ | ✅ | **Firecrawl gap** |
| Crawl durability (restart-safe) | ❌ Redis-only | ✅ Postgres + checkpoints | **V2 structural advantage** |
| SSE streaming | ❌ | ✅ | **Firecrawl gap** |
| Sitemap-guided | ✅ | ✅ Runtime sitemap fetch/parse is wired into crawl BFS through internal edge/orchestrator activities | Closed |
| Robots.txt | ✅ | ✅ Runtime robots cache/parser is wired into crawl BFS through internal edge/orchestrator activities | Closed |
| Natural language prompt | ✅ | ✅ Model Plane NL ranking when configured (`x-quarry-search-mode: model-ranked`); term-match fallback otherwise | Closed |

---

## 6. API Endpoint Surface Gaps

| Endpoint | V1 | V2 | Firecrawl |
|----------|----|----|-----------|
| `POST /v1/scrape` | ✅ | ✅ | ✅ |
| `POST /v1/crawl` | ✅ | ✅ (handoff to Go orchestrator) | ✅ |
| `GET /v1/crawl/:id` | ✅ | ✅ | ✅ |
| `DELETE /v1/crawl/:id` (cancel) | ✅ | ✅ | ✅ |
| `GET /v1/crawl/:id/errors` | ✅ | ✅ (event log) | ✅ |
| `POST /v1/batch` | ✅ | ✅ (handoff) | ✅ |
| `POST /v1/map` (URL discovery) | ✅ | ✅ | ✅ |
| `POST /v1/search` | ✅ | ✅ Model Plane NL ranking when configured; deterministic term-match fallback; `X-Quarry-Search-Mode` header signals mode | ✅ |
| `POST /v1/extract` (LLM schema) | ✅ | ✅ (single-URL sync wrapper; deeper schema/source-trace parity still open) | ✅ |
| `POST /v1/agent` | ✅ | ⚠ synchronous one-step fetch + answer wrapper | ✅ |
| `POST /v1/change/check` | ✅ | ✅ | ⚠ flag only |
| `GET /v1/change/latest` | ✅ | ✅ | ❌ |
| `POST /v1/schedules` | ✅ Temporal | ✅ schedule resource | ❌ |
| `GET /v1/jobs/:id/history` | ❌ | ✅ | ❌ |
| `POST /v1/presets` | ❌ | ❌ (explicit `501`; preset catalog remains GET-only) | ❌ |
| `POST /v1/security/blocklist` | ✅ | ✅ add/list/remove shim over canonical `/v1/blocklists`; remove via `ids` or pattern lookup + `DELETE /v1/blocklists/:id` | ❌ |
| `POST /v1/internal/run_page` | ❌ | ✅ (internal Rust API) | ❌ |
| `GET /v1/runs/:id/events` (SSE) | ✅ | ✅ content-negotiated: SSE live stream (`Accept: text/event-stream`) or JSON log proxy (default) | ❌ |
| `GET /v1/profiles` (browser sessions) | ✅ (session manager) | ✅ (lease model) | ❌ |
| `POST /v1/deep-research` | ❌ | ❌ (explicit `501`; Post-V2) | ✅ |

---

## 7. Durability & Operations Gaps

| Dimension | V1 | V2 | Firecrawl |
|-----------|----|----|-----------|
| State storage | Redis + Postgres (partial) | Postgres (append-only events) | Redis (volatile) |
| Job history durability | Temporal history | Full Postgres event log | None |
| Restart recovery | Temporal workflow resume | Postgres + checkpoint BFS | Jobs lost on restart |
| Webhook reliability | Temporal side-effects | HMAC + exp backoff + DLQ | Basic, no DLQ |
| Cursor pagination | ❌ | ✅ opaque base64 cursor | ❌ |
| Event idempotency | ✅ Temporal | ✅ idempotency_key | ❌ |
| Blocklist persistence | ✅ DB | ✅ Postgres + Bloom filter hint | ❌ |
| Presets catalog | ❌ | ✅ fast/polite/stealth/deterministic | ❌ |
| SSRF guard | ✅ URL heuristics | ✅ DnsGuard + URL signature | Playwright route intercept only |
| Secrets in source | ❌ (env only) | ❌ (env only) | ❌ (env only) |

---

## 8. Observability & Developer Experience Gaps

| Dimension | V1 | V2 | Firecrawl |
|-----------|----|----|-----------|
| Structured logging | ✅ (zap/slog) | ✅ (tracing crate, structured) | ✅ |
| OpenTelemetry traces | 🟡 partial | 🟡 partial | ✅ |
| Metrics endpoint | ✅ `/metrics` | ✅ `/health` + `/ready` | ✅ |
| Slow-request warnings | ✅ (middleware) | 🟡 (planned, not shipped) | ✅ |
| SDK | ❌ | ❌ | ✅ 6 languages |
| API docs / OpenAPI | ✅ | ❌ | ✅ |
| Self-host guide | ✅ | 🟡 (deploy/ scripts exist) | ✅ |
| Test coverage enforcement | ❌ (no CI gate) | ✅ Coverage gate in CI (`cargo llvm-cov` + Go coverage, 80% floor) | 🟡 |
| E2E test harness | ✅ (`tests/`) | 🟡 (eval harness Phase 8) | ✅ (`__tests__/snips/`) |

---

## 9. Performance Baseline

| Scenario | V1 (measured) | V2 (target) | Firecrawl (estimate) |
|----------|---------------|-------------|----------------------|
| Static scrape cold | ~8 600 ms | ≤ 120 ms p50 warm | ~1 000–3 000 ms |
| Static scrape warm (cache hit) | ~8 600 ms (bug: cache not hit) | ≤ 50 ms | ~50–200 ms |
| JS page cold | N/A (Rod, blocking) | ≤ 1 500 ms p50 | ~3 000–8 000 ms |
| Map / URL discovery | ~96 ms | ✅ basic endpoint shipped | ~200–500 ms |
| Change check (dry run) | ~116 ms | ≤ 50 ms target | N/A |
| Health endpoint | ~4 ms | ≤ 5 ms | ~10–30 ms |
| Async dispatch | ~30 ms | ≤ 20 ms | ~50–100 ms |

> V1 cache bug: Redis `maxAge` was not reducing scrape latency for the `quick` module. This is a known V1 defect, not V2.

---

## 10. Critical Gap Priority Matrix

Ranked by business impact × implementation effort.

| # | Gap | Impact | Effort | Owner Phase |
|---|-----|--------|--------|-------------|
| 1 | Eval harness + warm/cold benchmarks | P0 — cannot retire V1 without proof | Medium | Phase 8 |
| 2 | Donor scrape removal (V1 hot path) | Closed — donor path removed; AppState carries no donor URL; scrape handler routes 100% through PageRunner::run(); cargo check clean | Done | Done 2026-05-05 |
| 3 | Coverage CI gate (≥ 80 %) | Closed — CI workflow enforces Rust + Go coverage floors at 80% and fails on regression; local `make coverage` command documented | Done | Done 2026-05-06 |
| 4 | Sitemap + robots.txt in crawl BFS | Closed — `CrawlJobWF` now seeds from sitemap, checks robots before `RunPage`, and emits `page.blocked` on robots denial; targeted workflow tests pass | Done | Done 2026-05-05 |
| 5 | Browser action schema in edge API | Closed — public schema, runtime execution, retry, screenshot-after-each validated | Done | Done 2026-05-04 |
| 6 | Mobile emulation + geo targeting | P1 — parity with V1 + Firecrawl; Chromium/static/browserless baseline shipped, provider-wide fingerprint proof still pending | Low | Phase 9 |
| 7 | `/v1/map` endpoint | Closed — endpoint implemented in `quarry-edge` | Done | Done 2026-05-04 |
| 8 | LLM `summary`/`query`/`json` formats | Closed — delegates generation to Model Plane and retrieval context to Data Plane | Done | Done 2026-05-04 |
| 9 | SDK (at minimum Python + TypeScript) | P2 — DX, adoption | High | Phase 9 |
| 10 | OpenAPI spec | P2 — developer onboarding | Low | Phase 9 |
| 11 | PDF ingest (LlamaParse or equivalent) | Reduced — self-hosted text-PDF extraction shipped; OCR/layout parser remains optional parity work | Medium | Phase 9 |
| 12 | `branding` format (rendered / visual parity) | Reduced — static extractor shipped; rendered/browser-assisted enrichment remains optional | Medium | Phase 9 |
| 13 | Browser-device emulation parity (metrics/profile fidelity) | P3 — metrics/touch/locale shipped, full fingerprint benchmarking still partial | Medium | Phase 9 |
| 14 | `zeroDataRetention` option | Closed for `/v1/scrape` — cache/artifact/durable-event bypass validated | Done | Done 2026-05-04 |

---

## 11. Deep-Read Source Analysis (Firecrawl Implementation Details)

> Added after reading key Firecrawl source files directly. These findings refine or add to earlier gap entries.

### 11.1 Scrape Waterfall — `scrapeURL/index.ts`

- `buildFallbackList()` constructs an ordered `EngineOrder[]` from the request options; each entry carries its own timeout and action list.
- Every failed attempt is stored in `statusCodes[]` and `errors[]`; the final error payload contains the full attempt history.
- More than 20 distinct error subtypes (e.g. `EngineError`, `RobotsBlockedError`, `TimeoutError`, `PdfError`, `AbortError`).
- `AbortManager` singleton: callers can cancel an in-flight scrape via `abort(jobId)`; quarry-v2 uses Tokio `CancellationToken` which is equivalent.
- Robots.txt fetched per-origin with a 10 s timeout; result cached in Redis. Quarry-v2 now mirrors that baseline behavior in `crates/quarry-runtime/src/robots.rs`, wired into the live orchestrator crawl path through `quarry-edge` internal endpoints.
- OpenTelemetry spans wrap each engine attempt; quarry-v2 has `tracing` instrumentation but no OTEL exporter yet.

### 11.2 Fire Engine Adapter — `engines/fire-engine/index.ts`

- `performFireEngineScrape()` calls the hosted Fire Engine endpoint with HMAC-signed requests.
- **A/B test flag** on each scrape: any request can be tagged to a named experiment; result metadata includes which variant ran.
  - **Quarry-v2 gap**: no A/B testing infrastructure at any layer.
- **Branding script injection**: Fire Engine can inject a `@mendable/firecrawl-rs` WASM module that extracts `brandColors`, `brandFonts`, and `brandLogo` from the rendered page.
  - **Quarry-v2 update**: Quarry-v2 now ships a static Rust extractor for `branding` (Open Graph/meta logo candidates, colors, and font families from persisted HTML/CSS). Rendered/WASM-style branding enrichment is still optional parity work.
- Engine response carries `screenshot` (base64) and `fullPageScreenshot` separately; Quarry-v2 returns a single screenshot blob per scrape.

### 11.3 Playwright Microservice — `playwright-service-ts/api.ts`

- **Architecture**: completely separate Node.js/Express process on port 3003 — not embedded in the scrape API.
  - Quarry-v2 embeds `chromiumoxide` in the Rust process; this is intentional for latency but means browser isolation is handled in-process.
- `MAX_CONCURRENT_PAGES` env var (default 10): hard cap on open pages per process.
- `BLOCK_MEDIA` flag: ignores `Image`, `Media`, `Font` resource types by default.
- **SSRF / DNS-rebinding protection**: `assertSafeTargetUrl()` resolves all hostnames via DNS, caches results (30 s TTL), and rejects responses that resolve to private RFC-1918 / link-local ranges.
  - Quarry-v2 now enforces equivalent `DnsGuard` preflight on both the `wreq` fetch path and the chromiumoxide browser path; this gap is closed and covered by browser SSRF tests.
- User-agent rotation: `user-agents` npm package selects a random desktop UA per request.
  - Quarry-v2 rotates among a fixed static list; a weighted distribution library would be a small improvement.
- `ALLOW_LOCAL_WEBHOOKS` flag: opt-in for dev environments to allow `localhost` webhook targets.
- `InsecureConnectionError` propagated back as `407` to callers.

### 11.4 Browser Agent — `lib/scrape-interact/browser-agent.ts`

- **Vercel AI SDK** (`generateText`, tool calls) — not a custom LLM loop.
- `MAX_STEPS = 25` hard ceiling on agent turns.
- Supported agent-browser CLI actions: `snapshot`, `click`, `fill`, `type`, `select`, `check`, `press`, `hover`, `scroll`, `get`, `wait`, `find`, `frame`, `eval`, `back`.
- `execInBrowser()` sends each action as a `POST /execute` to the browser service (the playwright microservice above); the agent never directly controls the browser.
- Debug mode (`AGENT_DEBUG=true`): writes full action trace to a timestamped file under `/tmp/`.
- **Quarry-v2 gap**: browser agent requires an LLM to drive it; current design sends `browserActions[]` as a typed list from the API caller, not an autonomous loop. An LLM-driven action loop is not planned in the current phase roadmap.

### 11.5 Agent (Extract beta) — `controllers/v2/agent.ts`

- The `/v1/agent` endpoint is a **thin proxy** to `EXTRACT_V3_BETA_URL` (external hosted service).
- Before proxying: checks `zeroDataRetention` flag (ZDR) and rejects if the team's ZDR policy is incompatible with the beta endpoint.
- Free-tier quota enforced via a Supabase RPC call (`check_and_increment_free_quota`).
- **Quarry-v2 gap**: no agent endpoint and no proxy-to-external-service pattern. The ZDR flag pattern is worth adopting for any future agentic endpoint.

### 11.6 Extraction Service — `lib/extract/extraction-service.ts`

- `performExtraction()` is the top-level entrypoint; it fans out to URL scrapers, then feeds results into LLM completions.
- **SERP rephrasing**: generates multiple search queries from the user schema and issues them to the internal search index to augment source URLs.
- Schema helpers: `dereferenceSchema`, `spreadSchema`, `mixSchemas`, `deduplicateSchemaKeys`, `mergeNullValues` — a non-trivial schema manipulation layer.
- Two completion strategies based on detected structure:
  - `singleAnswerCompletion` — when the schema has a single top-level answer field.
  - `multiEntityCompletion` — when the schema expects an array of objects.
- `CostTracking`: tracks LLM token spend; aborts extraction if cost exceeds `1.5×` the estimated budget.
- `SourceTracker` + `URLTrace`: every extracted field records which source URL and which page fragment it came from.
- **Quarry-v2 gap**: extraction/LLM completion is delegated to the Data Plane service; source tracking and cost guard are not currently specified in the Quarry-v2 contracts.

### 11.7 Transformer Pipeline — `scraper/scrapeURL/transformers/index.ts`

Full ordered pipeline per scrape response:

```
rawHTML → sanitizeHtml → buildMarkdown
       → (optional) LLM extract / summary / query / cleanContent
       → (optional) uploadScreenshot → CDN URL in response
       → (optional) performAgent (browser-agent loop)
       → (optional) performAttributes (structured metadata)
       → (optional) deriveDiff (Blake3 + semantic delta)
       → (optional) fetchAudio (TTS)
       → (optional) brandingTransformer (colors/fonts/logo via WASM)
       → (optional) useIndex / useSearchIndex (ingest to vector/search store)
```

- `useIndex`: pushes processed chunks to a configured vector index after each scrape.
- `useSearchIndex`: pushes to a keyword search index.
- **Quarry-v2 gap**: the pipeline is modeled as a typed `TransformOptions` struct in Phase 6 but the search/vector index steps are not yet implemented. The Firecrawl-style rendered `brandingTransformer` still has no equivalent, although Quarry-v2 now performs static branding enrichment in edge/runtime from persisted HTML artifacts.

### 11.8 Deep Research Service — `lib/deep-research/deep-research-service.ts`

- `performDeepResearch(query, options)` iterates a loop:
  1. Generate sub-questions from current research state.
  2. Issue SERP queries for each sub-question.
  3. Scrape top-N URLs per query.
  4. Feed scraped content to LLM to update `ResearchStateManager`.
  5. Repeat until `maxDepth`, `maxUrls`, or wall-clock `timeLimit` is reached.
- `ResearchStateManager` tracks: visited URLs, accumulated findings, confidence score, iteration count.
- `billTeam()` call after each iteration: tokens used are billed to the team quota incrementally.
- **Quarry-v2 gap**: deep research is entirely absent. It requires SERP integration, an LLM orchestrator, and incremental billing — all out of scope for the current Quarry-v2 phase plan.

### 11.9 Zero-Data-Retention (ZDR) Flag

- ZDR is a first-class boolean on every team record in the DB.
- Scrape results for ZDR teams are **never written to Redis or Postgres**; they are streamed directly back to the caller.
- Any feature that stores intermediate content (screenshots, index ingest, agent loop history) checks ZDR before persisting.
- **Quarry-v2 status**: `/v1/scrape` now threads ZDR through cache, transform/artifact persistence, and event emission. Durable publisher channels stay empty while live SSE subscribers still receive ephemeral progress. Future async result stores, index ingest, and agent/data-plane features must preserve the same guard.

---

## 12. Updated Gap Priority Matrix

Extends Section 10 with new items from source analysis.

| # | Gap | Impact | Effort | Owner Phase |
|---|-----|--------|--------|-------------|
| 1 | Eval harness + warm/cold benchmarks | P0 — cannot retire V1 without proof | Medium | Phase 8 |
| 2 | Donor scrape removal (V1 hot path) | Closed — donor path removed; AppState carries no donor URL; scrape handler routes 100% through PageRunner::run(); cargo check clean | Done | Done 2026-05-05 |
| 3 | Coverage CI gate (≥ 80 %) | Closed — CI workflow enforces Rust + Go coverage floors at 80% and fails on regression; local `make coverage` command documented | Done | Done 2026-05-06 |
| 4 | Sitemap + robots.txt in crawl BFS | Closed — `CrawlJobWF` now seeds from sitemap, checks robots before `RunPage`, and emits `page.blocked` on robots denial; targeted workflow tests pass | Done | Done 2026-05-05 |
| 5 | Browser action schema in edge API | Closed — public schema, runtime execution, retry, screenshot-after-each validated | Done | Done 2026-05-04 |
| 6 | Mobile emulation + geo targeting | P1 — parity with V1 + Firecrawl; Chromium/static/browserless baseline shipped, provider-wide fingerprint proof still pending | Low | Phase 9 |
| 7 | `/v1/map` endpoint | Closed — endpoint implemented in `quarry-edge` | Done | Done 2026-05-04 |
| 8 | LLM `summary`/`query`/`json` formats | Closed — delegates generation to Model Plane and retrieval context to Data Plane | Done | Done 2026-05-04 |
| 9 | SDK (at minimum Python + TypeScript) | P2 — DX, adoption | High | Phase 9 |
| 10 | OpenAPI spec | P2 — developer onboarding | Low | Phase 9 |
| 11 | PDF ingest (LlamaParse or equivalent) | Reduced — self-hosted text-PDF extraction shipped; OCR/layout parser remains optional parity work | Medium | Phase 9 |
| 12 | `branding` format (rendered / WASM extractor parity) | Reduced — static Rust branding extractor shipped; rendered/WASM parity remains optional | Medium | Phase 9 |
| 13 | Browser-device emulation parity (metrics/profile fidelity) | P3 — metrics/touch/locale shipped, full fingerprint benchmarking still partial | Medium | Phase 9 |
| 14 | `zeroDataRetention` — pervasive threading through pipeline | Closed for `/v1/scrape` — cache/artifact/durable-event bypass validated | Done | Done 2026-05-04 |
| 15 | OTEL exporter (spans to Jaeger/Honeycomb) | P2 — observability parity | Low | Phase 8 |
| 16 | Robots.txt per-origin cache (Redis, 10 s TTL) | Closed — runtime Redis-backed robots cache is now exercised through the internal robots check endpoint used by crawl orchestration | Done | Done 2026-05-05 |
| 17 | Browser path SSRF via chromiumoxide (DnsGuard not called) | Closed — `guard_navigation_target`/`guard_page_request_target` (`crates/quarry-browser/src/navigation.rs`) wired into `ChromiumoxideDriver::goto()` and `install_network_guard()`; SSRF tests in `crates/quarry-browser/tests/ssrf.rs`. Re-verified 2026-08-06: `navigation.rs` had gone missing entirely since this was marked Done (unresolved `crate::navigation` import — the `chromiumoxide` feature failed to compile, invisible because it isn't a default feature). Module reconstructed from the recovered source; `cargo check`/`cargo test --features chromiumoxide` now pass clean. | Done | Done 2026-05-05, re-verified 2026-08-06 |
| 18 | A/B test tagging on scrape requests | P3 — experiment infra | High | Phase 9+ |
| 19 | LLM-driven browser agent loop (MAX_STEPS gate) | P3 — autonomous scraping | High | Phase 9+ |
| 20 | Vector/search index ingest step in transformer pipeline | P2 — RAG pipeline integration | Medium | Phase 6 |
| 21 | Deep research loop (SERP → scrape → LLM → iterate) | P3 — advanced feature | Very High | Post-V2 |
| 22 | Extraction source tracking (URLTrace per field) | P2 — auditability | Medium | Data Plane |
| 23 | Extraction cost guard (1.5× budget abort) | P2 — cost safety | Low | Data Plane |
| 24 | Browser path SSRF via Browserbase/Browserless/Kernel (REST drivers had zero guard) | Closed for entry point — all three now call `guard_navigation_target` before `goto()`; Browserless also guards redirects/subresources via `rejectRequestPattern`. Browserbase's and Kernel's declarative filters are main-frame-navigation-only per vendor docs, so redirects/subresources stay unguarded for those two. See §12.1. | Done (entry point) / Open (full-session for Browserbase, Kernel) | Done 2026-08-06 |

### 12.1 Remote browser provider SSRF capability audit (2026-08-06)

Item 17 covers the local `chromiumoxide` driver: `guard_navigation_target` blocks the entry point (session-create/`goto`), and `install_network_guard` additionally attaches a CDP `Fetch` listener that re-checks every subsequent request the page makes (redirects, XHR, iframes, images, stylesheets) — full-session containment.

The three remote-CDP drivers (`browserbase.rs`, `browserless.rs`, `kernel.rs`) now all call `guard_navigation_target` before every `goto()` too (previously none of them called any guard at all). None of them hold a local CDP `Page`, so none can install an in-process `Fetch` listener the way chromiumoxide does. Per-provider audit of what's actually available instead, from each vendor's current docs:

- **Browserbase** — `browserSettings.allowedDomains` (a session-creation field) restricts *main-frame* navigation to a caller-supplied domain list; Browserbase's own docs state it does not cover subframes, images, scripts, or XHR. Not wired in: it's an allowlist (this driver serves arbitrary caller-specified domains, so there's no fixed list to supply), it's a session-creation-time setting while this driver creates its Browserbase session eagerly in `acquire()` before any URL is known, and even if wired it wouldn't close the redirect/subresource gap. Docs don't clearly state whether the CDP `Fetch` domain is available or restricted over the `connectUrl` websocket for hosted sessions. No HAR/webhook network-observability export is documented (Session Inspector's Network tab and Session Replay are dashboard/visual features, not an export or webhook). Sources: docs.browserbase.com/reference/api/create-a-session, /guides/security, /platform/browser/observability/observability, /features/proxies.

- **Browserless** — `rejectRequestPattern`/`rejectResourceTypes` (JSON-body fields on `/content`, `/screenshot`, `/pdf`, etc.) are wired in (see `browserless.rs`'s `SSRF_REJECT_PATTERNS`). Browserless applies these across the whole page load for that call — the closest thing to full-session coverage any of the three remote drivers gets. It's a string/regex match against the request URL, not a DNS resolution, so it can't catch DNS-rebinding, and only covers the IP ranges/hostnames in the pattern list. Docs don't confirm or deny whether hosted CDP sessions expose the `Fetch` domain without restriction, though other documented recipes (`page.createCDPSession()` plus custom `Browserless.*` CDP commands) suggest the CDP transport isn't sandboxed. Session Replay records network requests for dashboard playback, but there's no documented HAR export or webhook. Sources: docs.browserless.io/rest-apis/request-configuration, /baas/session-management/recording-liveurl, /baas/interactive-browser-sessions/session-replay.

- **Kernel** — `chrome_policy` accepts standard Chrome Enterprise policy keys including `URLBlocklist`/`URLAllowlist`, but Kernel's docs state this gates top-level navigation only and does not block resources/requests a permitted page loads from other origins — the same scope `guard_navigation_target` already covers, more precisely (DNS-resolution-based, not string-pattern-based) — so wiring it in would be redundant rather than gap-closing. Docs don't confirm or deny `Fetch`-domain restriction on the `cdp_ws_url` websocket. Kernel does offer an opt-in `network` telemetry category (request/response/redirect events with full URLs, streamable or paginated) that could *detect*, not prevent, a session reaching a private address; no field records the resolved server IP, so it wouldn't catch DNS-rebinding either. That's a plausible future monitoring addition, not a request-blocking guard, and isn't wired in. Sources: Kernel OpenAPI spec (`BrowserRequest`/`Browser`/`BrowserNetworkRequestEvent`), docs `browsers/chrome-policies`, `browsers/telemetry/categories`, `security`, `shared-responsibility-model`.

**Net effect**: all four drivers now guard the entry point. Only chromiumoxide, and partially Browserless, guard redirects/subresources within a session; Browserbase and Kernel do not — closing that for them would require either a driver rewrite to a real CDP connection over their `connectUrl`/`cdp_ws_url` (unverified whether `Fetch` is even reachable there) or accepting the weaker, main-frame-only allow/block-list each already exposes.

---

## 13. V2 Unique Advantages (not present in V1 or Firecrawl)

1. **Self-owned TLS impersonation** — BoringSSL JA3/JA4/H2 via `wreq`; works in self-host with no Fire Engine subscription.
2. **Typed ULID ID scheme** — All resource IDs carry their type as prefix; prevents cross-resource confusion; immutable after issuance.
3. **Frozen REST contracts** — `docs/CONTRACTS.md` is the single source of truth; both Rust and Go derive from it; envelope shape is guaranteed stable.
4. **Append-only Postgres event log** — Full ordered history per run with cursor pagination and idempotency keys; no event is ever lost on restart.
5. **Blake3 fingerprint + semantic paragraph diff** — `SemanticDiff::is_real_delta` filters noise; consumers get structured change objects, not raw hash comparisons.
6. **Wiki-ready content chunks** — Deterministic fixed-size paragraph chunks with stable IDs; designed for RAG ingestion pipelines.
7. **DriverPlan typed waterfall** — Engine fallback is a structured type, not ad-hoc if/else; each tier is independently testable.
8. **Preset catalog** — `fast`/`polite`/`stealth`/`deterministic` presets drive both `DriverPlan` and crawl policy from a single registry.
9. **Crawl BFS durability** — 50-page checkpoints in Postgres; crawl jobs resume after restart without re-crawling already-visited pages.
10. **Security engine separation** — `DnsGuard` resolves the host at the network layer before any fetch; SSRF is blocked at DNS resolution, not just URL parsing.
11. **Self-hosted document ingest** — text, binary, Wikipedia, and PDF source routing now stays inside the Quarry runtime without a hosted parsing dependency for baseline extraction.
12. **Ephemeral ZDR event path** — zero-data-retention scrapes can keep live progress streaming while bypassing durable publisher channels and artifact writes.

---

## 14. Gap Closure Phase System

This section is the operating system for the rest of this document. Sections 10 and 12 identify the gaps; this section defines how each gap moves from open work to closed evidence.

### 14.1 Status Model

| Status | Meaning | Required Evidence |
|--------|---------|-------------------|
| Open | No shipped implementation or no accepted design. | Owner phase, acceptance criteria, and target test path. |
| Partial | Core behavior exists, but parity, coverage, or measurement is incomplete. | Shipped scope, residual risk, and the next proof item. |
| Reduced | Baseline behavior shipped; only optional or high-fidelity parity remains. | Regression coverage plus a written statement of remaining non-critical scope. |
| Closed | Behavior shipped and verified against its acceptance criteria. | Passing tests, benchmark or contract evidence when relevant, and matrix update. |
| Deferred | Intentionally outside the V2 cutover target. | Deferral reason, future owner, and trigger for reconsideration. |

### 14.2 Phase Tracks

| Closure Phase | Roadmap Anchor | Owns | Active Gap IDs | Exit Gate |
|---------------|----------------|------|----------------|-----------|
| Phase 3 - Cutover Gate | Fast-path cutover | Remove remaining donor scrape dependency and prove edge-owned scrape traffic. | #2 | `/v1/scrape` no longer depends on the V1 hot path; canary reaches 100%; warm scrape p50 stays within target. |
| Phase 4 - Browser Safety Gate | Browser lease model | Browser-path security and session correctness. | #17 | Chromiumoxide/browser routes enforce DNS/SSRF guard behavior equivalent to static fetch; lease/profile round-trip tests cover restore and cleanup. |
| Phase 5 - Crawl Correctness Gate | Crawl workers to Rust | Remaining crawl parity: richer include/exclude parity, backward/crawlEntireDomain controls, broader denial reasons, adaptive throttling, and eventual Rust frontier ownership. | — | Remaining crawl fixtures prove restart recovery and cancellation beyond the now-closed sitemap/robots/path-policy slice. |
| Phase 6 - Pipeline Integration Gate | Output normalization + change tracking | Post-transform indexing hooks and artifact-backed downstream integrations. | #20 | Transformer/index handoff is feature-gated, ZDR-safe, retryable, and covered by integration tests. |
| Phase 8 - Evidence Gate | Benchmark + retire old path | Benchmark truth, coverage enforcement, OTEL/export visibility, final V1 retirement decision. | #1, #3, #15 | Scoreboard generated; coverage gate enforced; warm/cold and JS benchmarks captured; retirement blocker list is empty. |
| Phase 9 - Product Parity Gate | Product parity hardening | SDKs, OpenAPI, fingerprint proof, OCR/layout parity, branding/audio, optional experimentation and agent loops. | #6, #9, #10, #11, #12, #13, #18, #19 | Public DX docs and SDKs are usable; optional/high-cost features are either shipped behind flags or explicitly deferred. |
| Data Plane Gate | Cross-plane extraction | Extraction source tracking and token/cost guardrails. | #22, #23 | Model/Data Plane contracts expose source trace and budget-abort semantics; Quarry callers receive auditable artifacts. |
| Post-V2 Gate | Future advanced surface | Deep research loop and other advanced agentic workflows. | #21 | New product brief accepted; SERP, scrape, LLM, billing, and ZDR constraints are designed before implementation. |

### 14.3 Priority Rules

- P0 gaps block V1 retirement and cannot be deferred without an explicit cutover exception.
- P1 gaps block parity claims for the affected surface area, but do not necessarily block unrelated phase work.
- P2 gaps can ship after cutover if the public contract is stable and the missing work is documented.
- P3 gaps are optional parity or advanced-product work; they should be feature-gated or moved to Post-V2 if they threaten cutover focus.

### 14.4 Promotion Checklist

Before moving a gap to `Closed`, update the row in Sections 10 and/or 12 and verify:

- Contract or API docs are updated when the external surface changes.
- Unit or integration coverage exists for the behavior and at least one failure path where applicable.
- Benchmark or scoreboard evidence exists for performance-sensitive work.
- ZDR, SSRF, and persistence behavior are explicitly checked for any feature that fetches, stores, indexes, or emits user content.
- `docs/PROGRESS.md` reflects the same phase status so roadmap and gap analysis stay in sync.

---

## 15. Remaining Gap Closure Plan

This section is the implementation plan for closing the rest of the gaps in this file. Treat it as the executable backlog until the matrices in Sections 10 and 12 are fully closed, reduced, or explicitly deferred.

### 15.1 Operating Rules

- `GAP.md` is the current source of truth. Companion docs must be synchronized from this file, not the other way around.
- Rust remains the scrape hot path. Do not add Python or Go to the synchronous `/v1/scrape` path.
- Go remains the durable orchestration/control layer. Use Go dependencies only where they improve workflow correctness, integration tests, or control-plane routing.
- Python belongs in `lab/evals/` or async Model/Data Plane workers unless a provider SDK makes Python unavoidable.
- LLM, speech, agent reasoning, retrieval, embeddings, and cost guards are Model/Data Plane responsibilities. Quarry captures evidence and executes deterministic browser actions.
- Every new dependency must pass license review, `cargo audit` / `govulncheck` / `pip-audit`, and a benchmark or quality fixture before production enablement.
- Any feature that sends content to Model Plane or Data Plane must be rejected or explicitly bypassed when `zeroDataRetention=true`.

### 15.2 Phase 8 - Evidence Gate Workplan

Owns open gaps: #1, #15.

| Work item | Implementation | Dependencies | Acceptance evidence |
|-----------|----------------|--------------|---------------------|
| Eval harness + scoreboard | Create `lab/evals/` with corpus manifests, runner adapters for Quarry V2, Quarry V1, Firecrawl local, and optional Firecrawl cloud. Generate `docs/SCOREBOARD.md`. | Python lab-only: `pytest`, `pytest-asyncio`, `httpx`, `rich`, `pandas`, `trafilatura`, `readability-lxml`, `curl_cffi`, `pymupdf`, optional `playwright`. | Scoreboard includes warm/cold scrape latency, JS success, block rate, markdown quality, chunk stability, PDF text quality, profile restore, crawl restart, and change precision. |
| Coverage CI gate | Add Rust `cargo llvm-cov` and Go coverage jobs. Fail below 80 percent aggregate and below agreed crate/service floor for touched modules. | CI tool, not runtime dependency: `cargo-llvm-cov`; Go native coverage. | CI fails on coverage regression; local command documented. |
| OTEL exporter | Add feature-gated OTLP in Rust edge/runtime and Go control/orchestrator. Propagate `traceparent` across edge -> runtime -> control -> orchestrator. | Rust: `opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry`. Go: `go.opentelemetry.io/otel`, `go.opentelemetry.io/contrib/instrumentation/net/http/otelhttp`, OTLP exporter. | Jaeger/OTel collector trace shows one scrape/crawl across all services with run_id, job_id, org_id, driver tier, and artifact IDs. |
| Doc synchronization gate | Add a doc-sync checklist to PR template or CI script that checks known stale claims in `GOAL.md`, `PLAN.md`, `PROGRESS.md`, `ROADMAP.md`, `CONTRACTS.md`, and `QUARRY_V2_MODEL_PLANE_PARITY.md`. | No runtime dependency. | Companion docs no longer contradict this file on donor removal, `/v1/map`, LLM formats, Phase 3, and Phase 9 status. |

Exit gate: `docs/SCOREBOARD.md` exists, P0 gaps #1 and #3 are closed, OTEL traces are visible for at least one scrape and one crawl, and stale companion docs are updated.

### 15.3 Phase 5/9 - Scraping, Crawl, and Fingerprint Proof

Owns open or partial gaps: #6, #13.

| Work item | Implementation | Dependencies | Acceptance evidence |
|-----------|----------------|--------------|---------------------|
| Fingerprint benchmark | Add internet-gated tests against TLS/HTTP fingerprint endpoints. Capture JA3/JA4/H2, UA, viewport, timezone, locale, proxy country, and device metrics per driver. | Keep `wreq` exact-pinned for hot path. Do not add `wreq-util` until license review. Use `curl_cffi` only as Python lab control. | Scoreboard proves static TLS, chromiumoxide, Browserless, and Browserbase profiles match intended browser/device families within accepted thresholds. |
| Mobile and geo provider parity | Request-level mobile, locale, header, touch, and ad-block wiring now exists in static, chromiumoxide, Browserless, and Browserbase paths. Remaining work is provider-specific geo validation, country-routing proof, and benchmark evidence. | Existing browser drivers. Add provider-specific test fixtures only. | Provider matrix marks each dimension pass/fail with captured artifacts. |
| Crawl policy parity | Closed for path-scope, matcher semantics, and typed denial reasons: backward crawling alias + `crawl_entire_domain` canonicalized; include/exclude now supports path + full-URL regex/glob; orchestrator decisions return stable denial codes plus operator-readable messages. | Existing Go matcher implementation; no new dependency required. | Workflow fixtures cover include/exclude, external links, subdomains, backward links, whole-domain behavior, sitemap scope, and typed denial-code/message pairs. |
| Adaptive throttling and session health | Implement per-host EWMA latency, max/min delay, block-response penalty, and session/proxy `good`/`bad`/`retired` states. | Prefer small in-house EWMA; add no dependency unless benchmark proves value. Use Crawlee/Scrapy behavior as reference only. | E2E crawl under synthetic 200/429/5xx server slows down on errors and never decreases delay from non-200 latency. |
| Rust frontier decision | Keep Go BFS if Phase 8 scoreboards prove it meets restart/cancel/SLO goals. Move to Rust frontier only if Go activity overhead or crash recovery misses targets. | No dependency until measurement. | Decision record in `PROGRESS.md`: keep Go BFS, or start Rust frontier implementation with measured reason. |

Exit gate: V2 can claim mobile/geo/fingerprint parity only after provider-wide matrix evidence exists. Crawl parity can close only after denial reason and restart/cancel fixtures pass.

### 15.4 Phase 9 - Developer Experience and Public Contract Parity

Owns open gaps: #9 and #10.

| Work item | Implementation | Dependencies | Acceptance evidence |
|-----------|----------------|--------------|---------------------|
| OpenAPI spec | Generate OpenAPI from Rust/Go handlers while keeping `CONTRACTS.md` as canonical. Snapshot generated spec in CI. | Rust: `utoipa` + `utoipa-axum` for Axum routes, or `aide` if route wrapping is preferred. Recommended first pass: `utoipa` because it has direct Axum bindings and broad schema support. | `openapi.json` includes scrape, crawl, map, change, schedule, profile, event history, presets, blocklist, and internal endpoints where appropriate. |
| SDK generation | Generate TypeScript and Python SDKs from OpenAPI, then hand-polish auth, streaming, and artifact helpers. | Use OpenAPI generator tooling in CI; do not write SDKs manually first. | Python and TS smoke tests can call scrape, stream SSE, crawl status, map, and retrieve artifacts against local compose. |
| Firecrawl-compatible response adapter | Add stable adapter for common Firecrawl field names while preserving Quarry envelopes internally. | No new dependency. | Compatibility tests compare expected Firecrawl-style response shape for `markdown`, `rawHtml`, `screenshot`, `json`, `summary`, `query`, `changeTracking`. |
| Self-host and migration guide | Update deployment docs with `wreq` MSRV/CMake/BoringSSL requirements, Browserbase/Browserless flags, Model/Data Plane env vars, and ZDR limits. | No dependency. | Fresh checkout guide runs local scrape, crawl, and Model Plane mock enrichment. |

Exit gate: OpenAPI is generated and stable, SDK smoke tests pass, and public docs do not expose internal-only implementation details as stable API.

### 15.5 Cross-Plane LLM, Model Plane, and Data Plane Synchronization

Owns open gaps: #22 and #23, `/v1/extract`, and structured JSON parity from `QUARRY_V2_MODEL_PLANE_PARITY.md`.

| Work item | Plane owner | Implementation | Acceptance evidence |
|-----------|-------------|----------------|---------------------|
| Public `/v1/extract` endpoint | Quarry Edge + Model Plane + Data Plane | Implement as an orchestration wrapper over deterministic scrape artifacts plus Model Plane structured extraction. Single-URL extract can run synchronously when below budget; multi-URL/query extract becomes async job with event history. | Endpoint accepts schema/prompt/source URLs, returns `extract.json` artifact refs, source trace, usage, and typed partial-failure report. |
| Structured JSON schema passthrough | Model Plane + Quarry Edge | Extend Model Gateway HTTP `InvokeRequest` with `structured_output_schema` and forward it to `InferRequest.structured_output_schema`. Update Quarry `ModelPlaneClient::invoke` to accept optional schema for `json` format instead of only embedding schema in the prompt. | Wiremock/e2e test proves Quarry sends schema, Model Gateway forwards it, inference-core receives it, and invalid model JSON fails closed. |
| Extraction source trace | Data Plane + Model Plane + Quarry Edge | Define `extract.json` as `{ value, source_trace, model_request_id, model_used, retrieval, usage }`. Source trace should map fields to source URL, artifact ID, chunk ID, and optional character offsets. | Contract test validates source trace for single-field and multi-entity extraction. |
| Cost guard and budget abort | Model Plane | Add per-request estimated budget and hard abort semantics. Firecrawl reference is `1.5x` estimated budget; Quarry should expose `max_cost_usd` / `max_tokens` in the format object or org policy. | Model Plane returns typed budget-aborted error; Quarry stores no partial AI artifact unless contract explicitly allows it. |
| Data Plane ingest lifecycle | Data Plane + Quarry Edge | Add explicit `dataPlaneIngest` option. Quarry sends markdown/chunks/meta to `/internal/v1/documents`, receives document ID plus indexing status, and never assumes immediate retrieval. | Response includes document ID and index status; `query` can use either existing retrieval or the newly indexed document only after status allows. |
| ZDR enforcement | Quarry Edge + Model/Data Plane | AI formats, Data Plane ingest, index hooks, agent loops, screenshots for agent traces, and audio/branding model calls must reject or run ephemeral-only under ZDR. | Tests prove `zeroDataRetention=true` rejects external-plane formats and writes no durable events/artifacts beyond allowed live response. |
| Cross-plane auth | All planes | Replace dev bearer bypasses with real internal service auth before production. Preserve `x-internal-key`, `x-org-id`, and service-name scoping for Data Plane. | Auth tests cover missing, wrong, and cross-org credentials. |

Exit gate: `summary`, `json`, and `query` are no longer just prompt wrappers; they are schema-aware, auditable, budgeted, and ZDR-safe across Quarry, Model Plane, and Data Plane.

### 15.6 Phase 9 Optional Format Parity

Owns open gaps: #11, #12, and the `audio` format row.

| Format | Best implementation | Dependencies | Closure status target |
|--------|---------------------|--------------|-----------------------|
| OCR/layout PDF ingest | Keep current Rust `pdf-extract` for text PDFs. Add async document-processing job for scanned/layout-heavy PDFs through Model Plane or a dedicated document worker. Use Python only in that async worker if needed. | Python async worker candidates: `pymupdf` for layout/text baseline; OCR provider or Azure Document Intelligence belongs in Model Plane provider traits. | Reduce, not P0 close, unless OCR/layout is a launch requirement. |
| `branding` | Static Rust extractor is shipped: parse persisted HTML for logo candidates, CSS colors, font families, and Open Graph metadata. Use browser screenshot/visual Model Plane only for canvas/rendered-heavy pages if product parity requires it. | Existing `scraper` + `regex`; `lightningcss` remains optional if stylesheet fidelity needs to increase. | Static branding closed; visual/rendered branding remains optional enhancement. |
| `audio` | Do not synthesize in Quarry. Add Model Plane speech provider routing and return `audio.json` with artifact reference, provider, voice, duration, and usage. | Model Plane speech provider traits; storage in Model Plane or Artifact Plane. | Deferred until Model Plane speech is real. Quarry should keep returning unsupported until then. |
| A/B scrape tagging | Add request metadata and result metadata only after scoreboards are stable. | No dependency. | Phase 9+ optional. |

Exit gate: no optional format is advertised as closed until it has a real provider, stored artifact, cost/ZDR behavior, and contract tests.

### 15.7 Agentic Browsing and Deep Research

Owns open gaps: #19 and #21.

| Work item | Best implementation | Acceptance evidence |
|-----------|---------------------|---------------------|
| Agent action loop contract | Model Plane owns planning. Quarry owns action execution and observations. Define `POST /v1/agent` as a long-running job that emits `agent.started`, `agent.delta`, `action.started`, `action.completed`, `preview.*`, and `agent.completed`. | Contract doc updated; mock Model Plane planner drives a browser task through Quarry action runtime in e2e. |
| Observation format | Quarry returns DOM snapshot summary, accessibility-like element list when available, screenshot artifact ID, current URL, action result, and policy denial reasons. | Golden fixtures for click/fill/scroll/query extraction over a local dynamic test site. |
| Guardrails | Enforce `max_steps`, `max_runtime_s`, max tokens, browser lease TTL, allowed domains, robots policy, SSRF, and ZDR. | Tests prove stop conditions and policy denials are emitted and persisted correctly. |
| BrowserBroker integration | Use Model Plane BrowserBroker for trusted grants/session lifecycle only. Do not let agent bypass Quarry action/runtime policy. | Revoked grant fails before browser action execution. |
| Deep research | Keep Post-V2. Requires SERP provider, iterative crawl, LLM state manager, billing, and ZDR design before implementation. | Product brief accepted before any endpoint ships. |

Exit gate: autonomous browsing can ship only behind a feature flag after deterministic typed actions, observations, cost limits, and ZDR behavior are proven.

### 15.8 Missing Public Endpoint Surface

Owns endpoint gaps from Section 6 that are not already covered by format parity.

| Endpoint | Best implementation | Dependencies | Acceptance evidence |
|----------|---------------------|--------------|---------------------|
| `POST /v1/search` | Initial deterministic multi-URL page-backed search is now shipped. Remaining parity work is provider-backed SERP/result discovery, provider metadata, and optional handoff into scrape/extract jobs. Do not confuse this with Data Plane retrieval, which searches internal knowledge. | Existing Rust/Go HTTP clients are sufficient for the current path; provider integration is still pending selection and review. | `quarry-edge` integration test returns ranked matches from scraped pages; provider contract tests remain required before claiming Firecrawl parity. |
| `POST /v1/extract` | Current public endpoint synchronously wraps scrape + Model Plane extraction for a single URL. Remaining parity work stays in Section 15.5: structured schema passthrough, source trace, usage metadata, and budget-abort behavior. | Model Gateway structured schema passthrough; Data Plane source trace when retrieval/ingest is involved. | Single URL sync test passes today; multi URL async, schema failure, and budget-abort tests remain open. |
| `POST /v1/agent` | Current public endpoint is a synchronous one-step fetch + model-answer wrapper. The long-running planner/action loop remains covered by Section 15.7 and is not yet closed. | BrowserBroker only for grants once the full loop exists; no direct agent bypass around Quarry policy. | Integration test covers the current sync wrapper; feature-gated e2e over a local dynamic site is still required before closing agent-loop parity. |
| `POST /v1/deep-research` | Endpoint now returns explicit `501 UNSUPPORTED`; keep the product surface deferred Post-V2 until `/v1/search`, `/v1/extract`, cost guard, and agent event contracts are stable. | SERP provider, scrape orchestration, Model Plane research state, billing/cost guard. | Accepted product brief before implementation. |

Exit gate: every endpoint row in Section 6 is either implemented with tests, reduced with explicit scope notes, or marked Post-V2 / unsupported with a trigger for reconsideration.

---

## 16. Dependency Decision Record

### 16.1 Adopt or Evaluate Now

| Dependency | Language | Decision | Why |
|------------|----------|----------|-----|
| `wreq` | Rust | Keep exact-pinned in hot path. | Current best fit for self-owned TLS/JA3/JA4/H2 browser emulation with BoringSSL. Continue monthly fingerprint validation. |
| `utoipa` + `utoipa-axum` | Rust | Adopt for OpenAPI if snapshot generation stays maintainable. | Direct Axum/OpenAPI support; closes OpenAPI and SDK foundation. |
| `opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry` | Rust | Adopt behind `otel` feature. | Closes Phase 8 exporter gap without replacing existing `tracing`. |
| `lightningcss` | Rust | Evaluate for `branding`. | CSS parser/transformer suitable for extracting fonts/colors from stylesheets without a browser-model call. |
| `proptest` | Rust dev | Adopt for cache keys, URL normalization, fingerprint stability, and policy invariants. | Raises confidence in security-sensitive transforms. |
| `cargo-llvm-cov` | Rust CI tool | Adopt for coverage gate. | Best fit for workspace coverage enforcement; not a runtime dependency. |
| `github.com/bmatcuk/doublestar/v4` | Go | Evaluate/adopt for crawl path globs. | Mature globstar matcher for Firecrawl-compatible `**` include/exclude behavior. |
| `testcontainers-go` | Go dev | Adopt for Postgres/Redis/Temporal integration tests where Docker is available. | Tests production-like stores instead of mocks. |
| `go.opentelemetry.io/otel` + `otelhttp` | Go | Adopt for control/orchestrator traces. | Standard Go OTEL path for HTTP handlers/clients. |
| `trafilatura` | Python lab | Adopt as extraction benchmark only. | Strong baseline for boilerplate removal and content extraction quality. |
| `curl_cffi` | Python lab | Adopt as TLS benchmark/control only. | Useful independent control for browser impersonation; not production Quarry runtime. |
| `pymupdf` | Python lab/async worker | Adopt for PDF quality benchmarks; evaluate for async OCR/layout worker. | Strong PDF text/layout baseline; keep out of sync scrape path. |

### 16.2 Defer or Reject

| Dependency or pattern | Decision | Reason |
|-----------------------|----------|--------|
| `wreq-util` in production | Defer. | Useful emulation presets, but requires explicit license and maintenance review before hot-path adoption. |
| `impit` in production | Defer to lab experiment. | Potentially useful, but patched/unstable dependency cost is too high until `wreq` fails a measured target. |
| Crawlee or Scrapy as Quarry runtime dependencies | Reject. | Use their session-health and throttling behavior as references; do not embed Python/Node crawling frameworks in Quarry V2. |
| LangChain/LlamaIndex inside Quarry | Reject. | Retrieval, agent memory, and reasoning belong to Data/Model Plane. |
| Hosted LlamaParse as default PDF path | Reject as default; allow optional integration later. | Quarry needs self-host baseline. Hosted parsers can be optional external providers with ZDR/cost gates. |
| Playwright microservice as default browser layer | Defer. | Chromiumoxide/Browserless/Browserbase already fit current architecture; add a separate browser service only if isolation or parity metrics require it. |

### 16.3 Dependency Sources Checked

- `wreq` docs: https://docs.rs/wreq/latest/wreq/
- `utoipa` / `utoipa-axum` docs: https://docs.rs/utoipa/latest/utoipa/ and https://docs.rs/utoipa-axum/latest/utoipa_axum/
- OpenTelemetry Rust docs: https://opentelemetry.io/docs/languages/rust/
- `lightningcss` docs: https://docs.rs/lightningcss
- `curl_cffi` docs: https://curl-cffi.readthedocs.io/en/stable/
- PyMuPDF docs: https://pymupdf.readthedocs.io/
- Scrapy AutoThrottle reference: https://doc.scrapy.org/en/latest/topics/autothrottle.html
- Crawlee session reference: https://crawlee.dev/python/docs/0.6/guides/session-management
- Go `doublestar` docs: https://pkg.go.dev/github.com/bmatcuk/doublestar/v4
- Testcontainers for Go docs: https://docs.docker.com/guides/testcontainers-go-getting-started/

---

## 17. Documentation Synchronization Checklist

Use this checklist after implementing any row above. The goal is to prevent stale plan documents from re-opening closed gaps.

| Doc | Required synchronization |
|-----|--------------------------|
| `docs/PROGRESS.md` | Align Phase 3 donor removal, Phase 8 status, Phase 9 status, tests snapshot, known gaps, and next actions with this file. |
| `docs/GOAL.md` | Update phase build status and product comparison rows that still describe `/v1/map`, `summary`, `query`, `json`, PDF ingest, and donor removal as missing if this file marks them closed/reduced. |
| `docs/PLAN.md` | Mark completed Phase 2/3/5/6/7 items closed; move remaining work to the closure plan rather than leaving old todos as active. |
| `docs/ROADMAP.md` | Keep phase descriptions high-level, but remove claims that LLM formats are purely future if the bridge remains shipped. |
| `docs/CONTRACTS.md` | Add only additive contract changes: structured schema passthrough metadata, Data Plane ingest option, extraction source trace, cost guard, and agent observation/action payloads. |
| `docs/QUARRY_V2_MODEL_PLANE_PARITY.md` | Track cross-plane work from Section 15.5 and keep transport recommendations aligned with actual Model Plane capabilities. |
| `docs/SCRAPING_ENGINE_GAPS.md` | Either update Browserbase status to closed when verified or replace it with a pointer to this file if it is no longer maintained. |
| `apps/Model Plane/docs/GAP.md` | Cross-link the structured output, cost guard, speech, BrowserBroker, and Data Plane gates that Quarry depends on. |

No gap can be promoted to `Closed` unless this checklist has been applied or the promotion explicitly states why companion docs were not touched.


---

## 18. Verification & Correction Analysis (2026-05-06)

> **Verification Method:** Live codebase audit of quarry-edge routes (routes.rs, proxy.rs) and quarry-runtime implementations. Compared claimed status with actual source code endpoints.

### Key Finding: Most Previously Open Endpoints Are Now Routed

After re-checking the live codebase and validating the new tests, the old “missing endpoint” narrative is no longer accurate. Most of the surface that was missing in the earlier audit now has a real handler, compatibility shim, proxy route, or an explicit `501 UNSUPPORTED` response.

> **Post-change verification:** `cargo test -p quarry-edge --test ai_outputs --test proxy_forward` and `cargo test -p quarry-browser --features browserbase`

### Newly Shipped Since The Prior Audit

- `POST /v1/search` now returns ranked matches across scraped pages.
- `POST /v1/extract` now exposes synchronous scrape + Model Plane extraction for a single URL.
- `POST /v1/agent` now exposes a synchronous one-step fetch + answer flow.
- `POST /v1/change/check` and `GET /v1/change/latest` now use runtime-backed fingerprint/change output instead of placeholders.
- `POST /v1/security/blocklist` now provides an add/list compatibility shim over canonical `/v1/blocklists`.
- `GET /v1/runs/:id/events` and `POST /v1/runs/:id/events` now proxy the control-plane event log surface.
- `POST /v1/presets` and `POST /v1/deep-research` now fail explicitly with `501 UNSUPPORTED` instead of silent route gaps.
- Browserbase now applies request-level mobile metrics, locale/header overrides, touch emulation, ad blocking, PDF support, and fail-fast environment validation.

### Current Endpoint Gap Snapshot

#### ✅ Implemented Or Explicitly Routed

- **Core scrape/crawl/batch/map:** `/v1/scrape`, `/v1/scrape/stream`, `/v1/crawl`, `/v1/batch`, `/v1/map`
- **Operational/public wrappers:** `/v1/search`, `/v1/extract`, `/v1/agent`, `/v1/change/check`, `/v1/change/latest`
- **Control-plane proxy surface:** `/v1/profiles`, `/v1/snapshots`, `/v1/artifacts`, `/v1/schedules`, `/v1/jobs/:id/history`, `/v1/restore`, `/v1/blocklists`, `/v1/runs/:id/events`
- **Internal/runtime routes:** `/v1/internal/run_page`, `/v1/internal/fetch_sitemap`, `/v1/internal/check_robots`
- **Explicit unsupported routes:** `POST /v1/presets`, `POST /v1/deep-research`

#### ⚠️ Reduced / Partial Parity

1. `POST /v1/search` now supports NL ranking via Model Plane (`model-ranked` mode) and deterministic term-match fallback (`page-backed`). Provider-backed SERP/discovery (no `urls` required) is still not implemented.
2. `POST /v1/extract` ships for single-URL synchronous extraction. Schema is now forwarded as `structured_output_schema` to Model Gateway. Source trace, usage metadata, and budget-abort semantics remain open in Model/Data Plane work.
3. `POST /v1/agent` is a synchronous one-shot wrapper, not the long-running planner/action/event loop described in Phase 15.7.
4. `POST /v1/security/blocklist` supports add/list/remove; removal is wired through `DELETE /v1/blocklists/:id` on both the control plane and edge shim.
5. `GET /v1/runs/:id/events` supports two modes: `Accept: text/event-stream` returns a live SSE stream; default JSON returns the proxied durable event log.
6. Browser/device request wiring is present across static, chromiumoxide, Browserless, and Browserbase paths, but fingerprint and provider-wide geo proof still require benchmark evidence.
7. `POST /v1/crawl` accepts a `prompt` field that is forwarded to the orchestrator; actual pre-BFS URL ranking requires the Go orchestrator to call Model Plane for the `rank_urls` helper (contract defined, orchestrator-side implementation is Phase 15.5 work).

#### ❌ Still Deferred / Not Implemented

- `POST /v1/presets` creation remains unsupported; only the GET preset catalog is real.
- `POST /v1/deep-research` remains Post-V2 and intentionally unsupported.
- Public DX work is still open: OpenAPI generation and SDK generation.
- Optional format parity is still open for `audio` and rendered/visual branding hardening.

### Implementation Completeness Assessment

#### ✅ Fully Implemented
- **Request parameters:** mobile, locale, location, blockAds, headers, cache, fingerprint, org_id, zero_data_retention, browser actions
- **Output formats:** markdown, html, rawHtml, links, images, attributes, screenshot, pdf, chunks, change, meta
- **LLM bridge formats:** summary, json extraction, query/Q&A
- **Operational routing:** change endpoints, job history, blocklist proxy surface, and run-event log routing
- **Browser actions:** All 8 core actions + screenshotAfterEach + retry support
- **Durability:** Postgres append-only events, idempotency keys, webhook DLQ, 50-page checkpoints, full job history
- **Browserbase request parity wiring:** mobile metrics, locale/header overrides, touch, ad blocking, PDF, and fail-fast env validation

#### ✅ Newly Closed (2026-05-06)
- **Security blocklist parity:** add/list/remove fully wired through shim → `DELETE /v1/blocklists/:id` → control plane Postgres. Tests: `blocklist_delete_is_forwarded`, `security_blocklist_shim_removes_by_id/pattern`.
- **Run events SSE:** `GET /v1/runs/:id/events` content-negotiated — live SSE or JSON log. Tests: `run_events_sse_streams_emitted_events`, `run_events_json_fallback_proxies_to_control`.

#### ✅ Newly Closed (2026-05-07)
- **NL prompt search:** `POST /v1/search` now uses `ModelPlaneClient::rank_search_results` when Model Plane is configured, returning `X-Quarry-Search-Mode: model-ranked`. Falls back to term-matching (`page-backed`) when Model Plane is unavailable or ranking fails. Tests: `search_endpoint_uses_model_plane_for_nl_ranking`.
- **NL prompt crawl contract:** `POST /v1/crawl` now accepts a `prompt` field and forwards it in the orchestrator payload so the Go orchestrator can rank discovered URLs via Model Plane before seeding BFS. Tests: `crawl_request_accepts_nl_prompt`.
- **Structured output schema passthrough:** `ModelPlaneClient::invoke_with_schema` forwards `structured_output_schema` to Model Gateway alongside the text prompt. `json` format requests with a schema now use `invoke_with_schema` so the Model Gateway can apply native JSON-mode or constrained decoding. Tests: `scrape_json_format_with_schema_sends_structured_output_schema_to_model_plane`.

#### ⚠️ Partially Implemented
- **Search parity:** NL ranking via Model Plane (`model-ranked`) now implemented with term-match fallback. Provider-backed SERP/discovery (without `urls`) is still not implemented.
- **Extract parity:** sync single-URL + `structured_output_schema` passthrough are shipped; source-trace, cost-guard, and async multi-URL extraction remain open.
- **NL crawl parity:** `prompt` field wired on edge and forwarded to orchestrator; Go orchestrator-side BFS URL ranking requires Phase 15.5 work.
- **Agent parity:** current route is a sync fetch+answer helper, not a full agent loop.
- **Browser stealth evidence:** baseline stealth and emulation are wired, but benchmark proof is still pending.

#### ❌ Not Implemented
- **Deep research workflow:** explicit Post-V2 deferral
- **Preset creation API:** explicit unsupported response only
- **Documentation / SDK DX:** OpenAPI/Swagger and language SDKs
- **Formats:** audio, plus rendered/visual branding parity beyond the shipped static extractor

### Recommendations

1. `POST /v1/search` now has Model Plane NL ranking. The remaining gap is provider-backed SERP discovery (no `urls` required) — decide if that's a V2 requirement before claiming full Firecrawl parity.
2. Keep `POST /v1/agent` marked reduced until the feature-gated planner/action/event loop from Section 15.7 exists.
3. Implement Go orchestrator-side NL URL ranking using the edge's `ModelPlaneClient::rank_urls` contract to close the crawl `prompt` gap end-to-end.
4. Leave `POST /v1/presets` and `POST /v1/deep-research` explicitly deferred until there is a real control-plane or product contract for them.
5. Continue the remaining Phase 9 parity work: fingerprint matrix evidence, rendered branding hardening if needed, and `audio`.

### Corrected Impact Summary

- The previous 9-endpoint “missing” list is down to **2 explicitly unsupported endpoints** plus **5 reduced-parity surfaces**.
- The main gap is no longer route absence; it is parity depth, product-surface decisions, and measurement evidence.
- Browserbase is no longer the obvious request-configuration gap owner; remaining browser work is benchmark and provider-proof driven.

