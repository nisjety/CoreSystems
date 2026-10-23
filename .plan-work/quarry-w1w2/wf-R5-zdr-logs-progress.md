# R5-zdr-logs progress

## 2026-10-18 (first agent)
- Task 1 (telemetry.rs EnvFilter at registry level): CONFIRMED same defect in quarry-edge (stage-1 run: 4 new RUST_LOG tests red on the legacy wiring).
  Fix in progress, UNCOMMITTED, mine: crates/quarry-edge/src/zdr_log.rs (ZdrLogFilter per-layer, exceeds_cap per event, mark() replace, production_subscriber, log_layer, export_filter, tests), src/telemetry.rs (env_filter() per layer), tests/zdr_log_capture.rs (uses production_subscriber).
- Task 2 scanner fix drafted in tests/log_field_discipline.rs (production_part cuts only at an inline cfg(test) mod): UNCOMMITTED, not yet run.
- page_renderer.rs:191: render_and_emit returns before it for ZDR (checked), annotation is true but stale; plan: convert to error_class = e.code.wire_name().
- Scratch: scratchpad/R5-zdr-logs/ (stage1.js, stage2.js, telemetry.js, lint1.js, logs).

## update (same agent, later)
- Lint run (lint-run1.log) with all my source edits: no new violation keys; every old runtime key reported "fixed". Edits applied UNCOMMITTED, mine:
  quarry-edge: zdr_log.rs, telemetry.rs, main.rs (zdr-log-ok-begin/end region, 2 lines), tests/{log_field_discipline,zdr_log_capture,zdr_feature_matrix}.rs
  quarry-runtime: page_renderer, fallback_driver, browser_driver(also has another unit's diff!), fetch, answer, page_extract, action_runtime (converted); publisher, cached_profile_store, events, usage, postgres_queue, browser_egress_proxy, nats_event_bus (annotated/converted)
  gateway: cache.rs, gdpr_nats.rs, domains/mail_task.rs, domains/mcp.rs, onboarding/recommendation/remote.rs, rate_limit.rs, onboarding/crawl_preview/handler.rs (converted + R4); domains/browser.rs R2 left (file held by another unit).
- Running: scratchpad/R5-zdr-logs/all1.log (cmd-all.txt: baseline regen, edge lib/bin zdr_log, edge tests, core zdr_matrix, runtime lib, check); gateway native lint in gw-lint.log.
- Next: read results, regenerate gateway baseline, commit in steps (telemetry; lint scanner+main region+baseline; matrix tests; runtime conversions; gateway).

## update 3
- First full run (all1.log): edge lib+bin zdr_log 27/27, log_field_discipline 20, zdr_claim_floor 22, zdr_feature_matrix 12, zdr_gap_rows 9, runtime lib 730, zdr_log_capture 11/12 (the stream control relied on the fallback line I converted -> moved to a driver probe in zdr_log_capture.rs, capture3.js). quarry-core test target `contracts` does not compile (BlockReason, another unit's WIP) -> use `--lib zdr_matrix`.
- Gateway: lint test passes, baseline regenerated (23 -> 1: only domains/browser.rs R2, file held by another unit); `cargo test` bin 726 pass 1 fail = browser.rs session_grants (another unit's WIP, unrelated).
- browser_driver.rs: my url hunk REVERTED (file carries another unit's HeldSession diff), baseline keeps browser_driver.rs::R1=1.
- Added: change_routes.rs `.in_current_span()` on the webhook spawn, run_sweeper.rs zdr-log-ok on the spawn (baseline R4 2 -> 0).
- Pending: all2.log (second run) then cmd-final.txt (regen + all suites), then commits C1..C5 (see plan in previous update).
## round 3: all committed (b275569d, 9a2bc002, e50046a9, 4c4dd4d2); see wf-R5-zdr-logs.md
