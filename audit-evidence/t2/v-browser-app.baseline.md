# Verevon baseline: in-app web, scrape and browser (as exposed to users and agents)

Audit 2 (Quarry v2 + Verevon in-app browser/web). Role: Verevon baseline auditor. Read-only.
Repo: `C:/dev/CoresSystem` at `afb83928` (newest commit 2026-10-10). All paths below are relative to that repo root.
Short prefixes: `GW` = `apps/Frontend Plane/verevonv3/apps/gateway/src`, `SPA` = `apps/Frontend Plane/verevonv3/src`, `QE` = `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src`, `EC` = `apps/Model Plane/rust/services/execution-core/src`, `MG` = `apps/Model Plane/rust/services/model-gateway/src`, `BB` = `apps/Model Plane/go/services/browser-broker`.

Provenance tags: SRC = read in source, DOC = repo docs, INF = my inference (basis stated). No project code was run, so "works" and "broken" are code-reading conclusions, not runtime observations. Where a conclusion depends on wiring I could not run, I give a confidence level.

Scope: how Verevon puts web search, scrape, crawl and a browser in front of users and agents. Quarry-v2's own engine internals are covered by the Quarry auditor. I touch them only where a Verevon surface depends on them.

---

## 0. Executive summary

1. **No standalone browser exists.** There is no `/browser` route. The browser is a "scrape preview" panel inside the dashboard home Crawl composer (`SPA/features/dashboard/home/KnowledgeComposer.tsx`, `KnowledgeScrapePreview.tsx`, `BrowserChrome.tsx` at 2,172 LoC). The chat live-run panel reuses it read-only (`SPA/features/chat/components/ChatLiveRunPanel.tsx:39,608`). Routes are listed in `SPA/app/App.tsx:172-240`. [SRC]
2. **The gateway browser facade is broad and well-guarded.** It has 20 routes: sessions, actions, control mode, tabs, suggestions, AI runs, artifacts, frames over HTTP/SSE/WS, devtools, and profile CRUD (`GW/domains/browser.rs:369-449`). Quarry now owns all session state. The BFF cache (`BrowserRunStore`) is compiled only under `#[cfg(test)]` (`browser.rs:188-189`). [SRC]
3. **The user's session is tightly bounded.** Each in-app session is a Quarry agent run with `max_steps: 12`, `max_runtime_s: 120` and `allowed_domains: [host of the first URL]` (`browser.rs:483-490`). Quarry blocks navigation to other domains (`QE/agent_routes.rs:2296-2304`) and the step budget (`agent_routes.rs:446-451`). [SRC] So the user cannot leave the first site's domain, and the omnibox fallback to Google search (`SPA/.../browser-omnibox.ts:14`) is rejected unless the session started on google.com. [INF, high: the code paths compose directly]
4. **The AI side of the browser is probably broken right now** (INF, high confidence; I could not run it):
   - "AI-steg" (single suggestion) calls model-gateway `/v1/browser/suggest-action` through `proxy_model_json`, which sends no `x-inference-authorization` (`GW/domains/chat/shared.rs:375-400`). The endpoint requires `VerifiedInferenceBearer` (`MG/http_routes.rs:2013-2017`), and that extractor returns 401 when the header is missing.
   - "AI-loop" (durable run) sends `grant_id: "session:{id}"` (`browser.rs:1235`) and no `x-browser-authorization`. Model-gateway requires a `VerifiedBrowserBearer` and validates the grant against BrowserBroker before any run is written (`MG/browser_run.rs:304,337,138-202`; `MG/auth.rs:427-439,942-955`). Nothing in the repo issues BrowserBroker grants in production. The only `AcquireGrant` callers are tests and a deny-by-default legacy Quarry path (`apps/Ingestion Plane/Quarry-v2/crates/quarry-runtime/src/grpc/browser_broker.rs:96-110`).
   - This hardening landed 2026-07-17 (`1f2ff1ed`), after the browser plan declared itself COMPLETE on 2026-07-09.
5. **The in-app browser also depends on dev-mode Quarry.** In `prod`/`production`, Quarry requires a broker grant on `/v1/agent/runs` (`QE/main.rs:183-184`, `agent_routes.rs:1377-1412`). The Verevon gateway never sends one, so in production the in-app browser cannot open a session at all. [SRC + INF high]
6. **Search and scrape are the mature parts.** These ship and are used daily:
   - Unified search: web, images, videos, answer SSE, query suggest, autocomplete, and "similar" (`GW/domains/search.rs:37-47`).
   - Knowledge scrape, crawl, map/discover, batch, product extract and crawl job events (`GW/domains/knowledge.rs:166-192`).
   - A full Ingestions console with runs, schedules, monitoring, sources, evidence and profiles (`SPA/features/ingestions/components/VerevonIngestionsPage.tsx:67-98`; `GW/domains/ingestions/mod.rs:27-48`).
   - Change monitoring (`GW/domains/monitoring.rs:56-70`) and the onboarding crawl preview over SSE (`GW/domains/onboarding/lookup.rs:18`).
