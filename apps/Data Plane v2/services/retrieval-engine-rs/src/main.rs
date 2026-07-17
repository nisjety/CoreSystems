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
use crate::grpc::interceptor::{JwtInterceptor, JwtVerifier};
use crate::grpc::pb_documents::document_service_server::DocumentServiceServer;
use crate::grpc::pb_knowledge::knowledge_service_server::KnowledgeServiceServer;
use crate::grpc::pb_retrieval::retrieval_service_server::RetrievalServiceServer;
use crate::pipeline::orchestrator::RetrievalPipeline;
use crate::search::rerank::RerankClient;
use crate::search::sparse::{
    DynSparseSearchBackend, FallbackSparseBackend, PostgresSparseBackend, QuickwitSparseBackend,
};
use event_envelope_rs::{EventSigner, EventVerifier};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init_tracing("retrieval-engine-rs");

    // Verified-JWT path (per-user identity + audited admin read-bypass): the
    // interceptor/HTTP middleware verify Bearer tokens against
    // `JWT_PUBLIC_KEY_PEM`. We support a mounted key file too (secrets-as-files,
    // matching auth-core's `CONVEX_AUTH_PUBLIC_KEY_FILE`) — hydrate the inline
    // env from the file once at startup so the verify code stays unchanged.
    // Unset file → no-op; the API-key + forwarded x-user-id path is unaffected.
    if let Ok(path) = std::env::var("JWT_PUBLIC_KEY_FILE") {
        let path = path.trim();
        if !path.is_empty() {
            match std::fs::read_to_string(path) {
                Ok(pem) if !pem.trim().is_empty() => {
                    std::env::set_var("JWT_PUBLIC_KEY_PEM", pem.trim());
                    tracing::info!(file = path, "loaded JWT public key from file");
                }
                Ok(_) => tracing::warn!(file = path, "JWT_PUBLIC_KEY_FILE is empty"),
                Err(e) => {
                    tracing::warn!(file = path, error = %e, "failed to read JWT_PUBLIC_KEY_FILE")
                }
            }
        }
    }

    let cfg = Config::from_env()?;
    let (document_event_verifier, retrieval_event_signer) = if signed_event_consumers_enabled(
        std::env::var("ENABLE_SIGNED_EVENT_CONSUMERS")
            .as_deref()
            .unwrap_or(""),
    ) {
        anyhow::ensure!(
            !cfg.documents_event_public_key_path.trim().is_empty()
                && !cfg.retrieval_event_private_key_path.trim().is_empty(),
            "signed retrieval events require producer verification and local signing key paths"
        );
        let verifier = Arc::new(EventVerifier::from_rsa_pem(
            &std::fs::read(&cfg.documents_event_public_key_path)?,
            "service:documents-api-go",
            "documents-events-v1",
            &cfg.event_auth_audience,
            "events:documents:publish",
            100_000,
        )?);
        let signer = Arc::new(EventSigner::from_rsa_pem(
            &std::fs::read(&cfg.retrieval_event_private_key_path)?,
            "service:retrieval-engine-rs",
            "retrieval-events-v1",
            &cfg.event_auth_audience,
            "events:retrieval:publish",
        )?);
        (Some(verifier), Some(signer))
    } else {
        (None, None)
    };
    let grpc_jwt_verifier = Arc::new(JwtVerifier::from_env()?);
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
        .map(|key| {
            RerankClient::with_endpoint(
                key,
                &cfg.reranker_model,
                &cfg.rerank_endpoint,
                !cfg.rerank_use_api_key,
            )
        });

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
        Ok(nats_url) => match nats_connection::connect(&nats_url).await {
            Ok(nats) => {
                if let Some(cache) = cache_layer.as_ref() {
                    if let Some(verifier) = document_event_verifier.as_ref() {
                        cache::invalidator::spawn_invalidator(
                            nats.clone(),
                            cache.clone(),
                            verifier.clone(),
                        );
                    } else {
                        tracing::warn!(
                            "retrieval cache invalidation disabled until signed producer events are configured"
                        );
                    }
                }
                Some(nats)
            }
            Err(e) => {
                tracing::warn!(error = %e, "NATS connect failed; cache invalidator + cost ledger disabled");
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

    // Control membership authorization defaults strict. The no-op client exists
    // only for an explicit insecure local-development posture.
    let policy: std::sync::Arc<dyn authz::PolicyClient> = match authz::EnforcementMode::from_env() {
        authz::EnforcementMode::Off => {
            anyhow::ensure!(
                std::env::var("ALLOW_INSECURE_DEV_DEFAULTS").as_deref() == Ok("1"),
                "CONTROL_PLANE_ENFORCEMENT=off requires ALLOW_INSECURE_DEV_DEFAULTS=1"
            );
            tracing::warn!("control-plane enforcement disabled for insecure local development");
            std::sync::Arc::new(authz::NoopPolicyClient)
        }
        mode => {
            let token_url = std::env::var("CONTROL_POLICY_TOKEN_URL").unwrap_or_else(|_| {
                "http://auth-core:3011/api/control-policy/internal-token".into()
            });
            let decision_url = std::env::var("CONTROL_PLANE_DECISION_URL").unwrap_or_else(|_| {
                "http://auth-core:3011/api/v1/internal/authorization/data-plane/decision".into()
            });
            let service_id = std::env::var("CONTROL_POLICY_SERVICE_ID")
                .map_err(|_| anyhow::anyhow!("CONTROL_POLICY_SERVICE_ID is required"))?;
            let service_api_key = std::env::var("CONTROL_POLICY_SERVICE_API_KEY")
                .map_err(|_| anyhow::anyhow!("CONTROL_POLICY_SERVICE_API_KEY is required"))?;
            anyhow::ensure!(
                !service_id.trim().is_empty(),
                "CONTROL_POLICY_SERVICE_ID is empty"
            );
            anyhow::ensure!(
                !service_api_key.trim().is_empty(),
                "CONTROL_POLICY_SERVICE_API_KEY is empty"
            );
            tracing::info!(?mode, %token_url, %decision_url, "control-plane enforcement: strict");
            std::sync::Arc::new(authz::HttpPolicyClient::new(
                token_url,
                decision_url,
                service_id,
                service_api_key,
            ))
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
    // user-core HTTP listens on :3012 (NOT :8080 — that was a fail-open landmine:
    // a wrong port makes every grant lookup error → empty grants → shares
    // silently never resolve). Matches documents-api's USER_CORE_URL.
    let user_core_url =
        std::env::var("USER_CORE_HTTP_URL").unwrap_or_else(|_| "http://user-core:3012".into());
    let visibility: std::sync::Arc<dyn authz::VisibilityClient> = std::sync::Arc::new(
        authz::HttpVisibilityClient::new(user_core_url, cfg.user_core_service_token.clone()),
    );
    // Grant reads are intentionally uncached in the secure MVP. This keeps
    // revocations effective on the next request even when the cross-plane
    // shared event bus is unavailable or misrouted.

    // Visual RAG arm query embedder (Cohere Embed v4). Online only when
    // COHERE_EMBED_V4_ENDPOINT is set; otherwise the visual arm stays dark and
    // `w_visual` has no effect (text-only deployment).
    let visual_embedder = match crate::embed::visual::VisualQueryEmbedder::from_config(&cfg) {
        Ok(Some(ve)) => {
            tracing::info!("visual query embedder (Embed v4) enabled");
            Some(ve)
        }
        Ok(None) => {
            tracing::info!("visual query embedder disabled (COHERE_EMBED_V4_ENDPOINT unset)");
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "visual query embedder misconfigured; visual arm disabled");
            None
        }
    };

    // ColQwen visual reranker — only when explicitly enabled AND an endpoint is
    // set. The model runs as a separate GPU inference server (local for
    // verification, Hetzner/Azure for prod); OFF by default everywhere else.
    let colqwen = if cfg.visual_rerank_enabled {
        match crate::search::colqwen::ColqwenClient::from_url(&cfg.colqwen_endpoint_url) {
            Some(c) => {
                tracing::info!(endpoint = %cfg.colqwen_endpoint_url, "ColQwen visual reranker enabled");
                Some(c)
            }
            None => {
                tracing::warn!("VISUAL_RERANK_ENABLED set but COLQWEN_ENDPOINT_URL is empty; visual reranker disabled");
                None
            }
        }
    } else {
        None
    };

    // Deep multi-hop graph arm — graph-index's traverse endpoint (Neo4j
    // read-model). Disabled by emptying GRAPH_INDEX_URL; bearer-less requests
    // skip it regardless (the arm degrades to in-process 1-hop grounding).
    let graph_remote = crate::search::graph_remote::GraphTraverseClient::from_config(
        &cfg.graph_index_url,
        cfg.graph_remote_timeout_ms,
    );
    if graph_remote.is_some() {
        tracing::info!(url = %cfg.graph_index_url, "deep graph arm (traverse endpoint) enabled");
    } else {
        tracing::info!("deep graph arm disabled (GRAPH_INDEX_URL empty); in-process 1-hop only");
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
        event_signer: retrieval_event_signer,
        sparse_backend,
        visual_embedder,
        colqwen,
        graph_remote,
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
    let interceptor = JwtInterceptor::new(grpc_jwt_verifier);

    let retrieval_svc = grpc::retrieval_svc::RetrievalSvc::new(pipeline.clone());
    let document_svc = grpc::document_svc::DocumentSvc::new(
        Arc::new(pool.clone()),
        pipeline.policy.clone(),
        pipeline.visibility.clone(),
    );
    let knowledge_svc = grpc::knowledge_svc::KnowledgeSvc::new(
        Arc::new(pool),
        pipeline.policy.clone(),
        pipeline.visibility.clone(),
    );

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

fn signed_event_consumers_enabled(value: &str) -> bool {
    value == "1"
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
