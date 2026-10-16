//! The `/v1/scrape` response: a [`NormalizedOutput`] plus what cannot be a
//! stored artifact.
//!
//! Output formats are artifact references, and a Zero-Data-Retention scrape
//! writes no artifacts, so under ZDR the extracted text used to be unreachable:
//! the response named nothing and held nothing. [`ScrapeResponse`] carries the
//! text inline in that case (and only that case, in this version). The
//! `NormalizedOutput` fields stay at the top level, so every existing client
//! keeps working.

use serde::{Deserialize, Serialize};

use crate::output::NormalizedOutput;

/// Inline text is capped so a ZDR response stays a reasonable size; the flag
/// [`ScrapeResponse::truncated`] says when the cap applied.
pub const DEFAULT_INLINE_MAX_BYTES: usize = 1_048_576;

/// A non-fatal note about the result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeWarning {
    /// Stable machine-readable code, e.g. `document_has_no_text_layer`.
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeResponse {
    #[serde(flatten)]
    pub output: NormalizedOutput,
    /// The extracted text, inline. Present only when nothing is stored for
    /// this run (ZDR); otherwise read it from `formats.markdown`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    /// `markdown` was cut at [`DEFAULT_INLINE_MAX_BYTES`].
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<ScrapeWarning>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ScrapeResponse {
    /// A response with no inline parts (a cache hit, or a non-ZDR run).
    pub fn from_output(output: NormalizedOutput) -> Self {
        Self {
            output,
            markdown: None,
            truncated: false,
            warnings: Vec::new(),
        }
    }
}

/// Cut `text` to at most `max_bytes` on a character boundary. Returns the text
/// and whether anything was removed.
pub fn truncate_on_char_boundary(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncation_respects_char_boundaries() {
        let (cut, truncated) = truncate_on_char_boundary("aæøå", 3);
        assert!(truncated);
        assert_eq!(cut, "aæ");
        let (whole, truncated) = truncate_on_char_boundary("abc", 3);
        assert!(!truncated);
        assert_eq!(whole, "abc");
        let (empty, truncated) = truncate_on_char_boundary("ææ", 1);
        assert!(truncated);
        assert_eq!(empty, "");
    }
}
