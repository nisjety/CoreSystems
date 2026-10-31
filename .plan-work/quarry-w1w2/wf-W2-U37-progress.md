# W2-U37 live progress (WS6.C-e, usage emission half)

Status: code and tests written; compile check on the clean export (HEAD plus this unit's files) in progress; commit not yet made.

## Files of this unit

- quarry-runtime `src/usage.rs`: seven metric constants and `BROWSER_METRICS`; `UsageEvent::keyed` with billing-core id rule; `UsageMeter::flush` (default no-op) and `browser_metrics_mode` (default off); `BrowserMetricsMode`; `PublishTracker` in `NatsUsageMeter`; tests in `metering_tests`.
- quarry-runtime `src/lib.rs`: re-export line (additive). `src/step_receipts.rs`: `BudgetCheckpoint.segment` (serde default, skipped at 0), one test literal updated.
- quarry-edge `src/usage_attrs.rs` (NEW): `Surface`, `surface_for`, `Outcome`, `UsageAttrs`, `metadata` (ZDR allowlist), `emit`, `emit_run_segment`, `ShadowMeter` gate, `browser_metrics_mode_from_env`, `corrected_rules`; tests named by the queue row.
- quarry-edge `src/budget.rs`: `RunBudget` segment fields, `with_surface`, `segment_usage`, `SegmentUsage`; `segment_tests`.
- quarry-edge `src/agent_routes/close.rs` (finish_run segment emission; finish_run lives here, not in lifecycle.rs), `src/agent_routes/lifecycle.rs` (start_run takes `HeaderMap`, stamps surface on budget).
- quarry-edge `src/routes.rs` (InternalRunPage.kind, meter_crawl_page, scrape metadata under on, crawl and batch billed on Ok under on), `src/extract_routes.rs`, `src/map_routes.rs` (gate-based emission, legacy search.query kept until on), `src/lib.rs` and `src/main.rs` (`usage_attrs` module, meter wrap).
- orchestrator `internal/activities/activities.go` (RunPageInput.Kind), `internal/workflows/workflows.go` (runPage kind param; crawl and batch callers), `internal/activities/activities_test.go` (NEW, httptest asserts kind).
- docs: `docs/BILLING.md`, `docs/CONTRACTS.md` (appended sections), `docs/RUNBOOKS.md` (appended section).
- depth: `.plan-work/quarry-w1w2/depth/W2-U37.md`.

## Build method

Other units have uncommitted hunks in several of these files (routes.rs, main.rs, lifecycle.rs, step_receipts.rs, runtime lib.rs, RUNBOOKS.md). The unit's edits were re-applied by the same scripts to the HEAD blobs (`U37/build-mirror.sh`), and the compile check runs on a clean export of HEAD plus the unit's files (`U37/export-tree.sh`), through a private index. The commit is made with qcommit-blobs.sh from the mirror, so other units' hunks stay uncommitted.

## Next

1. Read the compile result of the clean export (`U37/check3.log`).
2. Commit code (qcommit-blobs.sh plus qcommit.sh for the two new files), then docs, then the plan-work files.
3. Update W2-U37-progress.md with the hashes.
