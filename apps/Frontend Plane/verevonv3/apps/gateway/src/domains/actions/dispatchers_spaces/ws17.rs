//! WS17-S5: the four human-only personal-agent actions. Each rides the same core function as its REST
//! route in `domains/spaces/personal_agents.rs`, so the rollout flag, the membership check, Control's
//! decisions and the typed errors exist once. None is model-executable (`model-access.ts` closes them):
//! a model creating, re-scoping or moving a person's agent would be an authority inversion.
//!
//! WS17-S6: `spaces.open_private_panel` rides the open core of `domains/spaces/private_panel.rs` the same way.
//!
//! WS17-S7: `memory.declassify_to_space` rides the core of `domains/spaces/memory_declassify.rs` the same way. Its
//! input carries the route's three body fields (`memoryId`, `reviewedContent`, `clientRequestId`) and `spaceRef`.

use axum::{http::HeaderMap, response::Response, Json};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::personal_agents::{
    create_personal_agent_core, put_agent_access_core, set_primary_agent_core, withdraw_agent_core,
};
use crate::domains::spaces::memory_declassify::declassify_memory_core;
use crate::domains::spaces::private_panel::open_private_panel_core;
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

/// `spaces.open_private_panel` (WS17-S6): get or create the caller's private panel pointer for one room. Human-only;
/// the ZDR refusal and the flag are checked by the core, which reads the request headers for the ZDR check.
pub(in crate::domains::actions) async fn dispatch_spaces_open_private_panel(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.open_private_panel requires a non-empty 'spaceRef'");
    }
    let (status, Json(payload)) = open_private_panel_core(state, user, headers, space_ref).await;
    owner_json_response_to_envelope("spaces.open_private_panel", user, status, payload)
}

/// `memory.declassify_to_space` (WS17-S7): share one of the caller's personal memories into a shared Space room
/// after the caller has reviewed the text. Human-only. Action input keys are camelCase like every sibling arm
/// (`spaceRef`, `memoryId`, `reviewedContent`, `clientRequestId`); the core body keeps the route's snake_case
/// names. The text is read exactly as sent (`ticket_string` trims, so it is not used here); the core applies
/// the route's refusals in the route's order.
pub(in crate::domains::actions) async fn dispatch_memory_declassify_to_space(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = input.get("spaceRef").and_then(Value::as_str).unwrap_or_default();
    let body = json!({
        "memory_id": input.get("memoryId").cloned().unwrap_or(Value::Null),
        "reviewed_content": input.get("reviewedContent").cloned().unwrap_or(Value::Null),
        "client_request_id": input.get("clientRequestId").cloned().unwrap_or(Value::Null),
    });
    let (status, Json(payload)) = declassify_memory_core(state, user, headers, space_ref, body).await;
    owner_json_response_to_envelope("memory.declassify_to_space", user, status, payload)
}
