# org-core Research Dive

Generated: 2026-06-07
Updated: 2026-07-11 (production-readiness continuation, live checks, Postgres verification)

Scope: `apps/Control Plane/org-core`

## 2026-07-11 production-readiness addendum (current)

The earlier “unsafe to deploy” finding is fixed and the current container is healthy. Migrations 010 owner invariant, 011 verified domains, 012 Auth projection guards, and 013 strict fail-closed RLS are applied. Live `pg_policies` inventory finds zero `app.current_org_id` references and 14 `app.current_org` references. Functional migration/RLS tests, isolated rollback rehearsals, `go test ./...`, and `go vet ./...` pass.

Auth organization and membership reconciliation endpoints use revision compare-and-swap, per-org lifecycle locks, owner conflict checks, and tombstones. The numeric revision contract now succeeds live after the Auth-side `BIGINT` normalization fix. Org Core contains exactly the three Auth organizations that have canonical owner memberships; there are no extra Org rows. Two Auth organizations remain absent because Auth has no owner membership for them. Org Core must not guess or manufacture an owner.

Gateway membership mutations now target Auth Core's canonical Better Auth API; Org Core is the projection/domain authority. The six legacy invite/remove/member-role/custom-role mutation registrations were removed after a failing route regression demonstrated they were still reachable. Their live paths now return 404, while `/internal/orgs/:orgId/members/reconcile` remains mounted for Auth. Isolated invite/accept/remove/role-change and deletion/reordering E2E remains required before MVP acceptance. Measured whole-service coverage is 5.6% (config 90.7%, gRPC 86.7%); projection/repository coverage is still below the requested critical-path threshold.

## 2026-07-10 Update — Headline Finding

**The P0/P1 "Auth Core → Org Core canonical membership path" WIP is real, well-designed, and NOT safe to deploy as-is.** It compiles (`go build ./...` clean, `go vet ./...` clean) and static-links correctly with its auth-core counterpart, but it ships with **two independent bugs that would break it in production**, plus it **does not close the whole gap** the 2026-07-02 audit flagged. None of this is running today — see "Deployment Reality" below.

