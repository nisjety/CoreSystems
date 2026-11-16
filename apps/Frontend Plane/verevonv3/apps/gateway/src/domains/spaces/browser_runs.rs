//! Room browser run reads: the BFF half of WS20-S4 (R2 to R4).
//!
//! `GET /api/v1/spaces/{space_ref}/browser-runs` lists the Space's browser runs, newest first.
//! `GET /api/v1/spaces/{space_ref}/browser-runs/{run_id}/timeline` reads one run's room timeline. `GET
//! /api/v1/spaces/{space_ref}/browser-runs/{run_id}/evidence/{evidence_id}` returns one evidence image.
//! Session Core owns every decision: the Control `model.thread.read` verdict, the removed-member table, the audience
//! ceiling and the content-free projection. model-gateway relays them. This module decides nothing and stores nothing.
//! It checks the flag, the caller's org, the Space reference, the query shape, the zero-retention posture of the
//! request and the org (412 before any Control call), the Space lifecycle and the caller's Control read decision. It
//! then forwards with the caller's own bearers and reduces every answer to the allowed fields. A malformed upstream
//! answer is a 502, never passed through.
//!
//! SD-23: a room member sees receipts only, so a row carries ids, the starter's subject id, the phase, counts, hosts
//! and the closed fields of a timeline row. A URL, title, rationale, selector, frame or decided-by subject is not in
//! any shape this module accepts. Evidence is one sandboxed image, never cached and never sniffed.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_space` / `invalid_limit` / `invalid_cursor` /
//! `invalid_run`, 403 `active_membership_required` / `space_read_not_authorized`, 404 `feature_unavailable` /
//! `space_not_found` / `run_not_found` / `evidence_not_found`, 412 `zdr_feature_not_supported` (feature
//! `browser_run`), 413 `evidence_too_large`, 415 `evidence_unsupported`, 502 `invalid_model_gateway_response`, 503
//! `zdr_decision_unavailable` / `space_lifecycle_unavailable` / `delegated_auth_unavailable` / `session_unavailable`
//! / `evidence_unavailable`. The decision token rides the upstream query string only, as `room_artifacts` carries
//! it. It is never logged here and never appears in a response body.

use std::time::Duration;

use axum::{
    extract::{Extension, Path, Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref};
use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
    zdr::{self, OrgZdrPosture},
};

const DEFAULT_LIST_LIMIT: u32 = 20;
const MAX_LIST_LIMIT: u32 = 50;
const DEFAULT_TIMELINE_LIMIT: u32 = 50;
const MAX_TIMELINE_LIMIT: u32 = 200;
const MAX_SPACE_REF_LEN: usize = 200;
const MAX_ID_LEN: usize = 128;
const MAX_ROW_HOSTS: usize = 8;
/// A run has at most a few dozen actions (R1 caps `max_steps` at 40); this is a sanity bound, not a cap.
const MAX_STEP_COUNT: u64 = 10_000;
/// The largest evidence image relayed (R4: 4 MB).
const MAX_EVIDENCE_BYTES: usize = 4 * 1024 * 1024;
/// How long one evidence fetch may take before the route answers 503.
const EVIDENCE_TIMEOUT: Duration = Duration::from_secs(10);
/// The image types an evidence response may carry. Anything else is refused before it leaves the gateway.
const EVIDENCE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp"];
/// The room timeline's closed event kinds. A kind outside this list is dropped, never shown.
const EVENT_KINDS: &[&str] = &[
    "action",
    "observation",
    "paused",
    "resumed",
    "approval_required",
    "approval_decided",
    "handoff_requested",
    "handoff_resolved",
    "result",
];
/// The optional fields a timeline row may carry, in the order the closed contract names them.
const EVENT_FIELDS: &[&str] = &[
    "step",
    "action_type",
    "host",
    "status",
    "risk_category",
    "evidence_id",
    "approval_id",
    "decision",
    "handoff_kind",
    "outcome",
];
/// The control modes a room run may report. Only `agent` exists until the Quarry lease (WS20-S6) lands.
const CONTROL_MODES: &[&str] = &["agent"];
/// The view scopes a room run may report. Only `owner` exists until the room view (SD-23) is built.
const VIEW_SCOPES: &[&str] = &["owner"];
/// Model-gateway refusals the evidence route relays, as (code, status, fixed message). Anything else is 503.
const EVIDENCE_RELAYED: &[(&str, u16, &str)] = &[
    (
        "space_read_not_authorized",
        403,
        "Reading this Space's browser runs is not authorized.",
    ),
    ("feature_unavailable", 404, "Room browser runs are not enabled."),
    ("evidence_not_found", 404, "The evidence is not available."),
    ("run_not_found", 404, "The browser run is not available."),
    ("evidence_too_large", 413, "The evidence is too large."),
    ("evidence_unsupported", 415, "This evidence cannot be shown."),
    ("evidence_unavailable", 503, "The evidence is temporarily unavailable."),
    ("session_unavailable", 503, "The Space's browser runs could not be read."),
];

