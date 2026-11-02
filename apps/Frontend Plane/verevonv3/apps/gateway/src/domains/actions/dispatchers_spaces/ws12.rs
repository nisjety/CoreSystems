//! WS12-S3: action dispatchers for direct messages (`spaces.open_dm`, `spaces.hide_space`). Both
//! ride the same functions as `POST /api/v1/spaces/dm` and `POST`/`DELETE
//! /api/v1/spaces/{space_ref}/hide` in `domains/spaces/dm.rs`, so the flag, the Zero Data
//! Retention refusal, Control's pre-check and the typed errors exist once. Neither id is
//! model-executable: opening a DM names people and hiding one is a person's own sidebar state.

use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::dm::{open_dm_core, set_hidden};
use crate::domains::spaces::message_lifecycle::{revise_message, Revision};
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

/// WS12-S7: `spaces.edit_message` rides the same flow as `PATCH
/// /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}`.
pub(in crate::domains::actions) async fn dispatch_spaces_edit_message(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let (space_ref, thread_id, message_id, key) = (
        ticket_string(input, "spaceRef"),
        ticket_string(input, "threadId"),
        ticket_string(input, "messageId"),
        ticket_string(input, "idempotencyKey"),
    );
    if space_ref.is_empty() || thread_id.is_empty() || message_id.is_empty() {
        return ticket_bad_request(
            "spaces.edit_message requires non-empty 'spaceRef', 'threadId' and 'messageId'",
        );
    }
    let content = input
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let (status, axum::Json(resp)) = revise_message(
        state,
        user,
        headers,
        space_ref,
        thread_id,
        message_id,
        key,
        Revision::Edit {
            content: content.to_owned(),
        },
        Some(input),
    )
    .await;
    owner_json_response_to_envelope("spaces.edit_message", user, status, resp)
}

/// WS12-S7: `spaces.delete_message` rides the same flow as `DELETE
/// /api/v1/spaces/{space_ref}/threads/{thread_id}/messages/{message_id}`.
pub(in crate::domains::actions) async fn dispatch_spaces_delete_message(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let (space_ref, thread_id, message_id, key) = (
        ticket_string(input, "spaceRef"),
        ticket_string(input, "threadId"),
        ticket_string(input, "messageId"),
        ticket_string(input, "idempotencyKey"),
    );
    if space_ref.is_empty() || thread_id.is_empty() || message_id.is_empty() {
        return ticket_bad_request(
            "spaces.delete_message requires non-empty 'spaceRef', 'threadId' and 'messageId'",
        );
    }
    let (status, axum::Json(resp)) = revise_message(
        state,
        user,
        headers,
        space_ref,
        thread_id,
        message_id,
        key,
        Revision::Delete {
            moderation_reason: ticket_string(input, "moderationReason").to_owned(),
        },
        Some(input),
    )
    .await;
    owner_json_response_to_envelope("spaces.delete_message", user, status, resp)
}
