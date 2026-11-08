//! Org-scoped member status and agent availability chips: the BFF half of WS11-S10.
//!
//! * `GET /api/v1/spaces/status` reads the caller's own status in their organization;
//!   `PUT /api/v1/spaces/status {state, text?, expires_at?}` replaces it. A status is one
//!   Application row keyed by (organization, subject), so a text written in one organization is
//!   never visible in another (the Control profile `metadata.status` is global and is untouched).
//! * [`annotate_roster`] adds `status` to every `user` member of `GET /api/v1/spaces/{ref}/roster`
//!   and [`annotate_agents`] adds `activity` (`working`, `waiting`, `idle`) to every agent of
//!   `GET /api/v1/spaces/{ref}/agents`. Both run only after the route has already proved the
//!   caller's membership of the room (Control answers the roster; a non-member never gets here),
//!   and both degrade by leaving the field out: "could not check" is never drawn as "idle".
//! * `spaces.set_status` rides [`put_status_core`], the same function as the route.
//!
//! Content. The status text is the one free-text field of the attention tables. Setting one is a
//! durable write of stored content, so under Zero Data Retention it is the typed 412 before any
//! Application call; a state-only write (closed vocabulary, optional expiry) carries no content
//! and is allowed. Every read passes `includeText` only when the organization's posture is a
//! successful "not ZDR" (an unknown posture fails closed to no text). The text is never copied
//! into prompts, events or notifications by this module.
//!
//! The flag (`SPACE_STATUS_API`) is a rollout control, never an authorization input: the caller's
//! own row, Control's roster and Application's org-membership check decide what is read or written.
//! Flag off answers 404 `space_status_disabled` on the routes and leaves the roster and agent
//! responses unannotated.
//!
//! Errors are `{error:{code,message,details}}`: 400 `invalid_request` / `invalid_status`, 403
//! `active_membership_required`, 404 `space_status_disabled`, 412 `zdr_durable_mutation_forbidden`
//! (text under Zero Data Retention), 503 `status_unavailable` / `zdr_decision_unavailable`.

use axum::{
    extract::{rejection::JsonRejection, Extension, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::{convex_gateway_call, room_admin::convex_mutation_message};
use crate::{
    config::AppState,
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::authorized_org_id,
    zdr::{org_zdr_lookup, OrgZdrPosture},
};

type Reply = (StatusCode, Json<Value>);

/// The closed state words (the Application `spaceMemberStatus.state` union).
const STATES: [&str; 4] = ["available", "away", "dnd", "offline"];
/// Application answers at most this many subjects per status lookup.
const MAX_STATUS_SUBJECTS: usize = 200;
/// Application answers at most this many agents per activity lookup.
const MAX_ACTIVITY_AGENTS: usize = 100;

/// Registered through `module_routes()` behind `require_session`. A static segment: matchit ranks
/// it above `{space_ref}`, so a room can never shadow it.
pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/spaces/status", get(get_status).put(put_status))
}

fn refuse(status: StatusCode, code: &'static str, message: &'static str) -> Reply {
    (status, Json(error(code, message)))
}

fn disabled() -> Reply {
    refuse(
        StatusCode::NOT_FOUND,
        "space_status_disabled",
        "Space status is not enabled.",
    )
}

fn membership_required() -> Reply {
    refuse(
        StatusCode::FORBIDDEN,
        "active_membership_required",
        "An active organization membership is required to use a status.",
    )
}

fn unavailable() -> Reply {
    refuse(
        StatusCode::SERVICE_UNAVAILABLE,
        "status_unavailable",
        "Status is unavailable right now. Nothing was changed.",
    )
}

fn invalid_request() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "The request body is not valid.",
    )
}

fn invalid_status() -> Reply {
    refuse(
        StatusCode::BAD_REQUEST,
        "invalid_status",
        "state must be available, away, dnd or offline; text at most 80 characters; expiry within 7 days.",
    )
}

/// Flag, then organization. Everything both routes share.
async fn gate(state: &AppState, user: &AuthenticatedUser) -> Result<String, Reply> {
    if !state.space_features.status_api {
        return Err(disabled());
    }
    let org_id = authorized_org_id(state, user).await;
    if org_id.trim().is_empty() {
        return Err(membership_required());
    }
    Ok(org_id)
}

/// Whether text may be returned: only a successful "not ZDR" answer. Unknown fails closed.
async fn text_allowed(state: &AppState, user: &AuthenticatedUser) -> bool {
    org_zdr_lookup(state, user).await == OrgZdrPosture::Disabled
}

/// The wire shape of one status: `{state, text, expires_at}`.
fn status_json(value: &Value) -> Value {
    json!({
        "state": value.get("state").cloned().unwrap_or(Value::Null),
        "text": value.get("text").cloned().unwrap_or(Value::Null),
        "expires_at": value.get("expiresAt").cloned().unwrap_or(Value::Null),
    })
}

// ── the routes ───────────────────────────────────────────────────────────────────────────────────

