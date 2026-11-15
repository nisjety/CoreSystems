# Firecrawl engine and infrastructure: deep dive for Quarry v2 and Verevon

Audit 2, facet "engine-and-infra", source "Firecrawl" (C:/dev/firecrawl, HEAD 11e9dc5e, 2026-10-12). Read-only analysis. Companion file: firecrawl.engine-and-infra.features.json (109 records, ids firecrawl-engine-and-infra-001 to -109).

Provenance tags used below: SRC = read in source or config; DOC = documentation (README, SELF_HOST.md, code comments treated as docs, or the t2 Verevon baseline audit); INF = my inference, basis stated; GIT = read-only git history. Confidence is stated where it is not "high".

Path shorthand: `api/` = apps/api/src, `native/` = apps/api/native/src, `pw/` = apps/playwright-service-ts, `apps/` = repo apps directory. Line numbers refer to the checked-out tree.

---

## 0. Scope, method and honest limits

What I read: the scrape pipeline (`api/scraper/scrapeURL/**`), every engine in the repo, the Playwright microservice, the Rust napi crate, the Go HTML-to-Markdown service, the NuQ queue (Postgres schema and TypeScript client, FoundationDB backend headers, router), crawl state and crawler, concurrency and rate limiting, the index cache, ZDR, tracing, logging, SIEM, Safe Mode, threat protection, monitors, Hangar and FirePDF clients, billing calculation, compose and Dockerfiles, CI workflows, test infrastructure. I used the scout inventory (firecrawl.inventory.md) as a map and re-verified everything I rely on; corrections are in section 12.

What I did not do: run, build or test anything; read all 373 test files; read the closed services (fire-engine, FirePDF, Hangar, Exchange, smart-scrape, avgrab, fire-privacy, the index database and its SQL functions). Where a behaviour lives in a closed service I only know the contract the client code shows, and I say so.

Verevon side: I used the t2 baseline audit (v-quarry.baseline.md) as the description of Quarry v2 and spot-checked four of its claims in code: the stealth script is not referenced outside its own module, `execute_with_retry` is only re-exported (no caller), `FallbackDriver` with `needs_browser_escalation` exists, and `host_scheduler.rs` (577 lines) and `autoscale.rs` (165 lines) exist [SRC]. Everything else about Quarry is DOC (the baseline) and marked as such.

License caution [SRC R:LICENSE, apps/python-sdk/pyproject.toml per scout]: the API is AGPL-3.0, the Python SDK is MIT. Reusing Firecrawl source in Verevon would carry AGPL obligations. This document recommends re-implementing designs from observed behaviour, not copying code. That is an engineering caution, not legal advice.

---

## 1. Executive summary

1. The open-source repo is an engine skeleton plus orchestration. Real anti-bot rendering, the URL cache index, large-PDF OCR, hosted browsers, provider exchange and the agent runtime are closed services reached by URL (fire-engine, index DB, FirePDF, Hangar, Exchange). Parity should be measured against those contracts, not against the OSS Playwright service [SRC `api/scraper/scrapeURL/engines/index.ts:71-105`, `R:SELF_HOST.md:28-34`].
2. The most valuable ideas are in the orchestration layer, not in any single engine: a feature-scored, hedged engine waterfall; typed "add or remove feature" re-plan signals with bounded budgets; file-type handoff by Content-Type and magic bytes; and a result gate that only rejects empty 2xx pages [SRC `scrapeURL/index.ts:663-1106`, `engines/utils/specialtyHandler.ts:147-393`].
3. Caching is a first-class subsystem: raw HTML in object storage, metadata in a separate Postgres with hashed URL and domain-split columns, a fail-fast Redis LRU in front, negative caching, variant keys (mobile, ads, stealth, country, languages), freshness vocabulary (maxAge, minAge, lockdown) and a strict admission policy that never caches personalised or lower-quality results [SRC `engines/index/index.ts`, `services/index-cache.ts`].
4. Zero data retention is engineered across process boundaries: an OpenTelemetry sampler that makes ZDR spans unrecorded (including library spans), a log filter, a propagated header to helper services, monotonic retention on sessions, scrubbed run-history rows and a 24-hour delayed deletion queue for results kept for async retrieval [SRC `lib/otel-tracer.ts:35-140`, `lib/zdr-queue.ts`, `lib/browser-lifecycle.ts:66-140`]. This is the strongest transferable discipline for Verevon.
5. Compliance is modelled as an organisation-enforced profile (Safe Mode) with a worker-side backstop, plus lockdown (cache-only, zero outbound), URL threat protection with a no-persistence boundary and a per-org SIEM exporter. Requests can never widen what the org allows [SRC `lib/safe-mode.ts`, `lib/threat-protection/index.ts`, `lib/siem-logging/*`].
6. The queue is Postgres (NuQ) tuned hard, with a RabbitMQ prefetch layer and an experimental FoundationDB backend that moves concurrency gates into the queue. Synchronous scrapes bypass the queue entirely behind a Redis lease semaphore [SRC `apps/nuq-postgres/nuq.sql`, `api/services/worker/nuq.ts`, `api/controllers/v2/scrape.ts:340-460`].
7. Weak spots are real and instructive: crawl politeness is coarse and robots Crawl-delay is deliberately off; crawl state is Redis with a 24 h TTL; error typing from fire-engine is substring matching; webhooks have no visible retry; admin access is a secret in the URL path; the Playwright SSRF check resolves twice; CORS is open; the OpenAPI files drift from code [SRC, sections 3 to 8].
8. Quarry v2 is ahead of Firecrawl on SSRF (resolve-once-and-pin), per-host adaptive politeness (AIMD), receipts and step approvals, takeover, durable workflows, TLS impersonation in the open tier and webhook retry; it is behind on caching, crawl scope enforcement, robots enforcement, format and document coverage, change-detection quality, eval and observability wiring, and org policy tiers [DOC baseline plus SRC here].
9. Several of the best Firecrawl features are recent: Safe Mode (2026-09-15), ZDR cleanup on RabbitMQ (2026-09-16), the Hangar migration (2026-09-25), prompt-injection check (2026-08-26), SIEM (2026-07-27), threat protection (2026-07-07), FoundationDB queue (2026-06-12), deterministic JSON (2026-06-04), async FirePDF (2026-05-14), monitors (2026-05-06) [GIT]. All of these postdate the Verevon Firecrawl notes (OSS_PARITY_BACKLOG 2026-05-30, SCRAPING_ENGINE_GAPS 2026-05-28) and most postdate GAP.md (2026-08-06).
10. Ranked recommendations are in section 14. The top five: a URL-keyed cache with admission policy; typed escalation signals and a capability matrix in the driver layer; wiring the existing crawl frontier, robots cache and sitemap seeding; a ZDR-aware tracing and retention discipline; and a hermetic test site with record/replay and a post-deploy eval hook.

---

## 2. Architecture map

### 2.1 Services and what is open

| Service | In repo? | Role | Evidence |
|---|---|---|---|
| API plus workers (Node, Express 5) | yes | HTTP API, scrape pipeline, crawl logic, queues, billing hooks | `api/index.ts`, `api/services/worker/*` |
| NuQ Postgres | yes (schema, Dockerfile) | Job queue, groups, backlog; pg_cron for reaping and cleanup | `apps/nuq-postgres/nuq.sql` |
| FoundationDB queue backend | yes (client code), experimental | Queue with slot gating | `api/services/worker/nuq-fdb/*`, `R:docker-compose.yaml` |
| Playwright microservice | yes | Minimal Chromium renderer | `pw/api.ts` |
| Go html-to-md service and c-shared lib | yes | Markdown conversion | `apps/go-html-to-md-service`, `api/lib/html-to-markdown.ts` |
| Rust napi crate | yes | PDF classify and extract, HTML transform, crawl filters, sitemap parsing, engpicker verdict | `native/*.rs` |
| fire-engine | no (client only) | Stealth Chrome (CDP), TLS client, search, geo proxies | `api/scraper/scrapeURL/engines/fire-engine/*` |
| FirePDF, MinerU on RunPod | no (clients) | OCR and layout for PDFs and images | `.../engines/pdf/*` |
| Hangar | no (client) | Hosted browser sessions, profiles, execution sandbox, live view, recording | `api/lib/hangar.ts` |
| URL index (Postgres + GCS + Dragonfly) | partially (schema, client) | Cross-tenant URL content cache | `api/services/index.ts`, `api/db/schema/index-db.ts` |
| Exchange / Alexandria, fire-privacy, smart-scrape, avgrab, agent service | no (clients) | Provider tools, PII redaction, interaction agent, media download | various |
| Redis, RabbitMQ | external | Locks, caches, crawl state, rate limits; prefetch, webhooks, ZDR delay, SIEM | `R:docker-compose.yaml` |
| Bigtable, ClickHouse, Pub/Sub, GCS | hosted-only | Run history, analytics, credit shards, blobs | `api/services/logging/log_job.ts` |

