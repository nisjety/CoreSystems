//! quarry-edge-rs — public ingest boundary.
//!
//! Owns: `/v1/scrape`, `/v1/crawl`, `/v1/batch`, SSE, auth, validation,
//! preflight, cache resolution, handoff to runtime (fast path) or
//! orchestrator (scheduled).

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use quarry_core::output::DriverKind;
use tokio::net::TcpListener;

use quarry_browser::browserbase::{BrowserbaseConfig, BrowserbaseDriver};
use quarry_runtime::artifact_store::{ArtifactStore, FilesystemStore, InMemoryStore};
use quarry_runtime::browser_driver::BrowserDriverAdapter;
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::fetch::StaticDriver;
use quarry_runtime::ingest_client::IngestClient;
use quarry_runtime::lease_pool::RuntimeLeasePool;
use quarry_runtime::tls_driver::TlsProfileDriver;
use quarry_runtime::StepReceiptStore;
use quarry_security::preflight::DefaultEngine;
use quarry_tls::TlsProfile;

mod agent_routes;
mod fleet_routes;
mod answer_routes;
mod api_error;
mod audio_routes;
mod auth;
mod cache;
mod canary;
mod change_routes;
// Only consumed by the postgres-queue-gated fan-out in change_routes; the
// bin target has no external consumers, so an ungated declaration would be
// dead code in default-feature builds.
#[cfg(feature = "postgres-queue")]
mod change_webhook;
mod config;
mod experiments;
mod extract_routes;
mod graphql;
mod handoff;
mod internal_auth;
mod map_routes;
mod profile_routes;
mod queue_routes;
mod resource_routes;
mod routes;
mod schedule_routes;
mod search_routes;
mod source_registrar;
mod state;
#[cfg(test)]
mod test_support;
mod telemetry;

/// How often the local-index maintenance task wakes. Short relative to the
/// commit interval so the document-count trigger below reacts promptly during
/// a crawl burst instead of waiting out a full interval.
const LOCAL_INDEX_COMMIT_POLL: Duration = Duration::from_secs(1);
/// Commit the local index at least this often. A crash loses at most this much
/// recent corpus, which for a re-fetchable cache is an acceptable trade against
/// committing on every document (each commit writes a segment).
const LOCAL_INDEX_COMMIT_INTERVAL: Duration = Duration::from_secs(30);
/// …or this many uncommitted documents, whichever comes first.
const LOCAL_INDEX_COMMIT_DOCS: u64 = 200;
/// How often to run the age/ceiling retention pass. Hourly: the bounds are
/// measured in days and millions of documents, so a tighter cadence would only
/// burn merge cycles.
const LOCAL_INDEX_RETENTION_INTERVAL: Duration = Duration::from_secs(3_600);
/// Upper bound on how long shutdown waits for the final commit.
const LOCAL_INDEX_SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_secs(20);

