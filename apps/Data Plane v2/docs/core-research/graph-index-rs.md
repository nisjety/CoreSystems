# graph-index-rs Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/graph-index-rs`

## Snapshot

`graph-index-rs` is the graph extraction and graph-query core for Data Plane v2. It persists entities, claims, relationships, communities, and graph mappings derived from indexed content.

Current evidence highlights:

- Rust service with HTTP admin/API plus gRPC
- event-driven extraction from indexed content
- orphan-cleanup subscriber exists to prevent stale graph residue after re-chunking
- older GraphRAG planning language appears broader than the currently visible runtime

Non-generated file count from the current tree: about `12`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - Postgres, extractor, JetStream consumer, HTTP server, gRPC server, orphan cleanup subscriber
- `src/extractor.rs`
  - graph extraction logic
- `src/store.rs`
  - graph persistence and query backing
- `src/grpc/*`
  - graph retrieval wire
- `src/stream.rs`
  - event consumption and cleanup wiring

Surface:

- admin/API HTTP on `9203`
- gRPC graph surface on a separate port

## API And Relationship Map

Current relationships:

- `index-engine-rs` -> `graph-index-rs`
  - indexed content progression drives graph extraction
- `graph-index-rs` -> Postgres
  - persists entities, relationships, claims, communities, mappings
- `retrieval-engine-rs` -> `graph-index-rs`
  - graph retrieval and graph-aware search

## Duplicates, Redundancies, And Inactive Surfaces

No explicit inactive source residue was found in this pass.

The main documentation redundancy is:

- older design language describes future-facing AST-heavy graph work that is not evident as a separate live runtime surface here

## Stubs, Placeholders, And Missing Connections

No explicit code stubs or backup files were found in the active service tree.

Partial relationship to watch:

- graph runtime is live, but some older docs still describe future graph extraction ambitions rather than the current running boundary

## API Design And Performance Notes

API design:

- separate HTTP admin and gRPC retrieval wires make sense for this service
- same-store multi-wire design keeps the read model coherent

Performance and operational notes:

- orphan cleanup is the important correctness and performance safeguard here
- graph extraction quality and Postgres query shape will matter more than route complexity

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`

Update or archive, not delete:

- older GraphRAG planning notes when they over-describe not-yet-visible runtime scope

## Bottom Line

`graph-index-rs` appears live and structurally sound. The main risk is documentation drift around how much graph ambition is already runtime reality.
