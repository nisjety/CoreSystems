# notification-core

> **2026-07-13 superseding update.** Canonical intake remains `POST /api/v1/notification-requests`. Changed source requires organization-scoped idempotency, a typed Control user recipient, membership authorization, caller workflow allowlists, and service-specific HMAC delegation bound to tenant/user/role/method/URI/body/timestamp/nonce. Feed, preferences, channels, subscribers, and storage are organization scoped. Support requests are locally ZDR: payload content is transient, no content/feed projection is persisted, and lifecycle events redact the recipient. The full payload still reaches Novu and provider-side ZDR/retention is not proven, so no end-to-end ZDR claim is made. Explicit `novu`/`disabled` mode cannot fabricate delivery; provider acceptance is only `submitted`. Signed clients reject redirects. The Go race suite passes, but measured coverage is delegation 81.4%, notification 59.1%, HTTP 32.6%, and database 41.0%. The running image is stale: `/ready` is 404 and missing Novu configuration can still claim synthetic submission. Secure source has no trustworthy Control membership writer/backfill, so it intentionally denies legitimate access. Delivery/feed outbox, provider callbacks/reconciliation, durable preference sync, and HA replay state are absent. Support automation remains default-disabled because workflows do not supply authoritative organization/user mappings. Treat older stub/corruption claims below as July 11 history.

_Audit refreshed 2026-07-11. Evidence grades: [live-curl] host curl to :3140, [inspect] docker inspect/ps, [source-only] read from disk (Docker exec/build/logs unavailable — containerd content store corrupted)._

## Current State

`notification-core` (Go, container `notification-core`, host `:3140`) is the first-party
notification boundary for the Application Plane. It is **real and materially broader than
its own README**, which still lists inbox feeds, preferences, and read state as "non-goals for
V0" — those surfaces are now fully implemented in live code. [source-only]

At startup (`cmd/server/main.go`) it: loads config, connects `application-postgres`, runs
migrations (001–006), connects the local NATS, wires request/feed/preference/channel/subscriber
services, optionally connects the **shared** verevon-nats bus for cross-plane consumers, then
starts HTTP + background subscribers. [source-only]

- `/health` → `200 {"service":"notification-core","status":"ok"}` [live-curl]
- Container `running`, `StartedAt 2026-07-09`, reported `unhealthy` — this is the
  fleet-wide exec-based-healthcheck failure (containerd corruption), **not** the service being
  down; HTTP is serving. [inspect]
- `go build ./...` exit 0, `go vet ./...` clean, `go test ./...` all pass
  (config, consumers, http, notification, runtime green; other packages have no tests). Host go1.26.2. [source-only]
- Worktree **clean** for the service dir — `git status --porcelain` returns 0 lines; no uncommitted WIP.
  Last touched by commits `9a9ac7fc` (provider-business-modules baseline) and `1141ddcf`
  (insight-core/notification W3 producer legs + daily_brief delivery). [inspect]

## Entry Points

- Main: `apps/Application Plane/notification-core/cmd/server/main.go`
- Routes: `apps/Application Plane/notification-core/internal/http/server.go`
- Handlers: `apps/Application Plane/notification-core/internal/http/handlers.go`

## Exposed Surface (verified against `server.go`)

- `GET  /health`, `GET /ready` — unauthenticated liveness/readiness
- `POST /api/v1/notification-requests` — **the real notify/dispatch route**, signed caller + active membership required
- `GET  /notifications` — signed organization/user feed with active membership required
- `GET  /notifications/unread/count`, `GET /notifications/unseen/count`
- `POST /notifications/:id/read`, `POST /notifications/:id/seen`
- `POST /notifications/mark-all-read`, `POST /notifications/mark-all-seen`
- `DELETE /notifications/:id`
- `GET  /preferences`, `PUT /preferences/:eventType/:channel`
- `GET  /channels/config`, `PATCH /channels/config/:eventType/:channel`

There is no `/v1/notifications` or `POST /notifications`. The internal recipient-upsert route was removed; unsigned shared identity events are not authorization.

## Auth model [current changed source]

Every non-health route verifies a service-specific HMAC v2 delegation. The canonical signature binds service identity, `notification-core` audience, method, request URI, user, organization, role, timestamp, nonce, and body digest. Gateway and support credentials are separate; the raw credential is not sent. Missing, stale, replayed, body/method/URI/org/user/role-mismatched, legacy-key-only, and known placeholder credentials fail closed. Feed/preferences/channel routes additionally require an active Control-derived local membership; the gateway session is not itself a membership grant. Channel changes require admin role. The nonce cache is currently process-local, so restart/multi-replica replay protection remains incomplete.

## RESOLVED: the "POST /v1/notifications 404" route mismatch

**This is a caller bug, not a service bug.** The service's notify entrypoint is
`POST /api/v1/notification-requests` and it is correctly registered and gated (202 Accepted on
success; 400 validation; 502 on runtime-dispatch failure). [source-only + live-curl]

Current changed callers:
- `support-worker` uses the canonical route and typed ZDR contract with timeout/strict response checks, but is default-disabled because its workflows lack authoritative Control user/organization mapping.
- `insight-core` notification delivery is removed/disabled until a real subscription/user mapping exists.
- Verevon v3 notification feed/preferences use the signed organization/user proxy. Navbar support returns honest 503 instead of manufacturing a self-notification.
- Any remaining legacy/shared-key caller must migrate to an explicitly allowed service contract before deployment.

Also: verevon's admin health dashboard probes `notification-core:3140/healthz`
(`apps/Frontend Plane/verevon/src/app/api/admin/health/route.ts:57`) but the service only serves
`/health` — that dashboard will show notification-core as down (404). Caller-side path bug. [source-only + live-curl]

