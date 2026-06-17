# autocomplete-core Research Dive

Generated: 2026-06-07

Scope: `apps/Ingestion Plane/autocomplete-core`

## Snapshot

`autocomplete-core` is a small tenant-scoped suggestions sidecar. It serves autocomplete suggestions and can optionally ingest signals from NATS.

Current evidence highlights:

- Rust Axum service
- API can run without NATS
- no explicit runtime stubs or backup residue were found in the active tree

Non-generated, non-vendored file count from the current tree: about `17`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - tracing, settings, app state, optional NATS consumer, HTTP listener
- `src/lib.rs`
  - exported application surface and state construction

Surface:

- API server on the configured HTTP address
- optional NATS consumer for ingestion signals

## API And Relationship Map

Current relationships:

- Frontend Plane -> `autocomplete-core`
  - tenant-scoped suggestions
- `autocomplete-core` -> NATS
  - optional signal ingestion path

## Duplicates, Redundancies, And Inactive Surfaces

No obvious duplicate runtime surface was found in this pass.

This service is intentionally small and peripheral.

## Stubs, Placeholders, And Missing Connections

No explicit runtime stub, placeholder, or `.unused`/`.backup` residue was found in the active service tree.

The main operational caveat is simply that:

- NATS can be disabled and the API still serves

## API Design And Performance Notes

API design:

- small dedicated service boundary is fine for this role

Performance and operational notes:

- optional NATS ingestion means local and degraded modes may under-exercise the full suggestion-refresh behavior

## Current Doc Cleanup Read

Keep:

- `autocomplete-core/README.md`
- `autocomplete-core/docs/DESIGN.md`

## Bottom Line

`autocomplete-core` appears straightforward and real. It is not where the Ingestion Plane complexity lives.
