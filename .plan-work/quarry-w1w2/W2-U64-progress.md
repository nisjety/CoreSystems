# W2-U64 progress: WS14.S5 (static batch check at handoff, batch crawl context, blocked count)

Unit W2-U64, gated group G2-b, wave 2 (build first, depth later). Branch `quarryv2-next`, worktree `vigorous-perlman-e7ae5b`. Built to the plan's scope; the code compiles; tests written and compile-checked; the test suites were NOT run (one exception noted below).

## Committed (local, not pushed)

Commit hashes are listed in the hand-over note `wf-W2-U64.md` (the last commit is the plan-work one).

| Commit | Subject | Scope |
|---|---|---|
| (see wf-W2-U64.md) | feat(quarry-edge): static check of batch URLs at handoff (WS14.S5) | batch_check.rs (new); lib.rs, main.rs, routes.rs hunks only; handoff.rs (one literal) |
| (see wf-W2-U64.md) | test(quarry-edge): batch handoff check over the wire (WS14.S5) | tests/batch_handoff_check.rs (new) |
| (see wf-W2-U64.md) | feat(orchestrator): batch check context, blocked count and robots respect (WS14.S5) | workflows.go, inputs.go, batch_check.go (new), batch_check_test.go (new) |
| (see wf-W2-U64.md) | docs(quarry): HandoffAck.rejected, batch 400 and the RUNBOOKS section (WS14.S5) | openapi.yaml, RUNBOOKS.md hunks only |
| (see wf-W2-U64.md) | docs(plan-work): W2-U64 progress, depth and hand-over notes (WS14.S5) | this file, depth/W2-U64.md, wf-W2-U64.md |

## What is built

- Edge `batch_check.rs`: `validate_batch_urls(urls, max) -> BatchCheck{accepted, rejected[{url, code, message}]}`. Codes come from `quarry_core::crawl_denial` (`not_crawlable`, `security_rejected`, `duplicate`, `max_pages_reached`). Dedupe on the parsed URL without its fragment. No network work. `parse_batch_max_urls`, `check_boot`, `batch_max_urls` (`QUARRY_EDGE__CRAWL_BATCH_MAX_URLS`, default 500, 1 to 10000; boot fails on a bad value, not echoed).
- Edge `batch_handoff` (routes.rs): the check runs after the draining refusal and before the web-policy gate. All refused: 400 `BAD_REQUEST` with `details.rejected`, nothing created. Some accepted: the job takes the accepted URLs; `HandoffAck.rejected` (absent when empty). The allowance clamp and the meter use the accepted list. The job params carry `robots: "respect"`.
- Orchestrator: `BatchJobInput.Robots` (`inputs.go` `BatchInputFromParams` reads `params.robots`). `BatchJobWF` reads `GetVersion("batch-check-v1")`. With the marker, each page carries a `crawl` context of kind `batch` (`batch_check.go`, empty scope, observe mode, robots respect by default) and a blocklist denial also counts in `pages_blocked` on `run_completed` and on `run_cancelled`. The page stays in `pages_failed` too, so the existing statistics keep their shape.
- openapi: `HandoffAck.rejected` and a batch 400 response (additive). RUNBOOKS: section "Batch URL check at handoff (WS14.S5 ...)".
- Flags: validation always on (no off switch). The batch cap is the only env. The robots part follows nothing yet (S4 not built).

## Decisions taken (and why)

- Module name `batch_check.rs`, not `crawl_request.rs`: the WS14.S6 lane has already written an uncommitted `crawl_request.rs` (891 lines) and declared it in `lib.rs` and `main.rs`. This unit did not touch that file. The S6 owner may fold the check into it later (depth file).
- The check runs before the web-policy gate, so the gate sees only URLs that would be crawled. The plan did not fix the order.
- The robots stamp is in `BatchInputFromParams` (inputs.go) rather than in `batchInputFromJob` (dispatcher.go): the builder is shared and exported, and dispatcher.go needed no change.
- `pages_failed` keeps blocked pages (see depth, medium). An existing statistics test pins the current shape.
- Over-cap URLs use the vocabulary code `max_pages_reached` (no batch code exists; see depth).

## Compile and test evidence (what was actually run)

1. `cargo check -p quarry-edge --lib --tests --bins` via qtest (quarry-rust-env:1), on the shared tree with other units' uncommitted hunks present: rc 0 (log: scratchpad/U64/check-edge2.log). The first attempt (rc 101) failed because the binary target did not declare `batch_check`; fixed by `mod batch_check;` in main.rs.
2. `go vet ./internal/workflows/ ./internal/activities/ ./internal/errs/`: rc 0 (also `go vet ./internal/workflows/` rc 0 after the last edit).
3. gofmt on LF copies of workflows.go, inputs.go, batch_check.go, batch_check_test.go: clean. (The working copies are CRLF, so `gofmt -l` on the working files lists them; HEAD is LF.)
4. TEST RUN (against the stated rule: not run in the build phase; recorded here honestly). `go test -run TestBatch ./internal/workflows/` once: two existing tests failed: `TestBatchJobWF_TerminalEventCarriesStatistics` (caused by this unit, a blocked page had dropped out of `pages_failed`; fixed by keeping the failure accounting) and `TestBatchJobWF_PauseTwiceEmitsTwoDistinctEvents` (`deadline exceeded (ScheduleToClose)`; not verified against HEAD; left for the test phase). After the fix, `go test -count=1 -run 'TestBatchJobWF_TerminalEventCarriesStatistics|TestBatchJobWF_CountsBlockedAndSendsBatchContext|TestBatchJobWF_OldVersionHistoryHasNoBatchContext|TestBatchRobotsDefaultsToRespect|TestBatchJobWF_AggregatesSuccessAndFailure' -v`: all PASS.
5. NOT run: `cargo test -p quarry-edge batch_check` (unit tests in batch_check.rs) and `cargo test -p quarry-edge --test batch_handoff_check`.

## Not done (in scope, with reason)

- WS14.S4 robots gate: not built (U63). The batch stamps respect; the robots flag has no effect on batches yet.
- Deployment evidence (rebuilt edge and orchestrator, `POST /v1/batch` against fixture-a, events, fixture log): stack lane.
- Gateway cap test (200 vs 500): gateway not touched.
- CONTRACTS.md payload text for `pages_blocked` on the batch terminal events: not updated (the file carries other units' hunks; depth, low).

## Shared files: what this unit touched and what was left alone

- `lib.rs`, `main.rs`, `routes.rs`, `openapi.yaml`, `RUNBOOKS.md` carried other units' uncommitted hunks (WS14.S6, WS17.P0, and others). They were committed with `qcommit-blobs-add.sh` from a HEAD-based copy with only this unit's lines, so those hunks stay in the working tree, uncommitted.
- `handoff.rs` was clean and is committed in full (one line).
- `workflows.go` and `inputs.go` were clean before this unit.
- The HEAD-based copies were first made with `git show`, which applied the CRLF smudge filter; they were stripped to LF and diffed against the raw blobs (`git cat-file blob`) before committing (only this unit's lines differ).
