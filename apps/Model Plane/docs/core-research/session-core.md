# session-core Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/rust/services/session-core`

## Snapshot

`session-core` is the durable state authority of Model Plane. It owns thread, message, run, checkpoint, plan, todo, approval, lineage, event replay, and context-compaction state.

Current evidence highlights:

- Rust service with Postgres-backed durable state
- gRPC plus HTTP health surface
- background NATS, orchestration-event, and compaction workers are live
- older docs materially understated how much orchestration state is already implemented here

Non-generated file count from the current tree: about `21`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - Postgres connect and migrate, gRPC, HTTP health, NATS workers, orchestration NATS worker, compaction loop
- `src/grpc/*`
  - session and orchestration RPC surfaces
- `src/orchestration_grpc.rs`
  - plans, todos, approvals, lineage, event streaming
- `src/store/*`
  - durable state persistence
- `src/compaction/*`
  - compaction background logic

Primary surfaces:

- gRPC on `:9091`
- HTTP health/metrics on `:18081`

## API And Relationship Map

Current relationships:

- `model-gateway` -> `session-core`
  - session, orchestration, and approval durability
- `orchestrator-core` -> `session-core`
  - orchestration gRPC proxy paths
- `session-core` -> Postgres
  - source of truth for durable Model Plane state
- `session-core` -> NATS
  - event and orchestration signaling

## Duplicates, Redundancies, And Inactive Surfaces

The most important correction here is documentary:

- old docs still describe plans, todos, approvals, and lineage as mostly scaffolded
- the current code persists and emits them durably

No obvious inactive `.unused` or `.backup` source residue appeared in this pass.

## Stubs, Placeholders, And Missing Connections

This pass did not find a major live stub inside the core runtime boundary itself.

The main surrounding partial is external:

- other services and docs still sometimes behave as if session-core orchestration surfaces were not the durable source of truth yet

## API Design And Performance Notes

API design:

- session and orchestration durability belong together here
- keeping gRPC as the main service surface is appropriate

Performance and operational notes:

- this is a critical stateful service and one of the plane's strongest authorities
- compaction and event replay loops make correctness more important than raw route breadth

## Current Doc Cleanup Read

Keep:

- `MODEL_PLANE_DEEP_DIVE.md`

Update or archive, not delete:

- docs that still treat orchestration durability as mostly future work

## Bottom Line

`session-core` is a real durable authority, not a placeholder. The main remaining problem is doc drift and surrounding services still carrying older assumptions about its maturity.
