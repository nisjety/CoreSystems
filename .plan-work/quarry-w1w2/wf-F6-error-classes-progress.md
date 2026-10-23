# F6-error-classes progress

## 2026-10-18 step 1 in progress (error classes)
- Edits applied, UNCOMMITTED, not yet compiled: quarry-edge (admission, lifecycle, live, run_sweeper, ws, auth, cache,
  change_routes, config, resource_routes, routes, search_routes), quarry-runtime (action_runtime, autoprompt,
  browser_egress_proxy, fallback_driver, fetch, hybrid, intent_classifier, observation, page_extract, pipeline, rerank,
  serp, smart_router, transport_fallback_driver), quarry-browser (chromiumoxide, kernel, deadlines::cdp_class).
- Scripts: scratchpad/F6-error-classes/edit-edge.js, edit-rt.js (already run, do not re-run).
- Next: cargo check --workspace --all-targets via qtest; fix type errors (some sites assume QuarryError); then commit 1;
  then LOG_FIELD_BASELINE_UPDATE=1 (commit 2); then canary test + matrix row (commit 3).
- 17:xx cargo check --workspace --all-targets rc=0 after edits. Baseline regenerated (117 -> 56 total, R2 83 -> 23, no rise). cdp_class test passes (10 deadlines tests).
- Running full cargo test -p quarry-edge (scratchpad/F6-error-classes/edge1.log). Then commit 1+2, then canary test (use zdr_harness::Harness::start_with + agent_harness::RecordingDriver, feature browser-agent).
- DONE 2026-10-18: commits df7feedf (error classes), 1db86f58 (baseline 117->56, R2 83->23), 6b9c636a (canary + matrix row + golden). Tree clean for Quarry-v2.

## 2026-10-18 review round DONE
Commits 67cff1f3 (fix: fallback_driver/fetch drop url/host, stream runner .instrument(request_span), tls/preflight converted, 6 annotations, baseline 56->48), 700ac45e (capture tests + class_reqwest unit tests), 66a0cd16 (lint R5/R6/bare %e). Tree clean for Quarry-v2.
Tests: edge --no-fail-fast 22 bins 848 pass; runtime lib 713; core 162; tls+security lib 46; check --workspace --all-targets ok.

## 2026-10-18 second review round DONE
Commit 89c67974: scrape_stream matrix row -> Canary (golden regenerated), correction of 67cff1f3 baseline figures (44 total / 18 keys, not 48/21). Trade-off finding documented in the message. Tests: core all, edge zdr_feature_matrix 7 / zdr_gap_rows 9 / zdr_log_capture 12 pass.
