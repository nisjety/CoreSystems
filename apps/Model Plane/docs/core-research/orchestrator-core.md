# orchestrator-core Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/go/services/orchestrator-core`

## Snapshot

`orchestrator-core` is the Temporal-based outer workflow shell for Model Plane. It owns long-running supervision and task workflows, feedback-promotion loops, compatibility subscriptions, and orchestration gRPC proxying.

Current evidence highlights:

- Go Temporal worker with health HTTP surface
- dials multiple sibling gRPC services
- compatibility adapter and orchestration event subscriptions are live
- several activities degrade gracefully when downstream services are unavailable

Non-generated file count from the current tree: about `39`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - Temporal worker, activity registration, gRPC sibling client dial, NATS compat subscriptions, orchestration subscriber, health HTTP
- `cmd/activities/*`
  - activity implementations calling session, inference, execution, capability, sandbox, browser, and memory paths
- `cmd/workflows/*`
  - interactive supervision, deep task, memory consolidation, skill promotion, feedback promotion, wide research
- `internal/grpcclient/*`
  - sibling service clients
- `internal/compat/*`
  - legacy compat behavior

Primary surfaces:

- HTTP health on `:8084`
- Temporal task queue and workflow registration
- orchestration gRPC handlers via internal server paths

## API And Relationship Map

Current relationships:

- `orchestrator-core` -> Temporal
  - workflow engine ownership
- `orchestrator-core` -> `session-core`
  - orchestration proxy and durable state coordination
- `orchestrator-core` -> `inference-core`, `execution-core`, `capability-core`, `sandbox-manager`, `browser-broker`, `letta-bridge`
  - long-running activity fanout
- `orchestrator-core` -> NATS
  - compatibility and orchestration subscriptions

## Duplicates, Redundancies, And Inactive Surfaces

Runtime overlap:

- compatibility subscriptions keep old subjects alive beside the newer orchestration/event paths

That is migration residue, but it is active and intentional.

## Stubs, Placeholders, And Missing Connections

Active partials:

- several activities degrade gracefully when downstream services are absent
- this is resilient, but it also means some flows become placeholder-like in degraded conditions instead of hard failing

No obvious `.unused` or `.backup` residue was found in the active service tree.

## API Design And Performance Notes

API design:

- workflow shell ownership is correct here
- keeping long-running orchestration outside the public gateway is the right separation

Performance and operational notes:

- the main concern is degraded-mode correctness across multiple sibling services
- compatibility subscriptions increase operational surface and should eventually narrow

## Current Doc Cleanup Read

Keep:

- `go/services/orchestrator-core/README.md`
- `MODEL_PLANE_DEEP_DIVE.md`

## Bottom Line

`orchestrator-core` is real and central. The main debt is not absence of workflows. It is compatibility carryover and degraded-mode placeholder behavior when siblings are missing.
