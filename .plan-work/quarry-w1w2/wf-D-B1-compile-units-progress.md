# D-B1-compile-units: progress (unit id D-B1-compile-units, track W2-green)

Started 2026-10-27 on branch quarryv2-next (local only, no push, no PR). Scratch: scratchpad/D-B1-compile-units/.
No earlier progress file existed for this unit (fresh start, not a continuation).

## Scope (from DEPTH-PLAN-2026-10-27.md, section D-B1-compile-units)

- D2-009: add `tab_pin: None` at lab/evals/src/browser_benchmark.rs (BrowserSession literal); cargo check --workspace --all-targets must have 0 errors.
- quarry-browser lib: capability_wire_tests::capabilities_flag_defaults_false_and_is_additive_on_the_wire, chromiumoxide::bounded_calls_and_launch_dirs::cdp_calls_are_bounded, session::tests::metadata_save_load_roundtrip, stitch::tests::tile_plan_respects_max_tiles_and_marks_truncated.
- quarry-transform pdf: pdf::tests::a_blank_page_is_empty_and_a_short_text_page_is_text, pdf::tests::a_text_page_and_a_scanned_page_are_told_apart_in_page_order.
- quarry-edge lib and bin: routes::tests::a_malformed_body_and_a_bad_option_return_the_envelope_with_field_paths, scrape_contract::tests::bad_option_values_come_back_as_field_errors.
- D1-026: cap markdown table-cell padding; size regression test on a table-heavy fixture.
- D1-065: sweep_stale_launch_dirs temp-dir test; ready_browser Degraded and draining-precedence tests.

## Owned files

lab/evals/src/browser_benchmark.rs; crates/quarry-browser/src/{lib.rs,chromiumoxide.rs,session.rs,stitch.rs}; crates/quarry-transform/src/{pdf.rs,markdown.rs}; crates/quarry-edge/src/{routes.rs,scrape_contract.rs}; crates/quarry-edge/tests/ready_browser.rs.
Not mine (leave alone): quarry-edge src/{lib.rs,main.rs}, tests/support/{agent_harness.rs,fixture_site.rs}, quarry-runtime src/step_receipts.rs, .plan-work/next-slices.md, rb.patch, apps/Model Plane/go/go.work.sum.

## Milestones

- [x] M0: baseline qtest runs (cargo check --workspace --all-targets; the named tests), recorded below.
- [x] M1: D2-009 committed as 10994b578.
- [x] M2: quarry-browser lib failures fixed: 6c85d8a8e (capability wire), 946abd731 (metadata), 90bd8a25d (tile plan), 0c0c904c7 (CDP scan NOT_CDP for target_id).
- [x] M3: quarry-transform pdf failures fixed (fixture): 52f2434f9.
- [x] M4: quarry-edge routes and scrape_contract fixed ("pdf" is a supported format now): d83f69807.
- [x] M5: D1-026: the padding fix is already 217fc5484 (WS5.E); table-heavy size test in markdown.rs: b8e951f80.
- [x] M6: D1-065: sweeper and Degraded tests already existed and pass; draining-precedence test and #[serial] on ready_browser.rs: c78b4b70e.
- [x] M7: final cargo check --workspace --all-targets exit 0 (logs/test3.log, QTEST_CHECK2 rc=0); named tests green (see the final test log below); notes written: depth/D-B1-compile-units.md, wf-D-B1-compile-units.md.

## Test results log

- 2026-10-27 cargo check --workspace --all-targets (after the D2-009 fix): exit 0, no error lines. Log: scratchpad/D-B1-compile-units/logs/check1.log.
- Baseline (before any fix 2), logs/test1.log:
  - quarry-browser --lib (default features): 192 passed, 3 failed: capability_wire_tests::..., session::tests::metadata_save_load_roundtrip, stitch::tests::tile_plan_respects_max_tiles_and_marks_truncated. NOTE: default features do not compile chromiumoxide.rs, so cdp_calls_are_bounded and the launch-dir sweeper test need --features chromiumoxide.
  - quarry-transform --lib: 199 run, 4 failed: pdf::tests x2 (mine), robots::tests::a_named_group_with_no_rules_allows_everything and robots::tests::query_aware_rules_match_path_and_query (D-B1-crawl-robots, not mine).
  - quarry-edge --lib: 596 passed, 4 failed: routes::tests::a_malformed_body... and scrape_contract::tests::bad_option_values... (mine), agent_routes::tests::a_budget_refusal_comes_first_and_burns_no_single_use_approval (other unit), crawl_request::tests::pattern_total_boundary_is_8192_characters (D-B1-crawl-robots). The bin target did not run (cargo stops at the first failing binary; use --no-fail-fast).
  - quarry-edge --test ready_browser: 6 passed.

## Diagnoses (test or code)

