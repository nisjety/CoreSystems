//! Capture options for scrape-time screenshots and PDFs (WS13.2).
//!
//! Types, defaults and validation only: no driver, no I/O. The driver that
//! takes the captures (WS13.3) and the request wiring (WS13.4b) come later.
//! [`CapturedImage`] and [`CapturedPdf`] hold bytes and are never serialized.
//!
//! The wire names are camelCase (`fullPage`, `maxPages`, ...). Every object
//! refuses unknown keys, so a misspelt option is a field error, not a silent
//! default.

use serde::{Deserialize, Serialize};

use crate::scrape::FieldError;

/// At most this many captures per request, counting `formats` and `actions`.
pub const MAX_CAPTURES: usize = 8;
/// Encoded size cap for one capture, in bytes.
pub const MAX_CAPTURE_BYTES: usize = 32 * 1024 * 1024;
/// Largest viewport a capture may ask for, in CSS pixels.
pub const MAX_VIEWPORT_WIDTH: u32 = 3840;
pub const MAX_VIEWPORT_HEIGHT: u32 = 2160;
/// Device scale factor range accepted for a viewport.
pub const MIN_DEVICE_SCALE_FACTOR: f64 = 1.0;
pub const MAX_DEVICE_SCALE_FACTOR: f64 = 2.0;
/// Device pixels one Chromium capture may hold. Provisional until the
/// WS13.3 spike sets the measured limit for the pinned build.
pub const SINGLE_CAPTURE_MAX_DEVICE_PX: u32 = 16_000;
/// Tile limits for pages taller than one capture (WS13.6 uses these).
pub const MAX_TILE_CSS_PX: u32 = 4096;
pub const MAX_TILES: u32 = 16;
pub const MAX_CONTENT_CSS_PX: u32 = 60_000;
pub const MAX_STITCH_PX_PNG: u64 = 40_000_000;
pub const MAX_STITCH_PX_JPEG: u64 = 24_000_000;
/// PDF page limits: `maxPages` is 1 to 200, 100 by default.
pub const MAX_PDF_PAGES: u32 = 200;
pub const DEFAULT_PDF_MAX_PAGES: u32 = 100;
/// PDF scale range, 0.1 to 2.0, 1.0 by default.
pub const MIN_PDF_SCALE: f64 = 0.1;
pub const MAX_PDF_SCALE: f64 = 2.0;

/// The caps a request runs under. The environment can lower them; it never
/// raises them above the built-in values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureLimits {
    pub max_captures: usize,
    pub max_capture_bytes: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_captures: MAX_CAPTURES,
            max_capture_bytes: MAX_CAPTURE_BYTES,
        }
    }
}

impl CaptureLimits {
    /// Lower the caps to the given values. A value above the built-in cap is
    /// ignored, so an environment setting cannot widen the contract.
    pub fn lowered(max_captures: Option<usize>, max_capture_bytes: Option<usize>) -> Self {
        let built_in = Self::default();
        Self {
            max_captures: max_captures.map_or(built_in.max_captures, |value| {
                value.min(built_in.max_captures)
            }),
            max_capture_bytes: max_capture_bytes.map_or(built_in.max_capture_bytes, |value| {
                value.min(built_in.max_capture_bytes)
            }),
        }
    }
}

/// Encoding of a screenshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
}

