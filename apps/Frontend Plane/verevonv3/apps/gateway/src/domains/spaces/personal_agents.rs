//! WS17-S5: personal agents in product, the gateway side.
//!
//! Routes: `GET`/`POST /api/v1/me/agents`, `PUT /api/v1/me/primary-agent`,
//! `GET`/`PUT /api/v1/agents/{agent_id}/access`, `DELETE /api/v1/agents/{agent_id}/rooms/{space_ref}`.
//! Room joins: [`installable_agent_rows`] (behind `GET /spaces/{ref}/agents/available`) and
//! [`admit_for_bind`] (run before the Convex bind of `POST /spaces/{ref}/agents/bind` and after a
//! room-created agent is made).
//!
//! The gateway holds no state. Definitions, the primary-agent preference and the access projection live
//! in Convex (`convex/spaceAgents.ts`, `convex/users.ts`). Authority (roles, visibility, add policy,
//! admission, revisions) lives in Control (`/api/v1/internal/agents/**`, `/api/v1/internal/spaces/{ref}/
//! agent-*`), called under the caller's own signed delegation with [`proxy_json`]. Control's typed
//! refusals pass through unchanged. The personal routes answer 404 `feature_disabled` while
//! `space_personal_agents_api` is off. The room join is not gated: admission is authority, not a
//! rollout surface.

use std::collections::HashMap;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{delete, get, put},
    Json, Router,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState, contracts::ActionActor, envelope::error, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

use super::{convex_gateway_call, valid_avatar_color};

const MIN_NAME_CHARS: usize = 2;
const MAX_NAME_CHARS: usize = 60;
const MAX_INSTRUCTION_CHARS: usize = 4000;
const MAX_CLIENT_REQUEST_ID_CHARS: usize = 128;
/// Control's bound on one candidates call.
const MAX_CANDIDATES_PER_CALL: usize = 200;

/// The WS17-S5 routes, merged by `module_routes()` in `spaces/mod.rs`.
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/me/agents",
            get(list_my_agents).post(create_personal_agent),
        )
        .route("/api/v1/me/primary-agent", put(set_primary_agent))
        .route(
            "/api/v1/agents/{agent_id}/access",
            get(get_agent_access).put(put_agent_access),
        )
        .route(
            "/api/v1/agents/{agent_id}/rooms/{space_ref}",
            delete(withdraw_agent_from_room),
        )
}

async fn list_my_agents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> (StatusCode, Json<Value>) {
    list_my_agents_core(&state, &user).await
}

async fn create_personal_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    create_personal_agent_core(&state, &user, &body).await
}

async fn set_primary_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    set_primary_agent_core(&state, &user, &body).await
}

async fn get_agent_access(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(agent_id): Path<String>,
) -> (StatusCode, Json<Value>) {
    get_agent_access_core(&state, &user, agent_id.trim()).await
}

async fn put_agent_access(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(agent_id): Path<String>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    put_agent_access_core(&state, &user, agent_id.trim(), &body).await
}

async fn withdraw_agent_from_room(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((agent_id, space_ref)): Path<(String, String)>,
) -> (StatusCode, Json<Value>) {
    withdraw_agent_core(&state, &user, agent_id.trim(), space_ref.trim()).await
}

fn feature_disabled() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(error(
            "feature_disabled",
            "Personal agents are not available in this environment.",
        )),
    )
}

fn no_membership() -> (StatusCode, Json<Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(error(
            "active_membership_required",
            "An active organization membership is required.",
        )),
    )
}

fn not_found() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(error("agent_not_found", "This agent was not found.")),
    )
}

fn registry_unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "agent_registry_unavailable",
            "The agent registry is unavailable. Try again; nothing was changed.",
        )),
    )
}

fn binding_unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(error(
            "agent_binding_unavailable",
            "The agent could not be updated in this room. Try again.",
        )),
    )
}

fn ok_data(data: Value) -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "data": data })))
}

/// The actor the gateway signs for at Control, exactly as the existing room agent routes do.
fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// The Control subject of a definition. The gateway builds it in this one place.
fn subject_of(agent_id: &str) -> String {
    format!("agent-{agent_id}")
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn field(value: &Value, key: &str) -> Value {
    value.get(key).cloned().unwrap_or(Value::Null)
}

/// Registers a definition at Control under the caller's delegation. Control records the verified actor
/// as the owner, so a 201 and a same-owner 200 both mean registered. A 409 means another owner holds the
/// id. The caller decides what each status means.
async fn register_subject(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    subject_id: &str,
    visibility: &str,
    add_policy: &str,
) -> (StatusCode, Value) {
    let url = format!("{}/api/v1/internal/agents/subjects", state.user_core_url);
    let (status, Json(body)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "agent_subject_id": subject_id,
            "visibility": visibility,
            "add_policy": add_policy,
        })),
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    (status, body)
}