/// Resolves when the process is asked to stop: Ctrl-C anywhere, or SIGTERM on
/// unix (what Kubernetes and `docker stop` send).
///
/// Introduced with the local-index commit ticker — the edge previously ran
/// `axum::serve(...).await` with no graceful shutdown at all, so there was no
/// point at which any component could flush. Anything else needing a clean
/// stop should hang off this same signal rather than adding a second one.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!(error = %e, "failed to install Ctrl-C handler");
            // Never resolving is correct here: a broken handler must not be
            // read as "shutdown requested".
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("shutdown: Ctrl-C received"),
        _ = terminate => tracing::info!("shutdown: SIGTERM received"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    telemetry::init_telemetry();

    // Production safety net: `QUARRY_EDGE_AUTH_DEV_BYPASS=1` accepts any
    // bearer without verifying it against auth-core's JWKS. That's fine
    // for dev (orchestrator calls /v1/internal/run_page with a static
    // shared token), but in production it would let anyone with network
    // reach to edge run arbitrary pages on tenants' behalf. We refuse
    // to start if both `ENVIRONMENT=prod` (or `production`) and the
    // bypass are set — operators must explicitly choose one.
    {
        let env_name = std::env::var("ENVIRONMENT")
            .or_else(|_| std::env::var("APP_ENV"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        let bypass = std::env::var("QUARRY_EDGE_AUTH_DEV_BYPASS")
            .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);
        if bypass && (env_name == "prod" || env_name == "production") {
            tracing::error!(
                env = %env_name,
                "REFUSING TO START: QUARRY_EDGE_AUTH_DEV_BYPASS=1 is enabled but \
                 ENVIRONMENT={env_name}. Auth dev-bypass accepts any bearer and \
                 must never run in production. Unset QUARRY_EDGE_AUTH_DEV_BYPASS \
                 or change ENVIRONMENT.",
            );
            anyhow::bail!("auth dev-bypass enabled with ENVIRONMENT={env_name}; refusing to start");
        }
        if bypass && env_name == "dev" {
            tracing::info!("QUARRY_EDGE_AUTH_DEV_BYPASS is ON for local development");
        } else if bypass {
            tracing::warn!(
                env = %env_name,
                "QUARRY_EDGE_AUTH_DEV_BYPASS is ON — every bearer accepted without \
                 verification. Safe only in dev environments."
            );
        }
    }

    let cfg = config::EdgeConfig::from_env()?;
    let environment = std::env::var("ENVIRONMENT")
        .or_else(|_| std::env::var("APP_ENV"))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(environment.as_str(), "prod" | "production") {
        let secret = cfg
            .internal_secret
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!("QUARRY_EDGE__INTERNAL_SECRET is required in production")
            })?;
        internal_auth::InternalSigner::new(secret).map_err(|error| {
            anyhow::anyhow!("invalid QUARRY_EDGE__INTERNAL_SECRET in production: {error}")
        })?;
    }

    if cfg.cross_plane_auth_dev_bypass && matches!(environment.as_str(), "prod" | "production") {
        anyhow::bail!(
            "QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS must never be enabled in production"
        );
    }
    let durability_required = matches!(environment.as_str(), "prod" | "production")
        || std::env::var("QUARRY_EDGE__REQUIRE_DURABLE_RUNTIME")
            .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes"))
            .unwrap_or(false);
    let require_browser_grants =
        cfg.require_browser_grants || matches!(environment.as_str(), "prod" | "production");
    let browser_grant_http_configured = cfg
        .browser_grant_validator_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    let browser_grant_grpc_configured = cfg
        .browser_grant_validator_grpc_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    #[cfg(not(feature = "grpc"))]
    if browser_grant_grpc_configured {
        anyhow::bail!("QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL requires the grpc feature");
    }
    if require_browser_grants && !browser_grant_http_configured && !browser_grant_grpc_configured {
        anyhow::bail!(
            "browser grants are required but no BrowserBroker validation endpoint is configured"
        );
    }
    if durability_required
        && matches!(
            cfg.artifact_backend.trim().to_ascii_lowercase().as_str(),
            "memory" | "in_memory" | ""
        )
    {
        anyhow::bail!(
            "durable runtime is required but QUARRY_EDGE__ARTIFACT_BACKEND is in-memory; configure fs or s3"
        );
    }
    if durability_required
        && cfg
            .database_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .is_none()
    {
        anyhow::bail!("durable runtime is required but QUARRY_EDGE__DATABASE_URL is unset");
    }
    let cross_plane_tokens: Option<quarry_runtime::service_tokens::SharedServiceTokenProvider> =
        match cfg
            .quarry_service_api_key
            .as_deref()
            .filter(|credential| !credential.trim().is_empty())
        {
            Some(credential) => Some(Arc::new(
                quarry_runtime::service_tokens::AuthCoreServiceTokenProvider::new(
                    &cfg.auth_core_url,
                    credential,
                )?,
            )),
            None => None,
        };
    let cross_plane_configured = cfg.data_plane_url.is_some()
        || cfg.data_plane_ingest_url.is_some()
        || cfg.model_plane_url.is_some();
    if cross_plane_configured && cross_plane_tokens.is_none() && !cfg.cross_plane_auth_dev_bypass {
        anyhow::bail!(
            "QUARRY_EDGE__QUARRY_SERVICE_API_KEY is required when Data or Model Plane is configured"
        );
    }

    let redis = if let Some(url) = &cfg.redis_url {
        let client = redis::Client::open(url.as_str())?;
        Some(redis::aio::ConnectionManager::new(client).await?)
    } else {
        None
    };

    // Every local Chromium instance is forced through the loopback proxy.
    // The proxy resolves once and dials the vetted IP directly; this is the
    // transport complement to the per-request Fetch policy listener.
    #[cfg(feature = "browser-agent")]
    let browser_egress_proxy = Arc::new(quarry_runtime::PinnedBrowserEgressProxy::new());

    let timeout = Duration::from_secs(cfg.fetch_timeout_s);
    // Proxy pool — `QUARRY_PROXY_POOL` is a `;`-separated list of
    // proxy URIs (socks5://, socks5h://, http://, https://). Empty /
    // unset → direct egress only (the previous behaviour).
    let proxy_pool = std::env::var("QUARRY_PROXY_POOL")
        .map(|s| quarry_runtime::proxy_pool::ProxyPool::from_env_string(&s))
        .unwrap_or_else(|_| quarry_runtime::proxy_pool::ProxyPool::empty());
    if !proxy_pool.is_empty() {
        tracing::info!(count = proxy_pool.len(), "proxy pool wired");
    }
    let proxy_processor_id = if env_flag("QUARRY_PROXY_FIRST_PARTY") {
        tracing::info!("proxy pool classified as first-party egress");
        None
    } else if proxy_pool.is_empty() {
        None
    } else {
        let processor_id = std::env::var("QUARRY_PROXY_PROCESSOR_ID")
            .unwrap_or_else(|_| "quarry_proxy_pool".into());
        tracing::info!(%processor_id, "proxy pool requires request privacy processor approval");
        Some(processor_id)
    };
    let static_driver = StaticDriver::with_proxy_pool_and_processor(
        timeout,
        &cfg.user_agent,
        proxy_pool,
        proxy_processor_id,
    )
    .map_err(|e| anyhow::anyhow!("static driver init: {e:?}"))?;

    let mut drivers = DriverRegistry::new(DriverKind::Static);
    drivers.register(Arc::new(static_driver));

    match TlsProfileDriver::new(TlsProfile::Chrome, timeout, None) {
        Ok(tls) => {
            tracing::info!("tls driver registered (chrome profile)");
            drivers.register(Arc::new(tls));
        }
        Err(e) => {
            tracing::warn!(error = %e, "tls driver init failed; fallback to static only");
        }
    }

    if let (Some(api_key), Some(project_id)) =
        (&cfg.browserbase_api_key, &cfg.browserbase_project_id)
    {
        let bb_config = BrowserbaseConfig {
            api_key: api_key.clone(),
            project_id: project_id.clone(),
            base_url: cfg
                .browserbase_url
                .clone()
                .unwrap_or_else(|| "https://www.browserbase.com".into()),
            context_id: None,
            recording: false,
        };
        let bb_driver = Arc::new(BrowserbaseDriver::new(bb_config));
        let pool = Arc::new(RuntimeLeasePool::new(4));
        let processor_id = std::env::var("QUARRY_BROWSERBASE_PROCESSOR_ID")
            .unwrap_or_else(|_| "browserbase".into());
        let adapter = BrowserDriverAdapter::managed_provider(bb_driver, pool, processor_id);
        drivers.register(Arc::new(adapter));
        tracing::info!("browser driver registered (browserbase)");
    }

    #[cfg(feature = "browser-agent")]
    if !drivers.has(DriverKind::Browser) {
        let browser_driver: Arc<dyn quarry_browser::BrowserDriver> = Arc::new(
            quarry_browser::chromiumoxide::ChromiumoxideDriver::new()
                .with_pinned_egress_proxy(browser_egress_proxy.clone()),
        );
        let pool = Arc::new(RuntimeLeasePool::new(2));
        let adapter = BrowserDriverAdapter::new(browser_driver, pool);
        drivers.register(Arc::new(adapter));
        tracing::info!("browser driver registered (local chromiumoxide)");
    }

    let default_driver = drivers
        .default_driver()
        .ok_or_else(|| anyhow::anyhow!("no drivers available"))?;

    let security = if let Some(path) = cfg
        .security_snapshot_path
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        match DefaultEngine::new().with_persistence(path).await {
            Ok(engine) => {
                tracing::info!(path, "loaded quarry security snapshot");
                engine
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    path,
                    "could not load quarry security snapshot; using in-memory policy",
                );
                DefaultEngine::new()
            }
        }
    } else {
        DefaultEngine::new()
    };
    let artifacts_durable = matches!(
        cfg.artifact_backend.trim().to_ascii_lowercase().as_str(),
        "fs" | "filesystem" | "s3"
    );
    let artifacts: Arc<dyn ArtifactStore> = match cfg.artifact_backend.as_str() {
        "fs" | "filesystem" => {
            tracing::info!(root = %cfg.artifact_root, "artifact backend: filesystem");
            Arc::new(FilesystemStore::new(&cfg.artifact_root)?)
        }
        "s3" => {
            let bucket = cfg
                .s3_bucket
                .clone()
                .ok_or_else(|| anyhow::anyhow!("QUARRY_EDGE__S3_BUCKET required for s3 backend"))?;
            tracing::info!(%bucket, "artifact backend: s3");
            Arc::new(quarry_runtime::artifact_store::S3Store::new(bucket).await?)
        }
        _ => {
            tracing::info!("artifact backend: in-memory");
            Arc::new(InMemoryStore::new())
        }
    };

    // Event publisher: edge → control-plane event stream.
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(1024);
    let publisher_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let publisher = quarry_runtime::EventPublisher::new(
        event_rx,
        publisher_client,
        cfg.control_base_url.clone(),
        cfg.control_api_key.clone(),
    );
    tokio::spawn(async move { publisher.run().await });

    // P1 / cluster #nats — optional NATS JetStream fan-out. When
    // `QUARRY_EDGE__NATS_URL` is set, every event emitted by handlers is
    // also published to JetStream so cross-plane consumers
    // (autocomplete-core, model-plane, org-core) see them on a durable
    // subject. Connection failure is non-fatal: we log and fall back to
    // the mpsc-only sink so the service still boots.
    let event_sink = quarry_runtime::EventSink::new(event_tx);
    // The NatsEventBus also seeds the P3 billing meter. Keep it as an
    // `Option<NatsEventBus>` so we can derive a `NatsUsageMeter` from
    // the same connection below.
    let mut shared_nats_bus: Option<quarry_runtime::NatsEventBus> = None;
    let event_sink = if let Some(url) = cfg.nats_url.as_deref().filter(|s| !s.is_empty()) {
        let mut nats_cfg = quarry_runtime::nats_event_bus::NatsConfig::new(url);
        if let Some(prefix) = cfg.nats_subject_prefix.as_deref().filter(|s| !s.is_empty()) {
            nats_cfg.subject_prefix = prefix.to_string();
        }
        if let Some(creds) = cfg.nats_creds_file.as_deref().filter(|s| !s.is_empty()) {
            nats_cfg = nats_cfg.with_credentials(creds);
        } else if let Some(token) = cfg.nats_token.as_deref().filter(|s| !s.is_empty()) {
            nats_cfg = nats_cfg.with_token(token);
        }
        match quarry_runtime::NatsEventBus::connect(nats_cfg).await {
            Ok(bus) => {
                tracing::info!(%url, "nats event fan-out: connected (JetStream)");
                shared_nats_bus = Some(bus.clone());
                let bus_arc: Arc<dyn quarry_runtime::EventBus> = Arc::new(bus);
                event_sink.with_nats(bus_arc)
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    %url,
                    "nats connect failed — falling back to mpsc-only sink"
                );
                event_sink
            }
        }
    } else {
        tracing::info!("nats event fan-out: disabled (no QUARRY_EDGE__NATS_URL)");
        event_sink
    };

    // P3 / cluster #billing — Quarry-Edge → Control-Plane billing-core
    // usage meter. When NATS is up we publish to `usage.>` subjects;
    // otherwise we noop so dev / offline boots silently. Billing-core's
    // HTTP recovery endpoint covers any gap if NATS drops mid-flight.
    let usage: Arc<dyn quarry_runtime::UsageMeter> = match shared_nats_bus.as_ref() {
        Some(bus) => {
            tracing::info!("usage meter: NATS (publishes to usage.>)");
            Arc::new(quarry_runtime::NatsUsageMeter::from_bus(bus))
        }
        None => {
            tracing::warn!("usage meter: noop (no NATS) — billing-core will see no usage events");
            Arc::new(quarry_runtime::NoopUsageMeter)
        }
    };

    let cache = redis
        .as_ref()
        .map(|conn| cache::PageCache::new(conn.clone(), Duration::from_secs(cfg.cache_ttl_secs)));

    // P2 / cluster #grpc — pick HTTP or gRPC transport for Data Plane
    // ingest. The default is HTTP (`IngestClient`). When the `grpc`
    // cargo feature is compiled in AND `QUARRY_EDGE__DATA_PLANE_TRANSPORT`
    // is set to `grpc`, the runtime instead uses `GrpcIngestAdapter` —
    // protobuf over HTTP/2 with TLS. Both impls satisfy the
    // `DataPlaneIngest` trait so PageRunner stays transport-agnostic.
    // Ingest (durable write) goes to the documents-api write route, which is a
    // different service than the retrieval engine used for reads/vector search.
    // Prefer the dedicated ingest URL; fall back to `data_plane_url` so older
    // single-URL deployments keep working.
    let ingest_url = cfg
        .data_plane_ingest_url
        .as_ref()
        .filter(|s| !s.is_empty())
        .or(cfg.data_plane_url.as_ref());
    let ingest: Option<Arc<dyn quarry_runtime::ingest_client::DataPlaneIngest>> = match ingest_url {
        Some(url) => {
            if let Some(provider) = cross_plane_tokens.clone() {
                if cfg.data_plane_transport.as_deref() == Some("grpc") {
                    tracing::warn!(
                        "dynamic per-org auth currently selects HTTP ingest; gRPC static bearer reuse is disabled"
                    );
                }
                tracing::info!(%url, "data plane ingest: HTTP/JSON with per-org Auth Core tokens");
                Some(Arc::new(IngestClient::with_token_provider(url, provider)?)
                    as Arc<dyn quarry_runtime::ingest_client::DataPlaneIngest>)
            } else if cfg.cross_plane_auth_dev_bypass {
                let key = cfg
                    .data_plane_service_token
                    .as_deref()
                    .filter(|token| !token.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "QUARRY_EDGE__DATA_PLANE_SERVICE_TOKEN is required by the explicit development bypass"
                        )
                    })?;
                let transport = cfg
                    .data_plane_transport
                    .as_deref()
                    .unwrap_or("http")
                    .to_ascii_lowercase();
                match transport.as_str() {
                    #[cfg(feature = "grpc")]
                    "grpc" => match quarry_runtime::grpc::GrpcDataPlaneClient::connect(url).await {
                        Ok(mut grpc) => {
                            grpc = grpc.with_bearer_token(key)?;
                            tracing::info!(%url, "data plane ingest: gRPC (HTTP/2 + protobuf)");
                            Some(quarry_runtime::grpc::ingest_adapter_into_dyn(
                                quarry_runtime::grpc::GrpcIngestAdapter::new(Arc::new(grpc)),
                            ))
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "gRPC data plane connect failed — falling back to HTTP ingest"
                            );
                            Some(Arc::new(IngestClient::new(url, key)?)
                                as Arc<dyn quarry_runtime::ingest_client::DataPlaneIngest>)
                        }
                    },
                    #[cfg(not(feature = "grpc"))]
                    "grpc" => {
                        tracing::warn!(
                            "data_plane_transport=grpc but binary built without --features grpc; \
                             falling back to HTTP ingest"
                        );
                        Some(Arc::new(IngestClient::new(url, key)?)
                            as Arc<dyn quarry_runtime::ingest_client::DataPlaneIngest>)
                    }
                    _ => {
                        tracing::info!(%url, "data plane ingest: HTTP/JSON (default)");
                        Some(Arc::new(IngestClient::new(url, key)?)
                            as Arc<dyn quarry_runtime::ingest_client::DataPlaneIngest>)
                    }
                }
            } else {
                unreachable!("cross-plane auth preflight rejects this configuration")
            }
        }
        None => {
            tracing::info!("data plane ingest client not configured (no DATA_PLANE_URL)");
            None
        }
    };

    // C30.1 / cluster #6 — pick ProfileStore backend. When
    // `profile_store_kind == "postgres"` AND the binary was compiled
    // with `--features postgres-queue` AND `database_url` is set,
    // we build PostgresProfileStore (durable, multi-instance-safe).
    // Otherwise the dev-default `InMemoryProfileStore` runs. Either
    // backend can be wrapped with Redis hot-cache via
    // `cached_profile_store::maybe_cache`.
    #[allow(unused_mut)]
    let mut profiles_durable = false;
    let inner_profiles: Arc<dyn quarry_browser::session::ProfileStore> = {
        #[cfg(feature = "postgres-queue")]
        {
            match (
                cfg.profile_store_kind.as_deref(),
                cfg.database_url.as_deref().filter(|s| !s.is_empty()),
            ) {
                (Some("postgres"), Some(dsn)) => match sqlx::postgres::PgPool::connect(dsn).await {
                    Ok(pool) => {
                        profiles_durable = true;
                        tracing::info!("ProfileStore: PostgresProfileStore (durable)");
                        Arc::new(
                            quarry_runtime::postgres_profile_store::PostgresProfileStore::new(pool),
                        )
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "PgPool connect failed; falling back to InMemoryProfileStore");
                        Arc::new(quarry_browser::session::InMemoryProfileStore::new())
                    }
                },
                _ => {
                    tracing::info!("ProfileStore: InMemoryProfileStore (dev/test)");
                    Arc::new(quarry_browser::session::InMemoryProfileStore::new())
                }
            }
        }
        #[cfg(not(feature = "postgres-queue"))]
        {
            if cfg.profile_store_kind.as_deref() == Some("postgres") {
                tracing::warn!(
                    "profile_store_kind=postgres requested but binary built without \
                     --features postgres-queue; falling back to InMemoryProfileStore"
                );
            }
            Arc::new(quarry_browser::session::InMemoryProfileStore::new())
        }
    };
    // Optional Redis read-through cache (1h TTL by default).
    let profiles =
        quarry_runtime::cached_profile_store::maybe_cache(inner_profiles, cfg.redis_url.as_deref())
            .await;

    // P0 action proof. A production agent must not report readiness while
    // receipts would disappear with the edge process. The durable store uses
    // the same migration set as the queue/history stores and enforces org_id
    // in every list query.
    #[allow(unused_mut)]
    let mut receipts_durable = false;
    let receipts: Arc<dyn StepReceiptStore> = {
        #[cfg(feature = "postgres-queue")]
        {
            match cfg.database_url.as_deref().filter(|s| !s.is_empty()) {
                Some(dsn) => match sqlx::postgres::PgPool::connect(dsn).await {
                    Ok(pool) => match quarry_runtime::migrations::run_migrations(&pool).await {
                        Ok(()) => {
                            receipts_durable = true;
                            tracing::info!("step receipts: Postgres (durable, tenant-bound)");
                            Arc::new(quarry_runtime::PostgresStepReceiptStore::new(pool))
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, "step receipt migrations failed; using in-memory store");
                            Arc::new(quarry_runtime::InMemoryStepReceiptStore::new())
                        }
                    },
                    Err(error) => {
                        tracing::warn!(error = %error, "step receipt database connect failed; using in-memory store");
                        Arc::new(quarry_runtime::InMemoryStepReceiptStore::new())
                    }
                },
                None => Arc::new(quarry_runtime::InMemoryStepReceiptStore::new()),
            }
        }
        #[cfg(not(feature = "postgres-queue"))]
        {
            Arc::new(quarry_runtime::InMemoryStepReceiptStore::new())
        }
    };

    // Rust remains the durable frontier writer. The internal Go/Temporal
    // bridge receives a clone of this pool and binds tenant-scoped queue
    // handles per request; a missing pool is surfaced through readiness.
    #[cfg(feature = "postgres-queue")]
    let queue_pool = match cfg.database_url.as_deref().filter(|s| !s.is_empty()) {
        Some(dsn) => match sqlx::postgres::PgPool::connect(dsn).await {
            Ok(pool) => match quarry_runtime::migrations::run_migrations(&pool).await {
                Ok(()) => Some(pool),
                Err(error) => {
                    tracing::warn!(error = %error, "frontier migrations failed; durable queue routes disabled");
                    None
                }
            },
            Err(error) => {
                tracing::warn!(error = %error, "frontier database connect failed; durable queue routes disabled");
                None
            }
        },
        None => None,
    };

    let grant_validator: Arc<dyn quarry_runtime::GrantValidator> = {
        #[cfg(feature = "grpc")]
        if let Some(url) = cfg
            .browser_grant_validator_grpc_url
            .as_deref()
            .filter(|url| !url.trim().is_empty())
        {
            let validator = quarry_runtime::grpc::GrpcGrantValidator::connect(url).await?;
            let validator = match cfg.model_plane_token.as_deref() {
                Some(token) if cfg.cross_plane_auth_dev_bypass => {
                    validator.with_bearer_token(token.to_owned())
                }
                _ => validator,
            };
            Arc::new(validator)
        } else {
            browser_grant_http_validator(&cfg)?
        }
        #[cfg(not(feature = "grpc"))]
        {
            browser_grant_http_validator(&cfg)?
        }
    };

    let durable_history_configured = cfg.durable_event_history;
    let frontier_durable = {
        #[cfg(feature = "postgres-queue")]
        {
            queue_pool.is_some()
        }
        #[cfg(not(feature = "postgres-queue"))]
        {
            false
        }
    };
    let durable_ready = !durability_required
        || (artifacts_durable
            && profiles_durable
            && receipts_durable
            && frontier_durable
            && durable_history_configured
            && cfg
                .database_url
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()));
    if durability_required && !durable_ready {
        anyhow::bail!(
            "durable runtime could not be established (artifact/profile/history backend fell back to ephemeral storage)"
        );
    }

    // C30.1 / cluster #7 — durable job-history store. Optional;
    // requires postgres-queue feature + DSN + durable_event_history
    // toggle. When wired, `/v1/runs/:id/events` serves locally.
    #[cfg(feature = "postgres-queue")]
    let event_history: Option<
        Arc<quarry_runtime::postgres_event_history::PostgresEventHistory>,
    > = if cfg.durable_event_history {
        match cfg.database_url.as_deref().filter(|s| !s.is_empty()) {
            Some(dsn) => match sqlx::postgres::PgPool::connect(dsn).await {
                Ok(pool) => {
                    tracing::info!("durable event history: enabled");
                    Some(Arc::new(
                        quarry_runtime::postgres_event_history::PostgresEventHistory::new(pool),
                    ))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "durable event history: PgPool connect failed");
                    None
                }
            },
            None => {
                tracing::warn!("durable_event_history=true but database_url is unset; disabling");
                None
            }
        }
    } else {
        None
    };

    // C30.2 / cluster #9 — durable baseline + diff store.
    //
    // Phase 1 Track C: apply the embedded migration set against the DSN
    // BEFORE wiring the store. Historically this block only connected a
    // pool — `sqlx::migrate!` ran solely in test helpers — so a real
    // edge wired with `QUARRY_EDGE__DATABASE_URL` hit an empty database
    // and `/v1/change/check` failed with "relation quarry_baselines does
    // not exist" instead of serving a real comparison. We now run
    // migrations explicitly and only wire the store when they succeed,
    // so the route degrades to an honest 501 (store = None) rather than
    // a 500 against missing tables. Point a dedicated `quarry_edge`
    // database at the DSN to avoid `_sqlx_migrations` checksum
    // collisions with services sharing the same Postgres server.
    #[cfg(feature = "postgres-queue")]
    let baseline_store: Option<
        Arc<quarry_runtime::postgres_baseline_store::PostgresBaselineStore>,
    > = match cfg.database_url.as_deref().filter(|s| !s.is_empty()) {
        Some(dsn) => match sqlx::postgres::PgPool::connect(dsn).await {
            Ok(pool) => match quarry_runtime::migrations::run_migrations(&pool).await {
                Ok(()) => {
                    tracing::info!(
                        "change tracking: migrations applied; PostgresBaselineStore wired"
                    );
                    Some(Arc::new(
                        quarry_runtime::postgres_baseline_store::PostgresBaselineStore::new(pool),
                    ))
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "baseline store: migrations failed; change tracking disabled (route will 501)"
                    );
                    None
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "baseline store: PgPool connect failed");
                None
            }
        },
        None => None,
    };

    // Cycle 19 / cluster #16: build the local Tantivy index that
    // accumulates every successful scrape. Persistent on-disk when
    // `local_index_dir` is set; otherwise in-memory (dev only).
    let local_index: Option<quarry_runtime::local_index::TantivyLocalIndex> = match cfg
        .local_index_dir
        .as_deref()
    {
        Some(path) if !path.is_empty() => {
            match quarry_runtime::local_index::TantivyLocalIndex::open(path) {
                Ok(idx) => {
                    tracing::info!(%path, "local index: on-disk Tantivy");
                    Some(idx)
                }
                Err(e) => {
                    tracing::warn!(error = %e, "local index init failed; running without own-corpus search");
                    None
                }
            }
        }
        _ => match quarry_runtime::local_index::TantivyLocalIndex::in_memory() {
            Ok(idx) => {
                tracing::info!("local index: in-memory Tantivy (set QUARRY_EDGE__LOCAL_INDEX_DIR for persistence)");
                Some(idx)
            }
            Err(e) => {
                tracing::warn!(error = %e, "in-memory local index init failed");
                None
            }
        },
    };

    // Durability for the local corpus. Tantivy's autocommit is driven by
    // writer-heap pressure alone, so a low-traffic edge can hold every write
    // since boot in memory and lose all of them when the container is
    // recycled — which is what made the local tier look permanently empty in
    // production while `add_document` was plainly succeeding.
    //
    // The ticker commits on whichever comes first: elapsed time or accumulated
    // documents. The count trigger exists for the crawl case, where a single
    // burst can add thousands of documents inside one interval.
    let (index_shutdown_tx, index_shutdown_rx) = tokio::sync::watch::channel(false);
    let index_commit_task = local_index.as_ref().map(|idx| {
        let idx = idx.clone();
        let mut shutdown = index_shutdown_rx.clone();
        tokio::spawn(async move {
            let mut poll = tokio::time::interval(LOCAL_INDEX_COMMIT_POLL);
            poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut since_commit = Duration::ZERO;
            let mut since_retention = Duration::ZERO;
            loop {
                tokio::select! {
                    _ = poll.tick() => {}
                    _ = shutdown.changed() => {
                        // Final flush: everything added since the last tick
                        // would otherwise be lost, which is precisely the
                        // window an operator-initiated restart lands in.
                        if let Err(e) = idx.flush().await {
                            tracing::warn!(error = %e, "local index: shutdown flush failed");
                        } else {
                            tracing::info!(docs = idx.doc_count(), "local index: flushed on shutdown");
                        }
                        return;
                    }
                }
                since_commit += LOCAL_INDEX_COMMIT_POLL;
                since_retention += LOCAL_INDEX_COMMIT_POLL;

                let pending = idx.pending_writes();
                if pending > 0
                    && (since_commit >= LOCAL_INDEX_COMMIT_INTERVAL
                        || pending >= LOCAL_INDEX_COMMIT_DOCS)
                {
                    match idx.flush().await {
                        Ok(()) => tracing::debug!(pending, "local index: committed"),
                        Err(e) => tracing::warn!(error = %e, "local index: commit failed"),
                    }
                    since_commit = Duration::ZERO;
                }

                if since_retention >= LOCAL_INDEX_RETENTION_INTERVAL {
                    since_retention = Duration::ZERO;
                    match idx.enforce_retention().await {
                        Ok(outcome) => tracing::info!(
                            evicted = outcome.evicted_by_ceiling,
                            docs = outcome.docs_after,
                            "local index: retention pass"
                        ),
                        Err(e) => tracing::warn!(error = %e, "local index: retention pass failed"),
                    }
                }
            }
        })
    });

    // Cycle 19 / cluster #17: assemble the SearchProvider via
    // SmartSearchRouter — intent-aware routing with parallel widening,
    // per-provider circuit breakers, and a query-result TTL cache.
    //
    // Free tier order: Tantivy (own corpus) → Stract (independent SERP)
    // → SearXNG (aggregator). Paid backup tier (Brave / Serper) is
    // invoked only when the free chain returns < min_total_results.
    //
    // Each layer is optional; the router is built from whatever is
    // configured. If nothing is configured, /v1/search and /v1/answer
    // return 501.
    let mut builder = quarry_runtime::SmartSearchRouter::builder();
    let mut configured_any = false;

    if let Some(idx) = &local_index {
        builder = builder.with_tantivy(Arc::new(idx.clone()));
        tracing::info!("smart_router[+]: tantivy_local");
        configured_any = true;
    }
    if let Some(url) = cfg.stract_url.as_deref().filter(|s| !s.is_empty()) {
        match quarry_runtime::serp::StractSearch::new(url) {
            Ok(p) => {
                tracing::info!(%url, "smart_router[+]: stract");
                builder = builder.with_stract(Arc::new(p));
                configured_any = true;
            }
            Err(e) => tracing::warn!(error = %e, "stract init failed"),
        }
    }
    if let Some(url) = cfg.searxng_url.as_deref().filter(|s| !s.is_empty()) {
        match quarry_runtime::serp::SearXNGSearch::new(url) {
            Ok(p) => {
                tracing::info!(%url, "smart_router[+]: searxng");
                builder = builder.with_searxng(Arc::new(p));
                configured_any = true;
            }
            Err(e) => tracing::warn!(error = %e, "searxng init failed"),
        }
    }
    // Data residency: Brave & Serper are external SaaS — they receive the raw
    // query. In zero-SaaS mode we refuse to register them so web search stays
    // entirely in-infra (Tantivy/Stract/SearXNG + Data Plane). Otherwise they
    // register as paid backups, but we log loudly that queries may egress.
    let brave_keyed = cfg.brave_search_key.as_deref().filter(|s| !s.is_empty());
    let serper_keyed = cfg.serper_key.as_deref().filter(|s| !s.is_empty());
    if cfg.zero_saas_search {
        if brave_keyed.is_some() || serper_keyed.is_some() {
            tracing::warn!(
                "zero_saas_search=on: NOT registering Brave/Serper — web search stays \
                 in-infra (Tantivy/Stract/SearXNG + Data Plane); queries never egress"
            );
        }
    } else {
        if let Some(key) = brave_keyed {
            match quarry_runtime::serp::BraveSearch::new(key) {
                Ok(p) => {
                    tracing::warn!(
                        "smart_router[+]: brave (paid backup) — EXTERNAL: queries egress to \
                         api.search.brave.com; set QUARRY_EDGE__ZERO_SAAS_SEARCH=1 to disable"
                    );
                    builder = builder.with_brave(Arc::new(p));
                    configured_any = true;
                }
                Err(e) => tracing::warn!(error = %e, "brave init failed"),
            }
        }
        if let Some(key) = serper_keyed {
            match quarry_runtime::serp::SerperSearch::new(key) {
                Ok(p) => {
                    tracing::warn!(
                        "smart_router[+]: serper (paid backup) — EXTERNAL: queries egress to \
                         google.serper.dev; set QUARRY_EDGE__ZERO_SAAS_SEARCH=1 to disable"
                    );
                    builder = builder.with_serper(Arc::new(p));
                    configured_any = true;
                }
                Err(e) => tracing::warn!(error = %e, "serper init failed"),
            }
        }
    }

    let build_model_client = |url: &str| -> quarry_core::error::QuarryResult<
        quarry_runtime::mp_client::ModelPlaneClient,
    > {
        let mut client = quarry_runtime::mp_client::ModelPlaneClient::new(url)?;
        if let Some(provider) = cross_plane_tokens.clone() {
            client = client.with_token_provider(provider);
        } else if cfg.cross_plane_auth_dev_bypass {
            if let Some(token) = cfg
                .model_plane_token
                .as_deref()
                .filter(|token| !token.is_empty())
            {
                client = client.with_bearer_token(token);
            }
        }
        Ok(client)
    };

    // Cycle 19 / cluster #20: optional LLM-backed intent classifier.
    // When `llm_classify_intent` is on AND a Model Plane URL is set, we
    // build a `CachedClassifier(HybridClassifier(MpIntentClassifier))`
    // stack and plug it into the router. The Hybrid layer keeps the rule
    // fast-path for obvious queries (URL, quoted phrase, fresh keywords);
    // the LLM is only consulted on rule-default queries. The Cache layer
    // dedupes hot keys for 60 minutes. Any failure or timeout in the LLM
    // call silently falls back to `QueryIntent::Default` — the search
    // path is never blocked by the Model Plane.
    if cfg.llm_classify_intent {
        if let Some(mp_url) = cfg.model_plane_url.as_deref().filter(|s| !s.is_empty()) {
            match build_model_client(mp_url) {
                Ok(mp) => {
                    let mp_arc = Arc::new(mp);
                    let mut llm = quarry_runtime::MpIntentClassifier::new(mp_arc);
                    if let Some(m) = cfg.llm_classify_model.as_deref().filter(|s| !s.is_empty()) {
                        llm = llm.with_model(m);
                    }
                    let hybrid: Arc<dyn quarry_runtime::IntentClassifier> =
                        Arc::new(quarry_runtime::HybridClassifier::new(Arc::new(llm)));
                    let cached = quarry_runtime::CachedClassifier::new(
                        hybrid,
                        std::time::Duration::from_secs(3600),
                    );
                    tracing::info!(
                        model = cfg.llm_classify_model.as_deref().unwrap_or("<mp-default>"),
                        "smart_router intent classifier: LLM (cached, hybrid)"
                    );
                    builder = builder.with_intent_classifier(Arc::new(cached));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "MP client init failed; falling back to rule-only intent classifier");
                }
            }
        } else {
            tracing::warn!(
                "llm_classify_intent=true but MODEL_PLANE_URL is unset; using rule-only classifier"
            );
        }
    } else {
        tracing::info!("smart_router intent classifier: rule-only (LLM disabled)");
    }

    // Autoprompt — optional LLM query rewriter for Research/Comparative
    // queries. Same gating as the intent classifier: requires the flag AND a
    // Model Plane URL. The rewriter is degrade-safe (original query on any
    // failure), so this never blocks search.
    if cfg.autoprompt {
        if let Some(mp_url) = cfg.model_plane_url.as_deref().filter(|s| !s.is_empty()) {
            match build_model_client(mp_url) {
                Ok(mp) => {
                    let mut rewriter = quarry_runtime::ModelPlaneQueryRewriter::new(Arc::new(mp));
                    if let Some(m) = cfg.autoprompt_model.as_deref().filter(|s| !s.is_empty()) {
                        rewriter = rewriter.with_model(m);
                    }
                    tracing::info!(
                        model = cfg.autoprompt_model.as_deref().unwrap_or("<mp-default>"),
                        "smart_router autoprompt: enabled"
                    );
                    builder = builder.with_query_rewriter(Arc::new(rewriter));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "autoprompt: MP client init failed; rewriting disabled")
                }
            }
        } else {
            tracing::warn!("autoprompt=true but MODEL_PLANE_URL is unset; rewriting disabled");
        }
    }

    let search: Option<Arc<dyn quarry_runtime::serp::SearchProvider>> = if configured_any {
        match builder.build() {
            Ok(router) => {
                tracing::info!("SmartSearchRouter assembled");
                Some(Arc::new(router))
            }
            Err(e) => {
                tracing::warn!(error = %e, "SmartSearchRouter build failed");
                None
            }
        }
    } else {
        tracing::warn!("no SearchProvider configured; /v1/search and /v1/answer will 501");
        None
    };

    // Build the Data Plane vector index once (when DATA_PLANE_URL is set). It is
    // shared by the hybrid search provider below AND retained in AppState for
    // the find-similar route (`POST /v1/search/similar`), which queries it
    // directly rather than going through the lexical/SERP chain.
    let vector_index: Option<Arc<dyn quarry_runtime::vector_index::VectorIndex>> = cfg
        .data_plane_url
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|dp_url| {
            let mut vidx = quarry_runtime::DataPlaneVectorIndex::new(dp_url);
            if let Some(provider) = cross_plane_tokens.clone() {
                vidx = vidx.with_token_provider(provider);
            } else if cfg.cross_plane_auth_dev_bypass {
                if let Some(key) = cfg
                    .data_plane_service_token
                    .as_deref()
                    .filter(|s| !s.is_empty())
                {
                    vidx = vidx.with_api_key(key);
                }
            }
            Arc::new(vidx) as Arc<dyn quarry_runtime::vector_index::VectorIndex>
        });

    // OSS-parity P0 1C: hybrid search (Path A). When the vector index is
    // configured, wrap the lexical/SERP router in a HybridSearchProvider that
    // RRF-fuses Quarry's own-corpus lexical recall with the Data Plane's
    // semantic (Qdrant) retrieval. Best-effort: vector failures degrade to
    // lexical-only. Transparent to /v1/search + AnswerPipeline (both read
    // `search`). No new infra — delegates to the Data Plane's retrieval_v2.
    let search: Option<Arc<dyn quarry_runtime::serp::SearchProvider>> =
        match (search, vector_index.clone()) {
            (Some(lexical), Some(vidx)) => {
                tracing::info!("hybrid search: lexical ⊕ Data Plane vector (RRF) wired");
                Some(Arc::new(quarry_runtime::HybridSearchProvider::new(
                    lexical, vidx,
                )))
            }
            (other, _) => other,
        };

    // Exa-style semantic rerank — when enabled AND a Model Plane URL is set,
    // wrap the search provider so the top-N merged results are reordered by
    // query relevance with a short highlight attached per result. Degrade-safe
    // inside the reranker (original order on any failure). Wrapping here means
    // both /v1/search and the AnswerPipeline read reranked results, and the edge
    // cache stores the reranked output (no repeat LLM cost on cache hits).
    let search: Option<Arc<dyn quarry_runtime::serp::SearchProvider>> = match (
        search,
        cfg.semantic_rerank,
        cfg.model_plane_url.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(inner), true, Some(mp_url)) => match build_model_client(mp_url) {
            Ok(mp) => {
                let mut reranker = quarry_runtime::ModelPlaneSearchReranker::new(Arc::new(mp));
                if let Some(m) = cfg
                    .semantic_rerank_model
                    .as_deref()
                    .filter(|s| !s.is_empty())
                {
                    reranker = reranker.with_model(m);
                }
                let top_n = cfg.semantic_rerank_top_n.unwrap_or(10);
                tracing::info!(top_n, "semantic rerank: enabled");
                Some(Arc::new(quarry_runtime::RerankingSearchProvider::new(
                    inner,
                    Arc::new(reranker),
                    top_n,
                ))
                    as Arc<dyn quarry_runtime::serp::SearchProvider>)
            }
            Err(e) => {
                tracing::warn!(error = %e, "semantic rerank: MP client init failed; disabled");
                Some(inner)
            }
        },
        (other, _, _) => other,
    };

    // Cycle 19 / cluster #18: AnswerPipeline (Tavily replacement).
    // Requires BOTH a SearchProvider AND a Model Plane URL. When either
    // is missing, /v1/answer returns 501 Unsupported with a hint.
    let answer_pipeline: Option<Arc<quarry_runtime::answer::AnswerPipeline>> = match (
        &search,
        cfg.model_plane_url.as_deref().filter(|s| !s.is_empty()),
    ) {
        (Some(search_arc), Some(mp_url)) => {
            match build_model_client(mp_url) {
                Ok(mp_client) => {
                    let mp_arc = Arc::new(mp_client);
                    let formats = quarry_runtime::ai_formats::AiFormatRunner::new(mp_arc);
                    // Real HTTP markdown fetcher: GET → readability → markdown.
                    // Pages that block / 4xx-5xx / time out return None, and the
                    // AnswerPipeline falls back to the search-result snippet (which
                    // also carries Data Plane chunk text), so the synthesized answer
                    // is populated whenever a provider returned any text. A
                    // PageRunner-backed fetcher (TLS impersonation / JS render) is the
                    // natural follow-up — same trait, swap the impl.
                    let fetcher = Arc::new(
                        quarry_runtime::answer::SimpleHttpMarkdownFetcher::new()
                            .with_user_agent(cfg.user_agent.clone()),
                    );
                    let pipeline = quarry_runtime::answer::AnswerPipeline::new(
                        search_arc.clone(),
                        fetcher,
                        formats,
                    );
                    tracing::info!("answer pipeline: wired (search + model plane)");
                    Some(Arc::new(pipeline))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "answer pipeline disabled: model plane client init failed");
                    None
                }
            }
        }
        _ => {
            tracing::info!("answer pipeline disabled (search or model_plane_url unset)");
            None
        }
    };

    // Note: the legacy single-provider env switch (`QUARRY_EDGE__SEARCH_PROVIDER`)
    // is superseded by SmartSearchRouter above, which always wires every
    // configured provider into the priority-aware router.

    // Wave 7 — build the optional HTTP/3 driver. We only construct it
    // when the `http3` feature is compiled in AND
    // `QUARRY_EDGE__HTTP3_ENABLED` is truthy at startup. Build failure
    // is non-fatal: the edge keeps serving HTTP/1.1+2 and per-request
    // `prefer_http3` falls back to the default driver with a tracing
    // warning.
    #[allow(unused_variables)]
    let http3_enabled = std::env::var("QUARRY_EDGE__HTTP3_ENABLED")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes"))
        .unwrap_or(false);
    #[cfg(feature = "http3")]
    let http3_driver: Option<std::sync::Arc<dyn quarry_runtime::Driver>> = if http3_enabled {
        match quarry_runtime::http3::Http3Driver::new(
            std::time::Duration::from_secs(30),
            "Mozilla/5.0 (compatible; QuarryBot/1.0; +https://verevon.io/bot)",
        ) {
            Ok(d) => {
                tracing::info!("HTTP/3 driver enabled");
                Some(std::sync::Arc::new(d))
            }
            Err(e) => {
                tracing::warn!(error = %e, "HTTP/3 driver build failed; falling back");
                None
            }
        }
    } else {
        None
    };
    #[cfg(not(feature = "http3"))]
    let http3_driver: Option<std::sync::Arc<dyn quarry_runtime::Driver>> = None;

    // P7 — real chromiumoxide CDP driver for the agentic browser loop, plus
    // the live run→session map. Acquire fails at runtime if no Chrome is
    // reachable; that is a real (not mocked) driver, simply unconfigured.
    #[cfg(feature = "browser-agent")]
    let agent_driver: Arc<dyn quarry_browser::BrowserDriver> = Arc::new(
        quarry_browser::chromiumoxide::ChromiumoxideDriver::new()
            .with_pinned_egress_proxy(browser_egress_proxy.clone())
            .with_profile_store(profiles.clone()),
    );
    #[cfg(feature = "browser-agent")]
    let agent_runs = agent_routes::new_runs();

    let visual_processor: Option<Arc<dyn quarry_runtime::vision::VisualObservationProcessor>> = {
        let visual_feature_requested = cfg.visual_diff_enabled
            || cfg.screenshot_preprocessing_enabled
            || cfg.page_image_cleanup_enabled
            || cfg.visual_tiles_enabled
            || cfg.ocr_preconditioning_enabled
            || cfg.rendered_branding_visual_enabled;
        if cfg.vision_enabled && visual_feature_requested {
            let backend = cfg.vision_backend.as_deref().unwrap_or("sidecar");
            let options = quarry_runtime::vision::VisualProcessorOptions {
                max_regions: cfg.visual_max_regions.unwrap_or(32),
                diff: cfg.visual_diff_enabled,
                screenshot_preprocessing: cfg.screenshot_preprocessing_enabled
                    || cfg.page_image_cleanup_enabled,
                thumbnail: cfg.visual_tiles_enabled || cfg.page_image_cleanup_enabled,
                tiles: cfg.visual_tiles_enabled || cfg.page_image_cleanup_enabled,
                ocr_preconditioning: cfg.ocr_preconditioning_enabled,
                rendered_branding: cfg.rendered_branding_visual_enabled,
            };
            match backend {
                "sidecar" => match cfg.vision_sidecar_url.as_deref().filter(|s| !s.is_empty()) {
                    Some(url) => match quarry_runtime::vision::SidecarVisualProcessor::new(url) {
                        Ok(processor) => {
                            tracing::info!(
                                url,
                                diff = options.diff,
                                page_image_cleanup = cfg.page_image_cleanup_enabled,
                                "visual sidecar enabled"
                            );
                            Some(Arc::new(processor.with_options(options))
                                as Arc<
                                    dyn quarry_runtime::vision::VisualObservationProcessor,
                                >)
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "visual sidecar config invalid; disabled");
                            None
                        }
                    },
                    None => {
                        tracing::warn!(
                            "vision enabled but QUARRY_EDGE__VISION_SIDECAR_URL is unset; disabled"
                        );
                        None
                    }
                },
                other => {
                    tracing::warn!(backend = other, "unsupported vision backend; disabled");
                    None
                }
            }
        } else {
            None
        }
    };

    // Phase-2 visual RAG — page-image producer. Online only when CAS bucket,
    // Data Plane NATS, and edge base URL are all configured (browser-agent
    // feature provides the real BrowserDriver). Reuses agent_driver + AWS_* env.
    #[cfg(feature = "browser-agent")]
    let page_renderer: Option<std::sync::Arc<quarry_runtime::page_renderer::PageRenderer>> = match (
        cfg.cas_bucket.as_deref().filter(|s| !s.is_empty()),
        cfg.dataplane_nats_url.as_deref().filter(|s| !s.is_empty()),
        cfg.edge_internal_base_url
            .as_deref()
            .filter(|s| !s.is_empty()),
    ) {
        (Some(bucket), Some(nats_url), Some(base)) => {
            match quarry_runtime::page_renderer::PageRenderer::connect(
                agent_driver.clone(),
                nats_url,
                bucket.to_string(),
                std::env::var("AWS_ENDPOINT_URL").ok(),
                base.to_string(),
                if cfg.page_image_cleanup_enabled {
                    visual_processor.clone()
                } else {
                    None
                },
            )
            .await
            {
                Ok(r) => {
                    tracing::info!(bucket, nats_url, "visual RAG page-image producer enabled");
                    Some(std::sync::Arc::new(r))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "page-image producer connect failed; disabled");
                    None
                }
            }
        }
        _ => None,
    };
    #[cfg(not(feature = "browser-agent"))]
    let page_renderer: Option<std::sync::Arc<quarry_runtime::page_renderer::PageRenderer>> = None;

    // Page-title enrichment for `page_extracted`: the cheapest routed Model
    // Plane alias names pages whose `<title>` is missing/generic. Same client
    // builder (and therefore the same Auth Core token minting / dev bypass)
    // as the other Model Plane consumers above. Absence of a Model Plane URL
    // simply leaves `title_source` at `html`/`host`.
    let page_title_enricher: Option<Arc<quarry_runtime::page_extract::PageTitleEnricher>> =
        if !cfg.page_title_enrich.unwrap_or(true) {
            tracing::info!("page-title enrichment disabled by QUARRY_EDGE__PAGE_TITLE_ENRICH=false");
            None
        } else {
            match cfg.model_plane_url.as_deref().filter(|s| !s.is_empty()) {
                Some(mp_url) => match build_model_client(mp_url) {
                    Ok(mp) => {
                        let enricher = quarry_runtime::page_extract::PageTitleEnricher::new(
                            Arc::new(mp),
                        )
                        .with_model(cfg.page_title_model.clone().unwrap_or_default());
                        tracing::info!(
                            model = enricher.model(),
                            "page-title enrichment enabled (Model Plane /v1/invoke, routed alias)"
                        );
                        Some(Arc::new(enricher))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "page-title enrichment disabled: model plane client init failed");
                        None
                    }
                },
                None => {
                    tracing::info!("page-title enrichment disabled (MODEL_PLANE_URL unset)");
                    None
                }
            }
        };

    let app_state = state::AppState {
        readiness: state::ReadinessState {
            durable: durable_ready,
            reason: (!durable_ready)
                .then(|| "ephemeral backend configured; set durable runtime backends".to_string()),
        },
        receipts,
        grant_validator,
        require_browser_grants,
        driver: default_driver,
        drivers,
        http3: http3_driver,
        security: Arc::new(security),
        artifacts,
        // W1 — surface the operator-configured proxy pool name to
        // the per-run affinity resolver. Empty means "no operator
        // pool"; the resolver forces first-party egress in that
        // case so an unconfigured deployment still gets the safe
        // default.
        proxy_pool_name: std::env::var("QUARRY_PROXY_POOL").unwrap_or_default(),
        control_base_url: cfg.control_base_url.clone(),
        redis,
        cache,
        event_sink,
        ingest,
        profiles,
        search,
        vector_index,
        searxng_url: cfg.searxng_url.clone().filter(|s| !s.is_empty()),
        model_plane_url: cfg.model_plane_url.clone(),
        model_plane_token: cfg
            .cross_plane_auth_dev_bypass
            .then(|| cfg.model_plane_token.clone())
            .flatten(),
        page_title_enricher,
        service_token_provider: cross_plane_tokens.clone(),
        answer_pipeline,
        local_index,
        usage,
        #[cfg(feature = "postgres-queue")]
        event_history,
        #[cfg(feature = "postgres-queue")]
        queue_pool,
        #[cfg(feature = "postgres-queue")]
        baseline_store,
        // D2 / cluster #14 — HMAC signer for cross-plane forwards.
        // None when the secret env is unset → unsigned requests (dev
        // posture only; control plane MUST require signatures in prod).
        internal_signer: cfg
            .internal_secret
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| match internal_auth::InternalSigner::new(s) {
                Ok(signer) => {
                    tracing::info!("HMAC signer wired for control-plane forwards");
                    Some(std::sync::Arc::new(signer))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "internal_secret too short; HMAC signing disabled");
                    None
                }
            }),
        // Cycle 21 / cluster #2 — best-effort policy by default.
        // Operators wanting `strict` reproducibility set
        // `QUARRY_EDGE__RUN_POLICY=strict` (future config wiring; for
        // now the runtime knob lives at code level).
        policy: quarry_runtime::RunPolicy::default(),
        // Cycle 21 / cluster #3 — single shared HostScheduler with
        // default AIMD config. Cheap to share via Arc; per-host state
        // lives behind a tokio::Mutex<HashMap<host, slot>>.
        scheduler: Some(std::sync::Arc::new(
            quarry_runtime::HostScheduler::with_defaults(),
        )),
        page_renderer,
        visual_processor,
        #[cfg(feature = "browser-agent")]
        agent_driver,
        #[cfg(feature = "browser-agent")]
        browser_egress_proxy: Some(browser_egress_proxy),
        #[cfg(feature = "browser-agent")]
        agent_runs,
        fleets: quarry_edge::fleet_routes::new_fleet_state(),
    };

    let app = routes::router(app_state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!(%addr, "quarry-edge-rs listening");
    let listener = TcpListener::bind(&addr).await?;
    axum::serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Serve returned, so no new requests can add to the corpus. Signal the
    // maintenance task and wait for its final commit — dropping straight out
    // of `main` here would abort the task mid-flush and lose exactly the
    // writes the ticker exists to protect.
    let _ = index_shutdown_tx.send(true);
    if let Some(task) = index_commit_task {
        match tokio::time::timeout(LOCAL_INDEX_SHUTDOWN_FLUSH_TIMEOUT, task).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "local index: commit task panicked"),
            // A commit on a large index can outlive a tight orchestrator kill
            // window; losing it is better than never exiting and being killed
            // with -9 mid-write.
            Err(_) => tracing::warn!("local index: shutdown flush timed out"),
        }
    }
    Ok(())
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

/// Build the compatibility HTTP BrowserBroker validator. The gRPC path is
/// selected earlier when configured; keeping this construction in one helper
/// prevents the two transports from drifting in their auth treatment.
fn browser_grant_http_validator(
    cfg: &config::EdgeConfig,
) -> anyhow::Result<Arc<dyn quarry_runtime::GrantValidator>> {
    let Some(url) = cfg
        .browser_grant_validator_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    else {
        return Ok(Arc::new(quarry_runtime::NoopGrantValidator));
    };
    let validator = quarry_runtime::HttpGrantValidator::new(url)?;
    let validator = match cfg.model_plane_token.as_deref() {
        Some(token) if cfg.cross_plane_auth_dev_bypass => {
            validator.with_bearer_token(token.to_owned())
        }
        _ => validator,
    };
    Ok(Arc::new(validator))
}

pub fn routes_for_test(state: state::AppState) -> Router {
    routes::router(state)
}
