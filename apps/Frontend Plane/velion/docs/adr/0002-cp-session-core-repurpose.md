# ADR 0002 — Repurpose Control Plane `session-core` from agent-run state to user/org/billing session coordinator

- **Status**: accepted
- **Date**: 2026-05-09
- **Closes**: verevon-gap.md G10 (decision); implementation tracked separately
- **Supersedes**: none
- **Owners**: Control Plane (auth-core, user-core, org-core, billing-core, session-core), Application Plane (convex-core, notification-core), Frontend Plane (verevon)

---

## Context

Two different services in the CoreSystem pyramid are both called "session-core":

| Service | Path | Lang | Today's scope |
|---|---|---|---|
| Control Plane `session-core` | `apps/Control Plane/session-core` | Go | Plans, todos, lineage, approvals, session events for AI agent runs (`session_repository`, `plan_repository`, `todo_repository`, `lineage_repository`, `approval_repository`). Pubs to NATS, mirrors to Convex. |
| Model Plane `session-core` | `apps/Model Plane/rust/services/session-core` | Rust | "Authoritative state for thread timeline, run metadata, checkpoints, and context assembly" (per `main.rs` header). Plans + tasks + orchestration + compaction. |

The two services overlap heavily. CP `session-core` was built when the Model
Plane v2 didn't exist yet. Now that Model Plane v2 owns thread/run/checkpoint
state authoritatively, CP `session-core`'s plan/todo/lineage repos are dead
weight: schema duplication, NATS subject overlap, and operational confusion
(`docker compose logs session-core` requires the operator to remember which
plane they care about).

Meanwhile, verevon has a recurring need for a single endpoint that returns the
full **app-context snapshot** for the calling user — identity + active org +
entitlements + billing state + onboarding step — so the front-end can route
post-login, gate features by plan, render quota banners, and subscribe
reactively. Today this requires 3-4 separate calls to user-core / org-core /
billing-core, with no caching layer above the cores. We've patched the
post-login routing path with G18 (`/api/user/me/session-context`), but that
endpoint lives in user-core and only knows about user + primary org — no
billing, no entitlement detail, no NATS-driven invalidation.

This ADR commits CP `session-core` to a new role: the **Control Session
coordinator**. The agent-run responsibilities move entirely to Model Plane
`session-core` (the Rust service).

## Options considered

### Option A — Repurpose CP session-core to the app-context coordinator

CP session-core becomes the canonical **Control Session** authority:

- Aggregates `auth-core` user → `user-core` profile → `org-core` active org
  + entitlements + quotas → `billing-core` subscription state into one
  snapshot.
- Caches snapshots in `application-redis` keyed by `(user_id, org_id)`, TTL
  ~30s, invalidated on NATS events from any of the four authorities.
- Mirrors snapshots into Convex (`Application Plane/convex-core`) so verevon
  can subscribe reactively for plan gating and quota banners.
- Emits `app.session.upserted`, `app.session.org_switched`,
  `app.session.entitlements_changed`, `app.session.quota_warning` for
  notification-core to deliver toasts/emails.
- Drops all agent-run repos. Plan/todo/lineage/approval responsibility
  migrates to Model Plane `session-core`.

**Pros**:
- Removes the dual-ownership confusion (one service, one purpose).
- Gives verevon a single endpoint (`GET /api/v1/sessions/current`) to
  replace today's 3-call fan-out.
- NATS subject space gets cleaner: `session.*` becomes Model Plane only;
  `app.session.*` is Control Plane only.
- Convex mirror makes reactive UI cheap (no polling).

**Cons**:
- Rename is risky if consumers cache the service name. Mitigated by
  feature-flag rollout.
- Migrating agent-run repos means schema move (`plan`, `todo`, `lineage`,
  `approval` tables go from `session-core` Postgres to Model Plane
  Postgres). Requires a one-time data backfill or — preferred — a
  cutover where new sessions write only to Model Plane and old sessions
  drain from CP.

### Option B — Keep CP session-core as the agent-run authority; build a new "control-session" service

CP session-core stays as-is. A new service (`control-session`?) handles
the user/org/billing aggregate.

**Pros**:
- No data migration for agent-run repos.
- Lower-risk for in-flight agent runs.

**Cons**:
- Adds another microservice to operate (port, env vars, image, healthcheck).
- Doesn't resolve the "two services named session-core" cognitive cost —
  would actually add a third name (`control-session` vs Model Plane
  `session-core` vs CP `session-core`).
- Violates the existing pyramid principle that each plane has one
  authority per domain.

### Option C — Merge agent-run state into Model Plane session-core directly; delete CP session-core entirely

CP session-core is removed. Verevon calls Model Plane session-core for
agent-run data, and the user/org/billing aggregate lives in user-core
(extended with a session-context endpoint that already exists).

**Pros**:
- Smallest service count.
- No new aggregator to maintain.

**Cons**:
- Aggregation lives in user-core, which violates the Control Plane
  authority rules (user-core would need to read org-core and billing-core
  state to assemble snapshots). Inverted dependency.
- No place for the Convex projection — convex-core would have to subscribe
  to four separate NATS streams instead of one.
- Notification-core has no single subject to listen on for "Control
  Session changed".

## Decision

**Choose Option A**: repurpose CP `session-core` as the Control Session
coordinator. Migrate agent-run repos to Model Plane session-core.

Rationale:
- Fixes the naming confusion at the root.
- Centralises the user/org/billing aggregate in one well-placed service
  (Layer 1 of the pyramid, alongside the four authorities it aggregates).
- Gives verevon + convex-core + notification-core a clean dependency
  surface (one HTTP endpoint, one NATS subject space, one Convex
  projection).
