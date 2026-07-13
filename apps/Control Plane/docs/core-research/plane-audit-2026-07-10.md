# Control Plane Audit

Baseline date: 2026-07-02
Live re-verification date: 2026-07-10 (two passes: an earlier same-day docker/smoke pass, then a full 6-service parallel code + live-security audit)

Scope: `apps/Control Plane` (auth-core, org-core, user-core, billing-core, session-core, audit-core, plus controlplane-postgres/nats/dragonfly and the Lago billing stack)

## Production-readiness continuation — verified 2026-07-11

This section is the current disposition of the 2026-07-10 findings. The original audit narrative below is retained as chronology; statements below it saying fixes were unshipped, migrations unapplied, or containers stale are no longer current.

**MVP decision: not accepted.** The critical exploit fixes and durable-consumer work pass source/integration tests, but the live Docker stack is now degraded by containerd/BuildKit filesystem I/O errors. User/Session/Audit were recreated and became unhealthy when Docker lost consistent Postgres/container metadata; the final gateway signing image failed to build. Docker recovery plus matched-image redeployment is the first gate, followed by the ownerless-org, shared-key, producer-durability, lifecycle-E2E, lint, and coverage gates. Enterprise readiness remains deferred.

### Final operational incident

Earlier on 2026-07-11 all six services and the gateway were healthy and the live security/readiness curls below passed. During the final gateway rebuild, Docker failed to read a cached Rust `.rlib` and then failed writes/reads under BuildKit metadata, image blobs, container `/hosts`, and JSON logs with `input/output error`. `docker inspect` and `docker ps` contradicted each other about Postgres state. No restart, factory reset, volume prune, or database recreation was attempted; those require operator approval and preservation checks.

| Original finding | Current disposition and evidence |
|---|---|
| Session Core forged bearer/header impersonation | **Fixed, tested, deployed, live pass.** Missing bearer, `Bearer garbage`, and a forged JWT combined with a real `X-User-Id`, forged `X-Org-Id`, and `X-User-Role: admin` each return 401 from `/api/v1/sessions/current`; response bodies do not contain the fixture identity. The gateway `/api/v1/session/bootstrap` also returns 401. |
| Gateway-to-Session service delegation | **Source/test fixed; deployment blocked.** A scoped static token plus selected user header previously returned 200. Session now requires the same method/URI/body/subject-bound HMAC envelope used for User; unsigned delegation returns 403 in Go tests and the Rust signer matches a fixed vector. The gateway image build failed on Docker storage I/O, so the pair is not live-accepted. |
| Auth-to-Org projection silently lost organizations | **Transport/concurrency fixes deployed; historical-data gate remains.** Postgres `BIGINT` revisions are normalized to safe JSON numbers. Reconciliation calls abort after five seconds. Exact-revision `UPDATE ... RETURNING` gates success counting and follow-on notification, so a stale worker cannot acknowledge a newer row. Live state remains projections `5/3/2`, memberships `3/3/0`, errors 0; Auth=5, Org=3, missing=2, extra=0. Both pending rows have `owner_user_id IS NULL`; no owner was invented or manually assigned. |
| RLS GUC mismatch | **Fixed and live.** Org migrations 010-013 are applied. `pg_policies` inventory: zero references to `app.current_org_id`, 14 references to `app.current_org`. Functional and isolated rollback migration tests pass. |
| Billing port/deletion coupling | **Fixed and live-configured.** Auth points to Billing port 3014; Billing and Org completion are independently checkpointed. Billing migration 0005 (`billing_organization_tombstones`) and Org revision/tombstone guards are applied. No live tenant deletion was exercised. |
| Gateway direct Org membership mutations | **Fixed in source, route tests, and live routing.** Invite/accept/remove/role-change target Auth Core. Six competing Org mutations are not mounted and return 404 live; Auth's reconcile path remains mounted. Lifecycle mutation E2E is pending because real tenant roles/invitations were kept untouched. |
| User Core self-asserted admin/profile | **Fixed in source/tests; earlier paths live-verified, final handler sweep awaiting redeploy.** Role is never header-authoritative. A final regression found three handlers still consumed raw profile headers; all now use verified context. Gateway/Session→User calls require request-bound HMAC. The final User image is not live-accepted after the Docker incident. |
| Audit usage SQL | **Fixed, tested, deployed, live pass.** `/v1/usage/summary` returns 200 against the live schema. |
| Audit cross-plane NATS not connected | **Durable consumption was live before incident; inbox fix isolated-only.** Named consumers ACK/NAK/DLQ correctly. Migration 002 adds stream-sequence inbox uniqueness and TERM-after-DLQ-success; disposable Postgres proves dedupe. Live migration verification was blocked by Docker storage failure. Producer outboxes, pending metrics, and replay remain. |
| Email normalization | **Fixed at User Core's canonical repository boundary** with regression coverage. |
| Delayed resurrection | **Guarded for the new Auth/Org membership and organization projections and Billing lifecycle** with revisions/tombstones. Broader legacy NATS handlers still require isolated reordering E2E before acceptance. |

