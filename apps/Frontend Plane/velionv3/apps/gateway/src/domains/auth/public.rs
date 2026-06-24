use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::AppState,
    upstream::{browser_origin, proxy_auth, proxy_auth_with_headers},
};

use super::shared::cookie_header;

pub(super) async fn sign_up(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/sign-up/email", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    let extra_headers = captcha_response_headers(&headers);
    proxy_auth_with_headers(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        origin.as_deref(),
        &extra_headers,
    )
    .await
}

pub(super) async fn sign_in(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/sign-in/email", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn verify_two_factor(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v2/auth/2fa/verify", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn sign_out(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let url = format!("{}/api/auth/sign-out", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        None,
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

#[derive(Deserialize)]
pub(super) struct GetSessionQuery {
    /// When `true`, ask Better Auth to bypass its session-data cookie-cache and
    /// read the session fresh from storage. The SPA passes this right after
    /// changing the active org so `activeOrganizationId` reflects instantly
    /// instead of serving a stale cached value for the cookie-cache TTL.
    #[serde(rename = "disableCookieCache")]
    disable_cookie_cache: Option<String>,
}

pub(super) async fn get_auth_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GetSessionQuery>,
) -> Response {
    let mut url = format!("{}/api/auth/get-session", state.auth_core_url);
    if query.disable_cookie_cache.as_deref() == Some("true") {
        url.push_str("?disableCookieCache=true");
    }
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::GET,
        &url,
        None,
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn send_email_verification(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/send-verification-email", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn verify_email(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/verify-email", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn send_email_verification_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v2/auth/otp/email/send", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(email_verification_otp_body(body)),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn verify_email_verification_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v2/auth/otp/email/verify", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(email_verification_otp_body(body)),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn send_phone_verification_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v2/auth/otp/sms/send", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

pub(super) async fn verify_phone_verification_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/v2/auth/otp/sms/verify", state.auth_core_url);
    let cookie = cookie_header(&headers);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        origin.as_deref(),
    )
    .await
}

fn email_verification_otp_body(mut body: Value) -> Value {
    if let Value::Object(ref mut payload) = body {
        payload.insert(
            "type".to_owned(),
            Value::String("email-verification".to_owned()),
        );
        return body;
    }

    serde_json::json!({ "type": "email-verification" })
}

fn captcha_response_headers(headers: &HeaderMap) -> Vec<(&'static str, String)> {
    headers
        .get("x-captcha-response")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![("x-captcha-response", value.to_owned())])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{captcha_response_headers, email_verification_otp_body};
    use axum::http::{HeaderMap, HeaderValue};
    use serde_json::json;

    #[test]
    fn email_verification_otp_body_forces_email_verification_type() {
        let body = email_verification_otp_body(json!({
            "email": "ima@example.com",
            "otp": "313117",
            "type": "sign-in"
        }));

        assert_eq!(
            body,
            json!({
                "email": "ima@example.com",
                "otp": "313117",
                "type": "email-verification"
            })
        );
    }

    #[test]
    fn captcha_response_headers_forwards_only_non_empty_captcha_response() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-captcha-response",
            HeaderValue::from_static(" turnstile-token "),
        );

        assert_eq!(
            captcha_response_headers(&headers),
            vec![("x-captcha-response", "turnstile-token".to_owned())]
        );

        let mut blank = HeaderMap::new();
        blank.insert("x-captcha-response", HeaderValue::from_static("   "));
        assert!(captcha_response_headers(&blank).is_empty());
    }
}

pub(super) async fn check_password_strength(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!(
        "{}/api/v2/auth/password/check-strength",
        state.auth_core_url
    );
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        None,
        origin.as_deref(),
    )
    .await
}

pub(super) async fn send_password_reset(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/request-password-reset", state.auth_core_url);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        None,
        origin.as_deref(),
    )
    .await
}

pub(super) async fn reset_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let url = format!("{}/api/auth/reset-password", state.auth_core_url);
    let origin = browser_origin(&headers);
    proxy_auth(
        &state,
        Method::POST,
        &url,
        Some(body),
        None,
        origin.as_deref(),
    )
    .await
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
    headers: HeaderMap,
    Path(provider): Path<String>,
    Query(query): Query<OAuthInitiateQuery>,
) -> Response {
    let url = format!("{}/api/auth/sign-in/social", state.auth_core_url);
    let mut body = serde_json::json!({ "provider": provider });
    if let Some(callback_url) = query.callback_url.filter(|value| !value.is_empty()) {
        body["callbackURL"] = Value::String(callback_url);
    }

    let mut request = state.client.post(&url).json(&body);
    if let Some(origin) = browser_origin(&headers) {
        request = request.header("origin", origin);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            tracing::error!(%provider, error = %err, "oauth initiate: auth-core unreachable");
            return (StatusCode::BAD_GATEWAY, "oauth provider unavailable").into_response();
        }
    };

    // Better Auth sets the OAuth `state` cookie (e.g. `idknuten.state`) on this
    // response. Capture it BEFORE consuming the body and forward it onto the
    // browser redirect — otherwise the browser never receives it and the
    // provider callback fails state verification (state_mismatch).
    let set_cookies = collect_set_cookies(response.headers());

    let payload = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(err) => {
            tracing::error!(%provider, error = %err, "oauth initiate: invalid auth-core response");
            return (StatusCode::BAD_GATEWAY, "oauth provider error").into_response();
        }
    };

    match payload.get("url").and_then(Value::as_str) {
        Some(authorize_url) => redirect_with_cookies(authorize_url, &set_cookies),
        None => {
            tracing::error!(%provider, %payload, "oauth initiate: missing provider url");
            (StatusCode::BAD_GATEWAY, "oauth provider error").into_response()
        }
    }
}

