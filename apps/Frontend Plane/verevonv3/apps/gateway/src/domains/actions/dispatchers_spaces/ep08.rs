//! EP-8 S7: action dispatchers for the Space memory tab (`spaces.memory.list` and
//! `spaces.memory.delete`). They ride the same core as the BFF routes `GET /api/v1/spaces/{space_ref}/memory`
//! and `DELETE /api/v1/spaces/{space_ref}/memory/{memory_id}` (domains/spaces/memory.rs), so the rollout
//! switch `GATEWAY_SPACE_MEMORY_UI`, Control's `model.space.memory.manage` decision (the room's owner or a
//! manager) and the typed errors exist once. Neither id is model-executable (model-access.ts closes both):
//! an agent must never read or erase a room's memory.

use axum::http::HeaderMap;
use axum::response::Response;
use serde_json::Value;

use crate::config::AppState;
use crate::domains::spaces::memory;
use crate::middleware::AuthenticatedUser;

use super::super::dispatchers::{
    owner_json_response_to_envelope, ticket_bad_request, ticket_string,
};

pub(in crate::domains::actions) async fn dispatch_spaces_memory_list(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    if space_ref.is_empty() {
        return ticket_bad_request("spaces.memory.list requires a non-empty 'spaceRef'");
    }
    let (status, axum::Json(resp)) = memory::list_core(state, user, headers, space_ref).await;
    owner_json_response_to_envelope("spaces.memory.list", user, status, resp)
}

pub(in crate::domains::actions) async fn dispatch_spaces_memory_delete(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    input: &Value,
) -> Response {
    let space_ref = ticket_string(input, "spaceRef");
    let memory_id = ticket_string(input, "memoryId");
    if space_ref.is_empty() || memory_id.is_empty() {
        return ticket_bad_request(
            "spaces.memory.delete requires a non-empty 'spaceRef' and 'memoryId'",
        );
    }
    let (status, axum::Json(resp)) =
        memory::delete_core(state, user, headers, space_ref, memory_id).await;
    owner_json_response_to_envelope("spaces.memory.delete", user, status, resp)
}
