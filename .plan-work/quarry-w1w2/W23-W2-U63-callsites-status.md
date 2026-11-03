# W2-U63-callsites status: robots call sites (W23 lane c, step S1)

Unit W2-U63-callsites, wave W2, step S1 of the W23 finish program. Branch `w23/lane-c` (worktree `innh-lane-c`), synced to `w23/integ` at `4c6b78669` when the unit started. Local commits only: nothing pushed, no PR, no merge, no rebase.

Code only, not compiled. Owner order 2026-11-03: no cargo, go, pnpm or Docker runs in unit work, so the integration step compile-checks. `rustfmt --check` was run on the three files to keep the added lines in rustfmt form; that is a formatter check, not a build.

## Built

Already in place from W2-B4 (`49372b856`, `6aae683af`), checked and not rebuilt: `HostScheduler::reserve_start`, `StartTicket::wait`, `HostScheduler::acquire_reserved`, `metrics::robots_link_refusals`, `metrics::record_robots_link_refusals`, and the robots.txt parser fixes. The gap was the three call sites, built here:

- `quarry-runtime/src/pipeline.rs`, `run_inner`: the start ticket is reserved and waited out before the global autoscale permit, then `acquire_reserved` after it. A Crawl-delay wait no longer holds a global permit (D2-161). This replaces `scheduler.acquire` inside the global permit.
- `quarry-edge/src/routes.rs`, `with_crawl`: `metrics::record_robots_link_refusals(&outcome.report)` when `gate.mode() == Enforce`.
- `quarry-edge/src/crawl_sitemap.rs`, `select`: `metrics::record_robots_link_refusals(&outcome.report)` per chunk when a robots lookup is given. The handler builds that lookup only when the gate enforces (`robots_enforced`), so this is the enforce-only case.

## Tests

- `pipeline.rs`, `robots_gate_tests::a_crawl_delay_wait_holds_no_global_permit` (paused time, one global permit, 30 s Crawl-delay): at 10 s the delayed request holds no global permit and has not been fetched; another host is fetched meanwhile; the delayed fetch happens at or after 30 s and holds the permit; every permit is back at the end.
- `routes.rs`, `tests::robots_link_refusals_are_counted_only_when_the_gate_enforces`: observe removes no link and records nothing; enforce removes the disallowed link and records `quarry_robots_link_refusals_total{outcome="denied"} 1`. Uses a local Prometheus recorder.
- `crawl_sitemap.rs`, `tests::robots_refusals_are_counted_only_when_a_lookup_is_given`: no lookup records nothing; a lookup records one refusal.

## Commits

- `851e11c5c` fix(quarry-runtime): a Crawl-delay wait holds no global autoscale permit (W2-U63, D2-161): `pipeline.rs`.
- `0bd9d1c75` feat(quarry-edge): count enforce-mode robots link refusals on crawl pages and sitemaps (W2-U63, D2-091): `routes.rs`, `crawl_sitemap.rs`.
- This status file and the new depth rows: the docs commit that follows (see `git log`).

The two depth rows about the call sites (the first two W2-U63 rows in `W23-depth-lane-c.md`) are resolved by the two code commits.

## Flag and default

No flag. The permit ordering is a bug fix (D2-161) and the two counter calls wire an existing series. Robots observe and enforce defaults are unchanged, and nothing flips to enforce (X-10 is not approved). The series `quarry_robots_link_refusals_total` appears only when an enforcing run removes a link.

## Assumed decisions

- Permit ordering: the start is reserved and waited out before the global permit, and the per-host slot is taken after it. Start-to-start spacing is unchanged, because the start time is still reserved under the scheduler lock in the order `acquire` used. Recommended default taken without asking.
- `crawl_sitemap.rs` counts by the lookup's presence, not by a separate mode check, because the report is built inside `select`. The handler is unchanged. A future caller that passes a lookup without enforcing would miscount; the comment at the call site says so.
- Sitemap refusals and page-link refusals share `quarry_robots_link_refusals_total`, which has no source label. Recorded as a depth row for the soak.
- The tests use `metrics::with_local_recorder` with a `PrometheusRecorder` built for the closure, so no global recorder is needed and `tests/metrics_recorder.rs` is not changed (it is outside this unit's file list).

## MP asks

None. No Model Plane file was touched, and `MP-ASKS.md` is unchanged.

## Not verified

- Nothing was compiled or run. The new tests have never executed.
- The paused-time pipeline test is the first test in `pipeline.rs` that runs a full page under `start_paused`. A timer or a blocking parse added to the HTML page path would break its timing rather than the permit order. The current HTML path in the harness has neither (the local index and ingest spawns are off in the harness).
- The `tests/metrics_recorder.rs` case that the earlier depth row asked for is not added (outside this unit's files). A global-recorder check belongs to the integration step.
- Enforce end to end (`tests/crawl_scope_robots.rs`), the sitemap integration test and the scheduler benchmark were not run (depth D2-249).
- The sitemap-from-robots scanner's handling of CR-only files (the W2-U63 row about `lines()` in `W23-depth-lane-c.md`) was not checked; it is outside this unit's files.
