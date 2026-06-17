# Velion v3 Auth Boundary

## Current State

Auth in Velion v3 is presentation-only today.

`src/features/auth/components/AuthPage.tsx` renders a polished login/register screen, but submission calls `completeAuth()`, which navigates directly to `/onboarding`.

`AuthFormPanel` also routes social provider clicks and passkey clicks through `onCompleteAuth()`.

## Relationships

- There is no visible call to `auth-core`.
- There is no Better Auth client in Velion v3.
- There is no session validation before entering workspace routes.
- Onboarding can use dev actor headers through its gateway client, but that is not equivalent to Control Plane authentication.

## Stub, Mock, Placeholder, and Partial Audit

- Email and password values are initialized to demo values.
- Microsoft and Google social providers are marked active in UI data, but clicking them just navigates to onboarding.
- Passkey is displayed as an option, but it does not call WebAuthn or Control Plane.
- Okta and Vipps are displayed as inactive provider options.

## Risk

Velion v3 cannot be treated as an authenticated production frontend until this boundary is wired to Control Plane auth/session contracts or delegated through a trusted BFF.
