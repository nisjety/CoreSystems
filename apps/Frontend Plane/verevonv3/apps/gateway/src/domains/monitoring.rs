//! Change-monitoring domain — Phase 1 Track C (the "monitor" leg).
//!
//! Backs the Verevon v3 SPA's `/api/v1/monitoring/*` surface by proxying
//! quarry-edge's `/v1/change/*` routes (Quarry-v2 cluster #9 versioned change
//! history, served from `PostgresBaselineStore` when the edge is built with
//! `--features postgres-queue` and wired with `QUARRY_EDGE__DATABASE_URL`).
//!
//! ## What is real here
//!
//! This is **on-demand only** — a "check this URL now" action plus a read of
//! the latest baseline and the change history. There is deliberately NO
//! scheduling control: at-scale, Temporal-driven scheduled monitoring is
//! Phase 2 (C-FULL). Every value the SPA renders traces to a live edge
//! response: a `check` runs a real fetch (to compute a fingerprint) and a real
//! baseline comparison; `latest` / `history` read whatever baselines the org
//! has actually accrued (empty until the first check runs). Nothing is
//! synthesized.
//!
//! ## Auth model (mirrors `ingestions::shared`)
//!
//! Edge is the JWT-authenticated face of Quarry-v2. We mint the short-lived
//! `quarry` audience token from the *validated* session cookie and send it as
//! a bearer; edge derives org scope from the token's `claims.org_id`. We never
//! read a client-supplied `x-verevon-org-id` (there is no `x-org-id` header on
//! these calls) so there is no cross-tenant IDOR vector. `require_session`
//! gates every route.
//!
//! ## SSRF guard
//!
//! Every user-supplied URL passes through [`normalize_public_http_url`] before
//! it reaches the edge — the same guard the scrape / ingestions-schedule paths
//! use. It rejects non-http(s) schemes, embedded credentials, `localhost` /
//! `*.local` / `*.internal` hostnames, and any URL whose host parses to a
//! private / loopback / link-local / IPv4-mapped address.

use axum::{
    extract::{Extension, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    audience_tokens::get_audience_token,
    config::AppState,
    envelope::{error, ok},
    middleware::{require_session, AuthenticatedUser},
    public_url::normalize_public_http_url,
    upstream::proxy_bearer_json,
};

pub(crate) fn router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/monitoring/check", post(check_now))
        .route("/api/v1/monitoring/latest", get(latest))
        .route("/api/v1/monitoring/history", get(history))
        // Phase 2 (C-FULL): recurring, Temporal-driven change monitors.
        .route(
            "/api/v1/monitoring/schedules",
            get(list_monitors).post(create_monitor),
        )
        .route(
            "/api/v1/monitoring/schedules/:id",
            axum::routing::delete(delete_monitor),
        )
        .route_layer(axum::middleware::from_fn_with_state(state, require_session))
}

// ── Shared plumbing ──────────────────────────────────────────────────────────

fn cookie_header(headers: &HeaderMap) -> String {
    headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned()
}

/// Mint (or reuse) the cached `quarry` audience token for this user/session.
async fn quarry_token(state: &AppState, user: &AuthenticatedUser, cookie: &str) -> Option<String> {
    get_audience_token(state, &user.user_id, cookie, "quarry").await
}

/// Call quarry-edge with bearer auth, returning the raw `(status, body)`.
async fn quarry_call(
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

/// 422 with the standard `{error:{code,message}}` envelope.
fn validation(message: &str) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(error("validation_error", message.to_owned())),
    )
        .into_response()
}

/// Forward an upstream `(status, body)` failure verbatim.
fn forward(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}

