use std::{net::SocketAddr, path::Path, sync::Arc, time::Duration};

use anyhow::Context;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::info;
use vsv2_common::{config::AppConfig, TenantId};
use vsv2_orchestrator::EndpointRegistry;
use vsv2_state::StateStore;

use vsv2_http::metadata_store::MetadataStore;
use vsv2_http::{app, bootstrap::build_sync_executor, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,vsv2=debug")),
        )
        .with_target(false)
        .init();

    let cfg = AppConfig::from_env()?;
    let bearer_token = std::env::var("VSV2_HTTP_BEARER_TOKEN")
        .context("VSV2_HTTP_BEARER_TOKEN must be set for vsv2-http")?;
    if bearer_token.trim().is_empty() {
        anyhow::bail!("VSV2_HTTP_BEARER_TOKEN must not be empty for vsv2-http");
    }
    let backfill_timeout_secs = std::env::var("VSV2_HTTP_BACKFILL_TIMEOUT_SECS")
        .ok()
        .map(|value| {
            value
                .parse::<u64>()
                .with_context(|| format!("invalid VSV2_HTTP_BACKFILL_TIMEOUT_SECS: {value}"))
        })
        .transpose()?
        .unwrap_or(300);

    let pool = vsv2_state::connect_pool(&cfg.pg_dsn)
        .await
        .context("connect postgres pool")?;
    let registry = EndpointRegistry::load_dir(Path::new("config/endpoints"))
        .context("load endpoint registry")?;
    let state_store = StateStore::connect(&cfg.pg_dsn, &cfg.redis_url)
        .await
        .context("connect state store")?;
    let applied_migrations = state_store
        .migrate()
        .await
        .context("apply migrations to the state database")?;
    if applied_migrations > 0 {
        tracing::info!(
            applied_migrations,
            "applied migrations to the state database"
        );
    }
    let stale_operations = state_store
        .fail_stale_sync_operations(
            "vsv2-http restarted before async operation completed; inspect runs_url for partial progress",
        )
        .await
        .context("mark stale async operations failed")?;
    if stale_operations > 0 {
        tracing::warn!(
            stale_operations,
            "marked stale async operations as failed at startup"
        );
    }
    let abandoned_runs = state_store
        .abandon_stale_runs(vsv2_state::STALE_RUN_AFTER)
        .await
        .context("mark abandoned sync runs failed")?;
    if abandoned_runs > 0 {
        tracing::warn!(
            abandoned_runs,
            "marked runs left 'running' by a dead process as failed at startup"
        );
    }
    if let Some(company_id) = &cfg.aquatiq_datamart_company_id {
        state_store
            .upsert_company_id(&TenantId::new(cfg.aquatiq_tenant_id.clone()), company_id)
            .await
            .context("upsert tenant company id")?;
    }
    let metadata_store = Arc::new(state_store.clone());
    let sync_runner = Arc::new(build_sync_executor(registry.clone(), state_store, &cfg));
    let state = AppState::new(pool, registry, sync_runner)
        .with_metadata_store(metadata_store)
        .with_bearer_token(bearer_token)
        .with_backfill_timeout(Duration::from_secs(backfill_timeout_secs));

    let addr: SocketAddr = std::env::var("VSV2_HTTP_BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".into())
        .parse()
        .context("parse VSV2_HTTP_BIND_ADDR")?;
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind http listener on {addr}"))?;

    info!(address = %addr, "vsv2-http: listening");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve http")
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }

    tracing::info!("vsv2-http: shutdown signal received");
}
