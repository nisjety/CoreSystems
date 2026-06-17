# browser-broker Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/go/services/browser-broker`

## Snapshot

`browser-broker` owns trusted browser grant issuance, revocation, and validation.

Current evidence highlights:

- Go gRPC service plus HTTP health
- grant lifecycle methods are implemented
- storage is in-memory only

Non-generated file count from the current tree: about `16`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - health HTTP, gRPC, in-memory grant store
- `internal/grant/*`
  - grant store and logic
- `internal/server/*`
  - gRPC implementation

Primary surfaces:

- HTTP health on `:8087`
- gRPC on `:9095`

## API And Relationship Map

Current relationships:

- `model-gateway` and execution/browser flows depend on browser grant lifecycle
- `browser-broker` -> in-memory grant store

## Duplicates, Redundancies, And Inactive Surfaces

No obvious duplicate or inactive source residue was found in this pass.

## Stubs, Placeholders, And Missing Connections

Active partial:

- durability is still missing because grant storage is in-memory only

## API Design And Performance Notes

API design:

- narrow dedicated broker boundary is correct

Performance and operational notes:

- in-memory grants are fine for local and partial deployments
- restart durability is the main missing operational property

## Current Doc Cleanup Read

Update or archive, not delete:

- docs that still imply the broker is absent or only future work

## Bottom Line

`browser-broker` is real, but still not durable.
