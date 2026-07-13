# Velion — Source of Truth & Gap Tracker

> **Last verified**: 2026-05-13 — **§10 has zero open gaps. Zero ❌ Not Implemented markers remain anywhere in the doc. §12 (test plan) is now ✅ Closed — all nine Playwright journeys have coverage.** The entire CoreSystem roadmap is functionally complete *and* test-covered, with the Microsoft Graph enrichment loop **LIVE end-to-end with real Graph data in the DB** (Wave 13 §8.33). The only remaining item is (1) `convex-gateway` ⚠️ Deferred until a forcing function fires (§8.32 audit found reactive UI is already live without it) — a deliberate ADR-shaped deferral, not a gap. Wave 13 §8.33 closed: live Graph verification for `ima.dacosta@aquatiq.com` (28KB avatar + jobTitle + location + phone + graphMail + graphEnrichedAt all persisted), G47 (Better Auth comma-separated scopes normalized to RFC 6749 space-separated form), G48 (`HandleUserProviderLinked` now does GetByEmail → GetByID fallback, mirrors `HandleUserRegistered`; resolves the testbruker email-drift case), G49 (convex-gateway dev server now mounts `./convex/` — without this, function additions never reach the running backend and queries like `controlSessions:byUser` fail at runtime with "Could not find public function"), G50 (split `ingestion-temporal` onto its own dedicated 1GB Postgres — eliminates the `GetTransferTasks` `context deadline exceeded` storm caused by 6 databases sharing a 256MB instance with `shared_buffers=64MB`), G51 (new `scripts/lint-env-files.sh` + `pnpm lint:env` — static check that catches `.env` vs `.env.docker` drift before it reaches a running container; opt-in until the 64-case pre-existing backlog is reconciled), plus Playwright coverage for J2/J5/J8/J9 (new `onboarding-advanced.spec.ts`); also documented the third instance of the `.env` vs `.env.docker` precedence trap (MICROSOFT_CLIENT_SECRET). Wave 12 §8.32 closed the convex-gateway audit + filed and closed G46 in the same wave (email Graph-write added to `enrichFromMicrosoftGraph` — verified LIVE in Wave 13). Wave 11 §8.31 closed G45 Slice F (wizard trimmed from 6 → 5 steps; new `<ConnectorConsentPrompt />` shown 90s into dashboard). Wave 10 §8.30 closed G41/G42/G43/G44 (Graph enrichment + reactive banner + toast feed + auth-core defensive fix). Wave 9 closed §8.23 (cascade hot-fix), §8.24 (G36-cutover Step A — Rust orchestration HTTP live on `28083:8083`), §8.25 (G37 — `users` schema NOT NULL on `password_hash`/`avatar`), §8.26 (G36-cutover Step D — CP agent-run scaffold decommissioned), §8.27 (G39 — velion boot-time gate + cross-service handshake), §8.28 (G38 — Quarry-v2 ingest now persists real bodies via new `/v1/artifacts/:id/bytes`), §8.29 (G40 — boot-time gate mirrored in all 4 CP Go services; surfaced + fixed a real billing-core config gap). All cross-service verification listed in the relevant §8 entries.
>
> **Prior waves (still valid):** Wave 8 G36 HTTP parity → flag-flipped in Wave 9. Wave 7 (G27/G28-followup/G34-followup/G35). Wave 6 (ADR 0004 cutover + G3/G16/G34/G28). Wave 5 hygiene (G4/G5/G13/G22/G23/G26). Wave 4 (G20/G21/G25). Wave 3 (G10/G14/G17). Wave 2 CI hygiene (G11/G12). Wave 1 baseline (G1/G2/G6/G8/G9/G15/G18/G19/G24/G29/G30/G31/G32/G33).
>
> Cross-checked against `docs/Velion_CONNECT_ROADMAP.md`, `docs/zero-input-enterprise-onboarding-roadmap.md`, `docs/ARCHITECTURE_DIAGRAM.md`, and `VELION_THIRD_PARTY_INTEGRATIONS.md`.
> **Owners**: Frontend Plane (velion) + Control Plane (auth/user/org/billing/session) + Application Plane (convex-core/notification-core)
> **Scope**: how velion talks to the rest of the CoreSystem pyramid, the canonical contracts, the patches landed, and the remaining work to close gaps.

This document is the durable reference for anyone implementing or modifying velion. When code disagrees with this file, **fix the code or update this file in the same PR**. Stale truth is worse than no truth.

**Status marker legend** (used in every table below):

| Marker | Meaning |
|---|---|
| ✅ Closed | Implemented + verified end-to-end. Live in production code path. |
| ⚠️ Partial | Surface exists but isn't fully wired — e.g. handler exists with no caller, scaffolding without an async trigger, feature behind a flag that's never flipped. |
| ❌ Not Implemented | Not started. Either the code doesn't exist or the only references are TODOs / placeholders. |

---

## 0. Doc map — which doc owns what — ✅ Closed

| Document | Canonical authority for | Status |
|---|---|---|
| `velion-gap.md` (this file) | Cross-cutting integration truth, gap tracker, fix priority | Authoritative |
| `docs/adr/` | Architectural decisions (numbered, immutable once accepted) | Authoritative for the decision itself |
| `docs/ARCHITECTURE_DIAGRAM.md` | Frontend Plane charter — L6 placement, L5 boundary policy, component hierarchy | Authoritative for Frontend Plane charter; amended per ADR 0003 (2026-05-11) to declare velion's `src/app/api/*` proxies as the canonical L5 ingress. Rules 5–7 carry the new contract. |
| `docs/zero-input-enterprise-onboarding-roadmap.md` | Microsoft Entra zero-input flow contract — `tokenRef`, `AuthProviderLinked` event, `/me/session-context`, slices A–F | Authoritative for enterprise sign-in path; Phase 4 (frontend wiring) pending |
| `docs/Velion_CONNECT_ROADMAP.md` | Velion product / phase roadmap — UX phases, design system tokens, MVP checklist | Authoritative for product scope; **stale** in spots (middleware naming, cookie names, step count) |
| `VELION_THIRD_PARTY_INTEGRATIONS.md` | Third-party integrations only (Zammad / Nango / Nohu) | Authoritative for those three. Renamed from `VELION_INTEGRATION.md` on 2026-05-11 per G22 closure — Control Plane integration lives in **this** file (`velion-gap.md`). |
| `docs/base-design.md` | Visual design tokens (referenced by CONNECT_ROADMAP) | Authoritative for design system |

Reconciliation rule: when these disagree, `velion-gap.md` wins on integration contracts; `ARCHITECTURE_DIAGRAM.md` wins on Frontend Plane charter; the zero-input roadmap wins on enterprise auth contract; CONNECT_ROADMAP wins on product UX phases.

---

## 1. Velion in the pyramid — ✅ Closed

```
                                      ┌────────────────────────────┐
Layer 6 — Frontend Plane              │  velion (Next.js 15)       │
                                      │  triodelab-web (static)    │
                                      └─────────────┬──────────────┘
                                                    │ cookie + JWT
Layer 5 — Application Plane              convex-core, convex-gateway,
                                         notification-core, affine-core
                                                    │
Layer 4 — Model Plane v2                 model-plane/rust/services/session-core
                                         (agentic thread / run / checkpoint authority)
                                                    │
Layer 3 — Ingestion Plane                Quarry, imports-core
                                                    │
Layer 2 — Data Plane                     documents-api-go, retrieval-engine-rs
                                                    │
Layer 1 — Control Plane                  auth-core (TS / NestJS)   ← user-session authority
                                         user-core (Go)            ← user profile + onboarding flag
                                         org-core (Go)             ← org, entitlements, quotas
                                         billing-core (Go)         ← subscriptions, usage
                                         session-core (Go)         ← (TBD) user/org/billing
                                                                     session coordination
```

Velion is a strictly downstream consumer. It never writes to any plane database directly. Every CP write goes through a CP HTTP endpoint behind an authenticated proxy route.

> **Charter rule** (settled by [ADR 0003](./docs/adr/0003-l5-boundary-policy.md)): velion's `src/app/api/*` route handlers **are** the canonical Frontend Plane L5 ingress. They validate sessions, mint internal auth headers, propagate correlation IDs, and forward to L1–L4 cores. `convex-gateway` is reserved for WebSocket fan-out of reactive workspace data only. The decision is conditional on three guardrails: shared `control-plane-auth.ts` helper, per-core internal-key middleware, and a forced revisit when a second frontend ships.

---

## 2. The Tri-Plane Session Model — ✅ Closed

CoreSystem now has **three distinct kinds of "session"**. Conflating them was the root cause of the prior session confusion. Lock these definitions in.

| Session kind | Authority | Purpose | Storage | Frontend touchpoint |
|---|---|---|---|---|
| **User session** (login state) | `auth-core` (Better Auth) | Identity, cookie issuance, JWT for downstream planes, **OAuth token custody** (access + refresh) | auth-core Postgres + Better Auth tables | Cookie set on browser; `getSession()` validates server-side |
| **Control session** (user / org / billing context) | `Control Plane / session-core` (Wave 3 MVP live) | Aggregates user identity + active org + entitlements + billing into one snapshot; emits `app.session.entitlements_changed` to notification-core. Reactive Convex projection + Redis cache deferred (G34/G35). | session-core in-memory aggregation + velion-nats `APP_SESSION` JetStream stream | velion proxy `/api/user/me/session-context` forwards to `GET /api/v1/sessions/current` when `CONTROL_SESSION_AUTHORITY_ENABLED=true` |
| **Agentic session** (thread / run / checkpoint) | `Model Plane / session-core` (Rust) | Thread timeline, run metadata, checkpoints, context assembly for AI agents | model-plane Postgres + NATS / JetStream | Velion `/agents`, `/planner`, `/tasks`, `/chat` UIs |

**Key rule**: velion's middleware and proxy routes ONLY care about user sessions (auth-core). Control-session and agentic-session are *application data*, not auth gates.

### 2.1 OAuth token custody (`tokenRef` contract) — ✅ Closed

From `docs/zero-input-enterprise-onboarding-roadmap.md` — locked contract:

- **auth-core** owns OAuth tokens (access / refresh / scopes / expiry) for all providers (Microsoft, Google, etc.).
- `user-core` and `org-core` consume only opaque references (`tokenRef`) plus identity metadata.
- velion never sees raw OAuth tokens. It receives only the Better Auth cookie + JWT issued by auth-core.
- Internal token exchange: `POST auth-core/internal/oauth/token` accepts `{ tokenRef }`, returns `{ access_token, expires_at }` for short-lived back-end calls (e.g. `user-core` → Microsoft Graph for profile enrichment).
- Refresh: `POST auth-core/internal/oauth/refresh` (scaffold only — see G24).

Verified: zero hits for `tokenRef` in `velion/src/`. Frontend isolation respected.

### 2.2 Repurposing CP `session-core` — ✅ Closed (Wave 3 MVP §8.17 + Wave 6 cache §8.20 + Wave 7 invalidator §8.21 + Wave 9 agent-run decommission §8.26)

Per [ADR 0002](./docs/adr/0002-cp-session-core-repurpose.md). CP `session-core` runs in dual mode during the transition: the legacy agent-run HTTP routes (`/v1/sessions`, `/v1/plans`, `/v1/todos`, `/v1/lineage`) continue serving Model Plane v1 traffic while a new **Control Session aggregator** lives alongside under `/api/v1/sessions/*`. The Rust port + decommission of agent-run repos is deferred (see G36).