### 2.2 Scrape lifecycle (single URL)

```
HTTP /v2/scrape
  -> auth (ACUC cached 10 min, fail-open limits 60 s) -> rate limit by mode x plan multiplier
  -> validation (strict zod; defaults: skipTls true unless headers/actions, timeouts widened by feature)
  -> Safe Mode / threat protection / blocklist / ZDR resolution (controller), job built with skipNuq
  -> team lease semaphore (Redis Lua zset, wait <= 2/3 timeout, abort on client close)
  -> processJobInternal -> scrapeURL
        Safe Mode backstop (per URL) -> meta (feature flags, abort tiers, prefetch sentinels)
        threat check -> robots (only if org flag/Safe Mode) -> retry tracker
        buildFallbackList: exchange? | lockdown/agentIndexOnly pin | file-fetch route | tlsclient/fetch demotion
                           | wikipedia/x-twitter | score by feature flags | profile guard | sort by quality
        hedged race: start engine N; if it errors start N+1 now; if it exceeds MRT start N+1 in parallel; winner snipes losers
        catch AddFeature/RemoveFeature/antibot/proxy errors -> mutate flags -> re-plan (budgets: 6/3/3/2)
        result gate -> postprocessors (youtube) -> transformers (25-step stack)
           deriveHTML, markdown, cleanContent, redactPII, promptInjectionGuard, links, images, branding, metadata,
           product, menu, sendDocumentToIndex, searchIndex, LLM json, deterministicJson, summary, query, attributes,
           agent, removeBase64, deriveDiff, audio, video, coerce
        redirect re-check (threat protection) -> metrics/spans
  -> billing (failure free except guard/scan fees) -> logRequest/logScrape (Pub/Sub, ZDR-scrubbed) -> response
```

### 2.3 Crawl lifecycle

```
POST /v2/crawl -> clamp limit to remaining credits, init credit shards, StoredCrawl in Redis (24 h), group in NuQ (pg or fdb pinned)
  -> kickoff job: lock+queue start URL (crawl-source scrape, robots-checked)
                  -> kickoff_sitemap jobs (robots Sitemap lines, scoped dir, /sitemap.xml, root domain; <=25 sitemaps, 3 concurrent, 7-day cache)
                  -> index-assisted seeds (URLs the cache already knows), threat-filtered
  -> single_urls jobs: scrape -> discover links -> Rust filterLinks (scope, depth, regex, robots, file type...) -> lock (visited set, permutations) -> enqueue
  -> concurrency queue: team limit, crawl maxConcurrency (1 if delay), promotion on job done, reconciler every 60 s
  -> nuq_group_crawl_finished cron (15 s): no queued/active/backlog -> queue_crawl_finished -> finishCrawlSuper (log, credits, webhooks, notification)
  -> group TTL (default 24 h) -> batch cleanup (500 groups/min)
```

---

## 3. Rendering and browser engines (category 3)

### 3.1 Engine set and selection

Engine list is built from configuration presence, not from a registry: index and index;documents (INDEX_DATABASE_URL), x-twitter (xAI key or DB auth), wikipedia (credentials), six fire-engine variants (FIRE_ENGINE_BETA_URL), playwright (PLAYWRIGHT_MICROSERVICE_URL), then fetch, pdf, document, and image only when FirePDF is configured [SRC `engines/index.ts:71-105`].

Selection is score based [SRC `engines/index.ts:107-151, 296-609, 962-1063`]. Each request produces a set of feature flags (actions, waitFor, screenshot, screenshot@fullScreen, pdf, document, image, audio, video, atsv, location, mobile, skipTlsVerification, useFastMode, stealthProxy, branding, disableAdblock). Each flag has a priority (1 to 100). Each engine has a boolean matrix of supported flags and a quality number. Engines are admitted when the sum of priorities of flags they support is at least half of the total; ties are ordered by quality. Unsupported flags become a document warning ("your scrape may be partial") [SRC `scrapeURL/index.ts:1207-1217`]. Stealth engines have negative quality so they are only reached by explicit request or escalation, and the code has an explicit guard so a stealth request cannot silently lose its stealth engines to the positive-quality filter [SRC `engines/index.ts:1016-1040`].

Assessment: the matrix makes driver choice explainable and testable. Its weakness is that weights and quality numbers are hand tuned and several engines cannot be exercised from the repo.

### 3.2 The hedged waterfall and the re-plan loop

The loop in `scrapeURLLoop` [SRC `scrapeURL/index.ts:822-1106`] is the heart of the system:

- Engines start in order. After engine N starts, a timer for its max reasonable time (index 1.5 s, fetch, document and tlsclient 15 s, playwright waitFor + 30 s, chrome-cdp waitFor + all action time + 30 s) is armed. `Promise.race` over all in-flight engines and the timer decides what happens next.
- If an engine fails, the loop starts the next engine immediately. If the timer fires first, the next engine starts in parallel and the slow one keeps running ("hedging"). The first acceptable result wins and the others are cancelled through an `engine`-tier abort ("sniped").
- A scrape-level timeout (or 5 minutes by default) cancels everything.

Re-planning is separate [SRC `scrapeURL/index.ts:1475-1663`, `retryTracker.ts`]. Engines or checks can throw `AddFeatureError(["stealthProxy"|"pdf"|"document"|"image"], prefetch?)` or `RemoveFeatureError`. The outer loop mutates the feature flag set and re-runs the whole waterfall. File handoffs carry the downloaded file as a prefetch object. Budgets: 6 total attempts, 3 additions, 3 removals, 2 prefetches shared between antibot and proxy-failure paths. A subtle design point is the `undefined`/`null` prefetch sentinel: undefined means the browser never tried, null means it ran and delivered no file, and the two take different recovery paths [SRC `scrapeURL/index.ts:1518-1636`].

Assessment: best in class among open designs I know. The hedge reduces tail latency, the typed signals keep escalation logic out of engines, and the budgets prevent loops. Cost is duplicated work and an assumption that engines are idempotent reads.

### 3.3 The result gate

An engine succeeds unless it returns a 2xx or 304 page whose content converts to empty markdown (first with main-content extraction, then without). Non-2xx pages are returned as results with their status. The one exception is 401, 403 and 429 under proxy "auto" without the stealth flag, which throws a stealth escalation [SRC `scrapeURL/index.ts:663-808`]. A TODO in the code admits that deciding on bad statuses is hard. A 200 bot wall with text passes. In the cloud product this is covered by fire-engine's own `retryWithStealth` and `failureReason: site_protection` signals [SRC `engines/fire-engine/scrape.ts:208-209, 274-285`].

### 3.4 Fire-engine contract (closed service, visible client)

Protocol [SRC `engines/fire-engine/scrape.ts`, `index.ts`]: POST {base}/scrape with a typed request; the response is either a finished result, a `{jobId, processing}` or a failure; the client polls status every 500 ms, tolerates 3 unexpected poll errors, deletes the remote job after success, error or abort, optionally routes through an A/B staging host, unwraps GCS-offloaded results and gunzips inline files (50 MB cap). Request fields worth knowing: engine (chrome-cdp or tlsclient), url, scrapeId, format (html or rawBase64), headers, blockMedia, priority, instantReturn, geolocation {country, languages}, mobile, mobileProxy (this is what "stealth" means), autoProxy, timeout, maxAge, saveScrapeResultToGCS, zeroDataRetention, skipTlsVerification, profile {owner, name}, disableSmartWaitCache, forceNonRender, pdfMaxSize, and `behaviorOverrides` {disableSiteHandling, exposeWebdriver, useHeadlessUserAgent, disablePlatformSelection, disableCountrySelection, disableAutomaticReferrer}. The last group reveals what the hosted browser normally does for stealth: special handling per site, hiding the webdriver flag, a non-headless UA, OS/platform selection, geo/country selection and an automatic referrer. Safe Mode flips all six to the honest setting.

