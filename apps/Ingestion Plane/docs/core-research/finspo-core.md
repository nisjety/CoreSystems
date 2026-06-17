# finspo-core Research Dive

Generated: 2026-06-07

Scope: `apps/Ingestion Plane/finspo-core`

## Snapshot

`finspo-core` is the SharePoint and document-governance ingestion core. It owns source registration, delta sync, permissions capture, recommendations, audit trails, and optional execution of approved cleanup proposals.

Current evidence highlights:

- Go API service plus backfill utility
- Postgres-backed with optional NATS publisher
- consumes access tokens from `integration-corev2`
- writes source-object state into Data Plane documents APIs

Non-generated, non-vendored file count from the current tree: about `71`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/api/main.go`
  - config, Postgres, NATS publisher, Data Plane sink client, integration-core token provider, sync engine, executor, scheduler, API server
- `cmd/backfill-source-objects/main.go`
  - maintenance/backfill utility
- `internal/sync/*`
  - delta sync, scheduler, permissions capture, proposal execution
- `internal/sharepoint/*`
  - Graph delta, permissions, mutation, and browsing clients
- `internal/api/*`
  - source, analytics, proposals, execution routes

## API And Relationship Map

Current relationships:

- `integration-corev2` -> `finspo-core`
  - access token provider and sync-job linkage
- `finspo-core` -> Data Plane `documents-api-go`
  - source-object sink writes
- `finspo-core` -> Microsoft Graph
  - delta, permissions, mutation, browser clients
- `finspo-core` -> Postgres
  - sources, items, permissions, analytics, proposals, audit
- `finspo-core` -> NATS
  - optional event publication

## Duplicates, Redundancies, And Inactive Surfaces

No obvious inactive source residue showed up in this pass.

The main optional branch is deployment behavior:

- empty `NATS_URL` produces a no-op publisher
- `FINSPO_ALLOW_EXECUTION=true` enables destructive proposal execution

## Stubs, Placeholders, And Missing Connections

Active partials:

- NATS publishing is optional and degrades to no-op when unset
- proposal execution is gated by config and can remain disabled even while the review surface is live

No runtime source placeholders or `.unused`/`.backup` residue were found in the active service tree.

## API Design And Performance Notes

API design:

- Finspo has one of the cleanest ownership boundaries in the Ingestion Plane
- governance, sync, analytics, and proposal execution all belong together here

Performance and operational notes:

- continuous scheduler-driven sync means permissions capture and delta pagination quality matter more than route complexity
- optional destructive execution is operationally sensitive and correctly gated

## Current Doc Cleanup Read

Keep:

- `finspo-core/docs/ARCHITECTURE.md`
- `finspo-core/docs/API.md`

No delete-ready service-local docs were identified in this pass.

## Bottom Line

`finspo-core` looks more production-shaped than several neighboring ingestion services. The main caveats are optional event publication and config-gated destructive execution, not missing ownership.
