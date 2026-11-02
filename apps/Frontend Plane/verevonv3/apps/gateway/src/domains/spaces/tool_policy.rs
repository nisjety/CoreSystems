//! WS10-S5b: the per-tool policy editor routes.
//!
//! * `GET /api/v1/spaces/{space_ref}/tool-policy[?agent_subject_id=]`: Control's stored Space
//!   ceiling, every agent layer and (with `agent_subject_id`) the merged view for that agent,
//!   which names the level AND the layer that decided it per tool. Any member who can read the
//!   Space may read it; Control decides.
//! * `PUT /api/v1/spaces/{space_ref}/tool-policy` body `{entries, expected_revision}`: replaces the
//!   Space ceiling (every agent of the room).
//! * `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/tool-policy` same body: replaces one
//!   agent's layer. The binding is resolved to its Control subject id from Application's binding
//!   projection (the Space and organization must match and the binding must be active or paused);
//!   Control then refuses an agent that is not an active member of the Space.
//!
//! Control is the store and the only authority: it resolves the caller's live role from the
//! gateway's signed delegation (a manager or owner writes, an archived room is read-only, a
//! personal Space has no policy) and compares `expected_revision` per layer (409
//! `tool_policy_revision_conflict`). The body here carries no actor, organization, role or Space;
//! unknown fields are refused. A policy only tightens (allow < ask < block), so a write needs no
//! entitlement and Zero Data Retention organizations behave the same: keys and levels, never
//! content.
//!
//! After an agent-layer write Control has already committed. The gateway then records the new
//! layer revision and the merged digest on the binding in Application
//! (`spaceAgents:setSpaceAgentBindingPolicyForGateway`), the projection the editor and the legacy
//! filter read. That sync never rolls Control back: a failure answers `projection: "pending"` and
//! the next successful write or read of the merged view heals it. The legacy `approvalMode` and
//! `allowedTools` are deliberately NOT derived from the policy (`blocked` or a stripped `agentic`
//! feature would silence the agent, not tighten one tool).
//!
//! Behind `SpaceFeatureFlags::tool_policy_editor` (off answers 404 `feature_disabled`), a rollout
//! control and never an authorization input.

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use super::room_admin::convex_mutation_message;
use crate::{
    config::AppState,
    contracts::ActionActor,
    envelope::{error, error_with_details, unwrap_data},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

type Reply = (StatusCode, Json<Value>);

/// Control's own bounds (`MaxToolPolicyEntries`, `maxApprovalKeyLen`); Control validates fully.
const MAX_ENTRIES: usize = 200;
const MAX_TOOL_KEY_CHARS: usize = 200;
const MAX_SUBJECT_ID_CHARS: usize = 200;
const MAX_RELAYED_MESSAGE_CHARS: usize = 300;
const ALLOWED_FIELDS: [&str; 2] = ["entries", "expected_revision"];

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/spaces/{space_ref}/tool-policy",
            get(get_tool_policy_route).put(put_space_tool_policy_route),
        )
        .route(
            "/api/v1/spaces/{space_ref}/agents/{binding_ref}/tool-policy",
            put(put_agent_tool_policy_route),
        )
}

fn feature_disabled() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "The tool policy editor is not enabled.",
        )),
    )
}

fn unavailable() -> Reply {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "tool_policy_unavailable",
            "The tool policy is unavailable. Nothing was changed.",
        )),
    )
}

fn invalid(reason: &'static str) -> Reply {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            "tool_policy_invalid",
            "The tool policy is not valid.",
            json!({ "reason": reason }),
        )),
    )
}