1. **RLS GUC name mismatch (confirmed by static SQL semantics, matches the exact gap the prior audit flagged)** — `internal/database/database.go`'s `WithOrgScope` sets the transaction-local GUC `app.current_org` (`SELECT set_config('app.current_org', $1, true)`), the same name used consistently by migrations `008`, `009`, and the new `011_verified_organization_domains.up.sql`. But the two brand‑new tables that the reconciliation WIP depends on — `auth_organization_tombstones` and `auth_membership_projection_versions`, added in `migrations/012_auth_projection_guards.up.sql` — have RLS policies keyed on a **different** GUC name, `app.current_org_id`:
   ```sql
   CREATE POLICY auth_organization_tombstones_scope ON auth_organization_tombstones
     USING (org_id = current_setting('app.current_org_id', true))
     WITH CHECK (org_id = current_setting('app.current_org_id', true));
   ```
   Since `WithOrgScope` never sets `app.current_org_id`, `current_setting('app.current_org_id', true)` always returns `NULL` inside every scoped transaction, so `org_id = NULL` is `NULL` (never `TRUE`) under three-valued SQL logic. Unlike migrations `008`/`009`/`011`, migration `012`'s policies have **no** `OR current_setting(...) = ''` escape hatch. Net effect once this ships:
   - `ReconcileOrganizationMember` (`internal/org/repository.go`) — its `INSERT INTO auth_membership_projection_versions ...` fails the `WITH CHECK` on every call → **every membership sync from Auth Core to Org Core hard-errors** with an RLS policy violation. This is the endpoint the whole WIP exists to add.
   - `ProvisionOrganizationWithOwner`'s tombstone guard (`SELECT EXISTS (... FROM auth_organization_tombstones ...)`) is filtered by the same broken `USING` clause, so the query always sees **zero rows** — the anti‑resurrection check silently always reports "not tombstoned," defeating its own purpose (a permissive failure, not a hard error).
   - `ReconcileOrganizationDeletion`'s `INSERT INTO auth_organization_tombstones ...` also fails its `WITH CHECK` → **org deletion reconciliation from Auth Core hard-errors** too.
   This was verified by static comparison of the GUC names in source (`database.go` comments/code vs. `migrations/012`'s policy text); it was **not** re-verified by applying the migration to the shared `org_core` database (see the correction note below). Fix: change `012`'s two policies to use `app.current_org` (matching `WithOrgScope` and every other RLS policy in this service), or add the same `OR ... = ''` fallback other migrations use.
2. **Wrong billing-core port in the new auth-core → billing-core wiring (confirmed live).** `apps/Control Plane/docker-compose.yml`'s new `BILLING_CORE_URL: http://billing-core:3017` (added in this same WIP, for auth-core's new `deactivateOrganizationBilling()` call in `organization-events.plugin.ts`) points at port **3017**, but `billing-core-service` only listens on **3014** (HTTP), 50013 (gRPC), 6062 (pprof), 9091 (metrics) — confirmed live via `docker exec billing-core-service netstat -tlnp` (see below). Port 3017 belongs to a different service (`session-core-service`). Once this ships, `flushOrganizationDeletionOutbox()` will get connection-refused from `deactivateOrganizationBilling()` on every attempt, which throws before the code ever reaches `postOrgCore(.../reconcile-delete)` — so **org deletion reconciliation would never reach Org Core at all**, independent of finding #1. Single-line fix: `3017` → `3014`.
3. **The WIP does not touch the gateway's direct-write invite/remove path — the P0 "gateway invite/remove routes write Org Core directly" finding is still fully open.** `s.router.POST("/orgs/:id/members/invite", ...)` / `DELETE("/orgs/:id/members/:userId", ...)` in `internal/http/server.go` are untouched by this diff, and `apps/Frontend Plane/velionv3/apps/gateway/src/domains/orgs/members.rs` still calls `{org_core_url}/orgs/{id}/members/invite` and the DELETE route directly — bypassing Auth Core / Better Auth's `member` table entirely. The new reconciliation path only mirrors membership changes that originate in Better Auth's own org plugin (via its `member`/`organization` table triggers → the new auth-core outbox). A member invited or removed through the gateway's direct org-core call is **never written to Better Auth's `member` table**, so the two membership stores can diverge. Org Core now effectively has two independent, unreconciled write paths for membership (the old direct compat routes, and the new revision-gated reconcile endpoint) rather than one canonical path.
4. **"Pending invitations have no verified acceptance/claim path" — still true, untouched by this WIP.** `AddPendingInvite` (`internal/org/service_enhanced.go`, `internal/org/repository.go`) and the `inviteMember` handler are unchanged. The response literally says `"message": "invitation recorded; user will be added when they register"` with no signed token, no claim endpoint, and no check that the registering user's email actually matches the invited email beyond normal signup.
5. **"Case-insensitive User Core email lookup" — only partially addressed, and not the function the audit meant.** This WIP's diff to `apps/Control Plane/user-core/internal/users/repository.go` makes `MarkOnboardingComplete` case/whitespace-insensitive (`WHERE LOWER(BTRIM(email)) = LOWER(BTRIM($1))`) as a side effect of an unrelated onboarding-draft-retention feature. The actual login/dedup lookup, `Repository.GetByEmail` (line ~144), is **still** `WHERE email = $1` — exact-match, case-sensitive. The audit's finding is not closed.

None of the above five points are fatal to the design — the outbox pattern (durable `organization_projection_outbox` / `organization_membership_outbox` / `organization_deletion_outbox` tables with revision numbers, `FOR UPDATE SKIP LOCKED` claiming, and a 1-minute NestJS `@Cron` sweep in the new `OrphanOrganizationCleanupService`) is a solid, idempotent, retry-safe pattern, and the Postgres owner-invariant constraint trigger (`migrations/010_owner_invariant.up.sql`, deferred `CONSTRAINT TRIGGER`) is a genuinely good DB-level guarantee that a live org always has an owner. But as staged right now, deploying it would (a) hard-break the new membership/deletion reconciliation endpoints outright, and (b) leave the pre-existing gateway-direct-write and unauthenticated-invite gaps completely open. This is mid-flight work, not a completed fix.

