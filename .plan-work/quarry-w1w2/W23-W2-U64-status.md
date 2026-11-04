# W2-U64 status: applied-by tolerant of a missing scope_applied (version skew)

Branch w23/lane-b. Date 2026-11-02. Built.

## Built
- Code check first: the Go `appliedBy` is not in `jobs/dispatcher.go` (the queue's file list) but in `workflows/crawl_scope.go`, called from `CrawlJobWF` in `workflows/workflows.go` (in-memory loop and durable loop). The fix is there; `dispatcher.go` got the D2-073 refusal. Files outside the queue list that were touched: `crawl_scope.go`, `workflows.go`, and the new `workflows_scope_skew_test.go` (no other B4 unit lists them).
- `crawlScopeRun.verdict(report)`: applied, legacy-unscoped, or refused. A report with `scope_applied: true` always passes (as before). A missing or false one is tolerated only in observe mode (empty or unknown mode is observe) and only on executions that carry the new marker `crawl-scope-applied-tolerant-v1`; the page counts as a legacy-unscoped one, nothing is absorbed from its report, a workflow log line is written (no history event). Enforce mode is still refused with `edge_scope_unsupported`.
- Replay safety: the marker is a `GetVersion` taken only when the crawl has a non-legacy scope. Histories recorded before it get DefaultVersion and keep the strict check. The W2-U60 replay fixtures still replay (full workflows package green).
- `jobs/dispatcher.go` `crawlInputFromJob` (D2-073): a crawl job with no (or null) `scope` and `scope_mode: enforce` is refused; with no scope in observe (or no mode) it runs as the legacy-unscoped crawl it always did.
- `quarry-edge/src/crawl_request.rs`: `ScopeSkew` and `scope_skew(mode, scope_present, scope_applied)`, the same table as the Go side, with two unit tests.

## Commits (w23/lane-b)
- 38b7371d2 fix(quarry-orchestrator): applied-by tolerates a missing scope_applied in observe mode (W2-U64)
- the crawl_request.rs mirror commit and this status/depth commit follow it (see git log).

## Tests
- Go (host, GOWORK=off): `go test ./internal/jobs ./internal/workflows -count=1` ok; whole `go test ./internal/...` ok (8 packages); `go vet ./...` clean. New: TestScopeVerdict (11 cases), 3 workflow tests x 2 frontiers (observe goes on, enforce refused, pre-marker execution strict), TestCrawlInputFromJobAbsentScope (5 assertions). jobs + workflows packages together: 166 PASS, 0 FAIL, 2 SKIP (-v count after the new tests were added; the skips are pre-existing).
- Rust (qtest lane b): `cargo test -p quarry-edge --lib crawl_request` 30 passed, 0 failed (28 existing + 2 new).

## Flag and default
No flag: it is a rolling-deploy bug fix (D2-029 follow-up), version-gated by the workflow marker. Default behaviour change: observe-mode crawls through an edge that does not report scope_applied now continue instead of ending after page one.

## Assumed decisions
- D2-073: absent scope runs legacy-unscoped in observe, refused in enforce (the queue text). Applied to the dispatcher path only (see depth row U64-1 for scheduled crawls).
- Empty or unknown scope_mode is observe, matching `newCrawlScopeRun`; the Rust rule matches exactly "enforce" (lowercase wire value).
- The fix lives in `workflows/` rather than `dispatcher.go`, because that is where appliedBy is.

## Model Plane asks
None.

## Not verified
- No live Temporal or live skewed edge: the skew is simulated by a mocked RunPage that omits `crawl`.
- The scheduled-job child crawl path is not covered by the enforce refusal.
- `scope_skew` in Rust is not called by any runtime code; it is a pinned table.
