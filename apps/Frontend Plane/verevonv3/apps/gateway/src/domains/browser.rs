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
    audience_tokens::{
        get_audience_token, get_onboarding_preview_token, require_model_service_token,
        ModelServiceAudience, RequiredAudienceTokenError,
    },
    config::{AppState, BrowserGrantMode},
    domains::chat::shared::{
        data_plane_authorization_value, data_plane_token, delegated_auth_unavailable, model_token,
        proxy_model_json, proxy_model_json_with_data_plane, proxy_model_json_with_session,
        required_execution_token, required_inference_token, required_session_token,
    },
    envelope::{error, ok, unwrap_data},
    middleware::{require_session, AuthenticatedUser},
    public_url::normalize_public_http_url,
    rate_limit::{with_rate_class, RateClass},
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

/// Who is on the other end of a browser call, as the gateway verified it (D2-012). It is
/// a request extension the authentication layer inserts, never a field of the body.
///
/// Decided 2026-10-28 (MP-GATEWAY-AUTH): the Model Plane gets no credential to this
/// gateway. The gateway authenticates only a person's Better Auth session, and the Model
/// Plane never calls `/api/v1/browser`: it drives Quarry directly with a `browser:execute`
/// service token, and Quarry decides who the agent is from that verified token
/// (`derive_initiator`: a service principal is always the agent, a person's token is the
/// agent only when it says so). So nothing here produces `ServicePrincipal`; every call
/// through this gateway is `User`, and the rules below are the gateway's own early
/// refusals for an agent acting through a person's session (the SPA labels a model's
/// action `actor: agent`). Each of them is also a Quarry check: reload is refused there
/// (`agent_reload_denied`), and an agent action during a takeover gets `USER_TOOK_OVER`,
/// but only while `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` is on (off by default), so until it
/// is switched on the gateway is the only refusing guard for takeover. Reopen
/// D2-012 if a Model Plane to gateway browser path is ever planned; the class and
/// `effective_actor` stay so that path has a place to land.
///
/// Accepted residual: a person's session can still state `human`, and no gateway design
/// can tell a model acting through that session from the person.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CallerClass {
    /// A person's session. It cannot prove it is not a model acting through the
    /// person's own session, so `body.actor` stays advisory and can only tighten.
    #[default]
    User,
    /// A verified service principal. Always the agent. Not produced today (see the type
    /// docs: the Model Plane goes to Quarry, not through this gateway).
    // reason: no authentication layer produces this class, by decision (D2-012,
    // MP-GATEWAY-AUTH); only the tests construct it, to pin the rule a producer would get.
    #[allow(dead_code)]
    ServicePrincipal,
}

/// The actor a browser call acts as. A service principal is always the agent, whatever
/// its body or query says; under a user session the stated actor is taken, which can
/// only tighten (a stated `agent` is refused during takeover and cannot reload).
fn effective_actor(
    caller: Option<Extension<CallerClass>>,
    stated: BrowserActionActor,
) -> BrowserActionActor {
    match caller.map(|Extension(class)| class).unwrap_or_default() {
        CallerClass::ServicePrincipal => BrowserActionActor::Agent,
        CallerClass::User => stated,
    }
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
#[serde(rename_all = "camelCase")]
pub(crate) struct ControlBody {
    pub(crate) mode: BrowserControlMode,
    /// WS9.I: the control epoch the person saw. Quarry refuses the change with 409 when the
    /// run has moved on, so a stale Take over or Give back cannot flip control.
    #[serde(default)]
    pub(crate) expected_epoch: Option<u64>,
}

/// The body the gateway sends Quarry for a control change (WS9.I). `expected_epoch` is
/// forwarded only when the caller sent one; without it the change is unconditional, as before.
fn control_quarry_body(mode: BrowserControlMode, expected_epoch: Option<u64>) -> Value {
    let mut body = json!({ "mode": mode.as_str() });
    if let Some(expected) = expected_epoch {
        body["expected_epoch"] = json!(expected);
    }
    body
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewTabBody {
    #[serde(default)]
    pub(crate) url: Option<String>,
    /// Who is opening the tab. `agent` is what a model's call carries; Quarry
    /// refuses it while the person has taken over. Like `ActionBody.actor` it can
    /// only tighten, and it is never forwarded as anything but the fixed hint.
    #[serde(default)]
    pub(crate) actor: BrowserActionActor,
}

/// The query of a tab select or close: who is asking (`?actor=agent`). Absent is
/// the person. An unknown value is a 400 here, not read as the person's.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct TabActorQuery {
    #[serde(default)]
    pub(crate) actor: BrowserActionActor,
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
pub(super) const MAX_BROWSER_ARTIFACT_BYTES: u64 = 24 * 1024 * 1024;
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
    classed_routes(&state.rate_limiter)
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Every browser route with its rate class applied, before the session layer.
/// Split out so the route-to-class assignment is testable without a session.
fn classed_routes(limiter: &crate::rate_limit::RateLimiter) -> Router<AppState> {
    // Two rate classes, each its own per-organization bucket (see
    // `rate_limit::RateClass`): starting a session or an AI run is scarce and
    // expensive; everything done inside an open session is not, and must keep
    // working while an organization is out of start budget.
    let browser_start = Router::new()
        .route("/api/v1/browser/sessions", post(create_session))
        // Phase 2: start a durable, server-side, multi-step browser-agent run.
        // Progress streams over the existing chat run-event route
        // (`GET /api/v1/runs/:run_id/events`), unchanged by this facade.
        .route(
            "/api/v1/browser/sessions/{session_id}/ai-runs",
            post(start_ai_run),
        );
    let interactive = Router::new()
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
        // WS8.D: extends a human session's browser grant by one broker TTL.
        .route(
            "/api/v1/browser/sessions/{session_id}/keepalive",
            post(keepalive_session),
        )
        // Pause / resume / stop a durable browser-agent run.
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
        // WS10.S3: the run's typed events as SSE, resumable with Last-Event-ID.
        .route(
            "/api/v1/browser/sessions/{session_id}/events/stream",
            get(stream_agent_events),
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
        );
    Router::new()
        .merge(with_rate_class(
            browser_start,
            limiter,
            RateClass::BrowserStart,
        ))
        .merge(with_rate_class(
            interactive,
            limiter,
            RateClass::Interactive,
        ))
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
    // WS8.D: a person's open-web session asks for a human_browse grant (no hosts) when the
    // switch is on and grants are issued at all. Otherwise the start-host session grant.
    let human_scope = state.browser_human_browse && state.browser_grant_mode.issues_grants();
    let grant_scope = if human_scope {
        SessionGrantScope::HumanBrowse
    } else {
        SessionGrantScope::StartHost(allowed_domain.as_deref())
    };
    // A BrowserBroker session grant, before Quarry is asked for a lease: in strict
    // mode no grant means no session.
    let grant_id = match issue_session_grant(&state, &user, &headers, grant_scope, zdr).await {
        Ok(grant_id) => grant_id,
        Err(response) => return response,
    };
    // Without a human_browse grant (switch off, or a permissive issuance failure) the
    // first-host policy applies, as before WS8.D.
    let human_grant = human_scope && grant_id.is_some();
    let mut start_body = json!({
        "viewport": {
            "width": viewport.width,
            "height": viewport.height
        },
        "zdr": zdr
    });
    if let Some(constraints) = session_start_constraints(human_grant, allowed_domain.as_deref()) {
        start_body["constraints"] = constraints;
    }
    start_body["profile_scope"] = Value::String(profile_scope.as_str().to_owned());
    if let Some(profile_id) = profile_id {
        start_body["profile_id"] = Value::String(profile_id.to_owned());
    }
    if requested_persistent_profile {
        start_body["persist_profile"] = Value::Bool(true);
    }
    if let Some(grant_id) = grant_id.as_deref() {
        start_body["grant_id"] = Value::String(grant_id.to_owned());
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
        if let Some(grant_id) = grant_id.as_deref() {
            revoke_session_grant(&state, &user, &headers, grant_id).await;
        }
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
        if let Some(grant_id) = grant_id.as_deref() {
            revoke_session_grant(&state, &user, &headers, grant_id).await;
        }
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

/// `POST /api/v1/browser/sessions/:session_id/keepalive` (WS8.D): extends the session's
/// browser grant by one broker TTL, never past its absolute cap. The owner gate is
/// Quarry's owner projection, which is where the grant id is read; the id is never sent
/// back. The answer carries the new expiry only. A grant that cannot be renewed (ended,
/// revoked, at its cap, or not this person's) is 409 `browser_session_expired`.
pub(crate) async fn keepalive_session(
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
    let projection =
        match owner_browser_session_projection(&state, &user, token.as_deref(), &session_id).await
        {
            Ok(projection) => projection,
            Err(response) => return response,
        };
    let Some(grant_id) = projection_grant_id(&projection) else {
        return keepalive_refusal(StatusCode::CONFLICT, "browser_session_expired");
    };
    let (model, browser) = tokio::join!(
        model_token(&state, &user, &headers),
        browser_broker_token(&state, &user, &headers),
    );
    let browser = match browser {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };
    let url = format!(
        "{}/v1/browser/grants/{}/renew",
        state.model_gateway_url,
        urlencoding::encode(&grant_id)
    );
    let (status, Json(body)) = proxy_model_json_with_delegated(
        &state,
        Method::POST,
        &url,
        None,
        model.as_deref(),
        &[("x-browser-authorization", browser.as_str())],
        &user,
    )
    .await;
    match keepalive_outcome(status, &body) {
        Ok(payload) => (StatusCode::OK, Json(ok(payload))).into_response(),
        Err((status, code)) => keepalive_refusal(status, code),
    }
}

/// The keep-alive answer for a renew response: the expiry, or the refusal's status and
/// code. Model Gateway does not say why a renewal failed (expired, revoked, capped or
/// not the caller's), so every 403, 404 and 409 is the same ended session for the SPA.
/// Anything else is an upstream problem the person cannot act on.
fn keepalive_outcome(
    status: StatusCode,
    body: &Value,
) -> Result<Value, (StatusCode, &'static str)> {
    if status.is_success() {
        return Ok(keepalive_body(&unwrap_data(body)));
    }
    if matches!(
        status,
        StatusCode::CONFLICT | StatusCode::NOT_FOUND | StatusCode::FORBIDDEN
    ) {
        return Err((StatusCode::CONFLICT, "browser_session_expired"));
    }
    Err((StatusCode::SERVICE_UNAVAILABLE, "browser_grant_unavailable"))
}

/// The only field a keep-alive answer carries. The grant id and the absolute cap stay
/// inside the BFF.
fn keepalive_body(renewed: &Value) -> Value {
    json!({
        "expiresAt": renewed.get("expires_at").cloned().unwrap_or(Value::Null)
    })
}

fn keepalive_refusal(status: StatusCode, code: &'static str) -> Response {
    let message = if code == "browser_session_expired" {
        "This browser session has ended. Reopen it to keep browsing."
    } else {
        "Browser access is temporarily unavailable."
    };
    (status, Json(error(code, message))).into_response()
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
        // The session grant id is internal to the BFF: strip it here, the one
        // route that forwards Quarry's projection rather than mapping it.
        Ok(projection) => (StatusCode::OK, Json(ok(spa_owner_projection(projection)))).into_response(),
        Err(response) => response,
    }
}

/// Read the sole owner projection for a direct Quarry browser run. All callers
/// use this after an owner-authorized mutation instead of rebuilding session
/// state from BFF-local observations, tabs, or control flags.
pub(super) async fn owner_browser_session_projection(
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
    caller: Option<Extension<CallerClass>>,
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
    let actor = effective_actor(caller, body.actor);
    if reload_refused_for_actor(&action, actor) {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "browser_action_denied",
                "Reload is available to people, not to agents.",
            )),
        )
            .into_response();
    }
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
        Some(step_request_body(
            action,
            actor,
            &AgentAdvisory::from_headers(&headers).for_actor(actor),
        )),
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
    let expected_epoch = body.expected_epoch;
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        &format!(
            "/v1/agent/runs/{}/browser-session/control",
            urlencoding::encode(&session_id),
        ),
        Some(control_quarry_body(mode, expected_epoch)),
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
        Json(ok(json!({
            "tabs": tabs_with_opened_by(tabs, &projection),
            "session": response["session"]
        }))),
    )
        .into_response()
}

pub(crate) async fn new_tab(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    caller: Option<Extension<CallerClass>>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<NewTabBody>,
) -> Response {
    let actor = effective_actor(caller, body.actor);
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
    if let Some(refusal) =
        refuse_agent_tab_call_during_takeover(&state, &user, token.as_deref(), &session_id, actor)
            .await
    {
        return refusal;
    }
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        &format!("/v1/agent/runs/{}/tabs", urlencoding::encode(&session_id)),
        Some(new_tab_request_body(
            url,
            actor,
            &AgentAdvisory::from_headers(&headers).for_actor(actor),
        )),
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
            "tab": data
                .get("tab")
                .cloned()
                .map_or(Value::Null, |tab| tab_with_opened_by(tab, &projection)),
            "tabs": tabs_with_opened_by(tabs, &projection),
            "session": response["session"]
        }))),
    )
        .into_response()
}

pub(crate) async fn select_tab(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    caller: Option<Extension<CallerClass>>,
    headers: HeaderMap,
    Path((session_id, tab_id)): Path<(String, String)>,
    Query(call): Query<TabActorQuery>,
) -> Response {
    tab_mutation(
        state,
        user,
        headers,
        session_id,
        Some(tab_id),
        Method::POST,
        "select",
        effective_actor(caller, call.actor),
        None,
    )
    .await
}

pub(crate) async fn close_tab(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    caller: Option<Extension<CallerClass>>,
    headers: HeaderMap,
    Path((session_id, tab_id)): Path<(String, String)>,
    Query(call): Query<TabActorQuery>,
) -> Response {
    tab_mutation(
        state,
        user,
        headers,
        session_id,
        Some(tab_id),
        Method::DELETE,
        "close",
        effective_actor(caller, call.actor),
        None,
    )
    .await
}

/// The body of a tab-open call. An open the model proposed carries
/// `initiatedBy: "agent"`, so Quarry can refuse it while the person has taken over;
/// a person's own carries no hint. Only the enum is forwarded, never a client string.
fn new_tab_request_body(
    url: Option<String>,
    actor: BrowserActionActor,
    advisory: &AgentAdvisory,
) -> Value {
    match actor {
        BrowserActionActor::Agent => {
            let mut body = json!({ "url": url, "initiatedBy": "agent" });
            advisory.put_in(&mut body);
            body
        }
        BrowserActionActor::Human => json!({ "url": url }),
    }
}

/// The query suffix of a tab select or close (neither has a body): the same hint as
/// `new_tab_request_body`, for the same reason.
fn tab_call_query(actor: BrowserActionActor, advisory: &AgentAdvisory) -> String {
    match actor {
        // The advisory values are identifier-shaped (see `AgentAdvisory`), so they
        // need no percent-encoding.
        BrowserActionActor::Agent => {
            let mut query = String::from("?initiatedBy=agent");
            if let Some(run) = &advisory.ai_run_id {
                query.push_str("&aiRunId=");
                query.push_str(run);
            }
            if let Some(label) = &advisory.label {
                query.push_str("&agentLabel=");
                query.push_str(label);
            }
            query
        }
        BrowserActionActor::Human => String::new(),
    }
}

/// The header an agent's caller may send to say which AI run it acts for.
const AGENT_RUN_ID_HEADER: &str = "x-agent-run-id";
/// The header an agent's caller may send to label its agent.
const AGENT_LABEL_HEADER: &str = "x-agent-label";

/// What a caller says about *which* agent made a browser call, from the two headers
/// above (the Model Plane's gateway calls and the action surface set them; the
/// action surface forwards the caller's headers to these handlers unchanged).
///
/// Advisory only: Quarry stores it next to the identity its own token check
/// derived and marks it as the caller's word, and nothing decides on it. Values are
/// identifier-shaped (`[A-Za-z0-9._:-]`, 1 to 64 characters) or dropped, so a URL or
/// a page title cannot be smuggled into a record a ZDR run keeps. It rides along only
/// on a call that is already an agent's (`actor: agent`): a person's own call carries
/// none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentAdvisory {
    ai_run_id: Option<String>,
    label: Option<String>,
}

impl AgentAdvisory {
    fn from_headers(headers: &HeaderMap) -> Self {
        let read = |name: &str| {
            let value = headers.get(name)?.to_str().ok()?.trim();
            let shaped = !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'));
            shaped.then(|| value.to_owned())
        };
        Self {
            ai_run_id: read(AGENT_RUN_ID_HEADER),
            label: read(AGENT_LABEL_HEADER),
        }
    }

    fn for_actor(self, actor: BrowserActionActor) -> Self {
        match actor {
            BrowserActionActor::Agent => self,
            BrowserActionActor::Human => Self::default(),
        }
    }

    /// Add the fields to a JSON object body, under the names Quarry reads.
    fn put_in(&self, body: &mut Value) {
        if let Some(run) = &self.ai_run_id {
            body["aiRunId"] = json!(run);
        }
        if let Some(label) = &self.label {
            body["agentLabel"] = json!(label);
        }
    }
}