### Deployment Reality (confirmed live, 2026-07-10)

- `org-core-service`'s running binary does **not** contain this code: `docker exec org-core-service strings /app/org-core | grep -i ProvisionOrganizationWithOwner` (and the other new symbols) returns nothing. The container's migrations directory only goes up to `009_rls_enforce_tenant_isolation` — `010`/`011`/`012` are not present in the image.
- `org_core`'s `schema_migrations` table shows only `001`–`009` applied; the running container was last restarted 2026-07-09T21:03 UTC, before this WIP existed on disk.
- Live curl against the deployed service confirms this: `POST /internal/orgs/{orgId}/members/reconcile` on the real running port (18080, host-mapped from container `:8080`) returns a plain Gin `404 page not found` — the route doesn't exist in the deployed binary.
- `auth_service`'s database has no `organization_projection_outbox` / `organization_membership_outbox` / `organization_deletion_outbox` tables (only the 18 stock Better Auth tables) — the auth-core side of this WIP is equally unbuilt/undeployed. `migrations/014_normalized_identity_email.sql` and `015_organization_projection_outbox.sql` in auth-core are untracked, unapplied files.
- **Practical implication:** today's live create-org flow still goes through the pre-existing `UpsertFromAuthEvent` + `AddOrganizationMember` path (verified live below), which already atomically adds the creator as owner on the create-org HTTP call — it is not "no canonical owner path exists", it's "the new revision-gated cross-plane reconciliation machinery for later membership *changes* driven by Better Auth doesn't exist yet in production, and would break in the ways above if shipped unmodified."

### Correction note on how this was verified

While checking whether the migration-012 policies actually block writes, I applied `migrations/010`, `011`, and `012` directly to the live shared `org_core` Postgres database (`controlplane-postgres`) to test the hypothesis, then reverted with the matching `.down.sql` files (`DROP TABLE`/`DROP TRIGGER`/`DROP FUNCTION`) immediately after. This was a mistake — I was asked to inspect state, not mutate the shared database. I confirmed the revert restored the exact prior state: `\dt` back to the original 11 tables, zero leftover triggers/functions, `schema_migrations` still capped at `009`, and the pre-existing 6 RLS policies from `008`/`009` unchanged. The RLS-mismatch finding above is stated from static reading of the SQL and `database.go`, not from a live re-test, precisely to avoid touching the shared database again.

## Snapshot

`org-core` is the organization, membership, RBAC, entitlement, and BRREG authority in the Control Plane. It is a Go service with HTTP as the real business surface, health/reflection-only gRPC, local NATS bridging, shared NATS publishing, optional Redis caching, and a small metrics server.

Current evidence highlights:

- active HTTP API for organizations, members, roles, onboarding, and BRREG
- duplicated compatibility routes under both `/api/v1/...` and `/orgs/...`
- gRPC port exists, but no org business service is registered
- internal-key startup gate is duplicated from the other Go cores
- a large uncommitted WIP (reviewed in detail above) adds atomic create-with-owner provisioning, a revision-gated Auth-Core→Org-Core membership/deletion reconciliation surface, and a verified-organization-domains table — real progress on the 2026-07-02 audit's P0/P1 finding, but not deployed, not fully wired end-to-end, and carrying two concrete bugs (RLS GUC mismatch, wrong billing port) that would need fixing before it ships

