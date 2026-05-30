# Quarry vs Firecrawl — Full Analysis
> Generated: 18 February 2026  
> Based on live benchmarks against Quarry (local Docker stack) and Firecrawl v2 public docs/SDK

---

## 1. Executive Summary

| Dimension              | Quarry                            | Firecrawl                       |
|------------------------|-----------------------------------|---------------------------------|
| **Deployment**         | Self-hosted, Docker / Kubernetes  | Managed cloud (also OSS)        |
| **Primary focus**      | E-commerce domain extraction      | General-purpose LLM data prep   |
| **AI integration**     | Native (AI-Core gRPC + HTTP)      | LLM-based extract endpoint      |
| **Scheduling**         | Temporal-native, cron-ready       | Webhook callbacks only          |
| **Change tracking**    | First-class, Redis-persisted      | Format flag (`changeTracking`)  |
| **Async job model**    | Status polling + SSE + webhooks   | Status polling + webhooks       |
| **Auth model**         | API key                           | API key (tier-based quotas)     |
| **Cost**               | Infrastructure cost only          | Per-page credits                |

### Verdict
Quarry is stronger for **self-hosted, domain-specific, AI-enriched, scheduled scraping** with change monitoring. Firecrawl still leads on **managed DX, SDKs, and broader product polish**, but Quarry now closes most of the core `v2` crawl gap with recursive crawl, status/errors/cancel, sitemap seeding, dedupe, subdomain/external controls, and crawl webhooks.

---

## 2. Performance — Live Benchmarks (18 Feb 2026)

All measurements are end-to-end HTTP latency from `localhost` to Docker container.

### 2.1 Infrastructure Endpoints

| Endpoint              | Avg (ms) | Min (ms) | Max (ms) | Notes                                    |
|-----------------------|----------|----------|----------|------------------------------------------|
| `GET /health`         | **4**    | 2        | 14       | Excellent — pure fast-path               |
| `GET /metrics`        | **2**    | 2        | 3        | Sub-millisecond internally               |
| `GET /v1/modules`     | **2**    | 1        | 4        |                                          |
| `GET /ready`          | 758      | 758      | 764      | ⚠ Postgres check adds ~750ms — see §6    |

### 2.2 Discovery

| Endpoint                         | Avg (ms) | Status | Notes                                  |
|----------------------------------|----------|--------|----------------------------------------|
| `POST /v1/map` (limit=10)        | **96**   | 200    | Colly-backed, fast                    |
| `POST /v1/search` (keyword)      | **93**   | 200    | URL-scoped, not global web search     |

> **Firecrawl** `/v2/map` typically ~200–500ms (cloud overhead + validation)  
> **Quarry advantage**: self-hosted latency is ~2–5× lower for map/search.

### 2.3 Scrape

| Scenario                         | Quarry (ms) | Firecrawl estimate (ms) | Notes                                      |
|----------------------------------|-------------|-------------------------|--------------------------------------------|
| Cold (single page, markdown)     | **~8,600**  | ~1,000–3,000            | Quarry hits live network synchronously     |
| Cache warm (Redis hit)           | **~8,600**  | ~300–500                | ⚠ Cache not reducing scrape latency — bug  |
| Colly quick module               | ~26,000     | ~1,000–3,000            | Multi-product discovery; inherently slower |
| Firecrawl `maxAge` cache hit     | n/a         | ~50–200                 | Firecrawl caches aggressively at edge      |

> **Critical finding**: Warm scrape is the same speed as cold. Redis `maxAge` cache is not being hit on the scrape path for the `quick` module. This needs investigation (see §6.1).

### 2.4 Change Tracking

| Endpoint                         | Avg (ms) | Notes                                  |
|----------------------------------|----------|----------------------------------------|
| `POST /v1/change/check` dryRun   | **116**  | Compare only, no Redis write           |
| `POST /v1/change/check` persist  | **95**   | Fetch + hash + Redis write             |
| `GET /v1/change/latest?url=`     | **2**    | Pure Redis read — excellent            |

### 2.5 Async Dispatch