async fn get_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Reply {
    let org_id = match gate(&state, &user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let include_text = text_allowed(&state, &user).await;
    match convex_gateway_call(
        &state,
        "query",
        "spaceMemberStatus:getOwnStatusForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "includeText": include_text,
        }),
    )
    .await
    {
        // No row, or an expired one, reads as cleared: every field null.
        Ok(value) if value.is_null() => (
            StatusCode::OK,
            Json(json!({"data": {"state": null, "text": null, "expires_at": null}})),
        ),
        Ok(value) => (StatusCode::OK, Json(json!({"data": status_json(&value)}))),
        Err(()) => unavailable(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusBody {
    state: String,
    text: Option<String>,
    /// Epoch milliseconds; Application bounds it to the future and at most 7 days ahead.
    expires_at: Option<i64>,
}

async fn put_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Result<Json<Value>, JsonRejection>,
) -> Reply {
    let Ok(Json(body)) = body else {
        return invalid_request();
    };
    put_status_core(&state, &user, Some(&headers), &body).await
}

/// The status write, shared by the route and the `spaces.set_status` action. Replaces the caller's
/// own row entirely (an absent `text` or `expires_at` clears it). Text is refused typed for a
/// Zero Data Retention organization before any Application call.
pub(crate) async fn put_status_core(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: Option<&HeaderMap>,
    body: &Value,
) -> Reply {
    let org_id = match gate(state, user).await {
        Ok(org_id) => org_id,
        Err(reply) => return reply,
    };
    let Ok(body) = serde_json::from_value::<StatusBody>(body.clone()) else {
        return invalid_request();
    };
    if !STATES.contains(&body.state.as_str()) {
        return invalid_status();
    }
    let has_text = body
        .text
        .as_deref()
        .is_some_and(|text| !text.trim().is_empty());
    if has_text {
        if let Some(refusal) = crate::zdr::durable_write_refusal(state, user, headers, None).await {
            return refusal;
        }
    }
    let mut args = Map::new();
    args.insert("externalAuthId".to_owned(), json!(user.user_id));
    args.insert("externalOrgId".to_owned(), json!(org_id));
    args.insert("state".to_owned(), json!(body.state));
    if let Some(text) = &body.text {
        args.insert("text".to_owned(), json!(text));
    }
    if let Some(expires_at) = body.expires_at {
        args.insert("expiresAt".to_owned(), json!(expires_at));
    }
    match convex_mutation_message(state, "spaceMemberStatus:setStatusForGateway", Value::Object(args))
        .await
    {
        Ok(value) => (StatusCode::OK, Json(json!({"data": status_json(&value)}))),
        Err(Some(message)) if message.contains("invalid_status") => invalid_status(),
        Err(_) => unavailable(),
    }
}

// ── roster and agent annotations ─────────────────────────────────────────────────────────────────

/// Adds `status` (`{state, text, expires_at}`, or `null` when none or expired) to every `user`
/// member of a successful Control roster body. Called after Control has proved the caller's
/// membership of the room. Flag off, a failed lookup or a non-success body leave it untouched.
pub(super) async fn annotate_roster(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    status: StatusCode,
    body: &mut Value,
) {
    if !status.is_success() || !state.space_features.status_api || org_id.trim().is_empty() {
        return;
    }
    let Some(members) = body
        .get_mut("data")
        .and_then(|data| data.get_mut("members"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    let mut subjects: Vec<String> = Vec::new();
    for member in members.iter() {
        if member.get("subject_type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        if let Some(id) = member.get("subject_id").and_then(Value::as_str) {
            if !id.trim().is_empty() && !subjects.iter().any(|seen| seen == id) {
                subjects.push(id.to_owned());
            }
        }
    }
    if subjects.is_empty() {
        return;
    }
    let include_text = text_allowed(state, user).await;
    let mut found: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for chunk in subjects.chunks(MAX_STATUS_SUBJECTS) {
        let Ok(value) = convex_gateway_call(
            state,
            "query",
            "spaceMemberStatus:statusesForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "subjectIds": chunk,
                "includeText": include_text,
            }),
        )
        .await
        else {
            // A failed read leaves `status` out of the response entirely: absent means "not
            // checked", `null` means "checked, none".
            return;
        };
        for row in value.as_array().cloned().unwrap_or_default() {
            if let Some(id) = row.get("externalAuthId").and_then(Value::as_str) {
                found.insert(id.to_owned(), status_json(&row));
            }
        }
    }
    for member in members.iter_mut() {
        if member.get("subject_type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(id) = member.get("subject_id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        if let Some(object) = member.as_object_mut() {
            object.insert(
                "status".to_owned(),
                found.get(&id).cloned().unwrap_or(Value::Null),
            );
        }
    }
}

/// Adds `activity` (`working`, `waiting`, `idle`) to every agent of the room's agent list. A stale
/// or unknown row is `idle` by Application's rule; a failed lookup leaves the field out.
pub(super) async fn annotate_agents(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    agents: &mut [Value],
) {
    if !state.space_features.status_api || org_id.trim().is_empty() || agents.is_empty() {
        return;
    }
    let mut ids: Vec<String> = Vec::new();
    for agent in agents.iter() {
        if let Some(id) = agent.get("subject_id").and_then(Value::as_str) {
            if !id.trim().is_empty() && !ids.iter().any(|seen| seen == id) {
                ids.push(id.to_owned());
            }
        }
    }
    let mut found: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for chunk in ids.chunks(MAX_ACTIVITY_AGENTS) {
        let Ok(value) = convex_gateway_call(
            state,
            "query",
            "spaceMemberStatus:agentActivityForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "spaceRef": space_ref,
                "agentIds": chunk,
            }),
        )
        .await
        else {
            return;
        };
        for row in value.as_array().cloned().unwrap_or_default() {
            if let (Some(id), Some(chip)) = (
                row.get("agentId").and_then(Value::as_str),
                row.get("state").and_then(Value::as_str),
            ) {
                found.insert(id.to_owned(), chip.to_owned());
            }
        }
    }
    for agent in agents.iter_mut() {
        let Some(id) = agent.get("subject_id").and_then(Value::as_str).map(str::to_owned) else {
            continue;
        };
        if let Some(object) = agent.as_object_mut() {
            let chip = found.get(&id).map_or("idle", String::as_str);
            object.insert("activity".to_owned(), json!(chip));
        }
    }
}
