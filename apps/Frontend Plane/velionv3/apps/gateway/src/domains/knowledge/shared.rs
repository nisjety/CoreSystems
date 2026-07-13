use std::time::Duration;

use axum::http::{HeaderMap, Uri};
use reqwest::Method;
use serde_json::Value;

use crate::{
    audience_tokens::get_audience_token, config::AppState, contracts::ActionActor,
    middleware::AuthenticatedUser,
};

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

pub(super) fn qs(uri: &Uri) -> String {
    uri.query()
        .filter(|q| !q.is_empty())
        .map(|q| format!("?{q}"))
        .unwrap_or_default()
}

pub(super) fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

pub(super) async fn quarry_token(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "quarry").await
}

pub(super) async fn ingestion_token(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "ingestion").await
}

/// Mint the `data-plane` audience token for the current user. Attached as a
/// Bearer on Data Plane legs (documents-api, retrieval-engine, graph-index) so
/// documents-api can verify tenant identity once `AUTHCTX_ENFORCE=1`. Cached
/// per user+audience; `None` when auth-core is unreachable, in which case the
/// legs degrade to the internal-key + header path (works while enforce is off).
pub(super) async fn data_plane_token(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "data-plane").await
}

/// Build an internal service-to-service request (internal API key + actor +
/// optional org header), used by the knowledge aggregators to fan out across
/// Data Plane v2 / integration-core / finspo with per-call timeouts.
fn internal_request(
    state: &AppState,
    method: Method,
    url: &str,
    org_id: Option<&str>,
    actor: &ActionActor,
    timeout: Duration,
    bearer: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut req = state
        .client
        .request(method, url)
        .timeout(timeout)
        .header("x-internal-api-key", &state.internal_api_key)
        .header("x-user-id", actor.user_id.as_str());
    if !actor.user_email.is_empty() {
        req = req.header("x-user-email", actor.user_email.as_str());
    }
    if !actor.user_name.is_empty() {
        req = req.header("x-user-name", actor.user_name.as_str());
    }
    if let Some(org) = org_id.filter(|v| !v.trim().is_empty()) {
        req = req.header("x-org-id", org.trim());
    }
    // Data Plane legs additionally carry a verified `data-plane` audience
    // token so documents-api can enforce tenant identity from the signed
    // `org_id` claim (cross-checked against x-org-id) once enforce is on.
    if let Some(token) = bearer.filter(|t| !t.is_empty()) {
        req = req.bearer_auth(token);
    }
    req
}

/// Fire an internal request and return the parsed JSON body, or `None` on any
/// transport / non-2xx / non-JSON outcome. Aggregators degrade gracefully so one
/// slow or down upstream never fails the whole response.
pub(super) async fn fetch_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: &ActionActor,
    timeout: Duration,
) -> Option<Value> {
    fetch_json_bearer(state, method, url, body, org_id, actor, timeout, None).await
}

/// Like [`fetch_json`] but attaches a Bearer token (used for Data Plane legs
/// that must present a verified `data-plane` audience token).
#[allow(clippy::too_many_arguments)]
pub(super) async fn fetch_json_bearer(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: &ActionActor,
    timeout: Duration,
    bearer: Option<&str>,
) -> Option<Value> {
    let mut req = internal_request(state, method, url, org_id, actor, timeout, bearer);
    if let Some(body) = body {
        req = req.json(&body);
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok(),
        _ => None,
    }
}

/// Fire an internal request and report only whether it succeeded (2xx),
/// ignoring the response body. Used for fire-and-confirm sync triggers where
/// the upstream may return an empty body on success.
pub(super) async fn request_ok(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: &ActionActor,
    timeout: Duration,
) -> bool {
    let mut req = internal_request(state, method, url, org_id, actor, timeout, None);
    if let Some(body) = body {
        req = req.json(&body);
    }
    matches!(req.send().await, Ok(resp) if resp.status().is_success())
}