| Endpoint                    | Avg (ms) | Notes                                         |
|-----------------------------|----------|-----------------------------------------------|
| `POST /v1/crawl` dispatch   | ~30      | Temporal workflow dispatch                    |
| `GET /v1/jobs/:id`          | ~3       | Redis/Postgres status read                    |
| `POST /v1/batch` dispatch   | ~20      | Internal batch manager                        |
| `GET /v1/batch/:id`         | ~3       | Batch status read                             |

### 2.6 Agent Mode

| Endpoint       | Avg (ms) | Notes                                                            |
|----------------|----------|------------------------------------------------------------------|
| `POST /v1/agent` | ~800–2000 | AI Core HTTP round-trip + optional scrape; model-dependent    |

### 2.7 Concurrency

| Test                         | Result            | Notes                              |
|------------------------------|-------------------|------------------------------------|
| 20× `/health` parallel       | ~25ms total; ~800 req/s | Fiber handles concurrent well |
| 5× `/v1/map` parallel        | ~200ms total      | All 200, no queue stalling         |

---

## 3. Feature Comparison Matrix

### 3.1 Core Scraping

| Feature                          | Quarry       | Firecrawl    |
|----------------------------------|:------------:|:------------:|
| Single page scrape               | ✅           | ✅           |
| Markdown output                  | ✅           | ✅           |
| HTML / rawHTML output            | ✅           | ✅           |
| JSON structured output           | ✅           | ✅           |
| Links output                     | ✅           | ✅           |
| Screenshot output                | ✅ (Rod)     | ✅           |
| AI page summary format           | ✅           | ✅ (`summary`) |
| Brand identity format            | ❌           | ✅ (`branding`) |
| PDF generation output            | ✅           | ✅ (`pdf`)   |
| PDF parsing (ingest PDFs)        | ❌           | ✅           |
| `onlyMainContent` filter         | ✅           | ✅           |
| `includeTags` / `excludeTags`    | ✅           | ✅           |
| `waitFor` (JS render delay)      | ✅           | ✅           |
| Custom request headers           | ✅           | ✅           |
| `maxAge` cache control           | ✅           | ✅           |
| Block ads                        | ✅           | ✅ (`blockAds`) |
| Mobile device simulation         | ✅           | ✅ (`mobile`) |
| Geo / locale targeting           | ✅           | ✅ (`location: {country, languages}`) |
| `storeInCache` per-request       | ❌           | ✅           |
| `zeroDataRetention` option       | ❌           | ✅           |

### 3.2 Browser Actions

| Action                        | Quarry       | Firecrawl    |
|-------------------------------|:------------:|:------------:|
| `click` (CSS selector)        | ✅           | ✅           |
| `wait` (milliseconds)         | ✅           | ✅           |
| `screenshot`                  | ✅           | ✅           |
| `scroll` (up/down)            | ✅ (via Rod) | ✅           |
| `write` (fill text into input)| ✅           | ✅           |
| `press` (keyboard key)        | ✅           | ✅           |
| `executeJavascript`           | ✅           | ✅           |
| `generatePDF`                 | ✅           | ✅           |
| Screenshot after each action  | ✅ (`screenshotAfter`) | ✅  |
| Retry on action failure       | ✅           | ❌ (not documented) |

### 3.3 Crawling

| Feature                              | Quarry        | Firecrawl    |
|--------------------------------------|:-------------:|:------------:|
| Async crawl with job ID              | ✅            | ✅           |
| Depth control                        | ✅ (`maxDepth`) | ✅ (`maxDiscoveryDepth`) |
| Limit pages                          | ✅            | ✅           |
| Include / exclude paths (regex/glob) | ✅            | ✅           |
| Allow external links                 | ✅            | ✅           |
| Allow subdomains                     | ✅            | ✅           |
| Smart crawl via natural language     | ⚠ preview-first | ✅ (`prompt`) |
| Crawl params preview                 | ✅            | ✅           |
| Cancel crawl                         | ✅            | ✅           |
| Crawl error report endpoint          | ✅            | ✅           |
| Webhook on completion                | ✅            | ✅           |
| Scheduled crawl (cron / time)        | ✅ (Temporal) | ❌           |
| SSE streaming job updates            | ✅            | ❌           |
| Sitemap-guided crawl                 | ✅ (`sitemap: include/only`) | ✅ (`sitemap: include/only`) |
| Deduplication                        | ✅           | ✅           |

