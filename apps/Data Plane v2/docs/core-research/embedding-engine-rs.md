# embedding-engine-rs Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/embedding-engine-rs`

## Snapshot

`embedding-engine-rs` turns indexed knowledge units and wiki events into dense vectors and writes them into Qdrant. It is the vector-write authority inside Data Plane v2.

Current evidence highlights:

- Rust JetStream consumer with admin HTTP
- provisions Qdrant collections on boot
- handles both document knowledge units and wiki/entity-summary embeddings
- compose defaults still route embedding through direct provider HTTP rather than the intended Model Plane hop

Non-generated file count from the current tree: about `12`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - Postgres, Qdrant, provider selection, JetStream consumers, wiki subscriber, admin HTTP
- `src/provider.rs`
  - embedding backend selection
- `src/qdrant_writer.rs`
  - collection provisioning and vector writes
- `src/stream.rs`
  - knowledge-unit consumer
- `src/wiki_consumer.rs`
  - wiki publish subscriber

Surface:

- admin and health HTTP only
- JetStream consumers for index-time embedding flow

## API And Relationship Map

Current relationships:

- `index-engine-rs` -> `embedding-engine-rs`
  - knowledge-unit progression triggers dense embedding
- `wiki-store-go` -> `embedding-engine-rs`
  - wiki publish events can trigger wiki block embeddings
- `embedding-engine-rs` -> Qdrant
  - provisions and writes primary, wiki-block, and entity-summary collections
- `retrieval-engine-rs` -> `embedding-engine-rs`
  - depends on resulting Qdrant collections for dense retrieval

## Duplicates, Redundancies, And Inactive Surfaces

The main redundancy is architectural, not file-level:

- embedding provider selection is duplicated conceptually between Data Plane direct provider access and the intended Model Plane embedding route
- current runtime comments indicate this duplication is a workaround for the deeper gRPC embedding-path issue

## Stubs, Placeholders, And Missing Connections

No explicit source stubs or backup residue showed up in this service tree.

The main partial relationship is:

- Data Plane -> Model Plane embedding is not the default live path in compose
- direct Azure HTTP is the operational fallback

## API Design And Performance Notes

API design:

- keeping this service internal and admin-only is correct
- collection provisioning at boot is the right place for Qdrant readiness

Performance and operational notes:

- provider routing is the main architecture concern because it affects cost, latency, and consistency between indexing and query-time embedding
- multiple collections increase operational surface, but they match the retrieval product requirements

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `docs/quickwit-read-model.md`

No delete-ready service-local docs were found in this pass.

## Bottom Line

`embedding-engine-rs` is real and production-shaped. The important unresolved issue is not vector writing. It is the still-transitional provider path between Data Plane and Model Plane.
