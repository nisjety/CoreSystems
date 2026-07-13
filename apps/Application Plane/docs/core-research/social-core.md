# social-core

> **2026-07-13 superseding update.** The current Postgres container is Docker-healthy; the July 11 corruption-only blocker below is stale. The full Go race suite passes. Worker-enforced HITL and existing Velion v3 metrics/catalog routes remain confirmed and must not be rediscovered as missing. A safe real social publication was intentionally not performed. Authenticated UI→gateway→service tenant-isolation, provider failure/idempotency, token lifecycle, and live HITL bypass evidence remain pending; the shared-key/forwarded-header authority model is a plane-wide blocker.

_Audit date: 2026-07-11. Evidence grades: **[live-curl]** host curl to :3162 · **[source-only]** read from disk · **[inspect]** `docker ps` / `docker inspect` / host TCP. Docker `exec`/`build`/`logs` are unusable this pass (containerd content store corrupted); DB/env facts read from compose + `.env` on disk._

## Current State

`social-core` (Go, container `social-core`, host `:3162`) owns the Application Plane social surface: social accounts, campaigns, posts, HITL approvals, scheduling, a durable publish-job queue with real per-provider publishers, daily provider-metric snapshots, and read-only Meta Commerce catalog reads. It reaches every provider **only** through integration-corev2 (connections + token lease + actions surface) — it holds no provider secrets itself.

At startup (`cmd/server/main.go`) it: loads config → connects to `application-postgres` → runs embedded SQL migrations → connects to NATS/JetStream (best-effort; degrades to no-event mode if down) → builds the integration-corev2 client (account source + token broker + action executor) → wires the HTTP publisher → starts the HTTP server, the publish worker, and the metrics worker.

**Live status [live-curl]:** `GET /health` = **200** (`{"status":"ok","service":"social-core"}`). Auth gate verified: `GET /api/v1/social/accounts` with no key = **401** `unauthorized`; with key but no org = **400** `missing_org_id`. **However, every authenticated DB-backed read (`/accounts`, `/posts`, `/campaigns`, `/metrics`) returns 500 `internal_error` (3/3 retries).** Root cause is environmental, not code — see the live-DB finding below.

## Entry Points

- Main / wiring: `apps/Application Plane/social-core/cmd/server/main.go`
- Routes + auth gate: `apps/Application Plane/social-core/internal/http/server.go`
- Handlers: `apps/Application Plane/social-core/internal/http/handlers.go`
- Service (business logic + HITL enforcement): `internal/social/service.go`
- Postgres repository: `internal/social/repository.go`
- Provider publishers: `internal/social/publisher.go`
- Metrics collection: `internal/social/metrics.go` / `internal/social/metrics_repository.go`
- Commerce catalog: `internal/social/catalog.go`
- integration-corev2 client: `internal/integration/client.go` (accounts + token) and `internal/integration/actions.go` (actions surface)
- Migrations: `internal/database/migrations/001_create_social_core.up.sql`, `002_create_social_provider_metrics.up.sql`

## Exposed Surface

All routes under `/api/v1/social` sit behind `requireInternalKey` (constant-time compare of `x-internal-api-key` vs `INTERNAL_API_KEY`) **and** require an org (`x-org-id`, or `org_id`/`orgId` query). Actor is `x-user-id` (falls back to `internal-service`). `/health` and `/ready` are open. Body cap 2 MiB.

- `GET /health`, `GET /ready`
- `GET /api/v1/social/accounts` · `POST /api/v1/social/accounts/sync`
- `GET /api/v1/social/campaigns` · `POST /api/v1/social/campaigns`
- `GET /api/v1/social/posts` · `POST /api/v1/social/posts`
- `GET /api/v1/social/approvals` · `POST /api/v1/social/approvals/:id/decide`
- `POST /api/v1/social/posts/:id/schedule`
- `POST /api/v1/social/posts/:id/publish-jobs` · `GET /api/v1/social/publish-jobs/:id` · `POST /api/v1/social/publish-jobs/drain`
- `POST /api/v1/social/metrics/snapshot` · `GET /api/v1/social/metrics`
- `GET /api/v1/social/catalogs` · `GET /api/v1/social/catalogs/:id/products`

