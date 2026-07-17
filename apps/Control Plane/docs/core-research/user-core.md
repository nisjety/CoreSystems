# user-core Research Dive

Generated: 2026-06-07
Updated: 2026-07-15 (scoped runtime credential/static re-verification)

Scope: `apps/Control Plane/user-core`

## 2026-07-15 final secure-MVP addendum (current)

User gRPC no longer accepts a fleet-wide shared key. A required file-backed registry binds credential ID, principal, `user-core-grpc` audience, token, and exact full method paths; duplicates, placeholders, wildcard methods, wrong principals, retired entries, and cross-principal reuse fail closed. Auth→User and User→Auth clients use reciprocal single-credential files, allowing explicit old/new overlap without caller-asserted privilege. The new gRPC service-auth module measures 84.5% statement coverage.

The production overlay removes User's developer `env_file` and host-mounted Auth key, mounts the same Auth public-key secret used by Auth, and requires an explicit `AUTH_CORE_ISSUER`; the localhost default cannot enter a release render. User's gRPC server now requires a file-backed certificate/key pair with TLS 1.3 minimum; the real Auth/User integration passes and a plaintext channel is rejected. A root-only Compose handoff copies file-backed secrets into app-owned `0600` files before User drops to `appuser`. Existing signed gateway/session delegation, normalized email, verified-role/profile boundaries, durable multi-org GDPR fanout, lag/terminal visibility, and evidence-preserving requeue remain green. Full `go test ./...` and `go vet ./...` pass. Production credential injection/rotation remains operator-owned; no live profile, membership, or erasure row was mutated.

## 2026-07-15 durable GDPR fanout detail (superseded by final addendum above)

The erasure saga no longer relies on one audit org plus a core-NATS flush.
Migration 016 captures every active `user_org_memberships` organization in the
operation transaction before Auth or local cleanup, then dispatches one stable
child per org. A child is marked published only after an
`AQENCIA_CONTROLPLANE` PubAck with non-zero sequence; the parent completes only
when no unpublished child remains. A partial publish leaves the already-ACKed
child immutable and resumes only the missing children.

`/health` now includes durable audit/fanout pending, terminal, and oldest-lag
state and reports `degraded` without forcing a restart loop. Verified platform
admins can requeue 1-100 selected terminal audit/fanout IDs. Requeue preserves
payload/org snapshot, last error, and an incrementing requeue counter; published
and terminal GDPR audit evidence is not purged.

An isolated tmpfs PostgreSQL run passed pre-mutation two-org snapshot, partial
PubAck incompletion, restart/resume, terminal evidence, health, and bounded
requeue tests. Full serialized tests/vet and changed-package race tests pass;
the four changed GDPR ledger/saga files measure 80.8% (282/349 statements).
The shared publisher also rejects missing, wrong-stream, and zero-sequence
PubAcks. No User container or real tenant row was changed, so rollout of the new
shared stream/credential remains operational evidence rather than live proof.

## 2026-07-15 scoped-runtime addendum (current)

The verified-claim-only privilege and normalized-email fixes remain unchanged. User's release path uses a dedicated Control-bus principal and a dedicated shared-broker principal, sets token fallback to `0`, and has no runtime stream/consumer administration. A one-shot deployment provisioner owns topology; contract and scoped-ACL tests reject missing, placeholder, embedded, ambiguous, and over-privileged credentials. Full `go test ./...`, `go vet ./...`, and the earlier race pass remain green. No User container was recreated and no real profile was mutated in this final continuation; coordinated credential deployment is still an operational gate, not a production claim.

## 2026-07-14 Velion gateway addendum (historical deployment evidence)

The Velion gateway ingress strips caller-supplied user ID, organization ID, role, profile, and internal-key headers before any upstream call. User Core self-service continues to require the audience-, request-, body-, subject-, organization-, and verified-profile-bound HMAC delegation described below; browser `X-User-Role` is not an authority input. The User Core container is healthy. This continuation changed no User Core source and performed no profile mutation; it preserves the existing regression evidence while confirming the repaired SPA/gateway path does not reintroduce raw identity headers.

