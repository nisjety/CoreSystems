# Onboarding Live Crawl

How verevon's "drop your URL" onboarding step gets real `page_fetched`
and `branding_extracted` events back from Quarry, in under two seconds,
without authentication on the verevon → control hop.

This document covers the end-to-end chain, the env vars per service,
the failure modes, and the dev/prod posture for the auth bypass.

## End-to-end chain

```
┌────────────────┐    POST /api/onboarding/    ┌─────────────────────┐
│ verevon browser ├───►   crawl-preview         │ verevon Next.js      │
│ (wizard step 3)│    {url: "skinsecret.no"}   │ route.ts            │
└────────────────┘                             │                     │
       ▲                                       │  1. normalize URL   │
       │ SSE: snippet / branding /             │  2. SSRF guard      │
       │     warning / done                    │  3. POST to control │
       │                                       └──────────┬──────────┘
       │                                                  │
       │  GET /v1/jobs/{id}/events?after_seq=N           │
       │      (poll, 800ms cadence)                       │
       │                                                  │
       │   ┌──────────────────────────────────────────────▼─────────┐
       │   │ quarry-control:8081 (Go)                                │
       │   │  POST /v1/jobs/                                         │
       │   │  → checks Idempotency-Key header                        │
       │   │  → inserts row in postgres with status=accepted         │
       │   └──────────────────────────────┬──────────────────────────┘
       │                                  │
       │       (orchestrator polls /v1/jobs every 2s)
       │                                  │
       │   ┌──────────────────────────────▼──────────────────────────┐
       │   │ quarry-orchestrator (Go)                                │
       │   │  internal/jobs/dispatcher.go                            │
       │   │  → picks up status=accepted, kind in {scrape,crawl,…}   │
       │   │  → generates run_id                                     │
       │   │  → ExecuteWorkflow("CrawlJobWF", input) on Temporal     │
       │   │  → PUT /v1/jobs/{id} {status=running, run_id}           │
       │   └──────────────────────────────┬──────────────────────────┘
       │                                  │
       │             Temporal task queue: "quarry-orchestrator"
       │                                  │
       │   ┌──────────────────────────────▼──────────────────────────┐
       │   │ CrawlJobWF (Temporal workflow)                          │
       │   │  → emit run_started                                     │
       │   │  → loop frontier: runPage activity per URL              │
       │   │  → emit page_fetched + branding_extracted               │
       │   │    + page_extracted per page                            │
       │   │  → emit run_completed                                   │
       │   └──────────────────────────────┬──────────────────────────┘
       │                                  │
       │            runPage activity → /v1/internal/run_page
       │                                  │
       │   ┌──────────────────────────────▼──────────────────────────┐
       │   │ quarry-edge:8082 (Rust)                                 │
       │   │  → auth: dev_bypass OR JWKS verify                      │
       │   │  → PageRunner.run(): fetch + transform + branding       │
       │   │  → branding_rendered::extract: palette, favicon,        │
       │   │    logo_candidate, og_image, theme_color, font_family   │
       │   │  → returns RunPageResult{links, content_type, title,    │
       │   │    branding, display_title, title_source, excerpt,      │
       │   │    summary, word_count, lang, driver}                   │
       │   └──────────────────────────────┬──────────────────────────┘
       │                                  │
       │                workflow re-emits events via
       │   ┌──────────────────────────────▼──────────────────────────┐
       │   │ EmitEvent activity                                      │
       │   │  POST /v1/runs/{run_id}/events                          │
       │   │  [Event...]  ← always an array                          │
       │   │  Authorization: Bearer ${CONTROL_AUTH_TOKEN}            │
       │   └──────────────────────────────┬──────────────────────────┘
       │                                  │
       │   ┌──────────────────────────────▼──────────────────────────┐
       │   │ quarry-control event log                                │
       │   │  → assigns server-side seq (NextSeq) per run            │
       │   │  → indexes by run_id AND job_id (both set on event)     │
       │   └──────────────────────────────┬──────────────────────────┘
       │                                  │
       └──────────────────────────────────┘
         verevon's poll sees the events arrive and forwards them
         as SSE snippets + branding to the wizard.
```

## `page_extracted` — real titles and text for the wizard

`page_fetched` is emitted by `PageRunner` **before** the transform step and
carries only `{url, status, duration_ms, content_type}` (plus `title` when the
orchestrator re-emits it from `RunPageResult`). `artifact_written` carries
artifact ids and byte counts. Neither can carry page text, which is why the
wizard used to render "N utdrag samlet" over cards with an empty excerpt and
the host name as title.