7. **Model Plane has the richer agent tool protocol, but Verevon's UI does not use it.**
   - Execution-core offers the chat agent `web_search`, `web_fetch` and the MCP-style `browser.observe` / `browser.act` (`EC/runtime_loop/agent.rs:4184-4211`).
   - `browser.act` targets Quarry accessibility-snapshot refs (`click_ref`, `type_ref`, frame refs). It also covers `respond_dialog` and `upload_ref`/`download_ref`, each gated by a sensitive BrowserBroker approval grant (`agent.rs:3479-3506`; `BB/internal/grant/grant.go:32-36,250-265`).
   - These tools require broker grants nobody issues, so they are unreachable today. [INF high]
   - The Verevon UI's own action vocabulary is the older selector/coordinate set (`SPA/shared/api/browser-client.ts:3-17`; `browser.rs:3702-3760`).
8. **Remote support is a separate stack, not a web browser.** It uses RustDesk-protocol desktop sharing through `@verevon/remote-core` plus the Support Plane hbbs/hbbr. It is live-verified against a stock client per its own docs, and the AI frame sampler stays local (`packages/remote-core/docs/architecture.md:39,108-160`). [SRC+DOC]

---

## 1. Acquisition API surface

Everything below is reached same-origin through `/api/*` behind `require_session`. The SPA never holds Quarry tokens: the gateway mints a `quarry` audience token per user (`GW/domains/ingestions/shared.rs:65-72`; `browser.rs:3813-3829`). [SRC]

| Surface | What exists | Evidence | Maturity |
|---|---|---|---|
| Keyword web search | `POST /api/v1/search/web` → quarry `/v1/search`. Exa-style filters (topic, time_range, days, exact_match, include/exclude domains). `safe_search: true` is hard-coded. Results cached 4 h with stale fallback. | `GW/domains/search.rs:49-292` (payload at 236-258, cache at 218-232/285) | shipped |
| URL-as-query fetch | The same route detects a URL and calls quarry `/v1/scrape` (markdown, `render_page`, 1.8 s wait). On driver-unavailable or 5xx it retries as a static fetch and returns title, description and a 600-char excerpt. | `search.rs:112-203` | shipped |
| Images, videos, similar, query-suggest | `/api/v1/search/{images,videos,similar,suggest}` → quarry equivalents | `search.rs:39-43,295-520` | shipped (similar has a client but no UI caller found; see below) |
| Autocomplete | `/api/v1/search/suggestions` → autocomplete-core, degrades to empty | `search.rs:522-586` | shipped |
| Answer streaming | `POST /api/v1/search/answer/stream` → quarry `/v1/answer/stream` (SSE: citations, delta, done). Consumed by the dashboard search panel. | `search.rs:588-624`; `SPA/features/dashboard/home/DashboardSearchPanel.tsx:339` | shipped |
| Scrape | `/api/v1/knowledge/scrape` (SSRF-normalized proxy) and `/scrape-preview` (ZDR-aware cache admission plus a browser→static fallback) | `GW/domains/knowledge/quarry.rs:28-60,62-140`; `knowledge.rs:166-169` | shipped |
| Crawl / batch | `/api/v1/knowledge/crawl` → `/v1/crawl` (max_pages ≤ 5000) or `/v1/batch` (≤ 200 deduplicated URLs), with selective `ingest` flag | `knowledge/quarry.rs:306-470` | shipped |
| Map / discover | `/api/v1/knowledge/crawl/discover` → quarry `/v1/map`, feeding a page picker | `knowledge/quarry.rs:503-578`; `SPA/features/dashboard/home/CrawlPagePicker.tsx`; `KnowledgeComposer.tsx:549` | shipped |
| Structured extract | `/api/v1/knowledge/scrape/products` (+ summary) → `/v1/extract`. Ingestion runs `kind:"extract"` passes an optional `prompt`. Schedules can carry a `schema`. | `GW/domains/knowledge/products.rs:203`; `GW/domains/ingestions/runs.rs:268-334`; `ingestions/schedules.rs:157-189` | partial (no schema field on ad-hoc extract runs) |
| Ingestion runs | `/api/ingestions/runs` GET (fans out across 5 job registries: crawl, extract, search, agent, batch) and POST (crawl, batch, extract, scrape) | `ingestions/runs.rs:27,31-95,157-180` | shipped |
| Async jobs and event streams | Crawl/batch return `job_id` plus `/api/v1/knowledge/jobs/{id}/events` (SSE). Ingestion evidence uses `/v1/runs/{id}/events`. | `knowledge/quarry.rs:351-376,1098-1160`; `ingestions/evidence.rs:24-60` | shipped |
| Onboarding crawl preview | `POST /api/v1/onboarding/crawl-preview` streams a seed scrape (`/v1/scrape/stream`) plus crawl events | `GW/domains/onboarding/lookup.rs:18`; `GW/onboarding/crawl_preview/quarry.rs:35,118,177` | shipped |
| Action-contract dispatch | Registry actions `knowledge.scrape_url`, `crawl_site`, `recrawl_source`, `import_source`, `monitoring.check_now`, `ingestions.create_source/delete_source` dispatch to quarry | `SPA/shared/actions/action-registry.ts:1298-1340,2453,2596-2607`; `GW/domains/actions/dispatchers.rs:214-433` | shipped (human-invoked) |
| Browser interact API | 20 gateway routes (section 8) proxying Quarry `/v1/agent/runs*` | `browser.rs:369-449` | partial (see section 0, points 3-5) |
| Agent tools (Model Plane) | `web_search`/`web_fetch` (+ `web.search`/`web.read` aliases) → quarry `/v1/search` and `/v1/extract`. Read-only, no approval gate. | `EC/web_tools.rs:1-40,133,152`; `EC/runtime_loop/agent.rs:4184-4201` | shipped |
| MCP | `/api/v1/mcp/servers` registry: list, register, share, delete, OAuth start/callback, connect. Admin gate for org scope. Rate limited. | `GW/domains/mcp.rs:44-60` | shipped (registry only; Quarry is not exposed as an MCP server to external clients from Verevon) |
| SDK/CLI for customers | Quarry ships generated TS/Python SDKs (`Quarry-v2/sdks/`). The SPA's `quarry-client.ts` wraps the TS SDK at `/api/quarry`, but that BFF route does not exist, so the client is dead code (also flagged in `apps/verevon-web/plans/system-audits/CHAT_PARITY_AUDIT_2026-09-15.md:811`). | `SPA/shared/api/quarry-client.ts:9,30`; grep for `/api/quarry` in GW found only the onboarding-token auth route (`GW/audience_tokens.rs:142-164`) | stub/dead |
| Webhooks | No customer-facing crawl webhook config in the gateway or SPA (grep `webhook` in GW/domains/ingestions and SPA ingestions found nothing). Quarry has `change_webhook.rs` internally. | INF from search | absent in Verevon |

