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
    upstream::proxy_json, zdr::durable_write_refusal,
};

use super::{convex_gateway_call, valid_avatar_color};

const MIN_NAME_CHARS: usize = 2;
const MAX_NAME_CHARS: usize = 60;
const MAX_INSTRUCTION_CHARS: usize = 4000;
const MAX_CLIENT_REQUEST_ID_CHARS: usize = 128;
/// Control's bound on one candidates call.
const MAX_CANDIDATES_PER_CALL: usize = 200;
/// The surface a ZDR refusal names (the catalogue code is feature-less).
const PERSONAL_AGENTS_SURFACE: &str = "personal_agents";
/// Control's typed 403 on an organization-agent registration: the caller is not an active organization
/// member (`CodeAgentAccessForbidden` in Control's `agent_registry.go`).
const AGENT_ACCESS_FORBIDDEN: &str = "agent_access_forbidden";
/// Control's typed 422 on an organization-agent registration: the subject id or the visibility and add policy pair is not valid.
const AGENT_POLICY_INVALID: &str = "agent_policy_invalid";
/// Control's cap on one agent's access list (`maxAgentAccessEntries`).
const MAX_AGENT_ACCESS_ENTRIES: usize = 200;

/// D19/D22as, presentation only: whether an org owner or admin may edit this agent's access although
/// they are neither its owner nor a listed manager. It mirrors Control's `AgentAccessEditAuthority`
/// (ownerless, org-visible, canonical org owner or admin); Control decides again on every write.
fn org_admin_editable(user: &AuthenticatedUser, view: &Value) -> bool {
    str_field(view, "owner_user_id").trim().is_empty()
        && str_field(view, "visibility") == "org"
        && crate::middleware::has_authorized_org_role(user, &["owner", "admin"])
}

/// D24 mirrored into the display projection: an owner-to-org flip keeps the actor on the access list as a
/// manager unless they listed themselves, exactly as Control's `UpdateAgentAccess` stores it. Control's
/// answer carries no list, so without this the projection (and the next editor's read) would silently drop
/// them, and a save from that read would really remove them.
fn entries_as_stored(prior: &Value, to_visibility: &str, actor_id: &str, mut entries: Vec<Value>) -> Vec<Value> {
    let clears_owner = !str_field(prior, "owner_user_id").trim().is_empty()
        && str_field(prior, "visibility") == "owner"
        && to_visibility == "org";
    let listed = entries.iter().any(|entry| str_field(entry, "principal_id") == actor_id);
    if clears_owner && !listed && entries.len() < MAX_AGENT_ACCESS_ENTRIES && !actor_id.trim().is_empty() {
        entries.push(json!({ "principal_id": actor_id, "role": "manager" }));
    }
    entries
}

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

