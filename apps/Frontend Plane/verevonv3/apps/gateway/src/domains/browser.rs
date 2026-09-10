//! Browser sessions for the Verevon in-app browser surface.
//!
//! This is a narrow facade over Quarry-v2's existing Rust browser-agent lane:
//! `/v1/agent/runs` acquires a chromiumoxide-backed run, and `/step` executes
//! one browser action at a time. The SPA never sees raw CDP or Quarry tokens.
//!
//! Phase 5 ("Approvals & policy"): a durable AI run started via `start_ai_run`
//! below now gets real per-action HITL gating for risky browser actions
//! (login, checkout, posting forms, destructive actions, cross-domain
//! navigation, persistent cookie use) — classified and enforced inside
//! execution-core's browser loop, not here. This module intentionally adds
//! **no** new approval routes: pending approvals and decisions for a browser
//! run are listed/decided through the exact same generic, already-existing
//! surface `orchestration.rs` proxies (`GET
//! /api/v1/orchestration/runs/:run_id/approvals`, `POST
//! /api/v1/orchestration/approvals/:approval_id/decide`), keyed by the AI
//! run's own `run_id` (the value `start_ai_run` returns, not the browser
//! `session_id` this whole file otherwise keys on). The browser-specific
//! rationale (action type, URL, selector, risk category) rides on the
//! existing `GET /api/v1/runs/:run_id/events` SSE stream (`chat/streams.rs`,
//! reused byte-for-byte, unchanged) as two new event kinds emitted by
//! model-gateway: `browser_action_approval_required` /
//! `browser_action_decided`.

#[cfg(test)]
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex as StdMutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Body,
    extract::{
        ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
        Extension, Path, Query, State,
    },
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message as TungsteniteMessage},
};
use url::Url;

use crate::{
    audience_tokens::{get_audience_token, get_onboarding_preview_token},
    config::AppState,
    domains::chat::shared::{
        data_plane_token, delegated_auth_unavailable, model_token, proxy_model_json,
        proxy_model_json_with_data_plane, proxy_model_json_with_session, required_execution_token,
        required_inference_token, required_session_token,
    },
    envelope::{error, ok, unwrap_data},
    middleware::{require_session, AuthenticatedUser},
    public_url::normalize_public_http_url,
    upstream::{authorized_org_id, proxy_bearer_json, proxy_sse_stream},
};

/// Mirrors the SPA's `BrowserProfileScope` union (`browser-client.ts`).
/// Phase 3 continuation: promotes profile scope from an inferred
/// `persistent_profile` boolean into a real, explicit value the caller
/// can request and the ZDR guard can check directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BrowserProfileScope {
    Ephemeral,
    UserPrivate,
    OrgShared,
    RunScoped,
}

impl BrowserProfileScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ephemeral => "ephemeral",
            Self::UserPrivate => "user_private",
            Self::OrgShared => "org_shared",
            Self::RunScoped => "run_scoped",
        }
    }

    fn from_owner_value(value: Option<&str>) -> Self {
        match value {
            Some("user_private") => Self::UserPrivate,
            Some("org_shared") => Self::OrgShared,
            Some("run_scoped") => Self::RunScoped,
            _ => Self::Ephemeral,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateSessionBody {
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) profile_id: Option<String>,
    #[serde(default)]
    pub(crate) persistent_profile: bool,
    #[serde(default)]
    pub(crate) viewport: Option<Viewport>,
    /// Phase 3: caller-requested Zero Data Retention mode. Enforcement of
    /// "a ZDR session may never attach a persistent profile" happens
    /// server-side in `create_session` (`reject_zdr_persistent_profile`) —
    /// this flag is never trusted blindly for anything else, it only gates
    /// that one check plus the metadata recorded for this session.
    #[serde(default)]
    pub(crate) zdr: bool,
    /// Phase 3 continuation: explicit scope for the profile this session
    /// attaches (new or existing). Optional for back-compat — when
    /// omitted, the effective scope is inferred from `profile_id`/
    /// `persistent_profile` exactly as before (see
    /// `effective_profile_scope`).
    #[serde(default)]
    pub(crate) scope: Option<BrowserProfileScope>,
}

/// Resolves the real scope a session's profile attachment implies, so the
/// ZDR guard can check the actual four-value scope instead of a coarse
/// boolean. A caller-supplied `scope` generally wins (it is the explicit,
/// current-generation signal); when absent, back-compat callers that only
/// ever sent `profileId`/`persistentProfile` still resolve to a
/// non-ephemeral scope so the guard's behavior is unchanged for them.
///
/// One exception, load-bearing for the ZDR guard: when the *raw* facts
/// (`profile_id`/`persistent_profile`) already imply real persistence, an
/// explicit `scope: "ephemeral"` claim can never downgrade that back to
/// ephemeral. Without this, a client could send
/// `{zdr: true, profileId: "<real>", scope: "ephemeral"}` and pass
/// `reject_zdr_persistent_profile` (which only inspects the *resolved*
/// scope) while `create_session` still forwards the real `profile_id` to
/// Quarry — a ZDR-labeled session would end up attached to a real
/// persisted profile's cookies. The effective scope is therefore the more
/// restrictive of "what the raw facts imply" and "what the caller claims";
/// a caller can still *widen* an implied `user_private` up to
/// `org_shared`/`run_scoped` (unchanged from before), just never launder
/// real persistence signals down to `ephemeral`.
fn effective_profile_scope(
    profile_id: Option<&str>,
    persistent_profile: bool,
    requested_scope: Option<BrowserProfileScope>,
) -> BrowserProfileScope {
    let implied_by_facts = if profile_id.is_some() || persistent_profile {
        BrowserProfileScope::UserPrivate
    } else {
        BrowserProfileScope::Ephemeral
    };
    match requested_scope {
        Some(BrowserProfileScope::Ephemeral)
            if implied_by_facts != BrowserProfileScope::Ephemeral =>
        {
            implied_by_facts
        }
        Some(scope) => scope,
        None => implied_by_facts,
    }
}

/// Phase 3 ZDR enforcement: a ZDR session must never be able to select or
/// attach a persisted browser profile of ANY scope — persisted cookies/
/// storage would defeat the point of "no data retained for this session".
/// This is checked server-side, before any upstream Quarry call, so a
/// client can't bypass it by racing the request body against stale UI
/// state or by claiming a scope the guard doesn't recognize as persistent.
fn reject_zdr_persistent_profile(
    zdr: bool,
    scope: BrowserProfileScope,
) -> Option<(&'static str, &'static str)> {
    if zdr && scope != BrowserProfileScope::Ephemeral {
        Some((
            "zdr_persistent_profile_forbidden",
            "A Zero Data Retention session cannot use a persistent browser profile.",
        ))
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) type BrowserRunStore = Arc<StdMutex<HashMap<String, BrowserRunMetadata>>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BrowserRunOwner {
    user_id: String,
    org_id: String,
}

#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct BrowserRunMetadata {
    owner: BrowserRunOwner,
    lease_id: Option<String>,
    profile_id: Option<String>,
    /// Phase 3 continuation: the real requested/inferred scope (see
    /// `effective_profile_scope`) — replaces the old two-value
    /// `persistent_profile ? user_private : run_scoped` guess the session
    /// response used to render.
    profile_scope: BrowserProfileScope,
    last_observation: Option<Value>,
    observation_history: Vec<Value>,
    devtools_events: Vec<Value>,
    replay_events: Vec<Value>,
    tabs: Vec<Value>,
    viewport: Viewport,
    control_mode: BrowserControlMode,
    zdr: bool,
}

/// The narrow, non-durable state a BFF request needs from Quarry's owner
/// projection. It is reconstructed per request and deliberately has no owner,
/// replay, tab, or devtools cache fields.
#[derive(Debug, Clone)]
struct BrowserOwnerSessionState {
    profile_id: Option<String>,
    profile_scope: BrowserProfileScope,
    last_observation: Option<Value>,
    viewport: Viewport,
    zdr: bool,
}

#[derive(Debug, Clone, Copy)]
struct BrowserArtifactRunAccess {
    zdr: bool,
}

#[cfg(test)]
pub(crate) fn new_browser_run_store() -> BrowserRunStore {
    Arc::new(StdMutex::new(HashMap::new()))
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub(crate) struct Viewport {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ActionBody {
    pub(crate) action: Value,
    #[serde(default)]
    pub(crate) actor: BrowserActionActor,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BrowserActionActor {
    Agent,
    #[default]
    Human,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BrowserControlMode {
    AgentControl,
    HumanTakeover,
}

impl BrowserControlMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::AgentControl => "agent_control",
            Self::HumanTakeover => "human_takeover",
        }
    }

    fn from_owner_value(value: Option<&str>) -> Self {
        match value {
            Some("human_takeover") => Self::HumanTakeover,
            _ => Self::AgentControl,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ControlBody {
    pub(crate) mode: BrowserControlMode,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewTabBody {
    #[serde(default)]
    pub(crate) url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SuggestActionBody {
    #[serde(default)]
    pub(crate) goal: String,
    #[serde(default = "default_include_screenshot")]
    pub(crate) include_screenshot: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RestoreProbeBody {
    pub(crate) url: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StartAiRunBody {
    pub(crate) goal: String,
    #[serde(default)]
    pub(crate) allowed_domains: Option<Vec<String>>,
    #[serde(default)]
    pub(crate) max_steps: Option<i32>,
    #[serde(default)]
    pub(crate) max_runtime_s: Option<i32>,
    #[serde(default)]
    pub(crate) stop_criteria: Option<String>,
    /// Accepted for API back-compat only — deliberately never read. See
    /// `build_ai_run_request`'s doc comment: this controls a confirmed-broken
    /// legacy gate in execution-core, and the gateway must never arm it.
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) require_approval: Option<bool>,
    #[serde(default)]
    pub(crate) max_cost_usd: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FrameQuery {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    quality: Option<u8>,
    #[serde(default)]
    max_width: Option<u32>,
    #[serde(default)]
    max_height: Option<u32>,
}

const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 1280,
    height: 800,
};
const MAX_BROWSER_ARTIFACT_BYTES: u64 = 24 * 1024 * 1024;
const MAX_MODEL_SCREENSHOT_BYTES: u64 = 6 * 1024 * 1024;
const MAX_MODEL_VISUAL_JSON_BYTES: u64 = 1024 * 1024;
#[cfg(test)]
const MAX_BROWSER_TIMELINE_ENTRIES: usize = 32;
#[cfg(test)]
const MAX_BROWSER_REPLAY_EVENTS: usize = 96;
#[cfg(test)]
const MAX_BROWSER_DEVTOOLS_EVENTS: usize = 512;
#[cfg(test)]
const MAX_TIMELINE_CONSOLE_ENTRIES: usize = 20;
#[cfg(test)]
const MAX_TIMELINE_NETWORK_ENTRIES: usize = 30;
#[cfg(test)]
const MAX_TIMELINE_POLICY_DENIALS: usize = 10;

fn default_include_screenshot() -> bool {
    true
}

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/browser/sessions", post(create_session))
        .route("/api/v1/browser/sessions/{session_id}", get(get_session))
        .route(
            "/api/v1/browser/sessions/{session_id}/timeline",
            get(get_owner_timeline),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/actions",
            post(run_action),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/control",
            post(set_control_mode),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/tabs",
            get(get_tabs).post(new_tab),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/tabs/{tab_id}/select",
            post(select_tab),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/tabs/{tab_id}",
            delete(close_tab),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/suggestions",
            post(suggest_action),
        )
        // Phase 2: start/control a durable, server-side, multi-step
        // browser-agent run. Progress streams over the existing chat run-event
        // route (`GET /api/v1/runs/:run_id/events`), unchanged by this facade.
        .route(
            "/api/v1/browser/sessions/{session_id}/ai-runs",
            post(start_ai_run),
        )
        .route(
            "/api/v1/browser/runs/{run_id}/control",
            post(control_ai_run),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/artifacts/{artifact_id}",
            get(get_artifact),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/frame",
            get(get_live_frame),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/frames/stream",
            get(stream_live_frames),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/frames/ws",
            get(proxy_live_frames_ws),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}/devtools",
            get(get_devtools_events),
        )
        .route(
            "/api/v1/browser/sessions/{session_id}",
            delete(close_session),
        )
        .route(
            "/api/v1/browser/profiles",
            get(list_profiles).post(create_browser_profile),
        )
        .route(
            "/api/v1/browser/profiles/{profile_id}/restore-probe",
            post(restore_profile_probe),
        )
        .route(
            "/api/v1/browser/profiles/{profile_id}",
            patch(rename_browser_profile).delete(delete_profile),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

pub(crate) async fn create_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<CreateSessionBody>,
) -> Response {
    let target = match normalize_public_http_url(&body.url) {
        Ok(value) => value,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(error("invalid_url", message))).into_response()
        }
    };
    let viewport = body.viewport.unwrap_or(DEFAULT_VIEWPORT);
    let profile_id = body
        .profile_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let requested_persistent_profile = body.persistent_profile || profile_id.is_some();
    let profile_scope =
        effective_profile_scope(profile_id, requested_persistent_profile, body.scope);
    if let Some((code, message)) = reject_zdr_persistent_profile(body.zdr, profile_scope) {
        return (StatusCode::BAD_REQUEST, Json(error(code, message))).into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let allowed_domain = hostname(&target);

    // ZDR is caller-requested and enforced above against persistent profiles;
    // the flag is threaded through metadata so the SPA renders persistence
    // state from data instead of assuming it.
    let zdr = body.zdr;
    let mut start_body = json!({
        "constraints": {
            "max_steps": 12,
            "allowed_domains": allowed_domain.map(|domain| vec![domain]).unwrap_or_default(),
            "max_runtime_s": 120
        },
        "viewport": {
            "width": viewport.width,
            "height": viewport.height
        },
        "zdr": zdr
    });
    start_body["profile_scope"] = Value::String(profile_scope.as_str().to_owned());
    if let Some(profile_id) = profile_id {
        start_body["profile_id"] = Value::String(profile_id.to_owned());
    }
    if requested_persistent_profile {
        start_body["persist_profile"] = Value::Bool(true);
    }

    let (start_status, start_body) = quarry_call(
        &state,
        Method::POST,
        "/v1/agent/runs",
        Some(start_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !start_status.is_success() {
        return forward_quarry_failure(start_status, start_body);
    }

    let start_data = unwrap_data(&start_body);
    let Some(run_id) = str_field(&start_data, "run_id") else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "browser_session_invalid",
                "Quarry did not return a browser run id.",
            )),
        )
            .into_response();
    };
    let initial_action = json!({
        "type": "navigate",
        "url": target
    });
    let step_body = json!({
        "action": initial_action.clone(),
        "instruction": "Open the page for the Verevon in-app browser surface."
    });
    let (step_status, step_body) = quarry_call(
        &state,
        Method::POST,
        &format!("/v1/agent/runs/{}/step", urlencoding::encode(&run_id)),
        Some(step_body),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !step_status.is_success() {
        let _ = quarry_call(
            &state,
            Method::DELETE,
            &format!("/v1/agent/runs/{}", urlencoding::encode(&run_id)),
            None,
            token.as_deref(),
            &user.user_id,
        )
        .await;
        return forward_quarry_failure(step_status, step_body);
    }

    let observation = unwrap_data(&step_body);
    let projection =
        match owner_browser_session_projection(&state, &user, token.as_deref(), &run_id).await {
            Ok(projection) => projection,
            Err(response) => return response,
        };
    let response = browser_response_from_owner_projection(&projection, Some(observation));
    (StatusCode::OK, Json(ok(response))).into_response()
}

/// Reads Quarry's browser-session projection. This is deliberately not a
/// `BrowserRunStore` lookup: the BFF must not recreate an absent owner record
/// with defaults or merge its transient replay metadata into the response.
async fn get_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    match owner_browser_session_projection(&state, &user, token.as_deref(), &session_id).await {
        Ok(projection) => (StatusCode::OK, Json(ok(projection))).into_response(),
        Err(response) => response,
    }
}

/// Read the sole owner projection for a direct Quarry browser run. All callers
/// use this after an owner-authorized mutation instead of rebuilding session
/// state from BFF-local observations, tabs, or control flags.
async fn owner_browser_session_projection(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    session_id: &str,
) -> Result<Value, Response> {
    let (status, body) = quarry_call(
        state,
        Method::GET,
        &format!(
            "/v1/agent/runs/{}/browser-session",
            urlencoding::encode(session_id)
        ),
        None,
        token,
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return Err(forward_quarry_failure(status, body));
    }
    Ok(unwrap_data(&body))
}

/// Quarry owns browser audit history. The gateway only forwards the verified
/// session identity; it never reconstructs a timeline from process-local
/// observations or replay events.
async fn get_owner_timeline(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &format!(
            "/v1/agent/runs/{}/browser-session/timeline{}",
            urlencoding::encode(&session_id),
            query_suffix(&uri),
        ),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    (StatusCode::OK, Json(ok(unwrap_data(&body)))).into_response()
}

pub(crate) async fn run_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<ActionBody>,
) -> Response {
    let action = match sanitize_action(body.action) {
        Ok(value) => value,
        Err((code, message)) => {
            return (StatusCode::BAD_REQUEST, Json(error(code, message))).into_response()
        }
    };
    let actor = body.actor;
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let current_projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let current_mode = BrowserControlMode::from_owner_value(
        current_projection
            .get("controlMode")
            .and_then(Value::as_str),
    );
    if actor == BrowserActionActor::Agent && current_mode == BrowserControlMode::HumanTakeover {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "browser_human_takeover_active",
                "Human takeover is active for this browser session.",
            )),
        )
            .into_response();
    }
    if actor == BrowserActionActor::Human && current_mode != BrowserControlMode::HumanTakeover {
        let (status, body) = quarry_call(
            &state,
            Method::POST,
            &format!(
                "/v1/agent/runs/{}/browser-session/control",
                urlencoding::encode(&session_id),
            ),
            Some(json!({ "mode": "human_takeover" })),
            token.as_deref(),
            &user.user_id,
        )
        .await;
        if !status.is_success() {
            return forward_quarry_failure(status, body);
        }
    }
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        &format!("/v1/agent/runs/{}/step", urlencoding::encode(&session_id)),
        Some(json!({ "action": action })),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }

    let observation = unwrap_data(&body);
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let response = browser_response_from_owner_projection(&projection, Some(observation));
    (StatusCode::OK, Json(ok(response))).into_response()
}

pub(crate) async fn set_control_mode(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<ControlBody>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    // Control authority lives in Quarry. A BFF cache must neither authorize a
    // hand-off nor decide whether an owner session exists after a restart.
    let mode = body.mode;
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        &format!(
            "/v1/agent/runs/{}/browser-session/control",
            urlencoding::encode(&session_id),
        ),
        Some(json!({ "mode": mode.as_str() })),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    let response = browser_response_from_owner_projection(&unwrap_data(&body), None);
    (StatusCode::OK, Json(ok(response))).into_response()
}

async fn get_tabs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let tabs = match fetch_browser_tabs(&state, &user, token.as_deref(), &session_id).await {
        Ok(tabs) => tabs,
        Err(response) => return response,
    };
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let response = browser_response_from_owner_projection(&projection, None);
    (
        StatusCode::OK,
        Json(ok(json!({ "tabs": tabs, "session": response["session"] }))),
    )
        .into_response()
}

pub(crate) async fn new_tab(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<NewTabBody>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let url = match normalize_optional_public_url(body.url.as_deref()) {
        Ok(url) => url,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(error("invalid_url", message))).into_response()
        }
    };

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        &format!("/v1/agent/runs/{}/tabs", urlencoding::encode(&session_id)),
        Some(json!({ "url": url })),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    let data = unwrap_data(&body);
    let tabs = tabs_from_data(&data);
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let response = browser_response_from_owner_projection(&projection, None);
    (
        StatusCode::OK,
        Json(ok(json!({
            "tab": data.get("tab").cloned().unwrap_or(Value::Null),
            "tabs": tabs,
            "session": response["session"]
        }))),
    )
        .into_response()
}

pub(crate) async fn select_tab(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((session_id, tab_id)): Path<(String, String)>,
) -> Response {
    tab_mutation(
        state,
        user,
        headers,
        session_id,
        Some(tab_id),
        Method::POST,
        "select",
        None,
    )
    .await
}

