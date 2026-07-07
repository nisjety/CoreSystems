//! Browser sessions for the Velion in-app browser surface.
//!
//! This is a narrow facade over Quarry-v2's existing Rust browser-agent lane:
//! `/v1/agent/runs` acquires a chromiumoxide-backed run, and `/step` executes
//! one browser action at a time. The SPA never sees raw CDP or Quarry tokens.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use axum::{
    body::Body,
    extract::{Extension, Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json},
    envelope::{error, ok, unwrap_data},
    middleware::{require_session, AuthenticatedUser},
    public_url::normalize_public_http_url,
    upstream::proxy_bearer_json,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateSessionBody {
    url: String,
    #[serde(default)]
    profile_id: Option<String>,
    #[serde(default)]
    persistent_profile: bool,
    #[serde(default)]
    viewport: Option<Viewport>,
}

pub(crate) type BrowserRunStore = Arc<StdMutex<HashMap<String, BrowserRunMetadata>>>;

#[derive(Debug, Clone)]
pub(crate) struct BrowserRunMetadata {
    lease_id: Option<String>,
    profile_id: Option<String>,
    persistent_profile: bool,
    last_observation: Option<Value>,
    observation_history: Vec<Value>,
    viewport: Viewport,
    zdr: bool,
}

pub(crate) fn new_browser_run_store() -> BrowserRunStore {
    Arc::new(StdMutex::new(HashMap::new()))
}

#[derive(Debug, Deserialize, Clone, Copy)]
struct Viewport {
    width: u32,
    height: u32,
}

