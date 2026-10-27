# Hand-over: D-B1-compile-units (W2-green, batch 1 of the depth plan, 2026-10-27)

Read first: `.plan-work/quarry-w1w2/wf-D-B1-compile-units-progress.md` (milestones and the test log) and `.plan-work/quarry-w1w2/depth/D-B1-compile-units.md` (what is still open, with owners).

## What is done (local commits on quarryv2-next, nothing pushed)

| Commit | Item | File |
|---|---|---|
| 10994b578 | D2-009: `tab_pin: None` in the benchmark literal; the workspace compiles | lab/evals/src/browser_benchmark.rs |
| 6c85d8a8e | capability wire test pins a shape from before WS8.E and WS13.3 | crates/quarry-browser/src/lib.rs |
| 946abd731 | metadata_save_load_roundtrip: a save writes a metadata row | crates/quarry-browser/src/session.rs |
| 90bd8a25d | tile-plan test: the 40 Mpx cap binds at 1,280 px; two more limit cases | crates/quarry-browser/src/stitch.rs |
| 0c0c904c7 | CDP scan: `target_id` is a getter, listed in NOT_CDP | crates/quarry-browser/src/chromiumoxide.rs |
| 52f2434f9 | PDF test fixture named the image on pages that do not paint it | crates/quarry-transform/src/pdf.rs |
| d83f69807 | "pdf" is a supported format: the bad-format tests use "docx" | crates/quarry-edge/src/routes.rs, scrape_contract.rs |
| b8e951f80 | D1-026: table-heavy size test beside the converter | crates/quarry-transform/src/markdown.rs |
| c78b4b70e | D1-065: draining-precedence test; every ready_browser test is #[serial] | crates/quarry-edge/tests/ready_browser.rs |
| (notes commit, see progress file) | progress, findings, hand-over | .plan-work/quarry-w1w2/ |

Final verification (scratchpad logs, all through qtest):
- `cargo check --workspace --all-targets`: exit 0, no error lines (logs/check1.log, before the test fixes; logs/test3.log, after them, QTEST_CHECK2 rc=0).
- `cargo test -p quarry-browser --lib --features chromiumoxide --no-fail-fast`: 289 passed, 0 failed, 36 ignored (logs/test3.log).
- `cargo test -p quarry-browser --lib --no-fail-fast`: 195 passed, 0 failed (logs/test3.log).
- `cargo test -p quarry-transform --no-fail-fast`: all targets ok (logs/test2.log; lib 200 passed).
- `cargo test -p quarry-edge --lib --bins --no-fail-fast`: lib 601 passed, 1 failed; bin 588 passed, 1 failed; the one failure is `agent_routes::tests::a_persons_request_step_cap_lifts_only_when_budget_enforcement_is_on`, another unit's (logs/test2.log).
- `cargo test -p quarry-edge --test ready_browser`: 7 passed, 0 failed (logs/test2.log).

Not run: the live Chromium lane (CHROMIUMOXIDE_TEST=1), the Redis and Postgres suites, the Go and node lanes (not in this unit).

## Traps found (read before the next unit)

1. Default features do not compile `chromiumoxide.rs`. `cargo test -p quarry-browser --lib` silently skips `cdp_calls_are_bounded` and the launch-dir sweeper test. Use `--features chromiumoxide` for them.
2. `cargo test` stops at the first failing test binary. The quarry-edge bin tests did not show up in the first run for that reason. Use `--no-fail-fast`.
3. qtest output is buffered until the docker lock is granted. A `qwait.sh` on a queued log times out with an empty tail; the holder is in `scratchpad/locks/docker.d/holder` (the lock lives under the scratchpad `locks/` folder, not the repo). Run the wait with the Bash tool `timeout` set to 600000, or it is moved to the background at 120 s.
4. The drain flags are process-global (`quarry_edge::drain`). Any test in `ready_browser.rs` that calls `/ready` must be `#[serial]`, and a test that calls `drain::begin()` must reset the flag with a guard.
5. "pdf" is a supported scrape format since WS13.x: it parses as `Pdf(default)`. Any test that uses "pdf" as the unknown-format example is wrong.
6. The PDF page classifier reads resources, not the content stream: `PageClassification.has_image` means the page's resources name an image XObject. A fixture must name the image only on pages that paint it.
7. `MAX_TILES`, the stitch tile limit, binds only on very narrow pages at a high device scale; the 40 Mpx pixel cap binds first at normal widths.
8. Line endings: `lib.rs` and `routes.rs` are CRLF in the working copy, most others are LF (both match what `lib-edit.js` preserved). The "LF will be replaced by CRLF" git warnings come from that mix, not from these edits. Git normalises at commit.
9. The shared tree had other units' uncommitted edits (agent_routes/*, budget.rs, robots.rs, crawl_scope.rs, fetch.rs, crawl_filter.rs). A failure in those files says nothing about this unit; check `git diff` on a file before committing it (every commit above was checked to hold only this unit's hunks).
10. The Co-Authored-By trailer is `Claude Opus 5.5` on every commit of this unit, as the batch instruction says (the memory notes record the owner's convention).

## What is left for the next person

Nothing in the batch's list is open. The open findings with owners are in `depth/D-B1-compile-units.md`, items 1 to 11. The most useful next steps:
- item 2 (quarry-runtime cached profile store, Redis-gated): the same wrong expectation as the session.rs fix, not run here;
- item 3 (default scope of a metadata-less profile, in-memory vs Postgres): a decision, not a test fix;
- item 1 (agent_routes step cap test, 403 vs 429): belongs to the budget and agent-policy unit, whose edits were moving during this run.
