# Scrape-time capture: single-capture limits (WS13.3)

Status (2026-10-24): built, NOT run. The driver API (W2-U40, `quarry-browser`), the wire (`formats` captures, WS13.4b, W2-U45), the tile-and-stitch (WS13.6, W2-U50) and the scrape actions (WS13.5, W2-U53, partial) are in the tree, all behind `QUARRY_EDGE__SCRAPE_CAPTURE`, `QUARRY_EDGE__SCRAPE_STITCH` and `QUARRY_EDGE__SCRAPE_ACTIONS`, all off by default. The spike table below is not measured yet. Nothing here is evidenced on a stack.

## What the driver API does

`quarry-browser` gains three trait methods on `BrowserDriver` (`crates/quarry-browser/src/lib.rs`):

- `capture_screenshot(session, &ScreenshotOptions, &CaptureLimits) -> CapturedImage`. Legacy
  options (every option at its default) delegate to `screenshot`. Any other option needs a
  driver that overrides the method; the default refuses it with `unsupported`.
- `capture_pdf(session, &PdfOptions, &CaptureLimits) -> CapturedPdf`. Default options delegate
  to `pdf`; other options need an override.
- `current_url(session) -> Option<String>`. Default `None`.

`ChromiumoxideDriver` overrides all three and sets the capability flags `scrape_actions` and
`capture_options` to true. Other adapters (browserless, browserbase, kernel) inherit the
defaults unchanged, so a capture request against them is refused, never served badly.

The planning is pure and lives in `crates/quarry-browser/src/capture_plan.rs`:

- `plan_screenshot(content_w, content_h, &ScreenshotOptions, dsf)`: a full-page capture is one
  clip over the whole content, cropped at `SINGLE_CAPTURE_MAX_DEVICE_PX / dsf` CSS pixels and
  marked `truncated` when cropped. A clip is intersected with the content; a clip that lies
  outside it is `invalid_clip`, not an empty image.
- `image_dimensions(bytes)`: the pixel size from the PNG IHDR chunk or the JPEG start-of-frame
  segment. The size of the image Chromium returned is what goes in the response, never the
  size asked for.
- `check_capture_size(bytes, &CaptureLimits)`: `capture_too_large` above the cap.
- `pdf_page_ranges(max_pages)`: `1-N`.

## Constants (`crates/quarry-core/src/capture.rs`)

| Constant | Value | Source |
|---|---|---|
| `SINGLE_CAPTURE_MAX_DEVICE_PX` | 16,000 | PROVISIONAL. Set by the spike below, not measured. |
| `MAX_CAPTURE_BYTES` | 32 MiB | Encoded cap per capture. |
| `MAX_CAPTURES` | 8 | Per request, formats and actions together. |
| `MAX_PDF_PAGES` | 200 (default 100) | `maxPages`. |

The provisional 16,000 device pixels is a guess, not a measurement. Do not raise it, and do not
rely on a full-page capture of a page taller than it, until the spike table is filled in.

## Spike table (NOT MEASURED)

The spike captures fixtures of 20,000 and 32,000 CSS pixels tall at device scale factor 1 and 2,
with `captureBeyondViewport`, on the pinned Chromium. Record for each cell the largest correct
height, and the failure mode (truncation, blank image, or error). Then set
`SINGLE_CAPTURE_MAX_DEVICE_PX` from the result.

| Fixture (CSS px tall) | DSF | Chromium build | Result | Failure mode | Measured by |
|---|---|---|---|---|---|
| 20,000 | 1 | 149.0.7827.102 [UNVERIFIED for the pinned build] | not run | not run | not run |
| 32,000 | 1 | 149.0.7827.102 [UNVERIFIED for the pinned build] | not run | not run | not run |
| 20,000 | 2 | 149.0.7827.102 [UNVERIFIED for the pinned build] | not run | not run | not run |
| 32,000 | 2 | 149.0.7827.102 [UNVERIFIED for the pinned build] | not run | not run | not run |

Run it in the live lane of the rebuilt edge image (`docker run --rm` the built `quarry-edge`
image, or the WS3 live-lane container, with `CHROMIUMOXIDE_TEST=1`), not on a developer's
Chrome.

## Rebase notes

- WS3 S2 (`resolve_viewport`) and WS3 S3 (`Op::Screenshot`, 10 s) are the base of
  `capture_screenshot`. The viewport override is restored on every path after it is applied.
- WS13.4a added `FetchResponse.captures` (`quarry-runtime/src/capture.rs`). Nothing in this
  unit reads it yet; WS13.4b does.
- The agent path (`observation.rs`) still calls the legacy `screenshot` and `pdf`. It is not
  changed by this unit.

## Not done in this unit

- The spike measurement and the value of `SINGLE_CAPTURE_MAX_DEVICE_PX` (above).
- The request wiring: `formats`, `captures` on `/v1/scrape`, the flag `QUARRY_EDGE__SCRAPE_CAPTURE`
  (WS13.4b).
- Memory evidence for a tall `fullPage` capture. A capture is held as bytes until the response is
  built, so a 16,000 px PNG is held in full.

## Flags, limits and the evidence owed (WS13.8, 2026-10-24)

Switches (all off by default): `QUARRY_EDGE__SCRAPE_CAPTURE` (only `on`), `QUARRY_EDGE__SCRAPE_ACTIONS` (`user` for user tokens in dev; `on` also for service principals with `scrape:interact`), `QUARRY_EDGE__SCRAPE_STITCH` (`on`), `QUARRY_EDGE__SCRAPE_CAPTURE_INLINE_MAX_BYTES` (ZDR inline cap, 6 MiB default, 16 MiB ceiling).

Limits as built (`quarry-core/src/capture.rs` and `scrape_actions.rs`; the environment can only lower them):

| Limit | Value |
|---|---|
| captures per request, formats and actions together | 8 |
| bytes per capture | 32 MiB |
| viewport | 3840 by 2160 CSS px; device scale factor 1 to 2 |
| single capture ceiling | 16000 device px (provisional, see the WS13.3 spike) |
| stitch tile | 4096 CSS px; 16 tiles; 60000 CSS px of content |
| stitched pixels | 40 Mpx PNG; 24 Mpx JPEG |
| PDF pages | 100 by default; 200 at most; scale 0.1 to 2.0 |
| actions per request | 50 |
| total wait | 60 s; one wait 30 s; selector wait default 5 s |
| selector | 512 bytes; text 4096 bytes per write, 16384 in all |
| scroll | 20000 px at most |
| handler bound with actions | 115 s; router 125 s with actions, 60 s otherwise |

Evidence owed (none of it run): the named cargo tests in each slice's progress file; the live Chromium tests (`screenshot_mask_live`, `stitch_live`, `scrape_actions_live`); the measured spike table above; `make test-scrape-capture` on a clean stack; the ADR-6 ruling before `on`.
