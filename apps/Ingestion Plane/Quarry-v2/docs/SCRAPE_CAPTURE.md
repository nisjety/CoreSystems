# Scrape-time capture: single-capture limits (WS13.3)

Status: the driver API is built (W2-U40, `quarry-browser`), NOT run. The spike table below
is not measured yet. No request reaches these methods until WS13.4b wires `formats` and
`captures` into `POST /v1/scrape`, so nothing here changes a response today.

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
