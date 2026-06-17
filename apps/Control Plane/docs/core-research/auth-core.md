# auth-core Research Dive

Generated: 2026-06-07

Scope: `apps/Control Plane/auth-core`

## Snapshot

`auth-core` is the broadest Control Plane core. It is a NestJS + Better Auth service that owns auth flows, session issuance, plane-token issuance, OAuth/OIDC surfaces, auth event publication, NATS request-reply, and gRPC token validation.

Current evidence highlights:

- dual API layers: Better Auth native under `/api/auth/*` and enhanced oRPC-style routes under `/api/v2/auth/*`
- local NATS and shared NATS publishing
- gRPC registered for `auth.v1` and `dataplane.auth.v1`
- active mock fallbacks for email and SMS providers
- inactive source residue still present as `.unused` and `.backup`
- several old docs that no longer reflect the runtime

Non-generated/non-vendored file count from the current tree: about `113`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.ts`
  - Nest bootstrap
  - NATS microservice connection
  - gRPC microservice connection with reflection
  - Better Auth manual catch-all registration
  - Swagger setup
  - global CORS and security headers
- `src/app.module.ts`
  - wires `ORPCModule`, `AuthModule`, `NatsModule`, `InternalServicesModule`, email/docs, and the custom controllers
- `src/auth/auth.ts`
  - Better Auth configuration and provider/plugin wiring
- `src/auth/orpc-router.ts`
  - main enhanced auth surface
- `src/internal/auth-event.publisher.ts`
  - local and shared event fan-out

Main controllers/modules:

- `UsersController`
- `NatsAuthController`
- `ConvexAuthController`
- `ModelPlaneTokenController`
- `PlaneTokenController`
- `AuthGrpcController`
- `ORPCModule`
- `InternalServicesModule`

## API And Relationship Map

Primary surfaces:

- Better Auth native routes under `/api/auth/*`
- enhanced auth/oRPC routes under `/api/v2/auth/*`
- plane token routes under `/api/:audience/token` and `/api/model-plane/*`
- gRPC packages `auth.v1` and `dataplane.auth.v1`
- NATS request-reply subjects for auth/session validation

Current relationships:

- `auth-core` -> `user-core`
  - internal user-service clients
  - auth/user/session/provider events
- `auth-core` -> `org-core`
  - organization event middleware and org membership/create flows
- `auth-core` -> `audit-core`
  - shared audit publication on `velion.audit.v1.control.*` when `org_id` exists
- `auth-core` -> cross-plane consumers
  - shared subjects on `aqencia.controlplane.*`
- `auth-core` -> Frontend/Application Plane
  - Better Auth session surface, Convex auth bridge, plane token minting

## Duplicates, Redundancies, And Inactive Surfaces

Clear duplicate or redundant patterns:

- `src/auth/model-plane-token.controller.ts` explicitly duplicates token-minting logic rather than sharing with `convex-auth.controller.ts`.
- `src/auth/plane-token.controller.ts` and `src/auth/model-plane-token.controller.ts` overlap in purpose and should be treated as a duplication hotspot during later refactor.
- `src/auth/orpc-router.ts` is a large monolithic router with `110` indexed symbols; it is functionally central but structurally dense.

Inactive source residue:

- `src/auth/orpc-router.ts.backup`
- `src/orpc/consolidated-auth.controller.ts.unused`
- `src/orpc/unified-auth.controller.ts.unused`

Non-source noise:

- `.DS_Store`
- `src/.DS_Store`

These files are not part of the active runtime and only increase confusion.

## Stubs, Mocks, Placeholders, And Missing Connections

Active fallback or placeholder behavior:

- `src/auth/auth.ts`
  - mock Resend sender when `RESEND_API_KEY` is missing
  - mock Twilio Verify service when Twilio init fails
- `src/email/resend.service.ts`
  - development mock email logging
- `src/sms/twilio-verify.service.ts`
  - development mock SMS and successful verification fallback
- `src/internal/contracts/user-service.contract.ts`
  - placeholder-only export
- `src/auth/orpc-router.ts`
  - consent persistence TODOs
  - placeholder 2FA/passkey/OTP fallback logic
  - development mock OAuth URLs
  - HIBP TODO
  - placeholder OIDC client operations
  - placeholder API-key operations
  - bearer-token listing not implemented
  - admin/org-count placeholder paths

Missing or partial relationships:

- audit publication intentionally skips pre-onboarding users without `org_id`
- some advanced auth/admin surfaces expose live routes before the backend persistence or provider integration is complete

## API Design And Performance Notes

API design:

- Dual-route separation between `/api/auth/*` and `/api/v2/auth/*` is a sound boundary.
- The enhanced auth surface is much broader than the stable implementation behind it; several routes present as first-class API while still relying on placeholder fallback logic.
- Multiple token-minting controllers suggest the public contract evolved faster than the internal abstraction layer.

Performance and operational notes:

- startup constructs CORS allowlists and manual Better Auth routing once, which is fine
- `orpc-router.ts` is large enough to be a future maintainability and test-surface risk
- mock email/SMS fallback is operationally risky if production config ever drifts into dev behavior

## Current Doc Cleanup Read

Keep:

- `docs/DNS_CONFIGURATION.md`
- `docs/DNS_MIGRATION_PLAN.md`
- `docs/EMAIL_CONFIGURATION.md`

Delete-ready:

- `docs/auth-plan.md`
  - historical implementation plan, not current runtime truth
- `docs/api.md`
  - outdated route and behavior documentation
- `docs/SPRINT_4_TEST_REPORT.md`
  - point-in-time status report, not durable source of truth
- `.DS_Store`
- `src/.DS_Store`

Delete-ready inactive source residue:

- `src/auth/orpc-router.ts.backup`
- `src/orpc/consolidated-auth.controller.ts.unused`
- `src/orpc/unified-auth.controller.ts.unused`

## Bottom Line

`auth-core` is real, central, and heavily wired, but it is also the noisiest Control Plane core. The key problems are not uncertainty about ownership. They are:

- large monolithic enhanced-auth routing
- duplicated token-route logic
- active fallback behavior in sensitive auth paths
- inactive source residue
- old plan/report/api docs that no longer describe the runtime honestly
