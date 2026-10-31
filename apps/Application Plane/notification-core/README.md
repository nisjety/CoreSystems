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

Attention notifications (`modelplane.needs_approval`, `modelplane.needs_input`,
`modelplane.plan_ready`, WS16-S4) and scheduled-run outcome notices
(`modelplane.scheduled_run_outcome`, WS20-S7) are delivered without a provider.
With `NOTIFICATION_NATIVE_TYPES_ENABLED=true` (default `false`, literal `true`
only) they go to the native adapter in `internal/runtime/native.go`, which writes
the in-app feed row from the same dispatch and makes no outbound call. This works
with `NOTIFICATION_DELIVERY_MODE=disabled`. The adapter accepts only these types,
refuses zero-retention requests, and refuses any payload field outside the
content-free set (fixed copy, a deep link and opaque ids), so the native path
persists no run content. Every other type keeps the provider path. Turn the switch
on before capability-core's `CAPABILITY_CORE_ATTENTION_NOTIFY_ENABLED`: with it off,
the attention types take the provider path and fail while delivery is disabled.
For these types the preference gate reads the in-app channel alone: an email or
push opt-out does not suppress the in-app row, and an in-app opt-out suppresses
the request, because native delivery is the only channel they have in this build.
Readiness counts native delivery as available, so `/ready` is 200 with native
types on even while the provider is disabled.

Web Push (WS16-S6) is off unless `NOTIFICATION_WEB_PUSH_ENABLED=true` (literal
`true` only). It needs `NOTIFICATION_VAPID_PUBLIC_KEY`,
`NOTIFICATION_VAPID_PRIVATE_KEY`, a `mailto:` or `https://` contact in
`NOTIFICATION_VAPID_SUBJECT`, and `NOTIFICATION_PUSH_SUBSCRIPTION_KEY` (32 bytes,
hex). `scripts/run-application-plane.sh` mints the VAPID pair and the subscription
key, so no key is typed by hand, and only this service receives the private VAPID
key. A push goes only for a waiting native type: an attention
type, or a scheduled-run outcome that waits on its owner (`needs_review`,
`paused_expired` or `paused_failures`). It goes only to a recipient whose push
channel is not switched off, and never for a zero-retention request. Each push is
an encrypted wake-up whose only content is the constant `{"v":1,"k":"attention"}`,
so no thread, run, title or tool text leaves the service. Endpoints are accepted
only from the Google, Mozilla, Apple and Windows push hosts on the default port,
and the sender never follows a redirect. A push service that answers 404 or 410
revokes that subscription. Endpoints and keys are sealed with AES-256-GCM, bound to
their row. The gateway proxies `GET /api/v1/push/config`,
`POST /api/v1/push/subscriptions` and `DELETE /api/v1/push/subscriptions/{id}`.

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

Web Push is not ready to switch on. The queued delivery worker never sends a push,
subscriptions outlive a purged organisation until Control publishes a purge event,
and whether Chrome shows a notice for each push while a window is visible is not
verified.

Measured race-suite coverage is 81.4% for delegation, 59.1% for notification,
32.6% for HTTP, and 41.0% for database. This does not meet the 80% critical-module
gate. See `docs/core-research/notification-core.md` and the dated plane audit for
the evidence and deployment limitations.
