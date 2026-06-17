# model-gateway Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/rust/services/model-gateway`

## Snapshot

`model-gateway` is the public boundary of Model Plane. It owns HTTP, gRPC, SSE, auth validation, invoke normalization, orchestration routing, and cross-plane relays into Data Plane and Ingestion Plane.

Current evidence highlights:

- Rust public boundary service with HTTP and gRPC
- routes to `session-core`, `inference-core`, `execution-core`, `capability-core`, and several Go sidecars
- background consumers for capability-registry cache coherence and document-index readiness
- approval persistence is split between best-effort durable forwarding and a local in-memory store

Non-generated file count from the current tree: about `44`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - boot, telemetry, HTTP server, gRPC server, capability consumer, document-index consumer, fine-tune poller
- `src/http_routes/*`
  - public HTTP boundary
- `src/grpc.rs`
  - public gRPC boundary
- `src/auth.rs`
  - JWKS and optional dev-bypass auth behavior
- `src/approvals.rs`
  - approval gate behavior

Primary surfaces:

- HTTP on `:8080`
- gRPC on `:9090`
- invoke, invoke-stream, orchestration, approval, task, cron, memory, skill, and capability-facing routes

## API And Relationship Map

Current relationships:

- clients and upper planes -> `model-gateway`
  - main public entrypoint
- `model-gateway` -> `session-core`
  - durable session/orchestration state
- `model-gateway` -> `inference-core`
  - provider routing and multimodal inference
- `model-gateway` -> `execution-core`
  - runtime step execution
- `model-gateway` -> `capability-core`
  - workplane HTTP APIs and capability cache
- `model-gateway` -> Data Plane
  - retrieval, graph, and wiki relays
- `model-gateway` -> Ingestion Plane
  - Quarry-dependent fetch and extract-structured paths

## Duplicates, Redundancies, And Inactive Surfaces

Clear runtime overlap:

- approval state is split between an in-memory gateway store and best-effort durable persistence into orchestration state
- capability state is consumed through both runtime cache consumers and durable capability-core APIs

That overlap is intentional for latency and resilience, but it means the service still carries mixed authority boundaries.

## Stubs, Placeholders, And Missing Connections

Active partials:

- Quarry-dependent RPCs return `Unimplemented` when Quarry edge is not configured
- `src/auth.rs` still supports dev-bypass stub claims for local development
- `api/openapi.yaml` still describes `/v1/ai/realtime` as placeholder
- approval store is explicitly in-memory with best-effort durable sync

Missing or partial relationships:

- some Ingestion Plane-dependent features are honestly gated on `QUARRY_EDGE_URL`
- the gateway remains partially authoritative for approvals even though durable orchestration state exists

## API Design And Performance Notes

API design:

- this boundary is correctly broad
- routing orchestration, invoke, and workplane relays through one gateway is coherent for clients

Performance and operational notes:

- background best-effort consumers are correctly non-fatal
- the main risk is mixed authority around approvals and feature availability depending on downstream service configuration

## Current Doc Cleanup Read

Keep:

- `MODEL_PLANE_DEEP_DIVE.md`
- `README.md`

Update or archive, not delete:

- `docs/ARCHITECTURE.md`
  - under-describes the current live gateway and downstream routing behavior

## Bottom Line

`model-gateway` is real and broad. The important debt is not missing ownership. It is mixed authority in a few stateful edges and configuration-gated feature completeness.
