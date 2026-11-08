//! WS12.S5: paint over the sensitive fields of a screenshot, or refuse the image.
//!
//! The scan (`sensitive_rects`) says where the sensitive controls are in CSS
//! pixels. This module turns those rectangles into device pixels for one image,
//! paints them with a solid colour, and decides when the image must be withheld
//! instead. It is pure: no CDP call, no clock, so the geometry and the fail-closed
//! rules are testable without a browser.
//!
//! Fail closed (plan WS12.S5):
//! - a sensitive field that is rendered but has no usable rectangle (or a scan
//!   that failed) withholds the image;
//! - an image over 40 megapixels, an image the PNG decoder cannot read as 8-bit
//!   RGB or RGBA, or a JPEG that has something to mask is withheld;
//! - an image with nothing to paint is returned untouched, byte for byte.
//!
//! Flags: `QUARRY_BROWSER_SCREENSHOT_MASK=on|off` (default on; `off` is honoured
//! only outside `ENVIRONMENT=production`), `QUARRY_BROWSER_SCREENSHOT_OOPIF=
//! allow|mask|withhold` (default `allow`: cross-origin iframes are counted, not
//! looked into; `mask` blanks every cross-origin iframe rectangle; `withhold`
//! refuses the image when a cross-origin iframe is in it, because its content
//! cannot be scanned) and `QUARRY_BROWSER_SCREENSHOT_SCAN_WORLD=main|isolated`
//! (default `main`: the scan script runs in the page's own JavaScript world;
//! `isolated` runs it in a CDP isolated world a hostile page cannot tamper with,
//! and withholds when that world cannot be made).

// Without the Chromium feature nothing calls the plan, so the helpers are
// exercised only by the unit tests.
#![cfg_attr(not(feature = "chromiumoxide"), allow(dead_code))]

use std::io::Cursor;

use crate::sensitive_rects::{CssRect, RectScan, ScanTransport, ScanWorld};

/// The fill colour, `#1f2937`.
pub const MASK_RGB: [u8; 3] = [0x1f, 0x29, 0x37];

/// The most pixels one image may have to be masked.
pub const MAX_MASK_PIXELS: u64 = 40_000_000;

/// The margin added around every rectangle, in CSS pixels.
pub const PADDING_CSS_PX: f64 = 4.0;

/// A rectangle in the pixels of one image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// How the image is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskOptions {
    pub color: [u8; 3],
    pub max_pixels: u64,
}

impl Default for MaskOptions {
    fn default() -> Self {
        Self {
            color: MASK_RGB,
            max_pixels: MAX_MASK_PIXELS,
        }
    }
}

/// Why an image could not be painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskError {
    /// More pixels than `max_pixels`.
    TooLarge,
    /// Not a PNG, or a PNG that is not 8-bit RGB or RGBA after expansion.
    Unsupported,
    /// The PNG could not be decoded or the result could not be encoded.
    Codec,
}

/// Paint `rects` over a PNG and encode it again. A rectangle partly outside the
/// image is clipped; one fully outside paints nothing.
pub fn mask_png(
    png_bytes: &[u8],
    rects: &[PixelRect],
    opts: &MaskOptions,
) -> Result<Vec<u8>, MaskError> {
    let limits = png::Limits {
        bytes: usize::try_from(opts.max_pixels.saturating_mul(4).saturating_add(1 << 20))
            .unwrap_or(usize::MAX),
    };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(png_bytes), limits);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|_| MaskError::Unsupported)?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if u64::from(width) * u64::from(height) > opts.max_pixels {
        return Err(MaskError::TooLarge);
    }
    let (color, depth) = reader.output_color_type();
    let channels = match (color, depth) {
        (png::ColorType::Rgb, png::BitDepth::Eight) => 3usize,
        (png::ColorType::Rgba, png::BitDepth::Eight) => 4usize,
        _ => return Err(MaskError::Unsupported),
    };
    let mut buffer = vec![0u8; reader.output_buffer_size().ok_or(MaskError::Codec)?];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|_| MaskError::Codec)?;
    let row_bytes = width as usize * channels;
    if frame.buffer_size() < row_bytes * height as usize {
        return Err(MaskError::Codec);
    }
    for rect in rects {
        let x_end = rect.x.saturating_add(rect.w).min(width);
        let y_end = rect.y.saturating_add(rect.h).min(height);
        for y in rect.y.min(height)..y_end {
            for x in rect.x.min(width)..x_end {
                let at = y as usize * row_bytes + x as usize * channels;
                buffer[at..at + 3].copy_from_slice(&opts.color);
                if channels == 4 {
                    buffer[at + 3] = 255;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(png_bytes.len());
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|_| MaskError::Codec)?;
    writer
        .write_image_data(&buffer[..row_bytes * height as usize])
        .map_err(|_| MaskError::Codec)?;
    writer.finish().map_err(|_| MaskError::Codec)?;
    Ok(out)
}

/// One CSS rectangle as pixels of an image: shifted by `origin` (the page
/// position of the image's top-left corner, in CSS pixels), padded by
/// `padding_css`, scaled by `dpr`, rounded outwards and clamped to the image.
/// `None` when nothing of it lies inside the image.
pub fn scale_rect(
    rect: CssRect,
    origin: (f64, f64),
    dpr: f64,
    padding_css: f64,
    image: (u32, u32),
) -> Option<PixelRect> {
    if !(dpr.is_finite() && dpr > 0.0) {
        return None;
    }
    let edge = |value: f64, limit: u32, up: bool| -> u32 {
        let scaled = value * dpr;
        let rounded = if up { scaled.ceil() } else { scaled.floor() };
        if !rounded.is_finite() || rounded <= 0.0 {
            0
        } else if rounded >= f64::from(limit) {
            limit
        } else {
            rounded as u32
        }
    };
    let left = edge(rect.x - padding_css - origin.0, image.0, false);
    let right = edge(rect.x + rect.w + padding_css - origin.0, image.0, true);
    let top = edge(rect.y - padding_css - origin.1, image.1, false);
    let bottom = edge(rect.y + rect.h + padding_css - origin.1, image.1, true);
    (right > left && bottom > top).then_some(PixelRect {
        x: left,
        y: top,
        w: right - left,
        h: bottom - top,
    })
}

/// The page position of an image's top-left corner when the capture did not say:
/// an image larger than the viewport is the document from its origin, a
/// viewport-sized one is the viewport at the scroll offset the scan saw.
pub fn legacy_origin(scan: &RectScan, image: (u32, u32)) -> (f64, f64) {
    let image_w = f64::from(image.0) / scan.dpr;
    let image_h = f64::from(image.1) / scan.dpr;
    if image_w > scan.viewport_w + 2.0 || image_h > scan.viewport_h + 2.0 {
        (0.0, 0.0)
    } else {
        (scan.scroll_x, scan.scroll_y)
    }
}

/// What to do with cross-origin iframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OopifMode {
    /// Count them in the stamp and leave them as they are (the recorded limit).
    #[default]
    Allow,
    /// Blank every cross-origin iframe rectangle.
    Mask,
    /// Refuse the image when a cross-origin iframe with an area lies inside it:
    /// its content cannot be looked into, so it cannot be shown as it is. A frame
    /// with no area shows nothing and does not count.
    Withhold,
}

/// The two flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaskConfig {
    pub mask: bool,
    pub oopif: OopifMode,
    /// Which JavaScript world the scan script runs in.
    pub world: ScanWorld,
}

