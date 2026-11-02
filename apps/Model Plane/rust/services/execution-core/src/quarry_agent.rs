//! Quarry agent-browser client (P7 · Phase A).
//!
//! Model Plane *drives* the agentic browser loop; Quarry *executes* each action
//! against a leased browser session and returns an observation. Model Plane and
//! Quarry live in separate cargo workspaces, so this module MIRRORS Quarry's
//! JSON wire contract via local DTOs (the same approach `model-gateway`'s Fetch
//! client takes) rather than importing `quarry-core`.
//!
//! Transport — HTTP per-step against `quarry-edge` (`/v1/agent/*`):
//!   POST   /v1/agent/runs             → start a run (acquire a browser lease)
//!   POST   /v1/agent/runs/{id}/step   → execute one `AgentAction`, return observation
//!   DELETE /v1/agent/runs/{id}        → release the run / lease
//!
//! Quarry wraps every response in `Envelope { data, meta, error }`, so the
//! client unwraps `.data`. `AgentAction` is internally tagged
//! (`{"type":"navigate","url":...}`) to match quarry-core exactly; `zdr` is a
//! bool because the edge converts it via `ZdrMode::from(bool)`.

// Error enums are self-evident (`AgentClientError`); `# Errors` prose would be
// noise. `doc_markdown` over-flags wire tokens like `snake_case`. Both are
// low-signal pedantic lints — scoped-allowed here (cf. workspace-allowed
// `must_use_candidate`).
#![allow(clippy::missing_errors_doc, clippy::doc_markdown)]

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::browser_agent::{
    ActionType, BlockReason, BrowserAction, BrowserChallenge, BrowserObservation,
    BrowserSnapshotTarget, ChallengeKind, ChallengeRecovery, ChallengeStrength, ObservationStatus,
};
use crate::quarry_auth::TokenSource;

const AGENT_SCOPES: &[&str] = &["browser:execute"];

// ---------------------------------------------------------------------------
// Wire DTOs — mirror of quarry-core::contracts (JSON-compatible).
// ---------------------------------------------------------------------------

/// A single browser action. Internally tagged on `type`, snake_case — matches
/// quarry-core `AgentAction` exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    Navigate {
        url: String,
    },
    Click {
        selector: String,
    },
    ClickRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
    },
    FrameClickRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
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
    FrameTypeRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        text: String,
    },
    SelectRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        value: String,
    },
    FrameSelectRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        value: String,
    },
    WaitForRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        timeout_ms: u32,
    },
    FrameWaitForRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        timeout_ms: u32,
    },
    RespondDialog {
        dialog_id: String,
        accept: bool,
        approval_grant_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt_text: Option<String>,
    },
    UploadRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        artifact_id: String,
        approval_grant_id: String,
    },
    FrameUploadRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        artifact_id: String,
        approval_grant_id: String,
    },
    DownloadRef {
        snapshot_id: String,
        generation: u32,
        ref_id: String,
        approval_grant_id: String,
    },
    FrameDownloadRef {
        snapshot_id: String,
        generation: u32,
        frame_id: String,
        ref_id: String,
        approval_grant_id: String,
    },
    Press {
        key: String,
    },
    Scroll {
        target: String,
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
    GetContent,
}

/// Per-run safety budget. Mirrors quarry-core `AgentConstraints`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentConstraints {
    pub max_steps: u32,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_runtime_s: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

/// `POST /v1/agent/runs` request body.
#[derive(Debug, Clone, Serialize)]
pub struct StartRunRequest {
    pub org_id: String,
    /// Opaque broker-issued grant identifier, validated by execution-core
    /// immediately before launch. Forwarded so Quarry can bind the lease and
    /// every subsequent action to the same authorization context.
    pub grant_id: String,
    pub constraints: AgentConstraints,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub zdr: bool,
    /// Tighten-only request ask (Quarry `RequestAsk`). Absent unless set, so an
    /// older Quarry never sees the key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy: Option<WireRequestAsk>,
    /// Run trace context (Quarry run trace body). Absent unless set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<WireRunTrace>,
}

/// The subset of Quarry's `RequestAsk` a model-driven run may send. Only these
/// three keys exist here on purpose: `robots_max` and `proxy_max` must never be
/// set by a model-driven run, and Quarry's schema is `deny_unknown_fields`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRequestAsk {
    /// Capabilities to disable for this run (e.g. `browser.act.upload`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disable: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_domains: Option<Vec<String>>,
}

impl WireRequestAsk {
    /// True when the ask says nothing, so the caller can send no `policy` key.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.disable.is_empty() && self.deny_domains.is_empty() && self.allow_domains.is_none()
    }
}

/// Run trace context sent on `POST /v1/agent/runs` (`{run_id?, agent?, origin?}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRunTrace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<WireRunAgent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

/// The agent identity inside [`WireRunTrace`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireRunAgent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}

/// `POST /v1/agent/runs` response payload (inside the envelope `data`).
#[derive(Debug, Clone, Deserialize)]
pub struct StartRunResponse {
    pub run_id: String,
    pub lease_id: String,
}

/// `POST /v1/agent/runs/{id}/step` request body. Mirrors quarry-core
/// `AgentActionRequest` (the edge uses the run's stored constraints/zdr; the
/// extra fields are accepted and ignored server-side).
#[derive(Debug, Clone, Serialize)]
pub struct StepRequest {
    pub run_id: String,
    pub lease_id: String,
    /// Must match the broker-validated grant used for StartRun.
    pub grant_id: String,
    pub action: AgentAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    pub constraints: AgentConstraints,
    pub zdr: bool,
    /// Who initiated the action (`"agent"` for a delegated attach step).
    /// Absent unless set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<String>,
}

/// Observation returned by a step. Mirrors quarry-core `BrowserObservation`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WireObservation {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub step: u32,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub snapshot: Option<WireBrowserSnapshot>,
    #[serde(default)]
    pub dom_summary: Option<DomSummary>,
    #[serde(default)]
    pub screenshot_artifact_id: Option<String>,
    #[serde(default)]
    pub console_summary: Vec<ConsoleLine>,
    #[serde(default)]
    pub network_summary: Vec<NetworkEntry>,
    #[serde(default)]
    pub egress_receipts: Vec<serde_json::Value>,
    #[serde(default)]
    pub dialogs: Vec<serde_json::Value>,
    #[serde(default)]
    pub policy_denials: Vec<String>,
    #[serde(default)]
    pub telemetry: serde_json::Value,
    #[serde(default)]
    pub observed_at: String,
    /// Absent from an older Quarry. A value that does not read as a
    /// [`WireChallenge`] is dropped rather than failing the observation.
    #[serde(
        default,
        deserialize_with = "lenient_challenge",
        skip_serializing_if = "Option::is_none"
    )]
    pub challenge: Option<WireChallenge>,
}

/// Quarry's `ChallengeSignal` (`challenge-signal.schema.json`). The vocabularies
/// stay strings here so a value this build does not know cannot fail the
/// parse; [`observation_from_wire`] reads them into the closed enums. `evidence`
/// and `requires_escalation` are left out on purpose: evidence is text taken
/// from the page, and the planner is never given page text through this field.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WireChallenge {
    #[serde(default = "unknown_kind")]
    pub kind: String,
    #[serde(default)]
    pub confidence: f32,
    /// A signal from before the strength field existed asked for escalation,
    /// so it reads as strong (Quarry's own default).
    #[serde(default = "strong_strength")]
    pub strength: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default = "not_applicable_recovery")]
    pub recovery: String,
}

fn unknown_kind() -> String {
    "unknown".to_owned()
}

fn strong_strength() -> String {
    "strong".to_owned()
}

