//! WS13.6: tile-and-stitch for a full-page capture taller than one Chromium capture.
//!
//! Method A only: every tile is a clip of the page (`captureBeyondViewport`, no
//! scrolling, no overlap). Method B (scroll and trim) waits for the spike that
//! `docs/SCRAPE_CAPTURE.md` records; it is not built here. This module is planning
//! and encoding only. The tile captures are taken in `chromiumoxide.rs`, and the
//! flag that decides whether a permit is offered is read by the runtime.
//!
//! PNG output is streamed: each tile is decoded, written into the output and
//! dropped before the next tile arrives, so the held memory is one tile. JPEG
//! output has to hold the whole image (the encoder needs it), so its pixel cap is
//! lower.

use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use quarry_core::capture::{
    ImageFormat, MAX_CONTENT_CSS_PX, MAX_STITCH_PX_JPEG, MAX_STITCH_PX_PNG, MAX_TILES,
    MAX_TILE_CSS_PX, SINGLE_CAPTURE_MAX_DEVICE_PX,
};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use serde_json::json;

/// The most a tile may decode to. A tile is at most 4,096 CSS pixels tall at a
/// device scale of 2 and 1,280 wide, which is about 84 MB of RGBA, so the png
/// default of 64 MB would refuse a legal tile.
const TILE_DECODE_BYTES: usize = 256 * 1024 * 1024;

/// One tile of the page, in whole CSS pixels from the top of the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    pub y: u32,
    pub h: u32,
}

/// The tiles of one stitched capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TilePlan {
    /// The content width in whole CSS pixels, the clip width of every tile.
    pub width_css: u32,
    pub tiles: Vec<Tile>,
    /// The stitched height in CSS pixels: the sum of the tile heights.
    pub css_height: u32,
    /// The page is taller than the limits allow; the image stops at `css_height`.
    pub truncated: bool,
}

/// The device scale factor, or 1 when the value is not a positive number.
pub fn sanitize_scale(device_scale_factor: f64) -> f64 {
    if device_scale_factor.is_finite() && device_scale_factor > 0.0 {
        device_scale_factor
    } else {
        1.0
    }
}

/// A CSS length in device pixels, rounded. Every edge of a tile is rounded the same
/// way, so neighbouring tiles meet with no gap and no overlap.
pub fn device_px(css: f64, device_scale_factor: f64) -> u32 {
    let px = (css * sanitize_scale(device_scale_factor)).round();
    if px.is_finite() && px > 0.0 {
        px as u32
    } else {
        0
    }
}

/// The CSS height of one tile: at most 4,096 CSS pixels, and never more device
/// pixels than one capture holds.
pub fn tile_css_height(device_scale_factor: f64) -> u32 {
    let by_capture =
        (f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX) / sanitize_scale(device_scale_factor)).floor();
    let by_capture = if by_capture.is_finite() && by_capture >= 1.0 {
        by_capture as u32
    } else {
        1
    };
    MAX_TILE_CSS_PX.min(by_capture).max(1)
}

/// True when one capture cannot hold a page this tall at this device scale.
pub fn needs_stitch(content_height_css: f64, device_scale_factor: f64) -> bool {
    content_height_css * sanitize_scale(device_scale_factor)
        > f64::from(SINGLE_CAPTURE_MAX_DEVICE_PX)
}

