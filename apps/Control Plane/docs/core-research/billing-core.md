# billing-core Research Dive

Generated: 2026-06-07

Scope: `apps/Control Plane/billing-core`

## Snapshot

`billing-core` is the Control Plane billing, quota, and invoice facade. It is a Go service with HTTP as the real business API, health/reflection-only gRPC, local NATS ingestion for usage and organization lifecycle, shared NATS publishing for cross-plane billing events, provider adapters for Lago and Stripe, optional Redis caching, a retry processor, and a trial-expiry sweep.

Current evidence highlights:

- HTTP is the true business surface
- gRPC exists only for health/reflection
- local and shared NATS are both used
- retry and trial background loops are active
- README is broadly aligned, but some cross-core semantics still overlap with org-core

Non-generated/non-vendored file count from the current tree: about `37`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - internal-key startup gate
  - DB migrations
  - optional pprof
  - adapter construction for Stripe and Lago
  - billing service creation
  - shared NATS publisher hookup
  - retry processor background loop
  - trial-expiry sweep background loop
  - local NATS subscriber startup
  - HTTP and gRPC startup
- `internal/billing/service.go`
  - main billing domain logic
- `internal/http/server.go`
  - HTTP API surface
- `internal/nats/subscriber.go`
  - local event ingestion
- `internal/nats/shared_publisher.go`
  - shared cross-plane billing publication

## API And Relationship Map

Primary HTTP surface:

- `/health`
- `/api/v1/billing/orgs/:orgId/account`
- `/api/v1/billing/orgs/:orgId/usage`
- `/api/v1/billing/orgs/:orgId/entitlements/:feature`
- `/api/v1/billing/orgs/:orgId/quotas/:metric`
- `/api/v1/billing/orgs/:orgId/invoices`
- `/api/v1/billing/orgs/:orgId/checkout-session`

gRPC surface:

- health and reflection only

Current relationships:

- `billing-core` -> Lago adapter
- `billing-core` -> Stripe adapter
- `billing-core` -> local NATS subjects
  - `usage.>`
  - organization lifecycle/plan subjects
- `billing-core` -> shared NATS consumers
  - `aqencia.controlplane.billing.*`
  - plain notification subjects via `PublishPlain`

## Duplicates, Redundancies, And Non-Relationships

Clear duplication:

- `internal/internalkey/assert.go` is duplicated across the four Go cores.

Cross-core semantic overlap:

- `billing-core` and `org-core` both publish plan-change style events, but from different ownership angles.
- That is not automatically wrong, but it is a documentation and event-contract drift hotspot.

Non-relationship / partial relationship:

- gRPC port exists, but business methods are not registered

## Stubs, Placeholders, And Missing Connections

Observed placeholder logic:

- startup internal-key validation treats placeholder values as fatal in production
- provider adapters can run in non-charging/non-sync local mode when keys are absent

Missing or partial surfaces:

- no business gRPC API despite exposed port
- local and shared event paths increase contract coordination cost

No large active TODO cluster was found in the main core source on the first scan.

## API Design And Performance Notes

API design:

- billing HTTP routes are coherent and resource-shaped
- API is internal-key gated and clearly scoped by org
- gRPC presence without business services may mislead other teams

Performance and operational notes:

- retry processor with exponential backoff is a good resilience feature
- dead-letter emission on exhausted retries is explicit
- trial sweep is periodic and bounded
- optional Redis caching is operationally sensible

## Current Doc Cleanup Read

Keep for now:

- `README.md`
  - current enough to remain, though it should eventually mention health-only gRPC if we want docs to be exact

No delete-ready core-local text docs were identified beyond ordinary metadata noise.

## Bottom Line

`billing-core` is one of the cleaner Control Plane cores. The main concerns are:

- health-only gRPC despite an exposed gRPC port
- duplicated internal-key helper
- cross-core plan-change/event semantics that deserve clearer contract ownership
