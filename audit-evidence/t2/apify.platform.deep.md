# Apify platform — deep dive (facet: platform)

Audit 2 / Quarry v2 + Verevon in-app web. Researcher: Fable (public-documentation facet).
Date of research: 2026-10-13. Sources: docs.apify.com, apify.com (pricing, store, change-log, Actor pages), crawlee.dev, apify/actor-templates manifest on GitHub, blog.apify.com. All claims are **DOC** unless tagged **INF** (inference, basis stated). Nothing here is REA or SRC; no Apify code was inspected. Marketing claims are flagged as such.

Companion record file: `apify.platform.features.json` (112 records; ids `apify-platform-001..112`).

---

## 1. What Apify is, in one paragraph

Apify is a SaaS "cloud platform for web scraping and automation" whose unit of everything is the **Actor**: a Docker image + JSON input schema + README + three default storages (dataset, key-value store, request queue), run as an isolated container on Apify's workers and billed by memory-hours ("compute units") plus proxies, storage ops and data transfer. Around the Actor sit (a) a **marketplace** (Apify Store, "86,000+" Actors — marketing figure on the Store page) with developer monetization, (b) a **storage layer** with a persistent URL frontier, (c) **Apify Proxy** (datacenter/residential/SERP/Unblocker), (d) **scheduling, webhooks, integrations**, and since 2025–2026 a strong **agent-facing layer**: hosted MCP server, Actors-as-MCP-servers, MCP *connectors* (reverse direction), Agent Skills, agentic payments (x402/MPP/Skyfire) and an in-Console "Apify AI" that picks and runs Actors from natural language. The open-source crawling library **Crawlee** (JS + Python, Apache-2.0) is the engine most Actors are built on. (DOC: docs.apify.com/platform/actors, /storage, /proxy, /integrations/mcp, /integrations/mcp-connectors, apify.com/store, apify.com/change-log.)

For Quarry, the interesting halves are: the *platform substrate* (packaging, storages, frontier, scheduling, metering, permissions) and the *agent-safety/commerce layer*. The scraping engine itself (Crawlee) is covered more fully by the Firecrawl facets, but its adaptive-rendering and session-pool designs are called out below because they are best-in-class.

---

## 2. Architecture (as documented)

### 2.1 Control / data model

```
Account (user or Organization)
 ├─ API tokens (unscoped | scoped, expiring; personal vs org tokens)
 ├─ Actors (≤500/user)  ──► Versions (MAJOR.MINOR; source = IDE|Git|Zip|Gist)
 │      │                       └─► Builds (MAJOR.MINOR.BUILD, Docker image, tags latest/beta)
 │      ├─ Tasks (≤5,000; saved input + run options)
 │      ├─ Webhooks (≤100; ACTOR.RUN.* / ACTOR.BUILD.* → HTTP POST)
 │      └─ Integrations tab (Actor-to-Actor, Make/Zapier/n8n, data destinations)
 ├─ Runs  (container of a build; status machine; meta.origin; containerUrl; cost)
 │      └─ default storages: KV store (INPUT/OUTPUT), Dataset (append-only), Request queue
 ├─ Named storages (persist indefinitely; shareable by ACL, link, pre-signed URL)
 ├─ Schedules (≤100; cron+tz; ≤10 actors+10 tasks each)
 ├─ Proxy password / groups
 └─ MCP connectors (authorized third-party MCP servers; credentials held by Apify proxy)
```
(DOC: /platform/limits, /platform/actors/running/runs-and-builds, /platform/storage, /platform/integrations/api, /platform/integrations/mcp-connectors.)

### 2.2 Run lifecycle and worker model

- Statuses: READY → RUNNING → (TIMING-OUT | ABORTING) → SUCCEEDED | FAILED | TIMED-OUT | ABORTED. Graceful abort emits `aborting` and allows 30 s. Terminal runs can be **resurrected** with the same storages and optionally a different build/memory/timeout. `meta.origin` records the trigger (DEVELOPMENT, WEB, API, CLI, SCHEDULER, TEST, WEBHOOK, ACTOR, STANDBY, CI, MCP). (DOC: runs-and-builds.)
- Workers are ephemeral: "No run state persists on the worker node"; runs may be **migrated** to another worker at any time (`migrating {timeRemainingSecs}` over the events WebSocket, `persistState` tick every 60 s, `Actor.useState()` to KV, `Actor.reboot()`). (DOC: state-persistence, system-events, security/architecture.)
- Resource model: memory is the only knob; 4096 MB = 1 CPU core, disk = 2× memory, powers of two 128 MB–32 GB; CU = MB×hours at 1-s granularity. (DOC: usage-and-resources.)
- **Standby mode** turns an Actor into an autoscaled HTTP service with a stable hostname (`<actor>.apify.actor`), readiness probe header, idle-timeout, "desired/max requests per run" scaling, 5-min first-response and 2-min run-selection budgets, per-user rate limit header. Runs are per-user, never shared. (DOC: programming-interface/standby, running/standby.)
- Isolation: own FS/CPU/memory per run, platform services separated from customer workloads, SOC 2 Type II, Trust Center, vulnerability-disclosure policy. The sandbox technology is **not disclosed** (INF: likely plain Docker given the Dockerfile-based build model, but undocumented). (DOC: /platform/security, /security/architecture.)

