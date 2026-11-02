//! Prometheus metrics setup and middleware for model-gateway.
//!
//! Tracks: `gateway_requests_total`, `gateway_request_duration_seconds`,
//! and `gateway_active_streams`.

use std::time::Instant;

#[cfg(test)]
use std::sync::atomic::{AtomicI64, Ordering};

use axum::{
    extract::{MatchedPath, Request},
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

/// The `route` label value for a request that matched no route.
pub const UNMATCHED_ROUTE: &str = "unmatched";

/// The route label for a request: the matched route TEMPLATE (`/v1/threads/:thread_id`), never
/// the raw URI path, so thread, run and space ids cannot enter Prometheus. A request that
/// matched no route (the fallback) is `unmatched`. The middleware is a `Router::layer`, which
/// wraps each route, so axum has set [`MatchedPath`] before it runs.
fn route_label(request: &Request) -> String {
    request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| UNMATCHED_ROUTE.to_owned(), |m| m.as_str().to_owned())
}

/// Axum middleware that records request count, duration, and status metrics.
pub async fn metrics_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let route = route_label(&request);
    let start = Instant::now();

    let response = next.run(request).await;

    let status = response.status().as_u16().to_string();
    let duration = start.elapsed().as_secs_f64();

    counter!(
        "gateway_requests_total",
        "method" => method.clone(),
        "route" => route.clone(),
        "status" => status
    )
    .increment(1);

    histogram!(
        "gateway_request_duration_seconds",
        "method" => method,
        "route" => route
    )
    .record(duration);

    response
}

/// Test-only mirror of how many [`ActiveStreamGuard`]s are alive. The metrics facade has no
/// reader without a recorder, so the unit test reads the gauge through a local recorder and
/// checks this counter beside it. It is process-wide, which is safe because no lib test streams
/// through the handler, the one place a guard is opened.
#[cfg(test)]
static ACTIVE_STREAMS_MIRROR: AtomicI64 = AtomicI64::new(0);

/// One unit of the `gateway_active_streams` gauge: opening it counts a stream, dropping it
/// uncounts that stream. The count was once kept by hand, a call after the opening frame and one
/// per exit path, and the stopped and cancelled exits, an early return before the error-path
/// close and a panic skipped the close, so the gauge stayed high for good. Move the guard into the
/// task whose lifetime is the stream's: every way that task can end then uncounts the stream, and
/// no exit path has to remember anything. Never `mem::forget` one.
#[must_use = "dropping the guard uncounts the stream at once"]
pub struct ActiveStreamGuard {
    _private: (),
}

impl ActiveStreamGuard {
    /// Count one open stream. The gauge comes back down when the returned guard drops.
    #[must_use]
    pub fn open() -> Self {
        gauge!("gateway_active_streams").increment(1.0);
        #[cfg(test)]
        ACTIVE_STREAMS_MIRROR.fetch_add(1, Ordering::SeqCst);
        Self { _private: () }
    }
}

impl Drop for ActiveStreamGuard {
    fn drop(&mut self) {
        gauge!("gateway_active_streams").decrement(1.0);
        #[cfg(test)]
        ACTIVE_STREAMS_MIRROR.fetch_sub(1, Ordering::SeqCst);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// What the scrape endpoint would render for `gateway_active_streams`; zero until it is set.
    fn rendered_gauge(handle: &PrometheusHandle) -> f64 {
        handle
            .render()
            .lines()
            .find_map(|line| line.strip_prefix("gateway_active_streams "))
            .map_or(0.0, |value| value.parse().expect("the gauge renders a number"))
    }

    /// Two open streams count two, and each drop uncounts exactly its own stream. The gauge is
    /// read through a recorder that only this thread sees, so these are the facade's own calls.
    #[test]
    fn active_stream_guard_counts_each_open_stream_once() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        let mirror = ACTIVE_STREAMS_MIRROR.load(Ordering::SeqCst);
        metrics::with_local_recorder(&recorder, || {
            let first = ActiveStreamGuard::open();
            let second = ActiveStreamGuard::open();
            assert_eq!(rendered_gauge(&handle), 2.0);
            assert_eq!(ACTIVE_STREAMS_MIRROR.load(Ordering::SeqCst), mirror + 2);
            drop(first);
            assert_eq!(rendered_gauge(&handle), 1.0);
            assert_eq!(ACTIVE_STREAMS_MIRROR.load(Ordering::SeqCst), mirror + 1);
            drop(second);
            assert_eq!(rendered_gauge(&handle), 0.0);
            assert_eq!(ACTIVE_STREAMS_MIRROR.load(Ordering::SeqCst), mirror);
        });
    }
}
