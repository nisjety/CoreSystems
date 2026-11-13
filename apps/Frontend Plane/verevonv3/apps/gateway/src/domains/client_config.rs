//! Public client-resilience routes (WS07-S3).
//!
//! - `GET  /api/v1/client-config`: the contract pair, the version policy and the running
//!   revision, so an open tab can tell it is stale (`no-store`). With `CLIENT_ENV_IDENTITY` on
//!   (F-t3-098) it also names the environment: `production`, `staging`, `development` or `other`.
//! - `POST /api/v1/client-diagnostics`: a content-free beacon
//!   `{"kind": "render_error|chunk_error|watchdog", "build": "<=12 hex"}`, or a `long_task`
//!   `{"kind": "long_task", "build": "<=12 hex", "bucket": "50_100|100_250|250_1000|1000_plus"}`
//!   (F-t3-096): one main-thread task of 50 ms or more, in a closed duration bucket.
//!
//! Both are public on purpose: a crashed or logged-out app must still reach them. They
//! sit behind the gateway's IP rate limiter like every other route and store nothing; the
//! beacon only increments `verevon_client_failures_total{kind, build_match}`, or for a
//! `long_task` `verevon_client_long_tasks_total{bucket, build_match}`. The body is
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

/// The closed set of kinds a client may report: three failures, and `long_task`, the one
/// performance sample (F-t3-096). Only `long_task` carries a `bucket`.
const DIAGNOSTIC_KINDS: [&str; 4] = ["render_error", "chunk_error", "watchdog", "long_task"];

