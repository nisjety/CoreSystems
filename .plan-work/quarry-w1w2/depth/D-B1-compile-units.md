# D-B1-compile-units: findings this unit did not fix (2026-10-27)

Unit D-B1-compile-units (track W2-green). Its commits: 10994b578 (D2-009), 6c85d8a8e, 946abd731, 90bd8a25d, 0c0c904c7, 52f2434f9, d83f69807, b8e951f80, c78b4b70e. Progress: `.plan-work/quarry-w1w2/wf-D-B1-compile-units-progress.md`. Hand-over: `.plan-work/quarry-w1w2/wf-D-B1-compile-units.md`.

## Items of the batch: closed

| ID | Status | Evidence (qtest, 2026-10-27) |
|---|---|---|
| D2-009 | closed by 10994b578 | `cargo check --workspace --all-targets` exit 0, no error lines |
| D1-026 | closed: the padding fix is 217fc5484 (WS5.E); the size test for several wide columns is b8e951f80 | quarry-transform: `markdown::table_size_tests::...` ok; `tests/markdown_blowup.rs` 10 passed |
| D1-065 | closed: the sweeper test and the Degraded test already existed; the draining-precedence test is c78b4b70e | `ready_browser` 7 passed; `chromiumoxide::bounded_calls_and_launch_dirs::the_sweep_removes_only_the_directories_of_processes_that_are_gone` ok (chromiumoxide feature) |
| quarry-browser lib (4 named) | closed: 6c85d8a8e, 946abd731, 90bd8a25d, 0c0c904c7 | `--features chromiumoxide`: 289 passed, 0 failed, 36 ignored (live lane); default: 195 passed, 0 failed |
| quarry-transform pdf (2 named) | closed: 52f2434f9 | quarry-transform lib 200 passed, 0 failed |
| quarry-edge routes and scrape_contract (lib and bin) | closed: d83f69807 | quarry-edge lib: 601 passed, 1 failed (not this unit's, see item 1); bin: 588 passed, 1 failed (the same test); both named tests ok in both |

Every test of the batch was fixed in the test, not the code: each expectation contradicted the plan or the trait contract, or pinned a shape that a later slice had changed (details in each commit body).

## Open findings (not fixed here)

1. **Test, other unit: `agent_routes::tests::a_persons_request_step_cap_lifts_only_when_budget_enforcement_is_on`** fails in quarry-edge lib and bin with 403 `FORBIDDEN` ("sensitive browser action requires a parent BrowserBroker run grant") where the test expects 429 (`agent_routes/tests.rs` line 535 at the time of the run). In the first run of the same module the failing test was `a_budget_refusal_comes_first_and_burns_no_single_use_approval` (line 488); in the second run that one passed and this one failed. The failing set moves with the concurrent edits to `agent_routes/*.rs` and `budget.rs` by the budget and agent-policy unit. Not investigated here. Owner: the agent-policy/budget unit.

2. **Test, quarry-runtime (not run here): `cached_profile_store.rs` around line 615, `metadata_methods_pass_through_to_inner_uncached`.** It is Redis-gated (skips without `REDIS_URL`), and it asserts `load_metadata` is `None` right after a save: the same wrong expectation fixed in session.rs (946abd731). Fix it the same way: after a save the profile has a metadata row with no name. Owner: the quarry-runtime cache owner. Not changed: the file is not in this unit's list.

3. **Design, two stores disagree on the default scope.** A snapshot saved without metadata reads back as `ProfileMetadata` with scope `Ephemeral` from the in-memory store (`session.rs`, `InMemoryProfileStore::load_metadata`). The comment on `ProfileScope` says the Postgres migration backfills and defaults such rows to `UserPrivate`. Which default is right is a product and data decision (WS17); not changed. Owner: the profile-store owner.

4. **Test quality, other unit: `agent_routes/tests.rs` lock assertions.** At the first `cargo check` of this unit (`scratchpad/D-B1-compile-units/logs/check1.log`) rustc warned `unused tokio::sync::MutexGuard that must be used` four times around lines 2320-2339: `tokio::time::timeout(..., x.lock()).await.expect(...)` as a statement. The guard drops at the end of that statement, so the assertions "the lane is free while ... runs" do not hold a lock and cannot catch what they name. The file has moved since; re-check the current lines. Owner: the agent-policy unit.

5. **Sweeper coverage, `chromiumoxide.rs` (`sweep_stale_launch_dirs_in`, about line 5618).** It acts only on Linux (`cfg!(target_os = "linux")`) and decides liveness with `/proc/<pid>` in this PID namespace, so a launch directory of a process in another PID namespace is removed only when that pid is absent here. On Windows and macOS nothing is swept. The function documents this; it is a design limit, not changed. Still open from D1-065: the launch directories removed by `ChromiumLaunchDirs::remove` (about lines 2198 and 942, on retire and on a failed launch) have no test of repeated launches not accumulating; only the sweep is tested.

6. **Dead code, `quarry-browser/src/deadlines.rs` line 283.** `best_effort` is never used (rustc warning). Owner: the WS3.D author. Not changed.

7. **Design note, `stitch.rs`.** `MAX_TILES` (16) binds only on a narrow page at a device scale of about 3.9 or more. At 1,280 px and scale 1 the 40 Mpx PNG cap binds first (31,250 CSS px). Both are as designed; the test now pins the binding limit per case. No change.

8. **Design note, markdown nested lists.** Indentation still grows with the square of the list depth (WS5.E left it; `tests/markdown_blowup.rs` pins the shape). No change.

9. **Warnings only, other test targets.** Unused imports and variables in quarry-edge tests (`profile_guard.rs:17`, `browser_grants_prod.rs:32-35` and `EXAMPLE_ORG`), `tab_ledger.rs:21` (`LaneState`), `fleet_routes.rs:26` (`FleetStatus`), `agent_routes/attach.rs:611` (a `mut` that is not needed). None is a failure. Not changed.

10. **Not run: the live Chromium lane.** `quarry-browser --features chromiumoxide` ignores 36 tests without `CHROMIUMOXIDE_TEST=1` (WS8.E tab tests, WS12.S1 field probe, and others). Running them needs a Chromium session; not in this unit's test list.

11. **Robots and crawl-request tests (not this unit's, green in later runs of this unit).** `quarry-transform` `robots::tests::a_named_group_with_no_rules_allows_everything` and `robots::tests::query_aware_rules_match_path_and_query` failed in the first run of this unit (before the batch edits) and passed in the later runs, after `67e3b3d3e` (robots fix, the D-B1-crawl-robots unit). `crawl_request::tests::pattern_total_boundary_is_8192_characters` failed in the first run and passed later, after `2f67e6262`. This unit did not verify those commits in isolation.

## Shared tree notes

- At the start the tree had other agents' uncommitted edits. None of the seven files this unit touched had foreign hunks (checked with `git diff` before each commit).
- The stray mod-order and line-ending changes (`quarry-edge` `lib.rs`, `main.rs`, `tests/support/*`, `quarry-runtime` `step_receipts.rs`) were left alone.
