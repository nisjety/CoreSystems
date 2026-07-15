# auth-core Research Dive

Original generated: 2026-06-07
Updated (source + isolated verification): 2026-07-15

Scope: `apps/Control Plane/auth-core` (NestJS/TypeScript, container `auth-service`, port 3011 / gRPC 50011)

## 2026-07-15 final secure-MVP addendum (current)

The original fresh-image run exited before `/api/convex-auth/jwks` became ready; bounded redacted logs isolated legacy internal-key startup configuration without printing credentials. Auth now consumes file-backed gRPC, internal HTTP/NATS, and Auth→User credential registries. Exact credential ID, principal, audience, token, scope/method, ambiguity, retirement, and bounded rotation overlap are fail-closed. The real Nest gRPC transport matrix passes 9/9 cases.

Convex signing startup now requires a readable matching RSA private/public pair of at least 2048 bits. Mismatched, EC, weak, and unreadable keys fail before service readiness. Production health requires exactly one RSA/RS256 signing JWK. The rebuilt production-mode image reached that readiness before the 4/4 container lifecycle passed. Production User mounts the same public-key secret and requires an explicit issuer.

Migrations 017-027 cover invitation repair, reviewed owner preflight, revisioned membership/audit intent, identity/GDPR outboxes, publish fencing, removal preflight, and atomic membership mutation intent. The final lifecycle proof permits safe bounded retries with stable IDs while requiring durable logical cardinality. `pnpm build`, 39 active suites/372 tests, 6/6 lint-contract tests, and the 135-file lint ratchet pass. Eighty-four changed files are unsuppressed; 212 violations remain explicitly baselined in seven untouched legacy files. New scoped-auth/key helpers measure 88.23% statements, 83.2% branches, and 91.3% lines; the Auth→User TLS transport helper is 100% covered. The Docker proto path is `/app/proto`, not `/app/dist/src/proto`; the real Auth/User TLS integration passes and rejects plaintext. Production file-backed secrets use a root-only handoff into app-owned `0600` files before `appuser`. Real production secret injection/rotation and deployed-image verification remain operator-owned; no live tenant mutation was performed.

## 2026-07-15 durable invitation/scoped-caller detail (superseded by final addendum above)

Migrations 017-027 close the Better Auth 1.6.23 status/member transaction gap durably, add reviewed historical-owner preflight, revision-ordered membership audit intent, transactional registration/provider-link delivery, and the `invitation.created_at` field required by Better Auth's real adapter path. The repair worker never infers from all historical `accepted` rows; cancel/delete/removal tombstones and existing roles win. Owner repair reports and stops unless one existing canonical member has an explicit reviewed mapping. The immutable bootstrap migration is unchanged; later migrations are additive.

Auth's Org projection, Billing deletion, User membership, and Application membership-reconciliation callers require pairwise-distinct audience/scope-bound tokens and reject reuse with legacy credentials. The seven-phase disposable-Postgres runner passes 59 Auth invitation/owner/audit cases plus deletion lifecycle tests. The expanded four-phase fresh-image stack proves real invitation creation, Auth -> Org convergence, duplicate invite/role/remove replay with exact audit cardinality, deletion checkpoint/resume, and Billing tombstone protection without touching existing tenants. Current test/static counts are recorded in the final addendum above. The long-running dev Auth image was not replaced during this continuation.

## 2026-07-14 Velion v3 boundary addendum (historical deployment evidence)

Auth Core was rebuilt from the current worktree and is healthy. Its public Better Auth/frontend origin is now configured through `VELION_PUBLIC_ORIGIN` (local live value `http://localhost:5173`) rather than an Auth-container URL. Invitation email HTML escapes dynamic fields and links to Velion's real `/accept-invitation/:invitationId` page. The gateway's invite/accept/remove/role-change and organization list/switch routes use Auth as canonical authority. Acceptance now uses an Auth-owned idempotent wrapper: real Drizzle transactions cover member creation plus active-org selection; the wrapper dispatches through Better Auth's trusted-origin/rate-limited router and normalizes enumeration-prone errors. Every Better Auth IP-precedence header is replaced with a verified-actor-derived 120-bit HMAC address, the Dragonfly increment/expiry is atomic, cache failures propagate, and a direct canonical acceptance request is rejected unless it carries the wrapper's invitation-bound 30-second HMAC marker. A committed retry returns the existing acceptance only when invitation ID, normalized invitee email, authenticated user ID, membership, and organization all match. Better Auth 1.6.23 still changes the invitation to `accepted` before the member/active-org transaction and compensates afterward on failure, leaving a residual medium risk of partial state if that compensation fails or an ambiguous commit occurs. Criterion E therefore still requires a broader transaction or durable reconciliation plus isolated real-Postgres fault injection.

