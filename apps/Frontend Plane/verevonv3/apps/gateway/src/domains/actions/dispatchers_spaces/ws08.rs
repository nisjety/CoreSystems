//! WS08-S5: action dispatcher for the binding wake policy (`spaces.set_agent_wake_policy`). It
//! rides the same gateway function as `PATCH /api/v1/spaces/{space_ref}/agents/{binding_ref}/policy`,
//! so the flag, the owner-or-manager gate and the typed errors exist once. The id is human-only
//! (never model-executable): who may wake an agent is a room governance decision.
//!
//! EP-6 S9 (`spaces.agent_binding.pin`) rides the same family: the version pin of one bound agent
//! through `PUT /api/v1/spaces/{space_ref}/agents/{binding_ref}/pin`, with the same gate and the
//! same human-only rule.

use axum::response::Response;
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::agent_policy::{set_space_agent_pin, set_space_agent_wake_policy};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_set_agent_wake_policy(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let binding_ref = ticket_string(input, "bindingRef");
    if space_ref.is_empty() || binding_ref.is_empty() {
        return ticket_bad_request(
            "spaces.set_agent_wake_policy requires non-empty 'spaceRef' and 'bindingRef'",
        );
    }
    let mut body = Map::new();
    for (from, to) in [
        ("respondTo", "respond_to"),
        ("respondToSubjects", "respond_to_subjects"),
        ("expectedUpdatedAt", "expected_updated_at"),
    ] {
        if let Some(value) = input.get(from) {
            body.insert(to.to_owned(), value.clone());
        }
    }
    let (status, axum::Json(resp)) =
        set_space_agent_wake_policy(state, user, space_ref, binding_ref, &Value::Object(body))
            .await;
    owner_json_response_to_envelope("spaces.set_agent_wake_policy", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_agent_pin(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let binding_ref = ticket_string(input, "bindingRef");
    if space_ref.is_empty() || binding_ref.is_empty() {
        return ticket_bad_request(
            "spaces.agent_binding.pin requires non-empty 'spaceRef' and 'bindingRef'",
        );
    }
    // `version` is passed through as sent: a whole number pins that version, null clears the pin.
    let mut body = Map::new();
    if let Some(version) = input.get("version") {
        body.insert("version".to_owned(), version.clone());
    }
    let (status, axum::Json(resp)) =
        set_space_agent_pin(state, user, space_ref, binding_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.agent_binding.pin", user, status, resp)
}
