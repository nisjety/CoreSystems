# ADR 0005: V1 Navbar Parity With Real Integration Boundaries

## Status

Accepted

## Context

Verevon v2 needs the v1 dashboard navbar interaction model: history controls, workspace breadcrumbs, centered command search, AI/chat/message/notification/calendar/profile actions, and light/dark theme switching. The v2 implementation must not embed local example records for these controls.

## Decision

The navbar remains a feature-level client component for responsive UI state, while all data and writes cross typed route-handler boundaries:

- Novu notifications are read and updated through `/api/v1/navbar/notifications`.
- Calendar events and notes are saved through `/api/v1/navbar/calendar`, which forwards to user-core.
- Support requests are saved through `/api/v1/navbar/support`, which forwards to user-core.
- Theme preference is saved through `/api/v1/navbar/theme`, backed by user-core appearance settings.
- Global search runs through `/api/v1/navbar/search` so database credentials and row ownership checks stay server-side.

## Consequences

The navbar can render without seeded data, but production functionality requires signed-in Better Auth sessions, `DATABASE_URL`, user-core internal credentials, and Novu credentials. Missing services surface as visible configuration errors instead of silently falling back to fabricated records.
