# W2-U50 progress (WS13.6: tile-and-stitch for full-page screenshots taller than one capture, batch 9)

Status: BUILT to scope and compile-checked. NOT run: no test suite, no Chromium run, no spike, no stack evidence. Flag `QUARRY_EDGE__SCRAPE_STITCH` default off, so nothing changes until it is set.

## What is built

- `quarry-browser/src/stitch.rs` (NEW): pure planning (`plan_tiles`, `tile_css_height`, `needs_stitch`, `device_px`, `tile_device_rows`, `stitch_deadline`) and the encoders: `Stitcher` streams PNG (one decoded tile held at a time, `SharedBytes` sink) and encodes JPEG at the end (`jpeg-encoder`, over white). `decode_tile_rgba` reads a Chromium tile PNG (RGBA or RGB, 8-bit, cropped to the asked rows). Limits from `quarry-core/src/capture.rs` (4,096 CSS px per tile, 16 tiles, 60,000 CSS px, 40 Mpx PNG, 24 Mpx JPEG).
- `quarry-browser/src/chromiumoxide.rs`: `capture_screenshot` is now a wrapper over `capture_screenshot_stitched(.., stitch: Option<&StitchPermit>)`. `capture_screenshot_on` stitches a full page taller than one capture when a permit was passed (method A: clip tiles, `captureBeyondViewport`, no overlap). `screenshot_needs_stitch` reads the layout (one CDP call). `stitch_full_page` has the per-tile deadline (10 s) and the overall `stitch_deadline`.
- `quarry-browser/src/lib.rs`: `pub mod stitch`, `StitchPermit` (wraps an `OwnedSemaphorePermit`), trait default methods `screenshot_needs_stitch` (false) and `capture_screenshot_stitched` (falls back to `capture_screenshot`). Other drivers inherit the defaults.
- `quarry-browser/src/stitch_live.rs` (NEW, included in the chromiumoxide test module): `banded_25000px_page_has_correct_band_order_in_the_stitched_png`, `permit_busy_falls_back_to_crop_with_warning`.
- `quarry-browser/Cargo.toml`: `png = "0.18"`, `jpeg-encoder = "0.6"` (plain dependencies, see the deviation below).
- `quarry-runtime/src/browser_driver.rs`: `ENV_SCRAPE_STITCH` (`QUARRY_EDGE__SCRAPE_STITCH`), `scrape_stitch_from` / `scrape_stitch_enabled` (on for `on`, `1`, `true`; default off), an adapter field read at build time with `scrape_stitch()` and `with_scrape_stitch()`, `stitch_permit_for` (takes one heavy-lane permit with `try_acquire_owned` only for a full-page capture whose page needs stitching; a busy lane gives none and the capture is cropped with `screenshot_truncated`), and `take_captures` now calls `capture_screenshot_stitched`.
- `docs/RUNBOOKS.md`: section "Tall full-page screenshots: tile-and-stitch (WS13.6 ...)" appended.

## Deviations from the plan (stated)

- `png` and `jpeg-encoder` are plain dependencies, not optional behind the `chromiumoxide` feature. The planning and PNG tests run in the default build, and the plan's test command `cargo test -p quarry-browser stitch` needs them there.
- The flag is read by quarry-runtime from `QUARRY_EDGE__SCRAPE_STITCH` at adapter build time (the same way `QUARRY_SCRAPE_SESSION_ISOLATION` is read), not by quarry-edge's config struct.
- `QUARRY_SCRAPE_STITCH_METHOD` is not read: method B (scroll) is not built. The spike that picks the method has not run.
- The "stitch permit" is decided by a second layout read (`screenshot_needs_stitch`); the depth file records the double read.
- The stitched PNG and JPEG are the only new output; no wire change.

## Committed (local branch quarryv2-next, not pushed)

