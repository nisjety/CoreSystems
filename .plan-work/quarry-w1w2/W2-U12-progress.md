# W2-U12 progress (WS14.S0, WS14.S0b): build complete, tests not run

Unit W2-U12, batch 2 of the W2 queue. Slices: WS14.S0 (truth-telling docs and characterization tests) and WS14.S0b (hard go-test CI job). Build-first rule (2026-10-23): built, compile-checked, not run.

## Committed (branch quarryv2-next, local only, not pushed)
| Commit | Subject | Paths |
|---|---|---|
| ac0627b8e | test(quarry-orchestrator): pin legacy link following and the durable frontier loop (WS14.S0) | workflows/workflows_test.go (appended), workflows/workflows_durable_test.go (new) |
| 2489e2b63 | ci(quarry-v2): blocking go-test job for the Go workflow modules (WS14.S0b) | .github/workflows/quarry-v2-ci.yml |
| 73604c4d8 | docs(quarry-v2): correct the crawl scope and robots claims, add the WS14 contract (WS14.S0) | docs/GAP.md, Features.md, gap-quarry.md, ARCHITECTURE.md, POLICY.md, CONTRACTS.md (appended), CRAWL_SCOPE_ROBOTS.md (new); apps/STALE_DOC_DELETION_REGISTER.md (appended) |
| 9ecd83519 | docs(quarry): runbook section for the crawl characterization tests and go-test (WS14.S0, S0b) | docs/RUNBOOKS.md (one appended section; HEAD-blob commit) |

## Not committed by this unit
- `.plan-work/quarry-w1w2/` progress, depth and hand-over files (W2-U12-progress.md, wf-W2-U12-progress.md, wf-W2-U12.md, depth/W2-U12.md) are committed in the final docs(plan-work) commit of this unit.
- The working copy of `docs/RUNBOOKS.md` still holds other units' uncommitted sections; they are not ours.

## Verification actually run
- `go vet ./...` in `apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator`: exit 0, no output (compiles the test files, including the two new ones).
- `gofmt -l internal/workflows/`: `workflows_durable_test.go` is clean. The other listed files are CRLF files that gofmt reports on Windows; not a format change from this unit.
- YAML parse of `.github/workflows/quarry-v2-ci.yml` with gopkg.in/yaml.v3 (run from a scratch module): parses, 9 jobs, `go-test` present with the three-module matrix.
- `git diff --stat` before each commit: each commit contains only this unit's hunks (GAP.md 13 lines, Features.md 2 hunks, POLICY.md 1 line, gap-quarry.md 1 line, ARCHITECTURE.md 2 lines, CONTRACTS.md 9 appended lines, RUNBOOKS.md 9 appended lines, workflows_test.go 40 appended lines, quarry-v2-ci.yml 31 added lines).

## Not run (by design of the build phase, and by the user's build-first rule)
- No `go test` run. The two CrawlJob characterization tests and the other tests in the three go-test modules have not run.
- No CI run. The `go-test` job runs only after push.

## Not done (in scope, not finished)
- Branch protection for the `go-test (...)` checks: a repository setting, outside this branch.
- Ingestion owner review of `docs/CRAWL_SCOPE_ROBOTS.md` (definition of done for S0).
- The full GAP.md crawl-row audit (only the named rows were corrected; see depth file).
- Plan line references (CONTRACTS.md 352-368, 427-431; Features.md 39, 179) are stale in the plan; the edits were placed by text search.

## Notes for the next batch
- The durable test uses a hand-written fake queue; the Rust `PostgresRequestQueue` contract is not checked against it (depth file).
- WS14.S3 and S4 flag defaults (`observe`) conflict with the build brief's "dark by default" wording; the owner decides before S3 merges (depth file).
- Robots identity has three strings today; ADR-5 and WS14.S4 must choose one (depth file).
- Batch 3 units that touch `docs/CONTRACTS.md` or `apps/STALE_DOC_DELETION_REGISTER.md` must append, not edit in place (the queue's rule); this unit appended only.

Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U12.md`. Hand-over: `.plan-work/quarry-w1w2/wf-W2-U12.md`.
