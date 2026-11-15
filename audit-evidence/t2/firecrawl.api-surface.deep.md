# Firecrawl: public API surface deep dive (facet: api-surface)

Audit 2, read-only. Source: the open-source monorepo at `C:/dev/firecrawl` (HEAD `11e9dc5e`, 2026-10-12, 113 commits since 2026-10-01 [SRC git log]). Taxonomy categories this facet owns: 1 Acquisition API surface, 2 Output formats and content processing, 6 Platform and ecosystem, 10 Integration-plane overlap; a few records touch 3, 4, 5, 7, 8, 9 where the public contract is the interesting part.

Companion file: `firecrawl.api-surface.features.json` (117 records, ids `firecrawl-api-surface-001..117`). Record numbers below are cited as `#NNN`.

Provenance tags used here: **SRC** read in source or config, **DOC** README/skills/public web pages (pricing and rate-limit pages fetched 2026-10-13), **INF** my inference (basis stated). No REA tools were needed because this is open source. No project code, tests or builds were run. `apps/api/.env.local` was not opened (secret-bearing file skipped).

---

## 1. What this repo is, and what it is not

- The repo is the **API gateway, workers, schemas, SDKs and skills** of Firecrawl. The differentiating engines are **not here**: fire-engine (anti-bot browsers/proxies), FirePDF OCR, Hangar (hosted browsers), the agent service (spark-2), Exchange/Alexandria upstream, the research/developer/gov indexes, the product/menu extraction services, avgrab (media), Smart Scrape and Firebill are all reached over env-configured URLs and are closed [SRC `config.ts`, `controllers/v2/agent.ts:213`, `lib/hangar.ts`]. The README's "Open Source vs Cloud" split says the same [DOC].
- Consequence for Verevon: **parity must be measured against the hosted product's behaviour and contracts, not against what the OSS repo can do alone**. Self-hosted Firecrawl scrapes through a small Playwright service with no actions, screenshots or PDF endpoint (#116) and falls back to SearXNG or DuckDuckGo for search (#018).
- Code is ground truth. The in-repo `apps/api/openapi.json` is a **stale v1-era spec** (server `/v1`, deep-research, llmstxt); the current v2 spec lives in the external `firecrawl-docs` repo and is vendored in `apps/elixir-sdk/openapi.json` (45 paths) (#036).

## 2. Corrections to the scout inventory (verified against source)

| Scout statement | What the source shows |
|---|---|
| Crawl options `maxDepth`, `ignoreSitemap`, `allowBackwardLinks`, `includeSubdomains` | v1 names. v2: `sitemap` skip/include/only, `allowSubdomains`, `crawlEntireDomain`, `maxDiscoveryDepth`, `deduplicateSimilarURLs` (default **true**, so server-side dedupe is confirmed), `regexOnFullURL`, `delay`, `maxConcurrency`, `robotsUserAgent`, `prompt` (#008) |
| "No accessibility-tree snapshot, no vision/computer-use loop" | The prompt-driven interact loop uses `agent-browser` accessibility snapshots with `@e` refs, 25 steps, 40 000-char snapshot cap, per-step action-log reflection (#098) |
| `POST /v2/extract` is a core endpoint | Deprecated (`Deprecation` headers pointing to `/v2/scrape` with a `json` format); the Elixir generator skips extract and token usage (#023) |
| Webhook events list omitted one | Monitors also emit `monitor.check.completed` (#025) |
| Proxy default basic (spec) vs auto (code) open question | Resolved: the **v2 spec** (Elixir copy) says default `auto`, enum `basic/enhanced/auto`; only the stale spec says basic; code also accepts `stealth` (#036, #062) |
| Search params | Missed `lang`, `safe`, `enterprise [default,anon,zdr]`, `includeDomains/excludeDomains`, `objective`, `clientModel`, `highlights`, `domainTools`; `asyncScraping` is parsed but never read (dead option) (#015) |
| Index worker is a single RabbitMQ sink | Two sinks: a cache/page index via an indexer queue, and a separate sampled (default 10 percent) feed to a search service (#111) |
| `onlyCleanContent` not mentioned | LLM markdown cleaning (gpt-4o-mini, 120k-token trim, unsupported under ZDR) (#055) |
| Agent is only a thin proxy | Confirmed as proxy for the run logic, but its public contract is large: threads, trace, snapshots, skill export, approvals, `maxCredits`, partial results (#020, #022, #091, #085, #101) |
| Interact = scrape then prompt | Also a standalone session API (`/v2/browser` aliased `/v2/interact`) with TTL, profiles, replay, code execution and time-based billing (#006, #075) |
| `/v2/map` search ranking (inventory: "relevance ordering") | A term-frequency cosine over the **URL string only** (a TODO says title/description could be used) (#014) |
| Many SDKs have a WebSocket watcher | Only JS and Python do; the batch WS URL the JS watcher builds is not registered server-side (only `/v2/crawl/:jobId` is) (#012, #032) |

## 3. Architecture from the API point of view

```
client (SDK / CLI / MCP / skill)
  -> Express 5 app (JSON 10 MB, CORS defaults)
     routes v0 / v1 / v2 (v2 primary) + /labs + /exchange + /admin/<secret>
  -> middleware chain per route (routes/v2.ts)
       agentHints -> [researchCategoryNotice] -> authMiddleware(mode, allowKeyless, allowAgentManagedKey)
       -> agentHintsProvider -> countryCheck -> checkCreditsMiddleware(min) -> blocklist -> controller
  -> controller: zod strict parse -> Safe Mode / threat protection / permissions / key restrictions
       -> sync path: in-process processJobInternal under a team semaphore (scrape, search, map)
       -> async path: NuQ queue (Postgres or FoundationDB) + Redis crawl state (crawl, batch, agent proxy, monitor)
  -> workers: scrapeURL pipeline (engine waterfall -> transformers -> billing) -> webhooks (HMAC) / WS / polling
  stores: Postgres (+ separate index DB), Redis, RabbitMQ (index, webhooks), Bigtable, GCS, Pub/Sub, ClickHouse, Autumn (credits)
```

[SRC `routes/v2.ts`, `routes/shared.ts`, `controllers/v2/scrape.ts`, `docker-compose.yaml`]

Key structural choices:

1. **One options object, many endpoints.** The scrape options zod object is reused by scrape, batch, crawl, search hydration, extract, monitors and parse; every endpoint is `strictObject` (unknown keys rejected) (#002). This is why the surface feels consistent.
2. **Versions are adapters, not forks.** v0/v1 bodies are translated into the v2 internal model (`fromV1ScrapeOptions`, `toV0CrawlerOptions`) with v1-only fields parked in `internalOptions` (#081).
3. **Sync where the work is short, async where it is long.** `/v2/scrape` runs inline behind a per-team semaphore and returns the document (#001); only crawl, batch, agent, monitor and extract return job ids (#010, #020, #024).
4. **Entitlements are data.** Enterprise behaviour is team flags with `disabled|allowed|forced` modes plus per-API-key restrictions and org overrides (#083, #082, #071). Request options turn into entitlement checks (`checkPermissions`).
5. **Policy is enforced twice.** Safe Mode and credential-header stripping run at request time and again at the worker, because crawl children and search sub-scrapes inherit options without passing the request gate (#065).
6. **Agents are first-class API consumers.** Hints, keyless access, device-flow onboarding, agent-managed keys with a human sponsor, and an interop channel for the agent service's own calls (#029, #078, #079, #021).

## 4. Public surface map (v2)

| Area | Endpoints | State / notes | Records |
|---|---|---|---|
| Scrape | `POST /v2/scrape`, `GET /v2/scrape/:id`, `POST/DELETE /v2/scrape/:id/interact` | Primary. Sync. Interact needs stored context | #001 #004 #005 |
| Batch | `POST /v2/batch/scrape`, `GET/DELETE /:id`, `GET /:id/errors` | Shares crawl status code; `appendToId` | #007 |
| Crawl | `POST /v2/crawl`, `GET/DELETE /:id`, `/:id/errors`, `/ongoing` `/active`, `POST /params-preview`, `WS /:id` | NL prompt; credit-clamped limit | #008-#012 |
| Map | `POST /v2/map` | 1 credit flat; index + SERP + sitemap | #013 #014 |
| Search | `POST /v2/search`, `POST /search/:id/feedback`, `POST /feedback` | sources, categories, hydration, feedback refunds | #015-#018 #077 |
| Indexes | `/v2/search/research/*`, `/v2/search/developer`, `/v2/search/gov` | Research and gov free; developer billed like web; GitHub search deprecated (sunset 2026-11-03) | #030 #031 |
| Parse | `POST /v2/parse`, `/parse/upload-url`, `PUT /parse/upload/:id`, `GET /parse/formats` | New 2026-10-02 | #019 |
| Agent | `POST/GET /v2/agent`, `/:id`, `/:id/trace`, `/:id/skill`, `/:id/snapshots/:sid`, `/threads/:tid`, `DELETE /:id` | External runtime; threads and approvals | #020-#022 #091 #101 |
| Monitor | `POST/GET /v2/monitor`, `GET/PATCH/DELETE /:id`, `POST /:id/run`, `/:id/checks(/:cid)`, email confirm/unsubscribe | New in 2026; cron or plain English | #024 #072 #109 |
| Sessions | `/v2/browser` = `/v2/interact` (create, list, get, execute, replay, delete, profiles) | Time-priced | #006 #075 #097 |
| Team | `/v2/team/credit-usage` `token-usage` (+historical), `queue-status`, `activity`, `threat-protection`, `siem`, `/v2/concurrency-check` | Account mode, generous limit | #037 #066 #093 |
| Integrations | `/v2/slack/*`, `/v2/mcp/action-logs`, `/exchange/*`, `/labs/*`, `/v2/support/*` | Mostly proxies to closed services | #108 #107 #087 #090 #088 |
| Legacy | `/v0/*`, `/v1/*` incl. deep-research, llmstxt, `/v2/extract` | Deprecated with RFC headers | #023 #080 #113 |

## 5. Data model and protocols

**Request envelope.** JSON body, strict zod validation, `Authorization: Bearer fc-...` (or none for keyless), optional `x-idempotency-key` (crawl only), `x-request-id` (paid provider calls), `X-Firecrawl-Agent-Hints: true` (#026, #029, #104). SDKs also stamp `origin` and an `integration` tag (#084).

**Response envelope.** `{success, data}` or `{success:false, code?, error, details?}`; extras by endpoint: `warning`, `warnings[]`, `agent_hints[]`, `creditsUsed`, `expiresAt`, `next`, `scrape_id`. DNS failure is a **200 with success:false** on purpose; timeouts are 408 with optional `details.state='processing_continues'` and `Retry-After` (#027, #028).

**Document.** One shape for every format: markdown, html, rawHtml, links, images, screenshot, json, summary, answer, highlights, branding, product, menu, attributes, actions outputs, changeTracking, pages/blocks for PDF, plus `metadata` carrying `statusCode`, `sourceURL`, `url`, `scrapeId`, `proxyUsed`, `cacheState`, `cachedAt`, `creditsUsed`, `postprocessorsUsed`, `concurrencyLimited`, `provider` (#039).

**Async jobs.** UUIDv7 ids. Crawl/batch status is paged with `skip/limit`, `next` URL, and a 10 MiB response cap; data expires (`expiresAt`, default 24 h; team flag `crawlTtlHours`); errors endpoint returns `robotsBlocked`; progress is available by WebSocket (`catchup`, `document`, `done`, `error`) or webhooks (#010-#012, #025).

**Webhooks.** Per-job config `{url|string, headers, metadata, events}`; events namespaced (`crawl.page`, `batch_scrape.completed`, `monitor.check.completed`, agent `started|action|completed|failed|cancelled`); `X-Firecrawl-Signature: sha256=<HMAC of body>` with a per-team secret; private IPs refused; single 10 s in-process attempt or RabbitMQ handoff (#025).

**Run events (agent).** Canonical envelope `schemaVersion, eventId, runId, occurredAt, producerSequence, agent{id, role, name, parentId}` plus typed events (`run.*`, `agent.*`, `browser.session.*`, `progress.reported`, `reasoning.summary`, `tool_call.*`, `artifact.updated`, `error.occurred`) and artifact snapshots fetchable by id (#091).

**Auth identities.** `fc-` API keys (per-key endpoint/format restrictions, IP allow-list, spend limits); OAuth tokens introspected with audience binding; `fcmcp_` delegated credentials (HMAC, 120 s expiry window); agent-managed keys with sponsor status; keyless per-IPv4 daily caps; interop secret for the agent service (#082, #105, #106, #079, #078, #021).

**Deprecation protocol.** `Deprecation` (RFC 9745), `Sunset` (RFC 8594), `Link rel=deprecation|successor-version`, `Warning: 299`, body `warnings[]` and `replacement`, plus forward-looking notices attached only to requests using the affected option (#080).

## 6. Billing, credits and limits (one table)

| Item | Value | Source |
|---|---|---|
| Scrape / crawl page / map | 1 credit (map flat per call) | #074 DOC pricing |
| `json`, `question`, `highlights`, `audio`, `video`, lockdown, `checkPromptInjection`, `redactPII` | +4 each (redaction +4 per extra PDF page) | #074 #054 |
| PDF pages | +1 per page beyond the first; `parsers:[]` = 1 flat | #052 #074 |
| `deterministicJson` | 10 on generation, 3 on reuse | #043 |
| ZDR | `+zdrCost` (default +1) per scrape | #074 |
| Threat-protection scan | +2 per unique URL (none for zscaler mode) | #066 |
| Search | 2 per 10 results (10 under ZDR); gov index and research papers free | #015 #030 |
| Failed scrape | 0 (except lockdown cache-miss 1, prompt-injection verdict 5, threat scans already run) | #074 |
| Browser session | 120/h; interact (prompt used) 420/h; ZDR +120/h; minimum 1 minute; settled on status/stop | #075 |
| Monitor | 1 per page per check (7 with deterministic engine per pricing page) | #024 DOC |
| Agent | 5 free runs/day, then dynamic; `maxCredits` caps; >2500 never free | #020 DOC |
| Rate limits (x1 per minute) | scrape 10, map 10, crawl 2, search 10, agent/extract 2, interact 2, execute 10, status 500; x10 hobby, x50 standard, x500 growth, x1000 scale | #071 DOC |
| Concurrency | free 2, hobby 5, standard 25, growth 50, scale 100+; backlog up to clamp(conc x 2000, 50k, 2M) jobs, 48 h max | #069 DOC |
| Credit shortage | 402; crawl/search clamp `limit` to the remaining balance instead of failing | #027 |

## 7. What is best-in-class

1. **Format breadth behind one options object**, with typed per-format options and cross-format rules (#038). Few competitors expose json, deterministic extractor, change tracking, branding, product/menu, attributes, audio/video, PDF layout blocks, PII redaction and a prompt-injection guard in one call.
2. **Standards-based evolution.** The deprecation/sunset protocol and request-shape-specific forward notices are better than most vendors' changelog-only approach (#080). Combined with adapters for old versions (#081), it lets them rename and retire aggressively (extract, deep research, llms.txt, GitHub search) without breaking old clients silently.
3. **Long-job contract** ("processing continues" plus SDK auto-resume that re-attaches to the in-flight job) (#028).
4. **Policy model for regulated customers.** Safe Mode, lockdown (cache-only), threat protection with customer-owned Zscaler, forced ZDR, SIEM forwarding and per-key scoping form a coherent "governed web access" product (#064-#067, #093, #082, #112).
5. **Agent ergonomics as product**, not docs: hints with bounded declarative rules and quoted URLs (#029), device-flow onboarding in a skill (#079), keyless first-use (#078), endpoint-selection skills (#086), per-call MCP action logs with privacy-minimised team view (#107).
6. **Run observability contract** for the agent: ordered, typed events with parent-agent identity, artifact snapshots and retryable errors (#091).
7. **Billing as a pure function with explicit failure rules**, plus feedback-for-credits to harvest quality signals (#074, #077).
8. **Idempotent paid provider execution** in Alexandria: charge id from `x-request-id`, replay, `duplicate_request`/`request_in_flight`/`request_unresolved` outcomes (#104).

## 8. Weaknesses, risks and limits

- **Closed core.** Anti-bot, OCR, agent and browsers are proprietary; self-hosting is a reduced product (#089, #116). Treat any "Firecrawl can do X" claim as hosted-only unless the repo shows the code.
- **Idempotency on crawl is weak:** UUID-only, global (not team-scoped), no payload hash, no replay, non-atomic check-then-insert (#026).
- **Webhook delivery** has no visible retry/backoff in the in-process path; one HMAC secret per team (#025).
- **Map search is lexical on the URL only** (#014); change detection compares whitespace-stripped, **character-sorted** markdown, so reordering is invisible and meaning is ignored (#044).
- **Hidden defaults:** skipTlsVerification defaults to true unless headers/actions are present; default scrape serves a cache up to 2 days old (per-domain tuned) (#003, #063).
- **Spec drift:** the in-repo OpenAPI is stale; options like `stealth`, `parsers[].mode/blocks/pages`, `onlyCleanContent`, `asyncScraping` are undocumented or dead (#036, #015).
- **SDK drift:** Python 4.50.0 vs JS 4.46.0; seven SDKs lack at least one capability; only 502 is retried (#032, #033).
- **Fail-open prompt-injection guard** (chunks that cannot be classified pass, and are not billed) (#100).
- **Admin surface** uses a secret in the URL path; CORS is the Express default; JSON limit 10 MB (#110, #094).
- **Trust model for internal agent calls** is a shared secret that also controls `shouldBill` (#021).
- **WebSocket** progress polls the queue every second, has no resume cursor, and is registered only for crawl (#012).
- **Unproven paths:** Alexandria paid settlement is covered by mocked tests only (stated in its README) (#104).

## 9. Delta against Verevon's existing Firecrawl documents

Documents checked (read-only): `apps/Ingestion Plane/Quarry-v2/docs/GAP.md` (updated 2026-05-07 to 2026-08-06 sections), `OSS_PARITY_BACKLOG.md` (sources read 2026-05-30), `SCRAPING_ENGINE_GAPS.md` (2026-05-28), and `docs/FIRECRAWL_PARITY_ASSESSMENT.md` (only found in `.claude/worktrees/*`, dated **April 2024**, describing the legacy Go/Colly/Rod Quarry v1).

**Wrong or stale about Firecrawl (GAP.md section 6 and section 1):**

| Claim | Current fact |
|---|---|
| `POST /v1/schedules`: Firecrawl absent; "Firecrawl: no cron scheduling" | Monitors with cron or plain-English schedules, jitter, overlap guards, check history (#024, #072) |
| `GET /v1/profiles`: Firecrawl absent; "no persistent browser profiles" | Named persistent profiles on scrape and sessions; sessions with TTL, replay, live view (#058, #006) |
| `GET /v1/change/latest`: Firecrawl absent | `changeTracking` format with tag history and monitor check/page diffs (#044, #024) |
| "No event history" | Agent trace events, snapshots, monitor checks, webhook logs (#091, #025) |
| `POST /v1/deep-research`: present in Firecrawl | Deprecated in v1 in favour of search (#113) |
| `POST /v1/agent` (v1) | v2 agent with threads, approvals, trace, skill export (#020-#022, #101) |
| `POST /v1/extract` as core | Deprecated; replaced by scrape `json` format and agent (#023) |
| `/v1/runs/:id/events` SSE "Firecrawl: no" | Still true for SSE, but agent trace + WebSocket + webhooks cover the use (#091, #012) |

**Missing entirely from Verevon's four documents (zero mentions or only passing ones):** Alexandria/Exchange (#103, #104, #087), keyless tier and agent-led onboarding (#078, #079), Safe Mode and lockdown (#064, #065), threat protection/Zscaler (#066), SIEM (#093), FirePDF/PDF modes and image OCR (#052, #053), `redactPII` (#054), `checkPromptInjection` (#100), agent hints (#029), monitor API (#024, #109), parse API (#019), feedback-for-credits (#077), deprecation protocol (#080), per-key restrictions and entitlement flags (#082, #083), research/developer/gov indexes (#030, #031), MCP action logs and OAuth/delegated credentials (#105-#107), Slack app (#108), browser billing (#075), ZDR matrix (#112).

**What changed recently (GIT, via scout and my own read-only `git log`, 2026-10-01 to 10-13):** parse formats endpoint and nine SDK updates (10-02); agent threads/exchange/mode across SDKs (10-07); provider-supplied hint rules and removal of built-in hints (10-07/08); `checkPromptInjection` promoted to a top-level option (10-08); `Retry-After` on per-minute 429s, robots.txt checked before a crawl's start URL, nested-sitemap crash fix (10-08); browser/interact sessions by country and ZDR surcharge (10-09); monitor judge moved to gemini-3.5-flash (10-09); the "index" search renamed to `/search/gov` and the `gov` category added (10-05/06); Alexandria feedback window and refunds (10-06); MCP action logs write to the job log with objective and api_job_id (10-13); keyless suspicious-IP routing (10-12). Branding is being iterated almost daily (Jev shadow mode, fixes). Commit cadence (scout): about 1,370 commits April to 12 October.

**Quarry-v2 current gaps that this facet maps directly to (from `findings/t2/v-quarry.baseline.md`):** thin `/v1/scrape` request/response (no formats/actions/timeout/maxAge, artifact refs, ZDR scrape returns no text); crawl without scope/robots/sitemap/status/cancel/errors routes; sequential batch; schedules for change monitors only (trigger/backfill 501); hash-based change detection with an unwired semantic diff; no CLI/MCP/skills; OpenAPI about 60 percent complete; webhooks not tenant-scoped or exposed; no prompt-injection defences; agent budgets not enforced on the step path.

## 10. What Verevon should copy, adapt or avoid

Constraints that shape every recommendation (from `CLAUDE.md` and the baseline): Model Plane proposes, Quarry executes or rejects (no in-process agent loop in Quarry); no cross-plane DB access; Data Plane owns embeddings and reranking; ZDR must propagate; Space access never widens owner ACL; edge is Quarry's only public entry; Control Plane owns billing, quotas and entitlements.

### Copy (contract shapes, low risk)

1. **Scrape contract:** inline document response with a `formats` union, one reusable options object, `timeout`, `maxAge`/`cacheState`, per-call `creditsUsed` and effective-options echo (#001, #002, #038, #039). Highest leverage for model/SDK callers.
2. **Async job lifecycle:** `GET/DELETE /v1/crawl/:id`, `/errors`, `/ongoing`, size-capped cursor, `warning` strings, `expiresAt`; plus a catch-up-then-stream progress protocol (#010-#012).
3. **Crawl option vocabulary** (sitemap modes, include/exclude, dedupe, delay, maxConcurrency, robots entitlements) (#008). Closes baseline Known gaps 1-2 with proven field names.
4. **Error code vocabulary and `processing_continues` contract** with Retry-After (#027, #028) mapped onto the gateway envelope `{error:{code,message,details}}`.
5. **Deprecation headers and notices** for Quarry's own `/v1` evolution (#080).
6. **Monitor resource** (targets, check history, per-page status, jitter, credit reservation) to replace change-monitor-only schedules (#024, #072).
7. **Run event model** (envelope, sequence numbers, parent agent, artifact snapshots, retryable errors) for Model Plane run events shown in Verevon UI (#091).
8. **ZDR refusal matrix** with explicit `unsupported under ZDR` errors instead of empty bodies (#112).

### Adapt (needs Verevon-specific design)

1. **Safe-Mode-style governed browsing**: request-time plus worker-time enforcement of robots, credentials, stealth, allowlist; delivered by Control Plane flags and BrowserBroker grants, not Quarry-local config (#065, #067).
2. **Prompt-injection guard and observation labelling** at the ingestion edge, using Model Plane as the classifier host, with a fail-closed option for agent-bound content (#100).
3. **PII redaction format**, fail-closed, provided by a Verevon privacy service (#054).
4. **Agent hints** as bounded server-computed guidance in Quarry results for the Model Plane (#029).
5. **Alexandria-style capability catalog and idempotent execution** inside integration-corev2: discoverable (provider, capability, schema, price) plus charge-id replay semantics (#103, #104). Keep OAuth tokens in the integration vault.
6. **Scrape-to-interact continuity** by run id: Verevon's agent runs can start from a scrape result (replay of render context) while the Model Plane keeps the loop (#005).
7. **PDF modes and layout outputs**, image OCR and the office formats list, ideally backed by the Data Plane document pipeline (#052, #053, #019).
8. **Deterministic extractor ("compile once")** for monitors, with the code sandbox treated as a high-risk capability (#043).
9. **Plan/limit structure** (per-mode base x multiplier, separate status limits, per-key and per-org overrides) read from Control Plane principals (#071, #082).
10. **Feedback-for-credits loop** as an internal quality signal from the Model Plane (#077).

### Avoid

1. Idempotency like Firecrawl's crawl guard (#026); use the Alexandria design.
2. Cross-tenant shared page index/cache (#111): conflicts with ZDR and owner-plane ACLs.
3. Hidden option defaults such as skipTlsVerification=true (#003).
4. Lexical map ranking in Quarry (#014); delegate to the Data Plane rerank contract.
5. A bespoke queue (NuQ) where Temporal already provides durability (#070).
6. Free-form bash tool for the agent without approval classes (#098, #099); use the one-action `/step` contract.
7. Admin routes protected by a path secret (#110); secret-in-URL gates and Express-default CORS (#094).
8. Chasing deprecated surfaces: multi-URL `/extract`, `/deep-research`, llms.txt (#023, #113).

### Suggested order of work (INF, by leverage and independence)

1. Scrape request/response contract + options object (#001, #002, #038, #039).
2. Crawl scope/status/cancel/errors and credit-clamped limits (#008, #010, #011).
3. Error codes, `Retry-After`, deprecation headers (#027, #028, #080).
4. Monitors on top of existing Temporal change workflow (#024, #072).
5. Safe Mode-style policy + robots enforcement + ZDR matrix (#065, #067, #112).
6. Prompt-injection guard and PII redaction as formats (#100, #054).
7. SDK/OpenAPI single source of truth, then a thin read-only MCP/CLI (#036, #035, #032).

## 11. Open questions

1. Does the hosted `/v2/crawl/:id` WebSocket also serve `/v2/batch/scrape/:id`? The SDK builds that URL but no server route registers it (#012). The SDK falls back to polling, so the user-visible effect is unknown.
2. How does the agent service enforce domain constraints (`strictConstrainToURLs`, `overrideWhitelist`) given the code comment that in-page navigation in remote browsers cannot be intercepted by the API? (#102)
3. What is the exact retention of each sink under ZDR (worker job log, index, GCS)? The flag is plumbed, but sink behaviour was not traced (#112).
4. Is `onlyCleanContent` billed? No surcharge was found in `scrape-billing.ts` (#055).
5. What does Fireclaw (100-credit "plays" endpoint in v1) do? Not documented here; low value, left out of the records.
6. How will the 2026-11-16 research-category switch (results move from `data.web` to `data.research`) affect SDK types? The Elixir spec already lists research paper schemas (#016, #030).
7. Do the other SDK parity gaps (Go, Rust, Java, .NET, PHP, Ruby) reflect deliberate scope or lag? Counts are keyword-based; a method-by-method diff would be needed (#032).

## 12. Evidence index (primary files)

- Routes and middleware: `apps/api/src/routes/v2.ts`, `routes/shared.ts`, `routes/exchange.ts`, `routes/mcp-action-logs.ts`, `routes/admin.ts`
- Contracts: `apps/api/src/controllers/v2/types.ts` (3 042 lines; formats, actions, scrape/crawl/map/search/agent schemas, Document, TeamFlags)
- Controllers: `controllers/v2/{scrape,scrape-browser,browser,crawl,crawl-status,crawl-status-ws,crawl-params-preview,map,search,parse,parse-upload,agent*,monitor,research-proxy,slack}.ts`
- Billing and policy: `lib/{scrape-billing,browser-billing,browser-lifecycle,safe-mode,permissions,key-restriction,deprecations,keyless,error}.ts`, `services/{rate-limiter,webhook,monitoring,alexandria,oauth-token-introspection}`, `controllers/v2/feedback/refund-policy.ts`
- Pipeline pieces: `scraper/scrapeURL/{index,engines/index,engines/index/index,engines/pdf/index}.ts`, `transformers/{diff,promptInjectionGuard,redactPII,llmExtract,audio,video}.ts`, `lib/{deterministicJson,branding,scrape-interact,engpicker,map-utils,map-cosine,search-query-builder}`
- SDKs and skills: `apps/js-sdk/firecrawl/src/**`, `apps/python-sdk/firecrawl/**`, `apps/elixir-sdk/{generate.exs,openapi.json}`, `skills/**`
- Public pages (DOC, fetched 2026-10-13): `https://www.firecrawl.dev/pricing`, `https://docs.firecrawl.dev/rate-limits`
- Verevon side: `C:/dev/CoresSystem/apps/Ingestion Plane/Quarry-v2/docs/{GAP,OSS_PARITY_BACKLOG,SCRAPING_ENGINE_GAPS}.md`, `.claude/worktrees/clever-solomon-e5eba1/docs/FIRECRAWL_PARITY_ASSESSMENT.md`, and the baseline audit `findings/t2/v-quarry.baseline.md` (cited for Quarry facts; its own evidence is file:line in the Quarry repo)
