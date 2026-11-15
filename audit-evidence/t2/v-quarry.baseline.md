# Verevon baseline: Quarry-v2 + integration-corev2 (+ the Verevon in-app browser surface that sits on them)

Auditor role: Verevon baseline auditor. Read-only. **CODE is ground truth**; docs show intent.

## 0. Scope, method, provenance conventions

- Repo: `C:/dev/CoresSystem`. HEAD is `afb83928` (2026-10-10 20:18). The working tree also has **uncommitted edits** to about 30 Quarry-v2 files (edge `routes.rs`, `search_routes.rs`, `main.rs`, runtime `rerank.rs`, `serp.rs`, `smart_router.rs`, `token_request_contract.rs`, docs `Features.md`, `openapi.yaml`, ...). I audited the working tree as it stands on disk. The last Quarry commits are `2902caef` (2026-10-08, cross-plane service tokens and ZDR claim handling) and integration-core `391d8926` (2026-10-08, per-connection ZDR consent).
- Path shorthand used below:
  - `Q/` = `apps/Ingestion Plane/Quarry-v2/`
  - `IC/` = `apps/Ingestion Plane/integration-corev2/`
  - `V3/` = `apps/Frontend Plane/verevonv3/`
  - All of these are relative to `C:/dev/CoresSystem`.
  - In the JSON file every path is written out in full repo-relative form.
- Provenance tags:
  - **SRC**: read in source.
  - **DOC**: stated in docs.
  - **INF**: my inference; the basis is stated.
  - REA was not needed: everything here is first-party source.
- Excluded:
  - `Q/.claude/worktrees/vigilant-banach-6933aa/`, a stale copy of the crates.
  - `Q/target/`.
  - `Q/.env`, `apps/Ingestion Plane/.env*`, `.env.generated-secrets*`: secret-bearing files, skipped and not read.
  - `IC/` webhook/SCIM token stores were not opened.
- Size: about 120k lines across the crates, services, lab and SDKs. Test-attribute counts:
  - runtime 757, edge 273, core 137, transform 131, browser 88, security 31, tls 26.
  - Go: 222 `func Test`.
  - I did not run any tests or builds (read-only rule), so "shipped" below means wired into a reachable route or worker in source. It does not mean proven at runtime.

### Topology (SRC)

| Deployable | Lang | Entry | Role |
|---|---|---|---|
| `quarry-edge-rs` (:8082) | Rust/axum | `Q/crates/quarry-edge/src/main.rs`, router `routes.rs:79-307` | The only public/cross-plane entrypoint: JWT (auth-core JWKS), every `/v1/*` route, SSE/WS, fast-path page execution, agent browser runs |
| `quarry-runtime` (lib) | Rust | `Q/crates/quarry-runtime/src/lib.rs` | Drivers, pipeline, observation protocol, search router, stores |
| `quarry-browser` (lib) | Rust | `Q/crates/quarry-browser/src/lib.rs` | `BrowserDriver` trait; chromiumoxide (local CDP), Browserbase, Browserless, Kernel drivers |
| `quarry-control-go` (:8081) | Go/chi | `Q/services/quarry-control/cmd/control/main.go` | Durable registry (jobs, schedules, sources, snapshots, events, webhooks, blocklists, presets), webhook dispatcher, retention janitor, GDPR purge |
| `quarry-orchestrator-go` | Go/Temporal | `Q/services/quarry-orchestrator/cmd/orchestrator/main.go` | 4 workflows (Scrape/Crawl/Batch/ChangeMonitor), jobs poller (2s), schedule reconciler (30s) |
| `quarry-vision-sidecar` | Python/FastAPI/OpenCV | `Q/services/quarry-vision-sidecar/app/main.py:106,145` | Deterministic visual diff/preprocess for observations (opt-in) |
| `integration-corev2` (:3026) | Go/fiber | `IC/cmd/api/main.go`, routes `IC/internal/api/server.go` | OAuth connectors, token vault/broker, named provider actions, webhooks intake, sync jobs, SCIM, Codex-subscription inference, ZDR acknowledgements |
| Verevon gateway browser facade | Rust | `V3/apps/gateway/src/domains/browser.rs:369-458` | `/api/v1/browser/*` proxy to Quarry agent runs + Model Plane suggest/ai-runs |
| Verevon in-app browser UI | SolidJS | `V3/src/features/dashboard/home/BrowserChrome.tsx` (2172 lines) | Chrome/tabs/omnibox/live view/takeover/devtools/timeline/approvals |

Build features (SRC):
- `quarry-edge` default feature is `browser-agent` (`Q/crates/quarry-edge/Cargo.toml:69-71`).
- The production Dockerfile builds with `grpc,http3,postgres-queue,browser-agent` (`Q/deploy/compose/Dockerfile.edge:102`).
- Many durable behaviours (change tracking, durable receipts and checkpoints, durable frontier, Postgres profiles, event history) exist **only** under `postgres-queue` plus `DATABASE_URL`.

---

## 1. Acquisition API surface

**What exists (SRC, `Q/crates/quarry-edge/src/routes.rs:130-293`, `agent_routes.rs:3659-3729`, `fleet_routes.rs:219-222`).** There are about 75 public route entries, all JWT-gated:

