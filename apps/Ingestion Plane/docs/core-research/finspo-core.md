# finspo-core Research Dive

Generated: 2026-07-11 (supersedes the 2026-06-07 pass)

Scope: `apps/Ingestion Plane/finspo-core`

Evidence grades used below: **[live-curl]** = observed against the running
`finspo-api` on `127.0.0.1:3130`; **[source-only]** = read from disk, not
exercised live; **[logs]** = container logs. Docker `exec`/`build`/`logs` are
unavailable this session (containerd content store is corrupted — every
container shows `(unhealthy)` because its `wget`/exec healthcheck cannot run,
even though the already-running process serves traffic fine).

## Snapshot

`finspo-core` is the SharePoint / OneDrive **Microsoft Graph delta-sync
connector** for the Ingestion Plane (ported from the Aquatiq Duplicate
Management tool). It owns source registration, incremental delta sync,
per-file permission/ACL capture, governance analytics, review proposals,
audit trail, recommendations, and config-gated destructive cleanup execution.

Runtime footprint:

- `finspo-api` (Go, Fiber v2 + pgx v5 + zerolog), host-published `:3130`. **[live-curl]**
- `integration-finspo-worker` — lives in `integration-corev2`, port-less, `depends_on: finspo-api (healthy)`; it is the sync/webhook worker side, not a second finspo binary. **[source-only]**
- Backing store: `finspo` database on `ingestion-postgres` (`Up 2 days (healthy)`). **[live-curl/docker ps]**

Verdict: this remains one of the most **production-shaped** services in the
Ingestion Plane. The delta sync, permission capture, content extraction, and
Data Plane forwarding are all **real and test-covered** — not scaffolding.

## Build / vet / test (host) — [source-only]

- `go version` = go1.26.2 darwin/arm64
- `go build ./...` → exit 0
- `go vet ./...` → exit 0
- `go test ./...` → **all 12 packages `ok`, 0 failures** (`api`, `auth`,
  `config`, `content`, `dataplane`, `db`, `events`, `extract`, `sharepoint`,
  `store`, `sync`, `telemetry`; the two `cmd/*` have no tests).

## Live behavior — [live-curl] 2026-07-11

| Probe | Result |
|---|---|
| `GET /health` | **200** `{"service":"finspo-core","status":"ok"}` |
| `GET /ready` | **503** `{"checks":{"db":"down: context deadline exceeded","nats":"ok"},"status":"degraded"}` |
| `GET /api/v1/sharepoint/sites` (no key) | **401** `authentication required` |
| `GET /api/v1/sources` (no key) | **401** `authentication required` |
| `GET /api/v1/sources` (wrong key + org) | **401** `invalid API key` |
| `GET /api/v1/analytics/overview` (wrong key) | **401** `invalid API key` |
| `GET /api/v1/nonexistent` | **401** (group auth runs before route match) |

Two things worth calling out:

1. **`/health` uses the correct path.** The compose healthcheck hits
   `/health` (which returns 200), so the container's `(unhealthy)` badge is
   purely the corrupted-exec artifact, not a real failure. (Contrast:
   shipping-core's health path is `/healthz`.) **[live-curl + source]**

2. **`/ready` reports the DB down even though `ingestion-postgres` is
   healthy.** The 2s `pool.Ping` times out (`context deadline exceeded`) while
   NATS pings OK. `docker ps` shows `ingestion-postgres Up 2 days (healthy)`,
   the finspo binary applied its migrations at startup 2 days ago, and no host
   port is published for the DB so it cannot be probed directly. This is a
   **live degraded signal worth investigating** — most likely a stale/saturated
   pgx pool on a 2-day-old process running on a heavily-degraded docker host,
   rather than a code defect. It does not affect `/health` or the (auth-gated)
   API surface, but a sync run would fail its cursor writes in this state.
   **[live-curl] — WARNING.**

## Authorization posture — [live-curl + source]

Materially **stronger** than the imports-core baseline concern (which ran
handler logic before authz). `internal/auth/middleware.go`:

- API key checked **first** with `crypto/subtle.ConstantTimeCompare`; accepts
  either the configured header (`X-API-Key`) or `x-internal-api-key`.
- Org context (`X-Org-ID`) required **after** a valid key → `400 organization
  context is required`; org is pinned into `c.Locals` and read by every handler.
- Every `/api/v1/*` route (sharepoint browse, sources, analytics,
  recommendations, proposals) is behind the middleware group; only `/health`
  and `/ready` are unauthenticated. No route executes before authz.
- The external `FINSPO_API_KEY` is distinct from the shared `INTERNAL_API_KEY`
  (the latter is used only outbound, to the integration-core token broker).

## Architecture conformance — [source-only]

All three cross-plane rules hold:

- **Data Plane persistence goes only through the documents-api HTTP boundary.**
  Metadata → `POST /v1/source-objects/` (+ `/delete`); content-bearing files →
  `POST /v1/documents`. No direct Data Plane DB access
  (`internal/dataplane/*.go`).
- **Graph tokens are brokered through integration-corev2**
  (`POST /internal/connectors/token`, `connectorType=microsoft-graph`,
  `X-Internal-API-Key`). finspo never handles OAuth directly
  (`internal/sharepoint/http_token_provider.go`).
- **ZDR propagates.** The content ingestor stamps `zdr_classification`
  (default `internal`) on every forwarded document
  (`internal/content/ingestor.go`).

## Delta sync — REAL, not scaffolded — [source + tests]

- `internal/sharepoint/delta_client.go`: real Graph calls to
  `/v1.0/drives/{driveID}/root/delta`, proper `@odata.nextLink` /
  `@odata.deltaLink` pagination, bearer auth, Graph error surfacing.
- `internal/sync/delta.go` (Engine): resumes from the persisted `deltaLink`
  cursor; handles upserts, tombstone soft-deletes (including "unknown item"
  tombstones), best-effort per-file ACL capture, event publication, optional
  content forwarding, and `PageLimit` guarding against runaway initial crawls.
- `internal/sync/scheduler.go`: fans `SyncDrive` across enabled sources every
  `FINSPO_SYNC_INTERVAL` (default 5m) with per-source advisory locks.
- Content path (`internal/content` + `internal/extract`): downloads bytes,
  extracts text with a **real** PDF library (`github.com/ledongthuc/pdf`, with
  panic-recovery) plus plaintext formats, sanitizes to valid UTF-8, and
  forwards with a stable idempotency key
  (`finspo-sp:{driveID}:{itemID}`) so re-sync re-indexes in place.

Whether sync actually moves data depends on a seeded `microsoft-graph`
connection in integration-corev2 for the org; with none, the token broker
returns empty and the delta client cleanly returns `ErrNotConfigured` (inert,
not broken).

## Mocks / stubs / dead code — NONE in production code — [source-only]

Grep for `TODO|FIXME|mock|stub|fake|placeholder|not implemented|dummy|xxx`
across non-test `.go` returned only:

- Comments describing **test** fakes (`sources_handlers.go`, `sync/delta.go`,
  `sync/scheduler.go`).
- One benign telemetry note: OTEL endpoint "configured but not yet exported"
  (`internal/telemetry/telemetry.go`).

The memory-noted "finspo dead-code fix" is resolved: the formerly-inert Data
Plane forwarding hook now has a real content extractor behind it (the
`internal/content` package header documents this explicitly). Nil-safe
optional degradation is by design, not a stub: empty `NATS_URL` → no-op
publisher; unconfigured Data Plane token → no-op sink; content sink stays nil
unless `FINSPO_CAPTURE_CONTENT=true`.

## Uncommitted work — COMPLETE, not WIP — [git + source]

10 files, +49/-52. A single coherent rename:
`DataPlaneAPIKey` → `DataPlaneServiceToken` (env `DATA_PLANE_API_KEY` →
`DATA_PLANE_SERVICE_TOKEN`) across `config.go`, `cmd/api/main.go`,
`cmd/backfill-source-objects/main.go`, `dataplane/documents.go`,
`dataplane/source_objects.go`, their tests, `.env.example`, and
`docs/REQUIREMENTS.md`. This is the **ZDR / Control-issued short-lived
service-JWT** hardening (Data Plane now verifies + pins the tenant from the
credential and rejects caller-selected identity headers), matching the
resolved Data Plane Phase 2 contract note. It builds clean and the full test
suite is green — this is a finished refactor, not half-applied WIP.

Caveat: the running `finspo-api` image is ~2 days old and rebuild is
Docker-blocked, so the running binary predates this rename. It does not matter
functionally here because `DATA_PLANE_SERVICE_TOKEN` is empty by default in
compose (`${DATA_PLANE_SERVICE_TOKEN:-}`), so Data Plane forwarding is a no-op
in this deployment regardless of which env name the binary reads.

## Destructive execution — hard-gated and OFF — [source + compose]

`internal/sync/executor.go` returns `ErrExecutionDisabled` **before** any Graph
mutation when `AllowExecution=false`, and also requires the proposal to be
`Approved` (`ErrNotApproved`). `FINSPO_ALLOW_EXECUTION` is **not set** in
`docker-compose.yml`, so it defaults `false`: archive/delete proposals cannot
execute in the running deployment. Correct and safe. (The default read-only
Nango seed also lacks `Files.ReadWrite.All`/`Sites.ReadWrite.All`, a second
layer of safety.)

## Route inventory — [source-only]

All `/api/v1/*` are auth + org gated. `/health`, `/ready` are open.

- SharePoint browse: `GET /api/v1/sharepoint/sites`, `.../sites/:siteID/items`
- Sources: `POST|GET /sources`, `GET /sources/:id`, `GET /sources/:id/status`,
  `POST /sources/:id/sync`
- Analytics: `GET /analytics/{largest,inactive,by-site,duplicates}`
- Recommendations: `GET /recommendations`
- Proposals: `POST|GET /proposals`, `GET /proposals/:id`,
  `POST /proposals/:id/{approve,reject,execute}`

Schema is real: embedded migrations `0001_init.sql`,
`0002_review_proposals.sql` applied at startup via `internal/db/migrate.go`.

## Relation to the audit's headline goals

finspo-core is **not** on the "shipping time Oslo→Trondheim" or "Visma MCP"
critical path — those belong to shipping-core and integration-corev2. finspo
is a background delta-sync connector with an admin/governance API; it is **not
registered as a Model Plane chat tool**. No action needed here for the headline
goals beyond noting the boundary.

## Bottom line

finspo-core is honest, real, and well-tested: delta sync, ACL capture, PDF/text
extraction, and Data Plane forwarding are all implemented and green, auth is
enforced before every handler, and destructive execution is hard-disabled. The
one live blemish is `/ready` reporting the finspo DB pool as timed-out against
an otherwise-healthy `ingestion-postgres` — probably an environmental
pool/host-degradation artifact, but it should be re-checked once the docker
host is healthy, since a real DB outage would silently fail sync cursor writes.

## Doc cleanup read

Keep `finspo-core/docs/{ARCHITECTURE.md,API.md,REQUIREMENTS.md}`. No
delete-ready service-local docs found.