/// Plan the tiles for a page of `content_width_css` by `content_height_css`.
///
/// The stitched height is the content height, cut to the first of: the content
/// limit (60,000 CSS px), the tile limit (16 tiles), and the pixel cap of the
/// format (40 Mpx PNG, 24 Mpx JPEG at the stitched width). `truncated` is set when
/// the cut removed content. Tiles are whole CSS pixels, contiguous, and at most
/// `tile_css_height` tall.
pub fn plan_tiles(
    content_width_css: f64,
    content_height_css: f64,
    device_scale_factor: f64,
    format: ImageFormat,
) -> QuarryResult<TilePlan> {
    let scale = sanitize_scale(device_scale_factor);
    let width_css = content_width_css.floor();
    let height_css = content_height_css.floor();
    if !(width_css.is_finite() && height_css.is_finite() && width_css >= 1.0 && height_css >= 1.0) {
        return Err(QuarryError::new(
            ErrorCode::DriverFailed,
            "page has no content area to capture",
        )
        .with_details(json!({ "reason": "no_content_area" })));
    }
    let width_px = f64::from(device_px(width_css, scale));
    let max_px: u64 = match format {
        ImageFormat::Png => MAX_STITCH_PX_PNG,
        ImageFormat::Jpeg => MAX_STITCH_PX_JPEG,
    };
    let max_px = max_px as f64;
    let by_pixels = (max_px / (width_px * scale)).floor();
    let tile_css = f64::from(tile_css_height(scale));
    let limit = f64::from(MAX_CONTENT_CSS_PX)
        .min(height_css)
        .min(f64::from(MAX_TILES) * tile_css)
        .min(by_pixels)
        .floor();
    if limit.is_nan() || limit < 1.0 {
        return Err(
            QuarryError::new(ErrorCode::DriverFailed, "the page is too wide to stitch")
                .with_details(json!({ "reason": "capture_too_large" })),
        );
    }
    let total = limit as u32;
    let step = tile_css as u32;
    let mut tiles = Vec::new();
    let mut y = 0;
    while y < total {
        let h = (total - y).min(step);
        tiles.push(Tile { y, h });
        y += h;
    }
    Ok(TilePlan {
        width_css: width_css as u32,
        tiles,
        css_height: total,
        truncated: f64::from(total) < height_css,
    })
}

/// Device rows one tile contributes to the stitched image.
pub fn tile_device_rows(tile: Tile, device_scale_factor: f64) -> u32 {
    device_px(f64::from(tile.y + tile.h), device_scale_factor)
        .saturating_sub(device_px(f64::from(tile.y), device_scale_factor))
}

/// The overall deadline for a stitch: 10 seconds a tile, at most 60.
pub fn stitch_deadline(tiles: usize) -> Duration {
    Duration::from_secs((tiles as u64).saturating_mul(10).min(60))
}

fn stitch_error(reason: &'static str, message: &'static str) -> QuarryError {
    QuarryError::new(ErrorCode::DriverFailed, message).with_details(json!({ "reason": reason }))
}

fn unreadable_tile() -> QuarryError {
    stitch_error(
        "screenshot_unreadable",
        "a stitched tile is not a readable PNG",
    )
}

fn tile_mismatch() -> QuarryError {
    stitch_error("stitch_tile_mismatch", "a stitched tile has the wrong size")
}

fn encode_failed() -> QuarryError {
    stitch_error(
        "stitch_encode_failed",
        "the stitched image could not be encoded",
    )
}

/// Decode one tile (a PNG from Chromium) to RGBA bytes for its first `rows` rows.
/// A tile with more rows than asked is cropped; a tile with fewer rows, another
/// width or another bit depth is refused.
pub fn decode_tile_rgba(tile_png: &[u8], width_px: u32, rows: u32) -> QuarryResult<Vec<u8>> {
    let limits = png::Limits {
        bytes: TILE_DECODE_BYTES,
    };
    let mut reader = png::Decoder::new_with_limits(Cursor::new(tile_png), limits)
        .read_info()
        .map_err(|_| unreadable_tile())?;
    let (tile_width, tile_height, color, depth) = {
        let info = reader.info();
        (info.width, info.height, info.color_type, info.bit_depth)
    };
    if tile_width != width_px || tile_height < rows {
        return Err(tile_mismatch());
    }
    if depth != png::BitDepth::Eight {
        return Err(unreadable_tile());
    }
    let channels = match color {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        _ => return Err(unreadable_tile()),
    };
    let mut buffer = vec![0u8; reader.output_buffer_size().ok_or_else(unreadable_tile)?];
    reader
        .next_frame(&mut buffer)
        .map_err(|_| unreadable_tile())?;
    let wanted = rows as usize * width_px as usize * channels;
    let source = buffer.get(..wanted).ok_or_else(tile_mismatch)?;
    if channels == 4 {
        return Ok(source.to_vec());
    }
    Ok(source
        .chunks_exact(3)
        .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
        .collect())
}

