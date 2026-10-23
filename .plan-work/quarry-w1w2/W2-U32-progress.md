# W2-U32 progress: WS6.H job admission (row-locking claim, leased concurrency)

Unit W2-U32 (batch 6). Branch quarryv2-next, worktree vigorous-perlman-e7ae5b. Built to scope, committed, NOT run (wave build strategy). Depth items: `depth/W2-U32.md`. Hand-over: `wf-W2-U32.md`.

## Starting state (what W1 had already built)

Commit `40cb38df4` (W1 era, "job admission ... (WS6.H, off by default)") already carries migration 019, the claim endpoint, the bounded backlog (429 `org_backlog_full`), the TTL janitor, the dispatcher claim with 404 fallback, the edge typed 429, and the gateway `crawl_backlog_full` mapping. Its own "Not done" named two things: the row-locking claim and the plan-dependent cap stamp. U32 builds the first and records the second as a boundary.

## Commits (in order)

| Commit | Scope |
|---|---|
| cc75bdf33 | control: `ClaimAccepted` takes `pg_advisory_xact_lock` (key `jobClaimLockKey`) in one transaction before the claim statement, so concurrent claimers serialise; `TeamUsage().Concurrency` counts running plus leased accepted jobs. New `store/pg/job_claim_race_test.go` (two tests, Postgres, skip without `QUARRY_TEST_DSN`). |
| 0018ac497 | quarry-edge test `zdr_crawl_is_still_refused_so_no_backlog_row_exists` appended to `tests/job_admission.rs` (control double counts hits; asserts 403 and zero hits). |
| 3b8e841f2 | docs RUNBOOKS: WS6.H claim section; the "Limits to know" sentence that said the claim had no row locking is corrected. Committed hunk-only from the HEAD copy (`qcommit-patch.sh`). |
| 054d4b8c7 | docs openapi: typed 429 response on `POST /v1/crawl` and `POST /v1/batch` (additive, 12 lines). Committed hunk-only. |

## Verified (commands and results, all compile-only)

- `go vet ./internal/store/... ./internal/resources/...` in `apps/Ingestion Plane/Quarry-v2/services/quarry-control`: rc 0.
- `gofmt -l` on `job_claim_race_test.go` and on `resources.go` (CRLF stripped): clean. `resources_v2.go` reports an alignment issue in a snapshot scan function that predates U32 (not touched).
- `cargo check -p quarry-edge --lib --test job_admission` through qtest: rc 0.
- `cargo check -p quarry-edge --tests` through qtest: FAILED on the bin target, not in U32's files: `crates/quarry-edge/src/resource_routes.rs:32` imports `crate::job_stream`, which another unit (WS15.L3, U31) has not yet committed. Re-check later; the lib and the admission test target compile.

## Not run (by design of the wave)

- No Go test, no pg test (`QUARRY_TEST_DSN`), no Rust test. The first run is the wave's final test phase.
- No deployment evidence (the six points in the plan's WS6.H "Deployment evidence" paragraph).

## Not done (in scope of the queue row, outside U32 or blocked)

- Plan-dependent cap stamp: `handoff.rs` does not stamp `params.admission = {cap, backlog_cap}` from `state.limits`. Caps come from control defaults (`QUARRY_JOB_CAP_DEFAULT`, `QUARRY_JOB_BACKLOG_DEFAULT`). Needs WS6.D-s (plan values). Client input cannot set the cap: the crawl payload is built from typed fields only (`routes.rs` `crawl_handoff`, checked).
- Plan test names that exist under other names: `TestBacklogFullReturns429WithDetails` is `TestBacklogFullReturns429WithDetailsAndStoresNothing` (`resources/admission_test.go`); the four claim names are subtests of `storetest.AdmissionConformance` (`ClaimIsRoundRobinAcrossOrgs`, `ClaimRespectsPerOrgCapIncludingLeasedAndRunningJobs`, `OldestAcceptedIsNotStarvedByNewerJobsOfAnotherOrg`, `ClaimLeaseExpiresAndTheJobIsReclaimable`). `TestBacklogExpiryFailsOldJobsAndClosesTheirEventStream` and the dispatcher and gateway tests exist as named.
- REST_RESOURCES.md job-state text for admission (plan names it); QUEUE.md is the request-queue document, not the job queue, so it is not the right home.

## For the next batch

- Control `ClaimAccepted` now holds a transaction-scoped advisory lock (key 7245110311) for the whole claim. Anything else in the control DB that takes advisory locks must not reuse that key (the only other is `zdr_tombstones.go` `hashtext(...)`, a different key space).
- Team concurrency (`TeamUsage().Concurrency`) now includes leased accepted jobs when admission is on. Any UI that reads it sees queued-but-leased jobs as concurrent.
- Do not add a `params.admission` field to the public crawl or batch payloads; the cap is trusted from the stamp, and only the edge or internal callers may set it (depth item).
