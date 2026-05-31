//! quarry-runtime — the engine.
//!
//! Owns: driver selection, static fetch, browser execution, action runtime,
//! transform/output packaging, retry/block/escalation, artifact writes,
//! fingerprint/diff, page-level progress events.
//!
//! Donor: `internal/driver/`, `internal/scraper/`, `internal/pipeline/`,
//! parts of `internal/crawl/runner.go`.

pub mod action_runtime;
pub mod agent_loop;
pub mod ai_formats;
pub mod answer;
pub mod artifact_store;
pub mod browser_driver;
pub mod dns_guard;
pub mod driver;
pub mod driver_plan;
pub mod driver_registry;
pub mod event_bus;
pub mod events;
pub mod host_scheduler;
pub mod intent_classifier;
pub mod nats_event_bus;

pub mod cached_profile_store;
pub mod crawl_frontier;
pub mod crawl_ranker;
pub mod crawl_signals;
pub mod deep_research;
pub mod fallback_driver;
pub mod fetch;
pub mod grant_validator;
#[cfg(feature = "grpc")]
pub mod grpc;
#[cfg(feature = "http3")]
pub mod http3;
pub mod ingest_client;
pub mod lease_pool;
pub mod local_index;
pub mod mp_client;
pub mod observation;
pub mod pipeline;
pub mod planner;
pub mod policy;
#[cfg(feature = "postgres-queue")]
pub mod postgres_baseline_store;
#[cfg(feature = "postgres-queue")]
pub mod postgres_event_history;
#[cfg(feature = "postgres-queue")]
pub mod postgres_profile_store;
#[cfg(feature = "postgres-queue")]
pub mod postgres_queue;
pub mod proxy_pool;
pub mod publisher;
pub mod request_queue;
pub mod retry;
pub mod robots_cache;
pub mod s3_profile_store;
pub mod serp;
pub mod smart_router;
pub mod step_receipts;
pub mod structured_extract;
#[cfg(feature = "test-site")]
pub mod test_site;
pub mod agent_memory;
pub mod autoscale;
pub mod fingerprint_rotation;
pub mod fusion;
pub mod hybrid;
pub mod tls_driver;
pub mod transport_fallback_driver;
pub mod usage;
pub mod vector_index;

pub use action_runtime::{ActionResult, ActionRuntime};
pub use agent_loop::{AgentLoop, AgentLoopResult, LoopTermination};
pub use ai_formats::{AiFormatRunner, JsonResult, QueryResult, SummaryResult};
pub use answer::{
    AnswerPipeline, AnswerRequest, AnswerResult, Citation, ClosureFetcher, MarkdownFetcher,
    SimpleHttpMarkdownFetcher,
};
pub use browser_driver::BrowserDriverAdapter;
pub use crawl_frontier::{
    CrawlFrontier, FrontierCheckpoint, FrontierConfigSnapshot, FrontierEntry,
};
pub use crawl_ranker::{CrawlRanker, LexicalRanker, ModelPlaneRanker, RankedUrl};
pub use crawl_signals::{
    pair as crawl_signals_pair, CrawlSignal, CrawlSignalsConsumer, CrawlSignalsProducer,
};
pub use deep_research::{
    ResearchArtifacts, ResearchExecutor, ResearchStatus, ResearchTask, ResearchTaskEnvelope,
    ResearchTaskResult,
};
pub use driver::{BrowserMeta, Driver, DriverSelection};
pub use driver_plan::DriverPlan;
pub use driver_registry::DriverRegistry;
pub use event_bus::{EventBus, EventReceiver, InProcessEventBus};
pub use events::EventSink;
pub use fallback_driver::FallbackDriver;
pub use fetch::StaticDriver;
pub use agent_memory::{redact_sensitive, AgentScratchpad};
pub use autoscale::{global_autoscale, next_target, AutoscaledPool};
pub use fingerprint_rotation::{is_block_status, FingerprintRotator};
pub use fusion::{rrf_fuse, RRF_K};
pub use hybrid::HybridSearchProvider;
pub use vector_index::{DataPlaneVectorIndex, NoopVectorIndex, VectorHit, VectorIndex};
pub use grant_validator::{
    GrantValidation, GrantValidator, HttpGrantValidator, NoopGrantValidator,
};
pub use host_scheduler::{BadKind, HostScheduler, HostStats, SchedulerConfig, SlotPermit};
pub use ingest_client::IngestClient;
pub use intent_classifier::{
    CachedClassifier, HybridClassifier, IntentClassifier, MpIntentClassifier, RuleClassifier,
};
pub use lease_pool::RuntimeLeasePool;
pub use local_index::{LocalDocument, TantivyLocalIndex};
pub use mp_client::{
    ModelPlaneClient, ModelPlaneInvokeRequest, ModelPlaneInvokeResponse, ModelPlanePlanner,
};
pub use nats_event_bus::{NatsConfig, NatsEventBus};
pub use pipeline::PageRunner;
pub use planner::{MockPlanner, Planner, PlannerDecision};
pub use policy::{
    policy_fingerprint, record_determinism_inputs, BlockPolicy, CheckpointPolicy, Determinism,
    DeterminismIdentity, DiscoveryPolicy, ExtractionPolicy, FetchPolicy, RetryPolicy, RunPolicy,
};
pub use publisher::EventPublisher;
pub use quarry_tls::TlsProfile;
pub use request_queue::{InMemoryRequestQueue, Priority, QueueStats, QueuedRequest, RequestQueue};
pub use retry::{backoff_delay, classify, execute_with_retry, wait, IdempotencyKey, RetryClass};
pub use robots_cache::{ReqwestRobotsFetcher, RobotsCache, RobotsFetcher};
pub use s3_profile_store::S3ProfileStore;
pub use serp::{
    BraveSearch, FallbackSearchProvider, SearXNGSearch, SearchOptions, SearchProvider,
    SearchResult, SerperSearch, StractSearch,
};
pub use smart_router::{
    classify_intent, QueryIntent, RouterConfig, SmartSearchRouter, SmartSearchRouterBuilder,
};
pub use step_receipts::{
    InMemoryStepReceiptStore, ReceiptBuilder, StepOutcome, StepReceipt, StepReceiptStore,
};
pub use structured_extract::StructuredExtractClient;
pub use tls_driver::TlsProfileDriver;
pub use transport_fallback_driver::TransportFallbackDriver;
pub use usage::{metrics as usage_metrics, NatsUsageMeter, NoopUsageMeter, UsageEvent, UsageMeter};

#[cfg(test)]
pub(crate) mod tests {
    use async_trait::async_trait;
    use bytes::Bytes;
    use quarry_browser::{BrowserDriver, BrowserSession, SessionInner};
    use quarry_core::error::QuarryResult;
    use quarry_core::lease::BrowserLease;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    pub struct MockBrowserDriver;

    #[async_trait]
    impl BrowserDriver for MockBrowserDriver {
        async fn acquire(&self, lease: &BrowserLease) -> QuarryResult<BrowserSession> {
            Ok(BrowserSession {
                lease: lease.clone(),
                inner: Arc::new(Mutex::new(SessionInner::default())),
            })
        }
        async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
            Ok(())
        }
        async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
            Ok(())
        }
        async fn content(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::from_static(b"<html><body>mock</body></html>"))
        }
        async fn screenshot(
            &self,
            _session: &BrowserSession,
            _full_page: bool,
        ) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
        async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<Bytes> {
            Ok(Bytes::new())
        }
    }
}
