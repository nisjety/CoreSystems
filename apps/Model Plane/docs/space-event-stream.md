# Space event stream (WS02)

A shared room learns that something changed (a thread appeared, a post landed, a run changed state) through one resumable SSE stream per tab, then refetches through the authorized read path. The wire carries ids and closed words only, never message text, titles, previews or outputs.

```
messages / threads / runs  --(deferred constraint triggers, migration 0072)-->  space_event_log + space_event_heads
        |                                                                      pg_notify('space_events') at COMMIT
        v                                                                      (listener task -> in-process wake)
session-core  SpaceEvents.StreamSpaceEvents (gRPC)  -- verifies the Control decision, gates every delivery
        v
model-gateway GET /v1/spaces/:space_id/events (SSE)
        v
gateway BFF (WS02-S4) -> SPA (WS02-S5)
```

The log is the source of truth. A wake only removes latency: every stream also polls on a 30 s floor, so a lost `NOTIFY` costs latency, never a row.

## Contract

The SSE vocabulary, cursor grammar, kind registry, closing codes and HTTP error map are pinned by `api/contracts/space-events.json` (and its `.sha256`, checked by `scripts/tests/zdr-contract-sync.test.sh`). The gRPC shape is `proto/model_plane/v1/space_events.proto`. The SSE `id` is `<generation>.<seq>`; a client resumes with `Last-Event-ID`. `seq` is contiguous per generation: a row the subscriber may not see is a `hidden` stub, an erased row is `redacted`.

## Authority (every hop re-checks its own input)

1. **model-gateway**: verified bearer, user principal only (a service token is refused by the route allowlist). It forwards the decision from the `x-space-decision-ref` / `x-space-decision-token` headers (headers, not query, so they stay out of access logs) and holds no authority of its own.
2. **session-core, at open**: the caller's `user_id` equals the verified user; the Control decision verifies for the audience `model-plane-space-stream` (signature, key id, action, schema, payload digest, subject, org, Space, `thread:read` and no write permission, privacy completeness, time window). A read decision (`model-plane`) is refused by the stream, and a stream decision is refused by `ListThreads`.
3. **session-core, per delivery** (`space::events::gate`, pure): lease expiry (`min(decision expiry - 10 s, 85 to 100 s jittered)`), membership revocation (`space_membership_revocations` row at or after the decision's `issued_at - 5 s`, behind a 2 s cache), the decision's audience-revision ceiling (`reauthorize`), unknown scope or kind (stubbed `hidden`). A heartbeat re-runs the stream-wide checks without a Control call. The lease is the hard bound on revocation latency; each lease needs a fresh decision.
4. **Zero Data Retention**: a ZDR caller or a decision with `zero_data_retention` gets `connected{mode: "poll_only"}` then `closing{zdr_poll_only, reconnect: false}`, and no log row is read. The log never holds rows for a `zdr` run.

Row visibility inside a shared room equals the listing rule (`recipient_audience_revision <= decision revision`).

## Operation

| Setting | Where | Meaning |
|---|---|---|
| `SPACE_REALTIME_EMIT` | session-core | writes log rows; off in release artifacts (S1) |
| `SPACE_STREAM_DB_POOL` | session-core | stream reads use their own pool (default 8), so streams cannot starve the main pool |
| `SPACE_STREAM_MAX_STREAMS` | session-core | open streams per replica (default 5000), above it `RESOURCE_EXHAUSTED` (model-gateway: 503 `space_stream_capacity` with `Retry-After`) |

The service is inert until the gateway BFF calls it (WS02-S4, flag `space_realtime_stream`). Metrics (closed-word labels only): `mp_session_space_stream_active`, `mp_session_space_stream_opened_total`, `mp_session_space_stream_closed_total{reason}`, `mp_session_space_stream_frames_total{kind}`.

Backpressure: a send that blocks for 15 s ends the stream with `closing{slow_consumer}`. A cursor that cannot be honoured (retention, generation change, ahead of head, or a hole in the rows) answers `cursor_too_old` and the client resnapshots.
