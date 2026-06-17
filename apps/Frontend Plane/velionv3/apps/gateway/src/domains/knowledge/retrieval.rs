use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState, domains::knowledge::shared, middleware::AuthenticatedUser,
    upstream::proxy_json,
};

pub(super) async fn search_knowledge(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/v1/knowledge/search", state.retrieval_engine_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn get_retrieval_trace(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(trace_id): Path<String>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!(
        "{}/v1/retrieval/{}",
        state.retrieval_engine_url,
        urlencoding::encode(&trace_id)
    );
    proxy_json(
        &state,
        Method::GET,
        &url,
        None,
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn resolve_sources(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/v1/retrieve/sources", state.retrieval_engine_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn expand_chunks(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/v1/retrieve/chunks", state.retrieval_engine_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn graph_retrieve(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/v1/retrieve/graph", state.retrieval_engine_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}

pub(super) async fn wiki_retrieve(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let org_id = shared::org_id_from_headers(&headers);
    let url = format!("{}/v1/retrieve/wiki", state.retrieval_engine_url);
    proxy_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        org_id.as_deref(),
        Some(&shared::actor_for(&user)),
        None,
    )
    .await
}
