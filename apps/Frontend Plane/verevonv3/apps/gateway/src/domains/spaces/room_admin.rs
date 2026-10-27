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

use axum::{
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
    upstream::proxy_json,
};

type Reply = (StatusCode, Json<Value>);

const ROOM_ROLES: [&str; 4] = ["viewer", "editor", "manager", "owner"];
const MAX_IDEMPOTENCY_KEY_LEN: usize = 128;

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
    if let Some(key) = key {
        payload.insert("idempotency_key".to_owned(), json!(key));
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
        return (status, Json(control));
    }
    let data = unwrap_data(&control);
    let audience = reconcile_audience(state, &org_id, &space_ref).await;
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
        return (status, Json(control));
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
        return (status, Json(control));
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
async fn convex_mutation_message(
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