/// A rectangle in CSS pixels of the page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ClipRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ClipRect {
    fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.width.is_finite() && self.height.is_finite()
    }

    /// The request-level check: finite, `x` and `y` at least 0, a positive
    /// width and height.
    pub fn validate(&self, path: &str) -> Result<(), FieldError> {
        if !self.is_finite() || self.x < 0.0 || self.y < 0.0 || self.width <= 0.0 || self.height <= 0.0
        {
            return Err(FieldError::new(
                path,
                "invalid_clip",
                "a clip needs finite x and y of at least 0 and a positive width and height",
            ));
        }
        Ok(())
    }

    /// The part of this clip inside a content area of `content_width` by
    /// `content_height` CSS pixels, or `None` when nothing is left.
    pub fn intersect_content(&self, content_width: f64, content_height: f64) -> Option<ClipRect> {
        if !self.is_finite() {
            return None;
        }
        let x0 = self.x.max(0.0);
        let y0 = self.y.max(0.0);
        let x1 = (self.x + self.width).min(content_width);
        let y1 = (self.y + self.height).min(content_height);
        if x1 - x0 <= 0.0 || y1 - y0 <= 0.0 {
            return None;
        }
        Some(ClipRect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    /// [`ClipRect::intersect_content`] as a field error: a clip that lies
    /// outside the content is `invalid_clip`, not an empty image.
    pub fn within_content(
        &self,
        content_width: f64,
        content_height: f64,
        path: &str,
    ) -> Result<ClipRect, FieldError> {
        self.intersect_content(content_width, content_height)
            .ok_or_else(|| {
                FieldError::new(
                    path,
                    "invalid_clip",
                    "the clip lies outside the page content",
                )
            })
    }
}

/// A viewport override for one capture. Applied at capture time, not on the
/// lease (WS13.4b).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureViewport {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_device_scale_factor")]
    pub device_scale_factor: f64,
}

fn default_device_scale_factor() -> f64 {
    MIN_DEVICE_SCALE_FACTOR
}

impl CaptureViewport {
    pub fn validate(&self, path: &str) -> Result<(), FieldError> {
        if self.width == 0 || self.height == 0 {
            return Err(FieldError::new(
                path,
                "out_of_range",
                "a viewport needs a positive width and height",
            ));
        }
        if self.width > MAX_VIEWPORT_WIDTH || self.height > MAX_VIEWPORT_HEIGHT {
            return Err(FieldError::new(
                path,
                "viewport_too_large",
                format!("a viewport is at most {MAX_VIEWPORT_WIDTH} by {MAX_VIEWPORT_HEIGHT}"),
            ));
        }
        if !(MIN_DEVICE_SCALE_FACTOR..=MAX_DEVICE_SCALE_FACTOR).contains(&self.device_scale_factor)
        {
            return Err(FieldError::new(
                format!("{path}.deviceScaleFactor"),
                "device_scale_factor_out_of_range",
                "the device scale factor is 1 to 2",
            ));
        }
        Ok(())
    }
}

/// Options for one screenshot. All defaults is the legacy PNG of the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScreenshotOptions {
    #[serde(default)]
    pub full_page: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<ClipRect>,
    /// JPEG quality 1 to 100. Setting it selects JPEG; absent selects PNG.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewport: Option<CaptureViewport>,
}

impl ScreenshotOptions {
    /// JPEG if a quality is set, PNG otherwise.
    pub fn format(&self) -> ImageFormat {
        if self.quality.is_some() {
            ImageFormat::Jpeg
        } else {
            ImageFormat::Png
        }
    }

    /// True when every option is at its default: the existing viewport
    /// screenshot serves it unchanged.
    pub fn is_legacy(&self) -> bool {
        *self == Self::default()
    }

    /// Field checks; `path` names the format or action entry the options sit in.
    pub fn validate(&self, path: &str) -> Result<(), FieldError> {
        if let Some(clip) = &self.clip {
            clip.validate(&format!("{path}.clip"))?;
        }
        if let Some(quality) = self.quality {
            if !(1..=100).contains(&quality) {
                return Err(FieldError::new(
                    format!("{path}.quality"),
                    "quality_out_of_range",
                    "the JPEG quality is 1 to 100",
                ));
            }
        }
        if let Some(viewport) = &self.viewport {
            viewport.validate(&format!("{path}.viewport"))?;
        }
        Ok(())
    }
}

/// Paper sizes for a PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub enum PdfPaper {
    #[default]
    Letter,
    A3,
    A4,
    Legal,
    Tabloid,
}

impl PdfPaper {
    /// Width and height in inches, portrait.
    pub fn inches(self) -> (f64, f64) {
        match self {
            Self::Letter => (8.5, 11.0),
            Self::A3 => (11.69, 16.54),
            Self::A4 => (8.27, 11.69),
            Self::Legal => (8.5, 14.0),
            Self::Tabloid => (11.0, 17.0),
        }
    }
}

