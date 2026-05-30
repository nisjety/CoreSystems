//! execution-core — runtime loop ownership for agent execution.
//!
//! gRPC on :9093, HTTP health/metrics on :8083.

use anyhow::Result;
use execution_core::state::StateStore;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("execution-core")?;
    info!("execution-core starting");

    let state = StateStore::new();

    let grpc_handle = tokio::spawn(execution_core::grpc::serve(state.clone()));
    let http_handle = tokio::spawn(execution_core::http_health::serve());

    let shutdown = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
        info!("SIGTERM received, shutting down");
    };

    tokio::select! {
        result = grpc_handle => result??,
        result = http_handle => result??,
        () = shutdown => {},
    }

    info!("execution-core stopped");
    Ok(())
}
