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
       │   │  → emit page_fetched + branding_extracted per page      │
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
       │   │    branding}                                            │
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
