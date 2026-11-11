# Schedules: kinds, fire, overlap, trigger and backfill, fire-time posture

Cycle 23 / cluster #5 (lifecycle). WS15.S1 creates the rows, WS15.S2 fires them, WS15.S3 queues trigger and backfill, and WS15.S4a checks each fire. This page describes what the code does today. Every switch named here is off, or `observe`, by default. The go-live gates and the explicit not-done list are in `RUNBOOKS.md`, section "Schedules go-live". Nothing here has run on a live stack (see Tests).

## Why

Operators and tenants must not need direct Temporal access for schedule work. The edge exposes the lifecycle verbs below. Control owns the rows and the run history. The orchestrator owns the Temporal schedules and the fire. No plane reads another plane's database.

| Piece | Owns | Reads or calls |
| --- | --- | --- |
| quarry-edge | Auth and the org binding, the ZDR refusal (observe by default), the fire-time preflight | Forwards each verb to control. Reads org-core's posture once per fire |
| quarry-control | `schedules`, `schedule_requests`, the job each fire writes, `schedule_skipped` events, the ZDR-event consumer | Has no Temporal client. The orchestrator reads its desired set and claims its requests |
| quarry-orchestrator | The Temporal schedules (reconciled every 30 s), `ScheduledJobWF`, the job workflows, the request loop | Control, and the edge's preflight |

## Endpoints

Through the edge, with a user token. The org comes from the verified JWT, never from the body.

| Method | Path | Effect |
| --- | --- | --- |
| `GET` | `/v1/schedules` | The caller's schedules, keyset-paged |
| `POST` | `/v1/schedules` | Create an executable schedule (`kind`, `cron`) or a change monitor (`preset`) |
| `POST` | `/v1/schedules/:id/pause` | `enabled = false`. The Temporal schedule pauses on the next reconcile pass (about 30 s). The row stays |
| `POST` | `/v1/schedules/:id/unpause` | `enabled = true`. The Temporal schedule unpauses on the next pass |
| `POST` | `/v1/schedules/:id/trigger` | Fire once now. Control answers `501` unless `QUARRY_CONTROL__SCHEDULE_REQUESTS` is on; then it answers `202` with a request row (see Trigger and backfill) |
| `POST` | `/v1/schedules/:id/backfill` | Replay a past range of a scrape schedule. Same flag and rules as trigger |
| `GET` | `/v1/schedules/:id/runs` | The fires of one schedule, newest first, keyset-paged (`limit`, `cursor`). Control's job rows (WS15.S3a) |
| `GET` | `/v1/schedules/:id/requests/:rid` | One trigger or backfill request row: status, attempts, result, short error (WS15.S3a) |
| `DELETE` | `/v1/schedules/:id` | Remove the row. The orchestrator deletes the Temporal schedule on its next pass, unless it was the org's last row (Known gaps, item 10) |

The edge also serves `POST /v1/internal/schedules/preflight` for the orchestrator (runtime token and run binding). Clients never call it.

Control's own routes take `org_id` in the query and need an HMAC signature from the edge or the orchestrator. They are the same verbs, plus `enable`, `disable` and `GET /v1/schedules/{id}`. The edge has no route for those three yet. The fires of a schedule and its request rows are on the edge too (WS15.S3a), so operators go through the edge.

Refusals: control's refusals carry `error.details.reason`, and the edge keeps that field whatever the status. The edge maps 400 to `BAD_REQUEST`, 401 and 403 to `FORBIDDEN`, 404 to `NOT_FOUND`, 409 to `CONFLICT` and 501 to `UNSUPPORTED`. Any other control status reaches the client as `502 DRIVER_FAILED`.

## Tenancy

- A row belongs to one org. Another org's row is `404`, the same answer as a missing row. This covers read, delete, pause, unpause, enable, disable, trigger, backfill, the run history and the request read.
- Control's `GET /v1/schedules` requires `org_id`. Without it the answer is `400`, always. The legacy all-orgs list is gone (D2-042), and `QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG` changes nothing.
- The orchestrator's all-orgs set is `GET /v1/internal/schedules/desired` (internal path only). It returns every row, paused or not.
- A body `org_id` is ignored by the edge and by control.

