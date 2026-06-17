use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::IntoResponse,
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{config::AppState, domains::chat::shared, middleware::AuthenticatedUser};

pub(super) async fn invoke_chat(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/invoke", state.model_gateway_url);
    shared::proxy_model_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        &user,
    )
    .await
}

pub(super) async fn cancel_invocation(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/invoke/{}/cancel",
        state.model_gateway_url,
        urlencoding::encode(&request_id),
    );
    shared::proxy_model_json(&state, Method::POST, &url, None, token.as_deref(), &user).await
}

pub(super) async fn get_thread_messages(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/threads/{}/messages",
        state.model_gateway_url,
        urlencoding::encode(&thread_id),
    );
    shared::proxy_model_json(&state, Method::GET, &url, None, token.as_deref(), &user).await
}

pub(super) async fn list_models(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/models", state.model_gateway_url);
    shared::proxy_model_json(&state, Method::GET, &url, None, token.as_deref(), &user).await
}

pub(super) async fn submit_feedback(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/feedback", state.model_gateway_url);
    shared::proxy_model_json(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        &user,
    )
    .await
}
