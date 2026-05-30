# Notification Core

`notification-core` is the first-party notification boundary service for the
Application Plane.

V0 scope:

1. expose a health endpoint
2. accept authenticated notification requests from internal services
3. persist notification request state in Postgres
4. hand off requests to a hidden Novu runtime adapter
5. publish lifecycle events on the shared Velion NATS bus

Non-goals for V0:

1. inbox feeds
2. preference management
3. workflow editing
4. provider webhooks and delivery receipts
5. read or unread state projections for end-user notification feeds