/// First trimmed non-empty string across `keys`.
fn first_str(value: &Value, keys: &[&str]) -> Option<String> {
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

/// Pull a `fingerprint` out of a quarry `/v1/scrape` success body. The edge
/// wraps results in `{ data: NormalizedOutput }`; `NormalizedOutput.fingerprint`
/// is the blake3 content hash. Returns `None` if the shape is unexpected.
fn fingerprint_from_scrape(body: &Value) -> Option<String> {
    let data = body.get("data").unwrap_or(body);
    first_str(data, &["fingerprint"])
}

/// Owned string at `key` (snake_case quarry field), or `null`.
fn str_field(value: &Value, key: &str) -> Value {
    match value.get(key) {
        Some(v) if v.is_string() => v.clone(),
        _ => Value::Null,
    }
}

/// Normalize quarry's snake_case `BaselineSnapshot` to the SPA's camelCase
/// shape. Missing optional fields collapse to `null` — never a fabricated
/// value. Returns `null` for a `null`/non-object input (e.g. no baseline yet).
fn normalize_baseline(value: &Value) -> Value {
    if !value.is_object() {
        return Value::Null;
    }
    json!({
        "baselineId": str_field(value, "baseline_id"),
        "orgId": str_field(value, "org_id"),
        "sourceUrl": str_field(value, "source_url"),
        "fingerprint": str_field(value, "fingerprint"),
        "artifactId": str_field(value, "artifact_id"),
        "prevBaselineId": str_field(value, "prev_baseline_id"),
        "runId": str_field(value, "run_id"),
        "capturedAt": str_field(value, "captured_at"),
    })
}

/// Normalize quarry's snake_case `ChangeRecord` to the SPA's camelCase shape.
/// `status` stays verbatim (quarry already emits snake_case: new / unchanged /
/// changed / unreachable). Nested baselines are normalized recursively.
fn normalize_change_record(value: &Value) -> Value {
    json!({
        "sourceUrl": str_field(value, "source_url"),
        "orgId": str_field(value, "org_id"),
        "status": value.get("status").cloned().unwrap_or(Value::Null),
        "newBaseline": value.get("new_baseline").map(normalize_baseline).unwrap_or(Value::Null),
        "prevBaseline": value.get("prev_baseline").map(normalize_baseline).unwrap_or(Value::Null),
        "diffId": str_field(value, "diff_id"),
        "checkedAt": str_field(value, "checked_at"),
    })
}

// ── Routes ───────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CheckRequest {
    #[serde(default)]
    url: Option<String>,
    /// Optional pre-computed fingerprint. When omitted (the normal UI path),
    /// the gateway performs a real scrape to compute one so the comparison
    /// reflects the page's current content.
    #[serde(default)]
    fingerprint: Option<String>,
}

/// `POST /api/v1/monitoring/check` — run a fresh fetch of `url`, fingerprint it,
/// and compare against the org's latest baseline. The returned `ChangeRecord`
/// status is one of `new` / `unchanged` / `changed`, computed by the edge.
async fn check_now(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(req): Json<CheckRequest>,
) -> Response {
    let Some(raw_url) = req.url.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return validation("A URL is required.");
    };
    let target = match normalize_public_http_url(raw_url) {
        Ok(url) => url,
        Err(message) => return validation(&message),
    };

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let token = token.as_deref();
    let user_id = user.user_id.as_str();

    // Determine the fresh fingerprint. The local store requires one; rather
    // than fabricate it, fetch the page through the edge's real scrape path
    // and read the blake3 fingerprint it computes.
    let fingerprint = match req
        .fingerprint
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(fp) => fp.to_owned(),
        None => {
            let (status, body) = quarry_call(
                &state,
                Method::POST,
                "/v1/scrape",
                Some(json!({ "url": target })),
                token,
                user_id,
            )
            .await;
            if !status.is_success() {
                return forward(status, body);
            }
            match fingerprint_from_scrape(&body) {
                Some(fp) => fp,
                None => {
                    return (
                        StatusCode::BAD_GATEWAY,
                        Json(error(
                            "fingerprint_unavailable",
                            "The page was fetched but no content fingerprint was returned.",
                        )),
                    )
                        .into_response();
                }
            }
        }
    };

    let (status, body) = quarry_call(
        &state,
        Method::POST,
        "/v1/change/check",
        Some(json!({ "url": target, "fresh_fingerprint": fingerprint })),
        token,
        user_id,
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    (
        StatusCode::OK,
        Json(ok(normalize_change_record(&unwrap_data(body)))),
    )
        .into_response()
}

// ── Recurring monitors (Phase 2 / C-FULL) ────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CreateMonitorRequest {
    #[serde(default)]
    url: Option<String>,
    /// Fixed cadence: "hourly" | "daily" | "weekly". The edge/control derive the
    /// 5-field cron from this — free-form cron is intentionally not accepted.
    #[serde(default)]
    preset: Option<String>,
}

