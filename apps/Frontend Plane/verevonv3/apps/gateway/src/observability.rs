//! Prometheus metrics for the gateway BFF edge (Phase 6 B13).
//!
//! A global Prometheus recorder is installed at startup and rendered at
//! `/metrics`. A request-tracking middleware counts every request by method +
//! status, and the auth path (see `middleware::require_session`) records an
//! org/tenant-labeled counter so the edge has per-tenant request visibility —
//! the org being the authoritative id resolved from the validated session.

use std::time::Instant;

use axum::{extract::Request, middleware::Next, response::Response};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Install the global Prometheus recorder and return a handle for `/metrics`.
pub(crate) fn install_recorder() -> anyhow::Result<PrometheusHandle> {
    PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| anyhow::anyhow!("install prometheus recorder: {e}"))
}

/// Middleware: count every request by method + status and observe its latency.
/// Skips the scrape/health endpoints so they don't inflate their own series.
pub(crate) async fn track_metrics(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    if path == "/metrics" || path == "/health" {
        return next.run(req).await;
    }
    let method = req.method().as_str().to_owned();
    let start = Instant::now();

    let response = next.run(req).await;

    let status = response.status().as_u16().to_string();
    metrics::counter!(
        "gateway_http_requests_total",
        "method" => method.clone(),
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "gateway_http_request_duration_seconds",
        "method" => method,
    )
    .record(start.elapsed().as_secs_f64());

    response
}

/// Record an authenticated request labeled by tenant/org. Called from
/// `require_session` once the validated session is known, so the edge has
/// per-tenant request visibility. `org` is the authoritative org id from the
/// validated session, or "none" when unset.
pub(crate) fn record_authenticated_request(org: &str) {
    metrics::counter!(
        "gateway_authenticated_requests_total",
        "org" => org.to_owned(),
    )
    .increment(1);
}