/// The closed duration buckets of a `long_task`, in milliseconds: 50 to under 100, 100 to under
/// 250, 250 to under 1000, and 1000 and up.
const LONG_TASK_BUCKETS: [&str; 4] = ["50_100", "100_250", "250_1000", "1000_plus"];

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
    let mut data = json!({
        "contract": CLIENT_CONTRACT,
        "min_contract": policy.min_contract,
        "policy": policy.policy.as_str(),
        "revision": policy.revision,
        "build_date": policy.build_date,
    });
    // F-t3-098: the closed label read from `APP_ENV` once in `build_state`, only when
    // `CLIENT_ENV_IDENTITY` is on. The key is absent otherwise, so the answer is unchanged.
    if let Some(environment) = state.space_features.client_environment {
        data["environment"] = json!(environment);
    }
    ([(header::CACHE_CONTROL, "no-store")], Json(ok(data))).into_response()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientDiagnostic {
    kind: String,
    build: String,
    /// Required for `long_task` and refused for every other kind. The value is checked against
    /// [`LONG_TASK_BUCKETS`] in the handler.
    bucket: Option<String>,
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
    // A long_task names a closed duration bucket; every other kind must leave `bucket` out.
    let bucket = match (kind, diagnostic.bucket.as_deref()) {
        ("long_task", Some(wanted)) => {
            let Some(bucket) = LONG_TASK_BUCKETS
                .iter()
                .copied()
                .find(|known| *known == wanted)
            else {
                return invalid_diagnostic();
            };
            Some(bucket)
        }
        ("long_task", None) | (_, Some(_)) => return invalid_diagnostic(),
        (_, None) => None,
    };
    let matched = build_match(
        &diagnostic.build,
        &state.space_features.client_policy.revision,
    );
    match bucket {
        Some(bucket) => {
            metrics::counter!(
                "verevon_client_long_tasks_total",
                "bucket" => bucket,
                "build_match" => matched,
            )
            .increment(1);
        }
        None => {
            metrics::counter!(
                "verevon_client_failures_total",
                "kind" => kind,
                "build_match" => matched,
            )
            .increment(1);
        }
    }
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

#[cfg(test)]
mod tests {
    //! WS07-S3 and WS07-S6 tests: the real router (no session cookie: the routes are public),
    //! with a thread-local Prometheus recorder to read the counters back and a thread-local
    //! tracing subscriber to read the log line.
    use super::*;
    use crate::client_contract::{
        min_contract_from, ClientPolicy, ClientVersionPolicy, CLIENT_MIN_CONTRACT,
    };
    use axum::http::Request;
    use http_body_util::BodyExt;
    use metrics_exporter_prometheus::PrometheusBuilder;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    use tower::ServiceExt;

    async fn call(
        state: &AppState,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Vec<u8>,
    ) -> (u16, Option<String>, String) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(content_type) = content_type {
            builder = builder.header("content-type", content_type);
        }
        let response = crate::build_router(state.clone())
            .oneshot(builder.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let cache = response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let raw = String::from_utf8_lossy(&response.into_body().collect().await.unwrap().to_bytes())
            .into_owned();
        (status, cache, raw)
    }

    async fn diagnostic(state: &AppState, body: &str) -> (u16, String) {
        let (status, _, raw) = call(
            state,
            "POST",
            "/api/v1/client-diagnostics",
            Some("application/json"),
            body.as_bytes().to_vec(),
        )
        .await;
        (status, raw)
    }

    async fn csp(state: &AppState, content_type: Option<&str>, body: &[u8]) -> (u16, String) {
        let (status, _, raw) = call(
            state,
            "POST",
            "/api/v1/client-diagnostics/csp-report",
            content_type,
            body.to_vec(),
        )
        .await;
        (status, raw)
    }

    fn state() -> AppState {
        crate::tests::test_state(false)
    }

    fn map(value: Value) -> JsonMap {
        value.as_object().expect("an object").clone()
    }

    // ---- WS07-S3: policy and contract parsing ----

    #[test]
    fn policy_parse_defaults_to_banner_for_unset_or_garbage() {
        assert_eq!(ClientVersionPolicy::parse(None), ClientVersionPolicy::Banner);
        assert_eq!(ClientVersionPolicy::parse(Some("")), ClientVersionPolicy::Banner);
        assert_eq!(
            ClientVersionPolicy::parse(Some("sometimes")),
            ClientVersionPolicy::Banner
        );
        assert_eq!(ClientVersionPolicy::parse(Some(" OFF ")), ClientVersionPolicy::Off);
        assert_eq!(
            ClientVersionPolicy::parse(Some("Enforce")),
            ClientVersionPolicy::Enforce
        );
        assert_eq!(
            ClientVersionPolicy::parse(Some("banner")),
            ClientVersionPolicy::Banner
        );
        assert_eq!(ClientVersionPolicy::default().as_str(), "banner");
    }

    #[test]
    fn min_contract_from_only_raises_the_floor() {
        assert_eq!(min_contract_from(None), CLIENT_MIN_CONTRACT);
        assert_eq!(min_contract_from(Some("0")), CLIENT_MIN_CONTRACT);
        assert_eq!(min_contract_from(Some("not-a-number")), CLIENT_MIN_CONTRACT);
        assert_eq!(min_contract_from(Some("-4")), CLIENT_MIN_CONTRACT);
        assert_eq!(min_contract_from(Some(" 7 ")), 7.max(CLIENT_MIN_CONTRACT));
        assert!(min_contract_from(Some("9")) >= 9);
    }

    #[tokio::test]
    async fn client_config_returns_the_contract_envelope_with_no_store_and_no_session() {
        let mut state = state();
        state.space_features.client_policy = ClientPolicy {
            policy: ClientVersionPolicy::Enforce,
            min_contract: 4,
            revision: "abc1234def56".to_owned(),
            build_date: "2026-10-20".to_owned(),
        };
        let (status, cache, raw) = call(&state, "GET", "/api/v1/client-config", None, vec![]).await;
        assert_eq!(status, 200, "{raw}");
        assert_eq!(cache.as_deref(), Some("no-store"));
        let body: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            body,
            json!({"data": {
                "contract": CLIENT_CONTRACT,
                "min_contract": 4,
                "policy": "enforce",
                "revision": "abc1234def56",
                "build_date": "2026-10-20",
            }})
        );
    }

    #[tokio::test]
    async fn client_config_defaults_are_banner_unverified_unknown() {
        let (status, _, raw) = call(&state(), "GET", "/api/v1/client-config", None, vec![]).await;
        assert_eq!(status, 200, "{raw}");
        let body: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(body["data"]["policy"], "banner");
        assert_eq!(body["data"]["revision"], "unverified");
        assert_eq!(body["data"]["build_date"], "unknown");
        assert_eq!(body["data"]["min_contract"], CLIENT_MIN_CONTRACT);
    }

    // ---- WS07-S3: diagnostics beacon ----

    #[tokio::test]
    async fn diagnostics_accepts_each_kind_with_a_hex_or_dev_build() {
        for kind in DIAGNOSTIC_KINDS {
            for build in ["a", "abc123", "0123456789ab", "ABCDEF", "dev"] {
                let mut body = json!({"kind": kind, "build": build});
                if kind == "long_task" {
                    body["bucket"] = json!("50_100");
                }
                let (status, raw) = diagnostic(&state(), &body.to_string()).await;
                assert_eq!(status, 204, "{kind}/{build}: {raw}");
                assert!(raw.is_empty());
            }
        }
    }

    #[tokio::test]
    async fn diagnostics_refuses_unknown_kind_bad_build_extra_field_non_json_and_oversize() {
        let oversize = format!(
            r#"{{"kind":"watchdog","build":"abc","pad":"{}"}}"#,
            "x".repeat(600)
        );
        let cases: Vec<(&str, String)> = vec![
            ("unknown kind", json!({"kind": "boom", "build": "abc"}).to_string()),
            ("non-hex build", json!({"kind": "watchdog", "build": "xyz!"}).to_string()),
            (
                "too long build",
                json!({"kind": "watchdog", "build": "0123456789abc"}).to_string(),
            ),
            ("empty build", json!({"kind": "watchdog", "build": ""}).to_string()),
            (
                "extra field",
                json!({"kind": "watchdog", "build": "abc", "message": "free text"}).to_string(),
            ),
            ("missing build", json!({"kind": "watchdog"}).to_string()),
            ("not json", "kind=watchdog".to_owned()),
            ("oversize", oversize),
        ];
        for (label, body) in cases {
            let (status, raw) = diagnostic(&state(), &body).await;
            assert_eq!(status, 400, "{label}: {raw}");
            let parsed: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(parsed["error"]["code"], "invalid_diagnostic", "{label}");
        }
    }

    #[test]
    fn build_match_labels_are_a_closed_set() {
        assert_eq!(build_match("dev", "abc123"), "dev");
        assert_eq!(build_match("abc", "abc123456789"), "match");
        assert_eq!(build_match("ABC", "abc123456789"), "match");
        assert_eq!(build_match("fff", "abc123456789"), "mismatch");
        assert_eq!(build_match("abc", "unverified"), "unknown");
        assert_eq!(build_match("abc", "unknown"), "unknown");
        assert_eq!(build_match("abc", "  "), "unknown");
    }

    #[tokio::test]
    async fn the_failure_counter_carries_only_kind_and_build_match() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let mut state = state();
        state.space_features.client_policy.revision = "abc123456789".to_owned();
        for (build, expected) in [("abc123", "match"), ("fff000", "mismatch"), ("dev", "dev")] {
            let (status, _) = diagnostic(
                &state,
                &json!({"kind": "render_error", "build": build}).to_string(),
            )
            .await;
            assert_eq!(status, 204);
            let rendered = handle.render();
            assert!(
                rendered.lines().any(|line| {
                    line.starts_with("verevon_client_failures_total{")
                        && line.contains("kind=\"render_error\"")
                        && line.contains(&format!("build_match=\"{expected}\""))
                }),
                "{expected}: {rendered}"
            );
        }
        let rendered = handle.render();
        assert!(
            !rendered.contains("abc123\""),
            "the raw build id must not be a label: {rendered}"
        );
        assert!(!rendered.contains("fff000"), "{rendered}");
    }

    // ---- WS07-S6: csp-report route ----

    #[tokio::test]
    async fn csp_report_accepts_the_three_wire_shapes() {
        let legacy = json!({"csp-report": {
            "document-uri": "https://app.example.com/page?token=SECRET",
            "violated-directive": "img-src 'self'",
            "effective-directive": "img-src",
            "blocked-uri": "https://cdn.example.net/pic.png",
            "disposition": "report"
        }})
        .to_string();
        let reporting = json!([{"type": "csp-violation", "url": "https://app.example.com/", "body": {
            "effectiveDirective": "connect-src",
            "blockedURL": "https://api.example.net/v1",
            "disposition": "report"
        }}])
        .to_string();
        let bare =
            json!({"effective-directive": "script-src", "blocked-uri": "inline"}).to_string();
        for (content_type, body) in [
            ("application/csp-report", legacy.clone()),
            ("application/csp-report; charset=utf-8", legacy),
            ("application/reports+json", reporting),
            ("application/json", bare),
        ] {
            let (status, raw) = csp(&state(), Some(content_type), body.as_bytes()).await;
            assert_eq!(status, 204, "{content_type}: {raw}");
        }
    }

    #[tokio::test]
    async fn csp_report_refuses_other_content_types_non_json_and_reports_with_no_violation() {
        let valid = json!({"csp-report": {"effective-directive": "img-src"}}).to_string();
        let (status, raw) = csp(&state(), Some("text/plain"), valid.as_bytes()).await;
        assert_eq!(status, 400, "{raw}");
        assert!(raw.contains("invalid_report"));
        let (status, _) = csp(&state(), None, valid.as_bytes()).await;
        assert_eq!(status, 400);
        let (status, raw) = csp(&state(), Some("application/csp-report"), b"not json").await;
        assert_eq!(status, 400, "{raw}");
        assert!(raw.contains("invalid_report"));
        for body in [
            json!({"hello": "world"}).to_string(),
            json!([]).to_string(),
            json!([{"type": "network-error", "body": {"effectiveDirective": "img-src"}}])
                .to_string(),
            json!("string").to_string(),
        ] {
            let (status, raw) = csp(&state(), Some("application/json"), body.as_bytes()).await;
            assert_eq!(status, 400, "{body}: {raw}");
            assert!(raw.contains("invalid_report"));
        }
    }

    #[tokio::test]
    async fn csp_report_over_8_kib_is_413_report_too_large() {
        let body = json!({"csp-report": {
            "effective-directive": "img-src",
            "script-sample": "x".repeat(CSP_REPORT_BODY_LIMIT + 1),
        }})
        .to_string();
        let (status, raw) = csp(&state(), Some("application/csp-report"), body.as_bytes()).await;
        assert_eq!(status, 413, "{raw}");
        assert!(raw.contains("report_too_large"));
        let just_under = json!({"csp-report": {
            "effective-directive": "img-src",
            "script-sample": "x".repeat(CSP_REPORT_BODY_LIMIT - 200),
        }})
        .to_string();
        assert!(just_under.len() < CSP_REPORT_BODY_LIMIT);
        let (status, _) =
            csp(&state(), Some("application/csp-report"), just_under.as_bytes()).await;
        assert_eq!(status, 204);
    }

    #[test]
    fn directive_label_maps_into_the_closed_set() {
        for directive in CSP_DIRECTIVES {
            let by_effective = map(json!({"effective-directive": directive}));
            assert_eq!(directive_label(&by_effective), directive);
        }
        // The effective directive wins and only its first token counts.
        let both = map(
            json!({"effective-directive": "img-src", "violated-directive": "connect-src 'self'"}),
        );
        assert_eq!(directive_label(&both), "img-src");
        // The violated directive carries the policy text after the name.
        let violated = map(json!({"violated-directive": "connect-src 'self' https://x.example"}));
        assert_eq!(directive_label(&violated), "connect-src");
        let camel = map(json!({"effectiveDirective": "font-src"}));
        assert_eq!(directive_label(&camel), "font-src");
        let upper = map(json!({"effective-directive": "IMG-SRC"}));
        assert_eq!(directive_label(&upper), "img-src");
        // Anything else, including a client-invented name, is `other`.
        assert_eq!(
            directive_label(&map(json!({"effective-directive": "made-up-src"}))),
            "other"
        );
        assert_eq!(directive_label(&map(json!({"effective-directive": 5}))), "other");
        assert_eq!(directive_label(&map(json!({}))), "other");
    }

    #[test]
    fn disposition_label_is_report_enforce_or_unknown() {
        assert_eq!(disposition_label(&map(json!({"disposition": "report"}))), "report");
        assert_eq!(disposition_label(&map(json!({"disposition": "ENFORCE"}))), "enforce");
        assert_eq!(disposition_label(&map(json!({"disposition": "whatever"}))), "unknown");
        assert_eq!(disposition_label(&map(json!({"disposition": 1}))), "unknown");
        assert_eq!(disposition_label(&map(json!({}))), "unknown");
    }

    #[test]
    fn blocked_origin_is_scheme_and_host_only() {
        let origin = |blocked: &str| blocked_origin(&map(json!({"blocked-uri": blocked})));
        assert_eq!(
            origin("https://cdn.example.net/a/b.png?token=SECRET#frag"),
            "https://cdn.example.net"
        );
        assert_eq!(
            origin("https://user:pass@Host.Example.com:8443/p?q=1"),
            "https://host.example.com:8443"
        );
        assert_eq!(origin("wss://live.example.com/socket"), "wss://live.example.com");
        for keyword in ["inline", "eval", "data", "blob", "self", "wasm-eval", "filesystem"] {
            assert_eq!(origin(keyword), keyword);
        }
        assert_eq!(origin("data:image/png;base64,AAAA"), "other");
        assert_eq!(origin("something-else"), "other");
        assert_eq!(origin(""), "other");
        assert_eq!(origin("https://bad host/path"), "other");
        assert_eq!(origin("ht tp://host/path"), "other");
        assert_eq!(
            blocked_origin(&map(json!({"blockedURL": "https://r.example.com/x"}))),
            "https://r.example.com"
        );
        let stripped = origin("https://cdn.example.net/a/b.png?token=SECRET#frag");
        assert!(!stripped.contains("SECRET") && !stripped.contains("/a/"));
    }

    #[tokio::test]
    async fn no_url_reaches_a_csp_metric_label() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let _guard = metrics::set_default_local_recorder(&recorder);
        let body = json!({"csp-report": {
            "document-uri": "https://app.example.com/private/page?token=SECRET",
            "violated-directive": "img-src 'self'",
            "effective-directive": "invented-src",
            "blocked-uri": "https://evil.example.net/secret/path?x=1",
            "script-sample": "SAMPLE-TEXT",
            "disposition": "report"
        }})
        .to_string();
        let (status, _) = csp(&state(), Some("application/csp-report"), body.as_bytes()).await;
        assert_eq!(status, 204);
        let rendered = handle.render();
        let line = rendered
            .lines()
            .find(|line| line.starts_with("verevon_csp_reports_total{"))
            .unwrap_or_else(|| panic!("no counter in {rendered}"));
        assert!(line.contains("directive=\"other\""), "{line}");
        assert!(line.contains("disposition=\"report\""), "{line}");
        for leaked in [
            "evil.example.net",
            "secret",
            "SECRET",
            "SAMPLE-TEXT",
            "app.example.com",
            "https://",
        ] {
            assert!(!rendered.contains(leaked), "{leaked} leaked: {rendered}");
        }
    }

    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    async fn logged_for(log_hosts: bool) -> String {
        let buffer = LogBuffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || writer.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let mut state = state();
        state.space_features.csp_report_log_hosts = log_hosts;
        let body = json!({"csp-report": {
            "effective-directive": "img-src",
            "blocked-uri": "https://evil.example.net/secret/path?x=1",
            "document-uri": "https://app.example.com/private?token=SECRET",
            "disposition": "report"
        }})
        .to_string();
        let (status, _) = csp(&state, Some("application/csp-report"), body.as_bytes()).await;
        assert_eq!(status, 204);
        let bytes = buffer.0.lock().unwrap().clone();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    #[tokio::test]
    async fn the_blocked_origin_is_logged_only_when_csp_report_log_hosts_is_on() {
        let off = logged_for(false).await;
        assert!(!off.contains("csp report"), "{off}");
        assert!(!off.contains("evil.example.net"), "{off}");
        let on = logged_for(true).await;
        assert!(on.contains("csp report"), "{on}");
        assert!(on.contains("https://evil.example.net"), "{on}");
        for leaked in ["secret", "SECRET", "app.example.com", "x=1"] {
            assert!(!on.contains(leaked), "{leaked} leaked: {on}");
        }
    }

    #[tokio::test]
    async fn the_routes_need_no_session() {
        // No cookie, no auth-core mock (the base URL points at a closed port): all three answer.
        let state = state();
        let (status, _, _) = call(&state, "GET", "/api/v1/client-config", None, vec![]).await;
        assert_eq!(status, 200);
        let (status, _) = diagnostic(&state, r#"{"kind":"watchdog","build":"dev"}"#).await;
        assert_eq!(status, 204);
        let (status, _) = csp(
            &state,
            Some("application/csp-report"),
            br#"{"csp-report":{"effective-directive":"img-src"}}"#,
        )
        .await;
        assert_eq!(status, 204);
    }
}
