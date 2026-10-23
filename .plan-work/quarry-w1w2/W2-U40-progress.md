# W2-U40 progress (WS13.3: driver capture API and single-capture implementation)

Status: BUILT to scope and compile-checked in part. NOT run: no test suite, no Chromium run, no spike. Nothing is deployed. No flag (no caller; rollback = revert).

## Committed (local branch quarryv2-next, not pushed)
- 60744383 feat(quarry-core): screenshot_jpeg artifact kind and image/jpeg type (WS13.3). Files: quarry-core `src/artifact.rs`, quarry-runtime `src/artifact_store.rs` (kind_from_str arm).
- 40cab60e feat(quarry-browser): capture API on the driver, capture planning, typed element reasons (WS13.3). Files: quarry-browser `src/capture_plan.rs` NEW (pure planning, header reads, size cap, pdf ranges), `src/capture_live.rs` NEW (live tests, include! in the chromiumoxide test module), `src/chromiumoxide.rs` (capture_screenshot, capture_pdf, current_url, capability flags, typed details.reason on wait_for/click/type_text/select), `src/lib.rs` (pub mod capture_plan; capability fields scrape_actions and capture_options; trait defaults; capture_default_tests module).
- d4ae7ed4 docs(quarry-v2): docs/SCRAPE_CAPTURE.md NEW (spike table, empty), docs/RUNBOOKS.md (append-only WS13.3 section).

## Not committed by this unit
- Working-tree hunks of other units in quarry-browser `src/lib.rs` (W2-U39 open-web policy: EgressMode, BrokerNavPolicy, ...) and untracked `tests/open_web_policy.rs`. Not ours; left in place.
- Working-tree hunks in docs/RUNBOOKS.md, BILLING.md, CONTRACTS.md, openapi.yaml and others: not ours.
- The `.plan-work/quarry-w1w2/` notes (this file, wf-W2-U40.md, depth/W2-U40.md): committed in the last commit of this unit.

## Verified (what was actually run)
- `cargo check -p quarry-core --tests` (qtest, quarry-rust-env:2): rc 0 (before the commits, with the artifact change).
- `cargo check -p quarry-browser --tests`: rc 0.
- `cargo check -p quarry-browser --features chromiumoxide --tests` (includes the live test module, compiled not run): rc 0.
- `rustfmt --edition 2021 --check` on capture_plan.rs and capture_live.rs: clean after one `rustfmt` run that formatted those two new files only.
- `cargo check -p quarry-runtime --tests`: the lib builds with the artifact_store change; the lib-test build FAILS in `crates/quarry-runtime/src/step_receipts.rs` lines 878 and 1241 (`missing field policy_mode in AgentRunCheckpoint`). That file is W2-U39's in-progress work, not this unit's. Retried twice (after the first attempt the blocker was still there at the second). Not run: quarry-runtime tests.
- Gitleaks pre-commit hook: no leaks, both commits.

## Not run (build-first rule)
- Every test named in the plan for WS13.3: `cargo test -p quarry-browser plan_screenshot png_header`, `capture_default_tests`, `cargo test -p quarry-core artifact`, and the four live tests in capture_live.rs.
- The spike (20,000 and 32,000 CSS px at DSF 1 and 2). SINGLE_CAPTURE_MAX_DEVICE_PX stays the provisional 16000.

## For the next batch
- W2-U45 and W2-U53 depend on U40. The driver API is in place: `BrowserDriver::capture_screenshot(session, &ScreenshotOptions, &CaptureLimits)`, `capture_pdf(session, &PdfOptions, &CaptureLimits)`, `current_url(session)`.
- WS13.4b (wiring formats and captures into POST /v1/scrape) must map the capture_plan errors (invalid_clip is BadRequest; capture_too_large, screenshot_unreadable, pdf_unreadable are DriverFailed with details.reason).
- SINGLE_CAPTURE_MAX_DEVICE_PX must be set from the spike before WS13.4b ships a full-page default.
- Hand-over and depth: `.plan-work/quarry-w1w2/wf-W2-U40.md`, `.plan-work/quarry-w1w2/depth/W2-U40.md`.
