# P1-ws3f hand-over: WS3.F (driver caps, crash relaunch, reaper, probe, /ready) + WS3.D leftovers

## Commits (local, quarryv2-next)
- 77749d34 feat(quarry-browser): slot book, pool config and BrowserHealth (slots.rs, lib.rs)
- b86a32c8 feat(quarry-browser): driver caps, crash relaunch, idle reaper and health probe; narrow the tab-attach lock (chromiumoxide.rs, incl. 5 live tests)
- 5102b059 feat(quarry-edge): /ready reports the local browser and answers 503 browser_unhealthy (routes.rs ready(), tests/ready_browser.rs)
- 37b7ebb0 docs(quarry): ERROR_REASONS.md + RUNBOOKS.md appended sections

## Verified (all in the Linux container via qtest unless noted)
- CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide --lib: 110 passed, 0 failed, 4 ignored (pre-existing public-origin + measurement ignores). All 5 new live tests pass.
- cargo test -p quarry-edge --test ready_browser (3 passed) --test zdr_behavior (4 passed) --test log_field_discipline (11 passed).
- cargo check --workspace --all-targets: rc 0 (warnings only in fleet_routes.rs and run_sweeper_integration.rs, not ours).
- Native Windows: quarry-browser builds and its lib tests run with CARGO_FEATURE_PURE=1 (blake3) and a scratch target dir; quarry-edge does NOT build natively (boring-sys2 build script runs `git init` and fails), use docker for it.

## Not done
- R-26 boot check (session cap > MAX_CONCURRENT_RUNS + heavy lane) not wired (plan forbids main.rs edits in this slice).
- docs/openapi.yaml /ready shape (outside owned paths). Gateway mapping of browser_session_cap/browser_page_cap/session_reaped/browser_process_lost.
- Plan's deployment evidence (rebuilt edge, pkill, 13 runs at MAX_SESSIONS=12, hidden binary) not run.
- K > 1 (WS3.G).

## Traps
- RUNBOOKS.md is shared append-only: commit 37b7ebb0 also carried another unit's complete "## Scrape cleaning cost" section that was appended in the tree after mine (it was uncommitted when qcommit staged the whole file). That unit will find nothing left to commit in RUNBOOKS.md for that hunk.
- The stale-directory sweep skips the edge's own pid; in a container that always runs as pid 1 it will not sweep a previous container's leftovers on a shared volume.
- Live tests that need deterministic behaviour turn the background reaper/prober off with with_pool_config(BrowserPoolConfig { reaper: ReaperMode::Off, .. }).
- The navigation-race live test makes the race deterministic by pinning the pre-navigation execution context id on the first attempt; Chromium's real error text matched the retry signatures.

## Review-fix round, 2026-10-18 (c8c31a26)
- REAPER=0 now keeps the prober running. The prober is the only path that relaunches a crashed browser when /ready has taken the edge out of traffic.
- teardown_session takes the session out of the book first, under a per-key teardown gate. acquire waits for that gate. The reaper reaps one key at a time under the gate, through SlotBook::reap_candidates and reap_if_due.
- AdmissionRollback in acquire gives a fresh admission back when the call errors or is dropped.
- live_frame and invalidate_live_frame_cache create no cache slot for a released session.
- TestHooks covers four test-only hooks: the attach stall, the teardown stall, the live_frame stall and a stale-context content read. Mutant runs confirm that each new test catches the defect it targets.
- Trap: another unit's docs commit (c2b3d2e9) swept up my uncommitted RUNBOOKS edits. Commit docs straight after you edit them.
- Correction: 77749d34's "110 passed" belongs to the b86a32c8 tree. The 77749d34 tree itself has 105 passed and 4 ignored.
- Left: a drop guard in quarry-runtime BrowserDriverAdapter::do_fetch (outside my paths); R-26 boot check; openapi /ready; gateway reason mapping.

## Review round 2 (2026-10-18): 60e32caf, b2db250e
- Probe flake cause confirmed: Op::HealthProbe 3 s deadline Timeout under ~20 parallel live tests (2 of 6 runs). Deadline kept; the test retries up to 3 probes and only tolerates Timeout.
- AdmissionRollback is now per entry id with a pending/committed count in SlotBook: a joining acquire keeps the entry when the first acquire is dropped.
- RUNBOOKS: appended a "Superseded" section for the 329d2b98 "At HEAD" sentences.
- Trap: tokio::pin!(fut) then drop(fut) drops only the Pin<&mut>, not the future; a test that means to drop a future mid-flight must Box::pin it. A first version of the joiner test hung the whole suite on this.