Responses use typed envelopes: `{data, meta?}` on success, `{error:{code,message}}` on failure.

## Relationships

- **integration-corev2** (`INTEGRATION_CORE_URL`, default `http://integration-api:3026`): sole path to providers. `GET /api/v1/connections?category=social` (accounts), `POST /internal/connectors/token` (token lease per publish), `POST /api/v1/actions/execute` (metrics + catalog). Auth via `X-Internal-API-Key`. **[source-only]**
- **application-postgres** (DB `application_plane`, user `appuser`; shared within-plane with notification-core). No cross-plane DB crossing — social-core only touches its own `social_*` tables. **[source-only/inspect]**
- **NATS/JetStream** (`VELION_NATS_URL` → `velion-nats`, else `NATS_URL`): publishes lifecycle events on `velion.application.social.*` (account.synced, campaign.created, approval.requested/decided, post.created/scheduled, publish_job.queued/completed/failed/blocked, metrics.snapshotted). Consumers: insight-core reads real metric values via `GET /api/v1/social/metrics` after a `metrics.snapshotted` event (which carries only a count).
- **velion-gateway-rs** (`apps/Frontend Plane/velionv3/apps/gateway/src/domains/social.rs`, `SOCIAL_CORE_URL` default `http://social-core:3162`): the browser-facing proxy for the v3 social workspace. Forwards org server-side as `x-org-id`; reads degrade to `meta.source="unavailable"` and writes return 503 when social-core is down (no fabricated data). **[source-only]**

## Prior findings — re-verified

1. **"Publish path never checks ApprovalState — HITL decorative on live writes" → FIXED / refuted. [source-only]**
   `Service.ensurePublishApproved` (service.go:732) is enforced at **three** points: `SchedulePost` (217), `EnqueuePublish` (260), and — defense in depth — inside the worker's `processPublishJob` (312), which re-verifies at execution time and marks the job `blocked` if approval was revoked. It never trusts a client flag: `PGRepository.PostApprovalStatus` (repository.go:398) reads `approval_required` from `social_posts` and derives `approved` from an `EXISTS` over `social_approvals WHERE state='approved'` (the record only `DecideApproval` writes). Covered by unit tests `TestEnqueuePublishRequiresApproval`, `TestEnqueuePublishAllowsApprovedPost`, `TestSchedulePostRequiresApproval`, `TestProcessDuePublishJobsBlocksUnapprovedPost` — all green.

2. **"Social metrics + catalog have ZERO v3 gateway route (built but inert)" → FIXED. [source-only]**
   `social.rs` now routes `GET /api/v1/social/metrics` (line 52), `GET /api/v1/social/catalogs` (53), `GET /api/v1/social/catalogs/:id/products` (54–57). The in-file comment (75–82) explicitly documents closing this gap. Rows are mapped from real social-core payloads; the gateway never synthesizes a metric value (`core_metric_from_value` skips nameless rows rather than coercing to zero).

3. **Account path real via integration-corev2 → confirmed. [source-only]**
   `ListSocialAccounts` fetches live connections, filters to social providers, upserts into `social_accounts`, and emits `account.synced`. Publishing leases a fresh provider token per attempt and makes real provider HTTP calls (LinkedIn `/rest/posts`, X `/2/tweets`, Instagram media+publish with async-container polling, Facebook Page-token exchange then `/feed`|`/photos`, TikTok Direct Post init, Snapchat Public-Profile encrypted multipart). "Does any publish go out?" — yes, the machinery is real; actual delivery depends on integration-corev2 returning a valid token and the connected account carrying `social.post.write`. Otherwise the attempt is recorded `blocked`/`failed` with an honest message, never a fake success.

4. **WhatsApp/Messenger "replies silently never send" → not social-core's concern. [source-only]**
   The publisher explicitly refuses WhatsApp for organic publishing (publisher.go:130 "messaging surface — send via inbox workflows") and Meta Ads (133). That inbound/reply bug lived in conversation-core / integration; social-core correctly scopes itself to organic publishing.

