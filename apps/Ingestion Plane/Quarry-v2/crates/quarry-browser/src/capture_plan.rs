//! WS13.3: pure planning for scrape-time captures. No CDP call happens here.
//!
//! The chromiumoxide driver asks these functions what to send to Chromium
//! (the clip, whether the capture goes beyond the viewport, the PDF page range)
//! and reads back what came (the image size from its header). The module is not
//! behind the `chromiumoxide` feature, so the planning tests run in the default
//! build.

use quarry_core::capture::{
    CaptureLimits, ClipRect, ScreenshotOptions, SINGLE_CAPTURE_MAX_DEVICE_PX,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use serde_json::json;

/// What one screenshot asks Chromium for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShotPlan {
    /// The clip in CSS pixels of the page. `None` is the viewport.
    pub clip: Option<ClipRect>,
    /// Whether the capture reaches outside the viewport (a clip or a full page).
    pub capture_beyond_viewport: bool,
    /// The page was taller than one capture may be; the image stops at the limit.
    pub truncated: bool,
}

/// The CSS-pixel height one capture may cover at this device scale factor.
/// A factor that is not a positive number counts as 1.
pub fn max_css_height(device_scale_factor: f64) -> f64 {
    let scale = if device_scale_factor.is_finite() && device_scale_factor > 0.0 {
        device_scale_factor
    } else {
        1.0
    };
    f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX) / scale
}

/// Plan one screenshot of a page whose content is `content_width` by
/// `content_height` CSS pixels.
///
/// - A clip is intersected with the content. A clip that lies outside it is an
///   error, not an empty image.
/// - A full-page capture covers the whole content, cropped at the single-capture
///   limit (`truncated` when it is cropped).
/// - Otherwise the viewport is captured as it is.
pub fn plan_screenshot(
    content_width: f64,
    content_height: f64,
    options: &ScreenshotOptions,
    device_scale_factor: f64,
) -> QuarryResult<ShotPlan> {
    let cap = max_css_height(device_scale_factor);
    if let Some(clip) = options.clip {
        let within = clip
            .intersect_content(content_width, content_height)
            .ok_or_else(invalid_clip)?;
        let truncated = within.height > cap;
        return Ok(ShotPlan {
            clip: Some(ClipRect {
                height: within.height.min(cap),
                ..within
            }),
            capture_beyond_viewport: true,
            truncated,
        });
    }
    if options.full_page {
        if !(content_width > 0.0 && content_height > 0.0) {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "page has no content area to capture",
            )
            .with_details(json!({ "reason": "no_content_area" })));
        }
        return Ok(ShotPlan {
            clip: Some(ClipRect {
                x: 0.0,
                y: 0.0,
                width: content_width,
                height: content_height.min(cap),
            }),
            capture_beyond_viewport: true,
            truncated: content_height > cap,
        });
    }
    Ok(ShotPlan {
        clip: None,
        capture_beyond_viewport: false,
        truncated: false,
    })
}

fn invalid_clip() -> QuarryError {
    QuarryError::new(
        ErrorCode::BadRequest,
        "the clip lies outside the page content",
    )
    .with_details(json!({ "reason": "invalid_clip" }))
}

/// Refuse a capture whose encoded bytes exceed the cap for the request.
pub fn check_capture_size(bytes: usize, limits: &CaptureLimits) -> QuarryResult<()> {
    if bytes > limits.max_capture_bytes {
        return Err(
            QuarryError::new(ErrorCode::DriverFailed, "capture exceeds the size cap")
                .with_details(json!({ "reason": "capture_too_large" })),
        );
    }
    Ok(())
}

/// The `pageRanges` string for a PDF of at most `max_pages` pages.
pub fn pdf_page_ranges(max_pages: u32) -> String {
    format!("1-{max_pages}")
}

/// The pixel size of a PNG or JPEG, read from its header. `None` for anything
/// else, and for a header cut short.
pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    png_dimensions(bytes).or_else(|| jpeg_dimensions(bytes))
}

/// PNG: the IHDR chunk is always first, width and height are big-endian at 16..24.
fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((width, height))
}

/// JPEG: walk the marker segments to the first start-of-frame (SOF0..SOF15,
/// except DHT, JPG and DAC), where height and width are big-endian.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut index = 2;
    while index + 4 <= bytes.len() {
        if bytes[index] != 0xFF {
            return None;
        }
        let marker = bytes[index + 1];
        if marker == 0xFF {
            // A fill byte before a marker.
            index += 1;
            continue;
        }
        // Markers with no length: standalone, restart and SOI.
        if marker == 0x01 || marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            index += 2;
            continue;
        }
        // End of image or start of scan before any frame header: no dimensions.
        if marker == 0xD9 || marker == 0xDA {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]));
        if length < 2 {
            return None;
        }
        let is_frame = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_frame {
            if index + 9 > bytes.len() {
                return None;
            }
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]);
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]);
            return Some((u32::from(width), u32::from(height)));
        }
        index += 2 + length;
    }
    None
}

#[cfg(test)]
mod plan_screenshot {
    use super::*;
    use quarry_core::capture::{ImageFormat, PdfOptions, PdfPaper};

