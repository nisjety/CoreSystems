# information-core

## Current State

`information-core` is a small internal Application Plane API for weather, traffic, and news. It is live in compose and should be treated as a real service, even if it is strategically smaller than the other app-plane cores.

At startup it:

- loads config
- creates an in-memory cache
- wires outbound HTTP clients
- builds weather, traffic, and news services
- starts a Gin HTTP server

## Entry Points

- Main: `apps/Application Plane/information-core/cmd/server/main.go`
- Routes: `apps/Application Plane/information-core/internal/http/server.go`

## Exposed Surface

- `GET /health`
- `GET /ready`
- `GET /api/v1/weather`
- `GET /api/v1/weather/oslo`
- `GET /api/v1/traffic`
- `GET /api/v1/news`

## Relationships

- `velionv2` references `information-core` through `src/app/api/v1/information/_lib/upstream.ts`.
- `velionv2` dashboard cards consume that BFF path for weather, traffic, and news.
- The service is internal-key-gated and uses no dedicated plane database in the boot path.

## Stub, Mock, Placeholder, and Unused Audit

- No obvious runtime placeholder path in the main service path.
- The service is narrow and utility-like, but it is not unused inside the repo.
- Indirect `go.uber.org/mock` references are normal dependency noise, not a runtime concern.

## Notes

This is a live internal-support API, not a central collaborative core. Its role is honest but modest.
