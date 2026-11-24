//! WS04-S6: room member administration on the Control writer.
//!
//! With `SpaceFeatureFlags::membership_writer_api` on, adding, re-roling and removing (or
//! leaving) a named room's people goes to Control's actor-carrying routes
//! (`POST|PATCH|DELETE /api/v1/internal/spaces/{ref}/members[/{subject_id}]`). Control owns the
//! role matrix and decides under the caller's signed delegation: this module checks no role, so
//! it can never widen what Control allows. The organization and the acting user come from the
//! session, never from the request body. Control's typed error body and status pass through
//! unchanged; an outage is never reported as success.
//!
//! After an add or a remove the recipient audience is reconciled through the Application action
//! `spaceMembers:reconcileRoomAudienceForGateway`. The Control write is authoritative and is
//! never rolled back by an audience failure: the response says `audience.state` is `pending`,
//! `current` or `unconfirmed` (plus `meta.warnings:["room_audience_unconfirmed"]`), and the
//! reconcile cron heals the last case. Flag off keeps the legacy Convex-then-declare path in the
//! parent module unchanged. No message, topic or other content moves here, so there is nothing
//! for Zero Data Retention to refuse and no model is called.
//!
//! WS04-S7 adds `POST /api/v1/spaces/{space_ref}/archive` and `/unarchive` behind
//! `SpaceFeatureFlags::room_archive_api`, on the same footing: Control's actor-carrying routes own
//! the state and the role rule (manager or owner), the gateway forwards the session's identity and
//! the optional idempotency key, and Control's typed refusal passes through with its status. An
//! archived room reads and refuses writes; that is Control's registration state, not this flag.
//!
//! Every Control failure on these routes is relayed through [`relay_control_error`]: a typed body
//! keeps its status and only `code`, `message` and scalar `details`; anything else is a 503.
//! Adding, re-roling, removing and archiving share one per-user ceiling of 30 operations a minute.
//!
//! D18 (per-member room history) adds `PUT` and `GET /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`
//! behind `SpaceFeatureFlags::member_history_windows`, and the invite's optional `share_past_conversation` on the add
//! route, on the same footing: Control owns the role rule, the audit row and the floor every read signs, this module
//! checks no role and holds no state, and a window can only hide. See the D18 section below.

use std::sync::OnceLock;

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::put,
    Json, Router,
};
use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::Method;
use serde_json::{json, Map, Value};

use super::convex_gateway_call;
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    rate_limit::RateLimiter,
    upstream::proxy_json,
};

type Reply = (StatusCode, Json<Value>);

const ROOM_ROLES: [&str; 4] = ["viewer", "editor", "manager", "owner"];
const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;
/// Member administration and archive calls per user per minute (plan: 30/min). A test run shares
/// one process-wide limiter and one user, so the ceiling is lifted there; the limiter itself is
/// tested directly with its own small bucket.
const MEMBER_OPS_PER_MINUTE: f64 = if cfg!(test) { 100_000.0 } else { 30.0 };
pub(crate) const MAX_RELAYED_MESSAGE_CHARS: usize = 300;
pub(crate) const MAX_RELAYED_DETAILS: usize = 16;
const MAX_ARCHIVE_BODY_BYTES: usize = 4096;

/// Process-wide limiter, built on first use from the gateway's Dragonfly connection (in-process
/// fallback when the cache is off). Not an `AppState` field, like the directory limiter, so no
/// test state literal changes.
static MEMBER_OPS_LIMITER: OnceLock<RateLimiter> = OnceLock::new();

/// One member-operation token for the caller, or the typed 429. The Retry-After seconds travel in
/// `details.retry_after_seconds` because these functions answer a bare `(status, json)`.
async fn member_ops_allowed(
    state: &AppState,
    org_id: &str,
    user: &AuthenticatedUser,
) -> Result<(), Reply> {
    let limiter = MEMBER_OPS_LIMITER
        .get_or_init(|| RateLimiter::from_cache_with_rpm(&state.cache, MEMBER_OPS_PER_MINUTE));
    check_member_ops(limiter, org_id, &user.user_id).await
}

async fn check_member_ops(limiter: &RateLimiter, org_id: &str, user_id: &str) -> Result<(), Reply> {
    match limiter
        .try_acquire(&format!("space-member-ops:{}:{user_id}", org_id.trim()))
        .await
    {
        Ok(()) => Ok(()),
        Err(retry_after) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(error_with_details(
                "rate_limited",
                "Too many room changes. Retry shortly.",
                json!({ "retry_after_seconds": retry_after.max(1) }),
            )),
        )),
    }
}

/// True for a short lowercase token, the shape of every Control error code.
pub(crate) fn is_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Control's failure answer as the browser envelope. A typed body (`{error:{code,message,details}}`,
/// what every WS04 route answers) keeps its status and is rebuilt from `code`, a bounded `message`
/// and the scalar entries of `details` only, so nothing else an upstream puts in a body (extra
/// keys, text of another service) reaches the browser. A body without a well-formed code, and a
/// typed 401 (which the SPA would read as an expired session), become a 503 that says nothing was
/// changed: an outage is never reported as a refusal and never as success.
fn relay_control_error(status: StatusCode, body: Value) -> Reply {
    let typed = body
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| {
            let code = error.get("code")?.as_str().filter(|code| is_error_code(code))?;
            let message: String = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(MAX_RELAYED_MESSAGE_CHARS)
                .collect();
            let mut out = Map::new();
            out.insert("code".to_owned(), json!(code));
            out.insert("message".to_owned(), json!(message));
            if let Some(details) = error.get("details").and_then(Value::as_object) {
                let scalars: Map<String, Value> = details
                    .iter()
                    .filter(|(_, value)| !value.is_array() && !value.is_object())
                    .take(MAX_RELAYED_DETAILS)
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                out.insert("details".to_owned(), Value::Object(scalars));
            }
            Some(Value::Object(out))
        });
    match typed {
        Some(error) if status != StatusCode::UNAUTHORIZED => (status, Json(json!({ "error": error }))),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "room_admin_unavailable",
                "The room could not be changed. Nothing was changed.",
            )),
        ),
    }
}

/// True when the Control member writer is the path for this gateway.
pub(super) fn writer_enabled(state: &AppState) -> bool {
    state.space_features.membership_writer_api
}

/// A refused feature answers 404 `feature_disabled` (plan: gateway section).
pub(crate) fn feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "Room member administration is not enabled.",
        )),
    )
}

/// Lets the SPA know the writer path is on, so it offers role and leave controls only when the
/// server will accept them. Additive: an older SPA ignores the field.
pub(super) fn annotate_roster(state: &AppState, status: StatusCode, body: &mut Value) {
    if !status.is_success() || !writer_enabled(state) {
        return;
    }
    if let Some(data) = body.get_mut("data").and_then(Value::as_object_mut) {
        data.insert("writer_api".to_owned(), Value::Bool(true));
    }
}

fn actor_of(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn members_url(state: &AppState, space_ref: &str, member_id: Option<&str>) -> String {
    let base = format!(
        "{}/api/v1/internal/spaces/{}/members",
        state.user_core_url,
        urlencoding::encode(space_ref)
    );
    match member_id {
        Some(member) => format!("{base}/{}", urlencoding::encode(member)),
        None => base,
    }
}

fn invalid(field: &'static str, message: &'static str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "invalid_request",
            message,
            json!({ "field": field }),
        )),
    )
}

/// The organization and trimmed Space ref, or the typed refusal every Space route gives.
async fn scope(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
) -> Result<(String, String), Reply> {
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    let space_ref = space_ref.trim().to_owned();
    if org_id.is_empty() || space_ref.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(error(
                "active_membership_required",
                "An active organization membership and Space are required.",
            )),
        ));
    }
    Ok((org_id, space_ref))
}

fn optional_role(body: &Value) -> Result<Option<String>, Reply> {
    match body.get("role") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(role)) if ROOM_ROLES.contains(&role.trim()) => {
            Ok(Some(role.trim().to_owned()))
        }
        Some(_) => Err(invalid("role", "The role must be viewer, editor, manager or owner.")),
    }
}

fn optional_idempotency_key(body: &Value) -> Result<Option<String>, Reply> {
    match body.get("idempotency_key") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(key))
            if !key.trim().is_empty() && key.trim().len() <= MAX_IDEMPOTENCY_KEY_LEN =>
        {
            Ok(Some(key.trim().to_owned()))
        }
        Some(_) => Err(invalid("idempotency_key", "The idempotency key is not valid.")),
    }
}

/// What the Convex reconcile action said, as the one word the SPA shows.
///
/// `pending`: the new audience was applied (or the active one is not acknowledged yet), so the
/// added person may need a moment before they can read. `current`: nothing to change. Every
/// failure and refusal is `unconfirmed`: the cron retries, and the write already stands.
fn audience_state(status: &str) -> &'static str {
    match status {
        "noop" => "current",
        "replaced" | "waiting" => "pending",
        _ => "unconfirmed",
    }
}

async fn reconcile_audience(state: &AppState, org_id: &str, space_ref: &str) -> &'static str {
    let status = convex_gateway_call(
        state,
        "action",
        "spaceMembers:reconcileRoomAudienceForGateway",
        json!({ "externalOrgId": org_id, "spaceRef": space_ref }),
    )
    .await
    .ok()
    .and_then(|value| {
        value
            .get("status")
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    audience_state(status.as_deref().unwrap_or("unavailable"))
}

/// WS11-S8: tells the Application attention projection that `invitee` was added to the room.
///
/// Called only after Control confirmed a real change (`changed: true`), so a replayed or no-op add
/// raises nothing. Best effort: the Control write is authoritative and an attention failure never
/// changes the add's response. The mutation refuses when `SPACE_ATTENTION_INGEST` is off, when the
/// invitee is not a member of the organization (agents included), and for a self-add. Only ids
/// travel: no title, name or message text, so nothing here needs a Zero Data Retention refusal.
async fn record_invite(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    invitee: &str,
    idempotency_key: Option<&str>,
) {
    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "spaceRef": space_ref,
        "inviteeAuthId": invitee,
    });
    if let Some(key) = idempotency_key {
        args["idempotencyKey"] = json!(key);
    }
    if convex_gateway_call(state, "mutation", "spaceAttention:recordInviteForGateway", args)
        .await
        .is_err()
    {
        tracing::warn!(target: "spaces", "space invite attention signal was not recorded");
    }
}

fn with_audience_warning(mut body: Value, audience: &str) -> Value {
    if audience == "unconfirmed" {
        body["meta"] = json!({ "warnings": ["room_audience_unconfirmed"] });
    }
    body
}

/// `POST /api/v1/spaces/{space_ref}/members` on the Control writer. `body.member_id` (or
/// `subject_id`) names the person; `body.role` defaults to Control's own default (editor). D18:
/// `body.share_past_conversation` (a boolean) is the invite's choice: absent or true leaves the room's
/// own history mode in charge, false joins the person without the conversation posted before their join.
/// It is forwarded as given and the committed add answers `data.history_window` (see [`history_window_view`]).
pub(crate) async fn add_member(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    if !writer_enabled(state) {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let member_id = body
        .get("subject_id")
        .or_else(|| body.get("member_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if member_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error("member_required", "A member is required.")),
        );
    }
    let role = match optional_role(body) {
        Ok(role) => role,
        Err(reply) => return reply,
    };
    let key = match optional_idempotency_key(body) {
        Ok(key) => key,
        Err(reply) => return reply,
    };
    let share_past_conversation = match optional_share_past_conversation(body) {
        Ok(choice) => choice,
        Err(reply) => return reply,
    };
    let mut payload = Map::new();
    payload.insert("subject_id".to_owned(), json!(member_id));
    if let Some(role) = role {
        payload.insert("role".to_owned(), json!(role));
    }
    let idempotency_key = key.clone();
    if let Some(key) = key {
        payload.insert("idempotency_key".to_owned(), json!(key));
    }
    // D18: the invite's choice goes to Control exactly as given, `false` included, and is never gated by the
    // gateway's own window switch: a "don't share" is refused by Control while its switch is off, never dropped.
    if let Some(share) = share_past_conversation {
        payload.insert("share_past_conversation".to_owned(), json!(share));
    }
    if let Err(reply) = member_ops_allowed(state, &org_id, user).await {
        return reply;
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::POST,
        &members_url(state, &space_ref, None),
        Some(Value::Object(payload)),
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    let data = unwrap_data(&control);
    let audience = reconcile_audience(state, &org_id, &space_ref).await;
    if data.get("changed").and_then(Value::as_bool).unwrap_or(false) {
        record_invite(state, user, &org_id, &space_ref, &member_id, idempotency_key.as_deref())
            .await;
    }
    let mut out = json!({"data": {
        "member_count": data.get("member_count").and_then(Value::as_u64).unwrap_or(0),
        "changed": data.get("changed").and_then(Value::as_bool).unwrap_or(false),
        "role": data.get("role").cloned().unwrap_or(Value::Null),
        "audience": { "state": audience },
    }});
    // D18: the window the new member reads under, only when Control reports one (a committed add; a replay or an
    // already-active member has none). It is a courtesy on a write that already stands, so a malformed one is left
    // out rather than turned into an error, and a window is never made up here.
    if let Some(window) = history_window_view(data.get("history_window")) {
        out["data"]["history_window"] = window;
    }
    (StatusCode::OK, Json(with_audience_warning(out, audience)))
}

/// `PATCH /api/v1/spaces/{space_ref}/members/{member_id}` on the Control writer. The recipient
/// set is unchanged by a role change, so no audience reconcile follows.
pub(crate) async fn change_member_role(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    member_id: &str,
    body: &Value,
) -> Reply {
    if !writer_enabled(state) {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let member_id = match resolve_member_id(user, member_id) {
        Some(member) => member,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("member_required", "A member is required.")),
            )
        }
    };
    let role = match optional_role(body) {
        Ok(Some(role)) => role,
        Ok(None) => return invalid("role", "A role is required."),
        Err(reply) => return reply,
    };
    let key = match optional_idempotency_key(body) {
        Ok(key) => key,
        Err(reply) => return reply,
    };
    let mut payload = Map::new();
    payload.insert("role".to_owned(), json!(role));
    match body.get("expected_revision") {
        None | Some(Value::Null) => {}
        Some(value) => match value.as_i64().filter(|revision| *revision >= 0) {
            Some(revision) => {
                payload.insert("expected_revision".to_owned(), json!(revision));
            }
            None => {
                return invalid(
                    "expected_revision",
                    "The expected revision must be a non-negative integer.",
                )
            }
        },
    }
    if let Some(key) = key {
        payload.insert("idempotency_key".to_owned(), json!(key));
    }
    if let Err(reply) = member_ops_allowed(state, &org_id, user).await {
        return reply;
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::PATCH,
        &members_url(state, &space_ref, Some(&member_id)),
        Some(Value::Object(payload)),
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    let data = unwrap_data(&control);
    (
        StatusCode::OK,
        Json(json!({"data": {
            "role": data.get("role").cloned().unwrap_or(Value::Null),
            "from_role": data.get("from_role").cloned().unwrap_or(Value::Null),
            "changed": data.get("changed").and_then(Value::as_bool).unwrap_or(false),
        }})),
    )
}

/// `DELETE /api/v1/spaces/{space_ref}/members/{member_id}` on the Control writer. `member_id`
/// may be the caller's own id or the literal `me`: both leave the room. The audience is
/// reconciled afterwards because the recipient set shrank.
pub(crate) async fn remove_member(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    member_id: &str,
) -> Reply {
    if !writer_enabled(state) {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let member_id = match resolve_member_id(user, member_id) {
        Some(member) => member,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error("member_required", "A member is required.")),
            )
        }
    };
    if let Err(reply) = member_ops_allowed(state, &org_id, user).await {
        return reply;
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::DELETE,
        &members_url(state, &space_ref, Some(&member_id)),
        None,
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    let data = unwrap_data(&control);
    let audience = reconcile_audience(state, &org_id, &space_ref).await;
    let out = json!({"data": {
        "changed": data.get("changed").and_then(Value::as_bool).unwrap_or(false),
        "left": data.get("left").and_then(Value::as_bool).unwrap_or(false),
        "member_count": data.get("member_count").and_then(Value::as_u64).unwrap_or(0),
        "audience": { "state": audience },
    }});
    (StatusCode::OK, Json(with_audience_warning(out, audience)))
}

/// The literal `me` is the caller. Anything else is taken as given; Control decides whether it
/// is the actor (leave) or someone else (remove).
fn resolve_member_id(user: &AuthenticatedUser, member_id: &str) -> Option<String> {
    let member = member_id.trim();
    if member.is_empty() {
        return None;
    }
    if member == "me" {
        return Some(user.user_id.clone());
    }
    Some(member.to_owned())
}

/// Axum handler for `PATCH /api/v1/spaces/{space_ref}/members/{member_id}`.
pub(super) async fn change_member_role_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, member_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    change_member_role(&state, &user, &space_ref, &member_id, &body).await
}

