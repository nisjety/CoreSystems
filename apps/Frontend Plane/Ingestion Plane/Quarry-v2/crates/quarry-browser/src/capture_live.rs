// WS13.3: scrape-time captures against a real Chromium. This file is `include!`d at the
// end of `mod tests` in chromiumoxide.rs, so it shares that module's helpers
// (`live_driver`, `live_enabled`, `live_session_sized`).
//
// Run: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//      cargo test -p quarry-browser --features chromiumoxide capture_ -- --ignored --nocapture
//
// NOT RUN when this was written (build-first rule). The evidence run is the
// final test phase of wave 2, in the rebuilt edge image's Chromium.

/// A tall page with a marker at the top, so a full-page capture has real height.
fn capture_tall_fixture_html() -> String {
    concat!(
        "<!doctype html><html><head>",
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
        "</head><body style=\"margin:0\"><p id=\"top\">capture-top</p>",
        "<div style=\"height:3000px;background:linear-gradient(#f00,#00f)\"></div>",
        "</body></html>",
    )
    .to_owned()
}

/// A three-page fixture: each `div` breaks the page, so a PDF of it has three
/// page objects unless `maxPages` cuts it short.
fn capture_three_page_fixture_html() -> String {
    concat!(
        "<!doctype html><html><head><style>",
        "@page { size: letter; margin: 0 } .brk { break-after: page; height: 900px }",
        "</style></head><body>",
        "<div class=\"brk\">one</div><div class=\"brk\">two</div><div class=\"brk\">three</div>",
        "</body></html>",
    )
    .to_owned()
}

/// Page objects in a PDF: `/Type /Page` that is not `/Type /Pages`. A heuristic
/// over the uncompressed object dictionaries Chromium writes.
fn capture_pdf_page_objects(bytes: &[u8]) -> usize {
    let needle = b"/Type /Page";
    let mut count = 0;
    let mut index = 0;
    while index + needle.len() <= bytes.len() {
        if &bytes[index..index + needle.len()] == needle {
            let next = bytes.get(index + needle.len()).copied();
            if next != Some(b's') {
                count += 1;
            }
            index += needle.len();
        } else {
            index += 1;
        }
    }
    count
}

#[tokio::test]
#[ignore = "WS13.3 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn captures_full_page_clip_and_jpeg_on_a_fixture_page() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws13-3-capture-shape", None, "capture-shape").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(capture_tall_fixture_html())
        .await
        .expect("fixture");
    let limits = quarry_core::capture::CaptureLimits::default();

    let full = driver
        .capture_screenshot(
            &session,
            &quarry_core::capture::ScreenshotOptions {
                full_page: true,
                ..Default::default()
            },
            &limits,
        )
        .await
        .expect("full page");
    assert_eq!(full.format, quarry_core::capture::ImageFormat::Png);
    assert_eq!(&full.bytes[..4], &[0x89, 0x50, 0x4E, 0x47]);
    assert!(
        full.height_px >= 3000,
        "a full page is at least the fixture height: {}",
        full.height_px
    );
    assert!(!full.truncated);

    let jpeg = driver
        .capture_screenshot(
            &session,
            &quarry_core::capture::ScreenshotOptions {
                quality: Some(80),
                ..Default::default()
            },
            &limits,
        )
        .await
        .expect("jpeg");
    assert_eq!(jpeg.format, quarry_core::capture::ImageFormat::Jpeg);
    assert_eq!(&jpeg.bytes[..2], &[0xFF, 0xD8]);

    let clipped = driver
        .capture_screenshot(
            &session,
            &quarry_core::capture::ScreenshotOptions {
                clip: Some(quarry_core::capture::ClipRect {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 50.0,
                }),
                ..Default::default()
            },
            &limits,
        )
        .await
        .expect("clip");
    assert_eq!((clipped.width_px, clipped.height_px), (100, 50));
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS13.3 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn pdf_starts_with_percent_pdf_and_honours_max_pages() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws13-3-pdf-shape", None, "pdf-shape").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(capture_three_page_fixture_html())
        .await
        .expect("fixture");
    let limits = quarry_core::capture::CaptureLimits::default();

    let one = driver
        .capture_pdf(
            &session,
            &quarry_core::capture::PdfOptions {
                max_pages: 1,
                ..Default::default()
            },
            &limits,
        )
        .await
        .expect("one-page pdf");
    assert!(one.bytes.starts_with(b"%PDF-"));
    assert_eq!(capture_pdf_page_objects(&one.bytes), 1);

    let all = driver
        .capture_pdf(
            &session,
            &quarry_core::capture::PdfOptions::default(),
            &limits,
        )
        .await
        .expect("default pdf");
    assert!(all.bytes.starts_with(b"%PDF-"));
    assert!(capture_pdf_page_objects(&all.bytes) >= 3);
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS13.3 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn viewport_override_is_restored_after_capture_even_on_error() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let lease_viewport = BrowserViewport {
        width: 800,
        height: 600,
        device_scale_factor: 1.0,
        is_mobile: false,
    };
    let session = live_session_sized(
        &driver,
        "ws13-3-viewport-restore",
        Some(lease_viewport),
        "viewport-restore",
    )
    .await;
    let page = driver.current_page(&session).await.expect("current page");
    let limits = quarry_core::capture::CaptureLimits::default();
    let inner_width = || async {
        page.evaluate(String::from("window.innerWidth"))
            .await
            .expect("read innerWidth")
            .into_value::<u32>()
            .expect("a number")
    };
    assert_eq!(inner_width().await, 800);

    // The capture viewport is applied first; a clip far outside the content is then
    // refused. The lease's viewport must still come back.
    let failing = quarry_core::capture::ScreenshotOptions {
        clip: Some(quarry_core::capture::ClipRect {
            x: 1_000_000.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        }),
        viewport: Some(quarry_core::capture::CaptureViewport {
            width: 1024,
            height: 768,
            device_scale_factor: 1.0,
        }),
        ..Default::default()
    };
    let error = driver
        .capture_screenshot(&session, &failing, &limits)
        .await
        .expect_err("a clip outside the content is refused");
    let reason = error
        .details
        .as_ref()
        .and_then(|details| details.get("reason"))
        .and_then(|reason| reason.as_str());
    assert_eq!(reason, Some("invalid_clip"));
    assert_eq!(inner_width().await, 800, "the lease viewport is restored");
    driver.release(session).await.expect("release");
}

#[tokio::test]
#[ignore = "WS13.3 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn capture_does_not_navigate_or_mutate_the_page() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let marker = "no-mutation-marker";
    let session = live_session_sized(&driver, "ws13-3-no-mutation", None, marker).await;
    let url_before = driver.current_url(&session).await.expect("url before");
    let limits = quarry_core::capture::CaptureLimits::default();
    driver
        .capture_screenshot(
            &session,
            &quarry_core::capture::ScreenshotOptions {
                full_page: true,
                ..Default::default()
            },
            &limits,
        )
        .await
        .expect("capture");
    let url_after = driver.current_url(&session).await.expect("url after");
    assert_eq!(url_before, url_after);
    let content = driver.content(&session).await.expect("content");
    assert!(String::from_utf8_lossy(&content).contains(marker));
    driver.release(session).await.expect("release");
}