fn default_print_background() -> bool {
    true
}

fn default_scale() -> f64 {
    1.0
}

fn default_max_pages() -> u32 {
    DEFAULT_PDF_MAX_PAGES
}

/// Options for one PDF of the page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PdfOptions {
    #[serde(default)]
    pub format: PdfPaper,
    #[serde(default)]
    pub landscape: bool,
    #[serde(default = "default_print_background")]
    pub print_background: bool,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default = "default_max_pages")]
    pub max_pages: u32,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            format: PdfPaper::default(),
            landscape: false,
            print_background: default_print_background(),
            scale: default_scale(),
            max_pages: default_max_pages(),
        }
    }
}

impl PdfOptions {
    /// True when every option is at its default.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self, path: &str) -> Result<(), FieldError> {
        if !self.scale.is_finite() || !(MIN_PDF_SCALE..=MAX_PDF_SCALE).contains(&self.scale) {
            return Err(FieldError::new(
                format!("{path}.scale"),
                "invalid_scale",
                "the PDF scale is 0.1 to 2",
            ));
        }
        if !(1..=MAX_PDF_PAGES).contains(&self.max_pages) {
            return Err(FieldError::new(
                format!("{path}.maxPages"),
                "invalid_max_pages",
                "maxPages is 1 to 200",
            ));
        }
        Ok(())
    }
}

/// What a request captures from the page, after the formats are read: at most
/// one screenshot and one PDF. Actions add their own captures on top.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CapturePlan {
    pub screenshot: Option<ScreenshotOptions>,
    pub pdf: Option<PdfOptions>,
}

impl CapturePlan {
    pub fn is_empty(&self) -> bool {
        self.screenshot.is_none() && self.pdf.is_none()
    }

    pub fn count(&self) -> usize {
        usize::from(self.screenshot.is_some()) + usize::from(self.pdf.is_some())
    }
}

/// A captured image, held as bytes until the response is built. Never serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedImage {
    pub bytes: Vec<u8>,
    pub format: ImageFormat,
    pub width_px: u32,
    pub height_px: u32,
    pub tiles: u32,
    pub stitched: bool,
    pub truncated: bool,
}