## 2. Output formats & content processing

- **Markdown is the main format.** Every scrape preview requests `formats: ["markdown"]` (`search.rs:116-131`). The scrape preview also falls back to `/v1/extract` markdown when the scrape returns an artifact id instead of inline markdown (`knowledge/quarry.rs:580-700`). [SRC]
- **Metadata** (title/description) is surfaced for URL fetches (`search.rs:176-201`). Branding and snippets come from onboarding preview packets (`GW/onboarding/crawl_preview/normalize.rs`; `SPA/features/onboarding/lib/api/crawl-preview/packets.ts`). [SRC, lightly read]
- **Browser evidence is shown through artifacts:**
  - screenshots (PNG/JPEG) and `visual_observation` JSON, served by the owner-gated `/api/v1/browser/sessions/{id}/artifacts/{art_*}` (`browser.rs:1466-1600`)
  - a typed artifact viewer with images and collapsible JSON (`SPA/features/dashboard/home/BrowserTimelineDetail.tsx:57-180`)
  - DOM summary (interactive elements, node count, text snippet), console and network summaries, and policy denials (`browser-client.ts:19-44`; `browser-session.ts:137-170`)
- **Not surfaced in Verevon UI.** Quarry's AX/accessibility projection (`quarry-core/src/contracts.rs:25,374-386`), PDF action, OCR images, tiles, palettes and logo candidates are listed as "done" artifacts in the plan, but the SPA `BrowserObservation` type has no field for them (`browser-client.ts:32-44`). [SRC]
- **Change tracking:** `/api/v1/monitoring/{check,latest,history,schedules}` → quarry `/v1/change/*` and `/v1/schedules` (Temporal monitors). It powers the Ingestions page "monitoring" view (`GW/domains/monitoring.rs:56-70,256,415,460`; `SPA/features/ingestions/components/VerevonIngestionsPage.tsx:53-58,95`). [SRC]
- **Product extraction:** `/api/v1/knowledge/scrape/products` + `/summary` (LLM extract via quarry), with a ProductPicker UI (`knowledge.rs:171-178`; `SPA/features/dashboard/home/ProductPicker.tsx`). [SRC]
- **Search answer synthesis** happens in Quarry's AnswerPipeline, and the SPA only streams it. The answer pipeline's activation config is Quarry's concern. DOC says it was off pending env fixes (`docs/AI_SEARCH_PLATFORM_PLAN.md` §2). [DOC; not verified at runtime]

## 3. Rendering & browser engines

- **Engine:** Quarry-v2 chromiumoxide (CDP) driver. Each session gets its own browser context, the Phase 6 fix for a cookie leak between sessions (DOC `docs/BROWSER_WORKSPACE_PLAN.md:11`). Quarry also has `browserbase.rs`, `browserless.rs`, `kernel.rs`, `stealth.rs` drivers/modules (`Quarry-v2/crates/quarry-browser/src/`). The Chromium sandbox is on unless `QUARRY_BROWSER_NO_SANDBOX` is set (`chromiumoxide.rs:247-249`). [SRC]
- **Render modes** advertised to the SPA: `chromium | dom_snapshot | readability_fallback` (`browser-client.ts:67`). The gateway always reports `renderMode: "chromium"` for real sessions (`browser.rs:2674`). Search and scrape use an HTTP→static fallback when the browser driver fails (`search.rs:152-171`; `knowledge/quarry.rs:120-140`). [SRC]
- **Page actions exposed to Verevon users and the UI agent:** navigate, click (CSS selector), click_point (viewport coords), type (≤ 4000 chars), press, scroll, mouse_wheel, select, wait, wait_for, screenshot, back, forward, get_content. `evaluate` is explicitly denied. `reload` is emulated as re-navigate (`browser.rs:3702-3760`; `BrowserChrome.tsx:1166-1174`). [SRC]
- **Quarry supports more than Verevon exposes:** semantic actions (`ClickSemantic`, etc.), ref actions, frame refs, `RespondDialog`, `UploadRef`/`DownloadRef`, `Pdf`, `Evaluate` (`quarry-core/src/contracts.rs:639-825`). None of these pass the gateway's `sanitize_action`. [SRC]
- **Viewport:** the gateway accepts `viewport` (default 1280x800, `browser.rs:345`). The SPA always sends 1280x800 (`KnowledgeComposer.tsx:813-818`). No device, UA, locale or geolocation controls exist in the UI (grep in browser files for userAgent/geolocat/locale: none). [SRC]
- **Profiles/cookies:** scoped Quarry profiles (`ephemeral | user_private | org_shared | run_scoped`) with server-side guards:
  - ZDR forbids any persistent scope (`browser.rs:147-185`)
  - creating or rescoping a profile to `ephemeral` is rejected (`browser.rs:2315-2420`)
  - a restore-probe reports cookie counts (`browser.rs:2420-2464`)
  - the SPA has a full create/name/list/inspect/rename/delete popover (`BrowserChrome.tsx:1401-1600`)
  [SRC]
