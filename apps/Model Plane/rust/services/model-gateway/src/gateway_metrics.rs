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

/// Publish the zero-retention waiver contract version this gateway enforces, as the gauge
/// `gateway_zdr_waiver_contract_version`. The launcher reads it before it turns waivers on (Z-32),
/// so a gateway that does not know the version the minter issues is never sent one. Set once at
/// boot, after the recorder is installed.
pub fn record_zdr_waiver_contract_version() {
    gauge!("gateway_zdr_waiver_contract_version").set(f64::from(
        mp_contracts::retention::WAIVER_CONTRACT_VERSION,
    ));
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

/// How a run-control request (cancel, queue, probe) was answered. A closed set, so the
/// `outcome` label of `run_control_requests_total` cannot grow with request data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunControlOutcome {
    /// This replica owned the stream and handled it without the bus.
    Local,
    /// The replica that owns the stream answered over the bus.
    Peer,
    /// Nobody on the bus owns the stream (no responders).
    NoOwner,
    /// A subscriber was there but did not answer within the request timeout.
    Timeout,
    /// The bus could not carry the request at all.
    Error,
}

impl RunControlOutcome {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Peer => "peer",
            Self::NoOwner => "no_owner",
            Self::Timeout => "timeout",
            Self::Error => "error",
        }
    }
}

/// Count one run-control request. `op` is a [`crate::run_control::RunControlOp`] label.
pub fn run_control_request(op: &'static str, outcome: RunControlOutcome) {
    counter!(
        "run_control_requests_total",
        "op" => op,
        "outcome" => outcome.as_str()
    )
    .increment(1);
}

/// Handler that renders the Prometheus metrics scrape endpoint.
pub async fn metrics_handler(
    axum::extract::State(handle): axum::extract::State<PrometheusHandle>,
) -> impl IntoResponse {
    (StatusCode::OK, handle.render())
}
