mod api;
mod auth;
mod community;
mod config;
mod contradiction;
mod extractor;
mod gdpr;
mod gdpr_nats;
mod grpc;
mod inference_auth;
mod model;
mod neo4j;
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

    // Neo4j graph read-model (Phase 2). Fail closed on a missing secret when
    // enabled; degrade (not crash) on a connectivity/schema error so the
    // service still serves the Postgres-backed graph path. Threaded into the
    // dual-write consumer + traverse endpoint in later phases.
    if cfg.neo4j_enabled && cfg.neo4j_password.trim().is_empty() {
        anyhow::bail!("NEO4J_ENABLED=true requires NEO4J_PASSWORD (fail closed on missing secret)");
    }
    // The core graph service (Postgres extraction/query) must NOT depend on the
    // optional read-model, so there is no compose `depends_on: neo4j`. Instead
    // we connect with a bounded boot retry (handles the startup race) and then
    // degrade to the Postgres fallback rather than blocking or crashing.
    // The window must outlast a Neo4j 5 (JVM) COLD start — 20-60s, longer on
    // first run when the image is still being pulled — else NEO4J_ENABLED=true
    // silently loses the race and the mirror stays off for the whole process
    // lifetime. Default ≈ 90s (30 × 3s), env-tunable via NEO4J_BOOT_ATTEMPTS.
    let neo4j: Option<Arc<neo4j::Neo4jClient>> = if cfg.neo4j_enabled {
        let attempts = cfg.neo4j_boot_attempts.max(1);
        let mut connected = None;
        for attempt in 1..=attempts {
            match neo4j::Neo4jClient::connect(&cfg).await {
                Ok(client) => match client.ensure_schema().await {
                    Ok(()) => {
                        tracing::info!(attempt, "neo4j graph read-model connected; schema ensured");
                        connected = Some(Arc::new(client));
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(attempt, attempts, err = %e, "neo4j schema bootstrap failed; retrying")
                    }
                },
                Err(e) => {
                    tracing::warn!(attempt, attempts, err = %e, "neo4j connect failed; retrying")
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
        if connected.is_none() {
            tracing::error!(
                attempts,
                "neo4j unavailable after boot retries; degrading to postgres graph fallback \
                 (POST /v1/graph/rebuild can backfill the mirror once neo4j is reachable)"
            );
        }
        connected
    } else {
        tracing::info!("neo4j graph read-model disabled (NEO4J_ENABLED unset)");
        None
    };
    tracing::info!(neo4j_read_model = neo4j.is_some(), "graph read-model state");

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
        std::env::var("APP_ENV").as_deref().unwrap_or(""),
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
        // D17: idempotent, shared definition in `nats_connection::dlq`.
        nats_connection::ensure_or_warn(&js).await;
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
        // D17: idempotent, shared definition in `nats_connection::dlq`.
        nats_connection::ensure_or_warn(&js).await;
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

    let graph_limits = api::GraphLimits {
        max_hops: cfg.graph_max_hops,
        max_entities: cfg.graph_traverse_max_entities as i64,
    };
    let app = api::router(
        store.clone(),
        jwt_verifier.clone(),
        neo4j.clone(),
        graph_limits,
    );
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

    // Cross-plane GDPR organization-erasure consumer. Deliberately
    // independent of `event_runtime` above (signed/legacy graph-extraction
    // triggers) and spawned as its own supervised task (not raced inside the
    // `tokio::select!` below) so a shared-broker outage never takes down
    // HTTP/gRPC/graph-extraction: it binds a pre-provisioned pull consumer on
    // the SHARED cross-plane broker (control-shared-nats, stream
    // AQENCIA_CONTROLPLANE) under its own dedicated `graph-index-gdpr`
    // identity (GRAPH_INDEX_GDPR_NATS_URL/_USER/_PASSWORD) — never this
    // service's own Data-Plane-local `cfg.nats_url` connection, which does
    // not host that stream. Left unset, the consumer is intentionally
    // disabled rather than hot-looping doomed connection attempts. See
    // `gdpr_nats` for the full provisioning contract this depends on.
    let gdpr_nats_url = std::env::var("GRAPH_INDEX_GDPR_NATS_URL").unwrap_or_default();
    if gdpr_nats_url.is_empty() {
        tracing::warn!(
            "GRAPH_INDEX_GDPR_NATS_URL not set; graph-index GDPR erasure consumer disabled"
        );
    } else {
        let gdpr_nats_user = std::env::var("GRAPH_INDEX_GDPR_NATS_USER").unwrap_or_default();
        let gdpr_nats_password =
            std::env::var("GRAPH_INDEX_GDPR_NATS_PASSWORD").unwrap_or_default();
        let gdpr_store = store.clone();
        tokio::spawn(gdpr_nats::run_supervised(
            gdpr_store,
            gdpr_nats_url,
            gdpr_nats_user,
            gdpr_nats_password,
        ));
    }

    let consumer_neo4j = neo4j.clone();
    let community_min_size = cfg.community_min_size;
    let consumer_task = async move {
        match event_runtime {
            Some((consumer, cleanup_consumer, nats, verifier, cleanup_verifier, _)) => {
                let extraction = stream::run_consumer(
                    consumer,
                    store.clone(),
                    extractor,
                    nats,
                    verifier,
                    consumer_neo4j,
                    community_min_size,
                );
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

fn unverified_legacy_events_enabled(legacy: &str, insecure_dev: &str, app_env: &str) -> bool {
    legacy == "1" && insecure_dev == "1" && !app_env.trim().eq_ignore_ascii_case("production")
}

fn signed_event_consumers_enabled(value: &str) -> bool {
    value == "1"
}

#[cfg(test)]
mod event_containment_tests {
    use super::{signed_event_consumers_enabled, unverified_legacy_events_enabled};

    #[test]
    fn unsigned_graph_mutations_require_two_explicit_dev_gates() {
        assert!(!unverified_legacy_events_enabled("", "", "development"));
        assert!(!unverified_legacy_events_enabled("1", "", "development"));
        assert!(!unverified_legacy_events_enabled("", "1", "development"));
        assert!(unverified_legacy_events_enabled("1", "1", "development"));
    }

    #[test]
    fn production_posture_rejects_unsigned_graph_mutations_even_with_dev_flags() {
        assert!(!unverified_legacy_events_enabled("1", "1", "production"));
        assert!(!unverified_legacy_events_enabled("1", "1", " Production "));
    }

    #[test]
    fn signed_consumer_requires_explicit_enablement() {
        assert!(!signed_event_consumers_enabled(""));
        assert!(signed_event_consumers_enabled("1"));
    }
}