fn actor_of(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

fn control_url(state: &AppState, space_ref: &str, tail: &str) -> String {
    format!(
        "{}/api/v1/internal/spaces/{}/{}",
        state.user_core_url,
        urlencoding::encode(space_ref),
        tail
    )
}

/// The organization and trimmed Space ref, or the typed refusal every Space route gives.
async fn scope_of(
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

/// A well-formed lowercase snake_case code, as Control's typed errors use.
fn is_error_code(code: &str) -> bool {
    !code.is_empty()
        && code.len() <= 64
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// Control's failure as the browser envelope: a typed body keeps its status, code and bounded
/// scalar details; anything else (and a typed 401, which the SPA would read as an expired session)
/// is the 503 that says nothing changed.
fn relay_control_error(status: StatusCode, body: &Value) -> Reply {
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
                    .take(16)
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                out.insert("details".to_owned(), Value::Object(scalars));
            }
            Some(Value::Object(out))
        });
    match typed {
        Some(error) if status != StatusCode::UNAUTHORIZED => {
            (status, Json(json!({ "error": error })))
        }
        _ => unavailable(),
    }
}

/// The validated, normalized write: only `entries` and `expected_revision` ever reach Control.
fn parse_update(body: &Value) -> Result<Value, Reply> {
    let Some(object) = body.as_object() else {
        return Err(invalid("body_not_an_object"));
    };
    if let Some(field) = object.keys().find(|key| !ALLOWED_FIELDS.contains(&key.as_str())) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error_with_details(
                "unsupported_policy_field",
                "Only entries and expected_revision can be set here.",
                json!({"field": field.chars().take(64).collect::<String>()}),
            )),
        ));
    }
    let Some(entries) = object.get("entries").and_then(Value::as_array) else {
        return Err(invalid("entries_required"));
    };
    if entries.len() > MAX_ENTRIES {
        return Err(invalid("too_many_entries"));
    }
    let mut normalized = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(entry) = entry.as_object() else {
            return Err(invalid("bad_entry"));
        };
        let key = entry.get("tool_key").and_then(Value::as_str);
        let level = entry.get("level").and_then(Value::as_str);
        match (key, level) {
            (Some(key), Some(level @ ("allow" | "ask" | "block")))
                if !key.is_empty() && key.chars().count() <= MAX_TOOL_KEY_CHARS =>
            {
                normalized.push(json!({"tool_key": key, "level": level}));
            }
            _ => return Err(invalid("bad_entry")),
        }
    }
    let expected_revision = object
        .get("expected_revision")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).map(|f| f as u64))
        })
        .ok_or_else(|| invalid("expected_revision_required"))?;
    Ok(json!({"entries": normalized, "expected_revision": expected_revision}))
}

#[derive(Deserialize)]
pub(super) struct PolicyQuery {
    agent_subject_id: Option<String>,
}

pub(super) async fn get_tool_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Query(query): Query<PolicyQuery>,
) -> Reply {
    get_tool_policy(&state, &user, &space_ref, query.agent_subject_id.as_deref()).await
}

pub(super) async fn put_space_tool_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(space_ref): Path<String>,
    Json(body): Json<Value>,
) -> Reply {
    put_space_tool_policy(&state, &user, &space_ref, &body).await
}