impl Default for MaskConfig {
    fn default() -> Self {
        Self {
            mask: true,
            oopif: OopifMode::Allow,
            world: ScanWorld::Main,
        }
    }
}

impl MaskConfig {
    /// Read the flag values. Only `off`, `0`, `false` and `no` turn masking off,
    /// and only when `production` is false; anything else leaves it on. Only
    /// `mask` and `withhold` turn a strict iframe mode on, and only `isolated`
    /// selects the isolated scan world.
    #[must_use]
    pub fn from_values(
        mask: Option<&str>,
        oopif: Option<&str>,
        world: Option<&str>,
        production: bool,
    ) -> Self {
        let lower = |raw: Option<&str>| raw.map(|value| value.trim().to_ascii_lowercase());
        let off = matches!(lower(mask).as_deref(), Some("off" | "0" | "false" | "no"));
        Self {
            mask: production || !off,
            oopif: match lower(oopif).as_deref() {
                Some("mask") => OopifMode::Mask,
                Some("withhold") => OopifMode::Withhold,
                _ => OopifMode::Allow,
            },
            world: ScanWorld::from_value(world),
        }
    }

    /// The flags as the process reads them.
    #[must_use]
    pub fn from_env() -> Self {
        let production = {
            let value = std::env::var("ENVIRONMENT")
                .or_else(|_| std::env::var("APP_ENV"))
                .unwrap_or_default();
            let value = value.trim();
            value.eq_ignore_ascii_case("prod") || value.eq_ignore_ascii_case("production")
        };
        Self::from_values(
            std::env::var("QUARRY_BROWSER_SCREENSHOT_MASK")
                .ok()
                .as_deref(),
            std::env::var("QUARRY_BROWSER_SCREENSHOT_OOPIF")
                .ok()
                .as_deref(),
            std::env::var("QUARRY_BROWSER_SCREENSHOT_SCAN_WORLD")
                .ok()
                .as_deref(),
            production,
        )
    }
}

/// Why a screenshot is withheld. The note is the closed string the error and the
/// observation carry; it names no page content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withheld {
    /// A sensitive field is on screen and its rectangle is unknown, or the scan
    /// failed, or the page scrolled during the capture twice in a row.
    SensitiveFieldUnresolved,
    /// The image is over the pixel cap.
    ImageTooLarge,
    /// The image is not one this module can paint (a JPEG, an unusual PNG).
    ImageUnsupported,
    /// The painting failed.
    MaskFailed,
    /// A cross-origin iframe is in the image and `QUARRY_BROWSER_SCREENSHOT_OOPIF=
    /// withhold` is set: its content cannot be scanned.
    OopifUnscannable,
}

impl Withheld {
    /// `screenshot_withheld:<cause>`.
    #[must_use]
    pub const fn note(self) -> &'static str {
        match self {
            Self::SensitiveFieldUnresolved => "screenshot_withheld:sensitive_field_unresolved",
            Self::ImageTooLarge => "screenshot_withheld:image_too_large",
            Self::ImageUnsupported => "screenshot_withheld:image_unsupported",
            Self::MaskFailed => "screenshot_withheld:mask_failed",
            Self::OopifUnscannable => "screenshot_withheld:oopif_unscannable",
        }
    }
}

impl From<MaskError> for Withheld {
    fn from(error: MaskError) -> Self {
        match error {
            MaskError::TooLarge => Self::ImageTooLarge,
            MaskError::Unsupported => Self::ImageUnsupported,
            MaskError::Codec => Self::MaskFailed,
        }
    }
}

/// The rectangles to paint on one image, and what the scan could not look into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaskPlan {
    pub rects: Vec<PixelRect>,
    pub oopif_frames: u32,
}

