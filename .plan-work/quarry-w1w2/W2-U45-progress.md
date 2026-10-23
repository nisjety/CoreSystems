# W2-U45 progress (WS13.4b: screenshot and pdf on POST /v1/scrape, batch 8)

Status: BUILT to the plan's first shippable half, behind QUARRY_EDGE__SCRAPE_CAPTURE=off (committed default).
Compile-checked, NOT run (build-first rule, 2026-10-23 strategy). Nothing deployed; no stack evidence.

## Committed (local branch quarryv2-next, not pushed)
- 01c65beb feat(quarry-core): scrape response carries inline captures and measured captures (WS13.4b). scrape.rs only.
- e8781375 feat(quarry-runtime): browser captures on scrape, refused without the capability (WS13.4b).
  capture.rs, driver.rs, browser_driver.rs, pipeline.rs.
- 11c22dae feat(quarry-edge): screenshot and pdf on POST /v1/scrape behind QUARRY_EDGE__SCRAPE_CAPTURE (WS13.4b).
  scrape_actions.rs (new), tests/scrape_capture.rs (new), lib.rs, scrape_contract.rs, routes.rs (6 hunks),
  apps/Ingestion Plane/docker-compose.yml (2 lines). NOTE: its routes.rs blob reverted WS6.D-s (see next).
- 52cd874c8 fix(quarry-edge): restore the routes.rs WS6.D-s changes lost in 11c22dae (WS13.4b). Restores routes.rs
  to cfdc8a07a plus the six WS13.4b hunks only.
- 26a212c0 docs(quarry-v2): WS13.4b capture formats in CONTRACTS and RUNBOOKS (W2-U45). CONTRACTS.md, RUNBOOKS.md.

## Not committed by this unit (other units' hunks left in the tree)
- quarry-edge src/routes.rs: other units' hunks (router layers, WS6.D-s handoff code in the working copy, WS17/WS6.D test lines).
- quarry-edge src/lib.rs: `pub mod redaction_boot;` (WS12.S2, now committed by b2873b3df).
- openapi.yaml (other unit's hunks): the WS13.4b request and response additions are owed there.
- deploy/compose/docker-compose.yml: no quarry-edge scrape block; not edited.

## Verified (what was actually run, 2026-10-24, qtest quarry-rust-env:2)
- cargo check --tests -p quarry-core -p quarry-runtime -p quarry-edge: first run, only error sites were my
  RenderHints literal in routes.rs (fixed) and the other unit's budget.rs test helpers (not mine). Core had no errors.
- cargo check --tests -p quarry-runtime: rc 0 after the RenderHints test literal fix and the dead-assignment removal.
- cargo check -p quarry-edge --lib: rc 0.
- cargo check -p quarry-edge --test scrape_capture: rc 0.
- Not checked: cargo check --tests -p quarry-edge (lib test target blocked by budget.rs, another unit's); the five
  unit tests in src/scrape_actions.rs therefore compile-unverified.

## Not run (by strategy)
- Every test named in the queue row and the plan: the browser_driver tests (4), the pipeline capture tests (3),
  tests/scrape_capture.rs (3). Written and compiled, not run.
- The plan tests not written (listed in depth/W2-U45.md, first test-phase bullet).
- The live Chromium path, the 501 path on a live stack, ZDR inline bytes, the flag-on evidence.

## For the next batch
- Blob commits: rebuild the blob from HEAD immediately before committing and check `git diff HEAD -- <path>` shows only
  your hunks. 11c22dae is the example of what goes wrong (it reverted another unit's commit).
- The driver's refusal is the enforcement for captures; scrape_capabilities() is informational until an edge gate uses it.
- SINGLE_CAPTURE_MAX_DEVICE_PX (16000) is still provisional; W2-U53 (WS13.6 stitch) and the WS13.3 spike own it.
- Hand-over and depth: .plan-work/quarry-w1w2/wf-W2-U45.md and depth/W2-U45.md.
