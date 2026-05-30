mod api;
mod config;
mod model;
mod quickwit;
mod rebuild;
mod stream;

use std::sync::Arc;

use crate::config::Config;
use crate::quickwit::QuickwitClient;
use crate::rebuild::RebuildContext;

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

    if cfg.rebuild_on_start {
        let ctx = rebuild_ctx.clone();
        tokio::spawn(async move {
            if let Err(err) = rebuild::rebuild_all(ctx, None, false).await {
                tracing::error!(error = %err, "startup Quickwit rebuild failed");
            }
        });
    }

    let nats = async_nats::connect(&cfg.nats_url).await?;
    stream::spawn(nats, rebuild_ctx.clone()).await?;

    let app = api::router(rebuild_ctx);
    let addr = format!("0.0.0.0:{}", cfg.admin_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("admin server on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
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
