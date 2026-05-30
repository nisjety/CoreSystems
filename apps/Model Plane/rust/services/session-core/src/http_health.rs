//! HTTP health/metrics endpoints for session-core on :8081.

use axum::{routing::get, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use tracing::info;

pub async fn serve(handle: PrometheusHandle) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ok" }))
        .route(
            "/metrics",
            get(move || {
                let h = handle.clone();
                async move { h.render() }
            }),
        );

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8081").await?;
    info!("HTTP health listening on :8081");
    axum::serve(listener, app).await?;
    Ok(())
}