fn not_applicable_recovery() -> String {
    "not_applicable".to_owned()
}

fn lenient_challenge<'de, D>(deserializer: D) -> Result<Option<WireChallenge>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(raw.and_then(|value| serde_json::from_value(value).ok()))
}

/// The public portion of Quarry's active AX snapshot. These fields are enough
/// for Model Plane to plan a ref-bound action, while private CDP node bindings
/// remain solely inside Quarry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WireBrowserSnapshot {
    #[serde(default)]
    pub snapshot_id: String,
    #[serde(default)]
    pub generation: u32,
    #[serde(default)]
    pub targets: Vec<WireSnapshotTarget>,
    #[serde(default)]
    pub frames: Vec<WireBrowserFrame>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WireBrowserFrame {
    #[serde(default)]
    pub frame_id: String,
    #[serde(default)]
    pub parent_frame_id: Option<String>,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub child_frame_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WireSnapshotTarget {
    #[serde(default)]
    pub ref_id: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub frame_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DomSummary {
    #[serde(default)]
    pub node_count: u32,
    #[serde(default)]
    pub interactive_elements: Vec<InteractiveElement>,
    #[serde(default)]
    pub text_snippet: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InteractiveElement {
    pub tag: String,
    pub selector: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConsoleLine {
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkEntry {
    pub method: String,
    pub url: String,
    pub status: u16,
    #[serde(default)]
    pub content_type: Option<String>,
}

/// Quarry's REST envelope (`{ data, meta, error }`). We only need `data` +
/// `error`; `meta` is ignored.
#[derive(Debug, Clone, Deserialize)]
struct WireEnvelope<T> {
    #[serde(default = "Option::default")]
    data: Option<T>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Internal ⇄ wire mapping.
// ---------------------------------------------------------------------------

/// Map the loop's internal `BrowserAction` onto a Quarry wire `AgentAction`.
/// `Extract`/`Observe` have no direct Quarry equivalent — both resolve to
/// `GetContent`, which returns the current DOM/observation.
#[must_use]
pub fn action_to_wire(action: &BrowserAction) -> AgentAction {
    match action.action_type {
        ActionType::Goto => AgentAction::Navigate {
            url: action.url.clone(),
        },
        ActionType::Click => match action.target.as_ref() {
            Some(target) => match target.frame_id.as_deref() {
                Some(frame_id) => AgentAction::FrameClickRef {
                    snapshot_id: target.snapshot_id.clone(),
                    generation: target.generation,
                    frame_id: frame_id.to_owned(),
                    ref_id: target.ref_id.clone(),
                },
                None => AgentAction::ClickRef {
                    snapshot_id: target.snapshot_id.clone(),
                    generation: target.generation,
                    ref_id: target.ref_id.clone(),
                },
            },
            None => AgentAction::Click {
                selector: action.selector.clone(),
            },
        },
        ActionType::Type => match action.target.as_ref() {
            Some(target) => match target.frame_id.as_deref() {
                Some(frame_id) => AgentAction::FrameTypeRef {
                    snapshot_id: target.snapshot_id.clone(),
                    generation: target.generation,
                    frame_id: frame_id.to_owned(),
                    ref_id: target.ref_id.clone(),
                    text: action.value.clone(),
                },
                None => AgentAction::TypeRef {
                    snapshot_id: target.snapshot_id.clone(),
                    generation: target.generation,
                    ref_id: target.ref_id.clone(),
                    text: action.value.clone(),
                },
            },
            None => AgentAction::Type {
                selector: action.selector.clone(),
                text: action.value.clone(),
            },
        },
        ActionType::Scroll => AgentAction::Scroll {
            target: if action.value.is_empty() {
                action.selector.clone()
            } else {
                action.value.clone()
            },
        },
        ActionType::Wait => AgentAction::Wait {
            ms: u32::try_from(action.max_wait_ms).unwrap_or(0),
        },
        ActionType::Extract | ActionType::Observe => AgentAction::GetContent,
    }
}

/// Build the loop's internal `BrowserObservation` from a Quarry wire
/// observation. A non-empty `policy_denials` list marks the action `Blocked`.
#[must_use]
pub fn observation_from_wire(
    wire: &WireObservation,
    action_id: &str,
    grant_id: &str,
) -> BrowserObservation {
    let status = if wire.policy_denials.is_empty() {
        ObservationStatus::Success
    } else {
        ObservationStatus::Blocked
    };
    let extracted_text = wire
        .dom_summary
        .as_ref()
        .and_then(|d| d.text_snippet.clone())
        .unwrap_or_default();
    let error_message = if wire.policy_denials.is_empty() {
        String::new()
    } else {
        wire.policy_denials.join("; ")
    };
    let (dom_snapshot_ref, snapshot_generation, snapshot_targets) = wire
        .snapshot
        .as_ref()
        .map(|snapshot| {
            (snapshot.snapshot_id.clone(), Some(snapshot.generation), {
                let root_frame_id = snapshot
                    .frames
                    .iter()
                    .find(|frame| frame.parent_frame_id.is_none())
                    .map(|frame| frame.frame_id.as_str());
                snapshot
                    .targets
                    .iter()
                    .map(|target| BrowserSnapshotTarget {
                        ref_id: target.ref_id.clone(),
                        role: target.role.clone(),
                        name: target.name.clone(),
                        text: target.text.clone(),
                        // A top-level frame id is implementation detail for
                        // this model abstraction. Preserve only a child id:
                        // its presence selects Quarry's explicit frame-ref
                        // action instead of accidentally framing a root ref.
                        frame_id: target
                            .frame_id
                            .as_deref()
                            .filter(|frame_id| Some(*frame_id) != root_frame_id)
                            .map(str::to_owned),
                    })
                    .collect()
            })
        })
        .unwrap_or_else(|| (String::new(), None, Vec::new()));

    BrowserObservation {
        observation_id: format!("obs_{}_{:04}", wire.run_id, wire.step),
        action_id: action_id.to_owned(),
        grant_id: grant_id.to_owned(),
        status,
        page_url: wire.url.clone(),
        page_title: wire.title.clone().unwrap_or_default(),
        extracted_text,
        screenshot_ref: wire.screenshot_artifact_id.clone().unwrap_or_default(),
        dom_snapshot_ref,
        snapshot_generation,
        snapshot_targets,
        error_message,
        challenge: wire.challenge.as_ref().map(challenge_from_wire),
    }
}

/// Read Quarry's signal into the closed vocabularies; a value this build does
/// not know degrades as each `from_wire` documents, so nothing fails to parse.
#[must_use]
pub fn challenge_from_wire(wire: &WireChallenge) -> BrowserChallenge {
    // Clamped to 0..=1 first (NaN casts to 0), so the cast cannot truncate.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let confidence_per_mille = (wire.confidence.clamp(0.0, 1.0) * 1000.0).round() as u16;
    BrowserChallenge {
        kind: ChallengeKind::from_wire(&wire.kind),
        strength: ChallengeStrength::from_wire(&wire.strength),
        block_reason: wire.block_reason.as_deref().and_then(BlockReason::from_wire),
        recovery: ChallengeRecovery::from_wire(&wire.recovery),
        confidence_per_mille,
    }
}

/// The error code Quarry answers a refused action or scrape with when a bot
/// wall stands in the way (`docs/CONTRACTS.md` section 9.2).
const CHALLENGE_DETECTED_CODE: &str = "CHALLENGE_DETECTED";

/// Read a Quarry `403 CHALLENGE_DETECTED` body as a strong, exhausted signal.
/// Any other status or code is `None`, so every other 403 stays an error.
///
/// The block reason is `details.block_reason` (the scrape path) or
/// `details.reason` (the agent path), whichever is a value of the closed list;
/// the code alone is the verdict, so a refusal without a usable reason is still
/// a strong signal with no reason. Nothing else in the body is read, because
/// the message and the details are not for the planner.
fn challenge_from_refusal(status: u16, body: &str) -> Option<BrowserChallenge> {
    if status != 403 {
        return None;
    }
    let envelope: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = envelope.get("error")?;
    if error.get("code")?.as_str()? != CHALLENGE_DETECTED_CODE {
        return None;
    }
    let details = error.get("details");
    let block_reason = ["block_reason", "reason"].into_iter().find_map(|key| {
        details?
            .get(key)?
            .as_str()
            .and_then(BlockReason::from_wire)
    });
    Some(BrowserChallenge {
        kind: ChallengeKind::Unknown,
        strength: ChallengeStrength::Strong,
        block_reason,
        // The agent path only refuses after the one recovery was spent.
        recovery: ChallengeRecovery::Exhausted,
        // A refusal is a verdict, not a classification, so it carries no
        // confidence of its own.
        confidence_per_mille: 1000,
    })
}

/// The observation a refused action becomes: blocked, carrying the signal, so
/// the loop stops on it instead of retrying a refusal it could not clear.
/// `page_url` is the page the action was aimed at; no page text is carried.
#[must_use]
pub fn observation_from_challenge_refusal(
    challenge: BrowserChallenge,
    action_id: &str,
    grant_id: &str,
    page_url: &str,
) -> BrowserObservation {
    BrowserObservation {
        observation_id: format!("obs_refused_{action_id}"),
        action_id: action_id.to_owned(),
        grant_id: grant_id.to_owned(),
        status: ObservationStatus::Blocked,
        page_url: page_url.to_owned(),
        page_title: String::new(),
        extracted_text: String::new(),
        screenshot_ref: String::new(),
        dom_snapshot_ref: String::new(),
        snapshot_generation: None,
        snapshot_targets: Vec::new(),
        error_message: format!("the site refuses automation ({challenge})"),
        challenge: Some(challenge),
    }
}

// ---------------------------------------------------------------------------
// HTTP client.
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum AgentClientError {
    #[error("quarry agent transport error: {0}")]
    Transport(String),
    #[error("quarry agent returned {status}: {body}")]
    Status { status: u16, body: String },
    /// Quarry refused the action because the host is a bot wall (`403
    /// CHALLENGE_DETECTED`). Not a transient failure: retrying the same action
    /// is what Quarry just refused.
    #[error("quarry agent refused the action: bot challenge ({0})")]
    Challenge(BrowserChallenge),
    #[error("quarry agent decode error: {0}")]
    Decode(String),
    #[error("quarry agent authentication failed: {0}")]
    Authentication(String),
    /// A non-2xx answer whose body is a Quarry error envelope. Callers read
    /// `code` / `reason` / `details`; [`classify_quarry_error`] says what the
    /// loop should do with it. A body that is not an envelope stays `Status`.
    #[error("quarry agent returned {}: {} ({})", .0.status, .0.code, .0.message)]
    Typed(TypedError),
}

/// A parsed Quarry error envelope (`{ error: { code, message, details } }`).
#[derive(Debug, Clone, PartialEq)]
pub struct TypedError {
    pub status: u16,
    /// `error.code`, e.g. `BUDGET_EXHAUSTED`, `FORBIDDEN`, `USER_TOOK_OVER`.
    pub code: String,
    /// `error.message`. Operator text; never shown to the planner.
    pub message: String,
    /// `error.details.reason`, or (attach refusals, which carry the reason in
    /// `message`) the message when it is a bare lowercase token.
    pub reason: Option<String>,
    /// `error.details`, `Null` when absent.
    pub details: serde_json::Value,
    /// `error.details.retry_after_seconds` on a 429.
    pub retry_after_seconds: Option<u64>,
}

/// A reason token: lowercase letters, digits, `_` and `.`, 1 to 48 chars.
fn is_reason_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 48
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

/// Read a non-2xx Quarry body as a typed error. `None` when the body is not an
/// envelope with a string `error.code` (plain text, an array, a bare string).
#[must_use]
pub fn parse_typed_error(status: u16, body: &str) -> Option<TypedError> {
    let envelope: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = envelope.get("error")?;
    let code = error.get("code")?.as_str()?.to_owned();
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let details = error
        .get("details")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let reason = details
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| is_reason_token(&message).then(|| message.clone()));
    let retry_after_seconds = details
        .get("retry_after_seconds")
        .and_then(serde_json::Value::as_u64);
    Some(TypedError {
        status,
        code,
        message,
        reason,
        details,
        retry_after_seconds,
    })
}

/// A fixed-vocabulary description of a failed Quarry call for a run summary
/// and the planner's tool result. A typed error is `<status> <CODE>` plus its
/// `details.reason` when that is a plain token; Quarry's `message` (operator
/// text, which can echo a URL or a page) is never included. Other errors keep
/// their own `Display`.
#[must_use]
pub fn fixed_error_text(error: &AgentClientError) -> String {
    let AgentClientError::Typed(typed) = error else {
        return error.to_string();
    };
    let code_ok = !typed.code.is_empty()
        && typed.code.len() <= 48
        && typed
            .code
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
    let code = if code_ok { typed.code.as_str() } else { "UNKNOWN" };
    match typed.reason.as_deref().filter(|r| is_reason_token(r)) {
        Some(reason) => format!("{} {code} ({reason})", typed.status),
        None => format!("{} {code}", typed.status),
    }
}

/// The Quarry code for an exhausted agent-run budget (402).
pub const BUDGET_EXHAUSTED_CODE: &str = "BUDGET_EXHAUSTED";

/// Which Quarry run budget ran out (`error.details.dimension` of a 402).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetDimension {
    Steps,
    Attempts,
    Runtime,
    Cost,
    OrgAllowance,
    /// A dimension this build does not know.
    Other,
}

impl BudgetDimension {
    #[must_use]
    pub fn from_wire(value: &str) -> Self {
        match value {
            "steps" => Self::Steps,
            "attempts" => Self::Attempts,
            "runtime" => Self::Runtime,
            "cost" => Self::Cost,
            "org_allowance" => Self::OrgAllowance,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::Attempts => "attempts",
            Self::Runtime => "runtime",
            Self::Cost => "cost",
            Self::OrgAllowance => "org_allowance",
            Self::Other => "other",
        }
    }
}

/// What the loop should do with a failed Quarry call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepFailureClass {
    /// The planner can try something else (stale target, one slow action).
    Recoverable,
    /// The run cannot continue; it ends `Failed`.
    Terminal,
    /// A budget or backpressure stopped the run; it ends `resource_exhausted`,
    /// never `Failed`, and is never retried.
    ResourceLimit,
}

/// The budget dimension of a 402 `BUDGET_EXHAUSTED` answer, `None` for every
/// other error. A 402 whose dimension is missing or unknown reads as `Other`.
#[must_use]
pub fn quarry_budget_dimension(error: &AgentClientError) -> Option<BudgetDimension> {
    let AgentClientError::Typed(typed) = error else {
        return None;
    };
    if typed.status != 402 || typed.code != BUDGET_EXHAUSTED_CODE {
        return None;
    }
    Some(
        typed
            .details
            .get("dimension")
            .and_then(serde_json::Value::as_str)
            .map_or(BudgetDimension::Other, BudgetDimension::from_wire),
    )
}

/// True for a 429 (Quarry or an upstream is shedding load).
#[must_use]
pub fn is_quarry_backpressure(error: &AgentClientError) -> bool {
    match error {
        AgentClientError::Typed(typed) => typed.status == 429,
        AgentClientError::Status { status, .. } => *status == 429,
        _ => false,
    }
}

/// A failure the planner can work around (WS11.S3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoverableKind {
    /// 409 `TARGET_REPAIR_REQUIRED` / `ACTION_UNKNOWN`: the page moved on.
    StaleTarget,
    /// 501: the driver cannot do this action.
    UnsupportedAction,
    /// 504 `TIMEOUT`: the action ran too long. Never replayed.
    ActionTimeout,
    /// 502 `DRIVER_FAILED`: the loop replays the action once.
    DriverFailed,
}

impl RecoverableKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StaleTarget => "stale_target",
            Self::UnsupportedAction => "unsupported_action",
            Self::ActionTimeout => "action_timeout",
            Self::DriverFailed => "driver_failed",
        }
    }

    /// Fixed advice for the planner; carries nothing from Quarry's body.
    #[must_use]
    pub const fn advice(self) -> &'static str {
        match self {
            Self::StaleTarget => "the page changed; pick a target from the fresh observation",
            Self::UnsupportedAction => "this action is not supported; use a different action",
            Self::ActionTimeout => "the action timed out; it was not repeated, try another approach",
            Self::DriverFailed => "the browser driver failed twice; try another approach",
        }
    }
}

