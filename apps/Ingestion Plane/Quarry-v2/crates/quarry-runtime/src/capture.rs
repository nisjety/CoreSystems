//! Page captures carried beside a fetch response (WS13.4a).
//!
//! A `FetchResponse` may carry the screenshot, PDF, action outcomes and
//! capture warnings that a browser fetch produced. The browser driver fills it
//! when a scrape asks for a capture (WS13.4b); every other driver leaves it
//! `None`. Captured bytes are held in memory until the response is built and
//! are never serialized by this type.

use quarry_core::capture::{CapturedImage, CapturedPdf};
use quarry_core::scrape_actions::ActionOutcome;
use url::Url;

/// Bytes an action produced (for example a screenshot taken by an action),
/// held until the response is built. Never serialized.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ActionOutputBytes {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// A non-fatal capture problem. A capture failure is a warning and never an
/// error: the extracted page stays valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureWarning {
    pub code: String,
    pub message: String,
}

/// Everything a browser fetch captured beside the page body.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageCaptures {
    /// The page screenshot, when the request asked for one.
    pub screenshot: Option<CapturedImage>,
    /// The page PDF, when the request asked for one.
    pub pdf: Option<CapturedPdf>,
    /// Metadata for each action, in request order.
    pub actions: Vec<ActionOutcome>,
    /// Output bytes per action, index-aligned with `actions`.
    pub action_outputs: Vec<Option<ActionOutputBytes>>,
    /// The URL the browser ended on, when it differs from the fetch target.
    pub final_url: Option<Url>,
    /// Requests the egress policy blocked while the page loaded.
    pub blocked_requests: u32,
    /// Non-fatal capture problems, for example `screenshot_failed`.
    pub warnings: Vec<CaptureWarning>,
}

/// WS13.4b: the default inline cap for capture bytes returned under Zero Data
/// Retention (6 MiB, the model gateway's screenshot cap). Nothing is stored
/// under ZDR, so the bytes travel in the response; an over-cap capture is
/// omitted with `capture_exceeds_inline_cap`.
pub const DEFAULT_INLINE_CAPTURE_MAX_BYTES: usize = 6 * 1024 * 1024;

/// The highest inline cap an operator can set (16 MiB). A larger setting is
/// clamped down to this.
pub const MAX_INLINE_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
