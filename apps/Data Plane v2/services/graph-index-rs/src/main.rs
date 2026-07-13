mod api;
mod auth;
mod community;
mod config;
mod extractor;
mod grpc;
mod model;
mod store;
mod stream;

use std::sync::Arc;

use crate::config::Config;
use event_envelope_rs::EventVerifier;

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
    let jwt_verifier = Arc::new(auth::JwtVerifier::from_env().await?);
    tracing::info!("graph-index-rs starting");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;

    let store = Arc::new(store::GraphStore::new(pool.clone()));
    let extractor = Arc::new(extractor::GraphExtractor::new(&cfg)?);

    let signed_events_enabled = signed_event_consumers_enabled(
        std::env::var("ENABLE_SIGNED_EVENT_CONSUMERS")
            .as_deref()
            .unwrap_or(""),
    );
    let legacy_events_enabled = unverified_legacy_events_enabled(
        std::env::var("ALLOW_UNVERIFIED_LEGACY_EVENTS")
            .as_deref()
            .unwrap_or(""),
        std::env::var("ALLOW_INSECURE_DEV_DEFAULTS")
            .as_deref()
            .unwrap_or(""),
    );
    let event_runtime = if signed_events_enabled {
        if cfg.embedding_event_public_key_path.is_empty()
            || cfg.index_event_public_key_path.is_empty()
        {
            anyhow::bail!(
                "signed graph consumers require embedding and index producer public key paths"
            );
        }
        let verifier = Arc::new(EventVerifier::from_rsa_pem(
            &std::fs::read(&cfg.embedding_event_public_key_path)?,
            "service:embedding-engine-rs",
            "embedding-events-v1",
            &cfg.event_auth_audience,
            "events:embedding:publish",
            100_000,
        )?);
        let cleanup_verifier = Arc::new(EventVerifier::from_rsa_pem(
            &std::fs::read(&cfg.index_event_public_key_path)?,
            "service:index-engine-rs",
            "index-events-v1",
            &cfg.event_auth_audience,
            "events:index:publish",
            100_000,
        )?);
        let nats = nats_connection::connect(&cfg.nats_url).await?;
        let js = async_nats::jetstream::new(nats.clone());
        stream::setup_stream(&js).await?;
        stream::setup_cleanup_stream(&js).await?;
        Some((
            stream::create_consumer(&js).await?,
            stream::create_cleanup_consumer(&js).await?,
            nats,
            Some(verifier),
            Some(cleanup_verifier),
            false,
        ))
    } else if legacy_events_enabled {
        tracing::warn!("unsigned graph mutation triggers enabled for insecure development");
        let nats = nats_connection::connect(&cfg.nats_url).await?;
        let js = async_nats::jetstream::new(nats.clone());
        stream::setup_stream(&js).await?;
        stream::setup_cleanup_stream(&js).await?;
        Some((
            stream::create_consumer(&js).await?,
            stream::create_cleanup_consumer(&js).await?,
            nats,
            None,
            None,
            true,
        ))
    } else {
        tracing::warn!("graph extraction/cleanup consumers disabled until signed producer-scoped envelopes are available");
        None
    };

    let app = api::router(store.clone(), jwt_verifier.clone());
    let addr = format!("0.0.0.0:{}", cfg.admin_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("graph-index HTTP on {addr}");

    // gRPC server on a separate port — same verified principal and tenant
    // binding contract as HTTP.
    let grpc_addr: std::net::SocketAddr = format!("0.0.0.0:{}", cfg.grpc_port).parse()?;
    let grpc_service = grpc::GraphServiceServer::with_interceptor(
        grpc::GraphGrpc::new(store.clone()),
        auth::JwtInterceptor::new(jwt_verifier),
    );
    tracing::info!("graph-index gRPC on {grpc_addr}");

    let consumer_task = async move {
        match event_runtime {
            Some((consumer, cleanup_consumer, nats, verifier, cleanup_verifier, _)) => {
                let extraction =
                    stream::run_consumer(consumer, store.clone(), extractor, nats, verifier);
                let cleanup =
                    stream::run_cleanup_consumer(cleanup_consumer, store, cleanup_verifier);
                tokio::try_join!(extraction, cleanup).map(|_| ())
            }
            None => std::future::pending::<anyhow::Result<()>>().await,
        }
    };

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res { tracing::error!(err = %e, "API server error"); }
        }
        res = tonic::transport::Server::builder().add_service(grpc_service).serve(grpc_addr) => {
            if let Err(e) = res { tracing::error!(err = %e, "gRPC server error"); }
        }
        res = consumer_task => {
            if let Err(e) = res { tracing::error!(err = %e, "consumer error"); }
        }
    }

    Ok(())
}

fn unverified_legacy_events_enabled(legacy: &str, insecure_dev: &str) -> bool {
    legacy == "1" && insecure_dev == "1"
}

fn signed_event_consumers_enabled(value: &str) -> bool {
    value == "1"
}

#[cfg(test)]
mod event_containment_tests {
    use super::{signed_event_consumers_enabled, unverified_legacy_events_enabled};

    #[test]
    fn unsigned_graph_mutations_require_two_explicit_dev_gates() {
        assert!(!unverified_legacy_events_enabled("", ""));
        assert!(!unverified_legacy_events_enabled("1", ""));
        assert!(!unverified_legacy_events_enabled("", "1"));
        assert!(unverified_legacy_events_enabled("1", "1"));
    }

    #[test]
    fn signed_consumer_requires_explicit_enablement() {
        assert!(!signed_event_consumers_enabled(""));
        assert!(signed_event_consumers_enabled("1"));
    }
}
