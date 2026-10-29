use std::sync::Arc;

use axum::{routing::get, Json, Router};
use event_envelope_rs::EventVerifier;
use meilisearch_adapter_rs::{
    config::Config, indexer::IndexerContext, meilisearch::MeilisearchClient, stream,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "meilisearch_adapter_rs=info".into()),
        )
        .json()
        .init();

    let cfg = Config::from_env()?;
    tracing::info!(
        admin_port = cfg.admin_port,
        index = %cfg.meilisearch_index_uid,
        url = %cfg.meilisearch_url,
        "meilisearch-adapter-rs starting"
    );

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&cfg.database_url)
        .await?;

    let meilisearch = MeilisearchClient::new(
        cfg.meilisearch_url.clone(),
        cfg.meilisearch_index_uid.clone(),
        cfg.meilisearch_api_key.clone(),
    )?;
    meilisearch.ensure_index().await?;

    let indexer_ctx = Arc::new(IndexerContext {
        pool: pool.clone(),
        meilisearch,
    });

    if signed_event_consumers_enabled(
        std::env::var("ENABLE_SIGNED_EVENT_CONSUMERS")
            .as_deref()
            .unwrap_or(""),
    ) {
        let required_paths = [
            cfg.documents_event_public_key_path.as_str(),
            cfg.index_event_public_key_path.as_str(),
            cfg.embedding_event_public_key_path.as_str(),
        ];
        anyhow::ensure!(
            required_paths.iter().all(|path| !path.trim().is_empty()),
            "signed meilisearch-adapter consumers require all producer public key paths"
        );
        let documents = event_verifier(
            &cfg.documents_event_public_key_path,
            "service:documents-api-go",
            "documents-events-v1",
            &cfg.event_auth_audience,
            "events:documents:publish",
        )?;
        let index = event_verifier(
            &cfg.index_event_public_key_path,
            "service:index-engine-rs",
            "index-events-v1",
            &cfg.event_auth_audience,
            "events:index:publish",
        )?;
        let embedding = event_verifier(
            &cfg.embedding_event_public_key_path,
            "service:embedding-engine-rs",
            "embedding-events-v1",
            &cfg.event_auth_audience,
            "events:embedding:publish",
        )?;
        let security = Arc::new(stream::EventSecurity::new(documents, index, embedding));
        let nats = nats_connection::connect(&cfg.nats_url).await?;
        stream::spawn(nats, indexer_ctx.clone(), security).await?;
    } else {
        // No unsigned-legacy fallback in this adapter (unlike
        // quickwit-adapter-rs's isolated-e2e escape hatch): this is a v1
        // service with no existing isolated-harness dependency on an
        // unverified path, so leaving it out is a strictly smaller attack
        // surface rather than a missing feature. Add one later only if a
        // concrete isolated-test harness needs it.
        tracing::warn!(
            "meilisearch-adapter live mutation consumers disabled until \
             ENABLE_SIGNED_EVENT_CONSUMERS=1 and producer public keys are configured"
        );
    }

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .with_state(indexer_ctx);
    let addr = format!("0.0.0.0:{}", cfg.admin_port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("meilisearch-adapter health server on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn readyz(
    axum::extract::State(ctx): axum::extract::State<Arc<IndexerContext>>,
) -> (axum::http::StatusCode, Json<serde_json::Value>) {
    let db_ok = sqlx::query("SELECT 1").execute(&ctx.pool).await.is_ok();
    let meili_ok = ctx.meilisearch.healthy().await;
    let ready = db_ok && meili_ok;
    // The container HEALTHCHECK runs `curl -sf`, which treats any non-2xx as
    // failure — the status code, not just the JSON body, is what curl acts
    // on, so it must actually reflect `ready` or the healthcheck can never
    // catch a down dependency.
    let status = if ready {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(serde_json::json!({ "ready": ready, "database": db_ok, "meilisearch": meili_ok })),
    )
}

fn event_verifier(
    path: &str,
    issuer: &str,
    key_id: &str,
    audience: &str,
    scope: &str,
) -> anyhow::Result<Arc<EventVerifier>> {
    Ok(Arc::new(EventVerifier::from_rsa_pem(
        &std::fs::read(path)?,
        issuer,
        key_id,
        audience,
        scope,
        100_000,
    )?))
}

fn signed_event_consumers_enabled(value: &str) -> bool {
    value == "1"
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
mod tests {
    use super::signed_event_consumers_enabled;

    #[test]
    fn signed_mutation_consumers_require_explicit_enablement() {
        assert!(!signed_event_consumers_enabled(""));
        assert!(!signed_event_consumers_enabled("true"));
        assert!(signed_event_consumers_enabled("1"));
    }
}
