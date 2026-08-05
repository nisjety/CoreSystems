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
    let data_plane_token = shared::data_plane_token(&state, &user, &headers).await;
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let execution_token = match shared::required_execution_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let cost_token = match shared::required_cost_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let session_token = match shared::required_session_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(error) => return shared::delegated_auth_unavailable(error),
    };
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let org_name = crate::domains::auth::resolve_org_name(&state, &user, &org_id).await;
    let url = format!("{}/v1/invoke", state.model_gateway_url);
    let outbound_body = shared::normalized_model_body(body, &headers);
    let mut outbound_body = super::support::enrich_model_body(&state, &user, outbound_body).await;
    if let Err(message) = super::history::enforce_support_thread_policy(&mut outbound_body) {
        return shared::invalid_chat_request(message);
    }
    let outbound_body = shared::with_identity_context(outbound_body, &user.user_name, &org_name);
    shared::proxy_model_json_with_data_plane(
        &state,
        Method::POST,
        &url,
        Some(outbound_body),
        token.as_deref(),
        data_plane_token.as_deref(),
        Some(&inference_token),
        Some(&execution_token),
        Some(&cost_token),
        Some(&session_token),
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
    let session_token = shared::session_token(&state, &user, &headers).await;
    let url = format!(
        "{}/v1/threads/{}/messages",
        state.model_gateway_url,
        urlencoding::encode(&thread_id),
    );
    shared::proxy_model_json_with_session(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        session_token.as_deref(),
        &user,
    )
    .await
}

pub(super) async fn list_models(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let token = shared::model_token(&state, &user, &headers).await;
    // model-gateway's /v1/models proxies to inference-core ListModels, whose
    // handler requires the delegated inference credential (VerifiedInferenceBearer).
    // Forwarding only the model-gateway token 401s the catalog, so the model
    // selector renders empty for EVERY model. Mint + forward the inference bearer.
    let inference_token = match shared::required_inference_token(&state, &user, &headers).await {
        Ok(token) => token,
        Err(detail) => return shared::delegated_auth_unavailable(detail),
    };
    let url = format!("{}/v1/models", state.model_gateway_url);
    shared::proxy_model_json_with_inference(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        Some(&inference_token),
        &user,
    )
    .await
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