/// `POST /api/v1/me/agents`: the three-step create. Convex stores the definition as a draft owned by the
/// caller; Control registers it with the caller as owner (owner-only); Convex activates it. A Control
/// failure leaves an unlisted draft and a retryable 503. The same `client_request_id` replays the same
/// definition (200), so the retry runs the remaining steps.
pub(crate) async fn create_personal_agent_core(
    state: &AppState,
    user: &AuthenticatedUser,
    body: &Value,
) -> (StatusCode, Json<Value>) {
    if !state.space_features.personal_agents_api {
        return feature_disabled();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.is_empty() {
        return no_membership();
    }

    let name = str_field(body, "name").trim().to_owned();
    if !(MIN_NAME_CHARS..=MAX_NAME_CHARS).contains(&name.chars().count()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_agent_name",
                "An agent name must be between 2 and 60 characters.",
            )),
        );
    }
    let instructions = str_field(body, "instructions").trim().to_owned();
    if instructions.chars().count() > MAX_INSTRUCTION_CHARS {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "invalid_agent_instructions",
                "Agent instructions must stay under 4000 characters.",
            )),
        );
    }
    let avatar_color = str_field(body, "avatar_color").trim().to_owned();
    let client_request_id = str_field(body, "client_request_id").trim().to_owned();
    if client_request_id.is_empty() || client_request_id.chars().count() > MAX_CLIENT_REQUEST_ID_CHARS
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(error(
                "client_request_id_required",
                "A client_request_id is required to create an agent.",
            )),
        );
    }

    let mut args = json!({
        "externalAuthId": user.user_id,
        "externalOrgId": org_id,
        "name": name,
        "clientRequestId": client_request_id,
    });
    if !instructions.is_empty() {
        args["instructions"] = json!(instructions);
    }
    if valid_avatar_color(&avatar_color) {
        args["avatarColor"] = json!(avatar_color);
    }

    let Ok(draft) = convex_gateway_call(
        state,
        "mutation",
        "spaceAgents:createPersonalAgentForGateway",
        args,
    )
    .await
    else {
        return registry_unavailable();
    };
    let agent_ref = str_field(&draft, "agentRef").to_owned();
    let subject_id = str_field(&draft, "subjectId").to_owned();
    if agent_ref.is_empty() || subject_id.is_empty() {
        return registry_unavailable();
    }
    let replayed = draft
        .get("replayed")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let (registered, registration) =
        register_subject(state, user, &org_id, &subject_id, "owner", "owner_only").await;
    if !(registered == StatusCode::CREATED || registered == StatusCode::OK) {
        return registry_unavailable();
    }
    let Ok(_) = convex_gateway_call(
        state,
        "mutation",
        "spaceAgents:activateAgentForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "agentId": agent_ref,
        }),
    )
    .await
    else {
        return registry_unavailable();
    };

    let revision = registration
        .pointer("/data/revision")
        .cloned()
        .unwrap_or(json!(1));
    let status = if replayed {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    (
        status,
        Json(json!({ "data": {
            "agent_ref": agent_ref,
            "subject_id": subject_id,
            "visibility": "owner",
            "add_policy": "owner_only",
            "revision": revision,
        } })),
    )
}

/// `GET /api/v1/me/agents`: the caller's own definitions that Control lists, with their rooms and the
/// primary agent. A definition Control does not list (an unregistered draft) is not shown.
pub(crate) async fn list_my_agents_core(
    state: &AppState,
    user: &AuthenticatedUser,
) -> (StatusCode, Json<Value>) {
    if !state.space_features.personal_agents_api {
        return feature_disabled();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.is_empty() {
        return no_membership();
    }
    let Ok(mine) = convex_gateway_call(
        state,
        "query",
        "spaceAgents:listMyAgentsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
        }),
    )
    .await
    else {
        return registry_unavailable();
    };

    let listed_url = format!(
        "{}/api/v1/internal/agents/subjects?mine=1",
        state.user_core_url
    );
    let (listed_status, Json(listed)) = proxy_json(
        state,
        Method::GET,
        &listed_url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !listed_status.is_success() {
        return registry_unavailable();
    }
    let views: Vec<Value> = listed
        .pointer("/data/agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut agents: Vec<Value> = Vec::new();
    for agent in mine
        .get("agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let subject = str_field(&agent, "subjectId");
        let Some(view) = views
            .iter()
            .find(|view| str_field(view, "agent_subject_id") == subject)
        else {
            continue;
        };
        let rooms: Vec<Value> = agent
            .get("rooms")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|room| json!({ "space_ref": field(room, "spaceRef"), "state": field(room, "state") }))
            .collect();
        agents.push(json!({
            "agent_ref": field(&agent, "agentRef"),
            "subject_id": subject,
            "name": field(&agent, "name"),
            "status": field(&agent, "status"),
            "role": view.get("role").cloned().unwrap_or(json!("owner")),
            "visibility": field(view, "visibility"),
            "add_policy": field(view, "add_policy"),
            "revision": field(view, "revision"),
            "rooms": rooms,
        }));
    }

    let primary = str_field(&mine, "primarySubjectId");
    let primary_listed = agents
        .iter()
        .any(|agent| str_field(agent, "subject_id") == primary);
    let primary_subject_id = if primary_listed {
        json!(primary)
    } else {
        Value::Null
    };
    ok_data(json!({
        "agents": agents,
        "primary_subject_id": primary_subject_id,
    }))
}

