//! Shared application state for model-gateway.

use anyhow::Context;
use mp_contracts::dataplane::{
    documents_v2::document_service_client::DocumentServiceClient,
    graph_v1::graph_service_client::GraphServiceClient,
    knowledge_v2::knowledge_service_client::KnowledgeServiceClient,
    retrieval_v2::retrieval_service_client::RetrievalServiceClient,
    wiki_v1::wiki_service_client::WikiServiceClient,
};
use mp_contracts::model_plane::v1::{
    browser_broker_client::BrowserBrokerClient, capability_core_client::CapabilityCoreClient,
    execution_core_client::ExecutionCoreClient, finetune_jobs_client::FinetuneJobsClient,
    inference_core_client::InferenceCoreClient,
    managed_run_lifecycle_client::ManagedRunLifecycleClient,
    memory_service_client::MemoryServiceClient,
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    run_service_client::RunServiceClient, sandbox_manager_client::SandboxManagerClient,
    session_core_client::SessionCoreClient,
};
use mp_events::envelope::Envelope;
use mp_events::publisher::{EventPublisher, InMemoryPublisher, PublishError};
use std::sync::Arc;
use std::time::Duration;
use tonic::transport::{Channel, Endpoint};

use crate::finetune_azure::AzureFinetuneClient;
use crate::managed_start_key::ManagedStartKeyDeriver;
use crate::nats_publisher::NatsPublisher;
use crate::rate_limit::RateLimiter;
use crate::session_terminal_auth::SessionTerminalTokenProvider;

/// Type-erased publisher that dispatches to either NATS or in-memory.
pub enum DynPublisher {
    Nats(NatsPublisher),
    InMemory(InMemoryPublisher),
}

impl EventPublisher for DynPublisher {
    async fn publish(&self, subject: &str, envelope: &Envelope) -> Result<(), PublishError> {
        if envelope.zdr {
            tracing::debug!(subject = %subject, "ZDR envelope suppressed before publisher backend");
            return Ok(());
        }
        match self {
            Self::Nats(p) => p.publish(subject, envelope).await,
            Self::InMemory(p) => p.publish(subject, envelope).await,
        }
    }
}

