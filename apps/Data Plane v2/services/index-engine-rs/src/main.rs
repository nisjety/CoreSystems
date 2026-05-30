mod api;
mod builder;
mod chunker;
mod config;
mod extract;
mod fingerprint;
mod normalizer;
mod stream;

use crate::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "index_engine_rs=info".into()),
        )
        .json()
        .init();

    let cfg = Config::from_env()?;
    tracing::info!(
        chunk_size = cfg.chunk_size,
        chunk_overlap = cfg.chunk_overlap,
        "index-engine-rs starting"
    );

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;
    tracing::info!("postgres connected");

    let nats_client = async_nats::connect(&cfg.nats_url).await?;
    let js = async_nats::jetstream::new(nats_client.clone());

    stream::setup_stream(&js).await?;
    let consumer = stream::create_consumer(&js).await?;

    // Admin/health server
    let admin_app = api::router();
    let admin_addr = format!("0.0.0.0:{}", cfg.admin_port);
    let admin_listener = tokio::net::TcpListener::bind(&admin_addr).await?;
    tracing::info!("admin server on {admin_addr}");

    tokio::select! {
        res = axum::serve(admin_listener, admin_app) => {
            if let Err(e) = res { tracing::error!(err = %e, "admin server error"); }
        }
        res = stream::run_consumer(consumer, pool, cfg, nats_client) => {
            if let Err(e) = res { tracing::error!(err = %e, "consumer error"); }
        }
    }

    Ok(())
}
