use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{config::AppState, upstream::proxy_auth};

use super::shared::cookie_header;

pub(super) async fn sign_up(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/sign-up/email", state.auth_core_url);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie_header(&headers)),
    )
    .await
}

pub(super) async fn sign_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/sign-in/email", state.auth_core_url);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie_header(&headers)),
    )
    .await
}

pub(super) async fn verify_two_factor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v2/auth/2fa/verify", state.auth_core_url);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie_header(&headers)),
    )
    .await
}

pub(super) async fn sign_out(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let url = format!("{}/api/auth/sign-out", state.auth_core_url);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        None,
        Some(&cookie_header(&headers)),
    )
    .await
}

pub(super) async fn get_auth_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let url = format!("{}/api/auth/get-session", state.auth_core_url);
    proxy_auth(
        &state,
        Method::GET,
        &url,
        None,
        Some(&cookie_header(&headers)),
    )
    .await
}

pub(super) async fn send_email_verification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/send-verification-email", state.auth_core_url);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie_header(&headers)),
    )
    .await
}

pub(super) async fn verify_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/verify-email", state.auth_core_url);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie_header(&headers)),
    )
    .await
}

pub(super) async fn check_password_strength(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let url = format!(
        "{}/api/v2/auth/password/check-strength",
        state.auth_core_url
    );
    proxy_auth(&state, Method::POST, &url, Some(body), None).await
}

pub(super) async fn send_password_reset(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/forget-password", state.auth_core_url);
    proxy_auth(&state, Method::POST, &url, Some(body), None).await
}

pub(super) async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/reset-password", state.auth_core_url);
    proxy_auth(&state, Method::POST, &url, Some(body), None).await
}

#[derive(Deserialize)]
pub(super) struct OAuthInitiateQuery {
    #[serde(rename = "callbackURL")]
    callback_url: Option<String>,
}

/// Begin a social (OAuth) sign-in.
///
/// Same-origin BFF: the browser must NEVER be redirected to auth-core's internal
/// hostname. Better Auth's `sign-in/social` is a POST that returns the provider's
/// public authorize URL (`{ url, redirect }`); we call it server-side and redirect
/// the browser to that public URL. `callbackURL` (where Better Auth lands the user
/// after the provider round-trip) is forwarded from the client so it stays on the
/// SPA origin.
pub(super) async fn oauth_initiate(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(query): Query<OAuthInitiateQuery>,
) -> Response {
    let url = format!("{}/api/auth/sign-in/social", state.auth_core_url);
    let mut body = serde_json::json!({ "provider": provider });
    if let Some(callback_url) = query.callback_url.filter(|value| !value.is_empty()) {
        body["callbackURL"] = Value::String(callback_url);
    }

    let response = match state.client.post(&url).json(&body).send().await {
        Ok(response) => response,
        Err(err) => {
            tracing::error!(%provider, error = %err, "oauth initiate: auth-core unreachable");
            return (StatusCode::BAD_GATEWAY, "oauth provider unavailable").into_response();
        }
    };

    let payload = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(err) => {
            tracing::error!(%provider, error = %err, "oauth initiate: invalid auth-core response");
            return (StatusCode::BAD_GATEWAY, "oauth provider error").into_response();
        }
    };

    match payload.get("url").and_then(Value::as_str) {
        Some(authorize_url) => Redirect::temporary(authorize_url).into_response(),
        None => {
            tracing::error!(%provider, %payload, "oauth initiate: missing provider url");
            (StatusCode::BAD_GATEWAY, "oauth provider error").into_response()
        }
    }
}
