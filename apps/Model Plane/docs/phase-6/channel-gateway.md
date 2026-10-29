# Channel / Gateway

## Product role

Adapts external messaging channels (Slack, Teams, Discord, email, SMS,
webhooks) into Model Plane sessions. One inbound channel event becomes one or
more session turns; one outbound session event becomes one or more channel
messages.

## Transport

- **Inbound**: channel provider → HTTPS webhook on `model-gateway`.
- **Outbound**: `model-gateway` → channel provider REST/WS.
- **Auth**: per-channel signing secret verified at `model-gateway`.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Webhook signature verification, idempotency | `model-gateway` |
| Channel → session mapping, threading | `execution-core` |
| Rate-limit / backpressure per channel | `model-gateway` |
| Catalog of which models a channel may use | `capability-core` |

## `capability-core` responsibilities

- **Catalog**: channel-class allowlist (e.g. “Slack enterprise” may use
  higher-cost tiers; public email may not).
- **Policy**: redaction before egress, link-unfurl rules, attachment
  allow/deny.
- **Metadata**: per-channel display name, branding, signature footer.
- **Scheduling hints**: interactive priority for user-initiated channels vs.
  batch priority for webhook fan-in.

## Idempotency

All inbound channel events carry a provider-supplied idempotency key. The
gateway persists it with a TTL ≥ the channel’s retry horizon. Phase 3 idempotency
machinery is reused.

## Reference inputs

- `hermes-agent` — channel adapter patterns.

## Out of scope

- Channel-specific rich-UI widgets (Slack Block Kit, Teams Adaptive Cards) —
  rendered by per-channel adapters in a later phase.
