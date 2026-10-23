# W2-U48 progress (WS6.I, second half: agent-run, step, budget, cache, pool-refusal and drain series)

Status: DONE (built to scope, compile-checked, tests written and not run).

## What this unit built (all in the working tree)
- `quarry-edge/src/metrics.rs`: `route_label` (matched route template or `unmatched`, replaces `route_class`), `record_run_start/denied/expired`, `action_class` + `record_agent_step`, `record_cache`, `RunStats` + `register_run_stats` + `sample_runs`, `sample_all` (pools, runs, drain flag), descriptions for every new series, unit tests `route_label_is_the_matched_template_not_the_raw_path`, `action_classes_are_a_closed_set_of_eleven`.
- Counter sites (small exact-anchor edits): `agent_routes/lifecycle.rs` (draining, allowance, cap denial, commit), `run_registry.rs` (`RunSlot::drop` = aborted), `run_sweeper.rs` (expired), `close.rs` (`drain_runs` = shutdown), `step.rs` (both execute sites), `cache.rs` (page and search hit/miss/error), `drain.rs` (`quarry_edge_draining` at `begin`).
- `quarry-runtime/src/lease_pool.rs`: refusal, wait-timeout and wait-time counters in `LeaseMetricsSnapshot`, plus test `refusals_and_waits_are_counted`.
- `quarry-edge/src/main.rs` (hot, H3): one block that registers `RunRegistry::stats` with the sampler. `apps/Ingestion Plane/docker-compose.yml` (hot, H25): `expose: ["9092"]`. The metrics env lines were already there from the first half.
- Tests written (compile-checked only, not run): `crates/quarry-edge/tests/metrics_recorder.rs` (route test renamed/rewritten, new `agent_run_and_cache_series_use_closed_label_sets`, `sampler_exports_run_gauges_the_drain_flag_and_pool_refusals`).
- Docs: RUNBOOKS "Metrics" bullets corrected and section "WS6.I series" appended; PLAN.md line 359, GAP.md line 257 corrected; WEB_AGENT_ROADMAP_EXECUTION.md note appended.

## Not built (boundary)
- `quarry_usage_events_total` (U37), Control Plane Prometheus job and token secret (Control Plane), alert rules, lease-pool wait histogram (needs a recorder in quarry-runtime). See `depth/W2-U48.md`.

## Traps for the next batch
- `route` label is now the matched template: any dashboard or test that expects `route="scrape"` style classes must change.
- The metrics recorder is process-global: tests that read the exposition live in `tests/metrics_recorder.rs` only.
- `RUNBOOKS.md` carries a large pre-existing uncommitted diff of other units; commit U48's hunks with `qcommit-patch.sh`.
- During this unit the shared tree failed to compile twice for reasons outside it: `quarry-runtime` lib test (`redaction_rule_version` missing in `browser_procedure.rs` StepReceipt initializer, another unit's step_receipts.rs hunk) and `quarry-browser/src/stitch.rs` lifetime errors (another unit, uncommitted new file).

## State
- Commits: 5ad959ed (fix: `mod scrape_actions;` missing in the quarry-edge-rs bin crate, found by the compile check; HEAD bin build was broken since WS13.4b) and f57d3d81 (the WS6.I second half, 16 files).
- Compile check: `cargo check --tests -p quarry-edge --keep-going` and `cargo check --tests -p quarry-runtime`, both Finished, no errors. Two earlier attempts failed on other units' mid-edits (runtime lib test: redaction_rule_version; quarry-browser stitch.rs lifetimes) and the bin on the missing module declaration. No test was run.
- RUNBOOKS.md incident: the working-tree copy had lost about 280 lines of sections that other units had already committed (probably a stale-copy overwrite by some agent, not located). I repaired the working file to HEAD plus U48 plus the uncommitted U32 section (and U32's edited "Limits to know" line), committed only U48's hunks by patch. Backups: scratchpad W2-U48/RUNBOOKS.work-backup.md. Other agents appending to RUNBOOKS.md must re-read it right before writing; a read-modify-write that starts from a stale copy deletes committed text.
- Not committed: nothing of U48 is left uncommitted except these progress and depth notes.