The same-origin gateway now exposes bounded OAuth/OIDC and SAML callback routes. They pin Auth Core as upstream, do not follow upstream redirects, forward no browser-supplied identity/internal-auth headers, bound provider/body/response size, restrict SAML to form posts, and preserve only the callback response headers required by the browser. Production Auth/frontend URLs must be canonical HTTPS origins with no credentials/path/query/fragment; non-production HTTP is loopback-only. Gateway and release-nginx access logs are path-only and normalize invitation IDs, without changing query forwarding. Live fake-state OAuth produced a 302 to `http://localhost:5173/api/auth/error?error=state_mismatch`; an invalid provider returned 400, and fake callback/reset/invitation markers were absent from gateway/SPA logs.

A full Auth regression run exposed a fail-open ZDR posture in delegated token claims. Existing tests failed first; Model and plane token payloads now always set issuer-selected `zdr: true`, ignoring caller attempts to relax it. All 20 Jest suites/170 tests and `pnpm run build` pass; focused ESLint passes for the acceptance/rate-limit, atomic Dragonfly, Redis failure, and deployment-contract changes. The selected changed Auth security modules measure 95.16% lines, 94.07% statements, 93.75% functions, and 90% branches; the invitation controller measures 92.3% lines, 92.77% statements, 90% functions, and 90.47% branches; the atomic limiter is 100% covered. Public-origin/invitation-email logic measures 94.11% line coverage. This fail-closed contract can deliberately block a downstream provider that is not ZDR-attested; that is the secure-MVP behavior, not a UI fallback.

At the 2026-07-14 verification, Org/Billing/Audit still accepted a shared inbound key and lifecycle fault E2E was pending. The 2026-07-15 addendum above supersedes those source/isolation gaps; no live invitation, role, or deletion mutation was exercised in either pass.

## 2026-07-11 production-readiness addendum (historical)

This addendum supersedes older statements below that the outbox WIP was undeployed. Auth Core was rebuilt from the current worktree and is healthy. Its checksum migration runner is live with four ledger rows: `init_better_auth.sql`, `gdpr_hard_delete.sql`, `014_normalized_identity_email.sql`, and `015_organization_projection_outbox.sql`.

The first deployment reproduced a PostgreSQL migration-runner bug: `CASE WHEN EXISTS(...) THEN 1 / 0` was constant-folded and failed with division by zero even for an empty ledger. A failing deployment-contract test was added, and the runner now uses psql `\gset`/`\if`/`\quit 3`. The Docker runtime image also uses `COPY --chown` instead of a multi-minute recursive `chown -R`.

The live projection retry then exposed a second correctness bug: Postgres `BIGINT` revisions arrive in the TypeScript driver as strings, but Org Core binds `revision` as JSON `int64`. Three rows repeatedly returned HTTP 400. A failing test now covers number/string/bigint normalization and invalid revisions; `normalizeOutboxRevision` emits only positive safe JSON numbers. After redeploy, projection state moved from `5/0/5` total/published/pending to `5/3/2`, and membership state reached `3/3/0`. The two remaining projection rows have no canonical owner and are intentionally not claimable.

The 2026-07-11 continuation also closed two worker concurrency hazards. Org/Billing reconciliation HTTP now has a tested five-second abort deadline. Projection and membership acknowledgements use `UPDATE ... RETURNING` and increment success only when the exact claimed revision is still current; a stale worker can no longer mark or notify a newer row. The rebuilt live worker retained `5/3/2`, `3/3/0`, and zero outbox errors, as expected for the two unchanged ownerless rows.

Verification: Auth build passes; 12 Jest suites/78 tests pass. Measured coverage across the changed reconciliation/revision/service-principal set is 89.58%; `reconciliation-http.ts` and `outbox-revision.ts` are 100%, and `plane-service-principal.ts` is 85.71%. Full ESLint remains red at 591 errors/43 warnings. Auth is implemented/live but does not pass the MVP static gate yet.

The 2026-07-11 service-principal registry was audience/scope-bound and plane-token/policy endpoints failed closed. At that time Org/Billing/Audit still accepted a shared inbound key; the 2026-07-15 addendum records its scoped replacement.

## 2026-07-10 Update — Executive Summary