#[derive(Debug, Deserialize)]
struct ActionBody {
    action: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SuggestActionBody {
    #[serde(default)]
    goal: String,
    #[serde(default = "default_include_screenshot")]
    include_screenshot: bool,
}

#[derive(Debug, Deserialize)]
struct RestoreProbeBody {
    url: String,
}

const DEFAULT_VIEWPORT: Viewport = Viewport {
    width: 1280,
    height: 800,
};
const MAX_BROWSER_ARTIFACT_BYTES: u64 = 24 * 1024 * 1024;
const MAX_MODEL_SCREENSHOT_BYTES: u64 = 6 * 1024 * 1024;
const MAX_MODEL_VISUAL_JSON_BYTES: u64 = 1024 * 1024;
const MAX_BROWSER_TIMELINE_ENTRIES: usize = 32;
const MAX_TIMELINE_CONSOLE_ENTRIES: usize = 20;
const MAX_TIMELINE_NETWORK_ENTRIES: usize = 30;
const MAX_TIMELINE_POLICY_DENIALS: usize = 10;

fn default_include_screenshot() -> bool {
    true
}

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/browser/sessions", post(create_session))
        .route(
            "/api/v1/browser/sessions/:session_id/actions",
            post(run_action),
        )
        .route(
            "/api/v1/browser/sessions/:session_id/suggestions",
            post(suggest_action),
        )
        .route(
            "/api/v1/browser/sessions/:session_id/artifacts/:artifact_id",
            get(get_artifact),
        )
        .route(
            "/api/v1/browser/sessions/:session_id",
            delete(close_session),
        )
        .route("/api/v1/browser/profiles", get(list_profiles))
        .route(
            "/api/v1/browser/profiles/:profile_id/restore-probe",
            post(restore_profile_probe),
        )
        .route(
            "/api/v1/browser/profiles/:profile_id",
            delete(delete_profile),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

async fn create_session(
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
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let allowed_domain = hostname(&target);

    // ZDR sessions are a Phase-3 concern; the flag is threaded through metadata
    // so the SPA renders persistence state from data instead of assuming it.
    let zdr = false;
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
    if let Some(profile_id) = profile_id {
        start_body["profile_id"] = Value::String(profile_id.to_owned());
    }
    let requested_persistent_profile = body.persistent_profile || profile_id.is_some();
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
    let lease_id = str_field(&start_data, "lease_id");
    let returned_profile_id =
        str_field(&start_data, "profile_id").or_else(|| profile_id.map(str::to_owned));
    let step_body = json!({
        "action": {
            "type": "navigate",
            "url": target
        },
        "instruction": "Open the page for the Velion in-app browser surface."
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
    let metadata = BrowserRunMetadata {
        lease_id,
        profile_id: returned_profile_id,
        persistent_profile: requested_persistent_profile,
        last_observation: Some(observation.clone()),
        observation_history: vec![observation.clone()],
        viewport,
        zdr,
    };
    if let Ok(mut runs) = state.browser_run_store.lock() {
        runs.insert(run_id.clone(), metadata.clone());
    }

    let response = browser_response(&run_id, &metadata, Some(observation));
    (StatusCode::OK, Json(ok(response))).into_response()
}

async fn run_action(
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
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
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
    let metadata = update_browser_observation(&state, &session_id, observation.clone());
    let response = browser_response(&session_id, &metadata, Some(observation));
    (StatusCode::OK, Json(ok(response))).into_response()
}

async fn close_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> Response {
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
    if let Ok(mut runs) = state.browser_run_store.lock() {
        runs.remove(&session_id);
    }
    (StatusCode::OK, Json(ok(json!({ "closed": true })))).into_response()
}

async fn suggest_action(
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

    let metadata = browser_run_metadata(&state, &session_id);
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

async fn get_artifact(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((session_id, artifact_id)): Path<(String, String)>,
) -> Response {
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
                Err(err) => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(error("upstream_unavailable", err.to_string())),
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
        Err(err) => (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", err.to_string())),
        )
            .into_response(),
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

async fn restore_profile_probe(
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

async fn delete_profile(
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
                "scope": if metadata.persistent_profile { "user_private" } else { "run_scoped" },
                "storage": if metadata.persistent_profile { "persistent" } else { "isolated" }
            },
            "frame": frame,
            "visual": visual,
            "timeline": browser_timeline(run_id, &metadata.observation_history),
            "zdr": metadata.zdr,
            "capabilities": ["navigate", "back", "forward", "click", "type", "press", "scroll", "wait_for", "select", "inspect_dom", "screenshot_artifact", "visual_observation", "visual_change", "annotate", "persistent_profile"]
        },
        "observation": observation
    })
}

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

fn update_browser_observation(
    state: &AppState,
    session_id: &str,
    observation: Value,
) -> BrowserRunMetadata {
    let mut metadata = browser_run_metadata(state, session_id);
    metadata.last_observation = Some(observation.clone());
    metadata.observation_history.push(observation);
    if metadata.observation_history.len() > MAX_BROWSER_TIMELINE_ENTRIES {
        let remove_count = metadata.observation_history.len() - MAX_BROWSER_TIMELINE_ENTRIES;
        metadata.observation_history.drain(0..remove_count);
    }
    if let Ok(mut runs) = state.browser_run_store.lock() {
        runs.insert(session_id.to_owned(), metadata.clone());
    }
    metadata
}

fn browser_run_metadata(state: &AppState, session_id: &str) -> BrowserRunMetadata {
    state
        .browser_run_store
        .lock()
        .ok()
        .and_then(|runs| runs.get(session_id).cloned())
        .unwrap_or(BrowserRunMetadata {
            lease_id: None,
            profile_id: None,
            persistent_profile: false,
            last_observation: None,
            observation_history: Vec::new(),
            viewport: DEFAULT_VIEWPORT,
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

    let upstream = req.send().await.map_err(|err| {
        (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", err.to_string())),
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

    let bytes = upstream.bytes().await.map_err(|err| {
        (
            StatusCode::BAD_GATEWAY,
            Json(error("upstream_unavailable", err.to_string())),
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
            "Raw browser script evaluation is not exposed through the Velion UI facade.".to_owned(),
        )),
        _ => Err((
            "invalid_browser_action",
            "Unsupported browser action.".to_owned(),
        )),
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
    get_audience_token(state, &user.user_id, cookie, "quarry")
        .await
        .or_else(|| dev_quarry_token(state))
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
    fn browser_response_includes_scoped_screenshot_frame_url() {
        let metadata = BrowserRunMetadata {
            lease_id: Some("lease-1".to_owned()),
            profile_id: None,
            persistent_profile: false,
            last_observation: None,
            observation_history: vec![json!({
                "step": 0,
                "url": "https://example.com",
                "title": "Example",
                "screenshot_artifact_id": "art_01JZ9XM7EXAMPLESHOT00001"
            })],
            viewport: DEFAULT_VIEWPORT,
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
    }

    #[test]
    fn browser_response_includes_visual_observation_artifact_url() {
        let metadata = BrowserRunMetadata {
            lease_id: None,
            profile_id: None,
            persistent_profile: false,
            last_observation: None,
            observation_history: vec![json!({
                "step": 0,
                "url": "https://example.com",
                "visual_observation_artifact_id": "art_01JZ9XM7EXAMPLEVISION0001"
            })],
            viewport: DEFAULT_VIEWPORT,
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
    fn browser_timeline_entries_carry_bounded_step_evidence() {
        let console: Vec<Value> = (0..30)
            .map(|index| json!({ "level": "info", "text": format!("line {index}") }))
            .collect();
        let metadata = BrowserRunMetadata {
            lease_id: None,
            profile_id: None,
            persistent_profile: false,
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
            viewport: DEFAULT_VIEWPORT,
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
