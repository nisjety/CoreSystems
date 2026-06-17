# capability-core Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/go/services/capability-core`

## Snapshot

`capability-core` is a hybrid registry, policy, and workplane service. It owns capability metadata, gRPC policy evaluation, HTTP APIs for skills/MCP/routing/safety/memory/tasks/cron, and learning-review triggers.

Current evidence highlights:

- Go service with both HTTP and gRPC
- durable Postgres-backed workplane APIs
- in-memory registry abstraction still exists, but loads from Postgres when possible
- older docs that call it "in-memory only" are no longer accurate

Non-generated file count from the current tree: about `86`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - Postgres pool, registry loading, model registry, capability store, policy engine, learning consumer, HTTP APIs, gRPC server
- `internal/api/*`
  - workplane CRUD APIs
- `internal/server/*`
  - gRPC capability/policy server
- `internal/registry/*`
  - registry and store abstractions
- `internal/sessionreview/*`
  - learning review trigger paths

Primary surfaces:

- HTTP on `:8085`
- gRPC on `:9097`

## API And Relationship Map

Current relationships:

- `model-gateway` -> `capability-core`
  - tasks, cron, memory, skills, capability metadata
- `capability-core` -> Postgres
  - durable workplane and registry backing
- `capability-core` -> `session-core` and `inference-core`
  - delegated helper calls
- `capability-core` -> NATS
  - reconcile publication and learning-review trigger

## Duplicates, Redundancies, And Inactive Surfaces

Key hybrid overlap:

- gRPC registry/policy engine still works from an in-memory loaded `Registry`
- HTTP workplane APIs are clearly durable Postgres-backed CRUD surfaces

This is a genuine hybrid state, not a cleanly single-mode service.

## Stubs, Placeholders, And Missing Connections

Active partials:

- some internal catalogs remain seed-style in-memory metadata
- comments in `internal/server/server.go` still under-describe durability

Missing or partial relationships:

- convergence between loaded in-memory registry and fully durable capability authority is still incomplete

## API Design And Performance Notes

API design:

- hybrid registry plus workplane is coherent, but it must be described honestly
- delegating `/models` and compaction helpers outward is a sensible boundary choice

Performance and operational notes:

- registry reload and cache coherence behavior matter more than raw route complexity
- NATS-driven learning loops add useful capability but also add another non-trivial asynchronous path

## Current Doc Cleanup Read

Keep:

- `go/services/capability-core/README.md`

Update or archive, not delete:

- `go/services/capability-core/docs/gap-analysis.md`
  - likely under-describes current durable behavior
- `docs/ARCHITECTURE.md`
  - still undercalls the current live state here

## Bottom Line

`capability-core` is not in-memory only and not fully converged either. The right description is hybrid: durable workplane APIs plus a still-loaded registry abstraction.
