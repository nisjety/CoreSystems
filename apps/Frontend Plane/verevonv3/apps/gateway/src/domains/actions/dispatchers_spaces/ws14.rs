//! WS14-S7: action dispatchers for room files (`space.files.*`). `list`, `read_text` and `delete`
//! ride the same core functions as routes R3 to R5 in `domains/spaces/files.rs`, so the flag, the
//! Control decisions and the typed errors exist once. `attach` is human-only multipart: a JSON
//! dispatch cannot carry bytes and answers 422 `upload_requires_multipart`. None of the four ids
//! is model-executable (model-access.ts closes all of them).

use axum::{
    http::HeaderMap,
    response::{IntoResponse, Response},
};
use serde_json::Value;

use crate::config::AppState;
use crate::domains::spaces::files;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_space_files_attach() -> Response {
    files::attach_requires_multipart().into_response()
}

pub(in crate::domains::actions) async fn dispatch_space_files_list(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("space.files.list requires a non-empty 'spaceRef'");
    }
    let limit = input
        .get("limit")
        .and_then(Value::as_i64)
        .map(|n| n.to_string());
    let cursor = ticket_string(input, "cursor");
    let cursor = (!cursor.is_empty()).then_some(cursor);
    let (status, axum::Json(resp)) =
        match files::list_core(state, user, headers, space_ref, limit.as_deref(), cursor).await {
            Ok(reply) | Err(reply) => reply,
        };
    owner_json_response_to_envelope("space.files.list", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_space_files_read_text(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let document_id = ticket_string(input, "documentId");
    if space_ref.is_empty() || document_id.is_empty() {
        return ticket_bad_request(
            "space.files.read_text requires a non-empty 'spaceRef' and 'documentId'",
        );
    }
    let max_chars = input
        .get("maxChars")
        .and_then(Value::as_u64)
        .map(|n| n.to_string());
    let (status, axum::Json(resp)) = match files::read_text_core(
        state,
        user,
        headers,
        space_ref,
        document_id,
        max_chars.as_deref(),
    )
    .await
    {
        Ok(reply) | Err(reply) => reply,
    };
    owner_json_response_to_envelope("space.files.read_text", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_space_files_delete(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let document_id = ticket_string(input, "documentId");
    if space_ref.is_empty() || document_id.is_empty() {
        return ticket_bad_request(
            "space.files.delete requires a non-empty 'spaceRef' and 'documentId'",
        );
    }
    let (status, axum::Json(resp)) =
        match files::delete_core(state, user, headers, space_ref, document_id).await {
            Ok(reply) | Err(reply) => reply,
        };
    owner_json_response_to_envelope("space.files.delete", user, status, resp)
}
