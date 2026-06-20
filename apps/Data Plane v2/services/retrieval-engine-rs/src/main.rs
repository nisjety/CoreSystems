// See lib.rs — pre-existing clippy-1.94 cosmetic doc lint in untouched modules.
#![allow(clippy::doc_overindented_list_items)]

mod agent_config;
mod api;
mod audit;
mod authz;
mod cache;
mod config;
mod context_pack;
mod db;
mod embed;
mod grpc;
mod metrics;
mod pipeline;
mod rate_limit;
mod redact;
mod search;
mod telemetry;
mod trace;

use std::sync::Arc;
use std::time::Duration;

use tokio::signal;
use tonic::transport::Server as TonicServer;
use tower::limit::ConcurrencyLimitLayer;
use tower::load_shed::LoadShedLayer;
use tower::timeout::TimeoutLayer;

use crate::cache::CacheLayer;
use crate::config::Config;
use crate::embed::EmbeddingClient;
use crate::grpc::interceptor::ApiKeyInterceptor;
use crate::grpc::pb_documents::document_service_server::DocumentServiceServer;
use crate::grpc::pb_knowledge::knowledge_service_server::KnowledgeServiceServer;
use crate::grpc::pb_retrieval::retrieval_service_server::RetrievalServiceServer;
use crate::pipeline::orchestrator::RetrievalPipeline;
use crate::search::rerank::RerankClient;
use crate::search::sparse::{
    DynSparseSearchBackend, FallbackSparseBackend, PostgresSparseBackend, QuickwitSparseBackend,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init_tracing("retrieval-engine-rs");

    let cfg = Config::from_env()?;
    tracing::info!(
        http_port = cfg.http_port,
        grpc_port = cfg.grpc_port,
        hybrid = cfg.hybrid_enabled,
        "retrieval-engine-rs starting"
    );

    let pool = db::create_pool(&cfg.database_url).await?;

    let qdrant = qdrant_client::Qdrant::from_url(&cfg.qdrant_url)
        .build()
        .map_err(|e| anyhow::anyhow!("qdrant connect: {e}"))?;

    let embedder = EmbeddingClient::from_config(&cfg)?;
    tracing::info!(
        provider = embedder.provider_name(),
        model = embedder.model_name(),
        "embedding backend selected"
    );

    let reranker = cfg
        .cohere_api_key
        .as_ref()
        .filter(|k| !k.is_empty())
        .map(|key| RerankClient::new(key, &cfg.reranker_model));

    // Redis-compatible cache (Dragonfly in compose; optional and degraded to
    // no-op if unavailable).
    let cache_layer = match CacheLayer::connect(&cfg.redis_url).await {
        Ok(cache) => {
            tracing::info!("cache layer enabled");
            Some(cache)
        }
        Err(e) => {
            tracing::warn!("cache layer unavailable, running without cache: {e}");
            None
        }
    };

    // NATS client (optional — drives cache invalidator AND cost-ledger publish).
    // Connects once and reuses for both subscribers (invalidator) and publishers
    // (rerank cost events). If NATS_URL is unset or connect fails, both features
    // degrade gracefully to no-op.
    // §16.1.6 NATS env standardization: prefer `DPV2_NATS_URL` (this service),
    // fall back to `SHARED_NATS_URL` (cross-plane shared bus), then legacy
    // `NATS_URL` and `NATS_LOCAL_URL`. New deployments should set the first.
    let nats_client: Option<async_nats::Client> = match std::env::var("DPV2_NATS_URL")
        .or_else(|_| std::env::var("SHARED_NATS_URL"))
        .or_else(|_| std::env::var("NATS_URL"))
        .or_else(|_| std::env::var("NATS_LOCAL_URL"))
    {
        Ok(nats_url) => match async_nats::connect(&nats_url).await {
            Ok(nats) => {
                if let Some(cache) = cache_layer.as_ref() {
                    cache::invalidator::spawn_invalidator(nats.clone(), cache.clone());
                }
                Some(nats)
            }
            Err(e) => {
                tracing::warn!(error = %e, url = %nats_url, "NATS connect failed; cache invalidator + cost ledger disabled");
                None
            }
        },
        Err(_) => None,
    };

    // §16.5.5 JWKS — kick off background poller before the HTTP server
    // accepts traffic. If `JWT_JWKS_URL` is unset this is a no-op and we
    // continue with the static `JWT_PUBLIC_KEY_PEM` path.
    if let Some(_jwks) = authz::JwksCache::init_global() {
        tracing::info!("JWKS cache initialized; kid-based key lookup enabled");
    }

    // Wave 3 §15-B/C — build the policy client based on enforcement mode.
    // `off` → NoopPolicyClient (default for v2.3 back-compat).
    // `strict`/`permissive` → HttpPolicyClient hitting user-service + org-core.
    let policy: std::sync::Arc<dyn authz::PolicyClient> = match authz::EnforcementMode::from_env() {
        authz::EnforcementMode::Off => {
            tracing::info!("control-plane enforcement: off (NoopPolicyClient)");
            std::sync::Arc::new(authz::NoopPolicyClient)
        }
        mode => {
            let user_url = std::env::var("USER_SERVICE_HTTP_URL")
                .unwrap_or_else(|_| "http://user-service:3012".into());
            let org_url = std::env::var("ORG_CORE_HTTP_URL")
                .unwrap_or_else(|_| "http://org-core-service:8080".into());
            tracing::info!(?mode, %user_url, %org_url, "control-plane enforcement: on");
            std::sync::Arc::new(authz::HttpPolicyClient::new(user_url, org_url))
        }
    };

    let postgres_sparse: DynSparseSearchBackend =
        Arc::new(PostgresSparseBackend::new(pool.clone()));
    let sparse_backend: DynSparseSearchBackend =
        match cfg.sparse_search_backend.to_ascii_lowercase().as_str() {
            "quickwit" => {
                let quickwit: DynSparseSearchBackend = Arc::new(QuickwitSparseBackend::new(
                    cfg.quickwit_url.clone(),
                    cfg.quickwit_index_id.clone(),
                    cfg.quickwit_search_timeout_ms,
                ));
                Arc::new(FallbackSparseBackend::new(
                    quickwit,
                    postgres_sparse.clone(),
                ))
            }
            "postgres" => postgres_sparse,
            other => {
                tracing::warn!(
                    backend = other,
                    "unknown SPARSE_SEARCH_BACKEND; using postgres"
                );
                postgres_sparse
            }
        };
    tracing::info!(
        backend = sparse_backend.name(),
        "sparse search backend selected"
    );

    // Per-User Data Ownership & Sharing — resolves a viewer's explicit document
    // grants from user-core's resource_grants facade. ALWAYS-ON (not gated by
    // CONTROL_PLANE_ENFORCEMENT); fail-open to empty when user-core is
    // unreachable so owner + org/shared visibility still apply.
    let user_core_url =
        std::env::var("USER_CORE_HTTP_URL").unwrap_or_else(|_| "http://user-core:8080".into());
    let visibility: std::sync::Arc<dyn authz::VisibilityClient> = std::sync::Arc::new(
        authz::HttpVisibilityClient::new(user_core_url, cfg.internal_api_key.clone()),
    );
    // Evict the visibility cache on grant revoke so a revoke takes effect within
    // one query (5-min TTL is the backstop). Subscribes to the shared bus.
    if let Some(ref nats) = nats_client {
        authz::visibility::spawn_grant_invalidator(nats.clone(), visibility.clone());
    }

    let pipeline = Arc::new(RetrievalPipeline {
        pool: pool.clone(),
        qdrant,
        embedder,
        reranker,
        cache: cache_layer,
        config: cfg.clone(),
        policy,
        visibility,
        nats: nats_client,
        sparse_backend,
    });

    // Prometheus metrics
    let metrics_handle = Arc::new(metrics::init_metrics());

    // HTTP server (Axum)
    let app = api::router_with_metrics(pipeline.clone(), Some(metrics_handle));
    let http_addr = format!("0.0.0.0:{}", cfg.http_port);
    let http_listener = tokio::net::TcpListener::bind(&http_addr).await?;
    tracing::info!("http listening on {http_addr}");

    // gRPC server (Tonic) with Tower middleware stack
    let grpc_addr = format!("0.0.0.0:{}", cfg.grpc_port).parse()?;
    let interceptor = ApiKeyInterceptor::new(cfg.internal_api_key.clone());

    let retrieval_svc = grpc::retrieval_svc::RetrievalSvc::new(pipeline.clone());
    let document_svc = grpc::document_svc::DocumentSvc::new(Arc::new(pool.clone()));
    let knowledge_svc = grpc::knowledge_svc::KnowledgeSvc::new(Arc::new(pool));

    let grpc_timeout = Duration::from_secs(cfg.grpc_timeout_secs.into());

    // gRPC TLS: env-config plumbed in v2.3 wave 2. When both env vars are set,
    // the operator wants TLS — surface a clear warning if the build lacks the
    // `grpc-tls` cargo feature so they aren't fooled by a working plaintext
    // listener. Wiring `Server::builder().tls_config(...)` requires enabling
    // the tonic `tls-ring` feature, which is a workspace-wide change we
    // defer to v2.4. Until then this knob is a no-op + log line.
    if cfg.grpc_tls_cert_path.is_some() && cfg.grpc_tls_key_path.is_some() {
        tracing::warn!(
            cert = ?cfg.grpc_tls_cert_path,
            key = ?cfg.grpc_tls_key_path,
            "GRPC_TLS_CERT_PATH/KEY_PATH set but binary lacks `grpc-tls` feature; serving plaintext"
        );
    }

    let grpc_server = TonicServer::builder()
        .timeout(grpc_timeout)
        .concurrency_limit_per_connection(cfg.grpc_max_concurrent as usize)
        // §16.3.4 HTTP/2 keepalive — keep long-lived Model Plane connections
        // warm so intermittent traffic doesn't pay re-handshake cost. 30s ping
        // interval, 20s ping timeout, allow keepalive even with no active streams.
        .http2_keepalive_interval(Some(Duration::from_secs(30)))
        .http2_keepalive_timeout(Some(Duration::from_secs(20)))
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .layer(TimeoutLayer::new(grpc_timeout))
        .layer(ConcurrencyLimitLayer::new(cfg.grpc_max_concurrent as usize))
        .layer(LoadShedLayer::new())
        .add_service(RetrievalServiceServer::with_interceptor(
            retrieval_svc,
            interceptor.clone(),
        ))
        .add_service(DocumentServiceServer::with_interceptor(
            document_svc,
            interceptor.clone(),
        ))
        .add_service(KnowledgeServiceServer::with_interceptor(
            knowledge_svc,
            interceptor,
        ))
        .serve_with_shutdown(grpc_addr, shutdown_signal());

    tracing::info!("grpc listening on {grpc_addr}");

    // Run both servers with graceful shutdown
    let http_handle = tokio::spawn(async move {
        axum::serve(http_listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
    });

    let grpc_handle = tokio::spawn(grpc_server);

    tokio::select! {
        res = http_handle => {
            if let Ok(Err(e)) = res { tracing::error!("http server error: {e}"); }
        }
        res = grpc_handle => {
            if let Ok(Err(e)) = res { tracing::error!("grpc server error: {e}"); }
        }
    }

    tracing::info!("shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => { tracing::info!("ctrl+c received, starting graceful shutdown"); }
        _ = terminate => { tracing::info!("SIGTERM received, starting graceful shutdown"); }
    }
}