- **Auth/login handling:** the only support is reusing a persistent profile. There is no credential vault or autofill. Login pages trigger a Phase 5 HITL approval (`EC/browser_agent/browser_risk.rs`). [SRC]

## 4. Anti-bot, proxies & reliability

- **SSRF guard at the gateway:** http(s) only, no embedded credentials, blocks localhost/.local/.internal names and non-public IP literals (`GW/public_url.rs:34-90`). It is syntactic only, with no DNS-resolution check at this layer (INF: no resolver in the function; Quarry has its own `validate_navigation` with a `SecurityEngine`, `QE/agent_routes.rs:2306-2308`). [SRC]
- **Fallbacks:** browser→static retry on driver failure for search URL-fetch and scrape preview. Scrape preview classifies hung or bot-walled (403/429) failures as unrecoverable (`knowledge/quarry.rs:120-140,287-305`). Search serves stale cache on upstream error (`search.rs:286-291`). [SRC]
- **Caching:** gateway Redis "verevon:gw:*" keys (`GW/cache.rs:194-201`):
  - search-web is cached 4 h, keyed on query, limit and filters only. The key has **no org/user and no ZDR check** (`search.rs:216-232`; grep `zdr` in search.rs: none).
  - scrape-preview mirrors Quarry cache admission, including ZDR no-cache (`knowledge/quarry.rs:92-118`).
  [SRC]
- **Stealth/proxy/CAPTCHA:** these live in Quarry (`stealth.rs` masks basic headless tells; a pinned egress proxy disables QUIC, `chromiumoxide.rs:237-243`). Verevon exposes no proxy, geo or CAPTCHA controls. Searching SPA browser/ingestion files for `proxy` and `captcha` found nothing. [SRC]
- **Rate limits:** the MCP routes have per-org/user rate limiting (`mcp.rs:55-59`). The browser, search, knowledge and ingestion routers use only `require_session` (`browser.rs:448`; `search.rs:46`; `ingestions/mod.rs:47`). Quarry's per-run step budget is the only browser throttle. [SRC]
- **robots.txt/politeness:** nothing in the Verevon layer. This is Quarry's concern. [INF from search]

## 5. Orchestration & scale

- **Schedules:** `/api/ingestions/schedules` creates cron or one-shot (`schedule_at`) jobs with `overlap_policy` (default `skip`), plus `pause/unpause/trigger/backfill/delete` (`ingestions/schedules.rs:99-155,191-275`). The dashboard Crawl tab shows runs and schedules (`DashboardCrawlRuns.tsx`, `DashboardCrawlSchedules.tsx`). [SRC]
- **Sources:** durable `quarry_sources` CRUD (kinds crawl/scrape/search, optional monitor preset). The client cannot supply `org_id` (`ingestions/sources.rs:44-47,144-268`). [SRC]
- **Crawl scoping:** `max_pages` (1-50 in the ingestion UI path, `runs.rs:192-197`; ≤ 5000 in the knowledge path, `knowledge/quarry.rs:400-411`). Batch is ≤ 200 deduplicated URLs (`knowledge/quarry.rs:423-466`; `ingestions/shared.rs:46`). There are no include/exclude path filters, depth or sitemap options in the Verevon request builders (`normalize_crawl_body` forwards only url, max_pages and ingest). [SRC]
- **Selective ingest:** the user's `crawl_ingest_mode` (auto/never/prompt) resolves to `ingest: bool`, and Quarry defaults to NEVER (`knowledge/quarry.rs:412-419`; `KnowledgeComposer.tsx:482-526,569-592`). [SRC]
- **Concurrency/queues:** none are visible in Verevon. `quarry-client.ts` has `listQuarryRequestQueues`, `getTeamConcurrency` and `getTeamQueueStatus`, but these are dead code (no `/api/quarry` route). [SRC]
- **Browser session budgets:** the user session has 12 steps and 120 s (`browser.rs:483-490`). The AI loop has `BROWSER_AI_LOOP_MAX_STEPS = 5` (`KnowledgeComposer.tsx:149`). [SRC]
- **Fleets:** `FleetRunConsole.tsx` calls `fetch('/v1/fleets/...')` directly, a path the gateway and Vite proxy do not serve (Vite proxies only `/api`, `vite.config.ts:65-72`). Its "live view" tiles are placeholder text (`FleetRunConsole.tsx:116-135`). [SRC]

## 6. Platform & ecosystem

