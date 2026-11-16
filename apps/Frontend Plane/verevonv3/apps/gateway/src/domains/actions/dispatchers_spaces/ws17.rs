//! WS17-S5: the four human-only personal-agent actions. Each rides the same core function as its REST
//! route in `domains/spaces/personal_agents.rs`, so the rollout flag, the membership check, Control's
//! decisions and the typed errors exist once. None is model-executable (`model-access.ts` closes them):
//! a model creating, re-scoping or moving a person's agent would be an authority inversion.

use axum::{response::Response, Json};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::personal_agents::{
    create_personal_agent_core, put_agent_access_core, set_primary_agent_core, withdraw_agent_core,
};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_agents_create_personal(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let mut body = json!({
        "name": ticket_string(input, "name"),
        "client_request_id": ticket_string(input, "clientRequestId"),
    });
    if !ticket_string(input, "instructions").is_empty() {
        body["instructions"] = json!(ticket_string(input, "instructions"));
    }
    if !ticket_string(input, "avatarColor").is_empty() {
        body["avatar_color"] = json!(ticket_string(input, "avatarColor"));
    }
    let (status, Json(payload)) = create_personal_agent_core(state, user, &body).await;
    owner_json_response_to_envelope("agents.create_personal", user, status, payload)
}

pub(in crate::domains::actions) async fn dispatch_agents_update_access(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let agent_ref = ticket_string(input, "agentRef");
    if agent_ref.is_empty() {
        return ticket_bad_request("agents.update_access requires a non-empty 'agentRef'");
    }
    let entries: Vec<Value> = input
        .get("entries")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|entry| {
            json!({
                "principal_id": ticket_string(entry, "principalId"),
                "role": ticket_string(entry, "role"),
            })
        })
        .collect();
    let body = json!({
        "visibility": ticket_string(input, "visibility"),
        "add_policy": ticket_string(input, "addPolicy"),
        "entries": entries,
        "expected_revision": input.get("expectedRevision").cloned().unwrap_or(Value::Null),
    });
    let (status, Json(payload)) = put_agent_access_core(state, user, agent_ref, &body).await;
    owner_json_response_to_envelope("agents.update_access", user, status, payload)
}

pub(in crate::domains::actions) async fn dispatch_agents_set_primary(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let Some(agent_ref) = input.get("agentRef") else {
        return ticket_bad_request("agents.set_primary requires 'agentRef' (a string or null)");
    };
    let body = json!({ "agent_ref": agent_ref });
    let (status, Json(payload)) = set_primary_agent_core(state, user, &body).await;
    owner_json_response_to_envelope("agents.set_primary", user, status, payload)
}

pub(in crate::domains::actions) async fn dispatch_spaces_withdraw_agent(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let agent_ref = ticket_string(input, "agentRef");
    if space_ref.is_empty() || agent_ref.is_empty() {
        return ticket_bad_request("spaces.withdraw_agent requires non-empty 'spaceRef' and 'agentRef'");
    }
    let (status, Json(payload)) = withdraw_agent_core(state, user, agent_ref, space_ref).await;
    owner_json_response_to_envelope("spaces.withdraw_agent", user, status, payload)
}
