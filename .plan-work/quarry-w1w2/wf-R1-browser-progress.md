# R1-browser progress

Started 2026-10-18. Scratch: scratchpad/R1-browser/ (edit scripts *.js, logs *.log).
Committed: none yet.

## Task 1 flaky (in progress, uncommitted, mine: crates/quarry-browser/src/chromiumoxide.rs)
Evidence (scratchpad/R1-browser/probe-evidence.log, 3 instrumented live runs): the attach's
`browser.new_page` held the driver's browser Mutex up to 4208 ms under the parallel live suite;
the helpers waited up to 3525 ms for it. Cause = lock held across new_page (TabAttach 15 s deadline).
Fix: browser handle is RwLock; CDP calls read(); launch/kill write(); prober exit check try_write();
ensure_browser read fast path. Tests: attach_stalled hook (target appears before new_page returns),
release_during_attach teardown stall 10 s, captures_live_frame timeout 5000.
Running: 3 live runs -> R1-browser/flaky3.log.

## Other uncommitted work of mine
- quarry-runtime/src/browser_driver.rs: HeldSession drop guard + 2 tests; browser_health override.
- quarry-runtime/src/driver.rs (Driver::browser_health default), driver_registry.rs (browser_health) - not my paths, additive.
- quarry-edge/src/routes.rs ready(): browser + scrape_browser, 503 on either Unhealthy.
- quarry-edge/tests/ready_browser.rs: 3 new tests (scrape 503, degraded, documented keys).
- docs/openapi.yaml /ready (file also carries ANOTHER unit's 2 challenge hunks at /v1/scrape 403 and components ~4005: do not commit until they are committed or confirmed complete).

## Next
races 2a (owner id on live frame slots; page_closed(key,id)), R-26 pure fn in slots.rs + main.rs call, compile edge, tests, commits, 10-run loop.

## Round 3 update (2026-10-19)
- COMMITTED: 7420d3a6 flaky fix (RwLock).
- Uncommitted in tree (found at round 3 start): races 2a (LiveFrameCache + owner ids, page_closed(key,id), tests in chromiumoxide.rs/slots.rs),
  2b HeldSession drop guard + 2 tests (quarry-runtime browser_driver.rs), /ready both drivers (routes.rs, driver.rs, driver_registry.rs), ready_browser.rs tests, openapi /ready.
- main.rs diff in tree is R5's (zdr-log-ok-begin), not mine. R-26 boot check NOT yet done.
- Next: compile + run tests for races/adapter; commit races; R-26; commit contract; 10-run loop.
- committed 0b86c8c6 (races+adapter guard+R-26 pure fn). next: commit 3 ready/boot, 10-run loop

## Round 3 review-fix pass (2026-10-19)
All 3 earlier commits exist (7420d3a6, 0b86c8c6, a4447aff); 10-run live loop of old code passed 10/10.
Fixing 3 review findings (uncommitted, mine): 
- quarry-runtime/src/browser_driver.rs: finish() runs release on a spawned task (cancel-safe) + test a_fetch_cancelled_during_release_still_completes_the_release (passed).
- quarry-browser/src/chromiumoxide.rs: TestHooks goto_hold/close_tab_hold + live tests a_late_goto_..., close_tab_gives_the_page_back_..., non-live a_page_reservation_and_its_attach_guard_...
- quarry-edge/tests/ready_browser.rs: new ready_bodies_match_the_documented_schemas_both_ways (passes; mutant healthy-skip fails it). 
Running: live loop x10 -> scratchpad/R1-browser/loop3.log (marker QDONE). Then cargo check --workspace, log lint, node tests, commit (2 commits: adapter finish; wiring tests + ready test).
- committed 86b546e6, 46157832 (review fixes); all done

## Round 3 review-fix pass 2 (2026-10-19)
- committed e78b562c: a driver release that panics poisons the lease (nested spawn in HeldSession::finish) + test a_panicking_release_poisons_the_lease_and_frees_the_slot (mutation check fails without the inner spawn). quarry-runtime browser_driver: 16 passed.
- Full live quarry-browser suite after that: first 6 runs (cargo test directly after the commit) FAILED 1-5 tests each (close_tab_gives_the_page_back..., a_late_goto..., current_page expects); after my temporary eprintln instrumentation of slots.rs (removed again, no diff) forced a rebuild, the same test passed 6/6 and the full suite passed 5/5 (129 passed, 0 failed, 4 ignored). The earlier failures are consistent with a stale or mixed incremental build in the shared quarry-target volume; not proven.
- cargo check --workspace --all-targets: Finished (only a pre-existing unused import FleetStatus warning, not ours).
- Live-lane CI gating (quarry-browser-live is advisory) is outside this unit; see doc-fixes note.
