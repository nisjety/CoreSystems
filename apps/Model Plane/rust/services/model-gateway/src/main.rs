//! model-gateway — public boundary for HTTP/gRPC/SSE/WebSocket.
//!
//! HTTP on :8080, gRPC on :9090.
//! Emits ingress.accepted / ingress.rejected envelopes.

use anyhow::Result;
use model_gateway::{finetune_poller, gateway_metrics, grpc, http_routes, state};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("model-gateway")?;
    info!("model-gateway starting");

    let app_state = state::AppState::from_env().await?;
    let prom_handle = gateway_metrics::install_recorder().ok();

    let http_handle = tokio::spawn(http_routes::serve(app_state.clone(), prom_handle));
    let grpc_handle = tokio::spawn(grpc::serve(app_state.clone()));

    // Wave 7 slice 2c — fine-tuning poller. Only spawn when Azure is
    // configured; otherwise the gateway has no provider to refresh against.
    // The poller itself rechecks `FINETUNE_ENABLED` on every tick so an
    // operator can flip the kill switch without restarting.
    let poller_handle = app_state.azure_finetune.clone().map(|azure| {
        let client = app_state.finetune_jobs_client.clone();
        let publisher = app_state.publisher.clone();
        tokio::spawn(finetune_poller::run(azure, client, publisher))
    });

    // Graceful shutdown on SIGTERM
    let shutdown = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
        info!("SIGTERM received, shutting down");
    };

    if let Some(handle) = poller_handle {
        tokio::select! {
            result = http_handle => result??,
            result = grpc_handle => result??,
            result = handle => result??,
            () = shutdown => {},
        }
    } else {
        tokio::select! {
            result = http_handle => result??,
            result = grpc_handle => result??,
            () = shutdown => {},
        }
    }

    info!("model-gateway stopped");
    Ok(())
}