/// `PUT /api/v1/me/primary-agent`: a preference. Control must show the caller the agent first (a hidden
/// agent answers 404), and Convex stores the subject id. `null` clears it.
pub(crate) async fn set_primary_agent_core(
    state: &AppState,
    user: &AuthenticatedUser,
    body: &Value,
) -> (StatusCode, Json<Value>) {
    if !state.space_features.personal_agents_api {
        return feature_disabled();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.is_empty() {
        return no_membership();
    }
    let subject = match body.get("agent_ref") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) if !value.trim().is_empty() => {
            Some(subject_of(value.trim()))
        }
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(error(
                    "invalid_request",
                    "agent_ref must be an agent id or null.",
                )),
            )
        }
    };
    if let Some(subject) = &subject {
        let url = format!(
            "{}/api/v1/internal/agents/subjects/{}",
            state.user_core_url,
            urlencoding::encode(subject)
        );
        let (status, _) = proxy_json(
            state,
            Method::GET,
            &url,
            None,
            Some(org_id.as_str()),
            Some(&actor_for(user)),
            None,
        )
        .await;
        if status == StatusCode::NOT_FOUND {
            return not_found();
        }
        if !status.is_success() {
            return registry_unavailable();
        }
    }
    let Ok(_) = convex_gateway_call(
        state,
        "mutation",
        "users:setPrimaryAgentForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "agentSubjectId": subject,
        }),
    )
    .await
    else {
        return registry_unavailable();
    };
    ok_data(json!({ "primary_subject_id": subject }))
}

/// `GET /api/v1/agents/{agent_id}/access`: Control decides whether the caller may see the definition and
/// reports the role, visibility, add policy and revision. The access list is read from the Convex
/// projection that the PUT writes, because Control exposes no read of it.
pub(crate) async fn get_agent_access_core(
    state: &AppState,
    user: &AuthenticatedUser,
    agent_id: &str,
) -> (StatusCode, Json<Value>) {
    if !state.space_features.personal_agents_api {
        return feature_disabled();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.is_empty() {
        return no_membership();
    }
    let subject = subject_of(agent_id);
    let url = format!(
        "{}/api/v1/internal/agents/subjects/{}",
        state.user_core_url,
        urlencoding::encode(&subject)
    );
    let (status, Json(view)) = proxy_json(
        state,
        Method::GET,
        &url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(view));
    }
    let data = view.get("data").cloned().unwrap_or(Value::Null);

    let Ok(definition) = convex_gateway_call(
        state,
        "query",
        "spaceAgents:agentDefinitionForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "agentId": agent_id,
        }),
    )
    .await
    else {
        return registry_unavailable();
    };
    if definition.is_null() {
        return not_found();
    }
    let entries: Vec<Value> = definition
        .get("accessEntries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|entry| json!({ "principal_id": field(entry, "principalId"), "role": field(entry, "role") }))
        .collect();

    ok_data(json!({
        "agent_ref": agent_id,
        "subject_id": subject,
        "role": field(&data, "role"),
        "visibility": field(&data, "visibility"),
        "add_policy": field(&data, "add_policy"),
        "revision": field(&data, "revision"),
        "owner_subject_id": field(&data, "owner_user_id"),
        "owner_label": field(&definition, "ownerLabel"),
        "entries": entries,
    }))
}

