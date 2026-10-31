//! In-process stand-ins for every downstream gRPC service a gateway integration test does not
//! serve itself.
//!
//! `AppState::new()` points each downstream client at the dev stack's default port
//! (session-core `:9091`, inference-core `:9092`, orchestrator-core `:9080`, execution-core
//! `:9093`, ...) through a lazy channel. A test that serves only the clients its scenario
//! needs leaves the rest dialling whatever answers on those ports, so the same test saw three
//! different downstreams: a live dev stack answered with real data, a clean machine refused
//! the connection, and a wedged Docker engine (whose port proxy accepts and never answers)
//! hung it for as long as the test was allowed to run.
//!
//! [`offline_state`] starts from `AppState::new()` and replaces every gRPC client with one
//! over [`offline_channel`], which never opens a socket and never resolves a name: each call
//! fails at once with `Unavailable`, the answer a refused connection gives, so a scenario
//! that leaves a client unconfigured sees the same dependency failure on every machine. A
//! scenario that needs a client to answer overrides it afterwards, as it did before.
//!
//! Shared by the integration files that build `AppState` themselves, through
//! `#[path = "support/offline_downstreams.rs"]`. `offline_downstreams_contract.rs` pins the
//! behaviour and fails when `AppState` grows a gRPC client this function does not replace.
#![allow(dead_code)]

use hyper_util::rt::TokioIo;
use model_gateway::state::AppState;
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
    learning_governance_service_client::LearningGovernanceServiceClient,
    managed_run_lifecycle_client::ManagedRunLifecycleClient,
    memory_service_client::MemoryServiceClient,
    message_search_client::MessageSearchClient,
    message_tree_service_client::MessageTreeServiceClient,
    attention_service_client::AttentionServiceClient,
    orchestration_core_service_client::OrchestrationCoreServiceClient,
    run_service_client::RunServiceClient, sandbox_manager_client::SandboxManagerClient,
    session_core_client::SessionCoreClient,
};
use tonic::{
    codegen::http::Uri,
    transport::{Channel, Endpoint},
};

/// A channel whose every connect attempt fails in-process with `ConnectionRefused`: no socket,
/// no DNS, no port. A call over it answers `Unavailable` straight away and the channel tries
/// again on the next call, exactly as a lazy channel to a port nothing listens on does.
pub fn offline_channel() -> Channel {
    Endpoint::from_static("http://offline.invalid").connect_with_connector_lazy(tower::service_fn(
        |_: Uri| async {
            Err::<TokioIo<tokio::io::DuplexStream>, std::io::Error>(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "offline downstream double: this test does not serve this service",
            ))
        },
    ))
}

/// An `AppState` (with `AppState::new()`'s in-memory publisher and defaults) whose every gRPC
/// client is offline. Each client field is named below, so a client added to `AppState` has to
/// be added here too: `offline_downstreams_contract.rs` checks it.
pub fn offline_state() -> AppState {
    let mut state = AppState::new();
    state.inference_client = InferenceCoreClient::new(offline_channel());
    state.session_client = SessionCoreClient::new(offline_channel());
    state.message_search_client = MessageSearchClient::new(offline_channel());
    state.message_tree_client = MessageTreeServiceClient::new(offline_channel());
    state.attention_client = AttentionServiceClient::new(offline_channel());
    state.learning_client = LearningGovernanceServiceClient::new(offline_channel());
    state.managed_run_client = ManagedRunLifecycleClient::new(offline_channel());
    state.run_client = RunServiceClient::new(offline_channel());
    state.orchestration_client = OrchestrationCoreServiceClient::new(offline_channel());
    state.consume_client = OrchestrationCoreServiceClient::new(offline_channel());
    state.execution_client = ExecutionCoreClient::new(offline_channel());
    state.sandbox_client = SandboxManagerClient::new(offline_channel());
    state.browser_client = BrowserBrokerClient::new(offline_channel());
    state.memory_client = MemoryServiceClient::new(offline_channel());
    state.capability_client = CapabilityCoreClient::new(offline_channel());
    state.retrieval_client = RetrievalServiceClient::new(offline_channel());
    state.document_client = DocumentServiceClient::new(offline_channel());
    state.knowledge_client = KnowledgeServiceClient::new(offline_channel());
    state.graph_client = GraphServiceClient::new(offline_channel());
    state.finetune_jobs_client = FinetuneJobsClient::new(offline_channel());
    state.wiki_client = WikiServiceClient::new(offline_channel());
    state
}