/// The typed ZDR refusal, labelled with this surface (the catalogue code is feature-less).
fn label_zdr(status: StatusCode, mut body: Value) -> (StatusCode, Json<Value>) {
    if status == StatusCode::PRECONDITION_FAILED {
        if let Some(object) = body.get_mut("error").and_then(Value::as_object_mut) {
            object.insert("details".to_owned(), json!({ "surface": PERSONAL_AGENTS_SURFACE }));
        }
    }
    (status, Json(body))
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
/// definition (200), so the retry runs the remaining steps. The name and the instructions are stored
/// content: a Zero Data Retention org is refused (typed 412) after the flag and membership, before any
/// Control or Convex call.
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
    // Zero Data Retention, before any Control or Convex call. A failed org posture lookup is the typed 503.
    if let Some((status, Json(refusal))) =
        durable_write_refusal(state, user, None, Some(body)).await
    {
        return label_zdr(status, refusal);
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
        // D22as: an owner-to-org flip clears Control's owner; Convex keeps its creator, so its label is
        // shown only while Control still names an owner.
        "owner_label": if str_field(&data, "owner_user_id").trim().is_empty() {
            Value::Null
        } else {
            field(&definition, "ownerLabel")
        },
        "org_admin_editable": org_admin_editable(user, &data),
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
    // The state the edit starts from, so the projection can store the list as Control will (D24).
    let prior_url = format!(
        "{}/api/v1/internal/agents/subjects/{}",
        state.user_core_url,
        urlencoding::encode(&subject)
    );
    let (prior_status, Json(prior)) = proxy_json(
        state,
        Method::GET,
        &prior_url,
        None,
        Some(org_id.as_str()),
        Some(&actor_for(user)),
        None,
    )
    .await;
    if !prior_status.is_success() {
        return (prior_status, Json(prior));
    }
    let prior = prior.get("data").cloned().unwrap_or(Value::Null);
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
    let entries = entries_as_stored(&prior, &visibility, &user.user_id, entries);

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
/// does not know yet is registered first (org-visible, anyone; the binder is recorded as the registrant,
/// not an owner, because Control keeps an org definition ownerless), then admitted under Control's
/// `AdmitDenial`. Control's typed refusals pass through unchanged: on the registration, 403
/// `agent_access_forbidden` (the caller is not an active organization member) and 422 `agent_policy_invalid`
/// (the subject id or the visibility and add policy pair is not valid); on the admission, 403 `agent_not_addable` with
/// `details.reason`, 404 `agent_not_found`, 409 and 422. Any other registration failure is the 503, and so is an
/// admission outage.
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
        // stops the bind here. Control's typed 403 `agent_access_forbidden` and 422 `agent_policy_invalid`
        // are relayed unchanged; any other refusal is the retryable 503.
        let (registered, registration) =
            register_subject(state, user, org_id, &subject, "org", "anyone").await;
        let typed = registration.pointer("/error/code").and_then(Value::as_str);
        if (registered == StatusCode::FORBIDDEN && typed == Some(AGENT_ACCESS_FORBIDDEN))
            || (registered == StatusCode::UNPROCESSABLE_ENTITY && typed == Some(AGENT_POLICY_INVALID))
        {
            return Err((registered, Json(registration)));
        }
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

#[cfg(test)]
mod tests {
    //! WS17-S5 and W3-FIX-18 (the create guard), W3-DEC-GWFIX and W3-DEC-GWFIX2 (the registration relay of
    //! `admit_for_bind`). The create route runs through the real router, so the flag, the membership, the ZDR
    //! guard and its typed labels are checked on the wire. The helpers are called directly. Session, Control
    //! (user-core), org-core and Convex are doubles, and every "no call" is checked by path or by the Convex
    //! operation named in the request body.
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use serde_json::{json, Value};
    use tower::ServiceExt;
    use wiremock::matchers::{body_partial_json, method as wm_method, path as wm_path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::middleware::AuthorizedMembership;

    const ORG: &str = "org-1";
    const SPACE: &str = "space-room";
    const ORG_PATH: &str = "/api/v1/organizations/org-1";
    const SUBJECTS: &str = "/api/v1/internal/agents/subjects";
    const ADMISSIONS: &str = "/api/v1/internal/spaces/space-room/agent-admissions";
    const CREATE: &str = "spaceAgents:createPersonalAgentForGateway";
    const ACTIVATE: &str = "spaceAgents:activateAgentForGateway";
    const DEFINITION: &str = "spaceAgents:agentDefinitionForGateway";

    struct Fixture {
        state: AppState,
        auth: MockServer,
        user_core: MockServer,
        org_core: MockServer,
        convex: MockServer,
    }

    async fn fixture() -> Fixture {
        let auth = MockServer::start().await;
        let user_core = MockServer::start().await;
        let org_core = MockServer::start().await;
        let convex = MockServer::start().await;
        let mut state = crate::tests::test_state(false);
        state.auth_core_url = auth.uri();
        state.user_core_url = user_core.uri();
        state.org_core_url = org_core.uri();
        state.application_convex_url = convex.uri();
        state.application_convex_service_key = "application-test-key".into();
        state.space_features.personal_agents_api = true;
        Fixture {
            state,
            auth,
            user_core,
            org_core,
            convex,
        }
    }

    fn user() -> AuthenticatedUser {
        AuthenticatedUser {
            user_id: "user-1".to_owned(),
            user_email: "user@example.test".to_owned(),
            user_name: "User One".to_owned(),
            user_image: None,
            email_verified: true,
            auth_role: Some("member".to_owned()),
            active_org_id: Some(ORG.to_owned()),
            authorized_membership: Some(AuthorizedMembership {
                organization_id: ORG.to_owned(),
                role: "member".to_owned(),
            }),
        }
    }

    async fn answer(server: &MockServer, verb: &str, route: &str, response: ResponseTemplate) {
        Mock::given(wm_method(verb))
            .and(wm_path(route))
            .respond_with(response)
            .mount(server)
            .await;
    }

    /// A Convex answer for one named operation, whichever endpoint (`query` or `mutation`) carries it.
    async fn convex(f: &Fixture, operation: &str, response: ResponseTemplate) {
        Mock::given(wm_method("POST"))
            .and(body_partial_json(json!({ "path": operation })))
            .respond_with(response)
            .mount(&f.convex)
            .await;
    }

    fn convex_value(value: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({ "value": value }))
    }

    /// Control's 200 envelope.
    fn control(data: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({ "data": data }))
    }

    fn org_record(zdr: bool) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_json(json!({"data": {"metadata": {"interactiveRetention": {"zdr": zdr}}}}))
    }

    /// The session the router resolves for the caller: org-1, role member.
    async fn session(f: &Fixture) {
        answer(
            &f.auth,
            "GET",
            "/api/auth/get-session",
            ResponseTemplate::new(200).set_body_json(json!({
                "user": {"id": "user-1", "email": "user@example.com", "emailVerified": true},
                "session": {"activeOrganizationId": ORG}
            })),
        )
        .await;
        for slug in ["session-core", "capability-core"] {
            answer(
                &f.auth,
                "GET",
                &format!("/api/{slug}/token"),
                ResponseTemplate::new(200).set_body_json(json!({"token": "plane-token"})),
            )
            .await;
        }
        answer(
            &f.user_core,
            "GET",
            "/api/v1/me/session-context",
            ResponseTemplate::new(200).set_body_json(json!({
                "userId": "user-1", "orgId": ORG, "role": "member", "onboardingStatus": "COMPLETED"
            })),
        )
        .await;
    }

    /// One request through the real router with the session cookie: the status and the JSON body.
    async fn call(state: AppState, method: &str, uri: &str, body: Option<Value>) -> (u16, Value) {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("cookie", "better-auth.session_token=session-1");
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
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("a body")
            .to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn calls_to(server: &MockServer, part: &str) -> Vec<wiremock::Request> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.url.path().contains(part))
            .collect()
    }

    /// Every Convex operation a server received, in arrival order, by the `path` in each body.
    async fn convex_ops(server: &MockServer) -> Vec<String> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter_map(|body| body.get("path").and_then(Value::as_str).map(str::to_owned))
            .collect()
    }

    /// The `args` of every call to one Convex operation.
    async fn convex_args(server: &MockServer, operation: &str) -> Vec<Value> {
        server
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|request| serde_json::from_slice::<Value>(&request.body).ok())
            .filter(|body| body.get("path").and_then(Value::as_str) == Some(operation))
            .map(|body| body.get("args").cloned().unwrap_or(Value::Null))
            .collect()
    }

    fn create_body() -> Value {
        json!({"name": "Helper", "instructions": "Be brief.", "client_request_id": "req-1"})
    }

    /// The organisation definition Convex holds for `agent-1` at the given visibility.
    async fn definition(f: &Fixture, visibility: &str) {
        convex(
            f,
            DEFINITION,
            convex_value(json!({
                "agentRef": "agent-1",
                "subjectId": "agent-agent-1",
                "visibility": visibility,
                "name": "Helper"
            })),
        )
        .await;
    }

    async fn admit(f: &Fixture) -> Result<(), (StatusCode, Json<Value>)> {
        admit_for_bind(&f.state, &user(), ORG, SPACE, "agent-1").await
    }

    /// `admit_for_bind` for an organisation definition whose registration answers `registration`. Returns the
    /// refusal and how many admission calls Control received.
    async fn bind_with_registration(registration: ResponseTemplate) -> (StatusCode, Value, usize) {
        let f = fixture().await;
        definition(&f, "org").await;
        answer(&f.user_core, "POST", SUBJECTS, registration).await;
        answer(&f.user_core, "POST", ADMISSIONS, control(json!({}))).await;
        let (status, Json(body)) = admit(&f)
            .await
            .expect_err("a refused registration stops the bind");
        let admissions = calls_to(&f.user_core, "agent-admissions").await.len();
        (status, body, admissions)
    }

    // ---- W3-FIX-18: the ZDR guard on the create route -------------------------------------------

    /// A ZDR org is refused with the typed 412, labelled with this surface, before any Control or Convex call.
    #[tokio::test]
    async fn a_zdr_org_is_refused_on_create_with_no_control_or_convex_call() {
        let f = fixture().await;
        session(&f).await;
        answer(&f.org_core, "GET", ORG_PATH, org_record(true)).await;
        convex(&f, CREATE, convex_value(json!({"agentRef": "agent-x", "subjectId": "agent-agent-x"}))).await;
        answer(&f.user_core, "POST", SUBJECTS, control(json!({"revision": 1}))).await;
        let (status, body) = call(f.state.clone(), "POST", "/api/v1/me/agents", Some(create_body())).await;
        assert_eq!(status, 412, "{body}");
        assert_eq!(body["error"]["code"], "zdr_durable_mutation_forbidden");
        assert_eq!(body["error"]["details"]["surface"], "personal_agents");
        assert!(convex_ops(&f.convex).await.is_empty(), "no Convex request");
        assert!(calls_to(&f.user_core, "/api/v1/internal/").await.is_empty(), "no Control request");
    }

    /// A failed org posture read is the typed, retryable 503, and still no Control or Convex call.
    #[tokio::test]
    async fn an_org_posture_outage_is_503_zdr_decision_unavailable_on_create() {
        let f = fixture().await;
        session(&f).await;
        answer(
            &f.org_core,
            "GET",
            ORG_PATH,
            ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal"}})),
        )
        .await;
        convex(&f, CREATE, convex_value(json!({"agentRef": "agent-x", "subjectId": "agent-agent-x"}))).await;
        let (status, body) = call(f.state.clone(), "POST", "/api/v1/me/agents", Some(create_body())).await;
        assert_eq!(status, 503, "{body}");
        assert_eq!(body["error"]["code"], "zdr_decision_unavailable");
        assert!(convex_ops(&f.convex).await.is_empty(), "no Convex request");
        assert!(calls_to(&f.user_core, "/api/v1/internal/").await.is_empty(), "no Control request");
    }

    /// A body `zdr: true` is refused before the organisation posture is read.
    #[tokio::test]
    async fn a_body_zdr_true_is_refused_on_create_without_an_org_core_lookup() {
        let f = fixture().await;
        session(&f).await;
        answer(&f.org_core, "GET", ORG_PATH, org_record(false)).await;
        let mut body = create_body();
        body["zdr"] = json!(true);
        let (status, reply) = call(f.state.clone(), "POST", "/api/v1/me/agents", Some(body)).await;
        assert_eq!(status, 412, "{reply}");
        assert_eq!(reply["error"]["code"], "zdr_durable_mutation_forbidden");
        assert!(calls_to(&f.org_core, ORG_PATH).await.is_empty(), "the body flag decides first");
        assert!(convex_ops(&f.convex).await.is_empty(), "no Convex request");
    }

    /// A non-ZDR org passes the guard and reaches the Convex draft create, which is the first write.
    #[tokio::test]
    async fn a_non_zdr_org_still_reaches_the_convex_draft_create() {
        let f = fixture().await;
        session(&f).await;
        answer(&f.org_core, "GET", ORG_PATH, org_record(false)).await;
        convex(
            &f,
            CREATE,
            ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal"}})),
        )
        .await;
        let (status, reply) = call(f.state.clone(), "POST", "/api/v1/me/agents", Some(create_body())).await;
        assert_eq!(status, 503, "{reply}");
        assert_eq!(reply["error"]["code"], "agent_registry_unavailable");
        assert_eq!(convex_ops(&f.convex).await, vec![CREATE.to_owned()]);
        assert!(calls_to(&f.user_core, "/api/v1/internal/").await.is_empty(), "the draft failed first");
    }

    // ---- WS17-S5: the create route --------------------------------------------------------------

    /// A retry with the same client request id replays the draft (200) and runs the remaining steps again.
    #[tokio::test]
    async fn create_retry_replays_same_definition() {
        let f = fixture().await;
        session(&f).await;
        answer(&f.org_core, "GET", ORG_PATH, org_record(false)).await;
        convex(
            &f,
            CREATE,
            convex_value(json!({
                "agentRef": "agent-ref-1",
                "subjectId": "agent-agent-ref-1",
                "replayed": true
            })),
        )
        .await;
        answer(&f.user_core, "POST", SUBJECTS, control(json!({"revision": 3}))).await;
        convex(&f, ACTIVATE, convex_value(Value::Null)).await;
        let (status, reply) = call(f.state.clone(), "POST", "/api/v1/me/agents", Some(create_body())).await;
        assert_eq!(status, 200, "{reply}");
        assert_eq!(reply["data"]["agent_ref"], "agent-ref-1");
        assert_eq!(reply["data"]["subject_id"], "agent-agent-ref-1");
        assert_eq!(reply["data"]["revision"], 3);
        assert_eq!(
            convex_ops(&f.convex).await,
            vec![CREATE.to_owned(), ACTIVATE.to_owned()],
            "a replay runs the registration and the activation again"
        );
        let created = convex_args(&f.convex, CREATE).await;
        assert_eq!(created[0]["clientRequestId"], "req-1");
        assert_eq!(calls_to(&f.user_core, SUBJECTS).await.len(), 1);
    }

    /// A Control failure after the draft write answers a retryable 503 and never activates the draft.
    #[tokio::test]
    async fn create_control_failure_leaves_unlisted_draft_503() {
        let f = fixture().await;
        session(&f).await;
        answer(&f.org_core, "GET", ORG_PATH, org_record(false)).await;
        convex(
            &f,
            CREATE,
            convex_value(json!({
                "agentRef": "agent-ref-2",
                "subjectId": "agent-agent-ref-2",
                "replayed": false
            })),
        )
        .await;
        answer(
            &f.user_core,
            "POST",
            SUBJECTS,
            ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal"}})),
        )
        .await;
        convex(&f, ACTIVATE, convex_value(Value::Null)).await;
        let (status, reply) = call(f.state.clone(), "POST", "/api/v1/me/agents", Some(create_body())).await;
        assert_eq!(status, 503, "{reply}");
        assert_eq!(reply["error"]["code"], "agent_registry_unavailable");
        assert_eq!(
            convex_ops(&f.convex).await,
            vec![CREATE.to_owned()],
            "the draft is written, the activation never is"
        );
    }

    // ---- WS17-S5 and W3-DEC-GWFIX / W3-DEC-GWFIX2: admission on the bind -------------------------

    /// Control's typed 403 `agent_access_forbidden` on the registration is relayed unchanged; admission is never asked.
    #[tokio::test]
    async fn a_registration_403_agent_access_forbidden_is_relayed_unchanged_with_no_admission() {
        let refusal = json!({"error": {
            "code": "agent_access_forbidden",
            "message": "Only active members may register an agent."
        }});
        let (status, body, admissions) =
            bind_with_registration(ResponseTemplate::new(403).set_body_json(refusal.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, refusal);
        assert_eq!(admissions, 0, "no admission call");
    }

    /// Control's typed 422 `agent_policy_invalid` on the registration is relayed unchanged; admission is never asked.
    #[tokio::test]
    async fn a_registration_422_agent_policy_invalid_is_relayed_unchanged_with_no_admission() {
        let refusal = json!({"error": {
            "code": "agent_policy_invalid",
            "message": "That access choice is not valid for this agent."
        }});
        let (status, body, admissions) =
            bind_with_registration(ResponseTemplate::new(422).set_body_json(refusal.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body, refusal);
        assert_eq!(admissions, 0, "no admission call");
    }

    /// A Control outage on the registration is the retryable 503, and admission is never asked.
    #[tokio::test]
    async fn a_registration_500_stays_503_agent_registry_unavailable_with_no_admission() {
        let (status, body, admissions) = bind_with_registration(
            ResponseTemplate::new(500).set_body_json(json!({"error": {"code": "internal"}})),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "agent_registry_unavailable");
        assert_eq!(admissions, 0, "no admission call");
    }

    /// A registration 403 with any other code is not the typed relay: it is the retryable 503.
    #[tokio::test]
    async fn a_registration_403_with_another_code_stays_503() {
        let (status, body, admissions) = bind_with_registration(
            ResponseTemplate::new(403).set_body_json(json!({"error": {"code": "forbidden"}})),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "agent_registry_unavailable");
        assert_eq!(admissions, 0, "no admission call");
    }

    /// A registration 422 with any other code is not the typed relay: it is the retryable 503.
    #[tokio::test]
    async fn a_registration_422_with_another_code_stays_503() {
        let (status, body, admissions) = bind_with_registration(
            ResponseTemplate::new(422).set_body_json(json!({"error": {"code": "validation_failed"}})),
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["error"]["code"], "agent_registry_unavailable");
        assert_eq!(admissions, 0, "no admission call");
    }

    /// `admit_for_bind` asks Control to admit the agent once, after reading the definition, and answers Ok only on
    /// admission. The bind route calls it before its Convex bind mutation and stops on any refusal. The mutation
    /// ordering itself lives in the bind route (`spaces/mod.rs`) and is not asserted here.
    #[tokio::test]
    async fn bind_calls_control_admission_before_convex() {
        let f = fixture().await;
        definition(&f, "owner").await;
        answer(&f.user_core, "POST", ADMISSIONS, control(json!({"admitted": true}))).await;
        assert!(admit(&f).await.is_ok());
        let admissions = calls_to(&f.user_core, "agent-admissions").await;
        assert_eq!(admissions.len(), 1, "one admission for one bind");
        let sent: Value = serde_json::from_slice(&admissions[0].body).expect("a JSON body");
        assert_eq!(sent["agent_subject_id"], "agent-agent-1");
        assert_eq!(sent["idempotency_key"], "");
        assert_eq!(
            convex_ops(&f.convex).await,
            vec![DEFINITION.to_owned()],
            "the only Convex call here is the definition read"
        );
    }

    /// Control's typed 403 `agent_not_addable` on the admission is relayed with its `details.reason`.
    #[tokio::test]
    async fn agent_not_addable_maps_reason() {
        let f = fixture().await;
        definition(&f, "owner").await;
        let refusal = json!({"error": {
            "code": "agent_not_addable",
            "message": "This agent cannot be added to this room.",
            "details": {"reason": "space_role_below_floor"}
        }});
        answer(&f.user_core, "POST", ADMISSIONS, ResponseTemplate::new(403).set_body_json(refusal.clone())).await;
        let (status, Json(body)) = admit(&f)
            .await
            .expect_err("the admission refusal stops the bind");
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body, refusal);
        assert_eq!(body["error"]["details"]["reason"], "space_role_below_floor");
        assert!(
            calls_to(&f.user_core, SUBJECTS).await.is_empty(),
            "an owner definition is not registered again"
        );
    }

    // ---- WS17-S5: the installable list, the access write and the primary agent ------------------

    /// Control's per-subject answer decides each row: a personal agent Control does not show is dropped, a visible one
    /// that cannot be added is blocked with Control's reason, an organisation definition Control does not know is
    /// addable, and a bound one is already bound.
    #[tokio::test]
    async fn installable_list_drops_invisible_and_marks_blocked() {
        let f = fixture().await;
        answer(
            &f.user_core,
            "GET",
            SUBJECTS,
            control(json!({"agents": [
                {"agent_subject_id": "agent-addable"},
                {"agent_subject_id": "agent-blocked"},
                {"agent_subject_id": "agent-bound"}
            ]})),
        )
        .await;
        convex(
            &f,
            "spaceAgents:listInstallableSpaceAgentsForGateway",
            convex_value(json!([
                {"subjectId": "agent-hidden", "agentRef": "hidden", "name": "Hidden", "visibility": "owner", "alreadyBound": false},
                {"subjectId": "agent-blocked", "agentRef": "blocked", "name": "Blocked", "visibility": "owner", "alreadyBound": false},
                {"subjectId": "agent-addable", "agentRef": "addable", "name": "Addable", "visibility": "owner", "alreadyBound": false},
                {"subjectId": "agent-org-new", "agentRef": "org-new", "name": "Org", "visibility": "org", "alreadyBound": false},
                {"subjectId": "agent-bound", "agentRef": "bound", "name": "Bound", "visibility": "owner", "alreadyBound": true}
            ])),
        )
        .await;
        answer(
            &f.user_core,
            "POST",
            "/api/v1/internal/spaces/space-room/agent-candidates",
            control(json!({"candidates": [
                {"agent_subject_id": "agent-hidden", "visible": false, "can_add": false, "reason": ""},
                {"agent_subject_id": "agent-blocked", "visible": true, "can_add": false, "reason": "space_role_below_floor"},
                {"agent_subject_id": "agent-addable", "visible": true, "can_add": true, "reason": ""},
                {"agent_subject_id": "agent-bound", "visible": true, "can_add": false, "reason": ""}
            ]})),
        )
        .await;

        let rows = installable_agent_rows(&f.state, &user(), ORG, SPACE)
            .await
            .ok()
            .expect("the list answers");
        let row = |agent: &str| {
            rows.iter()
                .find(|candidate| candidate["agent_ref"] == agent)
                .cloned()
        };
        assert!(row("hidden").is_none(), "a personal agent Control does not show is dropped");
        let blocked = row("blocked").expect("the blocked agent is listed");
        assert_eq!(blocked["add_state"], "blocked");
        assert_eq!(blocked["reason"], "space_role_below_floor");
        assert_eq!(row("addable").expect("listed")["add_state"], "addable");
        assert_eq!(
            row("org-new").expect("listed")["add_state"],
            "addable",
            "an organisation definition Control does not know yet is addable"
        );
        assert_eq!(row("bound").expect("listed")["add_state"], "already_bound");
        let asked = convex_args(&f.convex, "spaceAgents:listInstallableSpaceAgentsForGateway").await;
        assert_eq!(
            asked[0]["visibleOwnerAgentIds"],
            json!(["agent-addable", "agent-blocked", "agent-bound"]),
            "Convex receives Control's visible subject ids"
        );
    }

    /// Control's stale-revision 409 on the access write is relayed with `details.current_revision`, and no Convex
    /// projection is written.
    #[tokio::test]
    async fn access_put_conflict_is_409_with_revision() {
        let f = fixture().await;
        let conflict = json!({"error": {
            "code": "agent_state_changed",
            "message": "The agent changed. Reload and try again.",
            "details": {"current_revision": 7}
        }});
        answer(
            &f.user_core,
            "GET",
            "/api/v1/internal/agents/subjects/agent-agent-1",
            control(json!({"agent_subject_id": "agent-agent-1", "visibility": "org", "add_policy": "anyone", "revision": 7})),
        )
        .await;
        answer(
            &f.user_core,
            "PUT",
            "/api/v1/internal/agents/subjects/agent-agent-1/access",
            ResponseTemplate::new(409).set_body_json(conflict.clone()),
        )
        .await;
        convex(
            &f,
            "spaceAgents:setAgentVisibilityProjectionForGateway",
            convex_value(Value::Null),
        )
        .await;
        let (status, Json(body)) = put_agent_access_core(
            &f.state,
            &user(),
            "agent-1",
            &json!({"visibility": "org", "add_policy": "anyone", "entries": [], "expected_revision": 6}),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body, conflict);
        assert_eq!(body["error"]["details"]["current_revision"], 7);
        assert!(
            convex_ops(&f.convex).await.is_empty(),
            "the projection is written only after Control accepts"
        );
    }

    /// Control shows the caller the agent first: a hidden agent answers 404 and is never made primary.
    #[tokio::test]
    async fn primary_agent_validated_at_control() {
        let f = fixture().await;
        answer(
            &f.user_core,
            "GET",
            "/api/v1/internal/agents/subjects/agent-agent-9",
            ResponseTemplate::new(404).set_body_json(json!({"error": {"code": "not_found"}})),
        )
        .await;
        answer(
            &f.user_core,
            "GET",
            "/api/v1/internal/agents/subjects/agent-agent-8",
            control(json!({"agent_subject_id": "agent-agent-8"})),
        )
        .await;
        convex(&f, "users:setPrimaryAgentForGateway", convex_value(Value::Null)).await;

        let (status, Json(body)) =
            set_primary_agent_core(&f.state, &user(), &json!({"agent_ref": "agent-9"})).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "agent_not_found");
        assert!(convex_ops(&f.convex).await.is_empty(), "a hidden agent is never made primary");

        let (status, _) =
            set_primary_agent_core(&f.state, &user(), &json!({"agent_ref": "agent-8"})).await;
        assert_eq!(status, StatusCode::OK);
        let written = convex_args(&f.convex, "users:setPrimaryAgentForGateway").await;
        assert_eq!(written[0]["agentSubjectId"], "agent-agent-8");
    }


    // ---- D22as / D19 / D24: org-admin edit of an ownerless org agent, and the owner-to-org flip -----------

    fn admin() -> AuthenticatedUser {
        let mut admin = user();
        admin.auth_role = Some("admin".to_owned());
        admin.authorized_membership = Some(AuthorizedMembership {
            organization_id: ORG.to_owned(),
            role: "admin".to_owned(),
        });
        admin
    }

    #[test]
    fn org_admin_editable_mirrors_controls_ownerless_org_visible_admin_rule() {
        let ownerless = json!({"visibility": "org", "role": ""});
        assert!(org_admin_editable(&admin(), &ownerless));
        assert!(!org_admin_editable(&user(), &ownerless), "a plain member is not an org admin");
        assert!(!org_admin_editable(&admin(), &json!({"visibility": "org", "owner_user_id": "user-9"})), "an owned agent");
        assert!(!org_admin_editable(&admin(), &json!({"visibility": "owner"})), "a personal agent");
    }

    #[test]
    fn an_owner_to_org_flip_stores_the_actor_as_a_manager_unless_listed_and_nothing_else_does() {
        let owned = json!({"owner_user_id": "user-1", "visibility": "owner"});
        let other = json!({"principal_id": "user-2", "role": "collaborator"});
        assert_eq!(
            entries_as_stored(&owned, "org", "user-1", vec![other.clone()]),
            vec![other.clone(), json!({"principal_id": "user-1", "role": "manager"})]
        );
        let listed = json!({"principal_id": "user-1", "role": "collaborator"});
        assert_eq!(entries_as_stored(&owned, "org", "user-1", vec![listed.clone()]), vec![listed]);
        assert_eq!(entries_as_stored(&owned, "owner", "user-1", vec![]), Vec::<Value>::new(), "no flip");
        let ownerless = json!({"visibility": "org"});
        assert_eq!(entries_as_stored(&ownerless, "org", "user-1", vec![]), Vec::<Value>::new(), "an admin edit adds no one");
        let full: Vec<Value> = (0..MAX_AGENT_ACCESS_ENTRIES)
            .map(|index| json!({"principal_id": format!("p-{index}"), "role": "collaborator"}))
            .collect();
        assert_eq!(entries_as_stored(&owned, "org", "user-1", full.clone()).len(), full.len(), "the cap holds");
    }

    #[tokio::test]
    async fn an_owner_to_org_flip_writes_the_former_owner_into_the_projection_and_the_answer() {
        let f = fixture().await;
        answer(
            &f.user_core,
            "GET",
            "/api/v1/internal/agents/subjects/agent-agent-1",
            control(json!({"agent_subject_id": "agent-agent-1", "owner_user_id": "user-1", "visibility": "owner", "revision": 3})),
        )
        .await;
        answer(
            &f.user_core,
            "PUT",
            "/api/v1/internal/agents/subjects/agent-agent-1/access",
            control(json!({"agent_subject_id": "agent-agent-1", "visibility": "org", "add_policy": "anyone", "revision": 4, "role": "manager"})),
        )
        .await;
        convex(&f, "spaceAgents:setAgentVisibilityProjectionForGateway", convex_value(json!({}))).await;
        let (status, Json(body)) = put_agent_access_core(
            &f.state,
            &user(),
            "agent-1",
            &json!({"visibility": "org", "add_policy": "anyone", "entries": [], "expected_revision": 3}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let manager = json!([{"principal_id": "user-1", "role": "manager"}]);
        assert_eq!(body["data"]["entries"], manager);
        let projected = convex_args(&f.convex, "spaceAgents:setAgentVisibilityProjectionForGateway").await;
        assert_eq!(projected[0]["accessEntries"], json!([{"principalId": "user-1", "role": "manager"}]));
    }

    #[tokio::test]
    async fn an_ownerless_agent_shows_no_owner_label_and_says_an_org_admin_may_edit_it() {
        let f = fixture().await;
        answer(
            &f.user_core,
            "GET",
            "/api/v1/internal/agents/subjects/agent-agent-1",
            control(json!({"agent_subject_id": "agent-agent-1", "visibility": "org", "add_policy": "anyone", "revision": 4, "role": ""})),
        )
        .await;
        convex(
            &f,
            "spaceAgents:agentDefinitionForGateway",
            convex_value(json!({"agentRef": "agent-1", "ownerLabel": "Former Owner", "accessEntries": []})),
        )
        .await;
        let (status, Json(body)) = get_agent_access_core(&f.state, &admin(), "agent-1").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body["data"]["owner_label"].is_null(), "the cleared owner is not shown as the owner");
        assert_eq!(body["data"]["org_admin_editable"], true);
        let (_, Json(member_view)) = get_agent_access_core(&f.state, &user(), "agent-1").await;
        assert_eq!(member_view["data"]["org_admin_editable"], false);
    }
}