| Endpoint | State | Notes / evidence |
|---|---|---|
| `POST /v1/scrape` (+`/v1/scrape/stream` SSE) | shipped | `routes.rs:398-530`, `:1067`. Request fields: `url, prev_fingerprint, cache, zdr, privacy, signals, ingest, render.waitForSelector/TimeoutMs, prefer_http3` (`routes.rs:345-375`). **No `formats`, `actions`, `onlyMainContent`, include/exclude tags, headers, location, mobile, timeout, maxAge.** The response is a `NormalizedOutput` with **artifact refs** for html/markdown, not inline text (`quarry-core/src/output.rs:131-147`, `pipeline.rs:690-704`). |
| `POST /v1/crawl` | shipped (thin) | `routes.rs:533-668`. Request is only `url, max_pages, max_depth, ingest, zdr`. Handoff to control and Temporal returns `{job_id, run_id?, accepted_at}` (`routes.rs:728-739`). Refused under ZDR (`routes.rs:575-588`). |
| `POST /v1/batch` | shipped (thin) | `routes.rs:670-726`. `urls, ingest, zdr`. Sequential workflow. |
| `POST /v1/map` | shipped | `map_routes.rs:132`. `/sitemap.xml` (plus up to 5 nested indexes), robots allow-filter, seed-page links, lexical rank by `search`, limit ≤ 1000. It does not read `Sitemap:` lines from robots and has no search-index assist. |
| `POST /v1/extract` | partial | `extract_routes.rs:184`. ≤25 URLs, **sequential**. Schema extraction via Model Plane `AiFormatRunner.json_for_org`. Without a schema it returns markdown; it always returns the `structured` JSON-LD/hydration harvest. `prompt` is accepted but is `#[allow(dead_code)]` (`:46-49`). There is no web-search expansion, no wildcard crawl and no async job. |
| `POST /v1/search` (+`/images`, `/videos`, `/similar`, `/suggest`) | shipped | `search_routes.rs:665,1092,1186,1360,1565`. Tavily-style params: `topic, time_range/days, exact_match, chunks_per_source, include_answer, format=context, highlight, facets, include/exclude_domains, intent, score_kinds, allow_paid_providers` (`:50-160`). |
| `POST /v1/answer` (+`/v1/answer/stream`) | shipped (gated) | `answer_routes.rs:44,145`. Search, then fetch, then Model Plane synthesis with citations. |
| `POST /v1/audio` | stub-gated | `audio_routes.rs:82,241`. TTS/STT proxy to Model Plane; manifest denies it by default (see §7). |
| Agent/interact: `POST /v1/agent/runs`, `/step`, `/interact` (alias of step), `/browser-session(/control,/timeline)`, `/tabs*`, `/frame`, `/frames/stream` (SSE), `/frames/ws` (WS + input), `/live-view`, `/events/stream`, `/devtools`, `/egress-receipts`, `/receipts`, `/procedure`, `/procedures/{replay,quality,impact}-check`, `DELETE /v1/agent/runs/:id` | shipped | `agent_routes.rs:3659-3729`. Detail in §9. |
| `/v1/fleets*` | partial | `fleet_routes.rs:219-222`. In-process registry plus budget. |
| Registry reads: `/v1/artifacts(/:id)`, `/v1/sources`, `/v1/snapshots`, `/v1/:kind/jobs`, `/v1/request-queues`, `/v1/benchmarks`, `/v1/team/{credit-usage,token-usage,concurrency,queue-status,activity}`, `/v1/runs/:id/events`, `/v1/jobs/:id/events` (SSE drain-and-close) | shipped (forwarding) | `resource_routes.rs:118-1130`. Most forward to control over HMAC. `/v1/jobs/:id/events` drains control once and closes (`resource_routes.rs:1050-1057`), so it is not a live tail. |
| Change: `/v1/change/{check,latest,history,snapshot,refresh}` | partial | `change_routes.rs:82-338`. See §2. |
| Schedules: `/v1/schedules` CRUD plus pause/unpause/trigger/backfill | partial | `schedule_routes.rs:239-428`. See §5. |
| `POST /graphql` (+schema/playground public) | stub | `graphql/schema.rs:71-95`. The only query `jobs` always returns an empty page. |
| Internal: `/v1/internal/run_page`, `/v1/internal/change/record`, `/v1/internal/page-images/...`, `/v1/internal/queues/:q/{enqueue,pop,ack}` | shipped | `routes.rs:104-122`, `queue_routes.rs:200-202`. Service-token plus HMAC org-binding (`routes.rs:898-968`). |

**Async jobs and status (SRC).**
- Crawl/batch return a `job_id`.
- Status comes from `/v1/:kind/jobs` and `/v1/jobs/:id/events`.
- There is **no** `GET /v1/crawl/:id`, `DELETE /v1/crawl/:id` (cancel), `/v1/crawl/:id/errors` or a result-pagination endpoint.
- Pause, resume and cancel exist only as Temporal signals inside the workflow (`workflows/signals.go:87-146`). No public route sends them.

**Webhooks (SRC).**
- The control `/v1/webhooks` CRUD is generic (`resources/webhooks.go:8-10`).
- The webhook model has **no `org_id`** (`store/store.go:376-383`).
- These routes are **not forwarded by edge**, so tenants cannot self-register crawl webhooks through the public surface.
- Delivery uses HMAC-SHA256 with backoff of 1s, 5s, 30s, 5m, 1h, max 5 attempts, then DLQ (`dispatcher/dispatcher.go:25-34`).
- Change-detected webhooks go edge → control → webhooks plus notification-core (`edge change_webhook.rs:36-60`, `control resources/change_webhook.go:52`).

**SDKs, CLI, MCP and skills (SRC).**
- OpenAPI-generated TS (`@quarry/client` 0.1.0) and Python clients exist, with 13 API classes (`Q/sdks/typescript/src/apis/*`, `Q/sdks/python/quarry_client/api/*`).
- `Q/docs/openapi.yaml` lists 46 paths. It is missing `/v1/profiles*`, `/v1/answer*`, `/v1/audio`, `/v1/search/{images,videos,similar,suggest}`, most `/v1/agent/runs/:id/*` browser routes, `/v1/fleets*` and `/graphql`.
- **No CLI, no MCP server, no skills.** MCP is explicitly deferred in `Q/docs/OSS_PARITY_BACKLOG.md` §2 "Skip".

**Service-principal scoping (SRC, `auth.rs:504-545`).** Service tokens (for example Model Plane) may call only `/v1/scrape`, `/v1/search`, `/v1/extract`, `/v1/artifacts/:id` and `/v1/agent/*`. `/v1/map`, `/v1/crawl`, `/v1/answer` and the others are user-token-only.

Maturity: **broad and real**, but the request shapes are much thinner than Firecrawl v2. The async lifecycle (status/cancel/errors/webhook-per-job) is incomplete.

## 2. Output formats & content processing

**Scrape pipeline (SRC, `Q/crates/quarry-runtime/src/pipeline.rs:298-1000`).** The order is:
1. Security preflight, DNS pin, global and per-host slots.
2. Driver fetch.
3. 16 MB cap.
4. Charset sniff (`quarry-transform/charset.rs`).
5. Soft-404 detect (`soft_404.rs:97`), which emits a `page_blocked` event.
6. Readability markdown (`readability.rs:235`, a Mozilla-Readability subset that strips nav/header/footer/aside/ARIA landmarks and images, then html2md).
7. Links (`links.rs`, absolute, with text/rel and **no scoping**).
8. Metadata (`metadata.rs:12`: title/desc/author/published/modified/keywords/canonical/og_image/robots, JSON-LD fallbacks).
9. Language detect (whatlang).
10. blake3 byte fingerprint plus text fingerprint.
11. New/changed/unchanged event.
12. Rendered branding (`branding_rendered.rs:60`: favicon, theme colour, palette, logo, font, og:image).
13. Artifacts `html`, `markdown`, `meta.json` (driver plan).
14. `page_extracted` (title provenance html/model/host, 300-char excerpt, word count).
15. Optional Model Plane title enrichment.
16. Local Tantivy write-back.
17. Optional Data Plane ingest with paragraph chunks (1000 chars) and a field-level `SourceTrace`.

**What the scrape response carries (SRC, `output.rs:131-147`, `pipeline.rs:690-704`).**
- Carried: `html` and `markdown` as `FormatRef{artifact_id, bytes}`, plus `links`, `metadata`, `branding`, `etag/last_modified`, `extraction`, `determinism` stamp and `privacy`.
- **Always `None`:** `raw`, `screenshot`, `pdf`, `extract`.
- `DriverSignals.screenshot/pdf=true` only forces the browser driver (`driver_plan.rs:36-49`). The browser adapter never captures a screenshot (`browser_driver.rs` only does goto/wait_for/content).
- **Under ZDR no artifacts are written, so a ZDR scrape returns no page text at all.** It returns only links, metadata and the extraction excerpt (`pipeline.rs:599-690`). [SRC]

**Format coverage versus code:**

| Format | State |
|---|---|
| markdown, html | via artifact fetch (`GET /v1/artifacts/:id`) |
| links | inline |
| metadata | inline |
| branding (static HTML) | inline |
| rawHtml | the `html` artifact *is* the raw body (`pipeline.rs:603-605`); no separate cleaned-HTML format |
| screenshot, pdf | **agent runs only** (`AgentAction::Screenshot/Pdf`) |
| images, attributes | library only; `images.rs` (lazy/srcset) and `attributes.rs` (hreflang, Dublin Core, tables) are unused except `attributes` in the unused crawl frontier |
| json/summary/query LLM formats | `ai_formats.rs` exists; json is used by `/v1/extract`, summary by `/v1/answer`; **not** selectable on scrape |
| chunks | ingest-side only (`chunk.rs`) |

