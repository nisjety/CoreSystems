# B1-driver-and-agent-routes progress (unit B1, round 4)

State at the last write: Docker Desktop stopped answering (`docker info` times out, also `docker ps`), so NOTHING after commit c92f42a3 is committed: the pre-commit hook runs gitleaks through `docker run` (no local gitleaks), and `--no-verify` is forbidden. All code below is in the working tree, uncommitted, and verified as stated. Do not discard it.

Scratch: `scratchpad/B1/` (hunk-pick.js makes a hunk-only patch; dockerwait.sh polls the engine).
Edit traps found: a heredoc in the Bash tool collapses `\\` to `\` (use the Write tool for scripts that contain backslashes); quarry-edge and quarry-runtime do NOT build natively on this host (boring-sys needs libclang, aws-lc cc fails), only quarry-browser builds natively (`CARGO_TARGET_DIR=<tmp> cargo test -p quarry-browser --features chromiumoxide --lib`); live Chromium tests need the container.

## Committed
- c92f42a3 test(quarry-edge): `insert_live_run` (test_support.rs) + RecordingDriver `evaluate` counter (agent_harness.rs) + the in-module step test uses the helper. Verified: qtest `--lib` 383 passed (before later edits); mutation (deny_raw_script condition forced false) made the two `deny_raw_script` unit tests fail.

## Uncommitted, by item (whole files are mine unless noted)
m1 WS2.A
- NEW `crates/quarry-edge/tests/agent_evaluate_denied.rs` (6 tests: step, interact, labels, service principal, WebSocket, log). VERIFIED qtest: 6 passed (twice). Integration-level mutation proof NOT observed (the first attempt printed nothing, the second was queued when Docker stopped; the lib-level mutation was observed).
m2 WS2.C
- `agent_routes/run_live.rs`: `taken_over_ms` + `taken_over_at_ms()`; `admission.rs`: `user_took_over(snapshot, taken_over_at_ms)` adds `details.taken_over_at` (RFC 3339, absent after a resume); doc on `admit_tab_operation` states the documented exception (tab routes keep `admit_tab_operation`; open revalidates grant/domain/URL itself; select and close do not ask the broker). `docs/openapi.yaml` (2 places) and `docs/CONTRACTS.md` (the takeover sentence).
- tests: `tests/agent_run_control.rs` +3 (`a_takeover_refusal_says_when_the_person_took_the_browser`, `a_service_principal_is_admitted_again_after_the_browser_is_handed_back`, `a_revoked_grant_stops_a_tab_open_and_an_action_but_not_a_tab_select_or_close`); `agent_routes/tests.rs` +2 unit tests; VERIFIED qtest run3: agent_run_control 33 passed, lib 394 passed.
- Migration idempotency: `quarry-runtime/src/postgres_step_receipts.rs` `migrations_0010_and_0011_apply_twice_and_leave_rows_alone` (+ a lifecycle round trip). NOT RUN against Postgres (DATABASE_URL unset in qtest: the tests skip and "pass"). Needs: a throwaway `postgres:16-alpine` named `b1-pg` (trust auth, published port) under the docker lock, then qtest with `DATABASE_URL=postgres://postgres@host.docker.internal:<port>/postgres REQUIRE_CONTRACT=1 cargo test -p quarry-runtime --features postgres-queue --lib postgres_step_receipts`, then remove the container.
m3 WS2.D
- `quarry-runtime/src/step_receipts.rs` (`Lifecycle { state, initiated_by }`, serde default/skip), `agent_routes/{types,projection,lifecycle}.rs` (expiry sets `System`; the owner timeline item says `initiatedBy: "system"`), `drain_runs` rewritten (no `Arc::try_unwrap`: waits bounded for a running action, marks the run closed, releases a session clone; `in_use` now means an action held the lock past the deadline). Tests: `tests/run_sweeper_integration.rs` (+1 new, 1 extended), `tests.rs` (drain tests replaced/added), step_receipts unit test. VERIFIED qtest run3: run_sweeper_integration 24 passed, step_receipts 20 passed, lib 394 passed. CONTRACTS.md sentence.
n1 WS3.D + e WS3.F (quarry-browser)
- `deadlines.rs`: new ops `TabMetadata` 3 s, `LiveFrame` 5 s, `GuardResolve` 5 s; the named exceptions are in the module doc. `chromiumoxide.rs`: the 8 census functions are bounded (`update_tab_metadata`/`list_tabs` via `page_url`, `install_network_guard` Fetch.enable + guard resolutions, `hydrate_cookies`, `hydrate_storage`, `install_popup_bridge`, `capture_live_frame` with typed `Timeout` for the frame wait through `next_live_frame`, `root_frame_id`); `with_profile_root` + `chromium_launch_dirs_in` + `sweep_stale_launch_dirs_in`; tests `bounded_calls_and_launch_dirs::*` (5 native + 1 linux-only sweep test) and the live `one_profile_directory_per_browser_launch_and_none_left_behind` (inside `mod tests`).
- VERIFIED natively on Windows (`cargo test -p quarry-browser --features chromiumoxide --lib -- bounded_calls_and_launch_dirs deadlines`): 15 passed incl. `cdp_calls_are_bounded`; the same scan run over the HEAD source (`git show HEAD:...chromiumoxide.rs`) fails and names exactly the 8 census functions (update_tab_metadata, install_network_guard, hydrate_cookies, hydrate_storage, install_popup_bridge, capture_live_frame, list_tabs, root_frame_id). NOT RUN: the linux-only sweep test, the live profile-directory test, the live lane x3 (needs Docker).
Post-504 step check
- NEW `tests/agent_step_timeout.rs` + `RecordingDriver::time_out_next_goto` (agent_harness.rs). NOT COMPILED, NOT RUN (written after the last qtest).
m4 WS2.B
- `quarry-rust-env:2` (clippy + rustfmt) NOT built: the build was started under the docker lock and killed when the engine hung (an empty log; `docker build` printed nothing). Dockerfile is `scratchpad/B1/img/Dockerfile` (`FROM quarry-rust-env:1` + `RUN rustup component add clippy rustfmt`). `qtest.sh` still uses `:1`. clippy and `cargo doc` were never run.
- Size: lifecycle.rs is 775 lines and types.rs 680 against "about 650". Not split: other units' uncommitted hunks sat in lifecycle.rs during this round (moving code would have conflicted). A split of the close/drain half (about 190 lines: close_run, CloseReason, finish_run, drain_runs) into a `close.rs` is the clean way to get under 650; no exception is recorded in RUNBOOKS yet.

