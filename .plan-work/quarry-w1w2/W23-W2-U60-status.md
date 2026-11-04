# W2-U60 status: Temporal replay test over a pre-scope crawl history (D2-071, D2-139)

Branch w23/lane-a. Date 2026-11-02.

## Built
- Replay gate (`workflows_replay_test.go`): five recorded CrawlJobWF histories in `testdata/replay/` replay through `worker.WorkflowReplayer` against the CrawlJobWF this tree builds, with the worker's own wrapper registration (name `CrawlJobWF`, activities injected):
  - `pre_bounded_inmemory`: no version marker at all (before crawl-bounded-v1).
  - `pre_scope_inmemory`, `pre_scope_durable`: crawl-bounded-v1 marker only, no crawl-scope-v1 marker (the pre-scope crawls that are in flight at deploy), on the in-memory and the durable (production) frontier.
  - `scoped_inmemory`, `scoped_durable`: both markers, a scope in the input, so later edits to the scope branch cannot break the crawls that run under it now.
- Teeth: `TestReplay_ADriftedCommandSequenceIsCaught` (one activity renamed in a fixture: "nondeterministic" error) and `TestReplay_AScopedInputOnAPreScopeHistoryRunsTheLegacyLoop` (a pre-scope history whose input now names a scope must still replay: GetVersion answers DefaultVersion and the loop stays legacy). Mutation check done by hand: with the `crawl-scope-v1` GetVersion guard replaced by `if true`, the second test fails for all three pre-scope fixtures; the guard was restored (`git diff` on workflows.go is empty).
- `TestReplay_PreScopeFixturesCarryNoScopeMarker` pins the markers each fixture has; `TestReplay_FixturesMatchTheirScenarios` keeps the files in step with `replayScenarios()` (event types, activity names and markers compared against a fresh recording).
- Workflow scope tests (`workflows_scope_test.go`, the D2-139 gap; only the sitemap/scrape tests ran scoped crawls before), each run on the in-memory and the durable frontier: context on every page (depth, seed hosts, max depth, seed url, mode, robots), observe/respect defaults, legacy scope sends no context and reports no scope fields, an edge without the scope proof fails the run with `edge_scope_unsupported` and follows no link, refused pages count as blocked (robots counted apart) and not failed, reports fold into the terminal event and widen the seed hosts, the depth bound, and one `FrontierEnqueueBatch` call per page on the durable frontier.
- Already existed, not rebuilt: `TestCrawlJobWF_OldVersionHistoryKeepsLegacyBehaviour` (test environment, crawl-bounded-v1), `crawl_scope_test.go` unit tests of crawlScopeRun, batch scope tests.
- The edge `crawl_scope_robots.rs` extension named in D2-139 is Rust and outside this unit's files; not done here.

## How the fixtures were recorded
No live Temporal was available and the test environment keeps no history of its own. `recordHistory` runs CrawlJobWF under TestWorkflowEnvironment with GetVersion pinned (DefaultVersion for the markers a scenario lacks), captures every activity the workflow scheduled and its result, and writes them out as the events a server records (workflow tasks, version marker + search-attribute upsert, activity scheduled/started/completed). Regenerate with `QUARRY_RECORD_REPLAY=1 go test ./internal/workflows -run TestRecordReplayFixtures`. A history exported from a live Temporal (`temporal workflow show --output json --workflow-id <id>`) dropped into `testdata/replay/` as `exported_*.json` (`exported_durable_*.json` for the durable frontier) is replayed by the same test with no code change.

## Commits
See `git log w23/lane-a` (replay test and fixtures, scope tests, this status file and depth rows).

## Tests (Go host, GOWORK=off, `go test ./internal/workflows -run 'Replay|Scope' -count=1 -v`)
- 48 PASS, 1 SKIP (TestRecordReplayFixtures, runs only with QUARRY_RECORD_REPLAY=1), 0 FAIL.
- `go test ./internal/workflows -count=1` ok (whole package); `go vet ./...` clean for quarry-orchestrator.

## Flag and default
None. Tests and fixtures only; no production code touched.

## Assumed decisions
- A fixture synthesized from the test environment stands in for the live export the plan asked for; the drop-in `exported_*.json` path closes the gap when an export exists.
- New files `workflows_replay_test.go` and `workflows_scope_test.go` instead of growing `workflows_test.go` (the queue lists workflows_test.go; the new files sit next to it in the same package and touch no shared file).
- Fixture frontier is read from the file name ("durable" in the name = durable acts), the same switch production uses via activities.New.

## Model Plane asks
None.

## Not verified
- The fixtures were not compared with a real pre-scope binary's history (no pre-scope worker build was run); they are what the current code does with the scope marker absent, which is what a replayer must handle.
- No history exported from a live Temporal has been replayed.
- Activity inputs are not recorded in the fixtures (the replayer does not compare them), so a drift in an activity's input shape is not caught by this gate.
- Only the success path is recorded: no pause/resume/cancel signals, no failed activity events, no sitemap seeding, no timers.