**Documents (SRC).**
- PDF text plus font-size headings via lopdf (`quarry-transform/src/pdf.rs`) is used **only** by the answer pipeline fetcher (`answer.rs:839`). The scrape pipeline runs readability over any body, including a PDF.
- DOCX (`docx.rs`) is library-only and unused.
- No XLSX/PPTX/ODT/RTF/EPUB.

**Structured extraction (SRC).**
- `/v1/extract` with a schema goes to Model Plane and is validated with `quarry_core::json_schema`, returning `schema_issues` (`ai_formats.rs:129-188`).
- A non-LLM `structured` harvester (JSON-LD, hydration state, key figures, tables) runs on every extract and answer source (`quarry-transform/src/structured.rs:343`).
- Agent observations accept a deterministic `ExtractionProfile` (JSON-LD/DOM/AX sources; NetworkJson/Visual declared but skipped, `observation.rs:2288`).

**Change tracking and diffing (SRC).**
- Byte fingerprint compare plus a baseline chain in Postgres (`postgres_baseline_store.rs:27`, `change_routes.rs:459-560`).
- `/v1/change/check` **does not fetch**. It requires the caller to supply `fresh_fingerprint`, and without `postgres-queue` it returns 501 (`change_routes.rs:82-121`).
- The scheduled ChangeMonitorWF fetches via run_page and records (`workflows.go:305-357`, `activities.go:393-456`).
- The paragraph "semantic diff" `diff_paragraphs`/`SemanticDiff::is_real_delta` (`quarry-transform/src/diff.rs:72`) is **not called anywhere**. A change is a raw-byte blake3 inequality, so dynamic tokens or timestamps count as changes. A `text_fingerprint` is emitted in events but not used for the decision (`pipeline.rs:516-540`).
- There is no JSON-mode or git-diff output and no visual diff for monitors (visual diff exists only for agent observations).

**Determinism (SRC).** `RunPolicy` fingerprint plus identity stamp on every output (`policy.rs`, `pipeline.rs:742-758`). It is an audit stamp only; sub-policies such as `respect_robots` are **not enforced** (see §4).

Maturity:
- Content cleaning and metadata: **solid**.
- Format breadth on `/scrape`: **far below Firecrawl**.
- Change tracking: **MVP**, hash-based and Postgres-only.

## 3. Rendering & browser engines

**Driver waterfall (SRC).**
- `DriverPlan` static, then TLS, then browser (`driver_plan.rs:36-95`) is chosen from `DriverSignals{actions, screenshot, pdf, prior_block_signals, profile_required}`.
- `FallbackDriver` (`fallback_driver.rs:56-260`) rotates on block status (401/403/407/429/503). It also detects JS shells (`needs_browser_escalation`, `:387`) and escalates to the browser, keeping the best shell.
- Optional HTTP/3 transport preference (`routes.rs:38-56`, `http3.rs:47`).
- Registered drivers (`main.rs:288-340`):
  - `StaticDriver` (reqwest/rustls, pinned DNS, hop-by-hop redirect re-validation, `fetch.rs:48,193-217`).
  - `TlsProfileDriver` with the **Chrome** profile only (wreq/BoringSSL JA3/JA4/H2, `quarry-tls/src/lib.rs`, `tls_driver.rs:21`).
  - Browser: **Browserbase** if keys are present, else **local chromiumoxide** via `BrowserDriverAdapter` (`browser_driver.rs:75`).
- **Kernel and Browserless drivers exist in the crate (`kernel.rs`, `browserless.rs`) but are not registered anywhere** (no reference in edge/runtime).

