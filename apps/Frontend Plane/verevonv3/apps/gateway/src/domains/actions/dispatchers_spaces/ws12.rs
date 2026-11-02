//! WS12-S3: action dispatchers for direct messages (`spaces.open_dm`, `spaces.hide_space`). Both
//! ride the same functions as `POST /api/v1/spaces/dm` and `POST`/`DELETE
//! /api/v1/spaces/{space_ref}/hide` in `domains/spaces/dm.rs`, so the flag, the Zero Data
//! Retention refusal, Control's pre-check and the typed errors exist once. Neither id is
//! model-executable: opening a DM names people and hiding one is a person's own sidebar state.

use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::dm::{open_dm_core, set_hidden};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_open_dm(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let Some(participants) = input.get("participants").filter(|value| value.is_array()) else {
        return ticket_bad_request("spaces.open_dm requires a 'participants' array");
    };
    let client_request_id = ticket_string(input, "clientRequestId");
    if client_request_id.is_empty() {
        return ticket_bad_request("spaces.open_dm requires a non-empty 'clientRequestId'");
    }
    // The open flow validates the shape and refuses anything else.
    let body = json!({
        "participants": participants,
        "client_request_id": client_request_id,
    });
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    let (status, axum::Json(resp)) = open_dm_core(state, user, headers, &bytes).await;
    owner_json_response_to_envelope("spaces.open_dm", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_hide_space(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.hide_space requires a non-empty 'spaceRef'");
    }
    // `hidden` defaults to true: the action hides; false shows the DM again.
    let hidden = input.get("hidden").and_then(Value::as_bool).unwrap_or(true);
    let (status, axum::Json(resp)) = set_hidden(state, user, space_ref, hidden).await;
    owner_json_response_to_envelope("spaces.hide_space", user, status, resp)
}