/// `PUT /api/v1/agents/{agent_id}/access`: Control replaces visibility, add policy and the access list at
/// the revision the editor read (409 `agent_state_changed` with `details.current_revision` on a stale
/// one). Only after Control accepts is the projection written. Control's refusals pass through typed.
pub(crate) async fn put_agent_access_core(
    state: &AppState,
    user: &AuthenticatedUser,
    agent_id: &str,
    body: &Value,
) -> (StatusCode, Json<Value>) {
    if !state.space_features.personal_agents_api {
        return feature_disabled();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.is_empty() {
        return no_membership();
    }
    let entries: Vec<Value> = body
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            json!({
                "principal_id": str_field(entry, "principal_id").trim(),
                "role": str_field(entry, "role").trim(),
            })
        })
        .collect();
    let visibility = str_field(body, "visibility").trim().to_owned();
    let add_policy = str_field(body, "add_policy").trim().to_owned();

    let subject = subject_of(agent_id);
    let url = format!(
        "{}/api/v1/internal/agents/subjects/{}/access",
        state.user_core_url,
        urlencoding::encode(&subject)
    );
    let (status, Json(view)) = proxy_json(
        state,
        Method::PUT,
        &url,
        Some(json!({
            "visibility": visibility,
            "add_policy": add_policy,
            "entries": entries,
            "expected_revision": field(body, "expected_revision"),
        })),
        Some(org_id.as_str()),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(view));
    }
    let data = view.get("data").cloned().unwrap_or(Value::Null);

    let Ok(_) = convex_gateway_call(
        state,
        "mutation",
        "spaceAgents:setAgentVisibilityProjectionForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "agentId": agent_id,
            "visibility": visibility,
            "addPolicy": add_policy,
            "accessEntries": entries
                .iter()
                .map(|entry| json!({ "principalId": field(entry, "principal_id"), "role": field(entry, "role") }))
                .collect::<Vec<Value>>(),
        }),
    )
    .await
    else {
        return registry_unavailable();
    };

    ok_data(json!({
        "agent_ref": agent_id,
        "subject_id": subject,
        "role": field(&data, "role"),
        "visibility": visibility,
        "add_policy": add_policy,
        "revision": field(&data, "revision"),
        "entries": entries,
    }))
}

/// `DELETE /api/v1/agents/{agent_id}/rooms/{space_ref}`: Control withdraws the admission and removes the
/// seat (the owner or a manager of the agent, or of the room, may do it). Then the room's binding is
/// revoked, so the next roster sync cannot seat the agent again. A retry repeats both steps safely.
pub(crate) async fn withdraw_agent_core(
    state: &AppState,
    user: &AuthenticatedUser,
    agent_id: &str,
    space_ref: &str,
) -> (StatusCode, Json<Value>) {
    if !state.space_features.personal_agents_api {
        return feature_disabled();
    }
    let org_id = crate::upstream::authorized_org_id(state, user).await;
    if org_id.is_empty() {
        return no_membership();
    }
    let subject = subject_of(agent_id);
    let url = format!(
        "{}/api/v1/internal/spaces/{}/agent-admissions/{}",
        state.user_core_url,
        urlencoding::encode(space_ref),
        urlencoding::encode(&subject)
    );
    let (status, Json(result)) = proxy_json(
        state,
        Method::DELETE,
        &url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !status.is_success() {
        return (status, Json(result));
    }
    let Ok(_) = convex_gateway_call(
        state,
        "mutation",
        "spaceAgents:withdrawSpaceAgentBindingForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "subjectId": subject,
        }),
    )
    .await
    else {
        return binding_unavailable();
    };
    ok_data(json!({
        "agent_ref": agent_id,
        "space_ref": space_ref,
        "state": "withdrawn",
        "changed": result.pointer("/data/changed").cloned().unwrap_or(json!(false)),
    }))
}

/// Control admission for a room bind (WS17-S5), run before the Convex bind. An org definition Control
/// does not know yet is registered first (org-visible, anyone; the binder is recorded as owner, the
/// WS17-S4 rule), then admitted under Control's `AdmitDenial`. Control's typed refusals pass through
/// unchanged: 403 `agent_not_addable` with `details.reason`, 404 `agent_not_found`, 409, 422.
pub(crate) async fn admit_for_bind(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
    agent_ref: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let Ok(definition) = convex_gateway_call(
        state,
        "query",
        "spaceAgents:agentDefinitionForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "agentId": agent_ref,
        }),
    )
    .await
    else {
        return Err(binding_unavailable());
    };
    if definition.is_null() {
        return Err(not_found());
    }
    let subject = subject_of(agent_ref);
    if str_field(&definition, "visibility") != "owner" {
        // A 409 means another registrant holds the id; admission below decides, so only an outage
        // stops the bind here.
        let (registered, _) =
            register_subject(state, user, org_id, &subject, "org", "anyone").await;
        if !(registered.is_success() || registered == StatusCode::CONFLICT) {
            return Err(registry_unavailable());
        }
    }
    let url = format!(
        "{}/api/v1/internal/spaces/{}/agent-admissions",
        state.user_core_url,
        urlencoding::encode(space_ref)
    );
    let (admitted, Json(answer)) = proxy_json(
        state,
        Method::POST,
        &url,
        Some(json!({
            "agent_subject_id": subject,
            "idempotency_key": "",
        })),
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if admitted.is_success() {
        return Ok(());
    }
    if admitted.is_client_error() {
        return Err((admitted, Json(answer)));
    }
    Err(registry_unavailable())
}

