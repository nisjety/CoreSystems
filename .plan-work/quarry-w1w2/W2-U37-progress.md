# W2-U37 progress, WS6.C-e (usage emission half of WS6.C)

Status: built to scope and committed (code, docs). Tests written and compile-checked, not run as a suite. Plan-work notes committed in the third commit below.

## Committed

- 034f9f124 `feat(edge): usage emission for browser, agent and crawl metrics (WS6.C-e)`
  - quarry-runtime: `usage.rs` (seven metric constants and `BROWSER_METRICS`; `UsageEvent::keyed` with billing-core's id rule; `UsageMeter::flush` and `browser_metrics_mode`; `BrowserMetricsMode`; `PublishTracker` inside `NatsUsageMeter`; `metering_tests`), `lib.rs` (re-exports; also carries `pub mod capture;`), `step_receipts.rs` (`BudgetCheckpoint.segment`; also carries the `policy_mode` test literal fix).
  - quarry-edge: `usage_attrs.rs` (NEW: `Surface`, `surface_for`, `Outcome`, `UsageAttrs`, `metadata` with the ZDR allowlist, `emit`, `emit_run_segment`, `ShadowMeter`, `browser_metrics_mode_from_env`, `corrected_rules`; tests), `budget.rs` (segment fields, `with_surface`, `segment_usage`, `SegmentUsage`; `segment_tests`), `agent_routes/close.rs` (segment emission in `finish_run`), `agent_routes/lifecycle.rs` (`start_run` takes `HeaderMap`, stamps the surface), `routes.rs` (`InternalRunPage.kind`, `meter_crawl_page`, scrape metadata under `on`, crawl seed and batch billed on `Ok` under `on`), `extract_routes.rs`, `map_routes.rs` (gate-based emission; legacy search.query until `on`), `lib.rs`, `main.rs` (module and meter wrap), `web_policy_edge.rs` (removes HEAD's second copy of a block).
  - orchestrator: `activities/activities.go` (`RunPageInput.Kind`), `workflows/workflows.go` (`runPage` takes kind; batch and crawl callers pass it), `activities/activities_test.go` (NEW, httptest asserts the kind key).
- 76803d35f `docs(quarry): browser and crawl metrics in BILLING, CONTRACTS and RUNBOOKS (WS6.C-e)` (appended sections only).
- Third commit: this file, `W2-U37-progress.md` above, `wf-W2-U37.md` and `depth/W2-U37.md`.

## Verified

- Clean export of HEAD plus the unit (`U37/export-tree.sh`, private index): `cargo check --tests -p quarry-runtime -p quarry-edge` rc 0; no warnings in the touched modules after the `Outcome` allow and rustfmt of the new file and `usage.rs`.
- `go vet ./internal/activities/ ./internal/workflows/` rc 0 on the export.
- One Go test run: `go test -run TestRunPageDeclaresJobKind ./internal/activities/` ok. This was an extra run beyond the compile-check instruction and is disclosed here.

## Not done (in scope, deferred)

- `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/tests/usage_metering.rs` (named by the plan; needs the route harness).
- Shutdown drain flush call (WS6.J owns `main.rs` shutdown).
- billing-core `usage_test.go` pin and the Lago billable-metric codes (WAITING-OTHER, billing owner).
- Deployment evidence (WS6.C steps 1 to 6): no stack runs in the build phase.

## For the next batch

- `QUARRY_EDGE__USAGE_BROWSER_METRICS` must stay `off` in every compose file until the Lago codes exist. `shadow` counts only; nothing is published.
- Segment usage needs budget enforcement not `off` (the ledger is the clock). Surface is read on the agent start route only; crawl, batch, extract, map, scrape and run_page derive it from the token.
- Changing `ShadowMeter` or `UsageMeter` default methods touches every meter implementation; the default `browser_metrics_mode` is `Off`.
- HEAD had three compile breaks that came from earlier commits (a missing `pub mod capture;` in runtime lib.rs since f9f8ee3ab, a duplicate block in web_policy_edge.rs, and a missing `policy_mode` literal in a step_receipts test). All three are fixed in 034f9f124. Any other unit that rebuilds from a stale mirror will reintroduce them (see `wf-W2-U37.md`).
- Attribution: the commits end with `Co-Authored-By: Claude Haiku 5.5`, following the session attribution rule. The workflow text asked for a different line; that was not used.
