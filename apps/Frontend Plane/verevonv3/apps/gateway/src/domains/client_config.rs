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
//! The CSP report route (`/api/v1/client-diagnostics/csp-report`) belongs to WS07-S6 and is
//! added to this module's router there.

use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
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
