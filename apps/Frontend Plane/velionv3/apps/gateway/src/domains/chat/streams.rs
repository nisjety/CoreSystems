use axum::{
    extract::{Extension, Path, State},
    http::HeaderMap,
    response::Response,
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    config::AppState, domains::chat::shared, middleware::AuthenticatedUser,
    upstream::proxy_sse_stream,
};

pub(super) async fn stream_chat(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let url = format!("{}/v1/invoke/stream", state.model_gateway_url);
    proxy_sse_stream(
        &state,
        Method::POST,
        &url,
        Some(body),
        token.as_deref(),
        None,
        Some((&user.user_id, org_id.as_str())),
        shared::zdr_flag(&headers),
    )
    .await
}

pub(super) async fn resume_stream(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let last_event_id = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let url = format!(
        "{}/v1/invoke/resume/{}",
        state.model_gateway_url,
        urlencoding::encode(&request_id),
    );
    proxy_sse_stream(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        last_event_id.as_deref(),
        Some((&user.user_id, org_id.as_str())),
        false,
    )
    .await
}

pub(super) async fn run_events_stream(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
) -> Response {
    let token = shared::model_token(&state, &user, &headers).await;
    let org_id = crate::upstream::authorized_org_id(&state, &user).await;
    let url = format!(
        "{}/v1/runs/{}/events",
        state.model_gateway_url,
        urlencoding::encode(&run_id),
    );
    proxy_sse_stream(
        &state,
        Method::GET,
        &url,
        None,
        token.as_deref(),
        None,
        Some((&user.user_id, org_id.as_str())),
        false,
    )
    .await
}