## Kinds

| Kind | How it is created | Config | Cadence floor | What a fire runs | Backfill |
| --- | --- | --- | --- | --- | --- |
| `scrape` | `kind: "scrape"` | `config.url` | 5 min | `ScheduledJobWF`, then `ScrapeJobWF` | Allowed (flag on) |
| `crawl` | `kind: "crawl"` | `config.url`; `max_pages` 1 to 5000 and `max_depth` 0 to 10 when present | 15 min | `ScheduledJobWF`, then `CrawlJobWF` | Refused: `backfill_kind_refused` |
| `batch` | `kind: "batch"` | `config.urls`, 1 to 200 http(s) URLs; the first one becomes `target_ref` | 15 min | `ScheduledJobWF`, then `BatchJobWF` | Refused |
| change monitor | `preset` (`hourly`, `daily`, `weekly`) and `target_ref` (or `config.url`) | none | Fixed by the preset | `ChangeMonitorWF` | Refused |

- Scrape, crawl and batch fires exist in Temporal only while `QUARRY_SCHEDULED_JOBS_ENABLED=1` on control. A change monitor is not behind that switch.
- A preset fixes the cron in UTC: `0 * * * *`, `0 9 * * *` and `0 9 * * 1`.

Checks on an executable create (control's validation; the edge checks the same shapes first):

- `cron` is exactly five fields (`invalid_cron`). The cadence floor is the smallest gap of the cron over three sample instants (`cadence_too_frequent` below it). `QUARRY_SCHEDULE_MIN_INTERVAL_S` on control replaces the floor for every kind.
- `timezone` is an IANA name, default `UTC`. `Local` and unknown names are `invalid_timezone`.
- `overlap_policy` is `skip` (default), `allow` or `cancel`. The edge's JSON decoder refuses any other string before control sees it (axum answers 422).
- `catchup_window_s` is 0 to 86400. Zero keeps Temporal's default catch-up window.
- `jitter_s` is 0 to 3600 in control, but neither the edge body nor the edge's forward to control carries it. An API-created schedule therefore always has jitter 0.
- `pause_on_failure` (bool) pauses the schedule when one of its fires fails.
- `config.url` and `config.urls` must be http or https, with a host and no credentials (`invalid_config`).
- `name` is a label. The view shows the target when it is empty.
- `schedule_at` (one-shot) answers `400 unsupported_schedule_at`. A `kind` other than the three executable kinds answers `unsupported_schedule_kind`. A monitor without a known preset answers `invalid_preset`, and one without a target answers `target_ref_required`.

## Activation

- An executable schedule starts disabled: `enabled: false`, `status: "paused"`, `needs_activation: true`. A user token may send `activate: true` to start it. The edge ignores `activate` from a service principal. The body's `enabled` is not read.
- A change monitor starts when it is created. The edge sends `activate: true` for it.
- Migration 017 left the pre-existing scrape, crawl and batch rows disabled, because they never ran. It does not touch change monitors. An owner re-enables each one with `POST /v1/schedules/:id/enable`, which stamps `updated_at`.
- `needs_activation` is true for a disabled non-monitor row whose `updated_at` still equals `created_at`.

## Response shape

The view carries `schedule_id`, `name`, `kind`, `cron`, `timezone`, `overlap_policy`, `catchup_window_s`, `pause_on_failure`, `jitter_s`, `status` (`active` or `paused`), `needs_activation`, `config`, RFC 3339 `created_at`, `updated_at`, `last_run_at`, `next_run_at`, `skipped_overlap_count`, and the legacy `id`, `target_kind`, `target_ref` and `enabled`. Control answers `201` and the edge answers `200`. `last_run_at`, `next_run_at` and `skipped_overlap_count` are not written by anything yet (Known gaps, item 4).

## Fire: scrape, crawl and batch

The switch is `QUARRY_SCHEDULED_JOBS_ENABLED` on control, default `0` (compose sets it). Control reads it when it builds the desired set, so a change lands on the next reconcile pass. Off: these kinds are sent without a workflow, so the orchestrator creates nothing for them, and an existing Temporal schedule for one is paused, not deleted. On: the kinds are mapped and their Temporal schedules are created.

Each fire runs the wrapper `ScheduledJobWF`, in this order:

1. **Preflight.** The orchestrator calls `POST /v1/internal/schedules/preflight` on the edge, with the runtime bearer token and `X-Quarry-Run-Sig` over `org_id` and `schedule:<id>`. Only network errors, timeouts and 5xx answers are retried, under the default activity policy (5 attempts, 1 s to 1 min back-off). If the call still fails, the fire ends with an error before any job exists, and `pause_on_failure` sees it.
2. **Denied.** On `allow: false` the wrapper registers a terminal job: status `skipped`, `terminal_reason: schedule_skipped`, and one `schedule_skipped` event carrying the reason. The fire returns no error, so `pause_on_failure` does not count it.
3. **Allowed.** The wrapper registers a running job with origin `{type: "schedule", actor: <schedule id>}`. Control dedupes that registration on `sched:<id>:<run key>`, where the run key is the fire's run id.
4. **Child.** The matching job workflow (`ScrapeJobWF`, `CrawlJobWF` or `BatchJobWF`) runs as `quarry-job-<jobID>`, with the same input builders the dispatcher uses.
5. **Failure.** A child error writes `run_failed` with reason `child_failed`, or `run_timed_out` with `child_timed_out` for a Temporal timeout. A child that cannot start writes `run_failed` with `invalid_params`. The payload carries `schedule_id`, `org_id`, `created_by` and the reason, never a URL. The error is returned, so `pause_on_failure` sees it.

The reconciler writes three values into every fire's args: the crawl `max_pages` and `max_depth`, bounded by the `QUARRY_CRAWL_DEFAULT_*` and `QUARRY_CRAWL_HARD_*` limits; the execution timeout, from `QUARRY_JOB_TIMEOUT_SCRAPE_S`, `QUARRY_JOB_TIMEOUT_CRAWL_S` or `QUARRY_JOB_TIMEOUT_BATCH_S`; and the overlap policy. A change in any of them is a spec change, applied on the next pass. The reconciler never deletes a schedule without an org memo, and never reaps across orgs.

## Overlap

The stored `overlap_policy` becomes the Temporal schedule's overlap policy:

| Stored | Temporal | A fire that comes while the last one still runs |
| --- | --- | --- |
| `skip` (default) | `SKIP` | Temporal drops it. No job and no event. Temporal's own schedule info counts the drops; the row's `skipped_overlap_count` is never written |
| `allow` | `ALLOW_ALL` | It runs alongside the first |
| `cancel` | `CANCEL_OTHER` | Temporal cancels the running wrapper. The wrapper signals its child to cancel, on a context that survives the cancellation, and waits at most 2 minutes for the child to stop. The fire ends with no failure event |

- A trigger request runs with `SKIP`. When a fire is still running, Temporal starts nothing, and the request still reads `done` (Known gaps, item 5).
- A backfill runs with `ALLOW_ALL` whatever overlap the request carried. The request's `overlap_policy` is stored, but it is not passed on (Known gaps, item 2).

## Trigger and backfill (WS15.S3)

Off by default. With `QUARRY_CONTROL__SCHEDULE_REQUESTS` unset, control answers `501 UNSUPPORTED` and mounts no request route. Through the edge that is `501 UNSUPPORTED` too.

With the flag on, trigger and backfill do not call Temporal. Control inserts a `schedule_requests` row and answers `202 {request_id, schedule_id, action, status: "pending"}`. The edge passes that `202` through. The orchestrator then applies the row, if its own switch `QUARRY_SCHEDULE_REQUESTS` is on (default off).

Checks, in order. The first refusal wins.

| Check | Answer |
| --- | --- |
| Backfill only: `end_at` after `start_at` | `400` |
| Backfill only: `end_at` at most one minute ahead of now | `400`, `backfill_window_in_future` |
| Backfill only: `overlap_policy` is `skip`, `allow` or `cancel` when given | `400` |
| The schedule belongs to the caller's org | `404` |
| The request store exists | `501` |
| The schedule is enabled | `409`, `needs_activation` |
| Scrape, crawl or batch: `QUARRY_SCHEDULED_JOBS_ENABLED=1` | `409`, `scheduled_jobs_disabled` |
| No open request for the schedule | `409`, `request_pending` |
| Backfill only: the kind is `scrape` | `409`, `backfill_kind_refused` |
| Backfill only: at most 24 windows, where windows = `ceil(span / the cron's smallest gap)` | `400`, `backfill_too_many_windows` |

- A schedule has at most one open request, a pending or claimed row. The database enforces it.
- `Idempotency-Key` (at most 128 characters) is stored. The edge forwards the caller's key in place of a minted one. An empty, longer or not visible ASCII value is ignored, as if no key were sent. The key is unique per org, so a repeated key is refused with `409 request_pending`. It is not answered with the first `202` (Known gaps, item 6).
- The orchestrator's loop claims up to 10 rows every 2 s, leases each one for 60 s, and calls Temporal with a 15 s timeout. Its outcomes:
  - Temporal accepts the call: `done`, with `{schedule_id, action, submitted_at}`. Temporal returns no workflow id for these calls.
  - The schedule is not in Temporal, or the stored params are bad or the action is unknown: `rejected`. Temporal is not called for bad params.
  - Any other error: `retry`. The row goes back to pending, up to attempt 3. Then `failed`.
  - A row whose worker dies is claimed again when its lease ends.
- Delivery is at-least-once. A worker that dies after Temporal accepted the call, but before it posted `done`, causes one more fire. Temporal's trigger has no idempotency key to stop it.
- A fire that a request starts is the schedule's own `ScheduledJobWF`, so it has the same preflight as a cron fire.
- The tenant reads a request at `GET /v1/schedules/{id}/requests/{rid}`, through the edge (WS15.S3a). The view shows the status, attempts, params, result and a short error text. It hides the claiming worker and the lease.

## Fire-time posture (WS15.S4a)

The edge checks every scrape, crawl and batch fire before it starts, including fires that a request started. Change monitors have no fire-time check yet. Their ZDR posture is stamped at create (WS4.H; see RUNBOOKS, "ZDR posture of a scheduled change monitor").

The switch is `QUARRY_EDGE__SCHEDULE_POSTURE` on the edge: `off`, `observe` or `enforce`. The default is `observe`. An unknown value logs a warning and means `observe`. The mode is read once at boot, so a change needs a restart.

- `off`: the S2 answer, `allow: true`, posture `unchecked`, entitlement `floor`. No reads.
- `observe` (default): every check runs and is logged. The fire is allowed. The answer carries `observed_reason`, the reason `enforce` would have used. Nothing is paused.
- `enforce`: a skip reason denies the fire (`allow: false`, with `reason`). `skipped_zdr` also asks control to pause the schedule, best effort and off the fire's path.

The checks run in this order, and the first reason wins:

| Reason | When | Note |
| --- | --- | --- |
| `skipped_disabled` | The caller sent `enabled: false` | Latent: the orchestrator sends no `enabled` |
| `skipped_kind_disabled` | The caller sent a kind outside scrape, crawl, batch and change_monitor | Latent: the orchestrator sends no kind |
| `skipped_zdr` | Effective ZDR is true: org-core's `zdrIntent` AND `entitled` | `GET /internal/orgs/:org/retention-posture`, 3 s timeout, one read per fire, never cached |
| `skipped_posture_unknown` | The read failed: a timeout, unreachable, 401, 403, 404, 5xx, a body that does not decode, or no URL or token configured | Fails closed under `enforce` |
| `skipped_no_entitlement` | The org's allowance for the kind is spent (crawl pages, batch URLs), or the limits resolver refused (fail mode closed) | Scrape has no metered allowance |

- The org-core URL is `QUARRY_EDGE__ORG_CORE_URL`, with compose default `http://org-core:8080`. The token is `QUARRY_EDGE__ORG_CORE_SERVICE_TOKEN`. Compose fills it from `QUARRY_ORG_CORE_SERVICE_TOKEN`, the quarry-edge credential minted in Control's secret ledger and copied into Ingestion's environment by hand.
- With the URL or the token unset, every posture read is `unknown`. Under `observe` the fire runs and the log says so. Under `enforce` every scheduled fire is skipped.
- Edge log lines: `schedule preflight: skip reason` (info, with `reason`, `mode`, `posture` and `entitlement`), `schedule preflight: posture unknown` (warn, with the closed `class`), and `schedule preflight: would_pause (observe)` (info).

## Skip and failure reasons

| Reason | Written by | Where it shows | Outcome |
| --- | --- | --- | --- |
| `skipped_zdr`, `skipped_posture_unknown`, `skipped_no_entitlement` | The edge's preflight, returned to the orchestrator, which writes the job | Job `skipped` with `terminal_reason: schedule_skipped`; event `schedule_skipped` | Fire skipped. Under `enforce`, `skipped_zdr` also pauses the schedule |
| `skipped_disabled`, `skipped_kind_disabled` | The edge's preflight | As above | Latent (see Fire-time posture) |
| `preflight_denied` | The orchestrator, when the edge denies without a reason | As above | Fire skipped |
| `skipped_zdr` | Control's ZDR-event consumer, under `enforce` | Event `schedule_skipped`, with no run and no job | Schedule paused. No fire |
| `child_failed`, `child_timed_out` | The orchestrator | `run_failed` or `run_timed_out` event | Fire failed. `pause_on_failure` sees it |
| `invalid_params` | The orchestrator, when the child cannot start | `run_failed` event | Fire failed |
| `skipped_overlap` | Nothing | Nothing | Vocabulary only. A Temporal `SKIP` is silent |
| `needs_activation`, `scheduled_jobs_disabled`, `request_pending`, `backfill_kind_refused`, `backfill_too_many_windows`, `backfill_window_in_future` | Control, on a trigger or backfill request | `error.details.reason` on the refusal | No row is written |

A skipped fire is not a failure, so it never trips `pause_on_failure`.

## ZDR and the X-9 consumer

- A change monitor keeps the ZDR stamp it was created with (`schedules.zdr`). Nothing re-checks that stamp.
- org-core publishes `controlplane.org.interactive_retention.enabled` when an org's effective ZDR turns on. That covers the explicit toggle, and a plan change once X-9's emit is wired. Control consumes the event when `QUARRY_CONTROL__ZDR_EVENT_CONSUMER` is `observe` or `enforce`; the default is `off`. `observe` logs `would_pause` for each enabled schedule and monitor of the org. `enforce` writes one `schedule_skipped` event (`skipped_zdr`) per schedule and pauses it.
- Nothing resumes automatically. Enforce does not restamp `schedules.zdr`, so a schedule that a user later unpauses keeps the old stamp (Known gaps, item 11).
- The consumer binds only when the audit-core consumer row and the NATS grants exist. org-core emits only when its two wiring lines are in. Both are in RUNBOOKS, "Schedules go-live", Not done.

## Known gaps

1. **Trigger and backfill are dark.** ~~With the flag on, the edge does not pass the 202 through (it answers 200 with the control body), has no request read route, and maps control's 409 and 501 to `502 DRIVER_FAILED`. The reason survives in `details.reason`; the status does not. Fix this before the control flag goes on.~~ The edge half is built (WS15.S3a): the edge passes the 202 through, serves the request read, and keeps control's 409 and 501 with their own codes and `details.reason`. The control flag stays off until its go-live.
2. **Backfill ignores the requested overlap policy.** The orchestrator hard-codes `ALLOW_ALL`.
3. **Backfill counts windows as an upper bound** for an irregular cron, because control has no next-fire iterator.
4. **Three row fields are never written.** `last_run_at`, `next_run_at` and `skipped_overlap_count` read `null`, `null` and `0`. The comment on the field says the executor writes them. Nothing calls the store method that would.
5. **A Temporal `SKIP` leaves no record.** A trigger that lands while a fire runs still reads `done`.
6. **Retries can duplicate.** A repeated `Idempotency-Key` on trigger or backfill is refused with `request_pending`, not answered with the first `202`. A trigger retried after the first one finished makes a second fire when the retry carries no key, or a different one. ~~The edge's `Idempotency-Key` is minted per forward, so it does not dedupe a client retry.~~ The edge forwards the caller's key (WS15.S3a), so a retry that repeats the key fires nothing new. Control dedupes no create, so a retried create can make a second row.
7. **The preflight is thinner than its design.** The orchestrator sends neither `target_kind` nor `enabled`, and reads none of `zdr`, `clamped_units` or `observed_reason`. Under `enforce` it acts on `allow` and `reason` only.
8. **No boot check for the token.** `enforce` without the org-core token denies every scheduled fire, and nothing at boot says so.
9. **Change monitors have no fire-time preflight.** The `monitor-preflight-v1` step is not built.
10. **An org's last schedule is never reaped.** The reaper removes a Temporal schedule only while its org still has at least one row in the desired set. This is deliberate, fail-closed behaviour, and `reaper_isolation_test.go` pins it. The result: deleting an org's last schedule leaves its Temporal schedule running. A scrape, crawl or batch orphan fails each fire at registration (`404`). A change-monitor orphan keeps its cron. Until a fix lands, an operator deletes such an orphan in Temporal by hand, after checking that its memo's `org_id` is the right org.
11. **Enforce pauses without restamping.** A schedule re-enabled after a `skipped_zdr` pause carries its create-time ZDR stamp again. Nothing reverses a pause automatically.
12. **A pending request has no cancel route.** Rows left pending while the flag is off run when it comes back on.
13. **Org erasure does not delete `schedule_requests`** yet (`gdpr_purge.go`).
14. **`QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG=1` in compose does nothing.** It is kept as a fence for an older control image.

## Tests

Written, not run. The integration step compiles and runs them. The go-live evidence is in RUNBOOKS.

- Control: `services/quarry-control/internal/resources/schedules_test.go`, `schedules_zdr_test.go`, `schedule_requests_test.go`, `schedule_runs_test.go` and `cycle23_aliases_test.go`; `internal/store/pg/schedules_017_test.go` and `schedule_requests_test.go` (these need `QUARRY_TEST_DSN`); `internal/cron/interval_test.go`; `internal/posture/posture_test.go`.
- Orchestrator: `internal/workflows/scheduled_test.go` (includes a replay of a synthetic history, not a capture from Temporal); `internal/schedules/schedules_test.go`, `spec_test.go`, `requests_test.go` and `reaper_isolation_test.go`; `internal/activities/scheduled_test.go`.
- Edge: `crates/quarry-edge/tests/schedule_create.rs`, `tests/schedule_preflight.rs` and `tests/zdr_posture_stamp.rs`, plus the in-file tests of `src/schedule_preflight.rs` and `src/schedule_routes.rs`.
- Gateway (Frontend Plane): `apps/gateway/src/domains/ingestions/schedules.rs` (the capability defaults, the kind refusal, and the typed 501 for an unflagged trigger).
- `scripts/tests/docs-contract.test.mjs` checks the RUNBOOKS names. It does not read this page.
