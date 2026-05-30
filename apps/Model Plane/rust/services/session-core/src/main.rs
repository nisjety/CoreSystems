//! session-core — authoritative state for thread timeline, run metadata,
//! checkpoints, and context assembly.
//!
//! gRPC on :9091, HTTP health/metrics on :8081.

use anyhow::Result;
use metrics_exporter_prometheus::PrometheusBuilder;
use tracing::info;

mod compaction;
mod finetune_grpc;
mod grpc;
mod http_health;
mod nats;
mod orchestration_grpc;
mod orchestration_nats;
mod orchestration_store;
mod store;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("session-core")?;
    info!("session-core starting");

    let prom_handle = PrometheusBuilder::new().install_recorder()?;

    let pool = store::connect_postgres().await?;
    store::run_migrations(&pool).await?;

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());

    let (events_tx, _events_rx_keepalive) = tokio::sync::broadcast::channel::<
        mp_contracts::model_plane::v1::OrchestrationEvent,
    >(orchestration_grpc::EVENTS_CHANNEL_CAPACITY);

    let grpc_handle = tokio::spawn(grpc::serve(pool.clone(), events_tx.clone()));
    let http_handle = tokio::spawn(http_health::serve(prom_handle));
    let nats_handle = tokio::spawn(nats::run(pool.clone(), nats_url.clone()));
    let orchestration_nats_handle =
        tokio::spawn(orchestration_nats::run(nats_url, events_tx.clone()));
    let compaction_handle = tokio::spawn(compaction::run(pool.clone()));

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
        result = nats_handle => result??,
        result = orchestration_nats_handle => result??,
        result = compaction_handle => result??,
        () = shutdown => {},
    }

    pool.close().await;
    info!("session-core stopped");
    Ok(())
}