/// The recoverable kind of a failed Quarry call, `None` when it is not one.
#[must_use]
pub fn recoverable_kind(error: &AgentClientError) -> Option<RecoverableKind> {
    let (status, code) = match error {
        AgentClientError::Typed(typed) => (typed.status, typed.code.as_str()),
        AgentClientError::Status { status, .. } => (*status, ""),
        _ => return None,
    };
    match (status, code) {
        (409, "TARGET_REPAIR_REQUIRED" | "ACTION_UNKNOWN") => Some(RecoverableKind::StaleTarget),
        (504, "TIMEOUT") => Some(RecoverableKind::ActionTimeout),
        (502, "DRIVER_FAILED") => Some(RecoverableKind::DriverFailed),
        (501, _) => Some(RecoverableKind::UnsupportedAction),
        _ => None,
    }
}

/// The longest a 429 is waited out before the one replay.
pub const MAX_BACKPRESSURE_WAIT_S: u64 = 10;

/// How long to wait before replaying a 429 once: the answer's
/// `retry_after_seconds` (1 s when absent), or `None` when it asks for more
/// than [`MAX_BACKPRESSURE_WAIT_S`], so the run ends `resource_exhausted`.
#[must_use]
pub fn backpressure_wait(error: &AgentClientError) -> Option<Duration> {
    if !is_quarry_backpressure(error) {
        return None;
    }
    let seconds = match error {
        AgentClientError::Typed(typed) => typed.retry_after_seconds.unwrap_or(1),
        _ => 1,
    };
    (seconds <= MAX_BACKPRESSURE_WAIT_S).then(|| Duration::from_secs(seconds))
}