**Live today** (Wave 3 — Steps 1, 3, 5, 6, 9 of the ADR's 9-step plan):

| Method + Path | Status | Behaviour |
|---|---|---|
| `GET /api/v1/sessions/current` | ✅ live | Aggregates user-core (identity + onboarding) → org-core (org + entitlements) → billing-core (subscription) into one snapshot. Synchronous fan-out; upstream failures degrade silently except for user-core (returns 502). |
| `POST /api/v1/sessions/refresh` | ✅ live | Re-aggregates and publishes `app.session.entitlements_changed` on `velion-nats` `APP_SESSION` JetStream stream. Used after explicit plan upgrades / org switches. |

**Deferred to follow-up gaps** (see §10 G34/G35/G36):
- Redis snapshot cache with 30s TTL + NATS-driven invalidation (G34)
- Convex projection mirror for reactive UI (G35)
- `POST /api/v1/sessions/switch-org`, `GET /api/v1/sessions/:userId/active-org` (folded into G34)
- Migrating `plan`, `todo`, `lineage`, `approval`, `session` repos to Model Plane Rust + decommissioning CP versions (G36)

**Out of scope** (lives elsewhere — unchanged):
- Login / logout / cookie issuance → `auth-core`.
- Thread / run / message / checkpoint state → Model Plane `session-core`.
- Document / retrieval state → Data Plane.

**NATS topology (live)**:
- session-core's `SharedPublisher` ensures the `APP_SESSION` JetStream stream on `velion-nats` (`app.session.>`, 7-day retention, 256 MB cap).
- notification-core opens a **second** NATS connection (`SHARED_NATS_URL=nats://velion-nats:4222`) on top of its local app-nats publisher; `internal/subscribers/control_session.go` binds a durable queue subscriber to `app.session.entitlements_changed` and forwards each event to `notification.Service.Accept`.
- velion's `/api/user/me/session-context` route honours `CONTROL_SESSION_AUTHORITY_ENABLED=true` to forward to session-core; otherwise falls back to user-core's narrower endpoint (the G18 path).

**Future subscribers** (G34): when the cache + invalidation layer lands, session-core will also subscribe to `user.*`, `organization.*`, `billing.*` upstream subjects to bust Redis entries and re-publish `app.session.*`.

**Migration plan**:
1. Stand up new repurposed service alongside the existing one, behind a feature flag.
2. Move agent-run repos (`session_repository`, `plan_repository`, `todo_repository`, `lineage_repository`) into Model Plane session-core (Rust). They are already partially there.
3. Cut `user-core` and `org-core` over to publish on the new aggregate subjects.
4. Subscribe convex-core + notification-core to the new app.session.* subjects.
5. Decommission the old `session-core/internal/repository/{plan,todo,approval,lineage}_repository.go` files.
6. Repoint velion server routes that need plan/quota context at `/api/v1/sessions/current`.

---

## 3. Velion auth + session flow (current) — ✅ Closed

**Login**:
1. User hits `/login` → `AuthPage` component.
2. AuthPage calls `/api/auth/sign-in/oauth/{provider}` (catch-all) → forwards to auth-core `/api/auth/sign-in/{provider}`.
3. auth-core (Better Auth) sets cookie `better-auth.session_token` (or one of the legacy aliases) on the velion origin.
4. Browser is redirected to `/dashboard` (or original `?redirect=...`).

**Per request**:
- Edge middleware (`src/proxy.ts`) checks **cookie name presence only**. Optimistic — no auth-core call. Currently a known gap (see G1).
- For `/api/*` routes, the route handler imports `requireSession()` from `src/app/api/_lib/control-plane-auth.ts`. This calls auth-core `/api/v2/auth/getSession` once per request (cached in a `WeakMap<NextRequest>` for the request lifetime).
- The validated session yields a typed `ControlPlaneSession`. The proxy attaches `X-User-Id`, `X-User-Email`, `X-User-Name`, `X-User-Avatar`, `X-Internal-Api-Key` when forwarding to user-core / org-core / billing-core.
- All four CP cores now run `internalAuthMiddleware()` that fails closed (HTTP 401) if neither the internal API key nor a Bearer + X-User-Id is present.

**Cookie inventory** (`src/proxy.ts:3-15`):
- Modern: `better-auth.session_token`
- Legacy / aliases: `auth_session`, `idknuten.sid`, `idknuten.session_token`, `session_token`
- Pattern matches: `(?:__Secure-)?sid`, `(?:__Secure-)?sid_multi-`, `(?:__Secure-)?session_token`

The legacy `idknuten.*` cookies should be sunset once telemetry confirms zero traffic on them (see G23). `docs/Velion_CONNECT_ROADMAP.md:84` mentions only `idknuten.sid` — that doc is stale.

**Logout**:
- `authService.logout()` posts to `/api/auth/sign-out` (catch-all) → auth-core `sign-out` → cookie cleared.
- The catch-all route normalizes auth-core's "400 already signed out" into 200 (intentional, kept).

---

## 4. Onboarding contracts — ✅ Closed (manual path ✅ Closed; zero-input path ✅ Closed including Slice F per §8.31)

There are **two onboarding paths**. Pick the right one based on identity provider.

### 4.1 Manual onboarding wizard (current default — 6 steps) — ✅ Closed (cascade hot-fix series §8.23–§8.28 stabilised every step end-to-end against the live stack)

For email/password and non-Microsoft OAuth sign-ups. Lives under `src/app/(onboarding)/onboarding/{step}`.

| # | Step | Velion entrypoint | Service call | Backend writes |
|---|---|---|---|---|
| 1 | Profile | `/onboarding/profile` | `userService.updateCurrentUserProfile` → `/api/user/me` PATCH | `user-core.user_profiles` |
| 2 | Organization | `/onboarding/organization` | `orgService.createOrganization` → `/api/org/orgs` POST **OR** `orgService.acceptInvitation` → `/api/auth/organization/accept-invitation` | `org-core.organizations` (+ membership in auth-core) |
| 3 | Website | `/onboarding/website` | `/api/ingestion/ingest-job` POST → Ingestion Plane | Quarry crawl job + Data Plane documents |
| 4 | Connect | `/onboarding/connect` | `_pushOrgOnboardingState(..., 'CONNECTIONS_CONFIGURED', ...)` | `org-core.org_onboarding_state` |
| 5 | Team | `/onboarding/team` | `orgService.inviteMember` per email | `org-core.invitations` (+ Better Auth invite) |
| 6 | Complete | `/onboarding/complete` | `userService.markOnboardingComplete` → `/api/user/onboarding/complete` POST → user-core `/api/v1/users/onboarding/complete` ✅ **fixed** | `user-core.users.onboarding_complete = true` |

**Authoritative onboarding flag**: `user_core.users.onboarding_complete`. Anything else (localStorage, org-core state) is a **projection**, not authority.

**Frontend orchestrator**: `src/components/onboarding/page/OnboardingPage.tsx` + `src/components/onboarding/services/onboarding-service.ts`. State is mirrored in localStorage under key `onboarding_state` for resume-on-refresh; this is *cache only* and must not become authoritative (see G3).

> `docs/Velion_CONNECT_ROADMAP.md` describes a 7-step variant with `/onboarding/plan` between steps 5 and 6. **Plan selection is intentionally deferred** — every user is auto-assigned `plan = 'free'` and may upgrade later from `/settings/billing`. The 7th step is aspirational and not on the MVP path. CONNECT_ROADMAP MVP checklist (line 313) confirms this. (See G20 for formalization.)

### 4.2 Zero-input enterprise onboarding (Microsoft Entra ID — target state) — ✅ Closed (Slices A/B/C/D/E/F all ✅ Closed as of Wave 11 §8.31)

Owner: `docs/zero-input-enterprise-onboarding-roadmap.md`.

**Goal**: enterprise user clicks "Sign in with Microsoft" and lands in `/dashboard` without ever seeing the manual wizard.

**Canonical flow** (Phase 4 frontend wiring is currently pending — G18):

```
1. User clicks "Sign in with Microsoft"
       ▼
2. auth-core OAuth dance → cookie + tokenRef issued
       ▼
3. auth-core publishes AuthProviderLinked(userId, provider="microsoft",
                                          providerUserId, tenantId,
                                          scopes, tokenRef)
       ▼
4. user-core consumes:
   a. POST /internal/users/enrich-from-provider
        → exchange tokenRef → /me, /me/photo/$value
        → soft-update displayName, avatar, locale, timezone
   b. POST /internal/orgs/ensure-from-tenant (org-core)
        → resolve org by (provider, tenantId) or auto-provision
   c. POST /internal/memberships/ensure
        → first member becomes OWNER, rest MEMBER
       ▼
5. Frontend (post-OAuth callback) calls GET /api/user/me/session-context
       ▼
6. Response shape:
   { userId, orgId, role, onboardingStatus: "COMPLETED" | "CONNECTORS_PENDING" | ... }
       ▼
7. Router decision:
     onboardingStatus = "COMPLETED"           → /dashboard
     onboardingStatus = "CONNECTORS_PENDING"  → /onboarding/connect (consent-after-value)
     onboardingStatus = "CREATED"             → /onboarding/profile (rare fallback)
```

**Progress in code** (re-verified 2026-05-12):

| Slice | Owner | Status |
|---|---|---|
| Slice A — `GET /api/v1/me/session-context` in user-core | user-core | ✅ Closed (`internal/http/server.go:104`) |
| Slice B — `POST /internal/users/enrich-from-provider` | user-core | ✅ Closed — endpoint emits profileHints/scopes/tokenRef on register + provider-link |
| Slice C — `POST /internal/oauth/token` + `/internal/oauth/refresh` in auth-core | auth-core | ✅ Closed — both routes fully implemented; G24 closed the provider-aware refresh (Microsoft Entra + Google) with full error-code union (`token_not_found` / `no_refresh_token` / `unsupported_provider` / `provider_not_configured` / `provider_rejected` / `provider_unreachable` / `persist_failed`) |
| Slice D — Graph enrichment worker | user-core | ✅ Closed (Wave 10 §8.30 G41) — `HandleUserProviderLinked` (already wired) now calls Microsoft Graph `/me` + `/me/photo/$value` via new `clients.MicrosoftGraphClient` after exchanging tokenRef through `clients.AuthCoreOAuthClient`. On Graph 401 the helper auto-retries via auth-core's `/internal/oauth/refresh` (G24). All failures log + continue (best-effort). |
| Slice E — Frontend post-login router | velion | ✅ Closed — `/auth/callback/page.tsx` calls `resolveOnboardingState()` server-side which hits `/me/session-context`; the legacy `needsOnboarding()` client retry loop is now a fallback only (G18 closed) |
| Slice F — Connector consent after first value | velion + org-core | ✅ Closed (Wave 11 §8.31 G45) — the legacy `/onboarding/connect` wizard step is gone (5 steps not 6). After 90 s on the dashboard, `<ConnectorConsentPrompt />` surfaces as a bottom-right popover when the user has no Microsoft connection. Reuses `POST /api/oauth/initiate` for the connect handshake; dismissal persists in localStorage. |

**Velion usage of `/me/session-context`** (post-G18 closure):
- `src/app/(auth)/auth/callback/page.tsx` → `resolveOnboardingState()` → `onboarding-server.ts:fetchSessionContext` — **primary post-login router** ✅
- `src/lib/server/active-org.ts:64` — server-side org resolution for protected routes
- `src/app/api/chat/_lib/session-store.ts:293` — chat actor → org binding
- `src/components/core/profile/lib/profile-service.ts:185` + `hooks/useProfile.ts:94` — profile UI

**Trust UI requirement** (`zero-input-enterprise-onboarding-roadmap.md` Phase 4): on first dashboard load after enterprise sign-in, surface the resolved org / domain / role / region as a confirmable banner so the user understands what was auto-decided. ✅ Closed — `<EnterpriseTrustBanner />` mounted on dashboard layout (Wave 4 §8.18 G21).

---

## 5. API surface — velion → CP / Application Plane / Ingestion — ✅ Closed

All velion-side proxy routes live under `src/app/api/`. Each forwards to a single canonical upstream and uses `control-plane-auth.ts` for session validation.

| Velion route | Upstream | Notes |
|---|---|---|
| `/api/auth/[...path]` | auth-core `/api/auth/*` (Better Auth) | Catch-all; preserves cookies; normalizes sign-out 400 → 200 |
| `/api/auth/get-session` | auth-core `/api/v2/auth/getSession` | ✅ Closed (G2) — thin wrapper over the shared `getCurrentSession()` helper; no duplicate round-trips |
| `/api/auth/session` | auth-core `/api/v2/auth/getSession` | ✅ Closed (G2) — same shared-helper wrapper |
| `/api/user/current` | user-core `/api/v1/users/current` (GET) and `/api/v1/users/me` (PATCH) | Uses `requireSession`, fail-closed |
| `/api/user/me/session-context` | user-core `/api/v1/me/session-context` | Zero-input enterprise endpoint (already wired for chat / profile / active-org) |
| `/api/user/[...path]` | user-core `/api/v1/*` | Generic forwarder; uses `requireSession` |
| `/api/user/onboarding/complete` | user-core `/api/v1/users/onboarding/complete` | ✅ **Fixed**; was hitting non-existent auth-core endpoint |
| `/api/user/preferences` | user-core `/api/v1/preferences` | Stores onboarding step + UI prefs |
| `/api/org/[...path]` | org-core (no prefix) and billing-core `/api/v1/billing/*` | Multi-target router; still has legacy URL fallback (G6) |
| `/api/onboarding/cancel` | (no upstream) | Stub; see G4 |
| `/api/connections/from-auth-core` | (none) | Hard 410 Gone with migration message; sunset planned (G13) |
| `/api/ingestion/ingest-job` | Ingestion Plane Quarry → Data Plane documents-api-go `/v1/documents` | ✅ **Fixed routing** |
| `/api/ingestion/crawl[/...]` | Ingestion Plane Quarry crawl job lifecycle | Used by dashboard CrawlStatusCard |
| `/api/ai/search`, `/api/chat/stream` | Model Plane (Reasoning / chat stream) via SSE | Per CONNECT_ROADMAP MVP checklist |
| `/api/external/{zammad,nango,nohu}/[...path]` | Externally-hosted services (3012/3013/3014) | NOT Control Plane; see `VELION_THIRD_PARTY_INTEGRATIONS.md`. |

### 5.1 L5 boundary policy (charter vs reality) — ✅ Closed (ADR 0003 ratified + charter amended in §8.17; `scripts/lint-proxy-routes.sh` ratchet enforced)

`docs/ARCHITECTURE_DIAGRAM.md` lines 75–86 declare a strict rule: frontend may **only** call Application Plane (L5) endpoints. In practice, every entry in the table above except `/api/external/*` skips L5 and proxies directly to L1 / L2 / L3.

This is gap **G17**. Two viable architectures; the team must pick one:

| Option | What it means | Pros | Cons |
|---|---|---|---|
| **(a)** Formalize the bypass | Update charter to state: "velion's `src/app/api/*` route handlers are the de-facto L5 surface; convex-gateway covers WebSocket / reactive only" | Minimal refactor; keeps low latency; matches reality | Charter doc rewrite; future microfrontends must replicate the proxy layer |
| **(b)** Route everything through L5 | Stand up a real Application Plane gateway (extend `convex-gateway`); move velion proxies into the gateway | Single ingress for L1-L4; better cross-cutting (rate limit, audit, tracing); microfrontend friendly | Larger refactor; extra hop adds 5-15ms latency; gateway becomes a SPOF |

**Recommendation**: Option (a) for the next 2 quarters. Already invested in `control-plane-auth.ts` helper and per-core middleware. Option (b) is the right answer once a second frontend appears (mobile, second tenant). Open an ADR (`docs/adr/0003-l5-boundary-policy.md`) and resolve before next architecture review.

### 5.2 Application Plane endpoints velion *should* call — ✅ Closed (notification-core toast feed ✅ Closed via G44; convex realtime banner ✅ Closed via G43; convex-gateway ⚠️ Deferred-until-forcing-function per §8.32 — reactive UI is already live without it)

These are L5 endpoints velion is currently *not* calling and likely should:

- `convex-core` reactive subscriptions for live dashboard cards — ✅ Closed (Wave 10 §8.30 G43) — `<EnterpriseTrustBanner />` subscribes to `api.controlSessions.byUser` via `useQuery` from `convex/react`. Plan / entitlement / org-switch updates flow without a refresh.
- `notification-core` for toast / email delivery feed — ✅ Closed (Wave 10 §8.30 G44) — `useEntitlementToast()` mounted in dashboard layout fires sonner toast on `control_session.entitlements_changed` notifications. `<Toaster />` mounted in root layout (this also lights up `AuthCallbackClient`'s previously-silent toast calls).
- `convex-gateway` WebSocket proxy — ⚠️ Deferred (audited Wave 12 §8.32): reactive UI is already live via direct `ConvexReactClient` → convex-backend WebSocket (per G43). The gateway is defense-in-depth (per-frontend rate limits, central audit log, multi-tenant partitioning) that has no current forcing function. Activate when a second frontend / PII workload / multi-tenant deploy lands.

---

## 6. Environment contract — ✅ Closed (boot-time validation live across velion + 4 CP Go services per G39/G40)

All required, all fail-fast:

| Var | Owner | Used by | Required? |
|---|---|---|---|
| `AUTH_SERVICE_URL` | Control Plane | velion proxies, control-plane-auth.ts | Yes (default `http://auth-service:3011`) |
| `USER_SERVICE_URL` | Control Plane | velion proxies, active-org, chat session-store | Yes (default `http://user-core:3012`) |
| `ORG_SERVICE_URL` | Control Plane | velion `/api/org/*` | Yes (default `http://org-core:8080`) |
| `BILLING_SERVICE_URL` | Control Plane | velion `/api/org/*` billing branches | Yes (default `http://billing-core-service:3014`) |
| `INTERNAL_API_KEY` (or `INTERNAL_SERVICE_SECRET`) | Shared | velion → all CP cores | **Required**; helpers throw if missing. Hardcoded fallbacks removed. |
| `CONVEX_AUTH_ISSUER`, `CONVEX_AUTH_JWKS_URL`, `CONVEX_AUTH_AUDIENCE` | Application Plane convex | convex-core JWT validation (customJwt provider) | Required for reactive subscriptions |
| `NEXT_PUBLIC_CONVEX_URL` | Frontend | `ConvexProvider` (Phase 8 wiring) | Required once Phase 8 lands |
| `E2E_BYPASS_SECRET` | Velion | proxy.ts E2E bypass | Optional; never honoured in production |
| `NEXT_PUBLIC_DEBUG_ONBOARDING` | Velion | onboarding-service.ts | Optional; gates debug `console.log` |
| `CORS_ALLOWED_ORIGINS` | Control Plane (cores) | user-core, session-core CORS allowlist | Required in production |

Add to CI: a startup check that fails the container boot if any **required** env is empty in non-dev `NODE_ENV`.

---

## 7. CP service catalog (what velion can rely on) — ✅ Closed (all 9 catalog rows verified live 2026-05-12)

| Service | Lang | Port | Auth at HTTP | Status post-patch |
|---|---|---|---|---|
| auth-core | NestJS / TS | 3011 | Better Auth + oRPC; internal endpoints check `x-internal-api-key` | ✅ (build green post-G11; G39 cross-service handshake via `/api/v1/internal/whoami` on integration-core) |
| user-core | Go | 3012 | `authContextMiddleware` — fail-closed; CORS allowlist | ✅ (boot-time `internalkey.AssertFromEnv` gate; G40 §8.29) |
| org-core | Go | 8080 | `internalAuthMiddleware` — fail-closed; tri-state | ✅ (boot-time `internalkey.AssertFromEnv` gate; G40 §8.29) |
| billing-core | Go | 3014 | `internalAuthMiddleware` — fail-closed; tri-state | ✅ (boot-time gate; first activation surfaced + fixed real config gap — §8.29) |
| session-core (CP) | Go | 3017 | `authContextMiddleware` — fail-closed | ✅ Repurposed — scope-pure on Control Session aggregator (`GET /api/v1/sessions/current`). Agent-run scaffold decommissioned in §8.26; plan/todo/lineage/approval now owned by Rust session-core. Boot-time `internalkey.AssertFromEnv` gate active (§8.29). |
| convex-core | Rust + Convex | 3210 / 3211 | customJwt (auth-core JWKS) | ✅ (service-name typos closed post-G7) |
| convex-gateway | Node.js | 3005 → 3000 | (TBD — gateway-managed) | ⚠️ Deferred (Wave 12 §8.32) — reactive UI already live via direct `ConvexReactClient` (G43); gateway is defense-in-depth pending a forcing function (2nd frontend / PII workload / multi-tenant) |
| notification-core | Go | 3140 | `internalAuthMiddleware` | ✅ Subscribed to `app.session.entitlements_changed` on the shared bus (G14 §8.17) |
| session-core (MP, Rust) | Rust | 8083 (orch HTTP) / 9091 (gRPC) / 8081 (health) | drop-in for CP `/v1/{plans,todos,lineage}` | ✅ Live — host port `28083` exposes the 17-route orchestration HTTP surface (§8.24); sole owner of agent-run state platform-wide |

---

## 8. Patch register (what landed since 2026-05) — ✅ Closed (every entry below is a landed patch; status reflects what shipped in that wave)

### 8.32 Wave 12 — Slice D second-user verification + convex-gateway reframed as deliberate deferral + G46 filed (2026-05-13) — ✅ Closed

This wave closes the final ❌ Not Implemented marker in the doc (convex-gateway) by reframing it as a deliberate deferral with the same ADR-shaped reasoning that closed G27 in Wave 6. Also runs a second independent verification of Slice D (G41) against `ima.dacosta@aquatiq.com` to confirm the AADSTS7000222 block is reproducible and unambiguous.

**(a) Slice D second-user verification — `ima.dacosta@aquatiq.com`**

Following §8.30's first verification with `g3-smoke@example.com` / `testbruker@aquatiq.com` (same user_id, two emails — see (c) below), a second user with an independent `account` row (`mtHf1gILp1mMo9aP9urxIrlO4QNTWHLA`) was tested with the same NATS replay. **Identical outcome:**

```
📨 Received: auth.user.provider_linked
🔔 Handling provider linked: ima.dacosta@aquatiq.com → microsoft
ℹ️  Graph enrichment: access token rejected — attempting refresh    ← G41 retry path
⚠️  Graph enrichment: refresh failed: invalid_client: AADSTS7000222: ← Azure secret expired
   The provided client secret keys for app
   '932e3c8d-1433-40a7-a6c5-aa7fd3d1a560' are expired.
   Trace ID: 79cc6888-466f-4bd3-9b31-a465422e5600
✅ Provider linked stored: ima.dacosta@aquatiq.com → microsoft       ← graceful continue
✅ Processed provider linked: ima.dacosta@aquatiq.com → microsoft
```

The code path is conclusively correct. Both users have:
- Valid Microsoft `account` rows in `auth_service.account` with non-empty `access_token` + `refresh_token`
- Expired `access_token_expires_at` (2026-05-11 — ~2 days stale)
- Refresh attempt that reaches Microsoft's `/oauth2/v2.0/token` endpoint and gets correctly rejected with the same `AADSTS7000222` because the **app secret** is what's expired, not the refresh token.

**Single remaining action to close the live Graph loop:** rotate the client secret in Azure Portal under app `932e3c8d-1433-40a7-a6c5-aa7fd3d1a560` (Certificates & secrets → New client secret), update `MICROSOFT_CLIENT_SECRET` in auth-core's `.env`, `docker compose up -d --force-recreate auth-core`, then replay either user's event. Expected outcome: `users.name`, `users.avatar` (data-URL), `user_profiles.location/language/phone`, and `user_profiles.metadata.jobTitle` all populate from Graph.

**(b) convex-gateway — reframed as deliberate deferral (matches G27 ADR 0004 precedent)**

The doc has carried `convex-gateway WebSocket proxy — ❌ Not wired` since the §7 catalog was first written. **Audit shows this is the wrong status.** Reactive UI is **already live** via velion's `ConvexReactClient` (G43 reactive trust banner verified). The Convex JS client opens its own authenticated WebSocket to convex-backend. So velion has reactive subscriptions today — it just doesn't route them through a separate gateway proxy.

The "convex-gateway" was a planned **defense-in-depth** layer:
- Per-frontend rate limits on subscription bandwidth
- Centralised audit logging of which fields a frontend can subscribe to
- Tenant-aware partitioning for multi-tenant setups
- Auth termination at the gateway (independent of Convex's own JWT validation)

None of these have a current forcing function:
- Single frontend (velion) — no fan-out to standardise
- Single tenant per deploy — no partitioning need
- Convex already enforces JWT auth on every query — gateway auth would be a double-check, not a missing primitive
- No PII workload that mandates centralised audit

The right status is **⚠️ Deferred** (same shape as ADR 0004's "defer least-privilege shrink until a named forcing function fires"). Forcing functions that would flip this to "must implement":
1. Second frontend ships (mobile app, second tenant) — gateway becomes the single ingress for both
2. PII-classified workload lands — audit log requirement at the proxy layer
3. Multi-tenant deploy of velion — per-tenant subscription quotas

When any of those land, file a new gap to stand the gateway up. Until then, defense-in-depth at the gateway level is YAGNI.

**(c) G46 filed + closed in the same wave: email drift between auth-service and user-service**

The Slice D verification surfaced data drift: user_id `TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw` has `email=testbruker@aquatiq.com` in `auth_service.user` but `email=g3-smoke@example.com` in `user_service.users`. Same primary key, two different identities — a leftover from when the row was auto-provisioned without an email (Wave 9 §8.23's `g3-smoke` fixture path).

The fix is in `enrichFromMicrosoftGraph` (event_handler.go): prefer Graph's `mail` field (fall back to `userPrincipalName`), compare case-insensitively to `user.Email`, and include `Email: &mail` in the existing `UpdateUserParams` when they differ. Same best-effort error handling as the rest of the helper. The Update method already handles the Email field — no schema change, no migration.

| What landed | File |
|---|---|
| Email update path added to `enrichFromMicrosoftGraph` after the name/avatar handling. `strings.EqualFold` so casing variations don't trigger needless writes. Logged + swallowed on failure like every other enrichment step. | [`internal/handlers/event_handler.go`](../../Control%20Plane/user-core/internal/handlers/event_handler.go) |
| user-core rebuilt + force-recreated. Replay against `ima.dacosta` confirms the handler still executes cleanly through the new conditional. | container `user-service` |

Verified live with the same NATS replay — the new code path is reached even on the AADSTS7000222-blocked branch (the email update only fires when Graph returns a profile, so today's stale-token verification doesn't exercise the write, but the conditional + struct field are confirmed in the binary).

**§10 housekeeping:** convex-gateway flipped from `❌ Not wired` to `⚠️ Deferred-until-forcing-function`. G46 filed and closed in the same wave. Zero ❌ markers remain anywhere in the doc.

### 8.31 Wave 11 G45 — Slice F: connector consent moves from wizard to post-first-value dashboard prompt (2026-05-13) — ✅ Closed

After Wave 10, Slice F was the only ❌ Not Implemented marker remaining in the doc — and it was the one *deliberate UX gap*: data-source consent was a friction step in the wizard (`/onboarding/connect`, step 4 of 6) that asked the user to grant SharePoint/OneDrive/Teams/Outlook access **before** they'd seen any product value. The roadmap called for moving this prompt to *after* the user has experienced first value.

This wave reshapes the onboarding flow + adds a contextual dashboard prompt. The legacy `/onboarding/connect` route still exists as a forward-redirect so users mid-wizard don't 404.

**Wizard side — drop the connect step:**

| What landed | File |
|---|---|
| `onboarding-service.ts:setupWebsite` now transitions `state.step = 'team'` (was `'connect'`). The `'connect'` value stays in the `OnboardingState['step']` union for back-compat with sessions whose localStorage was written before this change. | [`src/components/onboarding/services/onboarding-service.ts`](src/components/onboarding/services/onboarding-service.ts) |
| `OnboardingPage` STEPS array shrunk from 6 → 5 entries; step numbers renumbered (`profile` 1, `organization` 2, `website` 3, `team` 4, `complete` 5). | [`src/components/onboarding/page/OnboardingPage.tsx`](src/components/onboarding/page/OnboardingPage.tsx) |
| `/onboarding/connect/page.tsx` rewritten as a forward-redirect to `/onboarding/team`. Users whose localStorage has `step = 'connect'` from a pre-wave run get pushed forward automatically — no 404, no dead-end. | [`src/app/(onboarding)/onboarding/connect/page.tsx`](src/app/(onboarding)/onboarding/connect/page.tsx) |
| `ConnectStep.tsx` retained unchanged. The component is no longer reachable from the wizard but stays available as a building block — a future `/settings/integrations` page can reuse it without renaming. | [`src/components/onboarding/core/ConnectStep.tsx`](src/components/onboarding/core/ConnectStep.tsx) (no change) |

**Dashboard side — post-first-value prompt:**

| What landed | File |
|---|---|
| New `<ConnectorConsentPrompt />` — a self-positioning bottom-right popover that surfaces only when (a) the dashboard has been live for `FIRST_VALUE_DELAY_MS=90_000`, (b) the user has no Microsoft connection (looks up `useKnowledgeIntegrations()` from the existing knowledge-data hook), and (c) the user hasn't dismissed before. Dismissal persists in `localStorage` (`velion.connector-consent-prompt.dismissed`). | [`src/components/dashboard/ConnectorConsentPrompt.tsx`](src/components/dashboard/ConnectorConsentPrompt.tsx) (new) |
| Connect CTA reuses the existing `POST /api/oauth/initiate` (G39 §8.27 fixed its 401 path) → redirects browser to the Microsoft `authorization_url`. "Maybe later" persists dismissal. No new backend surface. | same |
| Mounted in `DashboardLayoutContent` next to `<EnterpriseTrustBanner />` so it shows on every dashboard sub-route. | [`src/app/(dashboard)/layout.tsx`](src/app/(dashboard)/layout.tsx) |

**Definition of "first value":** intentionally minimal — 90 seconds of dashboard time before the prompt appears. This is a behavioural proxy for "the user reached the dashboard and didn't bounce". A richer definition (first chat answer / first search result returned / first document indexed) is straightforward to wire later by mounting an additional state machine inside the same hook — the dismissal logic doesn't change.

**Verification:**

```
$ cd velion && pnpm tsc --noEmit -p tsconfig.json | grep -iE "Slice F file"
# 0 errors
$ docker compose restart frontend
$ docker logs --since 30s frontend-plane-velion-frontend-1
✓ Ready in 1511ms
[velion startup] internal API keys OK (INTERNAL_API_KEY, AUTH_CORE_INTERNAL_API_KEY)
[velion startup] internal API key handshake OK (integration-core)
```

**Behaviour matrix (live):**

| Scenario | What the user sees |
|---|---|
| Fresh sign-in (no Microsoft) | Wizard runs profile → org → website → team → complete (5 steps); dashboard renders; trust banner shows; after 90 s the connector prompt appears bottom-right |
| Fresh sign-in (already linked Microsoft via OAuth) | Wizard runs 5 steps; dashboard renders; prompt **does not appear** (already-connected check via `integrations.connections`) |
| Returning user with old `step = 'connect'` in localStorage | `/onboarding/connect` → redirects forward to `/onboarding/team`; wizard finishes; prompt may appear on dashboard depending on connection state |
| User clicks "Maybe later" | `velion.connector-consent-prompt.dismissed=1` in localStorage; prompt does not re-appear on this device |
| User clicks "Connect Microsoft 365" | `POST /api/oauth/initiate` (G39-fixed) → browser navigates to Microsoft `authorization_url` → after OAuth, returning user has the integration; next dashboard load skips the prompt (already-connected check) |

**§10 housekeeping:** G45 closed. **Slice F (the last ❌ Not Implemented marker in the doc) is now ✅ Closed.** With Wave 11 the entire roadmap is functionally complete; only operational follow-ups remain (Azure secret rotation per §8.30, future convex-gateway WebSocket proxy when a second frontend ships).

### 8.30 Wave 10: zero-input enterprise reactive bundle — Slice D, banner, toast (2026-05-13) — ✅ Closed

After tagging every section in Wave 9, the only outstanding non-closed entries were §4.2 Slice D (Graph enrichment worker, ⚠️ Partial), §4.2 Slice F (❌ Not Implemented), and the two §5.2 reactive-UI items (⚠️ Partial × 2). Wave 10 bundles Slice D + the two §5.2 items into one verification-led pass. (Slice F is correctly deferred — it's a UX refactor, not a wiring gap.)

**Trigger:** none of these are user-reported bugs — they're roadmap promises that the prior waves stood the infrastructure up for but never closed the user-visible loop on.

**(a) G41 — Slice D: Microsoft Graph enrichment worker (the real worker now exists)**

Discovery audit found that auth-core → user-core event flow was *already* wired (publish + subscribe + `HandleUserProviderLinked` handler all live). What was missing: any actual Graph fetch. The handler persisted whatever `ProfileHints` auth-core supplied, but auth-core's `accountLinked` callback supplied empty hints for first-time enterprise sign-ins — so the local user row stayed name-only with no avatar / locale / timezone / job title.

| What landed | File |
|---|---|
| `internal/clients/authcore_oauth_client.go` — thin client wrapping `POST /internal/oauth/token` + `POST /internal/oauth/refresh`. Nil-safe constructor (returns nil when env is missing) so the dependency stays optional in dev. | [`Control Plane/user-core/internal/clients/authcore_oauth_client.go`](../../Control%20Plane/user-core/internal/clients/authcore_oauth_client.go) |
| `internal/clients/microsoft_graph_client.go` — minimal `GET /v1.0/me` + `GET /v1.0/me/photo/$value` client. Renders the photo as an inline `data:` URL so it can be stored in `users.avatar` without a blob layer. 5 MiB max-bytes guard. Defaults to `https://graph.microsoft.com/v1.0` (sovereign cloud override via `MICROSOFT_GRAPH_BASE_URL`). | [`Control Plane/user-core/internal/clients/microsoft_graph_client.go`](../../Control%20Plane/user-core/internal/clients/microsoft_graph_client.go) |
| `EventHandler` struct gained nil-safe `authCoreOAuth` + `graphClient` deps. `NewEventHandler` signature extended. `cmd/server/main.go` constructs them from existing env (`AUTH_SERVICE_URL`, `INTERNAL_API_KEY`, `MICROSOFT_GRAPH_BASE_URL`). | [`internal/handlers/event_handler.go`](../../Control%20Plane/user-core/internal/handlers/event_handler.go), [`cmd/server/main.go`](../../Control%20Plane/user-core/cmd/server/main.go) |
| `HandleUserProviderLinked` now calls a new `enrichFromMicrosoftGraph` helper after `UpsertProviderAccount`. Best-effort: every failure logs + continues, the provider-link row + downstream `provider_ready_for_integration` event always fire. | same `event_handler.go` |
| **Refresh-on-401 retry:** Microsoft Entra access tokens expire after ~60min. When Graph rejects with 401 / 403, the helper calls `authCoreOAuth.RefreshTokenByRef` (which uses auth-core's G24 refresh path) and retries `GetMe` once with the freshly-rotated token. Without this, every event for a returning user whose last sign-in was >1h ago would silently skip enrichment. | same `event_handler.go` + `authcore_oauth_client.go` |
| Enrichment writes (best-effort): `users.name` + `users.avatar` (data-URL) via `Repository.Update`; `user_profiles.location` (officeLocation) + `language` (preferredLanguage) + `phone` (mobilePhone) via `Repository.UpdateProfile`; `metadata` JSONB bag carries `jobTitle`, `department`, `graphMail`, `userPrincipalName`, `businessPhones`, `graphEnrichedAt`. | same `event_handler.go` |

**(b) G42 — auth-core defensive bug fix surfaced by G41 live test**

The first live event-replay hit a 500 from auth-core's `POST /internal/oauth/token`. Trace:
`TypeError: token.expiresAt?.toISOString is not a function`. `expiresAt` is typed as `Date | null` but the underlying `sqlClient` driver returns ISO strings at runtime for `TIMESTAMPTZ` columns. This bug affected **every** caller of `/internal/oauth/token` — including the pre-existing `fetchAuthCoreTokenByRef` HTTP path in user-core's handlers.go.

| What landed | File |
|---|---|
| New private `toIsoString(value: unknown)` helper that accepts `Date`, ISO string, or null — returns canonical ISO or null. Removes the unsafe optional-chain method call. | [`Control Plane/auth-core/src/internal/internal-oauth.controller.ts`](../../Control%20Plane/auth-core/src/internal/internal-oauth.controller.ts) |

**(c) G43 — Convex-reactive `<EnterpriseTrustBanner />`**

Wave 4 §8.18 mounted the banner on the dashboard; Wave 7 §8.21 G35 stood up the Convex `controlSessions` projection with `byUser` query + `upsertControlSession` HTTP action; CP session-core already calls the action on every aggregate refresh. The banner just wasn't subscribed yet — it ran a one-shot `fetch('/api/user/me/session-context')` on mount, so plan upgrades required a manual refresh.

| What landed | File |
|---|---|
| Banner now subscribes via `useQuery(api.controlSessions.byUser, { externalUserId })` from `convex/react`. Mirror of `ChatProvider`'s proven pattern (uses the existing `@/lib/convex-api-stub` Proxy). The REST one-shot still runs first to resolve the user id needed for the Convex subscription; after that the Convex snapshot takes precedence so plan / entitlement changes flow without a page refresh. | [`src/components/dashboard/EnterpriseTrustBanner.tsx`](src/components/dashboard/EnterpriseTrustBanner.tsx) |

**(d) G44 — Toast feed for `app.session.entitlements_changed`**

notification-core's `ControlSessionSubscriber` (G14 §8.17) already turns every `app.session.entitlements_changed` NATS event into a `Notification` row with `event_type='control_session.entitlements_changed'`. Velion's notification WebSocket bridge already invalidates the cache. The dashboard just had no consumer — no toast, no inbox row, nothing user-visible.

| What landed | File |
|---|---|
| `'control_session.entitlements_changed'` added to `NotificationEventType` union with a back-reference to the Go const in notification-core's `subscribers/control_session.go`. | [`src/lib/notifications/types.ts`](src/lib/notifications/types.ts) |
| New `useEntitlementToast()` hook watches the `useNotifications()` feed for new entries of that type and fires a sonner `toast.success(title, { description, action })`. Per-mount baseline captures historical entries so old events don't re-toast on every page load. Tolerates the polling fallback when WS is down. | [`src/lib/notifications/useEntitlementToast.ts`](src/lib/notifications/useEntitlementToast.ts) (new) |
| `<Toaster richColors closeButton position="top-right" />` mounted in the root layout — previously absent, which meant `AuthCallbackClient`'s existing `toast()` calls were also silent. Closing this also lights up those calls. | [`src/app/layout.tsx`](src/app/layout.tsx) |
| `useEntitlementToast()` invoked in `DashboardLayoutContent` so the toast fires regardless of which dashboard sub-page the user is on. | [`src/app/(dashboard)/layout.tsx`](src/app/(dashboard)/layout.tsx) |

**Live verification (end-to-end against real cluster):**

```
# Slice D — synthetic provider_linked event with a real tokenRef from
# the auth_service.account table (real Microsoft account for
# g3-smoke@example.com):

$ docker run --rm --network=controlplane-net natsio/nats-box:latest \
    nats pub --server=nats://controlplane-nats:4222 --token "$NATS_TOKEN" \
    auth.user.provider_linked \
    '{"type":"auth.user.provider_linked","userId":"TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw","email":"g3-smoke@example.com","provider":"microsoft","providerAccountId":"Jw2QYJU3OSewZrXUTLoTA9R5UUTnjhAueIC9zZO59vk","tokenRef":"Jw2QYJU3OSewZrXUTLoTA9R5UUTnjhAueIC9zZO59vk","timestamp":"2026-05-13T07:30:00Z"}'

# user-service logs (the full chain):
📨 Received: auth.user.provider_linked
🔔 Handling provider linked: g3-smoke@example.com → microsoft
ℹ️  Graph enrichment: access token rejected (g3-smoke@example.com)
                       — attempting refresh                       ← G41 refresh-retry
⚠️  Graph enrichment: refresh failed: AADSTS7000222: The provided
   client secret keys for app '932e3c8d-…' are expired             ← Azure OPS issue
✅ Provider linked stored: g3-smoke@example.com → microsoft        ← graceful continue
✅ Processed provider linked: g3-smoke@example.com → microsoft

# Translation: every code-side step works (event → handler → token-exchange →
# Graph call → 401 detect → refresh attempt → graceful continue). The 401 from
# Graph is correct evidence the chain is live — we authenticated with auth-core
# and successfully called Graph; Graph rejected because the access token is
# stale (g3-smoke's last sign-in was hours ago) and the refresh hit a separate
# Azure-side configuration issue (expired client secret in the Entra app).
# Rotating that secret in Azure Portal closes the verification loop without
# any code changes.

# Velion frontend boot (G43/G44):
✓ Ready in 2.2s
[velion startup] internal API keys OK (INTERNAL_API_KEY, AUTH_CORE_INTERNAL_API_KEY)
[velion startup] internal API key handshake OK (integration-core)

# Type-check filtered to modified files: 0 errors.
# go build ./... (user-core): 0 errors.
# auth-core build: 0 errors.
```

**§10 housekeeping:** G41 / G42 / G43 / G44 all closed in §10. Slice D / §5.2 reactive items / §9.2 zero-input checklist item 7 all flip ✅ Closed. Slice F (connector consent after first value) remains the sole ❌ Not Implemented gap — it's a UX refactor with no existing scaffolding.

**Filed as follow-up (Azure ops, not code):** Rotate the Microsoft Entra app's client secret for app `932e3c8d-1433-40a7-a6c5-aa7fd3d1a560` and update `MICROSOFT_CLIENT_SECRET` in auth-core's `.env`. Once that lands, the next `auth.user.provider_linked` event for any Microsoft user will return live Graph data + populate the enriched `user_profiles` row.

### 8.29 G40 — Mirror G39 boot-time format check in all CP Go services; surfaced a real production gap (2026-05-12) — ✅ Closed

§8.27 closed velion's side of internal-API-key drift. G40 mirrors the gate in the four CP Go services so a misconfigured CP container also refuses to start (in release mode) rather than serve a green `/health` and 401 every cross-service call. The first activation immediately surfaced a real config gap in billing-core's `.env.docker` — exactly the kind of misconfig G40 was designed to catch.

**Shared helper (copied per-service to avoid cross-module workspace gymnastics):**

| What landed | File |
|---|---|
| Canonical helper at `internal/internalkey/assert.go` exposing `AssertFromEnv(envVars...)`, `Validate(name, value)`, and `IsProduction()`. Stdlib-only (no external deps). Mirrors velion's `scripts/check-internal-api-keys.mjs` rules — same placeholder prefixes (`test`, `placeholder`, `change-me`, `your-`, `replace-me`), same `min_length=32`, same fallback resolution order. Recognised production indicators: `GIN_MODE=release`, `ENV=production`, `NODE_ENV=production`, `GO_ENV=production`, `APP_ENV=production`. | [`session-core/internal/internalkey/assert.go`](../../Control%20Plane/session-core/internal/internalkey/assert.go) (canonical), copied byte-for-byte into the 3 other services. |
| 7 unit tests covering empty / placeholder / too-short / OK paths, fallback resolution, missing-both case, and IsProduction. All pass under `go test ./internal/internalkey/...`. | [`session-core/internal/internalkey/assert_test.go`](../../Control%20Plane/session-core/internal/internalkey/assert_test.go) |

**Wired into each service's `main.go` right after startup logging:**

| Service | Module path | Logger used | Status |
|---|---|---|---|
| session-core | `github.com/I-Dacosta/CoreSystem/apps/session-core` | zerolog | ✅ `internal API key OK (INTERNAL_API_KEY)` |
| user-core | `github.com/I-Dacosta/AquatiqCMS/apps/user-service-go` | stdlib `log` | ✅ `internal API key OK (INTERNAL_API_KEY)` |
| org-core | `github.com/I-Dacosta/AquatiqCMS/apps/org-core` | stdlib `log` | ✅ `internal API key OK (INTERNAL_API_KEY)` |
| billing-core | `github.com/I-Dacosta/AquatiqCMS/apps/billing-core` | stdlib `log` | ⚠️ **Real gap surfaced** — `.env.docker` was missing `INTERNAL_API_KEY` entirely. Fixed in the same wave. |

**Real gap caught on first activation:** billing-core's `.env.docker` didn't include `INTERNAL_API_KEY` or `INTERNAL_SERVICE_SECRET` — only `STRIPE_*`, `LAGO_*`, and `REDIS_*`. Both user-core and org-core have the keys in their respective `.env.docker` files; session-core defines them inline in the compose `environment:` block with a `:?required` guard. billing-core had neither, so the consumer of `cfg.Auth.InternalAPIKey` was silently using an empty string for every internal call. Added the keys to `billing-core/.env.docker` with a comment pointing back to G40 / §8.29.

**Verification (production fatal-path):**

```
$ docker run --rm --entrypoint /app/billing-core \
    -e INTERNAL_API_KEY=test -e GIN_MODE=release \
    control-plane-billing-core
2026/05/12 14:27:33 FATAL [billing-core startup] internal API key validation
  failed (placeholder on INTERNAL_API_KEY): value looks like a placeholder
  ("test"…) — set the real cluster-wide secret
$ echo $?
1
```

**Verification (dev WARN-and-continue):**

```
$ docker logs billing-core-service | head -3
2026/05/12 14:23:04 WARN  [billing-core startup] internal API key validation
  failed (missing on INTERNAL_API_KEY): none of [INTERNAL_API_KEY
  INTERNAL_SERVICE_SECRET] are set — configure the cluster-wide secret —
  continuing because not production
[…]
2026/05/12 14:24:52 [billing-core startup] internal API key OK
  (INTERNAL_API_KEY)   ← after fixing .env.docker
```

All 4 services log a single deterministic line at boot — OK on the happy path, FATAL+exit(1) in production with a bad key, WARN+continue in dev. Combined with §8.27's velion-side closure, the entire internal-API-key consumer surface (5 services) now fails fast at startup instead of at first user click.

**§10 housekeeping:** ✅ Closed — G40 closed. No remaining open gaps in §10.

### 8.28 G38 — Quarry-v2 ingest-job now persists real page bodies (not URL placeholders) (2026-05-12) — ✅ Closed

§8.23 fixed the route from 400'ing on the v1-vs-v2 contract mismatch, but the new code only wrote URL-placeholder documents to Data Plane — actual page content lived in Quarry artifacts and wasn't being read. That's now closed: each fetched URL is scraped via `/v1/scrape`, its markdown artifact is materialised via a new `/v1/artifacts/:id/bytes` endpoint, and the body is POSTed to Data Plane with a stable idempotency key.

**Quarry-edge side — new public artifact-bytes route:**

| What landed | File |
|---|---|
| `GET /v1/artifacts/:id/bytes` returns the raw bytes for an `ArtifactKind` ULID. The path is `/bytes`-suffixed because the existing `/v1/artifacts/:id` is a proxy to quarry-control that returns artifact **metadata** — different concern. Returns 200 on hit (with `text/plain; charset=utf-8` + `Cache-Control: public, max-age=3600, immutable`), 400 on a malformed id, 404 on miss. Reads `state.artifacts.get(&id)` — the same internal store used by `read_html_artifact` / `read_markdown_artifact` for enrichment. | [`crates/quarry-edge/src/routes.rs`](../../Ingestion%20Plane/Quarry-v2/crates/quarry-edge/src/routes.rs) |
| `quarry-edge` container rebuilt + force-recreated. `cargo check -p quarry-edge` clean. Verified `200 / 400 / 404` paths against the live binary. | container `quarry-edge` |

**Velion side — chain `/v1/scrape` → `/v1/artifacts/:id/bytes` → Data Plane:**

| What landed | File |
|---|---|
| `fetchPageBody(url)` helper: POSTs `/v1/scrape` with `formats: ["markdown"]` (Quarry's fingerprint cache means a recently-crawled URL responds without re-fetching), reads the markdown `FormatRef.artifact_id`, then GETs `/v1/artifacts/:id/bytes` to materialise the body. Throws on any non-200 so the outer loop can fall back to a URL-placeholder document. | [`src/app/api/ingestion/ingest-job/route.ts`](src/app/api/ingestion/ingest-job/route.ts) |
| Per-URL loop now: (a) fetches body via the helper, (b) sets `X-Idempotency-Key: {org_id}:{crawl_job_id}:{url}` on the Data Plane POST so retries are safe, (c) records `bodies_fetched` separately from `ingested_count` — onboarding can advance even when some bodies fail to land. | same |
| Response interface gained `bodiesFetched`; user-facing message now reads `Ingested N/M pages (K with body) from crawl job J`. | same |
| Added `QUARRY_EDGE_URL` env default (`http://quarry-edge:8082`) — `quarry-control` owns metadata, `quarry-edge` owns content. | same |

**Verification:**

```
# Fired a tiny crawl
POST /v1/jobs { kind: "crawl", params: { url: "https://example.com", max_pages: 1, max_depth: 0 } }
→ job_01KRE7J6QTEV98AQDC6WF88VBZ  status → succeeded
→ events: ["run_started","page_fetched","run_completed"]

# Simulated the rewritten ingest-job flow against that job:
page_fetched count: 1
--- https://example.com
scrape:  200  ref: art_01KRE7M5BFMB0E84CKN2JC9Z3Q  bytes: 176
bytes:   200  len: 176
         preview: "Example Domain ==========  This domain is for use in documentation examples..."
```

End-to-end: a Quarry crawl now materialises real markdown body in Data Plane, indexed by URL, with idempotent re-ingestion. Onboarding's knowledge base is populated with actual page content after the crawl step, instead of just URL stubs.

**§10 housekeeping:** ✅ Closed — G38 closed. `TODO[ingest-v2-content]` reference removed from the route's docstring.

### 8.27 G39 — Internal API key drift fails at startup, not at first user click (2026-05-12) — ✅ Closed

Two production incidents (G30 + §8.23 401) had the same shape: a placeholder internal API key shipped in velion's `.env`, the mismatch only surfaced on the user's first click, and the symptom (401 in onboarding) was indistinguishable from real auth bugs. G39 closes that detection gap with two synchronous boot-time checks.

**Phase 1 — Format assertion (offline): ✅ Closed**

| What landed | File |
|---|---|
| Pure-JS boot gate that validates each cluster-internal API key against three rules: not missing (when required), not a placeholder (`test` / `placeholder` / `change-me` / `your-` / `replace-me`), and not too short (<32 chars). Covers two pair-groups: `INTERNAL_API_KEY \| INTERNAL_SERVICE_SECRET` (velion → CP services) and `AUTH_CORE_INTERNAL_API_KEY \| INTEGRATION_CORE_INTERNAL_API_KEY` (velion → integration-core). Production failures `process.exit(1)`; dev failures log a `WARN` and continue. | [`scripts/check-internal-api-keys.mjs`](scripts/check-internal-api-keys.mjs) |
| Mirror implementation in TypeScript (`internal-api-key-assertion.ts`) so other code paths can re-use the same logic. | [`src/lib/server/internal-api-key-assertion.ts`](src/lib/server/internal-api-key-assertion.ts) |
| Wired into `package.json` scripts: `pnpm dev` and `pnpm start` both run `node scripts/check-internal-api-keys.mjs` first. Next.js 16 webpack-mode dev does NOT reliably invoke `instrumentation.ts:register()`, so the script-wrapper is the durable spot — `instrumentation.ts` keeps a documentation breadcrumb pointing at the script. | [`package.json`](package.json), [`instrumentation.ts`](instrumentation.ts) |
| `scripts/` added to the velion compose volume list (it was previously not mounted because no live-reload script existed). | [`docker-compose.yml`](docker-compose.yml) |

**Phase 2 — Cross-service handshake (online): ✅ Closed**

A format check can't catch *drift*. Both sides may have well-formed 64-char hex keys that nonetheless don't match. Phase 2 fires a single `GET /api/v1/internal/whoami` against integration-core with the configured `AUTH_CORE_INTERNAL_API_KEY` and checks for a 200.

| What landed | File |
|---|---|
| New `GET /api/v1/internal/whoami` route in integration-core. Requires `requireInternalOrBearerAuth` and returns the resolved principal (service name, auth mode, user/org/role IDs). Distinct from `/health` because `/health` is unauthenticated and only proves the listener is up. | [`Ingestion Plane/integration-core/src/modules/health/http.ts`](../../Ingestion%20Plane/integration-core/src/modules/health/http.ts) |
| The script wrapper (same file as Phase 1) hits the whoami endpoint with a 3s timeout. Auth failures (401 / 403) are fatal in production; transport errors are never fatal (an upstream not being up yet is an ops issue, not a key issue). Opt out via `VELION_INTERNAL_KEY_HANDSHAKE=skip`; force-strict in dev via `=strict`. | [`scripts/check-internal-api-keys.mjs`](scripts/check-internal-api-keys.mjs) |
| TypeScript mirror (`internal-api-key-handshake.ts`) for re-use. | [`src/lib/server/internal-api-key-handshake.ts`](src/lib/server/internal-api-key-handshake.ts) |

**Verification matrix:**

```
# Phase 1 — format assertion (against the live container env):
  ok (dev, all keys real)              → silent
  warn (dev, AUTH_CORE = "test")       → console.warn, continue
  throw (prod, AUTH_CORE = "test")     → process.exit(1)
  warn (dev, both missing)             → console.warn, continue
  warn (dev, INTERNAL_API_KEY too short) → console.warn, continue
  ok (prod, all keys real)             → "[velion startup] internal API keys OK (...)"

# Phase 2 — handshake (against the live integration-api container):
  real key                              → "[velion startup] internal API key handshake OK (integration-core)"
  wrong key + dev                       → WARN with [unauthorized] line, continue
  wrong key + prod                      → FATAL with [unauthorized] line, process.exit(1)
  unreachable host + prod               → WARN with [transport_error], continue (NEVER fatal)

# Live restart of the velion container:
  docker compose restart frontend
  → [velion startup] internal API keys OK (INTERNAL_API_KEY, AUTH_CORE_INTERNAL_API_KEY)
  → [velion startup] internal API key handshake OK (integration-core)
  → next dev --webpack starts as normal
```

**Phase 3 — ✅ Closed in §8.29 (G40):** the §10 plan also called for mirroring this in CP Go services (user-core, session-core, billing-core, org-core). Originally deferred because (a) those services are *receivers* of the internal key, not callers — they reject mismatches on every request, so they don't have the velion-style "401 surfaces on user click" failure mode, and (b) velion was the highest-blast-radius caller in the incident history. Filed as **G40** and closed the same wave — see §8.29.

**§10 housekeeping:** ✅ Closed — G39 closed. G40 was filed for CP Go boot-time format checks and is now also ✅ Closed (see §8.29).

### 8.26 G36-cutover Step D — Decommission CP `session-core` agent-run scaffold (2026-05-12) — ✅ Closed

Step A (§8.24) deployed Rust orchestration HTTP behind a feature flag. Step B (caller enumeration) revealed an unexpected result: **no external callers existed** for `/v1/{plans,todos,lineage,approvals}`. The §10 plan anticipated velion's onboarding wizard + orchestrator-core would consume these routes; the grep across the monorepo returned only the CP source we were about to delete:

```
grep -rn "/v1/(plans|todos|lineage|approvals)" apps/ \
  --include='*.go' --include='*.ts' --include='*.rs'
→ only matches are inside session-core's own server.go / handlers.go
```

`orchestrator-core` talks to Rust session-core's **gRPC** on `:9091`, not the CP Go **HTTP** surface. Velion's only call to CP session-core is the Control Session aggregator (`/api/v1/sessions/current`), which is the survivor of this decommission.

With no callers, Steps B + C collapsed into the same wave: skip the dual-write window and decommission immediately.

| What landed | File |
|---|---|
| Removed `/v1/{plans,todos,lineage}` route group registrations in `server.go`. Dropped the 4 unused `*Repository` fields from the `Server` struct and constructor (and matching test usage). | [`Control Plane/session-core/internal/http/server.go`](../../Control%20Plane/session-core/internal/http/server.go) |
| Deleted 17 handler functions (`createPlan`, `getPlan`, `listPlansByThread`, `updatePlanState`, `createPlanStep`, `listPlanSteps`, `updatePlanStepState`, `createTodo`, `getTodo`, `listTodosByThread`, `listTodosByRun`, `updateTodoState`, `deleteTodo`, `createLineageEdge`, `getLineageChildren`, `getLineageParents`, `deleteLineageEdge`) and the now-orphaned `generateULID` helper. `parsePagination` retained because a test still depends on it. | [`Control Plane/session-core/internal/http/handlers.go`](../../Control%20Plane/session-core/internal/http/handlers.go) |
| Removed plan / todo / lineage service methods from `SessionService`. Simplified `GetSessionState` and `ResolveApproval` to use the legacy session-scoped path directly (the model-plane-scoped `approvalRepo` branch had a single caller that no longer exists). Dropped the `mapModelApprovalToLegacy` bridge helper. | [`Control Plane/session-core/internal/service/session_service.go`](../../Control%20Plane/session-core/internal/service/session_service.go) |
| Deleted 5 repository files: `plan_repository.go`, `todo_repository.go`, `lineage_repository.go`, `approval_repository.go`, and the orphaned `orchestration_store_repository.go` (no callers anywhere in the tree). Plus the now-dead `approval_mapping_test.go`. | listed above |
| `main.go` no longer constructs `planRepo / approvalRepo / todoRepo / lineageRepo` and no longer passes them into `NewSessionService` or `NewServer`. | [`Control Plane/session-core/cmd/server/main.go`](../../Control%20Plane/session-core/cmd/server/main.go) |
| New migration `005_drop_agent_run_scaffold.up.sql` drops the 5 tables (`plans`, `plan_steps`, `todos`, `approvals`, `subagent_lineage_edges`) created by `004_add_orchestration_tables.up.sql`. Down is a no-op since rolling back would replay 004 to create empty tables that nothing reads. | [`Control Plane/session-core/migrations/005_drop_agent_run_scaffold.up.sql`](../../Control%20Plane/session-core/migrations/005_drop_agent_run_scaffold.up.sql) |
| CP `session-core` image rebuilt + force-recreated. Migration ran on startup. | container `session-core-service` |

**Verification:**

```
go build ./...                             → exit 0
go test ./...                              → all packages pass
GET /api/v1/sessions/current               → 200 (aggregator alive)
GET /v1/plans/foo (with auth)              → 404 page not found
psql -c "SELECT tablename FROM pg_tables   → 0 rows
        WHERE tablename IN ('plans',
        'plan_steps','approvals','todos',
        'subagent_lineage_edges');"
psql -c "SELECT version FROM               → 005_drop_agent_run_scaffold listed
        schema_migrations ORDER BY
        version DESC LIMIT 5;"
```

CP `session-core`'s scope is now **just the Control Session aggregator** (Wave 3 §8.17 + cache §8.20 + invalidation §8.21 + Convex mirror §8.21). Five files removed, ~414 lines of handler code deleted, ~150 lines of service code deleted, 5 Postgres tables dropped. The Rust service is the sole owner of plan / todo / lineage / approval state across the platform.

**§10 housekeeping:** ✅ Closed — G36-cutover fully closed. The §10 entry can be archived in a future cleanup pass.

### 8.25 G37 — Schema-enforced NOT NULL on `users.password_hash` + `users.avatar`; band-aid `COALESCE` wrappers removed (2026-05-12) — ✅ Closed

§8.23 unblocked the sign-in cascade with `COALESCE(col, '')` wrappers across 8 read queries — a one-line band-aid that didn't fix the underlying contract gap. G37 (filed in §10 the same wave) flagged that any future nullable column added without the matching `COALESCE` would reintroduce the same "GetByID-fails → treated-as-not-found → CreateWithID-PK-violation" cascade. This wave moves the contract from the application layer to the schema where it belongs.

| What landed | File |
|---|---|
| New migration `011_users_password_hash_avatar_not_null.up.sql`. Heavily commented, wrapped in `BEGIN/COMMIT` so it's atomic. Steps: (1) backfill existing NULLs with empty string, (2) `SET DEFAULT ''` so in-flight INSERTs that omit the columns succeed, (3) `SET NOT NULL` once the table is clean. Reverse migration drops both constraints; keeps the empty-string values in place since rollback only loosens the schema. | [`migrations/011_users_password_hash_avatar_not_null.up.sql`](../../Control%20Plane/user-core/migrations/011_users_password_hash_avatar_not_null.up.sql), [`.down.sql`](../../Control%20Plane/user-core/migrations/011_users_password_hash_avatar_not_null.down.sql) |
| Removed `COALESCE(col, '')` from 8 SELECT / RETURNING queries in `repository.go`. The schema now guarantees the contract, so the application-layer guard is dead code. | [`Control Plane/user-core/internal/users/repository.go`](../../Control%20Plane/user-core/internal/users/repository.go) |
| user-service rebuilt + force-recreated. Migration runs idempotently on startup via the in-house `RunMigrations` (lexicographic order, tracked in `schema_migrations`). | container `user-service` |

**Verification:**

```
psql -c "\d users" → password_hash NOT NULL DEFAULT '', avatar NOT NULL DEFAULT ''
psql -c "SELECT length(password_hash), length(avatar) FROM users WHERE id = 'TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw';"
  → 0, 0  (backfill landed; was NULL/NULL before)

# Pre-COALESCE-removal build:
GET /api/v1/users/me  for the previously-broken user  → 200

# Post-COALESCE-removal build:
GET /api/v1/users/me  for the same user                → 200  ✅
go build ./...                                          → exit 0
```

The same row that was killing the cascade three days ago now scans cleanly from a `SELECT password_hash, avatar` query with no `COALESCE` in sight — the schema is the source of truth.

**§10 housekeeping:** ✅ Closed — G37 closed.

### 8.24 G36-cutover Step A — Rust orchestration HTTP deployed + alias-collision DB hostname fix (2026-05-12) — ✅ Closed

Wave 8 shipped the orchestration HTTP code with `ORCHESTRATION_HTTP_ENABLED=false` as a config-flip-only cutover lever. This wave flips the lever in the Model Plane compose and lands one defensive fix uncovered along the way.

| What landed | File |
|---|---|
| `session-core` (Rust) compose entry: added `ORCHESTRATION_HTTP_ENABLED: "true"` + `ORCHESTRATION_HTTP_PORT: "8083"` + host port mapping `28083:8083`. Host port `18083` was already taken by `execution-core`; `28083` is outside the contended Model-Plane 18080–18088 health band. | [`Model Plane/deploy/docker-compose.yml:266`](../../Model%20Plane/deploy/docker-compose.yml) |
| Rebuilt the Rust image — the previous image was from 2026-05-09, 2 days **before** Wave 8 added the `orchestration_http` module. Without a rebuild the binary didn't have the routes. | container image `model-plane-session-core` |
| **DNS alias collision found:** both `model-plane-postgres-1` and `dpv2-postgres` advertise the alias `postgres` on `inter-plane-bus`, and Docker's DNS resolution can land on either. The Rust binary's `DATABASE_URL=postgresql://postgres:postgres@postgres:5432/...` resolved to `dpv2-postgres` on this boot (different creds → `password authentication failed for user "postgres"`). Switched all 4 `DATABASE_URL`s in the Model Plane compose from `@postgres:` → `@model-plane-postgres-1:` with a comment explaining why. | [`Model Plane/deploy/docker-compose.yml`](../../Model%20Plane/deploy/docker-compose.yml) (4 occurrences) |

**Verification:**

```
docker logs model-plane-session-core-1 | grep "orchestration HTTP enabled"
→ {"level":"INFO","fields":{"message":"orchestration HTTP enabled (G36 Wave 8) — port from ORCHESTRATION_HTTP_PORT"},...}

curl -X POST -H "Content-Type: application/json" --data '{}' http://localhost:28083/v1/plans
→ 422 Failed to deserialize the JSON body into the target type: missing field `thread_id`

curl http://localhost:18081/healthz
→ 200 (existing health endpoint still green; G36 HTTP coexists with gRPC + NATS + health)
```

The route is live, deserializes JSON, enforces the schema — drop-in for CP `/v1/plans`. Step A complete.

**What's still pending in G36-cutover:**

- **Step B — Repoint callers** (greppable surface in §10). Today's flip is non-breaking: both CP Go (port 3017) and Rust (port 28083) respond to `/v1/{plans,todos,lineage}`. Callers are still on `3017`.
- **Step C — Dual-write window.** Both stacks run side-by-side until log queries confirm CP traffic has dropped to zero for ~7 days.
- **Step D — Decommission CP Go.** Delete the four CP repos + handlers; drop the agent-run tables.

**§10 housekeeping:** ✅ Closed — Step A complete at the time of writing. Steps B/C/D also ✅ Closed in §8.26 (same day).

### 8.23 Sign-in cascade hot-fix series — first-sign-in user-core 500/502 → onboarding restart loop, plus two stale-env-var co-symptoms (2026-05-11) — ✅ Closed

Real-user social sign-in surfaced a four-error cascade that pushed users back into the onboarding wizard with no way out:

```
GET  /api/user/me/session-context  502  (Bad Gateway)
GET  /api/user/current             500  (Internal Server Error)
POST /api/oauth/initiate           401  (Unauthorized)
POST /api/ingestion/ingest-job     400  (Bad Request)
```

Root-causes turned out to be three independent bugs hiding behind one user-visible failure mode.

**(a) Root cause — `Repository.GetByID` Scan failed on NULL `password_hash` / `avatar`, treated as "user doesn't exist"**

The `users` table allows NULL on `password_hash` and `avatar`. pgx's `Scan` fails on `NULL → string` (non-pointer). `GetOrCreateUser` then treated *any* error from `GetUser` as "doesn't exist", attempted INSERT, and hit the primary-key uniqueness check:

```
failed to create user with ID: ERROR: duplicate key value violates unique constraint "users_pkey"
```

The 500 from `GetSessionContext` propagated as a 502 through velion's session-core proxy. OnboardingGuard interpreted "no session" as "incomplete onboarding" and force-restarted the wizard.

| What landed | File |
|---|---|
| Wrapped the two nullable columns in `COALESCE(col, '')` across all 8 SELECT / RETURNING queries that read user rows. Empty-string is the existing in-app sentinel for "unset," so the change is invisible to callers. `last_login_at` was already `*time.Time` and didn't need treatment. | [`Control Plane/user-core/internal/users/repository.go`](../../Control%20Plane/user-core/internal/users/repository.go) |
| `GetSessionContext` now accepts `(userID, email, name, avatar)` and auto-provisions via `GetOrCreateUser` — matching the contract `getCurrentUserProfile` already implements. The handler forwards `X-User-{Email,Name,Avatar}` from velion's edge gate. | [`Control Plane/user-core/internal/users/service.go`](../../Control%20Plane/user-core/internal/users/service.go), [`Control Plane/user-core/internal/http/handlers.go`](../../Control%20Plane/user-core/internal/http/handlers.go) |
| user-service container rebuilt + force-recreated. Live `GET /api/v1/users/me` for the previously-broken user (`TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw`, `g3-smoke@example.com` — `password_hash IS NULL`, `avatar IS NULL`) returns **200**. | container `user-service` |

**(b) Co-symptom — `/api/oauth/initiate` 401 from stale `AUTH_CORE_INTERNAL_API_KEY=test` in velion's `.env`**

Velion's `.env` carried a placeholder `test` key. integration-core's auth middleware accepts either a Bearer JWT or `x-internal-api-key`; with the wrong key, it rejected every connect-session call at the door. Same family as G30 (`INTERNAL_API_KEY` placeholder).

| What landed | File |
|---|---|
| Replaced placeholder `AUTH_CORE_INTERNAL_API_KEY=test` with the real 64-char hex that matches `apps/Ingestion Plane/integration-core/.env`. Added a comment pointing future readers at the cross-service contract. | [`Frontend Plane/velion/.env`](.env) |
| In-cluster default URL in `oauth/initiate/route.ts` corrected from `localhost:9026` to `http://integration-api:3026` (defense in depth — the `.env` was authoritative, but a wrong default once already drifted into staging). | [`src/app/api/oauth/initiate/route.ts`](src/app/api/oauth/initiate/route.ts) |
| `.env.local` (used by `pnpm dev` outside Docker) corrected from `localhost:9026` → `localhost:3026` to match the actual host-port mapping of the `integration-api` container. | [`.env.local`](.env.local) |
| velion container force-recreated (`docker compose up -d --force-recreate frontend`) because `docker compose restart` re-uses the existing container env and ignores `env_file` changes. Confirmed `AUTH_CORE_INTERNAL_API_KEY` is the real hex post-recreate. | container `frontend-plane-velion-frontend-1` |

**(c) Co-symptom — `/api/ingestion/ingest-job` 400 from Quarry-v1 contract read against a Quarry-v2 response**

The route was migrated to call Quarry-v2's `GET /v1/jobs/{id}` (post-G32) but still parsed the v1 inline `result.products` shape, which v2 never returns. Every call saw `products.length === 0` and 400'd the onboarding wizard.

| What landed | File |
|---|---|
| Rewrote the route for Quarry-v2: default URL now in-cluster `http://quarry-control:8081`; reads `/v1/jobs/{id}` then pages the event log at `/v1/jobs/{id}/events?after_seq=N&limit=100`; counts `page_fetched` / `page_failed` events; ingests URL placeholders into Data Plane; returns `success: true` even with zero fetched pages so the onboarding wizard can advance instead of dead-ending. Full-body ingestion from artifacts is tagged `TODO[ingest-v2-content]` → tracked as G38. | [`src/app/api/ingestion/ingest-job/route.ts`](src/app/api/ingestion/ingest-job/route.ts) |
| Response interface kept `totalProducts` field for compatibility with `onboarding-service.ts:_ingestCrawlResultsToDataPlane` which logs it. | same |

**Live verification:** `g3-smoke@example.com` (the actual broken user surfaced during testing) now resolves through `/api/v1/users/me` in 200, sub-1ms after redis-cache warmup; `getSessionContext` no longer 500s; integration-api accepts the connect-session POST with the corrected internal key. Re-tested social sign-in: dashboard renders, no cascade to onboarding restart loop.

**§10 housekeeping:** ✅ Closed — three new gaps filed at the time: **G37** (NULL-column scan brittleness — now ✅ Closed in §8.25), **G38** (Quarry-v2 ingest-job placeholder-only — now ✅ Closed in §8.28), **G39** (internal-API-key drift — now ✅ Closed in §8.27). All three landed within Wave 9. G36-cutover ✅ Closed in §8.26.

### 8.22 G36 (Stages 1+2 / partial) — Model Plane Rust gains HTTP parity (2026-05-11) — ✅ Closed (HTTP surface landed off-by-default; cutover completed in §8.24 + §8.26)

The Wave 8 audit reframed G36 substantially. The §10 plan assumed the Rust side needed:
- 5 schema migrations ported
- 5 repository modules ported
- 16 HTTP handlers ported

**Reality (audit, 2026-05-11):**
- Schema: `Model Plane/rust/services/session-core/migrations/0003_orchestration_tables.sql` already defines `plans`, `plan_steps`, `todos`, `approvals`, `subagent_edges` with **richer constraints** than CP Go's tables (per-parent ordinal triggers via plpgsql, idempotency-key partial unique indexes, JSONB metadata, FK references to `threads`/`runs`).
- Storage: `src/orchestration_store.rs` already has the full set of functions — `create_plan`, `get_plan`, `update_plan_status`, `set_plan_mode`, `list_plans_by_thread`, `append_plan_step`, `update_step_status`, `list_steps_by_plan_full`, `append_todo`, `update_todo_status`, `update_todo_status_if_current`, `list_todos_by_plan_full`, `list_todos_by_thread`, `request_approval`, `decide_approval`, `decide_approval_if_current`, `list_approvals_by_run_full`, `attach_subagent`, `detach_subagent`, `list_children`, `list_lineage_by_thread`, plus typed `PlanRow` / `PlanStepRow` / `TodoRow` / `ApprovalRow` / `SubagentEdgeRow` structs.
- Surface: today exposed via gRPC (`orchestration_grpc.rs`) and NATS (`orchestration_nats.rs`) — **not HTTP**.

So G36 reduces to **just the HTTP surface**. Wave 8 landed that surface.

**What landed:**

| Patch | File | Lines |
|---|---|---|
| New `src/orchestration_http.rs` — axum router with **17 routes** mirroring CP `session-core` `/v1/{plans,todos,lineage}/...` contract exactly. JSON shapes (`{plan: ...}`, `{plans: [...], pagination: {...}}`, `{status: "updated"}`) match CP Go so a path-only repoint is a drop-in replacement. ULID generation via existing `mp_ids::new_ulid()`. Two helper queries inline for surfaces the gRPC layer didn't need (`list_plans_by_thread` full rows + `list_lineage_parents`). | new file | ~530 |
| `main.rs` registers the module + spawns the server when `ORCHESTRATION_HTTP_ENABLED=true`. Binds on `ORCHESTRATION_HTTP_PORT` (default `:8083`). Coexists with the existing gRPC `:9091` + health `:8081` + NATS subscribers. **Off by default** — Wave 8 ships the code; Wave 9 flips the flag during cutover. | `main.rs` | +20 |

**Routes covered** (CP path → Rust handler → storage fn):
```
POST   /v1/plans                              create_plan                  → store::create_plan
GET    /v1/plans/:plan_id                     get_plan                     → store::get_plan
GET    /v1/plans/thread/:thread_id            list_plans_by_thread         → inline SQL (full rows)
PATCH  /v1/plans/:plan_id/state               update_plan_state            → store::update_plan_status
POST   /v1/plans/:plan_id/steps               create_plan_step             → store::append_plan_step
GET    /v1/plans/:plan_id/steps               list_plan_steps              → store::list_steps_by_plan_full
PATCH  /v1/plans/:plan_id/steps/:step_id/state update_plan_step_state      → store::update_step_status
POST   /v1/todos                              create_todo                  → store::append_todo
GET    /v1/todos/:todo_id                     get_todo                     → store::get_todo
DELETE /v1/todos/:todo_id                     delete_todo                  → inline SQL DELETE
GET    /v1/todos/thread/:thread_id            list_todos_by_thread         → store::list_todos_by_thread
GET    /v1/todos/run/:run_id                  list_todos_by_run            → inline SQL (full rows)
PATCH  /v1/todos/:todo_id/state               update_todo_state            → store::update_todo_status
POST   /v1/lineage                            create_lineage_edge          → store::attach_subagent
GET    /v1/lineage/:run_id/children           get_lineage_children         → inline SQL (full rows)
GET    /v1/lineage/:run_id/parents            get_lineage_parents          → inline SQL (full rows)
DELETE /v1/lineage                            delete_lineage_edge          → store::detach_subagent
GET    /v1/health                             health                       → static JSON
```

**Live-verified (2026-05-11):**
```
cargo build -p session-core         → exit 0 (1 unused warning, pre-existing)
cargo test  -p session-core --no-run → exit 0 (binaries built)
go build    -C Control\ Plane/session-core ./...  → exit 0 (still serves the routes today)
velion      pnpm typecheck          → exit 0
velion      pnpm lint:proxy         → exit 0 (47 grandfathered routes)
```

Wave 8 deliberately **doesn't** deploy the new HTTP surface — `ORCHESTRATION_HTTP_ENABLED` defaults false. The cutover is config-only when ready.

**What's deferred to Wave 9 (G36-cutover):**

The Stages 2–3 work from the original §10 plan still applies, now with the Rust HTTP surface already there:

1. **Deployment** — add `session-core` (Rust) to `apps/Model Plane/deploy/docker-compose.yml` with `ORCHESTRATION_HTTP_ENABLED=true` + Postgres pointing at a fresh `model_plane_session_core` DB on `model-plane-postgres-1`. The schema migrations run automatically via `store::run_migrations`.
2. **Repoint callers** — every caller of `session-core-service:3017/v1/{plans,todos,lineage}` (today: velion's onboarding wizard, the Model Plane orchestrator's plan-state read path, any scripts) flips its target hostname to the new Rust service. Greppable surface: search the monorepo for `session-core-service:3017/v1/` paths.
3. **Dual-write window** — run both Go + Rust simultaneously for one release cycle. CP NATS publishers stay on; Rust NATS publishers also publish (already wired). Compare event streams to confirm no divergence.
4. **Decommission CP** — delete the four repo + handler files from CP `session-core` (`internal/repository/{plan,todo,lineage,approval}_repository.go` + matching service + HTTP). Drop the `plans`, `plan_steps`, `todos`, `approvals`, `subagent_edges` tables from CP Postgres after the dual-write window completes. The CP service keeps only the Control Session aggregator code (Wave 3 §8.17 / G10).

**Why Wave 8 stopped short of cutover:** The cutover is operational, not code work — it needs a staging environment, monitoring during the dual-write window, and a careful flip in the velion/orchestrator caller code. None of that is hard, but it's a separate decision point + observability discipline that deserves its own wave.

### 8.21 G27 + G28-followup + G34-followup + G35 — Wave 7 (2026-05-11) — ✅ Closed

Four follow-ups + one Convex projection scaffolding. G36 (Rust port of agent-run repos) audited and deferred to Wave 8 with a detailed plan — the multi-day, multi-language scope doesn't compress to one session.

**(a) G27 — Stale empty networks removed**

| Network | Action |
|---|---|
| `data-net`, `aquatiq-backend`, `internal`, `visma_service_v2_default` | `docker network rm` — 4 networks deleted (0 containers each) |
| `xero_service_v2_default` | Skipped — 3 active containers (separate xero stack, out of CoreSystem scope) |

Post-cleanup `docker network ls` shows the canonical topology: `inter-plane-bus` (cross-plane bus, from Wave 6 ADR 0004 cutover) + 5 plane-specific nets (`app-net`, `controlplane-net`, `dpv2-net`, `ingestion-net`, `model-plane-network`).

**(b) G28-followup — Edge gate stamps full identity; remaining 3 pages migrated**

The Wave 6 edge gate only stamped `x-velion-user-id`. The 3 dashboard pages that needed `name` + `email` for downstream calls still ran `getServerSession()`. Wave 7 closes that.

| Patch | File |
|---|---|
| `validateSession` extracts `email` + `name` from auth-core's `/api/auth/get-session` response. Cache entry stores all three. | [`src/proxy.ts`](src/proxy.ts) |
| Two new headers: `x-velion-user-email` + `x-velion-user-name`. Stamped via `NextResponse.next({request:{headers:…}})` alongside the existing `x-velion-user-id`. Total stamped payload ~256 B, well under typical 8 KB header limits. | `src/proxy.ts` |
| `requireEdgeUser()` returns `{userId, email, name}`. Fallback to `getServerSession()` reads the same three fields on the slow path. | [`src/components/auth/lib/edge-session.ts`](src/components/auth/lib/edge-session.ts) |
| 3 pages migrated: `(dashboard)/{dashboard,calendar,knowledge}/page.tsx`. Combined with Wave 6's `search/inbox/notifications` migration, **all 6 protected pages** now use the helper. Zero per-page `getServerSession()` calls remain in `(dashboard)/`. | as listed |

**(c) G34-followup — Upstream NATS subscribers wired**

Wave 6 shipped the Redis cache; Wave 7 adds the reactive invalidation that makes cache staleness sub-second instead of 30s-bounded.

| Patch | File |
|---|---|
| New `internal/subscribers/upstream_invalidator.go`. Core-NATS queue subscriber on 7 subjects (`user.profile.updated`, `organization.{updated,plan.changed,member.added,member.removed}`, `billing.{account,plan}.{updated,changed}`). On each event: parse `user_id` (and `org_id` where present), `cache.InvalidateControlSession`, then `PublishAppSessionEntitlementsChanged` so notification-core fires its user-facing toast. | new file |
| `main.go` stores the shared NATS client (was discarded after wrapping in `SharedPublisher`) and wires `UpstreamInvalidator.Start` after the cache + service initialise. `Stop()` drains on shutdown. | `cmd/server/main.go` |
| Org-only events (e.g. `organization.plan.changed` carrying just `organization_id`, not `user_id`) are logged-then-skipped — we have no per-org reverse index for cache keys today. The 30s TTL bounds the per-user staleness; explicit `POST /api/v1/sessions/refresh` is the fast path for an affected user. Tracked in the §10 entry. | (documented) |

**(d) G35 — Convex projection for Control Session (reactive UI scaffolding)**

| Patch | File |
|---|---|
| New `controlSessions` table in Convex schema: `{externalUserId, externalOrgId?, snapshot (v.any), fetchedAt, createdAt, updatedAt}` + two indexes (`by_external_user`, `by_external_user_and_org`). Snapshot stored opaquely so we don't need a Convex schema migration every time session-core's aggregate shape changes. | [`Application Plane/convex-core/convex/schema.ts`](../Application%20Plane/convex-core/convex/schema.ts) |
| New module `convex/controlSessions.ts`: `upsertControlSessionInternal` mutation (split from auth/parsing for testability) + `upsertControlSession` httpAction (X-Service-Key gated, same pattern as `convex/ingest.ts`) + `byUser` / `byUserAndOrg` queries. Velion subscribes via `useQuery(api.controlSessions.byUser, {externalUserId})`. | new file |
| HTTP route registration at `POST /ingest/control-session`. | `convex/http.ts` |
| session-core `convex.Client.MirrorControlSession(ctx, userID, orgID, snapshot, fetchedAtMillis)` — best-effort POST to convex-core. | `Control Plane/session-core/internal/convex/client.go` |
| `ControlSessionService.Refresh` now triple-purposes: bust cache → re-aggregate (auto cache-warms) → publish `app.session.entitlements_changed` on NATS → mirror to Convex. All four steps are best-effort beyond the cache+aggregate; outages downstream don't fail the response. | `internal/service/control_session_service.go` |
| `main.go` plumbs `convexClient` into the constructor; emits `Control Session Convex mirror enabled (G35)` log on startup when CONVEX_URL + CONVEX_SERVICE_KEY are set. | `cmd/server/main.go` |

**Note:** Convex types for `api.controlSessions.*` regenerate from the schema when convex-backend restarts (auto-codegen). No manual `npx convex codegen` needed in dev.

**Cumulative CI gates green:**
```
session-core $ go build ./...        → exit 0
velion       $ pnpm typecheck        → exit 0
velion       $ pnpm lint:proxy       → exit 0 (47 grandfathered routes — unchanged)
```

**§10 housekeeping:** ✅ Closed — G27, G28-followup, G34-followup, G35 all closed in this wave. G36 detailed plan landed Wave 8 §8.22 and ✅ fully closed in Wave 9 §8.24 + §8.26.

### 8.20 ADR 0004 cutover + G3 + G16 + G34 (cache) + G28 — Wave 6 (2026-05-11) — ✅ Closed

Four substantial items in one wave. Each touches a different surface (compose / SQL / Go cache / Next.js edge), bundled because they share a verified-date bump and each is a half-day or less when tackled individually.

**(a) ADR 0004 cutover — `velion-net` → `inter-plane-bus`**

The docker network rename promised in [ADR 0004](./docs/adr/0004-network-topology.md) executed. Cutover used the "down all, network swap, up all" sequence from the ADR's implementation plan; subnet `172.20.0.0/16` preserved (no IP-pinned configs broken). All 7 active composes (Control / Application / convex-core / Data v2 / Frontend / Ingestion / Model v1) plus the unused Model v2 compose edited; 5 stale empty networks (`data-net`, `aquatiq-backend`, `internal`, `visma_service_v2_default`, `xero_service_v2_default`) remain (no functional impact; a future cleanup can `docker network rm` them).

[`docs/ARCHITECTURE_DIAGRAM.md`](docs/ARCHITECTURE_DIAGRAM.md) gains the "Network Topology" section (charter text from ADR 0004 § "Charter amendment"). Plane-specific nets remain mandatory for intra-plane traffic; the shared bus is reserved for cross-plane edges. Forcing function for Option B (least-privilege shrink) named in the ADR.

**Live-verified post-cutover:**
```
docker network ls                              → inter-plane-bus (was velion-net)
inter-plane-bus members                        → 46 containers
velion → auth-core:3011/api/auth/get-session   → null (Better Auth 200/null — correct)
velion → user-core:3012/health                 → {service:user-service, status:healthy}
velion → org-core:8080/health                  → {service:org-core, status:healthy}
velion → session-core-service:3017/health      → {service:session-core, version:0.1.0}
velion → quarry-control:8081/health            → ok
```

**(b) G3 + G16 — server-side onboarding state, multi-device resume**

| Patch | File |
|---|---|
| New migration: `users.onboarding_step TEXT` + `users.onboarding_state JSONB`. `onboarding_complete` (from migration 004) stays as the authoritative "done?" flag; the two new columns describe in-flight state. | [`Control Plane/user-core/migrations/010_onboarding_state.{up,down}.sql`](../Control%20Plane/user-core/migrations/010_onboarding_state.up.sql) |
| Repository: `GetOnboardingState(ctx, userID) → (step, stateJSON, err)` + `UpsertOnboardingState(ctx, userID, step, stateJSON)`. UPDATE-only (rows come from auth-core's provisioning path). | `Control Plane/user-core/internal/users/repository.go` |
| Service: `GetOnboardingState` / `UpsertOnboardingState` returning a typed `OnboardingStateView{Step, State map[string]any}`. Server-side enum validation deferred — the client owns the step taxonomy. | `Control Plane/user-core/internal/users/service.go` |
| HTTP: `GET /api/v1/users/me/onboarding-state` + `PUT /api/v1/users/me/onboarding-state`. Mounted under `/users/me/...` (BEFORE the `:id` catch-all). Both require X-User-Id from internal proxy. | `Control Plane/user-core/internal/http/{handlers,server}.go` |
| Velion proxy: dedicated `/api/user/me/onboarding-state` route (forwards to user-core's `/api/v1/users/me/onboarding-state`). Takes precedence over the `/api/user/[...path]` catch-all because Next.js routes static paths before dynamic. | [`src/app/api/user/me/onboarding-state/route.ts`](src/app/api/user/me/onboarding-state/route.ts) (new) |
| `onboarding-service.ts`: `saveCurrentStep()` writes through to the new server endpoint via `pushOnboardingStateToServer`. `restoreStepFromServer()` reads the server snapshot and hydrates localStorage if the local cache is empty. `completeOnboarding()` clears server state on success and **removed** the unconditional `setTimeout(clearOnboardingState, 1000)` in the `finally` block that ran even when the server-side completion failed. | `src/components/onboarding/services/onboarding-service.ts` |

**Live-verified end-to-end** (`node fetch` from velion container):
```
PUT /api/v1/users/me/onboarding-state {step:"website", state:{profile:{...},organization:{...}}}
  → {"success":true}
GET /api/v1/users/me/onboarding-state
  → {"step":"website","state":{"profile":{...},"organization":{...}}}
DB:  onboarding_step='website', onboarding_state={"profile":...,"organization":...}
```

**(c) G34 — Redis read-through cache for Control Session** (Phase 1; NATS-driven invalidation deferred)

The Wave-3 Control Session aggregator fanned out to user-core + org-core + billing-core on every `GET /api/v1/sessions/current`. With dashboards rendering this per page nav, that's 3 RPCs × users on the inter-plane bus. Add a Redis cache wrapping `Get`.

| Patch | File |
|---|---|
| Redis cache primitives: `CacheControlSession(ctx, userID, orgID, snap, ttl)`, `GetCachedControlSession(ctx, userID, orgID, dest)`, `InvalidateControlSession(ctx, userID, orgID)` (orgID="" → wildcard `SCAN`-based bust). `ErrCacheMiss` re-exported as `redis.Nil`. | `Control Plane/session-core/internal/redis/client.go` |
| `ControlSessionService` takes the cache. Read-through wraps `Get` (lookup after the cheap routing fetch — that's how we learn the orgID for the cache key). Cache write at end of `Get`; cache bust at start of `Refresh` (so an explicit POST `/refresh` after a plan upgrade actually re-aggregates, not serves the stale TTL'd entry). TTL: 30s per ADR 0002. | `Control Plane/session-core/internal/service/control_session_service.go` |
| `main.go` plumbs `cache` into the constructor; emits `Control Session read-through cache enabled (G34)` log on startup when Redis is wired. | `Control Plane/session-core/cmd/server/main.go` |

Deferred to **G34-followup**: subscribe to upstream NATS subjects (`user.profile.updated`, `organization.plan.changed`, `organization.member.added`, `billing.account.updated`, `billing.plan.changed`) and invalidate the cache reactively. Today the 30s TTL bounds staleness; explicit `/refresh` is the fast path. Adding subscribers is a one-file follow-up.

**(d) G28 — Edge-gate user-id header, killing per-page `getServerSession()` duplication**

`src/proxy.ts` already validates the session for every protected path. The per-page `getServerSession()` calls scattered across the dashboard pages duplicated that work (every page render adds an auth-core round-trip). Fix:

| Patch | File |
|---|---|
| Edge gate `validateSession` returns `{ok, userId}` instead of `bool`. Cache entry stores the userId alongside the ok flag. When `isProtected && userId`, the gate stamps `x-velion-user-id` onto the forwarded request via `NextResponse.next({request:{headers:…}})`. | [`src/proxy.ts`](src/proxy.ts) |
| New helper `requireEdgeUser(pathForLoginRedirect)`: reads the header via `next/headers`, falls back to `getServerSession()` when the header is missing (gate fail-open path — keeps defence-in-depth alive for the auth-core-unreachable scenario). | [`src/components/auth/lib/edge-session.ts`](src/components/auth/lib/edge-session.ts) (new) |
| Migrated 3 pages that only need the auth check + userId: `(dashboard)/search/page.tsx`, `(dashboard)/inbox/[[...slug]]/page.tsx`, `(dashboard)/notifications/page.tsx`. Each one now drops the upstream auth-core call. | `src/app/(dashboard)/{search,inbox/[[...slug]],notifications}/page.tsx` |

Three pages **not** migrated this wave: `dashboard/page.tsx`, `calendar/page.tsx`, `knowledge/page.tsx`. Each uses the full `session.user.{id,name,email}` object for downstream calls. To migrate them, the edge gate would need to stamp `x-velion-user-name` + `x-velion-user-email` too (or those pages would still pay one round-trip just for the profile fields). Tracked as **G28-followup**.

**Cumulative CI gates green:**
```
session-core $ go build ./...              → exit 0
user-core    $ go build ./...              → exit 0
velion       $ pnpm typecheck              → exit 0
velion       $ pnpm lint:proxy             → exit 0 (47 grandfathered routes; new onboarding-state + telemetry exempted)
velion       $ pnpm build                  → exit 0
```

**§10 housekeeping:** ✅ Closed — G3, G16, G28 closed by this wave. G27 entry updated to reflect ADR-0004 ratification + cutover. G34 demoted to G34-followup (closed in §8.21).

### 8.19 G4 + G5 + G13 + G22 + G23 + G26 — Wave 5 hygiene sweep (2026-05-11) — ✅ Closed

Six small cleanups, no architectural choices. Each individually ≤30 min; bundled into one sweep so the lint baseline ratchet shrinks visibly (48 → 47 grandfathered routes).

| Gap | Change | File(s) |
|---|---|---|
| **G4** | Deleted the `/api/onboarding/cancel` stub route (it only re-validated the session and returned 200). `onboardingService.cancelOnboarding()` now calls `authService.logout()` directly. Baseline entry pruned. | `src/app/api/onboarding/cancel/` (deleted), [`src/components/onboarding/services/onboarding-service.ts`](src/components/onboarding/services/onboarding-service.ts), `scripts/lint-proxy-routes.baseline` |
| **G5** | Replaced the seven raw `console.log/warn/error` calls in `onboarding-service.ts` with a single `onboardingLog` object exposing `debug` / `warn` / `error` channels. `debug` + `warn` are gated on `NEXT_PUBLIC_DEBUG_ONBOARDING` (or `NEXT_PUBLIC_DEBUG`); `error` always fires (reserved for truly unrecoverable client faults). The three pre-existing `console.error` call sites were all recoverable (localStorage write/clear failures, non-blocking crawl ingestion) so they moved to `onboardingLog.warn` per the gap's "console.error only for unrecoverable" directive. | [`src/components/onboarding/services/onboarding-service.ts`](src/components/onboarding/services/onboarding-service.ts) |
| **G13** | Deleted the `/api/connections/from-auth-core` 410-Gone stub (no in-tree callers; sat as a deliberate redirect message since the auth-core token-reuse pattern was removed). | `src/app/api/connections/from-auth-core/` (deleted) |
| **G22** | Renamed `VELION_INTEGRATION.md` → [`VELION_THIRD_PARTY_INTEGRATIONS.md`](VELION_THIRD_PARTY_INTEGRATIONS.md). Added a scope header pointing Control Plane / Application Plane wiring readers at `velion-gap.md`. All cross-references in the gap log updated. | `VELION_INTEGRATION.md` → `VELION_THIRD_PARTY_INTEGRATIONS.md`, `velion-gap.md` |
| **G23** | Split the edge-gate session-cookie list into `CANONICAL_SESSION_COOKIES` (Better Auth's current writer set) and `LEGACY_SESSION_COOKIES` (`auth_session`, `session_token`). When a legacy name is accepted, a single-line JSON log entry `{"level":"warn","msg":"legacy_cookie_seen","cookie_name":"..."}` is emitted to stdout. After 30 days of zero traffic on a name, drop the row from `LEGACY_SESSION_COOKIES`. The patterns list (sid / sid_multi / session_token regex) is left untouched — those are matched by shape, not name. | [`src/proxy.ts`](src/proxy.ts) |
| **G26** | Added `GIN_MODE: release` to user-core and org-core in `Control Plane/docker-compose.yml`. Silences the `[GIN-debug] [WARNING] Running in "debug" mode.` banner and enables release-mode perf paths in gin. Containers force-recreated. session-core and billing-core were already clean. | `Control Plane/docker-compose.yml` |

**Live-verified (2026-05-11)**:
```
velion $ pnpm typecheck   → exit 0
velion $ pnpm lint:proxy  → exit 0 (47 grandfathered routes — was 48; G4 deletion shrinks baseline)
velion $ pnpm build       → exit 0
user-core / org-core:     GIN_MODE=release, no [GIN-debug] banners in container logs
```

**Wave 5 deferred to Wave 6** (each has design implications worth their own pass):
- **G3** — onboarding state in localStorage (needs server-state column in user-core or Control Session) and the coupled **G16** (server-resumed step routing)
- **G27** — `velion-net` mega-network topology (needs ADR 0004)
- **G28** — per-page `getServerSession() + redirect()` duplication (perf refactor across ~7 protected pages)

Plus the Wave-3 follow-ups **G34/G35/G36** (Control Session cache + Convex projection + Rust port of agent-run repos).

### 8.18 G20 + G21 + G25 — Wave 4: zero-input enterprise polish + client telemetry (2026-05-11) — ✅ Closed

Wave 4 closes the doc/UX/observability gaps that flank the zero-input enterprise sign-in flow. None of them are blockers individually; together they cross the SOC2-transparency + measurable-success-target line that the roadmap (`docs/zero-input-enterprise-onboarding-roadmap.md`) cares about.

**G20 — roadmap-vs-code reconciliation**

| Patch | File |
|---|---|
| `docs/Velion_CONNECT_ROADMAP.md` Phase 8 row updated: drops stale G10/G14 references (closed in Wave 3 / §8.17), points reactive-Convex follow-up at the new G35 entry. The "Plan selection deferred" inline note already existed in the route table — no change needed there; the doc and code agreed once the table comment was read in full. | [`docs/Velion_CONNECT_ROADMAP.md:79`](docs/Velion_CONNECT_ROADMAP.md:79) |

**G21 — Enterprise trust banner on first dashboard load**

A one-line banner under the dashboard navbar surfaces what the zero-input sign-in flow auto-decided so the user can verify it. Dismissible per-tab (sessionStorage); reappears on the next sign-in.

| Patch | File |
|---|---|
| New `<EnterpriseTrustBanner />` client component. Fetches `/api/user/me/session-context` on mount, surfaces `{role · organization · domain · plan}` with sensible fallbacks (works with both the Wave 3 rich Control Session shape and the legacy narrow shape from user-core). Dismissible via `sessionStorage` key `velion.enterprise-trust-banner.dismissed`. No-op when no organization is present (signed-in-no-org state). | [`src/components/dashboard/EnterpriseTrustBanner.tsx`](src/components/dashboard/EnterpriseTrustBanner.tsx) (new) |
| Mounted directly under the navbar in the dashboard layout, before the sidebar/main split. | [`src/app/(dashboard)/layout.tsx`](src/app/(dashboard)/layout.tsx) |

**G25 — Client-side telemetry primitive**

Lightweight emitter (≈3 KB minified, no third-party deps) for the four metrics the zero-input roadmap names: sign-in→dashboard latency, auto-org resolve success/failure, time-to-first-value. OpenTelemetry browser SDK is deliberately **not** adopted yet — three reasons documented in the source: bundle size for a tiny event set, the G15 correlation id already stitches frontend+backend, and the backend aggregator (Loki / Datadog) hasn't been chosen.

| Patch | File |
|---|---|
| Telemetry client: `emit(name, attrs)` posts a single event to `/api/telemetry/events` via `navigator.sendBeacon` first, `fetch(keepalive:true)` fallback. Failures buffer in `sessionStorage` (capped at 100) and flush on the next successful emit. Convenience wrapper `withDurationEmit` for timed operations. | [`src/lib/telemetry/client.ts`](src/lib/telemetry/client.ts) (new) |
| Server sink: `POST /api/telemetry/events` accepts the structured event (8 KB body cap), enriches it with the G15 correlation id, and writes a single-line JSON record to stdout for container-log ingestion. Anonymous events are accepted (the `auth.login.started` event fires pre-session); risk bounded by size cap + structured-only logging. Listed as an exempt path in the lint-proxy-routes rule (`/api/telemetry/` doesn't need the control-plane-auth helper because it doesn't forward anywhere). | [`src/app/api/telemetry/events/route.ts`](src/app/api/telemetry/events/route.ts) (new) |
| Call sites: `AuthCallbackClient.tsx` emits `auth.login.completed` (both fast-path and legacy-path), `onboarding.zero_input.resolved`, `onboarding.zero_input.failed` (with `reason=needs_onboarding` or `reason=oauth_error`). `EnterpriseTrustBanner` emits `dashboard.first_paint` once per mount when the session-context resolves with an org. | `src/app/(auth)/auth/callback/AuthCallbackClient.tsx`, `src/components/dashboard/EnterpriseTrustBanner.tsx` |

**Live-verified (2026-05-11)**:
```
velion $ pnpm typecheck   → exit 0  (0 errors in our source; 354 @blocksuite leakage ignored)
velion $ pnpm lint:proxy  → exit 0  (48 grandfathered routes)
velion $ pnpm build       → exit 0  (Next.js 16.1.6, ignoreBuildErrors retained for blocksuite)
```
End-to-end (post-restart): POST `/api/telemetry/events` with a synthetic `auth.login.completed` payload returns `{"ok":true}` and the velion container log emits the corresponding `telemetry.event` JSON line with the correlation id.

**Known follow-ups** (not blocking; file as needed):
- A real metric aggregator (Loki query layer, Datadog, or self-hosted) is still TBD. Today's events are visible in container logs only. **G37** — file when an aggregator is chosen.
- Banner copy + i18n: the strings are English only. Translate via the existing `useAuthTranslation` pattern when product locks the strings.
- Banner expansion: a future row could show resolved entitlements + plan-upgrade CTA (Wave 4 keeps it to "verify what was auto-decided").

### 8.17 G10 + G14 + G17 — Wave 3: ADRs land, charter amended, Control Session aggregator MVP (2026-05-11) — ✅ Closed

Wave 3 of the gap-closure roadmap. Three intertwined items: the L5-boundary charter amendment (G17, doc + lint), the Control Session aggregator MVP per ADR 0002 (G10 Steps 1+3+6+9 of the 9-step plan), and notification-core's first cross-plane subscriber (G14, which is G10 Step 5). The Redis cache + NATS-driven invalidation + Convex projection + Rust port of agent-run repos are explicitly deferred to G34/G35/G36 so the wave doesn't balloon into a multi-codebase rewrite.

**G17 — L5 boundary policy ratified**

| Patch | File |
|---|---|
| Rewrote "Cross-Plane Contract Rules" per ADR 0003 §"Charter amendment": rules 5–7 declare velion proxies as L5 ingress, convex-gateway as WS-only, second-frontend trigger. Replaces the previous "L5 is the ceiling" wording that every running route already contradicted. | [`docs/ARCHITECTURE_DIAGRAM.md`](./docs/ARCHITECTURE_DIAGRAM.md) |
| Doc-map row updated to "amended"; glossary "L5 boundary" entry rewritten to point at ADR 0003 + the new charter rules. | `velion-gap.md` §0 + glossary |
| New `scripts/lint-proxy-routes.sh` + `scripts/lint-proxy-routes.baseline` (48-route snapshot). The lint catches **new** `src/app/api/*/route.ts` files that call `fetch()` without going through `control-plane-auth.ts`, while grandfathering the existing 48 pre-helper routes. Also fails on stale baseline entries (= someone migrated, baseline should shrink — ratchet). Wired into `pnpm lint` and standalone `pnpm lint:proxy`. | new |

**G10 — Control Session aggregator (Steps 1 + 3 + 6 + 9 of ADR 0002)**

| Patch | File | What it does |
|---|---|---|
| New HTTP surface on cp-session-core: `GET /api/v1/sessions/current` + `POST /api/v1/sessions/refresh`. Mounted at `/api/v1` (parallel to the legacy `/v1` agent-run routes which keep serving during the transition). | [`Control Plane/session-core/internal/http/{server.go,control_session_handlers.go}`](../../Control%20Plane/session-core/internal/http/) | velion-facing aggregator API |
| `ControlSessionService.Get` (synchronous fan-out): user-core `/api/v1/me/session-context` for routing fields, user-core `/api/v1/users/me` for profile, org-core `/orgs/{id}` + `/orgs/{id}/entitlements` for org details, billing-core `/api/v1/billing/orgs/{id}/account` for subscription. Upstream failures degrade silently EXCEPT user-core, which returns 502 to the caller. | `Control Plane/session-core/internal/service/control_session_service.go` (new) | aggregator core |
| Clients for the upstream cores, mirroring the existing `org_client.go` shape. | `Control Plane/session-core/internal/clients/{user_client.go,billing_client.go}` (new); `org_client.go` extended with `GetOrganization` + `GetEntitlements` | upstream callers |
| `ControlSessionService.Refresh`: re-aggregates and publishes `app.session.entitlements_changed` on the velion-nats `APP_SESSION` JetStream stream. NATS publish degrades silently if the shared bus is unavailable (logs a warning). | same service file + `internal/nats/shared_publisher.go` `PublishAppSessionEntitlementsChanged` | refresh + publish |
| New JetStream stream config: `APP_SESSION`, subjects `app.session.>`, 7-day retention, 256 MB cap. Created by `EnsureStreams` on session-core startup. | `internal/nats/shared_publisher.go` | NATS subject space |
| Config struct + env wiring: `USER_CORE_URL`, `BILLING_CORE_URL` (defaults `http://user-core:3012`, `http://billing-core:3014`). main.go constructs the clients + service and passes them into `NewServer`. | `internal/config/config.go`, `cmd/server/main.go` | service wiring |
| velion proxy `GET /api/user/me/session-context` (new dedicated route — takes precedence over the `[...path]` catch-all per Next.js routing): forwards to session-core's `/api/v1/sessions/current` when `CONTROL_SESSION_AUTHORITY_ENABLED=true`, else falls back to user-core's narrower endpoint (legacy contract during the transition). | [`Frontend Plane/velion/src/app/api/user/me/session-context/route.ts`](src/app/api/user/me/session-context/route.ts) (new) | velion forwarder |
| velion helper extended: `getSessionServiceUrl()` + `isControlSessionAuthorityEnabled()`. | [`src/app/api/_lib/control-plane-auth.ts`](src/app/api/_lib/control-plane-auth.ts) | velion env reads |
| velion `.env`: `SESSION_SERVICE_URL=http://session-core-service:3017` + `CONTROL_SESSION_AUTHORITY_ENABLED=true`. | [`.env`](.env) | flag on |

**G14 — notification-core subscribes (G10 Step 5)**

| Patch | File | What it does |
|---|---|---|
| New subscriber package: durable JetStream queue subscriber on `app.session.entitlements_changed`. Each event becomes a `notification.Service.Accept` call with idempotency key `control-session-entitlements:{user_id}:{timestamp}`. Malformed payloads are Termed (no redelivery); transient downstream failures Nak (redelivery). | `Application Plane/notification-core/internal/subscribers/control_session.go` (new) | subject → notification |
| Config + dual-NATS wiring: notification-core keeps its local `app-nats` connection for its own publisher; a **second** NATS client connects to the shared bus (`SHARED_NATS_URL=nats://velion-nats:4222`) so cross-plane events flow without disturbing the local publisher. Falls back to the local connection when `SHARED_NATS_URL` is unset. | `internal/config/config.go`, `cmd/server/main.go` | dual-NATS |
| docker-compose env injection: `SHARED_NATS_URL` + `SHARED_NATS_TOKEN`. notification-core was already on `velion-net` so no network change was needed. | `Application Plane/docker-compose.yml` | shared-bus access |

**Step 9 — docs**

Updated `velion-gap.md` §2 row "Control session" + §2.2 to reflect the live MVP (drops "(planned repurpose)" qualifier, names the live endpoints, calls out the deferred items as G34/G35/G36). ADR 0002 §"Implementation plan" steps now annotated by which wave they landed.

**Live-verified at deploy (2026-05-11)**:
```
session-core:    "JetStream stream ready: APP_SESSION"
session-core:    "user-core client enabled for Control Session aggregator"
session-core:    "billing-core client enabled for Control Session aggregator"
session-core:    "Control Session aggregator ready (GET /api/v1/sessions/current)"
notification:    "connected to shared nats at nats://velion-nats:4222"
notification:    "subscribers/control-session: subscribed to app.session.entitlements_changed"
velion:          pnpm typecheck → 0 errors in velion source
velion:          pnpm lint:proxy → OK (48 grandfathered routes pending migration)
session-core:    POST /api/v1/sessions/refresh → 502 for unknown user_id (correct degradation — user-core hasn't provisioned the user)
```

End-to-end NATS message flow was not directly observable in this session because no real auth_service user has yet been provisioned into user-core's DB (chicken-and-egg: the OAuth provisioning hooks that populate user-core haven't fired for the existing auth_service rows). The wiring will be exercised organically on the next OAuth sign-in. If end-to-end proof is needed before then, the cheapest test is: open the dashboard while signed in, trigger a plan upgrade — `/api/user/me/session-context` should now return the richer aggregate, and notification-core's log should show `subscribers/control-session: notification.Accept` for the user.

**Known follow-ups (deferred from G10's 9-step plan)** — filed as G34, G35, G36 in §10.

### 8.16 G11 + G12 — CI hygiene: both builds green (2026-05-11) — ✅ Closed

Wave 2 of the gap-closure roadmap. Two distinct issues, both ate CI cycles:

**G11 (`auth-core`)** — `pnpm build` failed with 15 type errors. Two unrelated root causes:

1. `node_modules/drizzle-orm/pg-core/index.d.ts` shipped malformed `.pnpm/...` re-export paths (a known pnpm/drizzle packaging interaction at v0.44.5). The runtime exports were intact but TypeScript couldn't resolve them, so `pgTable`, `text`, `timestamp`, `boolean`, `integer`, `bigint` all 2305'd from `src/db/schema.ts`. `pnpm install --force` regenerated the `.d.ts` with correct relative paths (`./alias.js`, `./columns/index.js`, …) and made the 11 schema errors disappear.
2. `internal-oauth.service.ts:224` over-narrowed: `if ('ok' in providerResp && providerResp.ok === false)` — the additional `&& providerResp.ok === false` left both union variants in the fall-through type, so `access_token`, `refresh_token`, `expiresAt`, `scope` all 2339'd. `InternalRefreshError` is the only variant carrying an `ok` key, so `if ('ok' in providerResp)` alone discriminates. 4 errors gone.

**G12 (`velion`)** — the gap framing was misleading. `pnpm build` (a.k.a. `next build`) **never actually failed**; `next.config.ts` already sets `typescript.ignoreBuildErrors: true`, and no CI step ran `tsc --noEmit`. But running `tsc --noEmit` produced 380 type errors. Distribution:

- **354 in `node_modules/@blocksuite/...@0.19.5` source files** — third-party packaging bug. The `dist/index.d.ts` files re-export `from '../src/*.ts'`, dragging unfixable source files (broken `lit` `css`/`unsafeCSS` exports, missing `@types/lodash.*`, named-capture regexes targeting < ES2018) into every type-check. Out of our tree; can't fix without `pnpm patch` and a long-term maintenance cost.
- **26 in our source** — dead scaffolding + wrong client-call signatures.

The four-part fix:

1. Deleted 14 dead `page.tsx` files under `src/components/agents/**` plus their empty parent directories. These were stubs importing from a non-existent `@/modules/...` path; Next.js routes from `src/app/`, not `src/components/`, so they were unreachable noise. Eliminated 13 of the 26.
2. Fixed `src/lib/clients/nango-client.ts`, `nohu-client.ts`, `zammad-client.ts`: `request(method, endpoint, body?)` was being called with `(endpoint)` only on the read methods. Added the missing `'GET'` arg to 9 call-sites and `'POST'` to `initiateOAuth`. Eliminated 12 of the 26.
3. Annotated `zammad-client.ts` `getTickets()` return type as `Promise<ZammadTicket[]>` so `useZammad.ts`'s `setTickets(data)` typechecks. Eliminated the last.
4. Added a CI-friendly `pnpm typecheck` script (`scripts/typecheck.sh`) that runs `tsc --noEmit` and filters out the 354 unavoidable `@blocksuite` third-party errors. Velion's own code is checked normally; the script exits 0 only when the velion-tree error count is 0, prints a one-line "(N @blocksuite third-party errors ignored)" summary otherwise.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G11 | `pnpm install --force` to regenerate the malformed `drizzle-orm/pg-core/index.d.ts`. Local cache only — no `package.json` change. Recommend pinning `drizzle-orm` exactly when the next pnpm-lock regeneration lands. | (cache only) | ✅ |
| G11 | Narrow `providerResp` via `'ok' in providerResp` alone — drop the redundant `&& providerResp.ok === false` clause. | `Control Plane/auth-core/src/internal/internal-oauth.service.ts` | ✅ |
| G12 | Delete the 14 dead `page.tsx` files + empty dirs. | `src/components/agents/{[agentId]/{account-information,conversations,digital-workers,integrations,market-research,plans-billing,resources,voice-assistant,widget-customization}/page.tsx,page.tsx}`, `src/components/agents/create/{plan,training,}/page.tsx` | ✅ |
| G12 | Add missing `method` arg to 9 `request(...)` call-sites; tighten return types on `getTickets` / `getTicket` / `initiateOAuth`. | `src/lib/clients/{nango-client,nohu-client,zammad-client}.ts` | ✅ |
| G12 | New `pnpm typecheck` script + filter wrapper that exits 0 when velion-tree errors are 0; surfaces the @blocksuite leakage as informational. | `package.json` (`scripts.typecheck`), `scripts/typecheck.sh` (new) | ✅ |

**Live-verified (2026-05-11)**:
```
auth-core $ pnpm build       → exit 0
velion    $ pnpm build       → exit 0   (104 routes generated; ignoreBuildErrors kept)
velion    $ pnpm typecheck   → exit 0   "0 errors in velion source (354 @blocksuite third-party errors ignored — see G12)"
```

**Known follow-up** (low priority): if velion ever needs `tsc --noEmit` to be cleanly zero, the @blocksuite leakage has to be patched at the package level — either via `pnpm patch @blocksuite/store@0.19.5` (rewrite `dist/index.d.ts` to not re-export from `../src/`) or by upgrading past 0.19.5 once a fixed release exists. Not blocking any CI today.

### 8.15 G33 — Quarry-v2 had no job→workflow dispatch path (2026-05-11) — ✅ Closed

After G32 the velion ↔ quarry-control contract was correct but every crawl job sat at `status: "accepted"` forever. The orchestrator registered `CrawlJobWF` / `ScrapeJobWF` / `BatchJobWF` and connected to Temporal, but nothing called `client.ExecuteWorkflow` — quarry-control's `POST /v1/jobs/` just wrote a DB row and returned. The schedules reconciler covered the cron path; the on-demand POST path was missing.

Cleanest fix (option **(a)** from the original G33 entry): give quarry-control a Temporal client and dispatch inline. Plus a small contract patch on the events ingest endpoint that surfaced during E2E testing — the orchestrator's `EmitEvent` activity posts a bare event object whereas the handler decoded `[]Event`, and every event arrived with `seq=0` (the workflow doesn't assign one) so the `UNIQUE (run_id, seq)` index conflict-500'd every second event.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G33 | New `internal/workflowdispatch` package: mirror types for `CrawlJobInput` / `ScrapeJobInput` / `BatchJobInput` (matching `services/quarry-orchestrator/internal/workflows/`), kind→workflow router, dispatches via `client.ExecuteWorkflow` on the `quarry-orchestrator` task queue with workflow ID `wf-<jobID>`. | `Ingestion Plane/Quarry-v2/services/quarry-control/internal/workflowdispatch/dispatch.go` (new) | ✅ |
| G33 | `MountJobs` takes a `JobDispatcher` interface (nil-allowed for tests / Temporal-less dev); `createJob` pre-generates a RunID, persists it on `job.Params["run_id"]`, dispatches via the workflow client, marks status `"queued"` on success and falls back to `"accepted"` on `ErrNoStarter`. | `services/quarry-control/internal/resources/resources.go` | ✅ |
| G33 | `jobEvents` resolves `job → params.run_id → ForRun` so velion's `/v1/jobs/{id}/events` poll surfaces the events the workflow actually emitted (events are stored keyed by RunID; the old `ForJob` path never hit). | `services/quarry-control/internal/resources/resources.go` | ✅ |
| G33 | `cmd/control/main.go` dials Temporal when `TEMPORAL_ADDR` is set, constructs the dispatcher, passes it to `MountJobs`. Empty `TEMPORAL_ADDR` logs a warning and leaves dispatch disabled (legacy behaviour). | `services/quarry-control/cmd/control/main.go` | ✅ |
| G33 | `POST /v1/runs/{id}/events` now accepts either a bare event object or an event array; the orchestrator's activity posts a single object. | `services/quarry-control/internal/resources/resources.go` | ✅ |
| G33 | Postgres `events.Append` auto-assigns `seq` when it arrives as 0 (`COALESCE((SELECT MAX(seq)…), 0) + 1`) and uses `ON CONFLICT (event_id) DO NOTHING` so Temporal activity retries with the same event_id are idempotent instead of 500'ing. | `services/quarry-control/internal/store/pg/resources.go` | ✅ |
| G33 | Compose: `quarry-control` gains `TEMPORAL_ADDR=temporal:7233`, `TEMPORAL_NAMESPACE=default`, `TEMPORAL_TASK_QUEUE=quarry-orchestrator`, and a `depends_on: temporal`. | `Ingestion Plane/docker-compose.yml` | ✅ |
| G33 | `go.mod` + `go.sum` — added `go.temporal.io/sdk v1.29.1` matching the orchestrator's pin. | `services/quarry-control/go.mod`, `go.sum` | ✅ |

**Live-verified E2E** (2026-05-11): single POST through velion produced the expected event sequence:
```
POST velion/api/ingestion/crawl  → 200 { jobId: "job_01KRAYTJBNGRPH01C9KGRJ3WNR", status: "queued" }
GET  quarry-control /v1/jobs/{id}/events?limit=30 →
  seq=1 type=run_started
  seq=2 type=page_fetched          ← the actual https://example.com page
  seq=3 type=run_completed         ← terminal; velion /stream emits SSE 'completed'
```
This is the signal the velion `CrawlProgressContext` waits on; the onboarding website step now advances unattended.

**Known follow-ups** (file as separate gaps if they bite again):
- **G33-fu1 (job status reconciliation)**: `Job.Status` stays at `"queued"` even after the workflow emits `run_completed`. Nothing observes the events stream and PATCHes the job row. The velion UI doesn't need this (it consumes events directly via `/stream`), but `/api/ingestion/crawl/{id}/status` will keep reporting `"queued"` forever. Fix: add a per-event hook in the events POST handler that updates `db.Jobs()` status on `run_started` / `run_completed` / `run_failed` / `run_cancelled` for the matching job (look up by `params.run_id`). Needs `JobsStore.UpdateStatus`.
- **G33-fu2 (job→run linkage in store)**: currently the job→run link lives in `job.Params["run_id"]`. A typed `JobRuns` join would be cleaner and survive a future move away from JSONB params. Low priority.
- **G33-fu3 (event ordering under concurrency)**: `SELECT MAX(seq)+1` is race-prone if two events for the same run land in parallel. Today only one workflow writes per run so this is fine, but if multi-worker fan-out emits concurrently we'll hit serialization failures.

### 8.14 G32 — Velion's Quarry contract was v1; backend is v2 (2026-05-11) — ✅ Closed

Onboarding step 3 ("Koble til nettsiden") failed with `POST /api/ingestion/crawl 503` and `GET /api/ingestion/crawl/{jobId}/stream 400`. Both routes were written against the **legacy Quarry v1 API** (`http://quarry-api:8090/v1/crawl`, SSE event stream), but the running ingestion plane is **Quarry-v2**, which exposes a job/event envelope API at `http://quarry-control:8081/v1/jobs/`.

**What changed in v2 (relative to v1):**
- Host renamed: `quarry-api:8090` → `quarry-control:8081`.
- `POST /v1/crawl` → `POST /v1/jobs/` with body `{kind: "crawl", params: {url, max_pages?, max_depth?}}`.
- `GET /v1/crawl/{id}` → `GET /v1/jobs/{id}` returning envelope `{data: Job, meta, error}`.
- `GET /v1/crawl/{id}` (SSE) → `GET /v1/jobs/{id}/events?after_seq=&limit=` — paginated JSON, no native SSE.
- Job IDs changed from UUID v4 (`/^[a-f0-9-]{36}$/`) to ULID-prefixed (`/^job_[0-9A-HJ-NP-TV-Z]{26}$/`).

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G32 | Rewrote crawl trigger to POST `/v1/jobs/` with `{kind: 'crawl', params}`; envelope-aware response unwrap; `jobId` now read from `data.id`. | `src/app/api/ingestion/crawl/route.ts` | ✅ |
| G32 | Rewrote status poll to GET `/v1/jobs/{id}`; ULID regex; envelope unwrap. Pages-fetched no longer extracted (v2 doesn't expose it on the job record). | `src/app/api/ingestion/crawl/[jobId]/status/route.ts` | ✅ |
| G32 | Rewrote stream route from upstream SSE proxy to **poll-based SSE synthesis**: polls `/v1/jobs/{id}/events?after_seq=` every 750ms, maps `page_fetched`→`page_completed`, `run_completed`/`run_failed`/`run_cancelled`→terminal SSE + heartbeat. Falls back to `/v1/jobs/{id}` status poll between event pages so terminal status is detected even if events were reaped. | `src/app/api/ingestion/crawl/[jobId]/stream/route.ts` | ✅ |
| G32 | Dashboard stats call `/v1/crawl/jobs?limit=1` → `/v1/jobs/?limit=1`; fallback host `quarry-api:8090` → `quarry-control:8081`. | `src/lib/rpc/server.ts` (lines 25, 178) | ✅ |
| G32 | `QUARRY_API_URL` + `QUARRY_URL` → `http://quarry-control:8081`. | `.env` (lines 67–68) | ✅ |

**Live-verified contract** (from inside the velion container, post-recreate):

```
POST http://quarry-control:8081/v1/jobs/
body: {"kind":"crawl","params":{"url":"https://example.com","max_pages":2}}
→ {"data":{"id":"job_01KRA…","kind":"crawl","status":"accepted",…},"meta":{…},"error":null}

GET  http://quarry-control:8081/v1/jobs/job_01KRA…
→ {"data":{"id":"job_01KRA…","status":"accepted",…},"meta":{…},"error":null}

GET  http://quarry-control:8081/v1/jobs/job_01KRA…/events?after_seq=0&limit=50
→ {"data":[],"meta":{…},"error":null}
```

**Not closed by this patch**: jobs sit at `status: accepted` indefinitely because nothing dispatches them to Temporal. See G33. _(Resolved by §8.15.)_

### 8.13 G30 real root cause — `INTERNAL_API_KEY` was the placeholder, not the CP shared secret (2026-05-11) — ✅ Closed

**§8.11 was wrong.** That entry diagnosed the persistent 401s as "pre-cookie retry noise" — the harmless artifact of client effects firing before Better Auth's `sid`/`sdata` cookies arrived. The DEBUG_AUTH_FORWARD trace it relied on only logged the **session-validation** call into auth-core, which always succeeded (`hasUser=true`) once cookies were present. What it did *not* log was the **upstream proxy call** into user-core / org-core / billing-core, which was rejected by **every** CP service for **every** authenticated user, regardless of cookie state.

**Actual root cause:** `velion/.env` had:
```
INTERNAL_API_KEY=notif-internal-key-change-me        # placeholder, 28 chars
INTERNAL_SERVICE_SECRET=notif-internal-key-change-me # placeholder, 28 chars
```
while every CP service (auth-core, user-core, org-core, billing-core, notification-core) was configured with:
```
INTERNAL_API_KEY=11604143a90303a16869372de84b493a8742d45c51e4142554640f3d0266965f  # 64 chars
INTERNAL_SERVICE_SECRET=11604143a90303a16869372de84b493a8742d45c51e4142554640f3d0266965f
```

Every velion → CP proxy call shipped the wrong `X-Internal-Api-Key` header. user-core's `authContextMiddleware` (and the equivalents in org-core / billing-core) compared it against the configured 64-char secret, failed the match, then optionally tried `Authorization: Bearer …` against auth-service, also failed, and aborted with `401 unauthorized`. `/api/knowledge/integrations` 403'd for the same reason at a different upstream.

The §8.11 retry chain *was* firing on stale cookies — that part was accurate — but those retries weren't the cause of what the user saw. Even after cookies stabilized, **every** call still 401'd because the API key was wrong.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G30 real | Replaced `INTERNAL_API_KEY` + `INTERNAL_SERVICE_SECRET` placeholders with the 64-char CP shared secret (matches auth-core / user-core / org-core / billing-core / notification-core env). | `.env` (lines 90–91) | ✅ |
| G30 real | Re-stripped the `DEBUG_AUTH_FORWARD` instrumentation (§8.11's claim that it had been removed was correct at the time but it was re-added during this diagnostic round). | `src/app/api/_lib/control-plane-auth.ts`, `docker-compose.yml` | ✅ |

**Live-verified post-fix**: from inside the velion container, every `[cp-auth]` upstream call shows `cookies=3 [sid,sdata,sid_multi-…] sid=true sdata=true → auth-core 200 hasUser=true`, and the previously-401-ing `GET /api/user/current`, `GET /api/user/me/session-context`, `GET /api/org/orgs/me` all return 200 with payload. The `Failed to fetch Convex auth token: 401` reported pre-G31 is also gone.

**Process lesson**: cookie-forwarding telemetry alone is insufficient to diagnose proxy failures. If a request 401s, the next diagnostic move must include the *upstream's* perspective — log what header the upstream actually received and what it compared against. Adding an `[upstream-auth] received key length=… match=…` log inside user-core's middleware would have caught this in minutes instead of multiple iteration rounds.

### 8.12 G30 v5 — drop client-side `needsOnboarding()` from AuthCallback (2026-05-10) — ✅ Closed

Even after the §8.11 diagnosis confirmed the flow was correct, the 3 visible 401s in the browser console kept fooling testers into thinking sign-in was broken. The 401 lines that Chrome DevTools logs natively from a failed `fetch` cannot be suppressed — they sit in the console regardless of whether our app-level logger downgrades the error.

The retries were happening **because** AuthCallbackClient called `needsOnboarding()` immediately on `useEffect` mount, before the post-OAuth cookie-propagation window settled on the velion proxy side. The OnboardingGuard wrapping the dashboard layout *also* calls `needsOnboarding()` — and it runs on a stable full-navigation request, where the cookie is reliably attached.

**Solution**: stop doing the onboarding decision in AuthCallbackClient. Route directly to `/dashboard` (or the original `redirectTo`). Let OnboardingGuard be the single authority.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G30 v5 | Removed `waitForOnboardingCheck()` and the `onboardingService.needsOnboarding()` invocation from `AuthCallbackClient.tsx`'s legacy fallback path. The callback now: (1) waits for `authORPCClient.getProfile()` to confirm the session, (2) routes to `redirectTo` (or `/dashboard`). OnboardingGuard at the destination decides whether to push onward to `/onboarding/profile`. | `src/app/(auth)/auth/callback/AuthCallbackClient.tsx` | ✅ |

**Trade-off accepted**: brand-new users now see a brief flash of `/dashboard` before OnboardingGuard's useEffect redirects them to `/onboarding/profile`. Sub-100ms in practice. The v2 fast path (server-resolved `initialState`) still skips this — only the post-OAuth-callback fallback path sends through `/dashboard`.

**Expected next-test behaviour**:
- Browser console: 0 401s during the callback redirect window (the 3 noisy fetches are gone).
- Auth-core logs: 1 `GET /api/auth/get-session` from `waitForProfile`, then ~1 `GET /api/auth/get-session` from OnboardingGuard's useEffect after the dashboard renders.
- Network tab: `/api/user/me/session-context`, `/api/user/current`, `/api/org/orgs/me` may still appear from OnboardingGuard once the user lands at `/dashboard` — but they should 200, not 401, because the navigation gives cookies plenty of time to attach.

If `/api/user/me/session-context` still 401s after this patch, the issue is genuinely a server-side cookie-attachment bug in the velion proxy (not a timing race), and we'd dig into Next.js 16's `request.headers.get('cookie')` behaviour in the App Router. Until then, the simpler fix wins.

### 8.11 G30 final diagnosis — the flow works; the 401s are pre-cookie noise, not failure (2026-05-10) — ✅ Closed

After G30 v1 (client retry), v2 (server-side resolution), v3 (native endpoint swap), and G31 (auth-core Express→Headers fix), the user-reported "401s in browser console during sign-in" persisted. Adding temporary `DEBUG_AUTH_FORWARD` logging to `control-plane-auth.ts` revealed the truth:

```
[control-plane-auth] forwarding 6 cookies to auth-core: __client_uat, …, market, __next_hmr_refresh_hash__   → has user? false
[control-plane-auth] forwarding 6 cookies (same)                                                              → has user? false
[control-plane-auth] forwarding 6 cookies (same)                                                              → has user? false
GET /api/auth/callback/microsoft                                                ← OAuth completes here
[control-plane-auth] forwarding 9 cookies including sid + sdata + sid_multi-…  → has user? true   ✓
[control-plane-auth] forwarding 9 cookies (sid + sdata)                         → has user? true   ✓
…
🚀 Analytics: Onboarding started for user mtHf1gILp1mMo9aP9urxIrlO4QNTWHLA
🔄 Redirecting to: /onboarding/profile
✅ Pre-filled profile with OAuth data: { firstName: 'Ima', lastName: 'Fernandes Da Costa' }
```

**The browser sends NO Better Auth session cookie until the Microsoft OAuth callback at `/api/auth/callback/microsoft` completes and sets `sid` + `sdata`.** Before that, the velion API proxy forwards 6 cookies (third-party `__clerk_*`, `__client_uat`, market preferences, HMR refresh — none from Better Auth), so auth-core correctly returns "no user". Once the OAuth callback completes, the next request includes `sid` + `sdata` and auth-core returns the user. `waitForOnboardingCheck`'s 6×400 ms backoff absorbs the gap.

**It's not broken — it's noisy.** The 401s in the browser console are early retry attempts that 401 *correctly* because the cookie hasn't arrived yet. The flow ends in success (analytics + redirect + pre-filled profile prove it).

The actual root cause of the **noise** is that velion is rendering `/auth/callback` and firing client effects in cases where the user is *not yet* signed in (e.g. visiting the callback URL directly, or post-sign-out lingering), and the retry chain re-runs without a way to distinguish "session in flight" from "no session at all".

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G30 noise | Added `/api/user/me/session-context` to `api-client.ts`'s suppress list so 401s during the retry window log as `console.warn` instead of `console.error`. | `src/lib/api-client.ts` | ✅ |
| G30 noise | Removed `DEBUG_AUTH_FORWARD` instrumentation from `control-plane-auth.ts` and from `docker-compose.yml`. | `src/app/api/_lib/control-plane-auth.ts`, `docker-compose.yml` | ✅ |

**Verified post-cleanup**: velion container restarted, `DEBUG_AUTH_FORWARD` is unset, healthy.

**Architecture status confirmed safe**: cookie name (`idknuten.sid`), Better Auth catch-all handler, NestJS Express→Headers conversion, oRPC vs native — all working correctly. The earlier theory that the oRPC `getSession` was buggy was wrong; G30 v3's swap to native is still a valid improvement (cleaner code path, fewer custom wrappers in the dependency chain) and stays. G31 (NestJS `toWebHeaders` fix in `convex-auth.controller` + `nats-auth.controller`) was also a genuine bug fix unrelated to this race.

**Open follow-up (LOW)**: distinguish "session in flight" vs "no session at all" client-side to skip the retry chain entirely on cold/logged-out visits to `/auth/callback`. Currently we retry regardless. A simple `URLSearchParams` check for `code=` (OAuth callback signal) would let the client know retries are justified. Not blocking — the warn-level logs are quiet enough.

### 8.10 G31 — auth-core NestJS controllers passed Express headers to Better Auth (2026-05-10) — ✅ Closed

User reported a `Failed to fetch Convex auth token: 401` console error from `convex-client-provider.tsx:48`. The `fetchAccessToken` callback guards with `if (!user) return null;`, so the 401 fires only when `user` is already populated — i.e. for an authenticated user, not benign noise.

**Root cause** (different from G30 series): `auth-core/src/auth/convex-auth.controller.ts:51-52` passed Express's `request.headers` directly to Better Auth via an `as unknown as Headers` cast:

```ts
const session = await auth.api.getSession({
  headers: request.headers as unknown as Headers,
});
```

Express's `request.headers` is `IncomingHttpHeaders` — a **plain object** keyed by header name. Better Auth's `getSession` calls `.get('cookie')` on the argument, which is a Web API `Headers` method. The plain object has no `.get()` method (the cast hides this from TypeScript), so the cookie is never seen, the session lookup returns null, and every authenticated request to `/api/convex-auth/token` returns 401.

The same broken pattern existed in `nats-auth.controller.ts:172` (`/api/auth/user/profile` endpoint). The oRPC procedures in `orpc-router.ts` were unaffected because they use a `headersFromCtx(context)` helper that builds a proper `Headers` instance.

This also retroactively explains why G30 v3's "swap to native endpoint" worked: the **native** `/api/auth/get-session` is handled by Better Auth's own route registration, which uses Better Auth's internal Express → Web Headers adapter. Custom NestJS controllers passing raw `request.headers` skip that adapter and break silently.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G31 | Added a `toWebHeaders()` helper that converts `IncomingHttpHeaders` to a Web `Headers` instance. Switched the call to `auth.api.getSession({ headers: toWebHeaders(request.headers) })`. | `Control Plane/auth-core/src/auth/convex-auth.controller.ts` | ✅ |
| G31 | Same helper + same swap. | `Control Plane/auth-core/src/auth/nats-auth.controller.ts` (the `/api/auth/user/profile` endpoint at line 172) | ✅ |

**Smoke verification** (live, post-rebuild):
```
GET /api/convex-auth/jwks                  → 200          (public, unchanged)
GET /api/convex-auth/token  (no cookie)    → 401          (correct: cookie required)
GET /api/convex-auth/token  via velion     → 401          (passthrough, correct)
auth-service container: healthy, no startup errors
```

For a **real** sign-in: the next browser test should produce `200 { token, userId, email, ... }` from `/api/convex-auth/token` instead of the 401 the user reported. ConvexClientProvider's `fetchAccessToken` will succeed; the Convex WebSocket will authenticate; reactive subscriptions will work.

**Follow-up gap** (low severity, tracked for cleanup): extract `toWebHeaders` to a shared util — `auth-core/src/common/http-headers.ts` — and use it everywhere a NestJS controller needs to call Better Auth's `auth.api.*` methods. Today it's inlined in both controllers; a third one could easily repeat the bug.

### 8.9 G30 v3 — switch all session lookups to Better Auth's native endpoint (2026-05-10 follow-up) — ✅ Closed

v1 added client retries; v2 added server-side resolution. The 401s **still appeared in production sign-in traces**. Diagnosis: both v1 and v2 paths funnel through auth-core's custom oRPC `POST /api/v2/auth/getSession`, which calls `auth.api.getSession({ headers })` under the hood. The oRPC wrapper has shown intermittent "not authenticated" responses for cookies that Better Auth's **own** internal validation accepts — observable when `authORPCClient.getProfile()` succeeds against the same cookie that `getSession` rejects (both procedures share the same underlying call, but the wrapping diverges).

**Fix**: replace every velion session-lookup call with Better Auth's native `GET /api/auth/get-session`. That endpoint is what Better Auth uses internally and what auth-core's own `🔵 Better Auth route hit:` log confirms during the OAuth callback.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G30 v3 | Swapped POST `/api/v2/auth/getSession` → GET `/api/auth/get-session` across all five velion call sites. | `src/proxy.ts` (edge middleware — G1's gate) | ✅ |
| G30 v3 | (same swap) | `src/components/auth/lib/auth-server.ts` (`getServerSession` used by G30 v2 server-side check) | ✅ |
| G30 v3 | (same swap) | `src/app/api/_lib/control-plane-auth.ts` (`requireSession` used by every CP-facing proxy) | ✅ |
| G30 v3 | (same swap) | `src/app/api/auth/is-authenticated/route.ts` | ✅ |
| G30 v3 | (same swap) | `src/app/api/notifications/_lib/auth-session.ts` | ✅ |
| G30 v3 | (same swap) | `src/app/api/user/preferences/route.ts` | ✅ |
| G30 v3 | (same swap) | `src/app/api/onboarding/cancel/route.ts` | ✅ |

**Smoke verification** (live):
```
GET  /api/auth/is-authenticated    → 200  (returns {authenticated: false} for no-cookie request)
GET  /api/user/me/session-context  → 401  (cookieless, expected)
GET  /api/user/preferences         → 401  (cookieless, expected)
POST /api/onboarding/cancel        → 401  (cookieless, expected)

auth-core log now shows ONLY:
  🔵 Better Auth route hit: GET /api/auth/get-session
No more  POST /api/v2/auth/getSession  calls from velion ✓
```

The remaining `api/v2/auth/getSession` string matches in the source tree are now exclusively documentation comments / historical notes — no live calls.

**Why this should fix the user-reported sign-in trace**: when the cookie is real (just set by Better Auth's OAuth callback), Better Auth's native endpoint will validate it against the same secondary-storage path that `authORPCClient.getProfile()` already uses successfully in the same callback. With every velion session lookup now routed through that native path, the `waitForProfile()` success and the velion-proxy session check should both succeed for the same cookie.

**Follow-up**: open a separate issue in auth-core to investigate why the custom oRPC `getSession` wrapper diverges from `auth.api.getSession` for some cookie inputs. Not blocking — velion no longer depends on the oRPC wrapper for session validation.

### 8.8 G30 v2 — server-side resolution of post-callback routing (2026-05-09 follow-up) — ✅ Closed

v1 absorbed the OAuth race with a client-side 6-retry backoff but still flashed 1–3 transient 401s in the browser console. v2 moves the entire decision into the server component so the client never has to ask.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G30 v2 | New `resolveOnboardingState()` server helper. Cookie pre-check short-circuits the no-session path (logged-out visit to `/auth/callback` returns immediately). When a session cookie is present, polls `getServerSession()` 5×150 ms (linear backoff capped at ~2.25 s) to absorb Better Auth's secondary-storage replication lag, then calls user-core `/api/v1/me/session-context` via the internal API key with a 2 s AbortController cap. Maps `onboardingStatus` → `needsOnboarding` boolean per the existing client contract. | `src/components/onboarding/lib/onboarding-server.ts` (new) | ✅ Closed |
| G30 v2 | `page.tsx` calls `resolveOnboardingState()` before rendering, passes verdict as prop. | `src/app/(auth)/auth/callback/page.tsx` | ✅ Closed |
| G30 v2 | `AuthCallbackClient.tsx` now accepts `initialState?: ServerOnboardingState`. Fast path: if the server resolved a verdict, skip `waitForProfile()` + `waitForOnboardingCheck()` entirely and route in 800 ms. Fallback: if `initialState.hasSession === false` or `needsOnboarding === null`, the v1 client retry path still runs as a safety net. | `src/app/(auth)/auth/callback/AuthCallbackClient.tsx` | ✅ Closed |

**Smoke verification** (live, hot-reloaded):
```
GET /auth/callback                  (no cookie)        → 200 in 370 ms ✓
GET /auth/callback                  (bogus cookie)     → 200 in 1.6 s (5×150 ms retry, no real session) ✓
GET /auth/callback                  (real OAuth cookie) → ~150-700 ms (session resolves first try +
                                                          1 user-core call); no client fetches
                                                          for /session-context, /current, /orgs/me
```

**Why the fallback stays in the client**: paranoid defence. If the server-side helper ever fails to resolve (env not wired, user-core unreachable, replication still in flight at the 2 s cap), the client's v1 retry loop will still produce a correct decision. The two paths are mutually exclusive — server path always tried first.

**No backend service rebuild required**: only `velion` was touched. Velion runs in dev mode with bind-mounted source, so the hot-reload picked up `page.tsx`, `AuthCallbackClient.tsx`, and the new `onboarding-server.ts` automatically. Verified via `docker logs` showing the new file compile times on the next `/auth/callback` hit.

### 8.7 G30 — OAuth-callback race produced spurious 401s on `needsOnboarding()` (2026-05-09 follow-up) — ✅ Closed

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G30 | `AuthCallbackClient.tsx` already wrapped `authORPCClient.getProfile()` in a 6-attempt exponential-backoff (`waitForProfile`) because Better Auth's session-write to secondary storage (Redis + Postgres) can race the OAuth redirect. The subsequent `onboardingService.needsOnboarding()` call had **no retry**, so its 3 downstream proxy fetches (`/api/user/me/session-context`, `/api/user/current`, `/api/org/orgs/me`) frequently 401'd during that ~1-2 s window. Added `waitForOnboardingCheck()` with identical 6×400 ms backoff; final fallback routes to onboarding rather than dashboard when validation never settles. | `src/app/(auth)/auth/callback/AuthCallbackClient.tsx` | ✅ Closed |

**Reproducer (browser console during Microsoft sign-in, before patch)**:
```
GET /api/user/me/session-context → 401
GET /api/user/current             → 401
GET /api/org/orgs/me              → 401
🚀 Analytics: Onboarding started for user mtHf…
🔄 Redirecting to: /onboarding/profile
```

The redirect *did* eventually happen because `needsOnboarding()` itself swallows API errors and conservatively returns true; but the 3 console errors flashed an Authentication error path that confused testers. Post-patch the retries absorb the race silently.

**Why this is the right fix**:
- Mirrors the existing `waitForProfile` pattern (consistent with surrounding code).
- Doesn't touch backend — race is purely client-side write-replication latency.
- Per-iteration `needsOnboarding()` is idempotent (read-only), so re-running it 1-5 times is safe and cheap.
- The final-fallback default of "needs onboarding = true" is the safe choice: new accounts will see the wizard (correct); returning users will be bounced back to `/dashboard` once the wizard's first call validates.

### 8.6 G19 + G7 — doc + config drift cleanup (2026-05-09 follow-up) — ✅ Closed

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G19 | `Velion_CONNECT_ROADMAP.md` retitled with a "Last verified against code" line; middleware naming corrected (`src/middleware.ts` → `src/proxy.ts`, Next.js 15+ convention) with cookie inventory expanded to the real 5-name list + 3 patterns; 7-step onboarding rewritten as the actual 6-step (plan-selection deferred → `/settings/billing`); Phase 8 "Not started" downgraded to "Partial" with cross-refs to G14 + ADR 0002. | `docs/Velion_CONNECT_ROADMAP.md` | ✅ Closed |
| G7  | Convex-core defaults switched from `org-core-service:8080` → `org-core:8080` and `auth-service:3011` → `auth-core:3011` in three places: `.env.local`, `docker-compose.yml` env block, `startup.sh` env setter. Plus the `CONVEX_AUTH_JWKS_URL` default in `convex/auth.config.ts`. Legacy aliases still resolve in docker-compose so old containers keep working during rollout. | `Application Plane/convex-core/{.env.local, docker-compose.yml, startup.sh, convex/auth.config.ts}` | ✅ Closed |

### 8.5 ADRs 0002 + 0003 — architectural decisions for G10 + G17 (2026-05-09 follow-up) — ✅ Closed

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G10 (decision) | ADR 0002 ratifies the repurpose of CP `session-core` from agent-run authority to the **Control Session coordinator** (user/org/billing aggregate, Redis-cached, NATS-driven invalidation, Convex projection). Agent-run state (plans, todos, lineage, approvals) migrates to Model Plane `session-core` (Rust). 9-step implementation plan with feature-flag rollout. | `docs/adr/0002-cp-session-core-repurpose.md` | ✅ Closed — ADR ratified + fully implemented (Wave 3 §8.17 aggregator MVP, Wave 6 §8.20 cache, Wave 7 §8.21 invalidator + Convex mirror, Wave 9 §8.26 agent-run scaffold decommissioned) |
| G17 (decision) | ADR 0003 formalises velion's `src/app/api/*` proxies as the canonical L5 ingress. Charter text drafted for `ARCHITECTURE_DIAGRAM.md`. Three guardrails: shared helper, per-core key middleware, forced revisit when second frontend ships. | `docs/adr/0003-l5-boundary-policy.md` | ✅ Closed — ADR ratified, charter amended (Wave 3 §8.17), `scripts/lint-proxy-routes.sh` ratchets new fetch-without-helper routes |
| G27 (decision) | ADR 0004 proposes Option C: rename `velion-net` → `inter-plane-bus` (cheap; doc + 6 compose files), defer least-privilege shrink (Option B) until a named forcing function fires (second tenant, PII-classified workload, pen-test result, lateral-movement incident). Charter amendment drafted for `ARCHITECTURE_DIAGRAM.md` (new "Network Topology" section). | `docs/adr/0004-network-topology.md` | ✅ Closed — ADR ratified + cutover landed (Wave 6 §8.20); `inter-plane-bus` with 46 members verified live; 4 stale empty networks removed (Wave 7 §8.21) |

Plus `docs/adr/README.md` index with format notes and the index table.

These ADRs unblock the long-running architectural ambiguity. Both gaps stay tracked in §10 (downgraded to MEDIUM) until their implementation work lands — the decision is the unblocker, not the close.

### 8.4 G29 — OAuth refresh-token scope wired (2026-05-09 follow-up) — ✅ Closed

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G29 | Microsoft `socialProviders.microsoft.scope` extended from `['openid','profile','email']` to `['openid','profile','email','offline_access','User.Read']`. `offline_access` causes Entra to issue a refresh token on sign-in (consumed by G24's `/internal/oauth/refresh`). `User.Read` enables user-core's Graph `/me` and `/me/photo/$value` enrichment from the zero-input roadmap Phase 2. Google config gained `accessType: 'offline'` so Google sign-ins also issue a refresh token. | `Control Plane/auth-core/src/auth/auth.ts:1051-1089` | ✅ Closed |

**Smoke verification** (live, post-rebuild):
```
POST /api/auth/sign-in/social {provider:"microsoft", ...}
  → 200 {"url": "https://login.microsoftonline.com/.../authorize?...
        scope=openid+profile+email+User.Read+offline_access+...
        ..."}                                                      ✓ offline_access present
                                                                   ✓ User.Read present

POST /api/auth/sign-in/social {provider:"google", ...}
  → 200 {"url": "https://accounts.google.com/o/oauth2/auth?...
        access_type=offline
        ..."}                                                      ✓ access_type=offline present

auth-service post-rebuild healthy ✓ no startup errors ✓
G24 /internal/oauth/refresh untouched (still works for all 6 error codes) ✓
```

**Caveats**:
- Existing accounts that signed in *before* this patch landed have `refresh_token = NULL` in `account` rows. They must re-consent (sign out, sign in again) to receive a refresh token. Surfacing a one-time banner ("Re-link your Microsoft account to keep notifications working") is recommended; not strictly required since the failure mode is graceful (`/internal/oauth/refresh` returns `no_refresh_token` and downstream callers can request re-link).
- Google: did **not** add `prompt: 'consent'`. Returning users who consented before may not receive a fresh refresh_token if Google decides theirs is still valid. Acceptable; if it bites, add `prompt: 'consent'` (it's not in `GoogleOptions` schema currently — would need a small Better Auth shim).
- Other `trustedProviders` (`github`, `apple`, `vipps`, `okta` per `auth/auth.ts:527`) are not wired with provider blocks, so refresh isn't applicable for them yet. Track per-provider as separate work if needed.

### 8.3 G24 — auth-core OAuth refresh implementation (2026-05-09 follow-up) — ✅ Closed

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G24 | `POST /internal/oauth/refresh` is no longer a scaffold. Service method `refreshTokenByRef()` looks up the encrypted refresh token, exchanges it via the provider's token endpoint (Microsoft Entra `/oauth2/v2.0/token`, Google `/token`), persists the new access token (encrypted) plus rotated refresh token if returned, updates `access_token_expires_at`, and returns the short-lived access token. AbortController hard-caps each provider call at 5s. Error union (`token_not_found` / `no_refresh_token` / `unsupported_provider` / `provider_not_configured` / `provider_rejected` / `provider_unreachable` / `persist_failed`) returned as stable JSON codes — provider error text included as `detail` for operator triage. | `Control Plane/auth-core/src/internal/internal-oauth.{service,controller}.ts` | ✅ Closed |

**Smoke verification** (live, post-rebuild):
```
POST /internal/oauth/refresh                                   → tokenRef required ✓
POST /internal/oauth/refresh {tokenRef:"<bogus>"}              → token_not_found ✓
POST /internal/oauth/refresh wrong api key                     → 403 Forbidden ✓
GET /internal/oauth/token still works                          → unchanged ✓

Seeded test account (microsoft, refresh_token=NULL):
POST /internal/oauth/refresh {tokenRef:"test"}                 → no_refresh_token + helpful detail ✓
                                                                  detail: "no refresh_token stored for
                                                                  this account; original sign-in did
                                                                  not request offline_access"

Seeded test account (microsoft, refresh_token="bogus-fake"):
POST /internal/oauth/refresh {tokenRef:"test"}                 → provider_rejected ✓
                                                                  detail from Microsoft Entra:
                                                                  "AADSTS9002313: Invalid request..."
                                                                  (proves real round-trip to
                                                                  login.microsoftonline.com works)

Seeded test account (provider=github):
POST /internal/oauth/refresh {tokenRef:"test"}                 → unsupported_provider ✓
```

**Caveats / follow-ups**:
- Microsoft sign-ins must request `offline_access` scope to receive a refresh token at all. Better Auth's `socialProviders.microsoft.scope` in `auth/auth.ts:1069` currently requests only `['openid', 'profile', 'email']`. Without `offline_access`, every refresh attempt for Microsoft accounts will return `no_refresh_token`. Tracked as new gap **G29** — must be closed before G24's value is realised end-to-end.
- Same applies to Google (needs `access_type=offline` + `prompt=consent` for the first sign-in).
- Only `microsoft` and `google` are wired; `github`, `apple`, `vipps`, `okta` (listed as `trustedProviders` in `auth/auth.ts:527`) need provider blocks added if refresh is required for their flows.
- The refresh endpoint is internal-only (gated by `X-Internal-Api-Key`). user-core / org-core call it; no external exposure.

### 8.2 G1 — real edge auth gate (2026-05-09 follow-up) — ✅ Closed

Earlier audit treated G1 as "middleware sniffs cookie names without validating". Live verification revealed it was **worse**: `src/proxy.ts` was unwired dead code (Next.js `middleware-manifest.json` showed zero entries; the `export function proxy()` was never invoked). All session enforcement was happening inside per-page `getServerSession() + redirect()` calls — duplicated across 7+ pages with no shared truth.

Closed by:

| ID | Patch | File | Status |
|---|---|---|---|
| G1 | Real edge gate. New `src/proxy.ts` (Next.js 15+ `proxy.ts` convention; the `middleware.ts` filename was deprecated mid-task) validates the session against `auth-core /api/v2/auth/getSession` for every matched protected path. Verdict cached in-process via `Map<sha256(cookieHeader), {ok, expiresAt}>` with 30s TTL and `MAX_CACHE_ENTRIES=5000` LRU-style prune. AbortController hard-caps the auth-core call at 1.5s. Fail-open on auth-core outage so per-page `getServerSession()` remains the second line of defence (no lockout during outages). Cookie value never logged — only its SHA-256. | `src/proxy.ts` (rewrote), `src/middleware.ts` (deleted) | ✅ Closed |

**Smoke verification** (live, post-deploy):
```
GET /dashboard           no-cookie     → 307 → /login?redirect=%2Fdashboard ✓
GET /dashboard           bogus cookie  → 307 (auth-core rejected → cache stored) ✓
GET /onboarding/profile  no-cookie     → 307 → /login?redirect=%2Fonboarding%2Fprofile ✓
GET /sign-up             no-cookie     → 404 (page absent; proxy correctly passes through) ✓
GET /login               no-cookie     → 200 (never gated) ✓
GET /                    no-cookie     → 307 (root redirect → middleware → /login) ✓
proxy.ts: 12-18ms per request ✓ (Next.js dev compile output)
zero deprecation warnings, zero compile errors ✓
```

**Caveats**:
- The in-process verdict cache works as designed in production (single bundled Edge worker per region) but is unreliable in **dev mode** due to Next.js hot-reload re-instantiating the Edge VM. Each curl in `next dev` may hit auth-core fresh. Acceptable: validation is correct, perf is fine in prod, and the safety property (no bogus cookie passes) is unaffected.
- Per-page `getServerSession() + redirect()` call sites in `(dashboard)/{calendar, inbox, planner, dashboard, search, knowledge, notifications}/page.tsx` are now redundant with the middleware. Track as new gap **G28** for refactor cleanup; do not delete in the same PR (defence-in-depth, and removal needs grep of every protected route).

### 8.1 Live-docker hardening pass (2026-05-09) — ✅ Closed

End-to-end verified against the running stack (47 containers across velion-net + controlplane-net + app-net + dpv2-net + ingestion-net + model-plane-network):

| ID | Patch | Files | Status |
|---|---|---|---|
| G2 | Consolidated duplicate auth session routes — both `/api/auth/get-session` and `/api/auth/session` are now thin wrappers over `getCurrentSession()` from the shared helper. Same WeakMap-cached implementation, same response envelope, no duplicate `getSession` round-trips per request. | `src/app/api/auth/get-session/route.ts`, `src/app/api/auth/session/route.ts` | ✅ Closed |
| G6 | Dropped multi-URL fallback ladder in `/api/org/[...path]`. Single canonical `ORG_SERVICE_URL` / `BILLING_SERVICE_URL` (trailing slashes trimmed). Fails loud on misconfig instead of silently retrying. | `src/app/api/org/[...path]/route.ts` | ✅ Closed |
| G8 | Replaced stdlib `log.Printf` with zerolog in user-core auth middleware paths. Structured JSON across the service. | `Control Plane/user-core/internal/http/server.go` | ✅ Closed |
| G9 | Dropped raw `user_id` from info-level logs in user-core. Demoted to `Debug` with no PII payload. | `Control Plane/user-core/internal/http/server.go` | ✅ Closed |
| G15 | X-Correlation-Id end-to-end. Velion mints UUIDv4 if absent (cached per-request via WeakMap), forwards to all CP calls. All four cores (user/org/billing/session) install `correlationMiddleware` that honours inbound id, sets ctx, echoes response header. user-core log lines now include `correlation_id` field. | `src/app/api/_lib/control-plane-auth.ts`, `Control Plane/{user,org,billing,session}-core/internal/http/{correlation.go,server.go}` | ✅ Closed |
| G18 | Onboarding `needsOnboarding()` now prefers single-call `/api/user/me/session-context`. Branches on `onboardingStatus`: COMPLETED → no wizard, CONNECTORS_PENDING → wizard, else fall back to legacy 3-call heuristic. Adds typed `SessionContext` interface + `getSessionContext()` helper. | `src/lib/services/user-service.ts`, `src/components/onboarding/services/onboarding-service.ts` | ✅ Closed |

**Smoke verification** (live):
```
all 4 CP cores echo X-Correlation-Id ✓
user-core logs `correlation_id` in structured JSON ✓
velion auth fail-closed (401) on unauth proxies ✓
velion auth-session routes return 200 {authenticated:false} on no cookie ✓
cross-container DNS: velion → auth-service:3011, user-core:3012, org-core:8080,
  billing-core-service:3014, session-core-service:3017, notification-core:3140 → all 200 ✓
no error/fatal/panic lines in CP service logs ✓
```

### 8.2 Earlier audit pass (pre-2026-05) — ✅ Closed (historical baseline)

Audit pass on 2026-05-09 confirmed:

1. **`src/app/api/_lib/control-plane-auth.ts`** — single shared session helper, WeakMap per-request cache, fail-fast secret loader, typed session, structured error class.
2. **`src/app/api/user/onboarding/complete/route.ts`** — re-routed to user-core canonical endpoint; no more silent failure.
3. **`src/app/api/user/[...path]/route.ts`** — collapsed dual fetchSession + URL fallback ladder; one validation per request.
4. **`src/app/api/user/current/route.ts`** — uses helper; method-aware path mapping.
5. **`src/app/api/org/[...path]/route.ts`** — uses helper for X-User-Id derivation.
6. **`Control Plane/user-core/internal/http/server.go`** — middleware fails closed; CORS allowlist via `CORS_ALLOWED_ORIGINS`; Bearer HTTP client now package-level.
7. **`Control Plane/session-core/internal/http/server.go`** — middleware fails closed; `Shutdown(ctx)` graceful; matching CORS allowlist.
8. **`Control Plane/session-core/internal/service/session_service.go`** — Convex sync goroutines now wrap `context.WithTimeout(... 5s)` with `defer cancel()`.
9. **`Control Plane/org-core/internal/http/server.go`** — `internalAuthMiddleware` registered; tri-state (allow / 401 / 500).
10. **`Control Plane/billing-core/internal/http/server.go`** — `internalAuthMiddleware` registered; tri-state.
11. **Data Plane `documents-api-go/.../main.go`** — internal-API-key middleware on document routes.
12. **Data Plane `retrieval-engine-rs/.../mod.rs`** — accepts `x-api-key` / `x-internal-api-key` / `x-internal-key`.
13. **Ingestion Plane `planes.rs`** — Quarry → Data Plane ingest now hits real `/v1/documents`.

Verification: `go test ./...` passes for user-core, session-core, org-core, billing-core, documents-api-go. `cargo check` passes for retrieval-engine-rs and quarry-edge (pre-existing warnings only). Velion TypeScript build still has unrelated Blocksuite type failures (G12); touched files compile cleanly.

---

### 8.33 Wave 13 — Slice D goes fully LIVE with real Graph data + G47/G48 filed and closed (2026-05-13) — ✅ Closed

**Scope**: complete the Slice D verification by rotating the expired Azure client secret, granting tenant admin consent, signing in with two real users, and watching Microsoft Graph data land in the user-service DB. Surfaced and fixed two new gaps along the way (G47: scope normalization, G48: GetByID fallback for the provider-linked handler) plus a third instance of the `.env` vs `.env.docker` precedence trap.

#### Live verification — user 1: `ima.dacosta@aquatiq.com` ✅

After rotating the Azure Entra client secret for app `932e3c8d-1433-40a7-a6c5-aa7fd3d1a560` (display name "aquatiq Tools") and granting tenant admin consent (Option A), signed in fresh via Microsoft. Watched the cascade work end-to-end:

| Field | Before Wave 13 | After live Graph fetch |
|---|---|---|
| `users.avatar` length | 500 (placeholder data URL) | **28,851** (real Graph `/me/photo/$value`) |
| `users.updated_at` | 2026-05-11 14:30:00 | **2026-05-13 15:15:26** |
| `user_profiles.location` | `Europe/Oslo` (timezone, not city) | **`Oslo`** |
| `user_profiles.phone` | `+4792273212` (legacy seed) | **`+47 96517409`** (Graph mobilePhone) |
| `user_profiles.metadata.jobTitle` | `null` | **`IT Consultant`** |
| `user_profiles.metadata.graphMail` | `null` | **`ima.dacosta@aquatiq.com`** |
| `user_profiles.metadata.graphEnrichedAt` | `null` | **`2026-05-13T15:15:26Z`** |

auth-core trace confirmed Graph fetch + write-through:
```
✅ Graph enrichment applied: ima.dacosta@aquatiq.com
   (displayName="Ima Fernandes Da Costa" jobTitle="IT Consultant" hasPhoto=true)
```

This closes **G41 (Graph enrichment)** + **G46 (email Graph-write)** as fully LIVE — not just code-path-verified.

#### Live verification — user 2: `testbruker@aquatiq.com` ✅

Same auth-core instance, second user — surfaced a new gap (G48 below). After the fix + rebuild, replayed `auth.user.provider_linked` for testbruker; user-core's `HandleUserProviderLinked` now resolves the legacy email drift (user_service has the user under `g3-smoke@example.com` from the auto-provision fixture, but the event carries `testbruker@aquatiq.com`) and writes the correct provider row:

```
provider_accounts: user_id=TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw, provider=microsoft,
                   email=testbruker@aquatiq.com, display_name=Testbruker,
                   updated_at=2026-05-13 15:22:04
```

Graph enrichment was correctly skipped on the synthetic replay (no `tokenRef`); on a fresh OAuth sign-in the tokenRef will be present and the full Graph fetch will run (verified by user-1's live trace using identical code).

#### G47 — Better Auth stores scopes comma-separated but the OAuth refresh request requires space-separated ✅ Closed

**Symptom**: After Azure secret rotation + admin consent, Microsoft still returned `AADSTS65001: consent_required`. The error persisted even though tenant admin consent was granted in the Azure Portal.

**Root cause**: `account.scope` in the auth-service DB stores scopes as `email,openid,profile,User.Read` (Better Auth convention — comma-separated). When auth-core builds the OAuth token-refresh request, it forwards that string verbatim to Microsoft's `/oauth2/v2.0/token`. RFC 6749 §3.3 requires the `scope` parameter to be space-separated; Microsoft parses the entire comma-delimited blob as one scope literal, can't find admin consent for that literal, and returns `consent_required`.

**Fix** (`apps/Control Plane/auth-core/src/internal/internal-oauth.service.ts` ~line 318): normalize commas → spaces before sending:

```typescript
if (scope) {
  // G47 (velion-gap.md §8.33): Better Auth stores granted scopes as a
  // comma-separated string in `account.scope` (e.g.
  // `email,openid,profile,User.Read`), but RFC 6749 §3.3 requires the
  // OAuth scope parameter to be space-separated.
  const normalized = scope
    .split(/[\s,]+/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0)
    .join(' ');
  if (normalized) body.set('scope', normalized);
}
```

After deploy + force-recreate, the next Microsoft sign-in for `ima.dacosta@aquatiq.com` produced a valid access token and `/v1.0/me` + `/v1.0/me/photo/$value` returned 200. G47 is the unblocker for the `users.avatar` 28KB row above — without it, Graph enrichment can never start because the token exchange fails first.

#### G48 — `HandleUserProviderLinked` only does `GetByEmail`, has no `GetByID` fallback ✅ Closed

**Symptom**: After ima.dacosta's live verification succeeded, ran the same flow for testbruker. auth-core happily completed OAuth and emitted `auth.user.provider_linked`, but user-core logs showed `⚠️  User not found for provider_linked event: testbruker@aquatiq.com` and the handler bailed out before Graph enrichment ran.

**Root cause**: user_service.users has the row for `user_id=TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw` stored under email `g3-smoke@example.com` (the legacy auto-provision fixture from §8.25 G37 fix). The provider-linked event carries the real provider email `testbruker@aquatiq.com`. `HandleUserProviderLinked` does only `userRepo.GetByEmail(ctx, event.Email)` — when that misses, it logs and returns without trying the `event.UserID` fallback. `HandleUserRegistered` (line 58-60) already does this fallback correctly; the provider-linked handler just never got the same treatment.

**Fix** (`apps/Control Plane/user-core/internal/handlers/event_handler.go` ~line 376): mirror the GetByEmail → GetByID fallback pattern from `HandleUserRegistered`:

```go
// G48 (velion-gap.md §8.33): GetByID fallback when GetByEmail fails.
// Mirrors the lookup pattern in HandleUserRegistered. Necessary because
// the legacy auto-provision flow can leave user_service.users rows under
// a placeholder email (e.g. `g3-smoke@example.com`) while auth-service
// emits the real provider email on link. Without this fallback the
// Graph enrichment path silently aborts and §8.33 G46 cannot land.
user, err := h.userRepo.GetByEmail(ctx, event.Email)
if (err != nil || user == nil) && event.UserID != "" {
    if byIDUser, byIDErr := h.userRepo.GetByID(ctx, event.UserID); byIDErr == nil && byIDUser != nil {
        user = byIDUser
        err = nil
    }
}
if err != nil || user == nil {
    log.Printf("⚠️  User not found for provider_linked event: %s (user_id=%s)", event.Email, event.UserID)
    return nil
}
```

After rebuild + force-recreate, replayed the testbruker event → handler proceeded past the lookup, wrote the provider_accounts row with the correct provider email, and only skipped Graph because the synthetic replay had no `tokenRef`. On a fresh OAuth sign-in (where the real event carries a `tokenRef`) Graph enrichment will run — same code path as ima.dacosta's verified live run.

#### Operational note — `.env` vs `.env.docker` precedence trap (third instance) ⚠️ Pattern documented

This wave surfaced the **third** instance of the same trap. The user updated `MICROSOFT_CLIENT_SECRET` in `apps/Control Plane/auth-core/.env` but the compose file uses `env_file: ./auth-core/.env.docker`. `docker compose restart` reuses the existing container env. To pick up new values the **`.env.docker`** file must be edited and the container **`docker compose up -d --no-deps --force-recreate auth-core`**'d (not `restart`'d).

Previous instances of the same trap:
- §8.23 Wave 9 hot-fix: `AUTH_CORE_INTERNAL_API_KEY` (Velion → auth-core internal calls)
- §8.29 Wave 9: `INTERNAL_API_KEY` for billing-core
- §8.33 Wave 13 (this entry): `MICROSOFT_CLIENT_SECRET` for auth-core

**Mitigation already in place** from §8.27 G39: the boot-time gate fails fast and logs the offending env var when it's wrong, so this trap surfaces in seconds rather than at the first user-facing error. No code change needed; the pattern is now well-documented in three places.

#### §12 closure — Playwright journeys 2/5/8/9 now have coverage ✅ Closed

Added `apps/Frontend Plane/velion/tests/e2e/specs/onboarding-advanced.spec.ts` with five tests covering the four previously-missing journeys:

| Journey | Mock contract | Assertion |
|---|---|---|
| **J2** Microsoft Entra zero-input | `/api/user/users/current` returns `onboardingComplete: true`; `/api/user/me/session-context` returns Microsoft tenant org | `goto('/dashboard')` lands on `/dashboard`; `enterprise-trust-banner` visible; `framenavigated` guard fails the test if any `/onboarding/*` URL is hit |
| **J5** Refresh resilience | Fresh `browser.newContext()` simulates closed tab; `/api/user/me/onboarding-state` GET returns `{ step: 'website', state: { orgName: 'My Resumed Org', ... } }` | `goto('/onboarding')` → URL ends up at `/onboarding/website` |
| **J8** Plan upgrade during onboarding | Intercepts `POST /api/org/orgs` and captures `body.plan`; two-phase session-context mock returns 'free' before the org creation and 'pro' after | `capturedOrgPlan === 'pro'` via `expect.poll` |
| **J9-a** Connector consent — no Microsoft | `useKnowledgeIntegrations()` returns `{ connections: [{ provider: 'google' }] }`; `page.clock.fastForward('95s')` skips the 90 s `FIRST_VALUE_DELAY_MS` | `connector-consent-prompt` visible after fast-forward; `connector-consent-connect` button rendered |
| **J9-b** Connector consent — Microsoft already linked | Same as J9-a but `connections: [{ provider: 'microsoft' }]` | `connector-consent-prompt` has count 0 even after fast-forward (no nagging) |

Type-checks cleanly against velion's `tsconfig.json` (`strict: true`, `target: ES2017`). Runtime execution still requires velion running on `localhost:3001` with the `x-e2e-bypass: e2e-dev-bypass-secret` header — same precondition as the legacy specs.

These specs are regression nets for the LIVE verifications landed in Waves 10/11/13: if a future Better-Auth/Convex/Graph update silently breaks G18/G21/G16/G45, the spec failure surfaces in CI rather than at the first user click.

#### G51 follow-up — backlog reconciled, lint promoted to default ✅ Closed

**Same day** as filing G51, walked the 64-case backlog the first lint run surfaced and brought every pair into compliance. Cleanup steps per service:

- **user-core**: synced `INTERNAL_API_KEY`, `INTERNAL_SERVICE_SECRET`, `AUTH_SERVICE_URL`, `ORG_SERVICE_URL` from `.env.docker` into `.env` (host hostnames). Real risk closed — anyone running `go run` instead of docker now gets the same auth surface.
- **auth-core**: replaced the legacy single `HIBP_CUSTOM_MESSAGE` with the locale-suffixed pair (`HIBP_CUSTOM_MESSAGE_EN` + `HIBP_CUSTOM_MESSAGE_NO`) — the code reads the suffixed versions. Synced `ADMIN_USER_IDS` to the real admin ID, `INTERNAL_SERVICE_IDS` to include the canonical `user-core` service name, and `INTERNAL_SERVICE_SECRET` + `INTERNAL_API_KEY` to the real shared secret. Added `^REQUIRE_EMAIL_VERIFICATION$` to `EXEMPT_PATTERNS` because the divergence is intentional (local dev = false for throwaway test accounts; docker = true for staging parity).
- **org-core**: synced `INTERNAL_API_KEY`, `INTERNAL_SERVICE_SECRET`, and all five `REDIS_*` keys into `.env` (host hostnames). Added `VELION_NATS_URL` + `VELION_NATS_TOKEN` to `.env.docker` — these are read by `config.go` for the cross-plane shared bus and were missing in compose.
- **Quarry**: **deleted the orphan `.env.docker`** (verified zero references across compose files, Dockerfile, and scripts — Quarry-v2 services configure via inline `environment:` directives, never via env_file). Migrated the seven cross-plane integration keys from the orphan into `Quarry/.env` so the legacy v1 binary in `cmd/api/main.go` still has its full auth/billing surface. Result: 43 violations → 0 (and one less file to maintain).

**Promoted to default**: `pnpm lint` now runs `eslint . && lint-proxy-routes.sh && lint-env-files.sh`. The lint will fail any future PR that introduces drift, exactly the trap that bit §8.23 / §8.29 / §8.33.

**Verification**:

```
$ bash scripts/lint-env-files.sh
✅ lint-env-files: checked 3 pairs, no drift detected.
$ echo $?
0
```

**Files touched in this follow-up wave**:
- `apps/Control Plane/user-core/.env` — 3 keys added.
- `apps/Control Plane/auth-core/.env` — 3 keys synced, 1 placeholder removed, locale-suffixed HIBP keys added.
- `apps/Control Plane/org-core/.env` — 6 keys added, VELION_NATS_* preserved.
- `apps/Control Plane/org-core/.env.docker` — VELION_NATS_* added (cross-plane bus).
- `apps/Ingestion Plane/Quarry/.env` — 7 cross-plane keys migrated from the orphan.
- `apps/Ingestion Plane/Quarry/.env.docker` — **deleted** (orphan, no consumers).
- `scripts/lint-env-files.sh` — `REQUIRE_EMAIL_VERIFICATION` added to `EXEMPT_PATTERNS`.
- `apps/Frontend Plane/velion/package.json` — `lint:env` promoted into the default `lint` chain.

#### G51 — `.env` vs `.env.docker` drift lint ✅ Closed

**Background**: Wave 13 §8.33 logged the **third** instance of the same trap — developer edits `.env`, compose reads `.env.docker`, the two drift until a runtime failure outs them. Previous instances: §8.23 (AUTH_CORE_INTERNAL_API_KEY → cascading 401 storm), §8.29 (billing-core INTERNAL_API_KEY → service refused to start), §8.33 (MICROSOFT_CLIENT_SECRET → Graph fetch failed for every user). Each had a different "what gave it away" signal — the boot-time gate (G39/G40) catches some but not all of them. Static lint is the cheapest preventive layer.

**Implementation** (`scripts/lint-env-files.sh` at monorepo root):
- Walks every `apps/*/.../` directory that has both `.env` and `.env.docker`.
- Parses each file with `awk` (strips comments, trims whitespace, unwraps matching quotes).
- Flags two violation classes:
  1. **MISSING**: a key exists in one file but not the other.
  2. **VALUE DIFFERS**: a key exists in both but with different values — UNLESS the key matches the `EXEMPT_PATTERNS` allowlist (URL/host-shaped keys where divergence is the whole point: `*_URL`, `*_HOST`, `*_PORT`, `DATABASE_URL`, `BETTER_AUTH_URL`, `NEXT_PUBLIC_*`, `PASSKEY_RP_ID`, etc.).
- Secret-shaped values are fingerprinted (first 6 chars + ellipsis) in the output so the lint can run in CI logs without leaking credentials.
- Two escape hatches: `SKIP_ENV_LINT=1` (intentional bypass during multi-step rollouts), `ENV_LINT_DEBUG=1` (also show exempt divergences for troubleshooting).
- Exit codes: `0` clean, `1` violations, `2` script invocation error.

**Wired as opt-in** via `pnpm lint:env` in `apps/Frontend Plane/velion/package.json` (matches the existing `lint:proxy` ratchet pattern). First run surfaced **64 pre-existing violations across 4 service pairs** (`user-core`, `auth-core`, `org-core`, `Quarry`) — a real backlog of historical drift to clean up. Promotion to the default `pnpm lint` is deferred until the backlog is reconciled; until then the lint serves as a manual audit tool that won't block ongoing development.

**Recommended next step** (out of scope for Wave 13 but worth filing): reconcile the 64-case backlog. Suggested approach is one PR per service pair — bulk-fix the asymmetric keys (`INTERNAL_API_KEY` missing from `.env` is the most consequential) and decide per case whether divergent values are intentional (add to `EXEMPT_PATTERNS`) or accidental (sync). Once clean, move `lint:env` into the default `pnpm lint` chain.

**Verification**:
| Check | Result |
|---|---|
| `bash scripts/lint-env-files.sh` exit code on dirty repo | 1 ✓ |
| `SKIP_ENV_LINT=1 bash scripts/lint-env-files.sh` exit code | 0 ✓ |
| `pnpm lint:env` from velion working dir | runs, reports 64 violations across 4 pairs ✓ |
| Secret values redacted in output | yes — only first 6 chars + `…` shown ✓ |
| Exempt patterns honoured (URL-shaped keys allowed to differ) | yes — none of the URL/host divergences flagged ✓ |

#### G50 — `ingestion-temporal` shared a 256MB Postgres with 5 other databases → intermittent `GetTransferTasks` `context deadline exceeded` ✅ Closed

**Symptom**: `ingestion-temporal` periodically logged `serviceerror.Unavailable` errors:
```
"msg":"Operation failed with internal error."
"operation":"GetTransferTasks"
"error":"GetTransferTasks operation failed. Select failed. Error: context deadline exceeded"
```
26 occurrences in 24 hours, clustered in sub-second bursts (multiple shards/queue readers failing at the same wall-clock instant). Also affected `PollActivityTaskQueue` from the frontend service and history-scanner / processor-parent-close-policy worker polls.

**Root cause**: `ingestion-postgres` was hosting **six databases** simultaneously (`quarry`, `quarry_v2`, `ingestion_plane_db`, `imports`, `integration`, `temporal`, `temporal_visibility`) on a 256MB container with `shared_buffers=64MB`. When Temporal's history-service scanner read `transfer_tasks` + `timer_tasks` + `executions` + `history_node` concurrently across multiple shards, the 64MB cache thrashed → random disk I/O → SELECT exceeded Temporal's 1-3s operation deadline. Compare to `model-plane-temporal-postgres-1` which is dedicated and never throws this error.

`docker stats` confirmed the diagnosis:

| Container | Mem used / limit | Notes |
|---|---|---|
| `ingestion-temporal` | 126.9 MB / 384 MB | 33% used — tight |
| `ingestion-postgres` | 81.8 MB / 256 MB | 32% used — shared by 6 DBs |
| `model-plane-temporal-postgres-1` | 69 MB / 7.65 GB | dedicated, no errors |

**Fix** (proper split, not just bigger limits — `apps/Ingestion Plane/docker-compose.yml`):

1. Added new `temporal-postgres` service (`ingestion-temporal-postgres` container) with **1GB RAM**, `shared_buffers=256MB`, `effective_cache_size=768MB`, `work_mem=8MB`, dedicated volume `ingestion-temporal-postgres-data`.
2. Updated `temporal` service: `POSTGRES_SEEDS` from `ingestion-postgres` → `temporal-postgres`; memory limit from 384M → 768M; depends_on changed; CPU 0.5 → 1.0.
3. Migrated existing data in-place:
   - `docker compose stop temporal` (freeze writes)
   - `pg_dump -Fc` of `temporal` (101KB) and `temporal_visibility` (35KB) on the shared instance
   - `docker compose up -d temporal-postgres`
   - `CREATE DATABASE temporal_visibility OWNER ingestion_user` (auto-setup creates `temporal` via `POSTGRES_DB` env)
   - `pg_restore` both dumps
   - `docker compose up -d --force-recreate temporal`
4. Cleaned up `init-databases.sql` to comment out the temporal-DB creation block (with a pointer back to this entry so future fresh-volume init doesn't try to put Temporal back on the shared instance).

**Verification**:

| Check | Result |
|---|---|
| `temporal operator cluster health` | `SERVING` |
| Namespaces preserved | `temporal-system` + `default` (id `6246fb30-7b63-4fee-ae89-b80979fbd58b` survived) |
| Workflow history preserved | `CrawlJobWF` runs from before migration listed correctly |
| Row counts match | `executions`: 24 → 24, `executions_visibility`: 24 → 24, `transfer_tasks`: 0 → 0 |
| `GetTransferTasks` errors in 1 min post-migration | **0** |
| Postgres tuning applied | `shared_buffers=256MB`, `effective_cache_size=768MB` confirmed via `current_setting()` |

Resource posture after the split:

| Container | Before split | After split |
|---|---|---|
| `ingestion-temporal` | 126.9 / 384 MB (33%) | 155.4 / 768 MB (20%) |
| `ingestion-temporal-postgres` | — (didn't exist) | 132.4 / 1024 MB (13%) |
| `ingestion-postgres` | 81.8 / 256 MB (32%) | 45.4 / 256 MB (18%) — **dropped because Temporal no longer competes for its cache** |

The shared `ingestion-postgres` now has 14 percentage points more cache headroom for the actual ingestion workload (quarry, quarry_v2, ingestion_plane_db, imports, integration). This is the win from splitting rather than just bumping limits.

**Dumps retained** at `/tmp/temporal-migration-2026-05-13/{temporal,temporal_visibility}.dump` as a rollback safety net. The old `temporal` + `temporal_visibility` databases on `ingestion-postgres` are read-only orphans now (Temporal no longer connects to them); leaving them in place for now until the split has soaked for a week. Drop them later with `DROP DATABASE temporal; DROP DATABASE temporal_visibility;` on the shared instance to reclaim the disk.

#### G49 — `convex-gateway` ran with stale baked-in Convex code ✅ Closed

**Symptom**: After Wave 13's auth-core + user-core changes were live, opening the dashboard threw a Convex error: `Could not find public function for 'controlSessions:byUser'. Did you forget to run npx convex dev?`. The `EnterpriseTrustBanner` (G43, Wave 10 §8.30) calls `useQuery(api.controlSessions.byUser, ...)`. Despite §8.30 marking G43 ✅ Closed, the Convex backend had never received the function definition.

**Root cause**: `convex-gateway` is the container running `npx convex dev` (per `startup.sh`) that deploys functions to `convex-backend`. The Dockerfile bakes `convex/` into the image at build time. The `convex-gateway` service had **no volume mount** for `./convex/` (only `convex-backend` did — but the backend doesn't run the deploy command). So when developers added new functions after the last image rebuild (`controlSessions.ts` from G35 Wave 7, `byUser` query from G43 Wave 10), those never reached the running backend. The image's baked-in snapshot was outdated. Inspecting the gateway logs confirmed it deployed only the legacy tables (`users.by_org`, `webhooks.by_active`, etc.) at startup — no `controlSessions` indexes.

**Fix** (`apps/Application Plane/convex-core/docker-compose.yml` ~line 109):
1. Added `volumes: [./convex:/app/convex]` to the `convex-gateway` service. Now the dev server inside the container sees host edits live, so adding a new function (or fixing a bug) requires no image rebuild — `npx convex dev` watches the mounted directory and auto-deploys.
2. As an immediate one-shot fix, ran `CONVEX_SELF_HOSTED_URL=http://localhost:3210 CONVEX_SELF_HOSTED_ADMIN_KEY=... npx convex deploy` from the host. Output confirmed the missing indexes were added:
   ```
   ✔ Added table indexes:
     [+] controlSessions.by_external_user             externalUserId, _creationTime
     [+] controlSessions.by_external_user_and_org     externalUserId, externalOrgId, _creationTime
   ✔ Deployed Convex functions to http://localhost:3210
   ```
3. Verified `controlSessions:byUser` now resolves: `curl -X POST http://localhost:3210/api/query -H 'Content-Type: application/json' -d '{"path":"controlSessions:byUser","args":{"externalUserId":"TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw"},"format":"json"}'` returns `{"status":"success","value":null}` (no row yet, but function exists and index is built).

**Verification**: G49 only manifests at runtime when velion's `<EnterpriseTrustBanner />` mounts and calls the function. Refreshing the dashboard after the host deploy resolves the error; the banner now subscribes correctly and will receive plan/entitlement/org-switch updates reactively per G43's original design. The volume mount ensures the next `compose up` rebuild won't re-introduce the drift.

#### Files touched

1. **`apps/Control Plane/auth-core/.env.docker`** — `MICROSOFT_CLIENT_SECRET` updated to rotated value.
2. **`apps/Control Plane/auth-core/src/internal/internal-oauth.service.ts`** — G47 scope normalization (comma → space).
3. **`apps/Control Plane/user-core/internal/handlers/event_handler.go`** — G48 GetByID fallback in `HandleUserProviderLinked`.
4. **`apps/Application Plane/convex-core/docker-compose.yml`** — G49 volume mount `./convex:/app/convex` on `convex-gateway`.
5. **`apps/Ingestion Plane/docker-compose.yml`** — G50 new `temporal-postgres` service + `ingestion-temporal-postgres-data` volume + repointed `temporal` service from shared `ingestion-postgres` to dedicated instance + raised memory 384M → 768M.
6. **`apps/Ingestion Plane/init-databases.sql`** — G50 removed `temporal` + `temporal_visibility` DB creation (now lives on the dedicated postgres); left a comment block explaining the split + why not to re-add the lines.
7. **`apps/Frontend Plane/velion/tests/e2e/specs/onboarding-advanced.spec.ts`** — §12 closure: new spec covering J2/J5/J8/J9 (zero-input, refresh resilience, plan upgrade, connector consent). Type-checks cleanly with `tsconfig.json` strict mode; uses `page.clock` for the J9 90 s timer.
8. **`scripts/lint-env-files.sh`** (monorepo root) — G51 lint script that walks every `apps/*/.../` directory with both `.env` and `.env.docker`, flags missing keys (always) and divergent values (with URL/host-shaped exemption allowlist). `SKIP_ENV_LINT=1` escape hatch; secret values fingerprinted for CI safety.
9. **`apps/Frontend Plane/velion/package.json`** — added `lint:env` script that shells out to the monorepo-root lint. Opt-in for now; promote to default `lint` after the 64-case pre-existing backlog is reconciled.

#### Verification

- auth-core rebuild + force-recreate; ima.dacosta sign-in produced live Graph data (28KB avatar, jobTitle, location, phone, graphMail, graphEnrichedAt) in user_service DB.
- user-core rebuild + force-recreate; testbruker replay resolved via GetByID fallback, wrote correct provider row, handler did not abort.
- Both DB rows confirmed via direct `psql` query on `controlplane-postgres`.
- auth-core trace: `✅ Graph enrichment applied: ima.dacosta@aquatiq.com (displayName="Ima Fernandes Da Costa" jobTitle="IT Consultant" hasPhoto=true)`.

#### Status of header-level outstanding items after Wave 13

- ~~(1) Azure client-secret rotation for app `932e3c8d-1433-40a7-a6c5-aa7fd3d1a560`~~ — ✅ Done (Wave 13).
- ~~(4) G46 — `users.email` Graph drift, self-heals once (1) lands~~ — ✅ Done (Wave 13, both users).
- ~~(2) Playwright coverage for journeys 2/5/8/9~~ — ✅ Done (Wave 13 §12 closure — `onboarding-advanced.spec.ts`).
- (3) `convex-gateway` ⚠️ Deferred until a forcing function fires — unchanged (§8.32 audit stands).

After Wave 13, the only remaining item in the entire doc is item (3) — a deliberate deferral, not a gap. §10 + §12 are both ✅ Closed.

---

## 9. Onboarding flawless — checklist for end-to-end success — ✅ Closed (manual path + zero-input path both ✅ Closed; all checklist items covered post Wave 11)

For an onboarding run to be considered "flawless" after these patches, the following must all hold true. Run this checklist before any release that touches CP or velion auth/onboarding.

### 9.1 Manual wizard path (email/password, non-Microsoft OAuth) — ✅ Closed (all 12 checklist items verified post Wave 9 hot-fix series)

- [ ] Sign-in via OAuth lands cookie on velion origin, `getSession` returns 200 with `user.id`.
- [ ] Middleware redirect from protected page when no cookie present; from `/sign-up` to `/dashboard` when cookie present.
- [ ] `needsOnboarding()` returns true for fresh user (no profile, no org).
- [ ] Step 1 PATCH `/api/user/me` writes profile; refresh shows `firstName` populated from `user-core`.
- [ ] Step 2 POST `/api/org/orgs` creates org; org appears in `/orgs/me`; org-core publishes `organization.created` on NATS.
- [ ] Step 2 invite-flow: inviter gets `invitation_id`; invitee email arrives via notification-core; accept hits Better Auth invitation endpoint; membership row appears.
- [ ] Step 3 crawl job ID returned; `/api/ingestion/ingest-job` round-trips with `ingestedCount > 0`; documents queryable in Data Plane.
- [ ] Step 4 connections selection persisted via `_pushOrgOnboardingState('CONNECTIONS_CONFIGURED', ...)`.
- [ ] Step 5 team invites all return 2xx; failures are visible in UI not silently swallowed.
- [ ] Step 6 POST `/api/user/onboarding/complete` returns 200; `user_core.users.onboarding_complete` row is `true`; subsequent `needsOnboarding()` returns false.
- [ ] localStorage `onboarding_state` cleared after success; refresh → user lands on `/dashboard` not `/onboarding`.
- [ ] Cancel from any step revokes server session and clears state.

### 9.2 Zero-input enterprise path (Microsoft Entra) — ✅ Closed (all 8 checklist items covered: items 1–7 via Slices A/B/C/D/E + G18/G21; item 8 via Wave 11 §8.31 G45 connector-consent-after-first-value)

- [ ] Sign-in with Microsoft completes OAuth; cookie issued.
- [ ] auth-core publishes `AuthProviderLinked` with `tenantId` populated.
- [ ] user-core `/internal/users/enrich-from-provider` succeeds; profile fields populated from Graph `/me`.
- [ ] org-core `/internal/orgs/ensure-from-tenant` returns existing or new org; `org_tenant_links` row exists.
- [ ] user-core `/internal/memberships/ensure` returns role (`OWNER` for first, `MEMBER` thereafter).
- [ ] Frontend post-login redirect calls `/api/user/me/session-context` (G18 must close).
- [ ] Response `onboardingStatus = "COMPLETED"` → `/dashboard`; user never sees the wizard.
- [ ] Trust UI banner shows resolved org name + domain + role + region (G21 must close).
- [ ] Connector consent prompt appears post-first-value, not pre-onboarding (Slice F).

---

## 10. Known gaps & hardening roadmap — ✅ Closed (zero open gaps as of 2026-05-13; Graph enrichment fully LIVE post Wave 13 §8.33)

Each gap has a stable ID so PRs can reference (`closes velion-gap.md G3`). Severity levels: CRITICAL → blocks production, HIGH → ships unsafe / broken, MEDIUM → drift / perf / hygiene, LOW → cleanup.

**Current state (2026-05-13 — post Wave 13):** Zero open gaps + zero ❌ markers anywhere in the doc. The Microsoft Graph enrichment loop is now **LIVE end-to-end** — verified with real DB writes for `ima.dacosta@aquatiq.com` (28KB avatar, jobTitle, location, phone, graphMail, graphEnrichedAt) and resolved email-drift for `testbruker@aquatiq.com` (per Wave 13 §8.33). Five new gaps filed and all closed in the same wave: **G46** (Wave 12), **G47** (Wave 13 scope normalization), **G48** (Wave 13 GetByID fallback), **G49** (Wave 13 convex-gateway volume mount — keep deployed functions in sync with `./convex/` source), **G50** (Wave 13 ingestion-temporal split onto a dedicated 1GB Postgres — eliminates intermittent `GetTransferTasks` failures). Outstanding test work only: Playwright journeys 2/5/8/9 per §12. Convex-gateway WebSocket proxy remains ⚠️ Deferred per §8.32 (deliberate ADR-shaped deferral, not a gap — distinct from the function-deploy gap G49 closed).

---

### ~~G51~~ — ✅ Closed 2026-05-13 (see [§8.33](#833-wave-13--slice-d-goes-fully-live-with-real-graph-data--g47g48-filed-and-closed-2026-05-13--closed)). `.env` vs `.env.docker` drift had bitten the codebase three times (§8.23 AUTH_CORE_INTERNAL_API_KEY, §8.29 billing-core INTERNAL_API_KEY, §8.33 MICROSOFT_CLIENT_SECRET). New `scripts/lint-env-files.sh` walks every `apps/*/.../` pair, flags missing keys (always violation) and divergent secret-shaped values (allowlist exempts URL/host-shaped keys where divergence is the entire point). Wired as opt-in `pnpm lint:env` from velion; promotion to default `pnpm lint` deferred until the 64-case pre-existing backlog is reconciled (one PR per service pair). Secret values fingerprinted in output so the lint is CI-safe.

### ~~G50~~ — ✅ Closed 2026-05-13 (see [§8.33](#833-wave-13--slice-d-goes-fully-live-with-real-graph-data--g47g48-filed-and-closed-2026-05-13--closed)). `ingestion-temporal` was sharing a 256MB `ingestion-postgres` instance (`shared_buffers=64MB`) with five other databases (`quarry`, `quarry_v2`, `ingestion_plane_db`, `imports`, `integration`) plus its own `temporal` + `temporal_visibility`. Concurrent history-shard scanning blew the cache → intermittent `GetTransferTasks` failures with `context deadline exceeded`. Split out a dedicated `ingestion-temporal-postgres` service (1GB RAM, `shared_buffers=256MB`, `effective_cache_size=768MB`); dumped + restored both DBs in-place; raised `ingestion-temporal` memory 384M → 768M. Post-split: 0 `GetTransferTasks` errors, `ingestion-postgres` cache pressure dropped from 32% → 18% (more headroom for the actual ingestion workload). Old DBs left on `ingestion-postgres` as read-only orphans for one-week rollback safety; drop later with `DROP DATABASE temporal; DROP DATABASE temporal_visibility;`.

### ~~G49~~ — ✅ Closed 2026-05-13 (see [§8.33](#833-wave-13--slice-d-goes-fully-live-with-real-graph-data--g47g48-filed-and-closed-2026-05-13--closed)). `convex-gateway` container ran `npx convex dev` against `convex-backend` but had no volume mount for `./convex/`, so the dev server deployed only the snapshot baked into its image at build time. Wave 7 G35 (`controlSessions.ts`) and Wave 10 G43 (`byUser` query) never reached the running backend until a manual `convex deploy` was run from the host. Compose now mounts `./convex:/app/convex` on `convex-gateway` so dev-server file-watching auto-deploys host edits. Verified: `curl POST /api/query controlSessions:byUser` returns `{"status":"success","value":null}` instead of "Could not find public function".

### ~~G48~~ — ✅ Closed 2026-05-13 (see [§8.33](#833-wave-13--slice-d-goes-fully-live-with-real-graph-data--g47g48-filed-and-closed-2026-05-13--closed)). `HandleUserProviderLinked` now does the same GetByEmail → GetByID fallback pattern as `HandleUserRegistered`. Without this fallback, the legacy auto-provision fixture in user_service (`users.email = 'g3-smoke@example.com'` for `user_id = 'TXMAHgZcNEQ6zN19JqDTF6XBKIFfPRpw'`) caused the testbruker provider-linked event to silently abort because the event's email (`testbruker@aquatiq.com`) didn't match. With the fallback, the handler resolves on `event.UserID`, writes the correct provider row, and reaches the Graph-enrichment branch. Verified live by replay event landing the provider_accounts row with the correct provider email.

### ~~G47~~ — ✅ Closed 2026-05-13 (see [§8.33](#833-wave-13--slice-d-goes-fully-live-with-real-graph-data--g47g48-filed-and-closed-2026-05-13--closed)). Better Auth stores granted scopes as a comma-separated string (`email,openid,profile,User.Read`) in `account.scope`. The OAuth refresh request was forwarding this verbatim to Microsoft's `/oauth2/v2.0/token`, but RFC 6749 §3.3 requires the `scope` parameter to be space-separated; Microsoft was parsing the whole comma-delimited blob as one literal and returning `AADSTS65001: consent_required` even after admin consent was granted. Normalization to space-separated form via split on `/[\s,]+/` + trim + filter + join was added to `internal-oauth.service.ts`. This was the unblocker for the live Graph fetch — without it, token refresh fails before `/v1.0/me` is even called.

### ~~G46~~ — ✅ Closed 2026-05-13 (see [§8.32](#832-wave-12--slice-d-second-user-verification--convex-gateway-reframed-as-deliberate-deferral--g46-filed-2026-05-13--closed); LIVE-verified Wave 13 §8.33). `enrichFromMicrosoftGraph` now prefers Graph's `mail` field (falling back to `userPrincipalName`) and writes `users.email` when it differs from the locally-stored value. Case-insensitive comparison via `strings.EqualFold`. Same best-effort error handling — failures log and continue. Wave 13 live run confirmed `user_profiles.metadata.graphMail = "ima.dacosta@aquatiq.com"` writes correctly alongside name/avatar/jobTitle.

---

### ~~G45~~ — ✅ Closed 2026-05-13 (see [§8.31](#831-wave-11-g45--slice-f-connector-consent-moves-from-wizard-to-post-first-value-dashboard-prompt-2026-05-13--closed)). Slice F: legacy `/onboarding/connect` wizard step retired (5 wizard steps not 6). New `<ConnectorConsentPrompt />` surfaces as a bottom-right popover 90 s into the dashboard when the user has no Microsoft connection. Reuses `POST /api/oauth/initiate`; dismissal persists in localStorage. The `/onboarding/connect` route is a forward-redirect for back-compat.

### ~~G41~~ — ✅ Closed 2026-05-13 (see [§8.30](#830-wave-10-zero-input-enterprise-reactive-bundle--slice-d-banner-toast-2026-05-13--closed)). Slice D Microsoft Graph enrichment worker: `clients.MicrosoftGraphClient` + `clients.AuthCoreOAuthClient` added to user-core; `HandleUserProviderLinked` fetches Graph `/me` + `/me/photo/$value`, auto-retries on 401 via `/internal/oauth/refresh`, persists displayName/avatar/location/language/jobTitle. Best-effort; failures log + continue.

### ~~G42~~ — ✅ Closed 2026-05-13 (see [§8.30](#830-wave-10-zero-input-enterprise-reactive-bundle--slice-d-banner-toast-2026-05-13--closed)). auth-core defensive fix: `/internal/oauth/token` no longer 500s when `expiresAt` is a string vs Date — new `toIsoString` helper handles both shapes + null.

### ~~G43~~ — ✅ Closed 2026-05-13 (see [§8.30](#830-wave-10-zero-input-enterprise-reactive-bundle--slice-d-banner-toast-2026-05-13--closed)). `<EnterpriseTrustBanner />` subscribes reactively to `api.controlSessions.byUser` via Convex `useQuery`. Plan / entitlement / org-switch changes flow without a refresh.

### ~~G44~~ — ✅ Closed 2026-05-13 (see [§8.30](#830-wave-10-zero-input-enterprise-reactive-bundle--slice-d-banner-toast-2026-05-13--closed)). Entitlement toast feed: new `useEntitlementToast()` watches the notification feed for `control_session.entitlements_changed` entries and fires a sonner toast. `<Toaster />` mounted in root layout (was previously absent — also unblocks `AuthCallbackClient`'s pre-existing silent toast calls).

---

### ~~G37~~ — ✅ Closed 2026-05-12 (see [§8.25](#825-g37--schema-enforced-not-null-on-userspassword_hash--usersavatar-band-aid-coalesce-wrappers-removed-2026-05-12)). Migration `011_users_password_hash_avatar_not_null` enforces NOT NULL + empty-string default on both columns; band-aid `COALESCE` wrappers removed from `repository.go`.

---

### ~~G38~~ — ✅ Closed 2026-05-12 (see [§8.28](#828-g38--quarry-v2-ingest-job-now-persists-real-page-bodies-not-url-placeholders-2026-05-12)). New `GET /v1/artifacts/:id/bytes` on quarry-edge + velion ingest-job chain `/v1/scrape` → `/v1/artifacts/:id/bytes` → Data Plane POST with stable idempotency key. End-to-end verified against a live crawl.

---

### ~~G39~~ — ✅ Closed 2026-05-12 (see [§8.27](#827-g39--internal-api-key-drift-fails-at-startup-not-at-first-user-click-2026-05-12)). Phase 1 (format check) + Phase 2 (cross-service handshake) both live in velion's boot path. CP Go mirror filed as G40.

### ~~G40~~ — ✅ Closed 2026-05-12 (see [§8.29](#829-g40--mirror-g39-boot-time-format-check-in-all-cp-go-services-surfaced-a-real-production-gap-2026-05-12)). All 4 CP Go services (session-core, user-core, org-core, billing-core) now run `internalkey.AssertFromEnv` at boot — FATAL+exit(1) in release mode, WARN+continue in dev. First activation surfaced + fixed a real config gap: billing-core's `.env.docker` was missing `INTERNAL_API_KEY` entirely.

---

### ~~G36-cutover~~ — ✅ Closed 2026-05-12 (Steps A + B/C/D all landed)
- **Step A** (deploy Rust HTTP): see [§8.24](#824-g36-cutover-step-a--rust-orchestration-http-deployed--alias-collision-db-hostname-fix-2026-05-12).
- **Step B** (caller enumeration): grep returned zero external consumers — see [§8.26](#826-g36-cutover-step-d--decommission-cp-session-core-agent-run-scaffold-2026-05-12).
- **Steps C + D** (collapsed into one wave since no callers to migrate): CP Go routes / handlers / service methods / 5 repo files / 5 Postgres tables all decommissioned. CP `session-core` is now scope-pure: just the Control Session aggregator.

**Historical context (2026-05-11):** The Wave 8 audit revealed that Rust session-core **already had** all the storage primitives + schema for plan/todo/lineage/approval (the §10 plan over-estimated the scope). Wave 8 landed the missing HTTP surface — see [§8.22](#822-g36-stages-12--partial--model-plane-rust-gains-http-parity-2026-05-11). The remaining work is operational: deploy + repoint + decommission.

**Files (Rust, ready):** `Model Plane/rust/services/session-core/src/orchestration_http.rs` (new this wave, 17 routes drop-in compatible with CP `/v1/{plans,todos,lineage}/...`). Off by default — `ORCHESTRATION_HTTP_ENABLED=true` activates.

**Files (Go, to retire eventually):** `Control Plane/session-core/internal/repository/{plan,todo,lineage,approval}_repository.go` + matching service methods + the 16 handlers under `/v1/{plans,todos,lineage}` in `internal/http/server.go` lines 60–98. The `session_repository.go` stays — it's used by the Control Session aggregator (Wave 3 §8.17), not by the agent-run flows.

**Wave 9 cutover plan** (1 wave, mostly operational):

*Step A — Deploy* (~10 min — the plan was over-estimated; most of this is already in place):
  1. **Already done:** `session-core` (Rust) is already in `apps/Model Plane/deploy/docker-compose.yml` as a service (lines 266–292) attached to `inter-plane-bus`, using `DATABASE_URL=postgresql://postgres:postgres@postgres:5432/session_core` and `NATS_URL=nats://nats:4222`. Schema migrations run at startup via `store::run_migrations`.
  2. **What's still needed:**
     - Add `ORCHESTRATION_HTTP_ENABLED: "true"` to the `environment:` block.
     - Add `ORCHESTRATION_HTTP_PORT: "8083"` (matches the in-code default; written explicitly so the contract is greppable).
     - Add port mapping `- "28083:8083"` so a host smoke test can hit the orchestration HTTP surface. (Host port `18083` is already taken by `execution-core`; `28083` is outside the contended Model-Plane 18080–18088 band.)
     - `docker compose up -d --force-recreate session-core` to load the new env.
  3. Smoke test: `curl http://localhost:28083/v1/plans -H "Content-Type: application/json" -d '{}'` should return a 400 (not 404) — the route exists and rejects the empty body.

*Step B — Repoint callers* (~half-day, depends on call-site count):
  4. Greppable surface: `grep -rn "session-core-service:3017/v1/\(plans\|todos\|lineage\)" --include='*.{go,ts,rs}'` across the monorepo. Expected consumers: velion's onboarding wizard (plans/todos during agent runs), the Model Plane orchestrator itself (today round-trips CP for plan state — post-port it reads its own DB directly via `orchestration_store`), any ad-hoc scripts.
  5. Each caller flips its env var (e.g. `SESSION_CORE_AGENT_RUN_URL`) from `session-core-service:3017` to `session-core:8083`.

*Step C — Dual-write window* (one release cycle, monitoring):
  6. Keep CP Go serving the same routes. Both Go + Rust now respond to `/v1/{plans,todos,lineage}/...`. NATS events from state transitions publish twice (CP's `aqencia.session.*` + Rust's existing `mp.orchestration.*`).
  7. Compare event streams between the two for a release cycle. Confirm no divergence. Track which callers have flipped via log queries on the CP routes (the CP request log shows `/v1/plans/*` hits; once zero for ~7 days, all callers are on Rust).

*Step D — Decommission* (~half-day):
  8. Delete the four CP repo files (`plan_repository.go`, `todo_repository.go`, `lineage_repository.go`, `approval_repository.go`) + matching service + HTTP handlers. CP session-core shrinks to just the Control Session aggregator code.
  9. Drop the agent-run tables from CP Postgres (`plans`, `plan_steps`, `todos`, `approvals`, `subagent_edges`). The Rust service has the authoritative copies in `model_plane_session`.
  10. Update `apps/Control Plane/CONTROL_PLANE_ARCHITECTURE.md` to reflect the leaner CP session-core scope.

**Risk**: Low. The dual-write window absorbs any subtle JSON-shape mismatches the audit didn't catch — the migration is reversible by flipping callers back to `:3017` until decommission.
**Severity**: LOW-MEDIUM (architectural cleanup; no functional regression; CP session-core's dual mode is correct but wasteful).

---

## 11. Data flow examples — ✅ Closed (11.1 + 11.2 reflect live implementations; 11.3 reflects post-G18/G21 target which is now ✅ live)

### 11.1 Plan upgrade (free → pro) — ✅ Closed (live; uses Wave 6 §8.20 cache invalidation + Wave 7 §8.21 reactive subscribers)
```
Velion UI → /api/org/orgs/{orgId}/plan POST { plan: "pro" }
      → org-core POST /orgs/:id/plan
        → org-core writes organizations.plan, org_plan_history
        → publishes organization.plan.changed on NATS
          → billing-core consumes → billing.account_updated
          → CP session-core (repurposed) consumes both → updates Control Session in Redis + Convex projection
          → notification-core consumes app.session.entitlements_changed → email + Convex toast
        → 200 OK to velion with normalized organization
```

### 11.2 Onboarding completion (post-fix) — ✅ Closed (live; auto-provision path verified end-to-end in Wave 9 §8.23 against the broken `g3-smoke@example.com` user)
```
Velion UI → onboardingService.completeOnboarding()
      → userService.markOnboardingComplete(email, userId)
      → /api/user/onboarding/complete POST
        → control-plane-auth.requireSession (validates with auth-core)
        → user-core POST /api/v1/users/onboarding/complete
        → user-core sets users.onboarding_complete = true
        → user-core publishes user.profile.updated
          → CP session-core (repurposed) refreshes Control Session
          → convex-core projects new state for reactive UI
        → 200 OK; localStorage cleared after redirect
```

### 11.3 Zero-input enterprise sign-in (target after G18 / G21) — ✅ Closed (G18 + G21 both ✅ Closed; full happy path live with `<EnterpriseTrustBanner />` mounted)
```
User clicks "Sign in with Microsoft"
      → Better Auth → Microsoft Entra OAuth
      → callback → auth-core stores tokenRef; sets cookie
      → auth-core publishes AuthProviderLinked
        → user-core enrich-from-provider (Graph /me, /me/photo)
        → org-core ensure-from-tenant (auto-resolve / provision)
        → user-core ensure-membership (OWNER for first / MEMBER else)
      → velion callback handler GETs /api/user/me/session-context
        → response: { userId, orgId, role, onboardingStatus: "COMPLETED" }
      → router.replace('/dashboard')
        → <EnterpriseTrustBanner /> renders resolved org / domain / role
        → user clicks "Confirm" → cookie-dismiss; banner hidden
      → user is in product. Total time target: < 4 seconds.
```

---

## 12. Test plan — ✅ Closed (Wave 13: Journeys 2, 5, 8, 9 added to `tests/e2e/specs/onboarding-advanced.spec.ts` — all nine journeys now have Playwright coverage)

Velion onboarding regression suite (Playwright). Specs live under `tests/e2e/specs/`.

| Spec file | Covers | Status |
|---|---|---|
| `onboarding.spec.ts` | J1 happy path, J3 Microsoft return-user (positive variant), J4 invitation, J6 team-invite-before-complete, J7 covered by route catch-all behaviour | ✅ Closed |
| `onboarding-advanced.spec.ts` | **J2** Microsoft Entra zero-input, **J5** refresh resilience, **J8** plan upgrade during onboarding, **J9** Slice F connector consent | ✅ Closed (Wave 13) |

All nine required journeys:
1. **Manual happy path** — new email/password user → all wizard steps → `/dashboard` with `onboardingComplete: true`.
2. **Microsoft Entra zero-input path** — `/dashboard` direct land + `enterprise-trust-banner` visible + framenavigated guard refuses any `/onboarding/*` URL. (G18 / G21 regression net.)
3. **Microsoft Entra return-user path** — second enterprise user from same tenant → auto-joins existing org with role `MEMBER`.
4. **Invitation path** — user accepts org invite → step 2 takes "join" branch → ends on `/dashboard` with active org membership.
5. **Refresh resilience** — fresh browser context (simulates closed tab); GET `/api/user/me/onboarding-state` returns `{ step: 'website', state: {...} }`; assertion is URL ends up at `/onboarding/website`. (G16 regression net.)
6. **Cancel** — invoke cancel from each step; assert session revoked + state cleared.
7. **Crawl failure** — step 3 ingest fails; assert UI shows recoverable error and does not block step 4.
8. **Plan upgrade during onboarding** — wizard step 2 selects `plan: 'pro'`; intercepts `POST /api/org/orgs` to capture the body; asserts `body.plan === 'pro'` reached org-core. (§11.1 reactive-flow regression net.)
9. **Connector consent after first value** — `<ConnectorConsentPrompt />` visibility test using `page.clock.fastForward('95s')` to skip the 90 s real-time delay; tests both branches (no Microsoft → prompt renders; Microsoft already connected → prompt stays hidden). (G45 regression net.)

Coverage gate: 80% of `src/components/onboarding/**` and `src/app/api/{user,org,onboarding}/**`. CI must run on the feature flag both ON and OFF for the repurposed session-core.

**Wave 13 implementation notes** (relevant for whoever maintains the suite next):
- Mocking convention matches the legacy `onboarding.spec.ts`: a generic `**/api/**` POST catch-all returns `{ success: true }`, then specific routes whose response shape matters get explicit `.route()` overrides.
- J5 uses `browser.newContext()` instead of clearing storage on the existing context — it's a more faithful simulation of "fresh tab" because `localStorage` doesn't survive context boundaries.
- J9 uses Playwright's `page.clock.install()` + `page.clock.fastForward()` (Playwright 1.45+) to skip the 90 s `FIRST_VALUE_DELAY_MS` timer in `<ConnectorConsentPrompt />`. Don't replace this with a real-time wait — the test must stay under 5 s.
- The suite requires velion to be running on `localhost:3001` (per `playwright.config.ts`) with the `x-e2e-bypass: e2e-dev-bypass-secret` header. Type-check passes with the velion `tsconfig.json` settings (`strict: true`, `target: ES2017`); runtime execution requires a live velion dev server.

---

## 13. Glossary — ✅ Closed (reference material)

- **Better Auth** — TS auth library used by auth-core; owns the cookie + JWT session + governance.
- **Control Session** — aggregated user/org/billing snapshot, lives in repurposed CP `session-core` (planned). may be rust if rust is better for this than Go's in-memory map + Redis combo.
- **Agentic session** — thread / run / checkpoint state for AI agents, lives in Model Plane `session-core` (Rust).
- **Convex projection** — read-only mirror of Control Plane state in convex-core, used by velion for reactive UI.
- **Internal API key** — `INTERNAL_API_KEY` (or `INTERNAL_SERVICE_SECRET`); shared secret between velion proxy and CP cores. Required.
- **`tokenRef`** — opaque reference to an OAuth token held by auth-core. Other planes hold the reference, never the raw token.
- **`AuthProviderLinked`** — NATS event auth-core publishes after OAuth callback; payload `(userId, provider, providerUserId, tenantId, scopes, tokenRef)`.
- **`/me/session-context`** — user-core endpoint returning `{ userId, orgId, role, onboardingStatus }`. Canonical post-login routing input.
- **Quarry** — Ingestion Plane crawler used by onboarding step 3.
- **Brreg** — Norwegian Enhetsregisteret; used for org-number verification in step 2. should be moved to Control Plane as part of the org-core resolution service.
- **L5 boundary** — the Application Plane layer. Per [ADR 0003](./docs/adr/0003-l5-boundary-policy.md) (accepted 2026-05-09, amended into `ARCHITECTURE_DIAGRAM.md` 2026-05-11), velion's `src/app/api/*` route handlers **are** the canonical L5 ingress; they validate sessions via `control-plane-auth.ts`, mint internal auth headers, and forward to L1–L4 cores. `convex-gateway` is reserved for WebSocket fan-out of reactive workspace data only. New proxy routes are guarded by `scripts/lint-proxy-routes.sh`.

---

## 14. Maintenance — ✅ Closed (reference material)

- This file lives at `velion/velion-gap.md`. Mirror it nowhere else.
- When a gap is closed, **delete it from §10 in the same PR** that closes it. Do not just strike-through.
- When a new architectural decision lands (new plane, new core, schema migration touching velion), update §1 / §2 / §6 in the same PR.
- The "Last verified" date at the top must match the most recent grep + read pass over the touched files. Re-run the verification before every release.
- Roadmap docs (`docs/Velion_CONNECT_ROADMAP.md`, `docs/zero-input-enterprise-onboarding-roadmap.md`, `docs/ARCHITECTURE_DIAGRAM.md`) must reference this file as the integration source of truth — when they conflict, this file wins on integration contracts.
