# Schedules — Lifecycle Convergence

Cycle 23 / cluster #5.

## Why

Pre-cycle-23, operators had to use `temporal` CLI (or write Go code)
to pause / unpause / trigger / backfill a scheduled crawl. That's a
high-cost dependency on a vendor-specific tool. Cluster #5's
acceptance criterion was "Operators never need direct Temporal access
for schedule work" — the lifecycle endpoints below close that gap.

## Endpoints

| Method | Path                                | Effect                                                  |
| ------ | ----------------------------------- | ------------------------------------------------------- |
| `GET`  | `/v1/schedules`                     | Paginated list, org-scoped                              |
| `POST` | `/v1/schedules`                     | Create (cron OR `schedule_at` one-shot)                  |
| `POST` | `/v1/schedules/:id/pause`           | Pause — stops firing; preserves config                   |
| `POST` | `/v1/schedules/:id/unpause`         | Resume after pause                                       |
| `POST` | `/v1/schedules/:id/trigger`         | Fire once manually, immediately                          |
| `POST` | `/v1/schedules/:id/backfill`        | Run for a specific time-range window                     |
| `DELETE` | `/v1/schedules/:id`               | Soft-delete (Temporal stops; row kept for audit)         |

All routes:
- Require auth (JWT middleware applies to every `/v1/*`).
- `org_id` is the verified claim — query-string overrides are ignored.
- Forward to Go control plane (Temporal client lives there).
- 502/500 mapped to `DriverFailed`; `404` to `NotFound`; `400` to
  `BadRequest`; `401/403` to `Forbidden`.

## State machine

```
                    POST /v1/schedules
                              │
                              ▼
                       ┌─────────────┐
            ┌─────────►│   active    │
            │          └─────┬───────┘
            │                │  POST /:id/pause
            │                ▼
  POST /:id/unpause     ┌─────────┐
            ◄───────────┤ paused  │
                        └─────────┘
                              │
                              ▼  DELETE /:id
                       ┌─────────────┐
                       │   deleted   │  (terminal, soft)
                       └─────────────┘
```

`trigger` and `backfill` don't change schedule state — they fire
runs against the current state.

## Create-request shape

```rust
pub struct CreateScheduleRequest {
    pub name: String,
    pub kind: JobResourceKind,          // crawl | search | extract | research | agent | batch
    pub cron: Option<String>,           // EXACTLY one of cron / schedule_at
    pub schedule_at: Option<DateTime>,
    pub overlap_policy: OverlapPolicy,   // skip (default) | cancel | allow
    pub catchup_window_s: u64,           // 0 disables catchup
    pub pause_on_failure: bool,
    pub config: serde_json::Value,       // per-kind payload (URL, query, etc.)
}
```

Validation at the edge:
- Exactly one of `cron` / `schedule_at` must be set (BadRequest).
- `cron` must have ≥ 5 whitespace-separated fields (BadRequest).
- All deeper cron validation happens in the control plane (vendor
  parser quirks).

## Backfill

```rust
pub struct BackfillRequest {
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,           // strictly after start_at
    pub overlap_policy: OverlapPolicy,
}
```

A backfill replays the schedule against every firing time between
`start_at` and `end_at`. Common pattern: after a long outage, set
`overlap_policy: allow` so the catchup work parallelises.

## OverlapPolicy

- `skip` (default) — if a previous run is still in flight when the
  next firing time arrives, skip the new run.
- `cancel` — cancel the in-flight run and start the new one.
- `allow` — let both run concurrently. Most workloads don't tolerate
  this; use sparingly.

## Idempotency

Backend (control plane) MUST dedupe creates by `(org_id, name)` so
retrying a 5xx network error doesn't produce two identical schedules.
The edge does not yet emit an idempotency-key header — that's wired
in cluster #14 alongside HMAC.

## Tenant isolation

- Verified `org_id` from JWT claim is the only org gate.
- Schedule IDs are globally unique ULIDs (`sch_...`) so cross-tenant
  ID-guessing is impractical, but the control plane also enforces
  `WHERE org_id = $verified` on every lookup.

## Tests

- 2 unit tests in `crates/quarry-edge/src/schedule_routes.rs` pin
  request body shape (`create_schedule_request_requires_cron_xor_schedule_at_check_pattern`, `backfill_request_serializes_with_required_fields`).
- 5 resource shape tests in `crates/quarry-core/src/resources.rs`
  pin `ScheduleSummary`, `OverlapPolicy`, and team shapes.
- Integration tests against a live Go control plane are deferred to
  cycle 28's benchmark harness.
