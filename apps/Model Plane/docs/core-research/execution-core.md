# execution-core Research Dive

Generated: 2026-06-09

Scope: `apps/Model Plane/rust/services/execution-core`

## Snapshot

`execution-core` is the runtime loop authority for agent execution. It is the clearest runtime evidence that Model Plane does support agents.

Current evidence highlights:

- Rust gRPC runtime-loop service
- owns step execution, resume, tool dispatch, approvals, shell execution, browser-agent loop, and subagent hooks
- state store is local and non-durable

Non-generated file count from the current tree: about `25`.

## Runtime Shape

Key runtime entrypoints:

- `src/main.rs`
  - state store, gRPC, HTTP health
- `src/grpc/*`
  - execution RPC surface
- `src/runtime_loop/*`
  - step loop and tool routing
- `src/browser_agent/*`
  - browser observation/action loop
- `src/subagent/*`
  - subagent hook and spawn support

Primary surfaces:

- gRPC on `:9093`
- HTTP health/metrics on `:18083`

## API And Relationship Map

Current relationships:

- `model-gateway` -> `execution-core`
  - run and step execution
- `orchestrator-core` -> `execution-core`
  - long-running orchestration activities
- `execution-core` -> approval flow
  - runtime can create approval-paused outcomes
- `execution-core` -> sandbox and browser-adjacent dependencies
  - shell and browser execution behavior

## Duplicates, Redundancies, And Inactive Surfaces

No obvious inactive source residue was found in the active service tree.

The main limitation is storage shape, not duplicated behavior:

- runtime state store is local rather than durable

## Stubs, Placeholders, And Missing Connections

This pass did not find a major live stub in the primary runtime path.

The important partial is architectural:

- agent execution is real, but some surrounding supporting services remain hybrid or in-memory

## API Design And Performance Notes

API design:

- agent runtime ownership is clear and correctly separated from durable session state

Performance and operational notes:

- browser-agent and subagent flows can be expensive and need surrounding infrastructure to be healthy
- lack of durable local state means surrounding coordination services matter for long-running reliability

## Current Doc Cleanup Read

Keep:

- `MODEL_PLANE_DEEP_DIVE.md`

## Bottom Line

`execution-core` is real agent runtime, not branding. The main follow-up question is durability and surrounding service maturity, not whether agent execution exists.
