//! WS11-S3 and WS11-S7: action dispatchers for "Mark all read" (`spaces.mark_all_read`), "Mark
//! unread" (`spaces.mark_unread`), the room level (`spaces.set_notify_level`), the attention inbox
//! (`spaces.set_attention_item_state`) and the thread follow control (`spaces.set_thread_follow`).
//! Each rides the same function as its route (`POST /api/v1/spaces/attention/read-all`, `POST
//! /api/v1/spaces/{space_ref}/unread`, `PUT /api/v1/spaces/{space_ref}/notify`, `POST
//! /api/v1/spaces/attention/items/{item_id}/state` and `PUT
//! /api/v1/spaces/{space_ref}/threads/{thread_id}/follow` in `domains/spaces/attention.rs`, so the
//! flag, the Zero Data Retention refusal and the typed errors exist once. Neither id is
//! model-executable: an inbox and a follow are a person's own attention.
//!
//! WS11-S10 adds `spaces.set_status` (the org-scoped status, `PUT /api/v1/spaces/status`) through
//! `put_status_core` in `domains/spaces/status.rs`: a text under Zero Data Retention is the typed 412.

use axum::{http::HeaderMap, response::Response};
use serde_json::{Map, Value};

use crate::config::AppState;
use crate::domains::spaces::attention::{
    mark_unread_core, put_follow_core, put_notify_core, read_all_core, set_item_state_core,
};
use crate::domains::spaces::status::put_status_core;
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

pub(in crate::domains::actions) async fn dispatch_spaces_mark_all_read(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let mut body = Map::new();
    if let Some(value) = input.get("spaceRefs") {
        body.insert("space_refs".to_owned(), value.clone());
    }
    if let Some(value) = input.get("kinds") {
        body.insert("kinds".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) = read_all_core(state, user, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.mark_all_read", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_mark_unread(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.mark_unread requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("beforeMs") {
        body.insert("before_ms".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        mark_unread_core(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.mark_unread", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_notify_level(
    state: &AppState,
    user: &AuthenticatedUser,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.set_notify_level requires a non-empty 'spaceRef'");
    }
    let mut body = Map::new();
    if let Some(value) = input.get("level") {
        body.insert("level".to_owned(), value.clone());
    }
    if let Some(value) = input.get("hidePreview") {
        body.insert("hide_preview".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        put_notify_core(state, user, space_ref, &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.set_notify_level", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_set_status(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let mut body = Map::new();
    if let Some(value) = input.get("state") {
        body.insert("state".to_owned(), value.clone());
    }
    if let Some(value) = input.get("text") {
        body.insert("text".to_owned(), value.clone());
    }
    if let Some(value) = input.get("expiresAt") {
        body.insert("expires_at".to_owned(), value.clone());
    }
    let (status, axum::Json(resp)) =
        put_status_core(state, user, Some(headers), &Value::Object(body)).await;
    owner_json_response_to_envelope("spaces.set_status", user, status, resp)
}