pub(crate) async fn close_tab(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((session_id, tab_id)): Path<(String, String)>,
) -> Response {
    tab_mutation(
        state,
        user,
        headers,
        session_id,
        Some(tab_id),
        Method::DELETE,
        "close",
        None,
    )
    .await
}

// reason: shared select/close tab route glue — every argument is a distinct
// request-scoped concern (auth, headers, path segments, upstream verb); a
// one-off params struct would only relocate the same list.
#[allow(clippy::too_many_arguments)]
async fn tab_mutation(
    state: AppState,
    user: AuthenticatedUser,
    headers: HeaderMap,
    session_id: String,
    tab_id: Option<String>,
    method: Method,
    operation: &str,
    body: Option<Value>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let Some(tab_id) = tab_id else {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_tab",
                "The requested browser tab id is invalid.",
            )),
        )
            .into_response();
    };
    if !is_valid_path_segment(&tab_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_tab",
                "The requested browser tab id is invalid.",
            )),
        )
            .into_response();
    }
    let suffix = if operation == "select" { "/select" } else { "" };
    let path = format!(
        "/v1/agent/runs/{}/tabs/{}{}",
        urlencoding::encode(&session_id),
        urlencoding::encode(&tab_id),
        suffix
    );
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) =
        quarry_call(&state, method, &path, body, token.as_deref(), &user.user_id).await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    let data = unwrap_data(&body);
    let tabs = tabs_from_data(&data);
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let response = browser_response_from_owner_projection(&projection, None);
    (
        StatusCode::OK,
        Json(ok(json!({ "tabs": tabs, "session": response["session"] }))),
    )
        .into_response()
}

pub(crate) async fn close_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::DELETE,
        &format!("/v1/agent/runs/{}", urlencoding::encode(&session_id)),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() && status != StatusCode::NOT_FOUND {
        return forward_quarry_failure(status, body);
    }
    (StatusCode::OK, Json(ok(json!({ "closed": true })))).into_response()
}

pub(crate) async fn suggest_action(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<SuggestActionBody>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let quarry_bearer = quarry_token(&state, &user, &cookie).await;
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        quarry_bearer.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let metadata = owner_session_state_from_projection(&projection);
    let Some(observation) = metadata.last_observation.clone() else {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "browser_observation_missing",
                "No browser observation is available for this session yet.",
            )),
        )
            .into_response();
    };

    let cookie = cookie_header(&headers);
    let quarry_bearer = quarry_token(&state, &user, &cookie).await;
    let screenshot = if body.include_screenshot {
        screenshot_artifact_id(&observation)
    } else {
        None
    };
    let screenshot_payload = match screenshot {
        Some(artifact_id) => {
            fetch_model_screenshot(&state, &user, quarry_bearer.as_deref(), &artifact_id).await
        }
        None => Ok(None),
    };
    let screenshot_payload = match screenshot_payload {
        Ok(value) => value,
        Err(response) => return response,
    };
    let visual_artifact_id = visual_observation_artifact_id(&observation);
    let visual_observation = match visual_artifact_id.as_deref() {
        Some(artifact_id) => {
            match fetch_visual_observation(&state, &user, quarry_bearer.as_deref(), artifact_id)
                .await
            {
                Ok(value) => value,
                Err(response) => return response,
            }
        }
        None => Value::Null,
    };

    let mut request_body = json!({
        "goal": trimmed_goal(&body.goal),
        "url": observation.get("url").and_then(Value::as_str).unwrap_or_default(),
        "title": observation.get("title").and_then(Value::as_str).unwrap_or_default(),
        "observation": observation,
        "visual_observation": visual_observation,
        "visual_observation_artifact_id": visual_artifact_id.as_deref().unwrap_or_default(),
    });
    if let Some((mime_type, content_base64)) = screenshot_payload {
        request_body["screenshot_mime_type"] = Value::String(mime_type);
        request_body["screenshot_base64"] = Value::String(content_base64);
    }

    let token = model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/browser/suggest-action", state.model_gateway_url);
    let (status, Json(response_body)) = proxy_model_json(
        &state,
        Method::POST,
        &url,
        Some(request_body),
        token.as_deref(),
        &user,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, response_body);
    }

    (StatusCode::OK, Json(ok(response_body))).into_response()
}

trait BrowserSessionRunState {
    fn profile_id(&self) -> Option<&str>;
    fn profile_scope(&self) -> BrowserProfileScope;
    fn zdr(&self) -> bool;
}

impl BrowserSessionRunState for BrowserOwnerSessionState {
    fn profile_id(&self) -> Option<&str> {
        self.profile_id.as_deref()
    }

    fn profile_scope(&self) -> BrowserProfileScope {
        self.profile_scope
    }

    fn zdr(&self) -> bool {
        self.zdr
    }
}

#[cfg(test)]
impl BrowserSessionRunState for BrowserRunMetadata {
    fn profile_id(&self) -> Option<&str> {
        self.profile_id.as_deref()
    }

    fn profile_scope(&self) -> BrowserProfileScope {
        self.profile_scope
    }

    fn zdr(&self) -> bool {
        self.zdr
    }
}

/// Builds the JSON body forwarded to model-gateway's `POST /v1/browser/runs`.
///
/// `require_approval` is deliberately **never** taken from the caller. That
/// field controls a different, older, still-broken mechanism —
/// execution-core's blanket per-run gate (`PlanConfig.require_approval` in
/// `browser_agent.rs`'s `decide_next_action`): when set, it fires before the
/// very first action, sets `PlanStatus::WaitingApproval`, and the outer loop
/// just `break`s with no poll/resume path, so `close_run` releases the
/// Quarry lease with zero actions ever taken and the run never comes back.
/// It predates Phase 5 and Phase 5 does not touch it. Real per-action HITL
/// gating — Phase 5's actual deliverable — fires automatically inside the
/// loop based on risk classification (login/checkout/destructive/
/// cross-domain-nav/persistent-cookie-use), independent of this flag, and
/// its approvals flow through the same generic, already-proxied surface
/// (`orchestration.rs`'s `/api/v1/orchestration/runs/:run_id/approvals` +
/// `.../approvals/:id/decide`) — nothing here needs to opt in, and nothing
/// here should ever arm the broken legacy switch on a caller's behalf.
fn build_ai_run_request<S: BrowserSessionRunState>(
    goal: &str,
    session_id: &str,
    metadata: &S,
    body: &StartAiRunBody,
    start_url: Option<&str>,
) -> Value {
    // Quarry assigns a `profile_id` to *every* session lease, including
    // purely ephemeral ones (confirmed live: a session created with no
    // `profileId`/`persistentProfile` still comes back with
    // `profile.scope: "ephemeral"` and a real `prof_...` id) — it is a
    // working-set identifier, not evidence of persistence. Forwarding it
    // unconditionally would make execution-core's Phase 5
    // `persistent_cookie_use` HITL gate (`PlanConfig.profile_id.is_some() &&
    // !zdr`) fire on every single AI run, not just ones actually reusing a
    // saved profile's cookies. Only forward it when this session's own
    // resolved scope (`effective_profile_scope`, computed once at
    // `create_session` time) says the attachment is genuinely persistent.
    let persistent_profile_id = (metadata.profile_scope() != BrowserProfileScope::Ephemeral)
        .then(|| metadata.profile_id().map(str::to_owned))
        .flatten();
    json!({
        "goal": goal,
        "grant_id": format!("session:{session_id}"),
        "profile_id": persistent_profile_id,
        "start_url": start_url,
        "allowed_domains": body.allowed_domains,
        "max_steps": body.max_steps,
        "max_runtime_s": body.max_runtime_s,
        "stop_criteria": body.stop_criteria,
        "require_approval": false,
        "max_cost_usd": body.max_cost_usd,
        // Server-derived only — mirrors `create_session`'s own zdr handling;
        // never sourced from the request body.
        "zdr": metadata.zdr(),
    })
}

/// `POST /api/v1/browser/sessions/:session_id/ai-runs` — start a durable,
/// server-side, multi-step browser-agent run (Phase 2). The Quarry browser
/// session's own `zdr` (never a client-supplied value) is forwarded so
/// Zero-Data-Retention carries over from the tab the run was launched from;
/// `profile_id` is forwarded *only* when the session's resolved scope is
/// genuinely persistent (see `build_ai_run_request`'s doc comment — Quarry
/// assigns a working-set `profile_id` to every lease, ephemeral ones
/// included, so the raw value alone is not evidence of persistence).
/// `grant_id` is an interim, session-scoped string — real Model-Plane
/// browser-grant *issuance* (a durable `BrowserGrant` record) is still
/// unbuilt; risk-based HITL *gating* of individual risky actions is live as
/// of Phase 5 (see `build_ai_run_request` above and
/// `docs/BROWSER_WORKSPACE_PLAN.md`).
pub(crate) async fn start_ai_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<StartAiRunBody>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }
    let goal = body.goal.trim();
    if goal.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_goal",
                "A goal is required to start an AI browser run.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let quarry_bearer = quarry_token(&state, &user, &cookie).await;
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        quarry_bearer.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let metadata = owner_session_state_from_projection(&projection);

    // Server-derived only, from the session's own last observation: a freshly
    // `start_run`'d Quarry lease the AI run acquires has no page loaded, so
    // the loop's first action needs an explicit destination — the URL of the
    // tab the user is already looking at (never a client-supplied value).
    let start_url = projection
        .get("currentUrl")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let request_body =
        build_ai_run_request(goal, &session_id, &metadata, &body, start_url.as_deref());

    let (token, data_plane, inference, execution, session) = tokio::join!(
        model_token(&state, &user, &headers),
        data_plane_token(&state, &user, &headers),
        required_inference_token(&state, &user, &headers),
        required_execution_token(&state, &user, &headers),
        required_session_token(&state, &user, &headers),
    );
    let Some(data_plane) = data_plane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "delegated_auth_unavailable",
                "Required data-plane authentication is temporarily unavailable",
            )),
        )
            .into_response();
    };
    let inference = match inference {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let execution = match execution {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let session = match session {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!("{}/v1/browser/runs", state.model_gateway_url);
    let (status, Json(response_body)) = proxy_model_json_with_data_plane(
        &state,
        Method::POST,
        &url,
        Some(request_body),
        token.as_deref(),
        Some(&data_plane),
        Some(&inference),
        Some(&execution),
        None,
        Some(&session),
        &user,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, response_body);
    }

    let response_data = unwrap_data(&response_body);
    let Some(_run_id) =
        str_field(&response_data, "run_id").or_else(|| str_field(&response_data, "runId"))
    else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "browser_run_invalid",
                "Model Gateway did not return a browser run id.",
            )),
        )
            .into_response();
    };
    (StatusCode::OK, Json(ok(response_body))).into_response()
}

/// `POST /api/v1/browser/runs/:run_id/control` — pause / resume / stop a
/// durable browser-agent run (Phase 2 B5). A thin proxy: the body
/// (`{"action": "pause" | "resume" | "stop"}`) is forwarded verbatim to
/// model-gateway, which maps it onto `ExecutionCore`'s
/// `PauseRun`/`ResumeRun`/`CancelRun` RPCs.
pub(crate) async fn control_ai_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if !is_valid_path_segment(&run_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_run",
                "The requested browser run id is invalid.",
            )),
        )
            .into_response();
    }
    let owner = match resolve_browser_run_owner(&state, &user).await {
        Ok(owner) => owner,
        Err(response) => return response,
    };
    if let Err(response) = owned_orchestration_run(&state, &user, &headers, &owner, &run_id).await {
        return response;
    }

    let (token, execution, session) = tokio::join!(
        model_token(&state, &user, &headers),
        required_execution_token(&state, &user, &headers),
        required_session_token(&state, &user, &headers),
    );
    let execution = match execution {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let session = match session {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!(
        "{}/v1/browser/runs/{}/control",
        state.model_gateway_url,
        urlencoding::encode(&run_id)
    );
    let (status, Json(response_body)) = proxy_model_json_with_data_plane(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        None,
        None,
        Some(&execution),
        None,
        Some(&session),
        &user,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, response_body);
    }

    (StatusCode::OK, Json(ok(response_body))).into_response()
}

/// `GET /api/v1/browser/sessions/:session_id/artifacts/:artifact_id` — serve one
/// piece of a run's captured evidence (a step screenshot, a DOM snapshot, a
/// visual observation) as bytes the SPA can point an `<img>` at.
///
/// `:session_id` is whichever id the run's evidence was registered under: a
/// Quarry browser session for the in-app browser surface, or an orchestration run
/// id for a chat turn whose browser loop ran inside execution-core. Both are
/// authorized by [`owned_browser_artifact_run`]; neither can read the other
/// tenant's evidence.
///
/// Quarry's own `GET /v1/artifacts/:id` applies no tenant scoping, so the
/// ownership gate below is the *only* isolation boundary on this path and must
/// run before any upstream call.
async fn get_artifact(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((session_id, artifact_id)): Path<(String, String)>,
) -> Response {
    // The `art_` prefix rule is mirrored client-side
    // (`chat-run-watch.ts::isFetchableArtifactRef`) so a malformed reference
    // degrades to an honest "could not fetch" state instead of being retried as
    // an `<img>` src against a 400.
    if !is_valid_path_segment(&session_id) || !is_valid_artifact_id(&artifact_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_artifact",
                "The requested browser artifact id is invalid.",
            )),
        )
            .into_response();
    }
    let metadata = match owned_browser_artifact_run(&state, &user, &headers, &session_id).await {
        Ok(metadata) => metadata,
        Err(response) => return response,
    };
    // Zero Data Retention: Quarry refuses every durable artifact write for a ZDR
    // run (`quarry-core::zdr` guards `WriteKind::Artifact`), so no capture for
    // this run can ever exist. Answer that as an explicit, terminal "no
    // artifact" — never a 5xx, and never an authorization code, both of which the
    // panel renders as different copy. Runs authorized as orchestration runs
    // carry no ZDR flag here (that posture lives with the turn, not this
    // gateway); they reach the same outcome through the not-found normalization
    // below, because the capture was never written.
    if metadata.zdr {
        return BrowserArtifactAbsence::Withheld.into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let url = format!(
        "{}/v1/artifacts/{}",
        state.quarry_edge_url,
        urlencoding::encode(&artifact_id)
    );
    let mut req = state.client.get(&url).header("x-user-id", &user.user_id);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }

    match req.send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            if !status.is_success() {
                // A capture that was never written (ZDR run) and one that has
                // aged out of the store are the same terminal fact to the
                // caller: there is no artifact. Normalize both into one stable
                // code instead of forwarding Quarry's envelope, so the panel can
                // tell "no image will ever come" apart from a real failure.
                if matches!(status, StatusCode::NOT_FOUND | StatusCode::GONE) {
                    return BrowserArtifactAbsence::NeverStored.into_response();
                }
                let body = upstream.json::<Value>().await.unwrap_or_else(
                    |_| json!({ "error": { "code": "browser_artifact_unavailable" } }),
                );
                return forward_quarry_failure(status, body);
            }

            if upstream
                .content_length()
                .is_some_and(|length| length > MAX_BROWSER_ARTIFACT_BYTES)
            {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(error(
                        "browser_artifact_too_large",
                        "The browser artifact is too large to preview.",
                    )),
                )
                    .into_response();
            }

            let upstream_content_type = upstream
                .headers()
                .get(header::CONTENT_TYPE.as_str())
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let bytes = match upstream.bytes().await {
                Ok(bytes) => bytes,
                Err(_) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(crate::envelope::upstream_unavailable()),
                    )
                        .into_response()
                }
            };
            if bytes.len() as u64 > MAX_BROWSER_ARTIFACT_BYTES {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(error(
                        "browser_artifact_too_large",
                        "The browser artifact is too large to preview.",
                    )),
                )
                    .into_response();
            }

            let content_type =
                safe_browser_artifact_content_type(upstream_content_type.as_deref(), &bytes);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "private, max-age=30")
                .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
                .body(Body::from(bytes))
                .unwrap_or_else(|_| {
                    (
                        StatusCode::BAD_GATEWAY,
                        Json(error(
                            "browser_artifact_failed",
                            "The browser artifact could not be returned.",
                        )),
                    )
                        .into_response()
                })
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )
            .into_response(),
    }
}

async fn get_live_frame(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Query(query): Query<FrameQuery>,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let metadata = owner_session_state_from_projection(&projection);

    let format = match query
        .format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("jpeg")
    {
        "jpg" | "jpeg" => "jpeg",
        "png" => "png",
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error(
                    "invalid_browser_frame",
                    "Frame format must be jpeg or png.",
                )),
            )
                .into_response()
        }
    };
    let quality = query.quality.unwrap_or(65).clamp(1, 100);
    let max_width = query
        .max_width
        .unwrap_or(metadata.viewport.width)
        .clamp(320, metadata.viewport.width.max(320));
    let max_height = query
        .max_height
        .unwrap_or(metadata.viewport.height)
        .clamp(240, metadata.viewport.height.max(240));
    let path = format!(
        "/v1/agent/runs/{}/frame?format={}&quality={}&maxWidth={}&maxHeight={}",
        urlencoding::encode(&session_id),
        format,
        quality,
        max_width,
        max_height
    );

    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &path,
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    let data = unwrap_data(&body);
    let Some(data_base64) = str_field(&data, "dataBase64") else {
        return (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "browser_frame_invalid",
                "Quarry did not return a live browser frame.",
            )),
        )
            .into_response();
    };
    let mime_type = match str_field(&data, "mimeType").as_deref() {
        Some("image/png") => HeaderValue::from_static("image/png"),
        _ => HeaderValue::from_static("image/jpeg"),
    };
    let bytes = match BASE64_STANDARD.decode(data_base64.as_bytes()) {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "browser_frame_invalid",
                    "Quarry returned an invalid live browser frame.",
                )),
            )
                .into_response()
        }
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime_type)
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| {
            (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "browser_frame_failed",
                    "The live browser frame could not be returned.",
                )),
            )
                .into_response()
        })
}

async fn stream_live_frames(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/agent/runs/{}/frames/stream{}",
        state.quarry_edge_url,
        urlencoding::encode(&session_id),
        query_suffix(&uri)
    );

    proxy_sse_stream(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        headers.get("last-event-id").and_then(|v| v.to_str().ok()),
        None,
        projection
            .get("zdr")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    )
    .await
}

async fn get_devtools_events(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    uri: Uri,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let projection = match owner_browser_session_projection(
        &state,
        &user,
        token.as_deref(),
        &session_id,
    )
    .await
    {
        Ok(projection) => projection,
        Err(response) => return response,
    };
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &format!(
            "/v1/agent/runs/{}/devtools{}",
            urlencoding::encode(&session_id),
            query_suffix(&uri)
        ),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }

    let data = unwrap_data(&body);
    let events = data
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let response = browser_response_from_owner_projection(&projection, None);

    (
        StatusCode::OK,
        Json(ok(json!({
            "events": events,
            "session": response["session"],
            "zdr": data
                .get("zdr")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| {
                    projection
                        .get("zdr")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
        }))),
    )
        .into_response()
}

async fn proxy_live_frames_ws(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    uri: Uri,
    ws: WebSocketUpgrade,
) -> Response {
    if !is_valid_path_segment(&session_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_session",
                "The requested browser session id is invalid.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    if let Err(response) =
        owner_browser_session_projection(&state, &user, token.as_deref(), &session_id).await
    {
        return response;
    }
    let upstream_url = match upstream_ws_url(
        &state.quarry_edge_url,
        &format!(
            "/v1/agent/runs/{}/frames/ws{}",
            urlencoding::encode(&session_id),
            query_suffix(&uri)
        ),
    ) {
        Ok(url) => url,
        Err(_) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(error(
                    "browser_ws_invalid_upstream",
                    "The browser websocket upstream is unavailable.",
                )),
            )
                .into_response()
        }
    };

    ws.on_upgrade(move |socket| {
        browser_ws_proxy_loop(socket, state, session_id, upstream_url, token)
    })
    .into_response()
}

