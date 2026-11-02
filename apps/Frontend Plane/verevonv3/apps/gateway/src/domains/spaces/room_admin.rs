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

use std::sync::OnceLock;

use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
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
/// `subject_id`) names the person; `body.role` defaults to Control's own default (editor).
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
    let mut payload = Map::new();
    payload.insert("subject_id".to_owned(), json!(member_id));
    if let Some(role) = role {
        payload.insert("role".to_owned(), json!(role));
    }
    let idempotency_key = key.clone();
    if let Some(key) = key {
        payload.insert("idempotency_key".to_owned(), json!(key));
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
    let out = json!({"data": {
        "member_count": data.get("member_count").and_then(Value::as_u64).unwrap_or(0),
        "changed": data.get("changed").and_then(Value::as_bool).unwrap_or(false),
        "role": data.get("role").cloned().unwrap_or(Value::Null),
        "audience": { "state": audience },
    }});
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
                "Only a named room can be renamed or given a topic.",
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
}
