# Notification Core

`notification-core` is the Application Plane's first-party notification intake,
provider-submission ledger, feed, preference, and channel-policy service.

## Current source contract — 2026-08-17

The canonical intake is `POST /api/v1/notification-requests`. Requests require:

- `organization_id`;
- an organization-scoped `idempotency_key`;
- a typed Control user recipient: `{ "kind": "user", "id": "..." }`;
- an allowlisted workflow `type` for the verified caller;
- `retention_mode: "standard" | "zdr"`;
- service-specific HMAC v2 delegation bound to caller, `notification-core`
  audience, method, URI, tenant, user, role, timestamp, nonce, and body digest.

Legacy `x-internal-api-key` authentication is rejected. Gateway and
support-worker credentials are separate, required at startup, and known public
placeholder families are rejected. Feed, preferences, channel configuration,
subscriber mapping, idempotency, and membership are organization scoped.
User-facing routes require an active Control-derived local membership; channel
changes also require admin role.

HTTP surfaces:

- `GET /health`, `GET /ready`;
- `POST /api/v1/notification-requests`;
- feed/read state under `/notifications`;
- preferences under `/preferences`;
- channel configuration under `/channels/config`.

The old `/v1/notifications` and `/internal/recipients/upsert` routes do not
exist.

## Delivery and retention semantics

Delivery mode is explicit: `disabled` or `novu`. Disabled mode returns readiness
503 and cannot fabricate provider success. Provider acceptance is `submitted`,
not `delivered`; only the separately config-gated callback route can advance a
delivery attempt after an exact provider receipt.

The source now contains the next bounded delivery contract: migration
`010_delivery_attempts.up.sql` adds a content-free attempt ledger with
`pending → claimed → sent_unconfirmed → acknowledged | failed | unknown`,
lease fencing, provider correlation, and an opt-in queue/worker path. Provider
transport errors become `unknown` and cannot be blindly retried. Callback
verification requires a fresh HMAC envelope and an external replay store. The
route returns `503` while `NOTIFICATION_DELIVERY_CALLBACK_SECRET` is empty.
The server now wires the queue and lease-fenced worker only when
`NOTIFICATION_DELIVERY_WORKER_ENABLED=true`; configuration rejects that flag
unless Novu mode and a callback verifier are present. The flag remains false
in dev. Submission now atomically creates the content-free
`notification_feed_projection_attempts` obligation. Callback acknowledgement
propagates the provider receipt into that obligation, and a second
lease-fenced projector idempotently writes the Activity/Inbox row, including
callback-before-projection ordering. Disposable Postgres covers two-worker
lease fencing and idempotent replay; real HA replay, provider-specific
ZDR/receipt attestation, and candidate deployment proof remain disabled until
proven together.

For `zdr`, payload content is used transiently for the current provider request
but is not stored in the notification ledger or feed. Only control metadata and
the server-computed request fingerprint remain, and lifecycle events redact the
recipient. This is local containment only: the full payload is still sent to
Novu, whose adapter does not enforce a proven provider-side ZDR/retention mode.
Do not enable ZDR content delivery until that downstream contract is established.
Standard mode persists payload/feed content according to the normal product
contract.

## Production blockers

This source is not deployed. The running July 13 image predates these changes.
There is no trustworthy signed/revisioned Control membership writer or scoped
backfill, so secure source intentionally denies legitimate requests rather than
guessing access. The provider submission and feed projection are now joined by
a source-level durable outbox, but the running service remains disabled by
default; provider callbacks/reconciliation, durable preference sync, deep
dependency readiness, and a multi-replica replay/candidate proof are absent
from the deployed service.
Support automation remains disabled because current workflows do not supply
authoritative organization/user mapping.

Measured race-suite coverage is 81.4% for delegation, 59.1% for notification,
32.6% for HTTP, and 41.0% for database. This does not meet the 80% critical-module
gate. See `docs/core-research/notification-core.md` and the dated plane audit for
the evidence and deployment limitations.
