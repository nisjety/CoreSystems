# Velion v3 Onboarding Gateway Integration

> Verified 2026-07-11 (source-only; SPA :5173 + gateway :3185 both down this pass, no live curl). Endpoint list, proxy config, persistence, and IDOR strip re-checked against current source. One prior claim (`finishOnboarding` error-swallowing) is now corrected below.

## Current State

Onboarding is the strongest live integration in Velion v3.

The onboarding calls (`src/features/onboarding/lib/api/*`, barrel-exported via `lib/api.ts`) go through `requestJson` (`src/shared/api/http.ts`), which resolves the gateway base URL via `gatewayBaseUrl()` in `src/shared/api/config.ts`:

1. same-origin default (empty base) so the browser only talks to its own origin and the Vite/nginx proxy forwards `/api` to the gateway (keeps the Better Auth session cookie first-party)
2. optional absolute `VITE_VELION_GATEWAY_URL` override

In development, `vite.config.ts` proxies `/api` and `/health` to `GATEWAY_PROXY_TARGET`, `VITE_VELION_GATEWAY_URL`, or `http://127.0.0.1:3185`. That target is the Velion v3 Rust gateway under `apps/gateway`.

## Covered Gateway Calls

Velion v3 calls:

- `GET /api/v1/session/bootstrap`
- `GET /api/v1/onboarding/status`
- `GET /api/v1/onboarding/brreg/search`
- `POST /api/v1/onboarding/actions/create-organization`
- `POST /api/v1/onboarding/actions/start-website-ingest`
- `POST /api/v1/onboarding/actions/start-connect-session`
- `GET /api/v1/onboarding/graph-preview`
- `POST /api/v1/onboarding/recommend-plan`
- `POST /api/v1/onboarding/actions/set-plan`
- `POST /api/v1/onboarding/actions/start-checkout`
- `GET /api/v1/onboarding/state`
- `PUT /api/v1/onboarding/state`
- `PUT /api/v1/onboarding/theme`
- `POST /api/v1/onboarding/complete`
- `POST /api/v1/onboarding/actions/discover-source`
- `POST /api/v1/onboarding/actions/cleanup-source`
- `POST /api/v1/onboarding/actions/warm-sharepoint-discovery`
- `POST /api/v1/onboarding/actions/start-integration-sync`
- `POST /api/v1/onboarding/actions/confirm-checkout`
- `POST /api/v1/onboarding/translate-recommendation`
- `GET /api/v1/onboarding/lifecycle`
- `GET /api/v1/shipping/carriers`
- `POST /api/v1/onboarding/crawl-preview` as SSE

## Relationships

- Control Plane, Ingestion Plane, Data Plane, Model Plane, and Application Plane are reached indirectly through the Velion v3 Rust gateway.
- Onboarding state is persisted both locally and remotely.
- Local persistence uses `localStorage` key `velionv3.onboarding.state.v1`.
- Actor fallback uses `localStorage` key `velionv3.onboarding.actor`.

## Stub, Mock, Placeholder, and Partial Audit

- Dev actor headers are enabled whenever `import.meta.env.DEV` is true or `VITE_ALLOW_DEV_ACTOR_HEADERS=true`.
- The default actor is `velion-v3-local-user` with `local@velion.dev`.
- Remote save failures are swallowed in `createOnboardingPersistence()` (debounced `saveOnboardingState().catch(() => undefined)`, `lib/persistence.ts`).
- `finishOnboarding()` (`components/OnboardingPage.tsx`) SURFACES completion failures rather than swallowing them: on a thrown error or a `{ completed: false }` result it sets `assemblyError` and clears the finalizing flag. It only removes the local state key and SPA-navigates to `/dashboard` on success. (Corrected 2026-07-11 — the earlier "swallows completion failures before clearing local state and redirecting" claim no longer holds.)

## Tenant Scoping (IDOR re-verification)

The historical `x-velion-org-id` cross-tenant IDOR remains fixed. `x-velion-org-id` (case-insensitively, so `X-Velion-Org-Id` too) is in the gateway's `STRIPPED_HEADERS` and dropped globally at ingress by `strip_inbound_identity_headers` (`apps/gateway/src/middleware.rs`). The org is derived server-side from the validated Better Auth session (`active_org_id` ← `activeOrganizationId`), never from a client header. Regression tests: `middleware.rs` (strip assertions) and `src/main.rs` (a forged `x-velion-org-id` is never forwarded as `x-org-id`).

## Notes

The dependency on the Rust gateway is important because Application Plane research still contains older `velion-gateway-rs` notes. For Velion v3, the current source path and proxy setup make this a Frontend Plane gateway unless a deployment decision says otherwise.
