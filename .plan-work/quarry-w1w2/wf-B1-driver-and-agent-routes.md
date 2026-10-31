# B1-driver-and-agent-routes hand-over (unit B1, round 4): DONE, all committed

Commits: c92f42a3 (insert_live_run + evaluate counter), 3b167c50 (quarry-browser deadlines, profile dirs, shutdown, main.rs call), a989fbb7 (WS2.B split, WS2.C taken_over_at, WS2.D system expiry and drain, Postgres tests, docs), 1c4e8f7e (agent_evaluate_denied, agent_step_timeout, harness), 9a640c35 (RUNBOOKS section).

Everything the earlier progress file listed as "NOT RUN" has now been run (container, quarry-rust-env:2): edge lib 394, agent_run_control 33, run_sweeper_integration 25, agent_evaluate_denied 6, agent_step_timeout 2, start_run_grants 7, browser_grants_prod 2, log_field_discipline 22; runtime step_receipts 20; Postgres postgres_step_receipts 7 (real database, REQUIRE_CONTRACT=1); quarry-browser live lane x3: 181 passed each. Mutations observed (deny_raw_script, taken_over_at).

Traps found this round
- agent_step_timeout: a step queued behind a stuck one is already counted in in_flight (guard taken at step start), so the count is 2 while it waits.
- Relaunch after a crash removed the dead browser's directories once; a renderer was still writing; fixed with remove_after_kill (retry thread) in retire_browser and the live test polls.
- Postgres test harness: scratchpad/B1/pgrun.sh (b1-pg + b1-net, run under with-lock.sh docker); `scratchpad/tools/pg-test.sh` does not take the docker lock.
- CRLF: git-bash `cat -A` hid ^M on admission.rs; sed on end-of-line anchors needs `\r`.
- qtest.sh now uses quarry-rust-env:2 (clippy, rustfmt). clippy exits 0 but is not a Quarry CI gate; 93 warning lines remain across quarry-edge/browser/runtime, none new in this unit except the fetch_update deprecation (pre-existing).

Not done: Atomic::fetch_update -> try_update (newest toolchain only); real async teardown on the Drop path; the post-504 tests pin existing behaviour (not run against a version without it).

## Round 5 (review fixes) - DONE, committed 11fc1e8f, 14015ab1, 7e3aa477
- HIGH page.metrics() unbounded: real (telemetry, chromiumoxide.rs). Now Op::Metrics 3 s. The claim in 3b167c50's subject / deadlines.rs / RUNBOOKS was false; docs corrected, subject cannot be amended (stated in the new commit message).
- MED scan too narrow: real. cdp_calls_are_bounded now goes by receiver (page, guarded_page, dialog_page, element, browser) with an explicit NOT_CDP list (with reasons), literal-skipping, and BOUNDED_BY_THE_CALLER checked at each call site. Mutation observed (metrics wrapper off -> fails naming telemetry).
- MED log lower bound: real. Exactly three lines naming the test's run. Mutation observed (run_id removed -> 2 of 3).
- MED update_tab_metadata overwrote hints with None: real. keep_last_known + bounded_hint; test forces the timeout under paused time (helper-level; not end to end). RUNBOOKS corrected (title is Content, not TabMetadata).
- MED shutdown relaunch: real, both paths. shutdown sets shut_down, aborts+awaits background tasks, takes the write lock BEFORE aborting the handler. Live test no_browser_comes_back_after_shutdown + native twin; mutation observed (second launch). The in-between-launch race has no test of its own.
- Extra found while testing: the profile-dir live test failed in ~3/9 full runs (a dying renderer recreated the dir after the first removal). Fix: always-on cleanup thread (75 attempts) + 500 ms settle; shutdown hands leftovers to it. Unit test with mutation (settle 0 fails).
Verified: native quarry-browser lib 183 passed (pre-last-change); live lane 4 x 185 passed/0 failed/4 ignored after the last change; edge agent_evaluate_denied 6 passed; clippy -p quarry-browser: no warning in the two files.
Seen, not chased: WS3.K a_control_under_its_own_label_passes_the_hit_test (target_unstable) once; one loaded-host run with 5 failures (output cut off).