/// `POST /api/v1/monitoring/schedules` — create a recurring change monitor. The
/// edge stamps org_id + the creator's user_id from the validated session token;
/// we never trust a client-supplied tenant. The URL is SSRF-guarded.
async fn create_monitor(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Json(req): Json<CreateMonitorRequest>,
) -> Response {
    let Some(raw_url) = req.url.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return validation("A URL is required.");
    };
    let target = match normalize_public_http_url(raw_url) {
        Ok(url) => url,
        Err(message) => return validation(&message),
    };
    let preset = req.preset.as_deref().map(str::trim).unwrap_or("daily");
    if !matches!(preset, "hourly" | "daily" | "weekly") {
        return validation("preset must be one of hourly, daily, or weekly.");
    }

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::POST,
        "/v1/schedules",
        Some(json!({ "preset": preset, "target_ref": target })),
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    (StatusCode::CREATED, Json(ok(unwrap_data(body)))).into_response()
}

/// `GET /api/v1/monitoring/schedules` — list the org's recurring change monitors
/// (change_monitor schedules only; other ingestion schedules live under
/// `/api/ingestions/schedules`).
async fn list_monitors(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        "/v1/schedules?limit=100",
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    // The edge returns a `Page` envelope (`{data:{items:[...]}}`); tolerate a
    // bare `{data:[...]}` array too. Then keep only change-monitor schedules.
    let data = unwrap_data(body);
    let monitors: Vec<Value> = data
        .get("items")
        .and_then(Value::as_array)
        .or_else(|| data.as_array())
        .map(|rows| {
            rows.iter()
                .filter(|s| s.get("target_kind").and_then(Value::as_str) == Some("change_monitor"))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    (StatusCode::OK, Json(ok(Value::Array(monitors)))).into_response()
}

/// `DELETE /api/v1/monitoring/schedules/:id` — stop + remove a recurring monitor.
async fn delete_monitor(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;
    let path = format!("/v1/schedules/{}", urlencoding(&id));
    let (status, body) = quarry_call(
        &state,
        Method::DELETE,
        &path,
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    StatusCode::NO_CONTENT.into_response()
}

#[derive(Debug, Deserialize)]
struct UrlQuery {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    limit: Option<u32>,
}

/// `GET /api/v1/monitoring/latest?url=` — the most recent baseline for `url`,
/// or a `null` payload when the org has never captured it (edge returns 404;
/// we normalize that to an honest empty success so the SPA shows "never
/// checked" rather than an error toast).
async fn latest(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(q): Query<UrlQuery>,
) -> Response {
    let Some(raw_url) = q.url.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return validation("A URL is required.");
    };
    let target = match normalize_public_http_url(raw_url) {
        Ok(url) => url,
        Err(message) => return validation(&message),
    };

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let path = format!("/v1/change/latest?url={}", urlencoding(&target));
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &path,
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;

    if status == StatusCode::NOT_FOUND {
        return (StatusCode::OK, Json(ok(Value::Null))).into_response();
    }
    if !status.is_success() {
        return forward(status, body);
    }
    (
        StatusCode::OK,
        Json(ok(normalize_baseline(&unwrap_data(body)))),
    )
        .into_response()
}

/// `GET /api/v1/monitoring/history?url=&limit=` — newest-first baseline chain
/// for `url`. Empty list until baselines accrue — never a synthesized entry.
async fn history(
    State(state): State<AppState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    Query(q): Query<UrlQuery>,
) -> Response {
    let Some(raw_url) = q.url.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return validation("A URL is required.");
    };
    let target = match normalize_public_http_url(raw_url) {
        Ok(url) => url,
        Err(message) => return validation(&message),
    };

    let cookie = cookie_header(&headers);
    let token = quarry_token(&state, &user, &cookie).await;

    let limit = q.limit.unwrap_or(50).clamp(1, 1000);
    let path = format!(
        "/v1/change/history?url={}&limit={}",
        urlencoding(&target),
        limit
    );
    let (status, body) = quarry_call(
        &state,
        Method::GET,
        &path,
        None,
        token.as_deref(),
        user.user_id.as_str(),
    )
    .await;
    if !status.is_success() {
        return forward(status, body);
    }
    let items = unwrap_data(body);
    let normalized: Vec<Value> = items
        .as_array()
        .map(|arr| arr.iter().map(normalize_baseline).collect())
        .unwrap_or_default();
    (StatusCode::OK, Json(ok(normalized))).into_response()
}

/// Unwrap quarry's `{ data: ... }` / `{ request_id, data }` envelope to the
/// inner payload; pass through anything that isn't enveloped.
fn unwrap_data(body: Value) -> Value {
    match body.get("data") {
        Some(data) => data.clone(),
        None => body,
    }
}

/// Minimal percent-encoding for a URL placed in a query-string value. We only
/// need to escape the handful of characters that would break the `?url=...`
/// parameter; the value has already passed the SSRF guard so it is a valid URL.
fn urlencoding(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssrf_guard_blocks_loopback_and_private_and_internal() {
        // Loopback / localhost / link-local / private / .internal must all be
        // rejected before any quarry call is made.
        assert!(normalize_public_http_url("http://localhost/").is_err());
        assert!(normalize_public_http_url("http://127.0.0.1/").is_err());
        assert!(normalize_public_http_url("http://169.254.169.254/").is_err());
        assert!(normalize_public_http_url("http://10.0.0.5/").is_err());
        assert!(normalize_public_http_url("http://192.168.1.1/").is_err());
        assert!(normalize_public_http_url("http://metadata.google.internal/").is_err());
        // IPv4-mapped IPv6 form of loopback.
        assert!(normalize_public_http_url("http://[::ffff:127.0.0.1]/").is_err());
        // Non-http schemes and embedded credentials.
        assert!(normalize_public_http_url("ftp://example.com/").is_err());
        assert!(normalize_public_http_url("https://user:pass@example.com/").is_err());
    }

    #[test]
    fn ssrf_guard_allows_public_urls() {
        assert_eq!(
            normalize_public_http_url("https://vg.no").unwrap(),
            "https://vg.no/"
        );
        assert!(normalize_public_http_url("https://example.com/path?q=1").is_ok());
    }

    #[test]
    fn fingerprint_extracted_from_enveloped_and_bare_scrape() {
        let enveloped = json!({ "data": { "fingerprint": "blake3:abc", "status": 200 } });
        assert_eq!(
            fingerprint_from_scrape(&enveloped).as_deref(),
            Some("blake3:abc")
        );
        let bare = json!({ "fingerprint": "blake3:def" });
        assert_eq!(
            fingerprint_from_scrape(&bare).as_deref(),
            Some("blake3:def")
        );
        let missing = json!({ "data": { "status": 200 } });
        assert!(fingerprint_from_scrape(&missing).is_none());
    }

    #[test]
    fn normalize_change_record_camelcases_and_keeps_status() {
        // quarry emits snake_case fields + snake_case status enum.
        let quarry = json!({
            "source_url": "https://vg.no/",
            "org_id": "org_a",
            "status": "changed",
            "prev_baseline": {
                "baseline_id": "bln_1",
                "org_id": "org_a",
                "source_url": "https://vg.no/",
                "fingerprint": "blake3:abc",
                "captured_at": "2026-06-19T00:00:00Z"
            },
            "checked_at": "2026-06-19T12:00:00Z"
        });
        let out = normalize_change_record(&quarry);
        assert_eq!(out["sourceUrl"], "https://vg.no/");
        assert_eq!(out["status"], "changed");
        assert_eq!(out["checkedAt"], "2026-06-19T12:00:00Z");
        assert_eq!(out["prevBaseline"]["baselineId"], "bln_1");
        assert_eq!(out["prevBaseline"]["capturedAt"], "2026-06-19T00:00:00Z");
        // First-check shape: no prev baseline.
        let first = json!({ "source_url": "https://x/", "org_id": "o", "status": "new", "checked_at": "2026-06-19T12:00:00Z" });
        assert_eq!(normalize_change_record(&first)["prevBaseline"], Value::Null);
    }

    #[test]
    fn normalize_baseline_handles_null_and_object() {
        assert_eq!(normalize_baseline(&Value::Null), Value::Null);
        let b = json!({ "baseline_id": "b1", "source_url": "https://x/", "fingerprint": "fp", "captured_at": "2026-06-19T00:00:00Z" });
        let out = normalize_baseline(&b);
        assert_eq!(out["baselineId"], "b1");
        assert_eq!(out["fingerprint"], "fp");
        // Missing optional fields collapse to null, not a fabricated value.
        assert_eq!(out["runId"], Value::Null);
    }

    #[test]
    fn unwrap_data_pulls_inner_payload() {
        let enveloped = json!({ "request_id": "r1", "data": { "status": "changed" } });
        assert_eq!(unwrap_data(enveloped), json!({ "status": "changed" }));
        let bare = json!({ "status": "new" });
        assert_eq!(unwrap_data(bare), json!({ "status": "new" }));
    }

    #[test]
    fn urlencoding_escapes_query_breaking_characters() {
        assert_eq!(
            urlencoding("https://example.com/p?q=1&x=2"),
            "https%3A%2F%2Fexample.com%2Fp%3Fq%3D1%26x%3D2"
        );
        assert_eq!(urlencoding("https://vg.no/"), "https%3A%2F%2Fvg.no%2F");
    }
}
