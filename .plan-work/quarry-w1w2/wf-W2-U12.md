# W2-U12 hand-over (WS14.S0, WS14.S0b)

## What was done
- WS14.S0b: `go-test` job added to `.github/workflows/quarry-v2-ci.yml` (commit 2489e2b63). Matrix: quarry-control, quarry-orchestrator, pkg/quarrycontracts. Blocking, no coverage threshold.
- WS14.S0 Go: two characterization tests (commit ac0627b8e): `TestCrawlJobWF_CharacterizesExternalLinkFollowing_Legacy` in `workflows_test.go` (appended) and `TestCrawlJobDurable_EnqueuesEveryLinkWithOneActivityEach` in `workflows_durable_test.go` (new).
- WS14.S0 docs (commit 73604c4d8): GAP.md rows corrected to Open; Features.md, gap-quarry.md, ARCHITECTURE.md, POLICY.md corrected; CONTRACTS.md appended section; `docs/CRAWL_SCOPE_ROBOTS.md` new (the WS14 contract); `apps/STALE_DOC_DELETION_REGISTER.md` appended section.
- RUNBOOKS section (commit 9ecd83519), committed with the HEAD-blob helper because the working copy holds other units' uncommitted sections.

## What is left
- Test phase: run `go test ./internal/workflows/ -run CrawlJob -count=1` in services/quarry-orchestrator, and `go test ./...` in the three go-test modules. Expect the two new tests to pass on unmodified code; they have not been run.
- Push and branch protection: list the `go-test (...)` checks as required. Repository setting, not in this branch.
- Ingestion owner review of CRAWL_SCOPE_ROBOTS.md (definition of done).
- GAP.md: a full audit of other crawl rows (depth file).

## Traps found
- The plan's CONTRACTS.md and Features.md line numbers are stale. Use grep.
- `docs/RUNBOOKS.md`: HEAD is LF, the working copy is CRLF. Any `git diff` on it shows whole-file churn. Commit it only through qcommit-blobs.sh.
- Temporal testsuite mocks: a function return must have exactly the activity's function type (`getMockFn` panics otherwise).
- `gofmt -l` on Windows lists the existing CRLF files; that is a line-ending report, not a format error.
- The attribution line used in the commits is the Haiku 5.5 one from the session rule, not the Sonnet line in the task text.

## Files that are mine and committed
- services/quarry-orchestrator/internal/workflows/workflows_test.go (appended)
- services/quarry-orchestrator/internal/workflows/workflows_durable_test.go (new)
- .github/workflows/quarry-v2-ci.yml (go-test job)
- docs/GAP.md, docs/Features.md, docs/gap-quarry.md, docs/ARCHITECTURE.md, docs/POLICY.md, docs/CONTRACTS.md (appended), docs/CRAWL_SCOPE_ROBOTS.md (new), apps/STALE_DOC_DELETION_REGISTER.md (appended)
- docs/RUNBOOKS.md (one appended section; the commit carries no other unit's hunk)
