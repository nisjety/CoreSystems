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
