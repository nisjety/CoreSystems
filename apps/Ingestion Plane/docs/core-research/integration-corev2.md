# integration-corev2 Research Dive

Generated: 2026-06-07

Scope: `apps/Ingestion Plane/integration-corev2`

## Snapshot

`integration-corev2` is the connector broker for the Ingestion Plane. It owns provider sessions, OAuth, connection discovery, actions, hotpath normalization, and handoff into downstream workers such as Finspo.

Current evidence highlights:

- Go API service plus a separate Finspo worker binary
- can run with Postgres or an in-memory repository
- NATS event publisher is optional behind config
- provider catalog breadth is ahead of provider parity in discovery/actions/OAuth

Non-generated, non-vendored file count from the current tree: about `67`.

## Runtime Shape

Key runtime entrypoints:

- `cmd/api/main.go`
  - config validation, repository setup, token vault, OAuth service, Control Plane clients, discovery/actions services, hotpath normalizer, API server
- `cmd/finspo-worker/main.go`
  - polling worker that claims sync jobs and hands them to Finspo
- `internal/oauth/*`
  - provider session and OAuth flow logic
- `internal/discovery/*`
  - provider discovery surface
- `internal/actions/*`
  - provider action execution surface
- `internal/api/*`
  - API routes including legacy compatibility routes

## API And Relationship Map

Current relationships:

- Frontend/Application Plane -> `integration-corev2`
  - provider connection and sync orchestration surface
- `integration-corev2` -> Control Plane
  - auth, org, billing, audit clients
- `integration-corev2` -> Finspo
  - job handoff through the worker path
- `integration-corev2` -> NATS
  - optional event publication
- `integration-corev2` -> Postgres or in-memory repository
  - connector persistence

## Duplicates, Redundancies, And Inactive Surfaces

Clear redundancy:

- `internal/api/legacy_routes.go` keeps a compatibility route family alive beside the main API shape
- repository can degrade to in-memory when no database URL is configured

That flexibility is pragmatic, but it increases behavior variation across environments.

## Stubs, Placeholders, And Missing Connections

Active partials:

- `internal/discovery/service.go`
  - returns not implemented for unsupported providers
- `internal/actions/service.go`
  - actions are not implemented for some providers
- `internal/oauth/service.go`
  - catalog presence does not guarantee direct OAuth support yet
- `internal/oauth/provider_client.go`
  - profile discovery is not implemented for some providers

Missing or partial relationships:

- provider catalog maturity is uneven
- some providers appear in the system before discovery, actions, and OAuth parity are complete

## API Design And Performance Notes

API design:

- the API plus worker split is appropriate
- keeping connector brokerage separate from SharePoint governance in Finspo is the right boundary

Performance and operational notes:

- in-memory repository fallback is acceptable for local dev, but it creates a large behavioral gap from real deployments
- multiple outbound Control Plane and provider HTTP clients make timeout tuning important

## Current Doc Cleanup Read

Keep:

- `integration-corev2/docs/ARCHITECTURE.md`

Review or archive, not delete:

- `integration-corev2/README.md`
  - still useful, but needs a strict truth pass against current provider parity

## Bottom Line

`integration-corev2` is real and important, but it is still a partial connector platform. The key issue is not whether the broker exists. It is that provider breadth still outruns fully implemented provider behavior.
