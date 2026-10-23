# W2-U52 progress (WS12.S5: screenshot masking by element rectangle, fail closed, batch 10)

Status: BUILT to scope and compile-checked. NOT run: no test suite, no Chromium, no stack evidence (build-first rule 2026-10-23). Flags: `QUARRY_BROWSER_SCREENSHOT_MASK=on|off` (default on, off ignored in production), `QUARRY_BROWSER_SCREENSHOT_OOPIF=allow|mask` (default allow). Note: masking is on by default (the plan's default); there is no dark switch other than the flag.

## Committed (local branch quarryv2-next, not pushed)
- 36d2f570 feat(quarry-core): stamp.rs (mask_ms, oopif_frames, rects).
- 8c4035e3 feat(quarry-browser): sensitive_rects.rs (NEW), screenshot_mask.rs (NEW), screenshot_mask_live.rs (NEW), lib.rs (MaskedScreenshot, screenshot_masked, mod lines), chromiumoxide.rs (masked screenshot(), screenshot_masked, masked capture_screenshot_on incl. stitched, pdf and capture_pdf refusal, capture_png helper, include of the live file).
- d311efb6 feat(quarry-runtime): observation.rs (screenshot_masked at both capture sites, withheld path, stamp), observation_redaction.rs (ScreenshotReport, Redactor::screenshot), test_site.rs (/secrets-lab iframe, /secrets-lab/frame, /secrets-lab/scrolled), three tests.
- 44d45d8b docs(quarry-v2): SECRET_REDACTION.md (S5 section and row), openapi.yaml (3 stamp fields, built from HEAD plus my hunk), RUNBOOKS.md (section, from HEAD plus my section).
- (this commit) docs(plan-work): this file, the depth file, the hand-over note.

## Not committed / not ours
- openapi.yaml and RUNBOOKS.md working copies still carry other units' uncommitted hunks; I appended the same section to the RUNBOOKS working copy, so a later whole-file commit by another unit would not duplicate it (it is already in HEAD) as long as that unit builds from HEAD.

## Verified (qtest, quarry-rust-env:2, 2026-10-24)
- cargo check -p quarry-browser --tests: rc 0, no warning from the new code.
- cargo check -p quarry-browser --features chromiumoxide --tests: rc 0 (compiles screenshot_mask_live.rs).
- cargo check -p quarry-runtime --tests: rc 0. cargo check -p quarry-core -p quarry-edge --tests: rc 0.
- rustfmt --edition 2021 on the three new browser files.

## Not run
- `cargo test -p quarry-browser screenshot_mask` (mask_png_blocks_exact_rect_and_leaves_rest_untouched, mask_png_rejects_oversize, rect_scaling_uses_device_pixel_ratio, padding_clamps_to_image, plus plan/origin/flag tests), `cargo test -p quarry-browser sensitive_rects`, `cargo test -p quarry-core redaction::stamp`, `cargo test -p quarry-runtime observation withheld_screenshot` (and the two sibling tests), live `screenshot_mask_live` (six cases), `cargo test --workspace`, log_field_discipline, cdp_calls_are_bounded.

## For the next batch / test phase
- chromiumoxide.rs: `capture_screenshot_on` now takes the masking decision from `MaskConfig::from_env()`; U55 (WS17.C1) queues after this edit. Every raw capture must go through `capture_masked` (`capture_png` is private for that reason).
- The scrape capture path returns SecurityBlocked 403 with `details.reason` `screenshot_withheld:*` / `pdf_withheld:*`; the edge scrape route, gateway and SPA have no wording for it (depth file).
- Hand-over and depth: `.plan-work/quarry-w1w2/wf-W2-U52.md`, `.plan-work/quarry-w1w2/depth/W2-U52.md`.
