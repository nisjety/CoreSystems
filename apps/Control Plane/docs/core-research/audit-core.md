# audit-core Research Dive

Generated: 2026-06-07
Updated: 2026-07-11 — production-readiness continuation. The 2026-07-10 SQL and network findings are fixed and live-verified; durable delivery remains open.

Scope: `apps/Control Plane/audit-core`

## 2026-07-11 production-readiness addendum (current)

Both 2026-07-10 live defects are fixed and deployed. `/v1/usage/summary` executes the corrected aggregate query and returned 200 against the live schema. `/readyz` returned 200 with `database_connected=true`, `primary_nats_connected=true`, `extra_nats_connected=[true]`, and `delivery_mode="jetstream_durable"`. Prometheus exposes per-bus connectivity plus event result/age metrics.

Audit and usage subjects on both buses now feed named durable consumers on a file-backed `VELION_CONTROL_OBSERVABILITY` stream. Database success is ACKed; a transient store failure is NAKed for bounded redelivery; malformed or five-times-failed messages are durably copied under `velion.dlq.audit-core.*` before TERM. Readiness marks an extra bus degraded if it connects but cannot create its consumers. Live startup logs confirm `audit-core-primary-{audit,usage}` and `audit-core-extra-1-{audit,usage}`.

`go test ./...` and `go vet ./...` pass. An embedded JetStream integration/race test proves successful persistence/ACK, transient retry, malformed DLQ, five-delivery exhaustion, and stream update. Final review then added migration `002_jetstream_inbox`: `(source_bus, source_stream_sequence)` is unique per audit/usage table, duplicate delivery ACKs without insertion, and a source message is TERM'd only after confirmed durable DLQ publication. Disposable Postgres 16 proves the migration ledger and one-row redelivery semantics. Subscriber coverage is 72.0% after the new branches.

The final Audit image was recreated, but Docker Desktop immediately developed containerd/BuildKit storage I/O errors and live Postgres became inaccessible/inconsistent before migration 002 could be verified. Therefore consumer durability was live before the incident, while inbox idempotency is **source + isolated integration only**. Remaining Audit MVP work is a Docker recovery/live migration check, transactional producer outboxes with JetStream publish acknowledgements, pending/backlog visibility, replay rehearsal, and replacement of the shared inbound HTTP key.

## Snapshot

`audit-core` is still the smallest Control Plane core, but it is no longer just "subscribe, persist, read." Since the 2026-06-07 pass it picked up:

- a daily retention-purge loop (`AUDIT_RETENTION_DAYS`, default 365, matching the velion settings-UI copy)
- a dedicated Prometheus `/metrics` server on port 9091 (Phase 6 B13), separate from the app port so scraping doesn't need the internal-API-key
- an HTTP ingest endpoint (`POST /v1/audit`) so non-NATS callers (e.g. the velionv2 BFF) can write audit rows through the same validate/normalize path as the subscriber
- a versioned `schema_migrations` ledger (Phase 6 B4) replacing the old "re-run idempotent DDL on every boot" approach
- cross-plane NATS aggregation via `EXTRA_NATS_URLS` — audit-core is meant to be the single sink across all plane buses, not just the Control Plane's own bus
- a hardened internal-auth default: the docker-compose `INTERNAL_API_KEY` default was a weak `dev-super-secret-...` string until commit `49dc5720` ("fix(security): harden internal-auth — kill weak defaults + close fail-open holes", 2026-07-07) made it fail loud (`:?INTERNAL_API_KEY must be set`) instead

Non-generated/non-vendored Go file count: 8 implementation files (`cmd/server/main.go`, `internal/api/api.go`, `internal/events/events.go`, `internal/metrics/server.go`, `internal/store/store.go`, `internal/store/migrate.go`, `internal/subscriber/subscriber.go`, plus embedded `internal/store/schema.sql`) + 2 test files (`internal/api/api_test.go`, `internal/store/store_test.go`, 322 lines combined).

