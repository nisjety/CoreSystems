# Quarry-v2 Features (Plain English)

A high-level map of what Quarry-v2 actually *does* for users — what each feature
is for, who in the market does the same thing, and which internal pieces make
it work. No code, no Rust types — just the story.

---

## What Quarry-v2 is

A self-hosted ingestion engine that turns the open web into clean,
structured, search-ready content for downstream agents and search products.
Think of it as the layer that goes out, fetches, watches, and remembers the
web on your behalf.

## The feature surface

### 1. Web scrape — single page → clean content
Give it a URL, get back the readable article in Markdown plus title, author,
publish date, language, citations, and a content fingerprint.
- **Parity with:** Firecrawl `/scrape`, Jina Reader, Apify single-page actor.
- **Powered by:** `quarry-runtime` driver selector → static fetcher
  (`rquest`/`wreq` with JA3/JA4 TLS fingerprinting) or browser lease →
  `quarry-transform` readability + metadata + language detection.

### 2. Web crawl — whole site → corpus
Pass a domain or seed URL, get a bounded crawl with robots.txt respect,
sitemap discovery, depth/page caps, and per-URL artifacts.
- **Parity with:** Firecrawl `/crawl`, Apify Website Content Crawler.
- **Powered by:** `quarry-runtime/crawl_frontier`, `robots_cache`, sitemap
  parser, plus Temporal-driven long-running orchestration in
  `quarry-orchestrator`.

### 3. Web search — query → ranked results
Ask a natural-language question, get back URLs + snippets + relevance,
optionally with a synthesized answer.
- **Parity with:** Tavily, Brave Search API, Exa neural search.
- **Powered by:** dedicated search driver in `quarry-runtime` + Brave/Bing
  API plug-ins; reranker can call **Model Plane v1**'s gateway for
  semantic reordering.

### 4. Structured extraction — page + schema → typed JSON
Hand over a URL and a JSON schema; get back JSON that matches it (e.g.
"give me the product name, price, and stock from this page").
- **Parity with:** Firecrawl `/extract`, Browse AI, Diffbot.
- **Powered by:** scrape pipeline → **Model Plane v1** gateway calls a
  schema-coerced LLM via Azure OpenAI; coercion failures retry with
  repair prompts.

### 5. Deep research — question → cited dossier
Long-running, multi-step research: plan → fetch → search → verify →
synthesize, with citations.
- **Parity with:** Perplexity Pro, Exa Research, Manus.ai.
- **Powered by:** `quarry-orchestrator` Temporal workflow drives the loop;
  **Model Plane v1** runs the agent + each tool step; Quarry runtime
  executes the actual fetches and searches.

### 6. Headless browser automation — JS-rendered pages
For sites that need real browsers (SPAs, anti-bot walls, login flows):
spin up a stealthed Chromium session with cookies, session affinity, and
action playback.
- **Parity with:** Browserbase, Browserless, Playwright Grid.
- **Powered by:** `quarry-browser` lease pool (host scheduler, kernel,
  driver); optionally delegates to **Browserbase** when the operator
  configures an external pool. Stealth profiles live in
  `quarry-runtime/stealth.rs` (fingerprint randomization, UA, viewport,
  TLS shape via `quarry-tls`).

### 7. Change tracking — diff between visits
Crawl the same URL on a schedule, get notified when content meaningfully
changes (ignoring boilerplate, ads, dates).
- **Parity with:** Diffbot Knowledge Graph monitor, Visualping, ChangeDetect.
- **Powered by:** `quarry-transform/fingerprint` + `diff`; Temporal
  schedules; webhook fan-out.

### 8. Schedules — recurring crawls
Cron-style or interval-based recurring ingestion jobs with retry, backoff,
SLAs, and pause/resume.
- **Parity with:** Apify schedules, Firecrawl scheduled jobs.
- **Powered by:** **Temporal** workflows + cron triggers inside
  `quarry-orchestrator`.

### 9. Webhooks — push on every event
Real-time push of crawl events, change-detect events, job-lifecycle events,
with HMAC-signed delivery and replay.
- **Parity with:** Stripe webhooks, GitHub webhooks.
- **Powered by:** `quarry-control` webhook registry + outbox; per-org HMAC
  secrets; retry queue with backoff.

