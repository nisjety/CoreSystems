# conversation-ingest-rs

## Current State

`conversation-ingest-rs` is a thin but real Rust ingest adapter in front of `conversation-core-go`.

It:

- exposes `/health` and `/ready`
- accepts normalized or raw email-style ingest payloads
- validates required fields
- canonicalizes message bodies, sender identity, and event metadata
- forwards accepted events into `conversation-core-go`

## Entry Points

- Main: `apps/Application Plane/conversation-core/conversation-ingest-rs/src/main.rs`
- Router and normalization logic: `apps/Application Plane/conversation-core/conversation-ingest-rs/src/lib.rs`

## Exposed Surface

- `GET /health`
- `GET /ready`
- `POST /internal/ingest/email`
- `POST /internal/ingest/normalized-email`

## Relationships

- Forwards to `conversation-core-go/internal/conversation-events`.
- Shares the Application Plane internal API key model.
- Exists as the ingest-side translator for conversation events rather than as an independently durable service.

## Stub, Mock, Placeholder, and Unused Audit

- No placeholder runtime path is obvious in the current code.
- The service is intentionally thin and mostly acts as validation plus translation.
- In-repo callers are not prominent, so its real traffic source is likely external or operational tooling rather than frontend code.

## Notes

This service is small, but it is not dead code. It should be documented as a narrow ingress adapter.
