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
    /// A compact, per-observation browser accessibility snapshot. Native
    /// drivers project the CDP accessibility tree and bind each exposed ref to
    /// an exact backend DOM node; legacy read-only drivers may expose a
    /// clearly non-authoritative compatibility projection. References are
    /// opaque handles that belong only to this run and generation; callers
    /// must re-observe after a page state change instead of replaying a stale
    /// selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<BrowserSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dom_summary: Option<DomSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_artifact_id: Option<kinds::ArtifactKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_observation_artifact_id: Option<kinds::ArtifactKind>,
    /// Bounded DOM, visual, and redacted-network change evidence for this
    /// observation. Omitted under ZDR or when artifact persistence is absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_delta_artifact_id: Option<kinds::ArtifactKind>,
    #[serde(default)]
    pub console_summary: Vec<ConsoleLine>,
    #[serde(default)]
    pub network_summary: Vec<NetworkEntry>,
    /// Redacted, per-request decisions emitted by Quarry's browser egress
    /// boundary. These receipts contain no query string, request body,
    /// credentials, or resolved IP address.
    #[serde(default)]
    pub egress_receipts: Vec<BrowserEgressReceipt>,
    /// JavaScript dialogs that are currently open. Quarry never accepts or
    /// dismisses these implicitly; a response is an explicit, grant-bound
    /// action referencing this opaque dialog id.
    #[serde(default)]
    pub dialogs: Vec<BrowserDialog>,
    #[serde(default)]
    pub policy_denials: Vec<String>,
    /// Deterministic execution outcome for the action that produced this
    /// observation.  `Unknown` is intentionally distinct from success: a
    /// browser API call completing does not prove that the intended business
    /// effect occurred.
    #[serde(default)]
    pub action_outcome: ActionOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_delta: Option<ObservationDelta>,
    /// Signals that the page is an access/challenge state rather than usable
    /// source material. This is advisory; escalation remains a Model/Policy
    /// decision and Quarry never attempts to bypass access controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<ChallengeSignal>,
    /// Optional deterministic extraction contract selected by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_profile: Option<ExtractionProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_result: Option<ExtractionResult>,
    /// Immutable evidence manifest for this observation/action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_bundle: Option<ProofBundle>,
    /// The snapshot-bound target Quarry resolved before executing this step.
    /// This makes an effectful action independently auditable without
    /// exposing Quarry's internal CSS selector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_resolution: Option<ResolvedTargetProof>,
    /// Measured browser execution facts for this observation. Fields are
    /// intentionally optional where Quarry has no provider-authoritative
    /// meter; absence is never a zero-cost or zero-resource claim.
    #[serde(default)]
    pub telemetry: BrowserTelemetry,
    pub observed_at: DateTime<Utc>,
}

/// Browser execution telemetry attached to every observation and step proof.
///
/// `renderer_*` values are sampled from the browser's active renderer, not
/// host-wide process accounting. This distinction prevents a Chromium metric
/// from being misrepresented as container or fleet resource usage.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserTelemetry {
    #[serde(default)]
    pub startup_mode: BrowserStartupMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_latency_ms: Option<u64>,
    #[serde(default)]
    pub usable_observation_latency_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer_task_cpu_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renderer_js_heap_used_bytes: Option<u64>,
    #[serde(default)]
    pub estimated_snapshot_tokens: u32,
    #[serde(default)]
    pub observed_action_count: u32,
    #[serde(default)]
    pub challenge_observation_count: u32,
    /// Challenge observations per thousand observations in this live run.
    #[serde(default)]
    pub challenge_rate_per_mille: u16,
    /// Provider-metered cost of the verified action, if one exists. `None`
    /// means Quarry did not receive an authoritative cost figure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_action_cost_micro_usd: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserStartupMode {
    Cold,
    Warm,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservationDelta {
    pub changed_fields: Vec<String>,
    pub url_changed: bool,
    pub title_changed: bool,
    pub dom_changed: bool,
    /// Whether the normalized page content fingerprint changed since the
    /// previous observation. A missing previous fingerprint is an initial
    /// observation rather than a content change.
    #[serde(default)]
    pub content_changed: bool,
}

