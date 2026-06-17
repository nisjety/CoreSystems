# index-engine-rs Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/index-engine-rs`

## Snapshot

`index-engine-rs` is the narrow event-driven indexing core for Data Plane v2. It converts canonical documents into chunked knowledge units and emits downstream progression for embedding and graph work.

Current evidence highlights:

- Rust JetStream consumer with admin HTTP surface
- no app-facing product API
- owns chunking, normalization, extraction, and fingerprinting stages
- appears structurally clean and intentionally small

Non-generated file count from the current tree: about `12`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - Postgres connection
  - JetStream setup and consumer start
  - admin HTTP server
- `src/chunker.rs`
- `src/fingerprint.rs`
- `src/normalizer.rs`
- `src/extract.rs`
- `src/builder.rs`
- `src/stream.rs`

Surface:

- admin and health HTTP only
- NATS JetStream consumer for document indexing events

## API And Relationship Map

Current relationships:

- `documents-api-go` -> `index-engine-rs`
  - canonical document events drive indexing
- `index-engine-rs` -> Postgres
  - knowledge-unit persistence and progression state
- `index-engine-rs` -> `embedding-engine-rs`
  - emits downstream progression for embedding work
- `index-engine-rs` -> `graph-index-rs`
  - emits downstream progression for graph extraction

## Duplicates, Redundancies, And Inactive Surfaces

No obvious duplicate surface showed up in this pass.

The service is intentionally headless:

- admin HTTP exists for health and operations
- actual product-facing behavior is consumed through downstream retrieval services

## Stubs, Placeholders, And Missing Connections

This pass did not find explicit runtime stubs, placeholders, or backup residue inside this service tree.

The main dependency risk is external:

- if upstream document events drift, this service has little independent contract surface to catch that at the API layer

## API Design And Performance Notes

API design:

- the service boundary is correct
- avoiding a broad human-facing API here keeps indexing logic internal and rebuildable

Performance and operational notes:

- the service is fundamentally throughput-bound on event consumption and Postgres writes
- because it is event-driven and narrow, the main performance questions belong in batch sizing, chunking strategy, and stream backpressure, not route design

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `docs/WIRE_RECONCILIATION.md`

No delete-ready service-local docs were found in this pass.

## Bottom Line

`index-engine-rs` looks like a clean internal engine. It is not the noisy part of Data Plane v2. The important follow-up work here is verification and rebuild confidence, not major ownership correction.
