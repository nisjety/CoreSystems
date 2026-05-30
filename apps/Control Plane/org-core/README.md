# Org Core (Control Plane)

Go service for control-plane organization metadata and policy decisions.

## Responsibilities

- Organization metadata (`plan`, `status`, org identity)
- Entitlements (feature flags and policy metadata)
- Policy/event publication (no enforcement)
- Cross-service compatibility endpoints for auth/user integration

## Boundaries

- Does NOT store user credentials or sessions (owned by auth-service)
- Does NOT own user profiles (owned by user-service)
- Does NOT perform retrieval/vector/document data-plane operations

## API

- `POST /api/v1/auth/login`
- `GET /api/v1/users/me`
- `GET /api/v1/organizations/:id`
- `GET /api/v1/organizations/:id/entitlements`

## Events Published

- `user.created`, `user.updated`, `user.deleted`
- `organization.created`, `organization.updated`, `organization.deleted`
- `session.created`, `session.ended`

## Run locally

```bash
cd Org-core
go mod tidy
go run ./cmd/server
```
