//! Public client-resilience routes (WS07-S3).
//!
//! - `GET  /api/v1/client-config`: the contract pair, the version policy and the running
//!   revision, so an open tab can tell it is stale (`no-store`).
//! - `POST /api/v1/client-diagnostics`: a content-free failure beacon
//!   `{"kind": "render_error|chunk_error|watchdog", "build": "<=12 hex"}`.
//!
//! Both are public on purpose: a crashed or logged-out app must still reach them. They
//! sit behind the gateway's IP rate limiter like every other route and store nothing; the
//! beacon only increments `verevon_client_failures_total{kind, build_match}`. The body is
//! a closed shape (unknown fields are refused), so no free text, URL, message or user id
//! can enter a metric or a log line, which is what keeps this ZDR-safe.
//!
//! - `POST /api/v1/client-diagnostics/csp-report` (WS07-S6): the sink for the
//!   `Content-Security-Policy-Report-Only` candidate. It accepts `application/csp-report`,
//!   `application/reports+json` and `application/json` (cap 8 KiB), maps each violation to a
//!   closed directive set and increments `verevon_csp_reports_total{directive, disposition}`.
//!   It never labels or logs the blocked URL, the document URL or the sample. Only when
//!   `CSP_REPORT_LOG_HOSTS` is on (never in production, read once in `build_state`) is the
//!   blocked ORIGIN (scheme and host) logged, for a host inventory in a non-production
//!   environment.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    client_contract::CLIENT_CONTRACT,
    config::AppState,
    envelope::{error, ok},
};

/// Largest accepted diagnostics body. The real payload is under 80 bytes.
const DIAGNOSTIC_BODY_LIMIT: usize = 512;

/// The closed set of failure kinds a client may report.
const DIAGNOSTIC_KINDS: [&str; 3] = ["render_error", "chunk_error", "watchdog"];

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/client-config", get(get_client_config))
        .route("/api/v1/client-diagnostics", post(post_client_diagnostic))
        .route(
            "/api/v1/client-diagnostics/csp-report",
            post(post_csp_report),
        )
}

async fn get_client_config(State(state): State<AppState>) -> Response {
    let policy = &state.space_features.client_policy;
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(ok(json!({
            "contract": CLIENT_CONTRACT,
            "min_contract": policy.min_contract,
            "policy": policy.policy.as_str(),
            "revision": policy.revision,
            "build_date": policy.build_date,
        }))),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientDiagnostic {
    kind: String,
    build: String,
}

fn invalid_diagnostic() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "invalid_diagnostic",
            "The diagnostic must be {kind, build} with a known kind and a build id.",
        )),
    )
        .into_response()
}

/// `1..=12` hex characters (a git sha prefix) or the literal `dev` of a local build.
fn valid_build_id(build: &str) -> bool {
    build == "dev"
        || ((1..=12).contains(&build.len()) && build.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// How the reported build relates to the one this gateway runs. A closed label set, so a
/// client cannot widen the metric's cardinality.
fn build_match(reported: &str, running_revision: &str) -> &'static str {
    if reported == "dev" {
        return "dev";
    }
    let running = running_revision.trim().to_ascii_lowercase();
    if running.is_empty() || running == "unverified" || running == "unknown" {
        return "unknown";
    }
    if running.starts_with(&reported.to_ascii_lowercase()) {
        "match"
    } else {
        "mismatch"
    }
}

async fn post_client_diagnostic(State(state): State<AppState>, body: Body) -> Response {
    let Ok(bytes) = axum::body::to_bytes(body, DIAGNOSTIC_BODY_LIMIT).await else {
        return invalid_diagnostic();
    };
    let Ok(diagnostic) = serde_json::from_slice::<ClientDiagnostic>(&bytes) else {
        return invalid_diagnostic();
    };
    let Some(kind) = DIAGNOSTIC_KINDS
        .iter()
        .copied()
        .find(|known| *known == diagnostic.kind)
    else {
        return invalid_diagnostic();
    };
    if !valid_build_id(&diagnostic.build) {
        return invalid_diagnostic();
    }
    let matched = build_match(
        &diagnostic.build,
        &state.space_features.client_policy.revision,
    );
    metrics::counter!(
        "verevon_client_failures_total",
        "kind" => kind,
        "build_match" => matched,
    )
    .increment(1);
    StatusCode::NO_CONTENT.into_response()
}

/// Largest accepted CSP report body (a real report is well under 2 KiB).
const CSP_REPORT_BODY_LIMIT: usize = 8 * 1024;

/// Media types a browser uses for CSP reports: `report-uri` sends `application/csp-report`,
/// the Reporting API sends `application/reports+json`.
const CSP_REPORT_CONTENT_TYPES: [&str; 3] = [
    "application/csp-report",
    "application/reports+json",
    "application/json",
];

/// The closed directive label set. Anything else (including a client-invented name) is
/// recorded as `other`, so a report can never widen the metric's cardinality.
const CSP_DIRECTIVES: [&str; 20] = [
    "default-src",
    "connect-src",
    "img-src",
    "media-src",
    "style-src",
    "style-src-elem",
    "style-src-attr",
    "script-src",
    "script-src-elem",
    "script-src-attr",
    "frame-src",
    "child-src",
    "worker-src",
    "manifest-src",
    "font-src",
    "object-src",
    "frame-ancestors",
    "base-uri",
    "form-action",
    "navigate-to",
];

type JsonMap = serde_json::Map<String, serde_json::Value>;

fn invalid_report() -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(error(
            "invalid_report",
            "The body must be a Content-Security-Policy violation report.",
        )),
    )
        .into_response()
}

