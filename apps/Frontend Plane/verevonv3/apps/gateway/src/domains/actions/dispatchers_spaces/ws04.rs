//! WS04-S6: action dispatchers for the room member writer (`spaces.change_member_role`,
//! `spaces.leave_room`). Both ride the same gateway functions as the member routes in
//! `domains/spaces/room_admin.rs`, so the flag, the Control delegation and the typed error
//! pass-through exist once. Neither id is model-executable. The caller-supplied idempotency key
//! is forwarded unchanged so a retried action replays instead of re-applying.

use axum::response::Response;
use serde_json::{json, Map, Value};

use crate::domains::spaces::room_admin::{
    change_member_role, feature_disabled, remove_member, update_room_details,
};
use crate::middleware::AuthenticatedUser;
use crate::config::AppState;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_change_member_role(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
    idempotency_key: &str,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let member_id = ticket_string(input, "memberId");
    let role = ticket_string(input, "role");
    if space_ref.is_empty() || member_id.is_empty() || role.is_empty() {
        return ticket_bad_request(
            "spaces.change_member_role requires non-empty 'spaceRef', 'memberId' and 'role'",
        );
    }
    let mut body = Map::new();
    body.insert("role".to_owned(), json!(role));
    if let Some(revision) = input.get("expectedRevision").and_then(Value::as_i64) {
        body.insert("expected_revision".to_owned(), json!(revision));
    }
    if !idempotency_key.trim().is_empty() {
        body.insert("idempotency_key".to_owned(), json!(idempotency_key.trim()));
    }
    let (status, axum::Json(resp)) =
        change_member_role(state, user, space_ref, member_id, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.change_member_role", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_leave_room(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.leave_room requires a non-empty 'spaceRef'");
    }
    // Leaving has no legacy path: with the writer off the action is refused rather than
    // mapped onto the legacy remove, which would look for a member literally named "me".
    if !state.space_features.membership_writer_api {
        let (status, axum::Json(resp)) = feature_disabled();
        return owner_json_response_to_envelope("spaces.leave_room", user, status, resp);
    }
    let (status, axum::Json(resp)) = remove_member(state, user, space_ref, "me").await;
    owner_json_response_to_envelope("spaces.leave_room", user, status, resp)
}

/// WS04-S2: `spaces.update_room_details`. Rides the same gateway function as
/// `PATCH /api/v1/spaces/{space_ref}`, so the flag, the owner-or-manager gate and the typed
/// errors exist once. The id is human-only (never model-executable).
pub(in crate::domains::actions) async fn dispatch_spaces_update_room_details(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.update_room_details requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    for field in ["name", "topic"] {
        if let Some(value) = input.get(field) {
            body.insert(field.to_owned(), value.clone());
        }
    }
    let (status, axum::Json(resp)) =
        update_room_details(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.update_room_details", user, status, resp)
}
