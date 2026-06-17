# wiki-store-go Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/wiki-store-go`

## Snapshot

`wiki-store-go` is the durable wiki and version-history source of truth in Data Plane v2. It owns pages, versions, backlinks, proposals, maintenance logs, and source logs.

Current evidence highlights:

- Go HTTP and gRPC service backed by Postgres
- NATS publication is optional
- wiki events feed downstream embedding behavior
- route surface is broad but internally coherent around page/version ownership

Non-generated, non-vendored file count from the current tree: about `15`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - Postgres, optional NATS publisher, HTTP server, gRPC server
- `internal/repo/*`
  - page, version, proposal, source-log, maintenance-log persistence
- `internal/handler/*`
  - HTTP wiki routes
- `internal/grpcserver/*`
  - gRPC `WikiService`

Primary surfaces:

- `/v1/wiki/pages`
- `/v1/wiki/pages/by-path`
- `/v1/wiki/pages/{pageID}`
- `/v1/wiki/pages/{pageID}/versions`
- `/v1/wiki/pages/{pageID}/diff`
- `/v1/wiki/pages/{pageID}/backlinks`
- `/v1/wiki/pages/{pageID}/proposals`
- `/v1/wiki/proposals/review`
- `/v1/wiki/pages/{pageID}/source-logs`
- `/v1/wiki/pages/{pageID}/maintenance-logs`
- `/v1/wiki/maintenance/sweep`
- gRPC `WikiService`

## API And Relationship Map

Current relationships:

- Frontend/Application Plane -> `wiki-store-go`
  - page and version product surfaces
- `wiki-store-go` -> Postgres
  - canonical wiki truth
- `wiki-store-go` -> NATS
  - optional `dataplane.wiki.version.published` path
- `embedding-engine-rs` depends on its publish flow

## Duplicates, Redundancies, And Inactive Surfaces

No explicit duplicate source residue was found in this pass.

The main operational split is transport, not ownership:

- HTTP and gRPC both exist over the same repo-backed logic

## Stubs, Placeholders, And Missing Connections

One active partial relationship matters:

- if `NATS_URL` is unset, the repo remains silent
- that is acceptable for local dev
- it also means downstream wiki embedding propagation disappears

This pass found no `.unused`, `.backup`, or obvious placeholder residue in the service tree.

## API Design And Performance Notes

API design:

- wiki ownership is cleanly centralized here
- the route family is broad, but it stays within one bounded context

Performance and operational notes:

- optional event silence is the main operational caveat
- because both HTTP and gRPC share the same repo, consistency risk is lower than in split implementations

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `docs/schemas/wiki_events.md`

No delete-ready service-local docs were found in this pass.

## Bottom Line

`wiki-store-go` looks like a real source-of-truth service. The thing to watch is not whether the wiki exists. It is whether local or partial deployments accidentally suppress downstream embedding updates.