### 2.3 Storage layer

| Store | Semantics | Notable limits / features |
|---|---|---|
| Dataset | append-only item list | JSON/JSONL/CSV/HTML/XLSX/XML/RSS export; `fields`/`omit`/`clean` projections; `#`-prefixed hidden fields; item ≤ 9 MB; 2,000 tabular columns; 400 rps push; schema with `fields` (JSON Schema + agent metadata) and `views` (transformation unwind/flatten/omit + table display formats) |
| Key-value store | mutable records with MIME type | keys ≤ 256 chars; Content-Encoding compression; `exclusiveStartKey` listing; collections schema; strong read-after-write (S3) |
| Request queue | persistent, deduped frontier | `uniqueKey` dedup, BFS/DFS, `forefront`, batch add/delete, **lease-based locking** (`listAndLockHead(lockSecs)`, prolong/delete, `clientKey`, `hadMultipleClients`), reclaim, reuse across runs for incremental crawls; 350 rps CRUD |

Retention: unnamed storages expire with the plan's retention (7 days default for queues/stores; free plan keeps the last 10 runs 4 months); **named storages never expire**. (DOC: /platform/storage/*.)

### 2.4 Actor definition contract

`.actor/actor.json` (spec v1): name, version, buildTag, environmentVariables, dockerfile, readme, input schema, `storages` (dataset / keyValueStore / datasets schemas), `webServerSchema` (OpenAPI 3 → Swagger "Endpoints" tab), `webServerMcpPath` (marks the Actor as an MCP server), `usesStandbyMode`, min/maxMemoryMbytes, changelog. (DOC: actor-json.)