### 3.4 Batch Scraping

| Feature                         | Quarry       | Firecrawl    |
|---------------------------------|:------------:|:------------:|
| Submit URL list for batch       | ✅           | ✅           |
| Batch status polling            | ✅           | ✅           |
| Batch error report              | ❌           | ✅           |
| Webhooks on batch completion    | ✅           | ✅           |

### 3.5 URL Discovery / Mapping

| Feature                   | Quarry       | Firecrawl    |
|---------------------------|:------------:|:------------:|
| Map site URLs             | ✅           | ✅           |
| Search-filtered map       | ✅ (limited) | ✅           |
| Include subdomains        | ✅           | ✅           |
| Sitemap-assisted map      | ✅           | ✅           |

### 3.6 Search

| Feature                          | Quarry       | Firecrawl    |
|----------------------------------|:------------:|:------------:|
| URL-scoped keyword search        | ✅           | ✅           |
| Global web search                | ✅           | ✅           |
| News search source               | ✅           | ✅ (`sources: ["news"]`) |
| Image search source              | ✅           | ✅ (`sources: ["images"]`) |
| Search + full content scrape     | ✅           | ✅           |

### 3.7 AI / Data Extraction

| Feature                              | Quarry              | Firecrawl           |
|--------------------------------------|:-------------------:|:-------------------:|
| AI-driven content enrichment         | ✅ (via AI Core)    | ✅ (via LLM)        |
| Schema-based JSON extraction         | ✅ (domain modules) | ✅ (`/v2/extract`)  |
| Free-form prompt extraction          | ✅ (agent mode)     | ✅ (`/v2/extract`)  |
| Web-search-augmented extraction      | ✅ (`enableWebSearch`) | ✅ (`enableWebSearch`) |
| Async extract job with status poll   | ✅                  | ✅                  |
| E-commerce product schema            | ✅ (first-class)    | ❌ (manual prompt)  |
| SEO analysis module                  | ✅                  | ❌                  |
| AI circuit breaker + SLO tracking    | ✅                  | ❌                  |

### 3.8 Change Tracking

| Feature                         | Quarry            | Firecrawl          |
|---------------------------------|:-----------------:|:------------------:|
| Change detection on scrape      | ✅ (Redis + hash) | ✅ (format flag)   |
| Persist baseline                | ✅                | ❌ (no persistence)|
| Dry-run compare (no persist)    | ✅                | ❌                 |
| On-demand change check          | ✅ `/v1/change/check` | ❌             |
| Retrieve latest snapshot        | ✅ `/v1/change/latest` | ❌            |
| Configurable TTL per collection | ✅                | ❌                 |

### 3.9 Security & Stealth

| Feature                         | Quarry                 | Firecrawl          |
|---------------------------------|:----------------------:|:------------------:|
| Stealth / anti-bot mode         | ✅ (Rod stealth)       | ✅ (`proxy: stealth`) |
| Proxy pool rotation             | ✅ (internal pool)     | ✅ (managed proxies)|
| Proxy type selection            | ❌ (pool only)         | ✅ (`basic/stealth/enhanced/auto`) |
| Human delay simulation          | ✅ (middleware)        | ❌                 |
| TLS certificate ignore          | ✅                     | ✅                 |
| API key auth                    | ✅                     | ✅                 |
| Rate limiting                   | ✅ (configurable)      | ✅ (tier-based)    |
| IP-based + key-based rate limit | ✅                     | ✅ (key-based)     |

### 3.10 Observability

| Feature                         | Quarry                 | Firecrawl          |
|---------------------------------|:----------------------:|:------------------:|
| Health check                    | ✅                     | ❌ (no public /health) |
| Readiness check                 | ✅                     | ❌                 |
| Metrics dashboard               | ✅ (`/metrics`)        | ❌                 |
| Prometheus-compatible metrics   | ✅                     | ❌                 |
| AI circuit breaker state        | ✅                     | ❌                 |
| Request ID tracing              | ✅                     | ❌                 |
| SSE job streaming               | ✅                     | ❌                 |

### 3.11 Developer Experience

