# cost-core Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/go/services/cost-core`

## Snapshot

`cost-core` owns the token and cost ledger surface for Model Plane.

Current evidence highlights:

- Go HTTP API plus gRPC health service
- ledger is in-memory
- usage feed subscriber still uses placeholder file-input mode until a real NATS client is wired

Non-generated file count from the current tree: about `8`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - HTTP API, gRPC health, in-memory ledger, placeholder usage subscriber
- `internal/ledger/*`
  - in-memory ledger store
- `internal/server/*`
  - HTTP API routes

Primary surfaces:

- HTTP on `:8089`
- gRPC health on `:9098`

## API And Relationship Map

Current relationships:

- usage producers should feed `cost-core`
- `cost-core` -> in-memory ledger
- placeholder subscriber path -> `NATS_FEED_PATH` file input

## Duplicates, Redundancies, And Inactive Surfaces

No obvious inactive source residue was found in this pass.

The main partial shell is transport:

- HTTP API exists
- gRPC is health-only
- usage subscriber is not yet a real NATS client path

## Stubs, Placeholders, And Missing Connections

Active partials:

- in-memory ledger only
- gRPC is health-only
- usage subscriber still reads from `NATS_FEED_PATH` placeholder input until real NATS wiring is added

## API Design And Performance Notes

API design:

- cost authority belongs in a dedicated service

Performance and operational notes:

- durability and real event ingestion matter more than route complexity

## Current Doc Cleanup Read

Update or archive, not delete:

- docs that imply `cost-core` is fully integrated with real NATS usage

## Bottom Line

`cost-core` exists, but it is still a partial surface. The core missing pieces are durability and real event ingestion, not the absence of an API shell.
