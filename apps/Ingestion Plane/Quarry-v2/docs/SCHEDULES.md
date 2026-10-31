# Schedules: lifecycle, tenancy and creation

Cycle 23 / cluster #5 (lifecycle), WS15.S1 (tenant-safe persistence and creation).

## Why

Operators must not need direct Temporal access for schedule work. The edge
exposes the lifecycle verbs below; control owns the rows and the Temporal client.

## Endpoints

| Method   | Path                           | Effect                                                         |
| -------- | ------------------------------ | -------------------------------------------------------------- |
| `GET`    | `/v1/schedules`                | The caller's schedules, keyset-paged, newest first             |
| `POST`   | `/v1/schedules`                | Create an executable schedule or a change monitor              |
| `POST`   | `/v1/schedules/:id/pause`      | Disable: stops firing, keeps the definition                    |
| `POST`   | `/v1/schedules/:id/unpause`    | Enable again                                                   |
| `POST`   | `/v1/schedules/:id/trigger`    | Fire once now (control answers 501 until a Temporal client is wired) |
| `POST`   | `/v1/schedules/:id/backfill`   | Replay a time range (control answers 501 until a Temporal client is wired) |
| `DELETE` | `/v1/schedules/:id`            | Remove the row. The orchestrator reaps the Temporal schedule on its next pass |

Every route takes the org from the edge-verified JWT. The edge sends it to
control as the `org_id` query parameter and never reads it from a body.

## Tenancy (WS15.S1)

- A row belongs to one org. A request for another org's row answers `404`, the
  same answer as a missing row. This covers read, delete, pause, unpause,
  enable, disable, trigger, backfill and the run history.
- Control's `GET /v1/schedules` requires `org_id`. Without it, the legacy
  unscoped list still answers (with every org's rows, in the orchestrator's
  wire shape) and logs a deprecation. Set `QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG=1`
  to make it a `400`. Remove the legacy path once the orchestrator reads
  `GET /v1/internal/schedules/desired` (all orgs, internal path only).
- A body `org_id` is ignored by control.

## Create

```json
{
  "name": "Nightly docs crawl",
  "kind": "crawl",
  "cron": "0 7 * * *",
  "timezone": "Europe/Oslo",
  "overlap_policy": "skip",
  "catchup_window_s": 0,
  "pause_on_failure": false,
  "config": { "url": "https://example.com/docs", "max_pages": 20 },
  "activate": false
}
```

- `kind` is `scrape`, `crawl` or `batch`. Any other kind is a `400` with
  `error.details.reason = "unsupported_schedule_kind"`.
- `cron` is exactly five fields. A six-field cron is a `400` (`invalid_cron`).
- `schedule_at` (one-shot) is not supported yet: `400`, `unsupported_schedule_at`.
- `config.url` (scrape, crawl) or `config.urls` (batch, 1 to 200 URLs) must be
  http or https with no credentials. Otherwise `400`, `invalid_config`.
- `max_pages` is 1 to 5000 and `max_depth` 0 to 10 when present.
- `timezone` is an IANA zone. `Local` and unknown names are `400`,
  `invalid_timezone`.
- `overlap_policy` is `skip`, `allow` or `cancel`. `catchup_window_s` is 0 to 86400.
- The cadence floor is 5 minutes for scrape and 15 minutes for crawl and batch,
  measured by `cron.MinInterval` (smallest gap over three sample instants). A
  faster cron is `400`, `cadence_too_frequent`. `QUARRY_SCHEDULE_MIN_INTERVAL_S`
  overrides the floor for every kind.

Refusals carry `error.details.reason`. Control's own refusals (for example
`invalid_timezone`) are passed through with the same field.

### Activation

A new executable schedule starts **disabled**. The response has
`enabled: false`, `status: "paused"` and `needs_activation: true`. A user token
may send `activate: true` to start it at once. The edge ignores `activate` from
a service principal. The body's `enabled` field is not read.

A change monitor (`preset` + `target_ref`) starts when created, as before. Its
posture is stamped (WS4.H). See `RUNBOOKS.md`.

### Response shape

The view carries `schedule_id`, `name`, `kind`, `cron`, `timezone`,
`overlap_policy`, `catchup_window_s`, `pause_on_failure`, `jitter_s`, `status`
(`active` or `paused`), `needs_activation`, `config`, RFC 3339 `created_at`,
`updated_at`, `last_run_at`, `next_run_at`, and the legacy `id`, `target_kind`,
`target_ref`, `enabled`. The create answers `200` through the edge (the WS4.H
tests pin that status).

## Not yet

- Nothing fires. The executor (WS15.S2) reads these rows, behind
  `QUARRY_SCHEDULED_JOBS_ENABLED`.
- `schedule_at`, `trigger` and `backfill` are unavailable.
- An invalid `overlap_policy` value is rejected by the edge's JSON decoder
  (`422`), before control sees it.

## State

```
POST /v1/schedules (activate:false) ──► paused (needs_activation: true)
                                          │  POST /:id/unpause or /enable
                                          ▼
                                       active ◄──── POST /:id/pause or /disable ──┐
                                          │                                       │
                                          └───────────────────────────────────────┘
DELETE /:id ──► row removed (hard delete)
```

Migration 017 leaves pre-existing scrape, crawl and batch rows disabled (they
never ran). It does not touch change monitors.

## Backfill

```rust
pub struct BackfillRequest {
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,           // strictly after start_at
    pub overlap_policy: OverlapPolicy,
}
```

Control answers `501` until a Temporal client is configured.

## Idempotency

The edge sends an `Idempotency-Key` on every mutating call. Control does not yet
dedupe creates by `(org_id, name)`, so a retried create can produce a second row.

## Tests

- Control (`services/quarry-control/internal/resources/schedules_test.go`,
  `schedules_zdr_test.go`, `cycle23_aliases_test.go`; `store/pg/schedules_017_test.go`
  against `QUARRY_TEST_DSN`; `cron/interval_test.go`).
- Edge (`crates/quarry-edge/tests/schedule_create.rs`; in-file unit tests in
  `src/schedule_routes.rs`; the WS4.H posture tests in `tests/zdr_posture_stamp.rs`).
- Core (`crates/quarry-core/src/resources.rs`, `schedule_summary_json_wire_shape`).
- Gateway (`apps/gateway/src/domains/ingestions/schedules.rs`).

These were written in the WS15.S1 build and have not been run yet.
