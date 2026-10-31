# W2-U54 progress (WS6.J usage segment, batch 10)

Status: BUILT to scope, compile-checked, NOT run (build-first strategy). Flag: none new; `QUARRY_EDGE__DRAIN_DEADLINE_S` (default 30) already existed. The usage events themselves stay governed by `QUARRY_EDGE__USAGE_BROWSER_METRICS` (U37).

## Committed

- f84baf097 feat(quarry-edge): drain ends each run with its last usage segment and agent.expired, then closes the streams
  - agent_routes/close.rs: `CloseReason::Drain(wait)`, `drain_runs_for_shutdown(state, wait)` (through `finish_run`, 8 at a time, bounded wait/usage/release; non-ZDR timeline `expired` by system, checkpoint stays active; ZDR writes nothing; usage segment; one release; `agent.expired` reason `shutdown`).
  - agent_routes/run_live.rs (hot, one small method `idle_and_age_s`), live.rs (event and frame streams end on the token), ws.rs (one select arm), drain.rs (`close_streams`, `streams_are_closing`, `streams_closing`, doc-hidden `reset_for_tests`).
  - main.rs (hot, exact anchors, committed from the HEAD blob plus this unit's hunks only): early drain task on `drain::started()`, then `close_streams()`; after serve a second pass, driver shutdown, `usage.flush(5s)`.
  - tests/drain.rs: serial + FlagGuard; six new tests (names below), written not run.
- cb7bb5cad docs(quarry): RUNBOOKS section (append-only, committed from the HEAD blob plus the section; other units' RUNBOOKS hunks untouched).

## Verified (what was run)

- `cargo check -p quarry-edge --bins --lib --tests` through qtest (rust container): finished with no errors, three times (before and after formatting). Only pre-existing dead-code warnings.
- `rustfmt --check --edition 2021` on src/drain.rs, src/agent_routes/close.rs, tests/drain.rs: clean after fixes (the check also printed a diff for tests/support/agent_harness.rs, another unit's uncommitted file; not touched).
- Not run: `cargo test -p quarry-edge --test drain`, the log-field lint, `agent_routes/tests.rs` and `run_sweeper_integration` (they use the old `drain_runs`, unchanged).

## Tests written (tests/drain.rs, `mod runs`, cfg browser-agent)

`drain_refuses_new_runs_with_503_and_retry_after`, `drain_closes_streams_so_graceful_shutdown_completes` (a real graceful server with an events stream and a frames stream), `drain_releases_every_session_exactly_once`, `drain_keeps_non_zdr_checkpoints_active_and_writes_none_for_zdr`, `drain_emits_the_usage_segment_before_exit` (also asserts the `agent.expired` durable event and the flush), `a_busy_run_does_not_block_the_drain`. The two existing tests got `#[serial]` and a FlagGuard. Not written: `release_timeout_does_not_block_the_drain`, `usage_flush_waits_for_inflight_publishes` (see depth).

## What the next batch needs to know

- The test binary mixes the dev-bypass edge (first test) and the real-token harness edge; FlagGuard removes `QUARRY_EDGE_AUTH_DEV_BYPASS` and resets the drain flags. If a test fails in the test phase, suspect ordering of those process-global flags first.
- The drain order changed: runs end at once on SIGTERM (not after the in-flight deadline), then streams close. See depth for the product question.
- `drain_runs` (state-less) still exists for old tests; do not delete it until those are migrated.
- main.rs still carries other units' uncommitted hunks (the ZDR boot warning); this unit's hunks are committed.

Depth bullets: .plan-work/quarry-w1w2/depth/W2-U54.md (14 items).