Failure typing is by substring match on error strings ("Chrome error: ", "Dns resolution error for hostname: ", "File exceeds size limit", "failed to finish without timing out", "proxies available for", "Element" or "Javascript execution failed") [SRC `scrape.ts:274-359`]. This is fragile and should not be copied.

Actions are lowered into one ordered list: waitFor (capped at 30 s), user actions, a screenshot action, an internal branding script, a cookie capture for media [SRC `engines/fire-engine/index.ts:386-440`]. Limits: 50 actions, 60 s total wait, nine action types (wait, click, write, press, scroll, scrape, screenshot, executeJavascript, pdf) [SRC `controllers/v2/types.ts:249-346`].

### 3.5 Playwright microservice (the OSS renderer)

One Chromium, one fresh context per request with a random user agent, 1280x800 viewport, service workers blocked, a semaphore on concurrent pages (default 10), 13 ad and analytics domains aborted, optional media blocking by extension, Cookie header converted to jar cookies (domain = last two host labels, which breaks for multi-part suffixes), JSON and text bodies returned from the network response, `/health` that opens a real page [SRC `pw/api.ts:160-262, 353-560`]. It supports waitFor, check_selector, headers and skip TLS only; no actions, screenshots or PDF. Runs with `--no-sandbox` inside a container with `cap_drop: ALL` and `no-new-privileges` [SRC `R:docker-compose.yaml:75-95`].

SSRF design [SRC `pw/api.ts:47-125, 236-262, 400-416`]: a loopback proxy-chain server refuses destinations whose DNS answers include any non-unicast address (fail closed on lookup failure) and chains to the optional upstream proxy; the Playwright route handler checks every request again; blocked navigations come back as a 403 result. Weakness: the check resolves the name and then the proxy connects with its own resolution, leaving a rebinding window. Quarry's pinned resolve-once design is stronger.

### 3.6 File handoff

