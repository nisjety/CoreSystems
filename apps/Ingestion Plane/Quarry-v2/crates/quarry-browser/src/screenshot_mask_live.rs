// WS12.S5: screenshot masking against a real Chromium. This file is `include!`d at
// the end of `mod tests` in chromiumoxide.rs, so it shares that module's helpers
// (`live_driver`, `live_enabled`, `live_session_sized`).
//
// Run: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//      cargo test -p quarry-browser --features chromiumoxide screenshot_mask_live -- --test-threads=1 --ignored --nocapture
//
// NOT RUN when this was written (build-first rule). The plan's p95 measurement for
// `mask_ms` (1280x800, 150 ms) is not taken here either.

/// The password value, joined from fragments so no secret-shaped literal is in the source.
const MASK_LIVE_CANARY: &str = concat!("M4sk", "P@ss-0451");

/// A password field with a large value so a leak is readable, a plain text field,
/// and a hidden (`display:none`) password that must be ignored.
fn mask_live_page(extra_body: &str) -> String {
    format!(
        concat!(
            "<!doctype html><html><head><meta name=\"viewport\" content=\"width=device-width\">",
            "</head><body style=\"margin:0;background:#ffffff\">",
            "<div style=\"padding:20px\">",
            "<label for=\"pw\">Password</label><br>",
            "<input id=\"pw\" type=\"password\" value=\"{canary}\" ",
            "style=\"font-size:40px;width:420px;margin-bottom:30px\"><br>",
            "<label for=\"plain\">City</label><br>",
            "<input id=\"plain\" value=\"visible-text\" style=\"font-size:40px;width:420px\">",
            "<input type=\"password\" value=\"hidden-value\" style=\"display:none\">",
            "</div>{extra}</body></html>",
        ),
        canary = MASK_LIVE_CANARY,
        extra = extra_body,
    )
}

/// The pixel (x, y) of a PNG as RGB.
fn mask_live_pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 3] {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("png header");
    let mut buffer = vec![0u8; reader.output_buffer_size().expect("size")];
    let frame = reader.next_frame(&mut buffer).expect("frame");
    let channels = match frame.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => panic!("unexpected colour type {other:?}"),
    };
    let index = ((y * frame.width + x) as usize) * channels;
    [buffer[index], buffer[index + 1], buffer[index + 2]]
}

/// Every sampled point inside `rect` (corners inset by 2 px, the centre) is the mask colour.
fn assert_rect_is_masked(bytes: &[u8], rect: crate::screenshot_mask::PixelRect) {
    let mask = crate::screenshot_mask::MASK_RGB;
    let points = [
        (rect.x + 2, rect.y + 2),
        (rect.x + rect.w - 3, rect.y + 2),
        (rect.x + 2, rect.y + rect.h - 3),
        (rect.x + rect.w - 3, rect.y + rect.h - 3),
        (rect.x + rect.w / 2, rect.y + rect.h / 2),
    ];
    for (x, y) in points {
        assert_eq!(
            mask_live_pixel(bytes, x, y),
            mask,
            "pixel ({x},{y}) of {rect:?}"
        );
    }
}