- Eliminates schema duplication between CP and Model Plane.

## Consequences

**Wins**:
- Verevon's `needsOnboarding()` + active-org resolution + plan gating + quota
  banners all source from one place. Post-login routing becomes a single
  fetch.
- NATS subject ownership becomes unambiguous: `session.*` belongs to Model
  Plane v2; `app.session.*` belongs to Control Plane.
- Convex projection schema can be derived from one canonical type, not
  composed from four sources.
- Future features (org-switch UX, plan-upgrade banners, audit timelines)
  have one obvious place to live.

**Costs**:
- One-time migration of `plan`, `todo`, `lineage`, `approval` tables from
  CP session-core Postgres into Model Plane Postgres. Plus or minus the
  rename of any existing NATS subjects in flight.
- All consumers of CP session-core's agent-run endpoints must be
  re-pointed at Model Plane session-core. Grep-able list:
  `session-core-service:3017/v1/{plans,todos,lineage,sessions/.../approvals}`.
- Verevon's existing usage of `/me/session-context` (chat session-store,
  active-org helper, profile hooks) must migrate to the new
  `/api/v1/sessions/current` once it lands. Backwards-compat is a 1-line
  forward in user-core during the transition.

**Risk we're accepting**:
- Brief period of dual-write while we cut over. Default to writing both
  places, reading from the new place; flip read first, then write second.

## Implementation plan

Tracked as a multi-PR migration. **Order matters** — each step must be
green before the next.

1. **Stand up the new service surface** behind a feature flag:
   - `cp-session-core` (renamed, new port `:3013` per verevon-gap.md §2.2)
   - HTTP routes:
     - `GET /api/v1/sessions/current` (uses X-User-Id from internal proxy)
     - `POST /api/v1/sessions/refresh` (cache-bust hook for plan upgrade,
       org switch)
     - `POST /api/v1/sessions/switch-org`
     - `GET /api/v1/sessions/:userId/active-org`
   - Backed by Redis cache keyed by `(user_id, org_id)`, 30s TTL, NATS
     subscribers invalidate on upstream events.
   - Internal-key middleware (same pattern as user/org/billing-core post-G15).
   - Correlation-id middleware (same pattern as G15).

2. **Subscribe to upstream NATS subjects**:
   - `user.created`, `user.profile.updated` (auth-core / user-core)
   - `organization.created`, `organization.member.added`,
     `organization.plan.changed`, `organization.feature.enabled`
     (auth-core / org-core)
   - `billing.account_updated`, `billing.quota_exceeded`,
     `billing.invoice_created`, `billing.plan_changed` (billing-core)

3. **Publish the new aggregate subjects**:
   - `app.session.upserted`
   - `app.session.org_switched`
   - `app.session.entitlements_changed`
   - `app.session.quota_warning`

4. **Wire convex-core** to subscribe to `app.session.*` and project the
   snapshot into the `controlSessions` table for reactive consumption.

5. **Wire notification-core** to subscribe to
   `app.session.entitlements_changed` and `app.session.quota_warning` for
   email + Convex toast delivery.

6. **Migrate verevon** server routes that need plan/quota context to
   `/api/v1/sessions/current`. Existing `/api/user/me/session-context`
   stays as a deprecated alias for one release cycle.

7. **Move agent-run repos** to Model Plane:
   - `plan_repository.go`, `todo_repository.go`, `lineage_repository.go`,
     `approval_repository.go`, `session_repository.go` → ported to Rust
     under `apps/Model Plane/rust/services/session-core/src/`.
   - Schema: copy DDL from CP session-core migrations into Model Plane
     migrations; backfill in-flight rows with a one-time job.
   - All `internalnats.SharedPublisher.Publish*` calls in CP session-core
     for agent events get rewritten to publish from Model Plane.

8. **Decommission** the old CP session-core agent-run code:
   - Delete the four repository files plus their HTTP handlers.
   - Drop the deprecated tables after one release cycle.
   - Remove `session-core-service:3017/v1/{plans,todos,lineage,...}`
     from any docker-compose env defaults.

9. **Documentation**:
   - Update `apps/Control Plane/CONTROL_PLANE_ARCHITECTURE.md` (currently
     says session-core owns plans/todos/lineage — that becomes Model Plane).
   - Update `verevon-gap.md` §2 (Tri-Plane Session Model) — remove the "(planned repurpose)" qualifier.

## Implementation notes

- **Naming**: keep the directory name `session-core` for the repurposed
  service. Do not rename. The path is what people grep for, and renaming
  breaks too many env defaults across the monorepo. The semantic meaning
  shifts; the path name stays.
- **Port**: `:3013` is the proposed new HTTP port per verevon-gap.md §2.2.
  Today CP session-core listens on `:3017`. Either renumber or keep `:3017`
  for the new service (less churn). Recommend keeping `:3017`.
- **Feature flag**: `CONTROL_SESSION_AUTHORITY_ENABLED=true` env on verevon
  + the new service. Off → verevon uses the legacy `/me/session-context`
  path; on → verevon uses `/api/v1/sessions/current`.
- **Don't break correlation IDs**: every new endpoint must carry the
  `correlationMiddleware` from G15 so traces continue working.
- **Test gate**: 80% coverage on the new aggregator; the legacy fallback
  must remain functional during cutover (CI runs both modes).

## References

- `verevon-gap.md` §2.2 Repurposing CP `session-core`
- `verevon-gap.md` G10 entry
- `apps/Control Plane/CONTROL_PLANE_ARCHITECTURE.md`
- `apps/Application Plane/APPLICATION_PLANE_ARCHITECTURE.md`
- `apps/Model Plane/rust/services/session-core/src/main.rs` (canonical
  agent-run authority)
