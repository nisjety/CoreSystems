# ADR: surface-neutral ingress and delivery

**Status:** accepted — 2026-08-13  
**Decision owner:** Control Plane (identity/link authority), with Application
Plane owning conversation projections and delivery records.

## Context

Verevon must let one person or room retain one agent, one workspace, one set
of permissions, one background queue, and one presence across web and future
external surfaces. Channel Plane is intentionally docs-only today; it must not
be treated as a deployed authority or runtime.

Email equality, a channel-local user ID, or a browser-supplied room ID is not a
stable identity link. A retrying external provider also cannot be used as a
delivery receipt. Both mistakes would create a second memory/work queue or let
an old/revoked external account continue acting in a Space.

## Decision

1. **Control owns verified external identity links.** A link is keyed by an
   immutable Control principal, provider, provider account subject, and link
   revision. It is created only through a human, provider-verification flow;
   matching email is discovery-only and never authorizes a link. Revocation,
   suspension, provider re-link, or organization removal advances the link's
   authority revision.
2. **Application owns a canonical conversation-to-Space projection.** An
   ingress adapter resolves a Control link, then asks Application for the
   authoritative `SpaceRef`, recipient-audience snapshot, and durable thread
   mapping. It may create a mapping only through the registered Space lifecycle
   path; it must never invent a parallel thread, memory namespace, task queue,
   or workspace from a provider conversation ID.
3. **Every ingress carries a signed, replay-safe envelope.** At minimum:
   `provider`, `provider_event_id`, `source_identity_ref`, `source_link_revision`,
   `space_ref`, `recipient_audience_ref/hash/revision`, `thread_ref`,
   `occurred_at`, `payload_digest`, `idempotency_key`, and `service_audience`.
   Content is separately minimized/screened and is omitted for ZDR surfaces.
   The adapter verifies provider signatures before resolving any mapping and
   atomically records the provider event id before forwarding work.
4. **Owner planes authorize effects again.** A mapped Space is necessary but
   never sufficient for a document, ticket, connector, or browser action. The
   owning plane checks its current resource decision; Model obtains a fresh
   execution/approval decision before an effect. A revoked identity, audience,
   membership, entitlement, resource, privacy, or ZDR revision invalidates
   queued/resumed work.
5. **Application owns delivery targets and receipts.** A `DeliveryTarget` is
   bound to a Space plus recipient-audience revision, preferences, quiet-hours,
   provider endpoint/ref, and retention posture. Model emits a non-content
   completion intent with an idempotency key; Application's durable outbox
   claims it and records `pending`, `claimed`, `sent_unconfirmed`,
   `acknowledged`, `failed`, or `unknown`. A provider callback/reconciliation is
   required before retrying an uncertain provider result. Processing is
   at-least-once; the Application activity/inbox projection is effectively once
   when its destination supports idempotency. No component promises exactly-once
   external delivery.
6. **The web surface is first.** V3 uses the same Space/thread/run/receipt
   identifiers rather than a web-only model. A future Slack adapter is an
   implementation of this contract, not an alternate source of identity,
   memory, or scheduling authority. No production call is made to Channel
   Plane until it has a deployed, reviewed runtime and the contract is adopted
   there.

## Required contracts and ownership

| Contract | Canonical owner | Consumers |
| --- | --- | --- |
| `ExternalIdentityLink` and link revision | Control | ingress adapter, Application |
| `SurfaceConversationMapping` and current recipient audience | Application | V3, ingress adapter, Model gateway |
| signed ingress envelope / provider verification | ingress adapter under Control policy | Application and Model gateway |
| per-resource authorization | resource-owning plane | action/retrieval/execution caller |
| `DeliveryTarget`, outbox, receipt, Activity projection | Application | Model completion publisher, V3 |

## Security and operational invariants

- A provider event is deduplicated by `(provider, provider_event_id)` before
  any downstream effect.
- External identity linkage is explicit; an email, display name, channel room
  name, or client header never substitutes for a Control link.
- All authorization comparisons include service audience, expiry, action/schema
  and payload binding, recipient audience, current revision(s), and the owning
  resource decision.
- Provider callbacks are authenticated and correlate a prior delivery attempt;
  unmatched callbacks change no user-visible state.
- ZDR forbids durable content in mapping, queue, delivery, activity, and
  provider payloads. Only strictly necessary non-content operational metadata
  may exist when policy allows it.
- A source or destination removal leaves a durable `unknown`/`failed` receipt
  for reconciliation; it is not silently retried or marked delivered.

## Adoption gates

Before enabling a non-web surface, add contract tests for signed ingress,
account-link takeover, replay dedupe, recipient/member/resource/privacy/ZDR
revocation, cross-surface thread resume, callback reconciliation, and
at-least-once delivery with an idempotent Activity projection. The first
adapter must not be implemented until these tests and a Control/Application
security review pass.

## Consequences

This keeps Channel Plane future scope rather than letting an adapter become an
undeclared identity or workspace authority. It also means the current V3
per-browser support-thread mapping and Model's org-wide cron/run paths cannot
be advertised as cross-surface continuity until they consume this contract.
