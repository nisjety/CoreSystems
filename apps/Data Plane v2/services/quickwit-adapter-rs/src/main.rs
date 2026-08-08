use std::sync::Arc;

use event_envelope_rs::EventVerifier;
use quickwit_adapter_rs::{
    api,
    auth::AdminVerifier,
    config::Config,
    gdpr_nats,
    jobs::{execute_claimed_job, AdminJobStore, PgAdminJobStore},
    quickwit::QuickwitClient,
    rebuild::RebuildContext,
    stream,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "quickwit_adapter_rs=info".into()),
        )
        .json()
        .init();

    let cfg = Config::from_env()?;
    let auth_public_key = cfg.auth_public_key()?;
    let admin_verifier = Arc::new(AdminVerifier::from_pem(
        &auth_public_key,
        &cfg.auth_audience,
        &cfg.auth_issuer,
    )?);
    tracing::info!(
        admin_port = cfg.admin_port,
        index = %cfg.quickwit_index_id,
        "quickwit-adapter-rs starting"
    );

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;
    let quickwit = QuickwitClient::new(
        cfg.quickwit_url.clone(),
        cfg.quickwit_index_id.clone(),
        cfg.quickwit_index_config_path.clone(),
    )?;
    quickwit.ensure_index().await?;

    let rebuild_ctx = Arc::new(RebuildContext {
        pool: pool.clone(),
        quickwit: quickwit.clone(),
        batch_size: cfg.batch_size,
    });
    let admin_jobs = Arc::new(PgAdminJobStore::new(
        pool.clone(),
        std::time::Duration::from_secs(cfg.admin_job_rate_seconds),
        std::time::Duration::from_secs(cfg.admin_job_lease_seconds),
    ));
    let runner_store = admin_jobs.clone();
    let runner_executor = rebuild_ctx.clone();
    tokio::spawn(async move {
        run_admin_job_worker(runner_store, runner_executor).await;
    });

    // The GDPR erasure consumer binds to AQENCIA_CONTROLPLANE on the shared
    // cross-plane broker (control-shared-nats), not this crate's own
    // Data-Plane-local `cfg.nats_url` (used below by `stream::spawn`/
    // `spawn_unverified_legacy`) — that broker never carries this subject.
    // NATS_SHARED_URL/NATS_SHARED_USER/NATS_SHARED_PASSWORD are the dedicated
    // `quickwit-adapter-gdpr` identity provisioned for this purpose alone.
    // Left unset, the consumer is intentionally disabled below rather than
    // hot-looping doomed connection attempts against this crate's own local
    // broker.
    let gdpr_nats_url = std::env::var("NATS_SHARED_URL").unwrap_or_default();
    if gdpr_nats_url.is_empty() {
        tracing::warn!("NATS_SHARED_URL not set; quickwit-adapter GDPR erasure consumer disabled");
    } else {
        let gdpr_pool = pool.clone();
        tokio::spawn(async move {
            run_gdpr_erasure_worker(gdpr_pool, gdpr_nats_url).await;
        });
    }

    if signed_event_consumers_enabled(
        std::env::var("ENABLE_SIGNED_EVENT_CONSUMERS")
            .as_deref()
            .unwrap_or(""),
    ) {
        let required_paths = [
            cfg.documents_event_public_key_path.as_str(),
            cfg.index_event_public_key_path.as_str(),
            cfg.embedding_event_public_key_path.as_str(),
            cfg.wiki_event_public_key_path.as_str(),
        ];
        anyhow::ensure!(
            required_paths.iter().all(|path| !path.trim().is_empty()),
            "signed Quickwit consumers require all producer public key paths"
        );
        let documents = event_verifier(
            &cfg.documents_event_public_key_path,
            "service:documents-api-go",
            "documents-events-v1",
            &cfg.event_auth_audience,
            "events:documents:publish",
        )?;
        let index = event_verifier(
            &cfg.index_event_public_key_path,
            "service:index-engine-rs",
            "index-events-v1",
            &cfg.event_auth_audience,
            "events:index:publish",
        )?;
        let embedding = event_verifier(
            &cfg.embedding_event_public_key_path,
            "service:embedding-engine-rs",
            "embedding-events-v1",
            &cfg.event_auth_audience,
            "events:embedding:publish",
        )?;
        let wiki = event_verifier(
            &cfg.wiki_event_public_key_path,
            "service:wiki-store-go",
            "wiki-events-v1",
            &cfg.event_auth_audience,
            "events:wiki:publish",
        )?;
        let security = Arc::new(stream::EventSecurity::new(
            documents, index, embedding, wiki,
        ));
        let nats = nats_connection::connect(&cfg.nats_url).await?;
        stream::spawn(nats, rebuild_ctx.clone(), security).await?;
    } else if unverified_legacy_events_enabled(
        std::env::var("ALLOW_UNVERIFIED_LEGACY_EVENTS")
            .as_deref()
            .unwrap_or(""),
        std::env::var("ALLOW_INSECURE_DEV_DEFAULTS")
            .as_deref()
            .unwrap_or(""),
        std::env::var("ISOLATED_E2E").as_deref().unwrap_or(""),
        std::env::var("DEPLOYMENT_ENVIRONMENT")
            .as_deref()
            .unwrap_or(""),
    ) {
        tracing::warn!("unsigned legacy Quickwit event consumers enabled for insecure development");
        let nats = nats_connection::connect(&cfg.nats_url).await?;
        stream::spawn_unverified_legacy(nats, rebuild_ctx.clone()).await?;
    } else {
        tracing::warn!("Quickwit live mutation consumers disabled until signed producer-scoped envelopes are available");
    }

    let app = api::router_with_store(rebuild_ctx, admin_verifier, admin_jobs);
    let addr = format!("0.0.0.0:{}", cfg.admin_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("admin server on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

fn event_verifier(
    path: &str,
    issuer: &str,
    key_id: &str,
    audience: &str,
    scope: &str,
) -> anyhow::Result<Arc<EventVerifier>> {
    Ok(Arc::new(EventVerifier::from_rsa_pem(
        &std::fs::read(path)?,
        issuer,
        key_id,
        audience,
        scope,
        100_000,
    )?))
}

fn signed_event_consumers_enabled(value: &str) -> bool {
    value == "1"
}

/// Runs the GDPR organization-erasure consumer forever, restarting on any
/// connection failure (missing pre-provisioned consumer, dropped broker
/// connection, etc.) rather than letting the task exit silently.
async fn run_gdpr_erasure_worker(pool: sqlx::PgPool, nats_url: String) {
    loop {
        if let Err(error) = gdpr_nats::run(pool.clone(), nats_url.clone()).await {
            tracing::error!(?error, "quickwit-adapter GDPR erasure consumer failed");
        } else {
            tracing::warn!("quickwit-adapter GDPR erasure consumer ended unexpectedly");
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}

async fn run_admin_job_worker(store: Arc<PgAdminJobStore>, executor: Arc<RebuildContext>) {
    let runner_id = format!("quickwit-adapter:{}", uuid::Uuid::new_v4());
    loop {
        match store.claim_next(&runner_id).await {
            Ok(Some(job)) => {
                if let Err(error) =
                    execute_claimed_job(store.as_ref(), executor.as_ref(), &runner_id, job).await
                {
                    tracing::error!(?error, "Quickwit admin job execution failed");
                }
            }
            Ok(None) => tokio::time::sleep(std::time::Duration::from_secs(2)).await,
            Err(error) => {
                tracing::error!(?error, "Quickwit admin job claim failed");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

fn unverified_legacy_events_enabled(
    legacy: &str,
    insecure_dev: &str,
    isolated_e2e: &str,
    deployment_environment: &str,
) -> bool {
    legacy == "1"
        && insecure_dev == "1"
        && isolated_e2e == "1"
        && deployment_environment == "isolated_e2e"
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("ctrl+c received"),
        _ = terminate => tracing::info!("SIGTERM received"),
    }
}

#[cfg(test)]
mod event_containment_tests {
    use super::{signed_event_consumers_enabled, unverified_legacy_events_enabled};

    #[test]
    fn unsigned_mutation_consumers_require_explicit_isolated_nonproduction_posture() {
        assert!(!unverified_legacy_events_enabled("", "", "", ""));
        assert!(!unverified_legacy_events_enabled("1", "", "", ""));
        assert!(!unverified_legacy_events_enabled("", "1", "", ""));
        assert!(!unverified_legacy_events_enabled("1", "1", "", ""));
        assert!(!unverified_legacy_events_enabled("1", "", "1", ""));
        assert!(!unverified_legacy_events_enabled("", "1", "1", ""));
        assert!(!unverified_legacy_events_enabled(
            "1",
            "1",
            "1",
            "production"
        ));
        assert!(unverified_legacy_events_enabled(
            "1",
            "1",
            "1",
            "isolated_e2e"
        ));
    }

    #[test]
    fn signed_mutation_consumers_require_explicit_enablement() {
        assert!(!signed_event_consumers_enabled(""));
        assert!(!signed_event_consumers_enabled("true"));
        assert!(signed_event_consumers_enabled("1"));
    }
}