async fn browser_ws_proxy_loop(
    mut client_socket: WebSocket,
    _state: AppState,
    _session_id: String,
    upstream_url: String,
    bearer_token: Option<String>,
) {
    let mut request = match upstream_url.as_str().into_client_request() {
        Ok(request) => request,
        Err(_) => {
            let _ = send_client_ws_error(
                &mut client_socket,
                "The browser websocket upstream is unavailable.".to_owned(),
            )
            .await;
            return;
        }
    };
    if let Some(token) = bearer_token.filter(|value| !value.trim().is_empty()) {
        match format!("Bearer {}", token.trim()).parse() {
            Ok(value) => {
                request.headers_mut().insert("authorization", value);
            }
            Err(_) => {
                let _ = send_client_ws_error(
                    &mut client_socket,
                    "The browser websocket credential is invalid.".to_owned(),
                )
                .await;
                return;
            }
        }
    }

    let upstream = match connect_async(request).await {
        Ok((socket, _response)) => socket,
        Err(_) => {
            let _ = send_client_ws_error(
                &mut client_socket,
                "The browser websocket upstream is unavailable.".to_owned(),
            )
            .await;
            return;
        }
    };

    let (mut client_tx, mut client_rx) = client_socket.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    loop {
        tokio::select! {
            client_msg = client_rx.next() => {
                match client_msg {
                    Some(Ok(message)) => {
                        let close = matches!(message, AxumWsMessage::Close(_));
                        if let Some(upstream_message) = axum_to_tungstenite_message(message) {
                            if upstream_tx.send(upstream_message).await.is_err() {
                                break;
                            }
                        }
                        if close {
                            break;
                        }
                    }
                    Some(Err(err)) => {
                        tracing::debug!(error = %err, "browser websocket client receive failed");
                        break;
                    }
                    None => break,
                }
            }
            upstream_msg = upstream_rx.next() => {
                match upstream_msg {
                    Some(Ok(message)) => {
                        let close = matches!(message, TungsteniteMessage::Close(_));
                        let client_message = tungstenite_to_axum_message(message);
                        if let Some(client_message) = client_message {
                            if client_tx.send(client_message).await.is_err() {
                                break;
                            }
                        }
                        if close {
                            break;
                        }
                    }
                    Some(Err(_)) => {
                        let _ = client_tx
                            .send(AxumWsMessage::Text(json!({
                                "type": "error",
                                "message": "The browser websocket upstream is unavailable."
                            }).to_string().into()))
                            .await;
                        break;
                    }
                    None => break,
                }
            }
        }
    }
}

async fn send_client_ws_error(socket: &mut WebSocket, message: String) -> Result<(), axum::Error> {
    socket
        .send(AxumWsMessage::Text(
            json!({ "type": "error", "message": message })
                .to_string()
                .into(),
        ))
        .await
}

#[cfg(test)]
#[derive(Debug)]
enum PreparedClientWsMessage {
    Client(AxumWsMessage),
    Upstream(AxumWsMessage),
}

#[cfg(test)]
#[derive(Debug)]
struct ClientWsPrepareError {
    code: &'static str,
    message: String,
}

#[cfg(test)]
fn prepare_client_ws_message(
    message: AxumWsMessage,
    state: &AppState,
    session_id: &str,
    pending_action: &mut Option<Value>,
    pending_actor: &mut String,
) -> Result<PreparedClientWsMessage, ClientWsPrepareError> {
    let AxumWsMessage::Text(text) = message else {
        return Ok(PreparedClientWsMessage::Upstream(message));
    };
    let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
        return Ok(PreparedClientWsMessage::Upstream(AxumWsMessage::Text(text)));
    };
    let Some(message_type) = value.get("type").and_then(Value::as_str) else {
        return Ok(PreparedClientWsMessage::Upstream(AxumWsMessage::Text(
            value.to_string().into(),
        )));
    };
    if message_type == "control" {
        return prepare_client_control_ws_message(value, state, session_id)
            .map(PreparedClientWsMessage::Client);
    }
    if message_type != "action" {
        return Ok(PreparedClientWsMessage::Upstream(AxumWsMessage::Text(
            value.to_string().into(),
        )));
    }

    let actor = value
        .get("actor")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| "human".to_owned());
    let current_metadata = browser_run_metadata(state, session_id);
    if actor == "agent" && current_metadata.control_mode == BrowserControlMode::HumanTakeover {
        return Err(ClientWsPrepareError {
            code: "browser_human_takeover_active",
            message: "Human takeover is active for this browser session.".to_owned(),
        });
    }
    *pending_actor = actor.clone();
    *pending_action = value.get("action").cloned();

    if actor == "human" && current_metadata.control_mode != BrowserControlMode::HumanTakeover {
        if let Some(metadata) =
            update_browser_control_mode(state, session_id, BrowserControlMode::HumanTakeover)
        {
            let _ = append_replay_event(
                state,
                session_id,
                control_replay_event(
                    session_id,
                    BrowserControlMode::HumanTakeover,
                    "human",
                    metadata.zdr,
                ),
            );
        }
    }
    if value.get("instruction").is_none() {
        value["instruction"] =
            Value::String("Human browser takeover action from Verevon.".to_owned());
    }
    Ok(PreparedClientWsMessage::Upstream(AxumWsMessage::Text(
        value.to_string().into(),
    )))
}

#[cfg(test)]
fn prepare_client_control_ws_message(
    value: Value,
    state: &AppState,
    session_id: &str,
) -> Result<AxumWsMessage, ClientWsPrepareError> {
    let mode = match value.get("mode").and_then(Value::as_str) {
        Some("agent_control") => BrowserControlMode::AgentControl,
        Some("human_takeover") => BrowserControlMode::HumanTakeover,
        _ => {
            return Err(ClientWsPrepareError {
                code: "invalid_browser_control_mode",
                message: "Browser control mode must be agent_control or human_takeover.".to_owned(),
            })
        }
    };
    let actor = value
        .get("actor")
        .and_then(Value::as_str)
        .unwrap_or("human");
    let Some(metadata) = update_browser_control_mode(state, session_id, mode) else {
        return Err(ClientWsPrepareError {
            code: "browser_session_not_found",
            message: "Browser session is not active.".to_owned(),
        });
    };
    let metadata = append_replay_event(
        state,
        session_id,
        control_replay_event(session_id, mode, actor, metadata.zdr),
    )
    .unwrap_or(metadata);
    let observation = metadata.last_observation.clone();
    let response = browser_response(session_id, &metadata, observation.clone());

    Ok(AxumWsMessage::Text(
        json!({
            "type": "control",
            "actor": actor,
            "control": { "mode": mode.as_str() },
            "observation": observation,
            "session": response["session"].clone()
        })
        .to_string()
        .into(),
    ))
}

#[cfg(test)]
fn process_upstream_ws_message(
    message: TungsteniteMessage,
    state: &AppState,
    session_id: &str,
    pending_action: &mut Option<Value>,
    pending_actor: &mut String,
) -> Option<AxumWsMessage> {
    let TungsteniteMessage::Text(text) = message else {
        return tungstenite_to_axum_message(message);
    };
    let Ok(mut value) = serde_json::from_str::<Value>(&text) else {
        return Some(AxumWsMessage::Text(text.to_string().into()));
    };
    if value.get("type").and_then(Value::as_str) == Some("frame") {
        return process_upstream_frame_ws_message(value, state, session_id);
    }
    if value.get("type").and_then(Value::as_str) == Some("devtools") {
        return process_upstream_devtools_ws_message(value, state, session_id);
    }
    if value.get("type").and_then(Value::as_str) != Some("observation") {
        return Some(AxumWsMessage::Text(value.to_string().into()));
    }
    let Some(observation) = value.get("observation").cloned() else {
        return Some(AxumWsMessage::Text(value.to_string().into()));
    };

    let mut metadata = update_browser_observation(state, session_id, observation.clone())?;
    if let Some(next_metadata) = append_replay_event(
        state,
        session_id,
        observation_replay_event(
            session_id,
            &observation,
            pending_actor.as_str(),
            pending_action.as_ref(),
            metadata.control_mode,
            metadata.zdr,
        ),
    ) {
        metadata = next_metadata;
    }
    value["session"] =
        browser_response(session_id, &metadata, Some(observation))["session"].clone();
    *pending_action = None;
    *pending_actor = "human".to_owned();
    Some(AxumWsMessage::Text(value.to_string().into()))
}

#[cfg(test)]
fn process_upstream_frame_ws_message(
    mut value: Value,
    state: &AppState,
    session_id: &str,
) -> Option<AxumWsMessage> {
    let Some(sequence) = value.get("sequence").and_then(Value::as_u64) else {
        return Some(AxumWsMessage::Text(value.to_string().into()));
    };
    let metadata = browser_run_metadata(state, session_id);
    if !should_record_frame_replay(&metadata.replay_events, sequence) {
        return Some(AxumWsMessage::Text(value.to_string().into()));
    }

    let frame_event =
        live_frame_replay_event(session_id, &value, metadata.control_mode, metadata.zdr);
    let metadata = append_replay_event(state, session_id, frame_event).unwrap_or(metadata);
    let observation = metadata.last_observation.clone();
    value["session"] = browser_response(session_id, &metadata, observation)["session"].clone();
    Some(AxumWsMessage::Text(value.to_string().into()))
}

#[cfg(test)]
fn process_upstream_devtools_ws_message(
    mut value: Value,
    state: &AppState,
    session_id: &str,
) -> Option<AxumWsMessage> {
    let events = value
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if events.is_empty() {
        return Some(AxumWsMessage::Text(value.to_string().into()));
    }

    let mut metadata = update_browser_devtools_events(state, session_id, events.clone())
        .unwrap_or_else(|| browser_run_metadata(state, session_id));
    if let Some(next_metadata) = append_replay_event(
        state,
        session_id,
        devtools_replay_event(session_id, &events, metadata.control_mode, metadata.zdr),
    ) {
        metadata = next_metadata;
    }
    let observation = metadata.last_observation.clone();
    value["session"] = browser_response(session_id, &metadata, observation)["session"].clone();
    Some(AxumWsMessage::Text(value.to_string().into()))
}

fn axum_to_tungstenite_message(message: AxumWsMessage) -> Option<TungsteniteMessage> {
    match message {
        // axum's Utf8Bytes and tungstenite's Utf8Bytes are each crate's own
        // newtype, with no cross-crate conversion between them, so text goes
        // through String (both implement Display + From<String>). Binary,
        // Ping, and Pong all hold the shared `bytes::Bytes` type directly and
        // need no conversion at all.
        AxumWsMessage::Text(value) => Some(TungsteniteMessage::Text(value.to_string().into())),
        AxumWsMessage::Binary(value) => Some(TungsteniteMessage::Binary(value)),
        AxumWsMessage::Ping(value) => Some(TungsteniteMessage::Ping(value)),
        AxumWsMessage::Pong(value) => Some(TungsteniteMessage::Pong(value)),
        AxumWsMessage::Close(_) => Some(TungsteniteMessage::Close(None)),
    }
}

fn tungstenite_to_axum_message(message: TungsteniteMessage) -> Option<AxumWsMessage> {
    match message {
        TungsteniteMessage::Text(value) => Some(AxumWsMessage::Text(value.to_string().into())),
        TungsteniteMessage::Binary(value) => Some(AxumWsMessage::Binary(value)),
        TungsteniteMessage::Ping(value) => Some(AxumWsMessage::Ping(value)),
        TungsteniteMessage::Pong(value) => Some(AxumWsMessage::Pong(value)),
        TungsteniteMessage::Close(_) => Some(AxumWsMessage::Close(None)),
        TungsteniteMessage::Frame(_) => None,
    }
}

async fn list_profiles(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        "/v1/profiles",
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    (StatusCode::OK, Json(ok(unwrap_data(&body)))).into_response()
}

/// Phase 3 continuation — `POST /api/v1/browser/profiles`. Explicitly
/// create a named, scoped profile (as opposed to attaching one implicitly
/// via `persistentProfile`/`profileId` at session-create time). `scope:
/// ephemeral` is rejected here too (defense in depth — quarry-edge
/// enforces the same rule) since a *stored* profile with "no persistence"
/// scope is a contradiction; ephemeral browsing simply attaches no
/// profile at all.
#[derive(Debug, Deserialize)]
pub(crate) struct CreateProfileBody {
    #[serde(default)]
    pub(crate) name: Option<String>,
    pub(crate) scope: BrowserProfileScope,
}

pub(crate) async fn create_browser_profile(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<CreateProfileBody>,
) -> Response {
    if body.scope == BrowserProfileScope::Ephemeral {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_profile_scope",
                "A created browser profile cannot use the ephemeral scope.",
            )),
        )
            .into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        "/v1/profiles/create",
        Some(json!({
            "name": body.name,
            "scope": body.scope.as_str(),
        })),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    (StatusCode::OK, Json(ok(unwrap_data(&body)))).into_response()
}

/// Phase 3 continuation — `PATCH /api/v1/browser/profiles/:profile_id`.
/// Rename and/or rescope an existing profile. At least one field must be
/// present; rescoping to `ephemeral` is rejected the same way creation
/// is (delete the profile instead of "rescoping it away").
#[derive(Debug, Deserialize)]
pub(crate) struct RenameProfileBody {
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) scope: Option<BrowserProfileScope>,
}

pub(crate) async fn rename_browser_profile(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(profile_id): Path<String>,
    Json(body): Json<RenameProfileBody>,
) -> Response {
    if !is_valid_profile_id(&profile_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_profile",
                "The requested browser profile id is invalid.",
            )),
        )
            .into_response();
    }
    if body.name.is_none() && body.scope.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_profile_update",
                "Provide a name and/or scope to update.",
            )),
        )
            .into_response();
    }
    if body.scope == Some(BrowserProfileScope::Ephemeral) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_profile_scope",
                "A stored browser profile cannot be rescoped to ephemeral.",
            )),
        )
            .into_response();
    }
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, resp_body) = quarry_call(
        &state,
        Method::PATCH,
        &format!("/v1/profiles/{}", urlencoding::encode(&profile_id)),
        Some(json!({
            "name": body.name,
            "scope": body.scope.map(BrowserProfileScope::as_str),
        })),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, resp_body);
    }
    (StatusCode::OK, Json(ok(unwrap_data(&resp_body)))).into_response()
}

pub(crate) async fn restore_profile_probe(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(profile_id): Path<String>,
    Json(body): Json<RestoreProbeBody>,
) -> Response {
    if !is_valid_profile_id(&profile_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_profile",
                "The requested browser profile id is invalid.",
            )),
        )
            .into_response();
    }

    let target = match normalize_public_http_url(&body.url) {
        Ok(value) => value,
        Err(message) => {
            return (StatusCode::BAD_REQUEST, Json(error("invalid_url", message))).into_response()
        }
    };
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        &format!(
            "/v1/profiles/{}/restore_probe",
            urlencoding::encode(&profile_id)
        ),
        Some(json!({ "url": target })),
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return forward_quarry_failure(status, body);
    }
    (StatusCode::OK, Json(ok(unwrap_data(&body)))).into_response()
}

pub(crate) async fn delete_profile(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(profile_id): Path<String>,
) -> Response {
    if !is_valid_profile_id(&profile_id) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_browser_profile",
                "The requested browser profile id is invalid.",
            )),
        )
            .into_response();
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::DELETE,
        &format!("/v1/profiles/{}", urlencoding::encode(&profile_id)),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    if !status.is_success() && status != StatusCode::NOT_FOUND {
        return forward_quarry_failure(status, body);
    }
    (StatusCode::OK, Json(ok(json!({ "deleted": true })))).into_response()
}