/// Registered through `module_routes()` behind `require_session`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/spaces/{space_ref}/browser-runs", get(list_room_browser_runs))
        .route(
            "/api/v1/spaces/{space_ref}/browser-runs/{run_id}/timeline",
            get(read_room_browser_timeline),
        )
        .route(
            "/api/v1/spaces/{space_ref}/browser-runs/{run_id}/evidence/{evidence_id}",
            get(read_room_browser_evidence),
        )
}

#[derive(Debug, Default, Deserialize)]
struct ListRunsParams {
    thread_id: Option<String>,
    limit: Option<String>,
    cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TimelineParams {
    after: Option<String>,
    limit: Option<String>,
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}

fn bad_upstream() -> Response {
    refuse(
        StatusCode::BAD_GATEWAY,
        "invalid_model_gateway_response",
        "Model Gateway returned an invalid browser run answer.",
    )
}

/// The typed zero-retention refusal of a browser run: the catalogue shape, 412, before any Control call.
fn zdr_browser_refusal() -> Response {
    (
        StatusCode::PRECONDITION_FAILED,
        Json(json!({
            "error": {
                "code": "zdr_feature_not_supported",
                "message": "This is not supported with zero data retention yet: browser runs. The request was refused before any model call.",
                "retryable": false,
                "action": "turn_off_feature",
                "feature": "browser_run",
                "details": {},
            }
        })),
    )
        .into_response()
}

/// Zero Data Retention fails closed before any Control call: the request's own flag, then the org's posture. A
/// failed lookup is 503, never "not zero retention".
async fn zdr_gate(state: &AppState, user: &AuthenticatedUser, headers: &HeaderMap) -> Option<Response> {
    if zdr::request_zdr(headers, None) {
        return Some(zdr_browser_refusal());
    }
    match zdr::org_zdr_lookup(state, user).await {
        OrgZdrPosture::Disabled => None,
        OrgZdrPosture::Enabled => Some(zdr_browser_refusal()),
        OrgZdrPosture::Unknown => Some(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(zdr::zdr_refusal_body(zdr::ZDR_DECISION_UNAVAILABLE)),
            )
                .into_response(),
        ),
    }
}

fn valid_space_ref(space_ref: &str) -> bool {
    !space_ref.is_empty()
        && space_ref.len() <= MAX_SPACE_REF_LEN
        && !space_ref.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

/// An id as the room sees it: path-safe, bounded, and free of anything a URL or a log line could misread.
fn valid_id(value: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

/// A closed word (a phase, a status, a kind of hand-off): ASCII letters, digits and underscores, bounded.
fn valid_word(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// A bare hostname: lowercase, bounded, and only hostname bytes. An IP literal and anything with a path, port or
/// userinfo is refused.
fn valid_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'.'))
}

/// A page size from its query text: absent is the default, and a value outside `1..=most` is refused.
fn page_limit(raw: Option<&str>, default: u32, most: u32) -> Result<u32, Response> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(default),
        Some(value) => match value.parse::<u32>() {
            Ok(limit) if (1..=most).contains(&limit) => Ok(limit),
            _ => Err(refuse(
                StatusCode::BAD_REQUEST,
                "invalid_limit",
                "limit is out of range.",
            )),
        },
    }
}

