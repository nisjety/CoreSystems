# letta-bridge Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/go/services/letta-bridge`

## Snapshot

`letta-bridge` is the memory indexing and search bridge of Model Plane. It can use an external backend when configured, but falls back to an in-memory substring store by default.

Current evidence highlights:

- Go gRPC service plus HTTP health
- optional external backend selected by `AGENT_MEMORY_URL`
- default runtime is still in-memory

Non-generated file count from the current tree: about `13`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - health HTTP, gRPC, backend selection
- `internal/agentmemory/*`
  - external backend adapter
- `internal/server/*`
  - gRPC implementation

Primary surfaces:

- HTTP health on `:8088`
- gRPC on `:9096`

## API And Relationship Map

Current relationships:

- `model-gateway` and memory-related flows depend on `letta-bridge`
- `letta-bridge` -> external memory backend when configured
- otherwise `letta-bridge` -> in-memory substring store

## Duplicates, Redundancies, And Inactive Surfaces

The main split is backend shape:

- external durable-ish adapter when configured
- in-memory fallback when not configured

## Stubs, Placeholders, And Missing Connections

Active partial:

- the "real" backend path is optional and not the default runtime

## API Design And Performance Notes

API design:

- keeping memory bridge behavior behind one gRPC surface is sensible

Performance and operational notes:

- in-memory fallback is useful for bootstrapping, but it is one of the clearest partial runtime surfaces in the plane

## Current Doc Cleanup Read

Update or archive, not delete:

- docs that imply external memory is the standard live deployment path

## Bottom Line

`letta-bridge` is live, but still strongly hybrid. The biggest gap is that durable external memory is optional rather than the default operating mode.
