# Innhenting — UX gap analysis and feature list (2026-10-28)

The Innhenting page (`src/features/ingestions`) is the operator surface for
Quarry v2. This note compares it with three reference products, ranked by the
owner from best UI/UX to least, and maps every feature worth copying to a Quarry
endpoint that is reachable through the same-origin gateway today, or marks it
**needs backend**.

References (read-only study, no sign-ups):

1. **Firecrawl Playground** (firecrawl.dev/playground)
2. **Apify Console** (actors, runs, schedules, datasets, monitoring; docs.apify.com)
3. **Browse AI** (robots, monitors, change detection; browse.ai)

## What the references do well

**Firecrawl.** One input card is the whole page. Endpoints sit in a grouped
segmented control: *Discover* (Search), *Extract* (Scrape, Parse), *Crawl* (Map,
Crawl). Under the URL field, one toolbar holds an options button, a format
picker and a "Get code" button next to the primary action. Options open as a
panel: main content only, parse PDF, include/exclude tags, wait, timeout, max
age. Formats open as a visual gallery (Markdown, Summary, Links, HTML,
Screenshot, JSON, Images …). "Get code" opens a drawer with SDK/cURL tabs and a
copy button. Results render in place as Markdown/JSON/screenshot tabs.

**Apify.** Every execution is a *run* with a clear lifecycle (READY → RUNNING →
SUCCEEDED / FAILED / TIMED-OUT / ABORTED), an origin (web, API, scheduler …),
duration and cost. The run list filters by status. A run detail page has tabs
for Output (dataset table, JSON, export), Log, Input and Storage, plus Abort and
Resurrect actions. Schedules are cron jobs that point at actors.

**Browse AI.** Point at a URL; the AI proposes columns; you approve. Monitors
re-run a robot hourly/daily/weekly/monthly and alert on change, with a change
history per monitor. Export to sheets/webhooks/integrations.

## Gap analysis against the page before this change

| Area | Before | Gap |
|---|---|---|
| Layout | Tabs + a 4-tile KPI card + a small form + a table, inline styles | No playground: the form is a narrow side column, results jump to another tab |
| Endpoints | Run-type select (scrape, crawl, extract, batch) | No search, no map, no endpoint grouping |
| Options | None for scrape/crawl; extract sends `schema`/`extractMode` that the gateway drops | Options silently ignored; no formats |
| Results | Raw `JSON.stringify` of the evidence on the Bevis tab | No Markdown/JSON/links/metadata preview, no copy |
| Code | None | No "copy as code" |
| Runs | Table: kind, target, status, created, progress | No status filter, no duration, no run detail with log/output, no re-run, no export |
| Monitoring | On-demand check + fingerprint history | No recurring monitors in the UI (the gateway has them), no changed/unchanged timeline |

## Feature list → Quarry endpoint

Status legend: **Wired** = reachable through an existing gateway route and used by
the redesigned page; **Stub** = shown in the UI as disabled with a "krever
backend" note; the right column says what is missing.

### Playground (Firecrawl)

| Feature | Quarry endpoint | Gateway route / SPA client | Status |
|---|---|---|---|
| Scrape a URL | `POST /v1/scrape` | `/api/v1/knowledge/scrape` · `scrapeUrl` | Wired |
| Formats: markdown, html, raw html, links | `/v1/scrape` `formats` | same (body passed through) | Wired |
| Options: main content only, include/exclude tags | `/v1/scrape` `only_main_content`, `include_tags`, `exclude_tags` | same | Wired |
| Screenshot capture | `/v1/scrape` `formats: screenshot` | same (`capture`) | Wired, **flag-gated** (`QUARRY_EDGE__SCRAPE_CAPTURE`, off on live: the edge answers 400 `feature_disabled`, and the playground names the flag and asks to drop the format) |
| Wait / timeout | `/v1/scrape` `render.waitForTimeoutMs`, `timeout_ms` | same (body passed through) | Wired (`timeout_ms` added to `ScrapeRequest`) |
| Browser actions (click/type/scroll) | `/v1/scrape` `actions` | — | **Stub**: `QUARRY_EDGE__SCRAPE_ACTIONS` off; action contract says actions are not exposed in v1 |
| Crawl a site | `POST /v1/crawl` | `/api/v1/knowledge/crawl` · `startCrawl` | Wired (`maxPages`, `maxDepth`, include/exclude paths, subdomains) |
| Map (discover URLs) | `POST /v1/map` | `/api/v1/knowledge/crawl/discover` · `discoverCrawlPages` | Wired |
| Search the web | `POST /v1/search` | `/api/v1/search/web` · `runWebSearch` | Wired (answer synthesis is off on live; not requested) |
| Extract with a prompt | `POST /v1/extract` | `/api/ingestions/runs` kind=extract · `createIngestionRun` | Wired (`prompt` only) |
| Extract with a JSON schema | `/v1/extract` `schema` | gateway drops `schema` (`runs.rs` `create_extract`) | **Stub**: forward `schema` in the gateway |
| Batch scrape | `POST /v1/batch` | `/api/ingestions/runs` kind=batch | Wired |
| Agent (autonomous browser task) | `/v1/agent/runs` | `/api/v1/browser/*` (Nettleser page) | Linked to Nettleser, not run from here |
| Result preview: Markdown / JSON / links / metadata | scrape response | — | Wired (client rendering) |
| Copy as code (cURL / JavaScript / Python) | the Quarry `/v1/*` request the page sends | — | Wired (client-only, pure function) |
| Send result to Kunnskap | `POST /v1/scrape` `ingest: true` | `/api/v1/knowledge/scrape` | Wired as a separate, explicit button |

