use axum::{
    body::to_bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    config::AppState,
    upstream::{browser_origin, proxy_auth, proxy_auth_callback, proxy_auth_with_headers},
};

use super::shared::cookie_header;

const AUTH_CALLBACK_MAX_QUERY_BYTES: usize = 16 * 1024;
const SAML_CALLBACK_MAX_BODY_BYTES: usize = 64 * 1024;
const AUTH_PROVIDER_ID_MAX_BYTES: usize = 128;

pub(super) async fn oauth_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    proxy_get_callback(&state, &provider, "api/auth/callback", request).await
}

pub(super) async fn sso_oidc_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    proxy_get_callback(&state, &provider, "api/auth/sso/callback", request).await
}

pub(super) async fn sso_saml_callback_get(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    proxy_get_callback(&state, &provider, "api/auth/sso/saml2/callback", request).await
}

pub(super) async fn sso_saml_callback_post(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    request: Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let Some(url) = auth_callback_url(
        &state.auth_core_url,
        "api/auth/sso/saml2/callback",
        &provider,
        parts.uri.query(),
    ) else {
        return (StatusCode::BAD_REQUEST, "invalid auth callback").into_response();
    };
    let content_type = parts
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();
    if !content_type.split(';').next().is_some_and(|value| {
        value
            .trim()
            .eq_ignore_ascii_case("application/x-www-form-urlencoded")
    }) {
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "invalid SAML callback content type",
        )
            .into_response();
    }
    let body = match to_bytes(body, SAML_CALLBACK_MAX_BODY_BYTES).await {
        Ok(body) => body,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "SAML callback body is too large",
            )
                .into_response()
        }
    };
    let cookie = cookie_header(&parts.headers);
    proxy_auth_callback(
        &state,
        Method::POST,
        &url,
        Some(body),
        Some(&cookie),
        Some("application/x-www-form-urlencoded"),
    )
    .await
}

async fn proxy_get_callback(
    state: &AppState,
    provider: &str,
    path: &str,
    request: Request,
) -> Response {
    let (parts, _) = request.into_parts();
    let Some(url) = auth_callback_url(&state.auth_core_url, path, provider, parts.uri.query())
    else {
        return (StatusCode::BAD_REQUEST, "invalid auth callback").into_response();
    };
    let cookie = cookie_header(&parts.headers);
    proxy_auth_callback(state, Method::GET, &url, None, Some(&cookie), None).await
}

fn auth_callback_url(
    auth_core_url: &str,
    path: &str,
    provider: &str,
    query: Option<&str>,
) -> Option<String> {
    if provider.is_empty()
        || provider.len() > AUTH_PROVIDER_ID_MAX_BYTES
        || !provider
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        || query.is_some_and(|value| value.len() > AUTH_CALLBACK_MAX_QUERY_BYTES)
    {
        return None;
    }

    let mut url = format!("{}/{path}/{provider}", auth_core_url.trim_end_matches('/'));
    if let Some(query) = query.filter(|value| !value.is_empty()) {
        url.push('?');
        url.push_str(query);
    }
    Some(url)
}

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