/// The RFC 3339 form of a Unix time in seconds, or `None` for a value chrono cannot represent.
fn rfc3339(unix: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp(unix, 0).map(|at| at.to_rfc3339())
}

fn string_of<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

fn checked_id<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    string_of(object, key).filter(|value| valid_id(value))
}

/// The hosts of a row: at most [`MAX_ROW_HOSTS`], each a bare hostname. Any other shape is malformed.
fn hosts_of(value: &Value) -> Option<Vec<String>> {
    let items = value.as_array()?;
    if items.len() > MAX_ROW_HOSTS {
        return None;
    }
    items
        .iter()
        .map(|host| host.as_str().filter(|host| valid_host(host)).map(str::to_owned))
        .collect()
}

/// The header fields every room run shows: ids, the starter, the phase, the times, and the fixed control, hand-off and
/// view states. `None` is a malformed answer.
fn run_header(object: &Map<String, Value>) -> Option<Map<String, Value>> {
    let run_id = checked_id(object, "run_id")?;
    let thread_id = checked_id(object, "thread_id")?;
    let started_by = object.get("started_by")?.as_object()?;
    let subject_id = checked_id(started_by, "subject_id")?;
    let phase = string_of(object, "phase").filter(|phase| valid_word(phase, 32))?;
    let started_at = rfc3339(object.get("started_at_unix")?.as_i64()?)?;
    let finished_at = match object.get("finished_at_unix") {
        None | Some(Value::Null) => Value::Null,
        Some(value) => json!(rfc3339(value.as_i64()?)?),
    };
    let control = object.get("control")?.as_object()?;
    let mode = string_of(control, "mode").filter(|mode| CONTROL_MODES.contains(mode))?;
    let handoff = object.get("handoff")?.as_object()?;
    let open = handoff.get("open")?.as_bool()?;
    let view = object.get("view")?.as_object()?;
    let scope = string_of(view, "scope").filter(|scope| VIEW_SCOPES.contains(scope))?;
    let mut out = Map::new();
    out.insert("run_id".to_owned(), json!(run_id));
    out.insert("thread_id".to_owned(), json!(thread_id));
    out.insert("started_by".to_owned(), json!({ "subject_id": subject_id }));
    out.insert("phase".to_owned(), json!(phase));
    out.insert("started_at".to_owned(), json!(started_at));
    out.insert("finished_at".to_owned(), finished_at);
    out.insert("control".to_owned(), json!({ "mode": mode }));
    out.insert("handoff".to_owned(), json!({ "open": open }));
    out.insert("view".to_owned(), json!({ "scope": scope }));
    Some(out)
}

/// One list row: the header plus the counts and the hosts.
fn run_row(item: &Value) -> Option<Value> {
    let object = item.as_object()?;
    let mut row = run_header(object)?;
    let hosts = hosts_of(object.get("hosts")?)?;
    let step_count = object
        .get("step_count")?
        .as_u64()
        .filter(|count| *count <= MAX_STEP_COUNT)?;
    row.insert("hosts".to_owned(), json!(hosts));
    row.insert("step_count".to_owned(), json!(step_count));
    Some(Value::Object(row))
}

