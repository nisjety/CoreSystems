# org-core Research Dive

Generated: 2026-06-07

Scope: `apps/Control Plane/org-core`

## Snapshot

`org-core` is the organization, membership, RBAC, entitlement, and BRREG authority in the Control Plane. It is a Go service with HTTP as the real business surface, health/reflection-only gRPC, local NATS bridging, shared NATS publishing, optional Redis caching, and a small metrics server.

Current evidence highlights:

- active HTTP API for organizations, members, roles, onboarding, and BRREG
- duplicated compatibility routes under both `/api/v1/...` and `/orgs/...`
- gRPC port exists, but no org business service is registered
- internal-key startup gate is duplicated from the other Go cores

Non-generated/non-vendored file count from the current tree: about `45`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate
  - DB migrations
  - optional pprof
  - repo/service construction
  - local NATS stream + bridge subscriber
  - shared NATS publisher hookup
  - HTTP, gRPC, and metrics startup
- `internal/http/server.go`
  - main business API
- `internal/org/service_enhanced.go`
  - org service logic
- `internal/rbac/repository.go`
  - role/capability persistence
- `internal/nats/subscriber.go`
  - auth/org/user/session event bridging

## API And Relationship Map

Primary HTTP surface:

- `/health`
- `/api/v1/auth/login`
- `/api/v1/users/me`
- `/api/v1/organizations`, `/:id`, `/:id/entitlements`, `/:id/members/search`
- `/api/v1/organizations/:id/plan`
- `/api/v1/organizations/:id/brreg`
- `/api/v1/brreg/search`
- `/api/v1/brreg/:orgnr`

Compatibility routes:

- `/orgs`, `/orgs/me`, `/orgs/:id`, `/orgs/:id/entitlements`
- `/orgs/:id/capabilities`
- `/orgs/:id/members`, invite/remove/search
- `/orgs/:id/roles/catalog`
- `/orgs/:id/roles`
- `/orgs/:id/members/:userId/role`

Internal helper routes:

- `/internal/orgs/by-tenant`
- `/internal/orgs/ensure-from-tenant`
- `/internal/orgs/:orgId/onboarding/state`

gRPC surface:

- health and reflection only

Current relationships:

- `org-core` -> `auth-core`
  - login/current-user compatibility path and auth-event bridge
- `org-core` -> `user-core`
  - configured HTTP relationship and shared membership lifecycle
- `org-core` -> shared NATS consumers
  - cross-plane org events
- `org-core` -> BRREG
  - external organization lookup and verification client

## Duplicates, Redundancies, And Non-Relationships

Clear duplication:

- `internal/internalkey/assert.go` is one of the four duplicated Control Plane copies.
- route duplication exists between `/api/v1/organizations...` and compatibility `/orgs...` paths

Intentional but broad redundancy:

- both REST route families are live to support old and new callers

Non-relationship / partial relationship:

- gRPC server is real infrastructure but does not carry org business methods, only health/reflection

## Stubs, Placeholders, And Missing Connections

Observed placeholders:

- `internal/rbac/repository.go` allows a role with no permissions as a placeholder
- startup internal-key validation explicitly treats placeholder values as misconfiguration

Missing or partial surfaces:

- no business gRPC API despite exposed gRPC port
- compatibility route duplication adds maintenance overhead and doc drift risk

No broad active TODO cluster was found in the core source on the first scan.

## API Design And Performance Notes

API design:

- The API surface is functional but split between canonical `/api/v1/...` and compatibility `/orgs/...` paths.
- That duplication is the largest design cost in this core.
- RBAC and member-management endpoints are reasonably grouped.

Performance and operational notes:

- optional Redis improves read path behavior without becoming a hard dependency
- metrics server is a good operational boundary
- compatibility routes increase handler surface and documentation cost, even if they do not materially hurt runtime performance

## Current Doc Cleanup Read

Keep for now:

- `README.md`
  - somewhat narrow, but not as misleading as the stale auth/user/session docs

Delete-ready:

- `.DS_Store`

## Bottom Line

`org-core` is a stable HTTP-first service. Its main issues are structural rather than functional:

- duplicate route families
- gRPC infrastructure without business methods
- duplicated startup helper code

The documentation risk here is lower than in auth/user/session, so cleanup should stay conservative.
