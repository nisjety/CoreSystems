# audit-core Research Dive

Generated: 2026-06-07

Scope: `apps/Control Plane/audit-core`

## Snapshot

`audit-core` is the smallest of the Control Plane cores. It subscribes to shared audit and usage subjects, persists them to Postgres, and exposes an internal-key-gated read/write API for audit and usage inspection.

Current evidence highlights:

- simple chi-based HTTP API
- queue-subscribed NATS ingest for audit and usage subjects
- no retry/NAK semantics by design for malformed or poison events
- no extra core-local docs to clean

Non-generated/non-vendored file count from the current tree: about `12`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/server/main.go`
  - config load
  - Postgres connection and idempotent schema migration
  - NATS connection
  - subscriber startup
  - chi router startup
- `internal/api/api.go`
  - mounted HTTP API
- `internal/subscriber/subscriber.go`
  - audit/usage queue subscriptions
- `internal/store/store.go`
  - persistence layer

## API And Relationship Map

HTTP surface:

- `GET /healthz`
- `GET /readyz`
- `GET /v1/audit`
- `POST /v1/audit`
- `GET /v1/usage`
- `GET /v1/usage/summary`

Required query/auth semantics:

- internal API key via `X-Internal-Api-Key` or `X-Api-Key`
- org-scoped read paths using `org_id`

Current relationships:

- `audit-core` -> shared NATS
  - subscribes to `velion.audit.v1.>`
  - subscribes to `velion.usage.v1.>`
- `audit-core` -> Postgres
  - durable audit and usage persistence
- upstream publishers
  - `auth-core`
  - other planes publishing usage or audit events on the shared bus

## Duplicates, Redundancies, And Non-Relationships

No large duplication cluster was found in this core.

Intentional simplicity:

- one subscriber per subject family
- one store implementation
- one mounted API surface

Non-relationship / design constraint:

- the core trusts the caller to attach a verified `org_id`; it does not independently resolve org identity from auth-core

## Stubs, Placeholders, And Failure Semantics

Test-only mock usage exists in:

- `internal/store/store.go` comments
- `internal/api/api_test.go`

These are ordinary test seams, not runtime concerns.

Important runtime behavior:

- malformed events are dropped
- store errors are logged
- events are not NAK'd or retried back into NATS

The subscriber comments make this explicit: audit/usage events are treated as observability data, not state-changing operations.

## API Design And Performance Notes

API design:

- small, coherent, and internal-only
- read endpoints are list/query oriented and correctly scoped by `org_id`

Performance and operational notes:

- queue subscriptions correctly distribute work across replicas
- at-most-once handling is acceptable for this observability role, but it trades completeness for throughput and simplicity
- absence of replay/retry means upstream publisher reliability matters

## Current Doc Cleanup Read

No delete-ready core-local docs were identified.

## Bottom Line

`audit-core` is simple and honest. The main thing to document is its operational contract:

- it is not a strong consistency pipeline
- it is a best-effort observability ingestion service with org-scoped read APIs
