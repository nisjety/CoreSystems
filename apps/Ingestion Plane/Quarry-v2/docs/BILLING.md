# Billing — Usage Metering Contract

P3 / cluster #billing (shipped after P2).

Quarry emits per-request usage events to Control Plane's `billing-core`,
which dedupes them and forwards to Lago for invoicing.

## Wire shape

Payload exactly mirrors `billing-core`'s `UsageEvent` Go struct
(`apps/Control Plane/billing-core/internal/billing/types.go:43`):

```jsonc
{
  "event_id":   "quarry:<request_id>:<metric>",  // primary dedup key
  "org_id":     "org_01H...",                    // verified JWT claim
  "metric":     "quarry.scrape.page",            // free-form, matches Lago
  "quantity":   1.0,                             // f64; semantics owned by Lago
  "source":     "quarry-edge",
  "occurred_at":"2026-05-19T12:34:56Z",          // RFC3339 UTC
  "metadata":   { "user_id": "usr_...", ... }    // free-form context
}
```

## Transport

**NATS JetStream**, subject filter `usage.>`. Quarry publishes to
`usage.<metric>` so each metric gets its own subject and consumers can
filter granularly:

| Subject                       | Producer | Consumer            |
| ----------------------------- | -------- | ------------------- |
| `usage.quarry.scrape.page`    | quarry-edge | billing-core      |
| `usage.quarry.crawl.seed`     | quarry-edge | billing-core      |
| `usage.quarry.search.query`   | quarry-edge | billing-core      |
| `usage.quarry.answer.synth`   | quarry-edge | billing-core      |
| `usage.quarry.batch.url`      | quarry-edge | billing-core      |

billing-core's JetStream stream `CONTROL_PLANE_EVENTS` covers
`["user.>", "organization.>", "session.>", "billing.>", "usage.>"]`,
so the subjects are routable end-to-end without extra config.

**Recovery path:** `POST /api/v1/billing/orgs/:orgId/usage` on
billing-core accepts the same JSON shape over HTTP if NATS is unhealthy.

## Metric taxonomy

| Constant                            | String                       | When emitted                                     | Quantity                |
| ----------------------------------- | ---------------------------- | ------------------------------------------------ | ----------------------- |
| `usage_metrics::SCRAPE_PAGE`        | `quarry.scrape.page`         | After successful `/v1/scrape` (skipped on error) | `1.0` per page          |
| `usage_metrics::CRAWL_SEED`         | `quarry.crawl.seed`          | On every `/v1/crawl` handoff to orchestrator     | `1.0` per seed URL      |
| `usage_metrics::SEARCH_QUERY`       | `quarry.search.query`        | On every successful `/v1/search`                 | `1.0` per query         |
| `usage_metrics::ANSWER_SYNTH`       | `quarry.answer.synth`        | On every successful `/v1/answer`                 | `1.0` per synthesis     |
| `usage_metrics::BATCH_URL`          | `quarry.batch.url`           | On every `/v1/batch` handoff                     | `len(urls) as f64`      |

**Pricing model:** these are free-form strings — Lago billable-metric
codes must match exactly. To add a new metric:

1. Define it in `crates/quarry-runtime/src/usage.rs::metrics`
2. Register a matching billable metric in Lago via the billing-core admin API
3. Emit from the handler

## Idempotency

billing-core dedupes by `event_id` in the `billing_usage_dedup` table.
Quarry constructs IDs as `quarry:<request_id>:<metric>` so a single
request that emits multiple distinct metric kinds won't collide with
itself. NATS-level redelivery is automatically de-duplicated.

## Tenant attribution

- `org_id` is **always** the verified JWT claim — never the
  client-supplied body field. P0 / cluster #auth+tenancy enforces this
  upstream by overwriting `req.org_id` with `claims.org_id` in every
  handler.
- `user_id` rides in `metadata.user_id` so billing-core can attribute
  cost back to the individual user when needed (Lago doesn't natively
  model per-user inside an org).

## Failure mode

| Failure                              | Behavior                                                  |
| ------------------------------------ | --------------------------------------------------------- |
| NATS broker down                     | Publish error logged; user request **succeeds** anyway     |
| Serialize error                      | warn-logged; user request succeeds                         |
| `NoopUsageMeter` (no NATS configured)| Silently discards; user request succeeds                   |
| billing-core temporarily down        | JetStream buffers (7-day retention); replays on recover    |

**Usage metering is never on the request critical path.** Every
`meter()` call `tokio::spawn`s the publish so the handler returns
immediately.

## Configuration

The meter is wired automatically in `main.rs`:

- `QUARRY_EDGE__NATS_URL` set + reachable → `NatsUsageMeter`
- Otherwise → `NoopUsageMeter` (warning log on boot)

No separate billing env vars — the meter rides on the same NATS
connection used for cross-plane event fan-out (P1 / cluster #nats).

## Tests

- `crates/quarry-runtime/src/usage.rs` — 5 unit tests:
  - `event_id_format_is_quarry_request_metric`
  - `serializes_to_billing_core_wire_shape` (pins JSON field names)
  - `subject_derivation_matches_usage_wildcard`
  - `noop_meter_silently_discards`
  - `recording_meter_captures_payload`

A `RecordingMeter` fixture is exposed at `crate::tests::RecordingMeter`
for downstream handler-level integration tests that want to assert the
emitted shape without spinning up NATS.

## Future work (not P3 scope)

- **Per-MP-token cost** — meter Model Plane spend on `/v1/answer` via a
  follow-up `quarry.answer.tokens.<model>` metric with quantity = total
  input+output tokens. Requires MP gateway to surface a token count
  per invoke (today the response carries only `model_used` + `content`).
- **Storage-day metric** — emit `quarry.storage.day` once per artifact
  per 24h for stored scrape outputs. Requires a background sweeper job.
- **Per-search SERP-provider cost** — split `search.query` into
  `search.query.brave` / `search.query.serper` so paid providers can
  attribute their unit cost to the right tenant.