**Local Chromium (SRC, `Q/crates/quarry-browser/src/chromiumoxide.rs`).**
- One shared Chromium process. Each session gets an isolated CDP `BrowserContext` (cookie jar per session, shared across that session's tabs, `:98-110`, `:446-475`).
- Per-session private download dir.
- Per-session loopback DNS-pinned HTTP/CONNECT egress proxy with QUIC disabled (`:186-243`, `browser_egress_proxy.rs:37`).
- CDP Fetch network guard (`:706`).
- Dialog collector (`:822`).
- DevTools collectors: console, log, request, response, lifecycle (`:575`, `:1446-1610`).
- AX-tree projection ≤300 nodes plus frame topology (`:1086-1200`).
- Atomic native-target actions by backend node id (`:1232`).
- Live JPEG/PNG frames with a 250 ms cache (`:1702`).
- Screenshot/PDF (`:2392-2445`).
- Tabs: list/new/select/close (`:2447-2590`).
- Renderer telemetry (CPU, heap, `:2651`).
- **Limitation (INF from `ensure_browser` `:194-226`):** a lease with a different viewport than the running browser **closes and relaunches the whole browser and clears all sessions' pages**. Concurrent sessions with mixed viewports therefore disrupt each other.

**Capabilities declared (SRC, `chromiumoxide.rs:1873-1891`).**
- `persistent_profile`, `devtools_trace`, `full_visual_fidelity`, `atomic_target_actions` are true.
- `isolated_egress` and `security_evidence` are true only with the pinned proxy.
- `downloads_to_artifacts=false` and `uploads_from_artifacts=false`: the code exists (`:2102`, `:2211`) but is fail-closed, so agent upload/download actions are rejected (`agent_routes.rs:1220-1268`).

**Stealth and fingerprinting (SRC).**
- `quarry-browser/src/stealth.rs` defines `STEALTH_SCRIPT` (webdriver, chrome shim, plugins, canvas/WebGL/audio jitter) and `CONSENT_DISMISS_SCRIPT` (OneTrust, Cookiebot, ...).
- **Neither is injected by any driver.** The only references are the module's own tests (grep across all crates).
- Chromium launches with only crash and dev-shm flags (`:226-244`); there is no UA/locale/timezone/geolocation override.
- TLS impersonation in the HTTP drivers is real (wreq). `fingerprint_rotation.rs` provides block detection; only one TLS profile is registered, so "rotation" means driver hopping.

**Page actions.**
- Full CDP action set for agent runs (§9).
- For **scrape** only `render.waitForSelector` plus an automatic hydration-settle (`browser_driver.rs:209-245`).
- `ActionRuntime`/`ActionScript` (Firecrawl-style actions with `on_error=retry`) exists (`action_runtime.rs`) but **no route invokes it**.

**Sessions, profiles and cookies (SRC).**
- `SessionSnapshot{cookies, local/session storage, indexed_db, UA, viewport, locale, tz}` (`session.rs:16-50`).
- Capture on release (`chromiumoxide.rs:675,1771`): cookies drop `expires`; storage only for the current origin; IndexedDB is **never captured** (`indexed_db: vec![]`).
- Restore hydrates cookies plus storage only (`:925-968`); captured UA, locale and timezone are not re-applied.
- Profile stores: in-memory or Postgres, plus a Redis read-through cache (`main.rs:551-600`). `S3ProfileStore` exists (`s3_profile_store.rs`) but is not wired.
- Profiles REST: save/create/list/load/patch/delete/restore_probe (`profile_routes.rs:102-432`), with scope `ephemeral/user/org...`.
- **No at-rest encryption of cookie/profile snapshots** (no crypto references in any profile store).

**Auth and login handling.** Detection only: `ChallengeKind::{Captcha, Waf, Login, Consent, RateLimit, AccessDenied}` (`observation.rs:158-230`). Login is otherwise user takeover plus a persisted profile. There is no credential vault or auto-login.

**Geolocation, locale and mobile.** `viewport.is_mobile` maps to CDP `emulating_mobile` (`chromiumoxide.rs:1851-1860`). Search has `country`/`language`. There is **no geo-IP proxy selection, no browser geolocation and no locale/timezone emulation**.

Maturity:
- Local Chromium agent path: **strong on governance/evidence, weak on stealth**.
- Remote providers: **gated off** for agent runs (no `isolated_egress`).

## 4. Anti-bot, proxies & reliability

| Item | State (SRC) |
|---|---|
| Proxy pool | `QUARRY_PROXY_POOL` lists socks5/socks5h/http/https; sticky (org, host) hash (`proxy_pool.rs`); first-party vs third-party processor approval via `PrivacyPolicy` (`main.rs:267-294`); per-run proxy affinity for agents (`proxy_affinity.rs`, `agent_routes.rs:1490-1500`). The `EgressBroker` health-orders `(host, proxy)` pairs (`egress_broker.rs`). No residential/provider integrations, no geo targeting. |
| CAPTCHA | Detect and report only (`observation.rs:170-175`); no solving, by design (DOC `OSS_PARITY_BACKLOG.md` ⛔). |
| Rate limits / politeness | AIMD per-host `HostScheduler` (`host_scheduler.rs`) plus global `AutoscaledPool` (`autoscale.rs`), both wired into `PageRunner` (`pipeline.rs:330-400`). |
| robots.txt | Parser (`quarry-transform/robots.rs`) and `RobotsCache` (`robots_cache.rs`) exist; **enforced only in `/v1/map` filtering** (`map_routes.rs:170-260`). **Not enforced** on scrape, crawl or batch. `RunPolicy.discovery.respect_robots=true` is only fingerprinted (`policy.rs:60-78`). |
| SSRF / compliance | Security preflight heuristics, blocklist and urlsig (`quarry-security`); DNS resolve-once-and-pin (`dns_guard.rs:104`); redirect re-validation (`fetch.rs:193-217`); browser DNS-pinned proxy plus CDP Fetch guard (§3). `/v1/extract` and `/v1/map` call drivers without `security.preflight` (they rely on the driver-level DNS guard) [SRC `extract_routes.rs:140-150`, INF]. |
| Retries / backoff | FallbackDriver chain, plus Temporal activity retry (5 attempts, exponential, `workflows.go:94-104`). `retry.rs` (`execute_with_retry`) is exported but unused. |
| Blocklists | Control `/v1/blocklists` (Postgres) plus the edge security snapshot file (`main.rs:342-360`). |
| Caching / freshness | Redis cache keyed by **content fingerprint**, hit only when the caller sends `prev_fingerprint` (`cache.rs:1-7`, `routes.rs:430-436`). There is no URL-keyed `maxAge` cache. Conditional GET hints exist but are not populated by edge. The SWR claimed in `PRESETS.md` is absent (no `stale_while_revalidate` use). The search router has its own 5-minute TTL cache. |
| Soft-404, body cap, charset | shipped (`pipeline.rs:430-500`) |

Maturity: SSRF and egress are **best-in-class**. Politeness is **good**. Robots compliance on crawls is **missing**. Anti-bot is **TLS-level only**.

## 5. Orchestration & scale

**Crawl (SRC, `Q/services/quarry-orchestrator/internal/workflows/workflows.go:396-640`).**
- BFS loop with `max_depth` and `max_pages`, pause/resume/cancel signals and progress query.
- Checkpoint every 50 pages.
- Two modes:
  - In-workflow slice.
  - Durable frontier via the Rust Postgres queue bridge (`/v1/internal/queues/*`, `postgres_queue.rs` SKIP LOCKED). This is used when edge credentials are set (`activities.go:67`).
- **The workflow enqueues every extracted link with no host/subdomain scoping, no include/exclude, no robots check, no canonicalisation beyond exact-string dedupe, and no sitemap seeding.** One page at a time, with no per-crawl concurrency.
- The Rust `CrawlFrontier` (scope, include/exclude globs, typed `CrawlDenialReason`, hreflang, checkpoints; `crawl_frontier.rs:54-380`) and `CrawlRanker` exist but **are not used by any live path**. INF: an unconstrained crawl seeded on a page with external links walks off-site until `max_pages`.

**Batch.** Sequential loop (`workflows.go:359-394`).

**Jobs dispatch.** The orchestrator polls control every 2s and starts workflows (`internal/jobs/dispatcher.go:128-313`, `main.go:144-151`).

**Scheduling.**
- Edge accepts cron or `schedule_at` or a change-monitor `preset` of hourly/daily/weekly (`schedule_routes.rs:239-340`).
- Control maps **only `target_kind=change_monitor`** to a Temporal workflow. Scrape/crawl/batch schedules return no workflow and are skipped (`resources/schedule_wire.go:39-51`).
- Trigger and backfill always return 501 because control is constructed with a nil Temporal client (`cmd/control/main.go:192`, `resources/cycle23.go:278-322`).
- Pause and unpause toggle `enabled`. The reconciler stamps an org memo and reaps org-scoped schedules (`schedules/schedules.go:104-330`).
- Cron grammar: strict 5-field (`internal/cron/cron.go`).

**Concurrency and queues.**
- Per-host AIMD plus global autoscale on the fast path.
- `InMemoryRequestQueue`/`PostgresRequestQueue` with priority, visibility timeouts and stats (`request_queue.rs:44-150`, `postgres_queue.rs:74`).
- `/v1/request-queues` read view.
- Agent runs live in an in-process map (`agent_routes.rs:337-344`); the local Chromium adapter lease pool has size 2 (`main.rs:337`).
- **No idle reaper** for abandoned agent runs. `ttl_s` is set on the lease but nothing enforces it (grep: no reaper/sweeper; INF).

**Budgets and limits.**
- Agent `max_steps` is enforced (`agent_routes.rs:446-451`).
- **`max_cost_usd` and `max_runtime_s` are not enforced on the HTTP/WS step path.** Enforcement lives in `AgentLoop` (`agent_loop.rs:155-216`), which no route uses; the code comment at `agent_routes.rs:1370-1372` claims otherwise.
- Default constraints are unbounded (`:433-440`).

**Fleets.** `/v1/fleets` uses an in-process registry with a budget aggregate (`fleet_routes.rs`, `runtime/fleet.rs`). The Go `internal/fleet` registry is unreferenced by workflows.

Maturity:
- Temporal durability: **good**.
- Crawl semantics (scope, robots, include/exclude, sitemap, concurrency): **missing in the live path**.
- Recurring crawls: **not executable**.

## 6. Platform & ecosystem

- **Actors / marketplace / store: absent.** Domain-specific collectors exist as **library-only Go packages, not mounted or scheduled**:
  - NAV job-vacancy feed (`quarry-control/internal/navacancies`).
  - TED and Doffin procurement (`internal/procurement`).
  - Official statistics (`internal/officialseries`).
  - Legal and parliamentary revisions (`internal/revisions`).
  - Data.norge DCAT catalog diff (`internal/catalogwatch`).
  - Grep shows no importer. These are the nearest thing to Apify "actors".
- **Presets / templates:**
  - Go control has 4 RunPolicy presets (`fast/polite/stealth/deterministic`) at `/v1/presets` on control only, not edge (`resources/presets.go:26-60`).
  - Rust `quarry-core/presets.rs` defines 6 domain bundles (docs-site, pricing-monitor, ...) that are **unused**.
  - `preset` on schedules means a change-monitor cadence only.
- **Storage:**
  - Artifacts: memory/FS/S3 (`artifact_store.rs:171,338,483`); a CAS store for page images.
  - Snapshots, sources ("tracked websites", materialised by crawl `run_page` via `EdgeSourceRegistrar`, `routes.rs:1003-1015`).
  - Local Tantivy corpus (`local_index.rs:166`).
  - No Apify-style dataset or key-value store API.
- **Integrations / connectors:**
  - Data Plane ingest (HTTP or gRPC, `ingest_client.rs`, `grpc/ingest_adapter.rs`).
  - Data Plane vector retrieve and rerank.
  - Visual-RAG page images to the DP embedding engine (`page_renderer.rs`, `page_image.rs`).
  - Model Plane (gated).
  - billing-core usage over NATS (`usage.rs:89-93`: `quarry.scrape.page`, `crawl.seed`, `search.query`, `answer.synth`, `batch.url`).
  - notification-core on change.
  - GDPR erasure subscriber (`gdpr/subscriber.go:48`).
- **Versioning / builds:** `/version` with git SHA (`routes.rs:336-343`). No per-actor build system.
- **Teams / orgs:** JWT org/user/service principals. `/v1/team/*` aggregates forward to control.
- **Monetisation:** usage metering only. No credit enforcement in Quarry.

## 7. Observability & operations

- **Events:**
  - Typed event envelope (`quarry-core/event.rs`).
  - In-process `EventSink` plus NATS bus (`nats_event_bus.rs:79`).
  - Durable Postgres event history (`postgres_event_history.rs:31`) under `postgres-queue`, plus `durable_event_history`.
  - ZDR events reach live subscribers only (`emit_for_zdr`, `pipeline.rs:110-118`).
- **Run history / replay:**
  - Control job event log.
  - Agent step receipts (immutable, actor-scoped, `step_receipts.rs:183-560`, migrations `0007-0009`).
  - Browser timeline (control/tab/lifecycle events).
  - Run checkpoints (`0006`).
  - `/browser-session/timeline` cursor API (`agent_routes.rs:1787`).
  - Agent events SSE with `?from_seq` replay (`:2658`).
- **Metrics:**
  - **Edge has none.** `metrics`/`metrics-exporter-prometheus` are inert and no `/metrics` route is served (`quarry-edge/src/telemetry.rs:6-14`).
  - OTLP tracing when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
  - `quarryotel` Go package.
  - integration-corev2 *does* serve `/metrics` (`IC/internal/api/server.go:178`).
- **Alerts:** absent in Quarry.
- **Cost tracking:**
  - Flat per-action cost table (`action_cost.rs`).
  - Receipt cost, `BrowserTelemetry.verified_action_cost_micro_usd` (None unless provider-metered).
  - Usage events to billing.
- **Self-hosting / deploy:**
  - `Q/deploy/compose/docker-compose.yml` runs postgres, dragonfly, temporal, quarry-edge, vision-sidecar, control, orchestrator, searxng.
  - Pinned Chromium in `Dockerfile.edge`.
  - `SELF_HOST.md`, `RUNBOOKS.md`.
  - `/ready` reports durable readiness (`routes.rs:312-334`); production refuses to start with ephemeral stores (`main.rs:690-704`) or a default/empty control key (`control main.go:63-117`).
- **Multi-tenancy:**
  - JWT org scoping, with client `org_id` ignored (`routes.rs:414-418`).
  - Agent runs scoped to org **and actor** (`agent_routes.rs:474-486`).
  - Artifacts tenant-stamped.
  - Control HMAC internal auth, with rollout mode outside production (`ARCHITECTURE.md` access rule).
  - Gap: control webhooks are not tenant-scoped (§1).
- **Model Plane gating:**
  - Every Model-Plane-backed feature (LLM intent, autoprompt, page-title enrich, answer synthesis, `/v1/extract` schema, suggest, audio) is **off by default**.
  - The cross-plane token manifest grants `model-invoke`, `model-stream` and `model-audio` as **false** (`Q/crates/quarry-runtime/contracts/cross-plane-token-requests.json:40-73`, gate `quarry-edge/src/model_plane_gate.rs:1-38`).
  - Only the dev-bypass static token path works. Data Plane rows (`vector-retrieve`, `ingest`, `web-rerank`) are granted.
- **Security / sandboxing:**
  - Chromium sandbox on by default (`QUARRY_BROWSER_NO_SANDBOX` opt-out).
  - Per-session browser contexts.
  - Egress proxy.
  - No per-session VM or container isolation for local Chromium (Kernel would have provided it, but it is unwired).
- **Evals / benchmarks:**
  - `Q/lab/evals` (Rust): bakeoff, browser_benchmark, provider_matrix, release_matrix, web_rerank_gate (3689 lines), fixtures.
  - The scoreboard is `_pending_` for every suite (`docs/SCOREBOARD.md`).

## 8. In-app browser shell (user-facing)

Quarry is the backend. The shell is Verevon v3. [SRC]

| Feature | State | Evidence |
|---|---|---|
| Browser chrome with tabs (list/new/select/close) | shipped | UI `BrowserChrome.tsx:1083-1134`; gateway `browser.rs:385-396`; Quarry `agent_routes.rs:3697-3702` |
| Address bar / omnibox | shipped (simple) | `browser-omnibox.ts:3-15`. A URL or host becomes https; free text becomes a **google.com search URL**, not Quarry `/v1/search`. |
| Back / forward / reload | shipped | aria labels in `BrowserChrome.tsx` (`AgentAction::Back/Forward`) |
| Interactive live page (click/scroll/keyboard on the frame) | shipped | `BrowserChrome.tsx:1870`, `:1740`. Gateway proxies `/frames/ws` (`browser.rs:428-430`); Quarry WS accepts `Action` messages (`agent_routes.rs:2968-3195`). |
| Human takeover toggle | shipped (UI-enforced) | `BrowserChrome.tsx:497,559-560,624,1228-1236`. Quarry records `Control{mode}` timeline events (`agent_routes.rs:1733-1785`) but **does not block agent `/step` while `human_takeover`** (no `control_mode` check in `step`, `:2228-2410`). Human actions go through the same step path with the instruction text "Human browser takeover action" (`BrowserChrome.tsx:720`), so receipts do not type the initiator. |
| Profiles manager (create/rename/delete/scope/restore-probe), privacy/ZDR badges | shipped | `BrowserChrome.tsx` (77 profile refs); gateway `browser.rs:440-455`; Quarry `profile_routes.rs` |
| DevTools panel (console/network/lifecycle) | shipped | `BrowserChrome.tsx:1912-1940`; `agent_routes.rs:3703`, `:3454` |
| Timeline / evidence / "Agent replay" view | shipped (read-only) | `BrowserChrome.tsx:2090,2121`, `BrowserTimelineDetail.tsx` |
| Screenshots (manual) and visual observation | shipped | `BrowserChrome.tsx` ("Ta skjermbilde"); vision sidecar |
| Approvals queue in browser UI | shipped (Model Plane-owned) | gateway header `browser.rs:1-25`: approvals via `/api/v1/orchestration/...`, risk classification in Model Plane `execution-core/src/browser_agent/browser_risk.rs` |
| Embedded in chat live-run panel | shipped | `V3/src/features/chat/components/ChatLiveRunPanel.tsx:39-40,608` |
| History / bookmarks / downloads manager / extensions / password autofill / reader mode / find-in-page / zoom / split view / annotate-on-page / recording | **absent** | grep of `BrowserChrome.tsx`: 0 hits for bookmark, download, upload, reader, zoom, annotat, recording. "Nettleserhistorikk" is the action timeline, not URL history. |
| Permission prompts (geo/camera/notifications) | absent | no CDP permission handling found in `chromiumoxide.rs` |

## 9. AI-browser control

**Protocol (SRC, `Q/crates/quarry-core/src/contracts.rs`).**
- Model Plane proposes one `AgentAction` per `/step` and receives a `BrowserObservation` (§1, `:19-97`).
- The observation carries:
  - URL and title.
  - `BrowserSnapshot{snapshot_id, generation, targets[@eN refs], accessibility (CDP AX projection), frames (origin-only)}` (`:366-460`).
  - `DomSummary` with a click map.
  - Screenshot, visual and evidence-delta artifact ids.
  - Console and network summaries.
  - Redacted egress receipts.
  - Open dialogs.
  - Policy denials.
  - `ActionOutcome{verified|failed|unknown}`.
  - `ObservationDelta`.
  - `ChallengeSignal`.
  - Extraction result.
  - `ProofBundle`.
  - `ResolvedTargetProof`.
  - `BrowserTelemetry` (startup, latency, renderer CPU/heap, snapshot token estimate, challenge rate, provider cost).

**Actions (`contracts.rs:640-827`):**
- navigate.
- click/type/select/wait_for by raw CSS, by snapshot ref, by **frame ref** and by **semantic locator** (role/text/label/placeholder/testId/nth).
- click_point.
- mouse_wheel.
- press.
- scroll.
- wait.
- respond_dialog.
- upload_ref / download_ref (+frame variants; driver-gated off).
- screenshot.
- pdf.
- **evaluate (arbitrary JS)**.
- back/forward.
- get_content.

**Element targeting.**
- Opaque `@eN` refs bound to CDP backend node ids, which never leave Quarry.
- Stale, absent or ambiguous refs fail closed with `TARGET_REPAIR_REQUIRED` (`observation.rs:1019`).
- Atomic verify-and-act in the driver (`chromiumoxide.rs:1232`).
- Read-only fingerprint repair scoring (`target_repair.rs`).

**Safety and authority:**
- BrowserBroker grant required in production (`main.rs:183-198`), validated via HTTP or gRPC (`grant_validator.rs`) on start and **before every action** (`agent_routes.rs:2257-2266`).
- In governed mode `allowed_domains` come only from the broker grant and are re-checked for exact equality (`:314-339`, `:1377-1414`).
- Pre- and post-action domain checks (`:2294-2400`).
- Sensitive actions (dialog, upload, download) need a **fresh one-shot approval grant** scoped to the exact frame, dialog or artifact (`:135-312`).
- Snapshot-action capability gating (`:1220-1268`).
- Agent runs refused unless the driver proves isolated egress (`:1131-1141`).
- ZDR: no durable receipts, checkpoints or timeline; no persistent profile allowed (`:527-534`, `:1460-1466`); ZDR runs not resumable (`:1347-1353`).
- "Ephemeral evidence" lab tier behind `QUARRY_LITE_EXPERIMENTAL` (`:1143-1178`, `:1283-1298`).
- **Gaps:**
  - `evaluate` and raw-CSS actions remain available on normal runs without approval [SRC `observation.rs:243-260`].
  - No prompt-injection labelling or detection exists in observations (grep: none). The design is in `QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026.md` §20.
  - `agent_memory::redact_sensitive` and the scratchpad are unwired.

**User takeover / co-browsing.**
- Control-mode transfer is recorded.
- Live frames over SSE and WS.
- Human input over WS.
- Provider live-view URLs (`live_view`, `:2608`; local Chromium returns None).

**Multi-tab and multi-session.**
- Tabs per run (one context).
- Many runs in parallel.
- Fleets for budgets.
- `window.open` is rewritten into same-tab navigation (`chromiumoxide.rs:969-1005`).

**Record / replay.**
- Receipts compile into a deterministic `BrowserProcedure` (`browser_procedure.rs:61`).
- Replay check, quality check and impact check endpoints exist.
- **There is no procedure store and no "execute procedure" endpoint**: compile and check only (`agent_routes.rs:2466-2538`).
- Run resume from checkpoint (`resume_run_id`, `:1308-1361`, `:1551-1585`).

**Credential vaults.** Absent. No secret-reference action. Credentials can only live in captured cookies, which are unencrypted (§3).

**Planner.** It is in the Model Plane (`execution-core/src/browser_agent.rs`, `llm_planner.rs`, `quarry_agent.rs`; gateway `/suggestions` and `/ai-runs`, `browser.rs:397-410`). Quarry's own `AgentLoop`/`Planner`/`ModelPlanePlanner` exist but are unused by routes.

## 10. Integration-plane overlap (integration-corev2)

**Connectors and OAuth (SRC).**
- First-party broker that replaces Nango (`IC/README.md`).
- Catalog keys:
  - microsoft, google, slack, notion, github, shopify, stripe, okta, scim.
  - meta: facebook, instagram, messenger, whatsapp, threads, ads.
  - linkedin, tiktok, snapchat, x, discord, shipping.
  - openai-codex-subscription.
  - Source: `IC/internal/providers/catalog.go:97-300`.
- Connect/reconnect sessions, OAuth callback, capability bundles, consents, discovery (`server.go:256-540`).
- AES-GCM token vault (`IC/internal/crypto/vault.go:16-20`).
- Internal short-lived token broker `POST /internal/connectors/token` (`server.go:1315`).

**Provider actions (SRC).**
- About 142 named operations in one `Execute` switch (`IC/internal/actions/service.go:164`).
- Signed write attestations (`IC/internal/attestation/verifier.go`).
- Raw provider proxy is refused with 410 (`IC/internal/api/legacy_routes.go:69-74`).
- Frozen contract `docs/actions-surface-operations.md`.

**Webhooks and triggers (SRC).**
- `POST /api/v1/webhooks/:provider`, mail-task webhooks (`server.go:933-1101`).
- Rust `webhook-normalizer-rs` hot path.
- Sync jobs with claim/progress/cancel/retry plus SSE snapshots (`server.go:589-930`).
- Email/Teams/Slack/Discord sync (`IC/internal/emailsync`).

**Data into knowledge (SRC).** Data Plane documents handoff client (`IC/internal/handoff/dataplane.go:92-182`), plus finspo SharePoint worker.

**ZDR.**
- Per-connection ZDR assessment and acknowledgement API, **dark by default** (`ZDR_ACKNOWLEDGEMENT_ENABLED`, `IC/internal/api/zdr_acknowledgement.go:1-60`).
- Codex-subscription inference routes for Model Plane (`codex_subscription.go:36-237`).

**SSRF.** Single-resolution guard, equivalent to Quarry's (`IC/internal/egress/guard.go`).

**MCP / tool exposure.** integration-corev2 exposes none. MCP-server registration is a Model Plane/gateway concern (`V3/apps/gateway/src/domains/mcp.rs:1-20`).

**Overlap with Quarry.**
- **None at runtime.** There is no code path where a Quarry browser run uses a connector token, and no path where a connector sync uses Quarry.
- Authenticated web access is either OAuth API actions (IC) or browser cookies (Quarry profiles); the two never meet.
- A unified "connected account" concept for browsing is absent [SRC grep `quarry` in IC finds only a comment in `egress/guard.go:23`].

---

## Planned / in-flight per docs

- **Browser automation program** (`Q/QUARRY_V2_BROWSER_AUTOMATION_IMPROVEMENTS_2026_OWNERSHIP_RECONCILED.md` §48, ledger §51). Next proof items:
  - Native AX/OOPIF stale-root behaviour.
  - Remote-provider containment proof (Browserbase/Kernel/Browserless).
  - Artifact upload/download quarantine.
  - Execution continuation for approvals.
  - Typed failure codes.
  - Readiness/image-digest probes.
  - Prompt-injection containment (§20: observation labelling, injection signals, eval).
  - Credential secret references (§19).
  - WebDriver BiDi adapter (§5.2).
  - Set-of-Marks (§15).
  - Site profiles / adaptive target memory (§25, §38).
  - Change intelligence (§43).
  - Quality OS (§44).
  - BrowserBench (§27).
  - Lightpanda/Patchright/Camoufox/Steel lab benchmarks (§48 P3).
- **Sovereign reliability layer** (`Q/docs/SOVEREIGN_SCRAPING_RELIABILITY.md`): domain intelligence registry, EgressBroker v2, session health, owned browser fleet, challenge classifier, fingerprint catalog.
- **Web-agent gates** (`Q/docs/WEB_AGENT_ROADMAP_EXECUTION.md`): crash/restart tests for `resume_run_id`, remote-provider SSRF tests, network-JSON/visual extraction profiles, persistent procedure rollout state, versioned SDK publishing.
- **Roadmap** (`Q/docs/ROADMAP.md`):
  - Phase 5: Rust crawl workers with sitemap/robots/include/exclude/denials.
  - Phase 6: Firecrawl format parity including images/attributes/summary/query/changeTracking.
  - Phase 7: presets driving DriverPlan.
  - Phase 8: benchmarks.
  - Phase 9: interact/session APIs, SDK ergonomics, crawl status/webhook parity.
- **Presets** (`Q/docs/PRESETS.md` "Pending wiring"): `request.preset` on scrape, Postgres org-custom presets.
- **GAP.md** §15.7-15.8: long-running `/v1/agent` job, `/v1/deep-research` (Post-V2).
- **Model Plane gate** (`model_plane_gate.rs:12-15`): granting the `model-invoke` manifest row, plus DEF-QB-T17-PART2 for an org-bound metered model path.
- **Program plan** (`docs/BROWSER_WORKSPACE_PLAN.md`): marked COMPLETE 2026-07-09. Its open items are file-transfer risk classes, live SPA click-test of HITL, and wiring the vision sidecar into compose.

## Known gaps (code-verified)

1. **Crawl scope:** the live crawl has no host/subdomain scoping, include/exclude, robots, sitemap or canonicalisation (`workflows.go:396-610`). The Rust `CrawlFrontier` and `RobotsCache` are unused.
2. **robots.txt** is not respected on scrape, crawl or batch; it is used only for `/v1/map` filtering.
3. **Scrape request/response is thin:**
   - No `formats`, `actions`, `onlyMainContent`, tag filters, headers, location or timeout.
   - Markdown/html only via artifact fetch.
   - No screenshot/pdf/images/raw/json/summary on scrape.
   - **ZDR scrape returns no content.**
4. **Recurring crawls/scrapes are not executable.** Only change-monitor schedules map to a workflow. Trigger and backfill always return 501.
5. **No public async-job lifecycle:** no crawl status/cancel/errors routes; no per-job webhook; webhooks are not tenant-scoped and not exposed via edge.
6. **Change tracking** is raw-byte hash based; the semantic diff is unwired; `/v1/change/check` does not fetch; it requires `postgres-queue`.
7. **Stealth and consent scripts are dead code.** No locale/timezone/geolocation/UA emulation. Only the Chrome TLS profile is registered. No residential/geo proxies.
8. **Kernel and Browserless drivers are unwired.** Remote providers cannot run agent sessions (no egress proof). The S3 profile store is unwired.
9. **Profiles:** no IndexedDB capture, cookie expiry dropped, no at-rest encryption, UA/locale/timezone not restored.
10. **Agent budgets:** `max_cost_usd` and `max_runtime_s` are not enforced on the step path; no idle-run reaper; viewport change relaunches the shared Chromium.
11. **Human takeover is advisory server-side.** Receipts do not type the initiator as human or agent.
12. **No prompt-injection defences.** `evaluate` and raw-CSS actions are allowed without approval. Redaction is unwired.
13. **Procedures** are compile/check only: no store, no replay execution.
14. **Model-Plane-backed features** (answer synthesis, schema extract, suggest, audio, LLM intent, title enrich) are disabled by the shipped token manifest except in dev bypass.
15. **Docs and libraries without callers:** PDF only on the answer path; DOCX/images/attributes/synonyms/source_trace/determinism verify unwired; `ActionRuntime`, `AgentLoop`, `retry.rs` and the control domain collectors are unwired.
16. **Observability:** no `/metrics` on edge; GraphQL is a stub; OpenAPI covers about 60% of routes; no CLI/MCP; benchmarks not run.
17. **In-app browser shell:** no URL history, bookmarks, downloads, extensions, autofill/vault, reader mode, find, zoom, split, page annotation or recording. The omnibox uses Google for free text.
18. **No connector ↔ browser bridge** between integration-corev2 and Quarry.

## Architecture constraints that limit what Verevon can adopt

- **Plane authority:**
  - Model Plane plans and proposes; Quarry executes or rejects (CLAUDE.md rule; enforced by the `/step` one-action contract and BrowserBroker grant revalidation).
  - A Codex/Aside-style *in-process* agent loop inside the browser layer is off-limits.
  - Planning, risk classification and approvals live in Model Plane (`execution-core`) and Application/Conversation. Quarry must not gain approval-decision authority (reconciled doc §48 P0.3).
- **BrowserBroker owns domain authority** in production. Client/model-supplied `allowed_domains` are replaced by grant domains (`agent_routes.rs:1396-1413`). Free roaming across sites needs broker-side grant UX.
- **No cross-plane DB access:**
  - Quarry persists knowledge only via the Data Plane ingest contract.
  - Embeddings and rerank are Data Plane-owned (`vector_index.rs`, `rerank.rs` call DP).
  - The local Tantivy corpus is an INF-noted tension with GOAL non-negotiable #6 ("no retrieval plane"), tolerated as a cache.
- **ZDR must propagate:**
  - ZDR forbids artifacts, durable events, cache, local index, ingest, receipts, checkpoints, persistent profiles and resume.
  - It refuses durable crawl/batch outright (`routes.rs:563-588`).
  - Any history, bookmarks, recording or replay feature must be ZDR-aware. Durable crawls cannot serve ZDR orgs until the orchestrator carries posture.
  - Paid SERPs (Brave/Serper) are never used under ZDR (`smart_router.rs:39-60`).
- **Space access never widens owner-plane ACL.** Browser runs are scoped to org **and actor** (`agent_routes.rs:474-486`), so sharing a live session with teammates (co-browsing by others) needs an owner-plane ACL design and cannot be a gateway-side share.
- **Edge is the only public entry.** Control is HMAC-internal and has no JWT/org awareness. New tenant features (webhooks, presets) must be fronted by edge with org scoping. There is a known exception: the onboarding crawl posts to control directly (DOC `ARCHITECTURE.md`).
- **Cross-plane token manifest:** each Quarry → Model/Data call needs a granted manifest row (`contracts/cross-plane-token-requests.json`). Model rows are false today.
- **Remote browser providers are blocked** for agent runs until they prove request-level containment (`require_isolated_agent_egress`). Managed-browser features (Browserbase live view, recording, captcha) cannot be used for governed agents.
- **"No OpenCV for anti-bot, no VLM in Quarry"** (`docs/BROWSER_WORKSPACE_PLAN.md` hard constraints). Captcha solving is out of scope by policy (`OSS_PARITY_BACKLOG.md` ⛔).
- **Gateway rule:** the browser never sees CDP endpoints, Quarry tokens or raw provider secrets (`browser.rs:1-6`, CLAUDE.md). Browser extensions or a native shell would have to respect this.

## Doc-vs-code discrepancies

| # | Doc claim | Code reality |
|---|---|---|
| 1 | `Features.md` #5: deep research is driven by a quarry-orchestrator Temporal workflow | No research workflow (4 workflows only, `orchestrator main.go:94-117`). `deep_research.rs` is an unrouted executor. |
| 2 | `Features.md` #8, `GAP.md` §5: scheduled/recurring crawls work | Only `change_monitor` maps to a workflow (`schedule_wire.go:39-51`). Trigger and backfill return 501 (nil Temporal client, `control main.go:192`). |
| 3 | `GAP.md` §5.1/5.2: include/exclude, sitemap-guided crawl, robots, allow_external/subdomains, `crawl_entire_domain`, `prompt` on `/v1/crawl` are "Closed" in the Go orchestrator | None exist in Go (grep shows no sitemap/robots/allow_external). `git log -S allow_external_links` finds nothing. `CrawlRequest` has no `prompt` (`routes.rs:533-555`). |
| 4 | `PARITY_IMPLEMENTATION_PLAN.md` 2B: crawl globs SHIPPED, "edge/Go plumbing is field pass-through" | Globs live in the unused Rust `CrawlFrontier`. Neither edge nor Go passes patterns. |
| 5 | `Features.md` #6 and `GAP.md` §3: stealth in `quarry-runtime/stealth.rs`; public scrape action schema with `screenshotAfterEach`/`on_error` | Stealth is at `quarry-browser/src/stealth.rs` and is **never injected**. `ScrapeRequest` has no actions. `ActionRuntime` is unrouted. |
| 6 | `GAP.md` §4, `OSS_PARITY_BACKLOG.md`: scrape gives markdown/json/screenshot/pdf/images/attributes/summary/query | The scrape output only carries html/markdown artifacts plus links/metadata/branding (`pipeline.rs:690-704`). |
| 7 | `GAP.md` §6: `GET/DELETE /v1/crawl/:id`, `/errors`, `/v1/deep-research` 501, `/v1/presets` 501, `/v1/security/blocklist`, `POST /v1/agent` sync wrapper | None of these routes exist on edge (`routes.rs:130-293`). |
| 8 | `GAP.md` §8: metrics endpoint `/metrics` | Edge serves no `/metrics`; the exporter is inert (`telemetry.rs:6-14`). |
| 9 | `Features.md` #17 and `ARCHITECTURE.md`: GraphQL in quarry-control (Go + async-graphql) / "overlay in control" | GraphQL is in edge Rust (`quarry-edge/src/graphql`), and `jobs` always returns empty (`schema.rs:71-95`). |
| 10 | `Features.md` #3: Brave/**Bing**/**Tavily** backends | Code has Tantivy, Stract, SearXNG, Brave, Serper, plus DP vector (`main.rs:880-960`). No Bing, no Tavily. |
| 11 | `Features.md` #11, `PROFILES.md`: profiles in S3 plus Postgres in quarry-control; S3 tier L2′ | Edge wires memory/Postgres plus Redis only (`main.rs:551-600`). `S3ProfileStore` is unwired. Snapshots capture no IndexedDB. |
| 12 | `DRIVER_MATRIX.md`: Kernel/Browserless fallback chain, `driverHints.preferredDriver`, `DriverInfo.attempts[]`, `requires_js`/`cost_ceiling` signals | Kernel/Browserless are unregistered. No such request fields. `DriverInfo` has no `attempts` (`output.rs:220-236`). |
| 13 | `DRIVER_MATRIX.md`, `WEB_AGENT_ROADMAP_EXECUTION.md`: `max_cost_usd` enforced via AgentLoop | AgentLoop is unused by the routes; the step path enforces `max_steps` only. Improvements doc (2026-08-14) says it is "rejected". Code removed the reject but never enforced the budget. |
| 14 | `CHANGE_TRACKING.md`: `/v1/change/check` performs a fresh fetch | It requires a caller-supplied `fresh_fingerprint` (`change_routes.rs:91-104`). |
| 15 | `PROGRESS.md` §6 / `GAP.md` §13.5: "paragraph semantic diff `SemanticDiff::is_real_delta`" drives change | `diff_paragraphs` has no caller. Change is a byte-hash compare. |
| 16 | `PRESETS.md`: SWR honoured by the cache layer; preset catalog ↔ `/v1/presets` | No `stale_while_revalidate` use. Rust presets are unused. `/v1/presets` exists on control only, with a different (Go) catalog. |
| 17 | `PROGRESS.md` (root and docs): Firecrawl-compat response adapter shipped | `firecrawl_adapter.rs` was deleted in commit `99475e13`. |
| 18 | `SCRAPING_ENGINE_GAPS.md`: Browserbase driver missing; adblock task; `driver_plan_enhanced` | Stale. Browserbase exists and is registered; no adblock code; no `driver_plan_enhanced.rs`. |
| 19 | `TAVILY_PARITY.md`: Tavily `/extract` → `/v1/crawl`; include/exclude domains not wired | `/v1/extract` exists; `include_domains`/`exclude_domains` are wired (`search_routes.rs:108-116`). Doc is stale in the other direction. |
| 20 | `docs/FIRECRAWL_PARITY_ASSESSMENT.md` (repo root): Colly/Rod, 75-80% parity | Describes legacy Quarry v1 (Go). Not applicable to Quarry-v2. |
| 21 | `Features.md` #15: per-tenant budgets in `quarry-runtime/policy` | `policy.rs` is a determinism/fingerprint bag. Tenant cost budgets are not enforced in Quarry. |
| 22 | `CODEBASE_INFORMATION_SYSTEM.md:134`: per-connection ZDR waiver "designed, not built" (as of 2026-10-06) | integration-corev2 now has a dark-by-default acknowledgement/assessment API (`zdr_acknowledgement.go`, commit `391d8926`, 2026-10-08). Doc lags code. |
| 23 | `docs/actions-surface-operations.md`: line refs `server.go:402/390` | Routes are now at `server.go:539/551`. The contract text likely still holds, but the line refs are stale. |
| 24 | `docs/BROWSER_WORKSPACE_PLAN.md`: "Program status COMPLETE" | Its own open items remain: file transfer is gated false, and the vision sidecar is not in the live compose stack (it is in `Q/deploy/compose`). INF: "complete" means phase-complete, not feature-complete. |
| 25 | `Features.md` layer table: `lab/` is Python | `lab/evals` is a Rust crate (`Q/lab/evals/Cargo.toml`). Python exists only in the vision sidecar and SDK tests. |