`page_extracted` (`quarry_core::event::EventType::PageExtracted`,
`quarrycontracts.EvtPageExtracted`, NATS token `page_extracted`) is emitted
once per successfully transformed HTML page, **after** readability, markdown
and metadata have run. It is additive: consumers that only know
`page_fetched` keep working.

```json
{
  "url": "https://aquatiq.com/",
  "title": "Aquatiq – hygiene for matindustrien",
  "title_source": "model",
  "excerpt": "Vi leverer hygieneløsninger, kjemikalier og kompetanse til …",
  "summary": "Leverandør av hygieneløsninger til matindustrien.",
  "word_count": 412,
  "lang": "no",
  "driver": "browser",
  "content_type": "text/html; charset=utf-8",
  "fingerprint": "blake3:…"
}
```

| Field | Meaning |
|-------|---------|
| `title` | Display title after provenance resolution; never empty. |
| `title_source` | `html` — cleaned `<title>`/`og:title`, judged specific. `model` — HTML title was missing/generic (empty, host name, "Home", "Untitled", "Forside", …) and Model Plane proposed a ≤ 60-char title. `host` — no usable title and no model result; host label. |
| `excerpt` | First ~300 chars of the readable markdown, markup stripped, whitespace collapsed, cut on a word boundary. Empty when the page had no readable text. |
| `summary` | One-sentence model summary. Only with `title_source: model`. |
| `word_count` | Whitespace-token count of the plain text. |
| `lang` | Detected/declared ISO-639-1 language. |
| `driver` | Driver that actually served the fetch: `static`, `tls` or `browser` (from `FetchResponse.served_by`, so a static-primary run rescued by the browser says `browser`). |

Paths to the wizard:

* **Seed scrape** — `/v1/scrape/stream` streams the runtime's live event sink,
  so the frame arrives as a full quarry-core `Event` envelope
  (`{event_id, run_id, type: "page_extracted", ts, seq, payload, idempotency_key}`).
* **Crawl job** — the orchestrator's `runPage` re-emits it from
  `RunPageResult` (`display_title`, `title_source`, `excerpt`, `summary`,
  `word_count`, `lang`, `driver`) into control's per-job event log, right
  after `page_fetched`/`branding_extracted`, so `/v1/jobs/{id}/events` polls
  see it. No event is emitted when the runtime supplied no extraction
  (older edge, non-HTML response).
* **Gateway** — `normalize.rs` maps both `page_fetched` and `page_extracted`
  to a `snippet` (`title`, `titleSource`, `excerpt`, `summary`, `wordCount`,
  `lang`, `driver`). A per-stream `SnippetLedger` keyed by URL forwards the
  first snippet per page, forwards a richer one as an update (same id, so the
  wizard replaces the card) and drops poorer duplicates; `pages` in the `done`
  packet counts unique pages, seed included. The preview follows the job's
  event log for at most `CRAWL_POLL_BUDGET_MS` (45s), a ceiling rather than a
  dwell time: the stream ends as soon as a terminal event arrives. It was
  18s, which could expire before the orchestrator had dispatched the workflow
  and emitted its first page, leaving the wizard with only the seed page.

### Model Plane title hop

When `title_source` would be `host`, `PageRunner` asks Model Plane
(`POST /v1/invoke`, the same client the AI formats use) for a clean title and
a one-sentence summary from a ≤ 1 200-char slice of the excerpt.

* **Model** — `QUARRY_EDGE__PAGE_TITLE_MODEL`, default `verevon-budget`. This
  is a *routing alias* resolved by inference-core's intent layer (its
  `RoutingPolicy.table.budget` ladder / `cheap_fallback`), i.e. the cheapest
  capable model the Model Plane currently routes. Never set a vendor model
  id here; use `verevon-budget`, `verevon-balance` or `verevon-genius`.
* **Enable/disable** — `QUARRY_EDGE__PAGE_TITLE_ENRICH` (default `true`
  whenever `QUARRY_EDGE__MODEL_PLANE_URL` is set). Without a Model Plane URL
  the hop is simply absent and `title_source` stays `html`/`host`.
* **Guards** — time-boxed at 2 s, at most 3 concurrent calls, a rolling cap
  of 600 calls/hour per edge process, results cached by content
  fingerprint (re-crawls of an unchanged page never pay twice), reply must
  be JSON with a non-generic ≤ 60-char title or it is discarded. Every
  failure degrades to `title_source: host`; the page never fails.
* **ZDR** — skipped when the request is `zdr: true` or the privacy
  classification is `zdr_ephemeral` / `credential_or_secret`, and nothing is
  cached for such runs. `page_extracted` itself goes through
  `EventSink::emit_for_zdr`, so under ZDR it reaches only already-connected
  live subscribers and never the durable publisher or NATS. Quarry only
  *proposes*/validates; Model Plane never sees the HTML, only the excerpt.

