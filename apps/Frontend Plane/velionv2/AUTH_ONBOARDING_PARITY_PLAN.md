# Plan: velionv2 Auth + Onboarding → Control Plane (production-ready parity)

**Date:** 2026-05-30 · **Status:** AWAITING CONFIRMATION (no code until approved)
**Goal:** Turn velionv2's login / sign-in / session / onboarding from an auth-only shell into a true, production-ready system at full parity with the Control Plane — every step wired to real cores (auth, user, org+BREG, billing; session optional).
**Companion docs:** `CONTROL_PLANE_PARITY_AUDIT.md` (gap analysis), `BACKEND_CONTRACT_CATALOG.md` (endpoint/event contracts), `V1_PORT_INVENTORY.md` (port kit from velion v1).

---

## 1. Requirements restatement

1. **Login / sign-in / session**: email+password, 2FA (TOTP), social (Google/Microsoft), passkey — all authoritatively backed by **auth-core** (Better Auth) via the same-origin proxy. Real session, fail-closed in prod (no silent standalone).
2. **Onboarding**: signup → org (with **BREG** company lookup) → profile → plan/paywall → complete — every step **persisted in the Control Plane**, not local React state.
3. **Org handling**: create/select org via **org-core**; org enriched from **BREG** registry (`data.brreg.no`); org context available app-wide.
4. **Billing/paywall**: plan selection persisted via **billing-core**; paid plans → Stripe checkout; workspace features gated on **real entitlements** (402).
5. **Production-ready**: route protection (middleware + RSC guards), hardening (rate limit, trusted origins, secure cookies, secret validation), audit, tests (≥80%).

---

## 2. Target architecture (call paths)

```
Browser ──cookie──> velionv2 (Next.js, same-origin)
  /api/auth/*        → proxyControlPlaneAuthRequest()      → auth-core :3011  (Better Auth, cookie idknuten.sid)
  /api/org/*  (NEW)  → requireSession + internal headers   → org-core :8080   (orgs, members, roles, BREG)
                       smart fan-out (billing/quota/plan/checkout) → billing-core :3013/3014
  /api/v1/*          → fetchUserCoreJson (X-Internal-Api-Key) → user-core :3012 (profile, session-context, onboarding)

Server context composer  getControlPlaneContext():
  auth-core /api/auth/get-session  → identity
  user-core /api/v1/me/session-context → { orgId, role, onboardingStatus }
  billing-core /entitlements        → { plan, features, quotas }

Event-driven provisioning (NATS, already built backend-side):
  org-core POST /orgs ──organization.created──> billing-core auto-provisions FREE account
  org-core POST /orgs/:id/plan ──organization.plan.changed──> billing-core updates entitlements
```

Auth-mode rule: **CP mode is authoritative.** `AUTH_CORE_URL` set ⇒ proxy to auth-core. Unset **in production ⇒ fail closed** (503). Local Better Auth (`auth.ts`) is dev-only.

---

## 3. Key decisions (recommended — confirm or modify)

| # | Decision | Recommended | Alternative |
|---|---|---|---|
| D1 | Route protection | **middleware.ts (coarse cookie check) + RSC layout guard (full validation + onboarding/org)** | layout-guard only (v1 style, no middleware) |
| D2 | Onboarding plans | **free/trial inline; paid → Stripe checkout redirect + resume** | free-only at onboarding, upgrade later in settings |
| D3 | Standalone Better Auth | **keep for local dev, fail-closed in prod** | remove entirely (CP-only) |
| D4 | session-core | **defer (auth-core session sufficient for parity)** | integrate `/api/v1/sessions/current` + `/refresh` now |
| D5 | Env var names | **standardize to `ORG_SERVICE_URL`,`BILLING_SERVICE_URL` (match ported proxy + docker-compose)** | keep `*_CORE_URL` aliases too |

---

## 4. Phased implementation

