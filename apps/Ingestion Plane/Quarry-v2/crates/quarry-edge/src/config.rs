use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct EdgeConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_ua")]
    pub user_agent: String,
    #[serde(default = "default_timeout")]
    pub fetch_timeout_s: u64,
    #[serde(default = "default_control_url")]
    pub control_base_url: String,
    #[serde(default)]
    pub redis_url: Option<String>,
    #[serde(default)]
    pub dragonfly_url: Option<String>,
    #[serde(default = "default_artifact_backend")]
    pub artifact_backend: String,
    #[serde(default = "default_artifact_root")]
    pub artifact_root: String,
    #[serde(default)]
    pub s3_bucket: Option<String>,
    #[serde(default)]
    pub control_api_key: String,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_secs: u64,
    /// Optional file-backed URL security snapshot. The feed sync script writes
    /// URLhaus/PhishTank host blocklists in quarry-security's native JSON shape.
    #[serde(default)]
    pub security_snapshot_path: Option<String>,
    #[serde(default)]
    pub data_plane_url: Option<String>,
    /// Dedicated Data Plane *ingest* (durable write) target. The retrieval
    /// engine (`data_plane_url`) serves reads/vector-similarity but has no
    /// `POST /v1/documents` write route — that route lives on the Go
    /// documents-api. When set, the ingest client (HTTP or gRPC) posts here;
    /// otherwise it falls back to `data_plane_url` for backward compatibility.
    #[serde(default)]
    pub data_plane_ingest_url: Option<String>,
    #[serde(default)]
    /// Development-only static Data Plane bearer. Production mints per-org
    /// tokens from Auth Core using `quarry_service_api_key`.
    pub data_plane_service_token: Option<String>,
    /// Auth Core base URL used for service-principal token minting.
    #[serde(default = "default_auth_core_url")]
    pub auth_core_url: String,
    /// Durable service-principal credential registered under the fixed
    /// `quarry-edge` identity in Auth Core.
    #[serde(default)]
    pub quarry_service_api_key: Option<String>,
    /// Explicit local-development escape hatch for legacy static plane tokens.
    /// Refused when ENVIRONMENT is prod/production.
    #[serde(default)]
    pub cross_plane_auth_dev_bypass: bool,
    #[serde(default)]
    pub browserbase_api_key: Option<String>,
    #[serde(default)]
    pub browserbase_project_id: Option<String>,
    #[serde(default)]
    pub browserbase_url: Option<String>,
    /// SERP provider — `brave`, `serper`, `searxng`, or empty/None to disable.
    #[serde(default)]
    #[allow(dead_code)] // scaffolding: wired in follow-up
    pub search_provider: Option<String>,
    #[serde(default)]
    pub brave_search_key: Option<String>,
    #[serde(default)]
    pub serper_key: Option<String>,
    #[serde(default)]
    pub searxng_url: Option<String>,
    /// Stract self-hosted independent SERP base URL. Cycle 19 / cluster #17.
    /// When set, slots into the SearchProvider chain between Tantivy and
    /// SearXNG: `Tantivy → Stract → SearXNG → Brave`.
    #[serde(default)]
    pub stract_url: Option<String>,
    /// On-disk directory for the Tantivy local index. Cycle 19 / cluster #16.
    /// When unset, the index is in-memory (lost on restart). Production
    /// should point this at a persistent volume.
    #[serde(default)]
    pub local_index_dir: Option<String>,
    /// Model Plane gateway URL — when set, /v1/audio routes to
    /// /v1/ai/speech + /v1/ai/transcribe via this URL.
    #[serde(default)]
    pub model_plane_url: Option<String>,
    /// Development-only static Model Plane bearer. Production uses Auth Core.
    #[serde(default)]
    pub model_plane_token: Option<String>,
    /// BrowserBroker grant validation endpoint. When set, every agent run
    /// must present a grant_id and every action revalidates that grant.
    #[serde(default)]
    pub browser_grant_validator_url: Option<String>,
    /// BrowserBroker gRPC endpoint. This is the preferred production path
    /// when the `grpc` feature is enabled; the HTTP validator URL remains a
    /// compatibility shim for deployments that have not exposed gRPC yet.
    #[serde(default)]
    pub browser_grant_validator_grpc_url: Option<String>,
    /// Fail closed when a caller omits BrowserBroker's per-run grant.
    #[serde(default)]
    pub require_browser_grants: bool,
    /// Enable LLM-backed intent classification on the search router.
    /// When `true` AND `model_plane_url` is set, the router consults the
    /// Model Plane for query intent (Research / Comparative / Local /
    /// Code) on queries that don't match a rule fast-path. Default off
    /// to preserve deterministic latency until validated in production.
    #[serde(default)]
    pub llm_classify_intent: bool,
    /// Optional Model Plane model override for intent classification.
    /// Prefer a small fast model (e.g. Haiku). Empty/None → MP picks.
    #[serde(default)]
    pub llm_classify_model: Option<String>,
    /// Autoprompt — when `true` AND `model_plane_url` is set, the router
    /// rewrites verbose `Research`/`Comparative` queries into a tighter
    /// web-search query before fan-out. Degrade-safe (falls back to the
    /// original query). Default off to preserve deterministic latency.
    #[serde(default)]
    pub autoprompt: bool,
    /// Optional Model Plane model override for autoprompt rewriting.
    #[serde(default)]
    pub autoprompt_model: Option<String>,
    /// Semantic rerank — when `true` AND `model_plane_url` is set, the merged
    /// result set is reordered by query-relevance via the Model Plane, with a
    /// short relevance highlight attached per result (Exa-style). Degrade-safe
    /// (falls back to the original order). Default off.
    #[serde(default)]
    pub semantic_rerank: bool,
    /// Optional Model Plane model override for semantic reranking.
    #[serde(default)]
    pub semantic_rerank_model: Option<String>,
    /// How many top results to rerank (the long tail is left in place). Default
    /// 10 when unset.
    #[serde(default)]
    pub semantic_rerank_top_n: Option<usize>,
    /// Zero-SaaS search posture (data residency): when `true`, the SmartRouter
    /// refuses to register external paid SERP providers (Brave/Serper) that send
    /// the query to a third party. Only in-infra providers (Tantivy / Stract /
    /// SearXNG + Data Plane) are used, so search queries never leave the cluster.
    /// Default `false` (Brave/Serper are registered as paid backups when keyed).
    #[serde(default)]
    pub zero_saas_search: bool,
    /// P1 / cluster #nats — NATS JetStream URL for cross-plane event
    /// fan-out (e.g. `nats://nats:4222`). When set, every `EventSink::emit`
    /// also publishes to JetStream so autocomplete-core, model-plane,
    /// and org-core can consume Quarry events directly. Empty/None
    /// leaves the sink mpsc-only (control-plane HTTP path unchanged).
    #[serde(default)]
    pub nats_url: Option<String>,
    /// Optional NATS auth token (NKEY/JWT). Mutually exclusive with
    /// `nats_creds_file`; if both are set, the credentials file wins.
    #[serde(default)]
    pub nats_token: Option<String>,
    /// Path to a `.creds` file (NATS user JWT + nkey seed). Production
    /// rollouts mount this from a secrets manager.
    #[serde(default)]
    pub nats_creds_file: Option<String>,
    /// Subject prefix on the JetStream stream. Default `quarry` matches
    /// the prefix that auto-create the `QUARRY_EVENTS` stream uses.
    #[serde(default)]
    pub nats_subject_prefix: Option<String>,
    /// P2 / cluster #grpc — Data Plane ingest transport. `"http"`
    /// (default) uses `IngestClient` (HTTP/JSON). `"grpc"` switches to
    /// `GrpcIngestAdapter` (HTTP/2 + protobuf). The `grpc` value is
    /// only meaningful when the binary was compiled with
    /// `--features grpc`; otherwise it falls back to HTTP with a
    /// warning. Wire empty/None to keep HTTP.
    #[serde(default)]
    pub data_plane_transport: Option<String>,
    /// D2 / cluster #14 — shared HMAC secret used to sign edge →
    /// control-plane requests. Hex-encoded; ≥ 32 chars (256 bits).
    /// When empty, requests go out unsigned (private-network dev
    /// posture only). Production MUST set this to the same value as
    /// `QUARRY_INTERNAL_SECRET` on the control plane.
    #[serde(default)]
    pub internal_secret: Option<String>,
    /// C30.1 / cluster #6 — Profile store backend.
    /// `"memory"` (default) — `InMemoryProfileStore` (dev / tests).
    /// `"postgres"` — `PostgresProfileStore` (requires `database_url`
    /// AND binary built with `--features postgres-queue`). When the
    /// feature isn't compiled in, this falls back to memory with a
    /// warning.
    #[serde(default)]
    pub profile_store_kind: Option<String>,
    /// C30.1 / cluster #1 + #6 — Postgres DSN used by the runtime
    /// for the durable request queue, profile store, and event
    /// history. Shared across all three because they live in the
    /// same database in production.
    #[serde(default)]
    #[allow(dead_code)] // read only under `postgres-queue`; wired in follow-up
    pub database_url: Option<String>,
    /// C30.1 / cluster #7 — Enable durable job-history emit + the
    /// `GET /v1/runs/:id/events` read endpoint. Requires
    /// `database_url` AND `--features postgres-queue`.
    #[serde(default)]
    #[allow(dead_code)] // read only under `postgres-queue`; wired in follow-up
    pub durable_event_history: bool,
    /// Phase-2 visual RAG producer — when all three are set (and `browser-agent`
    /// is on), ingested web pages are rendered to PNGs, written to the `cas_bucket`
    /// MinIO bucket, and `page_images.created` is published to `dataplane_nats_url`.
    /// `edge_internal_base_url` is the base of the image-serve endpoint the
    /// embedding-engine GETs. All unset (default) ⇒ producer inert.
    #[serde(default)]
    pub cas_bucket: Option<String>,
    #[serde(default)]
    pub dataplane_nats_url: Option<String>,
    #[serde(default)]
    pub edge_internal_base_url: Option<String>,
    /// Deterministic visual evidence processing. This is intentionally separate
    /// from page-image rendering: page images feed Data Plane embeddings, while
    /// visual observation processing produces debug/change artifacts for browser
    /// actions. Defaults off; the first backend is an isolated OpenCV sidecar.
    #[serde(default)]
    pub vision_enabled: bool,
    #[serde(default)]
    pub vision_backend: Option<String>,
    #[serde(default)]
    pub vision_sidecar_url: Option<String>,
    #[serde(default)]
    pub visual_diff_enabled: bool,
    #[serde(default)]
    pub screenshot_preprocessing_enabled: bool,
    #[serde(default)]
    pub page_image_cleanup_enabled: bool,
    #[serde(default)]
    pub visual_tiles_enabled: bool,
    #[serde(default)]
    pub ocr_preconditioning_enabled: bool,
    #[serde(default)]
    pub rendered_branding_visual_enabled: bool,
    #[serde(default)]
    pub visual_max_regions: Option<usize>,
}

