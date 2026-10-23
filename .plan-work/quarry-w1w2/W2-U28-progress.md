# W2-U28 progress: WS15.L2 cancel, pause, resume (desired state, orchestrator control loop, batch handlers, typed timeout)

Unit W2-U28 (batch 5). Branch quarryv2-next, worktree vigorous-perlman-e7ae5b. Built to scope, NOT run (wave build strategy).
Depends on WS15.L1 (U11, commits 7c1be7597, 735d23d33: migration 016 columns desired_state/desired_seq/applied_seq/workflow_id/deadline_at/terminal_reason/finished_at already exist).

## Status (updated as milestones land)

- [ ] control: store SetControl/AckControl/PendingControl/ListRunning/Finalize (memory + pg), jobs_lifecycle.go control handlers, internal routes
- [ ] control: event wire for control_requested, run_timed_out, schedule_skipped (Go consts + wire map)
- [ ] orchestrator: internal/timeouts/timeouts.go (ExecutionTimeoutFor) + test
- [ ] orchestrator: internal/jobs/control.go (RunControl, applyPending, escalate, sweep) + test; main.go wiring behind QUARRY_JOB_CONTROL_LOOP=1
- [ ] orchestrator: dispatcher markRunning CAS (if_status accepted), workflow_id/deadline_at, 409 terminates started workflow
- [ ] orchestrator: workflows signals.go (Await behind lifecycle-v1), workflows.go BatchJobWF control handlers behind batch-control-v1, pauseSeq idempotency
- [ ] edge: job_lifecycle_routes.rs cancel_job/pause_job/resume_job, authorize_control; routes.rs four routes (H2 route-table region); QUARRY_EDGE__JOB_CONTROL flag (501 off)
- [ ] quarry-core event.rs mirror of the three event types
- [ ] openapi.yaml control verbs; RUNBOOKS.md section (append-only)
- [ ] tests written (named in plan), compile-checked only

## Milestone log (appended during the build)

- control (Go): store/control.go (PlanControl, ApplyControl, memory store methods), store/store.go (JobsStore: SetControl, AckControl, PendingControl, ListRunning, Finalize), store/pg/jobs_control.go, resources/jobs_control.go (controlJob, pending/ack/running/finalize internal handlers), resources/resources.go (route mounts), pkg/quarrycontracts/event.go (3 consts), resources/event_wire.go (wire map). go vet ./... in quarry-control: rc 0 (re-run after the test rewrite: rc 0).
- orchestrator (Go): internal/timeouts/timeouts.go (+ test), internal/jobs/control.go (RunControl, applyPending, escalate, sweep, controlCall), internal/jobs/control_dispatch.go (executionTimeout, stopStarted, errJobChanged), dispatcher.go (ControlLoop config, CAS markRunning with if_status/workflow_id/deadline_at, 409 terminates start), cmd/orchestrator/main.go (RunControl behind QUARRY_JOB_CONTROL_LOOP), workflows/signals.go (pauseSeq, Await behind lifecycle-v1), workflows/workflows.go (BatchJobWF control behind batch-control-v1, controlGate, pauseSeq in event ids). go vet ./... in quarry-orchestrator: rc 0.
- Tests written (compile-checked via vet where the package compiled): control resources/jobs_control_test.go; orchestrator workflows/job_control_test.go, jobs/control_test.go, timeouts/timeouts_test.go.
- edge (Rust): job_lifecycle_routes.rs (ControlVerb, control_request, six verb handlers, JOB_CONTROL flag = QUARRY_EDGE__JOB_CONTROL), routes.rs (six routes after the L1 batch errors route), quarry-core event.rs (3 variants), quarry-runtime nats_event_bus.rs (3 token arms, one-line anchored insert in U25's file), tests/job_lifecycle.rs (4 tests + control double route).

## Commits (final)

| Commit | Scope |
|---|---|
| 31a2f7928 | control: store control rules and writes (memory and Postgres), internal control reads, org-scoped control route, event constants and wire map, handler tests |
| 12aae7ed0 | orchestrator: control loop (signal, ack, escalate, sweep), per-kind execution timeouts, start compare-and-set, pause via Await under lifecycle-v1, batch control under batch-control-v1, tests |
| 63b185124 | docs: openapi control paths and the WS15.L2 RUNBOOKS section (hunk-only; other units' hunks stay uncommitted) |
| d65c897be | edge: job_lifecycle_routes verbs, quarry-core event variants, quarry-runtime token arms (whole files, each only this unit's hunks) |
| f0e995d1a | edge: six control routes in routes.rs (hunk-only; other units' hunks stay uncommitted) |
| 2503209d6 | edge: four control tests and the control double route in tests/job_lifecycle.rs |

Plan deviations recorded for the arbiter: the verbs live in job_lifecycle_routes.rs (the plan detail), not resource_routes.rs (the queue row); control.desired uses the verb name on the verb answer (plan example) while GET uses control's desired_state; a 60 s floor on the typed timeouts; the forced grace is measured from first sight in the orchestrator process; the edge reads the job-control flag per request, not from AppState.

## Verification (commands and results)

- go vet ./... in services/quarry-control: rc 0 (compiles resources and store tests, after the test rewrite to plain testing).
- go vet ./... in services/quarry-orchestrator: rc 0 (compiles workflows, jobs, timeouts tests and cmd).
- cargo check --tests -p quarry-core -p quarry-browser --features chromiumoxide -p quarry-runtime -p quarry-edge via qtest (log: scratchpad W2-U28/qcheck1.log): CHECK_RC=0, no error lines; warnings pre-existing.
- gofmt -l on the new Go files: clean.

## Not run (by design of the wave)

- No Go or Rust test was run. No stack, image or deployment evidence. Tests are listed in depth/W2-U28.md.
- Gateway and SPA surfaces are WS15.L5 and are not in this unit.
