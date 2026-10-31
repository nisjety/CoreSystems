# W2-U07 progress (WS8.E, driver foundation, dark)

Status: BUILT to scope, compile-checked, NOT run. Nothing is deployed and no caller exists.

## Committed

- 6ec6a34bb feat(quarry-browser): background tabs, tab pins and per-tab egress (WS8.E)
  - quarry-browser: new `src/tab_control.rs` (OpenTabOptions, addressed_tab_id, resolve_tab_egress_policy, visible_through_pin, PINNED_TAB_CLOSED_REASON, 11 unit tests); `lib.rs` (mod line, re-export, capability bits background_tabs / tab_pinning / per_tab_egress with serde defaults, BrowserSession.tab_pin with pinned_to and tab_pin, trait defaults open_tab / configure_tab_egress_policy / active_tab_id returning Unsupported); `chromiumoxide.rs` (tab_egress_policies map, open_tab and new_tab delegation, background open, pinned resolution in active_tab_page and current_tab_page, goto refuses a stand-in tab for a pin, guard resolves tab then session then deny-all, devtools / receipts / dialogs filtered by pin, respond_dialog refuses another tab's dialog, cleanup in close_tab / teardown / ensure_browser, seven live tests); tab_pin: None on every literal in browserbase.rs, browserless.rs, kernel.rs, pool.rs.
  - quarry-runtime: tab_pin: None in lib.rs, browser_driver.rs (4), observation.rs.
  - quarry-edge: tab_pin: None in agent_routes/tests.rs (2), src/test_support.rs (1), tests/support/agent_harness.rs (1).
- RUNBOOKS section "Shared browser driver foundation ... (WS8.E, U07, built, NOT run)" is in HEAD. It was committed in 61c4e8d6e, which also swept in other sessions' staged files and dropped another section's text; see wf-W2-U07.md, section "Incident".
- Depth bullets: .plan-work/quarry-w1w2/depth/W2-U07.md.

## Verified (what was actually run)

- `cargo check -p quarry-browser --features chromiumoxide --tests` (qtest): rc 0.
- `cargo check -p quarry-browser --tests` (qtest): rc 0.
- `cargo check -p quarry-runtime --tests` (qtest): rc 0.
- `cargo check -p quarry-edge --tests` (qtest): rc 0, after the job_lifecycle_routes module landed in the shared tree (an earlier run failed on that module, owned by another unit).
- `rustfmt --edition 2021 --check` on tab_control.rs: clean. On chromiumoxide.rs: the remaining diffs were checked and are pre-existing code at HEAD (imports, the devtools listener logs, the profile/attach tests), not in the WS8.E regions.

## Not run (by the build-first rule)

- `cargo test -p quarry-browser tab_control` (11 unit tests, no Chromium).
- The seven live tests, `--features chromiumoxide -- --ignored` with CHROMIUMOXIDE_TEST=1: background_tab_leaves_active_tab_unchanged, pinned_session_clicks_pinned_tab_not_active_tab, per_tab_policy_blocks_third_host_in_agent_tab_only, human_tab_keeps_open_web_while_agent_tab_is_narrow, closing_pinned_tab_does_not_fall_back_to_active, pinned_observation_never_contains_other_tab_events, background_tab_is_not_throttled (its floor of 10 ticks is not measured).
- WS8.A measurements (F3 block count, broker validation latency, two-tab RSS): not taken.

## For the next batch

- Nothing in the edge, runtime or SPA calls the new surface. WS8.F (attach) is the first consumer; it needs the tab-first vs meet decision (depth, high) and the select_tab / list_tabs pin semantics (depth, medium) decided first.
- The Model Plane ledger entry is still owed (sibling (b) sign-off), as for U03.