- faf20741 feat(quarry-browser): tile-and-stitch planning and the PNG and JPEG stitchers (WS13.6). Files: quarry-browser stitch.rs (new), lib.rs (also carries the one-line cfg removal above capture_plan, see below), Cargo.toml.
- d72c9b1b feat(quarry-browser): stitched full-page screenshot in the chromiumoxide driver (WS13.6). Files: chromiumoxide.rs, stitch_live.rs (new).
- 8e6ed343 feat(quarry-runtime): stitch permit from the heavy lane behind QUARRY_EDGE__SCRAPE_STITCH (WS13.6). Files: browser_driver.rs.
- a9f69281 docs(quarry-v2): tile-and-stitch RUNBOOKS section for WS13.6 (W2-U50). Files: docs/RUNBOOKS.md (HEAD plus this section).
- (this commit) docs(plan-work): W2-U50 progress, depth and hand-over notes (WS13.6).

## Not committed by this unit

- Nothing of another unit is committed by this unit, except the one-line `lib.rs` hunk noted below.
- `quarry-browser/src/lib.rs` in the working tree carried another unit's uncommitted one-line change (the `#[cfg(feature = "chromiumoxide")]` removed above `pub mod capture_plan;`). The trait default methods in lib.rs call `crate::capture_plan`, so HEAD's default build does not compile without that line. The W2-U50 commit includes it and says so in its message.
- `docs/RUNBOOKS.md` had another unit's uncommitted hunks; the W2-U50 commit is built from HEAD plus this section only, and the working copy receives the same section append, so the other hunks stay uncommitted.

## Verified (what was actually run, 2026-10-24, qtest quarry-rust-env:2)

- `cargo check -p quarry-browser --tests` (default features): rc 0. No warnings from the new code (the pre-existing dead-code warnings in deadlines.rs are not ours).
- `cargo check -p quarry-browser --features chromiumoxide --tests` (includes stitch_live.rs, compiled not run): rc 0.
- `cargo check -p quarry-runtime --tests`: rc 0.
- `rustfmt --edition 2021` on stitch.rs and stitch_live.rs (written in place); the two rustfmt hunks in chromiumoxide.rs that were this unit's were reformatted by hand; the remaining rustfmt hunks in chromiumoxide.rs are pre-existing (lines 27-81, 764, 1549-2104, 2930, 3428 and later) and are not touched.

## Not run (build-first rule, 2026-10-23 strategy)

- Every test named in the queue row: `cargo test -p quarry-browser stitch`: `tile_plan_covers_the_content_with_no_gap_or_overlap`, `tile_plan_respects_max_tiles_and_marks_truncated`, `png_stitch_output_has_the_expected_dimensions_and_is_valid`, `streaming_writer_memory_is_bounded`, `jpeg_stitch_cap_is_lower_than_png`. Also written, not named in the queue: `dsf_two_halves_tile_css_height`, `deadline_is_min_of_tiles_times_10s_and_60s`, `a_tile_that_is_not_a_png_or_has_the_wrong_size_is_refused`.
- Live Chromium: `stitch_live` (2 tests).
- Edge tests named in the WS13.6 detail (`stitch_flag_off_crops_and_warns`, `stitch_on_returns_stitched_true`): not written (see depth).
- The spike (SCRAPE_CAPTURE.md) and the stack evidence.

## For the next batch

- U52 (WS12.S5, screenshot masking) edits chromiumoxide.rs at screenshot 2392-2406 and the pdf region, and lib.rs `BrowserDriver::screenshot_masked`. Its queue position is after U50 (H4 single driver queue). `capture_screenshot_on` now takes a `stitch` argument and `capture_screenshot` is a wrapper: U52 must mask the stitched and the cropped path, or mask the final image.
- `Cargo.toml` of quarry-browser now has `png` and `jpeg-encoder`. U52 adds `png = 0.17` per the queue (H22): one dependency entry, so align on `png` 0.18 (the version already here).
- The Model Plane's vendored copy of the scrape schema is not touched.
- Hand-over and depth: `.plan-work/quarry-w1w2/wf-W2-U50.md`, `.plan-work/quarry-w1w2/depth/W2-U50.md`.
