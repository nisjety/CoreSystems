//! Shared plumbing for the ingestions domain.
//!
//! All ingestion traffic targets **quarry-edge** (`state.quarry_edge_url`) — the
//! same seam the knowledge domain and verevonv2's BFF (`getQuarryEdgeUrl`) use.
//! Edge is the JWT-authenticated public face of Quarry-v2: it owns the
//! execution endpoints (`/v1/scrape|crawl|extract|batch`) and the browser
//! profiles, and it forwards durable registry reads/writes (jobs, schedules,
//! sources, run events) to quarry-control over HMAC-signed internal requests.
//! We do NOT call quarry-control directly: it requires HMAC signing the gateway
//! can't perform, exposes no JWT/org context, and lacks the execution +
//! profile endpoints this feature needs. (Verified against
//! `Quarry-v2/crates/quarry-edge/src/routes.rs` +
//! `services/quarry-control/cmd/control/main.go`.)
//!
//! Auth: edge accepts the short-lived `quarry` audience JWT the knowledge
//! domain mints. The token is minted server-side from the *validated* session
//! cookie, and edge derives org scope from the token's `claims.org_id` — so we
//! never read a client-supplied `x-verevon-org-id` header for quarry calls and
//! there is no IDOR vector to trust away. For the cross-plane calls the
//! `sources` aggregator makes (integration-core / documents-api / graph), the
//! org is resolved from user-core's session-context exactly like billing's
//! `authorized_org_id`, never from a client header.

use std::time::Duration;

use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    contracts::ActionActor,
    envelope::{error, ok},
    middleware::AuthenticatedUser,
    public_url::normalize_public_http_url,
    upstream::proxy_bearer_json,
};

/// Largest curated batch we hand quarry's `/v1/batch` in one request — quarry
/// documents "a few hundred URLs"; 200 keeps us comfortably inside that budget.
pub(super) const MAX_BATCH_URLS: usize = 200;

pub(super) fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

pub(super) fn actor_for(user: &AuthenticatedUser) -> ActionActor {
    ActionActor {
        user_id: user.user_id.clone(),
        user_email: user.user_email.clone(),
        user_name: user.user_name.clone(),
        user_role: user.auth_role.clone().unwrap_or_default(),
    }
}

/// Mint (or reuse) the cached `quarry` audience token for this user/session.
pub(super) async fn quarry_token(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "quarry").await
}

pub(super) async fn data_plane_token(
    state: &AppState,
    user: &AuthenticatedUser,
    cookie: &str,
) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "data-plane").await
}

/// Call quarry-edge with bearer auth and return the raw `(status, body)`.
/// `path` is appended to `quarry_edge_url` and may already carry a query string.
pub(super) async fn quarry_call(
    state: &AppState,
    method: Method,
    path: &str,
    body: Option<Value>,
    token: Option<&str>,
    user_id: &str,
) -> (StatusCode, Value) {
    let url = format!("{}{}", state.quarry_edge_url, path);
    let (status, Json(value)) = proxy_bearer_json(state, method, &url, body, token, user_id).await;
    (status, value)
}

/// The authenticated user's authoritative org id — cached per-user in `upstream`
/// and resolved from user-core's session-context (never a client header). Empty
/// string on any failure.
pub(super) async fn authorized_org_id(state: &AppState, user: &AuthenticatedUser) -> String {
    crate::upstream::authorized_org_id(state, user).await
}

/// Fire an internal service-to-service request (internal API key + actor +
/// optional org header) and return the parsed JSON body, or `None` on any
/// transport / non-2xx / non-JSON outcome. The `sources` aggregator degrades
/// gracefully so one slow or down upstream never fails the whole response.
pub(super) async fn fetch_internal_json(
    state: &AppState,
    method: Method,
    url: &str,
    body: Option<Value>,
    org_id: Option<&str>,
    actor: &ActionActor,
    timeout: Duration,
) -> Option<Value> {
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
    if let Some(body) = body {
        req = req.json(&body);
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok(),
        _ => None,
    }
}

/// Read a Data Plane projection as the interactive user. The audience token is
/// the only credential: never combine it with the shared internal key, which
/// could otherwise change private-document visibility semantics.
pub(super) async fn fetch_data_plane_json(
    state: &AppState,
    method: Method,
    url: &str,
    org_id: &str,
    actor: &ActionActor,
    timeout: Duration,
    bearer: &str,
) -> Option<Value> {
    let mut req = state
        .client
        .request(method, url)
        .timeout(timeout)
        .bearer_auth(bearer)
        .header("x-user-id", actor.user_id.as_str())
        .header("x-org-id", org_id.trim());
    if !actor.user_email.is_empty() {
        req = req.header("x-user-email", actor.user_email.as_str());
    }
    if !actor.user_name.is_empty() {
        req = req.header("x-user-name", actor.user_name.as_str());
    }
    match req.send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok(),
        _ => None,
    }
}