#[cfg(test)]
fn browser_response(
    run_id: &str,
    metadata: &BrowserRunMetadata,
    observation: Option<Value>,
) -> Value {
    let observed_url = observation
        .as_ref()
        .and_then(|v| v.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let title = observation
        .as_ref()
        .and_then(|v| v.get("title"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| hostname(observed_url))
        .unwrap_or_else(|| "Browser session".to_owned());
    let frame = observation
        .as_ref()
        .and_then(|v| v.get("screenshot_artifact_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|artifact_id| is_valid_artifact_id(artifact_id))
        .map(|artifact_id| artifact_frame(run_id, artifact_id, "screenshot", "image/png"));
    let visual = observation
        .as_ref()
        .and_then(|v| v.get("visual_observation_artifact_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|artifact_id| is_valid_artifact_id(artifact_id))
        .map(|artifact_id| {
            json!({
                "observationArtifactId": artifact_id,
                "observationUrl": artifact_url(run_id, artifact_id)
            })
        });

    json!({
        "session": {
            "id": run_id,
            "leaseId": metadata.lease_id,
            "status": "live",
            "renderMode": "chromium",
            "title": title,
            "url": observed_url,
            "viewport": {
                "width": metadata.viewport.width,
                "height": metadata.viewport.height
            },
            "profile": {
                "id": metadata.profile_id,
                // Phase 3 continuation: real requested/inferred scope
                // (was previously derived from a single boolean, so it
                // could only ever report 2 of the 4 possible values).
                "scope": metadata.profile_scope.as_str(),
                "storage": if metadata.profile_scope != BrowserProfileScope::Ephemeral { "persistent" } else { "isolated" }
            },
            "control": {
                "mode": metadata.control_mode.as_str()
            },
            "frame": frame,
            "liveFrameUrl": live_frame_url(run_id),
            "liveFrameStreamUrl": live_frame_stream_url(run_id),
            "liveFrameWsUrl": live_frame_ws_url(run_id),
            "tabsUrl": browser_tabs_url(run_id),
            "tabs": metadata.tabs.clone(),
            "devtoolsUrl": browser_devtools_url(run_id),
            "devtools": {
                "events": metadata.devtools_events.clone(),
                "eventCount": metadata.devtools_events.len(),
                "lastSequence": last_devtools_sequence(&metadata.devtools_events)
            },
            "visual": visual,
            "timeline": browser_timeline(run_id, &metadata.observation_history),
            "replay": {
                "events": metadata.replay_events.clone(),
                "eventCount": metadata.replay_events.len()
            },
            "zdr": metadata.zdr,
            "capabilities": ["navigate", "back", "forward", "click", "click_point", "type", "press", "scroll", "mouse_wheel", "wait_for", "select", "inspect_dom", "control_state", "human_takeover", "agent_control", "tabs", "live_frame", "live_frame_stream", "live_frame_ws", "devtools_events", "devtools_stream", "replay_timeline", "screenshot_artifact", "visual_observation", "visual_change", "annotate", "persistent_profile"]
        },
        "observation": observation
    })
}

/// Adapt Quarry's owner projection to the established browser-client shape.
/// The adapter is intentionally pure: no process-local browser run is read or
/// written here. Quarry remains the source for lease, profile, tab, control,
/// ZDR, lifecycle and URL state; the BFF only contributes same-origin route
/// URLs and safe presentation defaults.
fn browser_response_from_owner_projection(projection: &Value, observation: Option<Value>) -> Value {
    let observation = observation.or_else(|| {
        projection
            .get("lastObservation")
            .filter(|value| value.is_object())
            .cloned()
    });
    let run_id = projection
        .get("runId")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let current_url = projection
        .get("currentUrl")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let observed_url = observation
        .as_ref()
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(current_url);
    let title = observation
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            projection
                .get("tabs")
                .and_then(Value::as_array)
                .and_then(|tabs| {
                    tabs.iter()
                        .find(|tab| tab.get("active").and_then(Value::as_bool).unwrap_or(false))
                })
                .and_then(|tab| tab.get("title"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        })
        .or_else(|| hostname(observed_url))
        .unwrap_or_else(|| "Browser session".to_owned());
    let frame = observation
        .as_ref()
        .and_then(|value| value.get("screenshot_artifact_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|artifact_id| is_valid_artifact_id(artifact_id))
        .map(|artifact_id| artifact_frame(run_id, artifact_id, "screenshot", "image/png"));
    let visual = observation
        .as_ref()
        .and_then(|value| value.get("visual_observation_artifact_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|artifact_id| is_valid_artifact_id(artifact_id))
        .map(|artifact_id| {
            json!({
                "observationArtifactId": artifact_id,
                "observationUrl": artifact_url(run_id, artifact_id)
            })
        });
    let viewport = projection
        .get("viewport")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "width": DEFAULT_VIEWPORT.width,
                "height": DEFAULT_VIEWPORT.height
            })
        });
    let live = projection
        .get("live")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let closed = matches!(
        projection.get("status").and_then(Value::as_str),
        Some("closed")
    );

    json!({
        "session": {
            "id": run_id,
            "leaseId": projection.get("leaseId").cloned().unwrap_or(Value::Null),
            "status": if live { "live" } else if closed { "closed" } else { "degraded" },
            "renderMode": "chromium",
            "title": title,
            "url": observed_url,
            "viewport": viewport,
            "profile": {
                "id": projection.get("profileId").cloned().unwrap_or(Value::Null),
                "scope": projection.get("profileScope").cloned().unwrap_or_else(|| Value::String("ephemeral".to_owned())),
                "storage": if projection.get("profileStorage").and_then(Value::as_str) == Some("persistent") { "persistent" } else { "isolated" }
            },
            "control": {
                "mode": projection.get("controlMode").cloned().unwrap_or_else(|| Value::String("agent_control".to_owned()))
            },
            "frame": frame,
            "liveFrameUrl": live.then(|| live_frame_url(run_id)),
            "liveFrameStreamUrl": live.then(|| live_frame_stream_url(run_id)),
            // The owner projection is HTTP/SSE-safe. The legacy WebSocket
            // compositor is intentionally not advertised to new sessions.
            "liveFrameWsUrl": Value::Null,
            "tabsUrl": browser_tabs_url(run_id),
            "tabs": projection.get("tabs").cloned().unwrap_or_else(|| json!([])),
            "devtoolsUrl": live.then(|| browser_devtools_url(run_id)),
            "devtools": { "events": [], "eventCount": 0, "lastSequence": Value::Null },
            "visual": visual,
            "timeline": [],
            "replay": { "events": [], "eventCount": 0 },
            "zdr": projection.get("zdr").and_then(Value::as_bool).unwrap_or(false),
            "capabilities": ["navigate", "back", "forward", "click", "click_point", "type", "press", "scroll", "mouse_wheel", "wait_for", "select", "inspect_dom", "control_state", "human_takeover", "agent_control", "tabs", "live_frame", "live_frame_stream", "devtools_events", "screenshot_artifact", "visual_observation", "visual_change", "annotate", "persistent_profile"]
        },
        "observation": observation
    })
}

/// Temporary typed view for BFF request normalization. It is reconstructed
/// exclusively from a Quarry projection and is never inserted into the
/// process-local `BrowserRunStore`.
fn owner_session_state_from_projection(projection: &Value) -> BrowserOwnerSessionState {
    let viewport = projection
        .get("viewport")
        .and_then(Value::as_object)
        .map(|viewport| Viewport {
            width: viewport
                .get("width")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(DEFAULT_VIEWPORT.width),
            height: viewport
                .get("height")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(DEFAULT_VIEWPORT.height),
        })
        .unwrap_or(DEFAULT_VIEWPORT);
    BrowserOwnerSessionState {
        profile_id: projection
            .get("profileId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        profile_scope: BrowserProfileScope::from_owner_value(
            projection.get("profileScope").and_then(Value::as_str),
        ),
        last_observation: projection
            .get("lastObservation")
            .filter(|value| value.is_object())
            .cloned(),
        viewport,
        zdr: projection
            .get("zdr")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

#[cfg(test)]
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
fn action_type(action: Option<&Value>) -> Option<String> {
    action
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
fn observation_replay_event(
    run_id: &str,
    observation: &Value,
    actor: &str,
    action: Option<&Value>,
    control_mode: BrowserControlMode,
    zdr: bool,
) -> Value {
    let timestamp_ms = now_ms();
    let step = observation.get("step").and_then(Value::as_u64).unwrap_or(0);
    let screenshot_artifact_id = screenshot_artifact_id(observation);
    let visual_observation_artifact_id = visual_observation_artifact_id(observation);
    let screenshot_url = screenshot_artifact_id
        .as_deref()
        .map(|artifact_id| artifact_url(run_id, artifact_id));
    let visual_observation_url = visual_observation_artifact_id
        .as_deref()
        .map(|artifact_id| artifact_url(run_id, artifact_id));

    json!({
        "id": format!("{}:observation:{}:{}", run_id, step, timestamp_ms),
        "kind": "observation",
        "timestampMs": timestamp_ms,
        "step": step,
        "actor": actor,
        "actionType": action_type(action),
        "action": action.cloned().unwrap_or(Value::Null),
        "controlMode": control_mode.as_str(),
        "url": observation.get("url").and_then(Value::as_str).unwrap_or_default(),
        "title": observation.get("title").and_then(Value::as_str).unwrap_or_default(),
        "observedAt": observation.get("observed_at").and_then(Value::as_str).unwrap_or_default(),
        "screenshotArtifactId": screenshot_artifact_id,
        "screenshotUrl": screenshot_url,
        "visualObservationArtifactId": visual_observation_artifact_id,
        "visualObservationUrl": visual_observation_url,
        "consoleCount": observation
            .get("console_summary")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        "networkCount": observation
            .get("network_summary")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        "policyDenialCount": observation
            .get("policy_denials")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        "domNodeCount": observation
            .get("dom_summary")
            .and_then(|summary| summary.get("node_count"))
            .and_then(Value::as_u64),
        "domInteractiveCount": observation
            .get("dom_summary")
            .and_then(|summary| summary.get("interactive_elements"))
            .and_then(Value::as_array)
            .map(Vec::len),
        "zdr": zdr
    })
}

#[cfg(test)]
fn control_replay_event(
    session_id: &str,
    mode: BrowserControlMode,
    actor: &str,
    zdr: bool,
) -> Value {
    let timestamp_ms = now_ms();
    json!({
        "id": format!("{}:control:{}:{}", session_id, mode.as_str(), timestamp_ms),
        "kind": "control",
        "timestampMs": timestamp_ms,
        "actor": actor,
        "controlMode": mode.as_str(),
        "zdr": zdr
    })
}

#[cfg(test)]
fn devtools_replay_event(
    session_id: &str,
    events: &[Value],
    control_mode: BrowserControlMode,
    zdr: bool,
) -> Value {
    let timestamp_ms = now_ms();
    let categories = devtools_categories(events);
    json!({
        "id": format!("{}:devtools:{}:{}", session_id, last_devtools_sequence(events).unwrap_or_default(), timestamp_ms),
        "kind": "devtools",
        "timestampMs": timestamp_ms,
        "actor": "browser",
        "controlMode": control_mode.as_str(),
        "eventCount": events.len(),
        "categories": categories,
        "firstSequence": events
            .iter()
            .filter_map(devtools_sequence)
            .min(),
        "lastSequence": last_devtools_sequence(events),
        "zdr": zdr
    })
}

#[cfg(test)]
fn live_frame_replay_event(
    session_id: &str,
    frame: &Value,
    control_mode: BrowserControlMode,
    zdr: bool,
) -> Value {
    let timestamp_ms = now_ms();
    let sequence = frame
        .get("sequence")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let data_base64_len = frame
        .get("dataBase64")
        .and_then(Value::as_str)
        .map(str::len)
        .unwrap_or_default();
    json!({
        "id": format!("{}:frame:{}:{}", session_id, sequence, timestamp_ms),
        "kind": "frame",
        "timestampMs": timestamp_ms,
        "actor": "browser",
        "controlMode": control_mode.as_str(),
        "sequence": sequence,
        "mimeType": frame
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("image/jpeg"),
        "transport": "websocket",
        "transient": true,
        "persisted": false,
        "imagePayloadPersisted": false,
        "dataBase64Length": data_base64_len,
        "zdr": frame
            .get("zdr")
            .and_then(Value::as_bool)
            .unwrap_or(zdr)
    })
}

#[cfg(test)]
fn should_record_frame_replay(replay_events: &[Value], sequence: u64) -> bool {
    sequence == 1
        || sequence % 10 == 1
        || replay_events
            .iter()
            .rev()
            .filter(|event| event.get("kind").and_then(Value::as_str) == Some("frame"))
            .filter_map(|event| event.get("sequence").and_then(Value::as_u64))
            .next()
            .is_none()
}

#[cfg(test)]
fn devtools_categories(events: &[Value]) -> Vec<String> {
    let mut categories = events
        .iter()
        .filter_map(|event| event.get("category").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    categories.sort();
    categories.dedup();
    categories
}

#[cfg(test)]
fn browser_timeline(run_id: &str, observations: &[Value]) -> Vec<Value> {
    observations
        .iter()
        .enumerate()
        .map(|(index, observation)| {
            let screenshot_artifact_id = observation
                .get("screenshot_artifact_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|artifact_id| is_valid_artifact_id(artifact_id));
            let visual_observation_artifact_id = observation
                .get("visual_observation_artifact_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|artifact_id| is_valid_artifact_id(artifact_id));

            json!({
                "step": observation
                    .get("step")
                    .and_then(Value::as_u64)
                    .unwrap_or(u64::try_from(index).unwrap_or(0)),
                "url": observation.get("url").and_then(Value::as_str).unwrap_or_default(),
                "title": observation.get("title").and_then(Value::as_str).unwrap_or_default(),
                "observedAt": observation.get("observed_at").and_then(Value::as_str).unwrap_or_default(),
                "screenshotArtifactId": screenshot_artifact_id,
                "screenshotUrl": screenshot_artifact_id.map(|artifact_id| artifact_url(run_id, artifact_id)),
                "visualObservationArtifactId": visual_observation_artifact_id,
                "visualObservationUrl": visual_observation_artifact_id.map(|artifact_id| artifact_url(run_id, artifact_id)),
                "consoleSummary": capped_observation_array(observation, "console_summary", MAX_TIMELINE_CONSOLE_ENTRIES),
                "networkSummary": capped_observation_array(observation, "network_summary", MAX_TIMELINE_NETWORK_ENTRIES),
                "policyDenials": capped_observation_array(observation, "policy_denials", MAX_TIMELINE_POLICY_DENIALS),
                "domNodeCount": observation
                    .get("dom_summary")
                    .and_then(|summary| summary.get("node_count"))
                    .and_then(Value::as_u64),
                "domInteractiveCount": observation
                    .get("dom_summary")
                    .and_then(|summary| summary.get("interactive_elements"))
                    .and_then(Value::as_array)
                    .map(Vec::len),
            })
        })
        .collect()
}

/// A bounded copy of an observation's array field, so timeline payloads stay
/// small even for chatty pages. Entries are passed through verbatim.
#[cfg(test)]
fn capped_observation_array(observation: &Value, key: &str, cap: usize) -> Value {
    Value::Array(
        observation
            .get(key)
            .and_then(Value::as_array)
            .map(|entries| entries.iter().take(cap).cloned().collect())
            .unwrap_or_default(),
    )
}

fn artifact_frame(run_id: &str, artifact_id: &str, kind: &str, media_type: &str) -> Value {
    json!({
        "kind": kind,
        "artifactId": artifact_id,
        "mediaType": media_type,
        "url": artifact_url(run_id, artifact_id)
    })
}

fn artifact_url(run_id: &str, artifact_id: &str) -> String {
    format!(
        "/api/v1/browser/sessions/{}/artifacts/{}",
        urlencoding::encode(run_id),
        urlencoding::encode(artifact_id)
    )
}

fn live_frame_url(run_id: &str) -> String {
    format!(
        "/api/v1/browser/sessions/{}/frame?format=jpeg&quality=65",
        urlencoding::encode(run_id)
    )
}

fn live_frame_stream_url(run_id: &str) -> String {
    format!(
        "/api/v1/browser/sessions/{}/frames/stream?format=jpeg&quality=65&intervalMs=250",
        urlencoding::encode(run_id)
    )
}

#[cfg(test)]
fn live_frame_ws_url(run_id: &str) -> String {
    format!(
        "/api/v1/browser/sessions/{}/frames/ws?format=jpeg&quality=65&intervalMs=250",
        urlencoding::encode(run_id)
    )
}

fn browser_tabs_url(run_id: &str) -> String {
    format!(
        "/api/v1/browser/sessions/{}/tabs",
        urlencoding::encode(run_id)
    )
}

fn browser_devtools_url(run_id: &str) -> String {
    format!(
        "/api/v1/browser/sessions/{}/devtools",
        urlencoding::encode(run_id)
    )
}

#[cfg(test)]
fn devtools_sequence(event: &Value) -> Option<u64> {
    event.get("sequence").and_then(Value::as_u64)
}

#[cfg(test)]
fn last_devtools_sequence(events: &[Value]) -> Option<u64> {
    events.iter().filter_map(devtools_sequence).max()
}

fn query_suffix(uri: &Uri) -> String {
    uri.query()
        .filter(|query| !query.is_empty())
        .map(|query| format!("?{query}"))
        .unwrap_or_default()
}

fn upstream_ws_url(base_url: &str, path_and_query: &str) -> Result<String, String> {
    let base =
        Url::parse(base_url).map_err(|err| format!("invalid Quarry Edge URL {base_url}: {err}"))?;
    let mut url = base
        .join(path_and_query.trim_start_matches('/'))
        .map_err(|err| format!("invalid browser websocket path: {err}"))?;
    let scheme = match url.scheme() {
        "http" => "ws",
        "https" => "wss",
        "ws" | "wss" => url.scheme(),
        other => return Err(format!("unsupported Quarry Edge scheme {other}")),
    }
    .to_owned();
    url.set_scheme(&scheme)
        .map_err(|_| format!("could not convert Quarry Edge URL to {scheme}"))?;
    Ok(url.to_string())
}

#[cfg(test)]
fn update_browser_observation(
    state: &AppState,
    session_id: &str,
    observation: Value,
) -> Option<BrowserRunMetadata> {
    let mut runs = state.browser_run_store.lock().ok()?;
    let mut metadata = runs.get(session_id)?.clone();
    metadata.last_observation = Some(observation.clone());
    metadata.observation_history.push(observation);
    if metadata.observation_history.len() > MAX_BROWSER_TIMELINE_ENTRIES {
        let remove_count = metadata.observation_history.len() - MAX_BROWSER_TIMELINE_ENTRIES;
        metadata.observation_history.drain(0..remove_count);
    }
    runs.insert(session_id.to_owned(), metadata.clone());
    Some(metadata)
}

#[cfg(test)]
fn update_browser_devtools_events(
    state: &AppState,
    session_id: &str,
    events: Vec<Value>,
) -> Option<BrowserRunMetadata> {
    let mut runs = state.browser_run_store.lock().ok()?;
    let metadata = runs.get(session_id)?;
    let mut by_sequence = metadata
        .devtools_events
        .iter()
        .filter_map(|event| devtools_sequence(event).map(|sequence| (sequence, event.clone())))
        .collect::<BTreeMap<_, _>>();
    for event in events {
        if let Some(sequence) = devtools_sequence(&event) {
            by_sequence.insert(sequence, event);
        }
    }
    let mut devtools_events = by_sequence.into_values().collect::<Vec<_>>();
    if devtools_events.len() > MAX_BROWSER_DEVTOOLS_EVENTS {
        let remove_count = devtools_events.len() - MAX_BROWSER_DEVTOOLS_EVENTS;
        devtools_events.drain(0..remove_count);
    }
    let next_metadata = BrowserRunMetadata {
        devtools_events,
        ..metadata.clone()
    };
    runs.insert(session_id.to_owned(), next_metadata.clone());
    Some(next_metadata)
}

#[cfg(test)]
fn append_replay_event(
    state: &AppState,
    session_id: &str,
    event: Value,
) -> Option<BrowserRunMetadata> {
    let mut runs = state.browser_run_store.lock().ok()?;
    let metadata = runs.get(session_id)?;
    let mut replay_events = metadata.replay_events.clone();
    replay_events.push(event);
    if replay_events.len() > MAX_BROWSER_REPLAY_EVENTS {
        let remove_count = replay_events.len() - MAX_BROWSER_REPLAY_EVENTS;
        replay_events.drain(0..remove_count);
    }
    let next_metadata = BrowserRunMetadata {
        replay_events,
        ..metadata.clone()
    };
    runs.insert(session_id.to_owned(), next_metadata.clone());
    Some(next_metadata)
}

#[cfg(test)]
fn update_browser_control_mode(
    state: &AppState,
    session_id: &str,
    control_mode: BrowserControlMode,
) -> Option<BrowserRunMetadata> {
    let mut runs = state.browser_run_store.lock().ok()?;
    let metadata = runs.get(session_id)?;
    let next_metadata = BrowserRunMetadata {
        control_mode,
        ..metadata.clone()
    };
    runs.insert(session_id.to_owned(), next_metadata.clone());
    Some(next_metadata)
}

#[cfg(test)]
fn browser_run_owner_matches(
    owner_user_id: &str,
    owner_org_id: &str,
    user_id: &str,
    org_id: &str,
) -> bool {
    !owner_user_id.is_empty()
        && !owner_org_id.is_empty()
        && !user_id.is_empty()
        && !org_id.is_empty()
        && owner_user_id == user_id
        && owner_org_id == org_id
}

async fn resolve_browser_run_owner(
    state: &AppState,
    user: &AuthenticatedUser,
) -> Result<BrowserRunOwner, Response> {
    let user_id = user.user_id.trim();
    let org_id = authorized_org_id(state, user).await;
    let org_id = org_id.trim();
    if user_id.is_empty() || org_id.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "browser_owner_unavailable",
                "An authenticated organization is required for browser access.",
            )),
        )
            .into_response());
    }
    Ok(BrowserRunOwner {
        user_id: user_id.to_owned(),
        org_id: org_id.to_owned(),
    })
}

/// The single "this browser run is not yours / not here" answer. Deliberately
/// indistinguishable between "no such id" and "owned by someone else" so the
/// route never confirms the existence of another tenant's run.
fn browser_session_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "browser_session_not_found",
            "The browser session is not active.",
        )),
    )
        .into_response()
}

/// Why the in-process `browser_run_store` did not hand back owned metadata.
///
/// The three cases are kept apart because they authorize differently: an id this
/// gateway *registered* under a different owner is a decided denial, an id it
/// never registered at all is simply unknown to this store — and may still be an
/// orchestration run the caller owns (see [`owned_orchestration_run`]) — and a
/// poisoned lock is an infrastructure failure that must fail closed. Collapsing
/// them, as a plain `Option` does, is what forces every unregistered-but-owned
/// run to 404.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserRunMiss {
    /// An entry exists under a DIFFERENT owner. A decided denial: it must never
    /// get a second authorization attempt.
    Foreign,
    /// No entry under this id. NOT an authorization decision.
    Absent,
    /// The store lock is poisoned; ownership cannot be verified at all.
    Unavailable,
}

#[cfg(test)]
impl BrowserRunMiss {
    /// The response for a miss that ends the request. `Absent` is deliberately
    /// answered exactly like `Foreign` so the route never confirms the existence
    /// of another tenant's run.
    fn into_response(self) -> Response {
        match self {
            Self::Foreign | Self::Absent => browser_session_not_found(),
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(error(
                    "browser_store_unavailable",
                    "Browser run ownership could not be verified.",
                )),
            )
                .into_response(),
        }
    }
}

/// Read the in-process store under an already-resolved owner, reporting *why* a
/// lookup missed so callers can tell a decided denial from an id this store
/// simply does not know about.
#[cfg(test)]
fn browser_run_lookup(
    state: &AppState,
    owner: &BrowserRunOwner,
    run_id: &str,
) -> Result<BrowserRunMetadata, BrowserRunMiss> {
    let runs = state
        .browser_run_store
        .lock()
        .map_err(|_| BrowserRunMiss::Unavailable)?;
    match runs.get(run_id) {
        None => Err(BrowserRunMiss::Absent),
        Some(metadata)
            if browser_run_owner_matches(
                &metadata.owner.user_id,
                &metadata.owner.org_id,
                &owner.user_id,
                &owner.org_id,
            ) =>
        {
            Ok(metadata.clone())
        }
        Some(_) => Err(BrowserRunMiss::Foreign),
    }
}

#[cfg(test)]
async fn owned_browser_run_metadata(
    state: &AppState,
    user: &AuthenticatedUser,
    run_id: &str,
) -> Result<BrowserRunMetadata, Response> {
    let owner = resolve_browser_run_owner(state, user).await?;
    browser_run_lookup(state, &owner, run_id).map_err(BrowserRunMiss::into_response)
}

/// How long a *proven* orchestration-run ownership decision may be reused.
///
/// The live panel renders one frame per agent step and the browser issues those
/// `<img>` requests in parallel, so re-probing the Model Plane per frame turns
/// one panel open into dozens of upstream round trips. Only positive decisions
/// are cached, and the key includes the caller's own validated org and user, so
/// an entry can never be served to a different principal; denials are never
/// cached, so a run that becomes readable is not stuck denied.
const BROWSER_RUN_OWNERSHIP_TTL_SECS: u64 = 120;

/// Prove the caller owns `run_id` as an **orchestration** run.
///
/// A chat turn's browser loop runs inside execution-core: it publishes its
/// events keyed by the orchestration run id and never passes through
/// `create_session` / `start_ai_run`, so `browser_run_store` — written only by
/// those two — has no entry for it and the in-process check alone can only ever
/// answer "not found". Ownership is instead proven with exactly the mechanism
/// that already authorizes the panel's own subscription to that same run
/// (`chat/streams.rs::run_events_stream`; `agents_runs.rs::get_run` is the
/// single-run read built on it):
///
/// 1. the caller's user id and org id are resolved SERVER-SIDE by
///    [`resolve_browser_run_owner`] — `user.user_id` plus `authorized_org_id`,
///    which reads the live membership decision `require_session` attached, never
///    a client-supplied header. Nothing on this route lets a caller choose the
///    identity the probe runs under.
/// 2. the caller's own short-lived, active-organization-bound
///    `aud=model-gateway` and `aud=session-core` credentials are minted from
///    their session cookie. The session credential is *required*, not
///    best-effort: `GET /v1/runs/{run_id}` extracts a verified
///    `aud=session-core` **user** bearer from `x-session-authorization` and
///    forwards exactly that to session-core, so it is what makes the check below
///    user-scoped rather than service-scoped. Losing it must read as "auth
///    unavailable", never as "this run is not yours".
/// 3. the Model Plane run read model is asked to resolve the run under exactly
///    that identity (`GET /v1/runs/{run_id}` → session-core
///    `RunService.GetRun` → `authorize_run_owner`), which authorizes against the
///    stored `runs.org_id` / `runs.user_id`. Because the forwarded principal is a
///    user and not a service, both halves apply: another org is
///    `permission_denied`, another user is `permission_denied`, an unknown id is
///    `not_found`.
///
/// Every other outcome denies: a non-success status, a transport failure, a
/// degraded upstream that answers `200` with no run, or a run whose echoed id is
/// not the one asked for. There is no allow-on-error path.
async fn owned_orchestration_run(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    owner: &BrowserRunOwner,
    run_id: &str,
) -> Result<(), Response> {
    let cache_key = crate::cache::cache_key(
        "browser-run-owner",
        &[&owner.org_id, &owner.user_id, run_id],
    );
    if state
        .cache
        .lookup_within(&cache_key, BROWSER_RUN_OWNERSHIP_TTL_SECS)
        .await
        .is_some()
    {
        return Ok(());
    }

    let (token, session) = tokio::join!(
        model_token(state, user, headers),
        required_session_token(state, user, headers),
    );
    let session = match session {
        Ok(token) => token,
        Err(error) => return Err(delegated_auth_unavailable(error).into_response()),
    };
    let url = format!(
        "{}/v1/runs/{}",
        state.model_gateway_url,
        urlencoding::encode(run_id)
    );
    let (status, Json(body)) = proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&session),
        user,
    )
    .await;
    if !status.is_success() || !run_detail_confirms(&body, run_id) {
        return Err(browser_session_not_found());
    }

    state
        .cache
        .store_for_secs(
            &cache_key,
            &json!({ "run_id": run_id }),
            BROWSER_RUN_OWNERSHIP_TTL_SECS,
        )
        .await;
    Ok(())
}