Build/vet/test, run live today: `go build ./...` clean, `go vet ./...` clean, `go test ./...` → all green (`internal/api` and `internal/store` packages have tests; `cmd/server`, `internal/events`, `internal/metrics`, `internal/subscriber` have none).

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - config load (`DATABASE_URL`, `NATS_URL`/`NATS_TOKEN`, `EXTRA_NATS_URLS`, `HTTP_PORT`, `METRICS_PORT`, `INTERNAL_API_KEY`/`INTERNAL_SERVICE_SECRET`, `AUDIT_RETENTION_DAYS`)
  - Postgres connect + ping + `store.Migrate` (ledgered, see below)
  - primary NATS connect (fatal on failure — this is the Control Plane's own bus)
  - loop over `EXTRA_NATS_URLS`: connect + subscribe each extra plane bus, **best-effort** (warn + skip on failure, never fatal)
  - retention goroutine (`runRetention`): purges once at boot, then every 24h, until context cancellation
  - chi router mount + HTTP server on `HTTP_PORT` (default 8187)
  - Prometheus metrics server on `METRICS_PORT` (default 9091)
- `internal/api/api.go`
  - `GET /healthz`, `GET /readyz` (unauthenticated, both just return `{"status":"ok"}`)
  - `/v1/*` routes behind `internalAuth` middleware (constant-time compare against `INTERNAL_API_KEY`, header `X-Internal-Api-Key` or `X-Api-Key`)
- `internal/subscriber/subscriber.go`
  - queue-subscribes `velion.audit.v1.>` and `velion.usage.v1.>` under queue group `audit-core` on whichever `*nats.Conn` it's given (used once for the primary bus, once per extra bus)
- `internal/events/events.go`
  - shared wire-format structs (`AuditEvent`, `UsageEvent`) + `Validate()`/`Decode*()` — same functions used by both the NATS path and the HTTP ingest path, so the two paths can't drift
- `internal/store/store.go` + `migrate.go` + `schema.sql`
  - `InsertAudit`, `InsertUsage`, `ListAudit`, `ListUsage`, `SummariseUsage`, `Purge`
  - `Migrate` applies the embedded schema once inside a transaction and records it in `schema_migrations` (version `001_init`); safe to re-run against a pre-ledger DB because the DDL itself is idempotent
- `internal/metrics/server.go`
  - separate `http.Server` mounting only `promhttp.Handler()` + a bare `/health` — **no custom application metrics are registered anywhere in the codebase** (confirmed by grep: zero `Counter`/`Histogram`/`Gauge`/`MustRegister` call sites). `/metrics` only exposes the Go process's own runtime stats (GC, goroutines, memstats) today, not audit-ingestion counts, drop counts, or extra-bus connectivity state.

## API And Relationship Map

HTTP surface (all confirmed live against the running container on 2026-07-10):

- `GET /healthz` — 200, unauthenticated
- `GET /readyz` — 200, unauthenticated (identical handler to `/healthz`, not a real dependency check)
- `GET /v1/audit?org_id=...` — org-scoped list, internal-key gated
- `POST /v1/audit` — HTTP ingest, internal-key gated, 1 MiB body cap
- `GET /v1/usage?org_id=...` — org-scoped list, internal-key gated
- `GET /v1/usage/summary?org_id=...` — org-scoped aggregate, internal-key gated — **currently broken, see Bugs below**
- `GET /metrics` (port 9091, not exposed on the host) — Prometheus scrape target, unauthenticated by design (separate port so it doesn't need the internal key)

Required query/auth semantics: internal API key via `X-Internal-Api-Key` or `X-Api-Key`; every `/v1/*` read requires `org_id` (400 if missing) and every write requires `org_id`+`plane`+`event` (400 if any missing, enforced by `events.Validate()`).

Current relationships:

- `audit-core` → primary NATS (`controlplane-nats:4223` externally / `controlplane-nats:4222` in-network) — **confirmed subscribed and healthy** (see live checks)
- `audit-core` → extra plane NATS buses via `EXTRA_NATS_URLS` — **configured but not currently connected**, see Bugs below
- `audit-core` → Postgres (`controlplane-postgres`) — durable audit + usage persistence, schema-ledgered
- upstream publishers of `velion.audit.v1.*` / `velion.usage.v1.*` confirmed present in code today (grep, not just the 2026-06-07 doc's claim): `auth-core` (`audit-plugin.ts`, `organization-hooks.ts`, `auth-event.publisher.ts`, `direct-nats.service.ts`), `user-core` (`gdpr.go`, `service.go`), `org-core` (`service_enhanced.go`, `gdpr_handlers.go`, `nats/client.go`)
- downstream consumer: the velion `/settings/audit-log` and `/settings/usage` UI views read `GET /v1/audit` and `GET /v1/usage/summary` respectively (per the API doc-comments in `api.go`) — the summary path being broken (below) means the usage dashboard is not actually servable right now, independent of whether any usage data exists

## Live Verification (2026-07-10)

Container: `audit-core-service`, healthy, port 8187 exposed to host. `docker inspect` shows `Created: 2026-07-07T21:12:33Z`; `docker ps` shows "Up 13 hours" (i.e. restarted since creation, but not recreated — network attachments from creation time persist across a plain restart).

- `go build ./...`, `go vet ./...`, `go test ./...` — all clean/green.
- `printenv` inside the container confirms: `DATABASE_URL`, `NATS_URL=nats://controlplane-nats:4222`, `NATS_TOKEN=nats`, `EXTRA_NATS_URLS=nats://model-plane-nats-1:4222`, `HTTP_PORT=8187`, `INTERNAL_API_KEY=<64-hex>` (non-default, confirms the 49dc5720 fail-loud fix is actually deployed, not just committed).
- `curl http://localhost:8187/healthz` → 200; `/readyz` → 200.
- Auth checks: no key → 401; wrong key → 401; correct key + `org_id` → 200. Confirms the constant-time compare and the "no weak default" fail-loud path both work as coded.
- `GET /v1/audit?org_id=org_1782152609927&since=2020-01-01T00:00:00Z` returned 8 **real** rows — genuine historical events, not fixtures: Model Plane `tool_action` events (tools `news`, `yr_weather`, `fetch_url`, `company_lookup`, with `details.data_category`/`details.zdr` fields present, matching the ZDR/GDPR tagging described elsewhere in the program), and an Application Plane `lead_export` event referencing a real lead list. `audit_events` table total: 11 rows (the above 8, plus 2 `erasure` test rows from 2026-06-26, now 12 after the round-trip test below).
- Live round-trip test performed: `POST /v1/audit` with a synthetic event (`org_id=audit-live-verify-20260710`) → 202 Accepted; immediate `GET /v1/audit?org_id=audit-live-verify-20260710` → the row comes back with the correct `id`, `ingested_at`, and normalized `outcome:"ok"` default. This proves the full ingest → validate → persist → query path is genuinely wired end-to-end today, not mocked.
- `usage_events` table: **0 rows**, always has been in this environment. `GET /v1/usage?org_id=...` correctly returns `{"data":[],"meta":{"count":0}}` (200) — the list path itself is fine with no data. No conclusion is drawn about whether usage/cost producers are broken elsewhere; this environment simply has never received a `velion.usage.v1.*` message or an ingest for usage.
- All 11 pre-existing `audit_events` rows have `ingested_at` **older** than the current container's `Created` timestamp (2026-07-07T21:12:33Z) — i.e. every real event in the table today was written by a *previous* instance of this container (Postgres data survives recreation; the app container does not carry state). Since the current container came up, exactly one row has landed: the synthetic test event injected above via HTTP, not via NATS. This is not by itself alarming (it may simply mean no audit-worthy action has occurred against this live stack in the last 3 days), but it does mean the NATS ingestion path has had zero live exercise on the current container instance — so the bug below could have been silently open the entire time without symptoms.

## NATS Wiring — Confirmed Present, Partially Broken

Per the task's flag to verify: **the dual-subscribe design is real and implemented in code** (`main.go` loops `EXTRA_NATS_URLS`, connects + starts a second `subscriber.Subscriber` per extra bus, logged separately from the primary), and `EXTRA_NATS_URLS` **is** wired into the running container's environment (`nats://model-plane-nats-1:4222`, sourced from `docker-compose.yml`'s `${AUDIT_EXTRA_NATS_URLS:-nats://model-plane-nats-1:4222}`).

However, live logs show it is not actually working right now:

```
{"level":"info", ..., "message":"audit-core subscribed"}   <- primary bus (controlplane-nats), OK
{"level":"warn","error":"dial tcp: lookup model-plane-nats-1 on 127.0.0.11:53: no such host",
 "nats_url":"nats://model-plane-nats-1:4222","message":"extra nats bus connect failed; skipping"}
```

Root cause, confirmed via `docker inspect`: `docker-compose.yml` declares `audit-core-service` on **two** networks (`controlplane-net` and `inter-plane-bus` — `model-plane-nats-1` lives on `inter-plane-bus`), but the **currently-running container is attached to `controlplane-net` only**. Git history pins this precisely: the `inter-plane-bus` line for `audit-core` was added in commit `49dc5720` at `2026-07-07 23:16:21 +0200` (`21:16:21 UTC`) — **4 minutes after** the running container's `Created` timestamp (`21:12:33 UTC`). The same commit also removed the weak `INTERNAL_API_KEY` default (confirmed deployed, per the printenv check above), so the container was clearly recreated for that change, but recreated a few minutes *before* the network line was added to the compose file — and nothing has recreated it since.

Effect: cross-plane audit aggregation for Model Plane `tool_action` events (the exact use case the code's own comments describe — "session-core publishes `velion.audit.v1.model.tool_action`" on the model-plane bus) is currently **not happening**. By design this fails soft (logged warning, not fatal), so the service itself is healthy and nothing pages — but it means the tool_action audit trail this program has repeatedly described as "closed" is not actually flowing at runtime right now. The 8 real `tool_action` rows already in the table (dated 2026-06-22/23) predate this container entirely, so they don't prove current connectivity — see the timestamp analysis above.

**Fix**: recreate the container so it picks up the network config already committed in `docker-compose.yml` — `docker compose -f "apps/Control Plane/docker-compose.yml" up -d --force-recreate audit-core` (or an equivalent `docker compose up -d` that detects the network diff). No code or compose change is needed; this is a pure "the file says two networks, the live instance has one" drift, not a design gap. Recommend verifying after recreate: `docker exec audit-core-service getent hosts model-plane-nats-1` should resolve, and the boot log should show `"audit-core aggregating extra plane bus"` for `nats://model-plane-nats-1:4222` instead of the warn line above.

## Bugs Found (Live-Reproduced)

1. **`GET /v1/usage/summary` returns HTTP 500 unconditionally** (confirmed live, not data-dependent). Container logs show the underlying Postgres error:
   ```
   ERROR: column "usage_events.cost_cents" must appear in the GROUP BY clause
   or be used in an aggregate function (SQLSTATE 42803)
   ```
   Cause, in `internal/store/store.go` `SummariseUsage`: the query does `GROUP BY plane, op` and selects `COALESCE(SUM(cost_cents), 0)`, but the `ORDER BY` clause sorts by the bare `cost_cents DESC NULLS LAST, events DESC` instead of the aggregated expression (`SUM(cost_cents)` or a column alias). Postgres rejects this at parse time regardless of whether any rows would actually match — it is broken for every `org_id`, including ones with zero usage rows, which is exactly what live-reproduced it here (usage_events is empty in this environment). Note `events DESC` in the same ORDER BY is fine since `events` is itself the `COUNT(*)` alias — only the `cost_cents` reference is the problem.
   - Impact: this is the exact endpoint the velion `/settings/usage` dashboard is documented (in the handler's own comment) to read directly — so that dashboard is unconditionally broken today, independent of whether Model Plane cost/usage events ever start flowing.
   - Test coverage: neither `internal/store/store_test.go` nor `internal/api/api_test.go` has any test that calls `SummariseUsage`/`summariseUsage` — confirmed by grep (zero hits). This is why the bug shipped and has stayed unnoticed; `go test ./...` is green today only because nothing exercises this path.
   - Fix shape (not applied — out of scope for this audit pass): change `ORDER BY` to reference the aggregate, e.g. `ORDER BY SUM(cost_cents) DESC NULLS LAST, COUNT(*) DESC`, and add a test that calls it against an empty and a populated `usage_events` table.

2. **Cross-plane NATS aggregation (`EXTRA_NATS_URLS`) is configured but not connected on the live container** — see the NATS Wiring section above for full detail and root cause.

## Duplicates, Redundancies, And Non-Relationships

No large duplication cluster found. The HTTP ingest path (`POST /v1/audit`) and the NATS subscriber intentionally share the same `events.DecodeAudit`/`Validate()` function rather than duplicating normalization logic — this is a good pattern, not a redundancy.

Intentional simplicity retained from the original pass:

- one subscriber implementation, reused per NATS connection (primary + N extras)
- one store implementation
- one mounted API surface

Non-relationship / design constraint (unchanged from 2026-06-07): the core trusts the caller to attach a verified `org_id`; it does not independently resolve org identity from auth-core. This is consistent with the internal-key-gated, backend-to-backend nature of the service, but is worth flagging alongside the broader Control Plane `X-Org-ID` trust-boundary concern raised in `plane-audit-2026-07-02.md` — `audit-core` is one more internal consumer that assumes upstream org-scoping is already correct by the time an event or query reaches it.

## Stubs, Placeholders, And Failure Semantics

Grep for `TODO|FIXME|mock|stub|fake|placeholder|not implemented` across all `.go` files in the tree turned up **only test-only seams**, exactly as the 2026-06-07 pass found — nothing new:

- `internal/api/api_test.go:18,27,32` — comment-documented test stub for `store.Store`'s `InsertAudit`, explaining why a real interface isn't mocked (the store is concrete)
- `internal/store/store.go:19` — doc-comment noting tests *could* swap in an in-memory mock (they don't currently; `store_test.go` exercises `Purge` against real SQL logic, not a mock)

No runtime mocks, stubs, or "not implemented" paths exist in the current tree. This remains an honest, non-fake service.

Important runtime behavior (unchanged, reconfirmed in code):

- malformed NATS events are dropped (logged `warn`, not retried, not NAK'd)
- store errors on the NATS path are logged (`error`) and dropped — never retried
- the HTTP ingest path (`POST /v1/audit`), by contrast, does surface store errors as a 500 to the caller, so HTTP-side callers get a real failure signal that NATS-side publishers don't
- the extra-NATS-bus loop is explicitly best-effort per the code comment: an unreachable or misconfigured plane bus must never take the whole ingestion service down — confirmed live today (see Bugs #2), the service stayed healthy through the connection failure

## API Design And Performance Notes

API design: small, coherent, internal-only; read endpoints are list/query oriented and correctly scoped by `org_id` at the SQL layer (not just the handler layer — `WHERE org_id = $1` is always present, so an org-scoping bug in the handler couldn't fan out across tenants).

Performance and operational notes:

- queue subscriptions correctly distribute work across replicas (unchanged from original pass)
- at-most-once handling is acceptable for this observability role, but trades completeness for throughput — reconfirmed relevant given the NATS wiring finding: a plane bus being unreachable doesn't queue or retry, it just silently misses events until someone notices
- retention purge is bounded (5-minute timeout per sweep) and runs once at boot + daily — reasonable for an append-only table at this scale
- `/metrics` exposes zero business-relevant counters today (see Runtime Shape) — a `nats_extra_bus_connected` gauge or `audit_events_ingested_total{source="nats"|"http"}` counter would have caught the Bugs-#2 drift automatically instead of requiring a manual docker-log grep during this audit

## Current Doc Cleanup Read

No delete-ready core-local docs identified. This file itself needed the update it just received — the 2026-06-07 version undercounted the service's actual surface (missed retention, metrics, HTTP ingest, the migration ledger, and the extra-NATS-bus design entirely).

## Cross-Reference: Known Baseline (2026-07-10 pass)

- `test-control-plane-integration.sh` 9/9 pass — consistent with what's found here; that script does not appear to exercise `/v1/usage/summary` or the extra-NATS-bus path, so it would not have caught either bug in this doc.
- `test-all-services.sh` — confirmed stale (per the baseline given for this pass); not re-litigated here since it isn't audit-core-specific.
- `audit-core` itself has **no uncommitted working-tree changes** — `git status --porcelain` for the directory is empty. The uncommitted WIP diff listed for this pass (auth-core org-events plugin, org-core/user-core/billing-core repository/handler changes) does not touch `audit-core`. The most recent commit that *did* touch this service's operational config is `49dc5720` (2026-07-07), which is also the commit responsible for the NATS-wiring drift documented above — i.e. the drift is a deployment/ops gap following a real, already-committed security fix, not an in-flight or half-finished code change.

## Bottom Line

`audit-core` is still simple and its code is honest — no new mocks, stubs, or fakes since the last pass, and the live round-trip test proves the primary ingest→store→query path genuinely works end-to-end today. But two things need attention before it can be called fully trustworthy:

1. **Operational**: the running container is missing the `inter-plane-bus` network attachment that its own compose file already declares, so cross-plane (Model Plane) `tool_action` audit aggregation is silently not happening. Fix is a container recreate, not a code change.
2. **Code**: `GET /v1/usage/summary` — the endpoint the usage dashboard depends on — is unconditionally broken by an invalid `GROUP BY`/`ORDER BY` combination, and has zero test coverage, so it will stay broken until someone adds a test that actually calls it.

Neither issue is catastrophic (both fail soft — a warn log and a 500, not a crash), but both mean the audit/usage trail is currently less complete and less observable than the codebase's own comments claim it to be.
