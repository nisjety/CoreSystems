# W2-U52 status: mask scan in an isolated world, cross-origin iframe withhold (D2-066, D2-067)

Branch w23/lane-a. Date 2026-11-02.

## Built
Existing before this unit: the scan script, rectangle planning, painting, the `QUARRY_BROWSER_SCREENSHOT_MASK` flag (default on) and the `allow|mask` iframe flag. Built here (the gap):
- **Isolated scan world (D2-066).** `sensitive_rects::ScanWorld { Main, Isolated }`. In `Isolated` the scan script and the post-capture scroll check run in a CDP isolated world made with `Page.createIsolatedWorld` (name `__quarry_mask_scan__`, `grantUniveralAccess` false), one fresh world per call because a navigation destroys the old context. Same DOM, separate JavaScript wrappers, so a page that overrides `querySelectorAll`, `getBoundingClientRect` or `value` cannot hide a field. A world that cannot be created is a failed scan: the image is withheld (`screenshot_withheld:sensitive_field_unresolved`), never a fallback to the main world. The scroll check uses the same world as the scan (a page could otherwise fake `window.scrollY`).
- **Seam for tests.** `sensitive_rects::ScanTransport` (one async method) is implemented for the Chromium `Page`; `scan_page`, `scroll_now`, `capture_masked` and `pdf_allowed` are generic over it and no longer behind the `chromiumoxide` feature, so the full capture flow runs against a mock page in plain `cargo test`.
- **Cross-origin iframes (D2-067).** New `OopifMode::Withhold` (`QUARRY_BROWSER_SCREENSHOT_OOPIF=withhold`): an image with a cross-origin iframe that has an area and lies inside the image is withheld, note `screenshot_withheld:oopif_unscannable` (new `Withheld::OopifUnscannable`). A zero-area frame, and a frame wholly outside the image, do not count. A PDF is refused under `withhold` when such a frame is on the page: `pdf_withheld:oopif_unscannable`. Visible sensitive fields are still masked; an unresolved field still wins with its own note.
- `MaskConfig` has a `world` field; `from_values` takes the world as its third argument (only caller was `from_env` and the unit tests).
- Live tests (`#[ignore]`, not run) in `screenshot_mask_live.rs` (the file `include!`d by `chromiumoxide.rs`): hostile-page isolated-world masking, cross-origin iframe allow/mask/withhold plus the PDF, and a `mask_ms` p95 measurement per world (budget 150 ms, 1280x800) for D2-114.
- `chromiumoxide.rs` itself needed no change: its calls pass a `&Page` and `MaskConfig::from_env()`, both still fit.

True scanning of an out-of-process frame was not built: chromiumoxide 0.9 has no session-scoped command API for child targets, and it cannot be verified without Chromium. The queue allows "or withhold when unscannable", so that is what `withhold` does (see D-U52-1).

## Commits
See `git log w23/lane-a` (code and tests with live tests, this status and the depth rows).

## Tests
Rust through the wrapper, lane a:
- `cargo test -p quarry-browser --lib -- sensitive_rects screenshot_mask`: 37 passed, 0 failed.
- `cargo test -p quarry-browser --lib`: 222 passed, 0 failed.
- `cargo test -p quarry-browser --features chromiumoxide --lib`: 329 passed, 0 failed, 40 ignored (3 of the ignored are the new live tests).
- `rustfmt --check` on the two source files: clean.
New unit tests (mock transport): the isolated world sees what a hostile main world hides; an isolated failure never touches the main world; the scroll check runs in the scan's world; a scan timeout is a failed scan; flag parsing and defaults; withhold mode (frame in image, zero-area frame, frame outside the image, unresolved wins, fields still masked); capture end to end for isolated, failed world, withhold, allow and mask unchanged, masking off scans nothing; PDF follows world and withhold.

## Flag and default
- `QUARRY_BROWSER_SCREENSHOT_SCAN_WORLD=main|isolated`, default `main` (as built).
- `QUARRY_BROWSER_SCREENSHOT_OOPIF=allow|mask|withhold`, default `allow` (as built).
- `QUARRY_BROWSER_SCREENSHOT_MASK` unchanged (default on).
Read on every capture, no restart. Not added to compose or the docs (outside this unit's files): add the two lines to RUNBOOKS (screenshot masking) and SECRET_REDACTION when the flags are flipped.

## Assumed decisions
- Isolated world and `withhold` ship dark (default `main` / `allow`), per the queue ("flag stays off by default"), although D2-066/067 are leak risks: the live Chromium behaviour is unproven. Flip `SCAN_WORLD=isolated` first once the live tests pass.
- Fail closed in `isolated`: no fallback to the main world.
- `withhold` ignores zero-area frames (they show nothing) and frames outside the image, so tracking pixels do not block every screenshot. (`mask` mode keeps its older behaviour of withholding on a frame with no usable rectangle.)
- OOPIF scanning replaced by withhold (no child-session scanning).

## Model Plane asks
None.

## Not verified
- Nothing ran against Chromium: `createIsolatedWorld` plus `Runtime.evaluate` with a context id, the same-origin iframe walk from an isolated world (`contentWindow` wrappers) and the hostile-page test are untested live.
- D2-114: `mask_ms` p95 and the five live cases remain unmeasured (the ignored live tests exist).
- The runtime, OpenAPI and docs vocabulary were not updated for `oopif_unscannable` (D-U52-3).
