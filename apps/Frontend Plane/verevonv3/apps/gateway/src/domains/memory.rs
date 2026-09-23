//! `/api/v1/memory` — per-user durable memory management ("what do you
//! remember about me"), with per-item delete and correct.
//!
//! Thin authenticated proxy from the SPA to model-gateway's memory read/write
//! surface (`GET`/`POST /v1/memories` + `DELETE /v1/memories/{memory_id}` →
//! session-core `MemoryService.ListMemory`/`IndexMemory`/`DeleteMemory`).
//! Model-gateway names its hop plural (`/v1/memories`) to avoid colliding with
//! the unrelated `/v1/memory` capability-core proxy it already serves; the
//! public path exposed here stays singular (`/api/v1/memory`) to match this
//! BFF's existing settings-domain naming.
//!
//! Org and user scope are resolved server-side from the validated session —
//! the browser never picks whose memories it reads, deletes, or corrects.
//! session-core independently re-derives and enforces both from the verified
//! `x-session-authorization` bearer's claims (see `MemoryGrpc::list_memory`/
//! `index_memory`/`delete_memory` in session-core), so a client cannot widen
//! scope by tampering with anything sent here. A Zero Data Retention caller
//! gets an empty list / a no-op delete from session-core, never an error —
//! this BFF layer does not need its own ZDR branch, it just forwards the
//! response.
//!
//! There is no update-in-place RPC on session-core's `MemoryService` (only
//! `ListMemory`/`IndexMemory`/`DeleteMemory`), so "correct" (`PATCH`) is a
//! create-then-delete: index a fresh "USER"-topic entry with the corrected
//! content through the same write path the Dreaming extractor itself uses,
//! then delete the original by id. The new entry is created FIRST so a
//! failure partway through leaves the corrected fact present (a harmless
//! duplicate) rather than losing it outright.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, patch},
    Extension, Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json_with_session, session_token},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/memory", get(list_memory))
        .route(
            "/api/v1/memory/{id}",
            patch(correct_memory).delete(delete_memory),
        )
        // Per-org/user rate limiting, ordered like `agents_runs.rs`:
        // `require_session` (written last → outer) runs first and inserts
        // `AuthenticatedUser`, so `rate_limit_middleware` (written first →
        // inner) keys by the validated org/user rather than client IP.
        .route_layer(axum::middleware::from_fn(rate_limit_middleware))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

#[derive(Debug, Default, Deserialize)]
struct MemoryQuery {
    limit: Option<u32>,
}

async fn list_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(query): Query<MemoryQuery>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;
    let url = match query.limit {
        Some(limit) => format!("{}/v1/memories?limit={limit}", state.model_gateway_url),
        None => format!("{}/v1/memories", state.model_gateway_url),
    };
    let (status, body) = proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorrectMemoryBody {
    /// The current chat thread the correction was made from. Only used to
    /// satisfy `IndexMemory`'s thread-ownership check — the resulting
    /// "USER"-topic entry is not scoped to it and appears in every thread's
    /// recall, same as the entry it replaces.
    thread_id: String,
    content: String,
}

/// "Rediger" — corrects a single recalled memory's content in place from the
/// chat memory panel.
///
/// See the module doc for why this is create-then-delete rather than a real
/// update: the corrected entry is indexed under a NEW id, so a caller must
/// swap `memory_id` in whatever list it is rendering rather than expecting
/// the original id back.
async fn correct_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(memory_id): Path<String>,
    Json(body): Json<CorrectMemoryBody>,
) -> impl IntoResponse {
    let thread_id = body.thread_id.trim();
    if thread_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "thread_id is required"})),
        )
            .into_response();
    }
    let content = body.content.trim();
    if content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "content is required"})),
        )
            .into_response();
    }

    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;

    let create_url = format!("{}/v1/memories", state.model_gateway_url);
    let (create_status, Json(create_body)) = proxy_model_json_with_session(
        &state,
        Method::POST,
        &create_url,
        Some(json!({ "thread_id": thread_id, "content": content })),
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    if !create_status.is_success() {
        return (create_status, Json(create_body)).into_response();
    }
    let new_memory_id = create_body
        .get("memory_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();

    let delete_url = format!(
        "{}/v1/memories/{}",
        state.model_gateway_url,
        urlencoding::encode(&memory_id)
    );
    let (delete_status, Json(delete_body)) = proxy_model_json_with_session(
        &state,
        Method::DELETE,
        &delete_url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    // The corrected fact is already saved even if this cleanup step fails —
    // report the new id either way so the caller can show the correction, and
    // surface whether the stale original was actually removed rather than
    // silently claiming success.
    let deleted = delete_status.is_success()
        && delete_body
            .get("deleted")
            .and_then(Value::as_bool)
            .unwrap_or(false);

    Json(json!({
        "memory_id": new_memory_id,
        "deleted": deleted,
        "degraded": create_body.get("degraded").cloned().unwrap_or(json!(false)),
        "degradation_reason": create_body
            .get("degradation_reason")
            .cloned()
            .unwrap_or(json!("")),
    }))
    .into_response()
}

pub(crate) async fn delete_memory(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(memory_id): Path<String>,
) -> impl IntoResponse {
    let token = model_token(&state, &user, &headers).await;
    let session_token = session_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/memories/{}",
        state.model_gateway_url,
        urlencoding::encode(&memory_id)
    );
    let (status, body) = proxy_model_json_with_session(
        &state,
        Method::DELETE,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await;
    (status, body).into_response()
}