/// The room's installable agents for an owner or manager (WS17-S5). Convex supplies the definitions and
/// the caller's visible personal subjects; Control answers, per subject, whether the caller may see and
/// add each one. A personal agent the caller cannot see is dropped without a trace. A visible one that
/// cannot be added is `blocked` with Control's reason. An org definition Control does not know yet is
/// `addable`: binding registers it and admission decides the rest.
pub(crate) async fn installable_agent_rows(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    space_ref: &str,
) -> Result<Vec<Value>, (StatusCode, Json<Value>)> {
    let unavailable = || {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "installable_agents_unavailable",
                "The organization's agent definitions could not be loaded.",
            )),
        )
    };
    let visible_url = format!(
        "{}/api/v1/internal/agents/subjects?mine=1",
        state.user_core_url
    );
    let (visible_status, Json(visible)) = proxy_json(
        state,
        Method::GET,
        &visible_url,
        None,
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !visible_status.is_success() {
        return Err(unavailable());
    }
    let visible_ids: Vec<String> = visible
        .pointer("/data/agents")
        .and_then(Value::as_array)
        .map(|views| {
            views
                .iter()
                .map(|view| str_field(view, "agent_subject_id").to_owned())
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let Ok(definitions) = convex_gateway_call(
        state,
        "query",
        "spaceAgents:listInstallableSpaceAgentsForGateway",
        json!({
            "externalAuthId": user.user_id,
            "externalOrgId": org_id,
            "spaceRef": space_ref,
            "visibleOwnerAgentIds": visible_ids,
        }),
    )
    .await
    else {
        return Err(unavailable());
    };
    let definitions: Vec<Value> = definitions.as_array().cloned().unwrap_or_default();

    let subjects: Vec<String> = definitions
        .iter()
        .map(|definition| str_field(definition, "subjectId").to_owned())
        .filter(|subject| !subject.is_empty())
        .collect();
    let mut answers: HashMap<String, Value> = HashMap::new();
    for chunk in subjects.chunks(MAX_CANDIDATES_PER_CALL) {
        let url = format!(
            "{}/api/v1/internal/spaces/{}/agent-candidates",
            state.user_core_url,
            urlencoding::encode(space_ref)
        );
        let (status, Json(answer)) = proxy_json(
            state,
            Method::POST,
            &url,
            Some(json!({ "agent_subject_ids": chunk })),
            Some(org_id),
            Some(&actor_for(user)),
            None,
        )
        .await;
        if !status.is_success() {
            return Err((status, Json(answer)));
        }
        for candidate in answer
            .pointer("/data/candidates")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            answers.insert(str_field(&candidate, "agent_subject_id").to_owned(), candidate);
        }
    }

    let mut rows: Vec<Value> = Vec::new();
    for definition in &definitions {
        let subject = str_field(definition, "subjectId").to_owned();
        let owner_only = str_field(definition, "visibility") == "owner";
        let already_bound = definition
            .get("alreadyBound")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer = answers.get(&subject);
        let visible = answer
            .and_then(|candidate| candidate.get("visible"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let can_add = answer
            .and_then(|candidate| candidate.get("can_add"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let reason = answer
            .map(|candidate| str_field(candidate, "reason").to_owned())
            .unwrap_or_default();
        if !visible && owner_only {
            continue;
        }
        let (add_state, reason_out) = if already_bound {
            ("already_bound", Value::Null)
        } else if visible && can_add {
            ("addable", Value::Null)
        } else if visible {
            (
                "blocked",
                if reason.is_empty() {
                    Value::Null
                } else {
                    json!(reason)
                },
            )
        } else {
            ("addable", Value::Null)
        };
        rows.push(json!({
            "agent_ref": field(definition, "agentRef"),
            "name": field(definition, "name"),
            "description": field(definition, "description"),
            "definition_status": field(definition, "definitionStatus"),
            "already_bound": already_bound,
            "add_state": add_state,
            "reason": reason_out,
            "owner_label": field(definition, "ownerLabel"),
        }));
    }
    Ok(rows)
}
