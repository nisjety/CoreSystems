//! `/api/v1/memory` — per-user durable memory management ("what do you
//! remember about me"), with per-item delete.
//!
//! Thin authenticated proxy from the SPA to model-gateway's memory read/write
//! surface (`GET /v1/memories` + `DELETE /v1/memories/{memory_id}` →
//! session-core `MemoryService.ListMemory`/`DeleteMemory`). Model-gateway
//! names its hop plural (`/v1/memories`) to avoid colliding with the
//! unrelated `/v1/memory` capability-core proxy it already serves; the public
//! path exposed here stays singular (`/api/v1/memory`) to match this BFF's
//! existing settings-domain naming.
//!
//! Org and user scope are resolved server-side from the validated session —
//! the browser never picks whose memories it reads or deletes. session-core
//! independently re-derives and enforces both from the verified
//! `x-session-authorization` bearer's claims (see `MemoryGrpc::list_memory`/
//! `delete_memory` in session-core), so a client cannot widen scope by
//! tampering with anything sent here. A Zero Data Retention caller gets an
//! empty list / a no-op delete from session-core, never an error — this BFF
//! layer does not need its own ZDR branch, it just forwards the response.

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::{delete, get},
    Extension, Router,
};
use reqwest::Method;
use serde::Deserialize;

use crate::{
    config::AppState,
    domains::chat::shared::{model_token, proxy_model_json_with_session, session_token},
    middleware::{require_session, AuthenticatedUser},
    rate_limit::rate_limit_middleware,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/memory", get(list_memory))
        .route("/api/v1/memory/{id}", delete(delete_memory))
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
