//! session-core — authoritative state for thread timeline, run metadata,
//! checkpoints, and context assembly.
//!
//! gRPC on :9091, HTTP health/metrics on :8081.

use anyhow::Result;
use metrics_exporter_prometheus::PrometheusBuilder;
use std::{future::Future, time::Duration};
use tracing::{info, warn};

mod approval_delivery;
mod audit_publisher;
mod auth;
mod compaction;
mod continuation_crypto;
mod dream_extractor;
mod dreaming;
mod finetune_grpc;
mod gdpr;
mod gdpr_nats;
mod grpc;
mod http_health;
mod learning_events;
mod letta_adapter;
mod memory_grpc;
mod nats;
mod nats_connection;
mod orchestration_grpc;
mod orchestration_nats;
mod orchestration_store;
mod routing_policy_grpc;
mod run_service_grpc;
mod service_token;
mod space_deletion_reconciler;
mod space_membership_nats;
mod store;
mod terminalization;

#[tokio::main]
async fn main() -> Result<()> {
    let _otel_guard = mp_telemetry::init("session-core")?;
    info!("session-core starting");

    // Authentication configuration and JWKS are mandatory and loaded before
    // any listener/background worker starts. A missing issuer/audience or an
    // unavailable verification keyset therefore fails startup closed.
    let grpc_auth = auth::JwtVerifier::from_env().await?;
    let prom_handle = PrometheusBuilder::new().install_recorder()?;

    let pool = store::connect_postgres().await?;
    store::run_migrations(&pool).await?;
    // The adapter is optional only when no Letta endpoint is configured. Once
    // enabled, its dedicated Auth Core service-principal configuration is
    // mandatory so background dreaming/search never falls back to anonymous
    // cross-service calls.
    let letta_memory = letta_adapter::LettaMemoryAdapter::from_env()?;

    let nats_url = std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".into());

    let (events_tx, _events_rx_keepalive) = tokio::sync::broadcast::channel::<
        mp_contracts::model_plane::v1::OrchestrationEvent,
    >(orchestration_grpc::EVENTS_CHANNEL_CAPACITY);

    let grpc_handle = tokio::spawn(grpc::serve(
        pool.clone(),
        events_tx.clone(),
        letta_memory.clone(),
        grpc_auth,
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
    let gdpr_pool = pool.clone();
    // The GDPR erasure consumer binds to AQENCIA_CONTROLPLANE on the shared
    // cross-plane broker (control-shared-nats), not session-core's own
    // Model-Plane-local NATS_URL — that broker never carries this subject.
    // NATS_SHARED_URL/NATS_SHARED_USER/NATS_SHARED_PASSWORD are the dedicated
    // session-core-gdpr identity provisioned for this purpose alone. Left
    // unset, the consumer is intentionally disabled below rather than
    // hot-looping doomed connection attempts against session-core's local
    // broker (which was the previous, incorrect behavior).
    let gdpr_nats_url = std::env::var("NATS_SHARED_URL").unwrap_or_default();
    let orchestration_events_tx = events_tx.clone();
    let orchestration_nats_handle = tokio::spawn(supervise_background(
        "session-core orchestration NATS bridge",
        move || {
            let nats_url = nats_url.clone();
            let events_tx = orchestration_events_tx.clone();
            async move { orchestration_nats::run(nats_url, events_tx).await }
        },
    ));
    let gdpr_erasure_handle = tokio::spawn(async move {
        if gdpr_nats_url.is_empty() {
            warn!("NATS_SHARED_URL not set; session-core GDPR erasure consumer disabled");
            // Park forever instead of hot-looping a connection that can
            // never succeed without shared-broker credentials configured.
            std::future::pending::<()>().await;
        }
        supervise_background("session-core GDPR erasure consumer", move || {
            let pool = gdpr_pool.clone();
            let nats_url = gdpr_nats_url.clone();
            async move { gdpr_nats::run(pool, nats_url).await }
        })
        .await
    });
    let space_membership_pool = pool.clone();
    // Same shared cross-plane broker as the GDPR consumer above, over its own
    // independent connection — a plain subscribe, not a JetStream durable
    // consumer, so unlike the GDPR path this needs no extra deployment
    // provisioning to function (see space_membership_nats.rs's module docs).
    let space_membership_nats_url = std::env::var("NATS_SHARED_URL").unwrap_or_default();
    let space_membership_handle = tokio::spawn(async move {
        if space_membership_nats_url.is_empty() {
            warn!(
                "NATS_SHARED_URL not set; session-core Space membership change consumer disabled"
            );
            std::future::pending::<()>().await;
        }
        supervise_background("session-core Space membership change consumer", move || {
            let pool = space_membership_pool.clone();
            let nats_url = space_membership_nats_url.clone();
            async move { space_membership_nats::run(pool, nats_url).await }
        })
        .await
    });
    let compaction_handle = tokio::spawn(compaction::run(pool.clone()));
    let dreaming_pool = pool.clone();
    let dreaming_letta_memory = letta_memory.clone();
    let dreaming_handle = tokio::spawn(supervise_background(
        "session-core Dreaming Core",
        move || {
            let pool = dreaming_pool.clone();
            let letta = dreaming_letta_memory.clone();
            async move { dreaming::run(pool, letta).await }
        },
    ));
    let semantic_reconciliation_handle = {
        let pool = pool.clone();
        let letta = letta_memory.clone();
        tokio::spawn(async move {
            match letta {
                Some(letta) => {
                    supervise_background(
                        "session-core Space deletion semantic-memory reconciler",
                        move || {
                            let pool = pool.clone();
                            let letta = letta.clone();
                            async move { space_deletion_reconciler::run(pool, letta).await }
                        },
                    )
                    .await
                }
                None => {
                    warn!("Letta is not configured; Space deletion semantic-memory reconciliation is disabled");
                    std::future::pending::<Result<()>>().await
                }
            }
        })
    };
    let terminalization_pool = pool.clone();
    let terminalization_handle = tokio::spawn(supervise_background(
        "session-core managed-run terminalization recovery",
        move || {
            let pool = terminalization_pool.clone();
            async move { terminalization::run_recovery_worker(pool).await }
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
        result = gdpr_erasure_handle => result??,
        result = space_membership_handle => result??,
        result = compaction_handle => result??,
        result = dreaming_handle => result??,
        result = semantic_reconciliation_handle => result??,
        result = terminalization_handle => result??,
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
