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
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    upstream::{authorized_org_id, proxy_bearer_json, proxy_integration_json, proxy_sse_stream},
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

/// As [`proxy_for_user`], but an upstream refusal comes back exactly as Integration Core wrote it. The standard
/// path collapses a failed envelope into `integration_error` or `PLAN_REQUIRED`, which would hide the typed
/// refusals the ZDR acknowledgement routes depend on (`not_connection_owner`, `zdr_assessment_changed`,
/// `zdr_org_state_unavailable`). A success is unwrapped into the gateway's `{ data }` envelope as usual. The
/// caller's own audience-bound bearer is the only credential sent; there is no internal-key path.
pub(super) async fn proxy_typed_for_user(
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
    let (status, Json(raw)) =
        proxy_bearer_json(state, method, url, body, Some(&token), &user.user_id).await;
    if status.is_success() && raw.get("success").and_then(Value::as_bool) == Some(true) {
        let data = raw.get("data").cloned().unwrap_or_else(|| raw.clone());
        return (status, Json(ok(data)));
    }
    (status, Json(raw))
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