- **No actor store or marketplace, and no browser/scrape templates** in Verevon. "Presets" exist only as monitor presets on sources (`ingestions/sources.rs:248-255`) and as agent presets for runs (`SPA/shared/actions/preset-agents.ts`). [SRC]
- **Datasets/storage:** crawl results land in Data Plane only when `ingest: true`. Working-set evidence lives in Quarry artifacts. There is no dataset browser or export in the UI. [SRC/INF]
- **Billing/credits:** `getTeamCreditUsage`/`getTeamTokenUsage` exist only in the dead `quarry-client.ts:155-163`. The browser AI run accepts `max_cost_usd` (`browser.rs:300-302`). [SRC]
- **Teams/orgs:** every route derives org from the validated session. Browser profiles support an `org_shared` scope. Spaces never widen owner ACLs (CLAUDE.md rule). [SRC/DOC]

## 7. Observability & operations

- **Browser timeline is Quarry-owned:** `GET /api/v1/browser/sessions/{id}/timeline` proxies `/v1/agent/runs/{id}/browser-session/timeline` (`browser.rs:621-661`). The SPA shows it as "Varig nettleseraktivitet" (`BrowserChrome.tsx:2071`). [SRC]
- **DevTools panel** has tabs console, dom, network and vision, backed by `/devtools` (`BrowserChrome.tsx:199,1890-2020`; `browser.rs:1784-1857`). [SRC]
- **Run events:** the durable AI run streams `browser_action_dispatched`, `browser_observation_received`, `browser_run_paused`/`resumed`, `browser_action_approval_required` and `browser_action_decided` over `/api/v1/runs/{run_id}/events` (`KnowledgeComposer.tsx:1053-1095`; `ChatLiveRunPanel.tsx:371-402`). [SRC]
- **Ingestion evidence and warnings** come from `/v1/runs/{id}/events?limit=250` (`ingestions/evidence.rs:24-60`). An Insights "ingestion" section exists (`SPA/features/insights/components/InsightsPage.tsx:112-118`). [SRC]
- **Not surfaced in Verevon:** Quarry's step receipts, egress receipts, `/procedure` compile, replay-check, quality-check and impact-check, `live-view` and `events/stream` (`QE/agent_routes.rs:3680-3727`). No `egress-receipts`/`receipts`/`procedure` callers exist in GW or SPA. [SRC]
- **Security/sandboxing:**
  - Quarry's artifact store applies no tenant scoping. The gateway ownership gate is the only isolation for artifact reads (`browser.rs:1463-1465`).
  - ZDR runs return a typed `browser_artifact_withheld` 404 (`browser.rs:3846-3870`).
  - Live frames are served `Cache-Control: private, no-store` (`browser.rs:1715`).
  - Model Plane sandbox-manager has no browser role. It leases code/workspace sandboxes and a process registry only (grep `browser|chromium|playwright` in `go/services/sandbox-manager`: no hits).
  [SRC]

## 8. In-app browser shell (user-facing)

Location: dashboard home, Crawl tab, scrape preview panel (`DashboardHomeChrome.tsx:5` → `KnowledgeComposer` → `KnowledgeScrapePreview` → `BrowserChrome`). The UI is Norwegian-first.

| Feature | State | Evidence |
|---|---|---|
| Tab strip, new/select/close tab | shipped (Quarry tabs within one session) | `BrowserChrome.tsx:1083-1140`; `browser.rs:385-396,802-1021` |
| Toolbar: back/forward/reload, omnibox, navigate | shipped. Reload re-navigates. The omnibox turns non-URL text into a Google search URL, which the domain lock blocks (INF). | `BrowserChrome.tsx:1146-1210`; `browser-omnibox.ts:4-15` |
| Address bar search / cross-site browsing | effectively absent: one domain plus subdomains per session | `browser.rs:483-490`; `QE/agent_routes.rs:2296-2304` |
| Live frame | shipped over HTTP poll (700 ms) or SSE stream. The WS compositor is "intentionally not advertised" (`liveFrameWsUrl: null`). | `BrowserChrome.tsx:180,960-1045`; `browser.rs:1600-1782,2689-2691` |
| Human takeover vs agent control | shipped over HTTP. A human action auto-switches to takeover; agent actions get 409 during takeover. | `browser.rs:663-759,761-800`; `BrowserChrome.tsx:1230` |
| Coordinate click / wheel / keyboard on the frame | shipped (throttled wheel, viewport mapping) | `BrowserChrome.tsx:179-197,793-820` |
| Manual action bar (selector click/type/key/wait/scroll) | shipped | `BrowserChrome.tsx:1668-1765` |
| Screenshot button | shipped (screenshot action plus artifact) | `BrowserChrome.tsx:1256` |
| Profile/cookie popover (padlock) | shipped (CRUD, scope, probe) | `BrowserChrome.tsx:1181-1600` |
| ZDR toggle and always-visible ZDR chip | shipped (client mirror plus server guard) | `KnowledgeComposer.tsx:208-279,1413-1440`; `BrowserChrome.tsx:1245` |
| DevTools drawer | shipped (console/dom/network/vision) | `BrowserChrome.tsx:1890-2020` |
| Timeline + evidence drawer + approvals list | shipped | `BrowserChrome.tsx:2026-2160` |
| Comment/annotate on page | stub: one hard-coded anchor at (7%,18%) labeled "1" | `browser-session.ts:129-136`; `BrowserChrome.tsx:1844-1856` |
| Local web history ("Fortsett der du slapp") | shipped, browser-local only. Max 10 entries, scoped to workspace and user, purged under ZDR. No server history. | `SPA/features/dashboard/home/web-history.ts:10-40`; `DashboardWebHistory.tsx` |
| Search results page: Info/Videos/Images | shipped | `DashboardSearchPanel.tsx:729-735`; `dashboard-home-types.ts:5-9` |
| Search "Map" tab | stub: decorative pins over the top 4 web results, no maps provider | `DashboardSearchResults.tsx:389-420` |
| Search "Shopping" tab | partial: re-renders web results, no shopping provider | `DashboardSearchResults.tsx:436` |
| Chat split-view live browser panel | partial: compact read-only BrowserChrome for runs that emit browser frames, plus pause/resume control | `ChatLiveRunPanel.tsx:4-6,441-460,538-608` |
| Split view of a live page next to chat | partial (only the run panel above) | same |
| Bookmarks, downloads UI, extensions, password/autofill, reader mode, permission prompts, recording, multiple windows, cookie inspector | absent. I grepped the browser feature files and found nothing for bookmark, password, autofill, extension, reader, download and captcha. | grep over `BrowserChrome.tsx`, `KnowledgeComposer.tsx`, `KnowledgeScrapePreview.tsx`, `browser-session.ts`, `browser-client.ts` |
| Remote support (desktop) | shipped as a separate surface: `/support` → `RemoteSupportPage` using `@verevon/remote-core` (RustDesk protocol over WSS to Support Plane). Permission-gated pointer/keyboard. AI frame sampler stays local. | `SPA/features/support/components/RemoteSupportPage.tsx:6-35`; `GW/domains/remote_support.rs:28-80`; `packages/remote-core/docs/architecture.md:39,108-160`; `apps/Support Plane/README.md` |

