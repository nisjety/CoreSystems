//! Cross-plane contract schemas.
//!
//! These types define the wire format for communication between Quarry,
//! Model Plane, and Data Plane. They are Quarry's canonical view of each
//! contract — the other planes mirror equivalent definitions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::kinds;
use crate::privacy::PrivacyPolicy;
use crate::zdr::ZdrMode;

// ---------------------------------------------------------------------------
// §1  BrowserObservation  (Quarry → Model Plane)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserObservation {
    pub run_id: kinds::RunKind,
    pub step: u32,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dom_summary: Option<DomSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_artifact_id: Option<kinds::ArtifactKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_observation_artifact_id: Option<kinds::ArtifactKind>,
    #[serde(default)]
    pub console_summary: Vec<ConsoleLine>,
    #[serde(default)]
    pub network_summary: Vec<NetworkEntry>,
    #[serde(default)]
    pub policy_denials: Vec<String>,
    pub observed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomSummary {
    pub node_count: u32,
    pub interactive_elements: Vec<InteractiveElement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_snippet: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveElement {
    pub tag: String,
    pub selector: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleLine {
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEntry {
    pub method: String,
    pub url: String,
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
}

// ---------------------------------------------------------------------------
// §2  AgentActionRequest  (Model Plane → Quarry)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActionRequest {
    pub run_id: kinds::RunKind,
    pub lease_id: kinds::LeaseKind,
    pub action: AgentAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    pub constraints: AgentConstraints,
    pub zdr: ZdrMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    Navigate {
        url: String,
    },
    Click {
        selector: String,
    },
    ClickPoint {
        x: f64,
        y: f64,
    },
    Type {
        selector: String,
        text: String,
    },
    Press {
        key: String,
    },
    Scroll {
        target: String,
    },
    MouseWheel {
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    },
    Select {
        selector: String,
        value: String,
    },
    Wait {
        ms: u32,
    },
    WaitFor {
        selector: String,
        timeout_ms: u32,
    },
    Screenshot {
        full_page: bool,
    },
    Pdf,
    Evaluate {
        script: String,
    },
    Back,
    Forward,
    GetContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConstraints {
    pub max_steps: u32,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runtime_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

// ---------------------------------------------------------------------------
// §3  DataPlaneIngest  (Quarry → Data Plane)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPlaneIngestRequest {
    pub run_id: kinds::RunKind,
    pub org_id: String,
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html_ref: Option<kinds::ArtifactKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_ref: Option<kinds::ArtifactKind>,
    #[serde(default)]
    pub chunks: Vec<ChunkRef>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub fingerprint: String,
    pub zdr: ZdrMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy_policy: Option<PrivacyPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_trace: Option<SourceTrace>,
    /// Per-user ownership (Ownership phase). The initiating user's id, carried
    /// from the verified Edge JWT through the orchestrator. When present, the
    /// Quarry→Data Plane ingest forwards it as `x-user-id` so documents-api
    /// stamps `owner_id = <user>` and (absent an explicit `visibility`) defaults
    /// the doc to `private`. Absent = system/connector ingest → org-visible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiator_user_id: Option<String>,
    /// Explicit document visibility override (`private` | `org` | `shared`).
    /// Normally `None` → documents-api decides from viewer presence
    /// (viewer→private, no viewer→org). Set by the promote-to-org flow.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRef {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTrace {
    pub source_url: String,
    pub fetched_at: DateTime<Utc>,
    pub fingerprint: String,
    #[serde(default)]
    pub field_traces: Vec<FieldTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldTrace {
    pub field: String,
    pub source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPlaneIngestResponse {
    pub document_id: String,
    pub index_status: IndexStatus,
    pub knowledge_unit_count: u32,
    pub embedding_status: EmbeddingStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrievable_after: Option<DateTime<Utc>>,
    pub trace_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexStatus {
    Pending,
    Indexed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingStatus {
    Pending,
    Embedded,
    Failed,
    Skipped,
}

// ---------------------------------------------------------------------------
// §4  StructuredExtract  (Quarry → Model Plane)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredExtractRequest {
    pub source_artifact_ref: kinds::ArtifactKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output_schema: Option<serde_json::Value>,
    #[serde(default)]
    pub source_trace_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    pub zdr: ZdrMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredExtractResponse {
    pub artifact_id: kinds::ArtifactKind,
    pub data: serde_json::Value,
    pub schema_valid: bool,
    pub usage: ExtractionUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_trace: Option<SourceTrace>,
    pub model: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost_usd: f64,
}