    fn clip(x: f64, y: f64, width: f64, height: f64) -> ClipRect {
        ClipRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn full_page_under_limit_is_one_uncropped_clip() {
        let options = ScreenshotOptions {
            full_page: true,
            ..ScreenshotOptions::default()
        };
        let plan = plan_screenshot(1280.0, 5000.0, &options, 1.0).expect("plan");
        assert_eq!(plan.clip, Some(clip(0.0, 0.0, 1280.0, 5000.0)));
        assert!(plan.capture_beyond_viewport);
        assert!(!plan.truncated);
    }

    #[test]
    fn full_page_over_limit_is_cropped_and_truncated() {
        let options = ScreenshotOptions {
            full_page: true,
            ..ScreenshotOptions::default()
        };
        let plan = plan_screenshot(1280.0, 20_000.0, &options, 1.0).expect("plan");
        let cropped = plan.clip.expect("a clip");
        assert_eq!(cropped.height, f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX));
        assert_eq!(cropped.width, 1280.0);
        assert!(plan.truncated);
    }

    #[test]
    fn clip_is_intersected_with_content_and_empty_is_an_error() {
        let inside = ScreenshotOptions {
            clip: Some(clip(900.0, 900.0, 500.0, 500.0)),
            ..ScreenshotOptions::default()
        };
        let plan = plan_screenshot(1000.0, 1000.0, &inside, 1.0).expect("plan");
        assert_eq!(plan.clip, Some(clip(900.0, 900.0, 100.0, 100.0)));
        assert!(plan.capture_beyond_viewport);

        let outside = ScreenshotOptions {
            clip: Some(clip(2000.0, 0.0, 10.0, 10.0)),
            ..ScreenshotOptions::default()
        };
        let error = plan_screenshot(1000.0, 1000.0, &outside, 1.0).expect_err("outside clip");
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str()),
            Some("invalid_clip")
        );
    }

    #[test]
    fn device_scale_factor_two_halves_the_css_height_limit() {
        assert_eq!(max_css_height(1.0), f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX));
        assert_eq!(
            max_css_height(2.0),
            f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX) / 2.0
        );
        // A nonsense factor counts as 1 rather than dividing by zero.
        assert_eq!(max_css_height(0.0), f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX));
        let options = ScreenshotOptions {
            full_page: true,
            ..ScreenshotOptions::default()
        };
        let plan = plan_screenshot(1280.0, 9000.0, &options, 2.0).expect("plan");
        assert!(plan.truncated);
        assert_eq!(
            plan.clip.expect("clip").height,
            f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX) / 2.0
        );
    }

    #[test]
    fn quality_sets_jpeg_format() {
        let jpeg = ScreenshotOptions {
            quality: Some(80),
            ..ScreenshotOptions::default()
        };
        assert_eq!(jpeg.format(), ImageFormat::Jpeg);
        assert_eq!(ScreenshotOptions::default().format(), ImageFormat::Png);
    }

    #[test]
    fn viewport_capture_has_no_clip_and_stays_in_the_viewport() {
        let plan =
            plan_screenshot(1280.0, 5000.0, &ScreenshotOptions::default(), 1.0).expect("plan");
        assert_eq!(
            plan,
            ShotPlan {
                clip: None,
                capture_beyond_viewport: false,
                truncated: false,
            }
        );
    }

    #[test]
    fn pdf_paper_table_matches_chromium_inches() {
        assert_eq!(PdfPaper::Letter.inches(), (8.5, 11.0));
        assert_eq!(PdfPaper::A4.inches(), (8.27, 11.69));
        assert_eq!(PdfOptions::default().format, PdfPaper::Letter);
    }

    #[test]
    fn pdf_page_range_string_is_1_to_n() {
        assert_eq!(pdf_page_ranges(1), "1-1");
        assert_eq!(pdf_page_ranges(100), "1-100");
    }

    #[test]
    fn capture_size_cap_is_enforced() {
        let limits = CaptureLimits::lowered(None, Some(1024));
        assert!(check_capture_size(1024, &limits).is_ok());
        let error = check_capture_size(1025, &limits).expect_err("over the cap");
        assert_eq!(
            error
                .details
                .as_ref()
                .and_then(|d| d.get("reason"))
                .and_then(|r| r.as_str()),
            Some("capture_too_large")
        );
    }
}

#[cfg(test)]
mod png_header {
    use super::*;

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes
    }

    #[test]
    fn png_dimensions_are_read_from_ihdr() {
        assert_eq!(
            image_dimensions(&png_header(1280, 5000)),
            Some((1280, 5000))
        );
        // A header cut short is not a picture.
        assert_eq!(image_dimensions(&png_header(1280, 5000)[..20]), None);
        // Not a PNG signature.
        let mut bogus = png_header(3, 2);
        bogus[1] = 0x00;
        assert_eq!(image_dimensions(&bogus), None);
    }

    #[test]
    fn jpeg_dimensions_are_read_from_sof_after_an_app_segment() {
        let mut bytes = vec![0xFF, 0xD8];
        // APP0 with a two-byte payload, to prove the walk skips segments.
        bytes.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]);
        // SOF0: length 17, precision 8, height 600, width 800, three components.
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x02, 0x58, 0x03, 0x20]);
        bytes.extend_from_slice(&[0x03, 0x01, 0x22, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        assert_eq!(image_dimensions(&bytes), Some((800, 600)));
    }

    #[test]
    fn image_dimensions_refuses_other_bytes() {
        assert_eq!(image_dimensions(b"%PDF-1.7"), None);
        assert_eq!(image_dimensions(&[]), None);
        assert_eq!(image_dimensions(&[0xFF, 0xD8, 0xFF, 0xD9]), None);
    }
}
