# velionv2 Auth + Onboarding → Control Plane: Implementation Status

**Branch:** `feat/velionv2-cp-parity` · **Date:** 2026-05-30
**Plan:** `AUTH_ONBOARDING_PARITY_PLAN.md` · **Audit:** `CONTROL_PLANE_PARITY_AUDIT.md`
**Stack:** Next.js 16.2.6 (App Router, `proxy.ts`, async `cookies/headers/params`) · React 19 · Better Auth 1.6.11
**Verification:** `tsc --noEmit` = 0 errors · `vitest` = 120/120 pass (66 new) · `next build` = success.

---

## What shipped, by phase

### P0 — Auth-mode hardening ✅
- `src/app/api/auth/[...all]/route.ts` — **fail-closed in production**: if `AUTH_CORE_URL`/`CONTROL_PLANE_AUTH_URL` is unset, returns 503 instead of silently using standalone Better Auth (standalone is dev-only now).
- `src/lib/auth/auth-client.ts` — client `baseURL` falls back `NEXT_PUBLIC_AUTH_BASE_URL → NEXT_PUBLIC_APP_URL → window.origin` (correct for the same-origin proxy).
- `.env.example` — added `NEXT_PUBLIC_AUTH_BASE_URL`, `ORG_SERVICE_URL`, `BILLING_SERVICE_URL`, optional `SESSION_SERVICE_URL`/`CONTROL_SESSION_AUTHORITY_ENABLED`, `BETTER_AUTH_TRUSTED_ORIGINS`, `RATE_LIMIT_ENABLED`.

### P1 — Control Plane gateway + context backbone ✅
- `src/app/api/_lib/control-plane-auth.ts` (NEW) — `requireSession` (auth-core `/api/auth/get-session`), `buildControlPlaneHeaders` (X-Internal-Api-Key / X-User-* / X-Correlation-Id), env precedence aligned to v2.
- `src/app/api/org/[...path]/route.ts` (NEW) — unified **org-core + billing-core gateway** (ported from v1): smart fan-out for `billing|quota|plan|checkout`, BRREG passthrough, response normalization. Session-guarded.
- `src/lib/services/brreg-service.ts` (NEW) — BRREG client (`searchByName`, `lookupByOrgNr` with `AbortSignal`), `slugify`, `sizeFromEmployeeCount`, `formatBrregAddress`.
- `src/lib/integrations/billing-core.ts` (NEW) — server billing client: `fetchBillingAccount`, `checkEntitlement` (402 → false).
- `src/lib/control-plane/context-types.ts` (NEW) — shared `ControlPlaneContextValue`/`ControlPlaneEntitlements`, `hasFeature`.
- `src/lib/auth/control-plane-context.ts` (NEW) — `getControlPlaneContext()` composes auth-core identity + user-core session-context + billing-core entitlements; React `cache()` per request; never blocks on billing outage.
- `src/features/shell-v2/lib/control-plane-provider.tsx` (NEW) — `ControlPlaneProvider` + `useControlPlaneContext` / `useEntitlements` / `useHasFeature` / `useOrgId`.

### P2 — Onboarding wired end-to-end ✅
- `src/features/onboarding-v2/components/BrregSearch.tsx` (NEW) — debounced BRREG search (AbortController-cancelled), ported from v1.
- `src/features/onboarding-v2/lib/onboarding-service.ts` (NEW) — `createOrganization` (+ BREG snapshot), `setOrganizationPlan`, `startCheckout`, `updateProfile`, `completeOnboarding`, `isPaidPlan`.
- `src/app/api/v1/users/me/route.ts` (NEW) — GET/PATCH profile proxy to user-core (same-origin guarded).
- `VelionOnboardingPage.tsx` (WIRED) — org step does BREG lookup → `createOrganization` (idempotent on `orgId`); website step → `updateProfile`; paywall → free/trial `setOrganizationPlan` or paid `startCheckout` → Stripe redirect; assembly → `completeOnboarding` (guards on `orgId`). Org-create publishes `organization.created` → billing-core auto-provisions free (NATS).
- Review fixes applied: stale-closure on "skip to trial" (CRITICAL), assembly `orgId` guard, no plan clobber, BRREG fetch abort.

### P3 — Gating + paywall enforcement ✅
- `src/proxy.ts` (NEW) — Next 16 middleware (renamed): coarse auth-cookie gate for workspace routes (fail-open here / fail-closed in RSC).
- `src/app/(workspace)/layout.tsx` (WIRED) — auth-presence gate + mounts `ControlPlaneProvider` (completeness still per-page, since `/onboarding` lives here).
- `src/lib/auth/entitlements.ts` (NEW) — `requireEntitlement(feature)`, `hasEntitlement`, `getEntitlements`, `getQuotaLimit` — server-side paywall on **real billing-core entitlements**.
- Settings billing/members now read LIVE data (`useEntitlements` + `GET /api/org/orgs/:id/members`); invoices/audit show honest empty states (no endpoints yet). Section metadata extracted to server-safe `settings-v2/lib/settings-sections.ts` to fix the client/server boundary. Security review of the gateway/session/paywall applied (path-segment validation, upstream-error sanitization, callbackUrl open-redirect guard, correlation-id sanitize, upstream timeouts).

