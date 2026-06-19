//! session-core — authoritative state for thread timeline, run metadata,
//! checkpoints, and context assembly.
//!
//! gRPC on :9091, HTTP health/metrics on :8081.

use anyhow::Result;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::{future::Future, time::Duration};
use tracing::{info, warn};

mod audit_publisher;
mod compaction;
mod dreaming;
mod finetune_grpc;
mod grpc;
mod http_health;
mod letta_adapter;
mod memory_grpc;
mod nats;
mod orchestration_grpc;
mod orchestration_nats;
mod orchestration_store;
mod routing_policy_grpc;
mod run_service_grpc;
mod store;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("session-core")?;
    info!("session-core starting");

    let prom_handle = PrometheusBuilder::new().install_recorder()?;

    let pool = store::connect_postgres().await?;
    store::run_migrations(&pool).await?;
    let letta_memory = letta_adapter::LettaMemoryAdapter::from_env();

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());

    let (events_tx, _events_rx_keepalive) = tokio::sync::broadcast::channel::<
        mp_contracts::model_plane::v1::OrchestrationEvent,
    >(orchestration_grpc::EVENTS_CHANNEL_CAPACITY);

    let grpc_handle = tokio::spawn(grpc::serve(
        pool.clone(),
        events_tx.clone(),
        letta_memory.clone(),
    ));
    let http_handle = tokio::spawn(http_health::serve(prom_handle));
    let nats_pool = pool.clone();
    let nats_consumer_url = nats_url.clone();
    let nats_handle = tokio::spawn(supervise_background(
        "session-core NATS consumer",
        move || {
            let pool = nats_pool.clone();
            let nats_url = nats_consumer_url.clone();
            async move { nats::run(pool, nats_url).await }
        },
    ));
    let orchestration_events_tx = events_tx.clone();
    let orchestration_nats_handle = tokio::spawn(supervise_background(
        "session-core orchestration NATS bridge",
        move || {
            let nats_url = nats_url.clone();
            let events_tx = orchestration_events_tx.clone();
            async move { orchestration_nats::run(nats_url, events_tx).await }
        },
    ));
    let compaction_handle = tokio::spawn(compaction::run(pool.clone()));
    let dreaming_pool = pool.clone();
    let dreaming_handle = tokio::spawn(supervise_background(
        "session-core Dreaming Core",
        move || {
            let pool = dreaming_pool.clone();
            let letta = letta_memory.clone();
            async move { dreaming::run(pool, letta).await }
        },
    ));

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
        result = dreaming_handle => result??,
        () = shutdown => {},
    }

    pool.close().await;
    info!("session-core stopped");
    Ok(())
}

async fn supervise_background<F, Fut>(name: &'static str, mut run: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    loop {
        match run().await {
            Ok(()) => warn!(task = name, "background task ended; restarting"),
            Err(error) => warn!(task = name, %error, "background task failed; restarting"),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