/// A tab call the model proposed is refused while the person holds the browser,
/// the same 409 `run_action` answers, so the rule does not depend on Quarry's
/// take-over enforcement switch (off by default). Quarry still gets the hint and
/// stays the authority. `None` lets the call through (always for a person's own).
async fn refuse_agent_tab_call_during_takeover(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    session_id: &str,
    actor: BrowserActionActor,
) -> Option<Response> {
    if actor != BrowserActionActor::Agent {
        return None;
    }
    let projection = match owner_browser_session_projection(state, user, token, session_id).await {
        Ok(projection) => projection,
        Err(response) => return Some(response),
    };
    let mode = BrowserControlMode::from_owner_value(
        projection.get("controlMode").and_then(Value::as_str),
    );
    (mode == BrowserControlMode::HumanTakeover).then(|| {
        (
            StatusCode::CONFLICT,
            Json(error(
                "browser_human_takeover_active",
                "Human takeover is active for this browser session.",
            )),
        )
            .into_response()
    })
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
    actor: BrowserActionActor,
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
        "/v1/agent/runs/{}/tabs/{}{}{}",
        urlencoding::encode(&session_id),
        urlencoding::encode(&tab_id),
        suffix,
        tab_call_query(actor, &AgentAdvisory::from_headers(&headers).for_actor(actor))
    );
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    if let Some(refusal) =
        refuse_agent_tab_call_during_takeover(&state, &user, token.as_deref(), &session_id, actor)
            .await
    {
        return refusal;
    }
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
        Json(ok(json!({
            "tabs": tabs_with_opened_by(tabs, &projection),
            "session": response["session"]
        }))),
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
    // The session grant is read from the owner projection before the run is
    // closed, so it can be revoked with it. Best effort: an absent or foreign
    // run keeps today's idempotent close.
    let grant_id = owner_browser_session_projection(&state, &user, token.as_deref(), &session_id)
        .await
        .ok()
        .as_ref()
        .and_then(projection_grant_id);
    let (status, body) = quarry_call(
        &state,
        Method::DELETE,
        &format!("/v1/agent/runs/{}", urlencoding::encode(&session_id)),
        None,
        token.as_deref(),
        &user.user_id,
    )
    .await;
    // The grant is revoked whatever Quarry answered: a person who closed the
    // session wants its authority gone, and a close that failed upstream must not
    // leave the session and run grants live until their TTL (D1-020).
    if let Some(grant_id) = grant_id.as_deref() {
        revoke_session_grant(&state, &user, &headers, grant_id).await;
    }
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
    grant_id: &str,
    allowed_domains: Option<&[String]>,
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
        // The session's BrowserBroker grant (or, behind `BROWSER_AI_RUN_REAL_GRANT`,
        // one issued for this run). Model Gateway derives the run's own grant
        // from it; `allowed_domains` below can only narrow it.
        "grant_id": grant_id,
        "profile_id": persistent_profile_id,
        "start_url": start_url,
        "allowed_domains": allowed_domains,
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
/// `grant_id` is the session's BrowserBroker grant, read from Quarry's owner
/// projection; Model Gateway derives a narrower run grant from it (WS1.B).
/// A session without a grant (opened with `BROWSER_GRANT_ENFORCEMENT=off`, or
/// in `permissive` mode when issuance failed) gets 409
/// `browser_session_grant_missing` and Model Gateway is never called. A
/// grant that has expired or been revoked since the session opened is 409
/// `browser_session_grant_inactive` (see `session_grant_inactive`).
///
/// Behind `BROWSER_AI_RUN_REAL_GRANT` (WS21-S7), and never for a zero-retention
/// session, a session that has no grant of its own is not refused: model-gateway
/// issues a session grant over the run's hosts (`POST /v1/browser/grants`), and
/// the run derives from that grant under the browser bearer minted for this
/// request. With the switch off, or on a zero-retention session, such a session
/// gets the 409 above; no session string is ever sent as a grant.
///
/// An optional `Idempotency-Key` header makes a retry of this start safe: it
/// goes to Model Gateway, which answers a repeat with the same run and the same
/// derived grant (see `client_idempotency_key` and `idempotency_refusal`).
/// Risk-based HITL *gating* of individual risky actions is live as of Phase 5
/// (see `build_ai_run_request` above and `docs/BROWSER_WORKSPACE_PLAN.md`).
pub(crate) async fn start_ai_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(body): Json<StartAiRunBody>,
) -> Response {
    start_ai_run_with_switch(
        state,
        user,
        headers,
        session_id,
        body,
        browser_ai_run_real_grant_enabled(),
    )
    .await
}

/// `start_ai_run` with the `BROWSER_AI_RUN_REAL_GRANT` switch passed in, so the
/// real-grant path runs in tests without touching the process environment.
async fn start_ai_run_with_switch(
    state: AppState,
    user: AuthenticatedUser,
    headers: HeaderMap,
    session_id: String,
    body: StartAiRunBody,
    real_grant_switch: bool,
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
    let idempotency_key = match client_idempotency_key(&headers) {
        Ok(key) => key,
        Err(response) => return response,
    };

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

    let grant_source = match projection_grant_id(&projection) {
        Some(grant_id) => AiRunGrantSource::Session(grant_id),
        // Behind `BROWSER_AI_RUN_REAL_GRANT` (WS21-S7) a session without a grant
        // asks the broker for one over the run's hosts. A zero-retention session
        // never does: it gets no browser bearer and no grant here.
        None if real_grant_switch && !metadata.zdr() => {
            let Some(domains) = ai_run_requested_domains(&body, start_url.as_deref()) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(error(
                        "invalid_allowed_domains",
                        "Name the hosts the run may use, or open a page first.",
                    )),
                )
                    .into_response();
            };
            AiRunGrantSource::Issue(domains)
        }
        // No grant means no run otherwise: the broker refuses anything that is
        // not a grant it issued, so there is no placeholder to send.
        None => {
            return (
                StatusCode::CONFLICT,
                Json(error(
                    "browser_session_grant_missing",
                    "This browser session has no browser grant. Reopen it to start an AI run.",
                )),
            )
                .into_response()
        }
    };
    let names_hosts = body
        .allowed_domains
        .as_ref()
        .is_some_and(|domains| !domains.is_empty());

    let (token, data_plane, inference, execution, session, browser) = tokio::join!(
        model_token(&state, &user, &headers),
        data_plane_token(&state, &user, &headers),
        required_inference_token(&state, &user, &headers),
        required_execution_token(&state, &user, &headers),
        required_session_token(&state, &user, &headers),
        browser_broker_token(&state, &user, &headers),
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
    let browser = match browser {
        Ok(token) => token,
        Err(error) => return delegated_auth_unavailable(error).into_response(),
    };

    let (grant_id, allowed_domains) = match grant_source {
        AiRunGrantSource::Session(grant_id) => (grant_id, body.allowed_domains.clone()),
        AiRunGrantSource::Issue(requested) => {
            match issue_ai_run_grant(&state, &user, token.as_deref(), &browser, &requested).await {
                Ok(issued) => (issued.grant_id, Some(issued.allowed_domains)),
                Err(response) => return response,
            }
        }
    };
    let request_body = build_ai_run_request(
        goal,
        &grant_id,
        allowed_domains.as_deref(),
        &metadata,
        &body,
        start_url.as_deref(),
    );

    let url = format!("{}/v1/browser/runs", state.model_gateway_url);
    // A retry of this start carries the client's key to Model Gateway, which
    // scopes it to the user and this session's grant and answers a repeat with
    // the same run and the same derived grant.
    let idempotency_header: Vec<(&'static str, &str)> = idempotency_key
        .as_deref()
        .map(|key| ("idempotency-key", key))
        .into_iter()
        .collect();
    let (status, Json(response_body)) = proxy_model_json_with_delegated_headers(
        &state,
        Method::POST,
        &url,
        Some(request_body),
        token.as_deref(),
        &[
            ("x-data-plane-authorization", data_plane.as_str()),
            ("x-inference-authorization", inference.as_str()),
            ("x-execution-authorization", execution.as_str()),
            ("x-session-authorization", session.as_str()),
            ("x-browser-authorization", browser.as_str()),
        ],
        &idempotency_header,
        &user,
    )
    .await;
    if !status.is_success() {
        if let Some(response) = idempotency_refusal(status, &response_body) {
            return response;
        }
        if session_grant_inactive(status, &response_body, names_hosts) {
            return (
                StatusCode::CONFLICT,
                Json(error(
                    "browser_session_grant_inactive",
                    "This browser session's grant is no longer valid; it may have expired. Reopen the session to start an AI run.",
                )),
            )
                .into_response();
        }
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

/// The hosts a workspace AI run asks the broker to grant (WS21-S7): the hosts
/// the request names, else the host of the page the user is looking at, which is
/// where the run starts. `None` when neither names a host.
fn ai_run_requested_domains(body: &StartAiRunBody, start_url: Option<&str>) -> Option<Vec<String>> {
    match body.allowed_domains.as_deref() {
        Some(domains) if !domains.is_empty() => Some(domains.to_vec()),
        _ => start_url.and_then(hostname).map(|host| vec![host]),
    }
}

/// Whether a workspace AI run starts under a real broker grant (WS21-S7). Read
/// per request and exact, like the other switches here: only the word `true`
/// enables it.
fn browser_ai_run_real_grant_enabled() -> bool {
    browser_ai_run_real_grant_from(std::env::var("BROWSER_AI_RUN_REAL_GRANT").ok().as_deref())
}

fn browser_ai_run_real_grant_from(value: Option<&str>) -> bool {
    value == Some("true")
}

/// Where a workspace AI run's grant comes from.
enum AiRunGrantSource {
    /// The session's own BrowserBroker grant, from Quarry's owner projection (WS1.B).
    Session(String),
    /// None on the session: ask the broker for one over these hosts (WS21-S7).
    Issue(Vec<String>),
}

/// A session grant model-gateway issued for one workspace AI run (WS21-S7).
struct IssuedAiRunGrant {
    grant_id: String,
    /// The hosts the broker granted. The site policy can grant fewer than were
    /// requested, so the run starts under this list and never under the request.
    allowed_domains: Vec<String>,
}

/// Asks model-gateway's `POST /v1/browser/grants` for a session grant over
/// `requested` (WS21-S7). The grant is the broker's; the gateway only carries the
/// caller's own browser bearer to it.
async fn issue_ai_run_grant(
    state: &AppState,
    user: &AuthenticatedUser,
    token: Option<&str>,
    browser_bearer: &str,
    requested: &[String],
) -> Result<IssuedAiRunGrant, Response> {
    let url = format!("{}/v1/browser/grants", state.model_gateway_url);
    let (status, Json(response_body)) = proxy_model_json_with_delegated(
        state,
        Method::POST,
        &url,
        Some(json!({ "allowed_domains": requested })),
        token,
        &[("x-browser-authorization", browser_bearer)],
        user,
    )
    .await;
    if !status.is_success() {
        return Err(forward_quarry_failure(status, response_body));
    }

    let granted = unwrap_data(&response_body);
    let invalid_grant = |message: &'static str| {
        (
            StatusCode::BAD_GATEWAY,
            Json(error("browser_grant_invalid", message)),
        )
            .into_response()
    };
    let Some(grant_id) = str_field(&granted, "grant_id") else {
        return Err(invalid_grant(
            "Model Gateway did not return a browser grant id.",
        ));
    };
    // A zero-retention grant names no hosts, and the run it starts is refused by
    // model-gateway, so the requested hosts only stand in for a grant that names
    // none.
    let allowed_domains = match granted.get("allowed_domains") {
        None => requested.to_vec(),
        Some(hosts) => serde_json::from_value::<Vec<String>>(hosts.clone()).map_err(|_| {
            invalid_grant("Model Gateway returned an unreadable browser grant policy.")
        })?,
    };
    Ok(IssuedAiRunGrant {
        grant_id,
        allowed_domains,
    })
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
/// Quarry's `GET /v1/artifacts/:id` binds the artifact to the caller's org
/// (`resource_routes.rs` `get_artifact`, from the verified claims), so an id
/// from another tenant reads as not found. The ownership gate below adds the
/// per-run and per-user binding inside the org, and must run before any upstream
/// call.
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
    let mut req = crate::request_id::apply(state.client.get(&url)).header("x-user-id", &user.user_id);
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
                    |_| json!({ "error": { "code": "browser_artifact_failed" } }),
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

/// `GET /api/v1/browser/sessions/{session_id}/events/stream` (WS10.S3): the run's typed
/// events as SSE, proxied from Quarry without buffering. The owner check is Quarry's
/// (the projection call answers 404 to anyone else, and the stream is not opened). The
/// `Last-Event-ID` header and the query (`from_seq`, `max_events`) pass through; Quarry
/// owns the replay ring. The live-frame stream is the model.
async fn stream_agent_events(
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
        "{}/v1/agent/runs/{}/events/stream{}",
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
    // WS9.C: only the typed filters and cursor reach Quarry; unknown keys are dropped.
    let devtools_query = match super::browser_evidence::safe_devtools_query(&uri) {
        Ok(query) => query,
        Err(message) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("invalid_devtools_query", message)),
            )
                .into_response();
        }
    };
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
            devtools_query
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
            "nextSequence": data.get("nextSequence").cloned().unwrap_or(Value::Null),
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
            "capabilities": ["navigate", "back", "forward", "reload", "hard_reload", "click", "click_point", "type", "press", "scroll", "mouse_wheel", "wait_for", "select", "inspect_dom", "control_state", "human_takeover", "agent_control", "tabs", "live_frame", "live_frame_stream", "live_frame_ws", "devtools_events", "devtools_stream", "replay_timeline", "screenshot_artifact", "visual_observation", "visual_change", "annotate", "persistent_profile", "evidence"]
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
                "mode": projection.get("controlMode").cloned().unwrap_or_else(|| Value::String("agent_control".to_owned())),
                "epoch": projection.get("controlEpoch").cloned().unwrap_or(Value::Null)
            },
            "frame": frame,
            "liveFrameUrl": live.then(|| live_frame_url(run_id)),
            "liveFrameStreamUrl": live.then(|| live_frame_stream_url(run_id)),
            // The owner projection is HTTP/SSE-safe. The legacy WebSocket
            // compositor is intentionally not advertised to new sessions.
            "liveFrameWsUrl": Value::Null,
            "tabsUrl": browser_tabs_url(run_id),
            "tabs": Value::Array(tabs_with_opened_by(
                projection
                    .get("tabs")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                projection,
            )),
            "devtoolsUrl": live.then(|| browser_devtools_url(run_id)),
            "devtools": { "events": [], "eventCount": 0, "lastSequence": Value::Null },
            "visual": visual,
            "timeline": [],
            "replay": { "events": [], "eventCount": 0 },
            "zdr": projection.get("zdr").and_then(Value::as_bool).unwrap_or(false),
            "capabilities": browser_session_capabilities(projection)
        },
        "observation": observation
    })
}

/// The capabilities the SPA may act on for a session. The three WS8.D additions are read
/// from Quarry's projection, so none is advertised until Quarry reports it: `human_browsing`
/// needs `grantKind == "human_browse"`, `tab_control` the boolean `tabControl`, and `attach`
/// the boolean `attachAvailable` (false until WS8.F).
fn browser_session_capabilities(projection: &Value) -> Vec<&'static str> {
    let flag = |key: &str| projection.get(key).and_then(Value::as_bool).unwrap_or(false);
    let mut capabilities = vec![
        "navigate",
        "back",
        "forward",
        "reload",
        "hard_reload",
        "click",
        "click_point",
        "type",
        "press",
        "scroll",
        "mouse_wheel",
        "wait_for",
        "select",
        "inspect_dom",
        "control_state",
        "human_takeover",
        "agent_control",
        "tabs",
        "live_frame",
        "live_frame_stream",
        "devtools_events",
        "screenshot_artifact",
        "visual_observation",
        "visual_change",
        "annotate",
        "persistent_profile",
        "evidence",
    ];
    if projection.get("grantKind").and_then(Value::as_str) == Some("human_browse") {
        capabilities.push("human_browsing");
    }
    if flag("tabControl") {
        capabilities.push("tab_control");
    }
    if flag("attachAvailable") {
        capabilities.push("attach");
    }
    capabilities
}

/// The SPA's `openedBy` marker for a tab (WS8.F): `"agent"` or `"human"`, read from Quarry's
/// opener object (`{ initiatedBy, agent?, unattributed? }`, the owner projection's shape).
/// Absent for a tab the browser opened itself, and for an opener class the SPA does not act on
/// (a system opener is neither).
fn opened_by_marker(opened_by: Option<&Value>) -> Option<&'static str> {
    match opened_by?.get("initiatedBy").and_then(Value::as_str)? {
        "agent" => Some("agent"),
        "human" => Some("human"),
        _ => None,
    }
}

/// One tab of a tab response, with the SPA's `openedBy` marker (WS8.F). Quarry's tab list does
/// not carry the opener and the owner projection does, so a tab without one takes the marker of
/// the projection's tab with the same id. Every other field is kept as Quarry sent it; an opener
/// that is not a known class is removed rather than passed on.
fn tab_with_opened_by(mut tab: Value, projection: &Value) -> Value {
    let marker = opened_by_marker(tab.get("openedBy")).or_else(|| {
        let tab_id = tab.get("tabId").and_then(Value::as_str)?;
        projection
            .get("tabs")
            .and_then(Value::as_array)?
            .iter()
            .find(|candidate| candidate.get("tabId").and_then(Value::as_str) == Some(tab_id))
            .and_then(|candidate| opened_by_marker(candidate.get("openedBy")))
    });
    if let Some(object) = tab.as_object_mut() {
        object.remove("openedBy");
        if let Some(marker) = marker {
            object.insert("openedBy".to_owned(), Value::String(marker.to_owned()));
        }
    }
    tab
}