The **input schema** is the platform's most leveraged artifact: it drives the Console form, API validation, Store listing, Apify AI's input filling and MCP `fetch-actor-details`. Editors include domain-specific ones (`proxy`, `requestListSources`, `globs`, `pseudoUrls`, `resourcePicker` with `resourceType` dataset/keyValueStore/requestQueue/**mcpConnector** and `resourcePermissions`), `isSecret` fields (AES-256-GCM + per-(user,Actor) RSA-2048, decrypted only inside the run), `enumSuggestedValues`, `dateType`, sections/groups, custom `errorMessage`. (DOC: input-schema specification v1, secret-input.)

### 2.5 Security and permission model (the strongest part)

1. **Actor permission levels.** New Actors default to *limited*: the run token can only touch the run's own storages, storages from prior runs or passed by input, update/abort itself, metamorph to other limited Actors, read basic user info/proxy password, and call other limited Actors. *Full* permission shows a badge and requires explicit one-time user approval even via MCP/CLI/API; switching a published Actor to full is a breaking change that disables schedules/integrations until re-approved; full-permission Actors score lower and are excluded from MCP search and from Apify AI. (DOC: development/permissions, integrations/mcp, account/apify-ai.)
2. **Scoped API tokens** with resource- or account-level permissions, dependency rules (Write ⊃ Read; Run task ⇒ Run Actor + Read task), expiry, and **scope propagation** into runs ("restricted access" mode) and webhooks created with that token. Not supported in Standby. (DOC: integrations/api.)
3. **MCP connectors**: credentials for Notion/Slack/GitHub/Sentry/Supabase live in Apify's MCP proxy; the Actor only holds its run token and calls `ACTOR_MCP_CONNECTOR_BASE_URL`; the proxy injects credentials, enforces the tool allow-list declared in the input schema and the user's per-connector restriction, and revokes at run end. OAuth via Dynamic Client Registration, managed clients, or the user's own OAuth app. (DOC: integrations/mcp-connectors.)
4. **Secrets**: `isSecret` inputs and Secret env vars (encrypted, redacted from logs); build-time vars explicitly unencrypted. (DOC: env vars, secret-input.)
5. **Webhooks**: static egress IPs, dispatch-id header, 11 retries with exponential backoff to ~32 h, 2-min timeout, but **no HMAC signature** — only "put a secret in the URL". (DOC: webhooks/actions.)

### 2.6 Agent-facing surface (2025–2026)

- **Hosted MCP server** `https://mcp.apify.com` (Streamable HTTP; OAuth or Bearer; 30 rps): tool categories `actors, docs, runs, storage, tasks, schedules, dev`; default set = actors + docs + `apify/rag-web-browser` + `apify/web-fetch` + `report-problem`; `?tools=` selector; **dynamic tool discovery** (an agent searches the Store and calls any Actor by name; loading `call-actor` auto-adds run/dataset/KV/abort tools); hosted server performs **output-schema inference** before a call; rental and full-permission Actors excluded; telemetry on by default. Local `npx @apify/actors-mcp-server` for stdio clients. (DOC: integrations/mcp.)
- **Actors as MCP servers** (`webServerMcpPath`), templates `*-mcp-empty` and `*-mcp-proxy` (wrap any stdio/HTTP/SSE MCP server as a monetized Actor). (DOC: actor-json, templates manifest.)
- **Agent Skills** (`apify-ultimate-scraper`, `apify-actor-development`, `apify-actorization`, `apify-generate-output-schema`, `apify-sdk-integration`, `apify-integration-development`) + a plugin bundling skills, MCP server and a routing agent for Claude Code, Codex, Cursor, Copilot, VS Code, OpenCode, Windsurf, Gemini CLI, etc. CLI `apify help --skill` prints agent guidance. (DOC: agents/skills, cli reference.)
- **Agentic payments**: temporary spend-capped token from agi.apify.com (x402, MPP) or per-request Skyfire/x402 headers; only PPE Actors with limited permissions and no Standby; developer KYC; changelog 2026-06-26 "USDC on Base". (DOC: pricing, api/v2, monetize, change-log.)
- **Apify AI (beta, 2026-07-21)**: Console chat that searches the Store, shows schema/pricing, fills inputs, **always asks confirmation before running**, fetches/summarizes the dataset; no memory; excludes full-permission and rental Actors; daily token allowance per plan. (DOC: account/apify-ai, blog.)

### 2.7 Commercial model

- Plans Free $0 ($5 usage) / Starter $19 / Scale $199 / Business $999, annual −10%; CU $0.20/$0.20/$0.16/$0.13; proxies residential $8→$7/GB, datacenter IPs included then per-IP, SERP $2.50→$1.70 per 1k, Unblocker $1.50→$1.00 per 1k. (DOC: pricing.)
- Store models: **pay-per-event** (developer-defined events, optionally "+ usage"), **pay-per-usage**, and **rental** being sunset (no new rentals after 2026-04-01; conversion 2026-10-01). Users set `maxTotalChargeUsd`; the SDK's `Actor.charge()` returns `chargeableWithinLimit`/`eventChargeLimitReached` so the Actor can stop gracefully; local PPE test mode writes to a `charging-log` dataset. (DOC: actors-in-store, sdk pay-per-event.)
- Developer side: Insights → Analytics (revenue/cost/profit, funnels, JSON export), Actor **quality score** 0–100 (reliability, popularity, feedback, ease of use, pricing transparency, trustworthiness = limited permissions, developer history, congruency) recalculated several times daily and driving Store + MCP ranking; monthly payouts with KYC, $20/$100 thresholds, invoice approval window days 11–14, payout days 21–25, negative Actor profit floored at $0, user compensation deductions. (DOC: quality-score, monetize, monthly-payouts.)

---

## 3. Crawlee design decisions worth copying (engine side, briefly)

- **AdaptivePlaywrightCrawler** — HTTP-first; browser fallback on handler error / `page` access / `resultChecker` false; a sampled fraction (`renderingTypeDetectionRatio` 0.1) runs *both* modes and a `resultComparator` (equal / different / inconclusive) feeds a per-site `renderingTypePredictor`. Website Content Crawler ships this as its default "adaptive" mode. (DOC: AdaptivePlaywrightCrawlerOptions; WCC page.) This is the single most reusable idea for Quarry's cost control.
- **SessionPool** — sessions carry cookies + proxy binding; `markBad`/`retire`, `retireOnBlockedStatusCodes`, `retryOnBlocked` (Cloudflare Bot Management, Google rate-limit); session rotations don't consume `maxRequestRetries`. (DOC: session-management, BasicCrawlerOptions.)
- **AutoscaledPool** — concurrency 1→200 by default, scaled on system snapshots with 5 % steps every 10 s, `maxRequestsPerMinute` checked every second; the platform feeds `cpuInfo.isCpuOverloaded` ~1 s. (DOC: scaling-crawlers, system-events.)
- **BrowserPool** — fingerprint injection tied to proxy URL, `useIncognitoPages`, `retireBrowserAfterPageCount`, ordered hooks, retire-then-kill. Camoufox stealth Firefox + `handleCloudflareChallenge`. (DOC: browser-pool, avoid-blocking, templates.)
- **enqueueLinks** strategies (same-hostname default, same-domain, all), globs/regexps/pseudoUrls, `transformRequestFunction`; `respectRobotsTxtFile`; `Sitemap.load` incl. index + gzip; `RobotsTxtFile` sitemap discovery; `maxCrawlDepth`, `maxRequestsPerCrawl`, `keepAlive`. (DOC: adding-urls, BasicCrawlerOptions, Sitemap.)
- **Statistics** persisted under `SDK_CRAWLER_STATISTICS_*` surviving migrations/resurrects: retry histogram, status-code counters, final-vs-retry error trackers. (DOC: Statistics.)

---

## 4. What is best-in-class

1. **Least-privilege by construction** (limited Actors + scoped tokens + scope propagation + explicit human approval for full permissions, enforced uniformly across UI/API/CLI/MCP) — more complete than any peer in this audit. (DOC)
2. **MCP connectors**: a credential-injecting proxy with declared tool allow-lists and per-run revocation. Exactly the shape Verevon's integration-corev2 "provider actions" need. (DOC)
3. **Pay-per-event with user-set caps and SDK budget feedback** — metering is cooperative between platform and workload, and local-testable. (DOC)
4. **Request queue with leases** — a real distributed frontier primitive exposed over REST with explicit rate limits. (DOC)
5. **Adaptive rendering with learned per-site prediction** (Crawlee). (DOC)
6. **Schema-driven everything** — one input schema feeds the form, validation, Store listing, MCP tool definition and Apify AI; dataset schema feeds views, validation, data-quality alerts and agent metadata. (DOC)
7. **Marketplace flywheel** with quality score, analytics, payouts, agentic payments. Unique; not something Verevon should rebuild. (DOC; counts are marketing)

## 5. Weaknesses and limitations (documented or inferred)

- **No self-hosting** of the platform; only Crawlee/CLI run locally (INF from absence of any on-prem docs + SaaS-only pricing).
- **Webhooks are not signed**; security relies on a secret in the URL. (DOC)
- **Link-by-id access** to runs/builds/storages is on by default ("anyone with the ID"), configurable but the opposite of Verevon's "never widen ACL" rule. (DOC)
- **No ZDR / data-residency controls documented**; only retention periods and named-vs-unnamed storages. (DOC/INF)
- **Organization model is thin**: eight org permissions, no named roles, SSO not documented on the pages read. (DOC)
- **Standby caveats**: restricted-token mode unsupported; idle runs cost money; first-request cold start; selection timeout 429s. (DOC)
- **Page-action vocabulary is declarative and Actor-specific** (wait/scroll/click-expanders/cookie banners); there is no general action DSL comparable to Firecrawl `actions`, and user-authored `pageFunction`s (Web Scraper) are arbitrary JS in the page — powerful but un-sandboxed from the agent's perspective. (DOC)
- **Unblocker proxy is a black box** in the docs. (DOC)
- **Doc inconsistencies**: free-plan concurrent runs 5 (pricing) vs 25 (limits); `FINISHED` vs `SUCCEEDED` naming in runs docs; several docs URLs in llms.txt 404 at the /platform path. (DOC)
- **Apify AI** has no memory, cannot touch tasks/schedules/integrations, and token allowances are small on Free. (DOC)
- Telemetry on by default in the MCP server and CLI. (DOC)

## 6. What Verevon / Quarry v2 should copy, adapt, or avoid

**Copy (platform substrate)**
- The *three-storage-per-job* model (results / blobs+state / frontier) with named-vs-ephemeral retention, server-side export projections, hidden `#` debug fields.
- Request-queue primitives: `uniqueKey`, forefront, batch add, lease/lock with client key, reclaim, cross-run reuse for incremental crawls.
- Run record semantics: status machine, graceful abort, resurrect with same storages, `origin` (incl. `MCP`, `SCHEDULER`), per-run cost, status message, containerUrl-style debug endpoint.
- Budget contract: caller sets max cost; executor queries remaining budget before each billable step (`chargeableWithinLimit`) and shuts down gracefully → maps onto Model Plane cost tracking and the "Model proposes, Quarry executes or rejects" rule.
- Schema-driven tool surface: one JSON schema per Quarry capability / provider action that renders the UI form, validates API input and becomes the MCP tool definition; include agent-facing `description`/`example` metadata and output-schema URL templates.
- Permission precedent: tools default to limited scope; broad scopes need explicit human approval; agents are excluded from high-privilege tools; scope propagates caller → run → webhook.
- MCP-connector proxy pattern for integration-corev2: credentials never enter the sandbox; tool allow-list declared up front; revoke at run end.
- Monitoring: dataset-field statistics alerts (missing fields, outliers) and run-status alert aggregation.
- Adaptive rendering + session pool + autoscaled concurrency from Crawlee for the Quarry engine.

**Adapt**
- Standby-style warm, tenant-isolated service mode for low-latency "fetch as markdown" and "search" tools (Quarry already has synchronous needs from the Model Plane).
- Agent Skills / CLI `--skill` output: ship Verevon skills so external coding agents drive Quarry correctly.
- A quality/trust score for tools feeding the Model Plane's tool ranking.

**Avoid**
- Link-by-id default sharing of storages (conflicts with Space ACL rule).
- Unsigned webhooks — add HMAC.
- A public marketplace; consume Apify Store via API/MCP instead if third-party scrapers are needed.
- Retention-only data controls — Verevon's ZDR must be stronger and propagate to proxies/screenshots/logs.

## 7. Open questions (not answerable from public docs)

- Sandbox technology for runs (Docker? gVisor?) and network egress controls.
- Default Standby idle timeout / max-requests-per-run values.
- Whether `isSecret` values are guaranteed redacted from run logs (docs cover env Secrets, not input secrets explicitly).
- Internal mechanics of the Unblocker group and of Crawlee's `renderingTypePredictor` storage across runs on the platform.
- Whether Crawlee-Python has AdaptivePlaywrightCrawler parity (intro page silent).
- Exact weights of the quality score; how Store "Top 100" popularity decays.

## 8. Source list (all fetched 2026-10-13)

docs.apify.com: /platform/actors, /platform/actors/running/{runs-and-builds,usage-and-resources,tasks,standby,actors-in-store}, /platform/actors/development/{permissions,builds-and-runs/builds,builds-and-runs/state-persistence,deployment/source-types,deployment/continuous-integration}, /platform/actors/development/actor-definition/{actor-json,input-schema,input-schema/specification/v1,input-schema/secret-input,output-schema,web-server-schema}, /platform/actors/development/programming-interface/{standby,metamorph,environment-variables,container-web-server,system-events}, /platform/actors/publishing, /platform/actors/publishing/{monetize,monetize/monthly-payouts,quality-score}, /platform/storage, /platform/storage/{dataset,key-value-store,request-queue,dataset-schema}, /platform/proxy, /platform/proxy/usage, /platform/schedules, /platform/monitoring, /platform/collaboration, /platform/collaboration/{organization-account,list-of-permissions}, /platform/security, /platform/security/architecture, /platform/limits, /platform/console, /platform/integrations, /platform/integrations/{api,webhooks,webhooks/events,webhooks/actions,actors,github,mcp,mcp-connectors}, /platform/agents/skills, /platform/account/apify-ai, /api/v2, /cli/docs/reference, /sdk/js/docs/guides/pay-per-event, /academy/anti-scraping, /llms.txt.
apify.com: /pricing, /store, /change-log, /apify/rag-web-browser, /apify/website-content-crawler, /apify/web-fetch, /apify/web-scraper. blog.apify.com/introducing-apify-ai/.
crawlee.dev: /js/docs/introduction, /js/docs/introduction/adding-urls, /js/docs/guides/{scaling-crawlers,session-management,avoid-blocking,request-storage,got-scraping}, /js/api/playwright-crawler/interface/AdaptivePlaywrightCrawlerOptions, /js/api/basic-crawler/interface/BasicCrawlerOptions, /js/api/utils/class/Sitemap, /js/api/core/class/Statistics, /js/api/browser-pool, /python/docs/introduction.
github raw: apify/actor-templates templates/manifest.json.
