# support-worker Research Dive

Generated: 2026-06-07

Scope: `apps/Ingestion Plane/services/support-worker`

## Snapshot

`support-worker` is a standalone TypeScript Temporal worker with a NATS bridge for support automation workflows.

Current evidence highlights:

- separate Temporal worker process
- queue name is hard-coded as `support-task-queue`
- NATS bridge is started beside the worker
- deployment status is still unclear from code alone

Non-generated, non-vendored file count from the active `src` tree is small; the service also carries committed `dist/` output and `node_modules/`.

## Runtime Shape

Key runtime entrypoints:

- `src/index.ts`
  - Temporal native connection
  - Temporal client connection
  - worker creation
  - NATS bridge startup
  - graceful shutdown
- `src/workflows/*`
  - `triage`, `sla`, `csat`
- `src/activities/*`
  - classify, notify-agent, patch-zammad, send-csat
- `src/nats-bridge.ts`
  - bridge from NATS events into Temporal

## API And Relationship Map

Current relationships:

- `support-worker` -> Temporal
  - executes support workflows on `support-task-queue`
- `support-worker` -> NATS
  - bridge from message bus into workflow execution
- `support-worker` -> support tooling such as Zammad through activities

## Duplicates, Redundancies, And Inactive Surfaces

Operational redundancy and noise:

- committed `dist/` output exists beside `src/`
- committed `node_modules/` also exists in the service tree

That does not automatically mean the runtime is wrong, but it increases workspace noise and confusion.

## Stubs, Placeholders, And Missing Connections

This pass found no explicit active source stub in `src/`.

The main unresolved issue is deployment truth:

- code shows a real worker
- current runtime/deployment usage is not proven from this tree alone

## API Design And Performance Notes

API design:

- keeping support automation in a dedicated Temporal worker is coherent

Performance and operational notes:

- dual Temporal connections are expected for worker plus client roles
- the main operational question is not local code shape; it is whether this worker is still a live deployed part of the platform

## Current Doc Cleanup Read

Review:

- committed `dist/` output and `node_modules/` should be treated as operational noise until the repo policy for built artifacts is confirmed

## Bottom Line

`support-worker` looks like a real service in code, but its deployment status is still uncertain. It should be treated as active-but-needing-runtime-verification rather than assumed dead.