| Feature                         | Quarry                 | Firecrawl          |
|---------------------------------|:----------------------:|:------------------:|
| OpenAPI / Swagger docs          | ❌                     | ✅                 |
| Official SDKs (JS/Python)       | ❌                     | ✅                 |
| CLI tool                        | ❌                     | ✅                 |
| Playground / dashboard UI       | ❌                     | ✅                 |
| Error messages (descriptive)    | ✅ (good)              | ✅                 |
| Docker compose one-liner        | ✅                     | ✅ (OSS version)   |
| Kubernetes manifests            | ✅ (in docs)           | ✅                 |
| Public changelog                | ❌                     | ✅                 |

---

## 4. Use Case Analysis

### 4.1 Price/Inventory Monitoring (E-commerce)
| Step                            | Quarry               | Firecrawl             |
|---------------------------------|----------------------|-----------------------|
| Discover product URLs           | ✅ `/v1/map`         | ✅ `/v2/map`          |
| Scheduled scrape                | ✅ Temporal          | ⚠ Must build externally|
| Product structured extraction   | ✅ native schema     | ✅ via prompt/schema  |
| Detect price changes            | ✅ `/v1/change/check`| ⚠ Stateless only      |
| Alert on change                 | ✅ webhooks          | ✅ webhook callbacks  |
| **Winner**                      | **Quarry** (scheduling + change persistence) | **Firecrawl** (managed delivery) |

### 4.2 LLM Dataset Collection (Web Scraping at Scale)
| Step                            | Quarry               | Firecrawl             |
|---------------------------------|----------------------|-----------------------|
| Multi-page crawl                | ✅                   | ✅ (smarter)          |
| Clean markdown output           | ✅                   | ✅ (better quality)   |
| AI-powered summarization        | ❌                   | ✅ (native summary)   |
| Schema-guided extraction        | ✅ `/v2/extract`    | ✅ `/v2/extract`      |
| Batch throughput                | ✅                   | ✅                    |
| **Winner**                      | **Firecrawl**        |                       |

### 4.3 SEO Audit / Site Analysis
| Step                            | Quarry               | Firecrawl             |
|---------------------------------|----------------------|-----------------------|
| Site mapping                    | ✅                   | ✅                    |
| SEO extraction                  | ✅ (`seo` module)    | ⚠ Manual prompt       |
| Structured SEO output           | ✅                   | ❌                    |
| Link analysis                   | ✅                   | ✅                    |
| **Winner**                      | **Quarry**           |                       |

### 4.4 Research & Intelligence Automation
| Step                            | Quarry               | Firecrawl             |
|---------------------------------|----------------------|-----------------------|
| Web search                      | ❌                   | ✅                    |
| News monitoring                 | ❌                   | ✅                    |
| Multi-source research           | ❌                   | ✅                    |
| AI-guided goal extraction       | ✅ (agent mode)      | ✅ (extract + search) |
| **Winner**                      | **Firecrawl**        |                       |

### 4.5 Content Change Monitoring (scheduled)
| Step                            | Quarry               | Firecrawl             |
|---------------------------------|----------------------|-----------------------|
| Snapshot baseline               | ✅                   | ❌                    |
| Periodic re-check               | ✅ (Temporal)        | ❌ (manual cron)      |
| Diff/compare                    | ✅                   | ⚠ Basic flag          |
| Persist history                 | ✅ (Redis, TTL)      | ❌                    |
| **Winner**                      | **Quarry**           |                       |

### 4.6 JS-Heavy SPA Scraping with Browser Automation
| Step                            | Quarry               | Firecrawl             |
|---------------------------------|----------------------|-----------------------|
| Navigate and click              | ✅                   | ✅                    |
| Type into inputs / forms        | ❌                   | ✅                    |
| Execute arbitrary JS            | ❌                   | ✅                    |
| Generate PDF from page          | ❌                   | ✅                    |
| **Winner**                      | **Firecrawl**        |                       |

---

## 5. What Quarry Does Better

