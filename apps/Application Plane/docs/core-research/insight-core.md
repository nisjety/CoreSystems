# insight-core

> **2026-07-13 superseding update.** The current Postgres container is Docker-healthy, so the July 11 corruption finding below is historical. `insight-core` remains a real source-backed analytics/brief service, but the intended trigger-to-notification path and daily-brief configuration were not proven in this pass. Notification's current tenant/recipient design and stale synthetic-success runtime prevent an end-to-end delivery claim.

_Audit date: 2026-07-11. Evidence grades: **[live-curl]** = verified against the running container via host curl; **[inspect]** = `docker inspect`/`docker ps` config or state; **[source-only]** = read from source/config on disk (docker exec/build/logs unavailable — containerd content store corrupted)._

## Current State

`insight-core` (Go, Gin, container `insight-core`, host `:3163`) is the Application Plane analytics/briefs backend. It owns org-scoped, human-facing **workspace insight projections** across five surfaces — `social`, `inbox`, `agents`, `campaigns`, `external_analytics` — plus a connector registry and a daily-brief assembler/delivery leg.

It is a **real, wired, honest service**, not a placeholder. `go build ./...` is clean and the full unit suite passes (`briefs`, `config`, `consumers`, `http`, `insights`) on go1.26.2 [source-only]. `/health` returns `{"service":"insight-core","status":"ok"}` HTTP 200 [live-curl].

### Resolving the prior contradiction (AI-First "unwired/404" vs. later "real analytics")

The **later view is correct as of this audit**:

- **Wired to Velion v3.** The gateway registers the insights router (`apps/gateway/src/main.rs` merges `domains::insights::router`) exposing `GET /api/v1/insights/connectors` and `GET /api/v1/insights/overview`, proxying to `INSIGHT_CORE_URL` (default `http://insight-core:3163`) and forwarding `x-org-id` from the validated session. `domains/briefs.rs` additionally fans in `/api/v1/insights/overview` for the briefs surface. The SPA has a full `/insights` route tree (`overview|social|inbox|agents|campaigns|experiments`) in `src/app/App.tsx`, an `InsightsPage` component, `@/shared/api/insights-client`, and an `insights-workspace` lib [source-only]. The old "v3 `/api/v1/insights/overview` 404" is **stale** — the route exists end-to-end.
- **Consumes real data, produces no synthetic metrics.** Three producer legs record real lifecycle events as metric events (never fabricated):
  1. `conversation-core` → `surface=inbox` (ticket/conversation/message/ai_action counts) — shared `velion-nats` bus, subject `velion.application.conversation.*`.
  2. `social-core` → `surface=social` (post/campaign/approval/publish_job counts) plus `surface=external_analytics` (real provider metric rows fetched on `metrics.snapshotted` via `GET /api/v1/social/metrics`) — same bus, `velion.application.social.*`.
  3. `model-plane-agents` → `surface=agents` (`RUN_STARTED/COMPLETED/FAILED`, `ACTION_COMPLETED`, `APPROVAL_REQUESTED/DECIDED`) — the isolated **model-plane NATS bus**, subject `mp.v1.run.*.event` + `mp.v1.orchestration.approval`. This is the "RUN_* insights producer" from the Phase 7 B12 work.

  The rollup layer only counts events on an explicit allow-list (`conversationMapping`/`socialMapping`/`agentMetricMapping`); unmapped types are skipped, orgless events are skipped, and producer attribution (`source`) is threaded honestly from the subject domain. Briefs below `BriefMinEvents=5` are labelled `preview` with an honest disclosure — never a fabricated trend.

### LIVE degradation (infra, not code)

`GET /api/v1/insights/overview` currently returns **HTTP 500** for every org (`{"error":{"code":"internal_error"}}`) [live-curl], while `GET /api/v1/insights/connectors` returns 200 with the full real registry [live-curl]. Root cause is **not** an insight-core defect:

- The overview read hits the durable Postgres metric store; the connector registry is static/in-memory and DB-free — hence the split.
- A read-only host probe with the container's exact creds (`appuser@application-postgres:9540/application_plane`) fails with: `FATAL: could not open file "global/pg_filenode.map": I/O error (SQLSTATE 58030)` [live]. **`application-postgres`'s data volume is corrupted** — the same storage-layer I/O fault as the containerd corruption. The port accepts TCP but every query FATALs.
- The container booted 2026-07-09 (Ping+migrations succeeded then, else `main.go` `log.Fatalf` would crash-loop; `restarts=1`, stable "Up 2 days") [inspect]; the volume faulted afterward, so the pool can no longer serve reads.

So the analytics pipeline is real and correctly wired, but the overview/scorecards read is degraded **in this environment only** until the Postgres volume is repaired/reinitialized. This is consistent with `application-postgres` showing `(unhealthy)` — though note the container's `(unhealthy)` label alone is unreliable this pass because the exec-based healthcheck fails fleet-wide.

## Entry Points

- Main: `apps/Application Plane/insight-core/cmd/server/main.go`
- Routes: `apps/Application Plane/insight-core/internal/http/server.go`
- Handlers: `internal/http/handlers.go`
- Service/domain: `internal/insights/{service,connectors,types,helpers,ownership}.go`
- Repositories: `internal/insights/{memory_repository,pg_repository}.go` (implement `Repository`)
- Consumers: `internal/consumers/{metric_subscriber,agent_subscriber,consumer}.go`
- Briefs: `internal/briefs/{scheduler,brief,client}.go`
- DB + migrations: `internal/database/{database,migrate}.go`, `internal/database/migrations/00{1,2}_*.sql`

## Exposed Surface