The verevonv3 gateway wiring is correct: `apps/gateway/src/domains/notifications.rs` and
`navbar.rs` proxy its public `/api/v1/notifications*` surface to notification-core's
`/notifications*` gated routes. [source-only]

## Historical July 11 DB finding (superseded)

Every DB-backed read (`/notifications`, `/notifications/unread/count`, `/notifications/unseen/count`,
`/preferences`, `/channels/config`) currently returns **HTTP 500** despite `/health`=200 and auth
passing. [live-curl]

Root cause confirmed by a direct pgx probe from the host to `application-postgres` (:9540):

```
FATAL: could not open file "global/pg_filenode.map": I/O error (SQLSTATE 58030)
```

The Postgres **data volume is corrupted at the storage layer** — the same containerd/overlay I/O
corruption that breaks docker exec/build/logs has now reached the DB files. `application-postgres`
shows `Up 2 days (unhealthy)` and its unhealthy state here is *real*, not just the healthcheck
caveat. notification-core connected fine at startup (2026-07-09, else migrations would have
`log.Fatalf`'d) but reads now fail because the DB backing store is bad. [live-curl + live DB probe + inspect]

The service code is correct: `feed/repository.go` columns match migration `003_create_feed_items.up.sql`
exactly; the handlers surface a logged 500 on the DB error. Minor code nit: DB-unavailable is
reported as 500 rather than 503, masking an upstream-down condition as an internal error. [source-only]

## Feed / preferences / channels / subscribers ARE implemented (README is stale)

- **Feed** (`internal/feed/`): paginated list, unread/unseen counts, mark-read/seen, mark-all,
  archive (soft delete), idempotent create via `ON CONFLICT (recipient_id, channel,
  provider_transaction_id)`. Real pgx queries, real schema. [source-only]
- **Preferences** (`internal/preferences/`): per-`(user,event_type,channel)` toggle, writes
  best-effort synced to Novu. [source-only]
- **Channel configs** (`internal/channels/`): org-level `(event_type,channel)` policy with a
  seeded `_default` org policy (migration 005) covering session/auth/billing/org/mention/product
  events; migration 006 registers `daily_brief`. [source-only]
- **Subscribers** (`internal/subscribers/`): recipient identity upsert; dispatch auto-ensures a
  subscriber row via `WithSubscriberEnsurer`. [source-only]
- **Feed sink**: every dispatched notification is mirrored into `notification_feed_items` as an
  in-app row (`main.go` `WithFeedSink`), best-effort. [source-only]

## Cross-plane consumers [current changed source]

The prior shared-bus consumers are retained as dormant code but are not started. Shared NATS subjects and credentials do not provide signed, revisioned tenant authority, so using them for membership/session/social notification decisions would be fail-open. A Control-authoritative projection writer/backfill is required before secure deployment. Historical consumers were:
- `ControlSessionSubscriber` — CP session-core session events → notification.
- `IdentitySyncSubscriber` — `auth.user.>` + `org.member.>` → local subscriber row + Novu identify.
  (Handles member add/removal locally; unrelated to the Convex `onOrganizationMemberRemoved`
  undefined-handler bug, which is a `convex-core` issue.)
- `SocialPublishFailedSubscriber` — `verevon.application.social.publish_job.failed` from social-core
  → user-facing notification via the same `Accept` dispatch path. Real, not decorative. [source-only]

`ensureSharedConsumerStream` provisions/patches the `VEREVON_SHARED_CONSUMERS` stream for
`auth.user.>` and `org.member.>`.

## Stub / Mock / Placeholder Audit

1. **Novu runtime adapter — honest documented dev fallback, not a deceptive stub.**
   `NOVU_SECRET_KEY` unset → stub mode: logs a one-time warning, `Dispatch` returns synthetic
   `novu_req_*` transaction IDs, `IdentifySubscriber`/`UpdateSubscriberPreference` are no-ops.
   The live container has `NOVU_SECRET_KEY` **empty** → it is running in **stub mode**, so no real
   external (in-app/email) delivery happens end-to-end here; requests are accepted (202) and a feed
   row would be written if the DB were healthy. This is expected for an env with no Novu account,
   but it means "real delivery" is unverifiable in this environment. [source-only + inspect]

2. **Subscriber stub row** (`subscribers/service.go:97`) — intentional: inserts a sparse row that
   later richer identity events merge into. Documented design, not a fake. [source-only]

3. **`RESEND_*` env vars are inert.** `.env`/container carry `RESEND_API_KEY=re_placeholder` and
   `RESEND_FROM_*`, but Resend is **not wired** anywhere in the Go code — delivery is Novu-only.
   Vestigial config; safe to ignore or remove. [source-only + inspect]

4. **README documentation drift** — README still calls feed/preferences/read-state "non-goals";
   they are live. This doc supersedes the README. [source-only]

5. No deceptive production stubs. `TODO/FIXME/fake/placeholder` hits are confined to the honest
   Novu-stub docstrings, the subscriber-merge comment, an `iota` enum constant, and `_test.go`
   fakes. [source-only]

## Persistence boundary

Uses shared `application-postgres` (`application_plane` DB) with its own tables prefixed
`notification_*` and its own `notification_core_schema_migrations` tracker — no cross-plane table
crossing. NATS split is correct: local bus for its own publisher, shared bus consumed read-only. [source-only]

## Bottom line

Source now has a materially safer organization/user boundary, typed recipients, ZDR support path, and honest submitted/disabled delivery semantics. It is not production-ready or deployed. Legitimate access remains unavailable until Control supplies trustworthy membership authority; provider/feed durability, callbacks/reconciliation, HA replay protection, dependency readiness, and critical-module coverage remain release blockers. The running image is old and must not be used as evidence for the source fixes.