1. **Scheduled crawling** — Temporal-native scheduling, supports `scheduleAt` on any crawl. No external cron needed.
2. **Change tracking persistence** — First-class feature with Redis, TTL control, dry-run mode, and baseline snapshots. Firecrawl change tracking is stateless.
3. **Domain-specific modules** — `quick`, `multi`, and `seo` modules are purpose-built for targeted extraction patterns; no prompt engineering required for e-commerce use cases.
4. **E-commerce product schema** — SKU, price, stock, ingredients, use-case fields structured natively.
5. **AI enrichment pipeline** — Integrated `enrich` mode via AI-Core for post-scrape LLM enrichment.
6. **Observability stack** — Prometheus metrics, AI circuit breaker, SLO tracking, SSE job streaming, and request tracing are built in.
7. **Self-hosted cost model** — No per-credit billing. Infrastructure cost scales linearly.
8. **Human delay simulation** — Configurable per-request jitter makes scraping harder to detect.
9. **Agent Mode with goal state** — Free-form instruction scraping via AI Core using `/v1/agent`.
10. **On-demand change checks** — `POST /v1/change/check` with `dryRun` support.

---

## 6. What Needs to Be Fixed (Critical)

### 6.1 Scrape Cache Is Not Reducing Latency
**Problem:** Cache warm scrape (`maxAge` set, Redis configured) returns in ~8,600ms — same as cold. The `quick` module is re-fetching live on every request.  
**Impact:** High. Makes the system unusable for high-frequency scraping.  
**Fix:** Trace the scrape path in `internal/api/scrape.go` → module handler → verify Redis cache key construction and TTL lookup before dispatching to Colly/Rod.

### 6.2 `/ready` Endpoint — Postgres Validation Produces 503
**Problem:** `postgres: invalid config` appears in every readiness check, causing HTTP 503.  
**Impact:** Medium. Kubernetes readiness gates will never pass; load balancers may skip the pod.  
**Fix:** Validate the Postgres DSN at startup; if not configured, report `"postgres": "not configured"` and return 200 with `"degraded"` status rather than 503.

### 6.3 `/v1/change/latest` Requires Mandatory `?url=` Param
**Problem:** Without a `url` or `collection` param, the endpoint returns 400 with no useful info.  
**Impact:** Low-Medium from usability perspective.  
**Fix:** Either (a) allow listing recent snapshots when no param provided, or (b) return a clearer 400 describing the missing parameter.

### 6.4 Crawl Cancellation Exists, But Runtime Interruption Can Be Hardened
**Current State:** Quarry supports `DELETE /v1/crawl/:id` and stops scheduling new pages while letting in-flight work drain.  
**Remaining Gap:** Cancellation telemetry and deeper worker-level interruption can still be improved.

---

## 7. What Needs to Be Added (Must-Have for Competitiveness)

### 7.1 Browser Action: `write`, `press`, `executeJavascript`
Missing actions prevent form automation and dynamic JS interaction.  
```json
{ "type": "write", "selector": "#search", "text": "query" }
{ "type": "press", "key": "Enter" }
{ "type": "executeJavascript", "script": "window.scrollTo(0, document.body.scrollHeight)" }
```

### 7.2 Webhook Coverage
Quarry now supports per-request webhooks for crawl, batch, `/v2/extract`, and async `/v2/search` completion/failure. The remaining gap is consistency rather than existence:
- no per-event filtering on `/v2/extract`
- webhook payloads are simpler than Firecrawl's richer async job payloads

### 7.3 AI Extract Endpoint (`/v2/extract`)
Quarry now has a Firecrawl-style async `/v2/extract` flow with:
- JSON schema objects or JSON-encoded schema strings
- prompt-driven extraction with async job polling
- `systemPrompt`
- `enableWebSearch`
- `ignoreInvalidURLs`
- crawl-backed wildcard URL discovery for `/*`
- extract-local discovery controls (`ignoreSitemap`, `sitemap`, `includePaths`, `excludePaths`, `includeSubdomains`, `allowExternalLinks`)
- `urlTrace` + `invalidURLs` in the create response
- completion/failure webhooks

The remaining gaps versus Firecrawl are narrower:
- async `/v2/search` now uses a dedicated paginated result store with memory, Redis, and Postgres backends, but its webhook/status payloads are still simpler than Firecrawl's richer search job schema
- `systemPrompt` is currently composed into the extract prompt at the Quarry boundary rather than passed as a first-class ai-core field

