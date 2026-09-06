use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{config::AppState, middleware::AuthenticatedUser};

use super::shared::proxy_for_user;

/// Starts the official Codex device-code flow. The browser receives only the
/// short-lived verification URL/code and an opaque connection reference; the
/// subscription credential remains inside Integration Core.
pub(crate) async fn start_openai_codex_subscription(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/model-subscriptions/openai-codex/connect",
        state.integration_core_url
    );
    proxy_for_user(
        &state,
        &user,
        &headers,
        Method::POST,
        &url,
        Some(if body.is_object() { body } else { json!({}) }),
    )
    .await
}

pub(crate) async fn openai_codex_subscription_status(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path((connection_id, login_id)): Path<(String, String)>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/model-subscriptions/openai-codex/connect/{}/{}",
        state.integration_core_url,
        urlencoding::encode(&connection_id),
        urlencoding::encode(&login_id),
    );
    proxy_for_user(&state, &user, &headers, Method::GET, &url, None).await
}

pub(crate) async fn disconnect_openai_codex_subscription(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(connection_id): Path<String>,
) -> impl axum::response::IntoResponse {
    let url = format!(
        "{}/api/v1/model-subscriptions/openai-codex/connections/{}",
        state.integration_core_url,
        urlencoding::encode(&connection_id),
    );
    proxy_for_user(&state, &user, &headers, Method::DELETE, &url, None).await
}
