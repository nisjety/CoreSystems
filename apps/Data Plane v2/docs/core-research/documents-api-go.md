# documents-api-go Research Dive

Generated: 2026-06-07

Scope: `apps/Data Plane v2/services/documents-api-go`

## Snapshot

`documents-api-go` is the ingest and document-metadata authority for Data Plane v2. It owns document CRUD, bulk ingest, source object lifecycle, duplicate inspection, and the canonical document event outbox.

Current evidence highlights:

- Go HTTP service with Postgres and NATS
- primary write entrypoint for ingestion into Data Plane v2
- NATS outbox publisher is live
- `pkg/authctx` is still in a transition state
- usage publisher is constructed but not yet forwarded into handler call sites

Non-generated, non-vendored file count from the current tree: about `24`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go`
  - loads config
  - opens Postgres and NATS
  - starts outbox publishing loop
  - wires internal API key, auth context, and org middleware
- `internal/handler/*`
  - document and source-object HTTP handlers
- `internal/repo/*`
  - canonical document and source-object persistence
- `internal/events/*`
  - document lifecycle publication
- `pkg/authctx/authctx.go`
  - transition middleware from legacy org header trust toward JWT-based auth context

Primary HTTP surface:

- `/v1/documents`
- `/v1/documents/bulk`
- `/v1/documents/{documentID}`
- `/v1/sources`
- `/v1/source-objects`
- `/v1/source-objects/duplicates`
- `/v1/source-objects/delete`

## API And Relationship Map

Current relationships:

- Ingestion Plane -> `documents-api-go`
  - canonical synchronous document write path
- `documents-api-go` -> Postgres
  - document authority and source-object state
- `documents-api-go` -> NATS
  - lifecycle events via outbox
- `documents-api-go` -> downstream Data Plane services
  - `index-engine-rs` consumes its document progression events

Auth and boundary notes:

- internal API key middleware still exists
- `authctx` middleware is intended to replace blind trust in `X-Org-ID`
- `handler.OrgIDMiddleware` still participates in the transition path

## Duplicates, Redundancies, And Inactive Surfaces

Clear runtime redundancy:

- the auth boundary is currently split across internal API key middleware, observe-mode JWT decoding, and legacy org header handling
- this is transitional rather than accidental, but it is still a duplication hotspot in the request path

Decommissioned surface:

- old shared-NATS `quarry.documents.crawled` subscriber path has been intentionally removed in favor of synchronous HTTP ingest

## Stubs, Placeholders, And Missing Connections

Active partials:

- `pkg/authctx/authctx.go` is explicitly a stub
  - observe mode parses claims without signature verification
  - enforce mode fails closed with `503` because verification is not implemented
- `pkg/usagepub` is instantiated in `cmd/main.go`, but the comment states handler wiring is still follow-up work

Missing or partial relationships:

- auth-core JWT verification is not yet the live enforced source of truth for this service
- usage and audit publication from document handlers is not fully connected

## API Design And Performance Notes

API design:

- consolidating document writes into HTTP here is the right ownership boundary
- keeping source-object routes beside document routes is coherent because duplicate detection and source lifecycle share the same persistence context

Performance and operational notes:

- outbox publish loop with `FOR UPDATE SKIP LOCKED` is replica-safe and appropriate
- auth transition logic is the main operational risk, not throughput shape

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `docs/migration-v1-to-v2.md`

Update or archive, not delete:

- `docs/gap-data.md`
  - still useful as a historical completion ledger, but it overstates some closed areas compared with the current runtime

## Bottom Line

`documents-api-go` is real and central. The important remaining gaps are not about ownership. They are:

- unfinished verified JWT enforcement
- transitional duplicate auth-boundary logic
- usage publisher wiring that is present but not yet consumed by handlers