## 9. AI-browser control

- **Single-step suggestion ("AI-steg"):** the gateway assembles goal, observation, visual_observation JSON and an optional base64 screenshot, then posts to model-gateway `/v1/browser/suggest-action` (`browser.rs:1056-1156`). Model-gateway runs a vision summary (skipped under ZDR) and planner inference (`MG/http_routes.rs:2013-2130`). **Wiring regression (INF high):** the gateway sends no inference bearer (`shared.rs:375-400`) but the route requires one (`http_routes.rs:2016`).
- **Durable multi-step run ("AI-loop"):**
  - Chain: gateway `/ai-runs` → model-gateway `/v1/browser/runs` → `ExecuteStep(tool_name="browser_agent")` → `run_browser_agent_loop` (`browser.rs:1263-1387`; `MG/browser_run.rs:49-55,296-420`; `EC/browser_agent.rs:621-700`).
  - Pause, resume and stop go through `/api/v1/browser/runs/{id}/control` → `PauseRun/ResumeRun/CancelRun` (`browser.rs:1389-1464`; `MG/browser_run.rs:648-700`).
  - The run acquires its **own** Quarry lease. It does not drive the tab the user sees; continuity is only via `profile_id` and `start_url` (`EC/browser_agent.rs:669-684`; DOC plan Phase 2).
  - **Blocked (INF high)** by the missing broker grant and `x-browser-authorization` (section 0, point 4).
- **Planner:** by default the loop is deterministic `observe`. The LLM planner needs `QUARRY_BROWSER_AGENT_LLM=1` (`EC/llm_planner.rs:10,130-140`), and the Model Plane compose sets only `QUARRY_BROWSER_AGENT_ENABLED` (`apps/Model Plane/deploy/docker-compose.yml:1189`). The planner schema is text/AX-ref based (`ref_id`, `snapshot_id`, `generation`, `frame_id`) with actions navigate/click/type/scroll/wait/extract/done, and it does not send images (`llm_planner.rs:53-66`). [SRC]
- **HITL risk gates (Phase 5):**
  - Categories: login, checkout, posting_form, destructive, cross_domain_navigation and persistent_cookie_use. Each combines model self-report with a deterministic backstop, and the gate fails closed (`EC/browser_agent/browser_risk.rs`, 1,288 LoC; `browser_agent.rs:809-900`).
  - Approvals use the generic orchestration approvals API. The SPA shows an approve/reject card (`BrowserChrome.tsx:322-400`).
  - The cross-org approval IDOR was fixed (DOC plan Phase 6, `e26c724f`).
- **Agent tool protocol (chat/agent runs):**
  - `browser.observe` returns Quarry evidence, accessibility snapshot refs, dialogs, redacted egress receipts and telemetry.
  - `browser.act` takes ref-only actions (`click_ref`/`type_ref`/`select_ref`/`wait_for_ref` + `frame_*`, `respond_dialog`, `upload_ref`, `download_ref`). URLs, CSS, JS, coordinates and paths are not accepted (`EC/runtime_loop/agent.rs:4202-4211,3479-3506`).
  - Each call needs a BrowserBroker audience bearer (`EC/grpc.rs:856-870,895-905`). Sensitive actions also need a single-use child grant (`BB/internal/grant/grant.go:32-36,104-130,250-265`).
  - Status: implemented in Model Plane but unusable end to end, because nothing issues grants. [SRC + INF high]
