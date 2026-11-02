//! WS11-S7: action dispatchers for the attention inbox (`spaces.set_attention_item_state`) and the
//! thread follow control (`spaces.set_thread_follow`). Both ride the same functions as `POST
//! /api/v1/spaces/attention/items/{item_id}/state` and `PUT
//! /api/v1/spaces/{space_ref}/threads/{thread_id}/follow` in `domains/spaces/attention.rs`, so the
//! flag, the Zero Data Retention refusal and the typed errors exist once. Neither id is
//! model-executable: an inbox and a follow are a person's own attention.

use axum::{http::HeaderMap, response::Response};
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::attention::{put_follow_core, set_item_state_core};
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_set_attention_item_state(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let item_id = ticket_string(input, "itemId");
    if item_id.is_empty() {
        return ticket_bad_request("spaces.set_attention_item_state requires a non-empty 'itemId'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("state") {
        body.insert("state".to_owned(), value.clone());
    }
    if let Some(value) = input.get("snoozeUntil") {
        body.insert("snooze_until".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        set_item_state_core(state, user, item_id, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.set_attention_item_state", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_thread_follow(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let thread_id = ticket_string(input, "threadId");
    if space_ref.is_empty() || thread_id.is_empty() {
        return ticket_bad_request(
            "spaces.set_thread_follow requires a non-empty 'spaceRef' and 'threadId'",
        );
    }
    let mut body = Map::new();
    if let Some(value) = input.get("state") {
        body.insert("state".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) = put_follow_core(
        state,
        user,
        Some(headers),
        space_ref,
        thread_id,
        &Value::Object(body),
    )
    .await;
    owner_json_response_to_envelope("spaces.set_thread_follow", user, status, resp)
}