/// Classify a failed Quarry call. A 402 `BUDGET_EXHAUSTED` and any 429 are
/// resource limits, the recoverable kinds are recoverable, and everything else
/// is terminal.
#[must_use]
pub fn classify_quarry_error(error: &AgentClientError) -> StepFailureClass {
    if quarry_budget_dimension(error).is_some() || is_quarry_backpressure(error) {
        StepFailureClass::ResourceLimit
    } else if recoverable_kind(error).is_some() {
        StepFailureClass::Recoverable
    } else {
        StepFailureClass::Terminal
    }
}

/// Header carrying the caller's correlation id on start/step/close.
const REQUEST_ID_HEADER: &str = "x-request-id";

fn with_request_id(
    builder: reqwest::RequestBuilder,
    request_id: Option<&str>,
) -> reqwest::RequestBuilder {
    match request_id.map(str::trim).filter(|id| !id.is_empty()) {
        Some(id) => builder.header(REQUEST_ID_HEADER, id),
        None => builder,
    }
}

/// Parameters of [`QuarryAgentClient::start_run`].
#[derive(Debug, Clone)]
pub struct StartRunParams<'a> {
    pub org_id: &'a str,
    pub constraints: &'a AgentConstraints,
    pub zdr: bool,
    pub profile_id: Option<String>,
    pub grant_id: &'a str,
    pub policy: Option<WireRequestAsk>,
    pub trace: Option<WireRunTrace>,
    pub request_id: Option<&'a str>,
}

impl<'a> StartRunParams<'a> {
    #[must_use]
    pub fn new(
        org_id: &'a str,
        constraints: &'a AgentConstraints,
        zdr: bool,
        grant_id: &'a str,
    ) -> Self {
        Self {
            org_id,
            constraints,
            zdr,
            profile_id: None,
            grant_id,
            policy: None,
            trace: None,
            request_id: None,
        }
    }
}

/// Parameters of [`QuarryAgentClient::step`].
#[derive(Debug, Clone)]
pub struct StepParams<'a> {
    pub run_id: &'a str,
    pub lease_id: &'a str,
    pub org_id: &'a str,
    pub action: AgentAction,
    pub constraints: &'a AgentConstraints,
    pub zdr: bool,
    pub grant_id: &'a str,
    pub initiated_by: Option<String>,
    pub request_id: Option<&'a str>,
}

impl<'a> StepParams<'a> {
    #[must_use]
    pub fn new(
        run_id: &'a str,
        lease_id: &'a str,
        org_id: &'a str,
        action: AgentAction,
        constraints: &'a AgentConstraints,
        zdr: bool,
        grant_id: &'a str,
    ) -> Self {
        Self {
            run_id,
            lease_id,
            org_id,
            action,
            constraints,
            zdr,
            grant_id,
            initiated_by: None,
            request_id: None,
        }
    }
}

/// HTTP client for Quarry's agent-browser endpoint. Mirrors the auth scheme of
/// `model-gateway`'s Fetch client: bearer token + `X-Quarry-Org` header.
#[derive(Debug, Clone)]
pub struct QuarryAgentClient {
    http: reqwest::Client,
    base_url: String,
    auth: TokenSource,
}