#[tokio::test]
#[ignore = "WS12.S5 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_blocks_the_password_and_leaves_the_rest() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws12s5-basic", None, "mask-basic").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(mask_live_page(""))
        .await
        .expect("inject page");
    let masked = driver
        .screenshot_masked(&session, false)
        .await
        .expect("masked screenshot");
    let bytes = masked.bytes.as_ref().expect("an image, not withheld");
    assert_eq!(
        masked.state,
        quarry_core::redaction::ScreenshotState::Masked,
        "{masked:?}"
    );
    assert_eq!(masked.rects.len(), 1, "one sensitive field: {masked:?}");
    assert_rect_is_masked(bytes, masked.rects[0]);
    // A control region far from the password field keeps the page background.
    let (px, py) = (masked.rects[0].x + masked.rects[0].w + 40, 5);
    assert_eq!(mask_live_pixel(bytes, px, py), [255, 255, 255]);
    // The plain field is not painted over: its region is not the mask colour.
    let below = masked.rects[0].y + masked.rects[0].h + 60;
    assert_ne!(
        mask_live_pixel(bytes, masked.rects[0].x + 10, below),
        crate::screenshot_mask::MASK_RGB
    );
    // The driver's plain `screenshot` is the same masked image.
    let again = driver
        .screenshot(&session, false)
        .await
        .expect("screenshot");
    assert_rect_is_masked(&again, masked.rects[0]);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS12.S5 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_scales_with_the_device_pixel_ratio() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let viewport = BrowserViewport {
        width: 800,
        height: 600,
        device_scale_factor: 2.0,
        is_mobile: false,
    };
    let session = live_session_sized(&driver, "ws12s5-dpr", Some(viewport), "mask-dpr").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(mask_live_page(""))
        .await
        .expect("inject page");
    let masked = driver
        .screenshot_masked(&session, false)
        .await
        .expect("masked screenshot");
    let bytes = masked.bytes.as_ref().expect("an image");
    assert_eq!(masked.rects.len(), 1, "{masked:?}");
    // At scale 2 the 420 px wide field is at least 840 device px wide.
    assert!(masked.rects[0].w >= 840, "{:?}", masked.rects[0]);
    assert_rect_is_masked(bytes, masked.rects[0]);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS12.S5 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_finds_a_field_after_a_scroll() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws12s5-scroll", None, "mask-scroll").await;
    let page = driver.current_page(&session).await.expect("current page");
    let html = format!(
        concat!(
            "<!doctype html><html><body style=\"margin:0\">",
            "<div style=\"height:2000px;background:#eef\">spacer</div>",
            "<input id=\"spw\" type=\"password\" value=\"{canary}\" ",
            "style=\"font-size:40px;width:420px\">",
            "<div style=\"height:900px\"></div></body></html>",
        ),
        canary = MASK_LIVE_CANARY
    );
    page.set_content(html).await.expect("inject page");
    page.evaluate("window.scrollTo(0, 1900)")
        .await
        .expect("scroll");
    let masked = driver
        .screenshot_masked(&session, false)
        .await
        .expect("masked screenshot");
    let bytes = masked.bytes.as_ref().expect("an image");
    assert_eq!(masked.rects.len(), 1, "{masked:?}");
    // The field is about 100 px into a viewport scrolled to 1900, not 2000 px down.
    assert!(masked.rects[0].y < 400, "{:?}", masked.rects[0]);
    assert_rect_is_masked(bytes, masked.rects[0]);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS12.S5 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_finds_a_field_inside_a_same_origin_iframe() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws12s5-iframe", None, "mask-iframe").await;
    let page = driver.current_page(&session).await.expect("current page");
    // A srcdoc frame is same-origin with its parent.
    let inner = format!(
        "<body style='margin:8px'><input type='password' value='{MASK_LIVE_CANARY}' \
         style='font-size:32px;width:240px'></body>"
    );
    let html = format!(
        "<!doctype html><html><body style=\"margin:0\">\
         <div style=\"height:120px\">top</div>\
         <iframe srcdoc=\"{}\" width=\"320\" height=\"90\" style=\"border:2px solid #888\"></iframe>\
         </body></html>",
        inner.replace('"', "&quot;")
    );
    page.set_content(html).await.expect("inject page");
    let masked = driver
        .screenshot_masked(&session, false)
        .await
        .expect("masked screenshot");
    let bytes = masked.bytes.as_ref().expect("an image");
    assert_eq!(masked.rects.len(), 1, "{masked:?}");
    // The frame starts 120 px down: the field is below that line, not at the top.
    assert!(masked.rects[0].y >= 120, "{:?}", masked.rects[0]);
    assert_rect_is_masked(bytes, masked.rects[0]);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS12.S5 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_withholds_when_a_visible_field_cannot_be_placed() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws12s5-withheld", None, "mask-withheld").await;
    let page = driver.current_page(&session).await.expect("current page");
    // Rendered but zero-size: the scan cannot place it, so the image is withheld.
    let html = format!(
        "<!doctype html><html><body><p>text</p>\
         <input type=\"password\" value=\"{MASK_LIVE_CANARY}\" style=\"width:0;height:0;border:0;padding:0\">\
         </body></html>"
    );
    page.set_content(html).await.expect("inject page");
    let masked = driver
        .screenshot_masked(&session, false)
        .await
        .expect("a withheld screenshot is not an error");
    assert!(masked.bytes.is_none(), "{masked:?}");
    assert_eq!(
        masked.state,
        quarry_core::redaction::ScreenshotState::Withheld
    );
    assert_eq!(
        masked.note,
        Some("screenshot_withheld:sensitive_field_unresolved")
    );
    // The explicit screenshot call is the typed 403.
    let error = driver
        .screenshot(&session, false)
        .await
        .expect_err("withheld");
    assert_eq!(error.code, ErrorCode::SecurityBlocked);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS12.S5 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_ignores_a_hidden_password_and_refuses_a_pdf_with_a_value() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws12s5-hidden", None, "mask-hidden").await;
    let page = driver.current_page(&session).await.expect("current page");
    // Only a `display:none` password: nothing to mask, the image is untouched.
    page.set_content(
        "<!doctype html><html><body><p>plain page</p>\
         <input type=\"password\" value=\"hidden-value\" style=\"display:none\"></body></html>",
    )
    .await
    .expect("inject page");
    let masked = driver
        .screenshot_masked(&session, false)
        .await
        .expect("masked screenshot");
    assert!(masked.bytes.is_some());
    assert_eq!(
        masked.state,
        quarry_core::redaction::ScreenshotState::NotApplicable
    );
    assert!(masked.rects.is_empty());
    driver
        .pdf(&session)
        .await
        .expect("a pdf of a page with no sensitive value");
    // A visible password with a value: the PDF is refused.
    page.set_content(mask_live_page(""))
        .await
        .expect("inject page");
    let error = driver.pdf(&session).await.expect_err("pdf refused");
    assert_eq!(error.code, ErrorCode::SecurityBlocked);
    driver.release(session).await.expect("release");
}

