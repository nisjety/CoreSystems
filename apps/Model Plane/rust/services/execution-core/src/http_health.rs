//! HTTP health/metrics endpoints for execution-core on :8083.

use axum::{http::StatusCode, response::IntoResponse, routing::get, Router};
use tracing::info;

/// Start the HTTP health server on :8083.
///
/// # Errors
///
/// Returns an error if the server fails to bind or serve.
pub async fn serve() -> anyhow::Result<()> {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics_handler));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8083").await?;
    info!("HTTP health listening on :8083");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz() -> &'static str {
    "ok"
}

async fn metrics_handler() -> impl IntoResponse {
    (StatusCode::OK, "# HELP execution_core_steps_total\n")
}
