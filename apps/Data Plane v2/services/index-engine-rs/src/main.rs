mod api;
mod builder;
mod chunker;
mod config;
mod extract;
mod fingerprint;
mod gdpr;
mod gdpr_nats;
mod normalizer;
mod outbox;
mod stream;

use crate::config::Config;
use event_envelope_rs::{EventSigner, EventVerifier};
use std::sync::Arc;

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
        if cfg.documents_event_public_key_path.is_empty()
            || cfg.index_event_private_key_path.is_empty()
        {
            anyhow::bail!(
                "signed event consumers require producer public and local private key paths"
            );
        }
        let verifier = Arc::new(EventVerifier::from_rsa_pem(
            &std::fs::read(&cfg.documents_event_public_key_path)?,
            "service:documents-api-go",
            "documents-events-v1",
            &cfg.event_auth_audience,
            "events:documents:publish",
            100_000,
        )?);
        let signer = Arc::new(EventSigner::from_rsa_pem(
            &std::fs::read(&cfg.index_event_private_key_path)?,
            "service:index-engine-rs",
            "index-events-v1",
            &cfg.event_auth_audience,
            "events:index:publish",
        )?);
        let nats_client = nats_connection::connect(&cfg.nats_url).await?;
        let js = async_nats::jetstream::new(nats_client.clone());
        stream::setup_stream(&js).await?;
        Some((
            stream::create_consumer(&js).await?,
            nats_client,
            Some(verifier),
            Some(signer),
        ))
    } else if legacy_events_enabled {
        tracing::warn!("unsigned indexing mutation consumer enabled for insecure development");
        let nats_client = nats_connection::connect(&cfg.nats_url).await?;
        let js = async_nats::jetstream::new(nats_client.clone());
        stream::setup_stream(&js).await?;
        Some((stream::create_consumer(&js).await?, nats_client, None, None))
    } else {
        tracing::warn!(
            "indexing consumer disabled until signed producer-scoped envelopes are available"
        );
        None
    };

    // Admin/health server
    let admin_app = api::router();
    let admin_addr = format!("0.0.0.0:{}", cfg.admin_port);
    let admin_listener = tokio::net::TcpListener::bind(&admin_addr).await?;
    tracing::info!("admin server on {admin_addr}");

    // Cross-plane GDPR organization-erasure consumer. Binds to
    // AQENCIA_CONTROLPLANE on the shared cross-plane broker
    // (control-shared-nats) under the dedicated index-engine-gdpr identity —
    // never this service's own Data-Plane-local NATS_URL/DATAPLANE_NATS_TOKEN
    // connection to data-nats, which does not host that stream. Left unset,
    // the consumer is intentionally disabled rather than hot-looping doomed
    // connection attempts. See gdpr_nats.rs for the full rationale.
    let gdpr_pool = pool.clone();
    let gdpr_nats_url = std::env::var("NATS_SHARED_URL").unwrap_or_default();
    let gdpr_task = async move {
        if gdpr_nats_url.is_empty() {
            tracing::warn!("NATS_SHARED_URL not set; index-engine GDPR erasure consumer disabled");
            std::future::pending::<()>().await;
        }
        loop {
            let pool = gdpr_pool.clone();
            let nats_url = gdpr_nats_url.clone();
            match gdpr_nats::run(pool, nats_url).await {
                Ok(()) => tracing::warn!("GDPR erasure consumer ended; restarting"),
                Err(error) => {
                    tracing::warn!(%error, "GDPR erasure consumer failed; restarting")
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    };

    let consumer_task = async move {
        match event_runtime {
            Some((consumer, nats_client, verifier, signer)) => {
                let consumer_signer = signer.clone();
                let consumer_pool = pool.clone();
                let consumer_nats = nats_client.clone();
                let consumer = stream::run_consumer(
                    consumer,
                    consumer_pool,
                    cfg,
                    consumer_nats,
                    verifier,
                    consumer_signer,
                );
                match signer {
                    Some(signer) => {
                        let publisher = outbox::run_publisher(
                            pool,
                            async_nats::jetstream::new(nats_client),
                            signer,
                        );
                        tokio::try_join!(consumer, publisher).map(|_| ())
                    }
                    None => consumer.await,
                }
            }
            None => std::future::pending::<anyhow::Result<()>>().await,
        }
    };

    tokio::select! {
        res = axum::serve(admin_listener, admin_app) => {
            if let Err(e) = res { tracing::error!(err = %e, "admin server error"); }
        }
        res = consumer_task => {
            if let Err(e) = res { tracing::error!(err = %e, "consumer error"); }
        }
        () = gdpr_task => {}
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
    fn unsigned_index_mutations_require_two_explicit_dev_gates() {
        assert!(!unverified_legacy_events_enabled("", ""));
        assert!(!unverified_legacy_events_enabled("1", ""));
        assert!(!unverified_legacy_events_enabled("", "1"));
        assert!(unverified_legacy_events_enabled("1", "1"));
    }

    #[test]
    fn signed_consumer_requires_an_explicit_enablement() {
        assert!(!signed_event_consumers_enabled(""));
        assert!(!signed_event_consumers_enabled("true"));
        assert!(signed_event_consumers_enabled("1"));
    }
}
