//! The owner's private panel thread, created with conversation scope (W3-FIX-9, SD-22 v1).
//!
//! `POST /v1/chat/conversation-threads` gets or creates one chat thread for the verified caller under the
//! session key the gateway names, body `{"session_key": "..."}`. The thread is created with
//! `metadata.source_scope = "conversation"` (session-core `CreateThread`, W3-FIX-2): its scope is locked at
//! insert, and a create that finds a workspace thread under the key is refused, never returned. It has no
//! space, starts no run and stores no message. Only the narrowing scope is reachable from this route.
//!
//! The create is a durable write, so a zero-retention caller is refused FIRST, with the typed catalogue refusal
//! (`require_non_zdr_durable_mutation`), before the body or any upstream call. Authority is the verified session
//! bearer and its claims. The body is typed with `deny_unknown_fields`, so a forged `org_id`, `user_id`,
//! `space_id` or `source_scope` is a 400 and is never forwarded.
//!
//! Errors, as `{error:{code,message,details}}` with fixed words (never the upstream text): 400
//! `invalid_request`, 403 `user_principal_required`, 403 `session_not_authorized`, 409
//! `thread_scope_conflict` (session-core refused a workspace thread under the key), 412 the typed ZDR refusal,
//! and 503 `session_unavailable` for any other failure. A failure creates nothing the caller can see.

use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{Claims, VerifiedSessionBearer};
use crate::http_routes::require_non_zdr_durable_mutation;
use crate::queued_input::valid_client_message_id;
use crate::session_flow;
use crate::state::AppState;

/// The most bytes a session key may hold (the bound the Space create uses).
const MAX_SESSION_KEY_BYTES: usize = 128;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateConversationThreadBody {
    session_key: String,
}

/// `POST /v1/chat/conversation-threads`: the caller's conversation-scope thread for one session key.
pub(crate) async fn create_conversation_thread(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    bearer: VerifiedSessionBearer,
    body: Bytes,
) -> Response {
    // A durable write: a zero-retention caller is refused first, typed, before anything else is read.
    if let Err(refusal) = require_non_zdr_durable_mutation(&claims) {
        return refusal.into_response();
    }
    if claims.user_id.trim().is_empty() {
        return error_response(
            StatusCode::FORBIDDEN,
            "user_principal_required",
            "a user principal is required",
            json!({}),
        );
    }
    let body: CreateConversationThreadBody = match serde_json::from_slice(&body) {
        Ok(body) => body,
        Err(_) => return invalid_request("body"),
    };
    let session_key = body.session_key.trim();
    if session_key.is_empty()
        || session_key.len() > MAX_SESSION_KEY_BYTES
        || !valid_client_message_id(session_key)
    {
        return invalid_request("session_key");
    }
    match session_flow::create_conversation_thread(
        &state,
        &bearer,
        &claims.org_id,
        &claims.user_id,
        session_key,
    )
    .await
    {
        Ok(thread_id) => (StatusCode::OK, Json(json!({ "thread_id": thread_id }))).into_response(),
        Err(status) => {
            tracing::warn!(grpc_code = ?status.code(), "conversation thread create refused");
            session_error(&status)
        }
    }
}

/// The refusal for a session-core create that returned no thread. Nothing the caller can see was created.
fn session_error(status: &tonic::Status) -> Response {
    match status.code() {
        tonic::Code::InvalidArgument => invalid_request("session_key"),
        tonic::Code::FailedPrecondition => error_response(
            StatusCode::CONFLICT,
            "thread_scope_conflict",
            "a thread for this session already has a different source scope",
            json!({}),
        ),
        tonic::Code::PermissionDenied | tonic::Code::Unauthenticated => error_response(
            StatusCode::FORBIDDEN,
            "session_not_authorized",
            "the session is not authorized for this thread",
            json!({}),
        ),
        _ => error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "session_unavailable",
            "the conversation could not be opened right now",
            json!({ "retryable": true }),
        ),
    }
}

fn invalid_request(field: &str) -> Response {
    error_response(
        StatusCode::BAD_REQUEST,
        "invalid_request",
        "the request is not valid",
        json!({ "field": field }),
    )
}

fn error_response(status: StatusCode, code: &str, message: &str, details: Value) -> Response {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message, "details": details } })),
    )
        .into_response()
}