- **BrowserBroker:** a gRPC (:9095) service with Acquire/Revoke/Validate, a 15-minute TTL, canonical host allowlist (≤ 32, no wildcards or IPs), and org/owner scoping. **Grants are stored in memory** and lost on restart (`BB/cmd/main.go:62`; `BB/internal/grant/grant.go:62-74`; `BB/internal/server/server.go:22,41-110`). [SRC]
- **Prompt-injection defenses:**
  - Tool results are classified ExternalWeb, BrowserScraped or ThirdPartyMcp and screened unconditionally (`EC/provenance.rs:1-60,61-64`).
  - The browser action sanitizer denies `evaluate` (`browser.rs:3751-3755`).
  - The registry tells the model to treat browser actions as high-risk and approval-required (`action-registry.ts:2981-3070`).
  - No SPA browser or scrape registry action is model-executable (`SPA/shared/actions/model-eligibility.ts:127-134`; `agent-tools.ts:113-122`).
  [SRC]
- **Agent cursor/overlay visualization:** only the AI rationale bubble and step badges (`BrowserChrome.tsx:1120-1123,1824-1840`). There is no cursor overlay or element highlight. [SRC]
- **Multi-session/background agents:** the fleet console is a stub (section 5). [SRC]
- **Task record/replay:** Quarry has procedure compile and replay-check (`QE/agent_routes.rs:3684-3696`), but Verevon does not call them. The gateway replay buffer is test-only (`browser.rs:188-215`). [SRC]

## 10. Integration-plane overlap

- **integration-corev2 (gateway `integrations` domain):** providers, connect-sessions (Nango-style), connections, sync, inbox-sync, browse and content (`GW/domains/integrations.rs:39-95`). [SRC]
- **Unified sources view** aggregates Quarry sources, Data Plane documents, integration providers/connections/sync-jobs and a graph snapshot, with per-connection discovery (`ingestions/sources.rs:49-105,269-520`). [SRC]
- **Agent provider actions:** `list_provider_actions` / `execute_provider_action`, with writes approval-gated (`EC/integration_tools.rs:14-18,257`; `agent.rs:4174-4182`). [SRC]
- **SharePoint/M365 knowledge sync:** sites, drives and children (`knowledge.rs:52-70`). [SRC]
- **MCP:** the server registry has OAuth start/callback and connect (`mcp.rs`). `mcp-bridge` is a reference stdio/HTTP bridge (`apps/Model Plane/bridges/mcp-bridge/server.js:1-33`). [SRC]
- **Browser grants and connectors are not linked.** A connected account is never turned into a browser profile, nor the reverse. [INF from absence]

---

## Planned / in-flight per docs

- `docs/BROWSER_WORKSPACE_PLAN.md` (last touched 2026-08-09; status "COMPLETE 2026-07-09"):
  - open item 1: downloads/uploads risk classification "un-built"
  - pixel-level HITL card verification
  - ephemeral-scope cosmetic default
  - model-gateway clippy failures
  - vision sidecar not wired
  - target capabilities #10 (file up/download), #11 (dialog/popup handling), #12 (viewport/device controls) and #13 (debug export) were never closed as their own items
- `docs/AI_SEARCH_PLATFORM_PLAN.md` (DRAFT, 2026-08-05, still written against verevonv2):
  - org-scoped SERP and answer cache with ZDR gating
  - Data-Plane-first RAG
  - Convex persistence of search threads
  - saved searches and alerts
  - billing meters/quotas
  - deep research mode
  - Kart/Shopping specialized providers
  - answer-quality eval harness
- Gateway code comment: "real Model-Plane browser-grant issuance (a durable BrowserGrant record) is still unbuilt" (`browser.rs:1258-1262`).
- `apps/Ingestion Plane/Quarry-v2/QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026_OWNERSHIP_RECONCILED.md`:
  - accessibility snapshots as canonical observation (§5.3)
  - WebDriver BiDi adapter (§5.2)
  - Browser Action IR (§7)
  - ObservationBundle modes (§8)
  - Stagehand-style observe/preview/act/extract (§10)
  - hybrid planner routing (§11)
  - deterministic verification (§12)
  - procedure caching (§13)
  - controlled self-healing (§14)
  Quarry has partly landed these; Verevon has adopted none of them.
- `packages/remote-core/docs/architecture.md`: a durable remote-session audit record and forwarding AI-sampled frames to Model Plane (with ZDR) are both deliberately unbuilt.

## Known gaps

1. Nothing issues BrowserBroker grants. As a result:
   - in-app AI runs fail at model-gateway
   - MCP `browser.observe`/`act` cannot be used
   - prod-mode Quarry rejects the in-app browser session itself
   - the broker store is in-memory
2. The gateway does not forward the delegated inference bearer to `/v1/browser/suggest-action`, or the browser bearer to `/v1/browser/runs`.
3. Sessions are locked to one domain with a 12-step, 120 s budget. The address-bar web search fallback cannot work. Long user browsing is impossible.
4. The AI run uses a separate hidden lease, so there is no true co-browsing on the user's visible tab.
5. The LLM planner is off by default. The default durable run only observes.
6. The UI action vocabulary (selector/coords) does not match Model Plane and Quarry's AX-ref vocabulary. The UI cannot show or use snapshot refs, dialogs, uploads/downloads or PDF.
7. The WS live channel is not advertised (`liveFrameWsUrl: null`). Live view is HTTP poll or SSE.
8. Annotate/comment is a hard-coded placeholder. Map and Shopping tabs are cosmetic. The fleet console is a stub hitting an unrouted `/v1/fleets`.
9. The search-web gateway cache is global (no org/user in the key) and not ZDR-gated.
10. No rate limiting on the browser, search, knowledge or ingestion routers.
11. No crawl include/exclude/depth/sitemap options, webhooks, dataset export or customer API keys/SDK in Verevon. `quarry-client.ts` is dead.
12. No browser shell basics: bookmarks, server-side history, downloads, extensions, autofill/vault, permission prompts, reader mode, recording.
13. Quarry receipts, procedure compile/replay and live-view/events streams are not surfaced.
14. Quarry's artifact store is not tenant-scoped, so the gateway gate is the single isolation point.

