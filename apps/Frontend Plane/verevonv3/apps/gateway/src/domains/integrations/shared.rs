use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::Value;

use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    envelope::error,
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_integration_json, proxy_sse_stream},
};

pub(super) async fn proxy_for_user(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
    body: Option<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let Some(token) = get_audience_token(state, &user.user_id, cookie, "ingestion").await else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "integration_auth_unavailable",
                "Integration authentication is temporarily unavailable.",
            )),
        );
    };
    proxy_integration_json(state, method, url, body, Some(&token), &user.user_id).await
}

pub(super) async fn proxy_sse_for_user(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
) -> Response {
    let cookie = headers
        .get("cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let Some(token) = get_audience_token(state, &user.user_id, cookie, "ingestion").await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(error(
                "integration_auth_unavailable",
                "Integration authentication is temporarily unavailable.",
            )),
        )
            .into_response();
    };
    let org_id = authorized_org_id(state, user).await;
    proxy_sse_stream(
        state,
        method,
        url,
        None,
        Some(&token),
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok()),
        Some((&user.user_id, &org_id)),
        false,
    )
    .await
}
