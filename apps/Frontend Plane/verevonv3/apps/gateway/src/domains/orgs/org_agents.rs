//! W3-DEC-ORGAGENT (owner decision D19): the org-admin management routes of the org-visible
//! agents. Control (user-core) owns the rows, the canonical owner-or-admin check (auth-core,
//! re-resolved on every call), the lifecycle rules, the room cascade and the audit row. This
//! module is only the Frontend Plane's door to it: session role (defence in depth), body shape
//! check, then a proxy that signs the v1 delegation. It evaluates no policy and holds no state.
//!
//! Order on every route: require_org_admin, id and body shape check, proxy_json. The routes have
//! no flag: they are org-admin gated here and in Control, and carry no content.

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    envelope::{error, error_with_details},
    middleware::AuthenticatedUser,
    upstream::proxy_json,
};

use super::shared::{actor_for, require_org_admin, GatewayJsonResponse};

const CODE_INVALID: &str = "invalid_request";
const AGENT_ID_SUFFIX_MIN: usize = 6;
const AGENT_ID_SUFFIX_MAX: usize = 64;

fn invalid(field: &str, message: &str) -> GatewayJsonResponse {
    (
        StatusCode::BAD_REQUEST,
        Json(error_with_details(
            CODE_INVALID,
            message,
            json!({ "field": field }),
        )),
    )
}

fn agent_not_found() -> GatewayJsonResponse {
    (
        StatusCode::NOT_FOUND,
        Json(error("agent_not_found", "agent not found")),
    )
}

fn control_url(state: &AppState, suffix: &str) -> String {
    format!(
        "{}/api/v1/internal/agents/org-subjects{suffix}",
        state.user_core_url
    )
}

/// `agent-` followed by 6 to 64 ASCII letters, digits, `_` or `-`: the shape migration 045
/// enforces. Checked here so a malformed id never reaches the signed URL.
fn is_agent_subject_id(id: &str) -> bool {
    let Some(suffix) = id.strip_prefix("agent-") else {
        return false;
    };
    (AGENT_ID_SUFFIX_MIN..=AGENT_ID_SUFFIX_MAX).contains(&suffix.len())
        && suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The body of a lifecycle change: `{ "expected_revision": <non-negative integer> }` and nothing
/// else. The organization, the actor and the role come from the delegation, never from the body.
/// Returns the body rebuilt from the allowed field, or the refusal.
fn validate_lifecycle_body(body: &Value) -> Result<Value, GatewayJsonResponse> {
    let Some(object) = body.as_object() else {
        return Err(invalid("body", "The request body must be a JSON object."));
    };
    if object.keys().any(|key| key != "expected_revision") {
        return Err(invalid("body", "The request body has an unsupported field."));
    }
    let Some(revision) = object
        .get("expected_revision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision >= 0)
    else {
        return Err(invalid(
            "expected_revision",
            "expected_revision must be a non-negative integer.",
        ));
    };
    Ok(json!({ "expected_revision": revision }))
}

/// Shared by the routes below and the `spaces.*_org_agent` action dispatchers, so both ride the
/// same org-admin check and the same shape check. `suffix` selects the Control route: "" is the
/// definition itself (retire), "/suspend" and "/reactivate" are the two state changes.
pub(crate) async fn change_org_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    org_id: &str,
    agent_subject_id: &str,
    method: Method,
    suffix: &str,
    body: &Value,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(state, user, org_id).await {
        return response;
    }
    if !is_agent_subject_id(agent_subject_id) {
        return agent_not_found();
    }
    let forwarded = match validate_lifecycle_body(body) {
        Ok(forwarded) => forwarded,
        Err(response) => return response,
    };
    proxy_json(
        state,
        method,
        &control_url(state, &format!("/{agent_subject_id}{suffix}")),
        Some(forwarded),
        Some(org_id),
        Some(&actor_for(user)),
        None,
    )
    .await
}

/// GET /api/v1/orgs/:id/agents: the org-visible agents with their current lifecycle.
pub(crate) async fn list_org_agents(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<String>,
) -> GatewayJsonResponse {
    if let Err(response) = require_org_admin(&state, &user, &id).await {
        return response;
    }
    proxy_json(
        &state,
        Method::GET,
        &control_url(&state, ""),
        None,
        Some(&id),
        Some(&actor_for(&user)),
        None,
    )
    .await
}

/// PUT /api/v1/orgs/:id/agents/:agent_subject_id/suspend
pub(crate) async fn suspend_org_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, agent_subject_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    change_org_agent(
        &state,
        &user,
        &id,
        &agent_subject_id,
        Method::PUT,
        "/suspend",
        &body,
    )
    .await
}

/// PUT /api/v1/orgs/:id/agents/:agent_subject_id/reactivate
pub(crate) async fn reactivate_org_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, agent_subject_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    change_org_agent(
        &state,
        &user,
        &id,
        &agent_subject_id,
        Method::PUT,
        "/reactivate",
        &body,
    )
    .await
}

/// DELETE /api/v1/orgs/:id/agents/:agent_subject_id: retire the agent (soft delete). The
/// revision travels in the JSON body, as for the other two changes.
pub(crate) async fn retire_org_agent(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path((id, agent_subject_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> GatewayJsonResponse {
    change_org_agent(
        &state,
        &user,
        &id,
        &agent_subject_id,
        Method::DELETE,
        "",
        &body,
    )
    .await
}
