# P1-ws3f progress (WS3.F + WS3.D leftovers)

Status: DONE for code and tests; see wf-P1-ws3f.md for the hand-over.

Committed: 77749d34 (slots + lib), b86a32c8 (driver + live tests), 5102b059 (/ready + ready_browser.rs), 37b7ebb0 (docs).
Uncommitted files of this unit: none.

Test results (container): quarry-browser lib with live Chromium 110 passed / 4 ignored; quarry-edge ready_browser 3, zdr_behavior 4, log_field_discipline 11 passed; cargo check --workspace --all-targets rc 0.

Left: R-26 boot check, openapi.yaml /ready, gateway mapping, deployment evidence, WS3.G.

## Review-fix round (2026-10-18)
In progress (uncommitted, mine): crates/quarry-browser/src/{chromiumoxide.rs,slots.rs}, docs/RUNBOOKS.md (3 exact-anchor line edits; the file also carries another unit's 8-line hunk).
Fixes: prober runs with REAPER=0; teardown releases the book first under a per-key teardown gate (acquire waits for it); reaper reaps one key at a time under the gate (slots reap_candidates/reap_if_due); AdmissionRollback in acquire; live_frame/invalidate never create a slot for a released session; TestHooks (attach/teardown/live_frame stalls, stale content context); new tests for caps, typed losses, unhealthy-keep/recycle, REAPER=0 crash, release during attach, live frame after release, dropped acquire; lock-bounded context/page count helpers; nav-race test goes through driver.content().
Compile: cargo test -p quarry-browser --features chromiumoxide --lib --no-run rc 0, no warnings.
Committed c8c31a26 (review fixes). Uncommitted files of this unit: none.
Results: quarry-browser lib live 119 passed/0 failed/4 ignored; 8 mutants each killed by its targeted test; edge ready_browser 3, zdr_behavior 4, log_field_discipline 11; cargo check --workspace --all-targets rc 0.

## Review round 2 (2026-10-18)
In progress (uncommitted, mine): crates/quarry-browser/src/{chromiumoxide.rs,slots.rs}, docs/RUNBOOKS.md (appended "Superseded" section).
Fixes: AdmissionRollback by entry id with pending/committed counts in SlotBook (acquire_finished/acquire_abandoned/admission_id) so a joiner keeps the entry; probe test retries up to 3 probes tolerating only Timeout, probe errors captured by TestHooks.probe_errors; new tests abandoned_acquire_gives_back_only_an_entry_nobody_else_holds (slots) and acquire_joining_a_dropped_launch_keeps_the_admission (live).
Test: six full live runs of quarry-browser --lib with default threads (log scratchpad/P1-ws3f/run1.log).
Committed 60e32caf (joiner-safe admission rollback + probe test retry), b2db250e (RUNBOOKS superseded note). Uncommitted files of this unit: none.
Results: 6 full live runs of quarry-browser --lib (default threads): each 121 passed/0 failed/4 ignored; runs 1 and 4 logged one probe Timeout ("browser operation health_probe timed out") then passed. Mutant (acquire_abandoned ignoring pending) failed both new tests. quarry-edge ready_browser 3, zdr_behavior 4, log_field_discipline 11; cargo check --workspace --all-targets rc 0.
