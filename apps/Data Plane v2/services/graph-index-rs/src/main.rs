mod api;
mod community;
mod config;
mod extractor;
mod grpc;
mod model;
mod store;
mod stream;

use std::sync::Arc;

use crate::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "graph_index_rs=info".into()),
        )
        .json()
        .init();

    let cfg = Config::from_env()?;
    tracing::info!("graph-index-rs starting");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;

    let store = Arc::new(store::GraphStore::new(pool.clone()));
    let extractor = Arc::new(extractor::GraphExtractor::new(&cfg));

    let nats_client = async_nats::connect(&cfg.nats_url).await?;
    let js = async_nats::jetstream::new(nats_client.clone());

    stream::setup_stream(&js).await?;
    let consumer = stream::create_consumer(&js).await?;

    let app = api::router(store.clone());
    let addr = format!("0.0.0.0:{}", cfg.admin_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("graph-index HTTP on {addr}");

    // gRPC server on a separate port — same store, same logic, alternate wire.
    let grpc_addr: std::net::SocketAddr = format!("0.0.0.0:{}", cfg.grpc_port).parse()?;
    let grpc_service = grpc::GraphGrpc::new(store.clone()).into_server();
    tracing::info!("graph-index gRPC on {grpc_addr}");

    // Purge graph mappings for chunks orphaned by a content re-chunk so updated
    // documents don't leave stale entities/relationships in graph retrieval.
    stream::spawn_orphan_cleanup(nats_client.clone(), store.clone()).await?;

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { tracing::error!(err = %e, "API server error"); }
        }
        res = tonic::transport::Server::builder().add_service(grpc_service).serve(grpc_addr) => {
            if let Err(e) = res { tracing::error!(err = %e, "gRPC server error"); }
        }
        res = stream::run_consumer(consumer, store, extractor, pool, nats_client) => {
            if let Err(e) = res { tracing::error!(err = %e, "consumer error"); }
        }
    }

    Ok(())
}
