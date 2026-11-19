//! Room browser run reads: the BFF half of WS20-S4 (R2 to R4).
//!
//! `GET /api/v1/spaces/{space_ref}/browser-runs` lists the Space's browser runs, newest first.
//! `GET /api/v1/spaces/{space_ref}/browser-runs/{run_id}/timeline` reads one run's room timeline. `GET
//! /api/v1/spaces/{space_ref}/browser-runs/{run_id}/evidence/{evidence_id}` returns one evidence image.
//! `POST /api/v1/spaces/{space_ref}/browser-runs` (R1, WS20-S5) starts a run in a thread of the Space: this module
//! mints Control's browser-run decision and the goal's thread-append decision under the caller's own delegation,
//! takes the starter's parent grant, and relays to model-gateway's room start (see [`submit_start`]).
//! Session Core owns every decision: the Control `model.thread.read` verdict, the removed-member table, the audience
//! ceiling and the content-free projection. model-gateway relays them. This module decides nothing and stores nothing.
//! It checks the flag, the caller's org, the Space reference, the query shape, the zero-retention posture of the
//! request and the org (412 before any Control call), the Space lifecycle and the caller's Control read decision. It
//! then forwards with the caller's own bearers and reduces every answer to the allowed fields. A malformed upstream
//! answer is a 502, never passed through.
//!
//! SD-23: a room member sees receipts only, so a row carries ids, the starter's subject id, the phase, counts, hosts
//! and the closed fields of a timeline row. A URL, title, rationale, selector, frame or decided-by subject is not in
//! any shape this module accepts. Evidence is one sandboxed image, never cached and never sniffed, and it is answered
//! to the run's starter only: the projection names the starter, and any other caller (or a projection that names none)
//! gets 403 `browser_evidence_withheld`. Frames are never relayed here (the live view is WS20-S6/S8).
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_space` / `invalid_limit` / `invalid_cursor` /
//! `invalid_run`, 403 `active_membership_required` / `space_read_not_authorized` / `browser_evidence_withheld` /
//! `space_browser_not_authorized` / `browser_not_entitled`, 409 `space_authority_stale` / `room_archived`, 429
//! `browser_capacity`, 502 `browser_grant_unavailable`, 404 `feature_unavailable` /
//! `space_not_found` / `run_not_found` / `evidence_not_found`, 412 `zdr_feature_not_supported` (feature
//! `browser_run`), 413 `evidence_too_large`, 415 `evidence_unsupported`, 502 `invalid_model_gateway_response`, 503
//! `zdr_decision_unavailable` / `space_lifecycle_unavailable` / `delegated_auth_unavailable` / `session_unavailable`
//! / `evidence_unavailable`. The decision token rides the upstream query string only, as `room_artifacts` carries
//! it. It is never logged here and never appears in a response body.

use std::time::Duration;

use axum::{
    body::Bytes,
    extract::{Extension, Path, Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::human_posts::space_context_from_decision;
use super::{denial, shared_thread_read_decision_for_typed, space_lifecycle_by_ref};
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_json},
    zdr::{self, OrgPosture, OrgZdrPosture},
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
        .route(
            "/api/v1/spaces/{space_ref}/browser-runs",
            get(list_room_browser_runs).post(start_room_browser_run),
        )
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
fn zdr_browser_refusal() -> Reply {
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
}

/// Zero Data Retention fails closed before any Control call: the request's own flag, then the org's posture. A
/// failed lookup is 503, never "not zero retention". The room reads (R2 to R4) keep this cached posture; the start
/// takes [`zdr_start_gate`] instead, which reads the posture live.
async fn zdr_gate(state: &AppState, user: &AuthenticatedUser, headers: &HeaderMap) -> Option<Reply> {
    if zdr::request_zdr(headers, None) {
        return Some(zdr_browser_refusal());
    }
    match zdr::org_zdr_lookup(state, user).await {
        OrgZdrPosture::Disabled => None,
        OrgZdrPosture::Enabled => Some(zdr_browser_refusal()),
        OrgZdrPosture::Unknown => Some((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DECISION_UNAVAILABLE)),
        )),
    }
}

/// The start's zero-retention gate: the same order and the same typed answers as [`zdr_gate`], but the org's
/// posture is read live from org-core (`org_posture_live`), never through the 60-second cache. A run persists its
/// thread and its goal, so an org that has just turned ZDR on is refused at once. It runs before any Control call.
async fn zdr_start_gate(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    org_id: &str,
) -> Option<Reply> {
    if zdr::request_zdr(headers, None) {
        return Some(zdr_browser_refusal());
    }
    match zdr::org_posture_live(state, user, org_id).await {
        OrgPosture::NotZdr => None,
        OrgPosture::Zdr => Some(zdr_browser_refusal()),
        OrgPosture::Unknown => Some((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(zdr::zdr_refusal_body(zdr::ZDR_DECISION_UNAVAILABLE)),
        )),
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
        return Err(refusal.into_response());
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
    // SD-23: the evidence is the starter's own. The projection names the run's starter; any other caller, and a run
    // whose projection names no starter, gets the same typed refusal and no bytes.
    match run_starter_subject(&state, &user, &auth, &space_ref, &run_id).await {
        Ok(Some(starter)) if starter == user.user_id => {}
        Ok(_) => return browser_evidence_withheld(),
        Err(response) => return response,
    }
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

/// The starter's subject id as the room projection names it (R3's run header). `None` when the projection names no
/// starter. A refusal of the projection is returned as it is, so an unknown or foreign run answers as R3 answers it.
async fn run_starter_subject(
    state: &AppState,
    user: &AuthenticatedUser,
    auth: &RoomReadAuth,
    space_ref: &str,
    run_id: &str,
) -> Result<Option<String>, Response> {
    let url = format!(
        "{}/v1/spaces/{}/browser/runs/{}/timeline?limit=1&space_read_decision_ref={}&space_read_decision_token={}",
        state.model_gateway_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(run_id),
        urlencoding::encode(&auth.decision_ref),
        urlencoding::encode(&auth.decision_token),
    );
    let (status, Json(payload)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::GET,
        &url,
        None,
        auth.model_token.as_deref(),
        Some(&auth.session_token),
        user,
    )
    .await;
    if !status.is_success() {
        return Err((status, Json(payload)).into_response());
    }
    Ok(payload
        .get("run")
        .and_then(Value::as_object)
        .and_then(run_header)
        .and_then(|header| {
            header
                .get("started_by")
                .and_then(|by| by.get("subject_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|subject| !subject.trim().is_empty()))
}

/// SD-23's refusal of a run's evidence to anyone but its starter, and to every caller when the projection names none.
fn browser_evidence_withheld() -> Response {
    refuse(
        StatusCode::FORBIDDEN,
        "browser_evidence_withheld",
        "This evidence is shown to the browser run's starter only.",
    )
}

// ---------------------------------------------------------------------------------------------------------------------
// R1: the room browser run start (WS20-S5). `submit_start` is the one core function: the REST route and the
// `spaces.browser_run.start` action both call it. Order: the rollout flag (404), the org (403), the Space reference
// (400), zero retention (412, before any Control or model call), the body (400), the Space lifecycle, the bearers, then
// a new thread when the body names none (W3-DEC-BROWSER, D4: Control's thread-decision, then the shared-room create with
// the goal as its first post), then per attempt: Control's browser-run decision and the goal's thread-append decision,
// the starter's parent grant, and model-gateway's room start. The gateway stores nothing and decides nothing. A refused
// attempt revokes the grant it issued, and a stale decision is minted again once.
// ---------------------------------------------------------------------------------------------------------------------

/// The start's answer: a status and its JSON body (the route and the action both take this shape).
type Reply = (StatusCode, Json<Value>);

/// The longest goal, in characters (R1).
const MAX_GOAL_CHARS: usize = 2_000;
/// The most hosts a room run may name (R1: 1 to 8; Control and Quarry bound the run to them).
const MAX_RUN_HOSTS: usize = 8;
/// The step and runtime bounds of a room run (R1).
const STEPS_MIN: u32 = 5;
const STEPS_MAX: u32 = 40;
const RUNTIME_MIN_S: u32 = 30;
const RUNTIME_MAX_S: u32 = 600;
const DEFAULT_STEPS: u32 = 20;
const DEFAULT_RUNTIME_S: u32 = 300;
/// The cost a room run asks when it names none, and the most it may ask. Model-gateway clamps it again.
const DEFAULT_COST_USD: f64 = 0.25;
const MAX_COST_USD: f64 = 1.0;
/// The longest start page, in bytes.
const MAX_START_URL_BYTES: usize = 2_048;
/// The room run's start key is `space-browser-{client_send_id}`: the same key names the decision and the run. The goal's
/// thread-append decision carries its own key.
const START_KEY_PREFIX: &str = "space-browser-";
const APPEND_KEY_PREFIX: &str = "space-browser-append-";
/// The view a room run reports in v1: the starter alone (SD-23). The room view is not built.
const VIEW_OWNER: &str = "owner";

/// The start body, read strictly. An unknown field, such as a forged authority field, is a 400 and is never forwarded.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartRunBody {
    thread_id: Option<String>,
    goal: Option<String>,
    allowed_domains: Option<Vec<String>>,
    start_url: Option<String>,
    max_steps: Option<u32>,
    max_runtime_s: Option<u32>,
    max_cost_usd: Option<f64>,
    share_view: Option<bool>,
    client_send_id: Option<String>,
}

/// A start the body and the bounds allow, in the form every later step reads. `thread_id` is empty for a run that starts
/// a new thread (D4); the gateway creates that thread before the start.
struct ValidStart {
    thread_id: String,
    goal: String,
    hosts: Vec<String>,
    start_url: Option<String>,
    max_steps: u32,
    max_runtime_s: u32,
    max_cost_usd: f64,
    client_send_id: String,
}

/// One Control decision the gateway minted under the caller's delegation: the decision, its token and its claims.
struct MintedDecision {
    decision: Map<String, Value>,
    token: String,
    claims: Value,
}

/// The authority a start carries: the browser decision's token, claims and key, and the goal's append context.
struct StartDecisions {
    token: String,
    claims: Value,
    key: String,
    append: Value,
}

/// `POST /api/v1/spaces/{space_ref}/browser-runs` (R1): start a room browser run. The answer is 202 with the run ids.
async fn start_room_browser_run(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(space_ref): Path<String>,
    body: Bytes,
) -> Response {
    let body = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let (status, Json(payload)) = submit_start(&state, &user, &headers, space_ref.trim(), &body).await;
    let mut response = (status, Json(payload)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// The whole start (R1), shared by the REST route and the `spaces.browser_run.start` action.
pub(crate) async fn submit_start(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    space_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.browser_run {
        return start_refusal(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Room browser runs are not enabled.",
        );
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return start_refusal(
            StatusCode::FORBIDDEN,
            "active_membership_required",
            "An active organization membership is required to start a Space browser run.",
        );
    }
    let space_ref = space_ref.trim();
    if !valid_space_ref(space_ref) {
        return start_refusal(StatusCode::BAD_REQUEST, "invalid_space", "A Space reference is required.");
    }
    if let Some(refusal) = zdr_start_gate(state, user, headers, &org_id).await {
        return refusal;
    }
    let start = match parse_start(body) {
        Ok(start) => start,
        Err(refusal) => return refusal,
    };
    match space_lifecycle_by_ref(state, user, &org_id, space_ref).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return start_refusal(StatusCode::NOT_FOUND, "space_not_found", "Space is not available.");
        }
        Err(()) => {
            return start_refusal(
                StatusCode::SERVICE_UNAVAILABLE,
                "space_lifecycle_unavailable",
                "Space lifecycle is unavailable.",
            );
        }
    }
    let model_bearer = crate::domains::chat::shared::model_token(state, user, headers).await;
    let session_bearer = match crate::domains::chat::shared::required_session_token(state, user, headers).await {
        Ok(token) => token,
        Err(cause) => return crate::domains::chat::shared::delegated_auth_unavailable(cause),
    };
    // A run that names no thread starts one (D4): the shared-room create of WS01, before any browser decision or model
    // start. Get-or-create on the send key, so a retry lands on the same thread and the same first post.
    let thread_id = if start.thread_id.is_empty() {
        match create_run_thread(
            state,
            user,
            &org_id,
            space_ref,
            &start,
            model_bearer.as_deref(),
            &session_bearer,
        )
        .await
        {
            Ok(thread_id) => thread_id,
            Err(refusal) => return refusal,
        }
    } else {
        start.thread_id.clone()
    };
    let url = format!("{}/v1/browser/runs", state.model_gateway_url);
    for attempt in 0..2 {
        let decisions = match mint_start_decisions(state, user, &org_id, space_ref, &start, &thread_id).await {
            Ok(decisions) => decisions,
            Err(refusal) => return refusal,
        };
        let Some(grant) = issue_parent_grant(state, user, headers, &start.hosts).await else {
            return start_refusal(
                StatusCode::BAD_GATEWAY,
                "browser_grant_unavailable",
                "The browser grant for this run is not available.",
            );
        };
        let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
            state,
            Method::POST,
            &url,
            Some(start_request(&start, &thread_id, &grant, &decisions)),
            model_bearer.as_deref(),
            Some(&session_bearer),
            user,
        )
        .await;
        // The goal's audience moved between the two decisions: mint both again, once.
        if status == StatusCode::CONFLICT
            && refusal_code(&response) == Some("space_authority_stale")
            && attempt == 0
        {
            crate::domains::browser::revoke_session_grant(state, user, headers, &grant).await;
            continue;
        }
        if !status.is_success() {
            crate::domains::browser::revoke_session_grant(state, user, headers, &grant).await;
            return start_relay_refusal(status, &response);
        }
        if response.get("idempotent_replay").and_then(Value::as_bool) == Some(true) {
            // A keyed retry: the run this key names already holds its own grant, so this one is not needed.
            crate::domains::browser::revoke_session_grant(state, user, headers, &grant).await;
        }
        // The run is live: Model Plane revokes the parent grant when the run ends (W3-DEC-GRANT).
        return started(&response);
    }
    start_refusal(
        StatusCode::CONFLICT,
        "space_authority_stale",
        "The Space authority changed; try again.",
    )
}

/// The body and its checks. The goal is trimmed once: the digest Control signs and the goal text the run stores are the
/// same bytes. An empty `thread_id` starts a new thread (D4, W3-DEC-BROWSER): the gateway creates it under the shared-room
/// thread-create decision before the start, so the run never lands in a personal thread.
fn parse_start(body: &Value) -> Result<ValidStart, Reply> {
    let parsed: StartRunBody = serde_json::from_value(body.clone()).map_err(|_| invalid_start("body"))?;
    if parsed.share_view == Some(true) {
        return Err(start_refusal(
            StatusCode::BAD_REQUEST,
            "browser_share_view_unavailable",
            "Sharing the live view with the room is not available yet; only the starter sees the live view.",
        ));
    }
    let client_send_id = parsed.client_send_id.unwrap_or_default();
    if !valid_send_id(&client_send_id) {
        return Err(start_refusal(
            StatusCode::BAD_REQUEST,
            "invalid_client_send_id",
            "client_send_id must be 16 to 64 letters, digits, '_' or '-'.",
        ));
    }
    let thread_id = parsed.thread_id.as_deref().unwrap_or_default().trim().to_owned();
    if !thread_id.is_empty() && !valid_id(&thread_id) {
        return Err(invalid_start("thread_id"));
    }
    let goal = parsed.goal.as_deref().unwrap_or_default().trim().to_owned();
    if goal.is_empty() || goal.contains('\0') || goal.chars().count() > MAX_GOAL_CHARS {
        return Err(invalid_start("goal"));
    }
    let mut hosts = Vec::new();
    for host in parsed.allowed_domains.unwrap_or_default() {
        let host = host.trim().to_ascii_lowercase();
        if !room_host_ok(&host) {
            return Err(domain_policy_invalid());
        }
        hosts.push(host);
    }
    hosts.sort();
    hosts.dedup();
    if hosts.is_empty() || hosts.len() > MAX_RUN_HOSTS {
        return Err(domain_policy_invalid());
    }
    let start_url = match parsed.start_url.as_deref().map(str::trim).filter(|url| !url.is_empty()) {
        None => None,
        Some(url) => {
            let scheme_ok = url.starts_with("https://") || url.starts_with("http://");
            if !scheme_ok
                || url.len() > MAX_START_URL_BYTES
                || url.chars().any(|ch| ch.is_control() || ch.is_whitespace())
            {
                return Err(invalid_start("start_url"));
            }
            Some(url.to_owned())
        }
    };
    let max_steps = parsed.max_steps.unwrap_or(DEFAULT_STEPS);
    if !(STEPS_MIN..=STEPS_MAX).contains(&max_steps) {
        return Err(invalid_start("max_steps"));
    }
    let max_runtime_s = parsed.max_runtime_s.unwrap_or(DEFAULT_RUNTIME_S);
    if !(RUNTIME_MIN_S..=RUNTIME_MAX_S).contains(&max_runtime_s) {
        return Err(invalid_start("max_runtime_s"));
    }
    let max_cost_usd = parsed.max_cost_usd.unwrap_or(DEFAULT_COST_USD);
    if !(max_cost_usd.is_finite() && max_cost_usd > 0.0 && max_cost_usd <= MAX_COST_USD) {
        return Err(invalid_start("max_cost_usd"));
    }
    Ok(ValidStart {
        thread_id,
        goal,
        hosts,
        start_url,
        max_steps,
        max_runtime_s,
        max_cost_usd,
        client_send_id,
    })
}

/// Control's browser-run decision and the goal's thread-append decision, minted under the caller's delegation. Nothing
/// is cached: each attempt mints its own.
async fn mint_start_decisions(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    start: &ValidStart,
    thread_id: &str,
) -> Result<StartDecisions, Reply> {
    let digest = sha256_digest(&start.goal);
    // A new-thread run's browser decision binds no thread (D4); its thread is the one created above.
    let browser_thread = if start.thread_id.is_empty() { "" } else { thread_id };
    let key = format!("{START_KEY_PREFIX}{}", start.client_send_id);
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let browser = mint_control_decision(
        state,
        org_id,
        &actor,
        "browser-run-decision",
        json!({
            "space_ref": space_ref,
            "thread_id": browser_thread,
            "goal_digest": digest,
            "allowed_domains": start.hosts,
            "view_scope": VIEW_OWNER,
            "max_steps": start.max_steps,
            "max_runtime_s": start.max_runtime_s,
            "idempotency_key": key,
        }),
    )
    .await?;
    if !browser.claims.is_object() {
        return Err(decision_unavailable());
    }
    let appended = mint_control_decision(
        state,
        org_id,
        &actor,
        "thread-append-decision",
        json!({
            "space_ref": space_ref,
            "thread_id": thread_id,
            "content_digest": digest,
            "idempotency_key": format!("{APPEND_KEY_PREFIX}{}", start.client_send_id),
        }),
    )
    .await?;
    let append = space_context_from_decision(&appended.decision, &appended.token)?;
    Ok(StartDecisions {
        token: browser.token,
        claims: browser.claims,
        key,
        append,
    })
}

/// One Control decision call: the decision, its token and its claims. A refusal is Control's, mapped by
/// [`decision_refusal`].
async fn mint_control_decision(
    state: &AppState,
    org_id: &str,
    actor: &ActionActor,
    path: &str,
    request: Value,
) -> Result<MintedDecision, Reply> {
    let url = format!("{}/api/v1/internal/spaces/{path}", state.user_core_url);
    let (status, Json(response)) =
        proxy_json(state, Method::POST, &url, Some(request), Some(org_id), Some(actor), None).await;
    if !status.is_success() {
        return Err(decision_refusal(status, &response));
    }
    let data = unwrap_data(&response);
    let decision = data
        .get("decision")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(decision_unavailable)?;
    let token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned();
    if token.is_empty() {
        return Err(decision_unavailable());
    }
    let claims = data.get("claims").cloned().unwrap_or(Value::Null);
    Ok(MintedDecision {
        decision,
        token,
        claims,
    })
}

/// The new thread of a run that names none (D4, W3-DEC-BROWSER): the shared-room create of WS01 (`human_posts.rs`), with
/// the goal as the thread's first human post. Control's `thread-decision` is minted under the send key, then model-gateway's
/// `POST /v1/threads` creates the thread (get-or-create on that key) and stores the post under the same key, so a retry
/// finds the same thread and the same post. No personal or workspace thread is created here; a refusal is typed.
async fn create_run_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    start: &ValidStart,
    model_bearer: Option<&str>,
    session_bearer: &str,
) -> Result<String, Reply> {
    let send_key = format!("{START_KEY_PREFIX}{}", start.client_send_id);
    let actor = ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    };
    let minted = mint_control_decision(
        state,
        org_id,
        &actor,
        "thread-decision",
        json!({
            "space_ref": space_ref,
            "session_key": send_key,
            "idempotency_key": send_key,
        }),
    )
    .await?;
    let space_context = space_context_from_decision(&minted.decision, &minted.token)?;
    let url = format!("{}/v1/threads", state.model_gateway_url);
    let (status, Json(response)) = crate::domains::chat::shared::proxy_model_json_with_session(
        state,
        Method::POST,
        &url,
        Some(json!({
            "space_context": space_context,
            "session_key": send_key,
            "first_message": {
                "content": start.goal,
                "post_kind": "post",
                "client_message_id": send_key,
            },
        })),
        model_bearer,
        Some(session_bearer),
        user,
    )
    .await;
    if !status.is_success() {
        return Err(start_relay_refusal(status, &response));
    }
    response
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| valid_id(id))
        .map(str::to_owned)
        .ok_or_else(|| {
            start_refusal(
                StatusCode::BAD_GATEWAY,
                "invalid_model_gateway_response",
                "Model Gateway returned an invalid browser run answer.",
            )
        })
}

/// The starter's parent grant for one attempt: a session grant of the starter's own, narrowed to the run's hosts and
/// given the run's lifetime (W3-DEC-GRANT). Model Plane derives the room's run grant from it and revokes it when the
/// run ends. `None` when this gateway issues none, so the start refuses before any model call.
async fn issue_parent_grant(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    hosts: &[String],
) -> Option<String> {
    match crate::domains::browser::issue_session_grant(
        state,
        user,
        headers,
        crate::domains::browser::SessionGrantScope::RoomRunParent {
            hosts,
            ttl_s: i64::from(RUNTIME_MAX_S),
        },
        false,
    )
    .await
    {
        Ok(Some(grant_id)) if valid_id(&grant_id) => Some(grant_id),
        _ => None,
    }
}

/// The body model-gateway's room start reads. The browser's authority is the two decision contexts and the grant id; no
/// other field of the browser's body is forwarded as authority.
fn start_request(start: &ValidStart, thread_id: &str, grant_id: &str, decisions: &StartDecisions) -> Value {
    let mut body = json!({
        "thread_id": thread_id,
        "goal": start.goal,
        "grant_id": grant_id,
        "max_steps": start.max_steps,
        "max_runtime_s": start.max_runtime_s,
        "max_cost_usd": start.max_cost_usd,
        "space_append_context": decisions.append,
        "space_browser_authority": {
            "decision_token": decisions.token,
            "claims": decisions.claims,
            "idempotency_key": decisions.key,
        },
    });
    if let Some(url) = start.start_url.as_deref() {
        body["start_url"] = json!(url);
    }
    body
}

/// The 202 answer from model-gateway's start: the run's ids and the starter's view scope. A malformed answer is a 502.
fn started(response: &Value) -> Reply {
    let id = |key: &str| {
        response
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| valid_id(value))
            .map(str::to_owned)
    };
    match (id("run_id"), id("thread_id"), id("plan_id")) {
        (Some(run_id), Some(thread_id), Some(plan_id)) => (
            StatusCode::ACCEPTED,
            Json(json!({
                "data": {
                    "run_id": run_id,
                    "thread_id": thread_id,
                    "plan_id": plan_id,
                    "view": { "scope": VIEW_OWNER },
                }
            })),
        ),
        _ => start_refusal(
            StatusCode::BAD_GATEWAY,
            "invalid_model_gateway_response",
            "Model Gateway returned an invalid browser run answer.",
        ),
    }
}

/// Control's refusal of a decision, as the browser sees it. Entitlement, zero retention and the room's archive keep their
/// own typed codes; every other refusal is `space_browser_not_authorized`, since Control gives no reason a browser may
/// rely on.
fn decision_refusal(status: StatusCode, body: &Value) -> Reply {
    let code = refusal_code(body).unwrap_or_default();
    match (status.as_u16(), code) {
        (403, "space_entitlement_off") => start_refusal(
            StatusCode::FORBIDDEN,
            "browser_not_entitled",
            "Browser runs are not turned on for this organization.",
        ),
        (403, "space_zdr_forbids_effect") => zdr_browser_refusal(),
        (403, _) => start_refusal(
            StatusCode::FORBIDDEN,
            "space_browser_not_authorized",
            "Starting a browser run in this Space is not permitted.",
        ),
        (400, _) => invalid_start("request"),
        (404, _) => start_refusal(StatusCode::NOT_FOUND, "space_not_found", "Space is not available."),
        (409, "room_archived") => start_refusal(
            StatusCode::CONFLICT,
            "room_archived",
            "The room is archived; browser runs are closed.",
        ),
        (409, _) => start_refusal(
            StatusCode::CONFLICT,
            "space_authority_stale",
            "The Space authority changed; try again.",
        ),
        _ => decision_unavailable(),
    }
}

/// model-gateway's refusal of the start, as the browser sees it. Only the codes named here are relayed, each with its
/// status and a fixed message. Anything else is `session_unavailable`.
fn start_relay_refusal(status: StatusCode, body: &Value) -> Reply {
    let code = refusal_code(body).unwrap_or_default();
    match (status.as_u16(), code) {
        (412, _) => zdr_browser_refusal(),
        (429, "browser_capacity") => start_refusal(
            StatusCode::TOO_MANY_REQUESTS,
            "browser_capacity",
            "This Space already has the maximum number of browser runs.",
        ),
        (404, "feature_unavailable") => start_refusal(
            StatusCode::NOT_FOUND,
            "feature_unavailable",
            "Room browser runs are not enabled.",
        ),
        (404, _) => start_refusal(StatusCode::NOT_FOUND, "space_not_found", "Space is not available."),
        (400, _) => invalid_start("request"),
        (403, _) => start_refusal(
            StatusCode::FORBIDDEN,
            "space_browser_not_authorized",
            "Starting a browser run in this Space is not permitted.",
        ),
        (409, "space_authority_stale") => start_refusal(
            StatusCode::CONFLICT,
            "space_authority_stale",
            "The Space authority changed; try again.",
        ),
        (409, _) => start_refusal(
            StatusCode::CONFLICT,
            "idempotency_key_conflict",
            "This browser run was already sent with different settings.",
        ),
        (502, "browser_grant_unavailable") => start_refusal(
            StatusCode::BAD_GATEWAY,
            "browser_grant_unavailable",
            "The browser grant for this run is not available.",
        ),
        (503, "space_decision_unavailable") => decision_unavailable(),
        _ => start_refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "The Space's browser runs could not be started.",
        ),
    }
}

/// The closed code of a refusal body: Control's top-level `code`, or the typed envelope's `error.code`.
fn refusal_code(body: &Value) -> Option<&str> {
    body.get("code")
        .and_then(Value::as_str)
        .or_else(|| body.pointer("/error/code").and_then(Value::as_str))
}

fn start_refusal(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn invalid_start(reason: &'static str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_browser_run",
            "The browser run request is not valid.",
            json!({ "reason": reason }),
        )),
    )
}

fn domain_policy_invalid() -> Reply {
    start_refusal(
        StatusCode::BAD_REQUEST,
        "browser_domain_policy_invalid",
        "The domains must be one to eight public hostnames, without an address, port, path or wildcard.",
    )
}

fn decision_unavailable() -> Reply {
    start_refusal(
        StatusCode::SERVICE_UNAVAILABLE,
        "space_decision_unavailable",
        "The Space decision service is unavailable.",
    )
}

/// `^[A-Za-z0-9_-]{16,64}$`, the shape of a client send id (the same shape the post flow takes).
fn valid_send_id(id: &str) -> bool {
    (16..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// A hostname a room run may name: lowercase labels of letters, digits and inner hyphens, at least two labels, no
/// `localhost`, no IP literal (an all-digit final label), no wildcard and no port, path or scheme. Control validates the
/// same shape again; this is the browser's first answer.
fn room_host_ok(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host == "localhost" || host.ends_with(".localhost") {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let labels_ok = labels.iter().all(|label| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    let ip_literal = labels
        .last()
        .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()));
    labels_ok && !ip_literal
}

/// `sha256:` and the lowercase hex of the bytes: the form Control's goal and content digests take.
fn sha256_digest(text: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(text.as_bytes())))
}

#[cfg(test)]
mod tests {
    //! The room browser-run routes through the real router (WS20-S4 reads and evidence, WS20-S5 start, W3-DEC-GRANT and
    //! W3-DEC-GWFIX2). Session Core, Control (user-core), org-core, Convex and model-gateway are doubles, so each refusal
    //! order, each zero-retention refusal before any Control call, the parent grant's life and the evidence rule are read
    //! off the wire. `parse_start` is checked directly for the body rules.
    use axum::{
        body::Body,
        http::{HeaderMap, Request, StatusCode},
        Json,
    };
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::middleware::{AuthenticatedUser, AuthorizedMembership};

    const SPACE: &str = "space-room";
    const RUN: &str = "run-1";
    const THREAD: &str = "thread-1";
    const SEND: &str = "send-0123456789abcdef";
    const GOAL: &str = "Find the pricing page on example.com";
    /// The sha256 of [`GOAL`] in Control's form, computed outside this code with `sha256sum`.
    const GOAL_DIGEST: &str = "sha256:67da0de00da5e05ed0adcbe80da0059c645f866f8f527c3705d79e3d7c3882e3";
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nevidence-bytes";
    const COOKIE: &str = "better-auth.session_token=session-1";
    const ROOM_RUNS: &str = "/api/v1/spaces/space-room/browser-runs";
    const TIMELINE: &str = "/api/v1/spaces/space-room/browser-runs/run-1/timeline";
    const EVIDENCE: &str = "/api/v1/spaces/space-room/browser-runs/run-1/evidence/ev-1";
    const MODEL_RUNS: &str = "/v1/spaces/space-room/browser/runs";
    const MODEL_TIMELINE: &str = "/v1/spaces/space-room/browser/runs/run-1/timeline";
    const MODEL_EVIDENCE: &str = "/v1/spaces/space-room/browser/runs/run-1/evidence/ev-1";
    const MODEL_START: &str = "/v1/browser/runs";
    const MODEL_GRANT: &str = "/v1/browser/grants";
    const MODEL_REVOKE: &str = "/v1/browser/grants/grant-1";

    /// Every double a room route reaches. `user_core` answers Control, `model` answers model-gateway and `org_core` answers
    /// the zero-retention read.
    struct Fixture {
        state: AppState,
        user_core: MockServer,
        org_core: MockServer,
        model: MockServer,
        _others: Vec<MockServer>,
    }

    /// `org` is org-core's answer to the zero-retention read; `read` and `browser` are Control's answers to the room read
    /// decision and to the browser-run decision. The thread-append decision is granted, and model-gateway's grant and grant
    /// revoke succeed. A test mounts the model-gateway answer it exercises.
    async fn fixture(org: ResponseTemplate, read: ResponseTemplate, browser: ResponseTemplate) -> Fixture {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        for (slug, token) in [
            ("session-core", "session-token"),
            ("browser-broker", "broker-token"),
            ("model-plane", "model-token"),
        ] {
            Mock::given(wm_method("GET"))
                .and(wm_path(format!("/api/{slug}/token")))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "token": token })))
                .mount(&auth)
                .await;
        }
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": "org-1", "role": "member", "onboardingStatus": "COMPLETED"
            })))
            .mount(&user_core)
            .await;
        for (path, answer) in [
            ("/api/v1/internal/spaces/thread-read-decision", read),
            ("/api/v1/internal/spaces/browser-run-decision", browser),
            ("/api/v1/internal/spaces/thread-append-decision", append_granted()),
        ] {
            Mock::given(wm_method("POST"))
                .and(wm_path(path))
                .respond_with(answer)
                .mount(&user_core)
                .await;
        }
        let org_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org)
            .mount(&org_core)
            .await;
        let convex = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path("/api/query"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef": SPACE, "name": "Room", "kind": "room", "lifecycle": "active"}
            ]})))
            .mount(&convex)
            .await;
        let model = MockServer::start().await;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_GRANT))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"grant_id": "grant-1"}})))
            .mount(&model)
            .await;
        Mock::given(wm_method("DELETE"))
            .and(wm_path(MODEL_REVOKE))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"revoked": true}})))
            .mount(&model)
            .await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        state.application_convex_url = convex.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.model_gateway_url = model.uri();
        Fixture {
            state,
            user_core,
            org_core,
            model,
            _others: vec![auth, convex],
        }
    }

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: "org-1".to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    fn org_record(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}))
    }

    fn granted_read() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {"decision_ref": "read-ref-1"},
            "token": "read-token-secret"
        }}))
    }

    fn granted_browser() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {"decision_ref": "browser-ref-1"},
            "token": "browser-decision-token",
            "claims": {"sub": "user-1", "space_ref": SPACE}
        }}))
    }

    /// Control's refusal of a decision, as Control answers it: the closed code at the top level.
    fn refused(status: u16, code: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(json!({"code": code}))
    }

    fn append_granted() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {
                "space_ref": SPACE,
                "decision_ref": "append-ref-1",
                "recipient_audience_ref": "audience-1",
                "recipient_audience_revision": 4,
                "recipient_audience_hash": "audience-hash-1",
                "privacy_policy_ref": "policy-1",
                "authority_revision": 7,
                "resource_authorization_ref": "resource-1",
                "action_schema_hash": "schema-1",
                "payload_digest": "payload-1",
                "idempotency_key": "space-browser-append-send-0123456789abcdef"
            },
            "token": "append-token"
        }}))
    }

    /// model-gateway's answer to a start: the run's ids at the top level, the way the start reads them.
    fn started_answer() -> ResponseTemplate {
        ResponseTemplate::new(202).set_body_json(json!({
            "run_id": RUN, "thread_id": THREAD, "plan_id": "plan-1"
        }))
    }

    /// model-gateway's typed refusal of a start because the Space's authority moved.
    fn stale_answer() -> ResponseTemplate {
        ResponseTemplate::new(409).set_body_json(json!({"error": {"code": "space_authority_stale"}}))
    }

    /// A start the body rules accept: one host, a goal with surrounding space, and the thread the run lands in.
    fn start_json() -> Value {
        json!({
            "thread_id": THREAD,
            "goal": format!("  {GOAL} \n"),
            "allowed_domains": ["example.com"],
            "client_send_id": SEND,
        })
    }

    /// A room run's header as the projection names it. The starter is the run's own subject.
    fn run_json(starter: &str) -> Value {
        json!({
            "run_id": RUN,
            "thread_id": THREAD,
            "started_by": {"subject_id": starter},
            "phase": "running",
            "started_at_unix": 1800000000,
            "finished_at_unix": null,
            "control": {"mode": "agent"},
            "handoff": {"open": false},
            "view": {"scope": "owner"}
        })
    }

    /// One list row: the header plus the counts and hosts the list adds.
    fn list_row(starter: &str) -> Value {
        let mut row = run_json(starter);
        row["hosts"] = json!(["example.com"]);
        row["step_count"] = json!(3);
        row
    }

    fn timeline_answer(run: Value, events: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "run": run,
            "events": events,
            "next_event_id": null,
            "valid_until": 1800000600
        }))
    }

    fn png_answer() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(PNG.to_vec(), "image/png")
    }

    /// The request as the router answers it: the status, the headers and the raw body.
    async fn send(state: AppState, method: &str, uri: &str, body: Option<Value>) -> (u16, HeaderMap, Vec<u8>) {
        let mut builder = Request::builder().method(method).uri(uri).header("cookie", COOKIE);
        if body.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder
            .body(Body::from(body.map(|value| value.to_string()).unwrap_or_default()))
            .expect("a request");
        let response = crate::build_router(state)
            .oneshot(request)
            .await
            .expect("the router answers");
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes()
            .to_vec();
        (status, headers, bytes)
    }

    fn json_of(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).unwrap_or(Value::Null)
    }

    fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
        headers.get(name).and_then(|value| value.to_str().ok())
    }

    /// The requests a server recorded, as "METHOD path", for the paths that contain `part`.
    async fn hits(server: &MockServer, part: &str) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|request| format!("{} {}", request.method, request.url.path()))
            .filter(|hit| hit.contains(part))
            .collect()
    }

    /// The JSON bodies a server recorded for one method and one exact path.
    async fn bodies(server: &MockServer, verb: &str, path: &str) -> Vec<Value> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.method.as_str() == verb && request.url.path() == path)
            .map(|request| request.body_json::<Value>().unwrap_or(Value::Null))
            .collect()
    }

    /// Primes the cached org-zdr read to "not ZDR", as a read inside the minute does, then replaces org-core's answer with
    /// `org_after`. The cache is in memory so the route sees the same entry the priming wrote.
    async fn cached_not_zdr_then(fixture: &mut Fixture, org_after: ResponseTemplate) {
        fixture.state.cache = crate::cache::ResultCache::in_memory();
        assert_eq!(
            crate::zdr::org_zdr_lookup(&fixture.state, &user()).await,
            crate::zdr::OrgZdrPosture::Disabled,
            "the cached read remembers that the org is not ZDR"
        );
        fixture.org_core.reset().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(org_after)
            .mount(&fixture.org_core)
            .await;
    }

    // ---- WS20-S4: the room reads and the evidence -------------------------------------------------------------------

    #[tokio::test]
    async fn the_room_reads_are_404_and_call_nothing_while_the_flag_is_off() {
        let fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        for uri in [ROOM_RUNS, TIMELINE, EVIDENCE] {
            let (status, _, body) = send(fixture.state.clone(), "GET", uri, None).await;
            assert_eq!(status, 404, "{uri}");
            assert_eq!(json_of(&body)["error"]["code"], "feature_unavailable", "{uri}");
        }
        assert!(hits(&fixture.user_core, "/internal/spaces/").await.is_empty());
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    #[tokio::test]
    async fn an_org_under_zero_retention_is_refused_with_412_before_any_control_call() {
        let mut fixture = fixture(org_record(true), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        let (status, _, body) = send(fixture.state.clone(), "GET", ROOM_RUNS, None).await;
        assert_eq!(status, 412);
        assert_eq!(json_of(&body)["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(json_of(&body)["error"]["feature"], "browser_run");
        assert!(hits(&fixture.user_core, "/internal/spaces/").await.is_empty());
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    #[tokio::test]
    async fn control_declining_the_room_read_is_space_read_not_authorized() {
        let mut fixture = fixture(
            org_record(false),
            refused(403, "space_membership_required"),
            granted_browser(),
        )
        .await;
        fixture.state.space_features.browser_run = true;
        let (status, _, body) = send(fixture.state.clone(), "GET", ROOM_RUNS, None).await;
        assert_eq!(status, 403);
        assert_eq!(json_of(&body)["error"]["code"], "space_read_not_authorized");
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    #[tokio::test]
    async fn list_rows_keep_only_the_fields_the_room_may_see() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        let mut row = list_row("user-1");
        row["goal"] = json!("Secret goal text");
        row["grant_id"] = json!("grant-9");
        row["start_url"] = json!("https://example.com/private");
        row["space_browser_authority"] = json!({"decision_token": "leaked"});
        row["org_id"] = json!("org-2");
        Mock::given(wm_method("GET"))
            .and(wm_path(MODEL_RUNS))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"runs": [row], "next_cursor": null})))
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "GET", ROOM_RUNS, None).await;
        assert_eq!(status, 200);
        let answer = json_of(&body);
        let mut keys: Vec<&str> = answer["data"][0]
            .as_object()
            .expect("a row")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = vec![
            "control",
            "finished_at",
            "handoff",
            "hosts",
            "phase",
            "run_id",
            "started_at",
            "started_by",
            "step_count",
            "thread_id",
            "view",
        ];
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(answer["data"][0]["hosts"], json!(["example.com"]));
        assert_eq!(answer["meta"]["next_cursor"], Value::Null);
        let text = String::from_utf8_lossy(&body);
        assert!(!text.contains("Secret goal text"), "{text}");
        assert!(!text.contains("grant-9"), "{text}");
        assert!(!text.contains("leaked"), "{text}");
    }

    #[tokio::test]
    async fn a_malformed_run_row_is_an_invalid_upstream_answer_and_nothing_of_it_is_shown() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        let mut row = list_row("user-1");
        row["phase"] = Value::Null;
        row["goal"] = json!("Secret goal text");
        Mock::given(wm_method("GET"))
            .and(wm_path(MODEL_RUNS))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"runs": [row], "next_cursor": null})))
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "GET", ROOM_RUNS, None).await;
        assert_eq!(status, 502);
        assert_eq!(json_of(&body)["error"]["code"], "invalid_model_gateway_response");
        assert!(!String::from_utf8_lossy(&body).contains("Secret goal text"));
    }

    #[tokio::test]
    async fn an_unknown_timeline_kind_and_a_malformed_optional_field_are_dropped_and_the_page_still_answers() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        let events = json!([
            {"event_id": "evt-1", "at_unix": 1800000010, "kind": "action", "step": "step-1",
             "action_type": "click", "host": "example.com", "status": "completed",
             "url": "https://example.com/private?q=secret", "title": "Private page"},
            {"event_id": "evt-2", "at_unix": 1800000011, "kind": "frame", "image": "AAAA"},
            {"event_id": "evt-3", "at_unix": 1800000012, "kind": "observation", "evidence_id": "ev-1",
             "host": "not a host/path", "status": "ok"}
        ]);
        Mock::given(wm_method("GET"))
            .and(wm_path(MODEL_TIMELINE))
            .respond_with(timeline_answer(run_json("user-1"), events))
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "GET", TIMELINE, None).await;
        assert_eq!(status, 200);
        let answer = json_of(&body);
        assert_eq!(
            answer["data"]["events"],
            json!([
                {"event_id": "evt-1", "at": "2027-01-15T08:00:10+00:00", "kind": "action", "step": "step-1",
                 "action_type": "click", "host": "example.com", "status": "completed"},
                {"event_id": "evt-3", "at": "2027-01-15T08:00:12+00:00", "kind": "observation",
                 "evidence_id": "ev-1", "status": "ok"}
            ])
        );
        assert_eq!(answer["data"]["valid_until"], "2027-01-15T08:10:00+00:00");
        assert_eq!(answer["data"]["next_event_id"], Value::Null);
        assert_eq!(answer["data"]["run"]["run_id"], RUN);
    }

    #[tokio::test]
    async fn the_starter_receives_the_evidence_image_with_the_sandbox_headers() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("GET"))
            .and(wm_path(MODEL_TIMELINE))
            .respond_with(timeline_answer(run_json("user-1"), json!([])))
            .mount(&fixture.model)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path(MODEL_EVIDENCE))
            .respond_with(png_answer())
            .mount(&fixture.model)
            .await;
        let (status, headers, body) = send(fixture.state.clone(), "GET", EVIDENCE, None).await;
        assert_eq!(status, 200);
        assert_eq!(body, PNG);
        assert_eq!(header_text(&headers, "content-type"), Some("image/png"));
        assert_eq!(header_text(&headers, "cache-control"), Some("private, no-store"));
        assert_eq!(header_text(&headers, "x-content-type-options"), Some("nosniff"));
        assert_eq!(header_text(&headers, "content-security-policy"), Some("sandbox"));
    }

    #[tokio::test]
    async fn evidence_that_is_not_an_image_is_an_invalid_upstream_answer_and_is_not_relayed() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("GET"))
            .and(wm_path(MODEL_TIMELINE))
            .respond_with(timeline_answer(run_json("user-1"), json!([])))
            .mount(&fixture.model)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path(MODEL_EVIDENCE))
            .respond_with(ResponseTemplate::new(200).set_body_raw(b"<html>not an image</html>".to_vec(), "text/html"))
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "GET", EVIDENCE, None).await;
        assert_eq!(status, 502);
        assert_eq!(json_of(&body)["error"]["code"], "invalid_model_gateway_response");
        assert!(!String::from_utf8_lossy(&body).contains("<html>"));
    }

    // ---- WS20-S5: the room start --------------------------------------------------------------------------------------

    #[tokio::test]
    async fn the_room_start_is_404_and_calls_nothing_while_its_flag_is_off() {
        let fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 404);
        assert_eq!(json_of(&body)["error"]["code"], "feature_unavailable");
        assert!(hits(&fixture.user_core, "/internal/spaces/").await.is_empty());
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    #[tokio::test]
    async fn an_org_under_zero_retention_is_refused_with_412_before_any_control_or_model_call() {
        let mut fixture = fixture(org_record(true), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 412);
        assert_eq!(json_of(&body)["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(json_of(&body)["error"]["feature"], "browser_run");
        assert!(hits(&fixture.user_core, "/internal/spaces/").await.is_empty());
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    #[tokio::test]
    async fn a_start_body_with_an_authority_field_is_refused_400_and_never_forwarded() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        for field in ["space_browser_authority", "grant_id", "profile_id"] {
            let mut body = start_json();
            body[field] = json!("forged");
            let (status, _, bytes) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(body)).await;
            let answer = json_of(&bytes);
            assert_eq!(status, 400, "{field}");
            assert_eq!(answer["error"]["code"], "invalid_browser_run", "{field}");
            assert_eq!(answer["error"]["details"]["reason"], "body", "{field}");
        }
        assert!(hits(&fixture.user_core, "/internal/spaces/").await.is_empty());
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    #[test]
    fn a_domain_list_that_is_not_one_to_eight_public_hostnames_is_policy_invalid() {
        let nine: Vec<String> = (0..9).map(|n| format!("site{n}.example.com")).collect();
        let cases = [
            json!(["192.168.0.1"]),
            json!(["localhost"]),
            json!(["app.localhost"]),
            json!(["*.example.com"]),
            json!(["https://example.com"]),
            json!(["example.com/path"]),
            json!(["example.com:8443"]),
            json!(["example"]),
            json!(nine),
            json!([]),
        ];
        for hosts in cases {
            let mut body = start_json();
            body["allowed_domains"] = hosts.clone();
            let (status, Json(refusal)) = parse_start(&body).err().expect("the host list is refused");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{hosts}");
            assert_eq!(refusal["error"]["code"], "browser_domain_policy_invalid", "{hosts}");
        }
        let mut accepted = start_json();
        accepted["allowed_domains"] = json!(["Example.com", "docs.example.org"]);
        assert!(parse_start(&accepted).is_ok(), "two public hostnames are accepted");
    }

    #[test]
    fn share_view_is_refused_as_unavailable_and_false_is_not_a_refusal() {
        let mut body = start_json();
        body["share_view"] = json!(true);
        let (status, Json(refusal)) = parse_start(&body).err().expect("share_view true is refused");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(refusal["error"]["code"], "browser_share_view_unavailable");
        body["share_view"] = json!(false);
        assert!(parse_start(&body).is_ok());
    }

    #[test]
    fn the_step_runtime_and_goal_bounds_refuse_just_outside_and_accept_the_edges() {
        for (field, value, reason) in [
            ("max_steps", 4, "max_steps"),
            ("max_steps", 41, "max_steps"),
            ("max_runtime_s", 29, "max_runtime_s"),
            ("max_runtime_s", 601, "max_runtime_s"),
        ] {
            let mut body = start_json();
            body[field] = json!(value);
            let (status, Json(refusal)) = parse_start(&body).err().expect("out of bounds is refused");
            assert_eq!(status, StatusCode::BAD_REQUEST, "{field}={value}");
            assert_eq!(refusal["error"]["code"], "invalid_browser_run", "{field}={value}");
            assert_eq!(refusal["error"]["details"]["reason"], reason, "{field}={value}");
        }
        for (field, value) in [
            ("max_steps", 5),
            ("max_steps", 40),
            ("max_runtime_s", 30),
            ("max_runtime_s", 600),
        ] {
            let mut body = start_json();
            body[field] = json!(value);
            assert!(parse_start(&body).is_ok(), "{field}={value}");
        }
        let mut body = start_json();
        body["goal"] = json!("g".repeat(2001));
        let (_, Json(refusal)) = parse_start(&body).err().expect("a 2001-character goal is refused");
        assert_eq!(refusal["error"]["details"]["reason"], "goal");
        body["goal"] = json!("g".repeat(2000));
        assert!(parse_start(&body).is_ok(), "a 2000-character goal is accepted");
    }

    #[tokio::test]
    async fn a_control_403_is_browser_not_entitled_for_an_entitlement_and_a_browser_refusal_otherwise() {
        for (control_code, answer_code) in [
            ("space_entitlement_off", "browser_not_entitled"),
            ("space_membership_required", "space_browser_not_authorized"),
        ] {
            let mut fixture = fixture(org_record(false), granted_read(), refused(403, control_code)).await;
            fixture.state.space_features.browser_run = true;
            let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
            assert_eq!(status, 403, "{control_code}");
            assert_eq!(json_of(&body)["error"]["code"], answer_code, "{control_code}");
            // The browser decision refused, so no append decision, no grant and no start follow it.
            assert!(hits(&fixture.user_core, "thread-append-decision").await.is_empty(), "{control_code}");
            assert!(hits(&fixture.model, "").await.is_empty(), "{control_code}");
        }
    }

    #[tokio::test]
    async fn both_decisions_carry_the_sha256_of_the_trimmed_goal_and_the_send_keys() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(started_answer())
            .mount(&fixture.model)
            .await;
        let (status, _, _) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 202);
        let browser = bodies(&fixture.user_core, "POST", "/api/v1/internal/spaces/browser-run-decision").await;
        let append = bodies(&fixture.user_core, "POST", "/api/v1/internal/spaces/thread-append-decision").await;
        assert_eq!(browser.len(), 1);
        assert_eq!(append.len(), 1);
        assert_eq!(browser[0]["goal_digest"], GOAL_DIGEST);
        assert_eq!(append[0]["content_digest"], GOAL_DIGEST);
        assert_eq!(browser[0]["idempotency_key"], "space-browser-send-0123456789abcdef");
        assert_eq!(append[0]["idempotency_key"], "space-browser-append-send-0123456789abcdef");
        assert_eq!(browser[0]["allowed_domains"], json!(["example.com"]));
        assert_eq!(browser[0]["view_scope"], "owner");
        assert_eq!(browser[0]["thread_id"], THREAD);
        assert_eq!(append[0]["thread_id"], THREAD);
    }

    #[tokio::test]
    async fn a_started_run_forwards_only_the_decided_authority_and_answers_202_with_the_owner_view() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(started_answer())
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 202);
        assert_eq!(
            json_of(&body),
            json!({"data": {"run_id": RUN, "thread_id": THREAD, "plan_id": "plan-1", "view": {"scope": "owner"}}})
        );
        let sent = bodies(&fixture.model, "POST", MODEL_START).await;
        assert_eq!(sent.len(), 1);
        let forwarded = &sent[0];
        let mut keys: Vec<&str> = forwarded
            .as_object()
            .expect("the start body is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut expected = vec![
            "goal",
            "grant_id",
            "max_cost_usd",
            "max_runtime_s",
            "max_steps",
            "space_append_context",
            "space_browser_authority",
            "thread_id",
        ];
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(forwarded["goal"], GOAL);
        assert_eq!(forwarded["thread_id"], THREAD);
        assert_eq!(forwarded["grant_id"], "grant-1");
        assert_eq!(forwarded["max_steps"], 20);
        assert_eq!(forwarded["max_runtime_s"], 300);
        assert_eq!(forwarded["max_cost_usd"], 0.25);
        assert_eq!(
            forwarded["space_browser_authority"],
            json!({
                "decision_token": "browser-decision-token",
                "claims": {"sub": "user-1", "space_ref": SPACE},
                "idempotency_key": "space-browser-send-0123456789abcdef"
            })
        );
        assert_eq!(forwarded["space_append_context"]["space_decision_token"], "append-token");
        assert_eq!(forwarded["space_append_context"]["space_id"], SPACE);
        // A run that starts keeps its parent grant: Model Plane revokes it when the run ends.
        assert!(hits(&fixture.model, "DELETE").await.is_empty());
    }

    #[tokio::test]
    async fn a_refused_start_answers_its_typed_code_and_revokes_the_parent_grant_it_was_given() {
        for (upstream_status, upstream_code, answer_status, answer_code) in [
            (429, "browser_capacity", 429, "browser_capacity"),
            (403, "space_membership_required", 403, "space_browser_not_authorized"),
        ] {
            let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
            fixture.state.space_features.browser_run = true;
            Mock::given(wm_method("POST"))
                .and(wm_path(MODEL_START))
                .respond_with(
                    ResponseTemplate::new(upstream_status)
                        .set_body_json(json!({"error": {"code": upstream_code}})),
                )
                .mount(&fixture.model)
                .await;
            let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
            assert_eq!(status, answer_status, "{upstream_code}");
            assert_eq!(json_of(&body)["error"]["code"], answer_code, "{upstream_code}");
            assert_eq!(
                hits(&fixture.model, "/v1/browser/grants/grant-1").await,
                vec!["DELETE /v1/browser/grants/grant-1"],
                "{upstream_code}"
            );
        }
    }

    #[tokio::test]
    async fn a_second_stale_authority_is_surfaced_after_one_fresh_mint() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(stale_answer())
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 409);
        assert_eq!(json_of(&body)["error"]["code"], "space_authority_stale");
        // Exactly two attempts: each mints both decisions and issues its own grant, and each revokes that grant.
        assert_eq!(hits(&fixture.user_core, "browser-run-decision").await.len(), 2);
        assert_eq!(hits(&fixture.user_core, "thread-append-decision").await.len(), 2);
        assert_eq!(hits(&fixture.model, "POST /v1/browser/grants").await.len(), 2);
        assert_eq!(hits(&fixture.model, "POST /v1/browser/runs").await.len(), 2);
        assert_eq!(hits(&fixture.model, "DELETE /v1/browser/grants/").await.len(), 2);
    }

    #[tokio::test]
    async fn a_stale_authority_once_is_minted_again_and_the_retry_starts_the_run() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(stale_answer())
            .up_to_n_times(1)
            .mount(&fixture.model)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(started_answer())
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 202);
        assert_eq!(json_of(&body)["data"]["run_id"], RUN);
        assert_eq!(hits(&fixture.user_core, "browser-run-decision").await.len(), 2);
        assert_eq!(hits(&fixture.model, "POST /v1/browser/grants").await.len(), 2);
        // The first attempt's grant was revoked; the run keeps the second one until its own end.
        assert_eq!(hits(&fixture.model, "DELETE /v1/browser/grants/").await.len(), 1);
    }

    #[tokio::test]
    async fn a_keyed_replay_answers_the_run_and_revokes_the_parent_grant_it_does_not_need() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "idempotent_replay": true, "run_id": RUN, "thread_id": THREAD, "plan_id": "plan-1"
            })))
            .mount(&fixture.model)
            .await;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 202);
        assert_eq!(json_of(&body)["data"]["run_id"], RUN);
        assert_eq!(
            hits(&fixture.model, "/v1/browser/grants/grant-1").await,
            vec!["DELETE /v1/browser/grants/grant-1"]
        );
    }

    #[tokio::test]
    async fn the_evidence_is_withheld_from_anyone_but_the_starter_and_from_a_projection_that_names_none() {
        let mut no_starter = run_json("user-1");
        no_starter.as_object_mut().expect("an object").remove("started_by");
        for run in [run_json("user-2"), no_starter] {
            let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
            fixture.state.space_features.browser_run = true;
            Mock::given(wm_method("GET"))
                .and(wm_path(MODEL_TIMELINE))
                .respond_with(timeline_answer(run, json!([])))
                .mount(&fixture.model)
                .await;
            Mock::given(wm_method("GET"))
                .and(wm_path(MODEL_EVIDENCE))
                .respond_with(png_answer())
                .mount(&fixture.model)
                .await;
            let (status, _, body) = send(fixture.state.clone(), "GET", EVIDENCE, None).await;
            assert_eq!(status, 403);
            assert_eq!(json_of(&body)["error"]["code"], "browser_evidence_withheld");
            assert!(hits(&fixture.model, "/evidence/").await.is_empty(), "no image request upstream");
        }
    }

    // ---- W3-DEC-GRANT: the parent grant the start takes ---------------------------------------------------------------

    #[tokio::test]
    async fn the_parent_grant_names_the_start_hosts_and_a_ten_minute_lifetime_with_no_kind() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(started_answer())
            .mount(&fixture.model)
            .await;
        let mut body = start_json();
        body["allowed_domains"] = json!(["Example.com", "docs.example.org", "example.com"]);
        let (status, _, _) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(body)).await;
        assert_eq!(status, 202);
        let grants = bodies(&fixture.model, "POST", MODEL_GRANT).await;
        assert_eq!(
            grants,
            vec![json!({"allowed_domains": ["docs.example.org", "example.com"], "zdr": false, "ttl_s": 600})]
        );
    }

    // ---- W3-DEC-GWFIX2: the start reads the zero-retention posture live ----------------------------------------------

    #[tokio::test]
    async fn a_start_reads_the_org_posture_live_so_a_cached_not_zdr_does_not_admit_it() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        cached_not_zdr_then(&mut fixture, org_record(true)).await;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 412);
        assert_eq!(json_of(&body)["error"]["code"], "zdr_feature_not_supported");
        assert!(hits(&fixture.user_core, "/internal/spaces/").await.is_empty());
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    #[tokio::test]
    async fn a_start_whose_live_org_read_fails_is_503_before_any_control_call() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        cached_not_zdr_then(
            &mut fixture,
            ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal"}})),
        )
        .await;
        let (status, _, body) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(start_json())).await;
        assert_eq!(status, 503);
        assert_eq!(json_of(&body)["error"]["code"], "zdr_decision_unavailable");
        assert!(hits(&fixture.user_core, "/internal/spaces/").await.is_empty());
        assert!(hits(&fixture.model, "").await.is_empty());
    }

    // ---- W3-DEC-BROWSER: a start that names no thread creates it first, under the send key (D4) --------------------

    const THREAD_DECISION: &str = "/api/v1/internal/spaces/thread-decision";
    const MODEL_THREADS: &str = "/v1/threads";
    const NEW_THREAD: &str = "thread-new-1";
    const SEND_KEY: &str = "space-browser-send-0123456789abcdef";

    /// Control's thread decision for a run that names no thread: the fields the create reads, and the token.
    fn thread_decided() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": {
            "decision": {
                "space_ref": SPACE,
                "decision_ref": "thread-ref-1",
                "recipient_audience_ref": "audience-1",
                "recipient_audience_revision": 4,
                "recipient_audience_hash": "audience-hash-1",
                "privacy_policy_ref": "policy-1",
                "authority_revision": 7,
                "resource_authorization_ref": "resource-1",
                "action_schema_hash": "schema-1",
                "payload_digest": "payload-1",
                "idempotency_key": SEND_KEY
            },
            "token": "thread-decision-token"
        }}))
    }

    /// model-gateway's answer to the shared-room create: the thread id at the top level.
    fn thread_created(thread: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"thread_id": thread}))
    }

    /// model-gateway's 202 for a start on `thread`.
    fn started_in(thread: &str) -> ResponseTemplate {
        ResponseTemplate::new(202)
            .set_body_json(json!({"run_id": RUN, "thread_id": thread, "plan_id": "plan-1"}))
    }

    /// A start that names no thread: the goal with surrounding space, one host, and the client send id.
    fn new_thread_start() -> Value {
        json!({
            "goal": format!("  {GOAL} \n"),
            "allowed_domains": ["example.com"],
            "client_send_id": SEND,
        })
    }

    /// Mounts Control's thread decision and model-gateway's create on a fixture. `threads` is the create's answer.
    async fn mount_create(fixture: &Fixture, threads: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(THREAD_DECISION))
            .respond_with(thread_decided())
            .mount(&fixture.user_core)
            .await;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_THREADS))
            .respond_with(threads)
            .mount(&fixture.model)
            .await;
    }

    #[tokio::test]
    async fn a_start_that_names_no_thread_creates_it_under_the_send_key_and_starts_the_run_on_it() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        mount_create(&fixture, thread_created(NEW_THREAD)).await;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(started_in(NEW_THREAD))
            .mount(&fixture.model)
            .await;
        let (status, _, bytes) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(new_thread_start())).await;
        assert_eq!(status, 202, "{}", json_of(&bytes));
        assert_eq!(json_of(&bytes)["data"]["thread_id"], NEW_THREAD);

        // The thread: Control's decision under the send key, then the create with the trimmed goal as its first post.
        let thread_decisions = bodies(&fixture.user_core, "POST", THREAD_DECISION).await;
        assert_eq!(thread_decisions.len(), 1);
        assert_eq!(thread_decisions[0]["space_ref"], SPACE);
        assert_eq!(thread_decisions[0]["session_key"], SEND_KEY);
        assert_eq!(thread_decisions[0]["idempotency_key"], SEND_KEY);
        let threads = bodies(&fixture.model, "POST", MODEL_THREADS).await;
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0]["space_context"]["space_id"], SPACE);
        assert_eq!(threads[0]["session_key"], SEND_KEY);
        assert_eq!(threads[0]["first_message"]["content"], GOAL);
        assert_eq!(threads[0]["first_message"]["post_kind"], "post");
        assert_eq!(threads[0]["first_message"]["client_message_id"], SEND_KEY);

        // The run: the browser decision binds no thread, and the goal's append is minted for the created thread.
        let browser = bodies(&fixture.user_core, "POST", "/api/v1/internal/spaces/browser-run-decision").await;
        assert_eq!(browser.len(), 1);
        assert_eq!(browser[0]["thread_id"], "");
        assert_eq!(browser[0]["goal_digest"], GOAL_DIGEST);
        assert_eq!(browser[0]["idempotency_key"], SEND_KEY);
        let appends = bodies(&fixture.user_core, "POST", "/api/v1/internal/spaces/thread-append-decision").await;
        assert_eq!(appends.len(), 1);
        assert_eq!(appends[0]["thread_id"], NEW_THREAD);
        assert_eq!(appends[0]["content_digest"], GOAL_DIGEST);
        let starts = bodies(&fixture.model, "POST", MODEL_START).await;
        assert_eq!(starts.len(), 1);
        assert_eq!(starts[0]["thread_id"], NEW_THREAD);
        assert_eq!(starts[0]["goal"], GOAL);
        assert!(starts[0].get("space_context").is_none(), "the start carries decided authority, not a space context");
    }

    #[tokio::test]
    async fn a_threads_412_is_the_zdr_refusal_and_no_decision_or_start_follows_it() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        mount_create(
            &fixture,
            ResponseTemplate::new(412).set_body_json(json!({"error": {"code": "zdr_durable_mutation_forbidden"}})),
        )
        .await;
        let (status, _, bytes) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(new_thread_start())).await;
        assert_eq!(status, 412);
        assert_eq!(json_of(&bytes)["error"]["code"], "zdr_feature_not_supported");
        assert_eq!(hits(&fixture.user_core, "thread-decision").await.len(), 1);
        assert!(hits(&fixture.user_core, "browser-run-decision").await.is_empty());
        assert!(hits(&fixture.user_core, "thread-append-decision").await.is_empty());
        assert!(hits(&fixture.model, "/v1/browser/grants").await.is_empty());
        assert!(hits(&fixture.model, MODEL_START).await.is_empty());
    }

    #[tokio::test]
    async fn a_threads_answer_without_a_valid_thread_id_is_502_and_no_decision_or_start_follows_it() {
        for answer in [json!({"ok": true}), json!({"thread_id": "not a valid id"})] {
            let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
            fixture.state.space_features.browser_run = true;
            mount_create(&fixture, ResponseTemplate::new(200).set_body_json(answer.clone())).await;
            let (status, _, bytes) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(new_thread_start())).await;
            assert_eq!(status, 502, "{answer}");
            assert_eq!(json_of(&bytes)["error"]["code"], "invalid_model_gateway_response", "{answer}");
            assert!(hits(&fixture.user_core, "browser-run-decision").await.is_empty(), "{answer}");
            assert!(hits(&fixture.model, MODEL_START).await.is_empty(), "{answer}");
        }
    }

    #[tokio::test]
    async fn a_retry_with_the_same_client_send_id_sends_the_same_keys_to_control_and_to_model_gateway() {
        let mut fixture = fixture(org_record(false), granted_read(), granted_browser()).await;
        fixture.state.space_features.browser_run = true;
        mount_create(&fixture, thread_created(NEW_THREAD)).await;
        Mock::given(wm_method("POST"))
            .and(wm_path(MODEL_START))
            .respond_with(started_in(NEW_THREAD))
            .mount(&fixture.model)
            .await;
        for _ in 0..2 {
            let (status, _, _) = send(fixture.state.clone(), "POST", ROOM_RUNS, Some(new_thread_start())).await;
            assert_eq!(status, 202);
        }
        let creates = bodies(&fixture.user_core, "POST", THREAD_DECISION).await;
        let threads = bodies(&fixture.model, "POST", MODEL_THREADS).await;
        let browser = bodies(&fixture.user_core, "POST", "/api/v1/internal/spaces/browser-run-decision").await;
        let appends = bodies(&fixture.user_core, "POST", "/api/v1/internal/spaces/thread-append-decision").await;
        let starts = bodies(&fixture.model, "POST", MODEL_START).await;
        assert_eq!(creates.len(), 2);
        assert_eq!(threads.len(), 2);
        assert_eq!(browser.len(), 2);
        assert_eq!(appends.len(), 2);
        assert_eq!(starts.len(), 2);
        assert!(creates.iter().all(|body| body["session_key"] == SEND_KEY && body["idempotency_key"] == SEND_KEY));
        assert!(threads
            .iter()
            .all(|body| body["session_key"] == SEND_KEY && body["first_message"]["client_message_id"] == SEND_KEY));
        assert!(browser.iter().all(|body| body["idempotency_key"] == SEND_KEY && body["thread_id"] == ""));
        assert!(appends.iter().all(|body| {
            body["idempotency_key"] == "space-browser-append-send-0123456789abcdef" && body["thread_id"] == NEW_THREAD
        }));
        assert!(starts.iter().all(|body| {
            body["space_browser_authority"]["idempotency_key"] == SEND_KEY && body["thread_id"] == NEW_THREAD
        }));
    }
}