```json
POST /v2/extract
{
  "urls": ["https://example.com/products/*"],
  "systemPrompt": "Return strict JSON only",
  "prompt": "Extract product name, price, and availability",
  "schema": { "name": "string", "price": "number", "inStock": "boolean" },
  "sitemap": "only",
  "includePaths": ["/products/*"],
  "excludePaths": ["/products/archive/*"],
  "includeSubdomains": false
}
```

### 7.4 Crawl Path Filters (Include/Exclude)
Quarry already supports include/exclude path filters in crawl and extract discovery. The remaining gap is mostly API polish and preset surfacing rather than raw capability.
```json
{
  "url": "https://example.com",
  "includePaths": ["/products/**"],
  "excludePaths": ["/cart", "/checkout"]
}
```

### 7.5 Smart Crawl with Natural Language Prompt
Quarry now supports prompt-merged crawl planning directly in `POST /v1/crawl`, with `/v1/crawl/params-preview` retained as the inspect/debug endpoint. The remaining gap is tuning prompt-to-filter accuracy and showing richer resolved options.
```json
{
  "url": "https://example.com",
  "prompt": "Only crawl product pages"
}
```

### 7.6 Document Parsing And OCR
Quarry now supports `pdf`, `docx`, and `xlsx` document parsing through scrape formats, plus parser modes:
- `fast`: native text extraction only
- `auto`: native text first, fall back to AI-core OCR for scanned PDFs
- `ocr`: force OCR through AI-core document analysis

This closes the biggest scanned-PDF parsing gap. The remaining document gap is broader structured-document specialization, not baseline OCR/text recovery.

### 7.7 Typed Analyzer Outputs
Quarry now exposes typed `branding`, `seo`, `wcag`, and `pageStatus` outputs as scrape/crawl formats. The remaining gap is depth and scoring sophistication, not endpoint availability.

### 7.8 OpenAPI/Swagger Spec + Auto-Generated Docs
All routes are Fiber-registered but not documented in an OpenAPI spec. Adding `fiber-swagger` or generating from structs would dramatically improve adoption.

---

## 8. Nice-to-Have (Roadmap Candidates)

| Feature                              | Priority | Notes                                                          |
|--------------------------------------|----------|----------------------------------------------------------------|
| `blockAds` scrape option             | P1       | Reduce noise in markdown output                               |
| Mobile device simulation             | P1       | Many sites serve different content to mobile UAs              |
| Geo/locale targeting                 | P2       | Regional pricing / content differs                            |
| Additional search backends           | P2       | Current `/v1/search` already supports Brave, GitHub, and retrieval-backed sources |
| Source-specific ranking policies     | P3       | Current `/v1/search` supports weights, per-source caps, ranked/interleave blending, and URL dedupe |
| Official Python SDK                  | P1       | 10-line integration from any AI agent framework               |
| Official JS/TS SDK                   | P2       | Browser/Node.js consumers                                     |
| CLI tool                             | P2       | `quarry scrape https://...` one-liner for ops                 |
| Sitemap-guided crawl                 | P1       | Faster discovery for large sites                              |
| URL deduplication in crawl           | P1       | Prevents re-scraping already visited pages                    |
| Batch error report endpoint          | P2       | `GET /v1/batch/:id/errors`                                    |
| `allowSubdomains` crawl option       | P2       |                                                               |
| `storeInCache` per-request flag      | P2       | Give callers control over caching                             |
| Proxy type selection per request     | P2       | `basic/stealth/enhanced` per-call                             |
| `summary` format output              | P1       | AI-generated page summary from AI Core                        |
| `branding` format output             | P3       | Design identity extraction                                    |
| Crawl params dry-run preview         | P2       | Let users preview what paths will be crawled                  |
| Web-search augmented extraction      | P2       | Complement agent mode                                         |
| Temporal UI integration for jobs     | P2       | Surface `/v1/jobs` state in Temporal UI                       |
| Async extract job (`/v1/extract`)    | P1       | Firecrawl's most-used endpoint next to scrape                 |

---

## 9. Ease of Use Comparison

### Getting Started
| Task                             | Quarry             | Firecrawl          |
|----------------------------------|--------------------|--------------------|
| First scrape in under 2 min      | ❌ Docker required | ✅ API key + curl  |
| Find available parameters        | ❌ Read source     | ✅ Swagger UI      |
| SDK install                      | ❌ Not available   | ✅ `npm i @mendable/firecrawl-js` |
| Working code example             | ✅ test-endpoints.sh | ✅ docs + playground |
| Error messages                   | ✅ Descriptive JSON | ✅ Descriptive JSON |

