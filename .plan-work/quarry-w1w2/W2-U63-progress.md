# W2-U63 progress: WS14.S4 robots gate, observe build

Unit W2-U63, gated group G2-a, wave 2 (build first, depth later). Branch `quarryv2-next`, worktree `vigorous-perlman-e7ae5b`. Built to the plan's scope for the observe build; tests written and compile-checked, NOT run. Enforce is not flipped (X-10 is not among the approved 13).

## Committed (local, not pushed)

| Commit | Subject |
|---|---|
| 114ae9e50 | feat(quarry-runtime): robots.txt gate, origin cache rework and Crawl-delay spacing (quarry-transform robots.rs; runtime robots_cache.rs rework, robots_gate.rs NEW, host_scheduler.rs, pipeline.rs, driver.rs + fetch.rs, lib.rs, crawl_filter.rs, Cargo.toml dev-dep; tests/robots_gate_ssrf.rs NEW) |
| 081c442a8 | feat(quarry-edge): robots gate on the crawl and batch page route and on /v1/map (state.rs, main.rs hunks, routes.rs hunks, map_routes.rs, metrics.rs, test_support.rs, tests/support/mod.rs, tests/crawl_scope_robots.rs) |
| c4b08e4b7 | docs(quarry-v2): robots gate runbook section and the 403 details.denial schema (RUNBOOKS append, openapi hunks) |
| aed6360dd (U64's commit) | also carries my orchestrator hunk: `page_fetched.crawl_delay_ms` in `workflows.go` (U64 committed the file with my hunk in it) |

`routes.rs`, `main.rs`, `openapi.yaml` and `RUNBOOKS.md` carried other units' uncommitted hunks: my hunks went in as patches built against HEAD (scratch `W2-U63/` mirror and `mkpatch.sh`), the others' hunks stay uncommitted in the tree.

## What is built

- `quarry-transform::robots`: `is_allowed_any`, `denying_token`, `crawl_delay_any`, `path_and_query`; `?` is a literal (it was a wildcard); a named empty group allows everything.
- `robots_cache.rs` reworked (RobotsFetch, RobotsState, origin key, TTLs 24 h / 1 h / 5 min on tokio time, cap 10,000 oldest first, in_flight pruned, peek, get_uncached); `ReqwestRobotsFetcher` deleted (`git grep ReqwestRobotsFetcher` is empty).
- `robots_gate.rs`: `RobotsGate` over the guarded Driver chain, `RobotsDecision`, `RobotsGateConfig::from_env` (five variables, boot fails on garbage), `RobotsEnforcement`, content-free counters.
- `FetchHints.max_body_bytes` and `truncate_at_cap` plus the chunked capped read in `StaticDriver` (landed here because WS5 slice 1 has not; names and semantics as the plan fixes).
- `HostScheduler::set_min_interval` and start-time spacing in `acquire` (zero interval is the old path).
- `PageRunner.robots`: check after preflight and before DNS and the slot, `PageBlocked` with no `PageFailed` and no `record_bad`, final-URL check on an origin change, `Crawl-delay` into the scheduler in enforce only.
- Edge: `AppState.robots`, boot parse and effective-mode log, gate on `internal_run_page` when `crawl.robots == respect`, `crawl.crawl_delay_ms` and link filtering by cached rules (enforce only), `/v1/map` through the gate in enforce, metrics `quarry_robots_decisions_total`, `quarry_robots_cache_entries`.
- Orchestrator: `page_fetched.crawl_delay_ms` when the edge reports one.
- Docs: RUNBOOKS section, openapi 403 and `RobotsFetchDenial`.

## Tests (written, compile-checked, NOT run)

- `quarry-transform` robots.rs unit tests (query-aware, token set, delay, merge, `?` literal).
- `robots_cache.rs` (found, absent, unreachable, TTLs under `start_paused`, 50 concurrent callers share one fetch, in_flight pruned, eviction at the cap, origin keys by scheme and port, get_uncached).
- `robots_gate.rs` with a stub Driver (fetch_conditional not fetch, org and deny-third-party, no query, 512 KiB hints, 4xx absent, 5xx two attempts, refused target unreachable, timeout, ZDR, peek, counters, denial json, config parse).
- `host_scheduler.rs` (three starts at 0, 2, 4 s; other hosts unaffected; zero interval; AIMD state kept).
- `pipeline.rs` module `robots_gate_tests` (never reaches the driver, one PageBlocked, no PageFailed, no concurrency shrink, observe fetches and counts, off, crawl-delay in enforce only, redirect to a disallowed origin persists nothing, unreachable, ZDR).
- `tests/robots_gate_ssrf.rs` (loopback, metadata redirect, private literals through the real `StaticDriver`).
- `tests/crawl_scope_robots.rs` extension (typed 403, crawl_delay_ms, observe, ignore, 503 twice, 503 allow, metadata redirect, map drops /private/, map ZDR leaves no entry, map 403 unreachable).

## Checks run

1. `cargo check --tests -p quarry-transform -p quarry-runtime` through qtest: rc 0.
2. `cargo check --tests -p quarry-edge` through qtest: rc 0 (after fixing my test file; the first full run also showed another unit's `batch_check` errors, gone on the second run).
3. A warnings grep over the same check for robots, host_scheduler, map_routes, pipeline.rs, fetch.rs, metrics.rs, crawl_filter, driver.rs: no lines.
4. `go vet ./internal/workflows/` in the orchestrator: clean.
5. Nothing was run as a test.

## Not done (in scope, with reason)

- Test phase: every test above runs at the end of the wave.
- Enforce flip: X-10. Deployment evidence (rebuilt images, `scripts/e2e/crawl-scope-robots.sh --expect enforce`): stack lane.
- Knowledge composer summary row, `CRAWL_SCOPE_ROBOTS.md` and `CONTRACTS.md` text (shared files with other units' edits).
- Go test for `page_fetched.crawl_delay_ms`.

## For the next batch

- The decisions and risks are in `depth/W2-U63.md`. The two to read first: the Crawl-delay wait holds a global autoscale permit (fix before enforce), and 429 on robots.txt is read as unreachable (owner to confirm).
- `FetchHints` now has two fields; every existing literal uses `..FetchHints::default()`, so nothing else changed. WS5 slice 1 must keep those names.
- `PageRunner` has a new `robots` field: any new `PageRunner { .. }` literal needs `robots: None`.
- Run order for the test phase: transform, runtime unit tests, `--test robots_gate_ssrf`, then `--test crawl_scope_robots` in quarry-edge.
