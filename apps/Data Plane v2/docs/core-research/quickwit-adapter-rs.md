# quickwit-adapter-rs Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/quickwit-adapter-rs`

## Snapshot

`quickwit-adapter-rs` is the sparse read-model adapter for Data Plane v2. It ensures the Quickwit index exists, keeps it aligned with the canonical corpus, and supports rebuild workflows.

Current evidence highlights:

- Rust admin service with no product-facing API
- owns Quickwit ensure-index and rebuild orchestration
- optional rebuild-on-start path exists
- sparse retrieval remains optional because `retrieval-engine-rs` can fall back to Postgres sparse search

Non-generated file count from the current tree: about `9`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - Postgres, Quickwit client, ensure-index, optional rebuild-on-start, NATS subscriber, admin HTTP
- `src/quickwit.rs`
  - Quickwit client and index coordination
- `src/rebuild.rs`
  - rebuild logic
- `src/stream.rs`
  - event subscription
- `src/api/*`
  - admin surface

Surface:

- admin and health HTTP on `9204`

## API And Relationship Map

Current relationships:

- `quickwit-adapter-rs` -> Quickwit
  - sparse read model ownership
- `quickwit-adapter-rs` -> Postgres
  - rebuild source of truth
- `retrieval-engine-rs` -> `quickwit-adapter-rs`
  - optional sparse backend consumer

## Duplicates, Redundancies, And Inactive Surfaces

Intentional redundancy:

- sparse retrieval has two live strategies in the plane
  - Quickwit-backed sparse search
  - Postgres sparse fallback

That overlap is deliberate for resilience, but it increases tuning and contract surface.

## Stubs, Placeholders, And Missing Connections

This pass did not find explicit stubs, mock paths, or inactive source residue in this service tree.

The main partial relationship is architectural:

- Quickwit is important, but it is not the only sparse-search path, so some deployments may under-exercise it

## API Design And Performance Notes

API design:

- keeping this adapter off the product path is correct
- rebuild and ensure-index controls belong here

Performance and operational notes:

- rebuild-on-start is operationally useful but should be watched in large corpora
- the main value of this service is sparse-search quality and rebuild speed, not route breadth

## Current Doc Cleanup Read

Keep:

- `docs/quickwit-read-model.md`
- `DATA_PLANE_DEEP_DIVE.md`

No delete-ready service-local docs were found in this pass.

## Bottom Line

`quickwit-adapter-rs` looks purpose-built and clean. The interesting future work is not code noise. It is deciding how much of the sparse-search path should remain dual-backed versus being simplified later.
