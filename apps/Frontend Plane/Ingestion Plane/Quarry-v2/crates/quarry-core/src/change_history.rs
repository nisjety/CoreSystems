//! Versioned change history — wire shapes.
//!
//! Cycle 26 / cluster #9.
//!
//! Quarry tracks page versions by `(org_id, source_url)` and emits a
//! `ChangeRecord` whenever a fresh fetch produces a different
//! fingerprint than the last baseline. Frontends + webhooks consume
//! these records to render "what changed" UI without re-fetching.
//!
//! ## Lifecycle
//!
//! ```text
//! /v1/change/check  (POST {url})           → re-fetch, compare to baseline
//! /v1/change/latest (GET ?url=)            → most-recent baseline
//! /v1/change/history (GET ?url=&limit=)    → ordered list of baselines
//! ```
//!
//! ## Identity
//!
//! - `baseline_id` = `bln_<ulid>`.
//! - `(org_id, source_url)` is the natural key for "this URL's
//!   latest baseline".

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::kinds;

/// One captured version of a tracked URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineSnapshot {
    pub baseline_id: String,
    pub org_id: String,
    pub source_url: String,
    /// blake3 of the canonicalized content. Equality on this implies
    /// the content didn't materially change.
    pub fingerprint: String,
    /// The `ArtifactKind` that holds the raw bytes (markdown / html).
    /// Optional because some baselines are metadata-only snapshots
    /// for "URL exists but couldn't be fetched on this pass".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<kinds::ArtifactKind>,
    /// Pointer to the previous baseline, if any. Forms a singly-linked
    /// chain so consumers can walk the history without a separate
    /// LIST query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_baseline_id: Option<String>,
    pub captured_at: DateTime<Utc>,
    /// Run that produced this baseline. `None` for manual operator
    /// imports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<kinds::RunKind>,
}

/// Result of a `POST /v1/change/check` — fresh fetch versus the
/// previous baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub source_url: String,
    pub org_id: String,
    pub status: ChangeStatus,
    /// The newly-captured baseline. `None` when the URL was
    /// unreachable on this check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_baseline: Option<BaselineSnapshot>,
    /// The previous baseline (or `None` if this is the first time
    /// we've seen the URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_baseline: Option<BaselineSnapshot>,
    /// Pointer into the diff store. Present only for `status=Changed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_id: Option<String>,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    /// First time we've seen this URL. `new_baseline` set;
    /// `prev_baseline` is `None`.
    New,
    /// Fresh fetch hash matches the previous baseline.
    Unchanged,
    /// Hashes differ; `diff_id` points to a `DiffRecord`.
    Changed,
    /// URL was unreachable on this check. `new_baseline` is `None`.
    Unreachable,
}

/// Persisted diff between two baselines. Computed on the fly when a
/// `Changed` check happens; cached so the frontend can fetch it
/// cheaply on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffRecord {
    pub diff_id: String,
    pub org_id: String,
    pub from_baseline_id: String,
    pub to_baseline_id: String,
    pub source_url: String,
    /// `"text" | "markdown" | "html" | "json-patch"`.
    pub format: String,
    pub artifact_id: kinds::ArtifactKind,
    /// Compact text summary (≤ 1KB) the dashboard renders without
    /// downloading the full diff artifact.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ============================================================================
// INNH-4: the text diff between two baselines.
//
// A monitored URL's markdown is stored as an artifact on every new or changed
// baseline (`BaselineSnapshot.artifact_id`). A change also stores a JSON
// `TextDiffArtifact` (artifact kind `change_diff`) that `DiffRecord.artifact_id`
// points at, and `GET /v1/change/diff` answers a `ChangeDiffView`. All of it is
// behind `QUARRY_EDGE__CHANGE_TEXT_DIFF` and none of it exists under ZDR.
// ============================================================================

/// `TextDiffArtifact.version`.
pub const TEXT_DIFF_VERSION: u32 = 1;
/// `DiffRecord.format` of a stored text diff.
pub const TEXT_DIFF_FORMAT: &str = "json";
/// `ParagraphDiff.format`.
pub const PARAGRAPH_DIFF_FORMAT: &str = "paragraph";
/// Most ops a diff keeps; the counts stay complete and `truncated` is set.
pub const TEXT_DIFF_MAX_OPS: usize = 500;
/// Most bytes of a paragraph an op keeps (cut on a character boundary).
pub const TEXT_DIFF_MAX_OP_TEXT_BYTES: usize = 2048;
/// Most bytes of one page's markdown read for a diff or returned by `include=markdown`.
pub const TEXT_DIFF_MAX_MARKDOWN_BYTES: usize = 1 << 20;
/// Longest paragraph the diff treats as one unit (longer ones are cut into units).
pub const TEXT_DIFF_PARAGRAPH_CHARS: usize = 8192;
/// `DiffRecord.summary` is at most this many bytes.
pub const DIFF_SUMMARY_MAX_BYTES: usize = 1024;