/// A captured PDF, held as bytes until the response is built. Never serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedPdf {
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(x: f64, y: f64, width: f64, height: f64) -> ClipRect {
        ClipRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn quality_selects_jpeg_and_absence_selects_png() {
        let png = ScreenshotOptions::default();
        assert_eq!(png.format(), ImageFormat::Png);
        assert!(png.is_legacy());
        let jpeg = ScreenshotOptions {
            quality: Some(80),
            ..ScreenshotOptions::default()
        };
        assert_eq!(jpeg.format(), ImageFormat::Jpeg);
        assert!(!jpeg.is_legacy());
        assert!(jpeg.validate("formats[1]").is_ok());
    }

    #[test]
    fn clip_outside_content_is_invalid_clip() {
        // Inside the content: kept as it is.
        let inside = clip(10.0, 10.0, 100.0, 50.0);
        assert_eq!(inside.within_content(1000.0, 1000.0, "formats[0].clip"), Ok(inside));
        // Partly outside: cut to the content.
        let cut = clip(900.0, 0.0, 200.0, 10.0).intersect_content(1000.0, 1000.0);
        assert_eq!(cut, Some(clip(900.0, 0.0, 100.0, 10.0)));
        // Entirely outside: an error with the path, not an empty image.
        let error = clip(5000.0, 5000.0, 10.0, 10.0)
            .within_content(1000.0, 1000.0, "actions[2].clip")
            .unwrap_err();
        assert_eq!(error.code, "invalid_clip");
        assert_eq!(error.path, "actions[2].clip");
        // Negative origin is refused by the request check.
        let negative = clip(-1.0, 0.0, 10.0, 10.0).validate("formats[0].clip").unwrap_err();
        assert_eq!(negative.code, "invalid_clip");
        assert!(clip(0.0, 0.0, 0.0, 10.0).validate("x").is_err());
    }

    #[test]
    fn viewport_over_cap_is_viewport_too_large() {
        let ok = CaptureViewport {
            width: 3840,
            height: 2160,
            device_scale_factor: 2.0,
        };
        assert!(ok.validate("formats[0].viewport").is_ok());
        let wide = CaptureViewport {
            width: 3841,
            height: 2160,
            device_scale_factor: 1.0,
        };
        assert_eq!(
            wide.validate("formats[0].viewport").unwrap_err().code,
            "viewport_too_large"
        );
        let dense = CaptureViewport {
            width: 800,
            height: 600,
            device_scale_factor: 2.5,
        };
        assert_eq!(
            dense.validate("formats[0].viewport").unwrap_err().code,
            "device_scale_factor_out_of_range"
        );
        let zero = CaptureViewport {
            width: 0,
            height: 600,
            device_scale_factor: 1.0,
        };
        assert_eq!(zero.validate("x").unwrap_err().code, "out_of_range");
    }

    #[test]
    fn quality_out_of_range_is_refused() {
        let bad = ScreenshotOptions {
            quality: Some(0),
            ..ScreenshotOptions::default()
        };
        assert_eq!(bad.validate("actions[0]").unwrap_err().code, "quality_out_of_range");
        let top = ScreenshotOptions {
            quality: Some(100),
            ..ScreenshotOptions::default()
        };
        assert!(top.validate("actions[0]").is_ok());
    }

    #[test]
    fn pdf_defaults_print_background_true() {
        let defaults = PdfOptions::default();
        assert!(defaults.print_background);
        assert!(defaults.is_default());
        assert_eq!(defaults.scale, 1.0);
        assert_eq!(defaults.max_pages, 100);
        assert_eq!(defaults.format, PdfPaper::Letter);
        assert!(defaults.validate("formats[0]").is_ok());
        // A caller can turn it off, and that is no longer the default.
        let off: PdfOptions = serde_json::from_value(serde_json::json!({ "printBackground": false }))
            .expect("a valid option");
        assert!(!off.print_background);
        assert!(!off.is_default());
    }

    #[test]
    fn pdf_paper_table_matches_chromium_inches() {
        assert_eq!(PdfPaper::Letter.inches(), (8.5, 11.0));
        assert_eq!(PdfPaper::A4.inches(), (8.27, 11.69));
        assert_eq!(PdfPaper::Tabloid.inches(), (11.0, 17.0));
    }

    #[test]
    fn pdf_scale_and_pages_have_ranges() {
        let scale = PdfOptions {
            scale: 2.5,
            ..PdfOptions::default()
        };
        assert_eq!(scale.validate("formats[0]").unwrap_err().code, "invalid_scale");
        let pages = PdfOptions {
            max_pages: 201,
            ..PdfOptions::default()
        };
        assert_eq!(pages.validate("formats[0]").unwrap_err().code, "invalid_max_pages");
        let zero_pages = PdfOptions {
            max_pages: 0,
            ..PdfOptions::default()
        };
        assert_eq!(zero_pages.validate("formats[0]").unwrap_err().code, "invalid_max_pages");
    }

    #[test]
    fn lowered_limits_never_rise_above_the_built_in_caps() {
        let built_in = CaptureLimits::default();
        assert_eq!(built_in.max_captures, 8);
        assert_eq!(built_in.max_capture_bytes, 32 * 1024 * 1024);
        assert_eq!(CaptureLimits::lowered(Some(2), None).max_captures, 2);
        assert_eq!(CaptureLimits::lowered(Some(99), None), built_in);
        assert_eq!(CaptureLimits::lowered(None, Some(usize::MAX)), built_in);
    }

    #[test]
    fn capture_plan_counts_its_parts() {
        assert!(CapturePlan::default().is_empty());
        let plan = CapturePlan {
            screenshot: Some(ScreenshotOptions::default()),
            pdf: Some(PdfOptions::default()),
        };
        assert_eq!(plan.count(), 2);
        assert!(!plan.is_empty());
    }
}