### P4 — Hardening + tests ✅ (passkey deferred)
- Hardening already present + verified: Better Auth `rateLimit`, `trustedOrigins`, secure cookies in prod, `requireEmailVerification`, secret validation (throws in prod). 2FA (TOTP + backup codes) configured; social (Google/Microsoft) via auth-core in CP mode.
- Tests: `brreg-service.test.ts`, `onboarding-service.test.ts`, `context-types.test.ts`, `billing-core.test.ts` (66 new, all green).

### P5 — Production verification + full-core completion ✅
- **audit-core wired (last unconsumed core)** → velionv2 now consumes **all 6 cores**. `src/lib/integrations/audit-core.ts` (NEW, server client, `/v1/audit`), `src/app/api/v1/audit/route.ts` (NEW — org derived from session-context, not client input), and the settings "Recent security events" panel now renders live audit-core events. `AUDIT_SERVICE_URL` added to `.env.example`.
- **E2E** — `tests/e2e/cp-parity.spec.ts` (NEW): robust proxy-gating tests (run as-is) + a mock-driven onboarding journey (`test.fixme`, validate selectors then enable). Follows the repo's route-mock pattern (`smoke.spec.ts`), runs against `pnpm dev`.
- **Cutover/verification doc** — `PRODUCTION_CUTOVER.md` (NEW): env matrix, docker-compose service-name alignment, pre-flight assertions, and the live-stack smoke checklist.
- Verified green again after all P5 changes: tsc 0 · 120 unit tests · `next build` ok.

Core coverage now: auth ✅ user ✅ org+BREG ✅ billing ✅ audit ✅ · session-core optional (D4, flag-gated).

### P6 — Passkey + session-core authority + audit emission ✅ (post-plan, by request)
- **Passkey (WebAuthn)**: installed `@better-auth/passkey@1.6.12` + `@simplewebauthn/browser`+`/server` (bumped `better-auth` 1.6.11→1.6.12 for peer alignment). `passkeyClient()` in `auth-client.ts`; `passkey()` in standalone `auth.ts` (dev parity); `beginPasskeySignIn` calls `authClient.signIn.passkey()`. auth-core already ships the `passkey()` server plugin. `PASSKEY_RP_ID/RP_NAME/ORIGIN` in `.env.example`. ⚠️ **Version-skew caveat**: velionv2 client is better-auth 1.6.12, auth-core server is 1.3.9 — WebAuthn endpoints are stable but **verify the ceremony against the live stack**. Passkey REGISTRATION UI **added**: `PasskeySecuritySection` (register/list/remove) in the account Security section (`VelionSettingsPage`) + a CDP virtual-authenticator E2E (`tests/e2e/passkey.spec.ts`).
- **session-core authority**: `src/lib/integrations/session-core.ts` (`current`/`refresh` + entitlements mapping). `getControlPlaneContext()` uses session-core's single aggregate call when `CONTROL_SESSION_AUTHORITY_ENABLED=true`, else falls back to user-core+billing. session-core's billing shape lacks quotas/credits → default to empty under this path.
- **audit-core emission (write side)**: added a Go HTTP ingest `POST /v1/audit` to audit-core (internal-key auth, reuses the NATS validate+persist path; go build/vet/test green). velionv2 now EMITS `org.created` + `org.plan.changed` via `emitAuditEvent` in the `/api/org` gateway (fire-and-forget). With the P5 read panel, the audit loop (emit → ingest → read) is complete for velionv2-driven org/plan events. **auth-core now also emits `velion.audit.v1.control.*` over NATS**: 2FA (enable/disable/verify) + session-revoke fire when an active org is set; `sign_out` fires for org-scoped sessions; `sign_in`/`sign_up` no-op pre-org (Better Auth only sets `activeOrganizationId` after org selection, and audit-core requires `org_id`). **better-auth version alignment ASSESSED but NOT applied**: `@better-auth/sso@1.3.9` exact-pins `better-auth@1.3.9` (no 1.4–1.6 release) → bump unresolvable; migration path = replace `@better-auth/sso` with the native OIDC provider plugin, then bump auth-core to 1.6.x.
- Verified: velionv2 tsc 0 · 120 tests · `next build` ok; audit-core `go build`/`vet`/`test` ok.

---

## Required env to run against the Control Plane
`AUTH_CORE_URL`, `USER_CORE_URL`, `ORG_SERVICE_URL`, `BILLING_SERVICE_URL`, `INTERNAL_API_KEY` (must match Control Plane), `NEXT_PUBLIC_AUTH_BASE_URL`, `NEXT_PUBLIC_APP_URL`, `BETTER_AUTH_TRUSTED_ORIGINS`. See `.env.example`.

## Remaining / follow-ups (honest)
1. **Passkey** — DEFERRED: the passkey plugin is **not in Better Auth 1.6.11** as installed (no `passkeyClient` export, no server plugin). Wiring it requires adding the passkey package + WebAuthn config (rpId/origin) in auth-core. The sign-in button shows an accurate "not yet enabled" message. Needs a dependency decision.
2. **Audit-core** — ✅ DONE in P5 (`/api/v1/audit` route + `audit-core.ts` client + live settings "Recent security events" panel).
3. **E2E** — scaffolded in P5 (`tests/e2e/cp-parity.spec.ts`): proxy-gating tests run; the mocked onboarding journey is `test.fixme` pending selector validation against the running UI.
4. **session-core** (D4) — deferred; auth-core session is authoritative. Enable via `CONTROL_SESSION_AUTHORITY_ENABLED` later if durable session lineage is required.
5. Standalone `auth.ts` 2FA `otpOptions.period: 3` looks low (dev-only path) — review.
