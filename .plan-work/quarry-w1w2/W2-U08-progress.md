# W2-U08 progress (batch 2, slice WS6.K)

Branch `quarryv2-next`, worktree `C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b`. Queue row: `.plan-work/quarry-w1w2/W2-queue.md` section 3, U08 (batch 2).

## Finding at start

The orchestrator part of WS6.K was already built in 369caa89 (workflows/statistics.go, workflows.go terminal events for crawl and batch, control migration 020, jobColumns refactor, Job.Statistics, markJobTerminalIfNeeded copy, CONTRACTS.md). Every test the plan names exists: `TestCrawlJobWF_TerminalEventCarriesStatistics`, `TestStatisticsContainNoUrlOrErrorText`, `TestBatchJobWF_TerminalEventCarriesStatistics`, control `TestTerminalEventPersistsStatistics`, and the pg round trip in `pg_test.go`. The only item left was the run-duration histogram, which is a quarry-edge series and sits in the run lifecycle, not in the orchestrator.

## Committed

| Commit | What | Tests |
|---|---|---|
| d15ba874c | feat(quarry-edge): agent run-duration histogram per run class. `run_live.rs`: `RunLive::started_ms()`, `run_duration_seconds()` (saturating), `record_run_duration()`. `close.rs`: one call in `finish_run`. | `cargo check -p quarry-edge --tests` rc=0 (docker lock). Unit tests `run_duration_tests` (2) written, not run. |
| 4df36420d | docs(quarry): RUNBOOKS section (7 lines, append-only, committed by hunk via qcommit-patch.sh; other agents' hunks not included) | none |

Edits outside U08's owned paths: `agent_routes/close.rs` and `agent_routes/run_live.rs` (exact-anchor, 38 added lines, both files only my hunks). Both were free in batch 2 (U07 touches `agent_routes/tests.rs` only). The H1 rule (one writer per submodule per batch) holds.

## Verified (what was run)

- `go vet ./internal/workflows/ ./internal/jobs/ ./cmd/...` in `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator`: rc=0.
- `cargo check -p quarry-edge --tests` (qtest, CARGO_BUILD_JOBS=2): rc=0. Output filtered to errors and unused warnings; none in my files.
- Not run, by the build-first rule: all `go test`, all `cargo test`.

## Not done (and why)

- The histogram test named in the queue (`statistics_test.go`) is not possible in Go; the test is `run_duration_tests` in `run_live.rs`.
- Deployment evidence for WS6.K (rebuild, crawl with one broken link, psql check): evidence phase.
- WS6.I's `describe_all` does not list the histogram yet (U48 owns `metrics.rs`).
- The page-event error-classification gap (`errs.Classify` vs the Temporal wrapper) was noticed, not fixed; see depth.

## Hand-over for the next batch

- The queue row U08 should be corrected: the histogram is in quarry-edge (`close.rs`, `run_live.rs`), not in the orchestrator test file.
- When U48 (WS6.I) runs, add `quarry_agent_run_duration_seconds` to `describe_all` in `metrics.rs`.
- Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U08.md`. Hand-over: `.plan-work/quarry-w1w2/wf-W2-U08.md`.