/// One timeline row in the closed shape. An unknown kind is skipped (`Ok(None)`). A row without a valid event id, time
/// or kind is a contract break (`Err`). A malformed optional field is dropped from that row and never shown, so one
/// odd reference cannot blank a whole page.
fn event_row(item: &Value) -> Result<Option<Value>, ()> {
    let Some(object) = item.as_object() else {
        return Err(());
    };
    let Some(kind) = string_of(object, "kind") else {
        return Err(());
    };
    if !EVENT_KINDS.contains(&kind) {
        return Ok(None);
    }
    let event_id = checked_id(object, "event_id").ok_or(())?;
    let at = rfc3339(
        object
            .get("at_unix")
            .and_then(Value::as_i64)
            .ok_or(())?,
    )
    .ok_or(())?;
    let mut out = Map::new();
    out.insert("event_id".to_owned(), json!(event_id));
    out.insert("at".to_owned(), json!(at));
    out.insert("kind".to_owned(), json!(kind));
    for key in EVENT_FIELDS {
        let Some(value) = string_of(object, key).filter(|value| !value.is_empty()) else {
            continue;
        };
        let well_formed = match *key {
            "host" => valid_host(value),
            "step" | "evidence_id" | "approval_id" => valid_id(value),
            _ => valid_word(value, 64),
        };
        if well_formed {
            out.insert((*key).to_owned(), json!(value));
        }
    }
    Ok(Some(Value::Object(out)))
}

/// What a room read needs before it forwards: the org, the Control read decision and the bearers. Each refusal is a
/// finished response.
struct RoomReadAuth {
    org_id: String,
    decision_ref: String,
    decision_token: String,
    session_token: String,
    model_token: Option<String>,
}

/// The gate every room read passes, in order: the flag, the org, the Space reference, the zero-retention posture,
/// the Space lifecycle, the Control read decision and the bearers.
async fn room_read_auth(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
) -> Result<RoomReadAuth, Response> {
    if !state.space_features.browser_run {
        return Err(refuse(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Room browser runs are not enabled.",
        ));
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(refuse(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to read Space browser runs.",
        ));
    }
    if !valid_space_ref(space_ref) {
        return Err(refuse(
            StatusCode::BAD_REQUEST,
            "invalid_space",
            "A Space reference is required.",
        ));
    }
    if let Some(refusal) = zdr_gate(state, user, headers).await {
        return Err(refusal);
    }
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Err(refuse(
                StatusCode::NOT_FOUND,
                "space_not_found",
                "Space is not available.",
            ))
        }
        Err(()) => {
            return Err(refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            ))
        }
    }
    let (read, read_denial) =
        match shared_thread_read_decision_for_typed(state, user, &org_id, space_ref, "").await {
            Ok(decision) => decision.split(),
            Err(response) => return Err(response.into_response()),
        };
    let message = "Reading this Space's browser runs is not authorized.";
    let Some((decision_ref, decision_token)) = read else {
        return Err(match read_denial {
            Some(denial) => (
                StatusCode::FORBIDDEN,
                Json(error_with_details(
                    denial::OP_READ,
                    message,
                    denial.details_value(),
                )),
            )
                .into_response(),
            None => refuse(StatusCode::FORBIDDEN, denial::OP_READ, message),
        });
    };
    let session_token =
        match crate::domains::chat::shared::required_session_token(state, user, headers).await {
            Ok(token) => token,
            Err(reason) => {
                return Err(
                    crate::domains::chat::shared::delegated_auth_unavailable(reason).into_response(),
                )
            }
        };
    let model_token = crate::domains::chat::shared::model_token(state, user, headers).await;
    Ok(RoomReadAuth {
        org_id,
        decision_ref,
        decision_token,
        session_token,
        model_token,
    })
}

