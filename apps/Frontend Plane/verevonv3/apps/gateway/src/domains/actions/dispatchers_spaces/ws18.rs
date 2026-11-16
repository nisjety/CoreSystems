//! WS18-S5: action dispatcher for the agent self-API binding switch (`spaces.set_agent_self_api`). It rides the
//! same gateway function as `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/self-api`, so the flag, the
//! owner-or-manager gate and the typed errors exist once. The id is human-only (never model-executable): who may
//! let a room agent act on its own thread is a room governance decision.

use axum::response::Response;
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::agent_canvas_tools::set_space_agent_canvas_tools;
use crate::domains::spaces::self_api::set_space_agent_self_api;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_set_agent_self_api(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let binding_ref = ticket_string(input, "bindingRef");
    if space_ref.is_empty() || binding_ref.is_empty() {
        return ticket_bad_request(
            "spaces.set_agent_self_api requires non-empty 'spaceRef' and 'bindingRef'",
        );
    }
    // `enabled` is passed through as sent: the writer refuses anything but a JSON boolean.
    let body = json!({ "enabled": input.get("enabled").cloned().unwrap_or(Value::Null) });
    let (status, axum::Json(resp)) =
        set_space_agent_self_api(state, user, space_ref, binding_ref, &body).await;
    owner_json_response_to_envelope("spaces.set_agent_self_api", user, status, resp)
}

/// W3-DEC-CANVAS (D2): `spaces.set_agent_canvas_tools`, the canvas-tools binding switch. It rides the same gateway
/// function as `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/canvas-tools`, so the flag, the owner-or-manager
/// gate and the typed errors exist once. The id is human-only (never model-executable): a room agent may not turn its
/// own canvas access on.
pub(in crate::domains::actions) async fn dispatch_spaces_set_agent_canvas_tools(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let binding_ref = ticket_string(input, "bindingRef");
    if space_ref.is_empty() || binding_ref.is_empty() {
        return ticket_bad_request(
            "spaces.set_agent_canvas_tools requires non-empty 'spaceRef' and 'bindingRef'",
        );
    }
    // `enabled` is passed through as sent: the writer refuses anything but a JSON boolean.
    let body = json!({ "enabled": input.get("enabled").cloned().unwrap_or(Value::Null) });
    let (status, axum::Json(resp)) =
        set_space_agent_canvas_tools(state, user, space_ref, binding_ref, &body).await;
    owner_json_response_to_envelope("spaces.set_agent_canvas_tools", user, status, resp)
}
