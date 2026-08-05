use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

pub(super) async fn list_connections(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/api/v1/connections", state.integration_core_url);
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(super) async fn get_connection(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/connections/{}",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(super) async fn disconnect(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/connections/{}",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::DELETE, &url, None).await
}

pub(super) async fn trigger_sync(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/connections/{}/sync",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::POST, &url, None).await
}

pub(super) async fn trigger_inbox_sync(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/connections/{}/inbox-sync",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::POST, &url, Some(body)).await
}

pub(super) async fn extend_inbox_history(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/connections/{}/inbox-history",
        state.integration_core_url,
        urlencoding::encode(&id)
    );
    proxy_for_user(&state, &user, &headers, Method::POST, &url, None).await
}
