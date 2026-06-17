use std::sync::Arc;

use quarry_browser::session::ProfileStore;
use quarry_runtime::answer::AnswerPipeline;
use quarry_runtime::artifact_store::ArtifactStore;
use quarry_runtime::driver::Driver;
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::host_scheduler::HostScheduler;
use quarry_runtime::ingest_client::DataPlaneIngest;
use quarry_runtime::local_index::TantivyLocalIndex;
use quarry_runtime::serp::SearchProvider;
use quarry_runtime::usage::UsageMeter;
use quarry_runtime::vector_index::VectorIndex;
use quarry_security::SecurityEngine;

#[derive(Clone)]
pub struct AppState {
    pub driver: Arc<dyn Driver>,
    pub drivers: DriverRegistry,
    /// Wave 7 — optional HTTP/3 driver. `Some` when the runtime was
    /// built with `--features http3` and the operator opted in; `None`
    /// otherwise. Selected per-request by `ScrapeRequest.prefer_http3`.
    pub http3: Option<Arc<dyn Driver>>,
    pub security: Arc<dyn SecurityEngine>,
    pub artifacts: Arc<dyn ArtifactStore>,
    pub control_base_url: String,
    pub redis: Option<redis::aio::ConnectionManager>,
    pub cache: Option<crate::cache::PageCache>,
    pub event_sink: quarry_runtime::EventSink,
    pub ingest: Option<Arc<dyn DataPlaneIngest>>,
    pub profiles: Arc<dyn ProfileStore>,
    pub search: Option<Arc<dyn SearchProvider>>,
    /// Semantic retrieval backend (Data Plane v2 `retrieval_v2` → Qdrant).
    /// Powers `POST /v1/search/similar` (find-similar) directly, and is the
    /// same handle fused into the hybrid `search` provider above. `Some` when
    /// `DATA_PLANE_URL` is configured; `None` makes find-similar return 501.
    pub vector_index: Option<Arc<dyn VectorIndex>>,
    /// IMAGES vertical (`POST /v1/search/images`). The web `search` provider
    /// above is the `SmartSearchRouter` and has no image concept, so image
    /// search talks to SearXNG directly. `Some` when `SEARXNG_URL` is
    /// configured; `None` makes the image route return 501 with a hint.
    pub searxng_url: Option<String>,
    /// Model Plane gateway URL (e.g. `http://model-gateway:8080`). When set,
    /// `/v1/audio` proxies to `/v1/ai/speech` + `/v1/ai/transcribe`. When
    /// unset, the route returns 501 Unsupported.
    pub model_plane_url: Option<String>,
    /// Bearer token for the Model Plane gateway.
    pub model_plane_token: Option<String>,
    /// Cycle 19 / cluster #18: Tavily-replacement answer pipeline.
    /// Wired only when both a SearchProvider AND model_plane_url are
    /// configured. `/v1/answer` returns 501 Unsupported otherwise.
    pub answer_pipeline: Option<Arc<AnswerPipeline>>,
    /// Cycle 19 / cluster #16: own-corpus Tantivy index. Cloned cheaply
    /// (interior Arc). PageRunner's success path pushes new docs here.
    pub local_index: Option<TantivyLocalIndex>,
    /// P3 / cluster #billing — per-request usage emitter. Production
    /// wires `NatsUsageMeter` so events land on the `usage.>` subject
    /// that Control Plane's billing-core consumes. Tests / dev wire
    /// `NoopUsageMeter`. Always populated; never `Option`.
    pub usage: Arc<dyn UsageMeter>,
    /// Cycle 21 / cluster #2 — default RunPolicy applied to every
    /// scrape unless the handler overrides it. Defaults to
    /// `best_effort`; operators flip to `strict` via config for
    /// reproducibility-critical workloads.
    pub policy: quarry_runtime::RunPolicy,
    /// Cycle 21 / cluster #3 — per-host adaptive scheduler shared
    /// across handlers so concurrent scrapes coordinate. `None`
    /// disables throttling (test harnesses).
    pub scheduler: Option<Arc<HostScheduler>>,
    /// D2 / cluster #14 — HMAC signer for cross-plane requests. When
    /// `Some`, the edge stamps `X-Quarry-Sig*` headers on every
    /// forward; when `None`, calls go out unsigned (acceptable only
    /// in private-network single-tenant dev).
    pub internal_signer: Option<Arc<crate::internal_auth::InternalSigner>>,
    /// C30.1 / cluster #7 — durable job-history store. When `Some`,
    /// the edge serves `/v1/runs/:id/events` locally from Postgres
    /// instead of forwarding to control plane. Gated behind the
    /// `postgres-queue` feature so default builds don't pull `sqlx`.
    #[cfg(feature = "postgres-queue")]
    pub event_history: Option<Arc<quarry_runtime::postgres_event_history::PostgresEventHistory>>,
    /// C30.2 / cluster #9 — durable baseline + diff store. When
    /// `Some`, `/v1/change/*` routes serve locally; otherwise they
    /// return 501.
    #[cfg(feature = "postgres-queue")]
    pub baseline_store: Option<Arc<quarry_runtime::postgres_baseline_store::PostgresBaselineStore>>,
    /// P7 — real chromiumoxide-backed browser driver for the agentic loop.
    /// Acquires a leased session per `/v1/agent/runs`; one action per `/step`.
    #[cfg(feature = "browser-agent")]
    pub agent_driver: Arc<dyn quarry_browser::BrowserDriver>,
    /// P7 — live agent runs keyed by run_id (session + observation ctx + lease).
    #[cfg(feature = "browser-agent")]
    pub agent_runs: crate::agent_routes::AgentRuns,
}
