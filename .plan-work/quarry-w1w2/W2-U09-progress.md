# W2-U09 progress (WS13.2: contract types for scrape capture and actions)

Unit: W2-U09, slice WS13.2 (quarry-core only: types and validation, no caller).
Worktree: C:/dev/CoresSystem/.claude/worktrees/vigorous-perlman-e7ae5b, branch quarryv2-next.
Crate: apps/Ingestion Plane/Quarry-v2/crates/quarry-core.

## Status: built to scope, committed, tests written and compile-checked, NOT run

## Committed (local branch quarryv2-next)

- a6c7c42a5 feat(quarry-core): capture options and scrape action types, validated and dark (WS13.2)
  - NEW crates/quarry-core/src/capture.rs: ImageFormat, ClipRect, CaptureViewport, ScreenshotOptions, PdfPaper, PdfOptions, CapturePlan, CaptureLimits (lowered, never raised), CapturedImage, CapturedPdf; range checks with closed codes.
  - NEW crates/quarry-core/src/scrape_actions.rs: ScrapeAction (closed set), denied names, credential heuristic, ActionLimits, validate_actions, ValidatedActions, ActionsSummary, ActionOutcome metadata.
  - crates/quarry-core/src/lib.rs: two mod lines (H5).
- 242c58cc5 feat(quarry-core): scrape formats screenshot and pdf, and actions, parsed and checked, off by default (WS13.2)
  - crates/quarry-core/src/scrape.rs: formats screenshot/pdf (string and object form), actions and actionsOnError, feature flags captures_enabled and actions_enabled (default false, feature_disabled), ResolvedScrapeOptions capture/actions, cache_admissible(), canonical key additions only when present, EffectiveScrapeOptions.actions (counts only), 14 new or updated tests.
  - docs/schemas/scrape-options.schema.json and effective-scrape-options.schema.json regenerated.
- 130950cb3 test(quarry-core): rename the field-names pin test to the plan's name (W2-U09)
- 0a5707ac2 docs(quarry-core): RUNBOOKS section for WS13.2 (committed from the HEAD blob plus this section only; the working-tree copy still carries other units' uncommitted hunks, left alone)

## Verified (commands run and results)

- `cargo check -p quarry-core --tests` (qtest, quarry-rust-env:2): exit 0, no warnings from the W2-U09 files.
- `cargo check -p quarry-core --tests --features jsonschema`: exit 0.
- `UPDATE_SCHEMAS=1 cargo test -p quarry-core --features jsonschema --test scrape_schema` (regeneration, not a suite run): 2 passed; goldens rewritten.
- `cargo check -p quarry-edge --tests` (downstream consumer of the scrape types): exit 0; its warnings are pre-existing (fleet_routes, run_live, browser_grants_prod).

## Not run

- No test suite is run. The unit tests in capture.rs, scrape_actions.rs and scrape.rs are written and compile-checked; they run in the wave's final test phase.

## Not done (in scope but outside this unit's boundary)

- Nothing in quarry-edge reads the capture or action fields yet (WS13.4b and WS13.5). The flags are off, so requests for them get 400 feature_disabled.
- CONTRACTS.md and openapi.yaml text for the new fields and codes (WS13.8).
- Model Plane vendored copy of scrape-options.schema.json (sibling b).

## Notes for the next batch

- ScrapeFormat is PartialEq only (no Eq, no Hash). ResolvedScrapeOptions and EffectiveScrapeOptions are PartialEq only.
- ScrapeFormat has manual Serialize (names), Deserialize (names, default options for screenshot and pdf) and JsonSchema (anyOf) impls.
- WS13.4b must set ScrapeLimits.captures_enabled and actions_enabled from QUARRY_EDGE__SCRAPE_CAPTURE and QUARRY_EDGE__SCRAPE_ACTIONS, and read the env caps through CaptureLimits::lowered and ActionLimits::lowered.
- Depth bullets: .plan-work/quarry-w1w2/depth/W2-U09.md (17 bullets).