When fire-engine or fetch lands on a file, `specialtyScrapeCheck` decides the parser [SRC `engines/utils/specialtyHandler.ts:147-393`]: GCS references imply PDF; Content-Type decides for documents and PDFs; images need the header and magic-byte confirmation plus an OCR gate (so a mislabeled HTML error page served as image/jpeg is not OCR'd); otherwise `%PDF-`, `PK\x03\x04` and OLE2 signatures catch octet-stream and mislabeled files (a ZIP signature under a genuine archive media type is left alone); archives, audio, video and executables raise UnsupportedFileError. With fire-engine present, file URLs are routed directly to the browser engines, which fetch through the proxy infrastructure, and the file engines act as pure parsers on the prefetched file; proxy tunnel failures in direct downloads convert to typed errors that re-route through the browser [SRC `engines/index.ts:804-902`, `engines/utils/downloadFile.ts`].

### 3.7 Sessions, profiles and Hangar

Hosted browsers (Hangar, closed) have states starting, running, suspending, suspended, resuming, stopping, stopped, failed; absolute (30 to 3600 s, default 600) and inactivity (10 to 3600 s, default 300) TTLs; CDP URL, read-only and interactive live-view URLs; optional HLS recording; ad blocking; country; named persistent profiles; code execution in node, python or bash (code up to 100,000 bytes, 1 to 300 s) [SRC `api/lib/hangar.ts`, `controllers/v2/browser.ts:41-88`]. Create is idempotent (Idempotency-Key, 3 retries on 5xx, long-poll `?wait=30`, stop on readiness failure). Upstream error bodies are never exposed because they can contain capability URLs; recording playlists are validated (size cap, EXTM3U, same-origin segments) [SRC `hangar.ts:60-100, 196-268`].

A finished scrape can be promoted to an interactive session: ownership is validated, a replay script is generated from the stored scrape options (clamped, sanitised actions) and run in a new or adopted session, and retention policy is inherited [SRC `controllers/v2/scrape-browser.ts:160-330`, `lib/scrape-interact/scrape-replay.ts`]. The prompt mode runs an LLM that issues `agent-browser` CLI commands inside the sandbox with `@eN` refs from `agent-browser snapshot -i` (this corrects the scout's negative finding), 25 steps, a 40,000-character snapshot cap, a tab lock and an injected action log [SRC `lib/scrape-interact/browser-agent.ts`].

Profile rules worth copying: a request naming a profile can only run on chrome-cdp engines (or on parsers fed by bytes the authenticated browser downloaded); requests with headers, actions or a profile never read or write the shared cache; a profile cannot be combined with lockdown; sessions reject profiles and recordings under ZDR; the app-side profile listing uses a monotonic `GREATEST(saved_at)` upsert and a deletion tombstone so a late save cannot resurrect a deleted profile [SRC `engines/index.ts:981-990`, `controllers/v2/types.ts:875`, `lib/browser-lifecycle.ts:130-140`, `lib/browser-sessions.ts:216-340`].

### 3.8 Site-specific engines and URL rewrites

Wikipedia (Wikimedia Enterprise API, Redis-cached token behind a redlock, about half of Wikimedia URLs) and X/Twitter (xAI Grok with X search, structured output, cost recorded including per-post fees, fatal on failure) are normal waterfall engines. Google Docs, Slides, Sheets and Drive links are rewritten to export URLs. Operators can force engines per domain pattern with FORCED_ENGINE_DOMAINS [SRC `engines/wikipedia`, `engines/x-twitter`, `lib/rewriteUrl.ts`, `WebScraper/utils/engine-forcing.ts`]. For Verevon this is the natural seam with integration-corev2: a URL-to-provider resolver in the driver plan that prefers a connected-account action over scraping.

---

## 4. Anti-bot, proxies, reliability and caching (category 4)

### 4.1 What anti-bot really is in the open repo

There is no in-repo CAPTCHA handling, stealth patching or proxy rotation. The repo escalates flags. Tiers: basic, enhanced, stealth, auto. The code default is "auto" (OpenAPI says basic, and does not list "stealth") [SRC `controllers/v2/types.ts:846`, `A:api/openapi.json:2548-2551`]. Stealth equals fire-engine's mobile proxy. No surcharge applies [SRC `lib/scrape-billing.ts`]. Self-host proxying is one upstream (PROXY_SERVER) shared by fetch and Playwright [SRC `engines/utils/safeFetch.ts:63-91`, `pw/api.ts:99-125`].

The closest thing to an anti-bot intelligence loop is engpicker [SRC `native/engpicker.rs`, `api/lib/engpicker.ts`]: a background job samples 10 indexed pages of a domain, scrapes each with CDP and with the TLS client (basic and stealth), has gpt-4o-mini judge whether each scrape was real, computes Levenshtein similarity of TLS output to Chrome output in Rust, and stores a per-domain verdict used (opt-in) to promote the cheaper engine. Good idea; experimental in the API.

### 4.2 SSRF and egress

The HTTP path uses an undici dispatcher with a literal-IP and localhost pre-check on every dispatch, a post-connect check that destroys sockets whose remote address is not unicast, and an unbounded redirect cap (5000) [SRC `safeFetch.ts:19-123`]. A fix on 2026-09-30 added the literal check before proxy dispatch because a proxy hides the real destination from the socket check [GIT #4864]. URL schema validation accepts IP literals, so protection rests on the connect-time guard. Webhooks use the same dispatcher without cookies.

### 4.3 robots.txt, blocklists, threat protection, Safe Mode, lockdown

- robots: crawl workers enforce it on the start URL, discovered links and redirect targets; single scrapes check only when the org flag or Safe Mode asks; robots.txt is fetched through the same waterfall (so bot-protected hosts work), with an 8 s timeout and a 1-day cache; failures fail open; Crawl-delay is parsed and deliberately not applied (the assignment is commented out) [SRC `lib/robots-txt.ts`, `scrapeURL/shouldCheckRobots.ts`, `controllers/v2/crawl.ts:363-368`].
- Blocklist: global and per-org, tldts domain matching, keyword exceptions including regex, buffered hit logging; rows are written by operations, not by a public API [SRC `WebScraper/utils/blocklist.ts`].
- Threat protection: URL-level verdicts before the engine runs and after redirects, discovered crawl links filtered, a local rules layer, then Google Web Risk (hash-prefix database) or Zscaler, explicit fail-open or fail-closed policy, request-scoped dedup only, no verdict persisted, +2 credits per unique scanned URL [SRC `lib/threat-protection/index.ts`].
- Safe Mode: robots enforced, no stealth, no Authorization/Cookie/Proxy-Authorization headers, no write/press/executeJavascript actions, no profile, URL userinfo stripped, domain controls, honest browser toggles, optional lockdown, no browser or interact sessions, no v0 API; resolved per URL at the worker backstop so crawl children cannot bypass it; per-request opt-out only if the org allows it [SRC `lib/safe-mode.ts:22-215`, `scrapeURL/index.ts:1250-1308`].
- Lockdown: cache-only, zero outbound (no robots, sitemaps, redirects or media downloads), implies ZDR, +4 credits, two-year default maxAge, typed miss error [SRC `types.ts:2103`, `engines/index.ts:766-770`, `lib/map-utils.ts:124-135`].

### 4.4 The URL cache ("index")

Write path: after a successful, cache-eligible scrape, the raw HTML (plus screenshot and PDF metadata) is saved to object storage as `<id>.json`, and an insert job (Redis list, batched by a background worker) writes a metadata row to the index Postgres: url_hash, original and resolved URL, status, mobile, blockAds, stealth, country, languages, screenshot flags, wait time, 10 URL-prefix hash columns (`url_split_0..9`), 5 domain-level hash columns (`domain_splits_0..4`), title and description, `invalidated_at` [SRC `engines/index/index.ts:49-240`, `db/schema/index-db.ts:32-55`]. Because only raw HTML is stored, every format is derived on every hit.

Admission policy (what is never cached): ZDR, parse uploads, storeInCache=false, winners of fetch, tlsclient and exchange (except for the sitemap pseudo-team), custom headers, actions or profile, custom screenshot size or quality, truncated PDFs, page-aware or block-aware PDF variants, unparsed-versus-parsed PDF mismatches [SRC `engines/index/index.ts:49-90, 535-573`, `engines/index.ts:611-636`]. Note that redactPII is not in the list, so the raw HTML of a PII-redacted request is still stored (INF from the conditions).

Read path: resolve maxAge (explicit, per-domain dynamic from Redis or Postgres within 200 ms, default 2 days); look up Redis entries by a SHA-256 variant key (cap 32 entries, 7-day TTL, 150 ms read budget, offline queue disabled so the cache fails fast); filter by age window, screenshot capability and waitFor; on empty or miss, check a negative marker (opt-in), then query `index_get_recent_5` in Postgres; prefer the newest 2xx unless it is more than 3 entries back; load the body (self-healing: a missing object deletes the Redis entry); apply format-specific mismatch checks [SRC `engines/index/index.ts:243-617`, `services/index-cache.ts`]. `minAge` demands old data and fails with a typed 404-style error rather than waterfalling; lockdown pins the engines to the index.

Companions: demand-driven precrawl (domains with at least 1000 events, sqrt-priority budget, 10 to 50 URLs per domain, 25,000-page cap) [SRC `services/indexing/index-worker.ts:142-330`]; learned boilerplate signatures per domain (OMCE, semantics inferred) applied before selector-based main-content extraction [SRC `native/html.rs:456-505`, confidence medium].

### 4.5 Reliability plumbing

Three-tier abort model (external, scrape, engine) with typed throwables [SRC `lib/abortManager.ts`]. Sync requests give the concurrency wait at most two thirds of the timeout. A resilient internal HTTP client validates JSON with zod, retries on errors and non-2xx, disables socket timeouts (aborts come from signals), and caches DNS [SRC `lib/fetch.ts`]. Fifty-five typed error codes cross queues and webhooks; long PDF timeouts return `processing_continues` with estimated remaining time and a Retry-After [SRC `lib/error.ts`]. Rate limits are per endpoint class times the plan multiplier with org overrides and fail-open entitlement on outage [SRC `services/rate-limiter.ts`, `controllers/auth.ts:56-130`]. Idempotency is key-reuse rejection only (409) [SRC `services/idempotency/validate.ts`]. Option-dependent defaults: TLS verification is skipped by default unless headers or actions are present; default timeouts rise for JSON extraction, change tracking and stealth or auto proxy [SRC `controllers/v2/types.ts:908-990`].

---

## 5. Orchestration and scale (category 5)

### 5.1 NuQ (Postgres)

Schema `nuq` [SRC `apps/nuq-postgres/nuq.sql`]: `queue_scrape`, `queue_scrape_backlog` (with `times_out_at`), `queue_crawl_finished`, `group_crawl`. Claim: `WITH next AS (SELECT ... ORDER BY priority, created_at FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE ... SET status='active', lock=gen_random_uuid()` [SRC `api/services/worker/nuq.ts:1311-1360`]. Lock renewal every 15 s. pg_cron reaper every 15 s requeues jobs locked for over a minute (stalls < 9) or fails them and `pg_notify`s. Completed standalone rows are deleted after 1 h, failed after 6 h. A cron marks groups complete when no queued, active or backlog jobs remain, inserting one `queue_crawl_finished` row; a cron deletes 500 expired groups per minute with SKIP LOCKED and a statement timeout. Operations are code: WAL and checkpoint tuning, aggressive per-table autovacuum, a staggered REINDEX CONCURRENTLY per index, an 18-minute reindex watchdog, pruning of cron history.

Optional RabbitMQ layer: a prefetch worker moves up to 500 queued jobs to active and publishes them to `<queue>.prefetch`; workers `get` from the broker and fall back to SQL; per-pod listen queues or LISTEN/NOTIFY on a direct connection deliver completion to waiting API pods [SRC `nuq.ts:1272-1360, 297-420`].

### 5.2 FoundationDB backend

Concurrency gates (team, crawl, API key) are queue-native: job states include `pending` (waiting for a slot), flags F_GATED, F_CRAWL_GATED, F_KEY_GATED, ready shards, team-pending shards, time buckets, a delay index, a sweeper, and per-group `maxConcurrency` and `delaySeconds`. A router chooses the backend per team flag or globally, pins each crawl to one backend, and defaults unmarked reads to Postgres with throttled fallback warnings [SRC `nuq-fdb/keyspace.ts`, `nuq-router.ts:1-110`]. It exists partly to remove the dual-store drift of the Postgres backlog plus Redis concurrency queue (which needs a reconciler worker every 60 s) [SRC `lib/concurrency-limit.ts`, `lib/concurrency-queue-reconciler.ts`, `nuq-reconciler-worker.ts`].

### 5.3 Synchronous versus asynchronous work

Sync scrapes build a job with `skipNuq: true` and call `processJobInternal` in the API process behind `teamConcurrencySemaphore.withSemaphore`, a Redis Lua lease (zset, 30 s holder TTL, heartbeat) with exponential backoff and jitter, waiting at most 0.667 x timeout and aborting on client close [SRC `controllers/v2/scrape.ts:340-460`, `services/worker/team-semaphore.ts`]. Priority is dynamic: a team's priority degrades as it keeps more jobs in flight within a 60 s window, with the threshold and slope inferred from the plan multiplier [SRC `lib/job-priority.ts`]. Workers apply admission control on CPU and memory (80 percent, cgroup aware), report "stalled" after 25 refusals, and drain on SIGTERM [SRC `services/system-monitor.ts`, `services/queue-worker.ts:219-300`, `nuq-worker-runner.ts`].

### 5.4 Crawl engine

State in Redis with a 24 h TTL: visited sets (optionally locking one canonical permutation across www, http/https, trailing slash and index.html/php variants), unique-count against the limit, jobs, done markers with a repair queue, robots-blocked and threat-blocked lists [SRC `lib/crawl-redis.ts`]. Redirects are re-locked and a raced redirect aborts the job [SRC `scrape-worker.ts:640-680`].

Scope and filtering run in Rust (with a TypeScript fallback and a "keep in sync" comment): depth, include and exclude regexes (path or full URL, with caps of 1000 patterns, 2000 characters each and 100,000 total), directory-prefix scope when backward crawling is off, domain and subdomain rules, social links, non-web protocols, file extensions, fragments, robots, each rejected URL with a typed reason and a message that names the option to change [SRC `native/crawler.rs:117-540`, `lib/crawl-scope.ts`, `WebScraper/crawler.ts:41-290`, `lib/crawl-regex.ts`]. Kickoff discovers sitemaps from robots Sitemap lines, the scoped directory, `/sitemap.xml` and the registrable domain (at most 25 sitemaps, 3 in flight, streamed through a Redis set, 7-day cache through the waterfall) and seeds from URLs the index already knows [SRC `scrape-worker.ts:1294-1628`, `crawler.ts:563-740`]. Budget: the crawl limit is clamped to remaining credits at submit time and per-request credits use sharded counters [SRC `controllers/v2/crawl.ts:310-382`].

Politeness is the weakest part: a `delay` option forces crawl concurrency to 1 and sleeps inside the team's promotion loop; there is no per-host adaptive throttle; robots Crawl-delay is off [SRC `lib/concurrency-limit.ts:270-277, 355-366`].

### 5.5 Monitors and background work

Monitors (scrape, crawl or search targets, cron or natural-language schedules, 5-minute minimum) are claimed by schedulers with leases, deferred by `sha256(monitorId) mod min(interval/2, 5 min)` jitter, recorded as `skipped_overlap` when the previous run is active, healed when stale (60 min, 10 min for search-only), and reserve credits per run (`skipped_no_credits` exists) [SRC `services/monitoring/*`]. A single index worker multiplexes write-behind batchers (index inserts 100, webhook logs 1000), the 15 s browser-session reconciler, precrawl, OMCE and engpicker loops, billing batches and the SIEM consumer [SRC `services/indexing/index-worker.ts:700-790`].

---

## 6. Content processing (category 2)

PDF: Rust classification (text-based, scanned, image-based, mixed; confidence; complexity) then native markdown only for clean text PDFs (confidence at least 0.95 and no complex layout); otherwise FirePDF (sampled at 10 percent by default, forced for per-page, block or marker output, by reference up to 256 MB with async jobs, in-flight job adoption, a content-hash cache with variants, a fail-closed refresh budget of 10 per minute per team, deliberate non-cancellation of abandoned jobs, `processing_continues` timeouts), then MinerU on RunPod under 19 MB, then pdf-parse; fast mode fails scans with a typed error; extraction concurrency is bounded (3 per process, motivated by an OOM) and a per-page time budget (150 ms) fails early; Rust and OCR output are compared in a shadow job [SRC `engines/pdf/index.ts`, `engines/pdf/fire-pdf/*`, `lib/gcs-pdf-cache.ts`, `native/pdf.rs`]. Parser options: mode fast, auto, ocr; maxPages; per-page markdown; typed blocks with bbox, reading order, markdownSpan and confidences; page markers; refresh [SRC `controllers/v2/types.ts:500-600`].

Office and e-books: native conversion (anydoc) of DOCX, DOC, XLSX, PPTX, ODT/ODS/ODP, RTF, CSV, EPUB with DOCX headers and footers merged and OOXML reads under byte and part budgets [SRC `native/document.rs`]. Images: OCR as one-page scans behind a request opt-in, deployment switch and byte confirmation [SRC `lib/image-ocr-gate.ts`].

HTML: streaming-style native transform removes head, meta, style, script, noscript, applies include and exclude, strips 42 non-main selectors unless a force-include region is present, resolves lazy images to the largest srcset or data-src, and absolutises links; markdown conversion is a Go service, then an in-process Go library, then turndown, each followed by Rust post-processing [SRC `native/html.rs:335-660`, `lib/html-to-markdown.ts`].

LLM formats share `generateCompletions` [SRC `transformers/llmExtract.ts:440-990`]: model chosen by schema (gpt-4o-mini; gpt-4.1 for `$ref` schemas), input trimmed to 80 percent of context with a character pre-trim so tiktoken cannot freeze the loop, strict schema normalisation (defaults removed, root arrays wrapped in `{items}`), instructions to ignore directives in the content, JSON repair (fence stripping or model repair) with truncation detection that refuses to repair, fallback to gpt-4.1-mini on quota or rate-limit errors, typed refusal and output-length errors, a 2 M character cap for JSON extraction. Every call is cost tracked and telemetry is disabled under ZDR.

Two distinctive extraction designs: SmartScrape (the extraction schema includes `shouldUseSmartscrape` and a goal, and a browser agent is invoked only when the model says data is hidden behind interaction) and deterministicJson (the model writes a DOM extractor once per URL, schema and prompt; later runs execute the cached code in a remote sandbox with a host-call channel for bounded model questions, selector self-repair, priced at 10 credits to generate and 3 to reuse) [SRC `lib/extractSmartScrape.ts`, `lib/deterministicJson/*`].

Change tracking compares whitespace-stripped, iframe-stripped, character-sorted markdown with the previous scrape of the same URL and tag, optionally returns a git-style diff or an LLM or schema JSON diff, and is unsupported under ZDR; monitors add a goal-aware LLM judge that fails open to "meaningful" [SRC `transformers/diff.ts`, `services/monitoring/judgeChange.ts`]. PII redaction runs after markdown derivation through a separate service with accurate, aggressive and fast modes and fails closed to empty markdown [SRC `transformers/redactPII.ts`, `controllers/v2/types.ts:715-765`].

---

## 7. Observability and operations (category 7)

- ZDR-aware tracing: a context key, a sampler that returns NOT_RECORD for ZDR context or `zero_data_retention` attributes (covering library spans), a bounded registry (100,000) for traces that learn they are ZDR late, export-time dropping; tracing is off unless an OTLP endpoint is configured [SRC `lib/otel-tracer.ts:35-357`, `otel.ts`].
- ZDR beyond tracing: log filter on metadata, `X-Zero-Data-Retention` header honoured by the Go service, monotonic session retention (a ZDR request cannot touch a non-ZDR session; scrapes read their stored policy under a ZDR context first), scrubbed run-history rows (URL, options, cost tracking replaced by markers), a RabbitMQ quorum delay queue (24 h) that deletes retained result blobs found through a ClickHouse `request_children` index, features disabled under ZDR (change tracking, async FirePDF, cache, A/B mirror, PostHog events) [SRC `lib/logger.ts:28-70`, `go-html-to-md-service/handler.go:100-125`, `lib/browser-lifecycle.ts:66-140`, `services/logging/log_job.ts:429-720`, `lib/zdr-queue.ts`, `lib/zdrcleaner.ts`].
- Metrics: per-process `/metrics` and `/health` ports, Prometheus counters and histograms for semaphores, PDF extraction, reconcilers, queue depth, index lookups, event-loop and heap; canonical log lines with outcome labels and phase timings [SRC `nuq-worker-runner.ts`, `engines/pdf/semaphore.ts`, `engines/index/index.ts:330-380`].
- Native logs: Rust tracing is captured per call and re-emitted through the Node logger, or embedded in errors [SRC `native/logging.rs`, `lib/native-logging.ts`].
- Cost: request-scoped tracker with a throwing limit and call-site stacks; Vertex and Google labels carry team, function and scrape ids [SRC `lib/cost-tracking.ts`, `transformers/llmExtract.ts:270-370`].
- SIEM: per-org Azure Sentinel export, schema_version 1 events, RabbitMQ with `x-max-length` drop-head (100,000), per-org gzip batches (200 or 1 s), retry ladder 5 s, 30 s, 5 min, 30 min, DLQ with 7-day retention, destination restricted to credential-free `*.ingest.monitor.azure.com` HTTPS endpoints, encrypted per-org secret [SRC `lib/siem-logging/*`, `A:siem/azure-sentinel/*`].
- Experimentation and evals: A/B shadow traffic (sampled mirror or split to staging, never for ZDR); PR-triggered quality and OCR evals in an external repo; a post-deploy production benchmark labelled by commit SHA [SRC `services/ab-test.ts`, `R:.github/workflows/scrape-evals.yml`, `eval-prod.yml`].
- Tests: end-to-end "snips" through the real API against a local Astro test site (product, branding, robots, prompt injection pages), env gating for self-host and AI, record/replay of outbound HTTP including fire-engine responses via `useMock`, a CI matrix of engine x proxy x queue backend, knip dead-code gate, and an AGENTS.md rule that every API change ships a happy path and a failure path [SRC `apps/test-site`, `api/__tests__/snips/**`, `scrapeURL/lib/mock.ts`, `R:.github/workflows/test-server.yml`, `R:AGENTS.md`]. Older load-test reports in apps/test-suite are historical (Fly.io era).
- Self-host: compose with hardened browser container, resource limits, health checks, log rotation; default API unauthenticated and no persistent volumes; Kubernetes and Helm examples; harness supervisor starts API, N workers, prefetch, reconciler, extract and index workers; polyglot Dockerfile verifies artifacts and stamps a build SHA [SRC `R:docker-compose.yaml`, `R:SELF_HOST.md`, `A:api/Dockerfile`, `api/harness.ts`].
- Weak spots: admin surface keyed by a secret in the URL path (bull-board, metrics, cache clears), permissive CORS, 10 MB JSON limit, hand-written OpenAPI that omits formats and the pdf action [SRC `api/index.ts:62-118`].

---

## 8. Data model reference

Postgres `nuq` [SRC nuq.sql]: queue_scrape(id, status, data jsonb, priority, lock, locked_at, stalls, finished_at, listen_channel_id, returnvalue*, failedreason*, owner_id, group_id) (*self-host only); queue_scrape_backlog(... times_out_at); queue_crawl_finished; group_crawl(id, status, owner_id, ttl, expires_at).

Application Postgres [SRC `api/db/schema/public.ts` per scout, INF for exact columns]: teams, users, user_teams, subscriptions, monitors, monitor_checks, monitor_pages, monitor_check_pages, monitor_email_recipients, browser_sessions, browser_profiles, idempotency_keys, search_feedback, blocklist, blocklist_hits, threat_protection_config, ip_restriction_config, key_restriction_config, siem_logging_config, slack_installations, llm_texts, deterministic_json_scripts, deterministic_json_llm_cache, webhook_logs.

Index Postgres [SRC `db/schema/index-db.ts`]: index (id, created_at, url, url_hash, original_url, resolved_url, has_screenshot, has_screenshot_fullscreen, is_mobile, block_ads, location_country, location_languages, status, title, description, invalidated_at, is_precrawl, wait_time_ms, is_stealth, url_split_0..9_hash, domain_splits_0..4_hash), engpicker_queue, engpicker_verdicts. Bodies in GCS at `<id>.json`.

Redis keys [SRC `lib/crawl-redis.ts`, others]: `crawl:<id>` (+ `:visited`, `:visited_unique`, `:jobs`, `:jobs_qualified`, `:jobs_done`, `:jobs_donez_ordered`, `:kickoff:finish`, `:robots_blocked`, `:threat_blocked`, `:finish`, `:error`), `sitemap:<id>:links`, `limit_team_id:<team>` (priority window), per-team lease zsets, concurrency-limit queues, `idxc:<variant>`, `idxcneg:<variant>`, `idxma:<domain>`, `acuc_<purpose>_<key>_<scrape|extract>`, `index-insert-queue`, `omce-job-queue`, `webhook-insert-queue`, `browser-profile-deleted:<team,name>`, extract state (6 h).

Bigtable (hosted) [SRC `lib/job-access-store.ts`, `lib/change-tracking-store.ts`, `lib/request-credits-store.ts`]: job access rows with TTL, change-tracking baselines, sharded request credits, scrape and extract state, feedback jobs.

---

## 9. Protocol reference (closed-service contracts visible from clients)

- fire-engine: POST /scrape, GET status by jobId, DELETE job; typed success, processing and failure bodies; retryWithStealth and failureReason flags.
- Hangar: POST /v1/browsers?wait=30 (Idempotency-Key), GET /v1/browsers/<id>?wait=N, POST /<id>/stop, POST /<id>/execute {code, language, timeout}, DELETE /v1/profiles/<name>?owner=; browser object {id, status, created_at, ended_at, max_expires_at, recording, profile_saved_at}; creation returns cdp_url, view_url, control_url, playlist_url.
- FirePDF: sync POST /ocr; async POST /jobs (inline base64 or `input_gcs_uri`), poll (long-poll `wait_ms`, early page-aware schedule), GET result, DELETE /jobs/<scrapeId> (2 s timeout), content lookup to adopt jobs.
- Playwright service: POST /scrape {url, wait_after_load, timeout, headers, check_selector, skip_tls_verification} to {content, pageStatusCode, contentType, url, pageError}; GET /health.
- Go converter: POST /convert {html} to {markdown, success}; headers X-Request-ID and X-Zero-Data-Retention; 150 MB body cap.
- avgrab: GET /supported-urls (regex, 5-minute cache), POST /download and /download-video {url, cookies} to {public_url}.
- smart-scrape: POST /smart-scrape {url, prompt, sessionId, extractId, scrapeId} to {sessionId, success, scrapedPages[], tokenUsage}.
- Webhook: POST JSON {success, type, id|jobId, webhookId, data, error, metadata} with `X-Firecrawl-Signature: sha256=<hmac>`, 10 s timeout (30 s v0), single attempt in the visible path.
- SIEM event: schema_version 1, scrape_id, request_id, endpoint, team_id, org_id, api_key{id,name}, started_at, completed_at, url, domain, http_status, result, error, origin, integration, zero_data_retention, threat, audit_metadata.

---

## 10. Best in class, and what is only average

Best in class (relative to Apify/Crawlee-style open peers, in my judgement):
- The hedged waterfall with typed re-plan signals and bounded budgets (records 002, 003).
- File handoff by header plus magic bytes with browser prefetch (017).
- Profile isolation rules and cache admission policy (018, 034).
- ZDR discipline across tracing, logs, headers, sessions, run history and deletion (above all `otel-tracer.ts`).
- Safe Mode as an org-enforced policy with a worker backstop (031).
- PDF tiering and large-document async design (classify, route, adopt, do not cancel, honest timeouts).
- The test strategy: hermetic site, record/replay and CI matrix.
- SIEM delivery engineering and the prompt-injection guard prompt.

Average or weak:
- Anti-bot depth is not in the repo; the open Playwright path is basic.
- Result gate is crude; no content-based bot wall detection in-repo.
- Crawl politeness and robots Crawl-delay.
- Crawl state in Redis with a 24 h TTL and a drift reconciler between Postgres and Redis.
- String-matched error mapping from fire-engine; retry of non-idempotent submits.
- Webhook retry, replay and secret rotation are not visible; idempotency is key-reuse rejection.
- Admin access by path secret; open CORS; unauthenticated self-host default.
- Spec drift: OpenAPI versus code (formats, pdf action, proxy default); scrapeURL README describes LlamaParse and mammoth.
- Magic numbers throughout (weights, quality, thresholds); a deterministic extractor cache keyed by exact URL.

---

## 11. Firecrawl versus Quarry v2, per category

| Area | Firecrawl (this audit) | Quarry v2 (DOC baseline, spot-checked where noted) | Direction |
|---|---|---|---|
| Driver selection | Feature-scored matrix, hedged race, typed re-plan, budgets | Static plan from 3 signals; sequential FallbackDriver rotating on block statuses and JS-shell detection; Kernel and Browserless drivers unregistered; `retry.rs` unused [SRC spot-check] | Adopt matrix, typed signals, budgets; hedge only static to browser |
| Rendering | Closed stealth CDP; OSS Playwright minimal | Local chromiumoxide with per-session contexts, AX projection, DevTools, takeover; stealth script dead code [SRC spot-check] | Quarry ahead on governance and evidence, behind on stealth |
| Scrape options | 20 formats, 9 actions, location, mobile, maxAge, proxy tiers | No formats/actions/headers/location/timeout/maxAge on /v1/scrape | Large gap |
| SSRF | Connect-time check; Playwright proxy plus route check; TOCTOU in Playwright | Resolve-once-and-pin, redirect re-validation, CDP Fetch guard | Quarry ahead |
| Politeness | crawl delay (coarse), robots Crawl-delay off | Per-host AIMD plus global autoscale | Quarry ahead |
| Robots | Crawl enforced, scrape opt-in, fetched via waterfall and cached | Enforced only in /v1/map | Adopt |
| Crawl scope | Rust filter with typed reasons, sitemap and index seeding, dedupe permutations | Live Go path has none; Rust frontier unused | Wire existing frontier |
| Cache | URL index (maxAge/minAge, variants, negative, admission) | fingerprint-keyed, opt-in `prev_fingerprint` | Adopt |
| Queue | Postgres NuQ + RabbitMQ prefetch + FDB; sync bypass | Postgres SKIP LOCKED queue bridge + Temporal; in-process agent run map | Keep Temporal; copy tuning and fair-share |
| Monitors | Lease-claim scheduler, jitter, overlap, stale, credit reservation | Only change_monitor maps to a workflow; trigger/backfill 501 | Adopt operational layer |
| Change detection | Whitespace-insensitive compare, git diff, LLM judge | Raw blake3 inequality; semantic differ unwired | Adopt |
| Documents | Rust PDF classify, OCR tiers, Office, images | lopdf on answer path only; DOCX unwired | Adopt router and tiers |
| LLM extraction | Mature core, SmartScrape, deterministic JSON | Delegates to Model Plane; prompt ignored; Model Plane calls gated off by manifest | Put mechanics in Model Plane |
| ZDR | Cross-boundary discipline, 24 h retained results with cleanup | Strict: ZDR scrape returns no content; no durable artifacts | Adopt tracing and retention discipline; decide product trade-off |
| Org policy | Safe Mode, lockdown, threat protection, SIEM | RunPolicy presets (fingerprint only), blocklists | Adopt |
| Observability | Metrics, canonical logs, OTel, SIEM, A/B | No edge /metrics; OTLP optional; scoreboard pending | Adopt |
| Webhooks | HMAC, SSRF-safe, no visible retry | HMAC, backoff 1 s to 1 h, DLQ; not tenant-scoped or edge-exposed | Quarry ahead on delivery, behind on exposure |
| Approvals, takeover, receipts | Exchange approval gates only; live view and interactive URL | Per-step receipts, approvals via Model Plane, takeover | Quarry ahead |

---

## 12. Corrections: scout inventory and earlier Verevon notes

### 12.1 Scout inventory (firecrawl.inventory.md)

1. Section 3.3 and 3.9 say no accessibility-tree snapshot or element refs exist. Incorrect for the product: the interact agent drives `agent-browser` inside the Hangar sandbox using `snapshot -i` and `@eN` refs [SRC `lib/scrape-interact/browser-agent.ts:82-135`]. True that none of that implementation is in this repo.
2. Section 3.5 "URL dedupe in crawl [INF]": confirmed server-side. `lockURL` locks a canonical permutation when `deduplicateSimilarURLs` is set [SRC `lib/crawl-redis.ts:735-745`].
3. Section 3.5 monitor jitter "presumed": confirmed, `sha256(monitorId) mod min(interval/2, 5 min)` [SRC `services/monitoring/jitter.ts`].
4. Section 8 "naming collision: engine index versus the index search endpoint": the engine `index` is the URL content cache (INDEX_DATABASE_URL); `useSearchIndex` (SEARCH_SERVICE_URL) feeds a separate search service via `sendDocumentToSearchIndex` [SRC `services/index.ts:169-173`, `transformers/index.ts:622-650`].
5. CORS question: no restricted origin list anywhere; `cors()` with defaults is the whole configuration [SRC `api/index.ts:89`].
6. Proxy default and OpenAPI mismatch confirmed [SRC `types.ts:846`, `openapi.json:2548-2551`]; openapi.json also lacks "stealth".
7. Hangar was introduced as the browser backend on 2026-09-25 (#4757); the "Hangar upstream, INF" caveat remains: only the client is visible [GIT].
8. Everything else I re-checked in the engine, queue, cache, ZDR, PDF and monitor sections matched the scout.

### 12.2 Verevon GAP.md section 11 ("Deep-Read Source Analysis")

Claims I could not reproduce in code (all SRC):
- "AbortManager singleton, abort(jobId)": the manager is per scrape and tiered (external, scrape, engine) [`lib/abortManager.ts`].
- "Robots.txt fetched per origin with a 10 s timeout, cached in Redis": robots.txt is fetched through the waterfall with an 8 s timeout and a 1-day maxAge in the URL index; the crawl also stores the text on the StoredCrawl in Redis [`lib/robots-txt.ts:10-100`].
- "performFireEngineScrape calls Fire Engine with HMAC-signed requests": the client sends no signature (empty headers) [`engines/fire-engine/scrape.ts:232`, `checkStatus.ts:161`, `delete.ts:23`].
- "A/B test flag on each scrape, result metadata includes the variant": A/B is job-level sampling to a staging host or a fire-engine mirror or split; no variant in metadata [`services/ab-test.ts`].
- "Branding via a WASM module": it is an esbuild-bundled IIFE injected as an executeJavascript action [`engines/fire-engine/brandingScript.ts`].
- "BLOCK_MEDIA ignores image, media and font resource types by default": default is off; when on it blocks specific file extensions, not fonts [`pw/api.ts:24-25, 230-235`].
- "assertSafeTargetUrl resolves via DNS, caches 30 s": no cache, plain lookup per request [`pw/api.ts:47-88`].
- "InsecureConnectionError propagated as 407": it is returned as a JSON body with pageStatusCode 403 [`pw/api.ts:400-416`].
- "execInBrowser POSTs /execute to the playwright microservice": it calls Hangar `/v1/browsers/<id>/execute` [`lib/hangar.ts:161-185`].
- "AGENT_DEBUG writes to /tmp": debug logs are written under the app logs directory only outside production and never under ZDR [`browser-agent.ts:48-56`].
- "deriveDiff uses Blake3 plus semantic delta": it compares whitespace-stripped, character-sorted markdown and optionally builds a git-style or LLM diff [`transformers/diff.ts:130-142`].
- "useIndex pushes processed chunks to a vector index": `useIndex` writes raw HTML to the URL cache (GCS plus Postgres); no vectors are involved [`engines/index/index.ts:49-240`].
- ZDR "never written to Redis or Postgres, streamed back": results are retained (blobs, job state) for up to 24 hours for async retrieval and deleted by a delayed queue; logs, traces, cache and several features are bypassed [`lib/zdr-queue.ts`, `lib/zdrcleaner.ts`, `services/logging/log_job.ts`].

What GAP.md missed entirely (all postdating or outside its reading): the hedged waterfall and typed re-plan, file handoff, the URL cache and its admission policy, engpicker, OMCE, Safe Mode, lockdown, threat protection, SIEM, the prompt-injection guard, deterministicJson, SmartScrape, NuQ internals, FDB, ZDR sampler, FirePDF async design, Hangar contract, monitors.

OSS_PARITY_BACKLOG (2026-05-30) lists Firecrawl actions, change tracking and formats as parity and reads crawl infrastructure only through Crawlee; it contains nothing on caching, escalation or compliance. SCRAPING_ENGINE_GAPS (2026-05-28) is stale relative to the Quarry baseline (Browserbase exists; adblock code does not) and does not describe Firecrawl at all. FIRECRAWL_PARITY_ASSESSMENT.md (worktree copies) describes legacy Quarry v1 per the baseline.

---

## 13. Timeline of recent changes in this facet [GIT]

| Date | Change | Relevance |
|---|---|---|
| 2025-06-03 | URL index introduced (#1605) | Cache design |
| 2025-09-27 | Tracing rebuilt (#2219) | OTel base for ZDR sampler |
| 2025-12-05 | Engpicker proof of concept (#2501) | Domain calibration |
| 2026-05-06 | Monitor orchestration (#3470) | Scheduler |
| 2026-05-14 | Async FirePDF client (#3526) | Long PDF design |
| 2026-06-04 | deterministicJson (#3706) | Compiled extractors |
| 2026-06-12 | Queue rebuilt on FoundationDB (#3758) | Gates in queue |
| 2026-07-07 | Threat protection (#3949) | URL risk |
| 2026-07-27 | SIEM logging (#4122) | Audit export |
| 2026-08-26 | Prompt-injection check (#4392) | Agent safety |
| 2026-09-15 | Safe Mode (#4612) | Org policy |
| 2026-09-16 | ZDR cleanup on RabbitMQ (#4650) | Retention |
| 2026-09-25 | Browser and interact on Hangar (#4757) | Sessions |
| 2026-09-30 | Pre-proxy private target rejection (#4864); FirePDF long-poll and page-aware early poll (#4880, #4886) | SSRF, latency |
| 2026-10-02 to 10-12 | parse formats endpoint, ZDR for interact, country-scoped sessions, Exchange changes, keyless abuse controls | See api-surface facet |

Activity since 2026-08-06 (GAP.md date) [GIT]: 68 commits under scrapeURL/engines, 34 under engines/pdf, 29 under services/worker, 19 under native, 19 under monitoring.

---

## 14. Recommendations for Verevon

Constraint filter (from CLAUDE.md and the baseline): Model proposes and Quarry executes; ZDR must propagate; no cross-plane DB access; Quarry persists knowledge only through Data Plane contracts; embeddings and rerank stay in Data Plane; Model Plane calls from Quarry are currently denied by the token manifest (model-invoke false); no VLM or OpenCV for anti-bot inside Quarry; captcha solving out of scope; remote browser providers blocked for governed agents until they prove containment; edge is the only public entry; space access never widens owner ACL.

### 14.1 Quick wins (days to a week)

1. Typed error and denial vocabulary with remediation text (records 041 and 055): stable codes for all engine, crawl and policy refusals, messages that name the option to change. Lands in quarry-edge routes and the crawl frontier.
2. URL rewrite table and per-domain driver override config (Google Docs/Slides/Sheets/Drive to export URLs; `FORCED_ENGINE_DOMAINS`-style map) in quarry-runtime URL normalisation and presets.
3. Option-dependent defaults: relaxed TLS only for anonymous reads and recorded in the receipt; time budgets as a function of features.
4. Chromium container hardening and health: cap_drop, no-new-privileges, noexec tmpfs, resource limits, a probe that opens a real page, a page semaphore and a memory-aware admission gate (deploy/compose, quarry-browser).
5. Wire `/metrics` on the edge, add one canonical log line per scrape with outcome and phase timings and the attribute vocabulary engine.winner, engines_attempted, cache_state.
6. Change detection: normalise whitespace and iframes before comparing and scope baselines by tag.

### 14.2 Medium (one to three weeks each)

7. Content-type plus magic-byte router in the scrape pipeline with the PDF classify step (lopdf already present), DOCX/XLSX/PPTX conversion and a bounded-permit semaphore; unify with the answer-path PDF code.
8. URL-keyed cache with maxAge and minAge, variant key (mobile, ads, stealth, country, languages), negative caching, self-heal, and Firecrawl's admission list (never cache ZDR, headers, actions, profiles, lower-quality driver results), scoped per org because Quarry must not share content across tenants; add redactPII as an explicit exclusion.
9. Driver layer: capability matrix with weights, typed escalation signals (NeedsStealth, NeedsBrowser, IsFile, DropFeature), retry budgets, hedging only for static to browser, three-tier abort tokens (client, request, driver). This gives `escalate()` a purpose.
10. Wire the existing Rust CrawlFrontier, RobotsCache and sitemap code into the live crawl path: scope, include and exclude with caps, typed denials, sitemap seeding (robots Sitemap lines, scoped directory, root, registrable domain), permutation dedupe, clamp the limit to the Control Plane entitlement; honour robots Crawl-delay through the AIMD host scheduler.
11. ZDR engineering: sampler-level suppression of spans and events for ZDR runs, propagation header for helper services, monotonic retention on sessions and profiles, scrubbed run-history rows with deletion scheduling; decide explicitly whether ZDR scrapes return content synchronously (Firecrawl does, retaining blobs up to 24 h for async) or never (Quarry today).
12. Eval and test infrastructure: hermetic test site (robots, redirects, bot wall, PDF, prompt-injection pages), record/replay of driver HTTP and CDP exchanges, compose matrix, a labelled-PR eval hook and a post-deploy benchmark labelled by SHA; fill SCOREBOARD.md.
13. Monitor operations: skipped_overlap and skipped_no_credits runs, hash jitter, stale-run recovery, natural-language schedule parsing; make trigger and backfill real by passing a Temporal client to control.
14. Org policy tier (Safe Mode equivalent) enforced at the worker backstop (per URL, inherited by crawl children) and a lockdown/no-egress mode that answers only from stored artifacts.

### 14.2b Larger bets (need other planes)

15. Prompt-injection guard for page text entering Model Plane prompts: chunked classifier, randomised delimiters, capability-based criteria, and a fail-closed option for high-risk actions. Requires the Quarry to Model Plane manifest rows (currently false) or placing it in Model Plane.
16. Goal-aware change judge and SmartScrape-style escalation: Model Plane decides, Quarry executes under broker grants and approvals.
17. Compiled extractors (deterministicJson-like) as site profiles, executed in a sandbox with a bounded host-call channel; ties to the planned site-profile and adaptive target memory work.
18. Engpicker-style calibration feeding the planned EgressBroker and domain-intelligence registry, plus demand-weighted warming of hot tracked sources.
19. SIEM exporter driven by Control Plane audit with an allowlisted destination host, bounded delivery and retry ladder.
20. Tiered PDF with an OCR provider routed through Data Plane or Model Plane, async by-reference jobs, content-hash cache scoped per org.
21. Hosted-session API parity for the browser grant: absolute and idle TTLs, suspend and resume, idempotent create, validated recording manifests, reserve-settle metering with a higher rate for AI-driven use.

### 14.3 Avoid

- Path-secret admin surfaces; permissive CORS; unauthenticated defaults.
- Redis-only crawl state; keeping the same truth in two stores (Postgres backlog and Redis queue) with a reconciler.
- Sleeping inside a promotion loop for politeness.
- Substring matching on remote error strings; retrying non-idempotent submits blindly.
- Process-wide DNS caching alongside a resolve-once SSRF design; unbounded redirect caps.
- Fail-open prompt-injection checks for high-risk actions; fail-open cache of bad statuses as results without a policy.
- Redis-list write-behind for billing-grade data.
- Copying AGPL code; re-implement from behaviour.

### 14.4 What Quarry already does better and must not regress

Resolve-once DNS pinning and browser egress proxy; per-host AIMD and global autoscale; immutable step receipts, approvals and takeover records; TLS impersonation in the open tier; durable workflows with Temporal; webhook retry with DLQ; strict ZDR semantics; evidence-first artifacts.

---

## 15. Open questions

1. fire-engine: how are stealth, platform selection and country selection implemented, and how much would a Verevon equivalent cost to build versus buy (Browserbase, Kernel, Browserless are already drivers in the crate, unwired)?
2. OMCE: what exactly are the signatures (node hash modes in the external nodesig crate) and how many pages per domain are needed before they help?
3. Is the URL index shared across tenants legally and commercially, and how would an org-scoped variant change hit rates?
4. Hangar isolation model (VM, container, gVisor) and how takeover and recording are implemented.
5. How do production deployments retry and replay webhooks (the visible path attempts once)?
6. Whether `__experimental_*` flags are supported surface (omce, engpicker, cache) or internal.
7. What success rates and latency do the external scrape-evals report, and on what corpus?
8. Should Verevon allow ZDR scrapes to return content synchronously (as Firecrawl does) with explicit no-retention guarantees, instead of returning only metadata?
9. Does Quarry need a hedged race at all given its cost model, or only a static-to-browser hedge?
10. Where does a URL-to-connector resolver live: Quarry driver plan, integration-corev2, or the gateway?

---

## Appendix: where to look (all under C:/dev/firecrawl)

- Waterfall: `apps/api/src/scraper/scrapeURL/index.ts`, `engines/index.ts`, `retryTracker.ts`, `lib/abortManager.ts`, `error.ts`
- File handoff: `engines/utils/specialtyHandler.ts`, `engines/utils/downloadFile.ts`
- Cache: `engines/index/index.ts`, `services/index-cache.ts`, `services/index.ts`, `db/schema/index-db.ts`
- Queue: `apps/nuq-postgres/nuq.sql`, `services/worker/nuq.ts`, `nuq-router.ts`, `nuq-fdb/*`, `team-semaphore.ts`, `lib/concurrency-limit.ts`
- Crawl: `lib/crawl-redis.ts`, `scraper/WebScraper/crawler.ts`, `apps/api/native/src/crawler.rs`, `services/worker/scrape-worker.ts`
- PDF and docs: `engines/pdf/**`, `engines/document/index.ts`, `apps/api/native/src/{pdf,document,html}.rs`
- Security: `lib/safe-mode.ts`, `lib/threat-protection/**`, `engines/utils/safeFetch.ts`, `apps/playwright-service-ts/api.ts`
- ZDR and observability: `lib/otel-tracer.ts`, `lib/zdr-queue.ts`, `lib/zdrcleaner.ts`, `lib/siem-logging/**`, `services/logging/log_job.ts`
- Browsers: `lib/hangar.ts`, `lib/browser-lifecycle.ts`, `lib/scrape-interact/**`, `controllers/v2/browser.ts`, `controllers/v2/scrape-browser.ts`
- Ops: `docker-compose.yaml`, `SELF_HOST.md`, `apps/api/Dockerfile`, `.github/workflows/test-server.yml`, `scrape-evals.yml`, `eval-prod.yml`