- capability_wire_tests: TEST wrong. The `older` JSON predates WS8.E (background_tabs, tab_pinning, per_tab_egress) and WS13.3 (scrape_actions, capture_options), which are additive keys serialised as false. Fix: the older shape lists them as false; a separate oldest shape checks every additive key reads false.
- metadata_save_load_roundtrip: TEST wrong. A save writes a row (WS17.P1 order book, and Postgres keeps the snapshot and metadata in one row), so load_metadata answers Some(default). The trait doc says None means no row AND no snapshot. Fix: assert the saved profile has a metadata row with no name.
- tile_plan_respects_max_tiles_and_marks_truncated: TEST wrong. At width 1280 the 40 Mpx PNG cap (plan doc: at most 40 Mpx PNG) binds at 31,250 CSS px, not the 60,000 px content limit. Fix: assert 31,250; add a 600 px content-limit case (15 tiles) and a 30 px at scale 4.5 tile-limit case (16 tiles, 56,880 px).
- pdf page tests: TEST fixture wrong. build_pdf named the image XObject in every page's Resources, and the classifier reads resources (has_image = resources name an image), so text and blank pages looked like scans. Fix: a page's own Resources drop XObject unless the page paints the image.
- routes::tests and scrape_contract::tests: TEST wrong. They use "pdf" as the unsupported example; pdf is in ScrapeFormat::SUPPORTED and parses as Pdf(default) since WS13.x. Fix: "docx" (unsupported_format at formats[1]); scrape_contract also pins a bad capture option value (object form, invalid_type at formats[0]).
- D1-026: CODE already fixed by 217fc5484 (WS5.E, CompactTableHandler: cells unpadded). tests/markdown_blowup.rs (10 tests) pins the blow-up cases. The unit adds one table-heavy regression test (wide cells in three columns on different rows, 1,500 rows) inside src/markdown.rs, where the padding was.
- D1-065: the sweeper test and the Degraded test already exist; the draining-precedence test is new. Every test in ready_browser.rs is #[serial] because the drain flags are process-global (drain.rs: only a test binary of its own may set them).

## Edits made (uncommitted until noted)

- M1 10994b578: lab/evals/src/browser_benchmark.rs (tab_pin: None).
- batch 1 (scripted, scratchpad/D-B1-compile-units/edit-batch1.js): quarry-browser lib.rs, session.rs, stitch.rs; quarry-transform pdf.rs, markdown.rs; quarry-edge routes.rs, scrape_contract.rs.
- batch 2 (scratchpad/D-B1-compile-units/edit-ready.js): quarry-edge tests/ready_browser.rs (#[serial] on all 6 tests, DrainGuard, draining-precedence test).

- Final verification (after all eight test commits), logs/test3.log:
  - `cargo test -p quarry-browser --lib --features chromiumoxide --no-fail-fast`: 289 passed, 0 failed, 36 ignored (QTEST_B3 rc=0). Named tests ok: capability_wire_tests::..., chromiumoxide::bounded_calls_and_launch_dirs::cdp_calls_are_bounded, the_scan_reports_..., the_sweep_removes_only_..., session::tests::metadata_save_load_roundtrip, stitch::tests::tile_plan_respects_max_tiles_and_marks_truncated.
  - `cargo test -p quarry-browser --lib --no-fail-fast`: 195 passed, 0 failed (QTEST_B4 rc=0).
  - `cargo check --workspace --all-targets`: exit 0 (QTEST_CHECK2 rc=0), 0 error lines.
- From logs/test2.log (run after the batch-1 and ready_browser edits, before the NOT_CDP edit, which only touches quarry-browser):
  - quarry-transform `--no-fail-fast`: lib 200 passed, 0 failed; markdown_blowup 10 passed; every other target ok (QTEST_T2 rc=0). Named: pdf::tests::a_blank_page_is_empty_and_a_short_text_page_is_text ok, pdf::tests::a_text_page_and_a_scanned_page_are_told_apart_in_page_order ok, markdown::table_size_tests::... ok.
  - quarry-edge `--lib --bins --no-fail-fast`: lib 601 passed, 1 failed; bin 588 passed, 1 failed. The failure is agent_routes::tests::a_persons_request_step_cap_lifts_only_when_budget_enforcement_is_on (another unit's, depth item 1). Named routes::tests::a_malformed_body... and scrape_contract::tests::bad_option_values... ok in both.
  - quarry-edge `--test ready_browser`: 7 passed, 0 failed (QTEST_R2 rc=0).

## Environment notes

- The docker lock is shared; at 15:00 another agent held it (crawl_scope_robots segments). My qtest run waits in the queue; the Bash wait call is the right way to block.
- The shared tree has other agents' uncommitted edits (crawl_scope.rs, agent_routes/*, budget.rs, fetch.rs, crawl_filter.rs, robots.rs, ...). Only my 7 owned files are checked for foreign hunks: none found before the batch 1 edit (diffs reviewed: only my hunks).
