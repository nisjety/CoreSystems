# sandbox-manager Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/go/services/sandbox-manager`

## Snapshot

`sandbox-manager` owns sandbox lease and snapshot lifecycle for Model Plane.

Current evidence highlights:

- Go gRPC service plus HTTP health
- implementation is live
- stores are in-memory only
- entrypoint comment still incorrectly calls the gRPC surface a stub

Non-generated file count from the current tree: about `12`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - health HTTP, gRPC, in-memory lease store, in-memory snapshot store
- `internal/lease/*`
  - lease store
- `internal/snapshot/*`
  - snapshot store
- `internal/server/*`
  - gRPC service implementation

Primary surfaces:

- HTTP health on `:8086`
- gRPC on `:9094`

## API And Relationship Map

Current relationships:

- `model-gateway` and `execution-core` depend on sandbox lifecycle control
- `sandbox-manager` -> in-memory lease and snapshot state only

## Duplicates, Redundancies, And Inactive Surfaces

Documentation redundancy:

- `cmd/main.go` still says the gRPC server is a stub returning `Unimplemented`
- the server implementation is actually live

## Stubs, Placeholders, And Missing Connections

Active partials:

- durability is missing; stores are in-memory only

The misleading comment is stale commentary rather than missing implementation.

## API Design And Performance Notes

API design:

- the boundary is narrow and sensible

Performance and operational notes:

- correctness and durability are the main concerns
- in-memory state means leases and snapshots are not strong across restarts

## Current Doc Cleanup Read

Update or archive, not delete:

- docs that still describe this service as a pure stub

## Bottom Line

`sandbox-manager` is more live than old docs imply, but it is still operationally partial because the backing stores are in-memory.