impl DynPublisher {
    /// Drain buffered events when backed by the in-memory publisher.
    ///
    /// Returns an empty `Vec` for NATS-backed publishers (events are sent
    /// over the wire, not buffered). Intended for tests.
    #[must_use]
    pub fn drain(&self) -> Vec<(String, Envelope)> {
        match self {
            Self::InMemory(p) => p.drain(),
            Self::Nats(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod zdr_publisher_tests {
    use chrono::Utc;

    use super::*;

    #[tokio::test]
    async fn zdr_envelopes_never_enter_any_publisher_backend() {
        let publisher = DynPublisher::InMemory(InMemoryPublisher::new());
        let envelope = Envelope {
            event_id: "evt".into(),
            event_type: "TEST".into(),
            schema_version: 1,
            ts: Utc::now(),
            producer: "test".into(),
            correlation_id: "corr".into(),
            causation_id: String::new(),
            idempotency_key: "idem".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            resource_ref: "request/test".into(),
            payload: serde_json::json!({"content": "must-not-persist"}),
            zdr: true,
        };
        publisher.publish("subject", &envelope).await.unwrap();
        assert!(publisher.drain().is_empty());
    }
}

/// Shared state across HTTP and gRPC handlers.
#[derive(Clone)]
pub struct AppState {
    pub publisher: Arc<DynPublisher>,
    pub rate_limiter: RateLimiter,
    pub inference_client: InferenceCoreClient<Channel>,
    pub session_client: SessionCoreClient<Channel>,
    /// Additive managed-run terminalization service hosted by Session Core on
    /// the same gRPC endpoint. It deliberately remains separate from the
    /// legacy `SessionCore` client so existing callers retain their contracts.
    pub managed_run_client: ManagedRunLifecycleClient<Channel>,
    /// Fixed-scope Auth Core minting client used only for managed lifecycle
    /// terminal receipts and heartbeats. `None` is permitted solely for the
    /// in-memory unit-test constructor; `from_env` fails closed when absent.
    pub(crate) session_terminal_tokens: Option<Arc<SessionTerminalTokenProvider>>,
    /// Keyed, opaque transformer for all values persisted as managed start
    /// identities. This prevents public request/idempotency strings from
    /// becoming durable data (including on ZDR paths).
    pub(crate) managed_start_keys: ManagedStartKeyDeriver,
    /// Run read model + cancel path. Hosted by session-core on the same gRPC
    /// server, so it reuses the session endpoint/channel.
    pub run_client: RunServiceClient<Channel>,
    pub orchestration_client: OrchestrationCoreServiceClient<Channel>,
    /// execution-core (:9093). Used to resume a run after an approval is
    /// granted — the gateway is the approval decision point but does not drive
    /// the execution loop, so it signals execution-core directly.
    pub execution_client: ExecutionCoreClient<Channel>,
    pub sandbox_client: SandboxManagerClient<Channel>,
    pub browser_client: BrowserBrokerClient<Channel>,
    pub memory_client: MemoryServiceClient<Channel>,
    pub capability_client: CapabilityCoreClient<Channel>,
    /// Base URL of the capability-core HTTP API (e.g. "<http://capability-core:8085>").
    pub capability_core_base_url: String,
    /// Base URL of shipping-core's HTTP API (Ingestion Plane carrier aggregator),
    /// e.g. "<http://shipping-core:8080>". Used by the `shipping.get_quotes` tool.
    pub shipping_core_base_url: String,
    /// Base URL of information-core's HTTP API (Application Plane weather/
    /// traffic/news aggregator, wraps Yr/met.no), e.g.
    /// "<http://information-core:3190>". Used by the `get_weather` tool.
    pub information_core_base_url: String,
    /// Shared `x-internal-api-key` header value for calls to information-core.
    /// Empty disables the header entirely (dev-mode information-core may not
    /// enforce it); production sets `INFORMATION_CORE_INTERNAL_KEY` or falls
    /// back to the shared `INTERNAL_API_KEY`, mirroring execution-core's own
    /// `INFORMATION_CORE_INTERNAL_KEY` wiring for its separate `_EXEC` client.
    pub information_core_internal_key: String,
    /// Base URL of insight-core's HTTP API (Application Plane metrics/scorecard
    /// projection), e.g. "<http://insight-core:3163>". Used by the
    /// `insights.overview` read tool.
    /// Base URL of org-core's HTTP API (Control Plane), e.g.
    /// "<http://org-core:3010>". Read-only use: the per-org spend/token
    /// ceilings live there (`org_quotas`) because Control Plane owns quotas,
    /// and the gateway hands them to cost-core's budget check.
    pub org_core_base_url: String,
    /// Service identity presented to org-core (`x-service-id`). Must be
    /// registered in org-core's `ORG_CORE_SERVICE_CREDENTIALS`.
    pub org_core_service_id: String,
    /// Matching secret (`x-service-token`). Empty means "quotas unconfigured":
    /// no org ceiling is applied, and `SetPolicy` refuses the cap fields rather
    /// than pretending to store them.
    pub org_core_service_token: String,
    pub insight_core_base_url: String,
    /// Base URL of social-core's HTTP API (Application Plane social accounts,
    /// posts, and campaigns), e.g. "<http://social-core:3162>". Used by the
    /// `social.list_*` read tools.
    pub social_core_base_url: String,
    /// Shared `x-internal-api-key` header value for calls to the Application
    /// Plane cores that gate on it (insight-core, social-core). Both read
    /// `INTERNAL_API_KEY` on their own side, so this defaults to the same
    /// shared secret; `APPLICATION_CORE_INTERNAL_KEY` overrides it. Empty means
    /// the tools refuse to call rather than send an unauthenticated request —
    /// those cores 401 a missing key, and an honest local error beats a
    /// round-trip that can only fail.
    pub application_core_internal_key: String,
    /// Verevon's own public origin (e.g. "<http://localhost:5173>"), as seen by
    /// a user's browser through the gateway. Used only to build the OAuth
    /// `redirect_uri` for MCP server connections — an external authorization
    /// server must redirect back to a URL the browser can actually reach,
    /// never model-gateway's internal address.
    pub verevon_public_origin: String,
    /// Shared static secret proving to capability-core that a `GET
    /// .../oauth-token` call genuinely came from model-gateway, for the one
    /// call site (`proxy_mcp_tool`'s execution-core-triggered dispatch) that
    /// has no live per-user bearer to forward — unlike every other
    /// gateway->capability-core call. Deliberately Model-Plane-local rather
    /// than a minted JWT: auth-core's service-principal minter requires a
    /// fixed org allowlist in production, which does not fit "whichever
    /// arbitrary tenant owns the server being called." Empty disables the
    /// resolve path entirely (falls back to `server.token`).
    pub mcp_oauth_service_token: String,
    /// ADR-0003's platform layer of the authored-instruction hierarchy:
    /// deployment-level configuration, not a database row — no "platform
    /// operator" authority tier exists anywhere in this codebase (roles stop
    /// at org-admin), and inventing one for a value with exactly one
    /// realistic editor (whoever operates the deployment) is out of scope.
    /// Empty means no platform layer, same silent-absent contract as every
    /// other layer.
    pub platform_instructions: String,
    /// Shared reqwest client for HTTP proxy calls to capability-core.
    pub http_client: reqwest::Client,
    /// Phase 7 B5 — model price catalogue cache (cost-core `GET /api/v1/pricing`).
    /// Computes the streamed `Usage.cost_usd` off the same catalogue cost-core
    /// prices the durable ledger with, so the SSE and the ledger agree. Returns
    /// `None` (null cost) when cost-core is unreachable — never a fake figure.
    pub pricing: crate::pricing::PricingCache,
    // --- Data Plane v2 clients ---
    pub retrieval_client: RetrievalServiceClient<Channel>,
    pub document_client: DocumentServiceClient<Channel>,
    pub knowledge_client: KnowledgeServiceClient<Channel>,
    pub graph_client: GraphServiceClient<Channel>,
    /// `FinetuneJobs` runs inside session-core's gRPC server (same address as
    /// `session_client`). Shares the channel.
    pub finetune_jobs_client: FinetuneJobsClient<Channel>,
    /// Azure `OpenAI` HTTP client for the fine-tuning surface. `None` when
    /// `AZURE_OPENAI_ENDPOINT` / `AZURE_OPENAI_API_KEY` are unset; routes
    /// degrade to "persist row only" mode in that case.
    pub azure_finetune: Option<AzureFinetuneClient>,
    pub wiki_client: WikiServiceClient<Channel>,
    /// Phase 4 — Data Plane v2 durable-retrieval readiness registry. Populated
    /// by the `dataplane.documents.indexed` NATS consumer
    /// ([`crate::doc_indexed_consumer`]); lets the retrieval relay await a
    /// just-ingested document's embeddings landing instead of guessing.
    /// Best-effort, process-local, bounded; cheap clone (Arc-backed).
    pub doc_ready: crate::doc_indexed_consumer::DocReadyRegistry,
    /// chat-parity §4 — active in-flight stream cancellation registry. Populated
    /// by `/v1/invoke/stream`; flipped by `POST /v1/invoke/{id}/cancel`.
    pub cancels: crate::cancel_registry::CancelRegistry,
    /// Mid-run user input, delivered at the next tool-round boundary. Populated
    /// by `/v1/invoke/stream`; appended to by `POST /v1/invoke/{id}/queue`.
    /// Replaces the SPA silently discarding anything typed while streaming.
    pub queued_inputs: crate::queued_input::QueuedInputRegistry,
    /// chat-parity §1 — idempotent-regenerate guard for `/v1/invoke`. Keyed by
    /// a client-supplied `idempotency_key`; dedupes double-submit / regenerate
    /// retries so they neither re-run inference nor re-charge budget.
    pub idempotency: crate::idempotency_registry::IdempotencyRegistry,
    /// Wave 9 — Quarry-v2 edge client used by `Fetch` + `ExtractStructured`
    /// gRPC handlers. Constructed unconditionally; the client itself
    /// reports `Available() == false` when `QUARRY_EDGE_URL` is unset
    /// and the handlers degrade to `Unimplemented`.
    pub quarry: crate::quarry::Client,
    /// Wave 10b — in-memory plan-mode flag store. Shared across all
    /// handlers so the gateway and downstream tools see a consistent
    /// view. Cheap clone (Arc<DashMap>).
    pub plan_mode: crate::coordinator::PlanModeStore,
    /// Wave 10b — in-memory team-worker store. See
    /// `coordinator.rs` for the lifecycle.
    pub team_workers: crate::coordinator::TeamWorkerStore,
    /// Wave 10c — LSP bridge HTTP client. Empty base URL → handler
    /// returns Unimplemented. Configured via `LSP_BRIDGE_URL`.
    pub lsp: crate::lsp::BridgeClient,
    /// Wave 10e — in-memory approval store. Gateway-scoped, ephemeral.
    pub approvals: crate::approvals::ApprovalStore,
    /// Canvas artifact version counters, keyed by `(thread, artifact)`. The
    /// gateway assigns versions because the model cannot reliably remember what
    /// it emitted earlier in a thread; see `artifacts.rs` for the trade-off this
    /// in-memory store accepts.
    pub artifact_versions: crate::artifacts::ArtifactVersionStore,
    /// §23.6 tool result handles — oversized tool results held out of model
    /// context and queried by handle. Keyed `(org, user, handle)` and TTL'd;
    /// deliberately never populated on a ZDR turn (see
    /// `tool_result_handles`'s module docs).
    pub tool_results: crate::tool_result_handles::ToolResultStore,
    /// Wave 10f — in-memory trajectory ring-buffer. Bounded; older
    /// entries evicted FIFO. Durable retention should subscribe to the
    /// fixed `mp.v1.run.*.event` subject and filter `TRAJECTORY_RECORDED`.
    pub trajectories: crate::trajectory::TrajectoryStore,
    /// Resumable chat-stream delta buffer (`HARNESS_PHASE1` §3b). Process-local;
    /// powers `/v1/invoke/{request_id}/resume`. Swap for Redis in multi-replica.
    pub stream_buffers: crate::stream_buffer::StreamBufferStore,
    /// Wave 10d — skill catalogue (per-org markdown skills).
    pub skills: crate::skills::SkillStore,
    /// Wave 10g — MCP server registry + tool proxy client.
    pub mcp: crate::runtime_registries::McpRegistry,
    /// Per-resource ownership + sharing (owner-private / shared / org-wide),
    /// shared across the mcp/plugin/command/hook registries. Sits below tenant
    /// isolation; see [`crate::ownership`].
    pub ownership: crate::ownership::OwnershipStore,
    /// Wave 10h — plugin registry.
    pub plugins: crate::runtime_registries::PluginRegistry,
    /// Wave 10i — command, hook, permission, policy registries.
    pub commands: crate::runtime_registries::CommandRegistry,
    pub hooks: crate::runtime_registries::HookRegistry,
    /// Wave 10j — thread messages, analytics counters, task records.
    pub messages: crate::runtime_registries::MessageStore,
    pub analytics: crate::runtime_registries::AnalyticsStore,
    pub tasks: crate::runtime_registries::TaskStore,
    /// Bounds process-wide CONCURRENCY for untrusted tool-payload injection
    /// screening (`crate::moderation::screen_tool_payload`), per its
    /// size/deadline/concurrency contract. An instance field rather than a
    /// global static so tests can construct their own unshared semaphore
    /// instead of racing other tests through one process-wide singleton.
    /// `Arc`-wrapped solely because `AppState` derives `Clone` and
    /// `Semaphore` itself does not — every clone still shares the same
    /// bound, which is the point.
    pub screening_semaphore: Arc<tokio::sync::Semaphore>,
}

/// Cap on establishing a downstream gRPC connection, including the HTTP/2
/// handshake.
///
/// Without this a HALF-OPEN dependency wedges the caller forever: the TCP
/// connect succeeds so there is no fast `ECONNREFUSED`, but nothing ever
/// completes the handshake, and neither `connect_lazy` nor tonic imposes a
/// default bound. That defeats the point of the degrade-gracefully call sites
/// — `fetch_memory_context` catches an error and continues without context,
/// but it can never catch a call that does not return, so a single
/// unresponsive dependency blocks every non-ZDR invoke instead of costing it
/// some context.
///
/// Deliberately only a CONNECT bound, not a per-request `timeout`: a request
/// cap would also truncate legitimately long streaming RPCs.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// `Endpoint::from_static` + [`CONNECT_TIMEOUT`], for the fixed localhost
/// defaults. Panics on a malformed literal, which `from_static` does anyway.
fn lazy_static_channel(uri: &'static str) -> Channel {
    Endpoint::from_static(uri)
        .connect_timeout(CONNECT_TIMEOUT)
        .connect_lazy()
}

impl AppState {
    /// Create a new `AppState` with an in-memory publisher.
    #[must_use]
    pub fn new() -> Self {
        let inference_channel = lazy_static_channel("http://localhost:9092");
        let session_channel = lazy_static_channel("http://localhost:9091");
        // FinetuneJobs + RunService are hosted by session-core on the same gRPC
        // server, so they reuse the session channel. Cheap clone — Channel is
        // Arc<Inner>.
        let finetune_channel = session_channel.clone();
        let run_channel = session_channel.clone();
        let managed_run_channel = session_channel.clone();
        let orchestration_channel = lazy_static_channel("http://localhost:9080");
        let execution_channel = lazy_static_channel("http://localhost:9093");
        let sandbox_channel = lazy_static_channel("http://localhost:9094");
        let browser_channel = lazy_static_channel("http://localhost:9095");
        let memory_channel = lazy_static_channel("http://localhost:9091");
        let capability_channel = lazy_static_channel("http://localhost:9097");
        let dp_retrieval_channel = lazy_static_channel("http://localhost:50052");
        let dp_documents_channel = lazy_static_channel("http://localhost:50052");
        let dp_knowledge_channel = lazy_static_channel("http://localhost:50052");
        let dp_graph_channel = lazy_static_channel("http://localhost:50053");
        let dp_wiki_channel = lazy_static_channel("http://localhost:50054");
        Self {
            publisher: Arc::new(DynPublisher::InMemory(InMemoryPublisher::new())),
            rate_limiter: RateLimiter::from_env(),
            inference_client: InferenceCoreClient::new(inference_channel),
            session_client: SessionCoreClient::new(session_channel),
            managed_run_client: ManagedRunLifecycleClient::new(managed_run_channel),
            session_terminal_tokens: None,
            managed_start_keys: ManagedStartKeyDeriver::test_only(),
            run_client: RunServiceClient::new(run_channel),
            orchestration_client: OrchestrationCoreServiceClient::new(orchestration_channel),
            execution_client: ExecutionCoreClient::new(execution_channel),
            sandbox_client: SandboxManagerClient::new(sandbox_channel),
            browser_client: BrowserBrokerClient::new(browser_channel),
            memory_client: MemoryServiceClient::new(memory_channel),
            capability_client: CapabilityCoreClient::new(capability_channel),
            capability_core_base_url: "http://localhost:8085".to_owned(),
            shipping_core_base_url: "http://localhost:8080".to_owned(),
            information_core_base_url: "http://localhost:3190".to_owned(),
            information_core_internal_key: String::new(),
            org_core_base_url: "http://localhost:3010".to_owned(),
            org_core_service_id: "model-gateway".to_owned(),
            org_core_service_token: String::new(),
            insight_core_base_url: "http://localhost:3163".to_owned(),
            social_core_base_url: "http://localhost:3162".to_owned(),
            application_core_internal_key: String::new(),
            verevon_public_origin: "http://localhost:5173".to_owned(),
            mcp_oauth_service_token: String::new(),
            platform_instructions: String::new(),
            http_client: reqwest::Client::new(),
            // Pricing disabled by default (no cost-core URL); `from_env` wires it
            // from COST_CORE_URL. A disabled cache emits a null cost, never a fake.
            pricing: crate::pricing::PricingCache::new(None, reqwest::Client::new()),
            retrieval_client: RetrievalServiceClient::new(dp_retrieval_channel),
            document_client: DocumentServiceClient::new(dp_documents_channel),
            knowledge_client: KnowledgeServiceClient::new(dp_knowledge_channel),
            graph_client: GraphServiceClient::new(dp_graph_channel),
            finetune_jobs_client: FinetuneJobsClient::new(finetune_channel),
            azure_finetune: None,
            wiki_client: WikiServiceClient::new(dp_wiki_channel),
            // Phase 4 readiness registry. Shared as-is by `with_nats`/`from_env`
            // (both build on `new()`); the `dataplane.documents.indexed`
            // consumer is spawned against this same instance in `main.rs`.
            doc_ready: crate::doc_indexed_consumer::DocReadyRegistry::new(),
            cancels: crate::cancel_registry::CancelRegistry::new(),
            queued_inputs: crate::queued_input::QueuedInputRegistry::new(),
            idempotency: crate::idempotency_registry::IdempotencyRegistry::new(),
            // `Client::new` with empty base_url returns an Unavailable
            // client; the gRPC handlers degrade to Unimplemented in
            // that case.
            quarry: crate::quarry::Client::new(crate::quarry::Config::default()),
            plan_mode: crate::coordinator::PlanModeStore::new(),
            team_workers: crate::coordinator::TeamWorkerStore::new(),
            lsp: crate::lsp::BridgeClient::new(""),
            approvals: crate::approvals::ApprovalStore::new(),
            artifact_versions: crate::artifacts::ArtifactVersionStore::new(),
            tool_results: crate::tool_result_handles::ToolResultStore::new(),
            trajectories: crate::trajectory::TrajectoryStore::new(),
            stream_buffers: crate::stream_buffer::StreamBufferStore::new(),
            skills: crate::skills::SkillStore::new(),
            mcp: crate::runtime_registries::McpRegistry::new(),
            ownership: crate::ownership::OwnershipStore::new(),
            plugins: crate::runtime_registries::PluginRegistry::new(),
            commands: crate::runtime_registries::CommandRegistry::new(),
            hooks: crate::runtime_registries::HookRegistry::new(),
            messages: crate::runtime_registries::MessageStore::new(),
            analytics: crate::runtime_registries::AnalyticsStore::new(),
            tasks: crate::runtime_registries::TaskStore::new(),
            screening_semaphore: Arc::new(tokio::sync::Semaphore::new(
                crate::moderation::SCREENING_CONCURRENCY,
            )),
        }
    }

    /// Configure the narrow service credential exchange used only for managed
    /// run terminal receipts and heartbeats.
    ///
    /// This is intentionally separate from any caller or downstream user
    /// bearer. Production startup uses [`Self::from_env`]; explicit setup is
    /// provided for an embedding that supplies its own configuration source.
    ///
    /// # Errors
    ///
    /// Returns an error when the bounded Auth Core client cannot be created.
    pub fn configure_managed_terminalization(
        &mut self,
        auth_core_url: &str,
        service_id: &str,
        service_api_key: &str,
    ) -> anyhow::Result<()> {
        let provider = SessionTerminalTokenProvider::from_service_credential(
            auth_core_url,
            service_id.to_owned(),
            service_api_key.to_owned(),
        )
        .context("managed-run terminalization credential configuration failed")?;
        self.session_terminal_tokens = Some(Arc::new(provider));
        Ok(())
    }

    /// Create a new `AppState` with a NATS publisher.
    pub fn with_nats(nats: NatsPublisher) -> Self {
        let mut state = Self::new();
        state.publisher = Arc::new(DynPublisher::Nats(nats));
        state
    }

    fn read_endpoint(primary: &str, legacy: &str, default: &str) -> String {
        std::env::var(primary)
            .or_else(|_| std::env::var(legacy))
            .unwrap_or_else(|_| default.to_owned())
    }

    fn lazy_channel(primary: &str, legacy: &str, default: &str) -> anyhow::Result<Channel> {
        let endpoint = Self::read_endpoint(primary, legacy, default);
        let normalized = if endpoint.contains("://") {
            endpoint
        } else {
            format!("http://{endpoint}")
        };

        Endpoint::from_shared(normalized)
            .context("invalid downstream endpoint")
            .map(|endpoint| endpoint.connect_timeout(CONNECT_TIMEOUT).connect_lazy())
    }

    /// Create state from environment configuration.
    ///
    /// Uses `NatsPublisher` when `NATS_URL` is set, otherwise falls back to
    /// `InMemoryPublisher`.
    ///
    /// # Errors
    ///
    /// Returns an error if `NATS_URL` is set but the connection fails.
    // Flat constructor wiring ~18 env-keyed gRPC/HTTP clients across two
    // near-identical publisher branches (NATS vs in-memory). Both branches
    // assign the same fields — including `lsp` — so the only real difference
    // is the publisher backing. The repeated assignments keep each branch
    // readable but push the function past clippy's line limit.
    #[allow(clippy::too_many_lines)]
    pub async fn from_env() -> anyhow::Result<Self> {
        let inference_client = InferenceCoreClient::new(Self::lazy_channel(
            "INFERENCE_CORE_URL",
            "INFERENCE_CORE_ADDR",
            "http://localhost:9092",
        )?);
        let session_client = SessionCoreClient::new(Self::lazy_channel(
            "SESSION_CORE_URL",
            "SESSION_CORE_ADDR",
            "http://localhost:9091",
        )?);
        let managed_run_client = ManagedRunLifecycleClient::new(Self::lazy_channel(
            "SESSION_CORE_URL",
            "SESSION_CORE_ADDR",
            "http://localhost:9091",
        )?);
        let session_terminal_tokens = Arc::new(
            SessionTerminalTokenProvider::from_env()
                .context("managed-run terminalization credential configuration failed")?,
        );
        let managed_start_keys = ManagedStartKeyDeriver::from_env()
            .context("managed-run start-key credential configuration failed")?;
        let orchestration_client = OrchestrationCoreServiceClient::new(Self::lazy_channel(
            "ORCHESTRATOR_CORE_URL",
            "ORCHESTRATOR_CORE_ADDR",
            "http://localhost:9080",
        )?);
        let execution_client = ExecutionCoreClient::new(Self::lazy_channel(
            "EXECUTION_CORE_URL",
            "EXECUTION_CORE_ADDR",
            "http://localhost:9093",
        )?);
        let sandbox_client = SandboxManagerClient::new(Self::lazy_channel(
            "SANDBOX_MANAGER_URL",
            "SANDBOX_MANAGER_ADDR",
            "http://localhost:9094",
        )?);
        let browser_client = BrowserBrokerClient::new(Self::lazy_channel(
            "BROWSER_BROKER_URL",
            "BROWSER_BROKER_ADDR",
            "http://localhost:9095",
        )?);
        let memory_client = MemoryServiceClient::new(Self::lazy_channel(
            "MEMORY_SERVICE_URL",
            "LETTA_BRIDGE_ADDR",
            "http://localhost:9091",
        )?);
        let capability_client = CapabilityCoreClient::new(Self::lazy_channel(
            "CAPABILITY_CORE_URL",
            "CAPABILITY_CORE_ADDR",
            "http://localhost:9097",
        )?);
        let capability_core_base_url = std::env::var("CAPABILITY_CORE_HTTP_URL")
            .unwrap_or_else(|_| "http://localhost:8085".to_owned());
        let shipping_core_base_url = std::env::var("SHIPPING_CORE_URL")
            .unwrap_or_else(|_| "http://shipping-core:8080".to_owned());
        let information_core_base_url = std::env::var("INFORMATION_CORE_URL")
            .unwrap_or_else(|_| "http://information-core:3190".to_owned());
        let information_core_internal_key = std::env::var("INFORMATION_CORE_INTERNAL_KEY")
            .or_else(|_| std::env::var("INTERNAL_API_KEY"))
            .unwrap_or_default();
        // Control Plane's org-core. On `inter-plane-bus` it has no explicit
        // alias, so Docker resolves it by its COMPOSE SERVICE NAME (`org-core`),
        // not its container_name (`org-core-service`) — and its HTTP API is on
        // container port 8080, not the host-published 18080. Same shape as
        // AUTH_CORE_URL's `http://auth-core:3011`.
        let org_core_service_id = std::env::var("MODEL_GATEWAY_SERVICE_ID")
            .unwrap_or_else(|_| "model-gateway".to_owned());
        let org_core_service_token =
            std::env::var("MODEL_GATEWAY_ORG_CORE_SERVICE_TOKEN").unwrap_or_default();
        let org_core_base_url =
            std::env::var("ORG_CORE_URL").unwrap_or_else(|_| "http://org-core:8080".to_owned());
        let insight_core_base_url = std::env::var("INSIGHT_CORE_URL")
            .unwrap_or_else(|_| "http://insight-core:3163".to_owned());
        let social_core_base_url = std::env::var("SOCIAL_CORE_URL")
            .unwrap_or_else(|_| "http://social-core:3162".to_owned());
        let application_core_internal_key = std::env::var("APPLICATION_CORE_INTERNAL_KEY")
            .or_else(|_| std::env::var("INTERNAL_API_KEY"))
            .unwrap_or_default();
        let verevon_public_origin = std::env::var("VEREVON_PUBLIC_ORIGIN")
            .unwrap_or_else(|_| "http://localhost:5173".to_owned());
        let mcp_oauth_service_token = std::env::var("MCP_OAUTH_SERVICE_TOKEN").unwrap_or_default();
        let platform_instructions =
            std::env::var("PLATFORM_SYSTEM_INSTRUCTIONS").unwrap_or_default();
        let http_client = reqwest::Client::new();
        // Phase 7 B5 — pricing cache against cost-core's HTTP API (COST_CORE_URL).
        // Shared between both publisher branches below; cheap clone (Arc inner).
        let pricing_cache = crate::pricing::PricingCache::new(
            std::env::var("COST_CORE_URL").ok(),
            http_client.clone(),
        );

        // Data Plane v2 clients — retrieval-engine-rs serves Retrieval+Document+Knowledge on one port
        let retrieval_client = RetrievalServiceClient::new(Self::lazy_channel(
            "DATAPLANE_RETRIEVAL_URL",
            "DATAPLANE_RETRIEVAL_ADDR",
            "http://localhost:50052",
        )?);
        let document_client = DocumentServiceClient::new(Self::lazy_channel(
            "DATAPLANE_RETRIEVAL_URL",
            "DATAPLANE_RETRIEVAL_ADDR",
            "http://localhost:50052",
        )?);
        let knowledge_client = KnowledgeServiceClient::new(Self::lazy_channel(
            "DATAPLANE_RETRIEVAL_URL",
            "DATAPLANE_RETRIEVAL_ADDR",
            "http://localhost:50052",
        )?);
        let graph_client = GraphServiceClient::new(Self::lazy_channel(
            "DATAPLANE_GRAPH_URL",
            "DATAPLANE_GRAPH_ADDR",
            "http://localhost:50053",
        )?);
        let wiki_client = WikiServiceClient::new(Self::lazy_channel(
            "DATAPLANE_WIKI_URL",
            "DATAPLANE_WIKI_ADDR",
            "http://localhost:50054",
        )?);
        // FinetuneJobs is hosted by session-core; reuses the session endpoint.
        let finetune_jobs_client = FinetuneJobsClient::new(Self::lazy_channel(
            "SESSION_CORE_URL",
            "SESSION_CORE_ADDR",
            "http://localhost:9091",
        )?);
        // RunService is also hosted by session-core; reuses the session endpoint.
        let run_client = RunServiceClient::new(Self::lazy_channel(
            "SESSION_CORE_URL",
            "SESSION_CORE_ADDR",
            "http://localhost:9091",
        )?);
        // Azure OpenAI fine-tuning client (Some only when env is set).
        let azure_finetune = AzureFinetuneClient::from_env(http_client.clone());
        if azure_finetune.is_some() {
            tracing::info!("Azure OpenAI fine-tuning client configured");
        } else {
            tracing::info!(
                "AZURE_OPENAI_ENDPOINT/API_KEY not set; fine-tuning routes will persist rows only"
            );
        }

        // Wave 9 — Quarry edge for Fetch + ExtractStructured. Empty
        // QUARRY_EDGE_URL → client reports Available() == false and
        // the gRPC handlers return Unimplemented; safe in dev.
        let quarry_base_url = std::env::var("QUARRY_EDGE_URL").unwrap_or_default();
        let quarry_timeout = std::time::Duration::from_secs(
            std::env::var("QUARRY_EDGE_TIMEOUT_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
        );
        let quarry_client = if quarry_base_url.trim().is_empty() {
            crate::quarry::Client::new(crate::quarry::Config::default())
        } else {
            crate::quarry::Client::from_env(&quarry_base_url, quarry_timeout)?
        };
        if quarry_client.available() {
            tracing::info!("quarry edge client configured");
        } else {
            tracing::info!("QUARRY_EDGE_URL unset; Fetch/ExtractStructured RPCs disabled");
        }

        // Wave 10c — optional LSP bridge.
        let lsp_client = crate::lsp::BridgeClient::from_env();
        if lsp_client.available() {
            tracing::info!("LSP bridge configured");
        } else {
            tracing::info!("LSP_BRIDGE_URL unset; LspQuery RPC disabled");
        }

        if let Ok(url) = std::env::var("NATS_URL") {
            let nats = NatsPublisher::connect(&url).await?;
            let mut state = Self::with_nats(nats);
            state.inference_client = inference_client;
            state.session_client = session_client;
            state.managed_run_client = managed_run_client;
            state.session_terminal_tokens = Some(session_terminal_tokens);
            state.managed_start_keys = managed_start_keys;
            state.run_client = run_client;
            state.orchestration_client = orchestration_client;
            state.execution_client = execution_client.clone();
            state.sandbox_client = sandbox_client;
            state.browser_client = browser_client;
            state.memory_client = memory_client;
            state.capability_client = capability_client;
            state.capability_core_base_url = capability_core_base_url.clone();
            state.shipping_core_base_url = shipping_core_base_url.clone();
            state.information_core_base_url = information_core_base_url.clone();
            state.information_core_internal_key = information_core_internal_key.clone();
            state.org_core_base_url = org_core_base_url.clone();
            state.org_core_service_id = org_core_service_id.clone();
            state.org_core_service_token = org_core_service_token.clone();
            state.insight_core_base_url = insight_core_base_url.clone();
            state.social_core_base_url = social_core_base_url.clone();
            state.application_core_internal_key = application_core_internal_key.clone();
            state.verevon_public_origin = verevon_public_origin.clone();
            state.mcp_oauth_service_token = mcp_oauth_service_token.clone();
            state.platform_instructions = platform_instructions.clone();
            state.http_client = http_client;
            state.pricing = pricing_cache.clone();
            state.retrieval_client = retrieval_client;
            state.document_client = document_client;
            state.knowledge_client = knowledge_client;
            state.graph_client = graph_client;
            state.wiki_client = wiki_client;
            state.finetune_jobs_client = finetune_jobs_client;
            state.azure_finetune = azure_finetune.clone();
            state.quarry = quarry_client.clone();
            state.lsp = lsp_client.clone();
            // Redis-backed resume buffer when REDIS_URL is set (multi-replica).
            state.stream_buffers = crate::stream_buffer::StreamBufferStore::from_env().await;
            // plan_mode + team_workers default to empty stores —
            // explicit assignment is unnecessary but kept for symmetry
            // with the dev branch below.
            Ok(state)
        } else {
            tracing::info!("NATS_URL not set, using in-memory publisher");
            let mut state = Self::new();
            state.inference_client = inference_client;
            state.session_client = session_client;
            state.managed_run_client = managed_run_client;
            state.session_terminal_tokens = Some(session_terminal_tokens);
            state.managed_start_keys = managed_start_keys;
            state.run_client = run_client;
            state.orchestration_client = orchestration_client;
            state.execution_client = execution_client;
            state.sandbox_client = sandbox_client;
            state.browser_client = browser_client;
            state.memory_client = memory_client;
            state.capability_client = capability_client;
            state.capability_core_base_url = capability_core_base_url.clone();
            state.shipping_core_base_url = shipping_core_base_url.clone();
            state.information_core_base_url = information_core_base_url;
            state.information_core_internal_key = information_core_internal_key;
            state.org_core_base_url = org_core_base_url;
            state.org_core_service_id = org_core_service_id;
            state.org_core_service_token = org_core_service_token;
            state.insight_core_base_url = insight_core_base_url;
            state.social_core_base_url = social_core_base_url;
            state.application_core_internal_key = application_core_internal_key;
            state.verevon_public_origin = verevon_public_origin.clone();
            state.mcp_oauth_service_token = mcp_oauth_service_token.clone();
            state.platform_instructions = platform_instructions.clone();
            state.http_client = http_client;
            state.pricing = pricing_cache;
            state.retrieval_client = retrieval_client;
            state.document_client = document_client;
            state.knowledge_client = knowledge_client;
            state.graph_client = graph_client;
            state.wiki_client = wiki_client;
            state.finetune_jobs_client = finetune_jobs_client;
            state.azure_finetune = azure_finetune;
            state.quarry = quarry_client;
            // Mirror the NATS branch: without this, LspQuery is silently
            // disabled in dev mode even when LSP_BRIDGE_URL is configured.
            state.lsp = lsp_client;
            state.stream_buffers = crate::stream_buffer::StreamBufferStore::from_env().await;
            Ok(state)
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set or clear an env var, returning its prior value so the caller can
    /// restore it and leave the process environment unchanged for siblings.
    fn swap_env(key: &str, value: Option<&str>) -> Option<String> {
        let prev = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        prev
    }

    fn restore_env(key: &str, prev: Option<String>) {
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    /// Regression guard for the in-memory (`NATS_URL`-unset) branch of
    /// `from_env`: it must wire the LSP bridge client exactly like the NATS
    /// branch. A previous revision assigned `state.lsp` only on the NATS
    /// path, so `LspQuery` was silently disabled in dev mode even when
    /// `LSP_BRIDGE_URL` was configured. `NATS_URL` is removed to force the
    /// in-memory branch and `REDIS_URL` is removed to keep the stream buffer
    /// in-memory — both keep the call hermetic (no network I/O).
    #[tokio::test]
    #[serial_test::serial]
    async fn from_env_wires_lsp_on_in_memory_branch() {
        let prev_lsp = swap_env("LSP_BRIDGE_URL", Some("http://localhost:9099"));
        let prev_nats = swap_env("NATS_URL", None);
        let prev_redis = swap_env("REDIS_URL", None);
        let prev_auth_core = swap_env("AUTH_CORE_URL", Some("http://localhost:3011"));
        let prev_terminal_credential = swap_env(
            "MODEL_GATEWAY_SERVICE_API_KEY",
            Some("test-terminalization-credential"),
        );
        let prev_start_key_secret = swap_env(
            "MODEL_GATEWAY_MANAGED_START_KEY_SECRET",
            Some("test-managed-start-key-secret"),
        );

        let result = AppState::from_env().await;

        // Restore before asserting so an assertion failure can't leak env
        // state into other serial tests.
        restore_env("LSP_BRIDGE_URL", prev_lsp);
        restore_env("NATS_URL", prev_nats);
        restore_env("REDIS_URL", prev_redis);
        restore_env("AUTH_CORE_URL", prev_auth_core);
        restore_env("MODEL_GATEWAY_SERVICE_API_KEY", prev_terminal_credential);
        restore_env(
            "MODEL_GATEWAY_MANAGED_START_KEY_SECRET",
            prev_start_key_secret,
        );

        let state = result.expect("from_env must succeed on the in-memory path");
        assert!(
            state.lsp.available(),
            "in-memory branch must wire the LSP bridge when LSP_BRIDGE_URL is set"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn from_env_fails_closed_without_managed_terminalization_credentials() {
        let prev_nats = swap_env("NATS_URL", None);
        let prev_redis = swap_env("REDIS_URL", None);
        let prev_auth_core = swap_env("AUTH_CORE_URL", None);
        let prev_terminal_credential = swap_env("MODEL_GATEWAY_SERVICE_API_KEY", None);

        let result = AppState::from_env().await;

        restore_env("NATS_URL", prev_nats);
        restore_env("REDIS_URL", prev_redis);
        restore_env("AUTH_CORE_URL", prev_auth_core);
        restore_env("MODEL_GATEWAY_SERVICE_API_KEY", prev_terminal_credential);

        let error = match result {
            Ok(_) => panic!("managed terminalization credentials are mandatory"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("managed-run terminalization credential configuration failed"),
            "unexpected startup failure: {error:#}"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn from_env_fails_closed_without_managed_start_key_secret() {
        let prev_nats = swap_env("NATS_URL", None);
        let prev_redis = swap_env("REDIS_URL", None);
        let prev_auth_core = swap_env("AUTH_CORE_URL", Some("http://localhost:3011"));
        let prev_terminal_credential = swap_env(
            "MODEL_GATEWAY_SERVICE_API_KEY",
            Some("test-terminalization-credential"),
        );
        let prev_start_key_secret = swap_env("MODEL_GATEWAY_MANAGED_START_KEY_SECRET", None);

        let result = AppState::from_env().await;

        restore_env("NATS_URL", prev_nats);
        restore_env("REDIS_URL", prev_redis);
        restore_env("AUTH_CORE_URL", prev_auth_core);
        restore_env("MODEL_GATEWAY_SERVICE_API_KEY", prev_terminal_credential);
        restore_env(
            "MODEL_GATEWAY_MANAGED_START_KEY_SECRET",
            prev_start_key_secret,
        );

        let error = match result {
            Ok(_) => panic!("managed start-key secret is mandatory"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("managed-run start-key credential configuration failed"),
            "unexpected startup failure: {error:#}"
        );
    }
}
