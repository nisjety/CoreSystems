//! Prometheus metrics setup and middleware for model-gateway.
//!
//! Tracks: `gateway_requests_total`, `gateway_request_duration_seconds`,
//! and `gateway_active_streams`.

use std::time::Instant;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Install the Prometheus recorder and return a handle for rendering metrics.
///
/// # Errors
///
/// Returns an error if the recorder cannot be installed (e.g., already installed).
pub fn install_recorder() -> Result<PrometheusHandle, metrics_exporter_prometheus::BuildError> {
    PrometheusBuilder::new().install_recorder()
}

/// Axum middleware that records request count, duration, and status metrics.
pub async fn metrics_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let start = Instant::now();

    let response = next.run(request).await;

    let status = response.status().as_u16().to_string();
    let duration = start.elapsed().as_secs_f64();

    counter!(
        "gateway_requests_total",
        "method" => method.clone(),
        "path" => path.clone(),
        "status" => status
    )
    .increment(1);

    histogram!(
        "gateway_request_duration_seconds",
        "method" => method,
        "path" => path
    )
    .record(duration);

    response
}

/// Increment the active streams gauge.
pub fn stream_opened() {
    gauge!("gateway_active_streams").increment(1.0);
}

/// Decrement the active streams gauge.
pub fn stream_closed() {
    gauge!("gateway_active_streams").decrement(1.0);
}

/// Handler that renders the Prometheus metrics scrape endpoint.
pub async fn metrics_handler(
    axum::extract::State(handle): axum::extract::State<PrometheusHandle>,
) -> impl IntoResponse {
    (StatusCode::OK, handle.render())
}