/// A run read only counts as ownership proof when the upstream actually returned
/// *the* run. A `200` carrying an empty envelope, an error envelope, or some
/// other run must never read as an authorization.
fn run_detail_confirms(body: &Value, run_id: &str) -> bool {
    if body.get("error").is_some() {
        return false;
    }
    body.get("run")
        .or_else(|| body.get("data").and_then(|data| data.get("run")))
        .and_then(|run| run.get("run_id"))
        .and_then(Value::as_str)
        .is_some_and(|value| value == run_id)
}

/// Authorize an evidence-artifact read for `run_id`, whichever way the run was
/// started.
///
/// Checks the in-process store first, so a session registered by
/// `create_session` / `start_ai_run` keeps its recorded owner as the only
/// answer and that path is untouched. Only a genuinely unregistered id falls
/// through to the orchestration-run proof — a store entry owned by someone else,
/// and a poisoned store, both deny here rather than getting a second attempt.
async fn owned_browser_artifact_run(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    run_id: &str,
) -> Result<BrowserArtifactRunAccess, Response> {
    let owner = resolve_browser_run_owner(state, user).await?;
    let cookie = cookie_header(headers);
    let quarry_bearer = quarry_token(state, user, &cookie).await;
    match owner_browser_session_projection(state, user, quarry_bearer.as_deref(), run_id).await {
        Ok(projection) => Ok(BrowserArtifactRunAccess {
            zdr: projection
                .get("zdr")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        // A browser-session read deliberately hides both an unknown id and a
        // foreign id. The only legitimate second authority is a Model Plane
        // orchestration run, which independently rechecks the same actor and
        // tenant before granting its evidence path.
        Err(response) if response.status() == StatusCode::NOT_FOUND => {
            owned_orchestration_run(state, user, headers, &owner, run_id).await?;
            Ok(BrowserArtifactRunAccess { zdr: false })
        }
        Err(response) => Err(response),
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserRunStoreError {
    Unavailable,
    Conflict,
}

#[cfg(test)]
impl BrowserRunStoreError {
    fn into_response(self) -> Response {
        match self {
            Self::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(error(
                    "browser_store_unavailable",
                    "Browser run ownership could not be recorded.",
                )),
            )
                .into_response(),
            Self::Conflict => (
                StatusCode::CONFLICT,
                Json(error(
                    "browser_run_conflict",
                    "The browser run id is already active.",
                )),
            )
                .into_response(),
        }
    }
}

#[cfg(test)]
fn store_browser_run_metadata(
    state: &AppState,
    run_id: &str,
    metadata: BrowserRunMetadata,
) -> Result<(), BrowserRunStoreError> {
    let mut runs = state
        .browser_run_store
        .lock()
        .map_err(|_| BrowserRunStoreError::Unavailable)?;
    if runs.contains_key(run_id) {
        return Err(BrowserRunStoreError::Conflict);
    }
    runs.insert(run_id.to_owned(), metadata);
    Ok(())
}

#[cfg(test)]
fn browser_run_metadata(state: &AppState, session_id: &str) -> BrowserRunMetadata {
    state
        .browser_run_store
        .lock()
        .ok()
        .and_then(|runs| runs.get(session_id).cloned())
        .unwrap_or(BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: None,
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: None,
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        })
}

fn screenshot_artifact_id(observation: &Value) -> Option<String> {
    observation
        .get("screenshot_artifact_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|artifact_id| is_valid_artifact_id(artifact_id))
        .map(str::to_owned)
}

fn visual_observation_artifact_id(observation: &Value) -> Option<String> {
    observation
        .get("visual_observation_artifact_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|artifact_id| is_valid_artifact_id(artifact_id))
        .map(str::to_owned)
}

fn trimmed_goal(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "Decide the next safe browser action for evidence capture.".to_owned();
    }
    trimmed.chars().take(1200).collect()
}

async fn fetch_visual_observation(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    artifact_id: &str,
) -> Result<Value, Response> {
    let Some((bytes, _content_type)) =
        fetch_quarry_artifact_bytes(state, user, token, artifact_id, MAX_MODEL_VISUAL_JSON_BYTES)
            .await?
    else {
        return Ok(Value::Null);
    };
    serde_json::from_slice::<Value>(&bytes).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "browser_visual_observation_invalid",
                "The browser visual observation artifact is not valid JSON.",
            )),
        )
            .into_response()
    })
}

async fn fetch_model_screenshot(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    artifact_id: &str,
) -> Result<Option<(String, String)>, Response> {
    let Some((bytes, upstream_content_type)) =
        fetch_quarry_artifact_bytes(state, user, token, artifact_id, MAX_MODEL_SCREENSHOT_BYTES)
            .await?
    else {
        return Ok(None);
    };
    let content_type = safe_model_screenshot_content_type(upstream_content_type.as_deref());
    Ok(Some((
        content_type.to_owned(),
        BASE64_STANDARD.encode(bytes.as_ref()),
    )))
}

async fn fetch_browser_tabs(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    session_id: &str,
) -> Result<Vec<Value>, Response> {
    let (status, body) = quarry_call(
        state,
        Method::GET,
        &format!("/v1/agent/runs/{}/tabs", urlencoding::encode(session_id)),
        None,
        token,
        &user.user_id,
    )
    .await;
    if !status.is_success() {
        return Err(forward_quarry_failure(status, body));
    }
    Ok(tabs_from_data(&unwrap_data(&body)))
}

fn tabs_from_data(data: &Value) -> Vec<Value> {
    data.get("tabs")
        .and_then(Value::as_array)
        .map(|tabs| tabs.to_vec())
        .unwrap_or_default()
}

async fn fetch_quarry_artifact_bytes(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    artifact_id: &str,
    max_bytes: u64,
) -> Result<Option<(bytes::Bytes, Option<String>)>, Response> {
    let url = format!(
        "{}/v1/artifacts/{}",
        state.quarry_edge_url,
        urlencoding::encode(artifact_id)
    );
    let mut req = state.client.get(&url).header("x-user-id", &user.user_id);
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }

    let upstream = req.send().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )
            .into_response()
    })?;
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    if !status.is_success() {
        return Ok(None);
    }
    let content_type = upstream
        .headers()
        .get(header::CONTENT_TYPE.as_str())
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if upstream
        .content_length()
        .is_some_and(|length| length > max_bytes)
    {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(error(
                "browser_artifact_too_large",
                "The browser artifact is too large for model reasoning.",
            )),
        )
            .into_response());
    }

    let bytes = upstream.bytes().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            Json(crate::envelope::upstream_unavailable()),
        )
            .into_response()
    })?;
    if bytes.len() as u64 > max_bytes {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(error(
                "browser_artifact_too_large",
                "The browser artifact is too large for model reasoning.",
            )),
        )
            .into_response());
    }

    Ok(Some((bytes, content_type)))
}

fn normalize_optional_public_url(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if raw.eq_ignore_ascii_case("about:blank") {
        return Ok(None);
    }
    normalize_public_http_url(raw).map(Some)
}

fn sanitize_action(mut action: Value) -> Result<Value, (&'static str, String)> {
    let action_type = action
        .get("type")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();

    match action_type {
        "navigate" => {
            let raw = action.get("url").and_then(Value::as_str).ok_or_else(|| {
                (
                    "invalid_browser_action",
                    "Navigate requires a URL.".to_owned(),
                )
            })?;
            let normalized =
                normalize_public_http_url(raw).map_err(|message| ("invalid_url", message))?;
            action["url"] = Value::String(normalized);
            Ok(action)
        }
        "click" | "press" | "scroll" | "select" | "wait" | "wait_for" | "screenshot" | "back"
        | "forward" | "get_content" => Ok(action),
        "click_point" => {
            validate_viewport_coordinate(&action, "x")?;
            validate_viewport_coordinate(&action, "y")?;
            Ok(action)
        }
        "mouse_wheel" => {
            validate_viewport_coordinate(&action, "x")?;
            validate_viewport_coordinate(&action, "y")?;
            validate_wheel_delta(&action, "delta_x")?;
            validate_wheel_delta(&action, "delta_y")?;
            Ok(action)
        }
        "type" => {
            if action
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .len()
                > 4000
            {
                return Err((
                    "invalid_browser_action",
                    "Typed browser text is too large.".to_owned(),
                ));
            }
            Ok(action)
        }
        "evaluate" => Err((
            "browser_action_denied",
            "Raw browser script evaluation is not exposed through the Verevon UI facade."
                .to_owned(),
        )),
        _ => Err((
            "invalid_browser_action",
            "Unsupported browser action.".to_owned(),
        )),
    }
}

fn validate_viewport_coordinate(
    action: &Value,
    field: &'static str,
) -> Result<(), (&'static str, String)> {
    let value = action.get(field).and_then(Value::as_f64).ok_or_else(|| {
        (
            "invalid_browser_action",
            format!("{field} must be a finite viewport coordinate."),
        )
    })?;
    if value.is_finite() && (0.0..=10_000.0).contains(&value) {
        Ok(())
    } else {
        Err((
            "invalid_browser_action",
            format!("{field} must be between 0 and 10000 CSS pixels."),
        ))
    }
}

fn validate_wheel_delta(action: &Value, field: &'static str) -> Result<(), (&'static str, String)> {
    let value = action.get(field).and_then(Value::as_f64).ok_or_else(|| {
        (
            "invalid_browser_action",
            format!("{field} must be a finite wheel delta."),
        )
    })?;
    if value.is_finite() && (-5000.0..=5000.0).contains(&value) {
        Ok(())
    } else {
        Err((
            "invalid_browser_action",
            format!("{field} must be between -5000 and 5000 CSS pixels."),
        ))
    }
}

async fn quarry_call(
    state: &AppState,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
    user_id: &str,
) -> (StatusCode, Value) {
    let url = format!("{}{}", state.quarry_edge_url, path);
    let (status, Json(value)) = proxy_bearer_json(state, method, &url, body, token, user_id).await;
    (status, value)
}

async fn quarry_token(state: &AppState, user: &AuthenticatedUser, cookie: &str) -> Option<String> {
    if let Some(token) = get_audience_token(state, &user.user_id, cookie, "quarry").await {
        return Some(token);
    }
    if let Some(token) = get_onboarding_preview_token(state, &user.user_id, cookie).await {
        return Some(token);
    }
    dev_quarry_token(state)
}

fn dev_quarry_token(state: &AppState) -> Option<String> {
    dev_quarry_token_for(state.allow_dev_auth_bypass)
}

fn dev_quarry_token_for(allow_dev_auth_bypass: bool) -> Option<String> {
    allow_dev_auth_bypass.then(|| "dev-bypass".to_owned())
}

fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

/// Why an authorized artifact read has nothing to return.
///
/// Both variants are terminal — no image will ever arrive — and both answer
/// `404`. They are separate codes because the panel shows different copy for
/// them, and because neither may be confused with the two states that *do* mean
/// something went wrong: an authorization denial (`browser_session_not_found`)
/// or a transport/upstream failure (`5xx`). A ZDR run must land here, never on
/// either of those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserArtifactAbsence {
    /// Zero Data Retention run: Quarry refused the artifact write at capture
    /// time, so nothing was ever stored and nothing ever will be.
    Withheld,
    /// The store has no such artifact — never written (a ZDR run reaching this
    /// path without a local ZDR flag) or aged out of retention.
    NeverStored,
}

impl BrowserArtifactAbsence {
    fn into_response(self) -> Response {
        let (code, message) = match self {
            Self::Withheld => (
                "browser_artifact_withheld",
                "Zero Data Retention is on for this run, so no screenshot was captured.",
            ),
            Self::NeverStored => (
                "browser_artifact_missing",
                "The browser artifact is no longer available.",
            ),
        };
        (StatusCode::NOT_FOUND, Json(error(code, message))).into_response()
    }
}

