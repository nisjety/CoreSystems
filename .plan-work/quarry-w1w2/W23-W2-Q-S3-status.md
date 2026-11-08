# W2-Q-S3 status: WS15.S3 trigger and backfill through pull-model request rows (lane d, branch w23/lane-d)

## Built / partly
Partly: the control and orchestrator halves the scope names are built; the plan rows outside the unit's files are not (see Not done).

What existed before: `scheduleTrigger`/`scheduleBackfill` in `cycle23.go` called a `temporalclient.Client` that `cmd/control/main.go` always passes as nil, so both answered 501. The orchestrator `Manager.Trigger`/`Backfill` existed and nothing called them. No request table, store, routes or loop existed.

- `quarry-control/internal/store/pg/migrations/026_schedule_requests.sql`: table `schedule_requests` (plan's 018 renumbered; INNH-5 holds 025) with the plan's columns, `action` and `status` CHECKs, a partial index on open rows, a partial UNIQUE index of one open (pending or claimed) row per schedule, and UNIQUE `(org_id, idempotency_key)` where not null. LF, like its siblings.
- `quarry-control/internal/store/store.go`: `ScheduleRequest`, the action and status constants, `ScheduleRequestsStore` (`Create`, `GetByOrg`, `HasPending(scheduleID, action)`, `Claim(worker, limit, lease, now)`, `Complete`, `Fail(failed|rejected)`, `Release`), the optional `ScheduleRequestsProvider`, and the memory implementation (a new `memDB.schedReqs` field).
- `quarry-control/internal/store/pg/resources.go`: `postgresDB.ScheduleRequests()` and `scheduleRequestsStore`. `Claim` is `UPDATE ... WHERE id IN (SELECT ... WHERE status='pending' OR (status='claimed' AND lease_until < now) ORDER BY created_at LIMIT n FOR UPDATE SKIP LOCKED) RETURNING`, so it reclaims expired leases. Complete, Fail and Release act only on a row the named worker still holds (else ErrConflict; unknown id ErrNotFound).
- `quarry-control/internal/resources/schedule_requests.go` [new]: the flag, the request-mode `trigger` and `backfill` handlers, `GET /v1/schedules/{id}/requests/{rid}` (tenant view, no worker or lease), `POST /v1/internal/schedules/requests/claim {worker, limit, lease_s}` and `POST /v1/internal/schedules/requests/{rid}/result {worker, status: done|failed|rejected|retry, result|error}`.
  - Checks, in order: schedule id kind, org, `GetByOrg` (cross-org is 404), store has the table (else 501), enabled (else 409 `needs_activation`), the `QUARRY_SCHEDULED_JOBS_ENABLED` kill switch for scrape, crawl and batch (else 409 `scheduled_jobs_disabled`), one open request per schedule (else 409 `request_pending`, also when the DB index refuses a race).
  - Backfill also: `end_at > start_at`, `end_at` not in the future (400 `backfill_window_in_future`), overlap policy skip|allow|cancel, only `scrape` (409 `backfill_kind_refused`), at most 24 windows (400 `backfill_too_many_windows`). The row's params are `{start_at, end_at, windows, overlap_policy?}`.
  - Success is 202 `{request_id, schedule_id, action, status:"pending"}`. `Idempotency-Key` (max 128) is stored; `?user_id` becomes `requested_by`.
- `quarry-control/internal/resources/cycle23.go`: `MountScheduleAliases(r, db, tc)` keeps its signature (control's main.go is not in the unit) and calls `mountScheduleAliases(r, db, tc, ScheduleRequestsEnabled(os.Getenv))`. With the flag on, trigger and backfill use the request handlers and the request routes are mounted; with it off, everything is as before (501).
- `quarry-control/internal/store/storetest/schedule_requests.go` [new]: `ScheduleRequestsConformance` (tenant-scoped read, one open row, idempotency key per org, HasPending action filter, claim once oldest first, concurrent claims disjoint, expired lease reclaimed with the old worker locked out, fail, reject and release).
- `quarry-control/internal/store/pg/schedule_requests_test.go` [new]: the suite against Postgres (DSN-gated, isolated schema per subtest) and a no-DSN check that migration 026 carries the indexes.
- `quarry-control/internal/resources/schedule_requests_test.go` [new]: memory conformance; flag parsing; flag off keeps 501 and mounts no claim route; `TestTrigger_InsertsPendingAndReturns202`, `TestTrigger_DisabledScheduleIs409`, `TestTrigger_SecondPendingIs409`, `TestTrigger_ScheduledJobsSwitchOffIs409`, `TestBackfill_CrawlKindRefused`, `TestBackfill_OverTwentyFourWindowsRefused`, `TestBackfill_ThreeDailyWindowsAccepted`, `TestBackfill_InvalidWindowsRefused`, `TestRequest_CrossOrgIs404`, `TestClaimAndResult_RoundTrip`, `TestClaim_RetryGivesTheRowBack`, `TestClaim_WorkerRequired`, and a check that both internal paths are in the orchestrator-only HMAC group.
- `quarry-orchestrator/internal/schedules/requests.go` [new]: `RequestsEnabled`, `Manager.RunRequests(ctx)`. Every 2 s it claims up to 10 rows (lease 60 s), calls `Manager.Trigger` or `Manager.Backfill` (15 s timeout each) and posts the outcome:
  - Temporal accepted: `done` with `{schedule_id, action, submitted_at}`.
  - `serviceerror.NotFound`: `rejected` ("schedule not materialised yet").
  - Bad params or an unknown action: `rejected`, without calling Temporal.
  - Any other error: `retry` (the row goes back to pending), and `failed` on the third attempt.
  - It signs with `QUARRY_CONTROL_INTERNAL_SECRET`, falling back to the shared secret.
- `quarry-orchestrator/internal/schedules/requests_test.go` [new]: `TestRunRequests_TriggersAndPostsResult`, `TestRunRequests_BackfillPassesTheStoredWindow`, `TestRunRequests_BadBackfillParamsRejectedWithoutTemporal`, `TestRunRequests_TransientErrorRetriesThenFails`, `TestRunRequests_UnknownScheduleRejected`, `TestRunRequests_ClaimFailureCallsNothing`, `TestRunRequests_SignsWithTheControlInternalSecret` (recomputes the HMAC), `TestRunRequests_LoopStopsOnCancel`, plus flag parsing.
- `quarry-orchestrator/cmd/orchestrator/main.go`: `go schedMgr.RunRequests(rootCtx)` only when `schedules.RequestsEnabled()`.

The same fire-time preflight: a trigger or backfill goes through the Temporal schedule, so a scrape, crawl or batch fire is the schedule's own `ScheduledJobWF`, with the same `PreflightFire` as a cron fire. A change monitor gets its preflight in WS15.S4.

## Commits (w23/lane-d)
- 7fa039106 feat(quarry-control): schedule_requests table and store for pull-model trigger and backfill (W2-Q-S3, WS15.S3)
- 2f103e332 feat(quarry-control): trigger and backfill answer 202 request rows behind QUARRY_CONTROL__SCHEDULE_REQUESTS (W2-Q-S3, WS15.S3)
- b4de27454: the orchestrator files (`cmd/orchestrator/main.go`, `internal/schedules/requests.go`, `requests_test.go`) landed in W2-Q-WS7K's docs commit. Both units were staging in the shared lane index at the same moment, so their commit took my staged files. The content is mine and complete. The commit was not rewritten (no reset or amend of another unit's commit).
- this status file and the lane d depth rows (docs commit)

## Code only, not compiled
No go run (owner order 2026-11-03). The integration step compile-checks. Tests to run there:
- `cd services/quarry-control && go test ./internal/store/... ./internal/resources/... -run 'ScheduleRequest|Trigger|Backfill|Claim|Request_|MountScheduleAliases'`
- `go test ./internal/store/pg/... -run ScheduleRequests` with `QUARRY_TEST_DSN`
- `cd services/quarry-orchestrator && go test ./internal/schedules/... -run 'RunRequests|RequestsEnabled'`
- Also the existing `cycle23_aliases_test.go`, which must still pass with the flag unset.

## Flag and default
- Control: `QUARRY_CONTROL__SCHEDULE_REQUESTS` (truthy 1/true/yes/on). Default off: trigger and backfill answer 501 as before, and no request route is mounted.
- Orchestrator: `QUARRY_SCHEDULE_REQUESTS` (truthy). Default off: the loop does not start. The plan said default on; it is off here (owner rule: new behaviour ships off). It finds rows only when the control flag is on too.
- Gateway: the SCHEDULE capabilities `trigger` and `backfill` stay false (not touched). The edge still expects the old answer, so the control flag must stay off until the edge half is built.

## Assumed decisions
- X-9 / D14 option 2 (pull model), as the scope says.
- The internal routes are `/v1/internal/schedules/requests/claim` and `/{rid}/result`, not the plan's `/v1/internal/schedule-requests/...`. Under that prefix, control's `httpx.SplitHMAC` verifies them with the orchestrator-only secret (D2-050) without editing httpx.
- The orchestrator's `controlauth.IsControlInternalPath` does not list these paths, and controlauth is not in the unit. So the loop picks the control-internal secret itself and passes it to `Sign`, which uses the passed secret for a path it does not list.
- The request store is reached through an optional `store.ScheduleRequestsProvider` (the `RequestQueueReader` pattern), not a new `DB` method. `pg.go`, the `DB` interface and every fake stay unchanged. A DB without it answers 501.
- Backfill windows are counted as ceil(span / `cron.MinInterval`). That is exact for a regular cron and an upper bound for an irregular one, because `internal/cron` exports no next-fire function and is not in the unit. A window ending in the future is refused.
- Trigger and backfill both need an enabled schedule (409 `needs_activation`), and scrape, crawl and batch also need the `QUARRY_SCHEDULED_JOBS_ENABLED` switch (otherwise Temporal has no schedule and the row would only be rejected).
- Transient Temporal errors post `retry` (a new `Release`, which keeps the attempt count) up to attempt 3, then `failed`. This is not left to lease expiry. `Release` is not in the plan's method list. It is the "release the lease" step the plan describes.
- The result carries `{schedule_id, action, submitted_at}`, not a workflow id: Temporal's schedule trigger and backfill calls return none.
- Backfill overlap: the policy is stored in params, but `Manager.Backfill` still hard-codes ALLOW_ALL. `schedules.go` is not in the unit.
- `requested_by` comes from `?user_id` (stamped by the edge); the `Idempotency-Key` header is stored, and a repeat answers 409.
- The tenant view hides `claimed_by` and `lease_until`; the claim view carries `org_id` for logs.

## MP asks
None. No Model Plane surface is touched.

## Not done (plan rows outside the unit's files)
- `gdpr_purge.go`: `DELETE FROM schedule_requests WHERE org_id = $1` and `PurgeResult.ScheduleRequestsDeleted`. Org erasure does not yet cover the table (depth row).
- `internal/temporal/client.go` not deleted. The control `main.go` call stays `MountScheduleAliases(r, db, nil)`, and the `cycle23_aliases_test.go` cases are not replaced (they still pin the flag-off 501).
- `schedules.go` `Backfill` taking the overlap policy (`TestBackfillPassesOverlapPolicy`).
- Edge (`schedule_routes.rs` 202 plus `get_schedule_request`, ZDR refusal on trigger, backfill and unpause; `routes.rs` table), gateway and UI ("queued" and polling), the ADR `0001-quarry-control-commands-pull-model.md`.
- `controlauth.IsControlInternalPath` listing the request routes.

## Not verified
- Nothing compiled or run.
- The Postgres SKIP LOCKED behaviour is only exercised by the DSN-gated suite.
- No live proof: the plan's six deployment-evidence steps, including the orchestrator stop and start, are not run.
