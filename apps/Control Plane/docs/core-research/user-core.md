# user-core Research Dive

Generated: 2026-06-07

Scope: `apps/Control Plane/user-core`

## Snapshot

`user-core` is the Control Plane user-profile and settings authority. It is a Go service with a broad HTTP API, a real business gRPC surface, NATS event ingestion from auth/org flows, optional Redis caching, and optional Graph enrichment support.

Current evidence highlights:

- broad HTTP API for profile, onboarding, settings, provider links, calendar/navbar state, support requests, and internal helper endpoints
- real gRPC `UserService` and `DocumentAccessService`
- startup dependency on auth-service NATS authentication and Better Auth client setup
- shared NATS publishing exists, but document ACL gRPC wiring does not use it
- old implementation and README docs are materially stale

Non-generated/non-vendored file count from the current tree: about `58`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate
  - DB migrations
  - optional pprof
  - NATS auth bootstrap with retries
  - Better Auth client init
  - optional Redis
  - local NATS and shared NATS init
  - optional Microsoft Graph enrichment wiring
- `internal/http/server.go`
  - primary business API
- `internal/grpc/server.go`
  - registers real `UserService` and `DocumentAccessService`
- `internal/handlers/event_handler.go`
  - consumes auth and org membership events
- `internal/users/service.go`
  - main domain service

## API And Relationship Map

HTTP surface includes:

- `/health`
- `/api/v1/users/me`, `/current`, `/:id`, `/by-email/:email`
- `/api/v1/users/onboarding/complete`
- `/api/v1/users/me/onboarding-state`
- `/api/v1/me/session-context`
- `/api/v1/api-keys`
- `/api/v1/preferences`
- `/api/v1/settings/{appearance,language,privacy,notifications,security,accessibility,ai,storage}`
- `/api/v1/calendar/events`
- `/api/v1/calendar/notes`
- `/api/v1/support/requests`
- `/api/v1/providers`
- `/api/v1/internal/memberships/ensure`
- `/api/v1/internal/users/enrich-from-provider`

gRPC surface:

- `UserService`
- `DocumentAccessService`

Current relationships:

- `user-core` -> `auth-core`
  - Better Auth client
  - NATS auth bootstrap
  - auth event ingestion
  - optional auth-core OAuth client for enrichment
- `user-core` -> Microsoft Graph
  - optional provider profile enrichment
- `user-core` -> `org-core`
  - membership and org-related event projection
- `user-core` -> shared cross-plane consumers
  - shared NATS publication for user/provider readiness paths

## Duplicates, Redundancies, And Non-Relationships

Clear duplication:

- `internal/internalkey/assert.go` is a canonical copy duplicated across four Go Control Plane services.
- placeholder-avatar detection logic appears both in `internal/users/service.go` and `internal/http/handlers.go`, which is redundant logic drift risk.

Broad but intentional API redundancy:

- `/api/v1/users/me` and `/api/v1/users/current` are aliases for the same current-user profile flow.

Missing or broken relationship:

- `internal/grpc/server.go` constructs `NewDocumentAclHandler(aclRepo, s.publisher, nil)`.
- `internal/handlers/document_acl.go` only publishes shared cross-plane ACL changes when `sharedPublisher != nil`.
- Result: document ACL gRPC changes do not propagate shared-bus ACL-change events through this registration path.

## Stubs, Placeholders, And TODOs

Active TODOs:

- `internal/users/service.go`
  - TODO to log block reason in activities
  - TODO to log suspension reason in activities
  - TODO to handle suspension expiry
- `internal/http/handlers.go`
  - TODO to add admin-role check on `GET /api/v1/users/:id`

Intentional unimplemented gRPC areas:

- `CreateSession` is intentionally not implemented here; sessions are owned by auth-core/session-core
- device-management gRPC methods remain unimplemented

Development placeholder behavior:

- placeholder emails such as `@placeholder.local`
- placeholder avatar detection and replacement logic

Generated proto `Unimplemented` stubs are expected and are not themselves runtime findings.

## API Design And Performance Notes

API design:

- The HTTP API is broad and practical, but the settings surface is highly fragmented into many small endpoints.
- The current design favors explicitness over batching. That is acceptable for internal APIs, but it increases client round-trips.
- The current `/users/:id` admin-style route needs explicit authorization hardening.

Performance and operational notes:

- auth bootstrap retries can delay startup materially when auth/NATS are degraded
- optional Redis degradation is graceful
- pprof support is useful and already guarded by env
- Graph enrichment is correctly optional rather than hard-failing the core

## Current Doc Cleanup Read

Delete-ready:

- `IMPLEMENTATION.md`
  - claims 60 percent completion and several pending areas that are now live or differently owned
- `README.md`
  - old project structure, stale feature claims, and outdated path/module narrative

Keep:

- no other core-local docs require action in the current tree

## Bottom Line

`user-core` is a real, production-shaped service with broad HTTP and real gRPC business behavior. The main issues are:

- stale documentation
- one concrete missing shared-bus connection on document ACL changes
- duplicated helper logic
- a few clear TODOs around authorization and activity completeness