fn forward_quarry_failure(status: StatusCode, body: Value) -> Response {
    if body.get("error").is_some() {
        return (status, Json(body)).into_response();
    }
    (
        status,
        Json(error(
            "browser_session_failed",
            "Quarry browser session is not available.",
        )),
    )
        .into_response()
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn hostname(value: &str) -> Option<String> {
    Url::parse(value)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
}

fn is_valid_path_segment(value: &str) -> bool {
    let len = value.len();
    (3..=128).contains(&len)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn is_valid_artifact_id(value: &str) -> bool {
    value
        .strip_prefix("art_")
        .is_some_and(|suffix| !suffix.is_empty() && is_valid_path_segment(value))
}

fn is_valid_profile_id(value: &str) -> bool {
    value
        .strip_prefix("prof_")
        .is_some_and(|suffix| !suffix.is_empty() && is_valid_path_segment(value))
}

fn safe_browser_artifact_content_type(value: Option<&str>, bytes: &[u8]) -> HeaderValue {
    match value
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
    {
        "image/png" => HeaderValue::from_static("image/png"),
        "image/jpeg" => HeaderValue::from_static("image/jpeg"),
        "image/webp" => HeaderValue::from_static("image/webp"),
        "application/json" => HeaderValue::from_static("application/json"),
        "application/problem+json" => HeaderValue::from_static("application/json"),
        _ if looks_like_json_payload(bytes) => HeaderValue::from_static("application/json"),
        _ => HeaderValue::from_static("image/png"),
    }
}

fn safe_model_screenshot_content_type(value: Option<&str>) -> &'static str {
    match value
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
    {
        "image/jpeg" => "image/jpeg",
        "image/webp" => "image/webp",
        _ => "image/png",
    }
}

fn looks_like_json_payload(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .copied()
        .find(|b| !matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        .is_some_and(|b| matches!(b, b'{' | b'['))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_run_owner_requires_exact_validated_user_and_org() {
        assert!(browser_run_owner_matches(
            "user-owner",
            "org-owner",
            "user-owner",
            "org-owner"
        ));
        assert!(!browser_run_owner_matches(
            "user-owner",
            "org-owner",
            "user-attacker",
            "org-owner"
        ));
        assert!(!browser_run_owner_matches(
            "user-owner",
            "org-owner",
            "user-owner",
            "org-attacker"
        ));
        assert!(!browser_run_owner_matches(
            "user-owner",
            "org-owner",
            "user-owner",
            ""
        ));
    }

    #[tokio::test]
    async fn browser_session_read_proxies_the_quarry_owner_projection_without_cache_fallback() {
        use axum::body::to_bytes;
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-owner/browser-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "runId": "run-owner",
                    "status": "active",
                    "live": true,
                    "tabs": []
                }
            })))
            .mount(&quarry)
            .await;

        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        let mut stale = browser_run_metadata(&state, "missing");
        stale.owner = owner_of("user-owner", "org-owner");
        stale.last_observation = Some(json!({ "url": "https://stale.example.test" }));
        store_browser_run_metadata(&state, "run-owner", stale)
            .expect("the stale compatibility cache can exist");

        let response = get_session(
            State(state),
            Extension(test_user("user-owner", "org-owner")),
            HeaderMap::new(),
            Path("run-owner".to_owned()),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&body).expect("JSON response");
        assert_eq!(body["data"]["runId"], "run-owner");
        assert_eq!(body["data"]["status"], "active");
        assert!(body["data"].get("observation").is_none());
        assert!(body["data"].get("replay").is_none());
        assert_eq!(
            quarry
                .received_requests()
                .await
                .expect("recorded Quarry request")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn browser_timeline_read_proxies_quarry_without_legacy_replay_merge() {
        use axum::body::to_bytes;
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-owner/browser-session/timeline"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "items": [{
                        "id": "bevt_owner_01",
                        "occurredAt": "2026-08-11T10:00:00Z",
                        "kind": "control",
                        "mode": "human_takeover",
                        "initiatedBy": "human"
                    }],
                    "nextCursor": null
                }
            })))
            .mount(&quarry)
            .await;

        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        let mut stale = browser_run_metadata(&state, "missing");
        stale.owner = owner_of("user-owner", "org-owner");
        stale.replay_events =
            vec![json!({ "kind": "observation", "url": "https://stale.example.test" })];
        store_browser_run_metadata(&state, "run-owner", stale)
            .expect("the stale compatibility cache can exist");

        let response = get_owner_timeline(
            State(state),
            Extension(test_user("user-owner", "org-owner")),
            HeaderMap::new(),
            Path("run-owner".to_owned()),
            Uri::from_static("/timeline?limit=25"),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&body).expect("JSON response");
        assert_eq!(body["data"]["items"][0]["id"], "bevt_owner_01");
        assert!(body["data"].get("replay").is_none());
        assert_eq!(
            quarry
                .received_requests()
                .await
                .expect("recorded Quarry request")
                .len(),
            1
        );
    }

    fn test_user(user_id: &str, org_id: &str) -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: user_id.to_owned(),
            user_email: format!("{user_id}@example.test"),
            user_name: user_id.to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: None,
            active_org_id: Some(org_id.to_owned()),
            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                organization_id: org_id.to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    #[tokio::test]
    async fn browser_run_lookup_hides_metadata_from_wrong_user_or_org() {
        let state = test_app_state();
        let mut metadata = browser_run_metadata(&state, "missing");
        metadata.owner = BrowserRunOwner {
            user_id: "user-owner".to_owned(),
            org_id: "org-owner".to_owned(),
        };
        metadata.last_observation = Some(json!({ "url": "https://private.example" }));
        store_browser_run_metadata(&state, "run-owned", metadata).expect("store owned run");

        let wrong_user = owned_browser_run_metadata(
            &state,
            &test_user("user-attacker", "org-owner"),
            "run-owned",
        )
        .await
        .expect_err("wrong user must not read run metadata");
        let wrong_org = owned_browser_run_metadata(
            &state,
            &test_user("user-owner", "org-attacker"),
            "run-owned",
        )
        .await
        .expect_err("wrong org must not read run metadata");

        assert_eq!(wrong_user.status(), StatusCode::NOT_FOUND);
        assert_eq!(wrong_org.status(), StatusCode::NOT_FOUND);
        assert!(owned_browser_run_metadata(
            &state,
            &test_user("user-owner", "org-owner"),
            "run-owned"
        )
        .await
        .is_ok());
    }

    // ---------------------------------------------------------------------
    // Orchestration-run ownership (chat live panel frames)
    // ---------------------------------------------------------------------

    /// Stand in for model-gateway's `GET /v1/runs/:run_id`, reproducing the
    /// authority the real chain applies: session-core answers from the stored
    /// `runs.org_id` / `runs.user_id`, so `run-owned` resolves only for
    /// `org-owner` + `user-owner` and every other principal is
    /// `permission_denied`. Any other run id is unknown.
    async fn model_plane_run_read_model() -> wiremock::MockServer {
        use wiremock::{
            matchers::{header, method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let upstream = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/runs/run-owned"))
            .and(header("x-org-id", "org-owner"))
            .and(header("x-user-id", "user-owner"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "run": { "run_id": "run-owned", "status": "running" }
            })))
            .with_priority(1)
            .mount(&upstream)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/runs/run-owned"))
            .respond_with(
                ResponseTemplate::new(403).set_body_json(json!({ "error": "run access denied" })),
            )
            .with_priority(5)
            .mount(&upstream)
            .await;
        Mock::given(wm_method("GET"))
            .respond_with(
                ResponseTemplate::new(404).set_body_json(json!({ "error": "run not found" })),
            )
            .with_priority(10)
            .mount(&upstream)
            .await;
        upstream
    }

    /// Stand in for auth-core's plane-token endpoints so the tests run the real
    /// credential path (`model_token` + `required_session_token`) instead of
    /// skipping it. `GET /api/{audience}/token` → `{ "token": ... }`.
    async fn plane_token_issuer() -> wiremock::MockServer {
        use wiremock::{
            matchers::{method as wm_method, path_regex},
            Mock, MockServer, ResponseTemplate,
        };

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(path_regex(r"^/api/[a-z-]+/token$"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "token": "test-plane-token" })),
            )
            .mount(&auth)
            .await;
        auth
    }

    fn state_for(
        auth_core: &wiremock::MockServer,
        run_read_model: &wiremock::MockServer,
    ) -> AppState {
        let mut state = test_app_state();
        state.auth_core_url = auth_core.uri();
        state.model_gateway_url = run_read_model.uri();
        state
    }

    fn owner_of(user_id: &str, org_id: &str) -> BrowserRunOwner {
        BrowserRunOwner {
            user_id: user_id.to_owned(),
            org_id: org_id.to_owned(),
        }
    }

    /// The whole point of this route: the run's owner may read its evidence and
    /// nobody else may — not another user in the same org, not the same user id
    /// in another org, and not a run id that does not exist.
    #[tokio::test]
    async fn orchestration_run_ownership_admits_only_the_runs_own_user_and_org() {
        let upstream = model_plane_run_read_model().await;
        let auth = plane_token_issuer().await;
        let state = state_for(&auth, &upstream);
        let headers = HeaderMap::new();

        let owner = owned_orchestration_run(
            &state,
            &test_user("user-owner", "org-owner"),
            &headers,
            &owner_of("user-owner", "org-owner"),
            "run-owned",
        )
        .await;
        assert!(owner.is_ok(), "the run's own owner must be able to read it");

        let wrong_user = owned_orchestration_run(
            &state,
            &test_user("user-attacker", "org-owner"),
            &headers,
            &owner_of("user-attacker", "org-owner"),
            "run-owned",
        )
        .await
        .expect_err("another user in the same org must be denied");
        assert_eq!(wrong_user.status(), StatusCode::NOT_FOUND);

        let cross_org = owned_orchestration_run(
            &state,
            &test_user("user-owner", "org-attacker"),
            &headers,
            &owner_of("user-owner", "org-attacker"),
            "run-owned",
        )
        .await
        .expect_err("the same user id in another org must be denied");
        assert_eq!(cross_org.status(), StatusCode::NOT_FOUND);

        let unknown = owned_orchestration_run(
            &state,
            &test_user("user-owner", "org-owner"),
            &headers,
            &owner_of("user-owner", "org-owner"),
            "run-does-not-exist",
        )
        .await
        .expect_err("an unknown run id must be denied");
        assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    }

    /// Tenant isolation depends on the probe running under the caller's own
    /// validated identity, so a client-supplied `x-user-id` / `x-org-id` must
    /// never reach the upstream. If it could, forging those headers would hand
    /// the caller another tenant's frames.
    #[tokio::test]
    async fn orchestration_probe_ignores_client_supplied_identity_headers() {
        let upstream = model_plane_run_read_model().await;
        let auth = plane_token_issuer().await;
        let state = state_for(&auth, &upstream);
        let mut headers = HeaderMap::new();
        headers.insert("x-user-id", "user-owner".parse().unwrap());
        headers.insert("x-org-id", "org-owner".parse().unwrap());

        // A caller validated as `user-attacker`/`org-attacker` claiming to be the
        // owner in request headers is still probed as themselves, so denied.
        let forged = owned_orchestration_run(
            &state,
            &test_user("user-attacker", "org-attacker"),
            &headers,
            &owner_of("user-attacker", "org-attacker"),
            "run-owned",
        )
        .await
        .expect_err("forged identity headers must not authorize a foreign run");
        assert_eq!(forged.status(), StatusCode::NOT_FOUND);

        let received = upstream
            .received_requests()
            .await
            .expect("recorded requests");
        let last = received.last().expect("the probe must reach the upstream");
        assert_eq!(
            last.headers.get("x-user-id").map(|v| v.to_str().unwrap()),
            Some("user-attacker"),
            "the probe must carry the validated user, not the client's claim"
        );
        assert_eq!(
            last.headers.get("x-org-id").map(|v| v.to_str().unwrap()),
            Some("org-attacker"),
            "the probe must carry the validated org, not the client's claim"
        );
    }

    /// The session credential is what makes the upstream check user-scoped, so
    /// losing it must deny — but as "auth unavailable", not as a false
    /// "this run is not yours". Either way no bytes are served.
    #[tokio::test]
    async fn losing_the_session_credential_denies_without_claiming_the_run_is_foreign() {
        let upstream = model_plane_run_read_model().await;
        let mut state = test_app_state();
        state.model_gateway_url = upstream.uri();
        // `auth_core_url` is unreachable, so no `aud=session-core` token mints.

        let denied = owned_orchestration_run(
            &state,
            &test_user("user-owner", "org-owner"),
            &HeaderMap::new(),
            &owner_of("user-owner", "org-owner"),
            "run-owned",
        )
        .await
        .expect_err("an unmintable session credential must deny");
        assert_eq!(denied.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(
            upstream
                .received_requests()
                .await
                .expect("recorded requests")
                .is_empty(),
            "the probe must not run unscoped when the session credential is missing"
        );
    }

    /// A `200` is only proof when the upstream really returned the run asked
    /// for. A degraded or generic answer must not read as an authorization.
    #[test]
    fn run_detail_only_confirms_the_requested_run() {
        assert!(run_detail_confirms(
            &json!({ "run": { "run_id": "run-owned" } }),
            "run-owned"
        ));
        assert!(run_detail_confirms(
            &json!({ "data": { "run": { "run_id": "run-owned" } } }),
            "run-owned"
        ));
        assert!(!run_detail_confirms(&json!({}), "run-owned"));
        assert!(!run_detail_confirms(&json!({ "run": {} }), "run-owned"));
        assert!(!run_detail_confirms(
            &json!({ "run": { "run_id": "run-somebody-else" } }),
            "run-owned"
        ));
        assert!(!run_detail_confirms(
            &json!({ "error": { "code": "forbidden" }, "run": { "run_id": "run-owned" } }),
            "run-owned"
        ));
    }

    /// A stale test fixture must never authorize a browser run. Quarry's scoped
    /// projection is the only authority, even when the legacy fixture claims
    /// that the caller owns the same id.
    #[tokio::test]
    async fn quarry_owner_denial_wins_over_a_stale_browser_fixture() {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-owned/browser-session"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "not_found", "message": "run is not visible" }
            })))
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let mut metadata = browser_run_metadata(&state, "missing");
        metadata.owner = owner_of("user-owner", "org-owner");
        store_browser_run_metadata(&state, "run-owned", metadata).expect("store owned session");

        let denied = owner_browser_session_projection(
            &state,
            &test_user("user-owner", "org-owner"),
            None,
            "run-owned",
        )
        .await
        .expect_err("Quarry's scoped denial must override stale local state");
        assert_eq!(denied.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            quarry
                .received_requests()
                .await
                .expect("recorded requests")
                .len(),
            1,
            "the gateway performs exactly the owner-scoped Quarry read"
        );
    }

    /// An id Quarry does not own can fall through to the Model Plane's
    /// independently actor-scoped orchestration proof without creating a BFF
    /// browser-session record.
    #[tokio::test]
    async fn an_unregistered_orchestration_run_keeps_browser_cache_empty() {
        use wiremock::MockServer;

        let upstream = model_plane_run_read_model().await;
        let auth = plane_token_issuer().await;
        let quarry = MockServer::start().await;
        let mut state = state_for(&auth, &upstream);
        state.quarry_edge_url = quarry.uri();

        let metadata = owned_browser_artifact_run(
            &state,
            &test_user("user-owner", "org-owner"),
            &HeaderMap::new(),
            "run-owned",
        )
        .await
        .expect("the run's owner may read its evidence");
        assert!(!metadata.zdr);
        assert!(
            !state
                .browser_run_store
                .lock()
                .expect("store")
                .contains_key("run-owned"),
            "an orchestration run must not be registered as a browser session, \
             which would make every other /browser/sessions route accept it"
        );
    }

    // ---------------------------------------------------------------------
    // Artifact responses (owner reads bytes; ZDR reads a clean "no artifact")
    // ---------------------------------------------------------------------

    async fn artifact_response_body(response: Response) -> Value {
        use http_body_util::BodyExt;

        serde_json::from_slice(
            &response
                .into_body()
                .collect()
                .await
                .expect("artifact response body")
                .to_bytes(),
        )
        .expect("JSON error envelope")
    }

    async fn mount_browser_owner_projection(
        quarry: &wiremock::MockServer,
        session_id: &str,
        zdr: bool,
    ) {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, ResponseTemplate,
        };

        Mock::given(wm_method("GET"))
            .and(wm_path(format!(
                "/v1/agent/runs/{session_id}/browser-session"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "runId": session_id,
                    "leaseId": "lease_owner_01",
                    "profileId": "prof_owner_01",
                    "profileStorage": "ephemeral",
                    "profileScope": "ephemeral",
                    "viewport": { "width": 1280, "height": 800 },
                    "currentUrl": "https://example.test/",
                    "step": 1,
                    "status": "active",
                    "live": true,
                    "zdr": zdr,
                    "controlMode": "agent_control",
                    "tabs": [],
                    "lastObservation": null
                }
            })))
            .mount(quarry)
            .await;
    }

    /// PNG magic bytes — enough for `safe_browser_artifact_content_type` to keep
    /// the upstream image type rather than sniffing it as JSON.
    const PNG_BYTES: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    #[tokio::test]
    async fn the_owner_of_a_registered_session_reads_the_real_artifact_bytes() {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_shot1"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(PNG_BYTES.to_vec(), "image/png"))
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        mount_browser_owner_projection(&quarry, "run-owned", false).await;

        let response = get_artifact(
            State(state),
            Extension(test_user("user-owner", "org-owner")),
            HeaderMap::new(),
            Path(("run-owned".to_owned(), "art_shot1".to_owned())),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("image/png")
        );
    }

    /// ZDR is a distinct, terminal outcome: `404` with its own code, no upstream
    /// call at all — never a 5xx and never an authorization error, both of which
    /// the panel renders as different copy.
    #[tokio::test]
    async fn a_zdr_run_yields_a_clean_no_artifact_outcome_without_touching_quarry() {
        use wiremock::MockServer;

        let quarry = MockServer::start().await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        mount_browser_owner_projection(&quarry, "run-zdr01", true).await;

        let response = get_artifact(
            State(state),
            Extension(test_user("user-owner", "org-owner")),
            HeaderMap::new(),
            Path(("run-zdr01".to_owned(), "art_shot1".to_owned())),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            artifact_response_body(response).await["error"]["code"],
            json!("browser_artifact_withheld")
        );
        assert!(
            quarry
                .received_requests()
                .await
                .expect("recorded requests")
                .iter()
                .all(|request| request.url.path() != "/v1/artifacts/art_shot1"),
            "a ZDR run may read its owner projection but must never fetch a capture"
        );
    }

    /// The same terminal fact reached the other way: capture was never stored
    /// (ZDR without a local flag) or has aged out. Still `404` with a stable
    /// code, not Quarry's envelope and not a 5xx.
    #[tokio::test]
    async fn an_absent_capture_is_normalized_into_one_stable_no_artifact_code() {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let quarry = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/artifacts/art_shot1"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "not_found", "message": "artifact art_shot1 not found" }
            })))
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        mount_browser_owner_projection(&quarry, "run-owned", false).await;

        let response = get_artifact(
            State(state),
            Extension(test_user("user-owner", "org-owner")),
            HeaderMap::new(),
            Path(("run-owned".to_owned(), "art_shot1".to_owned())),
        )
        .await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            artifact_response_body(response).await["error"]["code"],
            json!("browser_artifact_missing")
        );
    }

    /// The `art_` prefix rule the SPA mirrors client-side must stay enforced, so
    /// a malformed reference is rejected before any ownership or upstream work.
    #[tokio::test]
    async fn malformed_artifact_references_are_rejected_before_authorization() {
        let state = test_app_state();
        for artifact_id in [
            "shot1",
            "art_",
            "art_/../etc",
            &format!("art_{}", "a".repeat(200)),
        ] {
            let response = get_artifact(
                State(state.clone()),
                Extension(test_user("user-owner", "org-owner")),
                HeaderMap::new(),
                Path(("run-owned".to_owned(), artifact_id.to_owned())),
            )
            .await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "`{artifact_id}` must not be accepted as an artifact id"
            );
        }
    }

    #[test]
    fn browser_run_store_never_overwrites_existing_owner() {
        let state = test_app_state();
        let mut owner_metadata = browser_run_metadata(&state, "missing");
        owner_metadata.owner = BrowserRunOwner {
            user_id: "user-owner".to_owned(),
            org_id: "org-owner".to_owned(),
        };
        store_browser_run_metadata(&state, "run-owned", owner_metadata)
            .expect("store original owner");

        let mut attacker_metadata = browser_run_metadata(&state, "missing");
        attacker_metadata.owner = BrowserRunOwner {
            user_id: "user-attacker".to_owned(),
            org_id: "org-attacker".to_owned(),
        };
        assert_eq!(
            store_browser_run_metadata(&state, "run-owned", attacker_metadata),
            Err(BrowserRunStoreError::Conflict)
        );
        assert_eq!(
            browser_run_metadata(&state, "run-owned").owner.user_id,
            "user-owner"
        );
    }

    #[test]
    fn browser_run_store_errors_are_fail_closed() {
        assert_eq!(
            BrowserRunStoreError::Unavailable.into_response().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            BrowserRunStoreError::Conflict.into_response().status(),
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn browser_run_owner_rejects_missing_validated_identity() {
        let state = test_app_state();
        let missing_user = resolve_browser_run_owner(&state, &test_user("", "org-owner"))
            .await
            .expect_err("blank user id must fail closed");
        let missing_org = resolve_browser_run_owner(&state, &test_user("user-owner", " "))
            .await
            .expect_err("blank org id must fail closed");

        assert_eq!(missing_user.status(), StatusCode::FORBIDDEN);
        assert_eq!(missing_org.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn browser_run_store_poisoning_fails_closed() {
        let state = test_app_state();
        let store = state.browser_run_store.clone();
        let _ = std::thread::spawn(move || {
            let _guard = store.lock().expect("acquire store before poisoning");
            panic!("poison browser store for fail-closed test");
        })
        .join();

        let lookup =
            owned_browser_run_metadata(&state, &test_user("user-owner", "org-owner"), "run-owned")
                .await
                .expect_err("poisoned lookup must fail closed");
        assert_eq!(lookup.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            store_browser_run_metadata(
                &state,
                "run-owned",
                BrowserRunMetadata {
                    owner: BrowserRunOwner {
                        user_id: "user-owner".to_owned(),
                        org_id: "org-owner".to_owned(),
                    },
                    ..browser_run_metadata(&test_app_state(), "missing")
                }
            ),
            Err(BrowserRunStoreError::Unavailable)
        );
    }

    #[tokio::test]
    async fn browser_http_operations_use_quarry_owner_denials_not_bff_cache() {
        use wiremock::MockServer;

        let quarry = MockServer::start().await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        let mut metadata = browser_run_metadata(&state, "missing");
        metadata.owner = BrowserRunOwner {
            user_id: "user-owner".to_owned(),
            org_id: "org-owner".to_owned(),
        };
        metadata.last_observation = Some(json!({ "url": "https://private.example" }));
        store_browser_run_metadata(&state, "run-owned", metadata).expect("store owned run");

        let attacker = test_user("user-attacker", "org-owner");
        let headers = HeaderMap::new();
        let responses = vec![
            run_action(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Json(ActionBody {
                    action: json!({ "type": "wait", "ms": 10 }),
                    actor: BrowserActionActor::Human,
                }),
            )
            .await,
            set_control_mode(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Json(ControlBody {
                    mode: BrowserControlMode::HumanTakeover,
                }),
            )
            .await,
            get_tabs(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
            )
            .await,
            new_tab(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Json(NewTabBody { url: None }),
            )
            .await,
            tab_mutation(
                state.clone(),
                attacker.clone(),
                headers.clone(),
                "run-owned".to_owned(),
                Some("tab-owned".to_owned()),
                Method::POST,
                "select",
                None,
            )
            .await,
            close_session(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
            )
            .await,
            suggest_action(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Json(SuggestActionBody {
                    goal: "private goal".to_owned(),
                    include_screenshot: false,
                }),
            )
            .await,
            start_ai_run(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Json(StartAiRunBody {
                    goal: "private goal".to_owned(),
                    allowed_domains: None,
                    max_steps: None,
                    max_runtime_s: None,
                    stop_criteria: None,
                    require_approval: None,
                    max_cost_usd: None,
                }),
            )
            .await,
            control_ai_run(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Json(json!({ "action": "stop" })),
            )
            .await,
            get_artifact(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path(("run-owned".to_owned(), "art_owned".to_owned())),
            )
            .await,
            get_live_frame(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Query(FrameQuery {
                    format: None,
                    quality: None,
                    max_width: None,
                    max_height: None,
                }),
            )
            .await,
            stream_live_frames(
                State(state.clone()),
                Extension(attacker.clone()),
                headers.clone(),
                Path("run-owned".to_owned()),
                Uri::from_static("/frames/stream"),
            )
            .await,
            get_devtools_events(
                State(state),
                Extension(attacker),
                headers,
                Path("run-owned".to_owned()),
                Uri::from_static("/devtools"),
            )
            .await,
        ];

        assert_eq!(responses.len(), 13);
        for (index, response) in responses.iter().enumerate() {
            let expected = if index == 5 {
                // Session close is deliberately idempotent: Quarry hiding an
                // absent/foreign run as 404 still produces a safe close.
                StatusCode::OK
            } else if matches!(index, 8 | 9) {
                // Durable Model Plane runs independently require delegated
                // Model/Session credentials. The isolated test deliberately
                // has no issuer, so it fails closed before probing that plane.
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::NOT_FOUND
            };
            assert_eq!(response.status(), expected, "response index {index}");
        }
    }

    fn test_app_state() -> AppState {
        let cache = crate::cache::ResultCache::disabled();
        AppState {
            client: reqwest::Client::new(),
            streaming_client: reqwest::Client::new(),
            internal_api_key: "test-key".into(),
            enforcement_mode: "off".to_string(),
            auth_core_url: "http://127.0.0.1:1".into(),
            verevon_public_origin: "http://localhost:5173".into(),
            session_core_url: "http://127.0.0.1:1".into(),
            session_core_service_token: "0123456789abcdef0123456789abcdef".into(),
            user_core_service_token: "abcdef0123456789abcdef0123456789".into(),
            billing_core_url: "http://127.0.0.1:1".into(),
            billing_core_service_token: "billing-test-secret-at-least-32-bytes".into(),
            cost_core_url: "http://127.0.0.1:1".into(),
            org_core_url: "http://127.0.0.1:1".into(),
            org_core_service_token: "org-test-secret-at-least-32-bytes".into(),
            integration_core_url: "http://127.0.0.1:1".into(),
            audit_core_url: "http://127.0.0.1:1".into(),
            audit_core_service_token: "audit-test-secret-at-least-32-bytes".into(),
            insight_core_url: "http://127.0.0.1:1".into(),
            leads_core_url: "http://127.0.0.1:1".into(),
            shipping_core_url: "http://127.0.0.1:1".into(),
            user_core_url: "http://127.0.0.1:1".into(),
            application_convex_url: String::new(),
            application_convex_service_key: String::new(),
            graph_index_url: "http://127.0.0.1:1".into(),
            quarry_edge_url: "http://127.0.0.1:1".into(),
            model_recommend_url: "http://127.0.0.1:1".into(),
            model_gateway_url: "http://127.0.0.1:1".into(),
            model_gateway_dev_bearer: String::new(),
            inference_core_url: "http://127.0.0.1:1".into(),
            documents_api_url: "http://127.0.0.1:1".into(),
            retrieval_engine_url: "http://127.0.0.1:1".into(),
            wiki_store_url: "http://127.0.0.1:1".into(),
            embedding_engine_url: "http://127.0.0.1:1".into(),
            quickwit_adapter_url: "http://127.0.0.1:1".into(),
            finspo_core_url: "http://127.0.0.1:1".into(),
            imports_api_url: "http://127.0.0.1:1".into(),
            notification_core_url: "http://127.0.0.1:1".into(),
            notification_core_service_token: "notification-test-secret-at-least-32-bytes".into(),
            conversation_core_service_token: "conversation-test-secret-at-least-32-bytes".into(),
            information_core_url: "http://127.0.0.1:1".into(),
            conversation_core_url: "http://127.0.0.1:1".into(),
            social_core_url: "http://127.0.0.1:1".into(),
            searxng_url: "http://127.0.0.1:1".into(),
            autocomplete_core_url: "http://127.0.0.1:1".into(),
            autocomplete_token: String::new(),
            zammad_api_url: "http://127.0.0.1:1".into(),
            zammad_api_token: String::new(),
            remote_support_rendezvous_url: String::new(),
            remote_support_relay_url: String::new(),
            remote_support_server_public_key: String::new(),
            audience_token_cache: crate::audience_tokens::new_audience_token_cache(),
            browser_run_store: new_browser_run_store(),
            cache: cache.clone(),
            rate_limiter: crate::rate_limit::RateLimiter::from_cache(&cache),
            studio_store: crate::domains::studio::StudioStore::new(),
            allow_dev_actor_headers: true,
            allow_dev_auth_bypass: true,
        }
    }

    #[test]
    fn sanitize_action_normalizes_navigation_urls() {
        let action = sanitize_action(json!({ "type": "navigate", "url": "https://example.com/a" }))
            .expect("valid action");
        assert_eq!(action["url"], "https://example.com/a");
    }

    #[test]
    fn sanitize_action_blocks_private_navigation_targets() {
        let err = sanitize_action(json!({ "type": "navigate", "url": "http://127.0.0.1" }))
            .expect_err("private URL should be blocked");
        assert_eq!(err.0, "invalid_url");
    }

    #[test]
    fn sanitize_action_denies_raw_script_evaluation() {
        let err = sanitize_action(json!({ "type": "evaluate", "script": "document.cookie" }))
            .expect_err("evaluate should be denied");
        assert_eq!(err.0, "browser_action_denied");
    }

    #[test]
    fn sanitize_action_allows_forward_history_navigation() {
        let action = sanitize_action(json!({ "type": "forward" })).expect("valid action");
        assert_eq!(action["type"], "forward");
    }

    #[test]
    fn sanitize_action_allows_coordinate_takeover_actions() {
        let click = sanitize_action(json!({ "type": "click_point", "x": 120.5, "y": 300.0 }))
            .expect("valid click point");
        assert_eq!(click["type"], "click_point");

        let wheel = sanitize_action(json!({
            "type": "mouse_wheel",
            "x": 120.5,
            "y": 300.0,
            "delta_x": 0.0,
            "delta_y": 480.0
        }))
        .expect("valid wheel action");
        assert_eq!(wheel["type"], "mouse_wheel");
    }

    #[test]
    fn sanitize_action_rejects_unbounded_coordinate_takeover_actions() {
        let err = sanitize_action(json!({ "type": "click_point", "x": -1.0, "y": 300.0 }))
            .expect_err("negative coordinate should be rejected");
        assert_eq!(err.0, "invalid_browser_action");

        let err = sanitize_action(json!({
            "type": "mouse_wheel",
            "x": 120.5,
            "y": 300.0,
            "delta_x": 0.0,
            "delta_y": 50_000.0
        }))
        .expect_err("oversized wheel delta should be rejected");
        assert_eq!(err.0, "invalid_browser_action");
    }

    #[test]
    fn zdr_session_is_rejected_when_requesting_a_persistent_profile_by_id() {
        let scope = effective_profile_scope(Some("prof_123"), false, None);
        let err = reject_zdr_persistent_profile(true, scope)
            .expect("a ZDR session selecting a persistent profile id must be rejected");
        assert_eq!(err.0, "zdr_persistent_profile_forbidden");
    }

    #[test]
    fn zdr_session_is_rejected_when_requesting_persistence_by_flag() {
        let scope = effective_profile_scope(None, true, None);
        let err = reject_zdr_persistent_profile(true, scope)
            .expect("a ZDR session requesting persistent_profile must be rejected");
        assert_eq!(err.0, "zdr_persistent_profile_forbidden");
    }

    #[test]
    fn zdr_session_without_a_persistent_profile_request_is_allowed() {
        let scope = effective_profile_scope(None, false, None);
        assert!(reject_zdr_persistent_profile(true, scope).is_none());
    }

    #[test]
    fn non_zdr_session_may_use_a_persistent_profile() {
        let scope = effective_profile_scope(Some("prof_123"), true, None);
        assert!(reject_zdr_persistent_profile(false, scope).is_none());
    }

    // Phase 3 continuation — the guard now checks a real 4-value scope
    // instead of a coarse boolean, so it must reject every non-ephemeral
    // scope explicitly, not just the two the boolean used to infer.
    #[test]
    fn zdr_session_is_rejected_for_explicit_org_shared_scope() {
        let err = reject_zdr_persistent_profile(true, BrowserProfileScope::OrgShared)
            .expect("org_shared must be rejected under ZDR");
        assert_eq!(err.0, "zdr_persistent_profile_forbidden");
    }

    #[test]
    fn zdr_session_is_rejected_for_explicit_run_scoped_scope() {
        let err = reject_zdr_persistent_profile(true, BrowserProfileScope::RunScoped)
            .expect("run_scoped must be rejected under ZDR");
        assert_eq!(err.0, "zdr_persistent_profile_forbidden");
    }

    #[test]
    fn zdr_session_is_rejected_for_explicit_user_private_scope() {
        let err = reject_zdr_persistent_profile(true, BrowserProfileScope::UserPrivate)
            .expect("user_private must be rejected under ZDR");
        assert_eq!(err.0, "zdr_persistent_profile_forbidden");
    }

    #[test]
    fn zdr_session_is_allowed_for_explicit_ephemeral_scope() {
        assert!(reject_zdr_persistent_profile(true, BrowserProfileScope::Ephemeral).is_none());
    }

    #[test]
    fn non_zdr_session_may_use_any_scope() {
        for scope in [
            BrowserProfileScope::Ephemeral,
            BrowserProfileScope::UserPrivate,
            BrowserProfileScope::OrgShared,
            BrowserProfileScope::RunScoped,
        ] {
            assert!(reject_zdr_persistent_profile(false, scope).is_none());
        }
    }

    #[test]
    fn effective_profile_scope_prefers_explicit_scope_over_inference() {
        // Even though profile_id + persistent_profile would infer
        // user_private, an explicit caller-supplied scope must win.
        let scope =
            effective_profile_scope(Some("prof_1"), true, Some(BrowserProfileScope::OrgShared));
        assert_eq!(scope, BrowserProfileScope::OrgShared);
    }

    #[test]
    fn effective_profile_scope_infers_ephemeral_with_no_profile_signal() {
        assert_eq!(
            effective_profile_scope(None, false, None),
            BrowserProfileScope::Ephemeral
        );
    }

    #[test]
    fn effective_profile_scope_infers_user_private_from_legacy_boolean() {
        // Back-compat: an older client that only ever sent
        // `persistentProfile: true` (no `scope` field) must still resolve
        // to a non-ephemeral scope so the ZDR guard's behavior is
        // unchanged for it.
        assert_eq!(
            effective_profile_scope(None, true, None),
            BrowserProfileScope::UserPrivate
        );
    }

    // Regression: a client cannot launder a real persistent-profile
    // attachment past the ZDR guard by explicitly claiming
    // `scope: "ephemeral"` alongside a real `profileId`/`persistentProfile`
    // signal. Fixed alongside this test — previously `effective_profile_scope`
    // let any explicit `requested_scope` (including `ephemeral`)
    // unconditionally override the raw facts.
    #[test]
    fn effective_profile_scope_cannot_be_downgraded_to_ephemeral_by_explicit_claim_over_profile_id()
    {
        let scope = effective_profile_scope(
            Some("prof_real"),
            false,
            Some(BrowserProfileScope::Ephemeral),
        );
        assert_eq!(scope, BrowserProfileScope::UserPrivate);
    }

    #[test]
    fn effective_profile_scope_cannot_be_downgraded_to_ephemeral_by_explicit_claim_over_persistent_flag(
    ) {
        let scope = effective_profile_scope(None, true, Some(BrowserProfileScope::Ephemeral));
        assert_eq!(scope, BrowserProfileScope::UserPrivate);
    }

    #[test]
    fn zdr_session_is_rejected_despite_explicit_ephemeral_scope_claim_over_real_profile_id() {
        // The exact bypass payload the audit flagged:
        // {zdr: true, profileId: "<real>", scope: "ephemeral"}.
        let scope = effective_profile_scope(
            Some("prof_real"),
            false,
            Some(BrowserProfileScope::Ephemeral),
        );
        let err = reject_zdr_persistent_profile(true, scope)
            .expect("an explicit ephemeral scope claim must not bypass a real profile_id");
        assert_eq!(err.0, "zdr_persistent_profile_forbidden");
    }

    #[test]
    fn zdr_session_is_rejected_despite_explicit_ephemeral_scope_claim_over_persistent_flag() {
        let scope = effective_profile_scope(None, true, Some(BrowserProfileScope::Ephemeral));
        let err = reject_zdr_persistent_profile(true, scope).expect(
            "an explicit ephemeral scope claim must not bypass a real persistent_profile flag",
        );
        assert_eq!(err.0, "zdr_persistent_profile_forbidden");
    }

    #[test]
    fn build_ai_run_request_never_forwards_client_requested_require_approval() {
        // Phase 5 regression guard: the legacy blanket `require_approval`
        // gate in execution-core is confirmed broken (ends the run with no
        // resume path, releases the Quarry lease, zero actions taken) — the
        // gateway must never arm it on a caller's behalf, no matter what a
        // client requests. Real risk-based per-action gating is automatic
        // and does not depend on this flag at all.
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-1".to_owned()),
            profile_id: Some("prof_1".to_owned()),
            profile_scope: BrowserProfileScope::UserPrivate,
            last_observation: None,
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        let body = StartAiRunBody {
            goal: "check out the cart".to_owned(),
            allowed_domains: None,
            max_steps: Some(5),
            max_runtime_s: None,
            stop_criteria: None,
            require_approval: Some(true),
            max_cost_usd: None,
        };

        let request = build_ai_run_request("check out the cart", "sess_1", &metadata, &body, None);

        assert_eq!(request["require_approval"], json!(false));
        // Sanity: everything else still forwards as expected.
        assert_eq!(request["goal"], json!("check out the cart"));
        assert_eq!(request["grant_id"], json!("session:sess_1"));
        assert_eq!(request["profile_id"], json!("prof_1"));
        assert_eq!(request["max_steps"], json!(5));
        assert_eq!(request["zdr"], json!(false));
    }

    #[test]
    fn build_ai_run_request_forwards_zdr_and_start_url_from_server_state() {
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-1".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: None,
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: true,
        };
        let body = StartAiRunBody {
            goal: "find the price".to_owned(),
            allowed_domains: None,
            max_steps: None,
            max_runtime_s: None,
            stop_criteria: None,
            require_approval: None,
            max_cost_usd: None,
        };

        let request = build_ai_run_request(
            "find the price",
            "sess_2",
            &metadata,
            &body,
            Some("https://example.com"),
        );

        assert_eq!(request["zdr"], json!(true));
        assert_eq!(request["start_url"], json!("https://example.com"));
        assert_eq!(request["profile_id"], json!(null));
    }

    #[test]
    fn build_ai_run_request_never_forwards_an_ephemeral_scope_profile_id() {
        // Live-confirmed bug fix: Quarry assigns a `profile_id` to every
        // session lease, including ephemeral ones (the create-session
        // response reports `profile.scope: "ephemeral"` alongside a real
        // `prof_...` id even when the caller never requested persistence).
        // Forwarding that id unconditionally would make execution-core's
        // Phase 5 `persistent_cookie_use` gate fire on every AI run, not
        // just genuine persistent-profile reuse.
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-1".to_owned()),
            profile_id: Some("prof_ephemeral_lease".to_owned()),
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: None,
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        let body = StartAiRunBody {
            goal: "browse around".to_owned(),
            allowed_domains: None,
            max_steps: None,
            max_runtime_s: None,
            stop_criteria: None,
            require_approval: None,
            max_cost_usd: None,
        };

        let request = build_ai_run_request("browse around", "sess_3", &metadata, &body, None);

        assert_eq!(request["profile_id"], json!(null));
    }

    #[test]
    fn build_ai_run_request_forwards_a_genuinely_persistent_profile_id() {
        for scope in [
            BrowserProfileScope::UserPrivate,
            BrowserProfileScope::OrgShared,
            BrowserProfileScope::RunScoped,
        ] {
            let metadata = BrowserRunMetadata {
                owner: BrowserRunOwner::default(),
                lease_id: Some("lease-1".to_owned()),
                profile_id: Some("prof_real".to_owned()),
                profile_scope: scope,
                last_observation: None,
                observation_history: Vec::new(),
                devtools_events: Vec::new(),
                replay_events: Vec::new(),
                tabs: Vec::new(),
                viewport: DEFAULT_VIEWPORT,
                control_mode: BrowserControlMode::AgentControl,
                zdr: false,
            };
            let body = StartAiRunBody {
                goal: "resume shopping".to_owned(),
                allowed_domains: None,
                max_steps: None,
                max_runtime_s: None,
                stop_criteria: None,
                require_approval: None,
                max_cost_usd: None,
            };

            let request = build_ai_run_request("resume shopping", "sess_4", &metadata, &body, None);

            assert_eq!(
                request["profile_id"],
                json!("prof_real"),
                "scope {scope:?} should forward a genuinely persistent profile id"
            );
        }
    }

    #[test]
    fn browser_response_includes_scoped_screenshot_frame_url() {
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-1".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: None,
            observation_history: vec![json!({
                "step": 0,
                "url": "https://example.com",
                "title": "Example",
                "screenshot_artifact_id": "art_01JZ9XM7EXAMPLESHOT00001"
            })],
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: vec![json!({
                "tabId": "tab-1",
                "title": "Example",
                "url": "https://example.com",
                "active": true
            })],
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        let response = browser_response(
            "run_browser_01",
            &metadata,
            Some(json!({
                "run_id": "run_browser_01",
                "url": "https://example.com",
                "screenshot_artifact_id": "art_01JZ9XM7EXAMPLESHOT00001"
            })),
        );

        assert_eq!(
            response["session"]["frame"]["url"],
            "/api/v1/browser/sessions/run_browser_01/artifacts/art_01JZ9XM7EXAMPLESHOT00001"
        );
        assert_eq!(response["session"]["frame"]["mediaType"], "image/png");
        assert_eq!(
            response["session"]["timeline"][0]["screenshotUrl"],
            "/api/v1/browser/sessions/run_browser_01/artifacts/art_01JZ9XM7EXAMPLESHOT00001"
        );
        assert_eq!(
            response["session"]["liveFrameUrl"],
            "/api/v1/browser/sessions/run_browser_01/frame?format=jpeg&quality=65"
        );
        assert_eq!(
            response["session"]["liveFrameStreamUrl"],
            "/api/v1/browser/sessions/run_browser_01/frames/stream?format=jpeg&quality=65&intervalMs=250"
        );
        assert_eq!(
            response["session"]["liveFrameWsUrl"],
            "/api/v1/browser/sessions/run_browser_01/frames/ws?format=jpeg&quality=65&intervalMs=250"
        );
        assert_eq!(
            response["session"]["tabsUrl"],
            "/api/v1/browser/sessions/run_browser_01/tabs"
        );
        assert_eq!(
            response["session"]["devtoolsUrl"],
            "/api/v1/browser/sessions/run_browser_01/devtools"
        );
        assert_eq!(response["session"]["devtools"]["eventCount"], 0);
        assert_eq!(response["session"]["tabs"][0]["tabId"], "tab-1");
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "live_frame"));
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "live_frame_stream"));
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "live_frame_ws"));
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "devtools_events"));
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "tabs"));
        assert_eq!(response["session"]["control"]["mode"], "agent_control");
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "control_state"));
    }

    #[test]
    fn browser_response_includes_visual_observation_artifact_url() {
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: None,
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: None,
            observation_history: vec![json!({
                "step": 0,
                "url": "https://example.com",
                "visual_observation_artifact_id": "art_01JZ9XM7EXAMPLEVISION0001"
            })],
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        let response = browser_response(
            "run_browser_01",
            &metadata,
            Some(json!({
                "run_id": "run_browser_01",
                "url": "https://example.com",
                "visual_observation_artifact_id": "art_01JZ9XM7EXAMPLEVISION0001"
            })),
        );

        assert_eq!(
            response["session"]["visual"]["observationArtifactId"],
            "art_01JZ9XM7EXAMPLEVISION0001"
        );
        assert_eq!(
            response["session"]["visual"]["observationUrl"],
            "/api/v1/browser/sessions/run_browser_01/artifacts/art_01JZ9XM7EXAMPLEVISION0001"
        );
        assert_eq!(
            response["session"]["timeline"][0]["visualObservationUrl"],
            "/api/v1/browser/sessions/run_browser_01/artifacts/art_01JZ9XM7EXAMPLEVISION0001"
        );
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "visual_observation"));
    }

    #[test]
    fn browser_response_includes_replay_events() {
        let observation = json!({
            "step": 7,
            "url": "https://example.com/pricing",
            "title": "Pricing",
            "screenshot_artifact_id": "art_01JZ9XM7EXAMPLESHOT00001",
            "visual_observation_artifact_id": "art_01JZ9XM7EXAMPLEVISION0001",
            "console_summary": [{ "level": "info", "text": "ready" }],
            "network_summary": [{ "method": "GET", "status": 200, "url": "https://example.com/pricing" }],
            "policy_denials": [],
            "dom_summary": {
                "node_count": 88,
                "interactive_elements": [
                    { "tag": "button", "selector": "button.cta" },
                    { "tag": "a", "selector": "a[href=\"/demo\"]" }
                ]
            }
        });
        let replay_event = observation_replay_event(
            "run_browser_01",
            &observation,
            "agent",
            Some(&json!({ "type": "click_point", "x": 10.0, "y": 20.0 })),
            BrowserControlMode::AgentControl,
            false,
        );
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: None,
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(observation.clone()),
            observation_history: vec![observation.clone()],
            devtools_events: Vec::new(),
            replay_events: vec![replay_event],
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        let response = browser_response("run_browser_01", &metadata, Some(observation));
        let replay = &response["session"]["replay"];
        let event = &replay["events"][0];

        assert_eq!(replay["eventCount"], 1);
        assert_eq!(event["kind"], "observation");
        assert_eq!(event["actor"], "agent");
        assert_eq!(event["actionType"], "click_point");
        assert_eq!(event["controlMode"], "agent_control");
        assert_eq!(event["consoleCount"], 1);
        assert_eq!(event["networkCount"], 1);
        assert_eq!(event["domNodeCount"], 88);
        assert_eq!(event["domInteractiveCount"], 2);
        assert_eq!(
            event["screenshotUrl"],
            "/api/v1/browser/sessions/run_browser_01/artifacts/art_01JZ9XM7EXAMPLESHOT00001"
        );
        assert_eq!(
            event["visualObservationUrl"],
            "/api/v1/browser/sessions/run_browser_01/artifacts/art_01JZ9XM7EXAMPLEVISION0001"
        );
        assert!(response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array")
            .iter()
            .any(|capability| capability == "replay_timeline"));
    }

    #[test]
    fn websocket_human_action_updates_control_replay() {
        let state = test_app_state();
        let session_id = "run_ws_01";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: None,
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 0,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let mut pending_action = None;
        let mut pending_actor = String::new();
        let prepared = prepare_client_ws_message(
            AxumWsMessage::Text(
                json!({
                    "type": "action",
                    "action": { "type": "click_point", "x": 1, "y": 2 }
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect("ws action accepted");

        assert!(matches!(
            prepared,
            PreparedClientWsMessage::Upstream(AxumWsMessage::Text(_))
        ));
        let metadata = browser_run_metadata(&state, session_id);
        assert_eq!(metadata.control_mode, BrowserControlMode::HumanTakeover);
        assert_eq!(pending_actor, "human");
        assert_eq!(
            pending_action.expect("pending action")["type"],
            "click_point"
        );
        assert_eq!(metadata.replay_events[0]["kind"], "control");
        assert_eq!(metadata.replay_events[0]["controlMode"], "human_takeover");
    }

    #[test]
    fn websocket_agent_action_is_rejected_during_human_takeover() {
        let state = test_app_state();
        let session_id = "run_ws_agent_blocked";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: None,
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 0,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::HumanTakeover,
            zdr: false,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let mut pending_action = None;
        let mut pending_actor = String::new();
        let err = prepare_client_ws_message(
            AxumWsMessage::Text(
                json!({
                    "type": "action",
                    "actor": "agent",
                    "action": { "type": "click_point", "x": 1, "y": 2 }
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect_err("agent action should be rejected");

        assert_eq!(err.code, "browser_human_takeover_active");
        assert_eq!(
            err.message,
            "Human takeover is active for this browser session."
        );
        assert!(pending_action.is_none());
        assert!(pending_actor.is_empty());
    }

    #[test]
    fn websocket_control_release_updates_state_and_returns_session() {
        let state = test_app_state();
        let session_id = "run_ws_release";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-release".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 3,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::HumanTakeover,
            zdr: false,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let mut pending_action = None;
        let mut pending_actor = String::new();
        let prepared = prepare_client_ws_message(
            AxumWsMessage::Text(
                json!({
                    "type": "control",
                    "mode": "agent_control",
                    "actor": "human"
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect("control release accepted");
        let PreparedClientWsMessage::Client(AxumWsMessage::Text(text)) = prepared else {
            panic!("expected local client control message");
        };
        let payload: Value = serde_json::from_str(&text).expect("valid control payload");
        let metadata = browser_run_metadata(&state, session_id);

        assert_eq!(payload["type"], "control");
        assert_eq!(payload["control"]["mode"], "agent_control");
        assert_eq!(payload["session"]["control"]["mode"], "agent_control");
        assert_eq!(payload["session"]["replay"]["eventCount"], 1);
        assert_eq!(payload["session"]["replay"]["events"][0]["kind"], "control");
        assert_eq!(
            payload["session"]["replay"]["events"][0]["controlMode"],
            "agent_control"
        );
        assert_eq!(metadata.control_mode, BrowserControlMode::AgentControl);
        assert!(pending_action.is_none());
        assert!(pending_actor.is_empty());

        let prepared = prepare_client_ws_message(
            AxumWsMessage::Text(
                json!({
                    "type": "action",
                    "actor": "agent",
                    "action": { "type": "wait", "ms": 10 }
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect("agent action accepted after release");

        assert!(matches!(
            prepared,
            PreparedClientWsMessage::Upstream(AxumWsMessage::Text(_))
        ));
        assert_eq!(pending_actor, "agent");
        assert_eq!(pending_action.expect("pending action")["type"], "wait");
    }

    #[test]
    fn websocket_observation_updates_replay_and_injects_session() {
        let state = test_app_state();
        let session_id = "run_ws_02";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-1".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 0,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::HumanTakeover,
            zdr: false,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let mut pending_action =
            Some(json!({ "type": "mouse_wheel", "x": 4, "y": 8, "delta_y": 120 }));
        let mut pending_actor = "human".to_owned();
        let message = process_upstream_ws_message(
            TungsteniteMessage::Text(
                json!({
                    "type": "observation",
                    "observation": {
                        "run_id": session_id,
                        "step": 1,
                        "url": "https://example.com/news",
                        "title": "News",
                        "screenshot_artifact_id": "art_01JZ9XM7EXAMPLESHOT00001"
                    }
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect("client message");
        let AxumWsMessage::Text(text) = message else {
            panic!("expected text message");
        };
        let payload: Value = serde_json::from_str(&text).expect("valid json");
        let metadata = browser_run_metadata(&state, session_id);

        assert_eq!(payload["session"]["control"]["mode"], "human_takeover");
        assert_eq!(payload["session"]["replay"]["eventCount"], 1);
        assert_eq!(
            payload["session"]["replay"]["events"][0]["screenshotUrl"],
            "/api/v1/browser/sessions/run_ws_02/artifacts/art_01JZ9XM7EXAMPLESHOT00001"
        );
        assert_eq!(metadata.last_observation.as_ref().unwrap()["step"], 1);
        assert_eq!(metadata.replay_events[0]["actionType"], "mouse_wheel");
        assert!(pending_action.is_none());
        assert_eq!(pending_actor, "human");
    }

    #[test]
    fn websocket_devtools_updates_session_and_replay() {
        let state = test_app_state();
        let session_id = "run_ws_devtools";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-devtools".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 0,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let mut pending_action = None;
        let mut pending_actor = String::new();
        let message = process_upstream_ws_message(
            TungsteniteMessage::Text(
                json!({
                    "type": "devtools",
                    "events": [
                        {
                            "sequence": 4,
                            "tabId": "tab-1",
                            "category": "network",
                            "name": "Network.responseReceived",
                            "method": "GET",
                            "url": "https://example.com/",
                            "status": 200,
                            "timestampMs": 1234,
                            "payload": {}
                        }
                    ],
                    "zdr": false
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect("client message");
        let AxumWsMessage::Text(text) = message else {
            panic!("expected text message");
        };
        let payload: Value = serde_json::from_str(&text).expect("valid json");
        let metadata = browser_run_metadata(&state, session_id);

        assert_eq!(payload["session"]["devtools"]["eventCount"], 1);
        assert_eq!(payload["session"]["devtools"]["lastSequence"], 4);
        assert_eq!(payload["session"]["replay"]["eventCount"], 1);
        assert_eq!(
            payload["session"]["replay"]["events"][0]["kind"],
            "devtools"
        );
        assert_eq!(payload["session"]["replay"]["events"][0]["lastSequence"], 4);
        assert_eq!(metadata.devtools_events.len(), 1);
        assert_eq!(metadata.replay_events[0]["categories"][0], "network");
        assert!(pending_action.is_none());
        assert!(pending_actor.is_empty());
    }

    #[test]
    fn websocket_frame_adds_transient_replay_without_image_payload() {
        let state = test_app_state();
        let session_id = "run_ws_frame";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-frame".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 0,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: false,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let mut pending_action = None;
        let mut pending_actor = String::new();
        let message = process_upstream_ws_message(
            TungsteniteMessage::Text(
                json!({
                    "type": "frame",
                    "sequence": 1,
                    "mimeType": "image/jpeg",
                    "dataBase64": "abcdef",
                    "zdr": false
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect("client message");
        let AxumWsMessage::Text(text) = message else {
            panic!("expected text message");
        };
        let payload: Value = serde_json::from_str(&text).expect("valid json");
        let metadata = browser_run_metadata(&state, session_id);
        let replay = &payload["session"]["replay"]["events"][0];

        assert_eq!(payload["dataBase64"], "abcdef");
        assert_eq!(replay["kind"], "frame");
        assert_eq!(replay["sequence"], 1);
        assert_eq!(replay["persisted"], false);
        assert_eq!(replay["imagePayloadPersisted"], false);
        assert!(replay.get("dataBase64").is_none());
        assert_eq!(metadata.replay_events[0]["dataBase64Length"], 6);
        assert!(pending_action.is_none());
        assert!(pending_actor.is_empty());
    }

    #[test]
    fn websocket_frame_replay_is_sampled_bounded_and_payload_free() {
        let state = test_app_state();
        let session_id = "run_ws_frame_stress";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-frame-stress".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 0,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::AgentControl,
            zdr: true,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let mut pending_action = None;
        let mut pending_actor = String::new();
        let final_sequence = (MAX_BROWSER_REPLAY_EVENTS as u64 + 4) * 10 + 1;
        for sequence in 1..=final_sequence {
            let message = process_upstream_ws_message(
                TungsteniteMessage::Text(
                    json!({
                        "type": "frame",
                        "sequence": sequence,
                        "mimeType": "image/jpeg",
                        "dataBase64": format!("payload-{sequence}"),
                        "zdr": true
                    })
                    .to_string()
                    .into(),
                ),
                &state,
                session_id,
                &mut pending_action,
                &mut pending_actor,
            )
            .expect("frame message");
            assert!(matches!(message, AxumWsMessage::Text(_)));
        }

        let metadata = browser_run_metadata(&state, session_id);
        assert_eq!(metadata.replay_events.len(), MAX_BROWSER_REPLAY_EVENTS);
        assert_eq!(metadata.replay_events[0]["sequence"], 51);
        assert_eq!(
            metadata.replay_events.last().expect("last replay event")["sequence"],
            final_sequence
        );
        assert!(metadata.replay_events.iter().all(|event| {
            event.get("kind").and_then(Value::as_str) == Some("frame")
                && event.get("persisted").and_then(Value::as_bool) == Some(false)
                && event.get("imagePayloadPersisted").and_then(Value::as_bool) == Some(false)
                && event.get("dataBase64").is_none()
                && event
                    .get("dataBase64Length")
                    .and_then(Value::as_u64)
                    .is_some()
                && event.get("zdr").and_then(Value::as_bool) == Some(true)
        }));
        assert!(pending_action.is_none());
        assert!(pending_actor.is_empty());
    }

    #[test]
    fn websocket_devtools_events_are_deduped_sorted_and_bounded() {
        let state = test_app_state();
        let session_id = "run_ws_devtools_stress";
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: Some("lease-devtools-stress".to_owned()),
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: Some(json!({
                "step": 0,
                "url": "https://example.com"
            })),
            observation_history: Vec::new(),
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::HumanTakeover,
            zdr: false,
        };
        state
            .browser_run_store
            .lock()
            .expect("browser store")
            .insert(session_id.to_owned(), metadata);

        let last_sequence = MAX_BROWSER_DEVTOOLS_EVENTS as u64 + 7;
        let mut events = (0..=last_sequence)
            .map(|sequence| {
                json!({
                    "sequence": sequence,
                    "tabId": "tab-1",
                    "category": if sequence % 2 == 0 { "network" } else { "console" },
                    "name": "Network.responseReceived",
                    "timestampMs": sequence * 10,
                    "payload": { "sequence": sequence }
                })
            })
            .collect::<Vec<_>>();
        events.push(json!({
            "sequence": last_sequence,
            "tabId": "tab-1",
            "category": "console",
            "name": "Runtime.consoleAPICalled",
            "timestampMs": 99_999,
            "payload": { "deduped": true }
        }));

        let mut pending_action = None;
        let mut pending_actor = String::new();
        let message = process_upstream_ws_message(
            TungsteniteMessage::Text(
                json!({
                    "type": "devtools",
                    "events": events,
                    "zdr": false
                })
                .to_string()
                .into(),
            ),
            &state,
            session_id,
            &mut pending_action,
            &mut pending_actor,
        )
        .expect("devtools message");
        let AxumWsMessage::Text(text) = message else {
            panic!("expected text message");
        };
        let payload: Value = serde_json::from_str(&text).expect("valid json");
        let metadata = browser_run_metadata(&state, session_id);

        assert_eq!(payload["session"]["devtools"]["eventCount"], 512);
        assert_eq!(
            payload["session"]["devtools"]["lastSequence"],
            last_sequence
        );
        assert_eq!(metadata.devtools_events.len(), MAX_BROWSER_DEVTOOLS_EVENTS);
        assert_eq!(metadata.devtools_events[0]["sequence"], 8);
        assert_eq!(
            metadata
                .devtools_events
                .last()
                .expect("last devtools event")["name"],
            "Runtime.consoleAPICalled"
        );
        assert_eq!(metadata.replay_events.len(), 1);
        assert_eq!(metadata.replay_events[0]["kind"], "devtools");
        assert_eq!(metadata.replay_events[0]["eventCount"], 521);
        assert_eq!(metadata.replay_events[0]["controlMode"], "human_takeover");
        assert_eq!(
            metadata.replay_events[0]["categories"]
                .as_array()
                .expect("categories"),
            &vec![json!("console"), json!("network")]
        );
        assert!(pending_action.is_none());
        assert!(pending_actor.is_empty());
    }

    #[test]
    fn upstream_ws_url_converts_http_origin_to_ws() {
        let url = upstream_ws_url(
            "http://quarry-edge:8082",
            "/v1/agent/runs/run_1/frames/ws?format=jpeg",
        )
        .expect("ws url");
        assert_eq!(
            url,
            "ws://quarry-edge:8082/v1/agent/runs/run_1/frames/ws?format=jpeg"
        );
    }

    #[test]
    fn browser_timeline_entries_carry_bounded_step_evidence() {
        let console: Vec<Value> = (0..30)
            .map(|index| json!({ "level": "info", "text": format!("line {index}") }))
            .collect();
        let metadata = BrowserRunMetadata {
            owner: BrowserRunOwner::default(),
            lease_id: None,
            profile_id: None,
            profile_scope: BrowserProfileScope::Ephemeral,
            last_observation: None,
            observation_history: vec![json!({
                "step": 3,
                "url": "https://example.com/kontakt",
                "title": "Kontakt",
                "console_summary": console,
                "network_summary": [
                    { "method": "GET", "status": 200, "url": "https://example.com/kontakt" }
                ],
                "policy_denials": ["Blocked navigation to unknown.example"],
                "dom_summary": {
                    "node_count": 42,
                    "interactive_elements": [
                        { "tag": "a", "selector": "a[href=\"/\"]" }
                    ]
                }
            })],
            devtools_events: Vec::new(),
            replay_events: Vec::new(),
            tabs: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
            control_mode: BrowserControlMode::HumanTakeover,
            zdr: false,
        };
        let response = browser_response("run_browser_01", &metadata, None);
        let entry = &response["session"]["timeline"][0];

        assert_eq!(
            entry["consoleSummary"].as_array().expect("console").len(),
            MAX_TIMELINE_CONSOLE_ENTRIES
        );
        assert_eq!(entry["networkSummary"][0]["status"], 200);
        assert_eq!(
            entry["policyDenials"][0],
            "Blocked navigation to unknown.example"
        );
        assert_eq!(entry["domNodeCount"], 42);
        assert_eq!(entry["domInteractiveCount"], 1);
        assert_eq!(response["session"]["zdr"], false);
        assert_eq!(response["session"]["control"]["mode"], "human_takeover");
    }

    #[test]
    fn artifact_ids_are_strict_path_segments() {
        assert!(is_valid_artifact_id("art_01JZ9XM7EXAMPLESHOT00001"));
        assert!(!is_valid_artifact_id("file_01JZ9XM7EXAMPLESHOT00001"));
        assert!(!is_valid_artifact_id("art_../secret"));
        assert!(!is_valid_artifact_id("art_"));
    }

    #[test]
    fn profile_ids_are_strict_path_segments() {
        assert!(is_valid_profile_id("prof_01JZ9XM7EXAMPLEPROFILE0001"));
        assert!(!is_valid_profile_id("art_01JZ9XM7EXAMPLESHOT00001"));
        assert!(!is_valid_profile_id("prof_../secret"));
        assert!(!is_valid_profile_id("prof_"));
    }

    #[test]
    fn dev_quarry_token_requires_gateway_dev_bypass() {
        assert_eq!(dev_quarry_token_for(false), None);
        assert_eq!(dev_quarry_token_for(true), Some("dev-bypass".to_owned()));
    }

    #[test]
    fn only_safe_browser_artifact_content_types_are_forwarded() {
        assert_eq!(
            safe_browser_artifact_content_type(Some("image/webp; charset=binary"), b""),
            "image/webp"
        );
        assert_eq!(
            safe_browser_artifact_content_type(Some("application/json"), b"{}"),
            "application/json"
        );
        assert_eq!(
            safe_browser_artifact_content_type(
                Some("application/octet-stream"),
                b" {\"ok\": true}"
            ),
            "application/json"
        );
        assert_eq!(
            safe_browser_artifact_content_type(Some("text/html"), b"<html></html>"),
            "image/png"
        );
    }

    #[test]
    fn model_screenshot_content_types_are_image_only() {
        assert_eq!(
            safe_model_screenshot_content_type(Some("image/jpeg; charset=binary")),
            "image/jpeg"
        );
        assert_eq!(
            safe_model_screenshot_content_type(Some("image/webp")),
            "image/webp"
        );
        assert_eq!(
            safe_model_screenshot_content_type(Some("application/json")),
            "image/png"
        );
    }

    #[test]
    fn screenshot_artifact_id_uses_strict_artifact_ids() {
        assert_eq!(
            screenshot_artifact_id(&json!({
                "screenshot_artifact_id": "art_01JZ9XM7EXAMPLESHOT00001"
            })),
            Some("art_01JZ9XM7EXAMPLESHOT00001".to_owned())
        );
        assert_eq!(
            screenshot_artifact_id(&json!({
                "screenshot_artifact_id": "../secret"
            })),
            None
        );
    }

    #[test]
    fn visual_observation_artifact_id_uses_strict_artifact_ids() {
        assert_eq!(
            visual_observation_artifact_id(&json!({
                "visual_observation_artifact_id": "art_01JZ9XM7EXAMPLEVISION0001"
            })),
            Some("art_01JZ9XM7EXAMPLEVISION0001".to_owned())
        );
        assert_eq!(
            visual_observation_artifact_id(&json!({
                "visual_observation_artifact_id": "https://example.com/art.json"
            })),
            None
        );
    }
}
