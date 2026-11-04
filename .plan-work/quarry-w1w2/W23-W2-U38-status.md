# W2-U38 status (lane b)

Unit: three missing scheduled-workflow tests and fixture. Plane: Ingestion (quarry-orchestrator).

## Built

Built in full. Before this unit `scheduled_test.go` held six tests (happy path, run id, preflight deny, child failure, batch and scrape kinds, unknown kind) and `internal/schedules/testdata/` did not exist.

- `TestScheduledJobWF_OverlapCancelSignalsChild`: overlap policy `cancel`, parent cancelled while the child runs; the child receives the cancel signal and the fire ends with no error and no failure event.
- `TestScheduledJobWF_OverlapCancelWaitIsBounded`: a child that ignores the signal does not hold the wrapper; it ends after `scheduledCancelWait`.
- `TestScheduledJobWF_NonCancelPolicyDoesNotSignalChild`: a policy other than `cancel` sends no signal.
- `TestScheduledJobWF_ChildTimeoutEmitsRunTimedOut`: a timed-out child gives one `run_timed_out` with reason `child_timed_out`, the schedule fields, no URL, and the fire still fails (pause-on-failure sees it).
- `TestScheduledJobWF_ReplaysRecordedHistory`: replays `internal/schedules/testdata/scheduled_job_crawl_history.json` (preflight, register, CrawlJobWF child, completion) with `worker.WorkflowReplayer`.
- `TestScheduledJobWF_ReplayRejectsAChangedCommand`: the same history with the activity renamed must fail to replay (the fixture has teeth).
- `TestRegenerateScheduledReplayFixture`: skipped unless `QUARRY_REGEN_REPLAY_FIXTURE=1`; rewrites the fixture and replays it first.
- `scheduledEnv` got an optional `crawlBody` hook for the CrawlJobWF fake.

## Defect found and fixed in the workflow

`ScheduledJobWF` waited with `child.Get(ctx, nil)`. In the Go SDK a cancelled parent context does not settle a child future until the child closes, so the overlap-cancel branch (signal the child, wait up to 2 minutes) only ran after the child had already stopped. The signal could never reach a running child, and a child that did not stop on the cancellation request kept the wrapper open indefinitely. Proof: `OverlapCancelSignalsChild` hung to the 3 s test timeout before the fix and passes after it. Fix: the wrapper waits on a selector over the child future and `ctx.Done()`. No new command is issued, so the recorded history still replays.

## D2-001 schedule spec tests

`TestCreate_AppliesOverlapTimezoneJitterCatchup` and `TestPrepareSpec_ResolvesCrawlBoundsAndTimeout` pass on the current tree (also `TestScheduledJobWF_HappyPathCrawl`). Nothing was stale, so `schedules/spec_test.go` is unchanged. The D2-001 failures were fixed earlier on the integration branch.

## Commits (lane b, local)

- 853f75dce fix(quarry-orchestrator): scheduled wrapper signals the child on overlap cancel while it still runs
- 2d6668cea test(quarry-orchestrator): scheduled wrapper cancel, timeout and recorded-history replay (W2-U38)
- status and depth rows: the commit that adds this file

## Tests (host, GOWORK=off)

`go vet ./internal/workflows ./internal/schedules` clean. `go test ./internal/workflows ./internal/schedules -count=1`: both packages ok; 100 PASS lines (top level and subtests), 1 SKIP (the regenerator), 0 FAIL. The `TestScheduledJobWF` group run 5 times with `-count=5`: ok. `-race` not run (needs cgo, unavailable).

## Flag and default

`QUARRY_SCHEDULED_JOBS_ENABLED=0`, unchanged. The wrapper change is behind the same flag.

## Assumed decisions

- The fixture lives in `internal/schedules/testdata/` as the scope names it; the workflows test reads it through `../schedules/testdata/`.
- No Temporal CLI or dev server on the host, so the history is built event by event in the shape the server records, not captured from a live cluster. The regenerator replays it before writing.
- The test environment does not enforce a child's execution timeout, so the timeout test has the child close with `temporal.NewTimeoutError` (what the server gives the parent for a timed-out child).
- The fixture file is LF (generated JSON); the Go files stay CRLF.

## Model Plane asks

None.

## Not verified

- The fixture is synthetic, not a capture from a real Temporal server; a capture from the stack session would be stronger.
- The cancel path was not run against a real Temporal server or a real CrawlJobWF; the test child is a fake that waits for the signal.
- Whether the real job workflows react to the SignalCancel received after a cancellation request is not tested here.