/// Decide what to paint, or that the image is withheld. `origin` is the page
/// position of the image's top-left corner in CSS pixels (the scroll offset for a
/// viewport image, the clip corner for a clip, 0,0 for a whole page), `image` its
/// size in pixels. The scan's rectangles are client coordinates, so the scroll the
/// scan saw is added to make them page coordinates.
pub fn plan_mask(
    scan: &RectScan,
    config: &MaskConfig,
    origin: (f64, f64),
    image: (u32, u32),
) -> Result<MaskPlan, Withheld> {
    if scan.unresolved_visible > 0 {
        return Err(Withheld::SensitiveFieldUnresolved);
    }
    let mut css: Vec<CssRect> = scan.rects.clone();
    if config.oopif == OopifMode::Mask {
        if usize::try_from(scan.oopif_frames).unwrap_or(usize::MAX) > scan.oopif_rects.len() {
            // A cross-origin frame with no usable rectangle cannot be blanked.
            return Err(Withheld::SensitiveFieldUnresolved);
        }
        css.extend(scan.oopif_rects.iter().copied());
    }
    let local = (origin.0 - scan.scroll_x, origin.1 - scan.scroll_y);
    if config.oopif == OopifMode::Withhold {
        // `oopif_rects` holds the frames that have an area; one with none shows
        // nothing. A frame that lies wholly outside this image is not in it.
        let in_image = scan
            .oopif_rects
            .iter()
            .any(|frame| scale_rect(*frame, local, scan.dpr, 0.0, image).is_some());
        if in_image {
            return Err(Withheld::OopifUnscannable);
        }
    }
    let rects = css
        .into_iter()
        .filter_map(|rect| scale_rect(rect, local, scan.dpr, PADDING_CSS_PX, image))
        .collect();
    Ok(MaskPlan {
        rects,
        oopif_frames: scan.oopif_frames,
    })
}

/// Is `bytes` a PNG (by its signature)?
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
}

/// Apply a plan to an image. `Ok(None)` means there was nothing to paint and the
/// original bytes stand. A JPEG with something to paint is withheld.
pub fn apply_plan(
    image: &[u8],
    plan: &MaskPlan,
    opts: &MaskOptions,
) -> Result<Option<Vec<u8>>, Withheld> {
    if plan.rects.is_empty() {
        return Ok(None);
    }
    if !is_png(image) {
        return Err(Withheld::ImageUnsupported);
    }
    mask_png(image, &plan.rects, opts)
        .map(Some)
        .map_err(Withheld::from)
}

/// Where the image's top-left corner is on the page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Origin {
    /// Decide from the image size and the scan (`legacy_origin`): the capture took no clip.
    Legacy,
    /// The image starts at this page position (a clip, or a whole page at 0,0).
    Page(f64, f64),
}

/// What one masked capture produced.
#[derive(Debug)]
pub enum Captured<M> {
    Image {
        bytes: Vec<u8>,
        meta: M,
        state: quarry_core::redaction::ScreenshotState,
        rects: Vec<PixelRect>,
        oopif_frames: u32,
        mask_ms: u32,
    },
    Withheld {
        reason: Withheld,
        oopif_frames: u32,
        mask_ms: u32,
    },
}

impl<M> Captured<M> {
    /// The image and what came with it, or the `SecurityBlocked` error a caller
    /// that needs the image gets when it was withheld.
    pub fn into_image(self) -> quarry_core::QuarryResult<(Vec<u8>, M)> {
        match self {
            Self::Image { bytes, meta, .. } => Ok((bytes, meta)),
            Self::Withheld { reason, .. } => {
                Err(crate::MaskedScreenshot::withheld_error(reason.note()))
            }
        }
    }

    /// The same outcome as the record the runtime stamps from.
    pub fn into_masked_screenshot(self) -> crate::MaskedScreenshot {
        match self {
            Self::Image {
                bytes,
                state,
                rects,
                oopif_frames,
                mask_ms,
                ..
            } => crate::MaskedScreenshot {
                bytes: Some(bytes::Bytes::from(bytes)),
                state,
                rects,
                oopif_frames,
                mask_ms,
                note: None,
            },
            Self::Withheld {
                reason,
                oopif_frames,
                mask_ms,
            } => crate::MaskedScreenshot::withheld(reason, oopif_frames, mask_ms),
        }
    }
}

