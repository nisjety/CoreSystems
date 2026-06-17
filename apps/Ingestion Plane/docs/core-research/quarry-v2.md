# Quarry-v2 Research Dive

Generated: 2026-06-07

Scope: `apps/Ingestion Plane/Quarry-v2`

Privacy contract: [`../../../GDPR_SUMMARY.md`](../../../GDPR_SUMMARY.md). Quarry
owns the egress/proxy broker and policy checks; managed browser, unblocker, and
proxy providers are processor-approved adapters only.

## Snapshot

`Quarry-v2` is the structurally dominant ingestion core in this plane. It is not one service. It is a split runtime composed of:

- `quarry-edge` as the public ingest boundary
- `quarry-runtime` as the hot-path execution engine
- `quarry-control` as the durable resource and operator API
- `quarry-orchestrator` as the Temporal workflow worker

Current evidence highlights:

- Rust edge/runtime plus Go control/orchestrator split is real
- public ingest, durable jobs, and long-running orchestration are clearly separated
- dev and rollout compatibility paths are still visible in several places
- broad planning/docs surface exists alongside the runtime, so documentation drift risk is high

Non-generated, non-vendored file count from the current tree: about `349`.

## Runtime Shape

Key runtime entrypoints:

- `crates/quarry-edge/src/main.rs`
  - public API, auth guards, dispatch, cache, event fan-out, artifact backend setup
- `crates/quarry-runtime/src/lib.rs`
  - driver registry, browser execution, action runtime, artifacts, queues, answer/search, vector hooks
- `services/quarry-control/cmd/control/main.go`
  - durable resource API, HMAC verification, Postgres/in-memory store selection, dispatcher
- `services/quarry-orchestrator/cmd/orchestrator/main.go`
  - Temporal worker, schedules manager, job dispatcher, workflow registration

Observed runtime split:

- `quarry-edge` owns public `/v1/scrape`, `/v1/crawl`, `/v1/batch`, auth, validation, preflight, SSE
- `quarry-runtime` owns the page-level execution engine
- `quarry-control` owns jobs, stores, snapshots, artifacts, profiles, schedules, webhooks, history
- `quarry-orchestrator` owns workflow execution and polling of posted jobs into Temporal

## API And Relationship Map

Current relationships:

- Frontend and upper planes -> `quarry-edge`
  - public ingestion boundary
- `quarry-edge` -> `quarry-runtime`
  - fast-path execution
- `quarry-edge` -> `quarry-control`
  - event and durable resource handoff
- `quarry-orchestrator` -> `quarry-control`
  - polls posted jobs and runs matching Temporal workflows
- `quarry-edge` -> Model Plane
  - answer, query, and agent-style downstream calls
- `quarry-edge` -> Data Plane
  - retrieval and search integration paths
- `quarry-control` and `quarry-orchestrator` -> Postgres and Temporal
  - durable state and orchestration backbone

## Duplicates, Redundancies, And Inactive Surfaces

Intentional migration overlap:

- edge can run fast-path work directly while durable paths still flow through control and orchestrator
- dev-compatible and rollout-compatible auth modes exist beside production enforcement guards
- event fan-out supports both local in-process and NATS-backed behavior depending on deployment

Documentation redundancy is substantial:

- the repo contains many planning, parity, progress, and gap docs
- several of them describe intended parity or future surfaces rather than only live-backed runtime behavior

## Stubs, Placeholders, And Missing Connections

Active partials and stubs:

- `crates/quarry-edge/src/auth.rs`
  - dev bypass can inject stub claims for local development
- `crates/quarry-edge/src/answer_routes.rs`
  - comments state downstream model gateway `/v1/invoke/stream` is still stubbed
- `services/quarry-control/internal/resources/cycle23.go`
  - source and schedule-related resource families still include empty or stubbed behavior
  - trigger and backfill note Temporal SDK wiring as pending
- `services/quarry-control/internal/store/store.go`
  - still carries in-memory store path for dev compatibility
- `crates/quarry-runtime/src/request_queue.rs`
  - durable Redis and Postgres queue backends are still TODO while in-memory backend remains the active generic path
- `crates/quarry-runtime/src/artifact_store.rs`
  - filesystem and S3 are real, but test-only unimplemented branches remain in the file

Missing or partial relationships:

- some schedule/resource contracts in `quarry-control` are ahead of their fully wired backend behavior
- downstream streamed answer flows depend on Model Plane behavior that is explicitly not fully complete

## API Design And Performance Notes

API design:

- the split between edge, runtime, control, and orchestrator is correct
- `quarry-control` is broad, but its breadth is coherent because it is the durable operator surface

Performance and operational notes:

- `quarry-edge` still supports in-memory artifact and local-index modes, which is useful for dev but not durable
- `quarry-orchestrator` polling plus Temporal workflows is operationally sensible, but it means ad-hoc job execution depends on the control/orchestrator loop staying healthy
- `quarry-runtime` still contains in-memory queue and cache-style defaults in important subsystems

## Current Doc Cleanup Read

Keep:

- `Quarry-v2/docs/ARCHITECTURE.md`
- `Quarry-v2/docs/CROSS_PLANE_INTEGRATION.md`

Review or archive, not delete:

- `Quarry-v2/docs/REST_RESOURCES.md`
  - useful for route intent, but not a current-runtime truth doc
- `Quarry-v2/docs/gap-quarry.md`
  - useful as progress history, risky as present-state authority

## Bottom Line

`Quarry-v2` is real and strategically central. The important concerns are:

- broad runtime/documentation surface area
- still-visible stubbed schedule/resource paths in control
- dev and rollout compatibility layers that remain live
- downstream Model Plane streamed-answer dependency that is not yet end-to-end complete