/// `GET /api/v1/spaces/{space_ref}/browser-runs` (R2): the Space's browser runs, newest first.
async fn list_room_browser_runs(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    Query(params): Query<ListRunsParams>,
) -> Response {
    let space_ref = space_ref.trim().to_owned();
    let auth = match room_read_auth(&state, &user, &headers, &space_ref).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let limit = match page_limit(params.limit.as_deref(), DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let thread_id = params.thread_id.unwrap_or_default().trim().to_owned();
    let cursor = params.cursor.unwrap_or_default().trim().to_owned();
    if (!thread_id.is_empty() && !valid_id(&thread_id)) || (!cursor.is_empty() && !valid_id(&cursor)) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "The cursor is not valid.",
        );
    }
    let mut url = format!(
        "{}/v1/spaces/{}/browser/runs?limit={limit}&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(&space_ref),
        urlencoding::encode(&auth.decision_ref),
        urlencoding::encode(&auth.decision_token),
    );
    if !thread_id.is_empty() {
        url.push_str("&thread_id=");
        url.push_str(&urlencoding::encode(&thread_id));
    }
    if !cursor.is_empty() {
        url.push_str("&cursor=");
        url.push_str(&urlencoding::encode(&cursor));
    }
    let (status, Json(payload)) = crate::domains::chat::shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        auth.model_token.as_deref(),
        Some(&auth.session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        // model-gateway's typed envelope (401, 403 `space_read_not_authorized`, 412 `zdr_feature_not_supported`, 404,
        // 503 `session_unavailable`) as is; it never echoes the decision.
        return (status, Json(payload)).into_response();
    }
    let Some(items) = payload.get("runs").and_then(Value::as_array) else {
        return bad_upstream();
    };
    if items.len() > limit as usize {
        return bad_upstream();
    }
    let mut runs = Vec::with_capacity(items.len());
    for item in items {
        match run_row(item) {
            Some(row) => runs.push(row),
            None => return bad_upstream(),
        }
    }
    let next_cursor = match payload.get("next_cursor") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(cursor)) if cursor.is_empty() => Value::Null,
        Some(Value::String(cursor)) if valid_id(cursor) => json!(cursor),
        Some(_) => return bad_upstream(),
    };
    let mut response = (
        StatusCode::OK,
        Json(json!({ "data": runs, "meta": { "next_cursor": next_cursor } })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `GET /api/v1/spaces/{space_ref}/browser-runs/{run_id}/timeline` (R3): one page of the run's room timeline.
async fn read_room_browser_timeline(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, run_id)): Path<(String, String)>,
    Query(params): Query<TimelineParams>,
) -> Response {
    let space_ref = space_ref.trim().to_owned();
    let run_id = run_id.trim().to_owned();
    if !valid_id(&run_id) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_run",
            "A browser run reference is required.",
        );
    }
    let auth = match room_read_auth(&state, &user, &headers, &space_ref).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let limit = match page_limit(params.limit.as_deref(), DEFAULT_TIMELINE_LIMIT, MAX_TIMELINE_LIMIT) {
        Ok(limit) => limit,
        Err(response) => return response,
    };
    let after = params.after.unwrap_or_default().trim().to_owned();
    if !after.is_empty() && !valid_id(&after) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_cursor",
            "The cursor is not valid.",
        );
    }
    let mut url = format!(
        "{}/v1/spaces/{}/browser/runs/{}/timeline?limit={limit}&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(&space_ref),
        urlencoding::encode(&run_id),
        urlencoding::encode(&auth.decision_ref),
        urlencoding::encode(&auth.decision_token),
    );
    if !after.is_empty() {
        url.push_str("&after=");
        url.push_str(&urlencoding::encode(&after));
    }
    let (status, Json(payload)) = crate::domains::chat::shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        auth.model_token.as_deref(),
        Some(&auth.session_token),
        &user,
    )
    .await;
    if !status.is_success() {
        return (status, Json(payload)).into_response();
    }
    let Some(run) = payload.get("run").and_then(Value::as_object).and_then(run_header) else {
        return bad_upstream();
    };
    let Some(items) = payload.get("events").and_then(Value::as_array) else {
        return bad_upstream();
    };
    if items.len() > limit as usize {
        return bad_upstream();
    }
    let mut events = Vec::with_capacity(items.len());
    for item in items {
        match event_row(item) {
            Ok(Some(row)) => events.push(row),
            Ok(None) => {}
            Err(()) => return bad_upstream(),
        }
    }
    let next_event_id = match payload.get("next_event_id") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(event)) if event.is_empty() => Value::Null,
        Some(Value::String(event)) if valid_id(event) => json!(event),
        Some(_) => return bad_upstream(),
    };
    let Some(valid_until) = payload
        .get("valid_until")
        .and_then(Value::as_i64)
        .and_then(rfc3339)
    else {
        return bad_upstream();
    };
    let mut response = (
        StatusCode::OK,
        Json(json!({
            "data": {
                "run": Value::Object(run),
                "events": events,
                "next_event_id": next_event_id,
                "valid_until": valid_until,
            }
        })),
    )
        .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// `GET /api/v1/spaces/{space_ref}/browser-runs/{run_id}/evidence/{evidence_id}` (R4): one evidence image. The
