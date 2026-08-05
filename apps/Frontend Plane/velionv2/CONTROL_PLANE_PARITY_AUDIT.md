# Verevon V2 ↔ Control Plane Parity Audit

**Date:** 2026-05-30 · **Subject:** `apps/Frontend Plane/verevonv2` · **Baseline:** `apps/Control Plane/{auth,session,org,billing,user,audit}-core`
**Method:** 6 parallel evidence-based exploration agents over the CodeGraph-indexed workspace (Go + TS/TSX), key claims spot-verified directly (grep/Read/CodeGraph).

---

## TL;DR — Verdict: ❌ NOT full parity (PARTIAL)

verevonv2 is today an **auth + user-profile shell**. It wires **2 of 6** Control Plane cores (**auth-core**, **user-core**). The Control Plane backend is rich and real — but verevonv2 does **not** consume most of it.

- **Sign-in → session:** ✅ real (proxied to auth-core Better Auth) — but no route middleware, passkey stubbed.
- **Org handling + BREG:** ⚠️ backend REAL, **verevonv2 frontend MISSING entirely** (0 refs). BREG end-to-end exists only in **verevon v1**.
- **Billing + paywall:** ❌ paywall is **cosmetic**; never calls billing-core; plan selection is dropped. Bypassable.
- **Onboarding:** ⚠️ only the completion handshake + gating are real; org/plan/connector steps are ephemeral local state.
- **session-core, audit-core:** ❌ unused by verevonv2.

> The user's premises — "org core uses BREG via its API" and "paywall uses the billing core" — are **TRUE for the backend cores** but **FALSE for verevonv2's wiring**. verevonv2 does not exercise "full control plane behaviour using all its cores."

---

## Scorecard

| Dimension | Backend (Control Plane) | verevonv2 wiring | Parity |
|---|---|---|---|
| Sign-in / password / 2FA / social | auth-core: Better Auth (emailPwd, twoFactor, OAuth, passkey, org, OIDC, SSO) — full | Proxies `/api/auth/*` → auth-core (cookie-transparent) | 🟡 PARTIAL |
| Session lifecycle | session-core (Go, Redis, NATS) + auth-core Redis store | No session-core ref; relies on auth-core cookie | 🟡 PARTIAL |
| Org handling | org-core: CRUD, members, RBAC, plan, capabilities | **None** (no `ORG_CORE_URL`, no `/api/org` proxy) | 🔴 MISSING |
| **BREG registry lookup** | org-core: **real** `data.brreg.no/enhetsregisteret` client | **None** (0 brreg refs in v2; v1 had it) | 🔴 MISSING |
| Billing | billing-core: **real** Lago (metering) + Stripe (checkout/invoices) + NATS plan sync + entitlement/quota API (402) | **None** (no `BILLING_CORE_URL`) | 🔴 MISSING |
| **Paywall enforcement** | entitlement/quota endpoints return 402 when denied | Plan choice = local React state; never persisted; no gate | 🔴 COSMETIC |
| Onboarding orchestration | user/org/billing/session cores available | Only `POST …/onboarding/complete` → user-core; gating real | 🟡 PARTIAL |
| User profile / settings | user-core: profile, prefs, settings, api-keys, avatar | 5 navbar/onboarding routes wired; **account/settings page hardcoded** | 🟡 PARTIAL |
| Audit | audit-core: NATS ingest + query API | **None**; settings "audit trail" is hardcoded rows | 🔴 MISSING |

---

## Core coverage matrix (verified)

`.env.example` provisions **only** `AUTH_CORE_URL` + `USER_CORE_URL` + `INTERNAL_API_KEY` (+ Quarry, autocomplete). org/billing/session/audit cores are absent **by design**, not just unconfigured.

| Core | Built backend? | verevonv2 consumes? | Evidence |
|---|---|---|---|
| auth-core | ✅ full (NestJS + Better Auth, :3011) | ✅ YES | `src/app/api/auth/[...all]/route.ts`, `src/lib/auth/control-plane.ts`; `AUTH_CORE_URL` (3 files) |
| user-core | ✅ full (:3012) | 🟡 PARTIAL | `src/lib/integrations/user-core.ts`; navbar/profile, theme, calendar, support, onboarding routes; **account page has 0 fetch calls** |
| org-core | ✅ full (+ BREG, :8080) | 🔴 NO (indirect via user-core session-context only) | `ORG_CORE_URL`=0 refs; `org-core`=0; `/api/org`=0 |
| billing-core | ✅ full (Lago+Stripe, :3013) | 🔴 NO | `BILLING_CORE_URL`=0; `billing-core`=0; settings invoices hardcoded |
| session-core | ✅ full (Go, :sess) | 🔴 NO | `SESSION_CORE_URL`=0; `session-core`=0 |
| audit-core | ✅ full (NATS+query) | 🔴 NO | `AUDIT_CORE_URL`=0; `audit-core`=0; audit rows hardcoded |

---

## Findings by dimension

