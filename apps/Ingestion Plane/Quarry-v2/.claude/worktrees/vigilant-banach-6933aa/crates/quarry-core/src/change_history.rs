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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_status_json_is_snake_case() {
        assert_eq!(serde_json::to_string(&ChangeStatus::New).unwrap(), "\"new\"");
        assert_eq!(serde_json::to_string(&ChangeStatus::Changed).unwrap(), "\"changed\"");
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
}