// ── Response envelopes ──────────────────────────────────────────────────────

/// 422 with the standard `{error:{code,message}}` envelope.
pub(super) fn validation(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(error("validation_error", message.to_owned())),
    )
        .into_response()
}

/// Forward an upstream `(status, body)` failure verbatim.
pub(super) fn forward(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// 201 Created with `data` wrapped in the `{data}` success envelope.
pub(super) fn created(data: Value) -> Response {
    (StatusCode::CREATED, Json(ok(data))).into_response()
}

/// 200 OK with `data` wrapped in the `{data}` success envelope.
pub(super) fn okay(data: Value) -> Response {
    (StatusCode::OK, Json(ok(data))).into_response()
}

// ── URL normalization (SSRF guard for user-supplied targets) ────────────────

/// Validate + normalize one user-supplied URL. Thin alias over the shared
/// public-URL guard so call sites read intentfully.
pub(super) fn normalize_target(raw: &str) -> Result<String, String> {
    normalize_public_http_url(raw)
}

/// Normalize a `urls: [...]` list for a batch run/schedule: SSRF-guard each,
/// drop blanks/invalid, collapse duplicates, preserve order, cap at
/// [`MAX_BATCH_URLS`]. Errors when nothing valid remains.
pub(super) fn normalize_batch_urls(raw: &[Value]) -> Result<Vec<String>, String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut urls = Vec::new();
    for entry in raw {
        let Some(candidate) = entry.as_str().map(str::trim).filter(|v| !v.is_empty()) else {
            continue;
        };
        let Ok(normalized) = normalize_public_http_url(candidate) else {
            continue;
        };
        if seen.insert(normalized.clone()) {
            urls.push(normalized);
            if urls.len() >= MAX_BATCH_URLS {
                break;
            }
        }
    }
    if urls.is_empty() {
        return Err("At least one valid URL is required.".to_owned());
    }
    Ok(urls)
}

// ── JSON value extraction ───────────────────────────────────────────────────

/// Trimmed string at `key`, or "".
pub(super) fn str_at(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_owned())
        .unwrap_or_default()
}

/// First trimmed non-empty string across `keys`.
pub(super) fn first_str(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = value.get(key).and_then(Value::as_str) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

/// First finite numeric value across `keys`, preserving int/float JSON shape.
pub(super) fn first_num(value: &Value, keys: &[&str]) -> Option<Value> {
    for key in keys {
        if let Some(v) = value.get(key) {
            if v.is_number() && v.as_f64().map(f64::is_finite).unwrap_or(false) {
                return Some(v.clone());
            }
        }
    }
    None
}

/// Value at `key` if it is a JSON string, else `null` — matches v2's `?? null`
/// for nullable timestamps (`startedAt`, `completedAt`).
pub(super) fn str_or_null(value: &Value, key: &str) -> Value {
    match value.get(key) {
        Some(v) if v.is_string() => v.clone(),
        _ => Value::Null,
    }
}

/// Owned array at `key`, or empty.
pub(super) fn array_at(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Object at `key`, or `{}`.
pub(super) fn obj_or_empty(value: &Value, key: &str) -> Value {
    value
        .get(key)
        .filter(|v| v.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}))
}

/// RFC3339 timestamp → epoch millis for recency sorting; unparseable → 0.
pub(super) fn date_millis(value: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_batch_urls_dedups_drops_invalid_and_caps() {
        let raw = vec![
            json!("https://vg.no/a"),
            json!("https://vg.no/a"),
            json!("   "),
            json!("not-a-url"),
            json!("ftp://vg.no/x"),
            json!("http://localhost/secret"),
            json!("https://vg.no/b"),
        ];
        let urls = normalize_batch_urls(&raw).unwrap();
        assert_eq!(urls, vec!["https://vg.no/a", "https://vg.no/b"]);
    }

    #[test]
    fn normalize_batch_urls_errors_when_nothing_valid() {
        let raw = vec![json!(""), json!("not-a-url"), json!("http://127.0.0.1")];
        assert!(normalize_batch_urls(&raw).is_err());
    }

    #[test]
    fn first_num_preserves_integer_shape_and_skips_non_numbers() {
        let stats = json!({ "completed": 3, "total": null, "max_pages": 12 });
        assert_eq!(first_num(&stats, &["completed"]), Some(json!(3)));
        assert_eq!(first_num(&stats, &["total", "max_pages"]), Some(json!(12)));
        assert_eq!(first_num(&stats, &["missing"]), None);
    }

    #[test]
    fn str_or_null_maps_missing_to_null() {
        let job = json!({ "started_at": "2026-06-14T00:00:00Z", "completed_at": null });
        assert_eq!(
            str_or_null(&job, "started_at"),
            json!("2026-06-14T00:00:00Z")
        );
        assert_eq!(str_or_null(&job, "completed_at"), Value::Null);
        assert_eq!(str_or_null(&job, "missing"), Value::Null);
    }
}
