//! WS01-S5: action dispatchers for the human post contracts (`spaces.create_post`,
//! `spaces.reply_in_thread`). Both ride the same function as the two REST routes in
//! `domains/spaces/human_posts.rs`, so the flag, the Zero Data Retention refusal, the Control
//! decision and the typed errors exist once. Neither id is model-executable: a post is a person
//! speaking in a room, never an agent proposal. The caller's `clientSendId` is forwarded
//! unchanged so a retried action lands on the same row instead of posting twice.

use axum::{http::HeaderMap, response::Response};
use serde_json::{json, Value};

use crate::config::AppState;
use crate::domains::spaces::human_posts::submit_post;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

/// The untrusted `{content, client_send_id}` body. The post flow validates every field and
/// rejects anything else, so a missing value is passed on as absent rather than checked here.
fn post_body(input: &Value) -> Value {
    json!({
        "content": input.get("content").cloned().unwrap_or(Value::Null),
        "client_send_id": input.get("clientSendId").cloned().unwrap_or(Value::Null),
    })
}

pub(in crate::domains::actions) async fn dispatch_spaces_create_post(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.create_post requires a non-empty 'spaceRef'");
    }
    let (status, axum::Json(resp)) =
        submit_post(state, user, headers, space_ref, None, &post_body(input)).await;
    owner_json_response_to_envelope("spaces.create_post", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_reply_in_thread(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let thread_id = ticket_string(input, "threadId");
    if space_ref.is_empty() || thread_id.is_empty() {
        return ticket_bad_request(
            "spaces.reply_in_thread requires non-empty 'spaceRef' and 'threadId'",
        );
    }
    let (status, axum::Json(resp)) = submit_post(
        state,
        user,
        headers,
        space_ref,
        Some(thread_id),
        &post_body(input),
    )
    .await;
    owner_json_response_to_envelope("spaces.reply_in_thread", user, status, resp)
}