## 2026-07-11 production-readiness addendum (historical)

User Core is rebuilt, healthy, and no longer authorizes caller-supplied `X-User-Role`. Privileged service calls resolve a registered scoped principal; user/admin authority comes from verified claims, not headers. A second regression found that bearer callers could inject `X-User-Email`/name/avatar into `GetOrCreate` and reach the ID-reassignment path. Bearer profile attributes now come only from Auth Core's verified identity response.

Gateway and Session `users:*:self` access now requires a 30-second HMAC-SHA256 delegation envelope bound to principal, `user-core` audience, method, request URI, request-body digest, user/org subject, and verified email/name/avatar. Body or subject substitution, expiry, and unsigned self delegation are rejected. Go and Rust share a fixed cross-language signature vector. Live: a valid gateway service token plus `X-User-Id` without the signed envelope returns 403. Distinct scoped credentials remain live for gateway, Session, Org, and Auth; Data authorization scopes stay disabled until their org/resource delegation is equivalently bound.

A final review found three handler call sites still passed raw `X-User-Email`/name/avatar after middleware verification (`ensureCanonicalCurrentUser`, session context, and profile update). A new failing regression was added; all three now use only `user_email`/name/avatar from verified Gin context. The full User suite and vet pass. This last image was recreated, but Docker Desktop storage I/O errors then made Postgres/container state inconsistent and User unhealthy, so the final handler fix is **not live-accepted** yet.

Email normalization now occurs on the canonical repository lookup boundary, closing the prior mixed-case login/dedup gap. Migration 013 is applied. `go test ./...` and `go vet ./...` pass. Measured whole-service coverage is 3.8%; the changed authorization taxonomy module is 93.3%, while the HTTP package is 12.3%. Additional focused service-auth/handler integration coverage is still required for MVP.

## Snapshot

`user-core` is the Control Plane user-profile and settings authority. It is a Go service with a broad HTTP API, a real business gRPC surface, NATS event ingestion from auth/org flows, optional Redis caching, and optional Graph enrichment support.

Current evidence highlights:

- broad HTTP API for profile, onboarding, settings, provider links, calendar/navbar state, support requests, and internal helper endpoints
- real gRPC `UserService` and `DocumentAccessService`
- startup dependency on auth-service NATS authentication and Better Auth client setup
- shared NATS publishing exists, and as of this pass the document-ACL gRPC path is correctly wired to it (previously flagged as broken — see "Fixed since 2026-06-07" below)
- old implementation and README docs were stale and are now confirmed deleted
- container `user-service` is healthy live (`docker ps`, `Up 13 hours (healthy)` at time of check; image `Created: 2026-07-07T21:12:55Z`) and `/health` returns 200 live
- an uncommitted working-tree diff exists (main.go, repository.go, service.go + a new migration) that implements **onboarding-draft retention/expiry**, NOT the case-insensitive email lookup or event revision/tombstone protection the prior audit flagged — see "Uncommitted WIP" below

Non-generated/non-vendored `.go`/`.sql` file count from the current tree (excluding `proto/`): about `65`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate
  - DB migrations (`db.RunMigrations`, sorted `*.up.sql` scan of a real filesystem dir + `schema_migrations` tracking table — a working, self-contained runner, not golang-migrate)
  - optional pprof
  - NATS auth bootstrap with retries
  - Better Auth client init
  - optional Redis
  - local NATS and shared NATS init
  - optional Microsoft Graph enrichment wiring
  - **new (uncommitted):** a background goroutine that runs an hourly `PurgeExpiredOnboardingDrafts` sweep (plus an immediate on-start sweep), each capped at a 30s context timeout
- `internal/http/server.go`
  - primary business API
- `internal/grpc/server.go`
  - registers real `UserService` and `DocumentAccessService`
  - `NewDocumentAclHandler(aclRepo, s.publisher, s.sharedPublisher)` — `sharedPublisher` is now passed through (not `nil`)
- `internal/handlers/event_handler.go`
  - consumes auth and org membership events
  - no revision/timestamp/tombstone guard on any handler (see Findings)