### Runs (Apify)

| Feature | Quarry endpoint | Gateway route | Status |
|---|---|---|---|
| Run list across kinds | `GET /v1/{kind}/jobs` | `/api/ingestions/runs` | Wired |
| Status filter, kind filter, text filter | — | client | Wired |
| Duration | `started_at` / `completed_at` | `/api/ingestions/runs` | Wired (derived) |
| Run log (event timeline) | `GET /v1/runs/{id}/events` | `/api/ingestions/evidence` | Wired |
| Run output / stats | job `stats` | `/api/ingestions/runs` | Wired (JSON view + download) |
| Re-run with the same input | create endpoints | as above | Wired (prefills the playground) |
| Cost / credits per run | `/v1/team/credit-usage` | not exposed; jobs carry no cost fields | **Stub** |
| Origin (manual vs schedule) | job `schedule_id` | gateway does not forward `schedule_id` | **Stub** |
| Per-URL errors | `GET /v1/{crawl,batch,jobs}/{id}/errors` | not exposed | **Stub** |
| Abort / pause / resume | `DELETE /v1/crawl/{id}`, `…/pause`, `…/resume` | not exposed; `QUARRY_EDGE__JOB_CONTROL` off | **Stub** |
| Per-page dataset table | none (no per-page results endpoint) | — | **Stub** |

### Schedules (Apify)

| Feature | Quarry endpoint | Gateway route | Status |
|---|---|---|---|
| Create / list schedules | `/v1/schedules` | `/api/ingestions/schedules` | Wired (kinds from `/schedules/capabilities`) |
| Pause / resume / delete | `/v1/schedules/{id}/…` | `/api/ingestions/actions` | Wired |
| Trigger now / backfill | `/v1/schedules/{id}/trigger`, `/backfill` | same, 501 unless enabled | Wired behind the capability flag |
| Cron presets + plain-language description | — | client | Wired |

### Monitoring (Browse AI)

| Feature | Quarry endpoint | Gateway route | Status |
|---|---|---|---|
| Check a page now | `POST /v1/change/check` | `/api/v1/monitoring/check` · `checkUrlNow` | Wired |
| Baseline history with changed/unchanged markers | `GET /v1/change/history` | `/api/v1/monitoring/history` | Wired (marker derived from consecutive fingerprints) |
| Recurring monitors (hourly / daily / weekly) | `/v1/schedules` `target_kind=change_monitor` | `/api/v1/monitoring/schedules` (GET/POST/DELETE) | Wired in this change (new client functions + action contracts) |
| Text / visual diff between versions | none: baselines are fingerprints only | — | **Stub** |
| Alerts (email / webhook on change) | none | — | **Stub** |
| AI-proposed columns ("robots") | `/v1/extract` with `schema` | schema not forwarded | **Stub** |

## What the redesign ships

- **Lekeplass** (new default tab): grouped endpoint picker (Oppdag: Søk, Kart ·
  Hent ut: Scrape, Uttrekk · Crawl: Crawl, Batch · Agent as a stub), one input
  card with an options panel, a format gallery and a "Hent kode" panel
  (cURL/JavaScript/Python, built from the same request the page sends). The
  result panel shows Markdown/JSON/links/metadata/HTML with copy and download,
  map hits with "Scrape" and "Batch valgte", search hits with "Scrape", and a
  receipt for durable crawl/batch jobs that links to the run.
- **Oversikt** tiles (active, succeeded with success rate, failed, scheduled)
  that open the run history pre-filtered.
- **Kjøringer**: status chips with counts, kind and text filters, duration and
  progress columns, and a run detail with Oversikt / Logg (Quarry events) /
  Utdata, plus "Kjør på nytt" and "Eksporter JSON". Abort, origin, cost and
  per-page results are visible stubs.
- **Tidsplaner**: cron presets and a plain-language reading (UTC); capability
  gating unchanged.
- **Overvåking**: recurring monitors (hourly/daily/weekly) with create/stop,
  the on-demand check, and a changed/unchanged/first-baseline timeline. New
  action contracts `monitoring.create_monitor` / `monitoring.delete_monitor`
  with gateway dispatchers that reuse the REST handlers.
- The old **Bevis** tab is folded into the run detail (log) and the playground
  result (manual scrape/extract output).

## Backend follow-ups, in value order

1. Forward `schema` on `/api/ingestions/runs` kind=extract (one line in `runs.rs`).
2. Expose `GET /v1/{kind}/{id}/errors` and job status for the run detail page.
3. Expose job control (cancel/pause/resume) once `QUARRY_EDGE__JOB_CONTROL` is on.
4. Forward `schedule_id` in `to_run_item` so runs show their origin.
5. A content diff artifact for change monitors (today: fingerprint transition only).
6. Usage/credit read (`/v1/team/credit-usage`) for per-run cost.
