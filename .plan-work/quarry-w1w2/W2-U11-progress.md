# W2-U11 progress: WS15.L1 job record v2, origin, status, errors and ongoing reads

Unit W2-U11 (batch 2). Branch quarryv2-next, worktree vigorous-perlman-e7ae5b. Built to scope, committed, NOT run (wave build strategy). Depth items: `depth/W2-U11.md`.

## Commits (in order)

| Commit | Scope |
|---|---|
| 7c1be7597 | control: migration 016 (IF NOT EXISTS), Job record v2 (origin, desired and applied state, workflow id, deadline, terminal reason, finish time), jobColumns/scanJob (one column list), origin validation at create, if_status compare-and-set and no leaving a terminal status on PUT, terminal events never regress, paused/resumed/timed_out/skipped mapping, GET /v1/jobs/ongoing, /{id}/summary, /{id}/errors, job wire with origin/phase/stats (one grouped count per page), ?status= filter on /v1/{kind}/jobs, orchestrator failed-page event final:true and code. Tests written: resources/jobs_lifecycle_test.go, store/pg/jobs_lifecycle_test.go |
| 735d23d33 | edge: crawl and batch handoff post {kind, params, origin}, signed, with Idempotency-Key; origin from verified claims (user, agent with x-verevon-run-id for Model Plane services, api); control_client (signed get/send, status mapping); job_lifecycle_routes (status, errors, ongoing for crawl, batch and any job; kind checks; 403 under ZDR floor on errors); quarry-core typed views (JobStatusView, JobErrorsView, RunOrigin, ...); JobSummary gains origin and phase; main.rs and lib.rs module lines. Tests written: tests/job_lifecycle.rs, control_client unit tests |
| 2c3cdd6b0 | docs: openapi.yaml paths for the lifecycle reads; CONTRACTS.md section; REST_RESOURCES.md table |
| 2680d8ea5 | docs: RUNBOOKS.md section, committed hunk-only (the file carries another unit's uncommitted hunks, left in the working tree) |

## Verified (commands and results)

- `go vet ./...` in services/quarry-control: rc 0 (compiles all test files); `go build ./...`: rc 0
- `go vet ./internal/workflows/` in services/quarry-orchestrator: rc 0
- gofmt -l on the new Go files and on the changed Go files with CRLF stripped: clean (cycle23.go has pre-existing import-order and blank-line diffs, not from this unit, left alone)
- `cargo check -p quarry-core -p quarry-edge --tests` via qtest: rc 0 (lib, bin and all test targets compile; the warnings listed are pre-existing, none from this unit's files)
- openapi.yaml parses as YAML with gopkg.in/yaml.v3 (module cache); path keys present
- Pre-commit gitleaks hook: no leaks found, on every commit

## Not run (by design of the wave)

- No Go or Rust test was run. The first test run will show whether the new tests and the route precedence (/v1/crawl/:id against /v1/:kind/jobs) hold.
- No stack, image or deployment evidence (see depth file).

## Not done (in scope of the queue row, outside the plan's L1 detail or blocked)

- Gateway and SPA: the queue row names gateway domains/ingestions and SPA ingestions files; the plan's L1 detail does not, and the L5 slice owns those surfaces. The gateway's to_run_item already reads stats.completed and stats.total, so the run rows show real progress with no gateway change.
- ZDR-floor 403 test: needs the RS256 harness of tests/zdr_claim_floor.rs.
- Rust mirror of run_timed_out and schedule_skipped event types (H8 checklist; owned by the L2 and S4 slices that emit them).

## Deviations from the plan text (recorded for the arbiter)

- `ListActive(limit)` became `ListActiveByOrg(orgID, limit)`: an unscoped list would be a tenant leak on a tenant-facing read.
- The errors view omits the raw error text (plan D7 lists a message field); category and code only.
- Control's `/v1/jobs/ongoing` route and the edge's `/v1/crawl/ongoing` and `/v1/batch/ongoing` routes: the plan named `/ongoing` per kind; no unscoped `/v1/jobs/ongoing` is exposed at the edge.

## For the next batch (U12 onward, and WS15.L2)

- The control `jobColumns` list and `scanJob` are the single place for job columns; WS15.L2 writes desired_state, desired_seq and applied_seq, and should use UpdateIfStatus at each caller.
- `updateJob` already refuses to leave a terminal status (409); L2's cancel must go through a desired-state write, not a status write.
- `markJobTerminalIfNeeded` does read-modify-write; see depth items for the race.
- Edge deploys after control: the new edge reads answer 404 against an old control (no such route).
- Edge route `get_crawl` delegates the path id "jobs" to the list; keep that when the route table changes.
