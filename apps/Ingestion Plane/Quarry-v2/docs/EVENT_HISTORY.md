# Durable Event History

Cycle 24 / cluster #7.

## The problem

Pre-cycle 24, every transport (SSE, NATS, webhooks, future GraphQL)
re-modeled "run progress" with slightly different field names.
Frontends simulated phases locally because the wire didn't carry
`completed/total/discovered` consistently. After a WebSocket
reconnect, the user saw a flat re-render instead of the actual run
state.

Cluster #7's acceptance criterion:
- Frontend doesn't simulate phases.
- Progress is visible after reconnect.
- One event model serves all transports.

## The canonical envelope

```rust
pub struct JobHistoryEvent {
    pub run_id:     RunKind,
    pub org_id:     String,
    pub kind:       JobResourceKind,   // crawl | search | extract | ...
    pub stage:      JobStage,          // queued | starting | running |
                                       // finalizing | completed | failed | cancelled
    pub status:     JobStatus,         // ok | warn | error
    pub seq:        u64,               // monotonic per run_id
    pub completed:  u32,
    pub total:      Option<u32>,
    pub discovered: u32,
    pub queued:     u32,
    pub retries:    u32,
    pub blocks:     u32,
    pub eta:        Option<DateTime<Utc>>,
    pub timestamp:  DateTime<Utc>,
    pub payload:    serde_json::Value, // free-form per-stage extras
}
```

Lives in `quarry_core::job_history`. Snake-case JSON, pinned by
test against accidental field renames.

## Subject taxonomy

Producers MUST use the helper functions — never invent the string
ad-hoc.

| Transport | Helper                                  | Example                                |
| --------- | --------------------------------------- | -------------------------------------- |
| NATS      | `nats_subject(kind, stage)`             | `quarry.jobs.crawl.running`            |
| Webhook   | `webhook_subject(kind, stage)`          | `quarry.webhook.search.completed`      |
| SSE       | `sse_event_name(stage)`                 | `running` (event field)                |
| GraphQL   | `graphql_subscription_field(kind)`      | `crawlJobProgress` (subscription name) |

Wildcards work at every NATS level:
- `quarry.jobs.crawl.>` — every crawl event
- `quarry.jobs.*.failed` — every kind's failures
- `quarry.jobs.>` — everything

## Durable storage

`PostgresEventHistory` (gated `postgres-queue`) backs three methods:

| Method                  | Use case                                          |
| ----------------------- | ------------------------------------------------- |
| `record(event)`         | Producer appends one event                          |
| `list_events(org, run, limit)` | Reconnect: fetch full run history                 |
| `replay_window(org, from, to, limit)` | Audit / dashboard rebuild over a time window |

Schema: `crates/quarry-runtime/migrations/0003_job_history.sql`.

- `(run_id, seq)` PK — duplicate emits return `BadRequest` so a
  buggy producer is caught immediately rather than corrupting the
  per-run total order.
- `(org_id, ts DESC)` index — replay window seek is O(log N).
- `(org_id, stage, ts DESC)` index — dashboard "current failing
  runs" query.
- `org_id <> ''` check constraint — every event MUST tag its tenant.

## Tenant isolation

Every method takes `org_id` as an explicit parameter. The
`quarry_job_history_org_ts_idx` index leads with `org_id` so
cross-tenant scans are statically impossible. Tested via
`tenant_isolation_cross_org_list_returns_empty`.

## Producer contract

When emitting an event:
1. Assign `seq = previous_seq + 1` for this `run_id`. Producers
   MUST NOT skip values.
2. Set `stage` to the current run phase.
3. Set `status` to `ok` for progress ticks, `warn` for non-fatal
   issues, `error` for the failing event.
4. Populate `completed/total/discovered/queued/retries/blocks` from
   the run's current counters.
5. `payload` carries free-form per-stage detail (URL just fetched,
   error message, fingerprint, etc.).
6. Call `PostgresEventHistory::record` AND publish via the matching
   transport subject (NATS, webhook, SSE).

## Consumer contract

Frontends + SDK consumers:
1. Subscribe via the transport-appropriate subject.
2. Gap detection: track the highest `seq` per `run_id`. On a gap
   (e.g. seq=5 → seq=8), call `list_events` to fetch the missing
   range.
3. Reconnect: call `list_events` for the run, then resume the live
   subscription.
4. Terminal stages (`completed`, `failed`, `cancelled`) stop the
   subscription loop.

## What's pending

- **`/v1/runs/:id/events` edge route** — currently the durable log
  is only readable via direct Postgres access. Cycle 25 adds the
  REST endpoint that proxies `PostgresEventHistory.list_events`.
- **Producer wiring in PageRunner** — `PageRunner` still uses the
  legacy `EventType` enum via `EventSink::emit`. Cycle 25 layers
  the new `JobHistoryEvent` emit alongside it (both during the
  rollout window).
- **`replay_job_event_window` SSE catch-up** — connection
  re-establishes should auto-replay since the last `seq` the
  client acked. Cycle 25 wires this on the `/v1/scrape/stream`
  reconnect path.

## Tests

| Test                                              | Coverage                                  |
| ------------------------------------------------- | ----------------------------------------- |
| `job_stage_serializes_snake_case`                 | Wire shape pin                            |
| `job_status_serializes_snake_case`                | Wire shape pin                            |
| `job_stage_is_terminal_helper`                    | Terminal-stage classification              |
| `nats_subject_canonical_shape`                    | NATS subject format                       |
| `webhook_subject_uses_webhook_prefix`             | Webhook subject format                    |
| `sse_event_name_is_just_stage`                    | SSE event-name format                     |
| `graphql_subscription_field_pattern`              | GraphQL field name format                 |
| `job_history_event_json_pins_field_names`         | Every field name + omission rules          |
| `job_history_event_roundtrips_through_json`       | Encode/decode roundtrip                   |
| `record_and_list_preserves_order` (pg-required)   | Append-then-list sequence ordering        |
| `duplicate_seq_returns_bad_request` (pg-required) | Per-run total order invariant             |
| `tenant_isolation_cross_org_list_returns_empty` (pg-required) | Cross-tenant leakage check |
| `replay_window_filters_by_timestamp` (pg-required) | Time-range replay correctness            |
| `replay_window_rejects_inverted_range` (pg-required) | Input validation                       |
