# Velion v3 Onboarding Gateway Integration

## Current State

Onboarding is the strongest live integration in Velion v3.

`src/features/onboarding/lib/api.ts` resolves the gateway URL from:

1. same-origin default through `src/shared/api/config.ts`
2. optional `VITE_VELION_GATEWAY_URL`

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
- `POST /api/v1/onboarding/crawl-preview` as SSE

## Relationships

- Control Plane, Ingestion Plane, Data Plane, Model Plane, and Application Plane are reached indirectly through the Velion v3 Rust gateway.
- Onboarding state is persisted both locally and remotely.
- Local persistence uses `localStorage` key `velionv3.onboarding.state.v1`.
- Actor fallback uses `localStorage` key `velionv3.onboarding.actor`.

## Stub, Mock, Placeholder, and Partial Audit

- Dev actor headers are enabled whenever `import.meta.env.DEV` is true or `VITE_ALLOW_DEV_ACTOR_HEADERS=true`.
- The default actor is `velion-v3-local-user` with `local@velion.dev`.
- Remote save failures are swallowed in `createOnboardingPersistence()`.
- `finishOnboarding()` swallows completion failures before clearing local state and redirecting to `/dashboard`.

## Notes

The dependency on the Rust gateway is important because Application Plane research still contains older `velion-gateway-rs` notes. For Velion v3, the current source path and proxy setup make this a Frontend Plane gateway unless a deployment decision says otherwise.