fn csp_report_too_large() -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        Json(error(
            "report_too_large",
            "The report exceeds the 8 KiB limit.",
        )),
    )
        .into_response()
}

/// True when `Content-Type` names one of the accepted media types (parameters ignored).
fn csp_content_type_ok(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            let media = value.split(';').next().unwrap_or("").trim();
            CSP_REPORT_CONTENT_TYPES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(media))
        })
}

/// The directive label: the first token of `effective-directive` (or `effectiveDirective`),
/// else of `violated-directive` (which carries the policy text after the name), mapped into
/// [`CSP_DIRECTIVES`] or `other`.
fn directive_label(report: &JsonMap) -> &'static str {
    let named = |key: &str| {
        report
            .get(key)
            .and_then(|value| value.as_str())
            .and_then(|value| value.split_whitespace().next())
            .map(str::to_ascii_lowercase)
    };
    let name = named("effective-directive")
        .or_else(|| named("effectiveDirective"))
        .or_else(|| named("violated-directive"))
        .or_else(|| named("violatedDirective"))
        .unwrap_or_default();
    CSP_DIRECTIVES
        .iter()
        .copied()
        .find(|known| *known == name)
        .unwrap_or("other")
}

/// `report` or `enforce` (the CSP disposition), else `unknown`.
fn disposition_label(report: &JsonMap) -> &'static str {
    match report
        .get("disposition")
        .and_then(|value| value.as_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("report") => "report",
        Some("enforce") => "enforce",
        _ => "unknown",
    }
}

/// The blocked ORIGIN only (`scheme://host[:port]`: no userinfo, path, query or fragment), or
/// one of the CSP keyword sources (`inline`, `eval`, `data`, `blob`, `self`, ...) when the
/// browser reports a keyword instead of a URL. Anything else is `other`. Used only for the
/// non-production host inventory.
fn blocked_origin(report: &JsonMap) -> String {
    let blocked = report
        .get("blocked-uri")
        .or_else(|| report.get("blockedURL"))
        .or_else(|| report.get("blockedUri"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if let Some((scheme, rest)) = blocked.split_once("://") {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
        let host = authority.rsplit('@').next().unwrap_or("");
        let scheme_ok = !scheme.is_empty()
            && scheme.len() <= 16
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
        let host_ok = !host.is_empty()
            && host.len() <= 255
            && host.bytes().all(|b| {
                b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']')
            });
        if scheme_ok && host_ok {
            return format!(
                "{}://{}",
                scheme.to_ascii_lowercase(),
                host.to_ascii_lowercase()
            );
        }
        return "other".to_owned();
    }
    match blocked {
        "inline" | "eval" | "data" | "blob" | "self" | "wasm-eval" | "filesystem" => {
            blocked.to_owned()
        }
        _ => "other".to_owned(),
    }
}

/// The violation objects in a body, whichever of the three wire shapes it uses:
/// `{"csp-report": {...}}` (`report-uri`), `[{"type":"csp-violation","body":{...}}]`
/// (Reporting API) or a bare violation object (`application/json`).
fn csp_violations(value: &serde_json::Value) -> Vec<&JsonMap> {
    use serde_json::Value;
    match value {
        Value::Array(entries) => entries
            .iter()
            .filter(|entry| entry.get("type").and_then(Value::as_str) == Some("csp-violation"))
            .filter_map(|entry| entry.get("body").and_then(Value::as_object))
            .collect(),
        Value::Object(map) => {
            if let Some(inner) = map.get("csp-report").and_then(Value::as_object) {
                vec![inner]
            } else if map.get("type").and_then(Value::as_str) == Some("csp-violation") {
                map.get("body").and_then(Value::as_object).into_iter().collect()
            } else if [
                "effective-directive",
                "effectiveDirective",
                "violated-directive",
                "violatedDirective",
            ]
            .iter()
            .any(|key| map.contains_key(*key))
            {
                vec![map]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

async fn post_csp_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    if !csp_content_type_ok(&headers) {
        return invalid_report();
    }
    let Ok(bytes) = axum::body::to_bytes(body, CSP_REPORT_BODY_LIMIT).await else {
        return csp_report_too_large();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return invalid_report();
    };
    let violations = csp_violations(&value);
    if violations.is_empty() {
        return invalid_report();
    }
    let log_hosts = state.space_features.csp_report_log_hosts;
    for violation in violations {
        let directive = directive_label(violation);
        let disposition = disposition_label(violation);
        metrics::counter!(
            "verevon_csp_reports_total",
            "directive" => directive,
            "disposition" => disposition,
        )
        .increment(1);
        if log_hosts {
            tracing::info!(
                directive,
                disposition,
                blocked_origin = %blocked_origin(violation),
                "csp report"
            );
        }
    }
    StatusCode::NO_CONTENT.into_response()
}
