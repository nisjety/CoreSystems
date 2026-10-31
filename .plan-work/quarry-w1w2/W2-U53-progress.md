# W2-U53 progress (WS13.5, scrape-time actions, dark by default)

Status: PARTIAL. The runtime core, the do_fetch action block, the edge gate, the path deadline and the RUNBOOKS section are committed and compile-checked. The wire response, the pipeline persistence, the gateway slice, the edge integration tests and the live Chromium test are not built (see not_done). Nothing has been run.

## Committed (branch quarryv2-next, local, not pushed)

| Commit | Scope |
|---|---|
| ab49f5ae3 | quarry-runtime: `action_runtime.rs` (attached runtime, `run_scrape`, `ActionBudget`, `ScrapeActionRun`, unit tests `scrape_run_tests`), `driver.rs` (`RenderHints.actions`, `actions_deadline`, `needs_browser`), `browser_driver.rs` (capability refusal, heavy permit, actions block after `wait_for`, `action_failed`, real final URL, outcomes on `PageCaptures`) |
| 82144afb4 | quarry-edge: `scrape_actions.rs` (`ENV_SCRAPE_ACTIONS` mode, `check_principal`, `apply_actions`, `run_deadline_ms`, `run_timeout_details`, `path_deadline`, unit tests `actions_tests`; `plan_for` sends actions to the browser), `auth.rs` (`SCOPE_SCRAPE_INTERACT` documentation constant only), `scrape_contract.rs` (`actions_enabled` from the switch), `routes.rs` (path-aware deadline layer replaces the 60 s `TimeoutLayer`, `ScrapeRequest.actions` and `initiated_by`, gate, actions-safe runner fields, render hint, run deadline, stream refusal) |
| 2e541cdfd | docs: `docs/RUNBOOKS.md` section "Scrape-time actions" (append-only) |

Commit 82144afb4 used a hunk-only patch (`routes.rs` and `auth.rs` carry other units' uncommitted hunks, which were left in the working tree). Its title says "safe-mode runner"; the separate `safe_mode_runner` function is not built (the body describes what was done: the three runner fields are unset for an actions request). Not amended, per the no-amend rule.

## Not committed by this unit

- Other units' uncommitted hunks still in `crates/quarry-edge/src/routes.rs` and `docs/RUNBOOKS.md` (and the rest of the tree). Not ours.
- `.plan-work/quarry-w1w2/W2-U53-progress.md`, `wf-W2-U53.md` and `depth/W2-U53.md` are committed in the next commit of this unit (plan-work notes).

## Checks run (compile only, on the working tree)

- `cargo check -p quarry-runtime --tests` (through qtest): rc 0, no warnings from the new code.
- `cargo check -p quarry-edge --tests` (through qtest): rc 0. One warning from our code path: `browser_unavailable` is never used (`scrape_actions.rs`, WS13.4b, pre-existing). Others are pre-existing in other files.
- Not run: any `cargo test`, the live Chromium test, the gateway build.

## Not done (in scope of the slice, owed)

1. Wire response: `data.actions[]` with the ordered outcomes, `output.artifact_id` and inline `data_base64` under ZDR. `ScrapeResponse` still echoes only counts.
2. `pipeline.rs` `persist_captures` (store action outputs under `action.<i>.<kind>`, ZDR inline cap) and the `zdr::guard` before any store put.
3. Edge `safe_mode_runner` as a function, and the per-field no-op check of the five optional `PageRunner` fields (`actions_force_safe_mode_so_no_ingest_index_or_title_hop` not written).
4. Integration tests: `tests/scrape_capture.rs` cases and `router_deadline_is_125s_...`; the live Chromium test `scrape_actions_live`.
5. Scroll by direction on the real driver (needs a viewport query on the trait; H4 file).
6. Child cancel token fired from the deadline.
7. Gateway: 125 s per-request timeout, `initiated_by:"agent"` stamping on the action-dispatcher path (`knowledge/quarry.rs`, `config.rs`). Not in U53's owned list.
8. Batch route check for `actions`.
9. Contract key row for `initiated_by` (so it is not reported as an unknown field).

## Next batch needs to know

- The flag is `QUARRY_EDGE__SCRAPE_ACTIONS` (`off` default). `user` and `on` both admit user tokens; `on` must not be set before the ADR-6 ruling.
- `actions_mode()` and `capture_mode()` are `OnceLock` reads: edge integration tests must set the variable before the first read, or run in a child process.
- U53 is a prerequisite for WS13.7b and WS13.8 (docs and OpenAPI for actions); WS13.8 owns the ERROR_REASONS entries listed in the depth file.
- Batch 11 gates (plan queue): U57 after U45, U50 and U53. U53 is built; its gateway half is not.

Depth bullets: `.plan-work/quarry-w1w2/depth/W2-U53.md`. Hand-over notes: `.plan-work/quarry-w1w2/wf-W2-U53.md`.