/// Scan, capture, check the page did not scroll, and mask.
///
/// `capture` takes the image (and anything the caller wants back with it, `M`);
/// an error from it is the caller's error and is returned as is. The order is the
/// plan's: scan first, capture, then read the scroll again. A scroll in between
/// moved the rectangles, so the scan is redone once; a second scroll withholds.
/// With `config.mask` off (outside production only) the image is captured as it
/// is and the state is `none`. `mask_ms` is the time spent outside `capture`.
pub async fn capture_masked<T, M, F, Fut>(
    page: &T,
    config: &MaskConfig,
    origin: Origin,
    mut capture: F,
) -> quarry_core::QuarryResult<Captured<M>>
where
    T: ScanTransport + ?Sized,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = quarry_core::QuarryResult<(Vec<u8>, M)>>,
{
    use crate::sensitive_rects::{same_scroll, scan_page, scroll_now};
    use quarry_core::redaction::ScreenshotState;
    use std::time::{Duration, Instant};

    let elapsed_ms = |d: Duration| u32::try_from(d.as_millis()).unwrap_or(u32::MAX);
    if !config.mask {
        let (bytes, meta) = capture().await?;
        return Ok(Captured::Image {
            bytes,
            meta,
            state: ScreenshotState::NotApplicable,
            rects: Vec::new(),
            oopif_frames: 0,
            mask_ms: 0,
        });
    }
    let mut spent = Duration::ZERO;
    for _attempt in 0..2 {
        let scan_started = Instant::now();
        let Some(scan) = scan_page(page, config.world).await else {
            return Ok(Captured::Withheld {
                reason: Withheld::SensitiveFieldUnresolved,
                oopif_frames: 0,
                mask_ms: elapsed_ms(spent + scan_started.elapsed()),
            });
        };
        spent += scan_started.elapsed();
        let (bytes, meta) = capture().await?;
        let check_started = Instant::now();
        // Only a scan with something to paint depends on where the page was
        // scrolled; with nothing sensitive on the page a scroll changes nothing.
        let depends_on_scroll = !scan.rects.is_empty()
            || (config.oopif != OopifMode::Allow && !scan.oopif_rects.is_empty());
        let still = if depends_on_scroll {
            scroll_now(page, config.world)
                .await
                .is_some_and(|now| same_scroll((scan.scroll_x, scan.scroll_y), now))
        } else {
            true
        };
        spent += check_started.elapsed();
        if !still {
            continue;
        }
        let paint_started = Instant::now();
        let image_size = crate::capture_plan::image_dimensions(&bytes);
        let outcome = (|| {
            let size = image_size.ok_or(Withheld::ImageUnsupported)?;
            let origin = match origin {
                Origin::Legacy => legacy_origin(&scan, size),
                Origin::Page(x, y) => (x, y),
            };
            let plan = plan_mask(&scan, config, origin, size)?;
            let painted = apply_plan(&bytes, &plan, &MaskOptions::default())?;
            Ok::<_, Withheld>((plan, painted))
        })();
        spent += paint_started.elapsed();
        let mask_ms = elapsed_ms(spent);
        return Ok(match outcome {
            Ok((plan, Some(painted))) => Captured::Image {
                bytes: painted,
                meta,
                state: ScreenshotState::Masked,
                rects: plan.rects,
                oopif_frames: plan.oopif_frames,
                mask_ms,
            },
            Ok((plan, None)) => Captured::Image {
                bytes,
                meta,
                state: ScreenshotState::NotApplicable,
                rects: Vec::new(),
                oopif_frames: plan.oopif_frames,
                mask_ms,
            },
            Err(reason) => Captured::Withheld {
                reason,
                oopif_frames: scan.oopif_frames,
                mask_ms,
            },
        });
    }
    // The page scrolled during both captures.
    Ok(Captured::Withheld {
        reason: Withheld::SensitiveFieldUnresolved,
        oopif_frames: 0,
        mask_ms: elapsed_ms(spent),
    })
}

/// Whether the page may be printed to a PDF. A PDF cannot be painted over, so it
/// is refused when a rendered sensitive field holds a value, when a sensitive
/// field's rectangle is unknown, or when the scan failed. With masking off
/// (outside production only) it is allowed.
/// With `QUARRY_BROWSER_SCREENSHOT_OOPIF=withhold` it is also refused when a
/// cross-origin iframe with an area is on the page.
pub async fn pdf_allowed<T: ScanTransport + ?Sized>(
    page: &T,
    config: &MaskConfig,
) -> Result<(), &'static str> {
    if !config.mask {
        return Ok(());
    }
    match crate::sensitive_rects::scan_page(page, config.world).await {
        None => Err(PDF_WITHHELD_UNRESOLVED),
        Some(scan) if scan.unresolved_visible > 0 => Err(PDF_WITHHELD_UNRESOLVED),
        Some(scan) if config.oopif == OopifMode::Withhold && !scan.oopif_rects.is_empty() => {
            Err(PDF_WITHHELD_OOPIF)
        }
        Some(scan) if scan.sensitive_with_value > 0 => Err(PDF_WITHHELD_VALUE),
        Some(_) => Ok(()),
    }
}

/// The closed reasons a PDF is refused.
pub const PDF_WITHHELD_VALUE: &str = "pdf_withheld:sensitive_field_value";
pub const PDF_WITHHELD_UNRESOLVED: &str = "pdf_withheld:sensitive_field_unresolved";
pub const PDF_WITHHELD_OOPIF: &str = "pdf_withheld:oopif_unscannable";

