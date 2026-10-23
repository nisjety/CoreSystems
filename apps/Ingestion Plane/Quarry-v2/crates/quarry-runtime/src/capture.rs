//! Page captures carried beside a fetch response (WS13.4a).
//!
//! A `FetchResponse` may carry the screenshot, PDF, action outcomes and
//! capture warnings that a browser fetch produced. This module only defines
//! the carrier. Nothing fills it yet: the field defaults to `None` for every
//! driver, and the browser capture (WS13.3) and the scrape formats wiring
//! (WS13.4b) come later. Captured bytes are held in memory until the response
//! is built and are never serialized by this type.

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