### JS-shell pages (aquatiq.com)

A page can trip every shell marker (`__NEXT_DATA__`, `_next/static`, dozens
of `<script>` blocks, >20 KB) and still be fully server-rendered.
aquatiq.com is exactly that: ~220 KB of HTML carrying ~4.8k characters of
real text. The heuristic used to escalate it anyway, costing a browser
session plus a hydration settle per page and returning no more text than the
static body already had. `is_js_shell_needing_browser` now also requires the
body to expose fewer than `SHELL_MAX_TEXT_CHARS` (2 000) visible characters,
so a server-rendered page is fetched once and a 4-page preview completes in
~10s instead of outliving the gateway's event-poll ceiling.

For pages that ARE content-less shells, three fixes make the escalation
actually produce text:

* `BrowserDriverAdapter` now waits for hydration when no `wait_for_selector`
  was given and the first snapshot still looks like a shell (scripts present,
  < 600 visible chars): it re-snapshots every 300 ms until the text stops
  growing or `QUARRY_BROWSER_SETTLE_MS` (default 3 500) elapses.
* `FallbackDriver` keeps the best shell response it saw; if the browser
  fails, or renders no more visible text than the shell, it returns the
  shell (title, links and metadata are still extractable) instead of an
  error or an emptier body.
* A non-browser fallback that returns an unfollowed 3xx (the TLS-profile
  driver never follows redirects — each hop needs its own SSRF/DNS
  preflight, which only the static driver performs) or less visible text
  than the shell no longer wins: the chain keeps rotating towards the
  browser. This was the actual source of the empty excerpts: static
  followed `aquatiq.com → www.aquatiq.com` into the shell, TLS answered
  with a 44-char "Redirecting (308)" body, and that body was extracted. Both decisions are logged at `info`/`warn`
  (`browser hydration settle`, `browser fallback rendered the JS shell`,
  `… keeping the static response`).

## Services + ports

| Service             | Port  | Language | Image                               |
|---------------------|-------|----------|-------------------------------------|
| verevon              | 3000  | TS       | `frontend-plane-verevon-frontend`    |
| quarry-control      | 8081  | Go       | `ingestion-plane-quarry-control`    |
| quarry-edge         | 8082  | Rust     | `ingestion-plane-quarry-edge`       |
| quarry-orchestrator | —     | Go       | `ingestion-plane-quarry-orchestrator` |
| Temporal            | 7233  | upstream | `temporalio/auto-setup`             |
| Postgres (ingestion)| 5432  | upstream | `postgres:16`                       |
| Redis (ingestion)   | —     | upstream | `redis:7` (internal only)           |

verevon talks to quarry-control via the compose network using
`http://quarry-control:8081`. Nothing on the host side calls control
directly except the smoke probe.

## Env vars

### verevon

```
QUARRY_API_URL=http://quarry-control:8081
```

That's it. verevon's route generates its own idempotency keys and uses
DNS to validate seed URLs before forwarding them.

### quarry-control

```
QUARRY_CONTROL_ADDR=:8081
QUARRY_CONTROL_DSN=postgres://…/quarry_v2
QUARRY_CONTROL_API_KEY=dev-quarry-control-key   # must match orchestrator's CONTROL_AUTH_TOKEN
ENVIRONMENT=dev|production
```

### quarry-edge

```
QUARRY_EDGE__PORT=8082
QUARRY_EDGE__CONTROL_BASE_URL=http://quarry-control:8081
QUARRY_EDGE_AUTH_DEV_BYPASS=1   # accepts ANY bearer — dev only
ENVIRONMENT=dev|production       # production + dev_bypass=1 = refuse to start
```

### quarry-orchestrator

```
TEMPORAL_ADDR=temporal:7233
RUNTIME_BASE_URL=http://quarry-edge:8082
CONTROL_BASE_URL=http://quarry-control:8081
CONTROL_AUTH_TOKEN=dev-quarry-control-key   # must match control's API_KEY
RUNTIME_AUTH_TOKEN=dev-quarry-runtime-token  # any non-empty in dev (edge bypasses)
```

## The dev-bypass posture

`QUARRY_EDGE_AUTH_DEV_BYPASS=1` makes quarry-edge accept any non-empty
`Authorization: Bearer` header without verifying against auth-core's
JWKS. This lets the orchestrator call `/v1/internal/run_page` with a
static shared dev token instead of a real tenant JWT.

**Production safety net.** The edge binary refuses to start if both
`QUARRY_EDGE_AUTH_DEV_BYPASS=1` and `ENVIRONMENT={prod,production}` are
set. See `crates/quarry-edge/src/main.rs`. To deploy to production:
either unset the bypass (operators must wire auth-core JWKS for edge),
or set ENVIRONMENT to a non-prod value (wrong — fix the bypass).