## Architecture constraints that limit what Verevon can adopt

- **Plane ownership:**
  - Quarry executes and captures. Model Plane reasons. Verevon displays and controls. VLM/planning must not move into Quarry (`CLAUDE.md`; plan "Hard constraints" `docs/BROWSER_WORKSPACE_PLAN.md:20-28`).
  - A Codex- or Aside-style client-side agent that drives the DOM directly in the user's browser would violate "Model may propose browser actions; Quarry-v2 executes or rejects them".
  - Any in-app browser must stay a remote, leased Chromium streamed as frames, not an embedded webview the SPA controls.
- **No cross-plane DB access:** browser state, timeline and profiles must be read through Quarry APIs. The gateway deliberately dropped its own cache (`browser.rs:188`, `get_session` comments at 566-569).
- **Authority model:**
  - browser grants come from Model Plane BrowserBroker (canonical host allowlist, single-use sensitive child grants)
  - delegated audience bearers per plane (inference, execution, session, browser)
  - identity always comes from verified claims, never body or headers
  - Every new capability has to thread these through the gateway (`MG/auth.rs:942-1000`).
- **ZDR must propagate:**
  - ZDR forbids persistent profiles (`browser.rs:174-185`)
  - artifacts are withheld (`browser.rs:3846-3870`)
  - model-gateway refuses browser runs under issuer-enforced ZDR (`MG/browser_run.rs:307-315,631-633`)
  - the screenshot vision summary is skipped under ZDR (`MG/http_routes.rs:2096-2100`)
  - local web history is purged (`web-history.ts:21-24`)
  - Under ZDR, features such as server-side history, saved sessions, recordings and cached SERPs must be disabled or kept ephemeral.
- **Space access never widens owner ACLs:** an `org_shared` profile or a shared browser session cannot be granted through a Space.
- **Model-executable actions are allowlisted:** browser registry actions are not model-executable (`model-eligibility.ts`). Exposing UI browser actions to the chat model needs governed Model operation paths, not the SPA registry.
- **Channel Plane is docs-only**, so there are no external channels for notifications.

## Doc-vs-code discrepancies

1. The plan says "Program status: COMPLETE (2026-07-09)" with live-verified AI runs. Since `1f2ff1ed` (2026-07-17), model-gateway requires a broker-validated grant and `x-browser-authorization`. The gateway still sends `grant_id: "session:{id}"` and no browser bearer (`browser.rs:1235`; `MG/browser_run.rs:138-202,304`).
2. The plan's Phase 2 spec says the gateway reads `state.browser_run_store`. In current code that store is `#[cfg(test)]` only, and all state comes from Quarry's owner projection (`browser.rs:188-215,566-620`).
3. The plan's Phase 4 says WS live frames and human-takeover gating live in the gateway (test `websocket_agent_action_is_rejected_during_human_takeover`). In production, the WS proxy is a raw pipe, the gating helpers are `#[cfg(test)]` (`browser.rs:2030-2260`), and `liveFrameWsUrl` is null for new sessions (`browser.rs:2689-2691`).
4. The plan lists the downloads/uploads capability as "correctly un-built — no ActionType/capability represents a file transfer anywhere". Code now has `upload_ref`/`download_ref`/`respond_dialog` in Quarry contracts, the BrowserBroker sensitive scopes and `browser.act` (`quarry-core/src/contracts.rs:765-825`; `grant.go:32-36`; `agent.rs:3497-3501`).
5. The plan says replay timeline and per-step evidence come through the gateway. The gateway now returns `timeline: []` and an empty replay (`browser.rs:2697-2698`), and the timeline comes from Quarry's `/browser-session/timeline`.
6. The plan says SSE/WS frames are "live-verified". The WS path is now disabled by design (point 3 above).
7. The doc says the Phase 2 AI loop "streamed 90+ actions". Those were deterministic `observe` steps, because the LLM planner is env-gated off in compose (`llm_planner.rs:130-140`; compose `:1189`). The doc does not make this clear.
8. `AI_SEARCH_PLATFORM_PLAN.md` describes verevonv2 (`SearchAnswerView`, `/api/chat/stream`). Verevon v3 has a different shape (`DashboardSearchPanel` + `/api/v1/search/answer/stream`). The plan's org-scoped/ZDR-gated SERP cache is not implemented at the gateway (`search.rs:216-232`).
9. `CODEBASE_INFORMATION_SYSTEM.md:230` calls browser-broker "Go browser grants". It omits that grants are in-memory only and that no production code path issues them.
10. The action registry marks `browser_session.*`, `browser_tab.*` and `browser_action.*` with `ownerPlane: 'model'` (`action-registry.ts:3003-3080`), but these are executed by Quarry (Ingestion Plane) through the gateway browser domain.
11. `browser.rs` advertises the `annotate` capability (`:2700`) and the SPA view-model defaults to `['annotate','dom_select','knowledge_ingest']` (`browser-session.ts:128`), but annotation is a hard-coded placeholder anchor.
