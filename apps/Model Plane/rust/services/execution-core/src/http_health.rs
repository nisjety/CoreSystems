//! HTTP health/metrics endpoints for execution-core on :8083.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Router};
use tracing::info;

/// Process readiness shared by the gRPC and HTTP servers.
///
/// Liveness means the process can answer HTTP. Readiness is stricter: it is
/// true only while Execution Core's required gRPC listener is accepting work.
#[derive(Clone, Default)]
pub struct Readiness(Arc<AtomicBool>);

impl Readiness {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_grpc_ready(&self, ready: bool) {
        self.0.store(ready, Ordering::Release);
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Start the HTTP health server on :8083.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve(readiness: Readiness) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics_handler))
        .with_state(readiness);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8083").await?;
    info!("HTTP health listening on :8083");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

fn ready_status(readiness: &Readiness) -> StatusCode {
    if readiness.is_ready() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn readyz(State(readiness): State<Readiness>) -> impl IntoResponse {
    let status = ready_status(&readiness);
    let body = if status == StatusCode::OK {
        "ok"
    } else {
        "gRPC unavailable"
    };
    (status, body)
}

async fn metrics_handler() -> impl IntoResponse {
    (StatusCode::OK, "# HELP execution_core_steps_total\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_is_false_until_grpc_listener_is_bound() {
        let readiness = Readiness::new();

        assert!(!readiness.is_ready());
        assert_eq!(ready_status(&readiness), StatusCode::SERVICE_UNAVAILABLE);

        readiness.set_grpc_ready(true);
        assert!(readiness.is_ready());
        assert_eq!(ready_status(&readiness), StatusCode::OK);

        readiness.set_grpc_ready(false);
        assert!(!readiness.is_ready());
        assert_eq!(ready_status(&readiness), StatusCode::SERVICE_UNAVAILABLE);
    }
}