### API Design
| Aspect                           | Quarry             | Firecrawl          |
|----------------------------------|--------------------|--------------------|
| Consistent HTTP verbs            | ✅                 | ✅                 |
| Versioned API (`/v1`)            | ✅                 | ✅ (`/v2`)         |
| Predictable response envelope    | ✅ `{success, data}` | ✅ `{success, data}` |
| Job ID in dispatch response      | ✅                 | ✅                 |
| Pagination on long lists         | ❌                 | ✅                 |

---

## 10. Automation & Integration

| Capability                       | Quarry             | Firecrawl          |
|----------------------------------|--------------------|--------------------|
| Temporal workflow engine         | ✅ native          | ❌                 |
| Webhook on completion            | ❌                 | ✅                 |
| Make / Zapier integration        | ❌                 | ✅ (official)      |
| n8n node                         | ❌                 | ✅ (community)     |
| LangChain / LlamaIndex loaders   | ❌                 | ✅                 |
| gRPC integration                 | ✅ (AI Core)       | ❌                 |
| NATS pub/sub                     | ✅ (configured)    | ❌                 |
| Redis caching                    | ✅ native          | ✅ (edge cache)    |
| Scheduled crawls                 | ✅ Temporal        | ❌ (DIY cron)      |

---

## 11. Priority Roadmap (Recommended Order)

```
P0  — Fix bugs that break existing behaviour
P1  — Parity with Firecrawl on most-needed features
P2  — Differentiation + polish
P3  — Nice-to-have extras
```

| # | Item                                   | Priority | Effort |
|---|----------------------------------------|----------|--------|
| 1 | Fix scrape cache not reducing latency  | P0       | M      |
| 2 | Fix `/ready` returning 503 on postgres | P0       | S      |
| 3 | Browser: `write`, `press`, `executeJS` actions | P1 | M  |
| 4 | Webhook callbacks for crawl/batch      | P1       | M      |
| 5 | `/v1/extract` with JSON schema prompt  | P1       | L      |
| 6 | Include/exclude path filters on crawl  | P1       | S      |
| 7 | `blockAds` scrape option               | P1       | S      |
| 8 | Global web search (SerpAPI/Brave)      | P1       | M      |
| 9 | `summary` format via AI Core           | P1       | S      |
| 10| Crawl cancellation hardening and telemetry | P2    | S      |
| 11| OpenAPI spec + Swagger UI              | P1       | M      |
| 12| Mobile device simulation               | P1       | S      |
| 13| Sitemap-guided crawl                   | P1       | M      |
| 14| URL deduplication in crawl             | P1       | S      |
| 15| Official Python SDK                    | P2       | L      |
| 16| Smart crawl via NL prompt tuning       | P2       | M      |
| 17| Advanced document intelligence beyond OCR | P3    | M      |
| 18| Geo/locale targeting                   | P2       | M      |
| 19| Async extract job + polling            | P2       | M      |
| 20| Batch error report endpoint            | P2       | S      |
| 21| CLI tool (`quarry scrape`)             | P2       | L      |
| 22| Proxy type per-request                 | P2       | S      |

---

## 12. What Quarry Can Own as a Moat

These Quarry capabilities are genuinely differentiated vs Firecrawl and should be deepened, not just maintained:

1. **Temporal-native scheduling** — Cron and `scheduleAt` scraping with retry + fault tolerance. No competitor has this natively.
2. **Domain module system** — `quick`, `multi`, `seo` modules with their own enrichment pipelines. Add `news`, `jobs`, `real-estate` modules.
3. **Change tracking with history** — Persist baselines, diff trends, TTL rotation. Build on this for alert rules.
4. **AI Core gRPC integration** — First-class enrichment, classification, summarisation as a post-scrape step. No SaaS scraper does this.
5. **Self-hosted with zero per-page billing** — The Firecrawl OSS version requires setup; Quarry is Docker-compose-ready.
6. **Observability** — Circuit breaker + SLO + Prometheus is not available in Firecrawl. Lean into this for enterprise users.