// ---------------------------------------------------------------------------------------------
// WS04-S7: archive and restore (`POST /api/v1/spaces/{space_ref}/archive|unarchive`).
// ---------------------------------------------------------------------------------------------

fn archive_feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error("feature_disabled", "Room archive is not enabled.")),
    )
}

/// The optional JSON body of an archive call: absent or blank is `{}`; anything but an object, an
/// unreadable body or an oversized one is a 400 before any upstream call.
fn parse_optional_body(raw: &Bytes) -> Result<Value, Reply> {
    if raw.len() > MAX_ARCHIVE_BODY_BYTES {
        return Err(invalid("body", "The request body is too large."));
    }
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    match serde_json::from_slice::<Value>(raw) {
        Ok(body) if body.is_object() => Ok(body),
        _ => Err(invalid("body", "The request body must be a JSON object.")),
    }
}

fn archive_url(state: &AppState, space_ref: &str, archive: bool) -> String {
    format!(
        "{}/api/v1/internal/spaces/{}/{}",
        state.user_core_url,
        urlencoding::encode(space_ref),
        if archive { "archive" } else { "unarchive" }
    )
}

/// Archives (`archive`) or restores a named room on Control. Control applies the role rule
/// (manager or owner), refuses the organization room (`room_not_archivable`) and a personal Space,
/// and answers `{data:{state,changed,replayed,archived_at,authority_revision}}`; the browser gets
/// `{data:{state,changed,archived_at}}`. Org and actor come from the session, never the body. The
/// call is idempotent set-to-state, so a repeat answers `changed:false`. Nothing here reads or
/// stores conversation content and no model is called, so a Zero Data Retention organization
/// behaves the same.
pub(crate) async fn set_room_archived(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
    archive: bool,
) -> Reply {
    if !state.space_features.room_archive_api {
        return archive_feature_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let key = match optional_idempotency_key(body) {
        Ok(key) => key,
        Err(reply) => return reply,
    };
    let mut payload = Map::new();
    if let Some(key) = key {
        payload.insert("idempotency_key".to_owned(), json!(key));
    }
    if let Err(reply) = member_ops_allowed(state, &org_id, user).await {
        return reply;
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::POST,
        &archive_url(state, &space_ref, archive),
        Some(Value::Object(payload)),
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    let data = unwrap_data(&control);
    let expected = if archive { "archived" } else { "active" };
    (
        StatusCode::OK,
        Json(json!({"data": {
            "state": data.get("state").and_then(Value::as_str).unwrap_or(expected),
            "changed": data.get("changed").and_then(Value::as_bool).unwrap_or(false),
            "archived_at": data.get("archived_at").cloned().filter(Value::is_string),
        }})),
    )
}

/// Axum handler for `POST /api/v1/spaces/{space_ref}/archive`. The body is optional.
pub(super) async fn archive_room_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    raw: Bytes,
) -> Reply {
    match parse_optional_body(&raw) {
        Ok(body) => set_room_archived(&state, &user, &space_ref, &body, true).await,
        Err(reply) => reply,
    }
}

/// Axum handler for `POST /api/v1/spaces/{space_ref}/unarchive`. The body is optional.
pub(super) async fn unarchive_room_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    raw: Bytes,
) -> Reply {
    match parse_optional_body(&raw) {
        Ok(body) => set_room_archived(&state, &user, &space_ref, &body, false).await,
        Err(reply) => reply,
    }
}

// ---------------------------------------------------------------------------------------------
// WS04-S2: room name and topic (`PATCH /api/v1/spaces/{space_ref}`).
// ---------------------------------------------------------------------------------------------

/// Roles that may rename a room or set its topic. The caller's live role comes from Control's
/// membership decision (`resolve_space_role`); Application trusts the gateway for the role gate,
/// exactly as it does for every other `*ForGateway` mutation.
const DETAILS_ROLES: [&str; 2] = ["owner", "manager"];

/// Raw-body ceiling checked before any upstream call. The authoritative rules (NFC, trim,
/// whitespace collapse, name 1-120, topic 0-250, no control or bidi characters) live in
/// Application's `spaceRoomRules.ts` and run on every write; this only refuses an absurd body.
const DETAILS_RAW_MAX_CHARS: usize = 2000;

fn details_invalid(field: &str, reason: &str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "room_details_invalid",
            "The room name or topic is not valid.",
            json!({ "field": field, "reason": reason }),
        )),
    )
}

fn details_unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "room_details_unavailable",
            "The room details could not be saved. Nothing was changed.",
        )),
    )
}

/// One optional string field of the body: absent or null is "not sent".
fn details_field(body: &Value, field: &'static str) -> Result<Option<String>, Reply> {
    match body.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if text.chars().count() <= DETAILS_RAW_MAX_CHARS => {
            Ok(Some(text.clone()))
        }
        Some(Value::String(_)) => Err(details_invalid(field, "too_long")),
        Some(_) => Err(details_invalid(field, "not_a_string")),
    }
}

/// Runs one Convex mutation and keeps its error text. `convex_gateway_call` drops it, but this
/// route needs Application's stable messages to answer with a typed code. Convex reports a
/// thrown error as a non-success status (or `status: "error"`) with an `errorMessage`; `Err(None)`
/// is every other failure (not configured, unreachable, unreadable).
pub(super) async fn convex_mutation_message(
    state: &AppState,
    path: &str,
    mut args: Value,
) -> Result<Value, Option<String>> {
    let convex_url = state.application_convex_url.clone();
    let service_key = state.application_convex_service_key.clone();
    if convex_url.trim().is_empty() || service_key.trim().is_empty() {
        return Err(None);
    }
    args.as_object_mut()
        .ok_or(None)?
        .insert("serviceKey".to_owned(), Value::String(service_key));
    let response = crate::upstream::send_with_retry(
        state
            .client
            .post(format!("{}/api/mutation", convex_url.trim_end_matches('/')))
            .json(&json!({ "path": path, "args": args })),
    )
    .await
    .map_err(|_| None)?;
    let http_ok = response.status().is_success();
    let body = response.json::<Value>().await.map_err(|_| None)?;
    if http_ok && body.get("status").and_then(Value::as_str) != Some("error") {
        return body.get("value").cloned().ok_or(None);
    }
    Err(body
        .get("errorMessage")
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// `room_details_invalid:<field>:<reason>` out of an Application error message, as validated
/// tokens only (nothing else from the message is ever returned).
fn invalid_details_from(message: &str) -> Option<(String, String)> {
    const PREFIX: &str = "room_details_invalid:";
    let start = message.find(PREFIX)? + PREFIX.len();
    let tail = message[start..]
        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .next()
        .unwrap_or_default();
    let (field, reason) = tail.split_once(':')?;
    let token_ok = !reason.is_empty() && reason.chars().all(|c| c.is_ascii_lowercase() || c == '_');
    (matches!(field, "name" | "topic" | "details") && token_ok)
        .then(|| (field.to_owned(), reason.to_owned()))
}

/// Maps Application's stable mutation messages to the typed browser errors.
fn map_details_failure(message: Option<&str>) -> Reply {
    let Some(message) = message else {
        return details_unavailable();
    };
    if message.contains("room_details_invalid:") {
        return match invalid_details_from(message) {
            Some((field, reason)) => details_invalid(&field, &reason),
            None => details_invalid("details", "invalid"),
        };
    }
    if message.contains("space_not_found") {
        return (
            StatusCode::NOT_FOUND,
            Json(error("space_not_found", "Space is not available.")),
        );
    }
    if message.contains("space_kind_not_editable") {
        return (
            StatusCode::CONFLICT,
            Json(error(
                "space_kind_not_editable",
                "The name and topic cannot be changed for this room.",
            )),
        );
    }
    if message.contains("room_not_active") {
        return (
            StatusCode::CONFLICT,
            Json(error("room_not_active", "This room is not active.")),
        );
    }
    details_unavailable()
}

/// `PATCH /api/v1/spaces/{space_ref}` body `{name?, topic?}`. Owner or manager only, decided from
/// Control's live membership BEFORE any Application request. An empty topic clears it. The topic
/// is a display label: it is never composed into a prompt, an event or a notification, so a Zero
/// Data Retention organization behaves the same and no model is called.
pub(crate) async fn update_room_details(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.room_details_ui {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let name = match details_field(body, "name") {
        Ok(name) => name,
        Err(reply) => return reply,
    };
    let topic = match details_field(body, "topic") {
        Ok(topic) => topic,
        Err(reply) => return reply,
    };
    if name.is_none() && topic.is_none() {
        return details_invalid("details", "empty_update");
    }
    match super::resolve_space_role(state, user, &org_id, &space_ref).await {
        Ok(role) if DETAILS_ROLES.contains(&role.as_str()) => {}
        Ok(_) => {
            return (
                StatusCode::FORBIDDEN,
                Json(error(
                    "room_role_forbidden",
                    "Only a room owner or manager can change the room's name or topic.",
                )),
            )
        }
        Err(refusal) => return refusal,
    }
    let mut args = Map::new();
    args.insert("externalAuthId".to_owned(), json!(user.user_id));
    args.insert("externalOrgId".to_owned(), json!(org_id));
    args.insert("spaceRef".to_owned(), json!(space_ref));
    if let Some(name) = name {
        args.insert("name".to_owned(), json!(name));
    }
    if let Some(topic) = topic {
        args.insert("topic".to_owned(), json!(topic));
    }
    match convex_mutation_message(state, "spaces:updateRoomDetailsForGateway", Value::Object(args))
        .await
    {
        Ok(result) => (
            StatusCode::OK,
            Json(json!({"data": {
                "space_ref": result.get("spaceRef").and_then(Value::as_str).unwrap_or(&space_ref),
                "name": result.get("name").and_then(Value::as_str).unwrap_or_default(),
                "topic": result.get("topic").and_then(Value::as_str),
            }})),
        ),
        Err(message) => map_details_failure(message.as_deref()),
    }
}

/// Axum handler for `PATCH /api/v1/spaces/{space_ref}`.
pub(super) async fn update_room_details_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(body): Json<Value>,
) -> Reply {
    update_room_details(&state, &user, &space_ref, &body).await
}

// ---------------------------------------------------------------------------------------------
// WS04-S8: room visibility, join role and self-join (`POST .../join`, `PATCH .../settings`).
//
// Both ride Control's actor-carrying routes under the caller's signed delegation. Control owns the
// rules (who may change settings, the per-org directory policy, which rooms are open, the role a
// joiner gets); this module checks no role and trusts no flag as authorization. The rollout flag
// `SPACE_ROOM_DIRECTORY_API` only decides whether the routes exist. No message, topic or other
// content moves, so a Zero Data Retention organization behaves the same.
// ---------------------------------------------------------------------------------------------

const VISIBILITIES: [&str; 2] = ["private", "org_public"];
/// A self-join never lands above editor, so the open-room role is one of these two.
const JOIN_ROLES: [&str; 2] = ["viewer", "editor"];
const HISTORY_MODES: [&str; 2] = ["full", "since_join"];

fn directory_feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error("feature_disabled", "The room directory is not enabled.")),
    )
}

fn space_ops_url(state: &AppState, space_ref: &str, op: &str) -> String {
    format!(
        "{}/api/v1/internal/spaces/{}/{op}",
        state.user_core_url,
        urlencoding::encode(space_ref)
    )
}

/// `POST /api/v1/spaces/{space_ref}/join`: the caller joins an open room of their organization.
///
/// Control answers 404 `room_not_found` for a private room, a disabled organization, a personal
/// Space, another organization's room and an unknown ref alike (byte-equal), and 409
/// `room_archived`; the typed refusal passes through. After a success the recipient audience is
/// reconciled (the joiner cannot read until it is current), and the response says so through
/// `audience.state` and `recipient_ready:false`; the SPA polls `.../membership` until it turns
/// true. The Control write is never rolled back by an audience failure.
pub(crate) async fn join_room(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.room_directory_api {
        return directory_feature_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let Some(fields) = body.as_object() else {
        return invalid("body", "The request body must be a JSON object.");
    };
    if fields.keys().any(|key| key != "idempotency_key") {
        return invalid("body", "Only idempotency_key is accepted.");
    }
    let key = match optional_idempotency_key(body) {
        Ok(key) => key,
        Err(reply) => return reply,
    };
    let mut payload = Map::new();
    if let Some(key) = key {
        payload.insert("idempotency_key".to_owned(), json!(key));
    }
    if let Err(reply) = member_ops_allowed(state, &org_id, user).await {
        return reply;
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::POST,
        &space_ops_url(state, &space_ref, "join"),
        Some(Value::Object(payload)),
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    let data = unwrap_data(&control);
    let audience = reconcile_audience(state, &org_id, &space_ref).await;
    let changed = data.get("changed").and_then(Value::as_bool).unwrap_or(false);
    let out = json!({"data": {
        "space_ref": data.get("space_ref").and_then(Value::as_str).unwrap_or(&space_ref),
        "role": data.get("role").cloned().filter(Value::is_string).unwrap_or(Value::Null),
        "changed": changed,
        "already_member": data
            .get("already_member")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "recipient_ready": false,
        "audience": { "state": audience },
    }});
    (StatusCode::OK, Json(with_audience_warning(out, audience)))
}

/// `PATCH /api/v1/spaces/{space_ref}/settings`: a room's visibility (`private`, `org_public`) and
/// the role a self-join receives (`viewer`, `editor`) and, from WS04-S11, the history window
/// (`full`, `since_join`). Control applies the role rule (owner only for `history_mode`; owner or
/// manager otherwise), refuses `org_public` while the organization's directory policy is off
/// (403 `directory_disabled`), the organization room (409 `room_roster_derived`) and an archived
/// room (409 `room_archived`). A visibility change does not alter the recipient set, so no
/// audience reconcile follows. `history_mode` is passed through unchanged: the floor claim is
/// derived and signed by Control, never by the gateway.
pub(crate) async fn update_room_settings(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.room_directory_api {
        return directory_feature_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let Some(fields) = body.as_object() else {
        return invalid("body", "The request body must be a JSON object.");
    };
    if fields
        .keys()
        .any(|key| {
            !["visibility", "join_role", "history_mode", "idempotency_key"].contains(&key.as_str())
        })
    {
        return invalid(
            "body",
            "Only visibility, join_role, history_mode and idempotency_key are accepted.",
        );
    }
    let mut payload = Map::new();
    match fields.get("visibility") {
        None | Some(Value::Null) => {}
        Some(Value::String(value)) if VISIBILITIES.contains(&value.trim()) => {
            payload.insert("visibility".to_owned(), json!(value.trim()));
        }
        Some(_) => return invalid("visibility", "The visibility must be private or org_public."),
    }
    match fields.get("join_role") {
        None | Some(Value::Null) => {}
        Some(Value::String(value)) if JOIN_ROLES.contains(&value.trim()) => {
            payload.insert("join_role".to_owned(), json!(value.trim()));
        }
        Some(_) => return invalid("join_role", "The join role must be viewer or editor."),
    }
    match fields.get("history_mode") {
        None | Some(Value::Null) => {}
        Some(Value::String(value)) if HISTORY_MODES.contains(&value.trim()) => {
            payload.insert("history_mode".to_owned(), json!(value.trim()));
        }
        Some(_) => return invalid("history_mode", "The history mode must be full or since_join."),
    }
    if payload.is_empty() {
        return invalid("body", "A visibility, a join role or a history mode is required.");
    }
    match optional_idempotency_key(body) {
        Ok(Some(key)) => {
            payload.insert("idempotency_key".to_owned(), json!(key));
        }
        Ok(None) => {}
        Err(reply) => return reply,
    }
    if let Err(reply) = member_ops_allowed(state, &org_id, user).await {
        return reply;
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::PATCH,
        &space_ops_url(state, &space_ref, "settings"),
        Some(Value::Object(payload)),
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    let data = unwrap_data(&control);
    (
        StatusCode::OK,
        Json(json!({"data": {
            "space_ref": data.get("space_ref").and_then(Value::as_str).unwrap_or(&space_ref),
            "visibility": data.get("visibility").cloned().filter(Value::is_string).unwrap_or(Value::Null),
            "join_role": data.get("join_role").cloned().filter(Value::is_string).unwrap_or(Value::Null),
            "history_mode": data.get("history_mode").cloned().filter(Value::is_string).unwrap_or(Value::Null),
            "changed": data.get("changed").and_then(Value::as_bool).unwrap_or(false),
        }})),
    )
}

/// Axum handler for `POST /api/v1/spaces/{space_ref}/join`. The body is optional.
pub(super) async fn join_room_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    raw: Bytes,
) -> Reply {
    match parse_optional_body(&raw) {
        Ok(body) => join_room(&state, &user, &space_ref, &body).await,
        Err(reply) => reply,
    }
}

/// Axum handler for `PATCH /api/v1/spaces/{space_ref}/settings`.
pub(super) async fn update_room_settings_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    raw: Bytes,
) -> Reply {
    match parse_optional_body(&raw) {
        Ok(body) => update_room_settings(&state, &user, &space_ref, &body).await,
        Err(reply) => reply,
    }
}

// ---------------------------------------------------------------------------------------------
// D18: per-member room history visibility.
//
// Two additions on the member writer. The invite's choice is the optional `share_past_conversation` boolean of
// `POST .../members` (absent or true: the room's own history mode decides, exactly as before; false: the person joins
// without the conversation posted before their join). A room owner or manager can later set one member's window with
// `PUT .../members/{subject_id}/history-window` (full, since_join, or since_date with an instant) and read it with `GET`
// on the same path.
//
// Control owns all of it: the role rule (a room manager or owner), the audit row, the floor every read signs, and its
// rollout switch `SPACE_MEMBER_HISTORY_WINDOWS`. This module checks no role, keeps no state, and sends Control only the
// closed fields below, so a body can never carry an organization, an actor or a floor. A window can only HIDE: nothing here
// widens what a member reads. It is a mode and an instant, no content, so a Zero Data Retention organization behaves the
// same and no model is called. The gateway's own switch (`SpaceFeatureFlags::member_history_windows`) only decides
// whether the two routes exist and whether the SPA offers the controls; it never gates the invite's choice.
// ---------------------------------------------------------------------------------------------

const MAX_WINDOW_SUBJECT_CHARS: usize = 200;
const MAX_INSTANT_CHARS: usize = 64;

/// The modes a person may request for one member. `room` is not one of them: only an invite writes it, inside Control.
/// (A `matches!`, not a string table: the SPA's error-code scan reads every table of the gateway as a table of codes.)
fn is_member_window_mode(mode: &str) -> bool {
    matches!(mode, "full" | "since_join" | "since_date")
}

/// The only body fields of the admin change. Anything else is refused, never forwarded.
fn is_member_window_field(field: &str) -> bool {
    matches!(field, "mode" | "since_at" | "expected_revision")
}

/// Both history-window routes answer this when the gateway switch is off, before any Control call.
fn member_history_windows_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "Per-member room history is not enabled.",
        )),
    )
}

