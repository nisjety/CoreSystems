# inference-core Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/rust/services/inference-core`

## Snapshot

`inference-core` is the provider-routing engine of Model Plane. It owns chat, embeddings, speech, translation, language analytics, vision, document intelligence, realtime, and video provider chains.

Current evidence highlights:

- Rust multimodal provider router, not just a plain LLM proxy
- stateless aside from in-memory cache
- breadth of modality support is larger than several older docs imply

Non-generated file count from the current tree: about `23`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - config, provider chain construction, gRPC, HTTP health
- `src/provider/*`
  - modality-specific provider chains
- `src/grpc/*`
  - inference RPC surface
- `src/cache/*`
  - in-memory caching

Primary surfaces:

- gRPC on `:9092`
- HTTP health/metrics on `:18082`

## API And Relationship Map

Current relationships:

- `model-gateway` -> `inference-core`
  - invoke and multimodal routing
- `capability-core` -> `inference-core`
  - delegated model listing and related helper paths
- Data Plane -> `inference-core`
  - some embedding paths still depend on it in cross-plane flows

## Duplicates, Redundancies, And Inactive Surfaces

The main redundancy is architectural:

- several modalities are wired through provider chains even when the deployment may not have every backing provider configured

That is not dead code. It is a broad capability surface with config-dependent activation.

## Stubs, Placeholders, And Missing Connections

This pass did not find a major runtime stub in the service entrypoint itself.

Partial surface:

- some modalities are production-shaped only when corresponding provider env vars are present

## API Design And Performance Notes

API design:

- centralizing multimodal routing here is correct
- the service boundary is broader than "LLM only," and the docs should reflect that

Performance and operational notes:

- provider-chain ordering and fallback behavior are the key latency and cost levers
- because state is not durable here, operational correctness depends on provider configuration rather than storage concerns

## Current Doc Cleanup Read

Keep:

- `MODEL_PLANE_DEEP_DIVE.md`

Update or archive, not delete:

- docs that still describe a separate future `ai-core` as if this routing did not already live here

## Bottom Line

`inference-core` is a real multimodal router. The main issue is not missing functionality ownership. It is making the broader current scope visible and keeping provider-specific availability honest.