This pass re-verified the 2026-06-07 findings against current source, ran a full
signup→verify→signin→create-org flow against the live container, and reviewed
the large **uncommitted working-tree diff** touching `organization-events.plugin.ts`,
`auth.ts`, and `app.module.ts` (plus new untracked files) to see whether it closes
the P0/P1 gap from `apps/Control Plane/docs/core-research/plane-audit-2026-07-02.md`
("gateway invite/remove routes write Org Core directly; pending invitations have
no verified acceptance/claim path").

**It does not.** The uncommitted change is a real, well-designed fix for a
*different, also-real* problem: organization/membership/deletion state drift
between auth-core's Better Auth tables and org-core's projection, caused by a
currently-deployed event-publish mechanism that this audit **live-confirmed is
completely non-functional** (see "Root cause of org↔org-core drift" below). The
invite/remove-member bypass in org-core's HTTP handlers is untouched by this
diff and remains exactly as open as the prior audit found it.

Additionally, the new fix **cannot run yet even if deployed**: it depends on an
org-core RLS policy that references the wrong Postgres GUC name, and on three
already-committed org-core migrations (010/011/012) plus two new uncommitted
auth-core migrations (014/015) that have never been applied to the live
database. The running `auth-service`/`org-core-service` images predate the
whole diff, so none of today's live checks exercise the new code at all.

What did improve since 2026-06-07: the three `.unused`/`.backup` residue files
and `.DS_Store` noise claimed deleted in `apps/STALE_DOC_DELETION_REGISTER.md`
are confirmed gone; the previously-flagged consent/HIBP "TODO" comments in
`orpc-router.ts` are gone (those subsystems now have real implementations,
verified by reading the code); zero `TODO`/`FIXME` remain anywhere in `src/`.
What did not improve: eslint problem count actually rose slightly (649 → 682),
duplicate token controllers are still both wired, and mock email/SMS fallback
paths are unchanged.

## Snapshot

`auth-core` is the broadest Control Plane core. It is a NestJS + Better Auth
service that owns auth flows, session issuance, plane-token issuance,
OAuth/OIDC surfaces, auth event publication, NATS request-reply, and gRPC
token validation.

Current evidence highlights (2026-07-10):

- dual API layers: Better Auth native under `/api/auth/*` and enhanced
  oRPC-style routes under `/api/v2/auth/*`
- local NATS and shared NATS publishing
- gRPC registered for `auth.v1` and `dataplane.auth.v1`
- active mock fallbacks for email and SMS providers (unchanged from 2026-06-07)
- inactive `.unused`/`.backup` source residue: **confirmed deleted**, see below
- zero `TODO`/`FIXME` comments remain in `src/` (was: several, incl. consent
  persistence and HIBP, per 2026-06-07 doc — those are now real code)
- eslint: **682 problems (641 errors, 41 warnings)**, up from 649 previously
  documented — driven partly by unformatted new WIP (`orphan-organization-cleanup.service.ts`
  has 4 unresolved `prettier/prettier` violations)
- a large uncommitted working-tree diff is present (see "Uncommitted WIP" below)
  that is architecturally sound but not yet deployable

## Runtime Shape

Key runtime entrypoints (unchanged structure from 2026-06-07, sizes current):

- `src/main.ts` (230 lines)
  - Nest bootstrap, NATS microservice connection, gRPC microservice connection
    with reflection, Better Auth manual catch-all registration, Swagger setup,
    global CORS and security headers
- `src/app.module.ts` (63 lines; +2 lines uncommitted — see below)
  - wires `ORPCModule`, `AuthModule`, `NatsModule`, `InternalServicesModule`,
    email/docs, and the custom controllers
- `src/auth/auth.ts` (1,376 lines; +58/-33 uncommitted — see below)
  - Better Auth configuration and provider/plugin wiring
- `src/auth/orpc-router.ts` (4,246 lines)
  - main enhanced auth surface — API keys, bearer tokens, passkeys, 2FA, HIBP
    password checks, consent, invite-member proxy, all with real
    implementations now (see "Stubs, Mocks, Placeholders" below for what is
    still genuinely a placeholder)
- `src/auth/organization-events.plugin.ts` (481 lines; +390/-63 uncommitted —
  see below)
  - was: local/shared NATS event fan-out for org create/member add/member remove
  - now (uncommitted): durable Postgres-outbox reconciliation with org-core,
    plus the old NATS fan-out kept as best-effort
- `src/middleware/organization-event.middleware.ts` (existing, not touched by
  the uncommitted diff) — a **second**, independent attempt to publish the same
  organization lifecycle events by monkey-patching Express `res.json`/`res.send`
  on Better Auth's organization routes. Live-verified non-functional (see below).
- `src/internal/auth-event.publisher.ts`
  - local and shared event fan-out

Main controllers/modules (current `src/app.module.ts`):

- `UsersController`
- `NatsAuthController`
- `ConvexAuthController`
- `ModelPlaneTokenController`
- `PlaneTokenController`
- `AuthGrpcController`
- `ORPCModule`
- `InternalServicesModule`
- `OrphanOrganizationCleanupService` (new uncommitted provider, `@Cron(EVERY_MINUTE)`)

## Uncommitted WIP: what it actually is (2026-07-10)

Diff stat (from repo root, `git diff --stat`):

```
apps/Control Plane/auth-core/src/app.module.ts                        |   2 +
apps/Control Plane/auth-core/src/auth/auth.ts                         |  58 ++-
apps/Control Plane/auth-core/src/auth/organization-events.plugin.ts   | 390 +++++++++++++++++++--
```
plus untracked new files:
```
apps/Control Plane/auth-core/migrations/014_normalized_identity_email.sql
apps/Control Plane/auth-core/migrations/015_organization_projection_outbox.sql
apps/Control Plane/auth-core/src/auth/account-linking.policy.ts
apps/Control Plane/auth-core/src/auth/account-linking.policy.spec.ts
apps/Control Plane/auth-core/src/services/orphan-organization-cleanup.service.ts
```

This is one coherent change with two independent halves:

### Half 1 — OAuth account-linking hardening (`src/auth/auth.ts`)

- `trustedProviders` for Better Auth's `accountLinking` config is hardcoded to
  `[]` (was: `configuredTrustedProviders()`, a ~25-line helper that trusted any
  OAuth provider with client credentials configured in env). The removed
  helper and its call site are gone; the comment left in its place explains
  the reasoning: a "trusted" provider bypasses Better Auth's own
  same-email-verification requirement before linking, which is an
  account-takeover path if any configured provider ever returns an
  unverified/spoofable email claim.
- New `src/auth/account-linking.policy.ts` (untracked) exports
  `normalizeIdentityEmail` (trim + lowercase) and
  `canImplicitlyLinkProviderIdentity` (pure predicate: verified + case-insensitive
  match). `normalizeIdentityEmail` **is** wired in, into new
  `databaseHooks.user.create.before` / `user.update.before` hooks in `auth.ts`
  (lines ~684-704), so canonical user emails are normalized at write time —
  paired with the new `migrations/014_normalized_identity_email.sql`
  (untracked), which adds `CREATE UNIQUE INDEX ... ON "user" (LOWER(BTRIM(email)))`
  and a unique index on `account(provider_id, account_id)`.
  **`canImplicitlyLinkProviderIdentity` itself is dead code** — it has full
  unit-test coverage in `account-linking.policy.spec.ts` (untracked) but is
  never imported anywhere outside its own spec file. It looks like it was
  meant to gate the linking decision explicitly; instead the actual wiring
  just sets `trustedProviders: []`, which achieves the same practical effect
  through Better Auth's own built-in verification gate. Flag for knip/cleanup.
- `creatorRole` for the organization-creation plugin config is hardcoded to
  `'owner'` (was: `process.env.ORG_CREATOR_ROLE || 'owner'`, configurable to
  `'admin'`). Comment explains: an org whose creator becomes only `admin`
  could exist with no `owner`, which the rest of this diff's org-core
  invariants assume can't happen.

This half is small, self-contained, and does not depend on any new
infrastructure — it is safe to consider effectively complete and correctly
scoped to what it claims to fix (OAuth account-linking / creator-role
footguns), independent of the rest of the diff.

### Half 2 — Organization/membership/deletion outbox reconciliation

`src/auth/organization-events.plugin.ts` grows from 91 to 481 lines. The old
behavior — publish an `organization.created`/`member_added`/`member_removed`
NATS event directly from inside the Better Auth `databaseHooks.organization.*`
callback, best-effort, no retry — is replaced with:

- `organization_projection_outbox`, `organization_membership_outbox`,
  `organization_deletion_outbox` — three new Postgres tables in auth-core's own
  `auth_service` DB, defined in `migrations/015_organization_projection_outbox.sql`
  (untracked). Rows are populated two ways: (a) directly by the plugin code on
  create/add-member/remove-member, and (b) by three new `AFTER INSERT/UPDATE/DELETE`
  triggers on `organization` and `member` — the trigger path means outbox rows
  get queued **even if the Better Auth hook that used to publish never fires**,
  which turns out to matter a great deal (see next section).
- `flushOrganizationProjectionOutbox()`, `flushOrganizationMembershipOutbox()`,
  `flushOrganizationDeletionOutbox()` (all exported from
  `organization-events.plugin.ts`) claim pending rows with `FOR UPDATE SKIP LOCKED`
  and call new org-core internal endpoints:
  - `POST /internal/orgs/:orgId/members/reconcile` (org-core `handlers.go`,
    new `reconcileOrganizationMember`) — revision-gated idempotent upsert/remove.
  - `POST /internal/orgs/:orgId/reconcile-delete` (org-core `handlers.go`, new
    `reconcileOrganizationDeletion`) — writes a permanent tombstone, then calls
    the existing `gdpr_hard_delete_organization` stored procedure.
  - Deletion also calls a new `POST /api/v1/billing/orgs/:orgId/deactivate` on
    billing-core (new `deactivateOrganization` handler there).
- New `src/services/orphan-organization-cleanup.service.ts` (untracked)
  registers `@Cron(CronExpression.EVERY_MINUTE)` to call all three flush
  functions, then separately sweeps and deletes organizations older than 2
  minutes with no `owner` member (a pre-existing behavior, now folded into the
  same cron tick).
- org-core gets `ProvisionOrganizationWithOwner` (new, `repository.go` +
  `service_enhanced.go`) — one transaction that upserts the org row, checks a
  new `auth_organization_tombstones` table to refuse resurrecting a deleted
  org, upserts the canonical owner (rejecting with `ErrOwnerConflict` if a
  *different* owner already exists), seeds default entitlements, and seeds
  onboarding state. `createOrganization` in `handlers.go` now calls this
  instead of the old two-step "upsert org, then add member" (which could
  leave an ownerless org on partial failure — this is exactly the gap
  `OrphanOrganizationCleanupService`'s pre-existing sweep exists to clean up
  after).

This is a legitimate, coherent fix for genuine data-integrity problems
(partial-write orgs, replayed/duplicate events, unordered NATS delivery
resurrecting stale state). It is **not** a fix for the invite-acceptance gap —
it never touches `AddPendingInvite`, the `inviteMember`/`removeMember` HTTP
handlers in org-core, or anything reachable from the gateway's
`/api/v1/orgs/:id/members/invite` route (see "Invite/remove gap — still open"
below).

## Root cause of org↔org-core drift (live-verified 2026-07-10, new finding)

This audit reproduced the exact class of gap the WIP is trying to fix, against
the **currently deployed** (pre-WIP) code:

1. Signed up a fresh test user against `http://localhost:3011`, retrieved the
   OTP from Dragonfly (`controlplane-dragonfly`, DB index 3 — the index
   embedded in auth-core's own `DRAGONFLY_URL`) at key
   `verification:email-verification-otp-<email>`, verified via
   `POST /api/auth/email-otp/verify-email`, signed in (`EMAIL_NOT_VERIFIED`
   correctly blocked the pre-verification attempt, confirming the documented
   baseline), then created an organization via
   `POST /api/auth/organization/create` (required an `Origin` header matching
   `BETTER_AUTH_TRUSTED_ORIGINS`, e.g. `http://localhost:5173`, or Better
   Auth's CSRF guard rejects with `MISSING_OR_NULL_ORIGIN`).
2. The organization and its owner-member row were created correctly in
   auth-core's own `organization`/`member` tables (`auth_service` DB).
3. **The same organization never appeared in org-core's `organizations` table
   — zero rows.** Counting both tables directly:
   `auth_service.organization` = 4 rows vs. `org_core.organizations` = 1 row.
   **3 of 4 organizations that exist in auth-core are invisible to org-core**
   in the currently running environment.
4. Root cause isolated by log inspection (`docker logs auth-service`, full
   9,458-line buffer covering the container's entire 13-hour uptime): **neither
   of the two mechanisms that are supposed to notify org-core ever fires its
   own success log, despite 4 real organization creations including the one
   performed live during this audit**:
   - `organizationEventsPlugin()`'s `databaseHooks.organization.create.after`
     hook (`organization-events.plugin.ts`) logs `'🎊 Organization created hook
     triggered:'` and `'📢 Published organization.created event:'` on success.
     Zero occurrences of either string anywhere in the log buffer. The plugin
     itself does initialize (`'🎉 Organization Events Plugin initialized'` and
     `'🔧 Setting organization event publisher for plugin: true'` both log
     once at container startup), so the publisher is wired — the hook body
     simply never runs for a real org-create call.
   - `OrganizationEventMiddleware` (`src/middleware/organization-event.middleware.ts`)
     monkey-patches `res.json`/`res.send` on `/api/auth/organization/*` routes
     and logs `'🎊 Organization created via Better Auth'` /
     `'📢 Published organization.created event: <id>'` on success. Its entry
     logs fire correctly (`'🔍 Middleware intercepting: POST
     /api/auth/organization/create'`, `'📥 Intercepting organization
     endpoint...'`), but its **response-handling** logs never appear anywhere,
     even for the org created live during this audit (whose JSON response body
     did contain `id`, which is all `handleResponse` requires to log
     success). This strongly suggests Better Auth's manual catch-all response
     writing in `src/main.ts` does not actually go through Express's
     `res.json`/`res.send` — so the monkey-patched methods are never invoked
     for the real response body, and this middleware has likely never worked
     for its stated purpose.

This is the concrete, live-reproduced mechanical failure that motivates the
uncommitted outbox+trigger redesign: a DB trigger on `organization`/`member`
populates the new outbox tables independent of whichever app-level hook does
or doesn't fire, which is a sound way to route around the bug above rather
than debug it. That said, the redesign has its own problems (next section).

## Is the uncommitted fix deployable as-is? No — two blocking gaps

1. **RLS GUC name mismatch (org-core, live bug).**
   `apps/Control Plane/org-core/migrations/012_auth_projection_guards.up.sql`
   (lines 26-27, 31-32; already committed, not part of this WIP but exercised
   by it) defines:
   ```sql
   CREATE POLICY auth_organization_tombstones_scope ON auth_organization_tombstones
     USING (org_id = current_setting('app.current_org_id', true))
     WITH CHECK (org_id = current_setting('app.current_org_id', true));
   -- (same pattern for auth_membership_projection_versions)
   ```
   But the only code that ever sets this class of GUC —
   `(*DB).WithOrgScope` in `apps/Control Plane/org-core/internal/database/database.go`
   (lines ~81-97), which every new reconciliation call in `repository.go` uses
   — sets **`app.current_org`** (no `_id` suffix), matching the GUC name used
   by the working tenant-isolation policies in migrations 008/009. Migration
   012's policies reference a GUC that is never set by any code path, and
   (unlike 008/009) have no `OR current_setting(...) = ''` escape hatch.
   Consequences if this were deployed today:
   - `ReconcileOrganizationMember`'s `INSERT INTO auth_membership_projection_versions`
     (`repository.go`) would fail its `WITH CHECK` on every single call — every
     membership-sync from the new outbox would permanently error (retried
     forever by the per-minute cron, `attempts`/`last_error` incrementing,
     never succeeding).
   - `ProvisionOrganizationWithOwner`'s tombstone check
     (`SELECT EXISTS(... FROM auth_organization_tombstones ...)`) would
     **silently** always return `false` — RLS filters `SELECT`s rather than
     erroring, so this doesn't crash, it just always reports "not tombstoned."
     That defeats the entire point of the tombstone table: a stale/replayed
     org-create call could resurrect a previously deleted organization.
2. **The tables this WIP depends on don't exist in the live database at all.**
   `org_core.schema_migrations` (checked live) shows the last applied
   migration is `009_rls_enforce_tenant_isolation`. Migrations
   `010_owner_invariant`, `011_verified_organization_domains`, and
   `012_auth_projection_guards` — all three **already committed to git**, not
   just part of this uncommitted diff — have never been run against
   `controlplane-postgres`/`org_core`. `auth_organization_tombstones` and
   `auth_membership_projection_versions` do not exist yet. Likewise
   auth-core's new (untracked) `014_normalized_identity_email.sql` and
   `015_organization_projection_outbox.sql` are unapplied —
   `auth_service.organization_projection_outbox` and the unique email index do
   not exist in the live DB either.
3. **The running images predate the whole diff.** `auth-service`'s image was
   built 2026-07-04 (container started 2026-07-07); the running
   `/app/dist` contains neither the `organization_projection_outbox` string
   nor `normalizeIdentityEmail`. All of today's live checks (including the
   org-creation flow above) exercised the **old** code path only. The doc note
   already added to `apps/Control Plane/docs/core-research/plane-audit-2026-07-02.md`
   by a concurrent pass ("The running images were built before the current
   uncommitted transformation set was fully reconciled...") is consistent with
   this and independently confirmed here.

Net: this half of the WIP is a correct diagnosis and a reasonable design, but
needs (a) migration 012's GUC name fixed, (b) migrations 010-012 (org-core)
and 014-015 (auth-core) actually applied, and (c) fresh images built and
deployed, before it does anything. None of that has happened yet.

## Invite/remove gap — still open, unrelated to and unfixed by this WIP

Confirmed by reading current (unmodified by this diff) org-core code,
`apps/Control Plane/org-core/internal/http/handlers.go` (~lines 360-401):

- `POST /orgs/:id/members/invite` (reached from the Frontend Plane gateway,
  `x-user-id` header trusted as the inviter) looks up the invited email
  against user-core. **If a matching user is found, it calls
  `s.orgService.AddOrganizationMember(...)` directly** — the invited user
  becomes an active org member immediately, with no consent step, no email
  confirmation, and no acceptance action from the invitee.
- If no matching user is found, it calls `AddPendingInvite`, which only
  records a row (comment: "invitation recorded; user will be added when they
  register") — there is no token-based claim/accept endpoint visible anywhere
  in this handler file.
- This is a **second, shadow invite mechanism** distinct from — and bypassing
  — auth-core's own Better Auth organization plugin, which already has a real,
  working, verified invite→accept flow: `POST /api/auth/organization/invite-member`
  creates a Better Auth `invitation` row with an expiry
  (`invitationExpiresIn`, `src/auth/auth.ts` ~line 895), and the email sent
  (`src/auth/auth.ts` lines ~906-931) links to
  `${BETTER_AUTH_URL}/accept-invitation/${data.id}` — verified server-side by
  Better Auth against the invitation ID, expiry, and (implicitly) the invited
  email. `inviteMemberProcedure` in `src/auth/orpc-router.ts` (lines 1878-1975)
  proxies to this same native endpoint.
- The uncommitted WIP does not touch `AddPendingInvite`, `AddOrganizationMember`,
  the `inviteMember`/`removeMember` HTTP handlers, or anything the gateway's
  invite/remove routes call. **This P0/P1 finding from the 2026-07-02 audit is
  confirmed still fully open.**

## Duplicates, Redundancies, And Inactive Surfaces

Clear duplicate or redundant patterns (unchanged from 2026-06-07, re-verified):

- `src/auth/model-plane-token.controller.ts` (238 lines) and
  `src/auth/plane-token.controller.ts` (295 lines) still both exist, both
  still wired into `app.module.ts`'s `controllers` array, and still overlap in
  purpose (token-minting).
- `src/auth/orpc-router.ts` is now 4,246 lines (grew since 2026-06-07's "110
  indexed symbols" note) — functionally central but structurally dense.
- **New in this pass**: `src/auth/organization-events.plugin.ts` (481 lines)
  and `src/middleware/organization-event.middleware.ts` are two independent,
  overlapping implementations of "publish organization lifecycle events" —
  one live-confirmed silently dead (the middleware, see above), one
  live-confirmed silently dead in its old form and being replaced (the
  plugin). Once the outbox redesign lands, the still-present
  `OrganizationEventMiddleware` becomes pure dead weight and should be deleted
  rather than left as a second, confusingly-similar mechanism.
- `canImplicitlyLinkProviderIdentity` (`src/auth/account-linking.policy.ts`)
  is unused dead code outside its own test — see "Half 1" above.

Inactive source residue claimed deleted in `apps/STALE_DOC_DELETION_REGISTER.md`:

- `src/auth/orpc-router.ts.backup` — **confirmed deleted** (not present, no
  `git log` history for the path either — was apparently never tracked).
- `src/orpc/consolidated-auth.controller.ts.unused` — **confirmed deleted**.
- `src/orpc/unified-auth.controller.ts.unused` — **confirmed deleted**.
- `.DS_Store` / `src/.DS_Store` — **confirmed deleted/absent**.

## Stubs, Mocks, Placeholders, And Missing Connections

Re-verified 2026-07-10. Active fallback or placeholder behavior:

- `src/auth/auth.ts`
  - `src/auth/auth.ts:270` — mock Resend sender when `RESEND_API_KEY` is
    missing (`console.warn('⚠️ RESEND_API_KEY not set, using mock resend')`)
  - `src/auth/auth.ts:290-302` — mock Twilio Verify service when Twilio init
    fails, including a hardcoded "APPROVED" verify response
    (`src/auth/auth.ts:302`)
- `src/email/resend.service.ts:24,62,100` — development mock email logging
  ("Mock: Verification email to...", "Mock: Password reset email to...",
  "Mock: OTP email to...")
- `src/sms/twilio-verify.service.ts:78,113,115,196` — development mock SMS,
  and a mock verify path that **always returns `true`** for verification
  (`twilio-verify.service.ts:115`, `return true; // Mock verification always
  succeeds`) — unchanged risk from 2026-06-07: if this fallback is ever
  reachable in a misconfigured production environment, any phone OTP check
  degrades to "always succeeds."
- `src/internal/contracts/user-service.contract.ts:1` — `export const
  placeholder = true;` — placeholder-only export, unchanged.

Improved since 2026-06-07 (previously flagged, now verified as real code, not
placeholders):
- Consent persistence (`orpc-router.ts` — `persistConsent`, `consentFromRow`,
  real DB-backed implementation, lines ~448-476, ~993-1015).
- HIBP password-range check (`orpc-router.ts:531` real HTTP call, gated by
  `HIBP_ENABLED` env, `orpc-router.ts:1691`).
- API keys (`createAPIKeyProcedure`, `listAPIKeysProcedure`,
  `deleteAPIKeyProcedure`, `validateAPIKeyProcedure` — all call real Better
  Auth API-key methods). `rotateAPIKeyProcedure`
  (`orpc-router.ts:2670-2688`) is a **deliberate, clearly-messaged
  non-implementation** ("API key rotation is not supported by Better Auth for
  existing key IDs. Create a replacement key, then delete the old key.") —
  this is an honest capability limit, not a silent placeholder.
- Bearer tokens (`validateBearerTokenProcedure` etc.) — real DB-backed
  (`schema.bearerToken`) plus a Redis fast-path cache.
- Passkeys — real calls into Better Auth's passkey API
  (`generatePasskeyRegistrationOptions`).

Zero `TODO`/`FIXME` comments remain anywhere in `src/**/*.ts` (checked via
`grep -rniE "TODO|FIXME"`, excluding specs) — the 2026-06-07 doc's "consent
persistence TODOs" and "HIBP TODO" no longer describe the code.

Missing or partial relationships:

- audit publication still intentionally skips pre-onboarding users without
  `org_id` (unchanged)
- **the org→org-core projection relationship is confirmed broken in the
  currently deployed build** — see "Root cause of org↔org-core drift" above.
  This supersedes the 2026-06-07 doc's vaguer "org membership/create flows"
  relationship note.

## API Design And Performance Notes

- Dual-route separation between `/api/auth/*` and `/api/v2/auth/*` remains a
  sound boundary.
- Multiple token-minting controllers still suggest the public contract
  evolved faster than the internal abstraction layer (unchanged).
- **New**: two independent, both-broken organization-event-publish mechanisms
  co-existed silently for at least 13 hours of production uptime without any
  error surfacing anywhere (no failed health check, no alert — the org-core
  drift is invisible unless someone diffs row counts across two databases, as
  this audit did). This is worth an operational takeaway independent of the
  in-flight fix: **cross-service projection consistency has no monitoring or
  alerting today.** The new `OrphanOrganizationCleanupService` cron logs
  counts on every flush, which is a step toward observability, but nothing
  currently pages on a growing backlog.
- Logging is extremely verbose (debug-level emoji-prefixed logs on every
  request path, cache hit/miss, procedure dispatch) with no apparent
  log-level gating in production — `docker logs auth-service` for a single
  org-create request produces dozens of lines. This makes exactly the kind of
  "did event X actually fire" question this audit had to answer harder than
  it should be, and is itself worth cleaning up (structured logging, and gate
  the `🔍`/`💾` cache-trace lines behind a debug flag).

## Current Doc Cleanup Read

Keep:
- `docs/DNS_CONFIGURATION.md`
- `docs/DNS_MIGRATION_PLAN.md`
- `docs/EMAIL_CONFIGURATION.md`

Delete-ready (docs, unchanged assessment):
- `docs/auth-plan.md` — historical implementation plan, not current runtime truth
- `docs/api.md` — outdated route and behavior documentation
- `docs/SPRINT_4_TEST_REPORT.md` — point-in-time status report

Delete-ready inactive source residue: **already deleted, confirmed** (see
"Duplicates, Redundancies, And Inactive Surfaces" above) — no action needed.

## Live Checks Performed 2026-07-10 (against localhost:3011, currently deployed
pre-WIP code)

1. `GET /api/auth/ok` → `200`.
2. `POST /api/auth/sign-up/email` → `200`, user created, `emailVerified:false`,
   `token:null` (consistent with `EMAIL_OTP_SEND_ON_SIGNUP=true`,
   `REQUIRE_EMAIL_VERIFICATION=true`).
3. `POST /api/auth/sign-in/email` before verification → `401`
   `{"message":"Email not verified","code":"EMAIL_NOT_VERIFIED"}` — confirms
   documented baseline behavior (not a bug).
4. Retrieved the OTP directly from Dragonfly:
   `docker exec controlplane-dragonfly redis-cli -n 3 -a <password from
   auth-service's DRAGONFLY_URL> GET
   'verification:email-verification-otp-<email>'` → JSON containing
   `"value":"<6-digit-otp>:0"`. This is a viable dev/test bypass for
   completing verification flows without a real email provider.
5. `POST /api/auth/email-otp/verify-email` with the retrieved OTP → `200`,
   `emailVerified:true`.
6. `POST /api/auth/sign-in/email` after verification → `200`, session cookie
   (`sid`) issued.
7. `POST /api/auth/organization/create` — first attempt without an `Origin`
   header → `400 MISSING_OR_NULL_ORIGIN` (CSRF guard working as intended);
   retried with `Origin: http://localhost:5173` (in
   `BETTER_AUTH_TRUSTED_ORIGINS`) → `200`, org + owner member created.
8. Cross-checked `org_core.organizations` for the new org ID → **0 rows**.
   Compared full table counts: `auth_service.organization` = 4,
   `org_core.organizations` = 1.
9. Searched the full `auth-service` log buffer (9,458 lines, full 13h
   uptime) for both event-publish mechanisms' success log lines → 0 hits for
   either, isolating the drift's root cause to hook/middleware wiring rather
   than network/env misconfiguration.
10. Confirmed via `docker exec auth-service grep -r
    organization_projection_outbox /app/dist` (and `normalizeIdentityEmail`)
    → no matches; confirmed via `docker inspect` the image predates the WIP
    (built 2026-07-04, container started 2026-07-07).
11. Confirmed via `org_core.schema_migrations` that migrations 010-012 are
    unapplied; confirmed via `\dt` that none of the new outbox/tombstone
    tables exist in either database.

No invitation, deletion, billing, or role-mutation destructive action was
performed beyond the one test org created above (left in place; harmless test
data, id visible in the checks above).

## Bottom Line (2026-07-10)

`auth-core` is real, central, and heavily wired — that has not changed. Since
2026-06-07:

- **Fixed**: dead `.unused`/`.backup`/`.DS_Store` residue is gone; several
  previously-placeholder subsystems (consent, HIBP, API keys, bearer tokens,
  passkeys) are now real; zero TODO/FIXME remain.
- **In flight, not yet safe to trust**: the uncommitted organization-events
  rewrite is a legitimate fix for a real, now live-confirmed problem (org
  creation events silently never reach org-core — 3 of 4 live orgs are
  invisible to org-core today), but it depends on an org-core RLS policy with
  the wrong GUC name and on five migrations (three already-committed,
  unapplied; two new, uncommitted, unapplied) that have not been run anywhere.
  It will not function as deployed even once committed, until those are fixed.
- **Still open, unrelated to the above**: gateway/org-core invite and
  remove-member routes write org-core directly with no verified
  acceptance/claim step, a second shadow mechanism sitting alongside
  auth-core's own working Better Auth invitation flow. This was flagged
  2026-07-02 and remains completely unaddressed by any code in this
  worktree.
- **Unchanged risk**: duplicated token-minting controllers, mock email/SMS
  fallback behavior in sensitive auth paths, and now-worse eslint hygiene
  (682 vs. 649 problems, with fresh unformatted code in the new WIP).
- **New operational gap surfaced this pass**: no monitoring exists for
  cross-service projection drift between auth-core and org-core; it took a
  manual live signup+create-org+row-count-diff to find a 75% invisibility
  rate.
