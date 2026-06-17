# session-core Research Dive

Generated: 2026-06-07

Scope: `apps/Control Plane/session-core`

## Snapshot

`session-core` is no longer the broad orchestration surface described in its older docs. The current Go core is primarily:

- the Control Session aggregator for frontend/BFF consumption
- the legacy session command/event bridge toward Model Plane
- the cache invalidation and republish boundary for entitlements/session snapshots

Current evidence highlights:

- current business HTTP split between legacy `/v1/sessions/*` and Control Session `/api/v1/sessions/*`
- gRPC listener exists but registers no services
- old docs still describe plans/todos/lineage APIs that were removed from this core
- upstream invalidation has a known org-only cache-bust gap and falls back to TTL

Non-generated/non-vendored file count from the current tree: about `46`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate
  - DB migrations
  - local and shared NATS setup
  - optional Redis
  - repository creation
  - legacy session service creation
  - Control Session aggregator service creation
  - Convex/user/org/billing client hookup
  - upstream invalidator subscriber startup
  - HTTP startup
  - gRPC listener startup without service registration
- `internal/service/control_session_service.go`
  - Control Session snapshot assembly and refresh
- `internal/service/session_service.go`
  - legacy session command/event bridge
- `internal/http/server.go`
  - route registration
- `internal/subscribers/upstream_invalidator.go`
  - shared-bus cache invalidation and republish logic

## API And Relationship Map

Legacy session HTTP surface:

- `POST /v1/sessions`
- `GET /v1/sessions/:id/state`
- `GET /v1/sessions/:id/events`
- `POST /v1/sessions/:id/messages`
- `POST /v1/sessions/:id/approvals/:approval_id`
- `POST /v1/sessions/:id/resume`

Current Control Session surface:

- `GET /api/v1/sessions/current`
- `POST /api/v1/sessions/refresh`

gRPC surface:

- listener only, no registered business services

Current relationships:

- `session-core` -> `user-core`
  - Control Session aggregation
- `session-core` -> `org-core`
  - Control Session aggregation and org validation
- `session-core` -> `billing-core`
  - Control Session aggregation
- `session-core` -> Convex
  - snapshot mirroring
- `session-core` -> shared NATS
  - session commands/events
  - `app.session.entitlements_changed`
- `session-core` -> Model Plane
  - versioned legacy/new session command routing

## Duplicates, Redundancies, And Non-Relationships

Clear duplication:

- `internal/internalkey/assert.go` is duplicated across the four Go Control Plane services.

Structural redundancy:

- the core carries both legacy session bridge routes and the newer Control Session aggregator routes
- old documentation still describes removed plans/todos/lineage behavior

Non-relationship / partial relationship:

- `cmd/server/main.go` explicitly says Rust Model Plane session-core now owns plan/todo/lineage/approval state
- gRPC listener is started, but no services are registered

## Stubs, Missing Connections, And Drift

Explicit current drift:

- `scripts/smoke_test_api.sh` still exercises todo endpoints that are no longer part of this core
- old docs claim plans/todos/lineage APIs remain here

Known missing connection:

- `internal/subscribers/upstream_invalidator.go` logs `org-only event, no user index - relying on TTL`
- result: org-only updates cannot immediately invalidate all affected cached user snapshots

## API Design And Performance Notes

API design:

- there are effectively two session APIs in one service: legacy bridge routes and the Control Session aggregator
- this is acceptable as a transition state, but it is documentation-heavy and easy to misread
- health-only gRPC should either be documented clearly or removed later

Performance and operational notes:

- read-through Redis cache is a good fit for the Control Session snapshot
- missing org-to-user reverse index means some cache invalidation falls back to TTL
- explicit `/refresh` is the manual correctness escape hatch

## Current Doc Cleanup Read

Delete-ready:

- `90_PERCENT_COMPLETE.md`
- `API_REFERENCE.md`
- `GAP_ANALYSIS.md`
- `IMPLEMENTATION_SUMMARY.md`

These files all describe the older plans/todos/lineage-heavy session-core role and are now misleading.

Keep for now:

- `scripts/smoke_test_api.sh`
  - stale for some routes, but this is a script cleanup task rather than doc cleanup

## Bottom Line

`session-core` is one of the clearest examples of architecture drift being fixed in code but not in docs. The current service is much narrower than its historical documentation suggests. The main issues are:

- stale status/reference docs
- gRPC infrastructure without registered services
- partial invalidation for org-only events
- residual legacy route surface during transition