/// One paragraph-level change. `index` is the paragraph's position in the new text
/// (`added`) or the old text (`removed`); `moved` carries both positions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TextDiffOp {
    Added { index: usize, text: String },
    Removed { index: usize, text: String },
    Moved { from: usize, to: usize },
}

/// The paragraph diff of two pages of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParagraphDiff {
    /// Always `paragraph`.
    pub format: String,
    /// False when nothing was added or removed (a pure reorder, a reflow, or a
    /// raw-byte change that left the text alone: an ad, a timestamp in markup).
    pub real_delta: bool,
    pub added: usize,
    pub removed: usize,
    pub moved: usize,
    pub unchanged: usize,
    /// At most `TEXT_DIFF_MAX_OPS`, each text at most `TEXT_DIFF_MAX_OP_TEXT_BYTES`.
    pub ops: Vec<TextDiffOp>,
    /// True when an op was dropped or a text was cut, or an input was cut at
    /// `TEXT_DIFF_MAX_MARKDOWN_BYTES`.
    pub truncated: bool,
}

/// Why a diff has no text to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffUnavailable {
    /// The URL has one baseline only.
    NoPreviousVersion,
    /// The older baseline was recorded before text was stored (or with the flag off).
    NoPreviousText,
    /// The newer baseline has no stored text.
    NoText,
    /// A baseline's artifact is gone (evicted by retention) or unreadable.
    ArtifactGone,
}

impl DiffUnavailable {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoPreviousVersion => "no_previous_version",
            Self::NoPreviousText => "no_previous_text",
            Self::NoText => "no_text",
            Self::ArtifactGone => "artifact_gone",
        }
    }
}

/// The JSON stored under artifact kind `change_diff` for a `Changed` check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextDiffArtifact {
    pub version: u32,
    pub source_url: String,
    pub from_baseline_id: String,
    pub to_baseline_id: String,
    pub from_fingerprint: String,
    pub to_fingerprint: String,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DiffUnavailable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<ParagraphDiff>,
}

/// One end of a comparison, as `GET /v1/change/diff` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffSide {
    pub baseline_id: String,
    pub fingerprint: String,
    pub captured_at: DateTime<Utc>,
    /// The stored markdown; `null` for a baseline recorded without text.
    pub artifact_id: Option<String>,
}

/// The two pages' markdown, only with `include=markdown`. Each is cut at
/// `TEXT_DIFF_MAX_MARKDOWN_BYTES`; `null` when that side has no readable text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffMarkdown {
    pub from: Option<String>,
    pub to: Option<String>,
    pub truncated: bool,
}

/// The data of `GET /v1/change/diff`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeDiffView {
    pub source_url: String,
    /// The older side; `null` when the URL has one baseline only.
    pub from: Option<DiffSide>,
    pub to: DiffSide,
    /// The stored diff record, `null` when the pair has none (the diff was computed for
    /// this request).
    pub diff_id: Option<String>,
    pub available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DiffUnavailable>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<ParagraphDiff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<DiffMarkdown>,
}

