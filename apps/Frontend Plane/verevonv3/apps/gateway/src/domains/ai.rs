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
        .route("/api/v1/ai/dictate", post(proxy_dictate))
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

/// Verevon Flow dictation: STT + LLM cleanup in one round trip. Proxies to
/// model-gateway /v1/ai/dictate, which chains inference-core TranscribeSpeech
/// and a cleanup Infer pass; both hops need the delegated inference credential.
pub(crate) async fn proxy_dictate(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(detail) => return shared::delegated_auth_unavailable(detail),
    };
    let url = format!("{}/v1/ai/dictate", state.model_gateway_url);
    shared::proxy_model_json_with_inference(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&inference_token),
        &user,
    )
    .await
}

pub(crate) async fn proxy_speech(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    // model-gateway's /v1/ai/speech (both TTS synthesis and STT transcription)
    // proxies to inference-core and requires the delegated inference credential
    // (VerifiedInferenceBearer). Forwarding only the model-gateway token 401s,
    // so mint + forward the inference bearer like the chat / models paths.
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(detail) => return shared::delegated_auth_unavailable(detail),
    };
    let url = format!("{}/v1/ai/speech", state.model_gateway_url);
    shared::proxy_model_json_with_inference(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        Some(&inference_token),
        &user,
    )
    .await
}
