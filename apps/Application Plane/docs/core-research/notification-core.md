# notification-core

## Current State

`notification-core` is the first-party notification boundary for the Application Plane. It has outgrown its README.

At startup it:

- loads config
- connects to `application-postgres`
- runs migrations
- connects to NATS
- wires request, feed, preference, channel, and subscriber services
- optionally connects to a shared NATS cluster for cross-plane consumers
- starts HTTP routes and background subscribers

## Entry Points

- Main: `apps/Application Plane/notification-core/cmd/server/main.go`
- Routes: `apps/Application Plane/notification-core/internal/http/server.go`

## Exposed Surface

- `GET /health`
- `POST /api/v1/notification-requests`
- `GET /notifications`
- `GET /notifications/unread/count`
- `GET /notifications/unseen/count`
- `POST /notifications/:id/read`
- `POST /notifications/:id/seen`
- `POST /notifications/mark-all-read`
- `POST /notifications/mark-all-seen`
- `DELETE /notifications/:id`
- `GET /preferences`
- `PUT /preferences/:eventType/:channel`
- `GET /channels/config`
- `PATCH /channels/config/:eventType/:channel`
- `POST /internal/recipients/upsert`

## Relationships

- `velion` and `velionv2` both reference `notification-core`.
- The service consumes Control Plane session and identity events from shared NATS when available.
- It uses shared `application-postgres` and plane Redis.
- It can dispatch through a Novu adapter and mirrors notifications into its own feed cache.

## Stub, Mock, Placeholder, and Partial Audit

1. Novu runtime adapter
   - When `NOVU_SECRET_KEY` is unset, delivery runs in local stub mode.
   - Stub mode generates synthetic transaction IDs and does not send real notifications.

2. Subscriber upsert flow
   - The service intentionally inserts stub subscriber rows for sparse identity states and merges them later.

3. Documentation drift
   - `README.md` still describes a much smaller V0 service and explicitly lists inbox feeds, preferences, and read state as non-goals.
   - Those surfaces now exist in live code.

## Notes

This is one of the clearest stale-doc cases in the Application Plane. The runtime is real and broader than the docs claim.
