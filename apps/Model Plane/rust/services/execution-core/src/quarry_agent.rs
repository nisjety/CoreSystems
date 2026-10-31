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
    BrowserSnapshotTarget, BrowserTargetRef, ChallengeKind, ChallengeRecovery, ChallengeStrength,
    ObservationStatus,
};
use crate::quarry_auth::TokenSource;

const AGENT_SCOPES: &[&str] = &["browser:execute"];

/// How long the readiness probe waits for Quarry's `/ready` answer.
const READY_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

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

/// The inverse of [`action_to_wire`], for the actions the loop can name. A wire action with no loop
/// equivalent (a select, a key press, a script) is `None`, so a caller that judges an action by the
/// loop's vocabulary says it has nothing to judge rather than guessing.
#[must_use]
pub fn browser_action_from_wire(action: &AgentAction) -> Option<BrowserAction> {
    fn loop_action(action_type: ActionType) -> BrowserAction {
        BrowserAction {
            action_id: String::new(),
            grant_id: String::new(),
            action_type,
            selector: String::new(),
            value: String::new(),
            url: String::new(),
            max_wait_ms: 0,
            target: None,
            reason: String::new(),
            risk_category: None,
        }
    }
    fn target_ref(
        snapshot_id: &str,
        generation: u32,
        frame_id: Option<&str>,
        ref_id: &str,
    ) -> Option<BrowserTargetRef> {
        Some(BrowserTargetRef {
            snapshot_id: snapshot_id.to_owned(),
            generation,
            ref_id: ref_id.to_owned(),
            frame_id: frame_id.map(str::to_owned),
        })
    }
    Some(match action {
        AgentAction::Navigate { url } => BrowserAction {
            url: url.clone(),
            ..loop_action(ActionType::Goto)
        },
        AgentAction::Click { selector } => BrowserAction {
            selector: selector.clone(),
            ..loop_action(ActionType::Click)
        },
        AgentAction::ClickRef {
            snapshot_id,
            generation,
            ref_id,
        } => BrowserAction {
            target: target_ref(snapshot_id, *generation, None, ref_id),
            ..loop_action(ActionType::Click)
        },
        AgentAction::FrameClickRef {
            snapshot_id,
            generation,
            frame_id,
            ref_id,
        } => BrowserAction {
            target: target_ref(snapshot_id, *generation, Some(frame_id), ref_id),
            ..loop_action(ActionType::Click)
        },
        AgentAction::Type { selector, text } => BrowserAction {
            selector: selector.clone(),
            value: text.clone(),
            ..loop_action(ActionType::Type)
        },
        AgentAction::TypeRef {
            snapshot_id,
            generation,
            ref_id,
            text,
        } => BrowserAction {
            target: target_ref(snapshot_id, *generation, None, ref_id),
            value: text.clone(),
            ..loop_action(ActionType::Type)
        },
        AgentAction::FrameTypeRef {
            snapshot_id,
            generation,
            frame_id,
            ref_id,
            text,
        } => BrowserAction {
            target: target_ref(snapshot_id, *generation, Some(frame_id), ref_id),
            value: text.clone(),
            ..loop_action(ActionType::Type)
        },
        // `action_to_wire` sends a scroll's target as its value or, failing that, its selector; the
        // classifier reads only the selector, so the target comes back as the selector.
        AgentAction::Scroll { target } => BrowserAction {
            selector: target.clone(),
            ..loop_action(ActionType::Scroll)
        },
        AgentAction::Wait { ms } => BrowserAction {
            max_wait_ms: i32::try_from(*ms).unwrap_or(i32::MAX),
            ..loop_action(ActionType::Wait)
        },
        AgentAction::GetContent => loop_action(ActionType::Observe),
        _ => return None,
    })
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
        org_id: &str,
        constraints: &AgentConstraints,
        zdr: bool,
        profile_id: Option<String>,
        grant_id: &str,
    ) -> Result<StartRunResponse, AgentClientError> {
        let body = StartRunRequest {
            org_id: org_id.to_owned(),
            grant_id: grant_id.to_owned(),
            constraints: constraints.clone(),
            profile_id,
            zdr,
        };
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = self
                .http
                .post(self.url("/v1/agent/runs"))
                .bearer_auth(&token)
                .header("x-quarry-org", org_id)
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
    pub async fn step(
        &self,
        run_id: &str,
        lease_id: &str,
        org_id: &str,
        action: AgentAction,
        constraints: &AgentConstraints,
        zdr: bool,
        grant_id: &str,
    ) -> Result<WireObservation, AgentClientError> {
        let body = StepRequest {
            run_id: run_id.to_owned(),
            lease_id: lease_id.to_owned(),
            grant_id: grant_id.to_owned(),
            action,
            instruction: None,
            constraints: constraints.clone(),
            zdr,
        };
        let endpoint = self.url(&format!("/v1/agent/runs/{run_id}/step"));
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = self
                .http
                .post(&endpoint)
                .bearer_auth(&token)
                .header("x-quarry-org", org_id)
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
        let endpoint = self.url(&format!("/v1/agent/runs/{run_id}"));
        let mut retried_unauthorized = false;
        let resp = loop {
            let token = self.token(org_id).await?;
            let resp = self
                .http
                .delete(&endpoint)
                .bearer_auth(&token)
                .header("x-quarry-org", org_id)
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
            Err(AgentClientError::Status {
                status: status.as_u16(),
                body,
            })
        }
    }

    /// Whether Quarry reports itself ready for agent runs: `/ready` answers 200 within the probe
    /// timeout. Read by the health reporter only; nothing here names a run.
    pub async fn ready(&self) -> bool {
        match self
            .http
            .get(self.url("/ready"))
            .timeout(READY_PROBE_TIMEOUT)
            .send()
            .await
        {
            Ok(response) => response.status().is_success(),
            Err(_) => false,
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
            return Err(AgentClientError::Status {
                status: status.as_u16(),
                body: text.to_owned(),
            });
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

    /// The wire form reads back as the loop action it was made from, for each action the loop sends,
    /// so a classifier judging the wire judges what Quarry receives. An observation reads back as
    /// an observe, the one of the two reads that `action_to_wire` maps to `get_content`.
    #[test]
    fn browser_action_from_wire_reads_back_what_action_to_wire_sends() {
        for action in [
            act(ActionType::Goto, "", "", "https://example.com/a"),
            act(ActionType::Click, "button.buy", "", ""),
            act(ActionType::Type, "input.q", "hallo", ""),
            act(ActionType::Scroll, "main", "", ""),
            act(ActionType::Wait, "", "", ""),
            act(ActionType::Observe, "", "", ""),
        ] {
            let back = browser_action_from_wire(&action_to_wire(&action)).expect("a loop action");
            assert_eq!(back.action_type, action.action_type, "{action:?}");
            assert_eq!(back.url, action.url, "{action:?}");
            assert_eq!(back.selector, action.selector, "{action:?}");
            assert_eq!(back.value, action.value, "{action:?}");
            if action.action_type == ActionType::Wait {
                assert_eq!(back.max_wait_ms, action.max_wait_ms, "{action:?}");
            }
        }
    }

    /// A reference-bound action keeps the snapshot, generation, frame and reference it names, which
    /// is what the classifier looks the element up by.
    #[test]
    fn a_reference_bound_action_reads_back_with_its_target() {
        let mut click = act(ActionType::Click, "", "", "");
        click.target = Some(BrowserTargetRef {
            snapshot_id: "snap-1".to_owned(),
            generation: 4,
            ref_id: "@e2".to_owned(),
            frame_id: None,
        });
        let back = browser_action_from_wire(&action_to_wire(&click)).expect("a loop action");
        assert_eq!(back.action_type, ActionType::Click);
        let target = back.target.expect("the target reads back");
        assert_eq!(
            (
                target.snapshot_id.as_str(),
                target.generation,
                target.ref_id.as_str(),
                target.frame_id
            ),
            ("snap-1", 4, "@e2", None)
        );

        let mut typed = act(ActionType::Type, "", "hallo", "");
        typed.target = Some(BrowserTargetRef {
            snapshot_id: "snap-1".to_owned(),
            generation: 4,
            ref_id: "@e3".to_owned(),
            frame_id: Some("frame-child".to_owned()),
        });
        let back = browser_action_from_wire(&action_to_wire(&typed)).expect("a loop action");
        assert_eq!(back.value, "hallo");
        assert_eq!(
            back.target.and_then(|target| target.frame_id).as_deref(),
            Some("frame-child")
        );
    }

    /// The wire actions the loop has no vocabulary for read back as nothing, so they are never judged
    /// by a classification they do not have.
    #[test]
    fn a_wire_action_with_no_loop_equivalent_reads_back_as_none() {
        for wire in [
            AgentAction::SelectRef {
                snapshot_id: "snap-1".to_owned(),
                generation: 1,
                ref_id: "@e1".to_owned(),
                value: "no".to_owned(),
            },
            AgentAction::Press {
                key: "Enter".to_owned(),
            },
            AgentAction::Evaluate {
                script: "1".to_owned(),
            },
            AgentAction::RespondDialog {
                dialog_id: "dialog-1".to_owned(),
                accept: true,
                approval_grant_id: "approval-1".to_owned(),
                prompt_text: None,
            },
        ] {
            assert!(browser_action_from_wire(&wire).is_none(), "{wire:?}");
        }
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
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            )
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
    async fn other_403s_and_other_statuses_stay_status_errors() {
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
                matches!(&err, AgentClientError::Status { status: s, .. } if *s == status),
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
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            )
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
    async fn ready_is_true_only_for_a_200() {
        let ready = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ready"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&ready)
            .await;
        assert!(QuarryAgentClient::new(ready.uri(), "tok").unwrap().ready().await);

        let down = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ready"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&down)
            .await;
        assert!(!QuarryAgentClient::new(down.uri(), "tok").unwrap().ready().await);
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
            .start_run(
                "org_1",
                &AgentConstraints::default(),
                false,
                None,
                "grant_broker_validated",
            )
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
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            )
            .await
            .unwrap();
        assert_eq!(obs.url, "https://example.com");
        assert_eq!(obs.title.as_deref(), Some("Example"));
        let mapped = observation_from_wire(&obs, "act_1", "grant_1");
        assert_eq!(mapped.extracted_text, "hi");
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
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::Navigate {
                    url: "https://evil.io".into(),
                },
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            )
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
            .step(
                "run_abc",
                "lease_xyz",
                "org_1",
                AgentAction::GetContent,
                &AgentConstraints::default(),
                false,
                "grant_broker_validated",
            )
            .await
            .expect_err("second 401 must be surfaced");
        assert!(matches!(
            error,
            AgentClientError::Status { status: 401, .. }
        ));
    }
}