/// Cuts `text` to at most `max` bytes on a character boundary. The flag says whether it was cut.
pub fn cut_utf8(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_status_json_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&ChangeStatus::New).unwrap(),
            "\"new\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeStatus::Changed).unwrap(),
            "\"changed\""
        );
        assert_eq!(
            serde_json::to_string(&ChangeStatus::Unreachable).unwrap(),
            "\"unreachable\""
        );
    }

    #[test]
    fn baseline_omits_optional_fields_when_none() {
        let b = BaselineSnapshot {
            baseline_id: "bln_x".into(),
            org_id: "org_a".into(),
            source_url: "https://example.com".into(),
            fingerprint: "blake3:abc".into(),
            artifact_id: None,
            prev_baseline_id: None,
            captured_at: Utc::now(),
            run_id: None,
        };
        let s = serde_json::to_string(&b).unwrap();
        // Pin omission for None fields.
        assert!(!s.contains("\"artifact_id\""));
        assert!(!s.contains("\"prev_baseline_id\""));
        assert!(!s.contains("\"run_id\""));
        // Required fields present.
        assert!(s.contains("\"baseline_id\":\"bln_x\""));
        assert!(s.contains("\"fingerprint\":\"blake3:abc\""));
    }

    #[test]
    fn change_record_roundtrips_through_json() {
        let now = Utc::now();
        let cr = ChangeRecord {
            source_url: "https://x/".into(),
            org_id: "org_a".into(),
            status: ChangeStatus::Changed,
            new_baseline: Some(BaselineSnapshot {
                baseline_id: "bln_new".into(),
                org_id: "org_a".into(),
                source_url: "https://x/".into(),
                fingerprint: "blake3:new".into(),
                artifact_id: None,
                prev_baseline_id: Some("bln_old".into()),
                captured_at: now,
                run_id: None,
            }),
            prev_baseline: None,
            diff_id: Some("diff_1".into()),
            checked_at: now,
        };
        let s = serde_json::to_string(&cr).unwrap();
        let back: ChangeRecord = serde_json::from_str(&s).unwrap();
        assert_eq!(back.status, ChangeStatus::Changed);
        assert_eq!(back.diff_id.as_deref(), Some("diff_1"));
        assert_eq!(
            back.new_baseline.unwrap().prev_baseline_id.as_deref(),
            Some("bln_old")
        );
    }

    #[test]
    fn the_stored_text_diff_pins_its_field_names() {
        let artifact = TextDiffArtifact {
            version: TEXT_DIFF_VERSION,
            source_url: "https://x/".into(),
            from_baseline_id: "bln_a".into(),
            to_baseline_id: "bln_b".into(),
            from_fingerprint: "blake3:a".into(),
            to_fingerprint: "blake3:b".into(),
            available: true,
            reason: None,
            diff: Some(ParagraphDiff {
                format: PARAGRAPH_DIFF_FORMAT.into(),
                real_delta: true,
                added: 1,
                removed: 1,
                moved: 1,
                unchanged: 2,
                ops: vec![
                    TextDiffOp::Added {
                        index: 3,
                        text: "new".into(),
                    },
                    TextDiffOp::Removed {
                        index: 1,
                        text: "old".into(),
                    },
                    TextDiffOp::Moved { from: 0, to: 2 },
                ],
                truncated: false,
            }),
        };
        let v = serde_json::to_value(&artifact).unwrap();
        assert_eq!(
            v,
            serde_json::json!({
                "version": 1,
                "source_url": "https://x/",
                "from_baseline_id": "bln_a",
                "to_baseline_id": "bln_b",
                "from_fingerprint": "blake3:a",
                "to_fingerprint": "blake3:b",
                "available": true,
                "diff": {
                    "format": "paragraph",
                    "real_delta": true,
                    "added": 1, "removed": 1, "moved": 1, "unchanged": 2,
                    "ops": [
                        {"op": "added", "index": 3, "text": "new"},
                        {"op": "removed", "index": 1, "text": "old"},
                        {"op": "moved", "from": 0, "to": 2}
                    ],
                    "truncated": false
                }
            })
        );
        let back: TextDiffArtifact = serde_json::from_value(v).unwrap();
        assert_eq!(back, artifact);
    }

    #[test]
    fn an_unavailable_diff_names_a_closed_reason() {
        for (reason, wire) in [
            (DiffUnavailable::NoPreviousVersion, "no_previous_version"),
            (DiffUnavailable::NoPreviousText, "no_previous_text"),
            (DiffUnavailable::NoText, "no_text"),
            (DiffUnavailable::ArtifactGone, "artifact_gone"),
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                serde_json::json!(wire)
            );
            assert_eq!(reason.as_str(), wire);
        }
    }

    #[test]
    fn the_diff_view_keeps_null_sides_and_omits_absent_parts() {
        let view = ChangeDiffView {
            source_url: "https://x/".into(),
            from: None,
            to: DiffSide {
                baseline_id: "bln_b".into(),
                fingerprint: "blake3:b".into(),
                captured_at: Utc::now(),
                artifact_id: None,
            },
            diff_id: None,
            available: false,
            reason: Some(DiffUnavailable::NoPreviousVersion),
            diff: None,
            markdown: None,
        };
        let v = serde_json::to_value(&view).unwrap();
        assert!(v["from"].is_null());
        assert!(v["diff_id"].is_null());
        assert!(v["to"]["artifact_id"].is_null());
        assert_eq!(v["reason"], "no_previous_version");
        assert!(v.get("diff").is_none());
        assert!(v.get("markdown").is_none());
    }

    #[test]
    fn cut_utf8_never_splits_a_character() {
        assert_eq!(cut_utf8("abc", 5), ("abc", false));
        assert_eq!(cut_utf8("abcdef", 3), ("abc", true));
        // three-byte character straddling the limit
        assert_eq!(cut_utf8("a\u{20ac}", 2), ("a", true));
        assert_eq!(cut_utf8("\u{20ac}", 1), ("", true));
    }
}