/// The error a refused PDF returns: `SecurityBlocked`, 403, with the closed reason.
pub fn pdf_withheld_error(reason: &'static str) -> quarry_core::error::QuarryError {
    quarry_core::error::QuarryError::new(
        quarry_core::error::ErrorCode::SecurityBlocked,
        "the pdf was withheld because a sensitive field holds a value or a cross-origin frame cannot be scanned",
    )
    .with_details(serde_json::json!({ "reason": reason }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `width` x `height` RGBA PNG whose pixel (x, y) is `(x, y, 7, 200)`.
    fn gradient_png(width: u32, height: u32) -> Vec<u8> {
        let mut data = Vec::new();
        for y in 0..height {
            for x in 0..width {
                data.extend_from_slice(&[x as u8, y as u8, 7, 200]);
            }
        }
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(&data).expect("data");
        writer.finish().expect("finish");
        out
    }

    fn rgb_png(width: u32, height: u32) -> Vec<u8> {
        let data = vec![0x80u8; (width * height * 3) as usize];
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        writer.write_image_data(&data).expect("data");
        writer.finish().expect("finish");
        out
    }

    fn decode(bytes: &[u8]) -> (u32, u32, usize, Vec<u8>) {
        let mut reader = png::Decoder::new(Cursor::new(bytes))
            .read_info()
            .expect("png header");
        let mut buffer = vec![0u8; reader.output_buffer_size().expect("size")];
        let frame = reader.next_frame(&mut buffer).expect("frame");
        let channels = match frame.color_type {
            png::ColorType::Rgba => 4,
            png::ColorType::Rgb => 3,
            other => panic!("unexpected colour type {other:?}"),
        };
        (frame.width, frame.height, channels, buffer)
    }

    #[test]
    fn mask_png_blocks_exact_rect_and_leaves_rest_untouched() {
        let source = gradient_png(20, 10);
        let rect = PixelRect {
            x: 5,
            y: 2,
            w: 6,
            h: 4,
        };
        let masked = mask_png(&source, &[rect], &MaskOptions::default()).expect("masked");
        let (width, height, channels, before) = decode(&source);
        let (w2, h2, c2, after) = decode(&masked);
        assert_eq!((width, height, channels), (w2, h2, c2));
        for y in 0..height {
            for x in 0..width {
                let at = (y * width + x) as usize * channels;
                let inside = (5..11).contains(&x) && (2..6).contains(&y);
                if inside {
                    assert_eq!(
                        &after[at..at + 3],
                        &MASK_RGB,
                        "pixel ({x},{y}) is the mask colour"
                    );
                    assert_eq!(after[at + 3], 255, "the mask is opaque");
                } else {
                    assert_eq!(
                        &after[at..at + channels],
                        &before[at..at + channels],
                        "pixel ({x},{y}) is untouched"
                    );
                }
            }
        }
    }

    #[test]
    fn mask_png_handles_rgb_and_clips_a_rectangle_that_overhangs() {
        let source = rgb_png(10, 10);
        let masked = mask_png(
            &source,
            &[
                PixelRect {
                    x: 8,
                    y: 8,
                    w: 50,
                    h: 50,
                },
                PixelRect {
                    x: 40,
                    y: 40,
                    w: 5,
                    h: 5,
                },
            ],
            &MaskOptions::default(),
        )
        .expect("masked");
        let (_, _, channels, pixels) = decode(&masked);
        assert_eq!(channels, 3);
        let at = |x: usize, y: usize| (y * 10 + x) * 3;
        assert_eq!(&pixels[at(9, 9)..at(9, 9) + 3], &MASK_RGB);
        assert_eq!(&pixels[at(7, 7)..at(7, 7) + 3], &[0x80, 0x80, 0x80]);
    }

    #[test]
    fn mask_png_rejects_oversize() {
        let source = gradient_png(20, 10);
        let opts = MaskOptions {
            max_pixels: 199,
            ..MaskOptions::default()
        };
        assert_eq!(
            mask_png(
                &source,
                &[PixelRect {
                    x: 0,
                    y: 0,
                    w: 1,
                    h: 1
                }],
                &opts
            ),
            Err(MaskError::TooLarge)
        );
        let exact = MaskOptions {
            max_pixels: 200,
            ..MaskOptions::default()
        };
        assert!(mask_png(
            &source,
            &[PixelRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1
            }],
            &exact
        )
        .is_ok());
    }

    #[test]
    fn mask_png_refuses_what_is_not_a_png() {
        assert_eq!(
            mask_png(b"\xff\xd8\xff\xe0 jpeg", &[], &MaskOptions::default()),
            Err(MaskError::Unsupported)
        );
    }

    #[test]
    fn rect_scaling_uses_device_pixel_ratio() {
        let rect = CssRect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 30.0,
        };
        let at_one = scale_rect(rect, (0.0, 0.0), 1.0, 0.0, (1000, 1000)).expect("rect");
        assert_eq!(
            at_one,
            PixelRect {
                x: 10,
                y: 20,
                w: 100,
                h: 30
            }
        );
        let at_two = scale_rect(rect, (0.0, 0.0), 2.0, 0.0, (2000, 2000)).expect("rect");
        assert_eq!(
            at_two,
            PixelRect {
                x: 20,
                y: 40,
                w: 200,
                h: 60
            }
        );
        let fractional = scale_rect(rect, (0.0, 0.0), 1.5, 0.0, (2000, 2000)).expect("rect");
        assert_eq!(
            fractional,
            PixelRect {
                x: 15,
                y: 30,
                w: 150,
                h: 45
            }
        );
        // The padding is CSS pixels, so it scales too.
        let padded = scale_rect(rect, (0.0, 0.0), 2.0, 4.0, (2000, 2000)).expect("rect");
        assert_eq!(
            padded,
            PixelRect {
                x: 12,
                y: 32,
                w: 216,
                h: 76
            }
        );
    }

    #[test]
    fn rect_scaling_rounds_outwards_and_shifts_by_the_origin() {
        let rect = CssRect {
            x: 10.3,
            y: 20.3,
            w: 10.0,
            h: 10.0,
        };
        let pixels = scale_rect(rect, (0.0, 0.0), 1.0, 0.0, (100, 100)).expect("rect");
        // floor(10.3) = 10 to ceil(20.3) = 21.
        assert_eq!((pixels.x, pixels.w), (10, 11));
        let shifted = scale_rect(rect, (5.0, 10.0), 1.0, 0.0, (100, 100)).expect("rect");
        assert_eq!((shifted.x, shifted.y), (5, 10));
    }

    #[test]
    fn padding_clamps_to_image() {
        let rect = CssRect {
            x: 1.0,
            y: 1.0,
            w: 98.0,
            h: 18.0,
        };
        let pixels = scale_rect(rect, (0.0, 0.0), 1.0, 4.0, (100, 20)).expect("rect");
        assert_eq!(
            pixels,
            PixelRect {
                x: 0,
                y: 0,
                w: 100,
                h: 20
            }
        );
    }

    #[test]
    fn a_rectangle_outside_the_image_paints_nothing() {
        let below = CssRect {
            x: 0.0,
            y: 5000.0,
            w: 10.0,
            h: 10.0,
        };
        assert_eq!(scale_rect(below, (0.0, 0.0), 1.0, 4.0, (100, 100)), None);
        let above = CssRect {
            x: 0.0,
            y: 10.0,
            w: 10.0,
            h: 10.0,
        };
        assert_eq!(scale_rect(above, (0.0, 600.0), 1.0, 4.0, (100, 100)), None);
        assert_eq!(scale_rect(above, (0.0, 0.0), 0.0, 4.0, (100, 100)), None);
    }

    fn scan(rects: Vec<CssRect>) -> RectScan {
        RectScan {
            rects,
            dpr: 1.0,
            viewport_w: 1280.0,
            viewport_h: 800.0,
            scroll_y: 300.0,
            ..RectScan::default()
        }
    }

    #[test]
    fn an_unresolved_field_withholds_with_the_documented_note() {
        let mut s = scan(vec![]);
        s.unresolved_visible = 1;
        let withheld =
            plan_mask(&s, &MaskConfig::default(), (0.0, 0.0), (100, 100)).expect_err("withheld");
        assert_eq!(withheld, Withheld::SensitiveFieldUnresolved);
        assert_eq!(
            withheld.note(),
            "screenshot_withheld:sensitive_field_unresolved"
        );
    }

    #[test]
    fn oopif_strict_mode_blanks_frames_and_refuses_a_frame_it_cannot_place() {
        let mut s = scan(vec![]);
        s.oopif_frames = 1;
        s.oopif_rects = vec![CssRect {
            x: 10.0,
            y: 10.0,
            w: 50.0,
            h: 50.0,
        }];
        let strict = MaskConfig::from_values(None, Some("mask"), None, true);
        let viewport_origin = (0.0, 300.0);
        let plan = plan_mask(&s, &strict, viewport_origin, (200, 200)).expect("plan");
        assert_eq!(plan.rects.len(), 1);
        assert_eq!(plan.oopif_frames, 1);
        let allow =
            plan_mask(&s, &MaskConfig::default(), viewport_origin, (200, 200)).expect("plan");
        assert!(allow.rects.is_empty(), "allow mode leaves the frame alone");
        assert_eq!(allow.oopif_frames, 1, "and still counts it");
        s.oopif_frames = 2;
        assert_eq!(
            plan_mask(&s, &strict, viewport_origin, (200, 200)),
            Err(Withheld::SensitiveFieldUnresolved)
        );
    }

    #[test]
    fn client_rectangles_land_at_the_right_pixels_for_a_viewport_and_for_a_page_image() {
        // The page is scrolled 300 px; the field is at client y = 50, so page y = 350.
        let s = scan(vec![CssRect {
            x: 20.0,
            y: 50.0,
            w: 100.0,
            h: 20.0,
        }]);
        let config = MaskConfig::default();
        // A viewport image starts at the scroll offset: the field is at image y = 50 - padding.
        let viewport = plan_mask(&s, &config, (0.0, 300.0), (1280, 800)).expect("plan");
        assert_eq!(
            viewport.rects,
            vec![PixelRect {
                x: 16,
                y: 46,
                w: 108,
                h: 28
            }]
        );
        // A whole-page image starts at 0,0: the field is at image y = 350 - padding.
        let page = plan_mask(&s, &config, (0.0, 0.0), (1280, 5000)).expect("plan");
        assert_eq!(
            page.rects,
            vec![PixelRect {
                x: 16,
                y: 346,
                w: 108,
                h: 28
            }]
        );
        // A clip starting at page y = 340 puts the field 10 px down, less the padding.
        let clip = plan_mask(&s, &config, (0.0, 340.0), (1280, 400)).expect("plan");
        assert_eq!(
            clip.rects,
            vec![PixelRect {
                x: 16,
                y: 6,
                w: 108,
                h: 28
            }]
        );
    }

    #[test]
    fn the_legacy_origin_is_the_scroll_for_a_viewport_image_and_zero_for_a_page() {
        let s = scan(vec![]);
        assert_eq!(legacy_origin(&s, (1280, 800)), (0.0, 300.0));
        assert_eq!(legacy_origin(&s, (1280, 5000)), (0.0, 0.0));
    }

    #[test]
    fn nothing_to_paint_keeps_the_original_bytes_and_a_jpeg_with_something_is_withheld() {
        let plan = MaskPlan {
            rects: vec![],
            oopif_frames: 0,
        };
        assert_eq!(
            apply_plan(b"anything", &plan, &MaskOptions::default()),
            Ok(None)
        );
        let plan = MaskPlan {
            rects: vec![PixelRect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            }],
            oopif_frames: 0,
        };
        assert_eq!(
            apply_plan(b"\xff\xd8\xff", &plan, &MaskOptions::default()),
            Err(Withheld::ImageUnsupported)
        );
    }

    #[test]
    fn the_flags_default_on_and_off_is_ignored_in_production() {
        assert!(MaskConfig::from_values(None, None, None, false).mask);
        assert!(!MaskConfig::from_values(Some("off"), None, None, false).mask);
        assert!(!MaskConfig::from_values(Some(" FALSE "), None, None, false).mask);
        assert!(MaskConfig::from_values(Some("off"), None, None, true).mask);
        assert!(MaskConfig::from_values(Some("garbage"), None, None, false).mask);
        assert_eq!(
            MaskConfig::from_values(None, None, None, true).oopif,
            OopifMode::Allow
        );
        assert_eq!(
            MaskConfig::from_values(None, Some("Mask"), None, true).oopif,
            OopifMode::Mask
        );
    }

    #[test]
    fn the_new_flags_default_to_the_behaviour_as_built() {
        let default = MaskConfig::from_values(None, None, None, true);
        assert_eq!(default, MaskConfig::default());
        assert_eq!(default.oopif, OopifMode::Allow);
        assert_eq!(default.world, ScanWorld::Main);
        // Garbage in either flag is the default, never a stricter or looser mode.
        let junk = MaskConfig::from_values(None, Some("on"), Some("yes"), true);
        assert_eq!(junk, MaskConfig::default());
        assert_eq!(
            MaskConfig::from_values(None, Some(" WITHHOLD "), None, true).oopif,
            OopifMode::Withhold
        );
        assert_eq!(
            MaskConfig::from_values(None, None, Some("Isolated"), false).world,
            ScanWorld::Isolated
        );
        // The world flag is not a production-only switch: it stays as set.
        assert_eq!(
            MaskConfig::from_values(None, None, Some("isolated"), true).world,
            ScanWorld::Isolated
        );
    }

    fn frame(x: f64, y: f64, w: f64, h: f64) -> CssRect {
        CssRect { x, y, w, h }
    }

    #[test]
    fn withhold_mode_refuses_an_image_that_has_a_cross_origin_frame_in_it() {
        let mut s = scan(vec![]);
        s.oopif_frames = 1;
        s.oopif_rects = vec![frame(10.0, 10.0, 50.0, 50.0)];
        let config = MaskConfig::from_values(None, Some("withhold"), None, true);
        let viewport_origin = (0.0, 300.0);
        let refused = plan_mask(&s, &config, viewport_origin, (200, 200)).expect_err("refused");
        assert_eq!(refused, Withheld::OopifUnscannable);
        assert_eq!(refused.note(), "screenshot_withheld:oopif_unscannable");
        // Allow mode still returns the image, counting the frame.
        let allow =
            plan_mask(&s, &MaskConfig::default(), viewport_origin, (200, 200)).expect("plan");
        assert_eq!(allow.oopif_frames, 1);
    }

    #[test]
    fn withhold_mode_ignores_a_frame_with_no_area_and_a_frame_outside_the_image() {
        let config = MaskConfig::from_values(None, Some("withhold"), None, true);
        // Counted but with no usable rectangle (zero area): it shows nothing.
        let mut s = scan(vec![]);
        s.oopif_frames = 1;
        let plan = plan_mask(&s, &config, (0.0, 300.0), (200, 200)).expect("plan");
        assert!(plan.rects.is_empty());
        assert_eq!(plan.oopif_frames, 1, "it is still counted");
        // A frame 2000 px down is not in a 200 px viewport image at scroll 300.
        s.oopif_rects = vec![frame(10.0, 2000.0, 50.0, 50.0)];
        s.oopif_frames = 1;
        assert!(plan_mask(&s, &config, (0.0, 300.0), (200, 200)).is_ok());
        // The same frame is in a whole-page image.
        assert_eq!(
            plan_mask(&s, &config, (0.0, 0.0), (400, 3000)),
            Err(Withheld::OopifUnscannable)
        );
    }

    #[test]
    fn withhold_mode_still_masks_the_fields_it_can_see_and_unresolved_wins() {
        let config = MaskConfig::from_values(None, Some("withhold"), None, true);
        let mut s = scan(vec![frame(20.0, 50.0, 100.0, 20.0)]);
        // No frame: the field is painted as in any mode.
        let plan = plan_mask(&s, &config, (0.0, 300.0), (1280, 800)).expect("plan");
        assert_eq!(plan.rects.len(), 1);
        // An unresolved field withholds first, with its own note.
        s.unresolved_visible = 1;
        s.oopif_rects = vec![frame(0.0, 0.0, 50.0, 50.0)];
        assert_eq!(
            plan_mask(&s, &config, (0.0, 300.0), (1280, 800)),
            Err(Withheld::SensitiveFieldUnresolved)
        );
    }

    // The mock driver: a page that answers the scan script with a canned answer,
    // and a capture closure that returns a PNG.

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    struct MockPage {
        main: serde_json::Value,
        isolated: Option<serde_json::Value>,
        scroll: serde_json::Value,
        worlds: Mutex<Vec<ScanWorld>>,
        scans: AtomicUsize,
    }

    impl MockPage {
        fn new(main: serde_json::Value, isolated: Option<serde_json::Value>) -> Self {
            Self {
                main,
                isolated,
                scroll: serde_json::json!([0.0, 0.0]),
                worlds: Mutex::new(Vec::new()),
                scans: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl ScanTransport for MockPage {
        async fn scan_eval(&self, script: &str, world: ScanWorld) -> Option<serde_json::Value> {
            self.worlds.lock().expect("worlds").push(world);
            if script == crate::sensitive_rects::SCROLL_SCRIPT {
                return Some(self.scroll.clone());
            }
            self.scans.fetch_add(1, Ordering::SeqCst);
            match world {
                ScanWorld::Main => Some(self.main.clone()),
                ScanWorld::Isolated => self.isolated.clone(),
            }
        }
    }

    fn scan_json(
        fields: Vec<serde_json::Value>,
        oopif: Vec<serde_json::Value>,
    ) -> serde_json::Value {
        serde_json::json!({
            "ok": true, "truncated": false, "scroll": [0.0, 0.0],
            "viewport": [100.0, 100.0], "dpr": 1.0,
            "fields": fields, "oopif": oopif,
        })
    }

    fn password_json() -> serde_json::Value {
        serde_json::json!({
            "facts": { "input_type": "password", "name_attr": "pw" },
            "has_value": true,
            "rect": { "x": 10.0, "y": 10.0, "w": 40.0, "h": 20.0 },
        })
    }

    fn frame_json() -> serde_json::Value {
        serde_json::json!({ "x": 5.0, "y": 60.0, "w": 80.0, "h": 30.0 })
    }

    async fn run_capture(
        page: &MockPage,
        config: &MaskConfig,
    ) -> quarry_core::QuarryResult<Captured<()>> {
        capture_masked(page, config, Origin::Page(0.0, 0.0), || async {
            Ok((gradient_png(100, 100), ()))
        })
        .await
    }

    #[tokio::test]
    async fn mock_driver_the_isolated_world_masks_what_a_hostile_main_world_hides() {
        // The page's own world reports a clean page; the isolated world is truthful.
        let page = MockPage::new(
            scan_json(vec![], vec![]),
            Some(scan_json(vec![password_json()], vec![])),
        );
        let isolated = MaskConfig::from_values(None, None, Some("isolated"), true);
        match run_capture(&page, &isolated).await.expect("capture") {
            Captured::Image { state, rects, .. } => {
                assert_eq!(state, quarry_core::redaction::ScreenshotState::Masked);
                assert_eq!(rects.len(), 1);
            }
            other => panic!("expected a masked image, got {other:?}"),
        }
        assert!(
            page.worlds
                .lock()
                .expect("worlds")
                .iter()
                .all(|w| *w == ScanWorld::Isolated),
            "the scan and the scroll check both ran in the isolated world"
        );
        // The default (main world, as built) is fooled: this is the exposure.
        let page = MockPage::new(
            scan_json(vec![], vec![]),
            Some(scan_json(vec![password_json()], vec![])),
        );
        match run_capture(&page, &MaskConfig::default())
            .await
            .expect("capture")
        {
            Captured::Image { state, rects, .. } => {
                assert_eq!(
                    state,
                    quarry_core::redaction::ScreenshotState::NotApplicable
                );
                assert!(rects.is_empty());
            }
            other => panic!("expected an unmasked image, got {other:?}"),
        }
        assert!(page
            .worlds
            .lock()
            .expect("worlds")
            .iter()
            .all(|w| *w == ScanWorld::Main));
    }

    #[tokio::test]
    async fn mock_driver_an_isolated_world_that_cannot_be_made_withholds_the_image() {
        let page = MockPage::new(scan_json(vec![], vec![]), None);
        let isolated = MaskConfig::from_values(None, None, Some("isolated"), true);
        match run_capture(&page, &isolated).await.expect("capture") {
            Captured::Withheld { reason, .. } => {
                assert_eq!(reason, Withheld::SensitiveFieldUnresolved);
            }
            other => panic!("expected a withheld image, got {other:?}"),
        }
        assert!(
            page.worlds
                .lock()
                .expect("worlds")
                .iter()
                .all(|w| *w == ScanWorld::Isolated),
            "no fallback to the main world"
        );
    }

    #[tokio::test]
    async fn mock_driver_withhold_mode_refuses_a_capture_with_a_cross_origin_frame() {
        let page = MockPage::new(scan_json(vec![], vec![frame_json()]), None);
        let withhold = MaskConfig::from_values(None, Some("withhold"), None, true);
        match run_capture(&page, &withhold).await.expect("capture") {
            Captured::Withheld {
                reason,
                oopif_frames,
                ..
            } => {
                assert_eq!(reason, Withheld::OopifUnscannable);
                assert_eq!(oopif_frames, 1);
            }
            other => panic!("expected a withheld image, got {other:?}"),
        }
        // As a record the runtime stamps from, and as the error an explicit
        // screenshot action gets.
        let record = run_capture(&page, &withhold)
            .await
            .expect("capture")
            .into_masked_screenshot();
        assert!(record.bytes.is_none());
        assert_eq!(
            record.state,
            quarry_core::redaction::ScreenshotState::Withheld
        );
        assert_eq!(record.note, Some("screenshot_withheld:oopif_unscannable"));
        assert!(run_capture(&page, &withhold)
            .await
            .expect("capture")
            .into_image()
            .is_err());
        // Allow (the default) and Mask keep returning an image.
        assert!(matches!(
            run_capture(&page, &MaskConfig::default())
                .await
                .expect("capture"),
            Captured::Image {
                oopif_frames: 1,
                ..
            }
        ));
        let mask = MaskConfig::from_values(None, Some("mask"), None, true);
        assert!(matches!(
            run_capture(&page, &mask).await.expect("capture"),
            Captured::Image {
                state: quarry_core::redaction::ScreenshotState::Masked,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn mock_driver_withhold_mode_sees_the_frame_that_the_isolated_scan_finds() {
        // The page's own world hides its iframe; the isolated world reports it.
        let page = MockPage::new(
            scan_json(vec![], vec![]),
            Some(scan_json(vec![], vec![frame_json()])),
        );
        let config = MaskConfig::from_values(None, Some("withhold"), Some("isolated"), true);
        assert!(matches!(
            run_capture(&page, &config).await.expect("capture"),
            Captured::Withheld {
                reason: Withheld::OopifUnscannable,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn mock_driver_masking_off_scans_nothing() {
        let page = MockPage::new(scan_json(vec![password_json()], vec![]), None);
        let off = MaskConfig::from_values(Some("off"), None, Some("isolated"), false);
        match run_capture(&page, &off).await.expect("capture") {
            Captured::Image { state, .. } => {
                assert_eq!(
                    state,
                    quarry_core::redaction::ScreenshotState::NotApplicable
                );
            }
            other => panic!("expected the image as it is, got {other:?}"),
        }
        assert_eq!(page.scans.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn mock_driver_pdf_follows_the_world_and_the_withhold_mode() {
        let hidden = scan_json(vec![], vec![]);
        let seen = scan_json(vec![password_json()], vec![frame_json()]);
        // Main world fooled, isolated truthful: only the isolated config refuses.
        let page = MockPage::new(hidden, Some(seen));
        assert_eq!(pdf_allowed(&page, &MaskConfig::default()).await, Ok(()));
        let isolated = MaskConfig::from_values(None, None, Some("isolated"), true);
        assert_eq!(pdf_allowed(&page, &isolated).await, Err(PDF_WITHHELD_VALUE));
        // A page with only a cross-origin frame: refused under withhold, not otherwise.
        let page = MockPage::new(scan_json(vec![], vec![frame_json()]), None);
        assert_eq!(pdf_allowed(&page, &MaskConfig::default()).await, Ok(()));
        let withhold = MaskConfig::from_values(None, Some("withhold"), None, true);
        assert_eq!(pdf_allowed(&page, &withhold).await, Err(PDF_WITHHELD_OOPIF));
        // An isolated config whose world cannot be made fails closed.
        let page = MockPage::new(scan_json(vec![], vec![]), None);
        assert_eq!(
            pdf_allowed(&page, &isolated).await,
            Err(PDF_WITHHELD_UNRESOLVED)
        );
    }
}
