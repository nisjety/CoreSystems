# retrieval-engine-rs Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/retrieval-engine-rs`

## Snapshot

`retrieval-engine-rs` is the main application-facing query surface in Data Plane v2. It owns hybrid retrieval, context packing, trace logging, graph and wiki search fusion, cache invalidation, and multiple transport wires.

Current evidence highlights:

- largest Data Plane v2 runtime service in this plane
- HTTP and gRPC surfaces are both live
- reads Postgres, Qdrant, Dragonfly cache, optional NATS, and optional Quickwit
- policy and JWKS support are present but still transitional in deployment shape
- test coverage still has ignored scaffold cases

Non-generated file count from the current tree: about `47`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - Postgres, Qdrant, embedder, reranker, Dragonfly cache, optional NATS, JWKS init, policy client, HTTP server, gRPC server
- `src/api/mod.rs`
  - HTTP retrieval and diagnostic surface
- `src/grpc/*`
  - retrieval, knowledge, and document gRPC services
- `src/pipeline/orchestrator.rs`
  - retrieval pipeline assembly
- `src/search/*`
  - dense, sparse, graph, wiki, rerank, timeline, contradiction support
- `src/cache/*`
  - Redis-compatible cache and invalidation
- `src/authz/*`
  - JWT and policy enforcement

Primary runtime surfaces:

- HTTP on `8014`
- gRPC on `50062`
- tool-style retrieval and knowledge search surfaces for upper planes

## API And Relationship Map

Current relationships:

- `retrieval-engine-rs` -> Postgres
  - canonical metadata, traces, sparse search fallback
- `retrieval-engine-rs` -> Qdrant
  - dense vector retrieval
- `retrieval-engine-rs` -> Dragonfly
  - optional Redis-compatible cache layer
- `retrieval-engine-rs` -> Quickwit
  - optional sparse backend
- `retrieval-engine-rs` -> Control Plane
  - optional policy context and JWT verification material
- Frontend/Application/Model Plane -> `retrieval-engine-rs`
  - main retrieval and context assembly surface

## Duplicates, Redundancies, And Inactive Surfaces

Clear duplication or transitional overlap:

- sparse retrieval can run through Postgres or Quickwit, with fallback logic kept live for compatibility
- query-time embedding can go through direct Azure HTTP rather than the intended Data Plane -> Model Plane gRPC route
- gRPC `DocumentService` still exists for reads while write methods are deprecated and blocked by default

This is mostly intentional migration overlap, but it increases contract complexity.

## Stubs, Placeholders, And Missing Connections

Active partials:

- `tests/pipeline_e2e.rs` is still scaffolded
  - `happy_path` exists
  - ZDR rejection and cache invalidation cases remain TODO
- compose and docs still describe multiple auth and policy modes
- deeper Data Plane -> Model Plane embedding-path issue is worked around rather than fully solved

Missing or partial relationships:

- end-to-end verification still lags the runtime breadth
- transport parity is present, but the migration overlap around deprecated document writes remains a cleanup target

## API Design And Performance Notes

API design:

- combining HTTP and gRPC here is justified because this service is the shared retrieval facade for multiple planes
- keeping deprecated document writes blocked by default is correct; write ownership belongs in `documents-api-go`

Performance and operational notes:

- this is the most performance-sensitive Data Plane v2 runtime
- cache invalidation via optional NATS means degraded modes are possible and should be watched closely
- reranker, direct embedding, sparse backend choice, and graph/wiki fusion all affect latency and cost

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `tests/e2e/README.md`

Update or archive, not delete:

- `docs/gap-data.md`
  - useful history, but it still mixes closed gaps with live transition debt

## Bottom Line

`retrieval-engine-rs` is powerful and real, but it is also the main concentration of migration overlap in Data Plane v2. The important follow-up work is:

- finish the ignored end-to-end cases
- reduce transitional auth and embedding modes
- simplify overlapping sparse and deprecated document-wire paths