fn default_port() -> u16 {
    8080
}
fn default_ua() -> String {
    "Quarry/2.0 (+https://triodelab.com/quarry)".into()
}
fn default_timeout() -> u64 {
    30
}
fn default_control_url() -> String {
    "http://quarry-control:8081".into()
}
fn default_auth_core_url() -> String {
    "http://auth-core:3011".into()
}
fn default_artifact_backend() -> String {
    "memory".into()
}
fn default_artifact_root() -> String {
    "./data/artifacts".into()
}
fn default_cache_ttl() -> u64 {
    crate::cache::DEFAULT_TTL_SECS
}

impl EdgeConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::Environment::with_prefix("QUARRY_EDGE").separator("__"))
            .build()?;
        let mut edge: Self = cfg.try_deserialize().unwrap_or_else(|err| {
            tracing::error!(error = %err, "EdgeConfig failed to deserialize from environment; falling back to all-defaults (check for a required field with no #[serde(default)])");
            Self::defaults()
        });
        if edge.redis_url.is_none() {
            edge.redis_url = edge.dragonfly_url.clone();
        }
        Ok(edge)
    }

    fn defaults() -> Self {
        Self {
            port: default_port(),
            user_agent: default_ua(),
            fetch_timeout_s: default_timeout(),
            control_base_url: default_control_url(),
            redis_url: None,
            dragonfly_url: None,
            artifact_backend: default_artifact_backend(),
            artifact_root: default_artifact_root(),
            s3_bucket: None,
            control_api_key: String::new(),
            cache_ttl_secs: default_cache_ttl(),
            security_snapshot_path: None,
            data_plane_url: None,
            data_plane_ingest_url: None,
            data_plane_service_token: None,
            auth_core_url: default_auth_core_url(),
            quarry_service_api_key: None,
            cross_plane_auth_dev_bypass: false,
            browserbase_api_key: None,
            browserbase_project_id: None,
            browserbase_url: None,
            search_provider: None,
            brave_search_key: None,
            serper_key: None,
            searxng_url: None,
            stract_url: None,
            local_index_dir: None,
            model_plane_url: None,
            model_plane_token: None,
            browser_grant_validator_url: None,
            browser_grant_validator_grpc_url: None,
            require_browser_grants: false,
            llm_classify_intent: false,
            llm_classify_model: None,
            autoprompt: false,
            autoprompt_model: None,
            semantic_rerank: false,
            semantic_rerank_model: None,
            semantic_rerank_top_n: None,
            zero_saas_search: false,
            nats_url: None,
            nats_token: None,
            nats_creds_file: None,
            nats_subject_prefix: None,
            data_plane_transport: None,
            internal_secret: None,
            profile_store_kind: None,
            database_url: None,
            durable_event_history: false,
            cas_bucket: None,
            dataplane_nats_url: None,
            edge_internal_base_url: None,
            vision_enabled: false,
            vision_backend: None,
            vision_sidecar_url: None,
            visual_diff_enabled: false,
            screenshot_preprocessing_enabled: false,
            page_image_cleanup_enabled: false,
            visual_tiles_enabled: false,
            ocr_preconditioning_enabled: false,
            rendered_branding_visual_enabled: false,
            visual_max_regions: None,
        }
    }
}
