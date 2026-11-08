// WS13.6: tile-and-stitch against a real Chromium. This file is `include!`d at the
// end of `mod tests` in chromiumoxide.rs, so it shares that module's helpers
// (`live_driver`, `live_enabled`, `live_session_sized`).
//
// Run: CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium \
//      cargo test -p quarry-browser --features chromiumoxide stitch_live -- --test-threads=1 --ignored --nocapture
//
// NOT RUN when this was written (build-first rule). The sticky-header and lazy-image
// fixtures named by the plan are not written here: they need the method spike in
// docs/SCRAPE_CAPTURE.md first.

/// Five bands of 5,000 CSS px, each a different colour: 25,000 px in all, which one
/// capture (16,000 device px at scale 1) cannot hold.
fn stitch_banded_fixture_html() -> String {
    concat!(
        "<!doctype html><html><head>",
        "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">",
        "</head><body style=\"margin:0\">",
        "<div style=\"height:5000px;background:#ff0000\"></div>",
        "<div style=\"height:5000px;background:#00ff00\"></div>",
        "<div style=\"height:5000px;background:#0000ff\"></div>",
        "<div style=\"height:5000px;background:#ffff00\"></div>",
        "<div style=\"height:5000px;background:#ff00ff\"></div>",
        "</body></html>",
    )
    .to_owned()
}

/// The pixel at (x, y) of a PNG as RGB, decoded here.
fn stitch_png_pixel(bytes: &[u8], x: u32, y: u32) -> (u32, u32, [u8; 3]) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("png header");
    let mut buffer = vec![0u8; reader.output_buffer_size().expect("size")];
    let frame = reader.next_frame(&mut buffer).expect("frame");
    let (width, height) = (frame.width, frame.height);
    let channels = match frame.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        other => panic!("unexpected colour type {other:?}"),
    };
    let index = ((y * width + x) as usize) * channels;
    (
        width,
        height,
        [buffer[index], buffer[index + 1], buffer[index + 2]],
    )
}

#[tokio::test]
#[ignore = "WS13.6 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn banded_25000px_page_has_correct_band_order_in_the_stitched_png() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws13-6-stitch-bands", None, "stitch-bands").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(stitch_banded_fixture_html())
        .await
        .expect("fixture");
    let limits = quarry_core::capture::CaptureLimits::default();
    let options = quarry_core::capture::ScreenshotOptions {
        full_page: true,
        ..Default::default()
    };
    assert!(
        driver
            .screenshot_needs_stitch(&session, &options)
            .await
            .expect("needs stitch"),
        "a 25,000 px page needs stitching at scale 1"
    );
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(1));
    let permit = crate::StitchPermit::new(semaphore.try_acquire_owned().expect("permit"));
    let image = driver
        .capture_screenshot_stitched(&session, &options, &limits, Some(&permit))
        .await
        .expect("stitched");
    assert!(image.stitched);
    assert!(image.tiles >= 2, "tiles: {}", image.tiles);
    assert!(!image.truncated);
    assert_eq!(image.height_px, 25_000);
    // The middle of each band, in order: red, green, blue, yellow, magenta.
    let expected = [
        [0xff, 0x00, 0x00],
        [0x00, 0xff, 0x00],
        [0x00, 0x00, 0xff],
        [0xff, 0xff, 0x00],
        [0xff, 0x00, 0xff],
    ];
    for (band, colour) in expected.iter().enumerate() {
        let y = band as u32 * 5_000 + 2_500;
        let (_, _, pixel) = stitch_png_pixel(&image.bytes, 10, y);
        for channel in 0..3 {
            assert!(
                (i32::from(pixel[channel]) - i32::from(colour[channel])).abs() <= 8,
                "band {band} at y={y}: {pixel:?} vs {colour:?}"
            );
        }
    }
}

#[tokio::test]
#[ignore = "WS13.6 live lane: needs Chromium (CHROMIUMOXIDE_TEST=1)"]
async fn permit_busy_falls_back_to_crop_with_warning() {
    if !live_enabled() {
        return;
    }
    let driver = live_driver();
    let session = live_session_sized(&driver, "ws13-6-stitch-busy", None, "stitch-busy").await;
    let page = driver.current_page(&session).await.expect("current page");
    page.set_content(stitch_banded_fixture_html())
        .await
        .expect("fixture");
    let limits = quarry_core::capture::CaptureLimits::default();
    let options = quarry_core::capture::ScreenshotOptions {
        full_page: true,
        ..Default::default()
    };
    // No permit (the heavy lane is busy): the image is cropped at the single-capture
    // limit and says so, rather than waiting for a permit.
    let image = driver
        .capture_screenshot_stitched(&session, &options, &limits, None)
        .await
        .expect("cropped");
    assert!(!image.stitched);
    assert!(image.truncated);
    assert_eq!(
        image.height_px,
        quarry_core::capture::SINGLE_CAPTURE_MAX_DEVICE_PX
    );
}