### Commands and results added by the continuation

| Command/check | Result |
|---|---|
| Focused Auth coverage (`reconciliation-http`, outbox revision, service principal) | 12 suites, 78 tests pass. Changed set 89.58%; reconciliation HTTP and outbox revision 100%; service principal 85.71%. |
| `pnpm run build` in auth-core | Pass. |
| `go test -coverprofile=/tmp/<service>.cover ./...` for session/user/org/billing/audit | All pass. Whole-service totals: 20.2%, 3.8%, 5.6%, 12.6%, 10.4%. |
| `go vet ./...` for all five Go services | Pass. |
| `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --all-targets` in the gateway | Pass; 209 tests. |
| Audit embedded JetStream integration + race test | Pass; ACK, transient NAK retry, malformed DLQ, five-delivery exhaustion, and stream update covered; subscriber coverage 81.0%. |
| `pnpm exec eslint "{src,apps,libs,test}/**/*.ts"` in auth-core | Fail: 591 errors, 43 warnings. Existing broad baseline; no bulk `--fix` was run. |
| `git diff --check` | Pass. |
| Docker health | Six Control services, gateway, Data documents API, and Data retrieval engine healthy. |

### Live migration evidence

- Auth ledger: `init_better_auth.sql`, `gdpr_hard_delete.sql`, `014_normalized_identity_email.sql`, `015_organization_projection_outbox.sql`.
- Org ledger: migrations 001 through 013, including owner invariant, projection guards, and strict fail-closed RLS.
- Billing ledger: migrations 0001 through 0005, including organization tombstones.
- User ledger: migrations through 013 onboarding retention.

No tenant membership, invitation, role, subscription, billing, or deletion row was mutated by the verification commands. Live infrastructure changes were limited to deploying the named images, the existing worker retrying its durable outbox, and Audit creating its owned JetStream stream/consumers. No commit, push, destructive Git operation, database reset, down-migration, or legacy-doc deletion was performed.

## Executive summary

All six Control Plane services and their infra (Postgres, NATS, Dragonfly, Lago) are up and individually healthy. But this pass found the plane has **one critical live security bypass**, **one fully-broken cross-plane sync path with real data loss**, and **an in-flight fix for that sync path that would itself fail if deployed as committed**. None of the running containers include today's large uncommitted WIP — it exists only in the working tree.

Read `CONTROL_PLANE_STATUS.md` for the current-state snapshot and `CONTROL_PLANE_ROADMAP.md` for the fix plan. Per-service detail (live checks, exact file:line citations, full curl transcripts) lives in `docs/core-research/{auth-core,org-core,user-core,billing-core,session-core,audit-core}.md`, all re-verified and rewritten today.

## Top findings, by severity

