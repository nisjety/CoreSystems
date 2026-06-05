mod api;
mod batch;
mod config;
mod provider;
mod qdrant_writer;
mod stream;
mod wiki_consumer;

use crate::config::Config;
use crate::provider::EmbeddingProvider;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "embedding_engine_rs=info".into()),
        )
        .json()
        .init();

    let cfg = Config::from_env()?;
    tracing::info!(
        batch_size = cfg.batch_size,
        collection = %cfg.qdrant_collection,
        "embedding-engine-rs starting"
    );

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;

    let qdrant = qdrant_client::Qdrant::from_url(&cfg.qdrant_url)
        .build()
        .map_err(|e| anyhow::anyhow!("qdrant connect: {e}"))?;

    qdrant_writer::ensure_collection(&qdrant, &cfg.qdrant_collection, cfg.embedding_dimension)
        .await?;
    // D4+D5 spec §2.3: provision wiki + entity-summary collections at boot so
    // wiki-store-go and the nightly summarizer can upsert without a separate
    // bootstrap step. Same vector dim as the primary collection — both use
    // the active Model Plane embedding route by default.
    qdrant_writer::ensure_collection(&qdrant, "wiki_block_embeddings", cfg.embedding_dimension)
        .await?;
    qdrant_writer::ensure_collection(
        &qdrant,
        "entity_summary_embeddings",
        cfg.embedding_dimension,
    )
    .await?;

    let provider = EmbeddingProvider::from_config(&cfg)?;
    tracing::info!(
        provider = provider.provider_name(),
        model = provider.model_name(),
        "embedding backend selected"
    );

    let nats_client = async_nats::connect(&cfg.nats_url).await?;
    let js = async_nats::jetstream::new(nats_client.clone());

    stream::setup_stream(&js).await?;
    let consumer = stream::create_consumer(&js).await?;

    // §16.3.8 — wiki publish subscriber: now DURABLE JetStream (own
    // DATAPLANE_WIKI stream + durable consumer) so wiki embeds survive restarts
    // and retry on failure, instead of best-effort core-NATS.
    if let Err(e) = wiki_consumer::spawn(js.clone(), qdrant.clone(), provider.clone()).await {
        tracing::warn!(error = %e, "wiki subscriber failed to start; continuing");
    }

    let admin_app = api::router();
    let admin_addr = format!("0.0.0.0:{}", cfg.admin_port);
    let admin_listener = tokio::net::TcpListener::bind(&admin_addr).await?;
    tracing::info!("admin on {admin_addr}");

    tokio::select! {
        res = axum::serve(admin_listener, admin_app) => {
            if let Err(e) = res { tracing::error!(err = %e, "admin error"); }
        }
        res = stream::run_consumer(consumer, pool, qdrant, provider, cfg, nats_client) => {
            if let Err(e) = res { tracing::error!(err = %e, "consumer error"); }
        }
    }

    Ok(())
}