// W2-U52 (D2-066, D2-067, D2-114): the isolated scan world and the cross-origin
// iframe modes. Written against the unit-tested seams; NOT RUN when this was
// written (no Chromium in the build lane). They pass a `MaskConfig` straight to
// `capture_masked`, so no environment variable is touched.

/// The page's own script makes every `querySelectorAll` answer empty, which blinds a
/// scan that runs in the page's world.
const MASK_LIVE_HOSTILE_SCRIPT: &str = "<script>\
for (const P of [Document.prototype, Element.prototype, DocumentFragment.prototype]) {\
  P.querySelectorAll = function () { return []; };\
}\
</script>";

async fn mask_live_capture(
    page: &::chromiumoxide::Page,
    config: &crate::screenshot_mask::MaskConfig,
) -> crate::screenshot_mask::Captured<()> {
    crate::screenshot_mask::capture_masked(
        page,
        config,
        crate::screenshot_mask::Origin::Legacy,
        || {
            let page = page.clone();
            async move { capture_png(&page, false).await.map(|png| (png, ())) }
        },
    )
    .await
    .expect("capture")
}

#[tokio::test]
#[ignore = "W2-U52 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_isolated_world_masks_a_field_a_hostile_page_hides() {
    if !live_enabled() {
        return;
    }
    use crate::screenshot_mask::{Captured, MaskConfig};
    use crate::sensitive_rects::{scan_page, ScanWorld};
    let driver = live_driver();
    let session = live_session_sized(&driver, "w2u52-world", None, "mask-world").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(mask_live_page(MASK_LIVE_HOSTILE_SCRIPT))
        .await
        .expect("inject page");
    // The page's own world is blinded: the scan finds no sensitive field.
    let main_scan = scan_page(&page, ScanWorld::Main).await.expect("a scan");
    assert!(
        main_scan.rects.is_empty(),
        "the main world was fooled: {main_scan:?}"
    );
    // The isolated world still sees it.
    let isolated_scan = scan_page(&page, ScanWorld::Isolated)
        .await
        .expect("a scan");
    assert_eq!(isolated_scan.rects.len(), 1, "{isolated_scan:?}");
    let config = MaskConfig::from_values(None, None, Some("isolated"), true);
    match mask_live_capture(&page, &config).await {
        Captured::Image { bytes, rects, state, .. } => {
            assert_eq!(state, quarry_core::redaction::ScreenshotState::Masked);
            assert_eq!(rects.len(), 1);
            assert_rect_is_masked(&bytes, rects[0]);
        }
        other => panic!("expected a masked image, got {other:?}"),
    }
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "W2-U52 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_withhold_mode_refuses_a_cross_origin_iframe() {
    if !live_enabled() {
        return;
    }
    use crate::screenshot_mask::{Captured, MaskConfig, Withheld};
    let driver = live_driver();
    let session = live_session_sized(&driver, "w2u52-oopif", None, "mask-oopif").await;
    let page = driver.current_page(&session).await.expect("current page");
    // A data: document is its own opaque origin: its contentDocument is not readable.
    let html = "<!doctype html><html><body style=\"margin:0\"><p>top</p>\
        <iframe width=\"320\" height=\"120\" \
         src=\"data:text/html,%3Cinput%20type%3Dpassword%20value%3Dsecret%3E\"></iframe>\
        </body></html>";
    page.set_content(html).await.expect("inject page");
    // Default (allow): the image comes back, the frame is counted.
    match mask_live_capture(&page, &MaskConfig::default()).await {
        Captured::Image { oopif_frames, .. } => assert_eq!(oopif_frames, 1),
        other => panic!("expected an image, got {other:?}"),
    }
    // mask: the frame rectangle is painted.
    let mask = MaskConfig::from_values(None, Some("mask"), None, true);
    match mask_live_capture(&page, &mask).await {
        Captured::Image { rects, state, .. } => {
            assert_eq!(state, quarry_core::redaction::ScreenshotState::Masked);
            assert_eq!(rects.len(), 1);
        }
        other => panic!("expected a masked image, got {other:?}"),
    }
    // withhold: no image, with the closed note.
    let withhold = MaskConfig::from_values(None, Some("withhold"), Some("isolated"), true);
    match mask_live_capture(&page, &withhold).await {
        Captured::Withheld { reason, oopif_frames, .. } => {
            assert_eq!(reason, Withheld::OopifUnscannable);
            assert_eq!(oopif_frames, 1);
        }
        other => panic!("expected a withheld image, got {other:?}"),
    }
    // The PDF is refused too, with its own closed reason.
    assert_eq!(
        crate::screenshot_mask::pdf_allowed(&page, &withhold).await,
        Err(crate::screenshot_mask::PDF_WITHHELD_OOPIF)
    );
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "W2-U52 live lane (D2-114 evidence): needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn screenshot_mask_live_mask_ms_p95_in_each_world() {
    if !live_enabled() {
        return;
    }
    use crate::screenshot_mask::{Captured, MaskConfig};
    let driver = live_driver();
    let viewport = BrowserViewport {
        width: 1280,
        height: 800,
        device_scale_factor: 1.0,
        is_mobile: false,
    };
    let session = live_session_sized(&driver, "w2u52-ms", Some(viewport), "mask-ms").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(mask_live_page("")).await.expect("inject page");
    for (label, world) in [("main", None), ("isolated", Some("isolated"))] {
        let config = MaskConfig::from_values(None, None, world, true);
        let mut samples = Vec::new();
        for _ in 0..30 {
            match mask_live_capture(&page, &config).await {
                Captured::Image { mask_ms, .. } => samples.push(mask_ms),
                other => panic!("expected an image, got {other:?}"),
            }
        }
        samples.sort_unstable();
        let p95 = samples[(samples.len() * 95).div_ceil(100) - 1];
        println!("mask_ms {label}: p50={} p95={p95}", samples[samples.len() / 2]);
        assert!(p95 <= 150, "{label} p95 {p95} ms is over the 150 ms budget");
    }
    driver.release(session).await.expect("release");
}