### 1. Auth & session — 🟡 PARTIAL (real but gapped)
- **Real path:** `VerevonAuthPage.tsx` → `authClient.signIn.email` → `/api/auth/[...all]/route.ts` → `isControlPlaneAuthConfigured()` ? `proxyControlPlaneAuthRequest()` → **auth-core** (Better Auth, Postgres+Redis) : local fallback `auth.handler`. Cookie pass-through + `Set-Cookie` domain rewrite. 2FA (TOTP) and social proxy through too.
- **auth-core capabilities:** emailAndPassword, twoFactor, phoneNumber (Twilio), emailOTP, organization, oidcProvider, multiSession, genericOAuth, haveIBeenPwned, apiKey, bearer, admin, passkey, sso. Plus plane-token JWT issuer (`plane-token.controller.ts`) for cross-plane RS256 tokens.
- **Gaps / risks:**
  - 🔴 **No `middleware.ts` anywhere** — no route-level auth gate; protection is per-handler via `getControlPlaneCurrentUser()`. Missing a call = unauthenticated access.
  - 🟡 **Standalone divergence:** if `AUTH_CORE_URL` unset at runtime, app silently runs its **own** Better Auth (separate DB/secret/cookies) — sessions not valid against auth-core. `.env.example` does set it, so this is a deploy-config risk.
  - 🟡 **Passkey stubbed:** `VerevonAuthPage.tsx:418` returns an error string; passkey plugin not loaded in v2's `auth.ts` (auth-core has it).
  - 🟡 **Client baseURL** uses `NEXT_PUBLIC_AUTH_BASE_URL` (distinct from server `AUTH_CORE_URL`); if unset, client calls break.
  - session-core (durable session lineage) is **not** called by v2.

### 2. Org + BREG — 🔴 MISSING in v2 (backend real)
- **org-core BREG client is real:** `org-core/internal/brreg/client.go` → `https://data.brreg.no/enhetsregisteret/api` — `SearchByName` (`/enheter?navn=`), `LookupByOrgNr` (`/enheter/{orgnr}`), maps orgform/address/næringskode/ansatte/konkurs… Routes: `GET /api/v1/brreg/search`, `GET /api/v1/brreg/:orgnr`, `PATCH /api/v1/organizations/:id/brreg` (stores `brreg_data` JSONB snapshot + `verification_status`).
- **verevon v1 wires it end-to-end:** `brreg-service.ts` + `BrregSearch.tsx` + `OrganizationStep.tsx` → `POST /api/org/orgs {name, org_number, brreg_data}`.
- **verevonv2:** **zero** brreg references; org step is a plain text input; no `ORG_CORE_URL`, no `/api/org` rewrite in `next.config.ts`. Orgs created via v2 (if any) would be unverified with no registry snapshot.

### 3. Billing & paywall — 🔴 COSMETIC in v2 (backend real)
- **billing-core real:** Lago adapter posts usage to `lago:3000/api/v1/events`; Stripe adapter hits `api.stripe.com` for customers/checkout/payment_intents; NATS subscriber syncs `organization.*`/`usage.>`; entitlement API returns **402** when denied; plan-gated defaults (free→enterprise).
- **verevonv2 paywall:** `PaywallStep` (`VerevonOnboardingPage.tsx:666`) stores plan in `useReducer` state; `onContinue` routes to "assembly" — **no API call**. No entitlement check anywhere in v2. No checkout route. `VerevonWorkspaceSettingsPage` billing/seats/invoices are hardcoded. **Selected plan is never transmitted.** → paywall is presentational and bypassable.

### 4. Onboarding — 🟡 PARTIAL
- **Real:** `markCurrentUserOnboardingComplete()` → `POST …/users/onboarding/complete` (user-core, with local-DB fallback). Server-side gating `requireCompletedOnboarding()` / `requireOnboardingAccess()` redirects correctly.
- **Not real:** steps 1–6 (org name/size, website, connectors, plan) are ephemeral local state — no org-core create, no BREG, no billing-core plan persist. Collected data is discarded. Not cross-core orchestration.
- ⚠️ Local-fallback completion can mark onboarding done without user-core ever knowing → user with no org/billing record in Control Plane.

### 5. Integration topology
- Two mechanisms, no central typed client: (a) auth-core via Next catch-all **reverse-proxy** (cookie-transparent); (b) user-core via **server-to-server** `X-Internal-Api-Key` + `X-User-Id`. No NATS in frontend (correct by design). No middleware.
- Env/service-name drift vs `docker-compose.yml` container names (`auth-service`, `user-service`) — resolves only if env values point at compose names; prior "stale container names" note pertained to **verevon v1**, which has its own `.env`; v2 ships only `.env.example`.

---

## What "full control-plane parity" requires (gap-closure checklist)

**P0 — security/correctness**
1. Add `middleware.ts` with session validation + route protection for `(workspace)/*`.
2. Make the paywall enforce **billing-core entitlements** (server check + 402 handling); gate features server-side.
3. Persist onboarding: org step → `POST org-core /orgs`; plan step → billing-core plan/trial; stop dropping wizard data.

**P1 — wire the missing cores**
4. org-core client (+ `ORG_CORE_URL`, `/api/org` proxy) and **port BREG search/lookup** from verevon v1 (`BrregSearch`, `brreg-service.ts`).
5. billing-core client (+ `BILLING_CORE_URL`): entitlement hook, checkout redirect, real billing settings.
6. Replace hardcoded **settings** (profile, members, roles, invoices, audit) with user-core/org-core/billing-core/audit-core reads; wire `PATCH /users/me`.
7. Decide session-core role (durable session lineage) and audit-event emission for sensitive actions.

**P1 — auth hardening**
8. Load passkey plugin in v2 (or remove the button); fix `NEXT_PUBLIC_AUTH_BASE_URL`; fail-closed if `AUTH_CORE_URL` unset in prod to prevent silent standalone mode; align with `better-auth-security-best-practices` (rate-limit, trusted origins, secure cookies).

---

## Confidence & limitations
- **High** on all frontend-absence findings (grep + Read + CodeGraph spot-verified by me: middleware=NONE; org/billing/session/audit/brreg = 0 refs; `.env.example` scope).
- **High** on backend capability claims (agents read Go source: brreg client URL, Lago/Stripe adapters, Better Auth plugins).
- **Not verified at runtime:** committed `.env`/secrets (gitignored), live deployment env injection, whether user-core's `onboarding/complete` internally bootstraps org/billing (gRPC handler not fully read), exact session-core `/v1` route bodies.
