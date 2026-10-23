# R1-browser hand-over

Commits: 7420d3a6 (flaky fix, RwLock), 0b86c8c6 (races, adapter drop guard, R-26 pure fn), a4447aff (/ready both drivers, openapi, tests).
Final tree, live lane x10 (CHROMIUMOXIDE_TEST=1, --features chromiumoxide --lib): 10/10 runs 126 passed 0 failed 4 ignored (log: scratchpad/R1-browser/loop10.log).

Root cause of the flake: a tab attach held the driver's browser Mutex across browser.new_page (up to 4.2 s under the parallel suite); the test helpers context_count/page_target_count waited up to 3.5 s behind it and hit their 5 s expect. Fix = RwLock (CDP calls share read).
Races: page_closed(key, admission_id); LiveFrameCache with owner ids; HeldSession guard in BrowserDriverAdapter::do_fetch.
/ready: `browser` (agent driver) + `scrape_browser` (scrape driver); 503 if either Unhealthy. openapi documents it; test decodes the real bodies with deny_unknown_fields.
R-26: slots::session_cap_warnings(max_sessions, max_runs, heavy_lane) WARN-only (the plan says WARN until the RSS is recorded, then fail); HEAVY_LANE_BUDGET=1 constant until the heavy lane exists. Called in main.rs after the run-limit ordering warnings.

Traps:
- R5's commit e50046a9 swept my main.rs hunk (calls slots::session_cap_warnings) before the function was committed (0b86c8c6): e50046a9 alone does not build.
- No mutation run (tests not run against reverted fixes).
- openapi.yaml commit a4447aff also carried R6-challenge's two uncommitted description hunks.
- Doc sentences for R8b: wf-R1-browser-doc-fixes.md.

## Round 3 review fixes (2026-10-19)
- 86b546e6: HeldSession::finish runs the release on a spawned task (cancel during release no longer leaks; slot returns after release).
- 46157832: live wiring tests for goto / close_tab / reserve_page+AttachGuard with TestHooks goto_hold / close_tab_hold (mutants caught); /ready test now compares real bodies with the openapi /ready schemas both ways (mutant caught).
- Live lane 10/10 green after the change (129 passed, 4 ignored each run). No doc behaviour changed, nothing new for wf-R1-browser-doc-fixes.md.
- Trap: the `Bash run_in_background` of a `( ... ) &` subshell returns at once; wait with qwait.sh on the log marker.

## Round 3 pass 2 addendum
- e78b562c closes the review finding on a panicking release (lease now poisoned). Full live suite 5/5 green (129 passed) after a clean rebuild; six earlier runs failed 1-5 tests each on what looked like a stale shared target volume (the same tests then passed 6/6 when slots.rs was touched). If this recurs, touch the crate sources or use a fresh QTARGET volume before assuming a code defect.
- The wiring tests in quarry-browser are live-gated and only run in the advisory quarry-browser-live lane; promoting it to required is a CI-owner decision.