/// One reply as a response that no cache may keep: a window is Control's live authority answer.
fn no_store(reply: Reply) -> Response {
    let (status, body) = reply;
    (status, [(header::CACHE_CONTROL, "no-store")], body).into_response()
}

/// The invite's choice. Absent or null is no choice and is not sent. A boolean is forwarded as given, `false` included.
/// Anything else is refused here, so a malformed value can never degrade into the default and share the past.
fn optional_share_past_conversation(body: &Value) -> Result<Option<bool>, Reply> {
    match body.get("share_past_conversation") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(share)) => Ok(Some(*share)),
        Some(_) => Err(invalid(
            "share_past_conversation",
            "share_past_conversation must be true or false.",
        )),
    }
}

/// The member a history-window route names: trimmed, non-empty and bounded. `.` and `..` are refused because a URL
/// parser would collapse them out of the Control path, and so are a path separator and any control character: none of
/// them is in a member id, and Control would answer such a path as an unknown route rather than an unknown member.
fn window_subject(subject_id: &str) -> Result<String, Reply> {
    let subject = subject_id.trim();
    if subject.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error("member_required", "A member is required.")),
        ));
    }
    let separator_or_control = |c: char| c.is_control() || c == '/' || c == '\\';
    if subject == "."
        || subject == ".."
        || subject.chars().count() > MAX_WINDOW_SUBJECT_CHARS
        || subject.chars().any(separator_or_control)
    {
        return Err(invalid(
            "subject_id",
            "subject_id is not a valid member id.",
        ));
    }
    Ok(subject.to_owned())
}

fn history_window_url(state: &AppState, space_ref: &str, subject_id: &str) -> String {
    format!(
        "{}/history-window",
        members_url(state, space_ref, Some(subject_id))
    )
}

/// An RFC 3339 instant as the one canonical string Control parses: UTC, `Z`, fractional seconds only when present.
/// The same instant, so nothing is decided here; a bad string is None.
fn canonical_instant(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_INSTANT_CHARS {
        return None;
    }
    DateTime::parse_from_rfc3339(raw).ok().map(|instant| {
        instant
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::AutoSi, true)
    })
}

/// The body of the admin change, checked for shape only and rebuilt from the closed fields: an object with `mode` (one
/// of [`is_member_window_mode`]), an optional RFC 3339 `since_at` and an optional non-negative `expected_revision`. What
/// the pair means (a `since_at` is required with `since_date`, not allowed with the others, and never in the future) is
/// Control's rule and comes back as its typed 422 `member_history_window_invalid`.
fn history_window_payload(body: &Value) -> Result<Value, Reply> {
    let Some(fields) = body.as_object() else {
        return Err(invalid("body", "The request body must be a JSON object."));
    };
    if fields.keys().any(|key| !is_member_window_field(key)) {
        return Err(invalid(
            "body",
            "Only mode, since_at and expected_revision are accepted.",
        ));
    }
    let mut payload = Map::new();
    match fields.get("mode") {
        Some(Value::String(mode)) if is_member_window_mode(mode.trim()) => {
            payload.insert("mode".to_owned(), json!(mode.trim()));
        }
        _ => {
            return Err(invalid(
                "mode",
                "mode must be full, since_join or since_date.",
            ))
        }
    }
    match fields.get("since_at") {
        None | Some(Value::Null) => {}
        Some(Value::String(at)) => match canonical_instant(at) {
            Some(instant) => {
                payload.insert("since_at".to_owned(), json!(instant));
            }
            None => return Err(invalid("since_at", "since_at must be an RFC 3339 instant.")),
        },
        Some(_) => return Err(invalid("since_at", "since_at must be an RFC 3339 instant.")),
    }
    match fields.get("expected_revision") {
        None | Some(Value::Null) => {}
        Some(value) => match value.as_i64().filter(|revision| *revision >= 0) {
            Some(revision) => {
                payload.insert("expected_revision".to_owned(), json!(revision));
            }
            None => {
                return Err(invalid(
                    "expected_revision",
                    "The expected revision must be a non-negative integer.",
                ))
            }
        },
    }
    Ok(Value::Object(payload))
}

/// One member's window as Control reports it, rebuilt as exactly four fields: where the rule comes from (`source`,
/// `member` or `room`), the mode, the instant it names (`since_at`, null when none) and the row's revision (what an
/// `expected_revision` compares against). A field that is not a plain word, an RFC 3339 instant or a non-negative integer
/// makes the whole window None: the callers then answer 502 (a read or a change, where the window IS the answer) or leave
/// it out (an add, where it is a courtesy), and a window is never made up. The words are not checked against the
/// vocabulary, so a mode Control adds later is not an outage here.
fn history_window_view(window: Option<&Value>) -> Option<Value> {
    let window = window?.as_object()?;
    let word = |field: &str| {
        window
            .get(field)
            .and_then(Value::as_str)
            .filter(|word| is_error_code(word))
    };
    let since_at = match window.get("since_at") {
        None | Some(Value::Null) => Value::Null,
        Some(Value::String(at)) if canonical_instant(at).is_some() => json!(at.trim()),
        Some(_) => return None,
    };
    let revision = match window.get("revision") {
        None | Some(Value::Null) => 0,
        Some(value) => value.as_u64()?,
    };
    Some(json!({
        "source": word("source")?,
        "mode": word("mode")?,
        "since_at": since_at,
        "revision": revision,
    }))
}

/// A Control answer that is not a window. Never reported as a success: the person would act on a made-up window. It is
/// the upstream layer's own 502 `upstream_unavailable` (the code the SPA already words, and the one a transport failure
/// gets), not a code of its own: the right move for the person is the same, to read the window again.
fn invalid_history_window_answer() -> Reply {
    (
        StatusCode::BAD_GATEWAY,
        Json(crate::envelope::upstream_unavailable()),
    )
}

/// The success body of the change (`with_changed`) and of the read: `{space_ref, subject_id, [changed], history_window}`.
/// The window is Control's, rebuilt; the two ids are the request's, so an answer is never relabelled.
fn history_window_reply(
    control: &Value,
    space_ref: &str,
    subject_id: &str,
    with_changed: bool,
) -> Reply {
    let data = unwrap_data(control);
    let Some(window) = history_window_view(data.get("history_window")) else {
        return invalid_history_window_answer();
    };
    let mut out = json!({
        "space_ref": space_ref,
        "subject_id": subject_id,
        "history_window": window,
    });
    if with_changed {
        let changed = data.get("changed").and_then(Value::as_bool).unwrap_or(false);
        out["changed"] = json!(changed);
    }
    (StatusCode::OK, Json(json!({ "data": out })))
}

/// `PUT /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`: sets one member's window. Control answers
/// who may (a room owner or manager), refuses an archived room (409 `room_archived`), a roster derived from the
/// organization (409 `room_roster_derived`), a member who is not active in the room (404 `member_not_found`), a stale
/// `expected_revision` (409 `room_member_changed`, `details.current_revision`), a bad pair (422
/// `member_history_window_invalid`, `details.field`) and, while its own switch is off, every write (503
/// `member_history_window_unavailable`). Every typed refusal passes through with its status. The organization and the
/// acting user come from the session, never from the path or the body. It is set-to-state, so a repeat answers
/// `changed:false`, and it shares the member-operation ceiling. A window changes what one member sees, never the
/// recipient set, so no audience reconcile follows.
pub(crate) async fn set_member_history_window(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    subject_id: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.member_history_windows {
        return member_history_windows_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let subject_id = match window_subject(subject_id) {
        Ok(subject) => subject,
        Err(reply) => return reply,
    };
    let payload = match history_window_payload(body) {
        Ok(payload) => payload,
        Err(reply) => return reply,
    };
    if let Err(reply) = member_ops_allowed(state, &org_id, user).await {
        return reply;
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::PUT,
        &history_window_url(state, &space_ref, &subject_id),
        Some(payload),
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    history_window_reply(&control, &space_ref, &subject_id, true)
}

/// `GET /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`: reads one member's window for a room owner or
/// manager (Control's rule; anyone else is refused and a stranger to the room gets the answer of an unknown room). A
/// member with no window of their own reads under the room's mode (`source: "room"`). A read is not a member operation
/// and is not rate limited, so a member list can read one window per row.
pub(crate) async fn get_member_history_window(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    subject_id: &str,
) -> Reply {
    if !state.space_features.member_history_windows {
        return member_history_windows_disabled();
    }
    let (org_id, space_ref) = match scope(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let subject_id = match window_subject(subject_id) {
        Ok(subject) => subject,
        Err(reply) => return reply,
    };
    let (status, Json(control)) = proxy_json(
        state,
        Method::GET,
        &history_window_url(state, &space_ref, &subject_id),
        None,
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, control);
    }
    history_window_reply(&control, &space_ref, &subject_id, false)
}

/// Axum handler for `PUT /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`. The switch is checked before
/// the body is read, and a malformed or oversized body is a typed 400 rather than the framework's plain-text rejection.
pub(super) async fn put_member_history_window_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, subject_id)): Path<(String, String)>,
    raw: Bytes,
) -> Response {
    if !state.space_features.member_history_windows {
        return no_store(member_history_windows_disabled());
    }
    no_store(match parse_optional_body(&raw) {
        Ok(body) => set_member_history_window(&state, &user, &space_ref, &subject_id, &body).await,
        Err(reply) => reply,
    })
}

/// Axum handler for `GET /api/v1/spaces/{space_ref}/members/{subject_id}/history-window`.
pub(super) async fn get_member_history_window_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, subject_id)): Path<(String, String)>,
) -> Response {
    no_store(get_member_history_window(&state, &user, &space_ref, &subject_id).await)
}

/// The two history-window routes, merged by `module_routes` so they are session-gated like every module route.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/spaces/{space_ref}/members/{subject_id}/history-window",
        put(put_member_history_window_route).get(get_member_history_window_route),
    )
}

#[cfg(test)]
mod tests {
    use super::{audience_state, invalid_details_from, map_details_failure};
    use crate::config::AppState;
    use crate::rate_limit::RateLimiter;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const ROOM: &str = "/api/v1/spaces/space-room";
    const CONTROL_MEMBERS: &str = "/api/v1/internal/spaces/space-room/members";

    struct Fx {
        state: AppState,
        user_core: MockServer,
        application: MockServer,
        model_gateway: MockServer,
    }