/// image is relayed as bytes with the sandbox headers; the JSON envelope is used only for a refusal.
async fn read_room_browser_evidence(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((space_ref, run_id, evidence_id)): Path<(String, String, String)>,
) -> Response {
    let space_ref = space_ref.trim().to_owned();
    let run_id = run_id.trim().to_owned();
    let evidence_id = evidence_id.trim().to_owned();
    if !valid_id(&run_id) || !valid_id(&evidence_id) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "invalid_run",
            "A browser run reference is required.",
        );
    }
    let auth = match room_read_auth(&state, &user, &headers, &space_ref).await {
        Ok(auth) => auth,
        Err(response) => return response,
    };
    let url = format!(
        "{}/v1/spaces/{}/browser/runs/{}/evidence/{}?space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(&space_ref),
        urlencoding::encode(&run_id),
        urlencoding::encode(&evidence_id),
        urlencoding::encode(&auth.decision_ref),
        urlencoding::encode(&auth.decision_token),
    );
    let mut request = state
        .client
        .get(&url)
        .timeout(EVIDENCE_TIMEOUT)
        .header("x-user-id", &user.user_id);
    request = crate::request_id::apply(request);
    if !auth.org_id.trim().is_empty() {
        request = request.header("x-org-id", &auth.org_id);
    }
    if let Some(token) = auth.model_token.as_deref().filter(|token| !token.is_empty()) {
        request = request.bearer_auth(token);
    }
    if let Some(value) = crate::domains::chat::shared::data_plane_authorization_value(&auth.session_token) {
        request = request.header("x-session-authorization", value);
    }
    let upstream = match request.send().await {
        Ok(upstream) => upstream,
        Err(_) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "evidence_unavailable",
                "The evidence is temporarily unavailable.",
            )
        }
    };
    let status = upstream.status();
    if !status.is_success() {
        let raw = upstream.bytes().await.unwrap_or_default();
        let code = serde_json::from_slice::<Value>(&raw)
            .ok()
            .and_then(|body| body["error"]["code"].as_str().map(str::to_owned))
            .unwrap_or_default();
        return match EVIDENCE_RELAYED.iter().find(|(known, _, _)| *known == code) {
            Some((known, relayed, message)) => refuse_with(
                StatusCode::from_u16(*relayed).unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
                *known,
                *message,
            ),
            None => refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "evidence_unavailable",
                "The evidence is temporarily unavailable.",
            ),
        };
    }
    if upstream
        .content_length()
        .is_some_and(|length| length > MAX_EVIDENCE_BYTES as u64)
    {
        return refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            "evidence_too_large",
            "The evidence is too large.",
        );
    }
    let media = upstream
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim().to_ascii_lowercase())
        .unwrap_or_default();
    let Some(media_type) = EVIDENCE_TYPES
        .iter()
        .copied()
        .find(|allowed| *allowed == media.as_str())
    else {
        return bad_upstream();
    };
    let bytes = match upstream.bytes().await {
        Ok(bytes) if bytes.len() <= MAX_EVIDENCE_BYTES => bytes,
        Ok(_) => {
            return refuse(
                StatusCode::PAYLOAD_TOO_LARGE,
                "evidence_too_large",
                "The evidence is too large.",
            )
        }
        Err(_) => {
            return refuse(
                StatusCode::SERVICE_UNAVAILABLE,
                "evidence_unavailable",
                "The evidence is temporarily unavailable.",
            )
        }
    };
    let mut response = (StatusCode::OK, bytes.to_vec()).into_response();
    let out = response.headers_mut();
    out.insert(CONTENT_TYPE, HeaderValue::from_static(media_type));
    out.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    out.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    out.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
    response
}

/// A refusal with a status chosen at run time and a code and message that are fixed words.
fn refuse_with(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (status, Json(error(code, message))).into_response()
}
