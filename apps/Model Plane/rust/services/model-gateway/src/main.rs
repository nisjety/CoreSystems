//! model-gateway — public boundary for HTTP/gRPC/SSE/WebSocket.
//!
//! HTTP on :8080, gRPC on :9090.
//! Emits ingress.accepted / ingress.rejected envelopes.

use anyhow::Result;
use model_gateway::{
    approvals, capability_consumer, doc_indexed_consumer, finetune_poller, gateway_metrics, grpc,
    http_routes, state,
};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("model-gateway")?;
    info!("model-gateway starting");

    let app_state = state::AppState::from_env().await?;
    let prom_handle = gateway_metrics::install_recorder().ok();

    let http_handle = tokio::spawn(http_routes::serve(app_state.clone(), prom_handle));
    let grpc_handle = tokio::spawn(grpc::serve(app_state.clone()));

    // D-1 (Phase 3 PR-4) — rehydrate the in-memory approval cache from
    // session-core's durable store so a restart never silently drops a pending
    // HITL gate. Best-effort and detached: if session-core is down it logs and
    // exits, never blocking or crashing boot. The durable store stays the path
    // of record regardless, so the cache simply warms on the next read-through.
    {
        let rehydrate_state = app_state.clone();
        tokio::spawn(async move {
            approvals::rehydrate_pending(
                &rehydrate_state.approvals,
                &mut rehydrate_state.orchestration_client.clone(),
            )
            .await;
        });
    }

    // §4.3 capability-registry cache-coherence consumer (read-path dual of the
    // H.1 MCP write-through). Detached best-effort daemon: it self-guards on
    // NATS_URL and never breaks the gateway when the bus is absent, so the
    // gateway's lifetime stays governed by the http/grpc servers + shutdown.
    tokio::spawn(capability_consumer::run(app_state.mcp.clone()));

    // Phase 4 — Data Plane v2 durable-retrieval readiness. Subscribes to
    // `dataplane.documents.indexed` and records (org, document) readiness so the
    // retrieval relay can await a just-ingested doc's embeddings landing instead
    // of guessing. Same best-effort, NATS_URL-self-guarding daemon shape as the
    // capability consumer above — absent NATS, it logs and exits.
    tokio::spawn(doc_indexed_consumer::run(app_state.doc_ready.clone()));

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