- `GET /health`, `GET /ready` — unauthenticated, `{status, service}`.
- `GET /api/v1/insights/connectors` — internal-key + org gated; static connector registry. **200 live** [live-curl].
- `GET /api/v1/insights/overview` — internal-key + org gated; per-org rollups/scorecards over a time window (`from`/`to`, `surface[]`/`surfaces=`). **500 live** (DB-volume corruption, see above) [live-curl].
- `POST /internal/insight-events` — internal-key gated; direct metric ingest (`IngestMetricEventInput`). Returns 202. (Not exercised live — writes.)

### Auth / boundary verification [live-curl]

- No key → `401 unauthorized` (constant-time compare; empty configured OR provided key both rejected).
- Key present, no `x-org-id`/`org_id` → `400 missing_org_id`.
- Bad surface → `422 validation_error` (validated before any DB access — proves input validation is upstream of the failing read).
- Body cap `1<<20`, `DisallowUnknownFields` on ingest.
- Org scope is mandatory on every read/ingest; brief delivery discovers orgs server-side from recorded data (`ListOrgIDsWithMetricsSince`), never from client input — IDOR-clean by design.

## Configuration & Wiring [inspect + source-only]

Running container env [inspect]:
- `DATABASE_URL=postgresql://appuser:application-postgres-secret@application-postgres:5432/application_plane` → durable PG mode active (schema `insight_core`, migration 002 namespaces the table away from the other cores sharing application-postgres).
- `NATS_URL=nats://velion-nats:4222` (+ token) → conversation/social metric subscriber active.
- `MODEL_PLANE_NATS_URL=nats://model-plane-nats-1:4222` → agents subscriber active; `main.go` provisions a bounded `MODEL_PLANE_RUN_EVENTS` stream (48h/64MB) so the durable consumer can bind to the Model Plane's otherwise stream-less core-NATS run events.
- `SOCIAL_CORE_URL=http://social-core:3162`, `INTEGRATION_CORE_URL=http://integration-api:3026`, `INTERNAL_API_KEY` = shared cross-plane key.
- **`NOTIFICATION_CORE_URL` is NOT set** (absent from the running env and from the compose block, lines 561-594) → the daily-brief scheduler is **disabled at runtime** (`main.go` requires `NOTIFICATION_CORE_URL != "" && DATABASE_URL != ""`). The delivery code is complete and unit-tested but inert here.

Compose caveats worth carrying forward: `container_name`-pinned, `replicas: 1`, `depends_on: application-postgres: service_healthy` (which now blocks clean restarts because the DB is unhealthy). The connector registry is process-local in both repos (even PG mode returns the static in-code registry), so it must never be scaled beyond one instance.

## Stub / Mock / Placeholder / Unused Audit

- `grep` for `todo|fixme|mock|stub|fake|placeholder|not-implemented|hardcode|change-me|dummy` across non-test `.go` → **zero hits** [source-only]. This service is unusually clean.
- Connector statuses `planned`/`disabled`/`requires_token_lease` are **honest registry guards**, not stubs: GA4 + Google Search Console carry real official upstream contract shapes (endpoints/scopes/dimensions) but are gated behind `integration-corev2` token leases (never store OAuth secrets inline — enforced by design); `campaign-core`/`seo_tool` are declared future slots. `connectorGaps()` surfaces these as explicit `connector_lag` with a named next-owner — the opposite of a hidden fake.
- Fail-open guards are honest optional-integration handling, not silent swallowing: `NewMetricSubscriber` with a nil social fetcher skips `metrics.snapshotted` (logged); nil NATS/DB config degrades to no-op/in-memory with a startup log line.
- Poison-message handling: undecodable JetStream messages are ack'd (not infinitely redelivered) and logged.

## Findings / Warnings

1. **[WARNING · live-curl/live]** `/api/v1/insights/overview` 500s for all orgs due to `application-postgres` data-volume corruption (`SQLSTATE 58030`, I/O error on `global/pg_filenode.map`). Infra fault, **not** an insight-core code defect (build + tests pass; connector-registry path unaffected). Blocks the SPA insights overview/scorecards and any brief assembly until the DB volume is repaired/reinitialized.
2. **[WARNING · inspect/source-only]** Daily-brief delivery leg is built + tested but **disabled**: `NOTIFICATION_CORE_URL` is unset in compose/env. Even if set, it also needs the (currently corrupt) durable store. Feature gap, not a break.
3. **[INFO · source-only]** Registry/reality drift: the `model-plane-agents` connector slot is labelled `status: planned` in `connectors.go`, yet the agent subscriber leg is actually wired and recording `surface=agents` metrics (`MODEL_PLANE_NATS_URL` is set). The registry **under-claims** (safe direction) but is stale — consider promoting it to `native` once agent metrics are confirmed flowing.
4. **[INFO · source-only]** Connector registry is process-local/in-memory in both repo implementations; correctly pinned to a single instance in compose. Documented, not a bug.

## Uncommitted WIP

`git status --porcelain` and `git diff --stat` for `apps/Application Plane/insight-core` are **empty** — the service directory has **no uncommitted changes** [source-only]. Last commit `9a9ac7fc` (2026-07-07 "provider-business-modules WIP baseline"); the running image was built the same day (`created 2026-07-07`) [inspect], so the live binary matches source (migration 002's schema-qualified queries are in effect — no stale-binary divergence for this service).

## Notes

`insight-core` is one of the healthier Application Plane cores: real event-sourced metrics from three genuine producers, scrupulous no-fabrication discipline, clean build/tests, mandatory org scope, and server-side (IDOR-clean) brief org discovery. Its only real problems this pass are **operational, not structural**: the shared Postgres volume is corrupted (breaking the overview read) and the brief-delivery URL is unwired. Fix the DB volume and set `NOTIFICATION_CORE_URL`, and the full analytics + briefs loop should light up without code changes.