pub(super) async fn put_agent_tool_policy_route(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((space_ref, binding_ref)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Reply {
    put_agent_tool_policy(&state, &user, &space_ref, &binding_ref, &body).await
}

/// Control's read, relayed: `{space_ref, ceiling, agents, merged}`.
pub(crate) async fn get_tool_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    agent_subject_id: Option<&str>,
) -> Reply {
    if !state.space_features.tool_policy_editor {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let mut url = control_url(state, &space_ref, "tool-policy");
    if let Some(agent) = agent_subject_id.map(str::trim).filter(|agent| !agent.is_empty()) {
        if agent.chars().count() > MAX_SUBJECT_ID_CHARS {
            return invalid("agent_subject_id_oversized");
        }
        url.push_str("?agent_subject_id=");
        url.push_str(&urlencoding::encode(agent));
    }
    let (status, Json(control)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(&org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return relay_control_error(status, &control);
    }
    (StatusCode::OK, Json(json!({ "data": unwrap_data(&control) })))
}

async fn put_control_layer(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    url: &str,
    payload: Value,
) -> Result<Value, Reply> {
    let (status, Json(control)) = proxy_json(
        state,
        Method::PUT,
        url,
        Some(payload),
        Some(org_id),
        Some(&actor_of(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return Err(relay_control_error(status, &control));
    }
    Ok(unwrap_data(&control).clone())
}

/// Replaces the Space ceiling on Control. Application keeps no ceiling, so nothing is projected.
pub(crate) async fn put_space_tool_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.tool_policy_editor {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let payload = match parse_update(body) {
        Ok(payload) => payload,
        Err(reply) => return reply,
    };
    let url = control_url(state, &space_ref, "tool-policy");
    match put_control_layer(state, user, &org_id, &url, payload).await {
        Ok(data) => (StatusCode::OK, Json(json!({ "data": data }))),
        Err(reply) => reply,
    }
}

/// The agent's Control subject id for a binding of THIS Space and organization that is still
/// bound (active or paused), from Application's binding projection.
async fn binding_subject_id(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    binding_ref: &str,
) -> Result<String, Reply> {
    let value = super::convex_gateway_call(
        state,
        "query",
        "spaceAgents:spaceAgentBindingsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
        }),
    )
    .await
    .map_err(|()| unavailable())?;
    value
        .as_array()
        .into_iter()
        .flatten()
        .find(|binding| {
            binding.get("bindingRef").and_then(Value::as_str) == Some(binding_ref)
                && matches!(
                    binding.get("status").and_then(Value::as_str),
                    Some("active" | "paused")
                )
        })
        .and_then(|binding| binding.get("subjectId").and_then(Value::as_str))
        .filter(|subject| !subject.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(error(
                    "agent_binding_not_found",
                    "That agent is not bound to this room.",
                )),
            )
        })
}

/// Replaces one agent's layer on Control, then projects the new revision and the merged digest
/// onto the binding. The Control write stands whatever the projection does.
pub(crate) async fn put_agent_tool_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    space_ref: &str,
    binding_ref: &str,
    body: &Value,
) -> Reply {
    if !state.space_features.tool_policy_editor {
        return feature_disabled();
    }
    let (org_id, space_ref) = match scope_of(state, user, space_ref).await {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let binding_ref = binding_ref.trim();
    if binding_ref.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "binding_ref_required",
                "A binding reference is required.",
            )),
        );
    }
    let payload = match parse_update(body) {
        Ok(payload) => payload,
        Err(reply) => return reply,
    };
    let subject_id = match binding_subject_id(state, user, &org_id, &space_ref, binding_ref).await {
        Ok(subject) => subject,
        Err(reply) => return reply,
    };
    let url = control_url(
        state,
        &space_ref,
        &format!("agents/{}/tool-policy", urlencoding::encode(&subject_id)),
    );
    let data = match put_control_layer(state, user, &org_id, &url, payload).await {
        Ok(data) => data,
        Err(reply) => return reply,
    };
    let revision = data
        .pointer("/policy/revision")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let digest = data
        .pointer("/merged/digest")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let projection = if digest.is_empty() {
        "pending"
    } else {
        match convex_mutation_message(
            state,
            "spaceAgents:setSpaceAgentBindingPolicyForGateway",
            json!({
                "externalAuthId": user.user_id,
                "externalOrgId": org_id,
                "spaceRef": space_ref,
                "bindingRef": binding_ref,
                "toolPolicyRevision": revision,
                "toolPolicyDigest": digest,
            }),
        )
        .await
        {
            Ok(_) => "synced",
            Err(_) => "pending",
        }
    };
    let mut out = data;
    if let Some(object) = out.as_object_mut() {
        object.insert("binding_ref".to_owned(), json!(binding_ref));
        object.insert("projection".to_owned(), json!(projection));
    }
    (StatusCode::OK, Json(json!({ "data": out })))
}
