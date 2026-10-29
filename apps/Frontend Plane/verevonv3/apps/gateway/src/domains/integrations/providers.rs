use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    Json,
};
use reqwest::Method;
use serde_json::Value;
use axum::response::IntoResponse;

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

pub(super) async fn list_providers(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl axum::response::IntoResponse {
    let url = format!("{}/api/v1/providers", state.integration_core_url);
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(super) async fn start_connect_session(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(provider): Path<String>,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    // Provider catalog identifiers are ASCII. Reject Unicode aliases before
    // Go's Unicode normalization can turn them into a privileged provider key.
    if !provider.is_ascii() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(crate::envelope::error("invalid_provider", "Choose a valid provider.")),
        ).into_response();
    }
    if provider.trim().eq_ignore_ascii_case("algolia") {
        return super::algolia::generic_connect(&state, &user, &headers, body).await;
    }
    let url = format!(
        "{}/api/v1/providers/{}/connect-session",
        state.integration_core_url,
        urlencoding::encode(&provider)
    );
    proxy_for_user(&state, &user, &headers, Method::POST, &url, Some(body)).await.into_response()
}
