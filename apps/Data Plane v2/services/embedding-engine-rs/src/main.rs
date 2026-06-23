mod api;
mod batch;
mod config;
mod image_consumer;
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

    // Visual RAG arm — Cohere Embed v4 page-image embeddings. Online only when
    // COHERE_EMBED_V4_ENDPOINT is configured; otherwise the visual collection and
    // consumer are skipped (text-only deployment).
    match crate::provider::visual::VisualEmbeddingProvider::from_config(&cfg) {
        Ok(Some(visual)) => {
            tracing::info!(model = visual.model_name(), "visual embedding (Embed v4) enabled");
            if let Err(e) = qdrant_writer::ensure_collection(
                &qdrant,
                &cfg.qdrant_visual_collection,
                cfg.visual_embedding_dimension,
            )
            .await
            {
                tracing::warn!(error = %e, "visual collection ensure failed; continuing");
            }
            if let Err(e) = image_consumer::spawn(
                js.clone(),
                qdrant.clone(),
                visual,
                cfg.qdrant_visual_collection.clone(),
            )
            .await
            {
                tracing::warn!(error = %e, "page-image subscriber failed to start; continuing");
            }
        }
        Ok(None) => tracing::info!("visual embedding disabled (COHERE_EMBED_V4_ENDPOINT unset)"),
        Err(e) => {
            tracing::warn!(error = %e, "visual embedding misconfigured; continuing without visual arm")
        }
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