### 10. Live event stream — SSE/WebSocket
Subscribe to a job-in-progress and watch pages arrive page-by-page (used
by Verevon's onboarding live crawl).
- **Parity with:** Vercel Live, Sanity Live Content, Convex subscriptions.
- **Powered by:** `quarry-edge` SSE handler + **NATS** internal event bus;
  no polling.

### 11. Profiles — reusable crawl configurations
Save a named set of options (depth, selectors, schedule, auth, geo) and
reuse it across jobs.
- **Parity with:** Apify Actor inputs, Firecrawl crawl options templates.
- **Powered by:** `quarry-runtime/cached_profile_store` (memory layer)
  backed by **S3** + Postgres in `quarry-control`.

### 12. Tenant search index — search what you've crawled
Every ingested page lands in a per-tenant search index so agents can ask
"find me what we know about X" without re-crawling. Two tiers:
- **Persistent tenant index** — crawled pages are published to **Data Plane v2**'s
  NATS pipeline; the **Quickwit** adapter
  (`apps/Data Plane v2/services/quickwit-adapter-rs`) indexes them into
  `dataplane-corpus-index`. The same Quickwit cluster will also host
  SharePoint (finspo-core, planned), so agents query one search plane
  across all sources.
  - *Parity with:* Elasticsearch/OpenSearch as the company search plane;
    Algolia/Meilisearch as managed alternatives.
- **Ephemeral per-session scratch index** — for a deep-research session
  (200 pages crawled, queried/reranked a few times, discarded), Quarry
  builds an in-process **Tantivy** index via
  `quarry-runtime/local_index.rs`. No network round-trip, sub-ms search,
  thrown away at session end.
  - *Parity with:* Lucene-in-process; Whoosh; the embedded-index pattern
    used by tools like ripgrep-all and Sourcegraph's smart-indexer.

### 13. PDF & rich-media ingestion
PDFs get parsed for headings, body text, metadata, and language; same
pipeline downstream as HTML.
- **Parity with:** Unstructured.io, Tika.
- **Powered by:** `quarry-transform/pdf.rs` extractor + language detection.

### 14. Anti-bot / TLS impersonation
Convincing real-browser TLS handshakes (JA3/JA4), realistic header order,
HTTP/2 and HTTP/3 with proper ALPN — sites that block `curl`/`requests`
still serve content.
- **Parity with:** ScrapingAnt, ScrapingBee, Bright Data Scraping Browser.
- **Powered by:** `quarry-tls` (`rquest`/`wreq` upstream — same engine
  TLS-Client-Identifier libraries use), `quarry-runtime/stealth.rs`.

### 15. Multi-tenant safety — SSRF guard + policy
Every URL passes a security preflight (no localhost, no IPv6 ULA, no AWS
metadata) and a tenant policy (allowed domains, max cost, depth caps).
- **Parity with:** Tavily request-isolation, Firecrawl per-tenant limits.
- **Powered by:** `quarry-security` preflight + discovered-URL re-check;
  per-tenant budgets in `quarry-runtime/policy`.

### 16. Audit trail / event history
Every fetch, browser action, transform, and emit is recorded with org +
URL + timing + outcome — replayable and queryable.
- **Parity with:** Snowplow, Segment server-side, Datadog APM.
- **Powered by:** `quarry-runtime/postgres_event_history` + NATS subjects
  consumed by `quarry-orchestrator` workflows; long-term storage in
  Postgres.

### 17. Public REST + GraphQL contract
Stable, versioned HTTP surface (REST resources + GraphQL for richer reads)
that Verevon and external clients talk to.
- **Parity with:** Firecrawl API, Apify API.
- **Powered by:** `quarry-edge` (axum) for REST + SSE, `quarry-control`
  (Go + `async-graphql`) for the typed GraphQL surface.

---

## How the layers fit together

| Layer | Lang | Role | Examples of who else does this |
|---|---|---|---|
| `quarry-edge` | Rust | Public ingest + SSE + cache | Cloudflare Workers, Vercel Edge |
| `quarry-runtime` | Rust | Fetch + browser + transform + artifacts | Firecrawl worker, Apify Actor runtime |
| `quarry-browser` | Rust | Browser lease pool, session affinity | Browserbase, Browserless |
| `quarry-tls` | Rust | JA3/JA4 TLS impersonation | TLS-Client-Identifier, curl-impersonate |
| `quarry-transform` | Rust | Readability, fingerprint, markdown | Mozilla Readability, Trafilatura |
| `quarry-security` | Rust | SSRF + policy preflight | aws-vault, Tavily preflight |
| `quarry-control` | Go | Jobs/stores/snapshots/webhooks/CRUD | Apify Console API, Firecrawl Dashboard API |
| `quarry-orchestrator` | Go | Temporal workflows, schedules | Trigger.dev, Inngest, Temporal directly |
| `lab/` | Python | Extraction/evasion experiments only | OpenAI Evals, Inspect AI |

## External services Quarry-v2 can lean on

| Service | What it gives Quarry-v2 |
|---|---|
| **Model Plane v1** (our Rust LLM gateway) | LLM coercion for structured extraction, deep-research planning, reranking. |
| **Browserbase** | Managed headless browsers when local browser leases are constrained. |
| **Tantivy** (embedded) | Per-tenant search index. |
| **Temporal** | Durable workflow orchestration, schedules, retries. |
| **NATS** | Internal event bus between Rust runtime ↔ Go orchestrator ↔ Verevon. |
| **S3 / MinIO** | Artifact storage (raw HTML, screenshots, PDFs, profiles). |
| **Postgres** | Jobs, stores, profiles, event history. |
| **Brave / Bing / Tavily** | Pluggable web-search backends. |
| **Firecrawl** (optional adapter) | Compatibility, migration, and benchmark target only by default; not a GDPR/default scraping fallback. |

## Where to read more

- Parity tracking: [`TAVILY_PARITY.md`](TAVILY_PARITY.md),
  [`QUARRY_V2_MODEL_PLANE_PARITY.md`](QUARRY_V2_MODEL_PLANE_PARITY.md).
- Architecture: [`ARCHITECTURE.md`](ARCHITECTURE.md).
- Contracts (REST + GraphQL): [`CONTRACTS.md`](CONTRACTS.md),
  [`openapi.yaml`](openapi.yaml).
- Driver matrix (which engine handles what): [`DRIVER_MATRIX.md`](DRIVER_MATRIX.md).
- Roadmap: [`ROADMAP.md`](ROADMAP.md).
- Future self-owned reliability layer:
  [`SOVEREIGN_SCRAPING_RELIABILITY.md`](SOVEREIGN_SCRAPING_RELIABILITY.md).