5. **Key-gated + caller-supplied org headers → confirmed, and IDOR-guarded. [source-only]**
   Trust model is exactly as noted: a shared internal key plus a caller-supplied `x-org-id`/`x-user-id`. Every query is org-scoped in SQL. Catalog products additionally guard IDOR: `ListCatalogProducts` resolves `accountId` only against accounts social-core already listed for that org — a caller cannot pass a foreign connection id (catalog.go:79–108).

## Stub / mock / placeholder audit

- **No genuine runtime stubs. [source-only]** The only `grep` hits for `not implemented` are honest ownership-decision comments: catalog.go / handlers.go document that **Shopify** commerce is deliberately deferred to conversation-core (a support concern, not social), and that catalog **write** ops are intentionally unexposed. These are scoping decisions with rationale, not dead placeholders.
- **Honest config gates (not stubs):**
  - `SNAPCHAT_LIVE_PUBLISHING` defaults **off** — the Snapchat Public Profile API is allowlist-only and needs vendor creds; with the gate off, every Snapchat attempt returns a clear `blocked` message instead of pretending to post (publisher.go:435). Same hard test-mode pattern as the Bring shipping adapter. Not set in compose → off.
  - Metrics coverage is deliberately partial and self-documented (metrics.go:18): Meta family = real ads insights (impressions/reach/clicks/spend); LinkedIn = campaign inventory only (`campaign.status`, no reporting op exists in the actions surface yet); snapchat/tiktok/x = no actions op → skipped, never errored.
  - Google Ads campaign workflows stay disabled until `GOOGLE_ADS_DEVELOPER_TOKEN` is supplied (config.go:38–44).
- **Test doubles:** 82 `mock/fake/stub` references, all in `*_test.go` (in-memory repositories/publishers) — normal.

## Findings

- **[live-curl + inspect] CRITICAL (environmental, not a code defect): all authenticated DB reads return 500.** `/accounts`, `/posts`, `/campaigns`, `/metrics` each return 500 `internal_error` while `/health`=200. A fresh host-side pgx connection to the same DB (`application_plane` @ `localhost:9540`, user `appuser`) fails with `FATAL: could not open file "global/pg_filenode.map": I/O error (SQLSTATE 58030)`. The `application-postgres` container is "Up 2 days (unhealthy)" and its data volume has hard I/O errors — the containerd-corruption failure reaching the Postgres storage layer. social-core booted 2 days ago against a then-healthy DB (migrations gate on the DB at boot, and the service is up), so this is post-boot storage degradation, not schema drift or a social-core bug. Live social reads/writes are effectively down until the DB volume is restored. Re-test once the DB recovers.
- **[source-only] Minor: Graph API version drift, cosmetic.** `docker-compose.yml` pins `INSTAGRAM_GRAPH_API_BASE_URL`/`FACEBOOK_GRAPH_API_BASE_URL` to `graph.facebook.com/v23.0`; `config.go` code defaults are `v25.0`. Compose wins at runtime; no functional impact, but worth aligning.
- **[source-only] Note: metrics worker + Snapchat live-publish env not set in compose.** `SOCIAL_METRICS_WORKER_ENABLED` (code default true) and `SNAPCHAT_BUSINESS_API_BASE_URL`/`SNAPCHAT_LIVE_PUBLISHING` (default off) are absent from the social-core compose block, so they use code defaults — consistent with the honest-gate design.

## Build / verification

- **[source-only]** Toolchain `go1.26.2`. `go build ./...` = clean (exit 0). `go vet ./...` = clean. Unit tests `go test ./internal/social/` = **ok** (approval/publish/schedule/metrics/catalog logic covered).
- **Uncommitted WIP:** none. `git status --porcelain` for the service dir is empty — social-core is fully committed. (A throwaway `cmd/dbprobe` used to diagnose the DB I/O error during this audit was removed; tree confirmed clean afterward.)

## Notes

social-core is one of the healthier services in the plane on the source axis: real provider publishing through the correct authority boundary (integration-corev2 only), genuine server-side HITL that a client flag cannot bypass and that the worker re-checks at execution time, IDOR-guarded catalog reads, honest config gates instead of fake successes, and a now-wired browser path through the gateway. Both headline prior-audit findings (decorative HITL; unreachable metrics/catalog) are resolved. The only blocker today is the shared `application-postgres` storage I/O failure, which takes every DB-backed endpoint down until the volume is restored.