/// One RGBA pixel over a white page, for JPEG (which has no alpha).
fn over_white(pixel: &[u8]) -> [u8; 3] {
    let alpha = u16::from(pixel[3]);
    let blend = |value: u8| ((u16::from(value) * alpha + 255 * (255 - alpha)) / 255) as u8;
    [blend(pixel[0]), blend(pixel[1]), blend(pixel[2])]
}

/// A byte sink the PNG stream can own. The stream takes its writer by value, so the
/// bytes are shared with the stitcher, which moves them into the caller's buffer at
/// the end.
#[derive(Clone, Default)]
struct SharedBytes(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBytes {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut guard = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SharedBytes {
    fn take(&self) -> Vec<u8> {
        let mut guard = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::take(&mut *guard)
    }
}

enum Sink {
    /// The streamed PNG: image data goes into `bytes` one tile at a time.
    Png {
        stream: png::StreamWriter<'static, SharedBytes>,
        bytes: SharedBytes,
    },
    /// The JPEG is encoded at the end, from the collected RGB rows.
    Jpeg { rgb: Vec<u8> },
}

/// Writes the stitched image tile by tile, in page order.
pub struct Stitcher<'a> {
    out: &'a mut Vec<u8>,
    width_px: u32,
    height_px: u32,
    quality: u8,
    rows_written: u32,
    peak_tile_bytes: usize,
    sink: Sink,
}

impl<'a> Stitcher<'a> {
    /// Start a stitched image of `width_px` by `height_px` device pixels into `out`.
    /// `quality` is the JPEG quality (1 to 100) and is ignored for PNG.
    pub fn new(
        format: ImageFormat,
        out: &'a mut Vec<u8>,
        width_px: u32,
        height_px: u32,
        quality: u8,
    ) -> QuarryResult<Self> {
        if width_px == 0 || height_px == 0 {
            return Err(QuarryError::new(
                ErrorCode::DriverFailed,
                "page has no content area to capture",
            )
            .with_details(json!({ "reason": "no_content_area" })));
        }
        let sink = match format {
            ImageFormat::Png => {
                let bytes = SharedBytes::default();
                let mut encoder = png::Encoder::new(bytes.clone(), width_px, height_px);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let writer = encoder.write_header().map_err(|_| encode_failed())?;
                let stream = writer.into_stream_writer().map_err(|_| encode_failed())?;
                Sink::Png { stream, bytes }
            }
            ImageFormat::Jpeg => {
                // The JPEG encoder takes 16-bit dimensions.
                if width_px > u32::from(u16::MAX) || height_px > u32::from(u16::MAX) {
                    return Err(stitch_error(
                        "capture_too_large",
                        "the stitched JPEG is too large",
                    ));
                }
                Sink::Jpeg { rgb: Vec::new() }
            }
        };
        Ok(Self {
            out,
            width_px,
            height_px,
            quality,
            rows_written: 0,
            peak_tile_bytes: 0,
            sink,
        })
    }

    /// Add one tile: a PNG from Chromium whose first `rows` device rows are the
    /// next rows of the page.
    pub fn append_tile(&mut self, tile_png: &[u8], rows: u32) -> QuarryResult<()> {
        if rows == 0 || self.rows_written.saturating_add(rows) > self.height_px {
            return Err(tile_mismatch());
        }
        let rgba = decode_tile_rgba(tile_png, self.width_px, rows)?;
        self.peak_tile_bytes = self.peak_tile_bytes.max(rgba.len());
        match &mut self.sink {
            Sink::Png { stream, .. } => stream.write_all(&rgba).map_err(|_| encode_failed())?,
            Sink::Jpeg { rgb, .. } => {
                for pixel in rgba.chunks_exact(4) {
                    rgb.extend_from_slice(&over_white(pixel));
                }
            }
        }
        self.rows_written += rows;
        Ok(())
    }

    /// The most decoded tile bytes this writer held at once.
    pub fn peak_tile_bytes(&self) -> usize {
        self.peak_tile_bytes
    }

    /// Complete the image. Every row of the height must have been written.
    pub fn finish(self) -> QuarryResult<()> {
        if self.rows_written != self.height_px {
            return Err(tile_mismatch());
        }
        let Stitcher {
            out,
            sink,
            width_px,
            height_px,
            quality,
            ..
        } = self;
        match sink {
            Sink::Png { stream, bytes } => {
                stream.finish().map_err(|_| encode_failed())?;
                *out = bytes.take();
                Ok(())
            }
            Sink::Jpeg { rgb } => jpeg_encoder::Encoder::new(out, quality)
                .encode(
                    &rgb,
                    width_px as u16,
                    height_px as u16,
                    jpeg_encoder::ColorType::Rgb,
                )
                .map_err(|_| encode_failed()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [u8; 3] = [200, 30, 30];
    const GREEN: [u8; 3] = [30, 200, 30];
    const BLUE: [u8; 3] = [30, 30, 200];

    /// A solid RGB tile as Chromium would send it (PNG).
    fn solid_tile_png(width: u32, rows: u32, rgb: [u8; 3]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, rows);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("header");
        let data: Vec<u8> = (0..width * rows).flat_map(|_| rgb).collect();
        writer.write_image_data(&data).expect("data");
        writer.finish().expect("finish");
        bytes
    }

    /// Decode a PNG to (width, height, RGBA bytes).
    fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
        let mut reader = png::Decoder::new(Cursor::new(bytes))
            .read_info()
            .expect("png");
        let mut buffer = vec![0u8; reader.output_buffer_size().expect("size")];
        let frame = reader.next_frame(&mut buffer).expect("frame");
        let (width, height) = (reader.info().width, reader.info().height);
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        (width, height, buffer)
    }

    fn pixel_at(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * width + x) * 4) as usize;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    }

    #[test]
    fn tile_plan_covers_the_content_with_no_gap_or_overlap() {
        let plan = plan_tiles(1280.0, 25_000.0, 1.0, ImageFormat::Png).expect("plan");
        assert!(!plan.truncated);
        assert_eq!(plan.css_height, 25_000);
        assert_eq!(plan.width_css, 1280);
        let mut next = 0;
        for tile in &plan.tiles {
            assert_eq!(tile.y, next, "no gap or overlap");
            assert!(tile.h >= 1 && tile.h <= MAX_TILE_CSS_PX);
            next = tile.y + tile.h;
        }
        assert_eq!(next, 25_000, "the tiles reach the end of the content");
    }

    #[test]
    fn tile_plan_respects_max_tiles_and_marks_truncated() {
        let plan = plan_tiles(1280.0, 90_000.0, 1.0, ImageFormat::Png).expect("plan");
        assert!(plan.truncated);
        assert_eq!(plan.css_height, MAX_CONTENT_CSS_PX);
        assert!(plan.tiles.len() <= MAX_TILES as usize);
        let sum: u32 = plan.tiles.iter().map(|tile| tile.h).sum();
        assert_eq!(sum, plan.css_height);
    }

    #[test]
    fn dsf_two_halves_tile_css_height() {
        assert_eq!(tile_css_height(1.0), MAX_TILE_CSS_PX);
        // At device scale 2 a tile holds the same CSS height but twice the device rows,
        // and still fits one capture.
        let device_rows = tile_css_height(2.0) * 2;
        assert!(device_rows <= SINGLE_CAPTURE_MAX_DEVICE_PX);
        // 9,000 CSS px at scale 2 is 18,000 device px: one capture cannot hold it.
        assert!(needs_stitch(9_000.0, 2.0));
        assert!(!needs_stitch(7_000.0, 2.0));
        // Narrow enough to stay inside the pixel cap, so nothing is cut.
        let plan = plan_tiles(800.0, 9_000.0, 2.0, ImageFormat::Png).expect("plan");
        assert!(!plan.truncated);
        let rows: u32 = plan
            .tiles
            .iter()
            .map(|tile| tile_device_rows(*tile, 2.0))
            .sum();
        assert_eq!(rows, device_px(9_000.0, 2.0));
    }

    #[test]
    fn png_stitch_output_has_the_expected_dimensions_and_is_valid() {
        let width = 64;
        let mut out = Vec::new();
        {
            let mut stitcher =
                Stitcher::new(ImageFormat::Png, &mut out, width, 97, 0).expect("stitcher");
            stitcher
                .append_tile(&solid_tile_png(width, 40, RED), 40)
                .expect("red");
            stitcher
                .append_tile(&solid_tile_png(width, 40, GREEN), 40)
                .expect("green");
            stitcher
                .append_tile(&solid_tile_png(width, 17, BLUE), 17)
                .expect("blue");
            stitcher.finish().expect("finish");
        }
        assert_eq!(&out[..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        let (decoded_width, decoded_height, rgba) = decode_png(&out);
        assert_eq!((decoded_width, decoded_height), (width, 97));
        assert_eq!(pixel_at(&rgba, width, 3, 0)[..3], RED);
        assert_eq!(pixel_at(&rgba, width, 3, 40)[..3], GREEN);
        assert_eq!(pixel_at(&rgba, width, 3, 96)[..3], BLUE);
    }

    #[test]
    fn streaming_writer_memory_is_bounded() {
        // Sixteen tiles of 1,024 by 1,024 RGBA (4 MiB each) go through one writer.
        let tile_rows = 1024;
        let width = 1024;
        let tile = solid_tile_png(width, tile_rows, RED);
        let mut out = Vec::new();
        let mut stitcher =
            Stitcher::new(ImageFormat::Png, &mut out, width, tile_rows * 16, 0).expect("stitcher");
        for _ in 0..16 {
            stitcher.append_tile(&tile, tile_rows).expect("tile");
        }
        let peak = stitcher.peak_tile_bytes();
        stitcher.finish().expect("finish");
        // One decoded tile at a time, never two held at once.
        assert_eq!(peak, 4 * 1024 * 1024);
    }

    #[test]
    fn jpeg_stitch_cap_is_lower_than_png() {
        assert!(MAX_STITCH_PX_JPEG < MAX_STITCH_PX_PNG);
        // 20,000 CSS px at 1,280 wide is 25.6 Mpx: inside the PNG cap, outside the JPEG cap.
        let png = plan_tiles(1280.0, 20_000.0, 1.0, ImageFormat::Png).expect("png");
        assert!(!png.truncated);
        assert_eq!(png.css_height, 20_000);
        let jpeg = plan_tiles(1280.0, 20_000.0, 1.0, ImageFormat::Jpeg).expect("jpeg");
        assert!(jpeg.truncated);
        assert!(u64::from(jpeg.css_height) * 1280 <= MAX_STITCH_PX_JPEG);
    }

    #[test]
    fn deadline_is_min_of_tiles_times_10s_and_60s() {
        assert_eq!(stitch_deadline(1), Duration::from_secs(10));
        assert_eq!(stitch_deadline(3), Duration::from_secs(30));
        assert_eq!(stitch_deadline(16), Duration::from_secs(60));
        assert_eq!(stitch_deadline(usize::MAX), Duration::from_secs(60));
    }

    #[test]
    fn a_tile_that_is_not_a_png_or_has_the_wrong_size_is_refused() {
        let mut out = Vec::new();
        let mut stitcher = Stitcher::new(ImageFormat::Png, &mut out, 32, 20, 0).expect("stitcher");
        let not_png = stitcher.append_tile(b"not a png", 10).unwrap_err();
        assert_eq!(
            not_png.details.as_ref().unwrap()["reason"],
            "screenshot_unreadable"
        );
        let short = stitcher
            .append_tile(&solid_tile_png(32, 5, RED), 10)
            .unwrap_err();
        assert_eq!(
            short.details.as_ref().unwrap()["reason"],
            "stitch_tile_mismatch"
        );
        let early_finish = stitcher.finish().unwrap_err();
        assert_eq!(
            early_finish.details.as_ref().unwrap()["reason"],
            "stitch_tile_mismatch"
        );
    }
}
