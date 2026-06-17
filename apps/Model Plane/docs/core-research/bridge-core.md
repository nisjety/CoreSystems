# bridge-core Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/go/services/bridge-core`

## Snapshot

`bridge-core` is the CLI, IDE, and channel ingress shell for Model Plane. It owns session registration/listing/get/close, payload ingress, and channel adapter registration.

Current evidence highlights:

- HTTP API is real
- gRPC server is created but no service registration is visible in the entrypoint
- default channel adapters are noop or skeleton implementations

Non-generated file count from the current tree: about `13`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - HTTP API, gRPC server, session registry, adapter registry
- `internal/session/*`
  - session registry
- `internal/channel/*`
  - adapter registry and adapter implementations
- `internal/server/*`
  - HTTP handler surface

Primary surfaces:

- HTTP on `:8091`
- gRPC on `:9100`

## API And Relationship Map

Current relationships:

- channel and ingress clients -> `bridge-core`
- `bridge-core` -> session registry
- `bridge-core` -> channel adapter registry

## Duplicates, Redundancies, And Inactive Surfaces

Active partial shell behavior:

- the ingress shell exists
- concrete channel delivery paths remain mostly noop or skeleton behavior

There is also a notable transport gap:

- entrypoint spins up a gRPC server without visible service registration

## Stubs, Placeholders, And Missing Connections

Active partials:

- default adapters are noop or skeleton implementations
- WebSocket-adjacent or richer delivery paths are still future-facing
- gRPC transport appears present at the server level but not fully wired in the entrypoint

## API Design And Performance Notes

API design:

- keeping ingress and session/channel registration together is coherent

Performance and operational notes:

- this is more of a surface-completeness question than a throughput question

## Current Doc Cleanup Read

Update or archive, not delete:

- docs that treat `bridge-core` as absent
- docs that imply channel delivery is already broadly wired

## Bottom Line

`bridge-core` is real, but still one of the clearest partial shells in Model Plane. The HTTP side exists; the concrete channel paths are still limited.