impl QuarryAgentClient {
    /// Construct a client against an explicit base URL + bearer token.
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
    ) -> Result<Self, AgentClientError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
        Ok(Self {
            http,
            base_url: base_url.into(),
            auth: TokenSource::Static(token.into()),
        })
    }

    /// Build a client from the environment, gated by `QUARRY_BROWSER_AGENT_ENABLED`
    /// (truthy) and `QUARRY_EDGE_URL` (non-empty) — analogous to how the Fetch
    /// tool gates on `QUARRY_EDGE_URL`. Returns `None` when the browser agent is
    /// disabled or unconfigured, so the loop can degrade gracefully.
    pub fn from_env() -> Result<Option<Self>, AgentClientError> {
        let enabled = std::env::var("QUARRY_BROWSER_AGENT_ENABLED")
            .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);
        if !enabled {
            return Ok(None);
        }
        let base_url = std::env::var("QUARRY_EDGE_URL")
            .ok()
            .filter(|s| !s.is_empty());
        let Some(base_url) = base_url else {
            return Ok(None);
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| AgentClientError::Transport(error.to_string()))?;
        let auth = TokenSource::from_env()
            .map_err(|error| AgentClientError::Authentication(error.to_string()))?;
        Ok(Some(Self {
            http,
            base_url,
            auth,
        }))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
    }

    async fn token(&self, org_id: &str) -> Result<String, AgentClientError> {
        self.auth
            .token(org_id, AGENT_SCOPES)
            .await
            .map_err(|error| AgentClientError::Authentication(error.to_string()))
    }

    async fn invalidate_if_matches(
        &self,
        org_id: &str,
        rejected_token: &str,
    ) -> Result<(), AgentClientError> {
        self.auth
            .invalidate_if_matches(org_id, AGENT_SCOPES, rejected_token)
            .await
            .map_err(|error| AgentClientError::Authentication(error.to_string()))
    }

    /// Start an agent run: Quarry acquires a browser lease/session and returns
    /// the `run_id` + `lease_id` used by subsequent steps.
    pub async fn start_run(
        &self,
        params: StartRunParams<'_>,
    ) -> Result<StartRunResponse, AgentClientError> {
        let org_id = params.org_id;
        let body = StartRunRequest {
            org_id: org_id.to_owned(),
            grant_id: params.grant_id.to_owned(),
            constraints: params.constraints.clone(),
            profile_id: params.profile_id,
            zdr: params.zdr,
            policy: params.policy,
            trace: params.trace,
        };
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = with_request_id(
                self.http
                    .post(self.url("/v1/agent/runs"))
                    .bearer_auth(&token)
                    .header("x-quarry-org", org_id),
                params.request_id,
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, &token).await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
        Self::decode(resp).await
    }

    /// Execute one action against an existing run; returns the observation.
    pub async fn step(&self, params: StepParams<'_>) -> Result<WireObservation, AgentClientError> {
        let org_id = params.org_id;
        let run_id = params.run_id;
        let body = StepRequest {
            run_id: run_id.to_owned(),
            lease_id: params.lease_id.to_owned(),
            grant_id: params.grant_id.to_owned(),
            action: params.action,
            instruction: None,
            constraints: params.constraints.clone(),
            zdr: params.zdr,
            initiated_by: params.initiated_by,
        };
        let endpoint = self.url(&format!("/v1/agent/runs/{run_id}/step"));
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = with_request_id(
                self.http
                    .post(&endpoint)
                    .bearer_auth(&token)
                    .header("x-quarry-org", org_id),
                params.request_id,
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, &token).await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
        if let Some(challenge) = challenge_from_refusal(status.as_u16(), &text) {
            return Err(AgentClientError::Challenge(challenge));
        }
        Self::decode_text(status, &text)
    }

    /// Release a run and its browser lease. Best-effort; non-2xx is surfaced.
    pub async fn close_run(&self, run_id: &str, org_id: &str) -> Result<(), AgentClientError> {
        self.close_run_with(run_id, org_id, None).await
    }

    /// [`Self::close_run`] carrying an `x-request-id` correlation header.
    pub async fn close_run_with(
        &self,
        run_id: &str,
        org_id: &str,
        request_id: Option<&str>,
    ) -> Result<(), AgentClientError> {
        let endpoint = self.url(&format!("/v1/agent/runs/{run_id}"));
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = with_request_id(
                self.http
                    .delete(&endpoint)
                    .bearer_auth(&token)
                    .header("x-quarry-org", org_id),
                request_id,
            )
            .send()
            .await
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !retried_unauthorized {
                self.invalidate_if_matches(org_id, &token).await?;
                retried_unauthorized = true;
                continue;
            }
            break resp;
        };
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(Self::refusal(status.as_u16(), body))
        }
    }

    /// A non-2xx answer: typed when the body is a Quarry error envelope,
    /// otherwise the raw status and body.
    fn refusal(status: u16, body: String) -> AgentClientError {
        match parse_typed_error(status, &body) {
            Some(typed) => AgentClientError::Typed(typed),
            None => AgentClientError::Status { status, body },
        }
    }

    /// Decode a Quarry `Envelope<T>` response, returning the unwrapped `data`.
    async fn decode<T: for<'de> Deserialize<'de>>(
        resp: reqwest::Response,
    ) -> Result<T, AgentClientError> {
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| AgentClientError::Transport(e.to_string()))?;
        Self::decode_text(status, &text)
    }

    fn decode_text<T: for<'de> Deserialize<'de>>(
        status: reqwest::StatusCode,
        text: &str,
    ) -> Result<T, AgentClientError> {
        if !status.is_success() {
            return Err(Self::refusal(status.as_u16(), text.to_owned()));
        }
        let env: WireEnvelope<T> =
            serde_json::from_str(text).map_err(|e| AgentClientError::Decode(e.to_string()))?;
        if let Some(err) = env.error {
            return Err(AgentClientError::Status {
                status: status.as_u16(),
                body: err.to_string(),
            });
        }
        env.data
            .ok_or_else(|| AgentClientError::Decode("envelope contained no data".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn act(action_type: ActionType, selector: &str, value: &str, url: &str) -> BrowserAction {
        BrowserAction {
            action_id: "act_1".to_owned(),
            grant_id: "grant_1".to_owned(),
            action_type,
            selector: selector.to_owned(),
            value: value.to_owned(),
            url: url.to_owned(),
            max_wait_ms: 3000,
            target: None,
            reason: String::new(),
            risk_category: None,
        }
    }

    #[test]
    fn agent_action_serializes_internally_tagged_snake_case() {
        let v = serde_json::to_value(AgentAction::Navigate {
            url: "https://example.com".into(),
        })
        .unwrap();
        assert_eq!(v["type"], "navigate");
        assert_eq!(v["url"], "https://example.com");

        let t = serde_json::to_value(AgentAction::Type {
            selector: "#q".into(),
            text: "hi".into(),
        })
        .unwrap();
        assert_eq!(t["type"], "type");
        assert_eq!(t["selector"], "#q");
        assert_eq!(t["text"], "hi");

        assert_eq!(
            serde_json::to_value(AgentAction::GetContent).unwrap()["type"],
            "get_content"
        );
    }

    #[test]
    fn action_to_wire_maps_each_internal_variant() {
        assert_eq!(
            action_to_wire(&act(ActionType::Goto, "", "", "https://x.io")),
            AgentAction::Navigate {
                url: "https://x.io".into()
            }
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Click, "#btn", "", "")),
            AgentAction::Click {
                selector: "#btn".into()
            }
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Type, "#q", "hello", "")),
            AgentAction::Type {
                selector: "#q".into(),
                text: "hello".into()
            }
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Extract, "", "", "")),
            AgentAction::GetContent
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Observe, "", "", "")),
            AgentAction::GetContent
        );
        assert_eq!(
            action_to_wire(&act(ActionType::Wait, "", "", "")),
            AgentAction::Wait { ms: 3000 }
        );
    }

    #[test]
    fn observation_from_wire_maps_fields_and_blocked() {
        let mut wire = WireObservation {
            run_id: "run_1".into(),
            step: 2,
            url: "https://example.com/p".into(),
            title: Some("Title".into()),
            dom_summary: Some(DomSummary {
                node_count: 10,
                interactive_elements: vec![],
                text_snippet: Some("page text".into()),
            }),
            ..Default::default()
        };
        let obs = observation_from_wire(&wire, "act_9", "grant_9");
        assert_eq!(obs.status, ObservationStatus::Success);
        assert_eq!(obs.page_url, "https://example.com/p");
        assert_eq!(obs.page_title, "Title");
        assert_eq!(obs.extracted_text, "page text");
        assert_eq!(obs.action_id, "act_9");

        wire.policy_denials = vec!["domain not allowed".into()];
        let blocked = observation_from_wire(&wire, "act_9", "grant_9");
        assert_eq!(blocked.status, ObservationStatus::Blocked);
        assert!(blocked.error_message.contains("domain not allowed"));
    }

    fn wire_from(json: serde_json::Value) -> WireObservation {
        serde_json::from_value(json).expect("an observation never fails to parse")
    }

    #[test]
    fn observation_from_wire_maps_challenge() {
        let wire = wire_from(serde_json::json!({
            "run_id": "run_1",
            "step": 3,
            "url": "https://example.com/",
            "challenge": {
                "kind": "waf",
                "confidence": 0.874,
                "evidence": ["page text the planner must never see"],
                "requires_escalation": true,
                "strength": "strong",
                "block_reason": "waf_interstitial",
                "recovery": "available"
            }
        }));
        let obs = observation_from_wire(&wire, "act_1", "grant_1");
        assert_eq!(obs.status, ObservationStatus::Success);
        assert_eq!(
            obs.challenge,
            Some(BrowserChallenge {
                kind: ChallengeKind::Waf,
                strength: ChallengeStrength::Strong,
                block_reason: Some(BlockReason::WafInterstitial),
                recovery: ChallengeRecovery::Available,
                confidence_per_mille: 874,
            })
        );
        // The wire value kept for the MCP output holds typed fields only.
        let echoed = serde_json::to_string(&wire).unwrap();
        assert!(!echoed.contains("never see"), "{echoed}");

        let weak = wire_from(serde_json::json!({
            "challenge": {"kind": "login", "confidence": 0.4, "strength": "non_signal",
                           "requires_escalation": false}
        }));
        let challenge = observation_from_wire(&weak, "a", "g").challenge.unwrap();
        assert_eq!(challenge.kind, ChallengeKind::Login);
        assert_eq!(challenge.strength, ChallengeStrength::NonSignal);
        assert_eq!(challenge.block_reason, None);
        assert_eq!(challenge.recovery, ChallengeRecovery::NotApplicable);
        assert!(!challenge.is_strong());
    }

    #[test]
    fn an_observation_from_an_older_quarry_has_no_challenge() {
        let wire = wire_from(serde_json::json!({"run_id": "r", "step": 1, "url": "https://x.io/"}));
        assert!(wire.challenge.is_none());
        assert!(observation_from_wire(&wire, "a", "g").challenge.is_none());
        assert!(!serde_json::to_string(&wire).unwrap().contains("challenge"));

        // A signal from before the strength field existed reads as strong.
        let legacy = wire_from(serde_json::json!({"challenge": {"kind": "captcha", "confidence": 0.9}}));
        let challenge = observation_from_wire(&legacy, "a", "g").challenge.unwrap();
        assert_eq!(challenge.strength, ChallengeStrength::Strong);
    }

    #[test]
    fn an_unknown_vocabulary_value_degrades_and_never_fails_the_observation() {
        let wire = wire_from(serde_json::json!({
            "url": "https://example.com/",
            "challenge": {
                "kind": "shadow_ban",
                "confidence": 7.5,
                "strength": "catastrophic",
                "block_reason": "moon_phase",
                "recovery": "later"
            }
        }));
        let obs = observation_from_wire(&wire, "a", "g");
        assert_eq!(obs.page_url, "https://example.com/");
        let challenge = obs.challenge.expect("the signal is kept, degraded");
        assert_eq!(challenge.kind, ChallengeKind::Unknown);
        // Never a verdict this build cannot name.
        assert_eq!(challenge.strength, ChallengeStrength::Weak);
        assert_eq!(challenge.block_reason, None);
        assert_eq!(challenge.recovery, ChallengeRecovery::NotApplicable);
        assert_eq!(challenge.confidence_per_mille, 1000);

        // A challenge that is not an object at all is dropped, the page is not.
        for bad in [
            serde_json::json!("blocked"),
            serde_json::json!(7),
            serde_json::json!({"kind": "waf", "confidence": "high"}),
        ] {
            let wire = wire_from(serde_json::json!({"url": "https://example.com/", "challenge": bad}));
            assert_eq!(wire.url, "https://example.com/");
            assert!(wire.challenge.is_none());
        }
    }

    #[test]
    fn challenge_confidence_is_clamped_to_per_mille() {
        let at = |confidence: f32| {
            challenge_from_wire(&WireChallenge {
                kind: "waf".into(),
                confidence,
                strength: "strong".into(),
                block_reason: None,
                recovery: "available".into(),
            })
            .confidence_per_mille
        };
        assert_eq!(at(0.0), 0);
        assert_eq!(at(-3.0), 0);
        assert_eq!(at(f32::NAN), 0);
        assert_eq!(at(0.5), 500);
        assert_eq!(at(42.0), 1000);
    }

    fn refusal_body(details: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "data": null,
            "meta": {"request_id": "req_9"},
            "error": {
                "code": "CHALLENGE_DETECTED",
                "message": "the site refuses automation",
                "retryable": false,
                "details": details
            }
        })
    }

    async fn step_against(status: u16, body: serde_json::Value) -> AgentClientError {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(status).set_body_json(body))
            .mount(&server)
            .await;
        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        client
            .step(StepParams::new(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            ))
            .await
            .expect_err("a refusal is an error from step")
    }

    #[tokio::test]
    async fn a_403_challenge_detected_maps_to_a_challenge_not_a_status_error() {
        // The agent path: `details.reason`, recovery exhausted, a host.
        let err = step_against(
            403,
            refusal_body(serde_json::json!({
                "reason": "challenge_loop",
                "recovery": "exhausted",
                "next": "user_takeover_or_other_source",
                "host": "shop.example.com"
            })),
        )
        .await;
        let AgentClientError::Challenge(challenge) = err else {
            panic!("expected Challenge, got {err:?}");
        };
        assert!(challenge.is_strong());
        assert_eq!(challenge.block_reason, Some(BlockReason::ChallengeLoop));
        assert_eq!(challenge.recovery, ChallengeRecovery::Exhausted);

        // The scrape path names the reason `block_reason`.
        let err = step_against(
            403,
            refusal_body(serde_json::json!({
                "reason": "captcha_widget",
                "block_reason": "captcha_widget",
                "recovery": "not_applicable",
                "next": "other_source"
            })),
        )
        .await;
        assert!(matches!(
            err,
            AgentClientError::Challenge(BrowserChallenge {
                block_reason: Some(BlockReason::CaptchaWidget),
                ..
            })
        ));

        // The code is the verdict: a reason this build does not know, or none,
        // is still a refusal, with no reason invented.
        for details in [serde_json::json!({"reason": "moon_phase"}), serde_json::Value::Null] {
            let err = step_against(403, refusal_body(details)).await;
            assert!(
                matches!(
                    err,
                    AgentClientError::Challenge(BrowserChallenge {
                        block_reason: None,
                        strength: ChallengeStrength::Strong,
                        ..
                    })
                ),
                "{err:?}"
            );
        }
    }

    #[tokio::test]
    async fn other_403s_and_other_statuses_are_typed_errors_not_challenges() {
        for (status, code) in [
            (403, "FORBIDDEN"),
            (403, "SECURITY_BLOCKED"),
            (403, "USER_TOOK_OVER"),
            (409, "CHALLENGE_DETECTED"),
            (429, "CHALLENGE_DETECTED"),
        ] {
            let mut body = refusal_body(serde_json::json!({"reason": "captcha_widget"}));
            body["error"]["code"] = serde_json::json!(code);
            let err = step_against(status, body).await;
            assert!(
                matches!(&err, AgentClientError::Typed(t) if t.status == status && t.code == code),
                "{status} {code}: {err:?}"
            );
        }
        // A 403 with a body that is not an envelope at all.
        let err = step_against(403, serde_json::json!("nope")).await;
        assert!(matches!(err, AgentClientError::Status { status: 403, .. }), "{err:?}");
    }

    #[tokio::test]
    async fn a_step_observation_with_a_challenge_comes_through_the_client() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "run_id": "run_abc", "step": 2, "url": "https://example.com/",
                    "challenge": {"kind": "captcha", "confidence": 0.9, "strength": "strong",
                                  "block_reason": "captcha_widget", "recovery": "available"}
                },
                "meta": {}, "error": null
            })))
            .mount(&server)
            .await;
        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let wire = client
            .step(StepParams::new(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            ))
            .await
            .unwrap();
        let obs = observation_from_wire(&wire, "a", "g");
        assert_eq!(obs.status, ObservationStatus::Success);
        assert_eq!(
            obs.challenge.map(|c| (c.kind, c.block_reason)),
            Some((ChallengeKind::Captcha, Some(BlockReason::CaptchaWidget)))
        );
    }

    #[test]
    fn a_refused_action_becomes_a_blocked_observation_carrying_the_signal() {
        let challenge = challenge_from_refusal(
            403,
            &refusal_body(serde_json::json!({"reason": "challenge_loop"})).to_string(),
        )
        .expect("a CHALLENGE_DETECTED refusal");
        let obs = observation_from_challenge_refusal(
            challenge.clone(),
            "act_0004",
            "grant_1",
            "https://shop.example.com/",
        );
        assert_eq!(obs.status, ObservationStatus::Blocked);
        assert_eq!(obs.page_url, "https://shop.example.com/");
        assert_eq!(obs.challenge, Some(challenge));
        assert!(obs.error_message.contains("block_reason=challenge_loop"));
        assert!(obs.extracted_text.is_empty() && obs.snapshot_targets.is_empty());
    }

    #[tokio::test]
    async fn start_run_posts_and_unwraps_envelope() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs"))
            .and(header("authorization", "Bearer tok"))
            .and(header("x-quarry-org", "org_1"))
            .and(body_json(serde_json::json!({
                "org_id": "org_1",
                "grant_id": "grant_broker_validated",
                "constraints": {"max_steps": 0, "allowed_domains": []},
                "zdr": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "run_id": "run_abc", "lease_id": "lease_xyz" },
                "meta": { "request_id": "req_1" },
                "error": null,
            })))
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let resp = client
            .start_run(StartRunParams::new(
                "org_1",
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            ))
            .await
            .unwrap();
        assert_eq!(resp.run_id, "run_abc");
        assert_eq!(resp.lease_id, "lease_xyz");
    }

    #[tokio::test]
    async fn step_posts_action_and_unwraps_observation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .and(body_json(serde_json::json!({
                "run_id": "run_abc",
                "lease_id": "lease_xyz",
                "grant_id": "grant_broker_validated",
                "action": {"type": "get_content"},
                "constraints": {"max_steps": 0, "allowed_domains": []},
                "zdr": false,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "run_id": "run_abc",
                    "step": 1,
                    "url": "https://example.com",
                    "title": "Example",
                    "dom_summary": {"node_count": 3, "interactive_elements": [], "text_snippet": "hi"},
                    "observed_at": "2026-05-30T00:00:00Z",
                },
                "meta": { "request_id": "req_2" },
                "error": null,
            })))
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let obs = client
            .step(StepParams::new(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            ))
            .await
            .unwrap();
        assert_eq!(obs.url, "https://example.com");
        assert_eq!(obs.title.as_deref(), Some("Example"));
        let mapped = observation_from_wire(&obs, "act_1", "grant_1");
        assert_eq!(mapped.extracted_text, "hi");
    }

    fn envelope_body(code: &str, message: &str, details: serde_json::Value) -> String {
        serde_json::json!({
            "data": null,
            "meta": {"request_id": "req_1"},
            "error": {"code": code, "message": message, "retryable": false, "details": details}
        })
        .to_string()
    }

    #[test]
    fn typed_error_reads_code_message_and_details_reason() {
        let body = envelope_body(
            "FORBIDDEN",
            "navigation denied",
            serde_json::json!({"reason": "policy_denied", "capability": "browser.act.upload"}),
        );
        let typed = parse_typed_error(403, &body).expect("an envelope");
        assert_eq!(typed.status, 403);
        assert_eq!(typed.code, "FORBIDDEN");
        assert_eq!(typed.message, "navigation denied");
        assert_eq!(typed.reason.as_deref(), Some("policy_denied"));
        assert_eq!(typed.details["capability"], "browser.act.upload");
    }

    #[test]
    fn typed_error_attach_exists_reason_comes_from_message() {
        // Attach refusals carry the reason in `message`, with no details.reason.
        let body = envelope_body("CONFLICT", "attach_exists", serde_json::Value::Null);
        let typed = parse_typed_error(409, &body).expect("an envelope");
        assert_eq!(typed.reason.as_deref(), Some("attach_exists"));
        // Prose is not a reason.
        let body = envelope_body("CONFLICT", "An attach already exists for this tab", serde_json::Value::Null);
        assert_eq!(parse_typed_error(409, &body).unwrap().reason, None);
        // details.reason wins over the message.
        let body = envelope_body("CONFLICT", "attach_exists", serde_json::json!({"reason": "other_reason"}));
        assert_eq!(
            parse_typed_error(409, &body).unwrap().reason.as_deref(),
            Some("other_reason")
        );
    }

    #[test]
    fn parse_typed_error_reads_code_reason_and_retry_after() {
        let body = envelope_body(
            "RATE_LIMITED",
            "slow down",
            serde_json::json!({"reason": "attach_steps", "retry_after_seconds": 7}),
        );
        let typed = parse_typed_error(429, &body).unwrap();
        assert_eq!(typed.reason.as_deref(), Some("attach_steps"));
        assert_eq!(typed.retry_after_seconds, Some(7));
    }

    #[test]
    fn parse_typed_error_tolerates_a_non_envelope_body() {
        for body in ["", "domain violation", "\"nope\"", "[1,2]", "{}", "{\"error\":null}", "{\"error\":{\"code\":5}}"] {
            assert!(parse_typed_error(500, body).is_none(), "{body:?}");
        }
        // A code with no message or details still parses.
        let typed = parse_typed_error(500, "{\"error\":{\"code\":\"INTERNAL\"}}").unwrap();
        assert_eq!(typed.message, "");
        assert_eq!(typed.reason, None);
        assert!(typed.details.is_null());
    }

    #[test]
    fn start_run_omits_policy_and_trace_when_none() {
        let constraints = AgentConstraints::default();
        let mut params = StartRunParams::new("org_1", &constraints, false, "g");
        let body = StartRunRequest {
            org_id: "org_1".into(),
            grant_id: "g".into(),
            constraints: constraints.clone(),
            profile_id: None,
            zdr: false,
            policy: params.policy.take(),
            trace: params.trace.take(),
        };
        let json = serde_json::to_value(&body).unwrap();
        assert!(json.get("policy").is_none() && json.get("trace").is_none(), "{json}");

        let body = StartRunRequest {
            policy: Some(WireRequestAsk {
                disable: vec!["browser.act.upload".into()],
                deny_domains: vec![],
                allow_domains: None,
            }),
            trace: Some(WireRunTrace {
                run_id: Some("run_1".into()),
                agent: Some(WireRunAgent {
                    id: Some("a1".into()),
                    role: None,
                    parent_id: None,
                }),
                origin: None,
            }),
            ..body
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["policy"], serde_json::json!({"disable": ["browser.act.upload"]}));
        assert_eq!(
            json["trace"],
            serde_json::json!({"run_id": "run_1", "agent": {"id": "a1"}})
        );
    }

    #[test]
    fn step_request_serialises_initiated_by_only_when_set() {
        let mut body = StepRequest {
            run_id: "r".into(),
            lease_id: "l".into(),
            grant_id: "g".into(),
            action: AgentAction::GetContent,
            instruction: None,
            constraints: AgentConstraints::default(),
            zdr: false,
            initiated_by: None,
        };
        assert!(serde_json::to_value(&body).unwrap().get("initiated_by").is_none());
        body.initiated_by = Some("agent".into());
        assert_eq!(serde_json::to_value(&body).unwrap()["initiated_by"], "agent");
    }

    #[tokio::test]
    async fn request_id_header_is_sent_when_present() {
        let server = MockServer::start().await;
        let ok = |data: serde_json::Value| {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": data, "meta": {}, "error": null
            }))
        };
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs"))
            .and(header("x-request-id", "req-abc"))
            .respond_with(ok(serde_json::json!({"run_id": "run_abc", "lease_id": "lease_xyz"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .and(header("x-request-id", "req-abc"))
            .respond_with(ok(serde_json::json!({"run_id": "run_abc", "url": "https://example.com"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/v1/agent/runs/run_abc"))
            .and(header("x-request-id", "req-abc"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let constraints = AgentConstraints::default();
        let mut start = StartRunParams::new("org_1", &constraints, false, "g");
        start.request_id = Some("req-abc");
        client.start_run(start).await.unwrap();
        let mut step = StepParams::new("run_abc", "lease_xyz", "org_1", AgentAction::GetContent, &constraints, false, "g");
        step.request_id = Some("req-abc");
        client.step(step).await.unwrap();
        client.close_run_with("run_abc", "org_1", Some("req-abc")).await.unwrap();

        // Absent id: no header is sent (a header matcher on the other side
        // would not match, so assert on the received request instead).
        let quiet = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs"))
            .respond_with(ok(serde_json::json!({"run_id": "r", "lease_id": "l"})))
            .mount(&quiet)
            .await;
        let client = QuarryAgentClient::new(quiet.uri(), "tok").unwrap();
        client
            .start_run(StartRunParams::new("org_1", &constraints, false, "g"))
            .await
            .unwrap();
        let received = quiet.received_requests().await.unwrap();
        assert!(received[0].headers.get("x-request-id").is_none());
    }

    #[test]
    fn recoverable_arms_cover_stale_timeout_driver_and_unsupported() {
        use RecoverableKind as K;
        for (err, kind) in [
            (typed(409, "TARGET_REPAIR_REQUIRED", serde_json::Value::Null), K::StaleTarget),
            (typed(409, "ACTION_UNKNOWN", serde_json::Value::Null), K::StaleTarget),
            (typed(504, "TIMEOUT", serde_json::Value::Null), K::ActionTimeout),
            (typed(502, "DRIVER_FAILED", serde_json::Value::Null), K::DriverFailed),
            (typed(501, "NOT_IMPLEMENTED", serde_json::Value::Null), K::UnsupportedAction),
            (AgentClientError::Status { status: 501, body: String::new() }, K::UnsupportedAction),
        ] {
            assert_eq!(recoverable_kind(&err), Some(kind), "{err:?}");
            assert_eq!(classify_quarry_error(&err), StepFailureClass::Recoverable, "{err:?}");
        }
        // A kind names no page content.
        assert!(!K::StaleTarget.advice().is_empty() && !K::ActionTimeout.as_str().is_empty());
    }

    #[test]
    fn backpressure_wait_is_bounded_to_ten_seconds() {
        let wait = |details| backpressure_wait(&typed(429, "RATE_LIMITED", details));
        assert_eq!(wait(serde_json::Value::Null), Some(Duration::from_secs(1)));
        assert_eq!(wait(serde_json::json!({"retry_after_seconds": 7})), Some(Duration::from_secs(7)));
        assert_eq!(wait(serde_json::json!({"retry_after_seconds": 10})), Some(Duration::from_secs(10)));
        assert_eq!(wait(serde_json::json!({"retry_after_seconds": 11})), None);
        assert_eq!(backpressure_wait(&typed(500, "INTERNAL", serde_json::Value::Null)), None);
        assert_eq!(
            backpressure_wait(&AgentClientError::Status { status: 429, body: String::new() }),
            Some(Duration::from_secs(1))
        );
    }

    fn typed(status: u16, code: &str, details: serde_json::Value) -> AgentClientError {
        AgentClientError::Typed(parse_typed_error(status, &envelope_body(code, "m", details)).unwrap())
    }

    #[test]
    fn fixed_error_text_never_carries_the_quarry_message() {
        let body = serde_json::json!({
            "data": null, "meta": {},
            "error": {"code": "FORBIDDEN", "message": "navigation to https://evil.example/x?token=abc denied",
                      "details": {"reason": "policy_denied"}}
        })
        .to_string();
        let typed = parse_typed_error(403, &body).expect("an envelope");
        let text = fixed_error_text(&AgentClientError::Typed(typed));
        assert_eq!(text, "403 FORBIDDEN (policy_denied)");
        let odd = serde_json::json!({"error": {"code": "not a code!", "message": "m",
            "details": {"reason": "Has Spaces And https://x"}}})
        .to_string();
        let typed = parse_typed_error(500, &odd).expect("an envelope");
        assert_eq!(fixed_error_text(&AgentClientError::Typed(typed)), "500 UNKNOWN");
    }

    #[test]
    fn classify_maps_402_budget_and_429_to_resource_limit() {
        let budget = typed(402, "BUDGET_EXHAUSTED", serde_json::json!({"dimension": "cost", "limit": 5}));
        assert_eq!(quarry_budget_dimension(&budget), Some(BudgetDimension::Cost));
        assert_eq!(classify_quarry_error(&budget), StepFailureClass::ResourceLimit);

        let unknown = typed(402, "BUDGET_EXHAUSTED", serde_json::json!({"dimension": "moon"}));
        assert_eq!(quarry_budget_dimension(&unknown), Some(BudgetDimension::Other));
        let bare = typed(402, "BUDGET_EXHAUSTED", serde_json::Value::Null);
        assert_eq!(quarry_budget_dimension(&bare), Some(BudgetDimension::Other));

        // 429 with and without an envelope is backpressure.
        let rate = typed(429, "RATE_LIMITED", serde_json::json!({"retry_after_seconds": 3}));
        assert_eq!(classify_quarry_error(&rate), StepFailureClass::ResourceLimit);
        let raw = AgentClientError::Status { status: 429, body: "slow".into() };
        assert_eq!(classify_quarry_error(&raw), StepFailureClass::ResourceLimit);

        // Everything else is terminal.
        for err in [
            typed(402, "PAYMENT_REQUIRED", serde_json::Value::Null),
            typed(403, "FORBIDDEN", serde_json::Value::Null),
            typed(500, "INTERNAL", serde_json::Value::Null),
            typed(409, "CONFLICT", serde_json::Value::Null),
            typed(504, "GATEWAY_TIMEOUT", serde_json::Value::Null),
            AgentClientError::Status { status: 401, body: String::new() },
            AgentClientError::Transport("boom".into()),
        ] {
            assert_eq!(classify_quarry_error(&err), StepFailureClass::Terminal, "{err:?}");
            assert_eq!(quarry_budget_dimension(&err), None);
        }
    }

    #[tokio::test]
    async fn step_surfaces_non_2xx_as_status_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(403).set_body_string("domain violation"))
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "tok").unwrap();
        let err = client
            .step(StepParams::new(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::Navigate {
                    url: "https://evil.io".into(),
                },
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            ))
            .await
            .unwrap_err();
        match err {
            AgentClientError::Status { status, .. } => assert_eq!(status, 403),
            other => panic!("expected Status error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn step_retries_exactly_once_after_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/agent/runs/run_abc/step"))
            .respond_with(ResponseTemplate::new(401))
            .expect(2)
            .mount(&server)
            .await;

        let client = QuarryAgentClient::new(server.uri(), "expired").unwrap();
        let error = client
            .step(StepParams::new(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            ))
            .await
            .expect_err("second 401 must be surfaced");
        assert!(matches!(
            error,
            AgentClientError::Status { status: 401, .. }
        ));
    }
}
