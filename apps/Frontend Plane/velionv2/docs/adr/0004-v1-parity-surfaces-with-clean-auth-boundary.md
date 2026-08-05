# ADR 0004: V1-Parity Surfaces With Clean Auth Boundary

## Status

Accepted.

## Context

Verevon v2 must preserve the strongest Verevon v1 user-facing surfaces while avoiding the v1 page-level coupling between UI, auth runtime, onboarding state, dashboard chrome, and chat providers.

The first parity scope is auth, onboarding, the dashboard heart, and the AI chat input page.

## Decision

We keep v1 UI decisions as product references, not source architecture. The v2 implementation is split by feature:

- `src/features/auth` owns auth UI and form validation.
- `src/lib/auth` owns Better Auth server/client config, 2FA plugin setup, env handling, and route integration.
- `src/features/onboarding-v2` owns the onboarding copy, step model, and two-pane wizard.
- `src/features/shell-v2` owns dashboard shell navigation.
- `src/features/dashboard-v2` owns the dashboard heart.
- `src/features/chat-v2` owns the ChatGPT/Manus-inspired chat and reusable composer.

Better Auth is mounted at `/api/auth/[...all]`. The UI submits through the Better Auth client and the server runtime requires `DATABASE_URL` before accepting real sessions. There is no simulated session route.

## Consequences

The v2 UI can match the v1 experience without importing its page-level state, providers, or dirty coupling. Auth runtime concerns are isolated and documented, and the route structure is stable for adding real persistence, OAuth providers, passkeys, and 2FA enrollment.