#[derive(Deserialize)]
pub(super) struct SsoInitiateQuery {
    /// Either an email (provider resolved by domain) or an org domain. The
    /// Better Auth SSO plugin accepts `email` and resolves the registered
    /// provider for that domain; we forward whichever the SPA supplies.
    email: Option<String>,
    domain: Option<String>,
    #[serde(rename = "callbackURL")]
    callback_url: Option<String>,
}

/// Begin an enterprise SSO (SAML/OIDC) sign-in.
///
/// Mirrors `oauth_initiate`: Better Auth's SSO plugin `sign-in/sso` is a POST
/// that resolves the IdP for the supplied email domain and returns the public
/// IdP authorize URL (`{ url, redirect }`). We call it server-side and redirect
/// the browser to that public URL so the session cookie stays first-party.
/// `callbackURL` is kept on the SPA origin (the onboarding-gated router decides
/// the landing route once the IdP round-trip completes).
pub(super) async fn sso_initiate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SsoInitiateQuery>,
) -> Response {
    let email = query.email.filter(|value| !value.trim().is_empty());
    let domain = query.domain.filter(|value| !value.trim().is_empty());
    if email.is_none() && domain.is_none() {
        return (StatusCode::BAD_REQUEST, "sso requires an email or domain").into_response();
    }

    let url = format!("{}/api/auth/sign-in/sso", state.auth_core_url);
    let mut body = serde_json::json!({});
    if let Some(email) = email {
        body["email"] = Value::String(email.trim().to_owned());
    }
    if let Some(domain) = domain {
        body["domain"] = Value::String(domain.trim().to_owned());
    }
    if let Some(callback_url) = query.callback_url.filter(|value| !value.is_empty()) {
        body["callbackURL"] = Value::String(callback_url);
    }

    let mut request = state.client.post(&url).json(&body);
    if let Some(origin) = browser_origin(&headers) {
        request = request.header("origin", origin);
    }

    let response = match request.send().await {
        Ok(response) => response,
        Err(err) => {
            tracing::error!(error = %err, "sso initiate: auth-core unreachable");
            return (StatusCode::BAD_GATEWAY, "sso provider unavailable").into_response();
        }
    };

    let status =
        StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    // Forward Better Auth's SSO `state` cookie onto the browser redirect (same
    // reason as oauth_initiate — without it the IdP callback fails state
    // verification). Captured before the body is consumed.
    let set_cookies = collect_set_cookies(response.headers());
    let payload = match response.json::<Value>().await {
        Ok(payload) => payload,
        Err(err) => {
            tracing::error!(error = %err, "sso initiate: invalid auth-core response");
            return (StatusCode::BAD_GATEWAY, "sso provider error").into_response();
        }
    };

    // No registered IdP for that domain (or SSO disabled): surface the upstream
    // status so the SPA can tell the user no SSO connection exists, instead of a
    // confusing redirect attempt.
    if !status.is_success() {
        return (status, "no sso provider for that domain").into_response();
    }

    match payload.get("url").and_then(Value::as_str) {
        Some(authorize_url) => redirect_with_cookies(authorize_url, &set_cookies),
        None => {
            tracing::error!(%payload, "sso initiate: missing provider url");
            (StatusCode::BAD_GATEWAY, "sso provider error").into_response()
        }
    }
}

/// Collect Better Auth's `Set-Cookie` headers from a server-side initiate
/// response so they can be replayed onto the browser redirect. The OAuth/SSO
/// `state` cookie lives here and MUST reach the browser, or the provider
/// callback fails state verification (`state_mismatch`).
fn collect_set_cookies(headers: &reqwest::header::HeaderMap) -> Vec<Vec<u8>> {
    headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .map(|value| value.as_bytes().to_vec())
        .collect()
}

/// Build a temporary redirect to `location` that also re-emits the captured
/// `Set-Cookie` headers to the browser.
fn redirect_with_cookies(location: &str, set_cookies: &[Vec<u8>]) -> Response {
    let mut response = Redirect::temporary(location).into_response();
    let headers = response.headers_mut();
    for cookie in set_cookies {
        if let Ok(value) = axum::http::HeaderValue::from_bytes(cookie) {
            headers.append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
}
