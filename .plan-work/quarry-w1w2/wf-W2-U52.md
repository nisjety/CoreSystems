# wf-W2-U52 hand-over (WS12.S5 screenshot masking)

## What was built
Scan -> classify -> paint, at the capture call, so every consumer of a screenshot gets the masked image.
- `quarry-browser/src/sensitive_rects.rs`: `SCAN_SCRIPT` (read-only walk: input/textarea/select/[contenteditable], open shadow roots, same-origin iframes with offsets, caps 20k nodes / 512 fields / 8 documents), `parse_scan` (classification is `quarry_core::redaction::field::classify`, never the script), `scan_page`, `scroll_now`, 1.5 s budget each.
- `quarry-browser/src/screenshot_mask.rs`: `mask_png` (png crate, `#1f2937`, 40 Mpx cap, RGB/RGBA 8-bit), `scale_rect` (origin, DPR, 4 CSS px padding, round outwards, clamp), `plan_mask`, `Withheld` (closed notes), `MaskConfig` flags, `capture_masked` (scan, capture, scroll re-check, one re-scan, else withhold), `pdf_allowed`.
- `lib.rs`: `MaskedScreenshot { bytes, state, rects, oopif_frames, mask_ms, note }`, `BrowserDriver::screenshot_masked` (default: `screenshot` + `none`).
- `chromiumoxide.rs`: `screenshot()` = masked bytes or `SecurityBlocked`; `screenshot_masked`; `capture_screenshot_on` masked (single, clip, stitched); `pdf` and `capture_pdf` refuse while a rendered sensitive field has a value.
- Runtime: `observation.rs` uses `screenshot_masked`; withheld = no artifact, no sidecar, previous frame cleared, `quarry.screenshot.withheld` info line, stamp `withheld`. Explicit screenshot action = 403.
- Stamp: `screenshot` (none/masked/withheld), `mask_ms`, `oopif_frames`, `rects`.

## Decisions and deviations
- `png` stays 0.18 (WS13.6's entry); the queue said 0.17; no Cargo.toml edit.
- Scan facts include the field's `value` (at most 40 chars, never for `type=password`) only for the Luhn card test; it is dropped after classification.
- Where the plan says "note `screenshot_withheld:sensitive_field_unresolved`": no `notes` field exists on `BrowserObservation`; the note is the stamp, a log line and the 403 reason.
- OOPIF default `allow` per plan: counted, not masked.

## Traps found
- `RectScan` rectangles are client coordinates of the top frame; `plan_mask` subtracts `origin - scroll` to get image pixels. A viewport image has origin = scroll, a whole-page image (0,0), a clip its corner.
- `cdp_calls_are_bounded` scans chromiumoxide.rs only; the scan/scroll `page.evaluate` calls live in sensitive_rects.rs under their own `tokio::time::timeout`.
- A bash heredoc over ~50 lines failed in the Bash tool; use the Write tool for long scripts.
