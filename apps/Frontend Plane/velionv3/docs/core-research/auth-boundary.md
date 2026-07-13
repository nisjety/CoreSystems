# Velion v3 Auth Boundary

> Verified 2026-07-11 (source-only; SPA :5173 + velion-gateway-rs :3185 containers down this pass).
> Fully rewritten: the prior "presentation-only, no auth-core call, demo credentials"
> description was obsolete. Auth is now wired end-to-end through the BFF gateway to
> Control Plane auth-core (Better Auth). Every claim below is grounded in current source.

## Current State

Velion v3 authenticates for real. There is no local mock and no direct-to-onboarding shortcut.

- `src/features/auth/components/AuthPage.tsx` `completeAuth()` calls `signUp` / `signIn`
  from `src/shared/api/auth-client.ts`, then handles the full flow: 2FA redirect
  (`verifyTwoFactor`), email/phone OTP verification, password reset, and enterprise SSO.
  On a successful sign-in it calls `setSessionUser` + `loadSession`, then routes by
  onboarding status: `onboardingStatus === 'COMPLETED'` → `/dashboard`, else `/onboarding`.
- `src/shared/api/auth-client.ts` is a typed fetch client hitting the gateway's
  `/api/v1/auth/*`, `/api/v1/session/current`, `/api/v1/me`, and `/api/v1/me/session-context`
  endpoints (no `better-auth/client` package in the SPA — the gateway is the Better Auth
  integration point).
- `src/shared/session/session-store.ts` `loadSession()` probes the nullable
  `/api/v1/auth/session` first (200 + null when logged out), then fetches the rich
  `/api/v1/session/current` snapshot (user + org + permissions + onboardingStatus).

## Route Gating (session validation before workspace routes)

`src/app/App.tsx` wraps every workspace route under `/` in `RequireAuth`, which redirects
`unauthenticated` sessions to `/login` and unfinished-onboarding users to `/onboarding`,
and only renders children when `status === 'authenticated' && onboardingStatus === 'COMPLETED'`.
`RequireOnboarding` gates `/onboarding`; `RequireWorkspaceAdmin` additionally gates
`/settings*` via `hasWorkspaceAdminAccess(session)`. `/login`, `/auth`, and `/reset-password`
render `AuthPage` unguarded.

## Gateway Enforcement (`apps/gateway/src`)

- `middleware.rs::require_session` validates the Better Auth session cookie via auth-core and
  injects an `AuthenticatedUser` (id, email, role, `active_org_id`) into request extensions.
  A real validated session is always authoritative over the dev-auth bypass.
- `middleware.rs::strip_inbound_identity_headers` runs globally before routing and removes
  every `STRIPPED_HEADERS` entry — including `x-org-id` and `x-velion-org-id` — so a
  browser-forged identity/tenant header never reaches a handler. Matching is case-insensitive,
  so `X-Velion-Org-Id` is dropped too.
- Org scoping is derived server-side from the validated session (`active_org_id`, with a
  user-core session-context fallback), never from a client header. This closes the
  cross-tenant `x-velion-org-id` IDOR found in the AI-First audit; it is covered by tests in
  `middleware.rs` and `main.rs` ("a client-forged x-velion-org-id must never be forwarded as
  x-org-id").
- `domains/auth.rs` exposes public routes (`sign-up`, `sign-in`, `2fa/verify`, `sign-out`,
  `session`, email/phone verification, password reset, `oauth/:provider`, `sso/initiate`) and
  protected routes (`session/current`, `me`, `me/session-context`, 2FA enrollment,
  `admin/users`) proxying to auth-core.

## Providers & Passkey

- `src/features/auth/lib/model.tsx` `SOCIAL_PROVIDERS`: Microsoft and Google are `active`
  (OAuth redirect flow via `handleSocialSignIn` → `/api/v1/auth/oauth/:provider`, first-party
  callback on the SPA origin). Apple, Okta, and Vipps are `active: false` (shown disabled).
- Passkey is rendered but disabled: `AuthPage` passes `passkeyEnabled={false}`, and
  `AuthFormPanel` sets `disabled={!props.passkeyEnabled}` so the click never fires
  `onCompleteAuth`. WebAuthn is not wired yet — the only accurate remaining "not wired" item.

## Dev Bypass (local only)

`src/shared/api/http.ts` / `sse.ts` attach `Authorization: Bearer dev-bypass` only when
`VITE_ALLOW_DEV_AUTH_BYPASS === 'true'`. The gateway treats a real session as authoritative
over this bypass, preventing distinct tenants from collapsing onto the shared dev identity.

## Residual Gaps

- Passkey / WebAuthn is display-only (button disabled).
- Apple / Okta / Vipps providers are inactive placeholders (no backend provider wired).
- Live re-verification of the running SPA + gateway was not possible this pass (containers down).