/// Every tab of a tab response, each with its `openedBy` marker (see [`tab_with_opened_by`]).
fn tabs_with_opened_by(tabs: Vec<Value>, projection: &Value) -> Vec<Value> {
    tabs.into_iter()
        .map(|tab| tab_with_opened_by(tab, projection))
        .collect()
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
    let mut req = crate::request_id::apply(state.client.get(&url)).header("x-user-id", &user.user_id);
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

/// Reload is a human-only wire action (WS9.B). An agent-actor request for it
/// is refused before any Quarry call.
fn reload_refused_for_actor(action: &Value, actor: BrowserActionActor) -> bool {
    actor == BrowserActionActor::Agent
        && action.get("type").and_then(Value::as_str) == Some("reload")
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
        "reload" => {
            let known_keys = action
                .as_object()
                .map(|object| {
                    object
                        .keys()
                        .all(|key| key == "type" || key == "ignore_cache")
                })
                .unwrap_or(false);
            if !known_keys {
                return Err((
                    "invalid_browser_action",
                    "Reload accepts only ignore_cache.".to_owned(),
                ));
            }
            let ignore_cache = match action.get("ignore_cache") {
                None => false,
                Some(Value::Bool(value)) => *value,
                Some(_) => {
                    return Err((
                        "invalid_browser_action",
                        "ignore_cache must be a boolean.".to_owned(),
                    ))
                }
            };
            Ok(json!({ "type": "reload", "ignore_cache": ignore_cache }))
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

pub(super) async fn quarry_call(
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

pub(super) async fn quarry_token(state: &AppState, user: &AuthenticatedUser, cookie: &str) -> Option<String> {
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

// ---------------------------------------------------------------------------
// BrowserBroker session grants (WS1.B)
// ---------------------------------------------------------------------------

/// Model Gateway JSON call that carries the delegated credentials a browser
/// grant or browser run needs, each on its own audience-bound header (for
/// example `x-browser-authorization` for `aud=browser-broker`). Org and role
/// headers follow `proxy_model_json`; the role is a presentation hint only.
///
/// Local to the browser domain until `chat::shared`'s proxy grows a browser
/// slot (the plan's `proxy_model_json_with_browser`); it never forwards a
/// credential under a header other than the one named for its audience.
async fn proxy_model_json_with_delegated(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    delegations: &[(&'static str, &str)],
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    proxy_model_json_with_delegated_headers(
        state,
        method,
        url,
        body,
        bearer_token,
        delegations,
        &[],
        user,
    )
    .await
}

/// `proxy_model_json_with_delegated` plus plain request headers
/// (`extra_headers`: name and the raw value, which a caller has already
/// checked). They are not credentials and never get a `Bearer ` prefix.
// reason: one more distinct argument than the plain call; the plain call is
// the wrapper above and the only other way in.
#[allow(clippy::too_many_arguments)]
async fn proxy_model_json_with_delegated_headers(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    bearer_token: Option<&str>,
    delegations: &[(&'static str, &str)],
    extra_headers: &[(&'static str, &str)],
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    let org_id = authorized_org_id(state, user).await;
    let user_role = user
        .auth_role
        .as_deref()
        .map(str::trim)
        .filter(|role| !role.is_empty())
        .unwrap_or("member");
    let mut req = state
        .client
        .request(method, url)
        .header("x-user-id", &user.user_id)
        .header("x-user-role", user_role);
    req = crate::request_id::apply(req);
    if !org_id.trim().is_empty() {
        req = req.header("x-org-id", org_id);
    }
    if let Some(token) = bearer_token {
        req = req.bearer_auth(token);
    }
    for (header_name, token) in delegations {
        if let Some(value) = data_plane_authorization_value(token) {
            req = req.header(*header_name, value);
        }
    }
    for (header_name, value) in extra_headers {
        if let Ok(value) = axum::http::HeaderValue::from_str(value) {
            req = req.header(*header_name, value);
        }
    }
    if let Some(body) = body {
        req = req.json(&body);
    }
    match req.send().await {
        Ok(resp) => {
            let status =
                StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let body = resp.json::<Value>().await.unwrap_or_else(|_| json!({}));
            (status, Json(body))
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            Json(error(
                "model_gateway_unreachable",
                "Model Gateway is temporarily unavailable.",
            )),
        ),
    }
}

/// The delegated `aud=browser-broker` credential for this user.
async fn browser_broker_token(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
) -> Result<String, RequiredAudienceTokenError> {
    require_model_service_token(
        state,
        &user.user_id,
        &cookie_header(headers),
        ModelServiceAudience::BrowserBroker,
    )
    .await
}

/// Why a session grant could not be issued, as the SPA sees it in strict mode.
fn session_grant_refusal(upstream: Option<StatusCode>) -> Response {
    let (status, code, message) = match upstream {
        Some(StatusCode::BAD_REQUEST) | None => (
            StatusCode::BAD_REQUEST,
            "browser_host_not_grantable",
            "This address cannot be opened in the in-app browser.",
        ),
        Some(StatusCode::FORBIDDEN) => (
            StatusCode::FORBIDDEN,
            "browser_grant_denied",
            "Browser access was not granted for this session.",
        ),
        Some(StatusCode::TOO_MANY_REQUESTS) => (
            StatusCode::TOO_MANY_REQUESTS,
            "browser_grant_limit_reached",
            "Too many browser sessions are open. Close one and try again.",
        ),
        Some(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "browser_grant_unavailable",
            "Browser access is temporarily unavailable.",
        ),
    };
    (status, Json(error(code, message))).into_response()
}

/// What a session's BrowserBroker grant covers (WS8.D).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionGrantScope<'a> {
    /// The start host only: the first-host session grant every session had before WS8.D.
    StartHost(Option<&'a str>),
    /// A person's own open-web browsing. A `human_browse` grant names no hosts.
    HumanBrowse,
}

/// The `POST /v1/browser/grants` body for a scope (WS8.D). `None` means there is no
/// host to grant (a start URL without one).
fn session_grant_request_body(scope: SessionGrantScope<'_>, zdr: bool) -> Option<Value> {
    match scope {
        SessionGrantScope::StartHost(Some(host)) => {
            Some(json!({ "allowed_domains": [host], "zdr": zdr }))
        }
        SessionGrantScope::StartHost(None) => None,
        SessionGrantScope::HumanBrowse => Some(json!({ "kind": "human_browse", "zdr": zdr })),
    }
}

/// The Quarry start-body `constraints` for a session (WS8.D). A human_browse session
/// sends none: Quarry takes its bounds from the broker policy, so no step, time or host
/// literal reaches it. Every other session keeps the first-host policy (12 steps, 120 s).
fn session_start_constraints(human_grant: bool, start_host: Option<&str>) -> Option<Value> {
    if human_grant {
        return None;
    }
    Some(json!({
        "max_steps": 12,
        "allowed_domains": start_host.map(|domain| vec![domain]).unwrap_or_default(),
        "max_runtime_s": 120
    }))
}

/// Obtains a BrowserBroker grant for a session (WS8.D: the start host, or a person's
/// open-web `human_browse` grant).
///
/// `Ok(None)` means the session opens without a grant: the mode is `Off`, or it is
/// `Permissive` and issuance failed. In `Strict` every failure is an error response
/// and no Quarry run is started. Hosts are never logged.
async fn issue_session_grant(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    scope: SessionGrantScope<'_>,
    zdr: bool,
) -> Result<Option<String>, Response> {
    let mode = state.browser_grant_mode;
    if !mode.issues_grants() {
        return Ok(None);
    }
    let strict = mode == BrowserGrantMode::Strict;
    let Some(request_body) = session_grant_request_body(scope, zdr) else {
        return if strict {
            Err(session_grant_refusal(None))
        } else {
            Ok(None)
        };
    };
    let (model, browser) = tokio::join!(
        model_token(state, user, headers),
        browser_broker_token(state, user, headers),
    );
    let browser = match browser {
        Ok(token) => token,
        Err(error) if strict => return Err(delegated_auth_unavailable(error).into_response()),
        Err(_) => {
            tracing::warn!(
                outcome = "browser_token_unavailable",
                "browser session opened without a grant (permissive)"
            );
            return Ok(None);
        }
    };
    let url = format!("{}/v1/browser/grants", state.model_gateway_url);
    let (status, Json(body)) = proxy_model_json_with_delegated(
        state,
        Method::POST,
        &url,
        Some(request_body),
        model.as_deref(),
        &[("x-browser-authorization", browser.as_str())],
        user,
    )
    .await;
    let grant_id = status
        .is_success()
        .then(|| str_field(&unwrap_data(&body), "grant_id"))
        .flatten()
        .filter(|id| is_valid_path_segment(id));
    match grant_id {
        Some(grant_id) => Ok(Some(grant_id)),
        None if strict => Err(session_grant_refusal(Some(status))),
        None => {
            tracing::warn!(
                status = status.as_u16(),
                outcome = "grant_not_issued",
                "browser session opened without a grant (permissive)"
            );
            Ok(None)
        }
    }
}

/// Revokes a session grant, best effort: a failure leaves the grant to expire
/// on its own TTL, and revoking an already-gone grant is a 200 upstream.
async fn revoke_session_grant(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    grant_id: &str,
) {
    if !is_valid_path_segment(grant_id) {
        return;
    }
    let (model, browser) = tokio::join!(
        model_token(state, user, headers),
        browser_broker_token(state, user, headers),
    );
    let Ok(browser) = browser else {
        return;
    };
    let url = format!(
        "{}/v1/browser/grants/{}",
        state.model_gateway_url,
        urlencoding::encode(grant_id)
    );
    let (status, _) = proxy_model_json_with_delegated(
        state,
        Method::DELETE,
        &url,
        None,
        model.as_deref(),
        &[("x-browser-authorization", browser.as_str())],
        user,
    )
    .await;
    if !status.is_success() {
        tracing::warn!(
            status = status.as_u16(),
            "browser session grant revoke failed; it expires on its TTL"
        );
    }
}

/// Quarry's owner projection as the SPA may see it: unchanged except that the
/// session grant id (`projection_grant_id`) is removed.
fn spa_owner_projection(mut projection: Value) -> Value {
    if let Some(object) = projection.as_object_mut() {
        object.remove("grantId");
        object.remove("grant_id");
    }
    projection
}

/// The client's `Idempotency-Key` for an AI-run start: one header, 1 to 128
/// characters of `A-Z a-z 0-9 - _ . :`. Model Gateway enforces the same shape;
/// checking it here keeps a malformed key from costing a token mint and a round
/// trip, and keeps the SPA's error in the gateway's own envelope.
fn client_idempotency_key(headers: &HeaderMap) -> Result<Option<String>, Response> {
    let refusal = || {
        (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_idempotency_key",
                "The Idempotency-Key header must be 1 to 128 letters, digits, '-', '_', '.' or ':'.",
            )),
        )
            .into_response()
    };
    let mut values = headers.get_all("idempotency-key").iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(refusal());
    }
    let key = first.to_str().map_err(|_| refusal())?.trim();
    if key.is_empty()
        || key.len() > 128
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(refusal());
    }
    Ok(Some(key.to_owned()))
}

/// Model Gateway's refusals of a keyed AI-run start, in the gateway's own
/// typed envelope. `None` for anything else. The raw upstream strings
/// (`idempotency_key_reused`, `idempotency_key_run_ended`,
/// `idempotency_replay_unverified`) never reach the SPA.
fn idempotency_refusal(status: StatusCode, body: &Value) -> Option<Response> {
    let code = body.get("error").and_then(Value::as_str)?;
    match (status, code) {
        (StatusCode::CONFLICT, "idempotency_key_reused") => Some(
            (
                StatusCode::CONFLICT,
                Json(error(
                    "browser_idempotency_key_reused",
                    "This request key was already used for a different goal. Use a new key to start a new run.",
                )),
            )
                .into_response(),
        ),
        (StatusCode::CONFLICT, "idempotency_key_run_ended") => {
            let mut response = error(
                "browser_idempotency_key_run_ended",
                "The run for this request key has already ended. Use a new key to start another run.",
            );
            if let Some(run_id) = body
                .get("run_id")
                .and_then(Value::as_str)
                .filter(|id| is_valid_path_segment(id))
            {
                response["error"]["details"] = json!({ "runId": run_id });
            }
            Some((StatusCode::CONFLICT, Json(response)).into_response())
        }
        (StatusCode::SERVICE_UNAVAILABLE, "idempotency_replay_unverified") => Some(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                [(axum::http::header::RETRY_AFTER, "5")],
                Json(error(
                    "browser_idempotency_unverified",
                    "The earlier run for this request key could not be confirmed. Try again shortly.",
                )),
            )
                .into_response(),
        ),
        _ => None,
    }
}

/// Whether Model Gateway refused an AI run because the session grant can no
/// longer back one. The session grant is issued once, when the session opens,
/// for the broker's grant TTL (15 minutes by default, at most an hour), and is
/// not renewed (WS1.B): a session older than that, or one whose grant was
/// revoked, lands here. Model Gateway never says which (it does not tell
/// expired, revoked and unknown grants apart), so a 403 is read this way only
/// when the request did not name hosts; with explicit hosts a 403 may be a
/// widening refusal and is forwarded unchanged.
fn session_grant_inactive(status: StatusCode, body: &Value, names_hosts: bool) -> bool {
    let code = body.get("error").and_then(Value::as_str);
    match status {
        StatusCode::CONFLICT => code == Some("browser_grant_not_derivable"),
        StatusCode::FORBIDDEN => !names_hosts && code == Some("browser grant is not authorized"),
        _ => false,
    }
}

/// The session grant id from Quarry's owner projection. Internal: it is read
/// to derive AI-run grants and to revoke on close, never sent to the SPA.
fn projection_grant_id(projection: &Value) -> Option<String> {
    projection
        .get("grantId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && is_valid_path_segment(id))
        .map(str::to_owned)
}

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
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
pub(super) enum BrowserArtifactAbsence {
    /// Zero Data Retention run: Quarry refused the artifact write at capture
    /// time, so nothing was ever stored and nothing ever will be.
    Withheld,
    /// The store has no such artifact — never written (a ZDR run reaching this
    /// path without a local ZDR flag) or aged out of retention.
    NeverStored,
}

impl BrowserArtifactAbsence {
    pub(super) fn into_response(self) -> Response {
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

/// The body of a `/step` call for an action the person or the model proposed. An
/// action the model proposed carries `initiated_by: "agent"`, so Quarry (which owns
/// the browser) can refuse it while the person has taken over. A person's own action
/// carries no hint: Quarry already takes it for the person's.
fn step_request_body(action: Value, actor: BrowserActionActor, advisory: &AgentAdvisory) -> Value {
    match actor {
        BrowserActionActor::Agent => {
            let mut body = json!({ "action": action, "initiated_by": "agent" });
            advisory.put_in(&mut body);
            body
        }
        BrowserActionActor::Human => json!({ "action": action }),
    }
}

/// A start or call Quarry refused because a cap is full, as the gateway's own
/// 429 with `Retry-After`. Only the retry hint survives; nothing else from the
/// upstream body is echoed. `None` for any other 429.
///
/// - The run caps (`global_run_cap`, `org_run_cap`, `org_interactive_run_cap`,
///   and the plan's own `org_plan_cap`), the organization's share of the render
///   pool (`org_render_cap`, WS6.G) and the driver's session cap (`browser_session_cap`, WS3.F) are one thing
///   to a person: the browser is full, try shortly. `browser_capacity_reached`.
/// - The driver's page cap (`browser_page_cap`) is a different one: this
///   browser holds too many tabs, and closing one also helps.
///   `browser_tab_limit_reached`.
fn browser_capacity_response(body: &Value) -> Option<Response> {
    let details = body.pointer("/error/details")?;
    let reason = details.get("reason")?.as_str()?;
    let (code, message) = match reason {
        "global_run_cap"
        | "org_run_cap"
        | "org_interactive_run_cap"
        | "org_plan_cap"
        | "org_render_cap"
        | "browser_session_cap" => (
            "browser_capacity_reached",
            "Browser capacity is full. Try again shortly.",
        ),
        "browser_page_cap" => (
            "browser_tab_limit_reached",
            "The browser has too many tabs open. Close a tab or try again shortly.",
        ),
        _ => return None,
    };
    let retry_after = details
        .get("retry_after_seconds")
        .and_then(Value::as_u64)
        .filter(|seconds| (1..=3600).contains(seconds))
        .unwrap_or(10);
    let mut response = error(code, message);
    response["error"]["retryAfterSeconds"] = json!(retry_after);
    Some(
        (
            StatusCode::TOO_MANY_REQUESTS,
            [(axum::http::header::RETRY_AFTER, retry_after.to_string())],
            Json(response),
        )
            .into_response(),
    )
}

/// A browser session the driver no longer holds (WS3.F), as `410 Gone`: Quarry
/// answers `502 DRIVER_FAILED` with `details.reason` `session_reaped` (the idle
/// reaper closed it) or `browser_process_lost` (Chromium exited). Neither is
/// retryable: repeating the call cannot help, the person starts a new session.
/// No `Retry-After`. `None` for any other `DRIVER_FAILED` (for example
/// `viewport_emulation_failed`, `no_current_page`), which passes through.
fn browser_session_lost_response(body: &Value) -> Option<Response> {
    if body.pointer("/error/code").and_then(Value::as_str) != Some("DRIVER_FAILED") {
        return None;
    }
    let (code, message) = match body.pointer("/error/details/reason")?.as_str()? {
        "session_reaped" => (
            "browser_session_reaped",
            "This browser session was closed after being idle. Start a new session.",
        ),
        "browser_process_lost" => (
            "browser_process_lost",
            "The browser behind this session stopped unexpectedly. Start a new session.",
        ),
        _ => return None,
    };
    Some((StatusCode::GONE, Json(error(code, message))).into_response())
}

/// Quarry's `403 CHALLENGE_DETECTED` on a browser call (WS3.H/WS3.J): the host
/// stands behind a bot wall, or the one recovery the run was allowed is spent
/// and Quarry refuses to act on it. The upstream body names the host and carries
/// Quarry's own message, so it is never forwarded: the SPA gets the gateway's
/// own `browser_site_blocked` with the two typed hints it has copy for.
///
/// - `blockReason`: `details.block_reason` (scrape path) or `details.reason`
///   (agent path, where it is `challenge_loop`), only when it is one of the
///   closed `BlockReason` strings; otherwise absent.
/// - `recovery`: `details.recovery`, only when it is one of the closed
///   `ChallengeRecovery` strings; otherwise absent.
///
/// `None` for any other code, so a 403 that is not a challenge (`FORBIDDEN`,
/// `SECURITY_BLOCKED`) passes through as before.
fn browser_site_blocked_response(body: &Value) -> Option<Response> {
    if body.pointer("/error/code").and_then(Value::as_str) != Some("CHALLENGE_DETECTED") {
        return None;
    }
    let typed = |pointers: &[&str], known: &[&'static str]| -> Option<&'static str> {
        pointers.iter().find_map(|pointer| {
            let value = body.pointer(pointer)?.as_str()?;
            known.iter().copied().find(|candidate| *candidate == value)
        })
    };
    let block_reason = typed(
        &["/error/details/block_reason", "/error/details/reason"],
        &[
            "captcha_widget",
            "waf_interstitial",
            "unexpected_bot_error",
            "challenge_loop",
        ],
    );
    let recovery = typed(
        &["/error/details/recovery"],
        &["available", "exhausted", "not_applicable"],
    );
    let mut response = error(
        "browser_site_blocked",
        "This site blocks automated browsing. Take over the browser yourself or try another source.",
    );
    if let Some(reason) = block_reason {
        response["error"]["blockReason"] = json!(reason);
    }
    if let Some(recovery) = recovery {
        response["error"]["recovery"] = json!(recovery);
    }
    Some((StatusCode::FORBIDDEN, Json(response)).into_response())
}

pub(super) fn forward_quarry_failure(status: StatusCode, body: Value) -> Response {
    if status == StatusCode::TOO_MANY_REQUESTS {
        if let Some(response) = browser_capacity_response(&body) {
            return response;
        }
    }
    if let Some(response) = browser_session_lost_response(&body) {
        return response;
    }
    if status == StatusCode::FORBIDDEN {
        if let Some(response) = browser_site_blocked_response(&body) {
            return response;
        }
    }
    // Quarry enforces the takeover itself; the SPA only knows the gateway's own
    // code for it, so translate (and keep the control epoch for the caller).
    if body.pointer("/error/code").and_then(Value::as_str) == Some("USER_TOOK_OVER") {
        let mut response = error(
            "browser_human_takeover_active",
            "Human takeover is active for this browser session.",
        );
        if let Some(details) = body.pointer("/error/details") {
            response["error"]["details"] = details.clone();
        }
        return (StatusCode::CONFLICT, Json(response)).into_response();
    }
    // WS9.I: a control change refused because the epoch moved on. Quarry answers CONFLICT with
    // the epochs in its details; only that shape maps, any other 409 passes through.
    if status == StatusCode::CONFLICT
        && body.pointer("/error/code").and_then(Value::as_str) == Some("CONFLICT")
        && body.pointer("/error/details/control_epoch").is_some()
    {
        let mut response = error(
            "browser_control_epoch_stale",
            "Browser control changed elsewhere. Refresh and try again.",
        );
        if let Some(details) = body.pointer("/error/details") {
            response["error"]["details"] = details.clone();
        }
        return (StatusCode::CONFLICT, Json(response)).into_response();
    }
    // A navigation outside the run's allowed domains (Quarry's 403 for a
    // cross-host click). WS8 will emit a typed code; until then the match is
    // on Quarry's message, and the SPA gets the typed code its copy is keyed on.
    if status == StatusCode::FORBIDDEN
        && body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .is_some_and(|message| message.contains("outside the run's allowed domains"))
    {
        return (
            StatusCode::FORBIDDEN,
            Json(error(
                "browser_navigation_outside_scope",
                "This session is limited to the domains of its run.",
            )),
        )
            .into_response();
    }
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

pub(super) fn is_valid_path_segment(value: &str) -> bool {
    let len = value.len();
    (3..=128).contains(&len)
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

pub(super) fn is_valid_artifact_id(value: &str) -> bool {
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

/// The content type a knowledge scrape artifact may be served as (WS13.7). Only
/// the three image types and `application/pdf` pass; an unknown or missing type is
/// `None` and the caller refuses it. Unlike `safe_browser_artifact_content_type`
/// there is no `image/png` fallback, so a capture stored under another type is
/// never served as an image.
pub(super) fn scrape_artifact_content_type(value: Option<&str>) -> Option<&'static str> {
    match value
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
    {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/webp" => Some("image/webp"),
        "application/pdf" => Some("application/pdf"),
        _ => None,
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
pub(crate) mod tests {
    use super::*;

    #[test]
    fn run_action_forwards_agent_initiator_to_quarry() {
        let action = json!({ "type": "back" });
        assert_eq!(
            step_request_body(action.clone(), BrowserActionActor::Agent, &AgentAdvisory::default()),
            json!({ "action": { "type": "back" }, "initiated_by": "agent" })
        );
        assert_eq!(
            step_request_body(action, BrowserActionActor::Human, &AgentAdvisory::default()),
            json!({ "action": { "type": "back" } }),
            "a person's own action carries no hint"
        );
    }

    #[test]
    fn tab_calls_forward_only_the_fixed_agent_hint_to_quarry() {
        assert_eq!(
            new_tab_request_body(
                Some("https://example.test/".to_owned()),
                BrowserActionActor::Agent,
                &AgentAdvisory::default()
            ),
            json!({ "url": "https://example.test/", "initiatedBy": "agent" })
        );
        assert_eq!(
            new_tab_request_body(None, BrowserActionActor::Human, &AgentAdvisory::default()),
            json!({ "url": null }),
            "a person's own tab open carries no hint"
        );
        assert_eq!(
            tab_call_query(BrowserActionActor::Agent, &AgentAdvisory::default()),
            "?initiatedBy=agent"
        );
        assert_eq!(
            tab_call_query(BrowserActionActor::Human, &AgentAdvisory::default()),
            "",
            "a person's own select or close carries no hint"
        );
    }

    fn headers_with(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_str(value).expect("a header value"));
        }
        headers
    }

    #[test]
    fn an_agents_tab_and_step_calls_forward_the_advisory_identity_to_quarry() {
        let headers = headers_with(&[
            (AGENT_RUN_ID_HEADER, "run_01J9"),
            (AGENT_LABEL_HEADER, "research-planner"),
        ]);
        let advisory = AgentAdvisory::from_headers(&headers).for_actor(BrowserActionActor::Agent);
        assert_eq!(
            new_tab_request_body(None, BrowserActionActor::Agent, &advisory),
            json!({
                "url": null,
                "initiatedBy": "agent",
                "aiRunId": "run_01J9",
                "agentLabel": "research-planner"
            })
        );
        assert_eq!(
            tab_call_query(BrowserActionActor::Agent, &advisory),
            "?initiatedBy=agent&aiRunId=run_01J9&agentLabel=research-planner"
        );
        assert_eq!(
            step_request_body(json!({ "type": "back" }), BrowserActionActor::Agent, &advisory),
            json!({
                "action": { "type": "back" },
                "initiated_by": "agent",
                "aiRunId": "run_01J9",
                "agentLabel": "research-planner"
            })
        );
    }

    #[test]
    fn a_persons_own_call_never_carries_the_advisory_identity() {
        let headers = headers_with(&[(AGENT_RUN_ID_HEADER, "run_01J9"), (AGENT_LABEL_HEADER, "x")]);
        let advisory = AgentAdvisory::from_headers(&headers).for_actor(BrowserActionActor::Human);
        assert_eq!(advisory, AgentAdvisory::default());
        assert_eq!(
            new_tab_request_body(None, BrowserActionActor::Human, &advisory),
            json!({ "url": null })
        );
        assert_eq!(tab_call_query(BrowserActionActor::Human, &advisory), "");
        assert_eq!(
            step_request_body(json!({ "type": "back" }), BrowserActionActor::Human, &advisory),
            json!({ "action": { "type": "back" } })
        );
    }

    #[test]
    fn the_advisory_identity_is_identifier_shaped_or_dropped() {
        let long = "a".repeat(65);
        for rejected in [
            "",
            "has space",
            "https://example.test/x",
            "a/b",
            "a&aiRunId=forged",
            "a?b",
            "tab\tname",
            long.as_str(),
        ] {
            // A header value cannot hold every one of these; those that it can are dropped.
            let Ok(value) = HeaderValue::from_str(rejected) else { continue };
            let mut headers = HeaderMap::new();
            headers.insert(AGENT_RUN_ID_HEADER, value.clone());
            headers.insert(AGENT_LABEL_HEADER, value);
            assert_eq!(
                AgentAdvisory::from_headers(&headers),
                AgentAdvisory::default(),
                "{rejected:?}"
            );
        }
        // A non-ASCII value is a header the gateway cannot read as text: dropped too.
        let mut headers = HeaderMap::new();
        headers.insert(
            AGENT_LABEL_HEADER,
            HeaderValue::from_bytes("caf\u{e9}".as_bytes()).expect("opaque bytes"),
        );
        assert_eq!(AgentAdvisory::from_headers(&headers), AgentAdvisory::default());
    }

    #[test]
    fn the_tab_actor_query_takes_human_or_agent_and_nothing_else() {
        let parse = |query: &str| {
            let uri: Uri = format!("/select{query}").parse().expect("a valid uri");
            Query::<TabActorQuery>::try_from_uri(&uri).map(|Query(parsed)| parsed.actor)
        };
        assert_eq!(parse("").expect("absent"), BrowserActionActor::Human);
        assert_eq!(parse("?actor=human").expect("human"), BrowserActionActor::Human);
        assert_eq!(parse("?actor=agent").expect("agent"), BrowserActionActor::Agent);
        assert!(parse("?actor=system").is_err(), "an unknown actor is refused");
        assert!(parse("?actor=").is_err(), "an empty actor is refused");
    }

    /// A Quarry that knows one run (\`run-1\`, one tab \`tab-1\`) in the given control
    /// mode, answering every tab route.
    async fn mount_tab_run(quarry: &wiremock::MockServer, control_mode: &str) {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, ResponseTemplate,
        };

        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": { "runId": "run-1", "controlMode": control_mode, "tabs": [] }
            })))
            .mount(quarry)
            .await;
        let tabs = ResponseTemplate::new(200).set_body_json(json!({ "data": { "tabs": [] } }));
        let opened = ResponseTemplate::new(200).set_body_json(json!({
            "data": { "tab": { "tabId": "tab-2" }, "tabs": [] }
        }));
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/agent/runs/run-1/tabs/tab-1/select"))
            .respond_with(tabs.clone())
            .mount(quarry)
            .await;
        Mock::given(wm_method("DELETE"))
            .and(wm_path("/v1/agent/runs/run-1/tabs/tab-1"))
            .respond_with(tabs)
            .mount(quarry)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/agent/runs/run-1/tabs"))
            .respond_with(opened)
            .mount(quarry)
            .await;
    }

    async fn quarry_mutations(quarry: &wiremock::MockServer) -> Vec<(String, String, Value)> {
        quarry
            .received_requests()
            .await
            .expect("recorded Quarry requests")
            .into_iter()
            .filter(|request| request.method.as_str() != "GET")
            .map(|request| {
                (
                    request.method.to_string(),
                    match request.url.query() {
                        Some(query) => format!("{}?{query}", request.url.path()),
                        None => request.url.path().to_owned(),
                    },
                    serde_json::from_slice(&request.body).unwrap_or(Value::Null),
                )
            })
            .collect()
    }

    async fn call_tab_routes(
        state: &AppState,
        actor: BrowserActionActor,
    ) -> [StatusCode; 3] {
        call_tab_routes_with(state, actor, HeaderMap::new()).await
    }

    async fn call_tab_routes_with(
        state: &AppState,
        actor: BrowserActionActor,
        headers: HeaderMap,
    ) -> [StatusCode; 3] {
        call_tab_routes_as(state, actor, headers, None).await
    }

    async fn call_tab_routes_as(
        state: &AppState,
        actor: BrowserActionActor,
        headers: HeaderMap,
        caller: Option<CallerClass>,
    ) -> [StatusCode; 3] {
        let user = test_user("user-owner", "org-owner");
        let select = select_tab(
            State(state.clone()),
            Extension(user.clone()),
            caller.map(Extension),
            headers.clone(),
            Path(("run-1".to_owned(), "tab-1".to_owned())),
            Query(TabActorQuery { actor }),
        )
        .await;
        let close = close_tab(
            State(state.clone()),
            Extension(user.clone()),
            caller.map(Extension),
            headers.clone(),
            Path(("run-1".to_owned(), "tab-1".to_owned())),
            Query(TabActorQuery { actor }),
        )
        .await;
        let open = new_tab(
            State(state.clone()),
            Extension(user),
            caller.map(Extension),
            headers,
            Path("run-1".to_owned()),
            Json(NewTabBody { url: None, actor }),
        )
        .await;
        [select.status(), close.status(), open.status()]
    }

    #[tokio::test]
    async fn a_models_tab_calls_reach_quarry_labelled_agent() {
        let quarry = wiremock::MockServer::start().await;
        mount_tab_run(&quarry, "agent_control").await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        assert_eq!(
            call_tab_routes(&state, BrowserActionActor::Agent).await,
            [StatusCode::OK; 3]
        );
        let mutations = quarry_mutations(&quarry).await;
        assert_eq!(
            mutations
                .iter()
                .map(|(method, target, _)| (method.as_str(), target.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("POST", "/v1/agent/runs/run-1/tabs/tab-1/select?initiatedBy=agent"),
                ("DELETE", "/v1/agent/runs/run-1/tabs/tab-1?initiatedBy=agent"),
                ("POST", "/v1/agent/runs/run-1/tabs"),
            ]
        );
        assert_eq!(mutations[2].2["initiatedBy"], "agent", "{:?}", mutations[2]);
    }

    /// An agent's step forwards the identity its headers state as `aiRunId` and
    /// `agentLabel` in the body (advisory fields; Quarry never decides on them), and a
    /// person's own step carries neither, whatever the headers say. This drives
    /// `run_action` itself, so removing its header read fails the test.
    #[tokio::test]
    async fn an_agents_step_forwards_its_advisory_identity_and_a_persons_does_not() {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, ResponseTemplate,
        };

        let headers = headers_with(&[
            (AGENT_RUN_ID_HEADER, "run_01J9"),
            (AGENT_LABEL_HEADER, "research-planner"),
        ]);
        for (actor, control_mode) in [
            (BrowserActionActor::Agent, "agent_control"),
            (BrowserActionActor::Human, "human_takeover"),
        ] {
            let quarry = wiremock::MockServer::start().await;
            mount_tab_run(&quarry, control_mode).await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/agent/runs/run-1/step"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "data": { "ok": true } })),
                )
                .mount(&quarry)
                .await;
            let mut state = test_app_state();
            state.quarry_edge_url = quarry.uri();

            let response = run_action(
                State(state),
                Extension(test_user("user-owner", "org-owner")),
                None,
                headers.clone(),
                Path("run-1".to_owned()),
                Json(ActionBody {
                    action: json!({ "type": "wait", "ms": 10 }),
                    actor,
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK, "{actor:?}");

            let steps: Vec<Value> = quarry_mutations(&quarry)
                .await
                .into_iter()
                .filter(|(_, target, _)| target == "/v1/agent/runs/run-1/step")
                .map(|(_, _, body)| body)
                .collect();
            assert_eq!(steps.len(), 1, "{actor:?}: {steps:?}");
            match actor {
                BrowserActionActor::Agent => {
                    assert_eq!(steps[0]["initiated_by"], "agent");
                    assert_eq!(steps[0]["aiRunId"], "run_01J9");
                    assert_eq!(steps[0]["agentLabel"], "research-planner");
                }
                BrowserActionActor::Human => {
                    assert!(steps[0].get("initiated_by").is_none(), "{:?}", steps[0]);
                    assert!(
                        steps[0].get("aiRunId").is_none() && steps[0].get("agentLabel").is_none(),
                        "{:?}",
                        steps[0]
                    );
                }
            }
        }
    }

    /// The agent identity the caller states in its headers reaches Quarry on an agent's
    /// tab open, select and close as the advisory fields, and only there: a person's own
    /// calls carry none, and a value that is not identifier-shaped is dropped.
    #[tokio::test]
    async fn an_agents_tab_calls_forward_its_advisory_identity_and_a_persons_do_not() {
        let headers = headers_with(&[
            (AGENT_RUN_ID_HEADER, "run_01J9"),
            (AGENT_LABEL_HEADER, "research-planner"),
        ]);
        let quarry = wiremock::MockServer::start().await;
        mount_tab_run(&quarry, "agent_control").await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        assert_eq!(
            call_tab_routes_with(&state, BrowserActionActor::Agent, headers.clone()).await,
            [StatusCode::OK; 3]
        );
        let mutations = quarry_mutations(&quarry).await;
        assert_eq!(
            mutations
                .iter()
                .map(|(method, target, _)| (method.as_str(), target.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "POST",
                    "/v1/agent/runs/run-1/tabs/tab-1/select?initiatedBy=agent&aiRunId=run_01J9&agentLabel=research-planner"
                ),
                (
                    "DELETE",
                    "/v1/agent/runs/run-1/tabs/tab-1?initiatedBy=agent&aiRunId=run_01J9&agentLabel=research-planner"
                ),
                ("POST", "/v1/agent/runs/run-1/tabs"),
            ]
        );
        assert_eq!(mutations[2].2["initiatedBy"], "agent");
        assert_eq!(mutations[2].2["aiRunId"], "run_01J9");
        assert_eq!(mutations[2].2["agentLabel"], "research-planner");

        let person = wiremock::MockServer::start().await;
        mount_tab_run(&person, "human_takeover").await;
        let mut state = test_app_state();
        state.quarry_edge_url = person.uri();
        assert_eq!(
            call_tab_routes_with(&state, BrowserActionActor::Human, headers).await,
            [StatusCode::OK; 3]
        );
        let mutations = quarry_mutations(&person).await;
        assert_eq!(
            mutations
                .iter()
                .map(|(method, target, _)| (method.as_str(), target.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("POST", "/v1/agent/runs/run-1/tabs/tab-1/select"),
                ("DELETE", "/v1/agent/runs/run-1/tabs/tab-1"),
                ("POST", "/v1/agent/runs/run-1/tabs"),
            ]
        );
        assert!(mutations[2].2.get("aiRunId").is_none() && mutations[2].2.get("agentLabel").is_none());
    }

    #[tokio::test]
    async fn a_persons_tab_calls_carry_no_initiator_hint() {
        let quarry = wiremock::MockServer::start().await;
        mount_tab_run(&quarry, "human_takeover").await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        // The person's own calls are admitted during their take-over, with no hint:
        // Quarry reads a person's token as the person's.
        assert_eq!(
            call_tab_routes(&state, BrowserActionActor::Human).await,
            [StatusCode::OK; 3]
        );
        let mutations = quarry_mutations(&quarry).await;
        assert_eq!(
            mutations
                .iter()
                .map(|(method, target, _)| (method.as_str(), target.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("POST", "/v1/agent/runs/run-1/tabs/tab-1/select"),
                ("DELETE", "/v1/agent/runs/run-1/tabs/tab-1"),
                ("POST", "/v1/agent/runs/run-1/tabs"),
            ]
        );
        assert!(mutations[2].2.get("initiatedBy").is_none(), "{:?}", mutations[2]);
    }

    /// With Quarry's take-over enforcement off (its default) Quarry only logs a
    /// refusable call, so the gateway refuses a model's tab call itself, as it does a
    /// model's action: the same 409, and nothing reaches Quarry.
    #[tokio::test]
    async fn a_models_tab_call_is_refused_during_a_takeover_before_it_reaches_quarry() {
        let quarry = wiremock::MockServer::start().await;
        mount_tab_run(&quarry, "human_takeover").await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        assert_eq!(
            call_tab_routes(&state, BrowserActionActor::Agent).await,
            [StatusCode::CONFLICT; 3]
        );
        assert!(
            quarry_mutations(&quarry).await.is_empty(),
            "a refused tab call never reaches Quarry"
        );
    }

    /// WS8.F (U62-66): an agent's tab carries the SPA's `openedBy` marker in the tab list and on
    /// the session's tabs. Quarry's tab list has no opener; the gateway reads it from the owner
    /// projection by tab id. A tab the person opened reads `human`; the browser's own tab carries
    /// no marker.
    #[tokio::test]
    async fn the_tab_list_and_the_session_name_each_tabs_opener_as_the_spa_reads_it() {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, ResponseTemplate,
        };

        let quarry = wiremock::MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/tabs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "tabs": [
                        { "tabId": "tab_home", "title": null, "url": null, "active": false },
                        { "tabId": "tab_agent", "title": "Plan", "url": "https://example.com/", "active": true },
                        { "tabId": "tab_person", "title": null, "url": null, "active": false }
                    ]
                }
            })))
            .mount(&quarry)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/v1/agent/runs/run-1/browser-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "runId": "run-1",
                    "controlMode": "agent_control",
                    "tabs": [
                        { "tabId": "tab_home", "title": null, "url": null, "active": false },
                        {
                            "tabId": "tab_agent",
                            "title": "Plan",
                            "url": "https://example.com/",
                            "active": true,
                            "openedBy": {
                                "initiatedBy": "agent",
                                "agent": { "basis": "user_session", "aiRunId": "run_01J9", "label": "planner" }
                            }
                        },
                        {
                            "tabId": "tab_person",
                            "title": null,
                            "url": null,
                            "active": false,
                            "openedBy": { "initiatedBy": "human" }
                        }
                    ]
                }
            })))
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let response = get_tabs(
            State(state.clone()),
            Extension(test_user("user-owner", "org-owner")),
            HeaderMap::new(),
            Path("run-1".to_owned()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("the body is read");
        let body: Value = serde_json::from_slice(&bytes).expect("a JSON body");

        for tabs in [&body["data"]["tabs"], &body["data"]["session"]["tabs"]] {
            assert!(tabs[0].get("openedBy").is_none(), "{body}");
            assert_eq!(tabs[1]["openedBy"], "agent", "{body}");
            assert_eq!(tabs[2]["openedBy"], "human", "{body}");
        }
        assert_eq!(body["data"]["tabs"][1]["title"], "Plan", "{body}");
    }

    #[tokio::test]
    async fn forward_quarry_failure_maps_cap_denial_to_browser_capacity_reached() {
        for reason in ["global_run_cap", "org_run_cap", "org_interactive_run_cap"] {
            assert_capacity_reason_maps(reason).await;
        }
        // Any other 429 passes through untouched.
        let other = json!({ "error": { "code": "RATE_LIMITED", "details": { "reason": "something_else" } } });
        let response = forward_quarry_failure(StatusCode::TOO_MANY_REQUESTS, other);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "RATE_LIMITED");
    }

    /// WS6.G: the plan's own cap and the organization's render share are the same
    /// person-facing thing as the process caps, with the same retry copy.
    #[tokio::test]
    async fn forward_quarry_failure_maps_org_caps_to_browser_capacity_reached() {
        for reason in ["org_plan_cap", "org_render_cap"] {
            assert_capacity_reason_maps(reason).await;
        }
    }

    /// WS9.I: the epoch the person saw travels to Quarry as a compare-and-set; a legacy body
    /// without it stays unconditional.
    #[test]
    fn control_body_forwards_expected_epoch() {
        let body: ControlBody = serde_json::from_value(json!({
            "mode": "human_takeover",
            "expectedEpoch": 2
        }))
        .expect("control body with an epoch");
        assert_eq!(body.expected_epoch, Some(2));
        let forwarded = control_quarry_body(body.mode, body.expected_epoch);
        assert_eq!(forwarded["mode"], "human_takeover");
        assert_eq!(forwarded["expected_epoch"], 2);

        let legacy: ControlBody =
            serde_json::from_value(json!({ "mode": "agent_control" })).expect("legacy body");
        assert_eq!(legacy.expected_epoch, None);
        assert!(control_quarry_body(legacy.mode, legacy.expected_epoch)
            .get("expected_epoch")
            .is_none());
    }

    /// WS9.I: Quarry's CONFLICT with the epochs in its details becomes the typed stale-epoch
    /// error; a CONFLICT without them is not reclassified.
    #[tokio::test]
    async fn stale_epoch_maps_to_typed_conflict() {
        let quarry = json!({ "error": {
            "code": "CONFLICT",
            "message": "the browser's control epoch has moved on",
            "details": { "control_epoch": 3, "expected_epoch": 2 }
        }});
        let response = forward_quarry_failure(StatusCode::CONFLICT, quarry);
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "browser_control_epoch_stale");
        assert_eq!(body["error"]["details"]["control_epoch"], 3);

        let other = json!({ "error": { "code": "CONFLICT", "message": "tab is busy" } });
        let response = forward_quarry_failure(StatusCode::CONFLICT, other);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_ne!(body["error"]["code"], "browser_control_epoch_stale");
    }

    async fn assert_capacity_reason_maps(reason: &str) {
        let quarry = json!({ "error": {
            "code": "RATE_LIMITED",
            "message": "browser capacity is full; try again shortly",
            "details": { "reason": reason, "scope": "org", "limit": 3, "active": 3, "retry_after_seconds": 12 },
            "retryable": true
        }});
        let response = forward_quarry_failure(StatusCode::TOO_MANY_REQUESTS, quarry);
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("retry-after").unwrap(), "12");
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "browser_capacity_reached");
        assert_eq!(body["error"]["retryAfterSeconds"], 12);
        assert!(!body.to_string().contains("RATE_LIMITED"), "{reason}");
    }

    #[tokio::test]
    async fn forward_quarry_failure_maps_navigation_scope() {
        let quarry = json!({ "error": {
            "code": "SECURITY_BLOCKED",
            "message": "browser navigation is outside the run's allowed domains"
        }});
        let response = forward_quarry_failure(StatusCode::FORBIDDEN, quarry);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = body_of(response).await;
        assert_eq!(body["error"]["code"], "browser_navigation_outside_scope");
        assert!(!body.to_string().contains("SECURITY_BLOCKED"));

        // Another 403 keeps Quarry's own envelope.
        let other = json!({ "error": { "code": "SECURITY_BLOCKED", "message": "blocked for another reason" } });
        let response = forward_quarry_failure(StatusCode::FORBIDDEN, other);
        assert_eq!(body_of(response).await["error"]["code"], "SECURITY_BLOCKED");
    }

    async fn body_of(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn driver_cap_body(reason: &str, retry_after: Value) -> Value {
        json!({ "error": {
            "code": "RATE_LIMITED",
            "message": "browser capacity exhausted",
            "details": {
                "reason": reason, "scope": "driver", "limit": 40, "active": 40,
                "retry_after_seconds": retry_after
            },
            "retryable": true
        }})
    }

    #[tokio::test]
    async fn the_driver_session_cap_is_browser_capacity_reached_with_the_retry_hint() {
        let response = forward_quarry_failure(
            StatusCode::TOO_MANY_REQUESTS,
            driver_cap_body("browser_session_cap", json!(15)),
        );
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("retry-after").unwrap(), "15");
        let body = body_of(response).await;
        assert_eq!(body["error"]["code"], "browser_capacity_reached");
        assert_eq!(body["error"]["retryAfterSeconds"], 15);
        let text = body.to_string();
        assert!(!text.contains("RATE_LIMITED") && !text.contains("browser_session_cap"));
        assert!(!text.contains("\"limit\"") && !text.contains("\"active\""));
    }

    #[tokio::test]
    async fn the_driver_page_cap_is_its_own_code_with_the_retry_hint() {
        let response = forward_quarry_failure(
            StatusCode::TOO_MANY_REQUESTS,
            driver_cap_body("browser_page_cap", json!(15)),
        );
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("retry-after").unwrap(), "15");
        let body = body_of(response).await;
        assert_eq!(body["error"]["code"], "browser_tab_limit_reached");
        assert_eq!(body["error"]["retryAfterSeconds"], 15);
        assert!(!body.to_string().contains("browser_page_cap"));
    }

    #[tokio::test]
    async fn a_missing_or_absurd_retry_hint_falls_back_to_ten_seconds() {
        for hint in [json!(null), json!(0), json!(86_400), json!("soon"), json!(-3)] {
            let response = forward_quarry_failure(
                StatusCode::TOO_MANY_REQUESTS,
                driver_cap_body("browser_session_cap", hint.clone()),
            );
            assert_eq!(response.headers().get("retry-after").unwrap(), "10", "{hint}");
            assert_eq!(body_of(response).await["error"]["retryAfterSeconds"], 10);
        }
    }

    /// The retry key can be absent altogether (an older Quarry, or a cap that sets none),
    /// not only null or out of range: `details` has no `retry_after_seconds`.
    #[tokio::test]
    async fn an_absent_retry_key_falls_back_to_ten_seconds_for_every_capacity_reason() {
        for (reason, code) in [
            ("global_run_cap", "browser_capacity_reached"),
            ("org_run_cap", "browser_capacity_reached"),
            ("org_interactive_run_cap", "browser_capacity_reached"),
            ("org_plan_cap", "browser_capacity_reached"),
            ("org_render_cap", "browser_capacity_reached"),
            ("browser_session_cap", "browser_capacity_reached"),
            ("browser_page_cap", "browser_tab_limit_reached"),
        ] {
            let quarry = json!({ "error": {
                "code": "RATE_LIMITED",
                "message": "browser capacity exhausted",
                "details": { "reason": reason, "scope": "org", "limit": 3, "active": 3 },
                "retryable": true
            }});
            let response = forward_quarry_failure(StatusCode::TOO_MANY_REQUESTS, quarry);
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS, "{reason}");
            assert_eq!(response.headers().get("retry-after").unwrap(), "10", "{reason}");
            let body = body_of(response).await;
            assert_eq!(body["error"]["code"], code, "{reason}");
            assert_eq!(body["error"]["retryAfterSeconds"], 10, "{reason}");
            assert!(!body.to_string().contains(reason), "{reason}");
        }
    }

    #[tokio::test]
    async fn a_lost_browser_session_is_410_with_its_own_code_and_no_retry_hint() {
        for (reason, code) in [
            ("session_reaped", "browser_session_reaped"),
            ("browser_process_lost", "browser_process_lost"),
        ] {
            let quarry = json!({ "error": {
                "code": "DRIVER_FAILED",
                "message": "browser session was closed after being idle too long",
                "details": { "reason": reason },
                "retryable": false
            }});
            let response = forward_quarry_failure(StatusCode::BAD_GATEWAY, quarry);
            assert_eq!(response.status(), StatusCode::GONE, "{reason}");
            assert!(response.headers().get("retry-after").is_none(), "{reason}");
            let body = body_of(response).await;
            assert_eq!(body["error"]["code"], code);
            assert!(body["error"].get("retryAfterSeconds").is_none());
            assert!(!body.to_string().contains("DRIVER_FAILED"), "{reason}");
        }
    }

    #[tokio::test]
    async fn other_driver_failures_and_lookalike_bodies_pass_through_untouched() {
        for (status, quarry) in [
            // Another DRIVER_FAILED reason keeps its upstream body.
            (
                StatusCode::BAD_GATEWAY,
                json!({ "error": { "code": "DRIVER_FAILED", "details": { "reason": "no_current_page" } } }),
            ),
            (
                StatusCode::BAD_GATEWAY,
                json!({ "error": { "code": "DRIVER_FAILED", "details": { "reason": "viewport_emulation_failed" } } }),
            ),
            // A lost-session reason under a different code is not ours to map.
            (
                StatusCode::BAD_GATEWAY,
                json!({ "error": { "code": "INTERNAL", "details": { "reason": "session_reaped" } } }),
            ),
            // A driver-cap reason on a non-429 is not a cap.
            (
                StatusCode::BAD_GATEWAY,
                json!({ "error": { "code": "DRIVER_FAILED", "details": { "reason": "browser_session_cap" } } }),
            ),
        ] {
            let response = forward_quarry_failure(status, quarry.clone());
            assert_eq!(response.status(), status);
            assert_eq!(body_of(response).await, quarry);
        }
    }

    #[tokio::test]
    async fn quarry_user_took_over_maps_to_browser_human_takeover_active() {
        let quarry = json!({ "error": {
            "code": "USER_TOOK_OVER",
            "message": "the person using the browser has taken control",
            "details": { "control_mode": "human_takeover", "control_epoch": 3 },
            "retryable": false
        }});
        let response = forward_quarry_failure(StatusCode::CONFLICT, quarry);
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], "browser_human_takeover_active");
        assert_eq!(body["error"]["details"]["control_epoch"], 3);
        assert!(!body.to_string().contains("USER_TOOK_OVER"));
    }

    #[tokio::test]
    async fn a_challenge_403_is_browser_site_blocked_with_typed_hints_and_nothing_else() {
        // The scrape path names the reason in `block_reason`, the agent path in
        // `reason` (with the host and `recovery: exhausted`).
        let scrape = json!({ "error": {
            "code": "CHALLENGE_DETECTED",
            "message": "blocked https://secret.example/private <html>wall</html>",
            "details": { "reason": "waf_interstitial", "block_reason": "waf_interstitial", "recovery": "not_applicable", "next": "other_source" },
            "retryable": false
        }});
        let agent = json!({ "error": {
            "code": "CHALLENGE_DETECTED",
            "message": "secret.example refused after the recovery",
            "details": { "reason": "challenge_loop", "host": "secret.example", "recovery": "exhausted" },
            "retryable": false
        }});
        for (quarry, reason, recovery) in [
            (scrape, "waf_interstitial", "not_applicable"),
            (agent, "challenge_loop", "exhausted"),
        ] {
            let response = forward_quarry_failure(StatusCode::FORBIDDEN, quarry);
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert!(response.headers().get("retry-after").is_none());
            let body = body_of(response).await;
            assert_eq!(body["error"]["code"], "browser_site_blocked");
            assert_eq!(body["error"]["blockReason"], reason);
            assert_eq!(body["error"]["recovery"], recovery);
            let text = body.to_string();
            assert!(
                !text.contains("secret.example")
                    && !text.contains("CHALLENGE_DETECTED")
                    && !text.contains("<html")
                    && !text.contains("\"host\""),
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn an_unknown_challenge_reason_or_recovery_is_dropped_not_echoed() {
        let quarry = json!({ "error": {
            "code": "CHALLENGE_DETECTED",
            "details": { "reason": "https://secret.example/x", "block_reason": 7, "recovery": "go_around" }
        }});
        let response = forward_quarry_failure(StatusCode::FORBIDDEN, quarry);
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = body_of(response).await;
        assert_eq!(body["error"]["code"], "browser_site_blocked");
        assert!(body["error"].get("blockReason").is_none());
        assert!(body["error"].get("recovery").is_none());
        assert!(!body.to_string().contains("secret.example"));
        assert!(!body.to_string().contains("go_around"));
    }

    #[tokio::test]
    async fn a_403_that_is_not_a_challenge_passes_through_untouched() {
        for quarry in [
            json!({ "error": { "code": "FORBIDDEN", "details": { "reason": "waf_interstitial" } } }),
            json!({ "error": { "code": "SECURITY_BLOCKED", "message": "policy" } }),
        ] {
            let response = forward_quarry_failure(StatusCode::FORBIDDEN, quarry.clone());
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_eq!(body_of(response).await, quarry);
        }
    }

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
                    "grantId": "0123456789abcdef0123456789abcdef",
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
        // The session grant id is internal to the BFF (WS1.B).
        assert!(body["data"].get("grantId").is_none(), "{body}");
        assert!(
            !body.to_string().contains("0123456789abcdef0123456789abcdef"),
            "grant id reached the SPA: {body}"
        );
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

    /// Answers `GET /api/{slug}/token` with `test-plane-token-{slug}`, so a
    /// test can tell which audience a forwarded credential was minted for.
    struct PerAudienceToken;

    impl wiremock::Respond for PerAudienceToken {
        fn respond(&self, request: &wiremock::Request) -> wiremock::ResponseTemplate {
            let slug = request
                .url
                .path()
                .trim_start_matches("/api/")
                .trim_end_matches("/token");
            wiremock::ResponseTemplate::new(200)
                .set_body_json(json!({ "token": format!("test-plane-token-{slug}") }))
        }
    }

    /// Stand in for auth-core's plane-token endpoints so the tests run the real
    /// credential path (`model_token` + `required_session_token`) instead of
    /// skipping it. `GET /api/{audience}/token` → `{ "token": "test-plane-token-{audience}" }`.
    async fn plane_token_issuer() -> wiremock::MockServer {
        use wiremock::{
            matchers::{method as wm_method, path_regex},
            Mock, MockServer, ResponseTemplate,
        };

        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(path_regex(r"^/api/[a-z-]+/token$"))
            .respond_with(PerAudienceToken)
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

    /// WS10.S1: the adopted x-request-id rides on the browser proxy's upstream
    /// read (the model-gateway run probe), so a support lookup can join the
    /// gateway log to the upstream log.
    #[tokio::test]
    async fn browser_proxy_forwards_request_id_to_quarry() {
        let upstream = model_plane_run_read_model().await;
        let auth = plane_token_issuer().await;
        let state = state_for(&auth, &upstream);
        let headers = HeaderMap::new();
        let owner = owner_of("user-owner", "org-owner");
        let _ = crate::request_id::scope_for_test(
            "gw-forward-1",
            owned_orchestration_run(
                &state,
                &test_user("user-owner", "org-owner"),
                &headers,
                &owner,
                "run-owned",
            ),
        )
        .await;
        let received = upstream
            .received_requests()
            .await
            .expect("recorded requests");
        let last = received.last().expect("the probe must reach the upstream");
        assert_eq!(
            last.headers.get("x-request-id").map(|v| v.to_str().unwrap()),
            Some("gw-forward-1"),
            "the adopted request id must be forwarded upstream"
        );
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

    /// WS10.S3: the events stream forwards `Last-Event-ID` and the query to Quarry.
    #[tokio::test]
    async fn browser_events_stream_proxies_last_event_id_and_query_to_quarry() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-owned/browser-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"zdr": false}})))
            .mount(&quarry)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-owned/events/stream"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("id: 4\nevent: action.started\ndata: {}\n\n"),
            )
            .mount(&quarry)
            .await;

        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "3".parse().unwrap());
        let response = stream_agent_events(
            State(state.clone()),
            Extension(test_user("user-owner", "org-owner")),
            headers,
            Path("run-owned".to_owned()),
            Uri::from_static("/api/v1/browser/sessions/run-owned/events/stream?max_events=5"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let requests = quarry.received_requests().await.unwrap_or_default();
        let forwarded = requests
            .iter()
            .find(|request| request.url.path() == "/v1/agent/runs/run-owned/events/stream")
            .expect("the events stream reaches Quarry");
        assert_eq!(forwarded.url.query(), Some("max_events=5"));
        assert_eq!(
            forwarded
                .headers
                .get("last-event-id")
                .and_then(|value| value.to_str().ok()),
            Some("3")
        );
    }

    /// WS10.S3: a caller Quarry does not admit to the run never opens the stream.
    #[tokio::test]
    async fn browser_events_stream_refuses_a_non_owner() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let quarry = MockServer::start().await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        Mock::given(method("GET"))
            .and(path("/v1/agent/runs/run-owned/browser-session"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "error": { "code": "NOT_FOUND", "message": "agent run not found" }
            })))
            .mount(&quarry)
            .await;

        let response = stream_agent_events(
            State(state.clone()),
            Extension(test_user("user-attacker", "org-owner")),
            HeaderMap::new(),
            Path("run-owned".to_owned()),
            Uri::from_static("/api/v1/browser/sessions/run-owned/events/stream"),
        )
        .await;
        assert!(response.status().is_client_error(), "got {}", response.status());

        let requests = quarry.received_requests().await.unwrap_or_default();
        assert!(
            requests
                .iter()
                .all(|request| !request.url.path().ends_with("/events/stream")),
            "a non-owner must not reach the events stream"
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
                None,
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
                    expected_epoch: None,
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
                None,
                headers.clone(),
                Path("run-owned".to_owned()),
                Json(NewTabBody::default()),
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
                BrowserActionActor::Human,
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

    // --- WS1.B: BrowserBroker session grants ------------------------------

    mod session_grants {
        use super::*;
        use axum::body::to_bytes;
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        const GRANT: &str = "0123456789abcdef0123456789abcdef";

        struct Upstreams {
            auth: MockServer,
            model: MockServer,
            quarry: MockServer,
        }

        async fn upstreams() -> Upstreams {
            Upstreams {
                auth: plane_token_issuer().await,
                model: MockServer::start().await,
                quarry: MockServer::start().await,
            }
        }

        fn state(up: &Upstreams, mode: BrowserGrantMode) -> AppState {
            let mut state = test_app_state();
            state.auth_core_url = up.auth.uri();
            state.model_gateway_url = up.model.uri();
            state.quarry_edge_url = up.quarry.uri();
            state.browser_grant_mode = mode;
            state
        }

        async fn mount_quarry_session(quarry: &MockServer, projection: Value) {
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/agent/runs"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "data": { "run_id": "run-1" } })),
                )
                .mount(quarry)
                .await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/agent/runs/run-1/step"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    json!({ "data": { "url": "https://example.com/", "title": "Example" } }),
                ))
                .mount(quarry)
                .await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/v1/agent/runs/run-1/browser-session"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "data": projection })),
                )
                .mount(quarry)
                .await;
            Mock::given(wm_method("DELETE"))
                .and(wm_path("/v1/agent/runs/run-1"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
                .mount(quarry)
                .await;
        }

        fn owner_projection(grant: Option<&str>) -> Value {
            let mut projection = json!({
                "runId": "run-1",
                "status": "active",
                "live": true,
                "currentUrl": "https://example.com/",
                "profileScope": "ephemeral",
                "zdr": false,
                "tabs": []
            });
            if let Some(grant) = grant {
                projection["grantId"] = json!(grant);
            }
            projection
        }

        async fn create(state: AppState) -> (StatusCode, Value) {
            let body: CreateSessionBody =
                serde_json::from_value(json!({ "url": "https://example.com/" })).expect("body");
            let response = create_session(
                State(state),
                Extension(test_user("user-a", "org-a")),
                HeaderMap::new(),
                Json(body),
            )
            .await;
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 256 * 1024)
                .await
                .expect("body");
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            )
        }

        async fn requests_to(
            server: &MockServer,
            method: &str,
            path: &str,
        ) -> Vec<wiremock::Request> {
            server
                .received_requests()
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|r| r.method.as_str() == method && r.url.path() == path)
                .collect()
        }

        #[tokio::test]
        async fn create_session_puts_the_issued_grant_on_the_quarry_start_and_never_in_the_response(
        ) {
            let up = upstreams().await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/grants"))
                .respond_with(
                    ResponseTemplate::new(201).set_body_json(
                        json!({ "grant_id": GRANT, "kind": "session", "zdr": false }),
                    ),
                )
                .mount(&up.model)
                .await;
            mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;

            let (status, body) = create(state(&up, BrowserGrantMode::Strict)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert!(
                !body.to_string().contains(GRANT),
                "grant id leaked to the SPA: {body}"
            );

            let grants = requests_to(&up.model, "POST", "/v1/browser/grants").await;
            assert_eq!(grants.len(), 1);
            assert_eq!(
                grants[0]
                    .headers
                    .get("x-browser-authorization")
                    .and_then(|v| v.to_str().ok()),
                Some("Bearer test-plane-token-browser-broker")
            );
            let grant_body: Value = serde_json::from_slice(&grants[0].body).expect("grant body");
            assert_eq!(grant_body["allowed_domains"], json!(["example.com"]));
            assert_eq!(grant_body["zdr"], json!(false));

            let starts = requests_to(&up.quarry, "POST", "/v1/agent/runs").await;
            let start_body: Value = serde_json::from_slice(&starts[0].body).expect("start body");
            assert_eq!(start_body["grant_id"], json!(GRANT));
        }

        #[tokio::test]
        async fn strict_mode_without_a_grant_never_starts_a_quarry_run() {
            for (upstream, want_status, want_code) in [
                (400, StatusCode::BAD_REQUEST, "browser_host_not_grantable"),
                (403, StatusCode::FORBIDDEN, "browser_grant_denied"),
                (
                    429,
                    StatusCode::TOO_MANY_REQUESTS,
                    "browser_grant_limit_reached",
                ),
                (
                    503,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "browser_grant_unavailable",
                ),
                (
                    502,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "browser_grant_unavailable",
                ),
            ] {
                let up = upstreams().await;
                Mock::given(wm_method("POST"))
                    .and(wm_path("/v1/browser/grants"))
                    .respond_with(
                        ResponseTemplate::new(upstream)
                            .set_body_json(json!({ "error": "browser_host_not_grantable" })),
                    )
                    .mount(&up.model)
                    .await;
                mount_quarry_session(&up.quarry, owner_projection(None)).await;

                let (status, body) = create(state(&up, BrowserGrantMode::Strict)).await;
                assert_eq!(status, want_status, "upstream {upstream}: {body}");
                assert_eq!(body["error"]["code"], want_code, "upstream {upstream}");
                assert!(
                    up.quarry
                        .received_requests()
                        .await
                        .unwrap_or_default()
                        .is_empty(),
                    "upstream {upstream}: Quarry must not be called without a grant"
                );
            }
        }

        #[tokio::test]
        async fn permissive_mode_opens_the_session_without_a_grant_when_issuance_fails() {
            let up = upstreams().await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/grants"))
                .respond_with(ResponseTemplate::new(503).set_body_json(json!({})))
                .mount(&up.model)
                .await;
            mount_quarry_session(&up.quarry, owner_projection(None)).await;

            let (status, _) = create(state(&up, BrowserGrantMode::Permissive)).await;
            assert_eq!(status, StatusCode::OK);
            let starts = requests_to(&up.quarry, "POST", "/v1/agent/runs").await;
            let start_body: Value = serde_json::from_slice(&starts[0].body).expect("start body");
            assert!(start_body.get("grant_id").is_none(), "{start_body}");
        }

        #[tokio::test]
        async fn off_mode_never_asks_for_a_grant() {
            let up = upstreams().await;
            mount_quarry_session(&up.quarry, owner_projection(None)).await;
            let (status, _) = create(state(&up, BrowserGrantMode::Off)).await;
            assert_eq!(status, StatusCode::OK);
            assert!(up
                .model
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty());
        }

        #[tokio::test]
        async fn a_failed_quarry_start_revokes_the_grant_it_was_given() {
            let up = upstreams().await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/grants"))
                .respond_with(
                    ResponseTemplate::new(201).set_body_json(json!({ "grant_id": GRANT })),
                )
                .mount(&up.model)
                .await;
            Mock::given(wm_method("DELETE"))
                .and(wm_path(format!("/v1/browser/grants/{GRANT}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "revoked": true })))
                .mount(&up.model)
                .await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/agent/runs"))
                .respond_with(ResponseTemplate::new(503).set_body_json(json!({})))
                .mount(&up.quarry)
                .await;

            let (status, _) = create(state(&up, BrowserGrantMode::Strict)).await;
            assert!(!status.is_success());
            let revokes =
                requests_to(&up.model, "DELETE", &format!("/v1/browser/grants/{GRANT}")).await;
            assert_eq!(revokes.len(), 1);
        }

        #[tokio::test]
        async fn a_failed_first_step_revokes_the_grant_it_was_given() {
            let up = upstreams().await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/grants"))
                .respond_with(
                    ResponseTemplate::new(201).set_body_json(json!({ "grant_id": GRANT })),
                )
                .mount(&up.model)
                .await;
            Mock::given(wm_method("DELETE"))
                .and(wm_path(format!("/v1/browser/grants/{GRANT}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "revoked": true })))
                .mount(&up.model)
                .await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/agent/runs"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "data": { "run_id": "run-1" } })),
                )
                .mount(&up.quarry)
                .await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/agent/runs/run-1/step"))
                .respond_with(ResponseTemplate::new(502).set_body_json(json!({})))
                .mount(&up.quarry)
                .await;
            Mock::given(wm_method("DELETE"))
                .and(wm_path("/v1/agent/runs/run-1"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": {} })))
                .mount(&up.quarry)
                .await;

            let (status, _) = create(state(&up, BrowserGrantMode::Strict)).await;
            assert!(!status.is_success());
            let revokes =
                requests_to(&up.model, "DELETE", &format!("/v1/browser/grants/{GRANT}")).await;
            assert_eq!(revokes.len(), 1);
        }

        fn ai_run_body() -> StartAiRunBody {
            StartAiRunBody {
                goal: "find the price".to_owned(),
                allowed_domains: Some(vec!["shop.example.com".to_owned()]),
                max_steps: None,
                max_runtime_s: None,
                stop_criteria: None,
                require_approval: None,
                max_cost_usd: None,
            }
        }

        async fn start_run(state: AppState) -> (StatusCode, Value) {
            let response = start_ai_run(
                State(state),
                Extension(test_user("user-a", "org-a")),
                HeaderMap::new(),
                Path("run-1".to_owned()),
                Json(ai_run_body()),
            )
            .await;
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 256 * 1024)
                .await
                .expect("body");
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            )
        }

        #[tokio::test]
        async fn an_ai_run_carries_the_session_grant_and_the_browser_bearer() {
            let up = upstreams().await;
            mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/runs"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(json!({ "run_id": "mp-run-1" })),
                )
                .mount(&up.model)
                .await;

            let (status, body) = start_run(state(&up, BrowserGrantMode::Strict)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            let runs = requests_to(&up.model, "POST", "/v1/browser/runs").await;
            assert_eq!(runs.len(), 1);
            let sent: Value = serde_json::from_slice(&runs[0].body).expect("run body");
            assert_eq!(sent["grant_id"], json!(GRANT));
            assert_eq!(sent["allowed_domains"], json!(["shop.example.com"]));
            for (header, audience) in [
                ("x-browser-authorization", "browser-broker"),
                ("x-session-authorization", "session-core"),
                ("x-execution-authorization", "execution-core"),
                ("x-inference-authorization", "inference-core"),
                ("x-data-plane-authorization", "data-plane"),
                ("authorization", "model-plane"),
            ] {
                assert_eq!(
                    runs[0].headers.get(header).and_then(|v| v.to_str().ok()),
                    Some(format!("Bearer test-plane-token-{audience}").as_str()),
                    "{header}"
                );
            }
        }

        #[tokio::test]
        async fn an_ai_run_on_a_session_without_a_grant_is_409_in_every_mode() {
            for mode in [
                BrowserGrantMode::Strict,
                BrowserGrantMode::Permissive,
                BrowserGrantMode::Off,
            ] {
                let up = upstreams().await;
                mount_quarry_session(&up.quarry, owner_projection(None)).await;
                let (status, body) = start_run(state(&up, mode)).await;
                assert_eq!(status, StatusCode::CONFLICT, "{mode:?}");
                assert_eq!(body["error"]["code"], "browser_session_grant_missing");
                assert!(up
                    .model
                    .received_requests()
                    .await
                    .unwrap_or_default()
                    .is_empty());
            }
        }

        #[tokio::test]
        async fn closing_a_session_revokes_its_grant() {
            let up = upstreams().await;
            mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
            Mock::given(wm_method("DELETE"))
                .and(wm_path(format!("/v1/browser/grants/{GRANT}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "revoked": true })))
                .mount(&up.model)
                .await;
            let response = close_session(
                State(state(&up, BrowserGrantMode::Strict)),
                Extension(test_user("user-a", "org-a")),
                HeaderMap::new(),
                Path("run-1".to_owned()),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            let revokes =
                requests_to(&up.model, "DELETE", &format!("/v1/browser/grants/{GRANT}")).await;
            assert_eq!(revokes.len(), 1);
            assert_eq!(
                revokes[0]
                    .headers
                    .get("x-browser-authorization")
                    .and_then(|v| v.to_str().ok()),
                Some("Bearer test-plane-token-browser-broker")
            );
        }

        /// D1-020: a close Quarry fails (anything but 2xx or 404) still revokes the
        /// session grant; the person still gets the failure.
        #[tokio::test]
        async fn a_close_that_quarry_fails_still_revokes_the_session_grant() {
            let up = upstreams().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/v1/agent/runs/run-1/browser-session"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    json!({ "data": owner_projection(Some(GRANT)) }),
                ))
                .mount(&up.quarry)
                .await;
            Mock::given(wm_method("DELETE"))
                .and(wm_path("/v1/agent/runs/run-1"))
                .respond_with(ResponseTemplate::new(500).set_body_json(
                    json!({ "error": { "code": "INTERNAL", "message": "close failed" } }),
                ))
                .mount(&up.quarry)
                .await;
            Mock::given(wm_method("DELETE"))
                .and(wm_path(format!("/v1/browser/grants/{GRANT}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "revoked": true })))
                .mount(&up.model)
                .await;
            let response = close_session(
                State(state(&up, BrowserGrantMode::Strict)),
                Extension(test_user("user-a", "org-a")),
                HeaderMap::new(),
                Path("run-1".to_owned()),
            )
            .await;
            assert_ne!(response.status(), StatusCode::OK, "the failure is still reported");
            let revokes =
                requests_to(&up.model, "DELETE", &format!("/v1/browser/grants/{GRANT}")).await;
            assert_eq!(revokes.len(), 1, "the grant is revoked all the same");
        }

        async fn start_run_with(state: AppState, body: StartAiRunBody) -> (StatusCode, Value) {
            let response = start_ai_run(
                State(state),
                Extension(test_user("user-a", "org-a")),
                HeaderMap::new(),
                Path("run-1".to_owned()),
                Json(body),
            )
            .await;
            let status = response.status();
            let bytes = to_bytes(response.into_body(), 256 * 1024)
                .await
                .expect("body");
            (
                status,
                serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            )
        }

        // The session grant is issued once and not renewed. When Model Gateway
        // refuses it as no longer valid (expired or revoked: 403 from
        // ValidateGrant, or 409 from DeriveRunGrant), the SPA gets one stable
        // code that says to reopen the session, not "the session has no grant".
        #[tokio::test]
        async fn an_expired_or_revoked_session_grant_is_reported_as_inactive() {
            for (upstream, code) in [
                (403, "browser grant is not authorized"),
                (409, "browser_grant_not_derivable"),
            ] {
                let up = upstreams().await;
                mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
                Mock::given(wm_method("POST"))
                    .and(wm_path("/v1/browser/runs"))
                    .respond_with(
                        ResponseTemplate::new(upstream).set_body_json(json!({ "error": code })),
                    )
                    .mount(&up.model)
                    .await;
                let mut body = ai_run_body();
                body.allowed_domains = None;
                let (status, response) =
                    start_run_with(state(&up, BrowserGrantMode::Strict), body).await;
                assert_eq!(status, StatusCode::CONFLICT, "{upstream}: {response}");
                assert_eq!(
                    response["error"]["code"],
                    json!("browser_session_grant_inactive"),
                    "{upstream}: {response}"
                );
                assert!(!response.to_string().contains(GRANT), "{response}");
            }
        }

        async fn start_run_response(
            state: AppState,
            headers: HeaderMap,
            body: StartAiRunBody,
        ) -> Response {
            start_ai_run(
                State(state),
                Extension(test_user("user-a", "org-a")),
                headers,
                Path("run-1".to_owned()),
                Json(body),
            )
            .await
        }

        fn key_headers(values: &[&str]) -> HeaderMap {
            let mut headers = HeaderMap::new();
            for value in values {
                headers.append(
                    "idempotency-key",
                    axum::http::HeaderValue::from_bytes(value.as_bytes()).expect("header value"),
                );
            }
            headers
        }

        async fn json_of(response: Response) -> (StatusCode, HeaderMap, Value) {
            let status = response.status();
            let headers = response.headers().clone();
            let bytes = to_bytes(response.into_body(), 256 * 1024)
                .await
                .expect("body");
            (
                status,
                headers,
                serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            )
        }

        #[tokio::test]
        async fn a_client_idempotency_key_reaches_model_gateway_and_only_when_sent() {
            let up = upstreams().await;
            mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/runs"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "run_id": "mp-run-1", "thread_id": "t-1", "plan_id": "p-1"
                })))
                .mount(&up.model)
                .await;

            for headers in [key_headers(&["retry-1"]), key_headers(&["  retry-1  "])] {
                let (status, _, body) = json_of(
                    start_run_response(state(&up, BrowserGrantMode::Strict), headers, ai_run_body())
                        .await,
                )
                .await;
                assert_eq!(status, StatusCode::OK, "{body}");
            }
            let (status, _, _) = json_of(
                start_run_response(
                    state(&up, BrowserGrantMode::Strict),
                    HeaderMap::new(),
                    ai_run_body(),
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::OK);

            let runs = requests_to(&up.model, "POST", "/v1/browser/runs").await;
            let sent: Vec<Option<String>> = runs
                .iter()
                .map(|run| {
                    run.headers
                        .get("idempotency-key")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_owned)
                })
                .collect();
            assert_eq!(
                sent,
                [Some("retry-1".to_owned()), Some("retry-1".to_owned()), None],
                "the key is forwarded trimmed, and absent when the client sent none"
            );
        }

        #[tokio::test]
        async fn a_malformed_or_repeated_idempotency_key_is_400_before_any_upstream_call() {
            let long = "k".repeat(129);
            for values in [
                vec!["has space"],
                vec!["semi;colon"],
                vec![""],
                vec!["ünicode"],
                vec![long.as_str()],
                vec!["one", "two"],
            ] {
                let up = upstreams().await;
                mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
                let (status, _, body) = json_of(
                    start_run_response(
                        state(&up, BrowserGrantMode::Strict),
                        key_headers(&values),
                        ai_run_body(),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{values:?}: {body}");
                assert_eq!(body["error"]["code"], "invalid_idempotency_key");
                for server in [&up.model, &up.quarry] {
                    assert!(
                        server
                            .received_requests()
                            .await
                            .unwrap_or_default()
                            .is_empty(),
                        "{values:?} reached an upstream"
                    );
                }
            }
        }

        #[tokio::test]
        async fn a_replayed_start_returns_the_same_run_the_first_one_reported() {
            let up = upstreams().await;
            mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
            let first = json!({ "run_id": "mp-run-1", "thread_id": "t-1", "plan_id": "p-1" });
            let replay = json!({
                "run_id": "mp-run-1", "thread_id": "t-1", "plan_id": "p-1",
                "idempotent_replay": true
            });
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/runs"))
                .respond_with(ResponseTemplate::new(200).set_body_json(first))
                .up_to_n_times(1)
                .mount(&up.model)
                .await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/runs"))
                .respond_with(ResponseTemplate::new(200).set_body_json(replay))
                .mount(&up.model)
                .await;

            let (_, _, one) = json_of(
                start_run_response(
                    state(&up, BrowserGrantMode::Strict),
                    key_headers(&["retry-1"]),
                    ai_run_body(),
                )
                .await,
            )
            .await;
            let (status, _, two) = json_of(
                start_run_response(
                    state(&up, BrowserGrantMode::Strict),
                    key_headers(&["retry-1"]),
                    ai_run_body(),
                )
                .await,
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            for field in ["run_id", "thread_id", "plan_id"] {
                assert_eq!(one["data"][field], two["data"][field], "{field}");
            }
            assert_eq!(two["data"]["idempotent_replay"], true);
            assert!(one["data"].get("idempotent_replay").is_none());
        }

        #[tokio::test]
        async fn model_gateways_keyed_start_refusals_get_typed_gateway_codes() {
            for (upstream, body, want_status, want_code, retry_after) in [
                (
                    409,
                    json!({ "error": "idempotency_key_reused" }),
                    StatusCode::CONFLICT,
                    "browser_idempotency_key_reused",
                    None,
                ),
                (
                    409,
                    json!({ "error": "idempotency_key_run_ended", "run_id": "mp-run-1" }),
                    StatusCode::CONFLICT,
                    "browser_idempotency_key_run_ended",
                    None,
                ),
                (
                    503,
                    json!({ "error": "idempotency_replay_unverified" }),
                    StatusCode::SERVICE_UNAVAILABLE,
                    "browser_idempotency_unverified",
                    Some("5"),
                ),
            ] {
                let up = upstreams().await;
                mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
                Mock::given(wm_method("POST"))
                    .and(wm_path("/v1/browser/runs"))
                    .respond_with(ResponseTemplate::new(upstream).set_body_json(body.clone()))
                    .mount(&up.model)
                    .await;
                let (status, headers, response) = json_of(
                    start_run_response(
                        state(&up, BrowserGrantMode::Strict),
                        key_headers(&["retry-1"]),
                        ai_run_body(),
                    )
                    .await,
                )
                .await;
                assert_eq!(status, want_status, "{response}");
                assert_eq!(response["error"]["code"], want_code);
                assert_eq!(
                    headers.get("retry-after").and_then(|v| v.to_str().ok()),
                    retry_after
                );
                // The upstream strings are exact JSON values; the gateway's own
                // codes only contain them as a suffix.
                let text = response.to_string();
                for raw in [
                    "\"idempotency_key_reused\"",
                    "\"idempotency_key_run_ended\"",
                    "\"idempotency_replay_unverified\"",
                ] {
                    assert!(!text.contains(raw), "an upstream string leaked: {response}");
                }
            }
            // The run id of an ended run is the person's own, and is offered as a detail.
            let up = upstreams().await;
            mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
            Mock::given(wm_method("POST"))
                .and(wm_path("/v1/browser/runs"))
                .respond_with(ResponseTemplate::new(409).set_body_json(
                    json!({ "error": "idempotency_key_run_ended", "run_id": "mp-run-1" }),
                ))
                .mount(&up.model)
                .await;
            let (_, _, response) = json_of(
                start_run_response(
                    state(&up, BrowserGrantMode::Strict),
                    key_headers(&["retry-1"]),
                    ai_run_body(),
                )
                .await,
            )
            .await;
            assert_eq!(response["error"]["details"]["runId"], "mp-run-1");
        }

        // With explicit hosts a 403 may be a widening refusal, so it is not
        // relabelled as an inactive grant; other failures pass through too.
        #[tokio::test]
        async fn other_ai_run_refusals_are_not_called_an_inactive_grant() {
            for (upstream, code, names_hosts) in [
                (403, "browser grant is not authorized", true),
                (409, "managed browser run already exists", false),
                (400, "browser_grant_hosts_invalid", false),
            ] {
                let up = upstreams().await;
                mount_quarry_session(&up.quarry, owner_projection(Some(GRANT))).await;
                Mock::given(wm_method("POST"))
                    .and(wm_path("/v1/browser/runs"))
                    .respond_with(
                        ResponseTemplate::new(upstream).set_body_json(json!({ "error": code })),
                    )
                    .mount(&up.model)
                    .await;
                let mut body = ai_run_body();
                if !names_hosts {
                    body.allowed_domains = None;
                }
                let (status, response) =
                    start_run_with(state(&up, BrowserGrantMode::Strict), body).await;
                assert_eq!(status.as_u16(), upstream, "{response}");
                assert_ne!(
                    response["error"]["code"],
                    json!("browser_session_grant_inactive"),
                    "{response}"
                );
            }
        }

        #[test]
        fn the_spa_owner_projection_drops_only_the_grant_id() {
            let projection = owner_projection(Some(GRANT));
            let spa = spa_owner_projection(projection.clone());
            assert!(!spa.to_string().contains(GRANT), "{spa}");
            let mut expected = projection;
            expected.as_object_mut().expect("object").remove("grantId");
            assert_eq!(spa, expected);
        }

        #[test]
        fn the_spa_projection_mapping_never_exposes_the_grant_id() {
            let mapped =
                browser_response_from_owner_projection(&owner_projection(Some(GRANT)), None);
            let text = mapped.to_string();
            assert!(!text.contains(GRANT) && !text.contains("grantId"), "{text}");
        }

        #[test]
        fn a_projection_grant_id_must_be_a_plain_path_segment() {
            assert_eq!(
                projection_grant_id(&owner_projection(Some(GRANT))).as_deref(),
                Some(GRANT)
            );
            assert_eq!(
                projection_grant_id(&owner_projection(Some("z_abc123"))).as_deref(),
                Some("z_abc123")
            );
            for bad in ["", "../x", "a/b", "g?x=1"] {
                assert_eq!(
                    projection_grant_id(&owner_projection(Some(bad))),
                    None,
                    "{bad:?}"
                );
            }
            assert_eq!(projection_grant_id(&owner_projection(None)), None);
        }
    }

    pub(crate) fn test_app_state() -> AppState {
        let cache = crate::cache::ResultCache::disabled();
        AppState {
            client: reqwest::Client::new(),
            streaming_client: reqwest::Client::new(),
            internal_api_key: "test-key".into(),
            enforcement_mode: "off".to_string(),
            browser_grant_mode: BrowserGrantMode::Permissive,
            browser_human_browse: false,
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
            anbud_core_url: "http://127.0.0.1:1".into(),
            anbud_api_key: String::new(),
            anbud_api_key_header: "X-API-Key".into(),
            marketing_core_url: "http://127.0.0.1:1".into(),
            marketing_core_service_key: String::new(),
            marketing_core_organization_id: String::new(),
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
            schedule_capabilities: crate::config::ScheduleCapabilities::default(),
            allow_dev_actor_headers: true,
            allow_dev_auth_bypass: true,
            mail_task_ingest_service_token: "mail-task-test-secret-at-least-32-bytes".into(),
            mail_task_rate_limiter: crate::rate_limit::RateLimiter::from_cache_with_rpm(
                &crate::cache::ResultCache::disabled(),
                20.0,
            ),
            mail_task_nats_url: String::new(),
            mail_task_nats_user: String::new(),
            mail_task_nats_password: String::new(),
            continue_park_form: false,
            model_entitlement_admin_enabled: false,
            mail_task_pipeline: None,
            space_features: Default::default(),
        }
    }

    /// The real browser route table, behind a stand-in session layer: the
    /// session-create and AI-run routes share one small per-org budget, and
    /// nothing done inside an open session spends it.
    #[tokio::test]
    async fn browser_start_routes_have_their_own_bucket_and_session_use_is_not_limited() {
        use axum::body::Body;
        use tower::ServiceExt;

        let app = {
            let _env = crate::config::TEST_ENV_LOCK.lock().await;
            std::env::remove_var("GATEWAY_RATE_CLASSES");
            std::env::set_var("GATEWAY_RATE_LIMIT_RPM_BROWSER_START", "1");
            let state = test_app_state();
            let app = classed_routes(&state.rate_limiter)
                .route_layer(axum::middleware::from_fn(
                    |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
                        // What `require_session` does for a member of org_a.
                        request.extensions_mut().insert(AuthenticatedUser {
                            user_id: "user-a".into(),
                            user_email: String::new(),
                            user_name: String::new(),
                            user_image: None,
                            email_verified: true,
                            auth_role: None,
                            active_org_id: Some("org_a".into()),
                            authorized_membership: Some(crate::middleware::AuthorizedMembership {
                                organization_id: "org_a".into(),
                                role: "member".into(),
                            }),
                        });
                        next.run(request).await
                    },
                ))
                .with_state(state);
            std::env::remove_var("GATEWAY_RATE_LIMIT_RPM_BROWSER_START");
            app
        };
        let call = |method: &'static str, uri: &'static str| {
            let app = app.clone();
            async move {
                app.oneshot(
                    axum::http::Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("authorization", "Bearer dev-bypass")
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"url":"https://example.com","goal":"x"}"#))
                        .unwrap(),
                )
                .await
                .unwrap()
            }
        };

        let first = call("POST", "/api/v1/browser/sessions").await;
        assert_ne!(
            first.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "the first create is allowed"
        );
        let second = call("POST", "/api/v1/browser/sessions").await;
        assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(second.headers().contains_key("retry-after"));
        // The AI-run start shares the same start budget.
        let ai_run = call("POST", "/api/v1/browser/sessions/s1/ai-runs").await;
        assert_eq!(ai_run.status(), StatusCode::TOO_MANY_REQUESTS);
        // Everything inside an open session is a different class.
        for (method, uri) in [
            ("GET", "/api/v1/browser/sessions/s1"),
            ("GET", "/api/v1/browser/sessions/s1/frame"),
            ("POST", "/api/v1/browser/sessions/s1/actions"),
            ("DELETE", "/api/v1/browser/sessions/s1"),
            ("POST", "/api/v1/browser/runs/r1/control"),
        ] {
            let response = call(method, uri).await;
            assert_ne!(
                response.status(),
                StatusCode::TOO_MANY_REQUESTS,
                "{method} {uri} must not spend the browser_start budget"
            );
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
    fn sanitize_action_allows_reload_without_extra_keys() {
        let hard = sanitize_action(json!({ "type": "reload", "ignore_cache": true }))
            .expect("valid action");
        assert_eq!(hard, json!({ "type": "reload", "ignore_cache": true }));
        let soft = sanitize_action(json!({ "type": "reload" })).expect("valid action");
        assert_eq!(soft, json!({ "type": "reload", "ignore_cache": false }));
    }

    #[test]
    fn sanitize_action_rejects_reload_with_url() {
        let err = sanitize_action(json!({ "type": "reload", "url": "https://example.com" }))
            .expect_err("reload with a url must be refused");
        assert_eq!(err.0, "invalid_browser_action");
        let typed = sanitize_action(json!({ "type": "reload", "ignore_cache": "yes" }))
            .expect_err("non-boolean ignore_cache must be refused");
        assert_eq!(typed.0, "invalid_browser_action");
    }

    /// D2-012: a verified service principal is the agent whatever it states, a person's
    /// session keeps the stated actor, and a call with no class is a person's.
    #[test]
    fn a_service_principal_is_the_agent_whatever_it_states() {
        let sp = || Some(Extension(CallerClass::ServicePrincipal));
        for stated in [BrowserActionActor::Human, BrowserActionActor::Agent] {
            assert_eq!(effective_actor(sp(), stated), BrowserActionActor::Agent, "{stated:?}");
        }
        for caller in [None, Some(Extension(CallerClass::User))] {
            assert_eq!(
                effective_actor(caller, BrowserActionActor::Human),
                BrowserActionActor::Human
            );
            assert_eq!(
                effective_actor(caller, BrowserActionActor::Agent),
                BrowserActionActor::Agent
            );
        }
    }

    /// D2-012: a service principal stating `human` still cannot reload (WS9.B). It is
    /// refused before any upstream call, so the test needs no Quarry.
    #[tokio::test]
    async fn a_service_principal_stating_human_cannot_reload() {
        let response = run_action(
            State(test_app_state()),
            Extension(test_user("user-owner", "org-owner")),
            Some(Extension(CallerClass::ServicePrincipal)),
            HeaderMap::new(),
            Path("run-1".to_owned()),
            Json(ActionBody {
                action: json!({ "type": "reload" }),
                actor: BrowserActionActor::Human,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// D2-012: during a person's takeover a service principal stating `human` is refused
    /// like any agent, and nothing reaches Quarry (no step, no control change).
    #[tokio::test]
    async fn a_service_principal_stating_human_is_refused_during_takeover() {
        let quarry = wiremock::MockServer::start().await;
        mount_tab_run(&quarry, "human_takeover").await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let response = run_action(
            State(state.clone()),
            Extension(test_user("user-owner", "org-owner")),
            Some(Extension(CallerClass::ServicePrincipal)),
            HeaderMap::new(),
            Path("run-1".to_owned()),
            Json(ActionBody {
                action: json!({ "type": "wait", "ms": 10 }),
                actor: BrowserActionActor::Human,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            call_tab_routes_as(
                &state,
                BrowserActionActor::Human,
                HeaderMap::new(),
                Some(CallerClass::ServicePrincipal)
            )
            .await,
            [StatusCode::CONFLICT; 3]
        );
        assert!(
            quarry_mutations(&quarry).await.is_empty(),
            "a refused call never reaches Quarry"
        );
    }

    /// D2-012: outside a takeover a service principal stating `human` neither flips the
    /// run into takeover nor sends a person's step: Quarry gets the agent's step only.
    #[tokio::test]
    async fn a_service_principal_stating_human_cannot_start_a_takeover() {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, ResponseTemplate,
        };

        let quarry = wiremock::MockServer::start().await;
        mount_tab_run(&quarry, "agent_control").await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/agent/runs/run-1/step"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "ok": true } })))
            .mount(&quarry)
            .await;
        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();

        let response = run_action(
            State(state.clone()),
            Extension(test_user("user-owner", "org-owner")),
            Some(Extension(CallerClass::ServicePrincipal)),
            HeaderMap::new(),
            Path("run-1".to_owned()),
            Json(ActionBody {
                action: json!({ "type": "wait", "ms": 10 }),
                actor: BrowserActionActor::Human,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mutations = quarry_mutations(&quarry).await;
        assert_eq!(mutations.len(), 1, "{mutations:?}");
        assert_eq!(mutations[0].1, "/v1/agent/runs/run-1/step");
        assert_eq!(mutations[0].2["initiated_by"], "agent", "{:?}", mutations[0]);

        let quarry = wiremock::MockServer::start().await;
        mount_tab_run(&quarry, "agent_control").await;
        state.quarry_edge_url = quarry.uri();
        assert_eq!(
            call_tab_routes_as(
                &state,
                BrowserActionActor::Human,
                HeaderMap::new(),
                Some(CallerClass::ServicePrincipal)
            )
            .await,
            [StatusCode::OK; 3]
        );
        let mutations = quarry_mutations(&quarry).await;
        assert_eq!(mutations.len(), 3, "{mutations:?}");
        assert!(mutations[0].1.ends_with("select?initiatedBy=agent"), "{:?}", mutations[0]);
        assert!(mutations[1].1.ends_with("tab-1?initiatedBy=agent"), "{:?}", mutations[1]);
        assert_eq!(mutations[2].2["initiatedBy"], "agent", "{:?}", mutations[2]);
    }

    #[test]
    fn reload_is_refused_for_agent_actor_only() {
        let reload = json!({ "type": "reload", "ignore_cache": true });
        assert!(reload_refused_for_actor(&reload, BrowserActionActor::Agent));
        assert!(!reload_refused_for_actor(&reload, BrowserActionActor::Human));
        let back = json!({ "type": "back" });
        assert!(!reload_refused_for_actor(&back, BrowserActionActor::Agent));
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

        let request = build_ai_run_request(
            "check out the cart",
            "sess_1",
            None,
            &metadata,
            &body,
            None,
        );

        assert_eq!(request["require_approval"], json!(false));
        // Sanity: everything else still forwards as expected.
        assert_eq!(request["goal"], json!("check out the cart"));
        assert_eq!(request["grant_id"], json!("sess_1"));
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
            None,
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

        let request = build_ai_run_request(
            "browse around",
            "sess_3",
            None,
            &metadata,
            &body,
            None,
        );

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

            let request = build_ai_run_request(
                "resume shopping",
                "sess_4",
                None,
                &metadata,
                &body,
                None,
            );

            assert_eq!(
                request["profile_id"],
                json!("prof_real"),
                "scope {scope:?} should forward a genuinely persistent profile id"
            );
        }
    }

    fn ai_run_body(allowed_domains: Option<Vec<String>>) -> StartAiRunBody {
        StartAiRunBody {
            goal: "buy the shoes".to_owned(),
            allowed_domains,
            max_steps: None,
            max_runtime_s: None,
            stop_criteria: None,
            require_approval: None,
            max_cost_usd: None,
        }
    }

    fn ai_run_metadata(zdr: bool) -> BrowserRunMetadata {
        BrowserRunMetadata {
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
            zdr,
        }
    }

    #[test]
    fn browser_ai_run_real_grant_switch_is_on_only_for_the_exact_word_true() {
        assert!(browser_ai_run_real_grant_from(Some("true")));
        for value in [
            None,
            Some(""),
            Some("1"),
            Some("TRUE"),
            Some(" true"),
            Some("true "),
            Some("yes"),
        ] {
            assert!(
                !browser_ai_run_real_grant_from(value),
                "{value:?} must not enable the switch"
            );
        }
    }

    #[test]
    fn ai_run_requested_domains_prefers_the_named_hosts_then_the_current_page() {
        let page = Some("https://shop.example.test/cart");

        assert_eq!(
            ai_run_requested_domains(
                &ai_run_body(Some(vec!["other.example.test".to_owned()])),
                page
            ),
            Some(vec!["other.example.test".to_owned()])
        );
        assert_eq!(
            ai_run_requested_domains(&ai_run_body(Some(Vec::new())), page),
            Some(vec!["shop.example.test".to_owned()])
        );
        assert_eq!(
            ai_run_requested_domains(&ai_run_body(None), page),
            Some(vec!["shop.example.test".to_owned()])
        );
        assert_eq!(ai_run_requested_domains(&ai_run_body(None), None), None);
        assert_eq!(
            ai_run_requested_domains(&ai_run_body(None), Some("about:blank")),
            None
        );
    }

    #[test]
    fn build_ai_run_request_starts_under_the_issued_grant_and_its_policy() {
        let issued = vec!["shop.example.test".to_owned()];

        let request = build_ai_run_request(
            "buy the shoes",
            "grant-issued",
            Some(issued.as_slice()),
            &ai_run_metadata(false),
            &ai_run_body(None),
            Some("https://shop.example.test/cart"),
        );

        assert_eq!(request["grant_id"], json!("grant-issued"));
        assert_eq!(request["allowed_domains"], json!(["shop.example.test"]));
        assert_eq!(
            request["start_url"],
            json!("https://shop.example.test/cart")
        );
    }

    /// The upstreams one `start_ai_run` call reaches: Quarry (the session
    /// projection), auth-core (the plane tokens) and model-gateway, whose grant
    /// and run routes each test mounts. The servers must outlive the call.
    async fn ai_run_upstreams(
        session_id: &str,
        current_url: &str,
        zdr: bool,
    ) -> (
        wiremock::MockServer,
        wiremock::MockServer,
        wiremock::MockServer,
        AppState,
    ) {
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, MockServer, ResponseTemplate,
        };

        let quarry = MockServer::start().await;
        let auth_core = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        for audience in [
            "quarry",
            "model-plane",
            "browser-broker",
            "data-plane",
            "inference-core",
            "execution-core",
            "session-core",
        ] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{audience}/token").as_str()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(json!({ "token": format!("{audience}-token") })),
                )
                .mount(&auth_core)
                .await;
        }
        let session_path = format!("/v1/agent/runs/{session_id}/browser-session");
        Mock::given(wm_method("GET"))
            .and(wm_path(session_path.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {
                    "runId": session_id,
                    "status": "active",
                    "currentUrl": current_url,
                    "zdr": zdr,
                    "profileScope": "ephemeral",
                }
            })))
            .mount(&quarry)
            .await;

        let mut state = test_app_state();
        state.quarry_edge_url = quarry.uri();
        state.auth_core_url = auth_core.uri();
        state.model_gateway_url = model_gateway.uri();
        (quarry, auth_core, model_gateway, state)
    }

    fn request_header<'a>(request: &'a wiremock::Request, name: &str) -> Option<&'a str> {
        request
            .headers
            .get(name)
            .and_then(|value| value.to_str().ok())
    }

    fn request_json(request: &wiremock::Request) -> Value {
        serde_json::from_slice(&request.body).expect("a JSON request body")
    }

    async fn browser_bearer_minted(auth_core: &wiremock::MockServer) -> bool {
        auth_core
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .any(|request| request.url.path() == "/api/browser-broker/token")
    }

    #[tokio::test]
    async fn a_real_grant_run_starts_under_the_issued_grant_with_the_browser_bearer() {
        use axum::body::to_bytes;
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, ResponseTemplate,
        };

        let (_quarry, _auth_core, model_gateway, state) =
            ai_run_upstreams("sess_real", "https://shop.example.test/cart", false).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/browser/grants"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "grant_id": "grant-issued",
                "kind": "session",
                "zdr": false,
                "allowed_domains": ["shop.example.test"],
            })))
            .mount(&model_gateway)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/browser/runs"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "run_id": "run-real",
                "thread_id": "thread-real",
                "plan_id": "plan-real",
            })))
            .mount(&model_gateway)
            .await;

        let response = start_ai_run_with_switch(
            state,
            test_user("user-real", "org-real"),
            HeaderMap::new(),
            "sess_real".to_owned(),
            ai_run_body(None),
            true,
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON response");
        assert_eq!(body["data"]["run_id"], json!("run-real"));

        let received = model_gateway
            .received_requests()
            .await
            .expect("recorded model-gateway requests");
        let [grant, run] = received.as_slice() else {
            panic!(
                "expected a grant request then a run request, got {}",
                received.len()
            );
        };
        assert_eq!(grant.url.path(), "/v1/browser/grants");
        assert_eq!(
            request_header(grant, "authorization"),
            Some("Bearer model-plane-token")
        );
        assert_eq!(
            request_header(grant, "x-browser-authorization"),
            Some("Bearer browser-broker-token")
        );
        assert_eq!(
            request_json(grant)["allowed_domains"],
            json!(["shop.example.test"])
        );
        assert_eq!(run.url.path(), "/v1/browser/runs");
        assert_eq!(
            request_header(run, "x-browser-authorization"),
            Some("Bearer browser-broker-token")
        );
        let run_body = request_json(run);
        assert_eq!(run_body["grant_id"], json!("grant-issued"));
        assert_eq!(run_body["allowed_domains"], json!(["shop.example.test"]));
        assert_eq!(
            run_body["start_url"],
            json!("https://shop.example.test/cart")
        );
    }

    #[tokio::test]
    async fn a_zero_retention_session_never_mints_a_browser_bearer_or_asks_for_a_grant() {
        use axum::body::to_bytes;

        // The session has no grant of its own; with the switch on, a
        // zero-retention session still gets no issued grant and no bearer.
        let (_quarry, auth_core, model_gateway, state) =
            ai_run_upstreams("sess_zdr", "https://shop.example.test/cart", true).await;

        let response = start_ai_run_with_switch(
            state,
            test_user("user-zdr", "org-zdr"),
            HeaderMap::new(),
            "sess_zdr".to_owned(),
            ai_run_body(Some(vec!["shop.example.test".to_owned()])),
            true,
        )
        .await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON response");
        assert_eq!(body["error"]["code"], json!("browser_session_grant_missing"));
        assert!(model_gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
        assert!(!browser_bearer_minted(&auth_core).await);
    }

    #[tokio::test]
    async fn a_grantless_session_with_the_switch_off_is_refused_and_sends_no_session_string() {
        use axum::body::to_bytes;

        let (_quarry, auth_core, model_gateway, state) =
            ai_run_upstreams("sess_off", "https://shop.example.test/cart", false).await;

        let response = start_ai_run_with_switch(
            state,
            test_user("user-off", "org-off"),
            HeaderMap::new(),
            "sess_off".to_owned(),
            ai_run_body(Some(vec!["shop.example.test".to_owned()])),
            false,
        )
        .await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON response");
        assert_eq!(body["error"]["code"], json!("browser_session_grant_missing"));
        assert!(model_gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
        assert!(!browser_bearer_minted(&auth_core).await);
    }

    #[tokio::test]
    async fn a_real_grant_run_with_no_host_to_grant_is_refused_before_any_browser_mint() {
        use axum::body::to_bytes;

        let (_quarry, auth_core, model_gateway, state) =
            ai_run_upstreams("sess_blank", "about:blank", false).await;

        let response = start_ai_run_with_switch(
            state,
            test_user("user-blank", "org-blank"),
            HeaderMap::new(),
            "sess_blank".to_owned(),
            ai_run_body(None),
            true,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON response");
        assert_eq!(body["error"]["code"], json!("invalid_allowed_domains"));
        assert!(model_gateway
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
        assert!(!browser_bearer_minted(&auth_core).await);
    }

    #[tokio::test]
    async fn a_refused_grant_is_answered_as_is_and_no_run_starts() {
        use axum::body::to_bytes;
        use wiremock::{
            matchers::{method as wm_method, path as wm_path},
            Mock, ResponseTemplate,
        };

        let (_quarry, _auth_core, model_gateway, state) =
            ai_run_upstreams("sess_denied", "https://shop.example.test/cart", false).await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/v1/browser/grants"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "error": "browser_grant_forbidden",
            })))
            .mount(&model_gateway)
            .await;

        let response = start_ai_run_with_switch(
            state,
            test_user("user-denied", "org-denied"),
            HeaderMap::new(),
            "sess_denied".to_owned(),
            ai_run_body(None),
            true,
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        let body: Value = serde_json::from_slice(&bytes).expect("JSON response");
        assert_eq!(body["error"], json!("browser_grant_forbidden"));
        let received = model_gateway
            .received_requests()
            .await
            .expect("recorded model-gateway requests");
        let [grant] = received.as_slice() else {
            panic!("expected only the grant request, got {}", received.len());
        };
        assert_eq!(grant.url.path(), "/v1/browser/grants");
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

#[cfg(test)]
mod human_browse_session_tests {
    use super::*;

    // The HTTP path (create_session, keepalive_session) needs a live broker, model gateway
    // and Quarry; these tests pin the pure decisions those handlers make. The wire-level
    // legs are the Playwright and stack evidence of WS8.D, not run in the build.

    #[test]
    fn create_session_requests_human_browse_grant_and_omits_budget_literals() {
        let grant = session_grant_request_body(SessionGrantScope::HumanBrowse, false)
            .expect("a human_browse grant has a body");
        assert_eq!(grant, json!({ "kind": "human_browse", "zdr": false }));
        assert!(grant.get("allowed_domains").is_none(), "{grant}");
        // With a human grant the start body carries no constraints at all: no max_steps,
        // no max_runtime_s and no allowed_domains.
        assert_eq!(session_start_constraints(true, Some("example.com")), None);
    }

    #[test]
    fn create_session_off_keeps_legacy_first_host_policy() {
        assert_eq!(
            session_grant_request_body(SessionGrantScope::StartHost(Some("example.com")), true),
            Some(json!({ "allowed_domains": ["example.com"], "zdr": true }))
        );
        assert_eq!(
            session_grant_request_body(SessionGrantScope::StartHost(None), false),
            None
        );
        assert_eq!(
            session_start_constraints(false, Some("example.com")),
            Some(json!({
                "max_steps": 12,
                "allowed_domains": ["example.com"],
                "max_runtime_s": 120
            }))
        );
    }

    #[test]
    fn keepalive_requires_owner_and_returns_expiry_only() {
        let renewed = json!({
            "expires_at": "2030-01-01T00:00:00Z",
            "absolute_expires_at": "2030-01-01T04:00:00Z",
            "grant_id": "g_secret"
        });
        let payload = keepalive_outcome(StatusCode::OK, &renewed).expect("a renewed grant");
        assert_eq!(payload, json!({ "expiresAt": "2030-01-01T00:00:00Z" }));
        assert!(!payload.to_string().contains("g_secret"), "{payload}");
    }

    #[test]
    fn keepalive_maps_not_renewable_to_409() {
        for status in [
            StatusCode::CONFLICT,
            StatusCode::NOT_FOUND,
            StatusCode::FORBIDDEN,
        ] {
            assert_eq!(
                keepalive_outcome(status, &json!({ "error": "browser_grant_not_renewable" })),
                Err((StatusCode::CONFLICT, "browser_session_expired")),
                "{status}"
            );
        }
        assert_eq!(
            keepalive_outcome(StatusCode::BAD_GATEWAY, &json!({})),
            Err((StatusCode::SERVICE_UNAVAILABLE, "browser_grant_unavailable"))
        );
    }

    #[test]
    fn projection_never_exposes_grant_id() {
        let projection = json!({
            "runId": "run-1",
            "grantId": "g_secret",
            "grantKind": "human_browse",
            "tabControl": true,
            "attachAvailable": false,
            "status": "live",
            "live": true
        });
        let response = browser_response_from_owner_projection(&projection, None);
        assert!(!response.to_string().contains("g_secret"), "{response}");
        assert!(response.pointer("/session/grantId").is_none());
        let capabilities = response["session"]["capabilities"]
            .as_array()
            .expect("capabilities array");
        assert!(capabilities.iter().any(|c| c == "human_browsing"));
        assert!(capabilities.iter().any(|c| c == "tab_control"));
        assert!(!capabilities.iter().any(|c| c == "attach"));
    }

    #[test]
    fn human_capabilities_are_not_advertised_without_quarry_saying_so() {
        let caps = browser_session_capabilities(&json!({ "runId": "run-1" }));
        assert!(!caps.contains(&"human_browsing"));
        assert!(!caps.contains(&"tab_control"));
        assert!(!caps.contains(&"attach"));
    }
}

#[cfg(test)]
mod tab_opened_by_tests {
    use super::*;

    fn agent_opener() -> Value {
        json!({
            "initiatedBy": "agent",
            "agent": { "basis": "user_session", "aiRunId": "run_01J9", "label": "planner" }
        })
    }

    #[test]
    fn the_marker_is_the_agent_or_the_person_and_nothing_else() {
        assert_eq!(opened_by_marker(Some(&agent_opener())), Some("agent"));
        assert_eq!(
            opened_by_marker(Some(&json!({ "initiatedBy": "agent", "unattributed": true }))),
            Some("agent")
        );
        assert_eq!(
            opened_by_marker(Some(&json!({ "initiatedBy": "human" }))),
            Some("human")
        );
        assert_eq!(
            opened_by_marker(Some(&json!({ "initiatedBy": "system" }))),
            None
        );
        assert_eq!(opened_by_marker(Some(&json!("agent"))), None);
        assert_eq!(opened_by_marker(Some(&Value::Null)), None);
        assert_eq!(opened_by_marker(None), None);
    }

    #[test]
    fn a_listed_tab_takes_its_opener_from_the_projection_and_keeps_its_own_fields() {
        let projection = json!({
            "tabs": [
                { "tabId": "tab-1", "active": false, "openedBy": agent_opener() }
            ]
        });
        let listed = json!({
            "tabId": "tab-1",
            "title": "Plan",
            "url": "https://example.com/",
            "active": true
        });
        assert_eq!(
            tab_with_opened_by(listed, &projection),
            json!({
                "tabId": "tab-1",
                "title": "Plan",
                "url": "https://example.com/",
                "active": true,
                "openedBy": "agent"
            })
        );
    }

    #[test]
    fn a_tab_with_no_known_opener_has_no_marker() {
        let projection = json!({
            "tabs": [
                { "tabId": "tab-1", "active": false, "openedBy": { "initiatedBy": "system" } }
            ]
        });
        let system = tab_with_opened_by(
            json!({ "tabId": "tab-1", "active": false, "openedBy": { "initiatedBy": "system" } }),
            &projection,
        );
        assert!(system.get("openedBy").is_none(), "{system}");
        let browser = tab_with_opened_by(json!({ "tabId": "tab-9", "active": false }), &projection);
        assert!(browser.get("openedBy").is_none(), "{browser}");
        let unprojected = tabs_with_opened_by(
            vec![json!({ "tabId": "tab-1", "active": false })],
            &json!({}),
        );
        assert!(unprojected[0].get("openedBy").is_none(), "{unprojected:?}");
    }

    #[test]
    fn the_session_tabs_carry_the_marker_the_spa_reads_and_no_agent_ids() {
        let projection = json!({
            "runId": "run-1",
            "controlMode": "agent_control",
            "tabs": [
                { "tabId": "tab-1", "title": null, "url": null, "active": true },
                { "tabId": "tab-2", "title": null, "url": null, "active": false, "openedBy": agent_opener() },
                { "tabId": "tab-3", "title": null, "url": null, "active": false, "openedBy": { "initiatedBy": "human" } }
            ]
        });
        let response = browser_response_from_owner_projection(&projection, None);
        let tabs = &response["session"]["tabs"];
        assert!(tabs[0].get("openedBy").is_none(), "{response}");
        assert_eq!(tabs[1]["openedBy"], "agent", "{response}");
        assert_eq!(tabs[2]["openedBy"], "human", "{response}");
        assert!(!response.to_string().contains("run_01J9"), "{response}");
    }
}