/// Redacted, bounded evidence persisted as `evidence_delta`. It tells a
/// planner what changed without exposing query strings, credentials, raw DOM,
/// or the browser's network trace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceDelta {
    pub version: u8,
    pub step: u32,
    pub dom: DomEvidenceDelta,
    pub network: NetworkEvidenceDelta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_observation_artifact_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomEvidenceDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_node_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_count: Option<u32>,
    pub changed: bool,
    pub snapshot_target_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkEvidenceDelta {
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
    #[serde(default)]
    pub current_count: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcomeStatus {
    Verified,
    Failed,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionOutcome {
    pub status: ActionOutcomeStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl ActionOutcome {
    pub fn verified(reason_code: impl Into<String>) -> Self {
        Self {
            status: ActionOutcomeStatus::Verified,
            reason_code: Some(reason_code.into()),
            detail: None,
        }
    }

    pub fn unknown(reason_code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: ActionOutcomeStatus::Unknown,
            reason_code: Some(reason_code.into()),
            detail: Some(detail.into()),
        }
    }

    pub fn failed(reason_code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status: ActionOutcomeStatus::Failed,
            reason_code: Some(reason_code.into()),
            detail: Some(detail.into()),
        }
    }
}

impl Default for ActionOutcome {
    fn default() -> Self {
        Self::unknown(
            "not_verified",
            "no deterministic postcondition was supplied",
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeKind {
    Captcha,
    Waf,
    Login,
    Consent,
    RateLimit,
    AccessDenied,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChallengeSignal {
    pub kind: ChallengeKind,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    /// Explicitly tells the planner that automatic bypass is forbidden.
    #[serde(default = "true_value")]
    pub requires_escalation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionSource {
    NetworkJson,
    JsonLd,
    Dom,
    Accessibility,
    Visual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionField {
    pub name: String,
    pub selector: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtractionProfile {
    pub profile_id: String,
    pub fields: Vec<ExtractionField>,
    /// Ordered fallback sources; earlier sources are preferred and later
    /// sources are only used when the field is absent.
    pub source_order: Vec<ExtractionSource>,
    #[serde(default = "default_extraction_max_bytes")]
    pub max_bytes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtractionResult {
    pub profile_id: String,
    pub fields: Vec<ExtractionFieldResult>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtractionFieldResult {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ExtractionSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_selector: Option<String>,
}

fn true_value() -> bool {
    true
}

fn default_extraction_max_bytes() -> u32 {
    1_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProofBundle {
    pub proof_id: String,
    pub source_url: String,
    pub run_id: kinds::RunKind,
    pub step: u32,
    pub action_outcome: ActionOutcome,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_ids: Vec<kinds::ArtifactKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub challenge: Option<ChallengeSignal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_resolution: Option<ResolvedTargetProof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_delta_artifact_id: Option<kinds::ArtifactKind>,
    pub observed_at: DateTime<Utc>,
}

/// Receipt evidence for a snapshot-backed action. The fingerprint proves the
/// action was bound to the target observed by Quarry, rather than a caller
/// supplied selector that may have drifted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResolvedTargetProof {
    pub snapshot_id: String,
    pub generation: u32,
    pub ref_id: String,
    pub fingerprint: ElementFingerprint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<SemanticLocator>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomSummary {
    pub node_count: u32,
    pub interactive_elements: Vec<InteractiveElement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_snippet: Option<String>,
    /// Stable action-planning index over the surviving interactive elements.
    /// Rows are dense and 1-based: row N names `@eN`, the same ref-id form
    /// agent snapshots mint, so a planner can go straight from this compact
    /// map to a snapshot-backed action without renumbering. Authoritative
    /// element detail lives at `interactive_elements[index - 1]`. Absent when
    /// the page offers nothing interactive; legacy payloads may omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_map: Option<Vec<ElementClickTarget>>,
}

/// Bounded agent-facing page snapshot. This is deliberately a stable wire
/// representation, not a raw DOM or CDP object: Model Plane receives only
/// the targets Quarry is willing to resolve and execute against.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSnapshot {
    /// Opaque, deterministic identifier for this run + page state.
    pub snapshot_id: String,
    /// Monotonic generation within the run. A ref must always name its source
    /// generation when an action uses it.
    pub generation: u32,
    #[serde(default)]
    pub targets: Vec<SnapshotTarget>,
    /// Browser-authored accessibility projection, when the selected driver
    /// supports it. This is intentionally distinct from `targets`: an AX node
    /// is not executable until Quarry binds it to an exact live DOM target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessibility: Option<AccessibilityProjection>,
    /// Origin-labelled browser frame topology. Frame ids are opaque browser
    /// ids; a model must request an explicit, grant-bound frame interaction
    /// before action execution can cross a frame boundary.
    #[serde(default)]
    pub frames: Vec<BrowserFrame>,
}

/// Bounded projection of the browser's computed accessibility tree. It is
/// produced by the renderer, rather than inferred from HTML by Quarry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityProjection {
    pub source: String,
    #[serde(default)]
    pub nodes: Vec<AccessibilityNode>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityNode {
    pub node_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default)]
    pub ignored: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    #[serde(default)]
    pub child_ids: Vec<String>,
}

/// A frame is disclosed with its security origin only. Raw URLs, query
/// strings, and frame documents remain browser evidence rather than a model
/// input surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserFrame {
    pub frame_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_frame_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub child_frame_ids: Vec<String>,
}

/// One target exposed by an agent snapshot. The selector remains internal to
/// Quarry's resolver: agent callers address `ref_id`, not CSS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotTarget {
    /// Opaque target handle in the familiar agent form, e.g. `@e1`.
    pub ref_id: String,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    /// The observed frame that owns this target. A child-frame target is
    /// executable only through the matching explicit `frame_*_ref` action;
    /// ordinary refs fail closed before the browser driver is reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<ElementFingerprint>,
    /// For scanner-produced targets: the stable 1-based DOM-summary click
    /// index this ref was minted from, aligned with the summary's click map.
    /// Native AX targets carry their own authority and leave this unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InteractiveElement {
    pub tag: String,
    pub selector: String,
    /// Ordered fallback locators. The first entry is canonical; later
    /// entries let an agent recover when framework-generated DOM changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selector_alternatives: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aria_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessible_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_id: Option<String>,
    /// Deterministic, non-LLM target identity used for read-only repair and
    /// evidence. Effectful actions still require an exact/approved target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<ElementFingerprint>,
    /// Stable 1-based position in the parent summary's click map, aligned
    /// with the `@eN` snapshot ref convention. Absent for legacy summaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_index: Option<u32>,
}

/// One compact row of a [`DomSummary`] click map. Deliberately narrower than
/// [`InteractiveElement`]: a planner scans this list cheaply and only fans
/// out to the full element (via `index`) when it needs placeholder, test-id,
/// or fingerprint detail.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ElementClickTarget {
    /// Agent-facing handle, identical to the snapshot ref for this index.
    pub ref_id: String,
    /// Dense, 1-based; equals this row's position in the click map plus one.
    pub index: u32,
    pub tag: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selector: Option<String>,
    /// Best-effort accessible name (aria-label, label association, or text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// A semantic locator is resolved against the latest Quarry observation, then
/// pinned to that observation's target fingerprint before browser execution.
/// It intentionally supports only deterministic, bounded matching rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticLocator {
    Role {
        role: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default)]
        exact: bool,
    },
    Text {
        text: String,
        #[serde(default)]
        exact: bool,
    },
    Label {
        label: String,
        #[serde(default)]
        exact: bool,
    },
    Placeholder {
        placeholder: String,
        #[serde(default)]
        exact: bool,
    },
    TestId {
        test_id: String,
        #[serde(default)]
        exact: bool,
    },
    Nth {
        selector: String,
        index: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ElementFingerprint {
    pub fingerprint_id: String,
    pub tag: String,
    #[serde(default)]
    pub normalized_text: String,
    #[serde(default)]
    pub attributes: Vec<(String, String)>,
    #[serde(default)]
    pub structural_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logical_id: Option<String>,
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

/// A policy decision made before a browser request is continued or aborted.
/// `Allow` is evidence of the guard's admission decision, not a claim that a
/// response was successfully received.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserEgressDecision {
    Allow,
    Block,
}

/// Immutable-safe representation of one browser request-policy decision.
/// The runtime copies a bounded set into the observation which is then stored
/// in the ordinary step receipt. The URL must be redacted by the driver.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserEgressReceipt {
    pub sequence: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    pub method: String,
    pub url: String,
    pub decision: BrowserEgressDecision,
    /// Stable policy category only; never the raw underlying error because it
    /// could contain page-controlled values.
    pub policy: String,
    pub timestamp_ms: u64,
}

/// A browser-level JavaScript dialog observed through the driver. The dialog
/// identifier is scoped to its live browser session and cannot be replayed
/// after resolution or resume.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserDialog {
    pub dialog_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    pub kind: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    pub opened_at_ms: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extraction_profile: Option<ExtractionProfile>,
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
    /// Click a target referenced from the caller's most recent snapshot.
    /// Quarry rejects missing, stale, or mismatched references before it
    /// reaches a browser driver.
    ClickRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
    },
    /// Click an exact target inside an observed child frame. The frame id is
    /// an opaque snapshot-scoped contract: Quarry verifies it against the
    /// native target before dispatch and never treats it as a URL or selector.
    FrameClickRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
    },
    /// Resolve a semantic locator from the latest observation then click its
    /// single deterministic match. Ambiguous matches fail closed.
    ClickSemantic {
        snapshot_id: String,
        generation: u32,
        locator: SemanticLocator,
    },
    ClickPoint {
        x: f64,
        y: f64,
    },
    Type {
        selector: String,
        text: String,
    },
    TypeRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        text: String,
    },
    /// Type into an exact target inside an observed child frame.
    FrameTypeRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        text: String,
    },
    TypeSemantic {
        snapshot_id: String,
        generation: u32,
        locator: SemanticLocator,
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
    SelectRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        value: String,
    },
    /// Select an option in an exact target inside an observed child frame.
    FrameSelectRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        value: String,
    },
    SelectSemantic {
        snapshot_id: String,
        generation: u32,
        locator: SemanticLocator,
        value: String,
    },
    Wait {
        ms: u32,
    },
    WaitFor {
        selector: String,
        timeout_ms: u32,
    },
    WaitForRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        timeout_ms: u32,
    },
    /// Wait for an already-observed target inside an observed child frame.
    FrameWaitForRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        timeout_ms: u32,
    },
    WaitForSemantic {
        snapshot_id: String,
        generation: u32,
        locator: SemanticLocator,
        timeout_ms: u32,
    },
    /// Reply to an observed JavaScript dialog. The edge requires a live,
    /// explicit browser grant for this action even when ordinary read actions
    /// are allowed without one.
    RespondDialog {
        dialog_id: String,
        accept: bool,
        /// Fresh BrowserBroker approval grant supplied with this response,
        /// rather than a broad run-level capability retained from an earlier
        /// observation. Quarry rejects replay within the live run; Model
        /// Plane must additionally issue it with dialog/action scope.
        approval_grant_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_text: Option<String>,
    },
    /// Upload a tenant-owned Quarry artifact to an exact file-input node from
    /// the latest accessibility snapshot. Callers can name only the opaque
    /// artifact id: Quarry stages the bytes in a short-lived private file and
    /// never accepts a host filesystem path.
    UploadRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        artifact_id: kinds::ArtifactKind,
        /// A fresh BrowserBroker approval grant for this irreversible data
        /// disclosure. The edge consumes it once per live run.
        approval_grant_id: String,
    },
    /// Upload a tenant-owned artifact to an exact child-frame input. This has
    /// the same one-shot approval semantics as [`Self::UploadRef`] plus an
    /// explicit frame contract.
    FrameUploadRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        artifact_id: kinds::ArtifactKind,
        approval_grant_id: String,
    },
    /// Collect a download caused by an exact native target. Quarry keeps the
    /// browser's temporary path private, admits only an allowed safe type, and
    /// promotes accepted bytes into tenant-owned artifact storage.
    DownloadRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        approval_grant_id: String,
    },
    /// Collect an artifact-only download from an exact child-frame target.
    FrameDownloadRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        approval_grant_id: String,
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
