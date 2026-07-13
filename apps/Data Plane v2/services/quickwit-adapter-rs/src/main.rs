use std::sync::Arc;

use quickwit_adapter_rs::{
    api,
    auth::AdminVerifier,
    config::Config,
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

    if unverified_legacy_events_enabled(
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
        stream::spawn(nats, rebuild_ctx.clone()).await?;
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
    use super::unverified_legacy_events_enabled;

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
}
