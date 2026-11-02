//! WS10-S5b: action dispatcher for the per-tool policy (`spaces.set_tool_policy`). It rides the
//! same gateway functions as `PUT /api/v1/spaces/{space_ref}/tool-policy` and
//! `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/tool-policy`, so the flag, Control's role
//! gate and the typed errors exist once. The id is human-only (never model-executable): an agent
//! must never edit the policy that limits it.

use axum::response::Response;
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::tool_policy::{put_agent_tool_policy, put_space_tool_policy};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_set_tool_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.set_tool_policy requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    for (from, to) in [("entries", "entries"), ("expectedRevision", "expected_revision")] {
        if let Some(value) = input.get(from) {
            body.insert(to.to_owned(), value.clone());
        }
    }
    let binding_ref = ticket_string(input, "bindingRef");
    let (status, axum::Json(resp)) = if binding_ref.is_empty() {
        put_space_tool_policy(state, user, space_ref, &Value::Object(body)).await
    } else {
        put_agent_tool_policy(state, user, space_ref, binding_ref, &Value::Object(body)).await
    };
    owner_json_response_to_envelope("spaces.set_tool_policy", user, status, resp)
}
