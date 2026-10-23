# W2-U38 progress: WS15.S2 executable scrape, crawl and batch schedules (batch 7)

Status: BUILT to scope, dark (`QUARRY_SCHEDULED_JOBS_ENABLED`, default 0). Tests WRITTEN and compile-checked (`go vet` for the Go packages; the edge check is listed below). Tests NOT run (the test phase is at the end of wave 2). Deployment evidence NOT run.

Plan: docs/plans/quarry-browser-build-plan-2026-10-14.md, "Slice WS15.S2 detail". Queue: W2-queue.md section 3, BATCH 7, U38. Depth: .plan-work/quarry-w1w2/depth/W2-U38.md.

## Commits (local, quarryv2-next, not pushed)

| Hash | Subject | Content |
|---|---|---|
| 4419d5f22 | feat(orchestrator): executable scheduled jobs, wrapper workflow and shared builders (WS15.S2) | workflows/inputs.go, workflows/scheduled.go (ScheduledJobWF), activities/scheduled.go (PreflightFire, RegisterScheduledRun), schedules/schedules.go + spec.go + spec_ops.go (reconciler), jobs/dispatcher.go (delegation to the shared builders), cmd/orchestrator/main.go, schedules_test.go (path) |
| 1aa3d0a7f | feat(control): executable scrape, crawl and batch schedules behind a kill switch (WS15.S2) | resources/schedule_runs.go (flag, register route, skipped event), schedule_runs_test.go, schedule_wire.go (ScheduledJobWF mapping, wire fields), schedules.go (route mount) |
| beb4ab33e | chore(compose): QUARRY_SCHEDULED_JOBS_ENABLED on control, default 0 (WS15.S2) | apps/Ingestion Plane/docker-compose.yml (control env line only; hunk-only, WS17.E3's hunk left uncommitted) |
| a699ae2af | docs(quarry): RUNBOOKS section for executable schedules (WS15.S2) | docs/RUNBOOKS.md (one appended section; hunk-only against the HEAD blob) |
| efbe8cd46 | feat(edge): fire-time preflight for executable scheduled jobs (WS15.S2) | crates/quarry-edge/src/schedule_preflight.rs (NEW), tests/schedule_preflight.rs (NEW), lib.rs (one mod line, hunk-only), routes.rs (one route, hunk-only) |

Shared files (edge): lib.rs, routes.rs and main.rs in quarry-edge carry other units' uncommitted hunks (internal_routes, usage_attrs, and others). The edge commit takes only the S2 hunks, via qcommit-patch.sh against the HEAD blob.

## Compile checks (what was run)

- `go vet ./internal/workflows/ ./internal/schedules/ ./internal/activities/ ./internal/jobs/ ./cmd/orchestrator/` in quarry-orchestrator: exit 0 (includes the new test files).
- `go vet ./internal/resources/ ./cmd/control/` in quarry-control: exit 0 before the test commit; `go vet ./internal/resources/` exit 0 after the test file was added.
- gofmt: clean on the new Go files; schedule_wire.go reformatted (CRLF kept).
- Edge: `cargo check --tests -p quarry-edge` through qtest.sh: exit 0 (second run, after the main.rs module line; the first run failed only on the missing binary-root module, since fixed). Warnings are in other units' files. Log: scratchpad U38/check-edge2.log.

## Deviations from the plan text (recorded in the depth file)

- Change detection uses Describe and a canonical form, not a `spec_hash` memo (the SDK cannot update a schedule memo).
- The reconciler writes the resolved crawl bounds, the overlap policy and the execution timeout into the fire's args. Control's args list is therefore a subset of the args Temporal receives.
- `ScheduledJobWF` is registered by name in main.go. The child start uses the wrapper's task queue (`TaskQueueName`).

## Not done in this batch (see the depth file for the full list)

- Test cases the plan names and that are not written: `TestScheduledJobWF_OverlapCancelSignalsChild`, `TestScheduledJobWF_ChildTimeoutEmitsRunTimedOut`, `TestScheduledJobWF_ReplaysRecordedHistory` (and its fixture `testdata/scheduled_job_history.json`), `TestReconcile_NoWorkflowPausesExistingTemporalSchedule` (covered by `TestPauseIfRunning_*` under another name), `TestScheduleRuns_ListsRegisteredRuns`.
- Deployment evidence (flag on, Oslo time zone, overlap skip, cancel, flag off pauses, batch and scrape fire).
- Standalone compose `Quarry-v2/deploy/compose/docker-compose.yml` control block not updated.
- The skipped-fire event append is not retried; the registration does not stamp `deadline_at`.

## What the next batch needs to know

- The flag is read in control only. Deploy order: control, orchestrator, edge. Keep it 0 until WS14 scope is deployed and WS6.A is live (S5 gate).
- `GET /v1/schedules/{id}/runs` already lists the fire's jobs (unchanged S1 route). The fire's jobs carry `origin.type = schedule`.
- The orchestrator's shared builders (`workflows/inputs.go`) are the one place the child input is built. A later WS14 or WS6 change to crawl input fields goes there, once.
- `hmacVerifier.Middleware` must keep covering the schedule mount (cmd/control/main.go). Another unit has uncommitted edits in that file; check the mount is still inside the group.
- Hand-over: .plan-work/quarry-w1w2/wf-W2-U38.md.