| # | Severity | Service | Finding | Live evidence |
|---|---|---|---|---|
| 1 | **CRITICAL — FIXED 2026-07-10, same session** | session-core | `authContextMiddleware`'s bearer-token branch performed **zero token validation**. Any non-empty `Bearer <string>` + a self-asserted `X-User-Id` header was accepted as a fully authenticated identity — no signature check, no expiry check, no auth-core round-trip. | **Fix applied**: the bearer branch now calls `resolveIdentityFromBearer()` (ported from user-core's existing, already-correct pattern) which round-trips the token to auth-core's `GET /api/auth/get-session` and only trusts the identity auth-core returns — client-supplied `X-User-Id`/`X-User-Email`/`X-User-Name` headers are never trusted standalone again. Re-verified live: the exact forged-bearer exploit from this audit now returns 401; a real signup→verify→signin bearer token still returns 200 with the correct own-user session (no regression); the internal-key path is unaffected (still 200). **Not yet done**: the five legacy handlers with no per-call ownership parameter (`GetSessionState`, `GetEventsSince`, `SendMessage`, `ResolveApproval`, `ResumeSession`) still need ownership checks, and there's still no automated regression test for this — see `CONTROL_PLANE_ROADMAP.md` Phase A, items 2-3. |
| 2 | **HIGH** | auth-core ↔ org-core | Organizations created via auth-core's Better Auth API **never reach org-core**. Both currently-deployed publish mechanisms (a `databaseHooks` hook and an Express `res.json`/`res.send` monkey-patch) silently never fire their success path. | Live test: created org via `POST /api/auth/organization/create` → present in `auth_service.organization`, **absent (0 rows)** in `org_core.organizations`. Table counts: `auth_service.organization=4` vs `org_core.organizations=1` — **75% invisible**. A 9,458-line full-lifetime log buffer contains zero occurrences of either mechanism's own success log line. |
| 3 | **HIGH** | auth-core/org-core WIP | The large uncommitted fix for #2 (Postgres outbox + DB triggers + per-minute cron + revision-gated org-core reconciliation) is a sound design but **cannot function once deployed**: migration `012_auth_projection_guards.up.sql`'s RLS policies check GUC `app.current_org_id`, but `WithOrgScope` (the only code that sets this class of GUC) sets `app.current_org` (no `_id`). Every reconcile insert would fail its `WITH CHECK`, and the tombstone anti-resurrection check would silently false-negative instead of erroring. | `database.go:92` sets `app.current_org`; `012_auth_projection_guards.up.sql` checks `app.current_org_id`. Confirmed by direct file comparison; this is the same GUC-mismatch finding flagged in the 2026-07-02 baseline, still present in the new code. |
| 4 | **HIGH** | billing-core WIP / docker-compose.yml | Uncommitted `docker-compose.yml` wires auth-core's new `BILLING_CORE_URL` to `http://billing-core:3017` — **but billing-core listens on :3014**; :3017 belongs to session-core. Once deployed, auth-core's per-minute deletion-outbox flush will fail the billing-deactivate call, which (because it's awaited before the org-core reconcile-delete call in the same try block) also **blocks org-core's own deletion reconciliation** from ever running, in an infinite per-minute retry loop. | `docker-compose.yml` diff adds `BILLING_CORE_URL: http://billing-core:3017`; `docker exec billing-core-service netstat -tlnp` shows LISTEN only on :3014/:9091/:50013/:6062. |
| 5 | **HIGH** | org-core (pre-existing, still open) | The 2026-07-02 audit's P0 — "gateway invite/remove routes write Org Core directly, no verified acceptance/claim path" — is **untouched by any file in today's WIP**. velionv3 gateway's `members.rs` still calls org-core's compat routes directly, bypassing auth-core's own working Better Auth invite/accept flow. `AddPendingInvite` still records an invite with no signed token and no claim/accept endpoint. Once the new reconciliation path also ships, org-core will have **two parallel, unreconciled membership write paths**. | `apps/Frontend Plane/velionv3/apps/gateway/src/domains/orgs/members.rs:38-104`; `org-core/internal/http/handlers.go:360-401` (unchanged by the diff). |
| 6 | **HIGH** | user-core | `GET /api/v1/users/:id` trusts a **self-supplied `X-User-Role: admin` header verbatim** once the caller presents the fleet-shared internal API key — no independent verification. Same class of bug as the already-flagged `X-Org-ID` trust gap, now confirmed on `X-User-Role`. | Live curl with the shared internal key + a self-added `X-User-Role: admin` header returned a full user record (200) from a route that correctly 403s with no role header or a non-admin role. |
| 7 | **MEDIUM** | audit-core | `GET /v1/usage/summary` **500s unconditionally** for every org — invalid SQL (`ORDER BY` references the bare `cost_cents` column instead of `SUM(cost_cents)` in a query that `GROUP BY`s `plane,op`). Zero test coverage for this path, so `go test ./...` stays green. Breaks the Velion usage dashboard entirely. | Live curl reproduced 500; container log: `column "usage_events.cost_cents" must appear in the GROUP BY clause or be used in an aggregate function`. |
| 8 | **MEDIUM** | audit-core | `EXTRA_NATS_URLS` cross-plane aggregation (meant to close the Model Plane `tool_action` audit gap) is configured in env and code, but the **running container lacks the `inter-plane-bus` network attachment** its own already-committed `docker-compose.yml` declares. Fails soft (warn log), so nothing pages — `tool_action` events are silently not being aggregated right now. Fix is a container recreate, no code change needed. | `printenv` shows `EXTRA_NATS_URLS=nats://model-plane-nats-1:4222`; log: `dial tcp: lookup model-plane-nats-1 ... no such host`. `docker inspect` confirms the container is on `controlplane-net` only. |
| 9 | **MEDIUM** | user-core / org-core | Case-insensitive email lookup (flagged 2026-07-02) is still **not fixed on the actual lookup path**. Today's WIP only made the narrow `MarkOnboardingComplete` clause case-insensitive; `Repository.GetByEmail` (used by login, dedup, and every NATS event handler) remains `WHERE email = $1`. Live-reproduced: `local@velion.dev` resolves, `Local@Velion.Dev` / `LOCAL@VELION.DEV` both 404 on the same user. | `user-core/internal/users/repository.go:~144` vs `:~845`. |
| 10 | **MEDIUM** | user-core, org-core | No revision/tombstone/timestamp protection exists on any NATS event handler in `event_handler.go` — untouched by today's diff despite the org-membership work elsewhere in the same change set targeting exactly this failure mode. A delayed `member_added` arriving after `member_removed` silently re-creates a membership org-core already revoked. | Code review of `event_handler.go`; no sequence/timestamp comparison on any handler. |
| 11 | **LOW** | Application Plane (found via Control Plane doc review) | `convex/http.ts` calls `internal.nats.onOrganizationMemberRemoved`, but that mutation is **never defined** anywhere in `convex-core` — would throw at runtime if an `organization.member.removed` event/webhook ever fires. | Grep of `apps/Application Plane/convex-core/convex/*.ts` — only `onOrganizationCreated/Updated/Deleted` and `onOrganizationMemberAdded` exist. |
| 12 | **LOW** | auth-core | ESLint problems rose 649 → 682 since 2026-06-07 (partly new unformatted WIP code). Duplicate token-minting controllers (`model-plane-token.controller.ts`, `plane-token.controller.ts`) both still wired into `app.module.ts`. `canImplicitlyLinkProviderIdentity` is fully unit-tested but never called from any runtime path — dead code from the account-linking WIP. | `npx eslint 'src/**/*.ts'` → 682 problems; `app.module.ts` controllers array. |

## What's confirmed working (don't re-litigate these)

- All 6 services + Postgres + NATS + Dragonfly + full Lago stack: healthy, `docker ps` confirmed.
- `bash test-control-plane-integration.sh`: 9/9 pass (current, accurate).
- Full signup → email-verify (via a live Dragonfly OTP-retrieval trick) → signin → create-org round trip on auth-core works end-to-end.
- billing-core's internal-key-gated HTTP surface (account auto-provision, usage recording, quota reads, entitlement checks) works live, backed by a genuinely separate, live Lago usage-metering integration.
- audit-core's core ingest → store → query path works end-to-end with real historical data (Model Plane `tool_action` events, Application Plane `lead_export`).
- The `.unused`/`.backup`/`.DS_Store` residue files the 2026-06-07 register claimed deleted are confirmed actually gone.
- Nexi Checkout now has real test-mode credentials configured (`PAYMENT_PROVIDER=nexi`) — supersedes the earlier "awaiting vendor creds" memory note.
- `go build`/`go vet`/`go test` clean across all Go services checked; auth-core `pnpm build` clean.

## Deployment-state caveat (read this before treating any WIP finding as "in production")

**None of the six running containers include today's uncommitted diff.** Confirmed per-service via image `Created` timestamp vs. file mtimes, and via binary/string search (`strings /app/org-core | grep ProvisionOrganizationWithOwner` → empty; `grep -r organization_projection_outbox /app/dist` in auth-core → empty). None of migrations 010/011/012 (org-core, already committed) or 014/015 (auth-core, new/uncommitted) have been applied to the live database — org-core's `schema_migrations` tops out at `009_rls_enforce_tenant_isolation`. Findings #3 and #4 above are real bugs in code that will ship, not active production incidents — but #1, #2, #6, #7, #8, #9, #10 are live, present-tense problems in what's running right now.

## Legacy top-level doc review (9 docs)

| Doc | Verdict | Action taken |
|---|---|---|
| `README.md` | **delete** | Not edited (per instructions, delete/archive verdicts are report-only). Boilerplate from an unrelated older template; every checkable claim (ports, Redis-not-Dragonfly, a nonexistent `:3000` frontend, a nonexistent `./test-services.sh`, fictitious container names) is wrong. Fully superseded by `CONTROL_PLANE_DEEP_DIVE.md`. Added to `STALE_DOC_DELETION_REGISTER.md`. |
| `CONTROL_PLANE_TEST_FLOW.md` | **delete** | Not edited. Manual test walkthrough wrong on nearly every concrete detail (container names, ports, Redis→Dragonfly, route paths). Superseded by `test-control-plane-integration.sh` (9/9, current) + `CONTROL_PLANE_DEEP_DIVE.md`. Added to register. |
| `DEPLOYMENT_CHECKLIST.md` | **delete** | Not edited. Describes an early 3-service topology; omits billing-core/session-core/audit-core/Lago entirely; wrong ports throughout. Added to register. |
| `QUICK_REFERENCE.md` | **archive** | Not edited. Nothing in it is factually wrong, but it's a frozen Feb-19 "what we just built" changelog presented in a way that could pass for current guidance. `CONTROL_PLANE_OWNERSHIP.md` is the living replacement. Added to register. |
| `CONVEX_INTEGRATION_SUMMARY.md` | **archive** | Not edited. Feb-19 snapshot claiming "Status: Complete"; every port in its diagram is stale, its recommended test script is the confirmed-broken one, and it claims `onOrganizationMemberRemoved` is implemented when it isn't (see finding #11). Added to register. |
| `CONTROL_PLANE_DEEP_DIVE.md` | **update** | Edited in place: added a "Verified 2026-07-10" note; corrected 4 stale claims (`.unused`/`.backup` files described as present, user-core `DocumentAccessService` nil-publisher claim fixed by commit `596edfa4`, audit-core empty-key fail-open claim fixed by commit `42d7fed7`). Still the primary living reference for this plane. |
| `CONTROL_PLANE_OWNERSHIP.md` | **update** | Edited in place: fixed wrong org-core port in curl examples (8080 → 18080, explaining the Model Plane collision), fixed a self-contradictory GDPR-deletion example (pointed at a nonexistent auth-core route; corrected to the real user-core route), fixed two dead Convex reference links. |
| `ENHANCEMENT_SUMMARY.md` | **update** | Edited in place: removed a fictitious `NewServiceEnhanced` constructor from a code sample (real wiring is `orgcore.NewService(...)` + `SetSharedPublisher`/`SetAuditPublisher`), corrected the "not yet built" endpoint list (plan-update and GDPR-erase/soft-delete already exist under different paths; quotas/billing/compliance genuinely don't). |
| `ENVIRONMENT_FILES.md` | **update** | Edited in place: corrected stale `aquatiq-*-local` hostnames to real container names, replaced Redis references with Dragonfly, added billing-core/session-core/audit-core to the port-mapping table (previously only auth/user/org). |

Full per-claim evidence for every row above is in each doc's own edit and in the audit agents' transcripts; nothing here should be re-verified from scratch next pass — start from "what changed since 2026-07-10."

## Frontend note: velionv2 is not in scope, is deprecated, and is not part of Control Plane's authority surface

A separate, standalone `apps/Frontend Plane/velionv2` Next.js process was found running on host port 3000 (PID 17071, started 2026-07-09 evening) during this session — unrelated to Control Plane's own stack, but flagged here because it was the source of a confusing chat transcript. No in-repo reverse-proxy/nginx config routes external traffic to it, so it reads as a local dev/test process, not confirmed production traffic. Per user direction (2026-07-10): **velionv2 is deprecated, v3 is canonical, and v2 should be removed from docs going forward** — this is being tracked as a cross-cutting doc-hygiene item, not a Control Plane finding. Its chat composer independently has a live bug (`"Velion Reasoner"`/`"GPT-4.1"` map to `model: undefined`) but no further action is planned here since the surface is deprecated.

## Commands run (this pass, in addition to the earlier same-day smoke pass)

| Command / check | Result |
|---|---|
| `bash test-control-plane-integration.sh` | 9/9 pass |
| `bash test-all-services.sh` | **Confirmed stale/broken** — checks a nonexistent `controlplane-redis` container, runs a `nats` CLI not installed in the NATS container, and its signin-after-signup test doesn't account for `REQUIRE_EMAIL_VERIFICATION=true` (by design, not a bug). Needs a rewrite or retirement in favor of the integration script above. |
| Full signup→verify→signin→create-org flow (auth-core), live, via Dragonfly OTP retrieval | Works end-to-end; surfaced finding #2 |
| Forged-bearer session impersonation attempt (session-core) | **Succeeded** — finding #1 |
| `go build`/`go vet`/`go test ./...` (org-core, user-core, billing-core, session-core, audit-core) | Clean/pass |
| `npx eslint 'src/**/*.ts'` (auth-core) | 682 problems (641 errors, 41 warnings), up from 649 |
| Cross-plane row-count comparison (`auth_service.organization` vs `org_core.organizations`) | 4 vs 1 — finding #2 |
| RLS GUC-name static comparison (`database.go` vs migration `012`) | Mismatch confirmed — finding #3 |
| `netstat` inside billing-core-service vs. new `docker-compose.yml` env | Port mismatch confirmed — finding #4 |

## Quality gate

- Go service tests: pass for all 5 checked services (org, user, billing, session, audit).
- auth-core build: pass. auth-core Jest: pass (checked suite).
- Go format/vet: vet clean; `gofmt -l` drift previously noted, not re-run this pass.
- auth-core ESLint/Knip: fail (682 problems; knip backlog not re-run this pass, assume still present).
- Live Control smoke (`test-control-plane-integration.sh`): pass, 9/9.
- Live security test (session-core impersonation): **fail — critical bypass confirmed live**.
- Live cross-plane consistency (auth-core org → org-core org): **fail — 75% data loss confirmed live**.
- Coverage gates: not run this pass.

## Recommended remediation order

See `CONTROL_PLANE_ROADMAP.md` for the phased plan. In short:

1. **Immediately**: fix session-core's bearer-token validation (finding #1) — this is a live, exploitable full-impersonation bypass against the exact endpoint velionv3 uses for session bootstrap.
2. Before committing/deploying the in-flight org-membership WIP: fix the GUC name mismatch (#3) and the billing-core port (#4) — as committed today, the fix would silently fail and/or deadlock its own retry loop.
3. Land the WIP once #3/#4 are fixed, then apply migrations 010/011/012 (org-core) and 014/015 (auth-core) to the live database and rebuild all four affected containers (auth-core, org-core, user-core, billing-core).
4. Close the still-open P0 (#5): route velionv3 gateway's invite/remove-member calls through auth-core's Better Auth flow instead of org-core's direct compat routes, and add a signed claim/accept token to `AddPendingInvite`.
5. Fix user-core's `X-User-Role` trust gap (#6) — likely the same fix shape as whatever closes the pre-existing `X-Org-ID` trust gap; do both together.
6. Fix audit-core's `usage/summary` SQL (#7) and recreate the audit-core container to pick up its already-committed `inter-plane-bus` network attachment (#8).
7. Finish case-insensitive email lookup on the actual `GetByEmail` path (#9), and add revision/timestamp guards to user-core/org-core's NATS event handlers (#10).
8. Define the missing `onOrganizationMemberRemoved` Convex mutation (#11) — small, isolated fix in Application Plane.
9. Mechanical cleanup: `gofmt`, auth-core ESLint/Knip backlog, dedupe token controllers, delete dead `canImplicitlyLinkProviderIdentity`.

---

## Appendix: 2026-07-02 baseline (superseded by the above, kept for history)

### Original commands run

| Command | Result | Notes |
|---|---|---|
| `go test ./...` in `audit-core` | Pass | Checked service tests passed. |
| `go test ./...` in `billing-core` | Pass | Checked service tests passed. |
| `go test ./...` in `org-core` | Pass | Checked service tests passed. |
| `go test ./...` in `session-core` | Pass | Checked service tests passed. |
| `go test ./...` in `user-core` | Pass | Checked service tests passed. |
| `pnpm exec jest --runInBand` in `auth-core` | Pass | 1 suite, 4 tests passed. |

### Original static-scan addendum

| Command | Result | Notes |
|---|---|---|
| `gofmt -l audit-core billing-core org-core session-core user-core` | Fail | Formatting drift across all 5 Go services. |
| Go vet | Pass | All 5 Go services pass vet. |
| `staticcheck` | Blocked | Go 1.25-built binary can't analyze Go 1.26 source. |
| `pnpm exec eslint` in `auth-core` | Fail | 649 problems (617 errors, 32 warnings). |
| `pnpm build` in `auth-core` | Pass | |
| `npx -y knip` in `auth-core` | Fail | 11 unused files, 12 unused deps, 8 unused devDeps, 30 unused exports, 75 unused exported types. |

The original P0/P1/P2/P3 findings table from this baseline is fully superseded by the "Top findings, by severity" table above — every item in it was re-verified this pass and either confirmed still-open (folded into findings #2, #5, #6, #9, #12) or found fixed (noted inline above).