## To finish (in this order, once `docker info` answers)
1. `bash scratchpad/tools/qtest.sh 'cargo test --no-fail-fast -p quarry-edge --lib --test agent_run_control --test run_sweeper_integration --test agent_evaluate_denied --test agent_step_timeout --test start_run_grants --test browser_grants_prod 2>&1 | grep -E "^test result|FAILED|^error|panicked" -A7'` and `cargo test -p quarry-browser --features chromiumoxide --lib` and `cargo test -p quarry-runtime --lib step_receipts`.
2. Mutation proofs (inside the container, restore with a trap): `deny_raw_script` off -> agent_evaluate_denied fails; `taken_over_at` omitted -> the new control test fails; drain back to try_unwrap is not worth a mutation.
3. Postgres run (see m2). 4. Live lane three times: `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide --lib` (record 3 pass counts).
5. Build `quarry-rust-env:2`, run `cargo clippy -p quarry-edge -p quarry-browser --all-targets -- -D warnings` and `cargo doc --no-deps -p quarry-edge -p quarry-browser`; only then point qtest.sh at `:2`.
6. Commit with `qcommit.sh` (whole files, mine only): (a) tests/agent_evaluate_denied.rs; (b) WS2.C+WS2.D files listed above (run_live, admission, lifecycle, projection, types, tests.rs, agent_run_control, run_sweeper_integration, step_receipts, postgres_step_receipts, openapi.yaml, CONTRACTS.md); (c) quarry-browser deadlines.rs + chromiumoxide.rs; (d) agent_step_timeout.rs + agent_harness.rs. Never add: main.rs, grant_boot.rs, browser_grants_prod.rs, go.work.sum, .plan-work.
7. RUNBOOKS append (not yet written) and the final notes file `wf-B1-driver-and-agent-routes.md`.

## Round 4 fix pass (after the review findings), still no Docker
Docker did not answer (`docker info` rc=143) at the start of this pass. Done without it, all UNCOMMITTED:
- quarry-browser chromiumoxide.rs: `cdp_calls_are_bounded` is now per call (paren-matched, string-aware: `bare_cdp_calls`), the fixture covers a bare call next to a wrapped one and a bracket inside a string; deadlines.rs module doc narrowed to what the scan proves. Native: `CARGO_TARGET_DIR=C:/b1t cargo test -p quarry-browser --features chromiumoxide --lib` 179 passed, 0 failed, 4 ignored.
- quarry-browser: `ChromiumLaunchDirs::remove_after_kill` (Drop retries removal from a thread, 25 x 200 ms) + test `removal_after_a_kill_is_retried_until_the_directories_are_gone` (native, passes); the live profile-dir test now polls (`eventually` 10 s) after `drop(driver)`. LIVE NOT RUN.
- quarry-edge (NOT COMPILED, NOT RUN): harness `time_out_next_goto_after(hold)` + test `a_step_queued_behind_a_navigation_that_is_stuck_runs_right_after_its_504` (agent_step_timeout.rs); integration drain test `the_shutdown_drain_releases_every_idle_runs_session_and_leaves_a_busy_one` (run_sweeper_integration.rs) using RecordingDriver::released(); unit drain tests renamed in agent_routes/tests.rs.
Still to do once docker answers: step 1,3,4,5,6 of the list above (qtest of the edge tests incl. the two new ones, Postgres run, live lane x3, then commits in groups a-d + the quarry-browser group).

## Round 4 final (all committed, see wf-B1-driver-and-agent-routes.md)
3b167c50, a989fbb7, 1c4e8f7e, 9a640c35 on top of c92f42a3. Nothing of this unit is uncommitted.

## Round 5 fix pass (review findings: 1 high, 4 medium) - ALL COMMITTED
Commits: 11fc1e8f (quarry-browser code), 14015ab1 (agent_evaluate_denied), 7e3aa477 (RUNBOOKS hunk only; C2's section left uncommitted).
Nothing of this unit is uncommitted. Trap: scratchpad/lib-edit.js was truncated to 0 bytes by another agent mid-run; a private copy is scratchpad/B1/lib-edit.js.
