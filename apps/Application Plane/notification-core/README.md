# Notification Core

`notification-core` is the Application Plane's first-party notification intake,
provider-submission ledger, feed, preference, and channel-policy service.

## Current source contract — 2026-07-13

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
not `delivered`; no callback/reconciliation path currently advances delivery.

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
guessing access. Provider submission and feed projection are not joined by a
durable outbox; provider callbacks/reconciliation, durable preference sync,
deep dependency readiness, and a multi-replica replay store are absent.
Support automation remains disabled because current workflows do not supply
authoritative organization/user mapping.

Measured race-suite coverage is 81.4% for delegation, 59.1% for notification,
32.6% for HTTP, and 41.0% for database. This does not meet the 80% critical-module
gate. See `docs/core-research/notification-core.md` and the dated plane audit for
the evidence and deployment limitations.
