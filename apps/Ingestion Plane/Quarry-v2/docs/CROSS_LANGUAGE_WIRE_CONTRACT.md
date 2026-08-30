# Cross-Language Wire Contract

This document is the source of truth for the wire shapes the Rust edge
expects from the Go control plane and the live `quarrycontracts`
golden tests pin against. The same shapes are tested on both sides
because the contract is byte-for-byte the source of truth — drift in
either language breaks the other.

## Two shapes, one wire

The control plane serves two wire shapes, deliberately:

1. **Single-object endpoints** return a bare JSON object that the Rust
   `forward_one::<T>` deserializer parses as `T`. The control-plane
   handler calls `httpx.WriteRawJSON` (`quarry-control/internal/httpx`)
   so the response body is exactly the bare object.
2. **List endpoints** return a `Page<T>` envelope of the form
   `{items: [...], next_cursor?: string, total_estimated?: number}`.
   The Rust `forward_list::<T>` deserializer accepts this shape; if a
   `data` wrapper is present, it is unwrapped, but the bare Page is
   the canonical emit shape.

The `quarrycontracts.RESTEnvelope { data, meta, error }` envelope
exists in `pkg/quarrycontracts/envelope.go` and is used by
`httpx.WriteJSON` **for handler-emitted error responses only** (via
`httpx.WriteErr`). It is **not** the success shape for the
single-object or list routes.

## Routes by shape

| Route family | Shape | Reason |
| --- | --- | --- |
| `GET /v1/team/{credit,token,concurrency,queue}-usage` | bare object | The four `TeamUsage.*` aggregates are decoded by the edge's `forward_one::<T>`; rust's `TeamCreditUsage`/`TeamTokenUsage`/etc. do not have a `data` field, so an envelope would force every Rust call site to carry a parallel decoder. |
| `POST /v1/sources` | bare object (the new source row) | `forward_one` decodes the response as `Source`. |
| `DELETE /v1/sources/{id}` | 204 NoContent | no body |
| `GET /v1/sources` | `Page<Source>` | `forward_list` decodes. |
| `GET /v1/snapshots` | `Page<Snapshot>` | `forward_list` decodes. |
| `GET /v1/snapshots/{id}` | bare object | `forward_one` decodes. |
| `GET /v1/request-queues` | `Page<RequestQueueSummary>` | `forward_list` decodes. |
| `GET /v1/team/activity` | `Page<TeamActivityEntry>` | `forward_list` decodes. |
| `GET /v1/{kind}/jobs` | `Page<JobSummary>` | `forward_list` decodes. |
| `GET /v1/jobs/{id}` | bare object | `forward_one` decodes. |
| `GET /v1/jobs/{id}/history` | `Page<JobHistoryEvent>` | `forward_list` decodes. |
| `GET /v1/runs/{id}/events` | `Page<RunEvent>` | `forward_list` decodes. |
| All other error responses | `quarrycontracts.RESTEnvelope { error }` via `httpx.WriteErr` | The Rust `Envelope<T>` decodes this for typed error handling. |
| `POST /v1/webhooks/change` | typed `ChangeAck` | the change-webhook receiver is an internal HMAC-gated endpoint, not edge-forwarded. |

## Why the bare-object / Page split, not the REST envelope

The `quarrycontracts.RESTEnvelope { data, meta, error }` shape is the
right envelope for handler-emitted errors. It is **not** the right
success shape for these routes because:

1. The Rust edge's `forward_one::<T>` and `forward_list::<T>`
   deserializers are typed: they call `serde_json::from_slice` into
   the specific `T` (or `Page<T>`). They do not currently branch on
   envelope presence.
2. Adding a `data` wrapper for single-object responses would force a
   parallel decoder on every Rust call site with no client-facing
   benefit (the response body is the same bytes).
3. The tests that lock the wire (`cycle24_test.go` and
   `quarrycontracts` golden tests) already pin the bare-object /
   `Page<T>` shapes byte-for-byte. Switching to envelopes would break
   the cross-language roundtrip tests that are the only thing
   preventing drift between Rust and Go.

The honesty rule from `gap-quarry.md`: when a wire shape changes, the
change is additive-only (per `CLAUDE.md` contracts-frozen-at-v1 rule
6) and a new ADR is required. Wrapping bare objects in
`RESTEnvelope.data` is a **breaking** change to the Rust edge's
`forward_one` decoder and a breaking change to every cycle-24
quarrycontracts golden test. Do not do it without an ADR.

## Envelope use in errors

Every error response in the control plane uses `httpx.WriteErr` with a
typed `quarrycontracts.ErrorEnvelope`:

```go
httpx.WriteErr(w, r, quarrycontracts.CodeBadRequest, "name and url are required", nil)
```

The `ErrorCode` is one of the constants in
`pkg/quarrycontracts/envelope.go` (`CodeBadRequest`, `CodeNotFound`,
`CodeRateLimited`, `CodeUnsupported`, etc.); the `HTTPStatus()` method
maps each constant to the correct status code. The Rust edge
`Envelope<T>::err` decodes the same shape.

`CodeUnsupported` (HTTP 501) is the dedicated code for routes that
are wired but require a runtime dependency that isn't configured —
Temporal client, GPU driver, etc. It is **never** a silent 200-OK
lie: when the route handler returns `UNSUPPORTED`, the caller's
client gets a typed, machine-actionable error.

## Test files that pin these contracts

| Layer | Test file | Pins |
| --- | --- | --- |
| Rust edge | `crates/quarry-edge/tests/change_webhook_proptest.rs` | change-webhook wire (signed body) |
| Rust edge | `crates/quarry-edge/tests/dom_summary_proptest.rs` | DOM-summarization contract |
| Rust edge | `crates/quarry-core/tests/change_history_proptest.rs` | change-history wire |
| Rust core | `crates/quarry-core/tests/properties.rs` (F1) | URL, fingerprint, ID, SSRF properties |
| Go control | `services/quarry-control/internal/resources/cycle23_test.go` | sources CRUD org scoping |
| Go control | `services/quarry-control/internal/resources/cycle24_test.go` | team aggregates wire + scoping |
| Go control | `services/quarry-control/internal/resources/schedules_test.go` | schedule lifecycle |
| Go control | `services/quarry-control/internal/resources/change_webhook_test.go` | change-webhook receiver |
| Go control | `services/quarry-control/internal/resources/cycle23_aliases_test.go` (F3) | trigger/backfill 501 |
| Go shared | `pkg/quarrycontracts/change_webhook_test.go` | cross-language change webhook |
| Go shared | `pkg/quarrycontracts/request_queue_test.go` | cross-language request-queue wire |
| Go shared | `pkg/quarrycontracts/snapshot_test.go` | cross-language snapshot wire |
| Go shared | `pkg/quarrycontracts/crossplane_test.go` | cross-language Data Plane + Model Plane contracts |
| Go shared | `pkg/quarrycontracts/envelope_test.go` | `RESTEnvelope` shape + error codes |
