# data-orchestrator-go Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/data-orchestrator-go`

## Snapshot

`data-orchestrator-go` is the maintenance and rebuild controller inside Data Plane v2. It owns internal job submission, reindex/rebuild orchestration, stale embedding detection, and cost-ledger consumption.

Current evidence highlights:

- Go HTTP operator service
- Postgres and NATS backed
- not a product-facing user surface
- cost consumer is optional and non-fatal on startup

Non-generated, non-vendored file count from the current tree: about `12`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - Postgres, NATS, executor, stale detector, cost consumer, HTTP server
- `internal/jobs/*`
  - rebuild and orchestration execution
- `internal/handler/*`
  - internal job routes
- `internal/cost/*`
  - ledger consumer

Primary surface:

- `/v1/orchestrator/jobs`
- `/v1/orchestrator/reindex`
- `/v1/orchestrator/stale-embeddings`

## API And Relationship Map

Current relationships:

- operators or internal automation -> `data-orchestrator-go`
- `data-orchestrator-go` -> Postgres
  - job state and stale detection
- `data-orchestrator-go` -> NATS
  - orchestration and cost-ledger behavior

## Duplicates, Redundancies, And Inactive Surfaces

No explicit duplicate source residue was found in this pass.

This service is intentionally separated from app-facing retrieval and indexing APIs, which is the correct boundary.

## Stubs, Placeholders, And Missing Connections

No explicit code stubs or backup files were found in the active tree.

Partial relationship:

- cost ledger consumer startup failure is logged and tolerated
- that is pragmatic, but it means some observability and accounting paths can disappear quietly in degraded environments

## API Design And Performance Notes

API design:

- keeping rebuild and maintenance routes here is correct
- this logic should stay operator-facing rather than leaking into product APIs

Performance and operational notes:

- the main risk is job correctness and recovery semantics, not API complexity
- stale embedding detection is a useful safeguard and should remain cheap relative to full reindex jobs

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`

No delete-ready service-local docs were found in this pass.

## Bottom Line

`data-orchestrator-go` looks like a focused internal control service. The important follow-up area is degraded-mode visibility around job and cost-consumer paths.
