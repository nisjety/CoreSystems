//! Normalized output envelope (runtime → edge/control). Mirrors CONTRACTS §9.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::kinds;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedOutput {
    pub run_id: kinds::RunKind,
    pub url: UrlTriple,
    pub status: u16,
    pub fetched_at: DateTime<Utc>,
    pub fingerprint: String,
    pub formats: OutputFormats,
    pub change: ChangeInfo,
    pub metadata: PageMetadata,
    pub driver: DriverInfo,
    /// Cycle 21 / cluster #2 — runtime stamps the Determinism mode +
    /// policy fingerprint + identity hash so downstream consumers can
    /// audit which RunPolicy produced the artifact. `None` for legacy
    /// outputs produced before P21 wiring. Always populated by the
    /// current PageRunner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub determinism: Option<DeterminismStamp>,
}

/// Compact policy/identity stamp embedded in [`NormalizedOutput`]. The
/// fields mirror [`crate::contracts`] / quarry-runtime's `policy.rs`
/// types but live in `quarry-core` so consumers don't need to depend
/// on the runtime crate to interpret the stamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterminismStamp {
    /// `"strict" | "best_effort" | "off"`.
    pub mode: String,
    /// `policy_fingerprint(&RunPolicy)` — e.g. `"blake3:..."`.
    pub policy_fp: String,
    /// `record_determinism_inputs(url, &policy).id` — `"dq:..."`.
    pub identity_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlTriple {
    pub requested: String,
    pub final_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputFormats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<FormatRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<FormatRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<FormatRef>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<FormatRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pdf: Option<FormatRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<ExtractRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatRef {
    pub artifact_id: kinds::ArtifactKind,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractRef {
    pub artifact_id: kinds::ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub href: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rel: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeInfo {
    pub status: ChangeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    New,
    Changed,
    Unchanged,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverInfo {
    pub kind: DriverKind,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_view_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverKind {
    Static,
    Browser,
    Tls,
}