### Phase 0 — Foundations & auth-mode hardening  *(complexity: LOW)*
- **Fail-closed auth mode**: `src/app/api/auth/[...all]/route.ts:10–15` — if `!isControlPlaneAuthConfigured()` and `NODE_ENV==="production"` → return 503 (don't import local `auth.ts`). Mirror check in `getCurrentAuthUser` (`onboarding-access.ts:118–145`).
- **Fix client base URL**: ensure `NEXT_PUBLIC_AUTH_BASE_URL` = app origin (`auth-client.ts:6`); add fallback to `NEXT_PUBLIC_APP_URL`.
- **Complete `.env.example`**: add `NEXT_PUBLIC_AUTH_BASE_URL`, `ORG_SERVICE_URL`, `BILLING_SERVICE_URL`, `BETTER_AUTH_TRUSTED_ORIGINS`, `RATE_LIMIT_ENABLED`, (optional `SESSION_SERVICE_URL`). Document CP-vs-standalone.
- **auth-core prod config (coordinate backend)**: `RATE_LIMIT_ENABLED=true`, set trusted origins, secure cookies. (Backend toggle; verify in `auth-core/src/auth/auth.ts`.)
- **DoD**: deterministic auth mode; `/api/v1/auth/config` reports `control-plane`; no silent fallback in prod.

### Phase 1 — CP gateway + session/org context backbone  *(MEDIUM)*  — depends on P0
- **Port `/api/org/[...path]/route.ts`** from v1 (`V1_PORT_INVENTORY.md` §3): `requireSession` → `buildControlPlaneHeaders` (X-Internal-Api-Key, X-User-Id, X-User-Email, X-Correlation-Id) → fan-out: `billing|quota|plan|checkout` → billing-core, BRREG + everything else → org-core. New file: `src/app/api/org/[...path]/route.ts`.
- **`getControlPlaneContext()`** server util (new `src/lib/auth/control-plane-context.ts`): compose get-session + `/api/v1/me/session-context` + billing entitlements; per-request memoized. Extend `getCurrentAuthUser` (`onboarding-access.ts:121–137`) to surface `orgId`/`role`.
- **Client context**: `ControlPlaneProvider` + `useControlPlaneContext()` / `useEntitlements()` hooks (new `src/features/shell-v2/...`).
- **DoD**: any component can read `{user, org, role, onboardingStatus, entitlements}`; gateway reaches org-core + billing-core with auth propagated.

### Phase 2 — Onboarding wired end-to-end  *(HIGH — core ask)*  — depends on P1
- **Port BREG kit** (`V1_PORT_INVENTORY.md` §2): `src/lib/services/brreg-service.ts` (`/api/org/api/v1/brreg/*`) + `BrregSearch.tsx` (300ms debounce, TanStack Query). Reuse as-is.
- **OrganizationStep** (`VelionOnboardingPage.tsx:342–427`): embed `BrregSearch`; on select capture `org_number` + `brreg_data`; on continue `POST /api/org/orgs {name, slug, plan, org_number, brreg_data}` → store `orgId` in reducer. (org-core emits `organization.created` → billing auto-provisions free.)
- **Profile/website** (`:429–493`): `PATCH /api/v1/users/me`; optional Quarry crawl (non-blocking, flagged).
- **PaywallStep** (`:666–803`): persist plan — free/trial → `POST /api/org/orgs/:id/plan`; paid → `POST /api/org/orgs/:id/checkout-session` (→ `{id,url}`) → redirect to Stripe, resume on return. Entitlement-aware copy.
- **AssemblyStep** (`:846–866`): keep `POST /api/v1/users/onboarding/complete` as capstone; require success in prod (drop silent local-only completion); gate on prior steps.
- **Progressive persistence**: best-effort `POST org-core internal onboarding/state` per step (resume on reload).
- **State shape** (`:41–50`): add `orgId, orgNumber, brregData, plan/priceId, profile`.
- **DoD**: signup→org(BREG)→profile→plan→complete all persisted; reload resumes; data never dropped.

### Phase 3 — Workspace gating + real paywall enforcement  *(MEDIUM)*  — depends on P1
- **`middleware.ts`** (new): edge presence-check of session cookie for `(workspace)/*` → redirect unauthenticated → `/login` (coarse/fast).
- **RSC guard**: insert `requireCompletedOnboarding()` into `src/app/(workspace)/layout.tsx:3` (currently bare `return children`).
- **Entitlement enforcement**: `requireEntitlement(feature)` server util (billing-core 402) gates premium RSC routes; `useEntitlements()` for UI; replace hardcoded billing/seats/invoice rows in `VelionWorkspaceSettingsPage` with live billing-core reads.
- **DoD**: no unauth access; no onboarding bypass; premium features gated on real entitlements; settings show live billing.

### Phase 4 — Auth completeness, hardening & tests  *(MEDIUM-HIGH — prod gate)*  — depends on P0–P3
- **Passkey**: add `passkeyClient` to `auth-client.ts`; wire `beginPasskeySignIn` (`VelionAuthPage.tsx:413–421`) → `authClient.passkey.signIn()` (proxied to auth-core passkey plugin).
- **Social/SSO**: verify Google/Microsoft callbackURL + `trustedOrigins` end-to-end.
- **2FA**: confirm enable + backup-codes UI.
- **Audit**: ensure login / org-create / plan-change produce audit-core events (via cores' NATS; add explicit emit only where frontend-initiated).
- **Hardening** (better-auth-security-best-practices): rate limiting, trusted origins, secure+sameSite cookies, startup secret validation (`INTERNAL_API_KEY`, `BETTER_AUTH_SECRET`), remove `console.log` (incl. oRPC v2 controller).
- **Tests** (repo rule ≥80%): Playwright e2e (signup→onboarding→workspace; paywall gate; 2FA; passkey); unit (brreg-service, gateway headers, context composer, entitlement util, zod schemas + existing `auth-schema.test.ts`); integration against cores (sandbox).
- **DoD**: full auth surface; hardened; green e2e + coverage gate.

---

## 5. Risks & mitigations

| Sev | Risk | Mitigation |
|---|---|---|
| HIGH | Silent standalone-auth in prod (diverging sessions) | P0 fail-closed; `/api/v1/auth/config` assertion in CI |
| HIGH | Cookie/domain mismatch through proxy (`idknuten.sid`) | `rewriteSetCookieForProxy` already strips domain; verify cross-subdomain + secure flags in staging |
| HIGH | Org→billing is **event-driven** (NATS) → entitlements lag right after org create | Don't block onboarding on billing read; assume free entitlements optimistically; refresh/poll post-create |
| MED | `session-context` has **no plan** field | Compose plan/entitlements from billing-core in `getControlPlaneContext()`; cache per request |
| MED | Paid-plan Stripe redirect mid-onboarding | Resume-state + idempotent org/plan create; return URL → onboarding step |
| MED | Missing `INTERNAL_API_KEY` ⇒ 503 from user-core/gateway | Validate secrets at startup; clear error |
| MED | Env var name drift vs docker-compose service names | D5 standardization + documented `.env.example` |
| LOW | `typedRoutes:true` needs typegen for new routes | run build/typegen in CI |

---

## 6. Env surface (final `.env.example`)
`NEXT_PUBLIC_APP_URL`, `NEXT_PUBLIC_AUTH_BASE_URL`, `AUTH_CORE_URL`, `USER_CORE_URL`/`USER_SERVICE_URL`, `ORG_SERVICE_URL`, `BILLING_SERVICE_URL`, `INTERNAL_API_KEY`, `BETTER_AUTH_TRUSTED_ORIGINS`, `RATE_LIMIT_ENABLED`, (optional `SESSION_SERVICE_URL`), Quarry + autocomplete vars; standalone-only (dev): `BETTER_AUTH_SECRET`,`BETTER_AUTH_URL`,`DATABASE_URL`.

---

## 7. Definition of Done — parity checklist
- [ ] Sign-in / 2FA / social / passkey all backed by auth-core (CP mode), fail-closed in prod
- [ ] Onboarding org step does BREG lookup → creates org in org-core with `brreg_data`
- [ ] Plan selection persisted (free→plan; paid→Stripe checkout); billing-core entitlements live
- [ ] Workspace gated (middleware + RSC); premium features enforce entitlements (402)
- [ ] Settings billing/members/audit read live cores (no hardcoded rows)
- [ ] `getControlPlaneContext()` powers app-wide user/org/plan context
- [ ] Hardening (rate-limit, trusted origins, secure cookies, secret validation) + audit events
- [ ] e2e + unit/integration tests green, ≥80% coverage

---

## 8. Effort (rough): P0 ~0.5d · P1 ~1.5d · P2 ~2–3d · P3 ~1.5d · P4 ~2–3d → **~8–11 dev-days**

**WAITING FOR CONFIRMATION** — approve (yes), or modify (e.g. answers to D1–D5, reorder, descope). No code will be written until you confirm.