- `internal/users/service.go`
  - main domain service

## API And Relationship Map

HTTP surface includes:

- `/health`
- `/api/v1/users/me`, `/current`, `/:id`, `/by-email/:email`
- `/api/v1/users/onboarding/complete`
- `/api/v1/users/me/onboarding-state`
- `/api/v1/me/session-context`
- `/api/v1/api-keys`
- `/api/v1/preferences`
- `/api/v1/settings/{appearance,language,privacy,notifications,security,accessibility,ai,storage}`
- `/api/v1/calendar/events`
- `/api/v1/calendar/notes`
- `/api/v1/support/requests`
- `/api/v1/providers`
- `/api/v1/users/:id/gdpr/erase`, `/:id/gdpr/anonymize`, `/:id/gdpr/export`
- `/api/v1/internal/memberships/ensure`
- `/api/v1/internal/users/enrich-from-provider`

gRPC surface:

- `UserService`
- `DocumentAccessService`

Current relationships:

- `user-core` -> `auth-core`
  - Better Auth client
  - NATS auth bootstrap
  - auth event ingestion
  - optional auth-core OAuth client for enrichment
- `user-core` -> Microsoft Graph
  - optional provider profile enrichment
- `user-core` -> `org-core`
  - membership and org-related event projection (`HandleOrganizationMemberAdded` / `HandleOrganizationMemberRemoved`)
- `user-core` -> shared cross-plane consumers
  - shared NATS publication for user/provider readiness paths and document-ACL changes

## Fixed Since 2026-06-07