    /// A gateway wired to mock Auth, Control (caller holds `role` in `space-room`), Application
    /// (Convex) and Model Gateway. Flags are all off; a test sets the one it needs on `fx.state`.
    async fn fx(role: &str) -> Fx {
        let auth = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/auth/get-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": "org-1"}
            })))
            .mount(&auth)
            .await;
        let user_core = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/me/session-context"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
            })))
            .mount(&user_core)
            .await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": role}
            })))
            .mount(&user_core)
            .await;
        let application = MockServer::start().await;
        let model_gateway = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.application_convex_url = application.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.model_gateway_url = model_gateway.uri();
        // Keep the auth mock alive for the whole test.
        std::mem::forget(auth);
        Fx { state, user_core, application, model_gateway }
    }

    async fn call(
        fx: &Fx,
        method: &str,
        uri: &str,
        body: Option<Value>,
        extra_headers: &[(&str, &str)],
    ) -> (u16, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        for (name, value) in extra_headers {
            request = request.header(*name, *value);
        }
        if body.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                request
                    .body(match &body {
                        Some(value) => Body::from(value.to_string()),
                        None => Body::empty(),
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload)
    }

    async fn mount_convex(fx: &Fx, route: &str, template: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(wm_path(route))
            .respond_with(template)
            .mount(&fx.application)
            .await;
    }

    async fn mount_control(fx: &Fx, method: &str, path: &str, template: ResponseTemplate) {
        Mock::given(wm_method(method))
            .and(wm_path(path))
            .respond_with(template)
            .mount(&fx.user_core)
            .await;
    }

    /// Convex requests the application mock saw.
    async fn convex_requests(fx: &Fx) -> Vec<Value> {
        fx.application
            .received_requests()
            .await
            .expect("requests")
            .iter()
            .map(|request| {
                let mut body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
                body["_route"] = json!(request.url.path());
                body
            })
            .collect()
    }

    /// Control requests other than session and membership reads (the writes and decisions).
    async fn control_calls(fx: &Fx) -> Vec<(String, String)> {
        fx.user_core
            .received_requests()
            .await
            .expect("requests")
            .iter()
            .filter(|request| {
                request.url.path().starts_with("/api/v1/internal/")
                    && !request.url.path().ends_with("/membership")
            })
            .map(|request| (request.method.to_string(), request.url.path().to_owned()))
            .collect()
    }

    fn details_result(topic: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "value": {"spaceRef": "space-room", "name": "Leveranse", "topic": topic}
        }))
    }

    fn convex_error(message: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": message}))
    }

    // ---- WS04-S2: PATCH /api/v1/spaces/{ref} ----------------------------------------------

    #[tokio::test]
    async fn flag_off_is_404_before_any_control_or_convex_request() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner").await;
        let (status, body) = call(&fx, "PATCH", ROOM, Some(json!({"name": "Ny"})), &[]).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "feature_disabled");
        assert!(control_calls(&fx).await.is_empty());
        let membership_reads = fx
            .user_core
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().ends_with("/membership"))
            .count();
        assert_eq!(membership_reads, 0, "the flag is checked before Control is asked anything");
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn a_viewer_or_editor_cannot_change_room_details_and_convex_is_never_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for role in ["viewer", "editor", ""] {
            let mut fx = fx(role).await;
            fx.state.space_features.room_details_ui = true;
            mount_convex(&fx, "/api/mutation", details_result(Value::Null)).await;
            let (status, body) = call(&fx, "PATCH", ROOM, Some(json!({"name": "Ny", "topic": "x"})), &[]).await;
            assert_eq!(status, 403, "role {role:?}");
            assert_eq!(body["error"]["code"], "room_role_forbidden");
            assert!(convex_requests(&fx).await.is_empty(), "role {role:?} must not reach Convex");
        }
    }

    #[tokio::test]
    async fn an_owner_or_manager_reaches_the_convex_mutation_and_gets_the_details_back() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for role in ["owner", "manager"] {
            let mut fx = fx(role).await;
            fx.state.space_features.room_details_ui = true;
            mount_convex(&fx, "/api/mutation", details_result(json!("Ukens leveranser"))).await;
            let (status, body) = call(
                &fx,
                "PATCH",
                ROOM,
                Some(json!({"name": "Leveranse", "topic": "Ukens leveranser", "org_id": "forged"})),
                &[],
            )
            .await;
            assert_eq!(status, 200, "{role}: {body}");
            assert_eq!(
                body,
                json!({"data": {"space_ref": "space-room", "name": "Leveranse", "topic": "Ukens leveranser"}})
            );
            let requests = convex_requests(&fx).await;
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0]["path"], "spaces:updateRoomDetailsForGateway");
            let args = &requests[0]["args"];
            assert_eq!(args["externalAuthId"], "user-1");
            assert_eq!(args["externalOrgId"], "org-1", "the org is the session's, never the body's");
            assert_eq!(args["spaceRef"], "space-room");
            assert_eq!(args["name"], "Leveranse");
            assert_eq!(args["topic"], "Ukens leveranser");
            assert!(args.get("org_id").is_none());
        }
    }

    #[tokio::test]
    async fn a_control_membership_error_status_passes_through_without_reaching_convex() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (status, body) in [
            (403, json!({"error": "not a member"})),
            (404, json!({"error": {"code": "space_not_found", "message": "gone"}})),
        ] {
            let mut fx = fx("owner").await;
            fx.state.space_features.room_details_ui = true;
            // A later mount does not override an earlier one in wiremock, so point the gateway at
            // a Control that answers the membership read with the error.
            let control = MockServer::start().await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/me/session-context"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                    "userId":"user-1", "orgId":"org-1", "role":"member", "onboardingStatus":"COMPLETED"
                })))
                .mount(&control)
                .await;
            Mock::given(wm_method("GET"))
                .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body.clone()))
                .mount(&control)
                .await;
            fx.state.user_core_url = control.uri();
            mount_convex(&fx, "/api/mutation", details_result(Value::Null)).await;
            let (got_status, got_body) = call(&fx, "PATCH", ROOM, Some(json!({"name": "Ny"})), &[]).await;
            assert_eq!(got_status, status);
            assert_eq!(got_body, body);
            assert!(convex_requests(&fx).await.is_empty());
        }
    }

    #[tokio::test]
    async fn convex_error_messages_map_to_typed_browser_errors() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases = [
            (
                "Uncaught Error: room_details_invalid:name:too_long at handler",
                400,
                "room_details_invalid",
                Some(json!({"field": "name", "reason": "too_long"})),
            ),
            (
                "room_details_invalid:topic:control_characters",
                400,
                "room_details_invalid",
                Some(json!({"field": "topic", "reason": "control_characters"})),
            ),
            ("Uncaught Error: space_not_found", 404, "space_not_found", None),
            ("space_kind_not_editable", 409, "space_kind_not_editable", None),
            ("room_not_active", 409, "room_not_active", None),
            ("Something else went wrong", 503, "room_details_unavailable", None),
        ];
        for (message, want_status, want_code, want_details) in cases {
            let mut fx = fx("owner").await;
            fx.state.space_features.room_details_ui = true;
            mount_convex(&fx, "/api/mutation", convex_error(message)).await;
            let (status, body) = call(&fx, "PATCH", ROOM, Some(json!({"name": "Ny"})), &[]).await;
            assert_eq!(status, want_status, "{message}");
            assert_eq!(body["error"]["code"], want_code, "{message}");
            if let Some(details) = want_details {
                assert_eq!(body["error"]["details"], details);
            }
            assert!(
                !body.to_string().contains("handler") && !body.to_string().contains("Uncaught"),
                "no Convex text reaches the browser: {body}"
            );
        }
    }

    #[tokio::test]
    async fn a_non_success_convex_status_and_an_unreachable_convex_answer_503() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // HTTP 500 carrying a known message still maps by message.
        let mut fx1 = fx("owner").await;
        fx1.state.space_features.room_details_ui = true;
        mount_convex(
            &fx1,
            "/api/mutation",
            ResponseTemplate::new(500).set_body_json(json!({"errorMessage": "room_not_active"})),
        )
        .await;
        let (status, body) = call(&fx1, "PATCH", ROOM, Some(json!({"name": "Ny"})), &[]).await;
        assert_eq!((status, &body["error"]["code"]), (409, &json!("room_not_active")));
        // HTTP 500 with no message, and Convex not configured or unreachable.
        let mut fx2 = fx("owner").await;
        fx2.state.space_features.room_details_ui = true;
        mount_convex(&fx2, "/api/mutation", ResponseTemplate::new(500).set_body_json(json!({}))).await;
        let (status, body) = call(&fx2, "PATCH", ROOM, Some(json!({"name": "Ny"})), &[]).await;
        assert_eq!((status, &body["error"]["code"]), (503, &json!("room_details_unavailable")));
        for url in ["", "http://127.0.0.1:1"] {
            let mut fx3 = fx("owner").await;
            fx3.state.space_features.room_details_ui = true;
            fx3.state.application_convex_url = url.to_owned();
            let (status, body) = call(&fx3, "PATCH", ROOM, Some(json!({"name": "Ny"})), &[]).await;
            assert_eq!((status, &body["error"]["code"]), (503, &json!("room_details_unavailable")), "{url:?}");
        }
    }

    #[test]
    fn invalid_details_from_accepts_only_token_fields_and_reasons() {
        assert_eq!(
            invalid_details_from("Error: room_details_invalid:name:too_long\n at x"),
            Some(("name".to_owned(), "too_long".to_owned()))
        );
        assert_eq!(
            invalid_details_from("room_details_invalid:details:empty"),
            Some(("details".to_owned(), "empty".to_owned()))
        );
        // Field outside name/topic/details, reason not a lowercase token, missing parts.
        assert_eq!(invalid_details_from("room_details_invalid:secret:too_long"), None);
        assert_eq!(invalid_details_from("room_details_invalid:name:Too_Long"), None);
        assert_eq!(invalid_details_from("room_details_invalid:name:too-long"), None);
        assert_eq!(invalid_details_from("room_details_invalid:name:"), None);
        assert_eq!(invalid_details_from("room_details_invalid:name"), None);
        assert_eq!(invalid_details_from("no prefix here"), None);
    }

    #[test]
    fn an_unparseable_invalid_message_still_answers_400_without_echoing_it() {
        let (status, axum::Json(body)) = map_details_failure(Some("room_details_invalid:secret:x"));
        assert_eq!(status.as_u16(), 400);
        assert_eq!(body["error"]["code"], "room_details_invalid");
        assert_eq!(body["error"]["details"], json!({"field": "details", "reason": "invalid"}));
        assert!(!body.to_string().contains("secret"));
        let (status, _) = map_details_failure(None);
        assert_eq!(status.as_u16(), 503);
    }

    #[tokio::test]
    async fn the_body_is_validated_before_any_upstream_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let too_long = "a".repeat(2001);
        let cases = [
            (json!({}), "details", "empty_update"),
            (json!({"name": null, "topic": null}), "details", "empty_update"),
            (json!({"name": 5}), "name", "not_a_string"),
            (json!({"topic": ["x"]}), "topic", "not_a_string"),
            (json!({"name": too_long.clone()}), "name", "too_long"),
            (json!({"topic": too_long}), "topic", "too_long"),
        ];
        for (body, field, reason) in cases {
            let mut fx = fx("owner").await;
            fx.state.space_features.room_details_ui = true;
            mount_convex(&fx, "/api/mutation", details_result(Value::Null)).await;
            let (status, got) = call(&fx, "PATCH", ROOM, Some(body.clone()), &[]).await;
            assert_eq!(status, 400, "{body}");
            assert_eq!(got["error"]["code"], "room_details_invalid");
            assert_eq!(got["error"]["details"], json!({"field": field, "reason": reason}), "{body}");
            assert!(convex_requests(&fx).await.is_empty(), "{body}");
            let membership_reads = fx
                .user_core
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|request| request.url.path().ends_with("/membership"))
                .count();
            assert_eq!(membership_reads, 0, "validation precedes the role read");
        }
    }

    #[tokio::test]
    async fn an_empty_topic_is_forwarded_to_clear_it_and_answers_null() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx("manager").await;
        fx.state.space_features.room_details_ui = true;
        mount_convex(&fx, "/api/mutation", details_result(Value::Null)).await;
        let (status, body) = call(&fx, "PATCH", ROOM, Some(json!({"topic": ""})), &[]).await;
        assert_eq!(status, 200, "{body}");
        assert!(body["data"]["topic"].is_null());
        let requests = convex_requests(&fx).await;
        assert_eq!(requests[0]["args"]["topic"], "", "the empty string is the clear request");
        assert!(requests[0]["args"].get("name").is_none(), "an unsent field stays unsent");
    }

    /// The topic is a display label: a ZDR organization behaves the same and no model is called.
    #[tokio::test]
    async fn zdr_org_room_admin_never_calls_the_model_plane() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx("owner").await;
        fx.state.space_features.room_details_ui = true;
        mount_convex(&fx, "/api/mutation", details_result(json!("t"))).await;
        let (status, body) = call(
            &fx,
            "PATCH",
            ROOM,
            Some(json!({"name": "Leveranse", "topic": "t", "zdr": true})),
            &[("x-zdr", "true")],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert!(fx.model_gateway.received_requests().await.unwrap().is_empty());

        // The member writer too.
        let fx2 = fx_writer("owner").await;
        mount_control(&fx2, "POST", CONTROL_MEMBERS, control_ok(json!({"member_count": 2, "changed": true, "role": "editor"}))).await;
        mount_convex(&fx2, "/api/action", ResponseTemplate::new(200).set_body_json(json!({"value": {"status": "noop"}}))).await;
        let (status, _) = call(&fx2, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), &[("x-zdr", "true")]).await;
        assert_eq!(status, 200);
        assert!(fx2.model_gateway.received_requests().await.unwrap().is_empty());
    }

    // ---- WS04-S6: member writer ------------------------------------------------------------

    async fn fx_writer(role: &str) -> Fx {
        let mut fx = fx(role).await;
        fx.state.space_features.membership_writer_api = true;
        fx
    }

    fn control_ok(data: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"data": data}))
    }

    fn reconcile(status: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({"value": {"status": status}}))
    }

    #[test]
    fn audience_state_maps_noop_to_current_replaced_and_waiting_to_pending_and_the_rest_to_unconfirmed() {
        assert_eq!(audience_state("noop"), "current");
        assert_eq!(audience_state("replaced"), "pending");
        assert_eq!(audience_state("waiting"), "pending");
        for status in ["control_unavailable", "unavailable", "failed", "refused", "", "applied", "NOOP"] {
            assert_eq!(audience_state(status), "unconfirmed", "{status:?}");
        }
    }

    #[tokio::test]
    async fn flag_off_the_writer_routes_and_functions_answer_404_feature_disabled() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner").await;
        // The PATCH member route exists only on the writer.
        let (status, body) = call(&fx, "PATCH", &format!("{ROOM}/members/user-2"), Some(json!({"role": "viewer"})), &[]).await;
        assert_eq!(status, 404);
        assert_eq!(body["error"]["code"], "feature_disabled");
        // The functions the leave action and the routes share.
        let user = crate::middleware::AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };
        let (status, axum::Json(body)) = super::remove_member(&fx.state, &user, "space-room", "me").await;
        assert_eq!((status.as_u16(), &body["error"]["code"]), (404, &json!("feature_disabled")));
        let (status, _) = super::add_member(&fx.state, &user, "space-room", &json!({"member_id": "u"})).await;
        assert_eq!(status.as_u16(), 404);
        let (status, _) = super::change_member_role(&fx.state, &user, "space-room", "u", &json!({"role": "viewer"})).await;
        assert_eq!(status.as_u16(), 404);
        assert!(control_calls(&fx).await.is_empty());
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn control_error_body_and_status_pass_through_unchanged_and_no_reconcile_runs() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let error_body = json!({"error": {"code": "last_owner", "message": "A room needs an owner."}});
        let members_user = format!("{CONTROL_MEMBERS}/user-2");
        let routes = [
            ("POST", format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), "POST", CONTROL_MEMBERS.to_owned()),
            ("PATCH", format!("{ROOM}/members/user-2"), Some(json!({"role": "viewer"})), "PATCH", members_user.clone()),
            ("DELETE", format!("{ROOM}/members/user-2"), None, "DELETE", members_user),
        ];
        for (method, uri, body, control_method, control_path) in routes {
            let fx = fx_writer("owner").await;
            mount_control(&fx, &control_method, &control_path, ResponseTemplate::new(409).set_body_json(error_body.clone())).await;
            mount_convex(&fx, "/api/action", reconcile("replaced")).await;
            let (status, got) = call(&fx, method, &uri, body, &[]).await;
            assert_eq!(status, 409, "{method}");
            assert_eq!(got, error_body, "{method}");
            assert!(convex_requests(&fx).await.is_empty(), "{method}: a refused write is never reconciled");
        }
    }

    #[tokio::test]
    async fn me_resolves_to_the_caller_and_leave_reconciles_the_audience() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_writer("editor").await;
        mount_control(
            &fx,
            "DELETE",
            &format!("{CONTROL_MEMBERS}/user-1"),
            control_ok(json!({"changed": true, "left": true, "member_count": 3})),
        )
        .await;
        mount_convex(&fx, "/api/action", reconcile("replaced")).await;
        let (status, body) = call(&fx, "DELETE", &format!("{ROOM}/members/me"), None, &[]).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body,
            json!({"data": {"changed": true, "left": true, "member_count": 3, "audience": {"state": "pending"}}})
        );
        assert_eq!(control_calls(&fx).await, vec![("DELETE".to_owned(), format!("{CONTROL_MEMBERS}/user-1"))]);
        let requests = convex_requests(&fx).await;
        assert_eq!(requests[0]["path"], "spaceMembers:reconcileRoomAudienceForGateway");
        assert_eq!(requests[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(requests[0]["args"]["spaceRef"], "space-room");
    }

    #[tokio::test]
    async fn the_audience_state_is_current_pending_or_unconfirmed_with_a_warning_for_the_last() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases = [
            ("noop", "current", false),
            ("replaced", "pending", false),
            ("waiting", "pending", false),
            ("control_unavailable", "unconfirmed", true),
            ("something_new", "unconfirmed", true),
        ];
        for (reconcile_status, want, warns) in cases {
            let fx = fx_writer("owner").await;
            mount_control(&fx, "POST", CONTROL_MEMBERS, control_ok(json!({"member_count": 4, "changed": true, "role": "editor"}))).await;
            mount_convex(&fx, "/api/action", reconcile(reconcile_status)).await;
            let (status, body) = call(&fx, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), &[]).await;
            assert_eq!(status, 200, "{reconcile_status}: {body}");
            assert_eq!(body["data"]["audience"]["state"], want, "{reconcile_status}");
            assert_eq!(body["data"]["member_count"], 4);
            assert_eq!(body["data"]["changed"], true);
            assert_eq!(body["data"]["role"], "editor");
            if warns {
                assert_eq!(body["meta"]["warnings"], json!(["room_audience_unconfirmed"]));
            } else {
                assert!(body.get("meta").is_none(), "{reconcile_status}: {body}");
            }
        }
    }

    /// The Control write is authoritative: a reconcile that errors, is refused or cannot be
    /// reached never turns the success into an error.
    #[tokio::test]
    async fn a_reconcile_failure_never_turns_a_control_success_into_an_error() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for failure in [
            Some(ResponseTemplate::new(500).set_body_json(json!({"errorMessage": "boom"}))),
            Some(ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": "x"}))),
            Some(ResponseTemplate::new(200).set_body_json(json!({"unexpected": true}))),
            None,
        ] {
            for (method, uri, body, control_path, data) in [
                ("POST", format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), CONTROL_MEMBERS.to_owned(), json!({"member_count": 2, "changed": true, "role": "editor"})),
                ("DELETE", format!("{ROOM}/members/user-2"), None, format!("{CONTROL_MEMBERS}/user-2"), json!({"changed": true, "left": false, "member_count": 1})),
            ] {
                let mut fx = fx_writer("owner").await;
                mount_control(&fx, method, &control_path, control_ok(data)).await;
                match &failure {
                    Some(template) => mount_convex(&fx, "/api/action", template.clone()).await,
                    None => fx.state.application_convex_url = "http://127.0.0.1:1".to_owned(),
                }
                let (status, got) = call(&fx, method, &uri, body, &[]).await;
                assert_eq!(status, 200, "{method}: {got}");
                assert_eq!(got["data"]["audience"]["state"], "unconfirmed");
                assert_eq!(got["meta"]["warnings"], json!(["room_audience_unconfirmed"]));
            }
        }
    }

    #[tokio::test]
    async fn add_member_forwards_the_role_only_when_given_and_the_org_comes_from_the_session() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (request, want_role) in [
            (json!({"member_id": "user-2"}), None),
            (json!({"subject_id": "user-2", "role": "viewer", "idempotency_key": "key-1", "org_id": "forged"}), Some("viewer")),
        ] {
            let fx = fx_writer("owner").await;
            mount_control(&fx, "POST", CONTROL_MEMBERS, control_ok(json!({"member_count": 2, "changed": true, "role": want_role.unwrap_or("editor")}))).await;
            mount_convex(&fx, "/api/action", reconcile("noop")).await;
            let (status, body) = call(&fx, "POST", &format!("{ROOM}/members"), Some(request), &[]).await;
            assert_eq!(status, 200, "{body}");
            let sent = fx
                .user_core
                .received_requests()
                .await
                .unwrap()
                .into_iter()
                .find(|request| request.url.path() == CONTROL_MEMBERS)
                .expect("Control write");
            let sent_body: Value = serde_json::from_slice(&sent.body).unwrap();
            assert_eq!(sent_body["subject_id"], "user-2");
            assert_eq!(sent_body.get("role").and_then(Value::as_str), want_role);
            assert!(sent_body.get("org_id").is_none());
            assert!(sent.headers.get("x-delegation-signature").is_some());
        }
    }

    #[tokio::test]
    async fn bad_member_writer_bodies_are_refused_before_control_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_writer("owner").await;
        let add = format!("{ROOM}/members");
        let change = format!("{ROOM}/members/user-2");
        for (method, uri, body, code) in [
            ("POST", add.clone(), json!({}), "member_required"),
            ("POST", add.clone(), json!({"member_id": "u", "role": "root"}), "invalid_request"),
            ("POST", add, json!({"member_id": "u", "idempotency_key": " "}), "invalid_request"),
            ("PATCH", change.clone(), json!({}), "invalid_request"),
            ("PATCH", change.clone(), json!({"role": "god"}), "invalid_request"),
            ("PATCH", change.clone(), json!({"role": "viewer", "expected_revision": -1}), "invalid_request"),
            ("PATCH", change, json!({"role": "viewer", "expected_revision": "3"}), "invalid_request"),
        ] {
            let (status, got) = call(&fx, method, &uri, Some(body.clone()), &[]).await;
            assert_eq!(status, 400, "{method} {body}");
            assert_eq!(got["error"]["code"], code, "{method} {body}");
        }
        assert!(control_calls(&fx).await.is_empty());
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn change_role_sends_the_expected_revision_and_never_reconciles() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_writer("owner").await;
        mount_control(
            &fx,
            "PATCH",
            &format!("{CONTROL_MEMBERS}/user-2"),
            control_ok(json!({"role": "manager", "from_role": "editor", "changed": true})),
        )
        .await;
        let (status, body) = call(
            &fx,
            "PATCH",
            &format!("{ROOM}/members/user-2"),
            Some(json!({"role": "manager", "expected_revision": 3, "idempotency_key": "key-2"})),
            &[],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body, json!({"data": {"role": "manager", "from_role": "editor", "changed": true}}));
        let sent = fx
            .user_core
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.method.to_string() == "PATCH")
            .expect("Control write");
        let sent_body: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(sent_body, json!({"role": "manager", "expected_revision": 3, "idempotency_key": "key-2"}));
        assert!(convex_requests(&fx).await.is_empty(), "the recipient set is unchanged by a role change");
    }

    // ---- WS04-S7: archive and restore ------------------------------------------------------

    const CONTROL_ARCHIVE: &str = "/api/v1/internal/spaces/space-room/archive";
    const CONTROL_UNARCHIVE: &str = "/api/v1/internal/spaces/space-room/unarchive";

    async fn fx_archive(role: &str) -> Fx {
        let mut fx = fx(role).await;
        fx.state.space_features.room_archive_api = true;
        fx
    }

    #[tokio::test]
    async fn flag_off_the_archive_routes_answer_404_feature_disabled_and_call_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner").await;
        for verb in ["archive", "unarchive"] {
            let (status, body) = call(&fx, "POST", &format!("{ROOM}/{verb}"), None, &[]).await;
            assert_eq!(status, 404, "{verb}");
            assert_eq!(body["error"]["code"], "feature_disabled", "{verb}");
        }
        assert!(control_calls(&fx).await.is_empty());
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn archive_and_unarchive_forward_the_session_identity_and_answer_state_changed_archived_at() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_archive("owner").await;
        mount_control(
            &fx,
            "POST",
            CONTROL_ARCHIVE,
            control_ok(json!({"state": "archived", "changed": true, "replayed": false,
                "archived_at": "2026-10-31T10:00:00Z", "authority_revision": 7})),
        )
        .await;
        mount_control(
            &fx,
            "POST",
            CONTROL_UNARCHIVE,
            control_ok(json!({"state": "active", "changed": false, "archived_at": null})),
        )
        .await;
        // No body at all is fine.
        let (status, body) = call(&fx, "POST", &format!("{ROOM}/archive"), None, &[]).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body,
            json!({"data": {"state": "archived", "changed": true, "archived_at": "2026-10-31T10:00:00Z"}})
        );
        // A key travels; a forged org or actor never does.
        let (status, body) = call(
            &fx,
            "POST",
            &format!("{ROOM}/unarchive"),
            Some(json!({"idempotency_key": " key-1 ", "org_id": "forged", "actor_id": "forged"})),
            &[],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body,
            json!({"data": {"state": "active", "changed": false, "archived_at": null}})
        );
        let sent = fx.user_core.received_requests().await.unwrap();
        let find = |path: &str| {
            sent.iter()
                .find(|request| request.url.path() == path)
                .unwrap_or_else(|| panic!("no Control request to {path}"))
        };
        let archive: Value = serde_json::from_slice(&find(CONTROL_ARCHIVE).body).unwrap();
        assert_eq!(archive, json!({}));
        let unarchive_request = find(CONTROL_UNARCHIVE);
        let unarchive: Value = serde_json::from_slice(&unarchive_request.body).unwrap();
        assert_eq!(unarchive, json!({"idempotency_key": "key-1"}));
        assert!(unarchive_request.headers.get("x-delegation-signature").is_some());
        assert_eq!(
            unarchive_request.headers.get("x-org-id").and_then(|v| v.to_str().ok()),
            Some("org-1")
        );
        assert!(convex_requests(&fx).await.is_empty(), "archive is Control's state alone");
    }

    #[tokio::test]
    async fn bad_archive_bodies_are_refused_before_control_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_archive("owner").await;
        let uri = format!("{ROOM}/archive");
        for body in [
            json!({"idempotency_key": " "}),
            json!({"idempotency_key": 7}),
            json!({"idempotency_key": "k".repeat(129)}),
            json!([1]),
            json!("text"),
        ] {
            let (status, got) = call(&fx, "POST", &uri, Some(body.clone()), &[]).await;
            assert_eq!(status, 400, "{body}");
            assert_eq!(got["error"]["code"], "invalid_request", "{body}");
        }
        assert!(control_calls(&fx).await.is_empty());
        use axum::body::Bytes;
        assert_eq!(super::parse_optional_body(&Bytes::new()).unwrap(), json!({}));
        assert_eq!(super::parse_optional_body(&Bytes::from_static(b" \n ")).unwrap(), json!({}));
        assert!(super::parse_optional_body(&Bytes::from_static(b"{not json")).is_err());
        assert!(super::parse_optional_body(&Bytes::from(vec![b' '; 5000].into_iter().chain(*b"{}").collect::<Vec<u8>>())).is_err());
    }

    #[tokio::test]
    async fn control_refusals_keep_their_status_and_code_and_nothing_else() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (control_status, code) in [
            (409_u16, "room_archived"),
            (403, "room_role_forbidden"),
            (409, "room_not_archivable"),
            (409, "room_state_conflict"),
            (404, "room_not_found"),
            (503, "space_authority_unavailable"),
        ] {
            let fx = fx_archive("owner").await;
            mount_control(
                &fx,
                "POST",
                CONTROL_ARCHIVE,
                ResponseTemplate::new(control_status).set_body_json(json!({
                    "error": {"code": code, "message": "msg",
                        "details": {"state": "archived", "n": 3, "nested": {"a": 1}, "list": [1]},
                        "stack": "internal trace", "token": "secret"},
                    "debug": "x"
                })),
            )
            .await;
            let (status, got) = call(&fx, "POST", &format!("{ROOM}/archive"), None, &[]).await;
            assert_eq!(status, control_status, "{code}");
            assert_eq!(
                got,
                json!({"error": {"code": code, "message": "msg", "details": {"state": "archived", "n": 3}}}),
                "{code}"
            );
        }
    }

    #[tokio::test]
    async fn an_untyped_or_session_looking_control_failure_is_a_503_that_says_nothing_changed() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases = [
            ResponseTemplate::new(500).set_body_json(json!({"error": "boom-internal"})),
            ResponseTemplate::new(403).set_body_json(json!({"error": "boom-internal"})),
            ResponseTemplate::new(401).set_body_json(
                json!({"error": {"code": "unauthorized", "message": "boom-internal"}}),
            ),
            ResponseTemplate::new(409).set_body_json(
                json!({"error": {"code": "Bad Code!", "message": "boom-internal"}}),
            ),
        ];
        for template in cases {
            let fx = fx_archive("owner").await;
            mount_control(&fx, "POST", CONTROL_ARCHIVE, template).await;
            let (status, got) = call(&fx, "POST", &format!("{ROOM}/archive"), None, &[]).await;
            assert_eq!(status, 503, "{got}");
            assert_eq!(got["error"]["code"], "room_admin_unavailable");
            assert!(!got.to_string().contains("boom-internal"), "{got}");
        }
        // A body that is not JSON never reaches this module as text: the upstream layer already
        // reduces it to its own fixed `upstream_error` words, which pass with their status.
        let fx = fx_archive("owner").await;
        mount_control(
            &fx,
            "POST",
            CONTROL_ARCHIVE,
            ResponseTemplate::new(502).set_body_string("<html>boom-internal</html>"),
        )
        .await;
        let (status, got) = call(&fx, "POST", &format!("{ROOM}/archive"), None, &[]).await;
        assert_eq!(status, 502, "{got}");
        assert_eq!(got["error"]["code"], "upstream_error");
        assert!(!got.to_string().contains("boom-internal"), "{got}");
    }

    #[tokio::test]
    async fn the_member_writer_failures_are_filtered_the_same_way() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_writer("owner").await;
        mount_control(
            &fx,
            "POST",
            CONTROL_MEMBERS,
            ResponseTemplate::new(500).set_body_json(json!({"error": "upstream stack trace"})),
        )
        .await;
        mount_convex(&fx, "/api/action", reconcile("noop")).await;
        let (status, got) =
            call(&fx, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), &[]).await;
        assert_eq!(status, 503);
        assert_eq!(got["error"]["code"], "room_admin_unavailable");
        assert!(!got.to_string().contains("stack trace"));
        assert!(convex_requests(&fx).await.is_empty(), "a failed write is never reconciled");
    }

    #[tokio::test]
    async fn the_member_ops_ceiling_is_per_user_and_per_org_and_answers_a_typed_429() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let state = AppState::for_tests();
        let limiter = RateLimiter::from_cache_with_rpm(&state.cache, 2.0);
        assert!(super::check_member_ops(&limiter, "org-1", "user-1").await.is_ok());
        assert!(super::check_member_ops(&limiter, "org-1", "user-1").await.is_ok());
        let (status, axum::Json(body)) = super::check_member_ops(&limiter, "org-1", "user-1")
            .await
            .expect_err("the third call in the minute is refused");
        assert_eq!(status.as_u16(), 429);
        assert_eq!(body["error"]["code"], "rate_limited");
        assert!(body["error"]["details"]["retry_after_seconds"].as_u64().unwrap() >= 1);
        // Another user, and the same user in another organization, have their own bucket.
        assert!(super::check_member_ops(&limiter, "org-1", "user-2").await.is_ok());
        assert!(super::check_member_ops(&limiter, "org-2", "user-1").await.is_ok());
    }

    #[test]
    fn relay_control_error_bounds_the_message_and_the_details() {
        let long = "m".repeat(1000);
        let details: serde_json::Map<String, Value> =
            (0..40).map(|i| (format!("k{i:02}"), json!(i))).collect();
        let (status, axum::Json(body)) = super::relay_control_error(
            axum::http::StatusCode::CONFLICT,
            json!({"error": {"code": "room_member_changed", "message": long, "details": details}}),
        );
        assert_eq!(status.as_u16(), 409);
        assert_eq!(body["error"]["message"].as_str().unwrap().chars().count(), 300);
        assert_eq!(body["error"]["details"].as_object().unwrap().len(), 16);
        // No details key in, none out.
        let (_, axum::Json(body)) = super::relay_control_error(
            axum::http::StatusCode::FORBIDDEN,
            json!({"error": {"code": "room_role_forbidden", "message": "no"}}),
        );
        assert!(body["error"].get("details").is_none());
    }

    /// The read handlers ask Control for the readable membership (an archived room resolves and
    /// reports its state); a write path keeps the strict one. The context reports `archived` and
    /// the `room_archive` feature flag.
    #[tokio::test]
    async fn the_read_handlers_ask_for_the_readable_membership_and_context_reports_archived() {
        use wiremock::matchers::query_param;
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx_archive("viewer").await;
        fx.state.space_features.room_details_ui = true;
        // Only a readable request sees the archived state; the strict one gets the plain fact.
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .and(query_param("readable", "1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"space_ref": "space-room", "kind": "room", "role": "viewer",
                    "state": "archived", "archived_at": "2026-10-31T10:00:00Z"}
            })))
            .with_priority(1)
            .mount(&fx.user_core)
            .await;
        mount_convex(
            &fx,
            "/api/query",
            ResponseTemplate::new(200).set_body_json(json!({"value": [
                {"spaceRef": "space-room", "name": "Leveranse", "kind": "room", "lifecycle": "active"}
            ]})),
        )
        .await;
        let (status, body) = call(&fx, "GET", &format!("{ROOM}/context"), None, &[]).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["archived"], true);
        assert_eq!(body["data"]["membership"]["state"], "archived");
        assert_eq!(body["data"]["membership"]["archived_at"], "2026-10-31T10:00:00Z");
        assert_eq!(body["data"]["features"]["room_archive"], true);
        assert_eq!(body["data"]["space"]["lifecycle"], "active", "Application's lifecycle is untouched");
        let (status, body) = call(&fx, "GET", &format!("{ROOM}/membership"), None, &[]).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["state"], "archived");

        // A write handler's role read does not ask for the readable state.
        let (status, _) = call(&fx, "PATCH", ROOM, Some(json!({"name": "Nytt navn"})), &[]).await;
        assert_eq!(status, 403, "a viewer cannot rename");
        let queries: Vec<Option<String>> = fx
            .user_core
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path().ends_with("/membership"))
            .map(|request| request.url.query().map(str::to_owned))
            .collect();
        assert_eq!(
            queries,
            vec![
                Some("readable=1".to_owned()),
                Some("readable=1".to_owned()),
                None
            ],
            "context and the membership route are read handlers; the rename role check is not"
        );
    }

    #[tokio::test]
    async fn an_active_room_is_not_archived_and_the_feature_flag_follows_the_gateway() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for flag in [true, false] {
            let mut fx = fx("owner").await;
            fx.state.space_features.room_archive_api = flag;
            mount_convex(
                &fx,
                "/api/query",
                ResponseTemplate::new(200).set_body_json(json!({"value": [
                    {"spaceRef": "space-room", "name": "Leveranse", "kind": "room", "lifecycle": "active"}
                ]})),
            )
            .await;
            let (status, body) = call(&fx, "GET", &format!("{ROOM}/context"), None, &[]).await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body["data"]["archived"], false);
            assert_eq!(body["data"]["features"]["room_archive"], flag);
        }
    }

    #[tokio::test]
    async fn zdr_org_archive_never_calls_the_model_plane() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_archive("owner").await;
        mount_control(
            &fx,
            "POST",
            CONTROL_ARCHIVE,
            control_ok(json!({"state": "archived", "changed": true, "archived_at": "2026-10-31T10:00:00Z"})),
        )
        .await;
        let (status, body) = call(&fx, "POST", &format!("{ROOM}/archive"), None, &[("x-zdr", "true")]).await;
        assert_eq!(status, 200, "{body}");
        assert!(fx.model_gateway.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_roster_is_annotated_with_writer_api_only_on_the_writer_path() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut state = crate::tests::test_state(false);
        let mut body = json!({"data": {"members": []}});
        super::annotate_roster(&state, axum::http::StatusCode::OK, &mut body);
        assert!(body["data"].get("writer_api").is_none());
        state.space_features.membership_writer_api = true;
        super::annotate_roster(&state, axum::http::StatusCode::FORBIDDEN, &mut body);
        assert!(body["data"].get("writer_api").is_none(), "a refusal is never annotated");
        super::annotate_roster(&state, axum::http::StatusCode::OK, &mut body);
        assert_eq!(body["data"]["writer_api"], true);
    }

    // ---- WS11-S8: the invite attention signal from add-member -----------------------------------

    async fn invite_calls(fx: &Fx) -> Vec<Value> {
        convex_requests(fx)
            .await
            .into_iter()
            .filter(|request| request["path"] == "spaceAttention:recordInviteForGateway")
            .collect()
    }

    #[tokio::test]
    async fn add_member_records_the_invite_only_when_control_says_changed_true() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (changed, want_calls) in [(true, 1usize), (false, 0usize)] {
            let fx = fx_writer("owner").await;
            mount_control(
                &fx,
                "POST",
                CONTROL_MEMBERS,
                control_ok(json!({"member_count": 2, "changed": changed, "role": "editor"})),
            )
            .await;
            mount_convex(&fx, "/api/action", reconcile("noop")).await;
            mount_convex(&fx, "/api/mutation", ResponseTemplate::new(200).set_body_json(json!({"value": {"recorded": true}}))).await;
            let (status, body) = call(
                &fx,
                "POST",
                &format!("{ROOM}/members"),
                Some(json!({"member_id": "user-2", "idempotency_key": "invite-key-1"})),
                &[],
            )
            .await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body["data"]["changed"], changed);
            let invites = invite_calls(&fx).await;
            assert_eq!(invites.len(), want_calls, "changed={changed}");
            if let Some(invite) = invites.first() {
                let args = &invite["args"];
                assert_eq!(args["inviteeAuthId"], "user-2", "the invitee is the member id");
                assert_eq!(args["externalAuthId"], "user-1", "the inviter is the session user");
                assert_eq!(args["externalOrgId"], "org-1");
                assert_eq!(args["spaceRef"], "space-room");
                assert_eq!(args["idempotencyKey"], "invite-key-1");
                assert_eq!(invite["_route"], "/api/mutation");
            }
        }
    }

    #[tokio::test]
    async fn add_member_without_an_idempotency_key_sends_none_to_the_invite_mutation() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_writer("owner").await;
        mount_control(&fx, "POST", CONTROL_MEMBERS, control_ok(json!({"member_count": 2, "changed": true, "role": "editor"}))).await;
        mount_convex(&fx, "/api/action", reconcile("noop")).await;
        mount_convex(&fx, "/api/mutation", ResponseTemplate::new(200).set_body_json(json!({"value": {"recorded": true}}))).await;
        let (status, _) = call(&fx, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-3"})), &[]).await;
        assert_eq!(status, 200);
        let invites = invite_calls(&fx).await;
        assert_eq!(invites.len(), 1);
        assert!(invites[0]["args"].get("idempotencyKey").is_none());
        assert_eq!(invites[0]["args"]["inviteeAuthId"], "user-3");
    }

    #[tokio::test]
    async fn a_convex_failure_leaves_the_add_member_response_unchanged() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let ok = fx_writer("owner").await;
        mount_control(&ok, "POST", CONTROL_MEMBERS, control_ok(json!({"member_count": 4, "changed": true, "role": "editor"}))).await;
        mount_convex(&ok, "/api/action", reconcile("noop")).await;
        mount_convex(&ok, "/api/mutation", ResponseTemplate::new(200).set_body_json(json!({"value": {"recorded": true}}))).await;
        let (ok_status, ok_body) = call(&ok, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), &[]).await;

        for failing in [
            ResponseTemplate::new(500),
            ResponseTemplate::new(200).set_body_json(json!({"status": "error", "errorMessage": "boom"})),
        ] {
            let fx = fx_writer("owner").await;
            mount_control(&fx, "POST", CONTROL_MEMBERS, control_ok(json!({"member_count": 4, "changed": true, "role": "editor"}))).await;
            mount_convex(&fx, "/api/action", reconcile("noop")).await;
            mount_convex(&fx, "/api/mutation", failing).await;
            let (status, body) = call(&fx, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), &[]).await;
            assert_eq!(status, ok_status);
            assert_eq!(body, ok_body, "the attention outcome never shows in the response");
            assert_eq!(invite_calls(&fx).await.len(), 1, "it was attempted");
        }
    }

    #[tokio::test]
    async fn a_failed_control_add_makes_no_invite_call() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for control_status in [403u16, 404, 409, 500] {
            let fx = fx_writer("owner").await;
            mount_control(
                &fx,
                "POST",
                CONTROL_MEMBERS,
                ResponseTemplate::new(control_status).set_body_json(json!({"error": {"code": "room_archived", "message": "no"}})),
            )
            .await;
            mount_convex(&fx, "/api/action", reconcile("noop")).await;
            mount_convex(&fx, "/api/mutation", ResponseTemplate::new(200).set_body_json(json!({"value": {"recorded": true}}))).await;
            let (status, _) = call(&fx, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), &[]).await;
            assert_ne!(status, 200, "control {control_status}");
            assert!(convex_requests(&fx).await.is_empty(), "control {control_status}: no reconcile and no invite");
        }
    }

    // ---- WS04-S8.gw: self-join and room settings -------------------------------------------------

    const CONTROL_JOIN: &str = "/api/v1/internal/spaces/space-room/join";
    const CONTROL_SETTINGS: &str = "/api/v1/internal/spaces/space-room/settings";

    async fn fx_directory(role: &str) -> Fx {
        let mut fx = fx(role).await;
        fx.state.space_features.room_directory_api = true;
        fx
    }

    async fn control_received(fx: &Fx, method: &str, path: &str) -> Vec<Value> {
        fx.user_core
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .filter(|request| request.method.to_string() == method && request.url.path() == path)
            .map(|request| serde_json::from_slice(&request.body).unwrap_or(Value::Null))
            .collect()
    }

    #[tokio::test]
    async fn join_and_settings_answer_404_feature_disabled_when_the_directory_flag_is_off() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner").await;
        for (method, uri, body) in [
            ("POST", format!("{ROOM}/join"), json!({})),
            ("PATCH", format!("{ROOM}/settings"), json!({"visibility": "private"})),
        ] {
            let (status, got) = call(&fx, method, &uri, Some(body), &[]).await;
            assert_eq!(status, 404, "{method}");
            assert_eq!(got["error"]["code"], "feature_disabled");
        }
        assert!(control_calls(&fx).await.is_empty());
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn join_and_settings_refuse_bad_bodies_before_control_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_directory("owner").await;
        let join = format!("{ROOM}/join");
        let settings = format!("{ROOM}/settings");
        for (method, uri, body) in [
            ("POST", join.clone(), json!({"role": "owner"})),
            ("POST", join.clone(), json!({"idempotency_key": " "})),
            ("POST", join.clone(), json!([])),
            ("PATCH", settings.clone(), json!({})),
            ("PATCH", settings.clone(), json!({"visibility": null, "join_role": null})),
            ("PATCH", settings.clone(), json!({"visibility": "public"})),
            ("PATCH", settings.clone(), json!({"visibility": 7})),
            ("PATCH", settings.clone(), json!({"join_role": "owner"})),
            ("PATCH", settings.clone(), json!({"join_role": "manager"})),
            ("PATCH", settings.clone(), json!({"history_mode": "forever"})),
            ("PATCH", settings.clone(), json!({"visibility": "private", "org_id": "forged"})),
            ("PATCH", settings, json!({"visibility": "private", "idempotency_key": ""})),
        ] {
            let (status, got) = call(&fx, method, &uri, Some(body.clone()), &[]).await;
            assert_eq!(status, 400, "{method} {body}");
            assert_eq!(got["error"]["code"], "invalid_request", "{method} {body}");
        }
        assert!(control_calls(&fx).await.is_empty());
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn join_and_settings_pass_control_refusals_through_by_code() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (control_status, code) in [(404u16, "room_not_found"), (409, "room_archived")] {
            let fx = fx_directory("viewer").await;
            let refusal = ResponseTemplate::new(control_status)
                .set_body_json(json!({"error": {"code": code, "message": "Refused.", "details": {"nested": {"x": 1}, "kind": "room"}}}));
            mount_control(&fx, "POST", CONTROL_JOIN, refusal.clone()).await;
            mount_control(&fx, "PATCH", CONTROL_SETTINGS, refusal).await;
            mount_convex(&fx, "/api/action", reconcile("noop")).await;
            let (status, got) = call(&fx, "POST", &format!("{ROOM}/join"), Some(json!({})), &[]).await;
            assert_eq!(status, control_status, "join");
            assert_eq!(got["error"]["code"], code);
            assert_eq!(got["error"]["details"], json!({"kind": "room"}), "only scalar details survive");
            let (status, got) = call(&fx, "PATCH", &format!("{ROOM}/settings"), Some(json!({"join_role": "viewer"})), &[]).await;
            assert_eq!(status, control_status, "settings");
            assert_eq!(got["error"]["code"], code);
            assert!(convex_requests(&fx).await.is_empty(), "a refused write reconciles nothing");
        }
    }

    #[tokio::test]
    async fn a_join_success_reconciles_the_audience_and_is_not_recipient_ready() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_directory("viewer").await;
        mount_control(
            &fx,
            "POST",
            CONTROL_JOIN,
            control_ok(json!({"space_ref": "space-room", "role": "editor", "changed": true, "already_member": false})),
        )
        .await;
        mount_convex(&fx, "/api/action", reconcile("replaced")).await;
        let (status, body) = call(&fx, "POST", &format!("{ROOM}/join"), Some(json!({"idempotency_key": "join-key-1"})), &[]).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"],
            json!({
                "space_ref": "space-room", "role": "editor", "changed": true, "already_member": false,
                "recipient_ready": false, "audience": {"state": "pending"}
            })
        );
        let sent = control_received(&fx, "POST", CONTROL_JOIN).await;
        assert_eq!(sent, vec![json!({"idempotency_key": "join-key-1"})]);
        let convex = convex_requests(&fx).await;
        assert_eq!(convex.len(), 1);
        assert_eq!(convex[0]["path"], "spaceMembers:reconcileRoomAudienceForGateway");
        assert_eq!(convex[0]["args"]["externalOrgId"], "org-1");
        assert_eq!(convex[0]["args"]["spaceRef"], "space-room");
        let _ = &fx.model_gateway;
    }

    #[tokio::test]
    async fn a_join_with_an_unconfirmed_audience_warns_and_never_rolls_back() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_directory("viewer").await;
        mount_control(&fx, "POST", CONTROL_JOIN, control_ok(json!({"changed": false, "already_member": true, "role": "viewer"}))).await;
        mount_convex(&fx, "/api/action", ResponseTemplate::new(500)).await;
        let (status, body) = call(&fx, "POST", &format!("{ROOM}/join"), None, &[]).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["data"]["audience"]["state"], "unconfirmed");
        assert_eq!(body["data"]["recipient_ready"], false);
        assert_eq!(body["data"]["already_member"], true);
        assert_eq!(body["meta"]["warnings"], json!(["room_audience_unconfirmed"]));
    }

    #[tokio::test]
    async fn settings_forward_only_the_whitelisted_fields_and_do_not_reconcile() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_directory("manager").await;
        mount_control(
            &fx,
            "PATCH",
            CONTROL_SETTINGS,
            control_ok(json!({"space_ref": "space-room", "visibility": "org_public", "join_role": "viewer", "history_mode": "since_join", "changed": true})),
        )
        .await;
        mount_convex(&fx, "/api/action", reconcile("replaced")).await;
        let (status, body) = call(
            &fx,
            "PATCH",
            &format!("{ROOM}/settings"),
            Some(json!({"visibility": " org_public ", "join_role": "viewer", "history_mode": "since_join", "idempotency_key": "set-key-1"})),
            &[],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"],
            json!({"space_ref": "space-room", "visibility": "org_public", "join_role": "viewer", "history_mode": "since_join", "changed": true})
        );
        let sent = control_received(&fx, "PATCH", CONTROL_SETTINGS).await;
        assert_eq!(
            sent,
            vec![json!({"visibility": "org_public", "join_role": "viewer", "history_mode": "since_join", "idempotency_key": "set-key-1"})]
        );
        assert!(convex_requests(&fx).await.is_empty(), "a settings change does not alter the recipient set");
    }

    // ---- D18: per-member room history -----------------------------------------------------------------

    const CONTROL_WINDOW: &str = "/api/v1/internal/spaces/space-room/members/user-2/history-window";
    const WINDOW: &str = "/api/v1/spaces/space-room/members/user-2/history-window";

    /// The gateway with its own history-window switch on and the caller holding `role`.
    async fn fx_windows(role: &str) -> Fx {
        let mut fx = fx(role).await;
        fx.state.space_features.member_history_windows = true;
        fx
    }

    /// Like `call`, but with a raw JSON body and the response headers.
    async fn send(
        fx: &Fx,
        method: &str,
        uri: &str,
        raw: Option<&str>,
        extra_headers: &[(&str, &str)],
    ) -> (u16, Value, axum::http::HeaderMap) {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
        for (name, value) in extra_headers {
            request = request.header(*name, *value);
        }
        if raw.is_some() {
            request = request.header("content-type", "application/json");
        }
        let response = crate::build_router(fx.state.clone())
            .oneshot(
                request
                    .body(match raw {
                        Some(raw) => Body::from(raw.to_owned()),
                        None => Body::empty(),
                    })
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let payload: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap_or(Value::Null);
        (status, payload, headers)
    }

    /// The one Control request with this method and path.
    async fn control_request(fx: &Fx, method: &str, path: &str) -> wiremock::Request {
        fx.user_core
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.method.as_str() == method && request.url.path() == path)
            .unwrap_or_else(|| panic!("no Control {method} {path}"))
    }

    fn header_of(request: &wiremock::Request, name: &str) -> Option<String> {
        request.headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)
    }

    fn membership_reads(requests: &[wiremock::Request]) -> usize {
        requests.iter().filter(|request| request.url.path().ends_with("/membership")).count()
    }

    #[tokio::test]
    async fn d18_the_history_window_routes_answer_404_feature_disabled_with_the_switch_off_and_call_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx("owner").await;
        assert!(!fx.state.space_features.member_history_windows, "off by default");
        let valid = json!({"mode": "since_date", "since_at": "2026-11-01T00:00:00Z"}).to_string();
        for (method, raw) in [
            ("PUT", Some(valid.as_str())),
            // The switch comes before the body is read: a malformed body is still the 404.
            ("PUT", Some("{not json")),
            ("PUT", None),
            ("GET", None),
        ] {
            let (status, body, headers) = send(&fx, method, WINDOW, raw, &[]).await;
            assert_eq!(status, 404, "{method} {raw:?}");
            assert_eq!(body["error"]["code"], "feature_disabled", "{method} {raw:?}");
            assert_eq!(headers.get("cache-control").and_then(|value| value.to_str().ok()), Some("no-store"));
        }
        assert!(control_calls(&fx).await.is_empty());
        assert!(convex_requests(&fx).await.is_empty());
        assert_eq!(
            membership_reads(&fx.user_core.received_requests().await.unwrap()),
            0,
            "the switch is checked before Control is asked anything"
        );
        // The functions the routes share answer the same way.
        let user = crate::middleware::AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.com".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some("org-1".to_owned()),
            authorized_membership: None,
        };
        let (status, axum::Json(body)) =
            super::set_member_history_window(&fx.state, &user, "space-room", "user-2", &json!({"mode": "full"})).await;
        assert_eq!((status.as_u16(), &body["error"]["code"]), (404, &json!("feature_disabled")));
        let (status, _) = super::get_member_history_window(&fx.state, &user, "space-room", "user-2").await;
        assert_eq!(status.as_u16(), 404);
        assert!(control_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn d18_put_forwards_the_closed_fields_in_canonical_form_and_returns_the_rebuilt_window() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_windows("owner").await;
        mount_control(
            &fx,
            "PUT",
            CONTROL_WINDOW,
            control_ok(json!({
                "space_ref": "space-room", "subject_id": "user-2", "changed": true,
                "history_window": {
                    "source": "member", "mode": "since_date", "since_at": "2026-11-01T08:00:00Z", "revision": 3,
                    "set_by": "internal-user", "floor_token": "internal-secret"
                },
                "debug": "internal"
            })),
        )
        .await;
        mount_convex(&fx, "/api/action", reconcile("replaced")).await;
        let request = json!({"mode": " since_date ", "since_at": " 2026-11-01T09:00:00+01:00 ", "expected_revision": 2});
        let (status, body, headers) = send(
            &fx,
            "PUT",
            WINDOW,
            Some(&request.to_string()),
            &[("x-org-id", "forged-org"), ("x-user-id", "forged-user"), ("x-delegation-signature", "forged")],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body,
            json!({"data": {
                "space_ref": "space-room", "subject_id": "user-2", "changed": true,
                "history_window": {"source": "member", "mode": "since_date", "since_at": "2026-11-01T08:00:00Z", "revision": 3}
            }}),
            "exactly four window fields come back: nothing else Control said"
        );
        assert_eq!(headers.get("cache-control").and_then(|value| value.to_str().ok()), Some("no-store"));
        assert!(!body.to_string().contains("internal"), "{body}");
        // Control got the closed fields only, the instant as one canonical UTC string, under the session's identity.
        let sent = control_request(&fx, "PUT", CONTROL_WINDOW).await;
        assert_eq!(
            serde_json::from_slice::<Value>(&sent.body).unwrap(),
            json!({"mode": "since_date", "since_at": "2026-11-01T08:00:00Z", "expected_revision": 2})
        );
        assert_eq!(header_of(&sent, "x-org-id").as_deref(), Some("org-1"), "the organization is the session's");
        assert_eq!(header_of(&sent, "x-user-id").as_deref(), Some("user-1"), "the actor is the session's");
        assert_eq!(header_of(&sent, "x-service-id").as_deref(), Some("verevon-gateway"));
        let signature = header_of(&sent, "x-delegation-signature").expect("a signed delegation");
        assert_ne!(signature, "forged");
        assert!(header_of(&sent, "x-delegation-body-sha256").is_some());
        assert!(
            convex_requests(&fx).await.is_empty(),
            "a window changes what one member sees, never the recipient set: no audience reconcile"
        );
    }

    #[tokio::test]
    async fn d18_put_sends_only_the_mode_for_full_and_since_join_and_leaves_the_pair_rule_to_control() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (mode, source, changed) in [("full", "member", true), ("since_join", "member", false)] {
            let fx = fx_windows("manager").await;
            mount_control(
                &fx,
                "PUT",
                CONTROL_WINDOW,
                control_ok(json!({
                    "changed": changed,
                    "history_window": {"source": source, "mode": mode, "since_at": null, "revision": 1}
                })),
            )
            .await;
            let (status, body, _) = send(&fx, "PUT", WINDOW, Some(&json!({"mode": mode}).to_string()), &[]).await;
            assert_eq!(status, 200, "{mode}: {body}");
            assert_eq!(body["data"]["changed"], changed, "{mode}");
            assert_eq!(body["data"]["history_window"]["mode"], mode);
            assert!(body["data"]["history_window"]["since_at"].is_null());
            let sent = control_received(&fx, "PUT", CONTROL_WINDOW).await;
            assert_eq!(sent, vec![json!({"mode": mode})], "an unsent field stays unsent");
        }
        // A pair Control refuses (an instant with a mode that takes none) is forwarded for Control to refuse:
        // the gateway checks the shape of the instant, not what the pair means.
        let fx = fx_windows("owner").await;
        mount_control(
            &fx,
            "PUT",
            CONTROL_WINDOW,
            ResponseTemplate::new(422).set_body_json(json!({"error": {
                "code": "member_history_window_invalid", "message": "since_at is only allowed with since_date",
                "details": {"field": "since_at"}
            }})),
        )
        .await;
        let (status, body, _) = send(
            &fx,
            "PUT",
            WINDOW,
            Some(&json!({"mode": "full", "since_at": "2026-11-01T00:00:00Z"}).to_string()),
            &[],
        )
        .await;
        assert_eq!(status, 422, "{body}");
        assert_eq!(body["error"]["code"], "member_history_window_invalid");
        assert_eq!(body["error"]["details"], json!({"field": "since_at"}));
        assert_eq!(
            control_received(&fx, "PUT", CONTROL_WINDOW).await,
            vec![json!({"mode": "full", "since_at": "2026-11-01T00:00:00Z"})]
        );
    }

    #[tokio::test]
    async fn d18_put_refuses_bad_bodies_before_control_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_windows("owner").await;
        let cases: Vec<(Value, &str)> = vec![
            (json!({}), "mode"),
            (json!({"since_at": "2026-11-01T00:00:00Z"}), "mode"),
            (json!({"mode": null}), "mode"),
            (json!({"mode": 3}), "mode"),
            (json!({"mode": ""}), "mode"),
            (json!({"mode": "forever"}), "mode"),
            // `room` is the explicit no-override row only an invite writes; a person can never request it.
            (json!({"mode": "room"}), "mode"),
            (json!({"mode": "Full"}), "mode"),
            (json!({"mode": "since_date", "since_at": "yesterday"}), "since_at"),
            (json!({"mode": "since_date", "since_at": "2026-11-01"}), "since_at"),
            (json!({"mode": "since_date", "since_at": "2026-11-01T09:00:00"}), "since_at"),
            (json!({"mode": "since_date", "since_at": 1_730_000_000}), "since_at"),
            (json!({"mode": "since_date", "since_at": ""}), "since_at"),
            (json!({"mode": "since_date", "since_at": "9".repeat(80)}), "since_at"),
            (json!({"mode": "since_date", "since_at": ["2026-11-01T00:00:00Z"]}), "since_at"),
            (json!({"mode": "full", "expected_revision": -1}), "expected_revision"),
            (json!({"mode": "full", "expected_revision": "3"}), "expected_revision"),
            (json!({"mode": "full", "expected_revision": 1.5}), "expected_revision"),
            (json!({"mode": "full", "org_id": "forged"}), "body"),
            (json!({"mode": "full", "subject_id": "user-9"}), "body"),
            (json!({"mode": "full", "actor_id": "forged"}), "body"),
            (json!({"mode": "full", "history_floor_at": "2026-01-01T00:00:00Z"}), "body"),
            (json!({"mode": "full", "idempotency_key": "k"}), "body"),
            (json!(["full"]), "body"),
            (json!("full"), "body"),
        ];
        for (body, field) in cases {
            let (status, got, _) = send(&fx, "PUT", WINDOW, Some(&body.to_string()), &[]).await;
            assert_eq!(status, 400, "{body}");
            assert_eq!(got["error"]["code"], "invalid_request", "{body}");
            assert_eq!(got["error"]["details"]["field"], field, "{body}");
        }
        assert!(control_calls(&fx).await.is_empty(), "nothing reached Control");
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn d18_put_answers_a_typed_400_for_a_missing_malformed_or_oversized_body() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_windows("owner").await;
        let oversized = format!("{}{{}}", " ".repeat(5000));
        for (raw, field) in [
            (None, "mode"),
            (Some(""), "mode"),
            (Some("   \n"), "mode"),
            (Some("{not json"), "body"),
            (Some("null"), "body"),
            (Some(oversized.as_str()), "body"),
        ] {
            let (status, got, _) = send(&fx, "PUT", WINDOW, raw, &[]).await;
            assert_eq!(status, 400, "{raw:?}");
            assert_eq!(got["error"]["code"], "invalid_request", "a typed envelope, never the framework's text: {raw:?}");
            assert_eq!(got["error"]["details"]["field"], field, "{raw:?}");
        }
        assert!(control_calls(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn d18_put_and_get_pass_every_typed_control_refusal_through_with_its_status() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let refusals: Vec<(u16, &str, Option<Value>)> = vec![
            (400, "invalid_request", Some(json!({"field": "mode"}))),
            (403, "room_role_forbidden", None),
            (403, "room_delegation_required", None),
            (404, "room_not_found", None),
            (404, "member_not_found", None),
            (409, "room_archived", None),
            (409, "room_roster_derived", None),
            (409, "room_member_changed", Some(json!({"current_revision": 7}))),
            (422, "member_history_window_invalid", Some(json!({"field": "since_at"}))),
            (503, "member_history_window_unavailable", None),
            (503, "space_authority_unavailable", None),
        ];
        let put_body = json!({"mode": "since_date", "since_at": "2026-11-01T00:00:00Z"});
        for (control_status, code, details) in refusals {
            for (method, body) in [("PUT", Some(put_body.clone())), ("GET", None)] {
                let fx = fx_windows("owner").await;
                // Control's typed envelope always carries `details`, an empty object when a refusal has none.
                let wanted = details.clone().unwrap_or_else(|| json!({}));
                let mut with_extras = wanted.clone();
                // What Control adds beyond scalars never reaches the browser.
                with_extras["nested"] = json!({"x": 1});
                with_extras["list"] = json!([1]);
                mount_control(
                    &fx,
                    method,
                    CONTROL_WINDOW,
                    ResponseTemplate::new(control_status).set_body_json(json!({
                        "error": {"code": code, "message": "Refused.", "details": with_extras},
                        "debug": "internal trace", "token": "secret"
                    })),
                )
                .await;
                mount_convex(&fx, "/api/action", reconcile("replaced")).await;
                let (status, got) = call(&fx, method, WINDOW, body, &[]).await;
                assert_eq!(status, control_status, "{method} {code}");
                assert_eq!(
                    got,
                    json!({"error": {"code": code, "message": "Refused.", "details": wanted}}),
                    "{method} {code}"
                );
                assert!(convex_requests(&fx).await.is_empty(), "{method} {code}: a refusal reconciles nothing");
            }
        }
    }

    #[tokio::test]
    async fn d18_an_untyped_or_session_looking_control_failure_is_a_503_that_leaks_nothing() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let cases = [
            ResponseTemplate::new(500).set_body_json(json!({"error": "boom-internal"})),
            ResponseTemplate::new(403).set_body_json(json!({"error": "boom-internal"})),
            // A typed 401 would read as an expired session in the SPA: it becomes the 503 as well.
            ResponseTemplate::new(401).set_body_json(json!({"error": {"code": "unauthorized", "message": "boom-internal"}})),
            ResponseTemplate::new(409).set_body_json(json!({"error": {"code": "Bad Code!", "message": "boom-internal"}})),
        ];
        for template in cases {
            for (method, body) in [("PUT", Some(json!({"mode": "full"}))), ("GET", None)] {
                let fx = fx_windows("owner").await;
                mount_control(&fx, method, CONTROL_WINDOW, template.clone()).await;
                let (status, got) = call(&fx, method, WINDOW, body, &[]).await;
                assert_eq!(status, 503, "{method} {got}");
                assert_eq!(got["error"]["code"], "room_admin_unavailable", "{method}");
                assert!(!got.to_string().contains("boom-internal"), "{method} {got}");
            }
        }
        // A body that is not JSON is reduced by the upstream layer to its own fixed words.
        let fx = fx_windows("owner").await;
        mount_control(&fx, "GET", CONTROL_WINDOW, ResponseTemplate::new(502).set_body_string("<html>boom-internal</html>")).await;
        let (status, got) = call(&fx, "GET", WINDOW, None, &[]).await;
        assert_eq!(status, 502, "{got}");
        assert_eq!(got["error"]["code"], "upstream_error");
        assert!(!got.to_string().contains("boom-internal"), "{got}");
    }

    #[tokio::test]
    async fn d18_a_control_success_that_is_not_a_window_is_a_502_and_never_a_success() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let bad: Vec<Value> = vec![
            json!({}),
            json!({"data": {}}),
            json!({"data": {"changed": true}}),
            json!({"data": {"history_window": "full"}}),
            json!({"data": {"history_window": null}}),
            json!({"data": {"history_window": {"mode": "full"}}}),
            json!({"data": {"history_window": {"source": "member"}}}),
            json!({"data": {"history_window": {"source": "member", "mode": 7}}}),
            json!({"data": {"history_window": {"source": "Member Row", "mode": "full"}}}),
            json!({"data": {"history_window": {"source": "member", "mode": "since_date", "since_at": 5}}}),
            json!({"data": {"history_window": {"source": "member", "mode": "since_date", "since_at": "not a date"}}}),
            json!({"data": {"history_window": {"source": "member", "mode": "full", "revision": -1}}}),
            json!({"data": {"history_window": {"source": "member", "mode": "full", "revision": "1"}}}),
            json!({"data": {"history_window": {"source": "member", "mode": "full", "revision": 1.5}}}),
        ];
        for control_body in bad {
            for (method, body) in [("PUT", Some(json!({"mode": "full"}))), ("GET", None)] {
                let fx = fx_windows("owner").await;
                mount_control(&fx, method, CONTROL_WINDOW, ResponseTemplate::new(200).set_body_json(control_body.clone())).await;
                let (status, got) = call(&fx, method, WINDOW, body, &[]).await;
                assert_eq!(status, 502, "{method} {control_body}");
                // The upstream layer's own code, which the SPA already words: no code of its own for a should-never-happen.
                assert_eq!(got["error"]["code"], "upstream_unavailable", "{method} {control_body}");
                assert!(got.get("data").is_none(), "{method} {control_body}");
            }
        }
        // A 200 whose body is not JSON at all is the same: the upstream layer reads it as an empty answer.
        for (method, body) in [("PUT", Some(json!({"mode": "full"}))), ("GET", None)] {
            let fx = fx_windows("owner").await;
            mount_control(&fx, method, CONTROL_WINDOW, ResponseTemplate::new(200).set_body_string("<html>boom-internal</html>")).await;
            let (status, got) = call(&fx, method, WINDOW, body, &[]).await;
            assert_eq!(status, 502, "{method} {got}");
            assert_eq!(got["error"]["code"], "upstream_unavailable", "{method}");
            assert!(!got.to_string().contains("boom-internal"), "{method} {got}");
        }
    }

    #[tokio::test]
    async fn d18_get_reads_the_window_with_no_body_and_a_member_without_a_row_reads_under_the_room() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_windows("manager").await;
        mount_control(
            &fx,
            "GET",
            CONTROL_WINDOW,
            control_ok(json!({
                "space_ref": "space-room", "subject_id": "user-2",
                "history_window": {"source": "room", "mode": "since_join", "since_at": null, "revision": 0}
            })),
        )
        .await;
        let (status, body, headers) = send(
            &fx,
            "GET",
            &format!("{WINDOW}?org_id=forged&mode=full"),
            None,
            &[("x-org-id", "forged-org")],
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body,
            json!({"data": {
                "space_ref": "space-room", "subject_id": "user-2",
                "history_window": {"source": "room", "mode": "since_join", "since_at": null, "revision": 0}
            }}),
            "a read has no `changed`"
        );
        assert_eq!(headers.get("cache-control").and_then(|value| value.to_str().ok()), Some("no-store"));
        let sent = control_request(&fx, "GET", CONTROL_WINDOW).await;
        assert!(sent.body.is_empty(), "a read forwards no body");
        assert_eq!(sent.url.query(), None, "nothing of the browser's query reaches Control");
        assert_eq!(header_of(&sent, "x-org-id").as_deref(), Some("org-1"));
        assert_eq!(header_of(&sent, "x-user-id").as_deref(), Some("user-1"));
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn d18_the_ids_in_the_answer_are_the_requests_and_a_changed_flag_defaults_to_false() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_windows("owner").await;
        // Control names other ids: the answer is never relabelled by them.
        mount_control(
            &fx,
            "PUT",
            CONTROL_WINDOW,
            control_ok(json!({
                "space_ref": "space-other", "subject_id": "user-9",
                "history_window": {"source": "member", "mode": "full"}
            })),
        )
        .await;
        let (status, body, _) = send(&fx, "PUT", &format!("{ROOM}/members/user-2/history-window"), Some(&json!({"mode": "full"}).to_string()), &[]).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            body["data"],
            json!({
                "space_ref": "space-room", "subject_id": "user-2", "changed": false,
                "history_window": {"source": "member", "mode": "full", "since_at": null, "revision": 0}
            })
        );
    }

    #[tokio::test]
    async fn d18_subject_ids_that_would_leave_the_control_path_or_are_not_ids_are_refused() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_windows("owner").await;
        let long = "u".repeat(201);
        for (subject, code) in [
            ("%2E%2E", "invalid_request"),
            ("%2E", "invalid_request"),
            ("%20", "member_required"),
            ("%00", "invalid_request"),
            ("user%0A2", "invalid_request"),
            ("user%2F2", "invalid_request"),
            ("user%5C2", "invalid_request"),
            (long.as_str(), "invalid_request"),
        ] {
            for (method, raw) in [("PUT", Some(r#"{"mode":"full"}"#)), ("GET", None)] {
                let (status, got, _) =
                    send(&fx, method, &format!("{ROOM}/members/{subject}/history-window"), raw, &[]).await;
                assert_eq!(status, 400, "{method} {subject}: {got}");
                assert_eq!(got["error"]["code"], code, "{method} {subject}");
            }
        }
        assert!(control_calls(&fx).await.is_empty(), "no refused id reached Control");
    }

    #[tokio::test]
    async fn d18_the_history_window_path_takes_put_and_get_only_and_the_member_routes_still_work() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx_windows("owner").await;
        fx.state.space_features.membership_writer_api = true;
        for method in ["POST", "PATCH", "DELETE"] {
            let (status, _, _) = send(&fx, method, WINDOW, Some(r#"{"mode":"full"}"#), &[]).await;
            assert_eq!(status, 405, "{method}");
        }
        assert!(control_calls(&fx).await.is_empty());
        // Both routes share the `members/{id}` segment: the role change and the removal still resolve.
        mount_control(&fx, "PATCH", &format!("{CONTROL_MEMBERS}/user-2"), control_ok(json!({"role": "viewer", "from_role": "editor", "changed": true}))).await;
        let (status, got) = call(&fx, "PATCH", &format!("{ROOM}/members/user-2"), Some(json!({"role": "viewer"})), &[]).await;
        assert_eq!(status, 200, "{got}");
        mount_control(&fx, "DELETE", &format!("{CONTROL_MEMBERS}/user-2"), control_ok(json!({"changed": true, "left": false, "member_count": 1}))).await;
        mount_convex(&fx, "/api/action", reconcile("noop")).await;
        let (status, got) = call(&fx, "DELETE", &format!("{ROOM}/members/user-2"), None, &[]).await;
        assert_eq!(status, 200, "{got}");
    }

    /// The window routes move a mode and an instant, no content: a ZDR organization is not refused, and no model is called.
    /// The positive control proves the fixture really is ZDR: a durable content write is refused with the typed 412.
    #[tokio::test]
    async fn d18_a_zdr_organization_may_set_and_read_a_window_and_no_model_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let mut fx = fx_windows("owner").await;
        let org = MockServer::start().await;
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/organizations/org-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": {"metadata": {"interactiveRetention": {"zdr": true}}}
            })))
            .mount(&org)
            .await;
        fx.state.org_core_url = org.uri();
        let (status, _) = call(&fx, "PATCH", &format!("{ROOM}/instructions"), Some(json!({"instructions": "x"})), &[]).await;
        assert_eq!(status, 412, "the fixture is a ZDR organization: a durable content write is refused");
        mount_control(
            &fx,
            "PUT",
            CONTROL_WINDOW,
            control_ok(json!({"changed": true, "history_window": {"source": "member", "mode": "since_join", "since_at": null, "revision": 1}})),
        )
        .await;
        mount_control(
            &fx,
            "GET",
            CONTROL_WINDOW,
            control_ok(json!({"history_window": {"source": "member", "mode": "since_join", "since_at": null, "revision": 1}})),
        )
        .await;
        let (status, body, _) = send(&fx, "PUT", WINDOW, Some(r#"{"mode":"since_join"}"#), &[("x-zdr", "true")]).await;
        assert_eq!(status, 200, "{body}");
        let (status, body, _) = send(&fx, "GET", WINDOW, None, &[("x-zdr", "true")]).await;
        assert_eq!(status, 200, "{body}");
        assert!(fx.model_gateway.received_requests().await.unwrap().is_empty());
    }

    #[test]
    fn d18_canonical_instant_is_the_same_instant_in_utc_and_a_bad_string_is_none() {
        assert_eq!(super::canonical_instant("2026-11-01T08:00:00Z").as_deref(), Some("2026-11-01T08:00:00Z"));
        assert_eq!(super::canonical_instant(" 2026-11-01T09:00:00+01:00 ").as_deref(), Some("2026-11-01T08:00:00Z"));
        assert_eq!(super::canonical_instant("2026-11-01T09:00:00.5+01:00").as_deref(), Some("2026-11-01T08:00:00.500Z"));
        assert_eq!(super::canonical_instant("2026-10-31T23:30:00-05:00").as_deref(), Some("2026-11-01T04:30:00Z"));
        for bad in ["", "  ", "2026-11-01", "2026-11-01T09:00:00", "yesterday", "2026-13-01T00:00:00Z", "2026-11-01T24:30:00Z"] {
            assert_eq!(super::canonical_instant(bad), None, "{bad:?}");
        }
        assert_eq!(super::canonical_instant(&"9".repeat(65)), None);
    }

    #[test]
    fn d18_history_window_view_rebuilds_four_fields_and_rejects_anything_that_is_not_a_window() {
        let view = super::history_window_view(Some(&json!({
            "source": "member", "mode": "since_date", "since_at": " 2026-11-01T08:00:00Z ", "revision": 4,
            "set_by": "user-1", "extra": {"nested": true}
        })))
        .expect("a window");
        assert_eq!(
            view,
            json!({"source": "member", "mode": "since_date", "since_at": "2026-11-01T08:00:00Z", "revision": 4})
        );
        // A missing revision is 0 and a missing or null instant is null: both are what a room-sourced window reports.
        assert_eq!(
            super::history_window_view(Some(&json!({"source": "room", "mode": "full"}))),
            Some(json!({"source": "room", "mode": "full", "since_at": null, "revision": 0}))
        );
        // A mode the gateway does not know is a plain word and passes; a window is not an outage.
        assert!(super::history_window_view(Some(&json!({"source": "member", "mode": "since_message"}))).is_some());
        for bad in [
            None,
            Some(json!(null)),
            Some(json!([])),
            Some(json!("full")),
            Some(json!({})),
            Some(json!({"source": "member"})),
            Some(json!({"mode": "full"})),
            Some(json!({"source": "", "mode": "full"})),
            Some(json!({"source": "member", "mode": "since join"})),
            Some(json!({"source": "member", "mode": "full", "since_at": "soon"})),
            Some(json!({"source": "member", "mode": "full", "revision": -2})),
            Some(json!({"source": "member", "mode": "full", "revision": 1.25})),
        ] {
            assert_eq!(super::history_window_view(bad.as_ref()), None, "{bad:?}");
        }
    }

    // ---- D18: the invite's choice on POST .../members -------------------------------------------------

    #[tokio::test]
    async fn d18_add_member_forwards_share_past_conversation_as_given_and_returns_the_window() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // The gateway's own window switch is not what carries the choice: it is forwarded either way.
        for windows_switch in [false, true] {
            for (request_value, want_sent, window) in [
                (
                    Some(json!(false)),
                    Some(false),
                    json!({"source": "member", "mode": "since_join", "since_at": null, "revision": 1, "set_by": "user-1"}),
                ),
                (
                    Some(json!(true)),
                    Some(true),
                    json!({"source": "room", "mode": "full", "since_at": null, "revision": 0}),
                ),
                (None, None, json!({"source": "room", "mode": "since_join", "since_at": null, "revision": 0})),
                (
                    Some(Value::Null),
                    None,
                    json!({"source": "room", "mode": "full", "since_at": null, "revision": 0}),
                ),
            ] {
                let mut fx = fx_writer("owner").await;
                fx.state.space_features.member_history_windows = windows_switch;
                mount_control(
                    &fx,
                    "POST",
                    CONTROL_MEMBERS,
                    control_ok(json!({"member_count": 2, "changed": true, "role": "editor", "history_window": window})),
                )
                .await;
                mount_convex(&fx, "/api/action", reconcile("noop")).await;
                mount_convex(&fx, "/api/mutation", ResponseTemplate::new(200).set_body_json(json!({"value": {"recorded": true}}))).await;
                let mut request = json!({"member_id": "user-2", "role": "viewer"});
                if let Some(value) = &request_value {
                    request["share_past_conversation"] = value.clone();
                }
                let (status, body) = call(&fx, "POST", &format!("{ROOM}/members"), Some(request), &[]).await;
                assert_eq!(status, 200, "{request_value:?}: {body}");
                let sent = control_received(&fx, "POST", CONTROL_MEMBERS).await;
                assert_eq!(sent.len(), 1);
                assert_eq!(
                    sent[0].get("share_past_conversation").and_then(Value::as_bool),
                    want_sent,
                    "switch={windows_switch} {request_value:?}: forwarded exactly as given, absent when not chosen"
                );
                assert!(
                    sent[0].get("share_past_conversation").is_none() || sent[0]["share_past_conversation"].is_boolean(),
                    "only ever a boolean"
                );
                assert_eq!(sent[0]["subject_id"], "user-2");
                // The window the member reads under comes back rebuilt, with nothing else Control said.
                let got = &body["data"]["history_window"];
                assert_eq!(got.as_object().map(|window| window.len()), Some(4), "{body}");
                assert_eq!(got["source"], window["source"]);
                assert_eq!(got["mode"], window["mode"]);
                assert!(got["since_at"].is_null());
                assert!(!body.to_string().contains("set_by"), "{body}");
                // The rest of the answer is exactly what it was before D18.
                assert_eq!(body["data"]["member_count"], 2);
                assert_eq!(body["data"]["changed"], true);
                assert_eq!(body["data"]["role"], "editor");
                assert_eq!(body["data"]["audience"]["state"], "current");
            }
        }
    }

    #[tokio::test]
    async fn d18_add_member_answers_no_window_when_control_reports_none_and_never_makes_one_up() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // A replay or an already-active member: Control answers no `history_window`.
        let fx = fx_writer("owner").await;
        mount_control(&fx, "POST", CONTROL_MEMBERS, control_ok(json!({"member_count": 2, "changed": false, "role": "editor"}))).await;
        mount_convex(&fx, "/api/action", reconcile("noop")).await;
        let (status, body) = call(&fx, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2"})), &[]).await;
        assert_eq!(status, 200, "{body}");
        assert!(body["data"].get("history_window").is_none(), "{body}");
        // A malformed one is left out and the success stands: the add already committed in Control.
        for malformed in [json!("full"), json!({"mode": 7}), json!({"source": "member", "mode": "full", "revision": -1}), json!([])] {
            let fx = fx_writer("owner").await;
            mount_control(
                &fx,
                "POST",
                CONTROL_MEMBERS,
                control_ok(json!({"member_count": 3, "changed": true, "role": "editor", "history_window": malformed})),
            )
            .await;
            mount_convex(&fx, "/api/action", reconcile("noop")).await;
            mount_convex(&fx, "/api/mutation", ResponseTemplate::new(200).set_body_json(json!({"value": {"recorded": true}}))).await;
            let (status, body) = call(&fx, "POST", &format!("{ROOM}/members"), Some(json!({"member_id": "user-2", "share_past_conversation": false})), &[]).await;
            assert_eq!(status, 200, "{malformed}: {body}");
            assert_eq!(body["data"]["member_count"], 3);
            assert_eq!(body["data"]["changed"], true);
            assert!(body["data"].get("history_window").is_none(), "{malformed}: {body}");
        }
    }

    #[tokio::test]
    async fn d18_add_member_refuses_a_choice_that_is_not_a_boolean_before_control_is_called() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_writer("owner").await;
        // A value that is not a boolean must never degrade into "no choice" and share the past.
        for value in [json!("false"), json!("true"), json!("no"), json!(0), json!(1), json!([]), json!([false]), json!({}), json!({"value": false})] {
            let (status, got) = call(
                &fx,
                "POST",
                &format!("{ROOM}/members"),
                Some(json!({"member_id": "user-2", "share_past_conversation": value})),
                &[],
            )
            .await;
            assert_eq!(status, 400, "{value}");
            assert_eq!(got["error"]["code"], "invalid_request", "{value}");
            assert_eq!(got["error"]["details"]["field"], "share_past_conversation", "{value}");
        }
        assert!(control_calls(&fx).await.is_empty(), "nothing reached Control");
        assert!(convex_requests(&fx).await.is_empty());
    }

    #[tokio::test]
    async fn d18_add_member_passes_the_window_unavailable_refusal_through_and_adds_nobody() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for (status_in, code) in [
            (503u16, "member_history_window_unavailable"),
            (503, "space_authority_unavailable"),
            (403, "room_role_forbidden"),
            (409, "room_archived"),
        ] {
            let fx = fx_writer("owner").await;
            // Control's typed envelope always carries `details`, an empty object when a refusal has none.
            let error = json!({"code": code, "message": "Refused.", "details": {}});
            mount_control(&fx, "POST", CONTROL_MEMBERS, ResponseTemplate::new(status_in).set_body_json(json!({"error": error}))).await;
            mount_convex(&fx, "/api/action", reconcile("replaced")).await;
            mount_convex(&fx, "/api/mutation", ResponseTemplate::new(200).set_body_json(json!({"value": {"recorded": true}}))).await;
            let (status, got) = call(
                &fx,
                "POST",
                &format!("{ROOM}/members"),
                Some(json!({"member_id": "user-2", "share_past_conversation": false})),
                &[],
            )
            .await;
            assert_eq!(status, status_in, "{code}");
            assert_eq!(got, json!({"error": {"code": code, "message": "Refused.", "details": {}}}), "{code}");
            assert!(
                convex_requests(&fx).await.is_empty(),
                "{code}: a refused add reconciles no audience and records no invite"
            );
        }
    }

    // ---- D18: the caller's own window on the membership read and the room context ----------------------

    fn room_list() -> Value {
        json!({"value": [{"spaceRef": "space-room", "name": "Leveranse", "kind": "room", "lifecycle": "active"}]})
    }

    /// Control answers the membership read with `data` (the one mock that wins over the fixture's plain one).
    async fn mount_membership(fx: &Fx, template: ResponseTemplate) {
        Mock::given(wm_method("GET"))
            .and(wm_path("/api/v1/internal/spaces/space-room/membership"))
            .respond_with(template)
            .with_priority(1)
            .mount(&fx.user_core)
            .await;
    }

    #[tokio::test]
    async fn d18_the_membership_read_and_the_room_context_pass_the_callers_own_window_through_unchanged() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        for window in [
            json!({"source": "member", "mode": "since_date", "since_at": "2026-11-01T00:00:00Z", "hidden": true}),
            json!({"source": "member", "mode": "since_join", "since_at": "2026-10-20T09:15:00Z", "hidden": true}),
            json!({"source": "room", "mode": "full", "since_at": null, "hidden": false}),
            json!({"source": "room", "mode": "since_join", "since_at": "2026-10-20T09:15:00Z", "hidden": false}),
        ] {
            let fx = fx_windows("viewer").await;
            mount_membership(
                &fx,
                ResponseTemplate::new(200).set_body_json(json!({"data": {
                    "space_ref": "space-room", "kind": "room", "role": "viewer", "history_window": window
                }})),
            )
            .await;
            mount_convex(&fx, "/api/query", ResponseTemplate::new(200).set_body_json(room_list())).await;
            let (status, body) = call(&fx, "GET", &format!("{ROOM}/membership"), None, &[]).await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body["data"]["history_window"], window, "the membership read carries it unchanged");
            assert_eq!(body["data"]["role"], "viewer");
            let (status, body) = call(&fx, "GET", &format!("{ROOM}/context"), None, &[]).await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(
                body["data"]["membership"]["history_window"], window,
                "the room context carries the same window"
            );
        }
    }

    #[tokio::test]
    async fn d18_a_membership_without_a_window_stays_without_one_and_a_failed_read_makes_none_up() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // An older Control that reports no window: none is added, in either route.
        let fx = fx_windows("editor").await;
        mount_convex(&fx, "/api/query", ResponseTemplate::new(200).set_body_json(room_list())).await;
        let (status, body) = call(&fx, "GET", &format!("{ROOM}/membership"), None, &[]).await;
        assert_eq!(status, 200, "{body}");
        assert!(body["data"].get("history_window").is_none(), "{body}");
        let (status, body) = call(&fx, "GET", &format!("{ROOM}/context"), None, &[]).await;
        assert_eq!(status, 200, "{body}");
        assert!(body["data"]["membership"].get("history_window").is_none(), "{body}");
        // Control cannot read the window and fails the read (it answers 500 rather than an unwindowed membership):
        // the gateway relays the failure and builds no membership of its own.
        let fx = fx_windows("editor").await;
        mount_membership(
            &fx,
            ResponseTemplate::new(500).set_body_json(json!({"error": "Space membership resolution failed"})),
        )
        .await;
        mount_convex(&fx, "/api/query", ResponseTemplate::new(200).set_body_json(room_list())).await;
        let (status, body) = call(&fx, "GET", &format!("{ROOM}/membership"), None, &[]).await;
        assert_eq!(status, 500, "{body}");
        assert!(body.get("data").is_none(), "{body}");
        let (status, body) = call(&fx, "GET", &format!("{ROOM}/context"), None, &[]).await;
        assert_eq!(status, 500, "{body}");
        assert!(body.get("data").is_none(), "{body}");
    }

    #[tokio::test]
    async fn d18_the_room_context_reports_member_history_windows_from_the_gateway_switch_alone() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        // (history-window switch, member writer switch): the feature mirrors the first and nothing else.
        for (windows, writer) in [(false, false), (true, false), (false, true), (true, true)] {
            let mut fx = fx("owner").await;
            fx.state.space_features.member_history_windows = windows;
            fx.state.space_features.membership_writer_api = writer;
            mount_convex(&fx, "/api/query", ResponseTemplate::new(200).set_body_json(room_list())).await;
            let (status, body) = call(&fx, "GET", &format!("{ROOM}/context"), None, &[]).await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(body["data"]["features"]["member_history_windows"], windows, "windows={windows} writer={writer}");
        }
        // Off by default: a context from a default state reports the feature as false, not absent.
        let fx = fx("owner").await;
        mount_convex(&fx, "/api/query", ResponseTemplate::new(200).set_body_json(room_list())).await;
        let (_, body) = call(&fx, "GET", &format!("{ROOM}/context"), None, &[]).await;
        assert_eq!(body["data"]["features"]["member_history_windows"], false);
    }

    /// The two routes sit behind the session gate like every module route: no session is a 401 and no Control call.
    #[tokio::test]
    async fn d18_the_history_window_routes_need_a_session() {
        let _env = crate::config::TEST_ENV_LOCK.lock().await;
        let fx = fx_windows("owner").await;
        mount_control(&fx, "GET", CONTROL_WINDOW, control_ok(json!({"history_window": {"source": "room", "mode": "full"}}))).await;
        for (method, body) in [("PUT", Body::from(r#"{"mode":"full"}"#)), ("GET", Body::empty())] {
            let response = crate::build_router(fx.state.clone())
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(WINDOW)
                        .header("content-type", "application/json")
                        .body(body)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 401, "{method}");
        }
        assert!(control_calls(&fx).await.is_empty(), "no session, no Control call");
    }
}
