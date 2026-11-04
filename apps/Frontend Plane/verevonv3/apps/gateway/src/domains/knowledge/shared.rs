use std::time::Duration;

use axum::{
    http::{HeaderMap, StatusCode, Uri},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    audience_tokens::get_audience_token, config::AppState, contracts::ActionActor,
    middleware::AuthenticatedUser, upstream::proxy_user_bearer_json,
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
/// each service verifies tenant identity from signed claims. Cached per
/// user+audience; `None` when Auth Core is unreachable, which interactive Data
/// routes treat as a fail-closed 503.
pub(super) async fn data_plane_token(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "data-plane").await
}

/// Proxy an interactive Knowledge request to Data Plane v2 with the
/// session-bound `aud=data-plane` token. Token issuance failure is a hard 503:
/// falling back to shared-key/header identity would either fail strict auth or
/// accidentally turn a user read into a broader service-principal read.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_data_plane_json(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    content_type: Option<&str>,
) -> (StatusCode, Json<Value>) {
    let cookie = cookie_header(headers);
    let Some(token) = data_plane_token(state, user, &cookie).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "delegated_auth_unavailable",
                    "message": "A scoped Data Plane authorization token could not be minted."
                }
            })),
        );
    };
    let actor = actor_for(user);
    proxy_user_bearer_json(
        state,
        method,
        url,
        body,
        org_id,
        &actor,
        &token,
        content_type,
    )
    .await
}

/// As `proxy_data_plane_json`, carrying a Control-signed Space decision.
///
/// The decision is authority the Data Plane verifies for itself: it names the
/// Space, the recipient audience, the privacy policy and the resource
/// authorization, and retrieval-engine resolves its own binding from that. The
/// gateway therefore relays it verbatim and adds nothing — a gateway-shaped
/// Space claim would be exactly the forged scoping header this boundary
/// exists to refuse.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn proxy_data_plane_json_with_space_decision(
    state: &AppState,
    user: &AuthenticatedUser,
    headers: &HeaderMap,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    space_decision: &str,
) -> (StatusCode, Json<Value>) {
    let cookie = cookie_header(headers);
    let Some(token) = data_plane_token(state, user, &cookie).await else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": {
                    "code": "delegated_auth_unavailable",
                    "message": "A scoped Data Plane authorization token could not be minted."
                }
            })),
        );
    };
    let actor = actor_for(user);
    crate::upstream::proxy_user_bearer_json_with_extra_headers(
        state,
        method,
        url,
        body,
        org_id,
        &actor,
        &token,
        None,
        std::collections::BTreeMap::from([(
            "x-space-decision".to_owned(),
            space_decision.trim().to_owned(),
        )]),
    )
    .await
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
        .header("x-user-id", actor.user_id.as_str());
    if bearer.is_none() {
        req = req.header("x-internal-api-key", &state.internal_api_key);
    }
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

/// The gateway's typed refusal for a failed scrape whose upstream error is a
/// `DRIVER_FAILED` naming one of the fetch reasons Quarry adds to its waterfall
/// (E3.8.1, with `QUARRY_EDGE__FETCH_SUB_CODES` on): `dns`, `connect` or `tls`.
/// Returns the gateway code and a fixed sentence, both static, never upstream text.
///
/// Any other body returns `None`, so the status-based classification in
/// `knowledge/quarry.rs` still applies: `all_drivers_failed` stays `scrape_failed`,
/// and the E3.8.2 `TIMEOUT` reasons (`tier_stalled`, `fetch_deadline_exceeded`)
/// stay `scrape_timeout` on their 504.
///
/// `classify_scrape_failure` in `knowledge/quarry.rs` calls this first.
pub(super) fn fetch_sub_code_refusal(upstream: &Value) -> Option<(&'static str, &'static str)> {
    if upstream.pointer("/error/code").and_then(Value::as_str) != Some("DRIVER_FAILED") {
        return None;
    }
    match upstream.pointer("/error/details/reason")?.as_str()? {
        "dns" => Some((
            "scrape_host_not_found",
            "Vi fant ikke adressen til nettstedet. Sjekk lenken og prøv igjen.",
        )),
        "connect" => Some((
            "scrape_unreachable",
            "Nettstedet svarer ikke akkurat nå. Prøv igjen om litt, eller en annen kilde.",
        )),
        "tls" => Some((
            "scrape_tls_failed",
            "Det var ikke mulig å opprette en sikker forbindelse til nettstedet. Prøv en annen kilde.",
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn upstream(code: &str, reason: &str) -> Value {
        json!({
            "error": {
                "code": code,
                "message": "upstream text that is never shown",
                "details": { "reason": reason }
            }
        })
    }

    fn refusal(code: &str, reason: &str) -> Option<(&'static str, &'static str)> {
        fetch_sub_code_refusal(&upstream(code, reason))
    }

    fn gateway_code(code: &str, reason: &str) -> Option<&'static str> {
        refusal(code, reason).map(|(gateway, _)| gateway)
    }

    #[test]
    fn the_three_reasons_with_no_code_of_their_own_get_one() {
        assert_eq!(
            gateway_code("DRIVER_FAILED", "dns"),
            Some("scrape_host_not_found")
        );
        assert_eq!(
            gateway_code("DRIVER_FAILED", "connect"),
            Some("scrape_unreachable")
        );
        assert_eq!(
            gateway_code("DRIVER_FAILED", "tls"),
            Some("scrape_tls_failed")
        );
    }

    #[test]
    fn each_refusal_is_one_sentence_with_no_id_and_no_upstream_text() {
        for reason in ["dns", "connect", "tls"] {
            let (gateway, message) = refusal("DRIVER_FAILED", reason).expect(reason);
            assert!(message.ends_with('.'), "{gateway}");
            assert!(!message.contains('_'), "{gateway}: no internal id");
            assert!(!message.contains("upstream"), "{gateway}: no upstream text");
            assert!(!message.contains("http"), "{gateway}: no address");
        }
    }

    #[test]
    fn the_other_fetch_reasons_keep_the_status_classification() {
        // all_drivers_failed is scrape_failed on its 502, or scrape_timeout after 20 s, as before.
        assert_eq!(gateway_code("DRIVER_FAILED", "all_drivers_failed"), None);
        // The E3.8.2 reasons come on a 504 TIMEOUT, which is scrape_timeout already.
        assert_eq!(gateway_code("TIMEOUT", "tier_stalled"), None);
        assert_eq!(gateway_code("TIMEOUT", "fetch_deadline_exceeded"), None);
    }

    #[test]
    fn a_reason_is_read_only_on_a_driver_failed_and_only_as_an_exact_match() {
        assert_eq!(gateway_code("DRIVER_FAILED", "session_reaped"), None);
        assert_eq!(gateway_code("DRIVER_FAILED", "DNS"), None);
        assert_eq!(gateway_code("DRIVER_FAILED", " dns"), None);
        assert_eq!(gateway_code("CHALLENGE_DETECTED", "dns"), None);
        let bare = json!({ "error": { "code": "DRIVER_FAILED" } });
        assert_eq!(fetch_sub_code_refusal(&bare), None);
        assert_eq!(fetch_sub_code_refusal(&Value::Null), None);
    }
}
