# W2-U69 progress: WS14.S8 rollout closeout (partial)

Unit W2-U69, gated group G3, wave 2 (build first, depth later). Branch `quarryv2-next`, worktree `vigorous-perlman-e7ae5b`. Slice WS14.S8 (census WS14.I). Status: PARTIAL. The docs part of the slice is built. The flip, the legacy removal and the test removal are not built, because each is gated (see "Not done").

## Gate check (2026-10-24)

- G2-a..c (S2 to S6) are committed: S2 `9bee0f4d5`..`1b4ff9384` area (U20), S3 `ace74d619`..`ca8569508` (U60), S4 `114ae9e50`, `081c442a8`, `c4b08e4b7` (U63), S5 `7edf11250`..`832feaf08` (U64), S6 `6dd1456ce`, `614df6ad7`, `a30a01d3a` (U65). The unit's gate is met.
- The flip gate is NOT met: X-10 is not among the 13 approved decisions (plan 2.1 and 7.1) and not in `DECISIONS-2026-10-23.md`. The plan also needs the soak data from `observe`, which does not exist.

## Built

| Item (plan S8 file list) | Result |
|---|---|
| `docs/ROADMAP.md` Phase 5 status line | Built: status of S2 to S6, the defaults in observe, what is still open, and the CrawlFrontier keep-or-delete note. |
| `apps/STALE_DOC_DELETION_REGISTER.md` | Built: section "WS14.S8 (2026-10-24)" with one row for `CrawlFrontier` (no production caller). Appended, CRLF kept. |

Commits: see the table in the final answer (filled after the commit).

## Not built (boundary, gated)

| Item | Why stopped | Who unblocks |
|---|---|---|
| `QUARRY_EDGE__CRAWL_SCOPE_MODE` and `QUARRY_EDGE__CRAWL_ROBOTS` default to `enforce` (`crawl_policy.rs`, `robots_gate.rs`; the plan names `docker-compose.yml`, which sets neither) | X-10 not approved; no soak data; no scope would-drop aggregate exists | The user (X-10); the soak run on a live stack |
| Delete the legacy branch and the `crawl-scope-v1` GetVersion in `workflows.go` | Needs the Temporal running-execution check on a live stack (outside the branch); a separate revertable commit per plan | Stack lane |
| Remove S0 characterization tests | Must go with the legacy branch (only pin of today's behaviour) | Same change as the branch removal |
| Replay test with histories from both builds | No recorded histories in the repo | Stack lane |
| Harness `scripts/e2e/crawl-scope-robots.sh --expect enforce` | Needs rebuilt images after the flip | Stack lane |

## Tests

- None written or run for this unit. The plan's named tests for S8 are the removal of S0 tests, `go test ./internal/workflows -run CrawlJob` after the removal, and the Temporal check. All sit behind the gate above. No code changed, so no compile check was needed.

## For the next batch

- Read `depth/W2-U69.md` first. The two highest items: the soak aggregate has no source (no scope would-drop metric), and absent-scope crawls (scheduled crawls, stored params) have no defined behaviour once the legacy branch goes.
- The RUNBOOKS and CRAWL_SCOPE_ROBOTS env-name mismatch (`QUARRY_EDGE__CRAWL_ROBOTS_UNREACHABLE` documented, `QUARRY_EDGE__ROBOTS_UNREACHABLE` in code) is still open.
- Attribution: the session attribution rule asks for the Haiku 5.5 co-author line; the unit text asked for the Sonnet 5.5 line. The commits use the session rule's line.