## Hardening landed in this iteration

1. **SSRF guard** (verevon route) — refuses to forward private,
   loopback, link-local, and non-http(s) hosts to Quarry. Resolves
   DNS first and validates resolved addresses too.
2. **Branding URL allow-list** (`WebsiteStep.normalizeBranding`) —
   every URL field in the branding payload must be absolute
   `http`/`https`. `data:`, `javascript:`, relative paths, and
   private-IP literals are all dropped before they reach the BrandStrip
   `<img>` tags.
3. **Dev-bypass kill switch** — edge refuses to start in production
   with the bypass enabled (see above).
4. **Dispatcher hygiene** (`internal/jobs/dispatcher.go`) — bounded
   in-memory dedupe set (1 h TTL by default), per-job attempt counter,
   and `markFailed` PUT to control after `MaxAttempts` (5) consecutive
   dispatch errors so failing jobs stop spamming.
5. **Idempotent job creation** — verevon sends an
   `Idempotency-Key: verevon-crawl-<sha256>` header derived from
   `(seedUrl, cap, day-bucket)`. Control returns 200 with the existing
   record on collision instead of 201 + duplicate row.
6. **Structured warning events** — the route emits
   `event: warning {code, message}` with codes
   `invalid_url`, `bad_scheme`, `no_hostname`, `dns_failed`,
   `private_address`, `control_unreachable`, `no_events`. The wizard
   surfaces a Norwegian message per code.

## Smoke probe

From inside the compose network:

```bash
docker run --rm --network=inter-plane-bus alpine sh -c "
  apk add --no-cache curl >/dev/null 2>&1 &&
  curl -sS -X POST \
    -H 'Content-Type: application/json' \
    -H 'Accept: text/event-stream' \
    -d '{\"url\":\"skinsecret.no\",\"maxPages\":3}' \
    --max-time 12 \
    http://frontend-plane-verevon-frontend-1:3000/api/onboarding/crawl-preview"
```

Expected: `started → snippet → branding → … → done {source: "live"}`
inside ~2 s. If you see `warning` events with code = `control_unreachable`
or `no_events`, follow the failure-mode table below.

## Failure modes

| Symptom (in SSE stream) | Most likely cause | Fix |
|---|---|---|
| `warning code: control_unreachable` | quarry-control container down or verevon can't DNS-resolve it | `docker ps | grep quarry-control`; check verevon is on `inter-plane-bus` |
| `warning code: no_events` (job created but no events flow) | orchestrator not running, or Temporal worker disconnected, or workflow registration broken | `docker logs quarry-orchestrator` — look for `jobs dispatcher started` |
| `warning code: private_address` | user typed an internal host | expected; SSRF guard working |
| `warning code: dns_failed` | typo'd domain | expected; surfaces in wizard |
| `client_4xx (400)` in orchestrator logs | EmitEvent shape — control expects `[Event]` not `Event` | fixed in `activities/activities.go`; if returns, check serialisation |
| `client_4xx (500)` in orchestrator logs | seq collision (multiple events with seq=0) | fixed in `MountEvents` via `NextSeq`; verify control rebuild contains it |
| Orchestrator logs `"expected 2 args for function: CrawlJobWF but found 1"` | workflow closure wrappers not registered | check `cmd/orchestrator/main.go` uses `RegisterWorkflowWithOptions` with `Name:` |
| Edge logs `"REFUSING TO START"` | dev_bypass=1 + ENVIRONMENT=prod | unset one |

## Where the code lives

- verevon route: `apps/Frontend Plane/verevon/src/app/api/onboarding/crawl-preview/route.ts`
- verevon consumer: `apps/Frontend Plane/verevon/src/components/auth/onboarding/steps/WebsiteStep.tsx`
- verevon brand strip: `apps/Frontend Plane/verevon/src/components/auth/onboarding/OnboardingFrame.tsx`
- control jobs: `services/quarry-control/internal/resources/resources.go`
- control store: `services/quarry-control/internal/store/store.go` (mem) + `…/pg/resources.go` (pg)
- jobs dispatcher: `services/quarry-orchestrator/internal/jobs/dispatcher.go`
- workflow wrappers: `services/quarry-orchestrator/cmd/orchestrator/main.go`
- branding extraction: `crates/quarry-transform/src/branding_rendered.rs`
- pipeline emits: `crates/quarry-runtime/src/pipeline.rs`
- edge HTTP: `crates/quarry-edge/src/routes.rs::internal_run_page`
- edge auth bypass: `crates/quarry-edge/src/auth.rs` + `main.rs` kill switch
