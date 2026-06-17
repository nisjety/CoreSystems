use axum::{
    extract::{Extension, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState,
    domains::chat::shared,
    middleware::{require_session, AuthenticatedUser},
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/ai/speech", post(proxy_speech))
        .route("/api/v1/ai/transcribe", post(proxy_speech))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

async fn proxy_speech(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let url = format!("{}/v1/ai/speech", state.model_gateway_url);
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