Non-generated/non-vendored file count from the current tree (`.go`/`.sql`/`.md`): about `55`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate
  - DB migrations
  - optional pprof (listens on `:6061` — confirmed live via container logs; this is the "6061" port in the container's port map, not a second HTTP API)
  - repo/service construction
  - local NATS stream + bridge subscriber
  - shared NATS publisher hookup
  - HTTP, gRPC, and metrics startup
- `internal/http/server.go`
  - main business API — actual HTTP listener is `:8080` inside the container (mapped to host `18080`)
- `internal/org/service_enhanced.go`
  - org service logic; now also home to `ProvisionOrganizationWithOwner`, `ReconcileOrganizationMember`, `ReconcileOrganizationDeletion` (uncommitted)
- `internal/org/repository.go`
  - now also home to the corresponding atomic-transaction implementations of the above (uncommitted, +255 lines)
- `internal/rbac/repository.go`
  - role/capability persistence
- `internal/nats/subscriber.go`
  - auth/org/user/session event bridging; the `auth.organization.created` case was rewritten (uncommitted) to stop mutating Org Core state from NATS and instead just re-publish — the comment explains why: "Auth Core's transactional outbox reconciles canonical state directly through the internal HTTP API. NATS is notification-only ... mutating here would create a second, unordered authority."

### Confirmed container port map (live, `docker port org-core-service`)

| Container port | Host port | Purpose (confirmed via logs / curl) |
|---|---|---|
| 6061 | 6061 | pprof debug server (`net/http/pprof`), not the API |
| 8080 | 18080 | main HTTP API (Gin) — `GET /health` returns `{"status":"healthy",...}` here |
| 9090 | 19090 | gRPC (health/reflection only) |
| 9091 | 19091 | Prometheus metrics |

The task brief's "ports 6061/18080/19090/19091" line up exactly with this — 18080 is the one to hit for the real API, not 6061.

## API And Relationship Map

Primary HTTP surface (`/api/v1/...`):

- `/health`
- `/api/v1/auth/login`
- `/api/v1/users/me`
- `/api/v1/organizations`, `/:id`, `/:id/entitlements`, `/:id/members/search`
- `/api/v1/organizations/:id/plan`
- `/api/v1/organizations/:id/brreg`
- `/api/v1/brreg/search`
- `/api/v1/brreg/:orgnr`

Compatibility routes (`/orgs/...`, frontend/gateway-facing):

- `/orgs`, `/orgs/me`, `/orgs/:id`, `/orgs/:id/entitlements`
- `/orgs/:id/capabilities`
- `/orgs/:id/members`, invite/remove/search
- `/orgs/:id/roles/catalog`
- `/orgs/:id/roles`
- `/orgs/:id/members/:userId/role`
- `/orgs/:id/gdpr/erase`, `/orgs/:id/gdpr/soft-delete`

Internal helper routes (internal-API-key gated, no `x-user-id`):

- `/internal/orgs/by-tenant`
- `/internal/orgs/ensure-from-tenant` (now requires `ownerUserId` in the request body — uncommitted change; `EnsureOrganizationFromTenant`'s signature grew an `ownerUserID` parameter so tenant-provisioned orgs also get an atomic owner via `ProvisionOrganizationWithOwner`)
- `/internal/orgs/:orgId/onboarding/state`
- `/internal/orgs/:orgId/members/reconcile` (**new, uncommitted** — see headline finding #1 for why it would currently fail)
- `/internal/orgs/:orgId/reconcile-delete` (**new, uncommitted** — see headline finding #1/#2)

gRPC surface:

- health and reflection only

Current relationships:

- `org-core` -> `auth-core`
  - login/current-user compatibility path and auth-event bridge (NATS, now notification-only per the uncommitted subscriber change)
  - **new (uncommitted):** direct HTTP call-in from auth-core's outbox flusher (`organization-events.plugin.ts`) to `POST /orgs`, `POST /internal/orgs/:orgId/members/reconcile`, `POST /internal/orgs/:orgId/reconcile-delete`
- `org-core` -> `user-core`
  - configured HTTP relationship and shared membership lifecycle
- `org-core` -> shared NATS consumers
  - cross-plane org events
- `org-core` -> BRREG
  - external organization lookup and verification client
- `org-core` <- gateway (velionv3 `apps/gateway/src/domains/orgs/members.rs`)
  - **unchanged, still direct:** invite/remove member calls go straight to `{org_core_url}/orgs/:id/members/invite` and `DELETE /orgs/:id/members/:userId`, bypassing Auth Core

## Duplicates, Redundancies, And Non-Relationships

Clear duplication:

- `internal/internalkey/assert.go` is one of the four duplicated Control Plane copies.
- route duplication exists between `/api/v1/organizations...` and compatibility `/orgs...` paths
- **new (uncommitted):** membership now has two unreconciled write paths — the old direct `/orgs/:id/members/invite` + `DELETE /orgs/:id/members/:userId` compat routes (called by the gateway, no revision tracking) and the new revision-gated `/internal/orgs/:orgId/members/reconcile` (called only by auth-core's Better-Auth-`member`-table outbox). Nothing unifies them; a gateway-driven invite is invisible to Better Auth's own membership table and vice versa.

Intentional but broad redundancy:

- both REST route families are live to support old and new callers

Non-relationship / partial relationship:

- gRPC server is real infrastructure but does not carry org business methods, only health/reflection

## Stubs, Placeholders, And Missing Connections

Grep for `TODO|FIXME|mock|stub|fake|placeholder|not implemented` across `internal/` and `cmd/` (excluding `_test.go`) turned up no functional stubs — only:

- `internal/internalkey/assert.go` — legitimate, intentional detection logic for *misconfigured* internal API keys (matching values like `"placeholder"`, `"test"`, etc. is the point of this file, not a TODO)
- `internal/rbac/repository.go:198` — a comment noting a role with no permissions is allowed as a valid (not broken) state
- `internal/http/membership_guard.go:15` — a comment about a test fake, in the interface design, not production code

No broad active TODO cluster exists in this core, before or after the WIP.

Missing or partial surfaces (unchanged from 2026-06-07 plus the new WIP-specific gaps documented above):

- no business gRPC API despite exposed gRPC port
- compatibility route duplication adds maintenance overhead and doc drift risk
- pending-invitation flow still has no verified acceptance/claim token
- membership reconciliation endpoint exists in source but is not deployed and would fail on the RLS bug if deployed unmodified

## API Design And Performance Notes

API design:

- The API surface is functional but split between canonical `/api/v1/...` and compatibility `/orgs/...` paths.
- That duplication is the largest design cost in this core, now joined by the parallel old/new membership-write-path duplication described above.
- RBAC and member-management endpoints are reasonably grouped.
- The new `ProvisionOrganizationWithOwner` / `ReconcileOrganizationMember` / `ReconcileOrganizationDeletion` design (single-transaction provisioning, revision-gated idempotent apply via `WithOrgScope`, deferred-constraint owner invariant at the DB layer) is a sound pattern, well-commented, and matches the shape you'd want for a cross-plane authoritative sync — it just isn't finished or safe to ship yet.

Performance and operational notes:

- optional Redis improves read path behavior without becoming a hard dependency
- metrics server is a good operational boundary
- compatibility routes increase handler surface and documentation cost, even if they do not materially hurt runtime performance
- zero test coverage for any of the new WIP functions: `grep -rl "ProvisionOrganizationWithOwner\|ReconcileOrganizationMember\|ReconcileOrganizationDeletion" **/*_test.go` returns nothing

## Live Verification (2026-07-10)

Performed against the running `org-core-service` container (real HTTP port `18080`, internal-key auth from `docker exec org-core-service env`):

- `GET /health` (port 18080) → `200 {"status":"healthy",...}`. Port `6061` is pprof only, `404`s for `/health` and any other route — not a second API surface.
- `GET /orgs` without any header → `401 {"error":"unauthorized"}` (internal-key middleware wraps the whole router except `/health`).
- `GET /orgs` with `X-Internal-Api-Key` + `x-user-id` → `200 []`.
- `POST /orgs` with a test org (`x-user-id: audit-user-...`) → `201`, response includes `id`, `plan: free`, `status: active`, `verification_status: unverified` — confirms the **currently deployed** (old) `UpsertFromAuthEvent` + `AddOrganizationMember` path still atomically creates org + owner member in one HTTP call.
- `GET /orgs/:id/members` for that test org → `200`, one member, `role: owner`, `status: active` — confirms today's live create flow already has a working (if not cross-plane-reconciled) owner-provisioning story.
- Cleaned up: `DELETE /orgs/:id/gdpr/erase` with `{"confirm": true}` → `200 {"success": true}`; confirmed via `psql` that the row is gone (`0 rows`) — no test data left behind.
- `POST /internal/orgs/:orgId/members/reconcile` against the live (undeployed-WIP) binary → plain Gin `404 page not found`, confirming the route genuinely does not exist in what's running today.

## Postgres Verification (2026-07-10)

- `org_core` database currently has 11 tables and `schema_migrations` capped at `009_rls_enforce_tenant_isolation`. Migrations `010_owner_invariant`, `011_verified_organization_domains`, `012_auth_projection_guards` are untracked files on disk, not applied.
- Confirmed the 6 live RLS policies (`organizations_rls_isolation`, `organization_members_rls_isolation`, `org_entitlements_rls_isolation`, `org_onboarding_states_rls_isolation`, `org_role_mappings_rls_isolation`, `org_tenant_links_rls_isolation`) all key off `current_setting('app.current_org', true)`, matching `WithOrgScope` exactly — the *existing* tables are fine.
- The **new** tables' policies (`auth_organization_tombstones_scope`, `auth_membership_projection_versions_scope`, both from migration `012`) key off `current_setting('app.current_org_id', true)` instead — the mismatch described in headline finding #1. `organization_domains` (migration `011`) correctly uses `app.current_org`, so only migration `012`'s two new tables have the bug.
- `billing-core-service` container confirmed (via `docker exec ... netstat -tlnp`) listening on `3014` (HTTP), `50013` (gRPC), `6062` (pprof), `9091` (metrics) — not `3017`, confirming headline finding #2.
- `auth_service` database has no outbox tables yet (`organization_projection_outbox`, `organization_membership_outbox`, `organization_deletion_outbox` all absent) — the auth-core half of the WIP is equally unapplied.

## Current Doc Cleanup Read

Keep for now:

- `README.md`
  - somewhat narrow, but not as misleading as the stale auth/user/session docs

Delete-ready:

- `.DS_Store`

## Bottom Line

`org-core` is a stable HTTP-first service in production today; the structural issues from the 2026-06-07 pass are unchanged (duplicate route families, gRPC infrastructure without business methods, duplicated startup helper code).

On top of that baseline, there is now a large, well-intentioned, uncommitted WIP (spanning org-core, auth-core, user-core, billing-core) aimed squarely at the 2026-07-02 audit's P0/P1 finding — "gateway invite/remove routes write Org Core directly; pending invitations have no verified acceptance/claim path; RLS GUC mismatch; X-Org-ID trust needs a real cross-plane contract." The verdict on that WIP, as of 2026-07-10:

- **Not deployed.** Neither the org-core binary nor the auth-core service (nor their respective DB migrations) reflect this code in the running containers.
- **Not complete.** It only reconciles membership changes that flow through Better Auth's own organization plugin; the gateway's direct-to-org-core invite/remove path is untouched, so the "gateway bypasses Auth Core" P0 finding is not closed. The unauthenticated pending-invitation flow is also untouched.
- **Contains two concrete bugs that would break it if shipped as-is:** an RLS GUC name mismatch (`app.current_org_id` vs. `app.current_org`) that would hard-fail every membership-reconcile and deletion-reconcile call and silently defeat the anti-resurrection tombstone check, and a wrong billing-core port (`3017` instead of `3014`) that would break the deletion-outbox's billing-deactivation step before it ever reaches Org Core.
- **Is a reasonable direction** — atomic create-with-owner provisioning, a durable revision-gated outbox with `FOR UPDATE SKIP LOCKED` claiming, and a DB-level deferred-constraint owner invariant are all sound patterns — but it needs the two bugs fixed, the gateway direct-write path folded in or explicitly deprecated, and at least basic test coverage before it should be considered a fix rather than a work-in-progress.