- **Document ACL shared-bus wiring.** The 2026-06-07 doc flagged `internal/grpc/server.go` constructing `NewDocumentAclHandler(aclRepo, s.publisher, nil)`, meaning gRPC document-ACL grant/revoke never emitted shared-bus events. Current tree: `dAclHandler := handlers.NewDocumentAclHandler(aclRepo, s.publisher, s.sharedPublisher)` — `sharedPublisher` is real, and `internal/handlers/document_acl.go` calls `h.sharedPublisher.PublishDocumentAclChanged(...)` on both grant and revoke. This gap is closed (not part of today's uncommitted diff — already in the committed tree).
- **Admin-role check on `GET /api/v1/users/:id`.** The 2026-06-07 doc listed this as an open TODO. Current tree has a real gate: `getUserByID` calls `isAdminRequest(c)` and returns 403 if the caller's `user_role` context value isn't `admin`/`superadmin`. Live-verified below — but see the Findings section: the gate itself works, but the role value it trusts is attacker-influenceable under one auth path.
- **`IMPLEMENTATION.md` / `README.md` deletion.** Both files are confirmed absent from `apps/Control Plane/user-core/` on disk, matching `apps/STALE_DOC_DELETION_REGISTER.md` rows 33–34 (marked `delete`, already-deleted `yes`).

## Uncommitted WIP (working tree, not yet committed)

`git diff` was run against the current working tree for the three tracked files plus the two new untracked migration files:

- `apps/Control Plane/user-core/cmd/server/main.go` (+26)
- `apps/Control Plane/user-core/internal/users/repository.go` (+73/-, several rewritten queries)
- `apps/Control Plane/user-core/internal/users/service.go` (+4)
- `apps/Control Plane/user-core/migrations/013_onboarding_retention.up.sql` (new, untracked)
- `apps/Control Plane/user-core/migrations/013_onboarding_retention.down.sql` (new, untracked)

**What it actually does:** implements a 30-day onboarding-draft retention/expiry policy, not the two gaps the baseline briefing suggested it might close.

- `MarkOnboardingComplete` / `MarkOnboardingCompleteByID` now also null out `onboarding_step`/`onboarding_state`/`onboarding_expires_at` and stamp `onboarding_completed_at` on completion.
- `MarkOnboardingComplete`'s `WHERE` clause changed from `email = $1` to `WHERE LOWER(BTRIM(email)) = LOWER(BTRIM($1))` — the **only** case-insensitive email match added anywhere in this diff, and it's scoped to this one completion-by-email path.
- `GetOnboardingState` now opportunistically expires (nulls) a stale draft in the same query if `onboarding_complete = false AND onboarding_expires_at < NOW()`, while leaving the canonical user identity untouched.
- `UpsertOnboardingState` now stamps `onboarding_started_at` (first write) and `onboarding_expires_at` (`NOW() + 30 days`, only while incomplete), and is now guarded with `WHERE id = $1 AND onboarding_complete = false` (a completed user's draft can no longer be silently re-opened by a stray upsert).
- New `Repository.PurgeExpiredOnboardingDrafts` / `Service.PurgeExpiredOnboardingDrafts` bulk-clear all expired incomplete drafts fleet-wide.
- `main.go` wires an hourly sweep goroutine (immediate first run + `time.NewTicker(time.Hour)`, context-cancelled on shutdown) that calls the new purge method and logs the cleared count.
- Migration `013_onboarding_retention` adds the four new nullable timestamp columns plus a partial index (`idx_users_incomplete_onboarding_expiry`) and backfills sane defaults for existing rows (30-day expiry from `created_at` for incomplete drafts; `onboarding_completed_at` backfilled for already-complete users). The migration reads cleanly and is consistent with the Go code that depends on it.

**Assessment against the audit gaps this WIP was suspected to address:**

- **Case-insensitive User Core email lookup (prior audit gap): NOT closed.** The general-purpose `Repository.GetByEmail` — used by `Service.GetUserByEmail`, `Service.UpdateUser`'s email-uniqueness check, `Service.CreateUser`'s dedup path, and all of `event_handler.go`'s email-keyed event handlers — is untouched by this diff and remains `WHERE email = $1` (exact match, no `LOWER()`/`BTRIM()`). Confirmed both by reading `repository.go:144-149` and by a live curl reproduction (see Live Checks below): the same seeded user resolves on `local@velion.dev` but 404s on `Local@Velion.Dev` and `LOCAL@VELION.DEV`. The diff only added case-insensitivity to the narrow `MarkOnboardingComplete` match, which creates an inconsistency: onboarding completion now tolerates case drift but every other email-keyed lookup (dedup, uniqueness, event correlation) does not. A user whose auth-core session carries a differently-cased email than the row stored in `user_service.users` will still fail `GetUserByEmail`/`UpdateUser` dedup checks and can end up with duplicate rows.
- **Revision/tombstone protection across delayed events (prior audit gap): NOT touched at all.** `event_handler.go` is not part of this diff. `HandleOrganizationMemberAdded`/`HandleOrganizationMemberRemoved` (and every other handler) apply whatever NATS message arrives with no sequence number, timestamp comparison, or tombstone check — an out-of-order `member_added` arriving after a `member_removed` for the same (user, org) pair will silently resurrect a membership that org-core already revoked. This is unchanged from the 2026-06-07 baseline and is still open.
- **What it does close:** a real, previously-undocumented gap — onboarding drafts had no TTL/retention path at all (abandoned partial signups would sit in `onboarding_state` forever). This diff is a legitimate, self-contained feature addition, not a mock/stub, and the migration + Go code are mutually consistent.

**Deployment status:** the running `user-service` container was created `2026-07-07T21:12:55Z`, predating this diff, so the live binary does **not** include any of this onboarding-retention code yet. `migrations/013_onboarding_retention.up.sql` has **not** been applied to the live `user_service` database — confirmed via `\d users` (no `onboarding_started_at`/`onboarding_expires_at`/`onboarding_state_updated_at`/`onboarding_completed_at` columns present) and `schema_migrations` (latest applied version is `012_resource_grants`). This is expected for uncommitted/unbuilt code, not a bug: the migration runner is a simple sorted-directory scan, so `013` will apply cleanly after `012` on the next rebuild + restart. Flagging only so it isn't mistaken for already-live behavior.

## Live Checks (2026-07-10)

- `docker ps` — `user-service` container: `Up 13 hours (healthy)`, ports `3012`, `6060` (pprof/metrics), `50012` (gRPC) all mapped.
- `GET http://localhost:3012/health` → `200 {"service":"user-service","status":"healthy",...}`.
- `GET /api/v1/users/by-email/...` and `/api/v1/users/:id` with no credentials → `401 {"error":"unauthorized"}` (auth middleware correctly rejects unauthenticated calls).
- Using the container's own `X-Internal-Api-Key` (fleet-shared internal key, read from the running container's env):
  - `GET /api/v1/users/by-email/local@velion.dev` → `200`, returns the seeded dev user (`iivCjw2n4ZNjvBtShmz4Bugo0qF3eWod`).
  - `GET /api/v1/users/by-email/Local@Velion.Dev` → `404 {"error":"User not found"}`.
  - `GET /api/v1/users/by-email/LOCAL@VELION.DEV` → `404 {"error":"User not found"}`.
  - This directly reproduces the still-open case-insensitive-lookup gap for the same live email/user, live, today.
  - `GET /api/v1/users/<id>` with no `X-User-Role` header → `403 {"error":"admin role required"}`.
  - `GET /api/v1/users/<id>` with `X-User-Role: member` → `403` (correctly rejected).
  - `GET /api/v1/users/<id>` with `X-User-Role: admin` (self-supplied, no other credential change) → `200`, full user record returned.

## Findings

**HIGH — self-declared admin role is trusted under the internal-key auth path.** `internal/http/server.go`'s auth middleware has two paths: (1) `X-Internal-Api-Key` matches a configured fleet-shared key, or (2) a `Bearer` token is resolved against auth-service. Under path (1) — the one every other Control Plane service and any internal caller uses — the middleware reads `X-User-Role` / `X-Auth-Role` / `X-User-Roles` straight off the incoming request header with zero independent verification and puts it directly into Gin context as `user_role`. `isAdminRequest` (the gate now protecting `GET /api/v1/users/:id` and, by extension, any handler using the same helper) trusts that context value verbatim. Live-reproduced above: supplying `X-Internal-Api-Key: <shared key>` plus `X-User-Role: admin` from a bare `curl` call — no bearer token, no upstream auth-core session — returns a full user record that a real admin-gated endpoint should not release to an arbitrary caller. Per project memory this internal key is shared fleet-wide, so this is the same class of problem as the already-flagged "X-Org-ID header trust needs a real cross-plane validation contract" finding in `plane-audit-2026-07-02.md` — it just surfaces here as `X-User-Role` instead of `X-Org-ID`. The admin-check code itself is real and not a stub; the gap is that nothing upstream of `user-core` is contractually prevented from (or verified against) forging that header when talking over the internal-key channel.

**MEDIUM — case-insensitive email lookup gap remains open on all primary paths, live-confirmed.** See "Uncommitted WIP" above. `Repository.GetByEmail` (case-sensitive) backs `Service.GetUserByEmail`, `Service.UpdateUser`'s "already taken" dedup check, `Service.CreateUser`'s existing-email path, and every email-keyed NATS event handler in `event_handler.go`. Only `MarkOnboardingComplete` got a case-insensitive match in the current WIP, creating inconsistent behavior across the service rather than closing the gap.

**MEDIUM — no revision/tombstone protection on NATS event handlers, unchanged from baseline.** `HandleOrganizationMemberAdded` / `HandleOrganizationMemberRemoved` (and all other handlers in `event_handler.go`) apply events in arrival order with no timestamp/sequence guard. A delayed `member_added` arriving after a `member_removed` for the same user/org will silently re-create a membership org-core has already revoked. This is exactly the kind of gap the org-membership-focused P0/P1 work elsewhere in this uncommitted change set (auth-core, org-core) is trying to close, but no equivalent protection has landed in `user-core`'s own event consumer.

**LOW — onboarding-retention migration not yet applied to the live database.** Not a bug (see Deployment status above), but flagged so a future pass doesn't assume the retention sweep is already active. `schema_migrations` on the live `user_service` DB tops out at `012_resource_grants`; the new `013_onboarding_retention` migration and its dependent Go code are both still uncommitted and unbuilt.

**LOW — duplicated helper logic (carried over from 2026-06-07, still present).** `internal/internalkey/assert.go` is a canonical copy duplicated across four Go Control Plane services. Placeholder-avatar/placeholder-name detection logic is duplicated between `internal/users/service.go` (`isReplaceableAvatar`) and `internal/http/handlers.go` (`isPlaceholderAvatar`, `isPlaceholderName`) — same intent, two implementations to keep in sync.

## Stubs, Placeholders, And TODOs

Active TODOs (`internal/users/service.go`, unchanged from 2026-06-07):

- line ~503: TODO to log block reason in activities
- line ~536: TODO to log suspension reason in activities
- line ~537: TODO to handle suspension expiry

Resolved since 2026-06-07:

- The `internal/http/handlers.go` TODO to add an admin-role check on `GET /api/v1/users/:id` is gone; `isAdminRequest` now gates the route (see Findings for the caveat on how that role value is sourced).

Intentional unimplemented gRPC areas (expected, not findings):

- `CreateSession` intentionally not implemented — sessions are owned by auth-core/session-core (explicit comment in `internal/grpc/handlers.go`)
- device-management gRPC methods (`RegisterDevice`, `ListDevices`, `UpdateDevice`, `DeactivateDevice`) remain unimplemented, explicitly commented as such
- generated proto `Unimplemented` stubs in `proto/user/v1/user_grpc.pb.go` are expected scaffolding, not runtime findings

Development placeholder behavior (by design, not a gap):

- placeholder emails such as `@placeholder.local` used for auto-provisioned rows pending real identity resolution
- placeholder avatar/name detection and replacement logic (see duplication finding above)

## API Design And Performance Notes

API design:

- The HTTP API is broad and practical, but the settings surface is highly fragmented into many small endpoints.
- The current design favors explicitness over batching. That is acceptable for internal APIs, but it increases client round-trips.
- `GET /api/v1/users/:id` is now authorization-gated in code, but see the HIGH finding above on the trust boundary of the role value itself.

Performance and operational notes:

- auth bootstrap retries can delay startup materially when auth/NATS are degraded
- optional Redis degradation is graceful
- pprof support is useful and already guarded by env, live-confirmed exposed on `:6060`
- Graph enrichment is correctly optional rather than hard-failing the core
- the new (uncommitted) onboarding-draft purge sweep runs hourly with a bounded 30s per-run context timeout and is cleanly cancelled on shutdown via the service's root context — reasonable operational shape, no runaway-loop or unbounded-query risk apparent in the query itself (bulk `UPDATE ... WHERE onboarding_complete = false AND onboarding_expires_at < NOW()`, backed by the new partial index)

## Current Doc Cleanup Read

Delete-ready: none remaining — `IMPLEMENTATION.md` and `README.md` are both confirmed already deleted (see `apps/STALE_DOC_DELETION_REGISTER.md` rows 33-34, both marked already-deleted `yes`).

Keep: no other core-local docs require action in the current tree.

## Bottom Line

`user-core` is a real, production-shaped service with broad HTTP and real gRPC business behavior, live-healthy today. Since the 2026-06-07 pass, the document-ACL shared-bus gap closed and an admin-role gate landed on the `:id` route — but that gate inherits a header-trust weakness shared with the rest of the fleet's internal-key auth path (self-declared `X-User-Role` is accepted at face value). The uncommitted working-tree diff is a legitimate, well-built onboarding-draft-retention feature (migration + code are consistent, not yet deployed) — but it is not the fix for either of the two audit gaps it might be mistaken for: case-insensitive email lookup and delayed-event revision/tombstone protection are both still open, and the case-insensitive gap is now live-reproducible end-to-end against the running service. The three original service-level TODOs (block/suspension activity logging, suspension expiry) also remain untouched.


## 2026-07-17 optimization-program reconciliation

The per-request Auth Core `get-session` bearer resolution (and the double call on `/users/me`) is now cached on Dragonfly (`resolveIdentityCached`, SHA-256 token key, success-only, 30s TTL, nil/error-safe passthrough), commit `f6fa26a4`.
