# conversation-core-go

## Current State

`conversation-core-go` is a live first-party support and conversation API. It is not a placeholder package.

At startup it:

- loads config
- connects to `application-postgres`
- runs migrations
- optionally connects to NATS
- wires repository, service, and HTTP server layers

## Entry Points

- Main: `apps/Application Plane/conversation-core/conversation-core-go/cmd/server/main.go`
- Routes: `apps/Application Plane/conversation-core/conversation-core-go/internal/http/server.go`

## Exposed Surface

- `GET /health`
- `GET /ready`
- `GET /api/v1/inboxes`
- `GET /api/v1/inboxes/:id/queue`
- `GET /api/v1/conversations`
- `GET /api/v1/conversations/:id`
- `POST /api/v1/conversations/search`
- `POST /api/v1/conversations/:id/messages`
- `POST /api/v1/conversations/:id/notes`
- `PATCH /api/v1/conversations/:id/status`
- `PATCH /api/v1/conversations/:id/assignment`
- `POST /api/v1/conversations/:id/tags`
- `DELETE /api/v1/conversations/:id/tags/:tag`
- `POST /api/v1/ai-actions/:id/review`
- `POST /api/v1/ai-actions/:id/approve`
- `POST /api/v1/ai-actions/:id/reject`
- `POST /internal/conversation-events`
- `GET /internal/conversations/:id/projection`

## Relationships

- `conversation-ingest-rs` forwards normalized events into `/internal/conversation-events`.
- Known historical caller: `velionv2` talks to this service through `src/lib/integrations/conversation-core.ts`.
- Current Velion v3 usage should be verified through the Frontend Plane gateway domains before treating it as live.
- The service uses shared `application-postgres`.
- It can publish events through NATS when JetStream wiring is available.

## Stub, Mock, Placeholder, and Unused Audit

- No runtime stub path stood out in the main boot path.
- NATS publishing is optional and degrades when the client cannot connect.
- The service appears live; its current Velion v3 caller path needs verification.

## Notes

This service is missing from some older Application Plane narratives, but it is one of the clearest live ownership surfaces in the plane.
