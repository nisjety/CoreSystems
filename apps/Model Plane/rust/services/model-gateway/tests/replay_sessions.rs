//! Replays the synthetic `01-plain-answer` cassette through the real model-gateway router.
//!
//! `POST /v1/invoke/stream` runs the production handler: prompt assembly, the managed run, the
//! stream and the terminal receipt. Only the model is replaced, by `mp_inference_replay`, which
//! answers the gateway's calls from the cassette. The test asserts the SSE events and the final
//! run state, and `ReplayInference::finish` proves every recorded model call was served and matched.
//!
//! The session-core and managed-run doubles are copied from `e2e_invoke_chain_test.rs` (the blocks
//! this streamed chat path touches), so this suite runs without that 14,000-line file. Keep them in
//! step with it until both move to `tests/support/`; the copy leaves some items unused on purpose.
#![allow(dead_code, unused_imports)]

use axum::{
    body::to_bytes,
    body::Body,
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, Request, StatusCode,
    },
    middleware::{self, Next},
    Router,
};
use model_gateway::{
    author_guidance::AuthorGuidance,
    http_routes::build_router,
    latency_levers::LatencyLevers,
    review_proposal_rule::{ReviewProposalRule, PROPOSAL_RULE, PROPOSAL_RULE_ID},
    state::{AppState, DynPublisher},
};
use mp_contracts::model_plane::v1 as mpv1;
use mp_contracts::model_plane::v1::{
    execution_core_client::ExecutionCoreClient,
    execution_core_server::{ExecutionCore, ExecutionCoreServer},
    inference_core_client::InferenceCoreClient,
    inference_core_server::{InferenceCore, InferenceCoreServer},
    managed_run_lifecycle_client::ManagedRunLifecycleClient,
    managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
    message_tree_service_client::MessageTreeServiceClient,
    message_tree_service_server::{MessageTreeService, MessageTreeServiceServer},
    session_core_client::SessionCoreClient,
    session_core_server::{SessionCore, SessionCoreServer},
    AnalyzeDocumentRequest, AnalyzeDocumentResponse, AnalyzeImageRequest, AnalyzeImageResponse,
    AnalyzeLanguageRequest, AnalyzeLanguageResponse, AppendMessageRequest, AppendMessageResponse,
    BatchTranslateTextRequest, BatchTranslateTextResponse, CompactNowRequest, CompactNowResponse,
    CompleteStepRequest, CompleteStepResponse, ContextSegment, CreateEmbeddingRequest,
    CreateEmbeddingResponse, CreateRealtimeSessionRequest, CreateRealtimeSessionResponse,
    CreateThreadRequest, CreateThreadResponse, CreateVideoGenerationJobRequest,
    CreateVideoGenerationJobResponse, DetectTextLanguageRequest, DetectTextLanguageResponse, Event,
    ExtractImageTextRequest, ExtractImageTextResponse, FinalizeToolActionRequest,
    FinalizeToolActionResponse, GenerateImageRequest, GenerateImageResponse, GeneratedImage,
    GetContextAssemblyRequest, GetContextAssemblyResponse, GetVideoGenerationJobRequest,
    GetVideoGenerationJobResponse, HeartbeatManagedRunRequest, HeartbeatManagedRunResponse,
    InferChunk, InferRequest, InferResponse, LanguageAnalysisResult, ListAgentSkillsRequest,
    ListAgentSkillsResponse, ListConversationRequest, ListConversationResponse, ListModelsRequest,
    ListModelsResponse, ListSpeechVoicesRequest, ListSpeechVoicesResponse, ListThreadsRequest,
    ListThreadsResponse, ListTranslationLanguagesRequest, ListTranslationLanguagesResponse,
    ManagedRunSource, ModelInfo, RecordTerminalOutcomeRequest, RecordTerminalOutcomeResponse,
    ReplayThreadRequest, ReserveToolActionRequest, ReserveToolActionResponse,
    SaveCheckpointRequest, SaveCheckpointResponse, SessionMessage, SetActiveLeafRequest,
    SetActiveLeafResponse, SpeechVoiceInfo, StartManagedRunRequest, StartManagedRunResponse,
    StartRunRequest, StartRunResponse, StartScheduledRunRequest,
    StreamVideoGenerationContentRequest, StreamVideoGenerationContentResponse,
    SynthesizeSpeechRequest, SynthesizeSpeechResponse, TerminalOutcome, TranscribeSpeechRequest,
    TranscribeSpeechResponse, TranslateTextRequest, TranslateTextResponse, TranslationDetection,
    TranslationLanguageInfo,
};
use mp_events::publisher::InMemoryPublisher;
use mp_inference_replay::{cassette_dir, Cassette, ReplayInference};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio::{net::TcpListener, sync::OnceCell};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    transport::{Endpoint, Server},
    Request as TReq, Response, Status,
};
use tower::ServiceExt;
use wiremock::{
    matchers::{body_string_contains, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[path = "support/offline_downstreams.rs"]
mod offline_downstreams;

type MockStream = Pin<Box<dyn futures::Stream<Item = Result<InferChunk, Status>> + Send>>;
type MockVideoContentStream = Pin<
    Box<dyn futures::Stream<Item = Result<StreamVideoGenerationContentResponse, Status>> + Send>,
>;
type MockReplayStream = Pin<Box<dyn futures::Stream<Item = Result<Event, Status>> + Send>>;
type CapturedMessages = Arc<Mutex<Vec<Vec<(String, String)>>>>;
/// Each captured prompt tagged with the RPC that carried it: `"infer"` for the
/// tool-decision and auxiliary calls, `"infer_stream"` for a streamed answer.
type CapturedCalls = Arc<Mutex<Vec<(&'static str, Vec<(String, String)>)>>>;
type CapturedZdrRequests = Arc<Mutex<Vec<(&'static str, bool)>>>;
/// One unary `infer`: its request id, the tool names it offered and its
/// messages as (role, content).
type CapturedOffer = (String, Vec<String>, Vec<(String, String)>);
type CapturedOffers = Arc<Mutex<Vec<CapturedOffer>>>;
/// Every unary `infer` request as the gateway sent it, tool definitions
/// (descriptions and parameter schemas) and message bodies included.
type CapturedRequests = Arc<Mutex<Vec<InferRequest>>>;
/// The assistant turns Session Core was asked to save (with the turn's
/// metadata, its artifacts included), in order.
type CapturedCompletions = Arc<Mutex<Vec<mpv1::CompleteDirectRunWithMessageRequest>>>;

const TEST_AUTH_KID: &str = "model-gateway-zdr-route-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";

/// (authorization, request) per `CompleteDirectRunWithMessage` call.
type DirectCompletionCaptures =
    Arc<Mutex<Vec<(String, mpv1::CompleteDirectRunWithMessageRequest)>>>;
/// Committed operations: ((run id, idempotency key), message sequence).
type DirectCompletions = Arc<Mutex<Vec<((String, String), u64)>>>;

#[derive(Clone)]
struct MockSessionHandles {
    create_thread_count: Arc<AtomicUsize>,
    append_message_count: Arc<AtomicUsize>,
    managed_start_captures: Arc<Mutex<Vec<StartManagedRunRequest>>>,
    terminal_outcome_captures: Arc<Mutex<Vec<(String, RecordTerminalOutcomeRequest)>>>,
    heartbeat_captures: Arc<Mutex<Vec<(String, HeartbeatManagedRunRequest)>>>,
    context_assembly_count: Arc<AtomicUsize>,
    /// (role, `thread_id`) per `append_message` call
    append_captures: Arc<Mutex<Vec<(String, String)>>>,
    conversation: Arc<Mutex<Vec<(String, String, String)>>>,
    context_assembly_requests: Arc<Mutex<Vec<(String, String, u32)>>>,
    context_segments: Arc<Mutex<Option<Vec<ContextSegment>>>>,
    append_missing_thread_once: Arc<Mutex<Option<String>>>,
    reserve_tool_action_captures: Arc<Mutex<Vec<ReserveToolActionRequest>>>,
    finalize_tool_action_captures: Arc<Mutex<Vec<FinalizeToolActionRequest>>>,
    fail_reserve_tool_action: Arc<AtomicBool>,
    fail_finalize_tool_action: Arc<AtomicBool>,
    fail_terminal_outcome: Arc<AtomicBool>,
    fail_heartbeat: Arc<AtomicBool>,
    /// From this heartbeat on (1-based), report the run already terminal, as
    /// Session Core does once a durable cancel or its deadline ended it. 0:
    /// never.
    terminal_from_heartbeat: Arc<AtomicUsize>,
    /// Every `CompleteDirectRunWithMessage` call: (authorization, request).
    direct_completion_captures: DirectCompletionCaptures,
    /// Completed operations by (run id, idempotency key) -> message sequence,
    /// so a retry of the same operation replays instead of saving again.
    direct_completions: DirectCompletions,
    /// Answer UNIMPLEMENTED, as a Session Core without the atomic call does.
    direct_completion_unimplemented: Arc<AtomicBool>,
    /// Refuse the atomic call with this status, writing nothing, as Session
    /// Core does on a credential or ownership check.
    refuse_direct_completion: Arc<Mutex<Option<(tonic::Code, &'static str)>>>,
    /// Commit the first atomic completion, then lose its response (answer
    /// UNAVAILABLE), as a dropped connection after commit would.
    lose_first_completion_response: Arc<AtomicBool>,
    /// A durable cancel commits right after this heartbeat (1-based) reported
    /// the run live. From then on the run is terminal for every lifecycle call.
    /// 0: never.
    cancel_after_heartbeat: Arc<AtomicUsize>,
    run_cancelled: Arc<AtomicBool>,
    /// An atomic completion committed this turn's run. Heartbeats and terminal
    /// receipts then see it terminal, as Session Core would. Reset when the
    /// next turn starts its run (this mock reuses one run id per thread).
    run_completed: Arc<AtomicBool>,
    /// With `lose_first_completion_response`: the retry reaches a Session Core
    /// replica that has not been rebuilt and answers UNIMPLEMENTED.
    retry_reaches_old_replica: Arc<AtomicBool>,
    /// The delegated user bearer has expired by the time the assistant turn is
    /// saved: `AppendMessage(assistant)` answers UNAUTHENTICATED. The user turn,
    /// saved at the start of the request, is unaffected.
    expire_delegated_assistant_append: Arc<AtomicBool>,
    /// (role, `thread_id`) of `AppendMessage` RPCs only, unlike
    /// `append_captures`, which also holds messages the atomic call saved.
    append_rpc_captures: Arc<Mutex<Vec<(String, String)>>>,
    /// Every `RecordRunContext` call: (authorization, request).
    run_context_captures: Arc<Mutex<Vec<(String, mpv1::RecordRunContextRequest)>>>,
    /// `GetContextAssembly` never answers: it is counted, then the call stalls, as
    /// a session-core replica that accepted the connection and wedged would.
    context_assembly_hangs: Arc<AtomicBool>,
    /// Every `ListConversation` call: the thread-history read a `full` run context makes.
    list_conversation_count: Arc<AtomicUsize>,
    /// Every `SetActiveLeaf` call, as the gateway sent it (WS17 S9).
    set_active_leaf_captures: Arc<Mutex<Vec<SetActiveLeafRequest>>>,
    /// Scripted answers for the next `AppendMessage` calls, front first (WS01-S4). Empty answers
    /// as before.
    append_script: Arc<Mutex<std::collections::VecDeque<Result<AppendMessageResponse, Status>>>>,
    /// Every `AppendMessage` request, whole.
    append_request_captures: Arc<Mutex<Vec<AppendMessageRequest>>>,
    /// Refuse `StartManagedRun` as Session Core does when the Space thread already has a live
    /// root run: `Aborted` with `x-mp-error-code: thread_run_active` (WS09-S1).
    refuse_start_thread_run_active: Arc<AtomicBool>,
}

struct MockSessionCore {
    handles: MockSessionHandles,
}

impl MockSessionCore {
    fn new() -> (Self, MockSessionHandles) {
        let handles = MockSessionHandles {
            create_thread_count: Arc::new(AtomicUsize::new(0)),
            append_message_count: Arc::new(AtomicUsize::new(0)),
            managed_start_captures: Arc::new(Mutex::new(Vec::new())),
            terminal_outcome_captures: Arc::new(Mutex::new(Vec::new())),
            heartbeat_captures: Arc::new(Mutex::new(Vec::new())),
            context_assembly_count: Arc::new(AtomicUsize::new(0)),
            append_captures: Arc::new(Mutex::new(Vec::new())),
            conversation: Arc::new(Mutex::new(Vec::new())),
            context_assembly_requests: Arc::new(Mutex::new(Vec::new())),
            context_segments: Arc::new(Mutex::new(None)),
            append_missing_thread_once: Arc::new(Mutex::new(None)),
            reserve_tool_action_captures: Arc::new(Mutex::new(Vec::new())),
            finalize_tool_action_captures: Arc::new(Mutex::new(Vec::new())),
            fail_reserve_tool_action: Arc::new(AtomicBool::new(false)),
            fail_finalize_tool_action: Arc::new(AtomicBool::new(false)),
            fail_terminal_outcome: Arc::new(AtomicBool::new(false)),
            fail_heartbeat: Arc::new(AtomicBool::new(false)),
            terminal_from_heartbeat: Arc::new(AtomicUsize::new(0)),
            direct_completion_captures: Arc::new(Mutex::new(Vec::new())),
            direct_completions: Arc::new(Mutex::new(Vec::new())),
            direct_completion_unimplemented: Arc::new(AtomicBool::new(false)),
            refuse_direct_completion: Arc::new(Mutex::new(None)),
            lose_first_completion_response: Arc::new(AtomicBool::new(false)),
            cancel_after_heartbeat: Arc::new(AtomicUsize::new(0)),
            run_cancelled: Arc::new(AtomicBool::new(false)),
            run_completed: Arc::new(AtomicBool::new(false)),
            retry_reaches_old_replica: Arc::new(AtomicBool::new(false)),
            expire_delegated_assistant_append: Arc::new(AtomicBool::new(false)),
            append_rpc_captures: Arc::new(Mutex::new(Vec::new())),
            run_context_captures: Arc::new(Mutex::new(Vec::new())),
            context_assembly_hangs: Arc::new(AtomicBool::new(false)),
            list_conversation_count: Arc::new(AtomicUsize::new(0)),
            set_active_leaf_captures: Arc::new(Mutex::new(Vec::new())),
            append_script: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            append_request_captures: Arc::new(Mutex::new(Vec::new())),
            refuse_start_thread_run_active: Arc::new(AtomicBool::new(false)),
        };
        (
            Self {
                handles: handles.clone(),
            },
            handles,
        )
    }
}

#[tonic::async_trait]
impl SessionCore for MockSessionCore {
    type ReplayThreadStream = MockReplayStream;

    async fn create_thread(
        &self,
        request: TReq<CreateThreadRequest>,
    ) -> Result<Response<CreateThreadResponse>, Status> {
        self.handles
            .create_thread_count
            .fetch_add(1, Ordering::SeqCst);
        let req = request.into_inner();
        let thread_id = if req.session_key.is_empty() {
            "thread-generated".to_owned()
        } else {
            format!("thread-{}", req.session_key)
        };
        Ok(Response::new(CreateThreadResponse {
            thread_id,
            created_at: None,
        }))
    }

    async fn append_message(
        &self,
        request: TReq<AppendMessageRequest>,
    ) -> Result<Response<AppendMessageResponse>, Status> {
        self.handles
            .append_message_count
            .fetch_add(1, Ordering::SeqCst);
        let req = request.into_inner();
        self.handles
            .append_request_captures
            .lock()
            .unwrap()
            .push(req.clone());
        if let Some(scripted) = self.handles.append_script.lock().unwrap().pop_front() {
            return scripted.map(Response::new);
        }
        {
            let mut missing = self.handles.append_missing_thread_once.lock().unwrap();
            if missing.as_deref() == Some(req.thread_id.as_str()) {
                *missing = None;
                return Err(Status::internal(
                    r#"insert or update on table "messages" violates foreign key constraint"#,
                ));
            }
        }
        if req.role == "assistant"
            && self
                .handles
                .expire_delegated_assistant_append
                .load(Ordering::SeqCst)
        {
            return Err(Status::unauthenticated("invalid caller credential"));
        }
        self.handles
            .append_rpc_captures
            .lock()
            .unwrap()
            .push((req.role.clone(), req.thread_id.clone()));
        self.handles
            .append_captures
            .lock()
            .unwrap()
            .push((req.role.clone(), req.thread_id.clone()));
        self.handles
            .conversation
            .lock()
            .unwrap()
            .push((req.role, req.thread_id, req.content));
        Ok(Response::new(AppendMessageResponse {
            sequence: 1,
            ..Default::default()
        }))
    }

    async fn start_run(
        &self,
        _: TReq<StartRunRequest>,
    ) -> Result<Response<StartRunResponse>, Status> {
        Err(Status::unimplemented(
            "legacy StartRun must not be called by the managed Gateway path",
        ))
    }

    async fn start_scheduled_run(
        &self,
        _: TReq<StartScheduledRunRequest>,
    ) -> Result<Response<StartRunResponse>, Status> {
        Err(Status::unimplemented(
            "scheduled StartRun is not part of the gateway test path",
        ))
    }

    async fn record_scheduled_run_result(
        &self,
        _: TReq<mp_contracts::model_plane::v1::RecordScheduledRunResultRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::RecordScheduledRunResultResponse>, Status>
    {
        Err(Status::unimplemented(
            "record_scheduled_run_result is not part of the gateway test path",
        ))
    }

    // Not part of the persona spec — stubbed only so this mock keeps
    // implementing the full `SessionCore` trait after another in-flight
    // change added this RPC.
    async fn prepare_user_owned_thread(
        &self,
        _: TReq<mp_contracts::model_plane::v1::PrepareUserOwnedThreadRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::PrepareUserOwnedThreadResponse>, Status>
    {
        Err(Status::unimplemented(
            "prepare_user_owned_thread is not part of the gateway test path",
        ))
    }

    async fn complete_step(
        &self,
        _: TReq<CompleteStepRequest>,
    ) -> Result<Response<CompleteStepResponse>, Status> {
        Err(Status::unimplemented(
            "legacy CompleteStep must not be called by the managed Gateway path",
        ))
    }

    async fn reserve_tool_action(
        &self,
        request: TReq<ReserveToolActionRequest>,
    ) -> Result<Response<ReserveToolActionResponse>, Status> {
        if self.handles.fail_reserve_tool_action.load(Ordering::SeqCst) {
            return Err(Status::unavailable("audit outbox unavailable"));
        }
        self.handles
            .reserve_tool_action_captures
            .lock()
            .unwrap()
            .push(request.into_inner());
        Ok(Response::new(ReserveToolActionResponse { created: true }))
    }

    async fn save_loop_checkpoint(
        &self,
        _: TReq<mp_contracts::model_plane::v1::SaveLoopCheckpointRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::SaveLoopCheckpointResponse>, Status> {
        Err(Status::unimplemented(
            "save_loop_checkpoint not needed in test",
        ))
    }

    async fn claim_loop_checkpoint(
        &self,
        _: TReq<mp_contracts::model_plane::v1::ClaimLoopCheckpointRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::ClaimLoopCheckpointResponse>, Status> {
        Err(Status::unimplemented(
            "claim_loop_checkpoint not needed in test",
        ))
    }

    async fn list_tool_actions(
        &self,
        _: TReq<mp_contracts::model_plane::v1::ListToolActionsRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::ListToolActionsResponse>, Status> {
        Err(Status::unimplemented(
            "list_tool_actions not needed in test",
        ))
    }

    async fn close_loop_checkpoint(
        &self,
        _: TReq<mp_contracts::model_plane::v1::CloseLoopCheckpointRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::CloseLoopCheckpointResponse>, Status> {
        Err(Status::unimplemented(
            "close_loop_checkpoint not needed in test",
        ))
    }

    async fn finalize_tool_action(
        &self,
        request: TReq<FinalizeToolActionRequest>,
    ) -> Result<Response<FinalizeToolActionResponse>, Status> {
        if self
            .handles
            .fail_finalize_tool_action
            .load(Ordering::SeqCst)
        {
            return Err(Status::unavailable("audit outbox unavailable"));
        }
        self.handles
            .finalize_tool_action_captures
            .lock()
            .unwrap()
            .push(request.into_inner());
        Ok(Response::new(FinalizeToolActionResponse { updated: true }))
    }

    async fn save_checkpoint(
        &self,
        _: TReq<SaveCheckpointRequest>,
    ) -> Result<Response<SaveCheckpointResponse>, Status> {
        Err(Status::unimplemented(
            "save_checkpoint not needed in this test",
        ))
    }

    async fn list_agent_skills(
        &self,
        _: TReq<ListAgentSkillsRequest>,
    ) -> Result<Response<ListAgentSkillsResponse>, Status> {
        Err(Status::unimplemented(
            "list_agent_skills not needed in this test",
        ))
    }

    async fn list_conversation(
        &self,
        request: TReq<ListConversationRequest>,
    ) -> Result<Response<ListConversationResponse>, Status> {
        self.handles
            .list_conversation_count
            .fetch_add(1, Ordering::SeqCst);
        let req = request.into_inner();
        if req.include_inactive {
            // A version read (WS17 S9): every turn, each continuing the one before it, all on the
            // active path, so the newest turn is the leaf.
            let turns = mock_turns(&self.handles, &req.thread_id);
            let messages = turns
                .iter()
                .enumerate()
                .map(|(index, (id, role, content))| SessionMessage {
                    message_id: id.clone(),
                    role: role.clone(),
                    content: content.clone(),
                    agent_name: String::new(),
                    parent_message_id: index
                        .checked_sub(1)
                        .map(|previous| turns[previous].0.clone())
                        .unwrap_or_default(),
                    sibling_ordinal: 1,
                    sibling_count: 1,
                    on_active_path: true,
                    ..Default::default()
                })
                .collect();
            return Ok(Response::new(ListConversationResponse {
                messages,
                has_more: false,
                ..Default::default()
            }));
        }
        let messages = self
            .handles
            .conversation
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, thread_id, _)| thread_id == &req.thread_id)
            .map(|(role, _, content)| SessionMessage {
                message_id: String::new(),
                role: role.clone(),
                content: content.clone(),
                agent_name: String::new(),
                ..Default::default()
            })
            .collect();
        Ok(Response::new(ListConversationResponse {
            messages,
            has_more: false,
            ..Default::default()
        }))
    }

    async fn list_threads(
        &self,
        _: TReq<ListThreadsRequest>,
    ) -> Result<Response<ListThreadsResponse>, Status> {
        Ok(Response::new(ListThreadsResponse {
            threads: vec![],
            next_cursor: String::new(),
        }))
    }

    async fn update_thread_presentation(
        &self,
        _: TReq<mp_contracts::model_plane::v1::UpdateThreadPresentationRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::UpdateThreadPresentationResponse>, Status>
    {
        Err(Status::unimplemented(
            "update_thread_presentation not needed in this test",
        ))
    }

    async fn archive_thread(
        &self,
        _: TReq<mp_contracts::model_plane::v1::ArchiveThreadRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::ArchiveThreadResponse>, Status> {
        Err(Status::unimplemented(
            "archive_thread not needed in this test",
        ))
    }

    async fn archive_threads(
        &self,
        _: TReq<mp_contracts::model_plane::v1::ArchiveThreadsRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::ArchiveThreadsResponse>, Status> {
        Err(Status::unimplemented(
            "archive_threads not needed in this test",
        ))
    }

    async fn delete_thread(
        &self,
        _: TReq<mp_contracts::model_plane::v1::DeleteThreadRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::DeleteThreadResponse>, Status> {
        Err(Status::unimplemented(
            "delete_thread not needed in this test",
        ))
    }

    async fn delete_threads(
        &self,
        _: TReq<mp_contracts::model_plane::v1::DeleteThreadsRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::DeleteThreadsResponse>, Status> {
        Err(Status::unimplemented(
            "delete_threads not needed in this test",
        ))
    }

    async fn delete_space_threads(
        &self,
        _: TReq<mp_contracts::model_plane::v1::DeleteSpaceThreadsRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::DeleteSpaceThreadsResponse>, Status> {
        Err(Status::unimplemented(
            "delete_space_threads not needed in this test",
        ))
    }

    async fn prepare_scheduled_run_thread(
        &self,
        _: TReq<mp_contracts::model_plane::v1::PrepareScheduledRunThreadRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::PrepareScheduledRunThreadResponse>, Status>
    {
        Err(Status::unimplemented(
            "prepare_scheduled_run_thread not needed in this test",
        ))
    }

    async fn claim_scheduled_step(
        &self,
        _: TReq<mp_contracts::model_plane::v1::ClaimScheduledStepRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::ClaimScheduledStepResponse>, Status> {
        Err(Status::unimplemented(
            "claim_scheduled_step not needed in this test",
        ))
    }

    async fn record_scheduled_step_receipt(
        &self,
        _: TReq<mp_contracts::model_plane::v1::RecordScheduledStepReceiptRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::RecordScheduledStepReceiptResponse>, Status>
    {
        Err(Status::unimplemented(
            "record_scheduled_step_receipt not needed in this test",
        ))
    }

    async fn replay_thread(
        &self,
        _: TReq<ReplayThreadRequest>,
    ) -> Result<Response<Self::ReplayThreadStream>, Status> {
        Err(Status::unimplemented(
            "replay_thread not needed in this test",
        ))
    }

    async fn get_context_assembly(
        &self,
        request: TReq<GetContextAssemblyRequest>,
    ) -> Result<Response<GetContextAssemblyResponse>, Status> {
        self.handles
            .context_assembly_count
            .fetch_add(1, Ordering::SeqCst);
        let req = request.into_inner();
        self.handles
            .context_assembly_requests
            .lock()
            .unwrap()
            .push((req.thread_id, req.run_id, req.max_tokens));
        if self.handles.context_assembly_hangs.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        let Some(segments) = self.handles.context_segments.lock().unwrap().clone() else {
            return Err(Status::unimplemented(
                "get_context_assembly not configured in this test",
            ));
        };
        Ok(Response::new(GetContextAssemblyResponse {
            segments,
            estimated_tokens: 123,
            persona_enabled: Some(true),
        }))
    }

    async fn compact_now(
        &self,
        _: TReq<CompactNowRequest>,
    ) -> Result<Response<CompactNowResponse>, Status> {
        Err(Status::unimplemented("compact_now not needed in this test"))
    }

    async fn set_agent_skill_enabled(
        &self,
        _: TReq<mp_contracts::model_plane::v1::SetAgentSkillEnabledRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::SetAgentSkillEnabledResponse>, Status> {
        Err(Status::unimplemented(
            "set_agent_skill_enabled not needed in test",
        ))
    }

    async fn upsert_agent_skill(
        &self,
        _: TReq<mp_contracts::model_plane::v1::UpsertAgentSkillRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::UpsertAgentSkillResponse>, Status> {
        Err(Status::unimplemented(
            "upsert_agent_skill not needed in this test",
        ))
    }

    async fn set_run_mode(
        &self,
        _: TReq<mp_contracts::model_plane::v1::SetRunModeRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::SetRunModeResponse>, Status> {
        Err(Status::unimplemented(
            "set_run_mode not needed in this test",
        ))
    }

    async fn record_run_context(
        &self,
        request: TReq<mp_contracts::model_plane::v1::RecordRunContextRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::RecordRunContextResponse>, Status> {
        let authorization = request
            .metadata()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        self.handles
            .run_context_captures
            .lock()
            .unwrap()
            .push((authorization, request.into_inner()));
        Ok(Response::new(
            mp_contracts::model_plane::v1::RecordRunContextResponse {},
        ))
    }
}

#[derive(Clone)]
struct MockManagedRunLifecycle {
    handles: MockSessionHandles,
}

fn managed_terminal_step(source: ManagedRunSource) -> &'static str {
    match source {
        ManagedRunSource::GatewayDirect => "model-gateway-direct-inference-final",
        ManagedRunSource::ExecutionAgent => "execution-core-agent-final",
        ManagedRunSource::ExecutionBrowser => "execution-core-browser-final",
        ManagedRunSource::GatewayAgentDispatchRejected => "model-gateway-agent-dispatch-rejected",
        ManagedRunSource::GatewayBrowser => "model-gateway-browser-agent-final",
        ManagedRunSource::Unspecified => "",
    }
}

fn managed_source(source: i32) -> Result<ManagedRunSource, Status> {
    let source = ManagedRunSource::try_from(source)
        .map_err(|_| Status::invalid_argument("invalid managed terminal source"))?;
    if source == ManagedRunSource::Unspecified {
        return Err(Status::invalid_argument(
            "managed terminal source is required",
        ));
    }
    Ok(source)
}

fn service_authorization<T>(request: &TReq<T>) -> String {
    request
        .metadata()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[tonic::async_trait]
impl ManagedRunLifecycle for MockManagedRunLifecycle {
    async fn start_managed_run(
        &self,
        request: TReq<StartManagedRunRequest>,
    ) -> Result<Response<StartManagedRunResponse>, Status> {
        if self
            .handles
            .refuse_start_thread_run_active
            .load(Ordering::SeqCst)
        {
            let mut status = Status::aborted("thread already has an active run");
            status.metadata_mut().insert(
                "x-mp-error-code",
                tonic::metadata::MetadataValue::from_static("thread_run_active"),
            );
            status.metadata_mut().insert(
                "x-mp-active-run-id",
                tonic::metadata::MetadataValue::from_static("run-already-live"),
            );
            return Err(status);
        }
        let request = request.into_inner();
        let source = managed_source(request.terminal_source)?;
        let thread_id = if request.thread_id.is_empty() {
            "metadata-only-thread".to_owned()
        } else {
            request.thread_id.clone()
        };
        let run_id = format!("managed-run-for-{thread_id}");
        self.handles.run_completed.store(false, Ordering::SeqCst);
        self.handles
            .managed_start_captures
            .lock()
            .unwrap()
            .push(request);
        Ok(Response::new(StartManagedRunResponse {
            run_id,
            created_at: None,
            terminal_step_id: managed_terminal_step(source).to_owned(),
            already_started: false,
            thread_id,
        }))
    }

    async fn record_terminal_outcome(
        &self,
        request: TReq<RecordTerminalOutcomeRequest>,
    ) -> Result<Response<RecordTerminalOutcomeResponse>, Status> {
        let authorization = service_authorization(&request);
        let request = request.into_inner();
        let source = managed_source(request.source)?;
        self.handles
            .terminal_outcome_captures
            .lock()
            .unwrap()
            .push((authorization.clone(), request.clone()));
        if authorization != "Bearer gateway-terminalizer-token" {
            return Err(Status::unauthenticated(
                "managed terminal receipt requires the scoped service credential",
            ));
        }
        if self.handles.fail_terminal_outcome.load(Ordering::SeqCst) {
            return Err(Status::unavailable("terminal receipt unavailable"));
        }
        if self.handles.run_cancelled.load(Ordering::SeqCst)
            || self.handles.run_completed.load(Ordering::SeqCst)
        {
            return Err(Status::already_exists(
                "run terminal outcome is already assigned",
            ));
        }
        Ok(Response::new(RecordTerminalOutcomeResponse {
            run_id: request.run_id,
            source: source as i32,
            terminal_step_id: managed_terminal_step(source).to_owned(),
            step_index: 1,
            receipt_id: "receipt-managed-terminal".to_owned(),
            applied_at: None,
            already_applied: false,
            reconciliation_required: false,
        }))
    }

    /// Session Core's one-transaction save-and-complete. The message and the
    /// Completed receipt land in the same handles the two-call path fills, so
    /// every existing assertion about "the answer was saved and the run
    /// completed" holds for both paths; `direct_completion_captures` and
    /// `append_rpc_captures` tell the two apart.
    async fn complete_direct_run_with_message(
        &self,
        request: TReq<mpv1::CompleteDirectRunWithMessageRequest>,
    ) -> Result<Response<mpv1::CompleteDirectRunWithMessageResponse>, Status> {
        let authorization = service_authorization(&request);
        let request = request.into_inner();
        self.handles
            .direct_completion_captures
            .lock()
            .unwrap()
            .push((authorization.clone(), request.clone()));
        if self
            .handles
            .direct_completion_unimplemented
            .load(Ordering::SeqCst)
        {
            return Err(Status::unimplemented(
                "Operation is not implemented or not supported",
            ));
        }
        if authorization != "Bearer gateway-terminalizer-token" {
            return Err(Status::unauthenticated(
                "direct run completion requires the scoped service credential",
            ));
        }
        if let Some((code, reason)) = *self.handles.refuse_direct_completion.lock().unwrap() {
            return Err(Status::new(code, reason));
        }
        if self.handles.fail_terminal_outcome.load(Ordering::SeqCst) {
            return Err(Status::unavailable("terminal receipt unavailable"));
        }
        let receipt = |already_applied: bool| RecordTerminalOutcomeResponse {
            run_id: request.run_id.clone(),
            source: ManagedRunSource::GatewayDirect as i32,
            terminal_step_id: managed_terminal_step(ManagedRunSource::GatewayDirect).to_owned(),
            step_index: 1,
            receipt_id: "receipt-managed-terminal".to_owned(),
            applied_at: None,
            already_applied,
            reconciliation_required: false,
        };
        let operation = (request.run_id.clone(), request.idempotency_key.clone());
        let replayed = self
            .handles
            .direct_completions
            .lock()
            .unwrap()
            .iter()
            .find(|(done, _)| *done == operation)
            .map(|(_, sequence)| *sequence);
        if let Some(message_sequence) = replayed {
            return Ok(Response::new(mpv1::CompleteDirectRunWithMessageResponse {
                already_terminal: false,
                receipt: Some(receipt(true)),
                message_sequence,
            }));
        }
        // This mock reuses one run id per thread across turns, so only the
        // explicit cancel flag, not an earlier completion, ends the run here.
        if self.handles.run_cancelled.load(Ordering::SeqCst) {
            return Ok(Response::new(mpv1::CompleteDirectRunWithMessageResponse {
                already_terminal: true,
                receipt: None,
                message_sequence: 0,
            }));
        }
        // Commit: the message (if any) and the receipt, together.
        let message_sequence = if request.content.trim().is_empty() {
            0
        } else {
            self.handles
                .append_message_count
                .fetch_add(1, Ordering::SeqCst);
            self.handles
                .append_captures
                .lock()
                .unwrap()
                .push(("assistant".to_owned(), request.thread_id.clone()));
            self.handles.conversation.lock().unwrap().push((
                "assistant".to_owned(),
                request.thread_id.clone(),
                request.content.clone(),
            ));
            2
        };
        self.handles
            .terminal_outcome_captures
            .lock()
            .unwrap()
            .push((
                authorization,
                RecordTerminalOutcomeRequest {
                    run_id: request.run_id.clone(),
                    source: ManagedRunSource::GatewayDirect as i32,
                    outcome: TerminalOutcome::Completed as i32,
                    failure_code: String::new(),
                },
            ));
        self.handles
            .direct_completions
            .lock()
            .unwrap()
            .push((operation, message_sequence));
        self.handles.run_completed.store(true, Ordering::SeqCst);
        if self
            .handles
            .lose_first_completion_response
            .swap(false, Ordering::SeqCst)
        {
            if self
                .handles
                .retry_reaches_old_replica
                .load(Ordering::SeqCst)
            {
                self.handles
                    .direct_completion_unimplemented
                    .store(true, Ordering::SeqCst);
            }
            return Err(Status::unavailable("connection reset after commit"));
        }
        Ok(Response::new(mpv1::CompleteDirectRunWithMessageResponse {
            already_terminal: false,
            receipt: Some(receipt(false)),
            message_sequence,
        }))
    }

    // Only execution-core's delegation path records a run answer; the gateway
    // chain never should. Unimplemented rather than a stub success, so a caller
    // that starts doing it fails loudly instead of silently storing nothing.
    async fn record_run_output(
        &self,
        _: TReq<mp_contracts::model_plane::v1::RecordRunOutputRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::RecordRunOutputResponse>, Status> {
        Err(Status::unimplemented(
            "the gateway chain does not record run answers",
        ))
    }

    async fn heartbeat_managed_run(
        &self,
        request: TReq<HeartbeatManagedRunRequest>,
    ) -> Result<Response<HeartbeatManagedRunResponse>, Status> {
        let authorization = service_authorization(&request);
        let request = request.into_inner();
        let heartbeat = {
            let mut captures = self.handles.heartbeat_captures.lock().unwrap();
            captures.push((authorization.clone(), request.clone()));
            captures.len()
        };
        if authorization != "Bearer gateway-terminalizer-token" {
            return Err(Status::unauthenticated(
                "managed heartbeat requires the scoped service credential",
            ));
        }
        let _ = managed_source(request.source)?;
        if self.handles.fail_heartbeat.load(Ordering::SeqCst) {
            return Err(Status::unavailable("managed heartbeat unavailable"));
        }
        if self.handles.run_cancelled.load(Ordering::SeqCst)
            || self.handles.run_completed.load(Ordering::SeqCst)
        {
            return Ok(Response::new(HeartbeatManagedRunResponse {
                renewed_until: None,
                already_terminal: true,
            }));
        }
        let terminal_from = self.handles.terminal_from_heartbeat.load(Ordering::SeqCst);
        let already_terminal = terminal_from > 0 && heartbeat >= terminal_from;
        if !already_terminal
            && heartbeat == self.handles.cancel_after_heartbeat.load(Ordering::SeqCst)
        {
            // The cancel commits after this heartbeat has answered "live".
            self.handles.run_cancelled.store(true, Ordering::SeqCst);
        }
        Ok(Response::new(HeartbeatManagedRunResponse {
            renewed_until: None,
            already_terminal,
        }))
    }
}

fn zdr_route_test_keypair() -> &'static (String, String) {
    use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};

    static KEYPAIR: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
    KEYPAIR.get_or_init(|| {
        let mut rng = rand::thread_rng();
        let private_key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation");
        let public_key = rsa::RsaPublicKey::from(&private_key);
        (
            private_key
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private key PEM")
                .to_string(),
            public_key
                .to_public_key_pem(LineEnding::LF)
                .expect("public key PEM"),
        )
    })
}

async fn signed_claims_token(claims: serde_json::Value) -> (wiremock::MockServer, String) {
    use base64::Engine as _;
    use rsa::{pkcs8::DecodePublicKey, traits::PublicKeyParts};

    const AUDIENCE: &str = "model-gateway";

    let (private_pem, public_pem) = zdr_route_test_keypair();
    let public_key = rsa::RsaPublicKey::from_public_key_pem(public_pem).expect("decode public key");
    let n = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.n().to_bytes_be());
    let e = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(public_key.e().to_bytes_be());
    let jwks = wiremock::MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/jwks.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": TEST_AUTH_KID,
                "n": n,
                "e": e
            }]
        })))
        .mount(&jwks)
        .await;

    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
    std::env::set_var(
        "AUTH_CORE_JWKS_URL",
        format!("{}/.well-known/jwks.json", jwks.uri()),
    );
    std::env::set_var("AUTH_CORE_ISSUER", TEST_AUTH_ISSUER);
    std::env::set_var("AUTH_CORE_AUDIENCE", AUDIENCE);

    let token = encode_test_token(&claims, private_pem);
    (jwks, token)
}

fn encode_test_token(claims: &serde_json::Value, private_pem: &str) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(TEST_AUTH_KID.to_owned());
    encode(
        &header,
        claims,
        &EncodingKey::from_rsa_pem(private_pem.as_bytes()).expect("encoding key"),
    )
    .expect("signed JWT")
}

fn token_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_secs()
}

/// Set every key of the object `extra` on the object `claims`.
fn merge_claims(claims: &mut serde_json::Value, extra: &serde_json::Value) {
    for (key, value) in extra.as_object().expect("extra claims are an object") {
        claims[key] = value.clone();
    }
}

#[derive(Clone)]
struct DelegatedUserTokens {
    data_plane: String,
    session: String,
    inference: String,
    execution: String,
}

/// The user bearers of one turn. Every bearer carries the same `zdr` as the
/// turn's ingress token, because the gateway refuses a mixed set
/// (`zdr_posture_mismatch`); the caller passes the ingress posture.
async fn signed_delegated_user_tokens(
    org_id: &str,
    user_id: &str,
    zdr: bool,
) -> (wiremock::MockServer, DelegatedUserTokens) {
    signed_delegated_user_tokens_with_retention(org_id, user_id, serde_json::json!({ "zdr": zdr }))
        .await
}

/// The user bearers of one turn, each carrying `retention`: the object of retention claims (`zdr`, and for a
/// waived turn `org_zdr` and `zdr_waiver`) the turn's ingress token carries too.
async fn signed_delegated_user_tokens_with_retention(
    org_id: &str,
    user_id: &str,
    retention: serde_json::Value,
) -> (wiremock::MockServer, DelegatedUserTokens) {
    let dev_bypass = std::env::var("MODEL_GATEWAY_AUTH_DEV_BYPASS").ok();
    let now = token_now();
    let claims = |audience: &str| {
        let mut claims = serde_json::json!({
            "sub": user_id,
            "iss": TEST_AUTH_ISSUER,
            "aud": audience,
            "exp": now + 300,
            "nbf": now - 5,
            "org_id": org_id,
            "user_id": user_id,
            "principal_type": "user"
        });
        merge_claims(&mut claims, &retention);
        claims
    };
    let (jwks, inference) = signed_claims_token(claims("inference-core")).await;
    if let Some(dev_bypass) = dev_bypass {
        std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", dev_bypass);
    }
    let (private_pem, _) = zdr_route_test_keypair();
    std::env::set_var("DATA_PLANE_AUTH_AUDIENCE", "data-plane");
    std::env::set_var("SESSION_CORE_AUTH_AUDIENCE", "session-core");
    std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");
    std::env::set_var("EXECUTION_CORE_AUTH_AUDIENCE", "execution-core");
    (
        jwks,
        DelegatedUserTokens {
            data_plane: encode_test_token(&claims("data-plane"), private_pem),
            session: encode_test_token(&claims("session-core"), private_pem),
            inference,
            execution: encode_test_token(&claims("execution-core"), private_pem),
        },
    )
}

fn with_delegated_user_tokens(app: Router, tokens: DelegatedUserTokens) -> Router {
    app.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let tokens = tokens.clone();
            async move {
                let headers = request.headers_mut();
                for (name, token) in [
                    ("x-data-plane-authorization", tokens.data_plane),
                    ("x-session-authorization", tokens.session),
                    ("x-inference-authorization", tokens.inference),
                    ("x-execution-authorization", tokens.execution),
                ] {
                    headers.insert(
                        name,
                        HeaderValue::from_str(&format!("Bearer {token}"))
                            .expect("signed JWT is valid HTTP header content"),
                    );
                }
                next.run(request).await
            }
        },
    ))
}

/// Delegated user bearers for a non-ZDR turn. This is the posture of the dev
/// bypass ingress (`zdr: false`) and of a non-ZDR signed ingress.
async fn authenticated_user_router(
    app: Router,
    org_id: &str,
    user_id: &str,
) -> (Router, wiremock::MockServer) {
    let (jwks, tokens) = signed_delegated_user_tokens(org_id, user_id, false).await;
    (with_delegated_user_tokens(app, tokens), jwks)
}

struct SessionMockClients {
    session: SessionCoreClient<tonic::transport::Channel>,
    lifecycle: ManagedRunLifecycleClient<tonic::transport::Channel>,
    message_tree: MessageTreeServiceClient<tonic::transport::Channel>,
}

async fn spawn_session_mock(svc: MockSessionCore) -> SessionMockClients {
    let lifecycle = MockManagedRunLifecycle {
        handles: svc.handles.clone(),
    };
    let message_tree = MockMessageTree {
        handles: svc.handles.clone(),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(SessionCoreServer::new(svc))
            .add_service(ManagedRunLifecycleServer::new(lifecycle))
            .add_service(MessageTreeServiceServer::new(message_tree))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let ch = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    SessionMockClients {
        session: SessionCoreClient::new(ch.clone()),
        message_tree: MessageTreeServiceClient::new(ch.clone()),
        lifecycle: ManagedRunLifecycleClient::new(ch),
    }
}

/// The turns of one mock thread as (message id, role, content), oldest first. A turn's id is its thread
/// and its place, so a test names a turn without a real Session Core.
fn mock_turns(handles: &MockSessionHandles, thread_id: &str) -> Vec<(String, String, String)> {
    handles
        .conversation
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, thread, _)| thread == thread_id)
        .enumerate()
        .map(|(index, (role, _, content))| {
            (
                format!("{thread_id}-m{index}"),
                role.clone(),
                content.clone(),
            )
        })
        .collect()
}

struct MockMessageTree {
    handles: MockSessionHandles,
}

#[tonic::async_trait]
impl MessageTreeService for MockMessageTree {
    async fn set_active_leaf(
        &self,
        request: TReq<SetActiveLeafRequest>,
    ) -> Result<Response<SetActiveLeafResponse>, Status> {
        let req = request.into_inner();
        self.handles
            .set_active_leaf_captures
            .lock()
            .unwrap()
            .push(req.clone());
        let turns = mock_turns(&self.handles, &req.thread_id);
        let leaf = turns
            .last()
            .map(|(id, _, _)| id.clone())
            .unwrap_or_default();
        if req.expected_leaf_message_id != leaf {
            return Err(Status::aborted("active_leaf_stale"));
        }
        let Some(target) = turns
            .iter()
            .position(|(id, _, _)| *id == req.target_message_id)
        else {
            return Err(Status::invalid_argument(
                "target_message_id does not name a message in this thread",
            ));
        };
        Ok(Response::new(SetActiveLeafResponse {
            active_leaf_message_id: req.target_message_id.clone(),
            active_path: turns[..=target]
                .iter()
                .map(|(id, _, _)| id.clone())
                .collect(),
        }))
    }
}

async fn terminal_auth_core_url() -> String {
    static AUTH_CORE: OnceCell<MockServer> = OnceCell::const_new();
    let auth_core = AUTH_CORE
        .get_or_init(|| async {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/api/session-core/internal-token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "token": "gateway-terminalizer-token",
                    "expiresInSeconds": 300,
                    "audience": "session-core"
                })))
                .mount(&server)
                .await;
            server
        })
        .await;
    auth_core.uri()
}

async fn make_state(
    client: InferenceCoreClient<tonic::transport::Channel>,
    session_clients: SessionMockClients,
) -> (AppState, Arc<DynPublisher>) {
    let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
    let mut state = offline_downstreams::offline_state();
    state.publisher = publisher.clone();
    state.inference_client = client;
    state.managed_run_client = session_clients.lifecycle;
    state.session_client = session_clients.session;
    state.message_tree_client = session_clients.message_tree;
    state
        .configure_managed_terminalization(
            &terminal_auth_core_url().await,
            "model-gateway",
            "test-model-gateway-service-credential",
        )
        .expect("configure scoped terminalization credential");
    (state, publisher)
}

/// Return the only thread that Gateway prepared with `StartManagedRun`.
///
/// A client without a durable session key gets a fresh server-generated key,
/// so tests must verify ownership through the actual managed-start request rather
/// than a fixture-only literal thread id.
fn prepared_thread_id(handles: &MockSessionHandles) -> String {
    let starts = handles.managed_start_captures.lock().unwrap();
    assert_eq!(
        starts.len(),
        1,
        "Gateway must prepare exactly one run before terminalizing it"
    );
    starts[0].thread_id.clone()
}

fn managed_start_count(handles: &MockSessionHandles) -> usize {
    handles.managed_start_captures.lock().unwrap().len()
}

/// Assert that Gateway persisted its one user turn on the same thread supplied
/// to `StartManagedRun`, with no orphaned or duplicate user message.
fn assert_single_user_message_on_prepared_thread(
    handles: &MockSessionHandles,
    prepared_thread_id: &str,
) {
    let appended = handles.append_captures.lock().unwrap();
    let user_thread_ids = appended
        .iter()
        .filter(|(role, _)| role == "user")
        .map(|(_, thread_id)| thread_id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        user_thread_ids,
        vec![prepared_thread_id.to_owned()],
        "Gateway must persist exactly one user turn on the one prepared thread"
    );
}

/// Assert the exact direct-inference terminal receipt belongs to the run
/// returned by the prepared `StartManagedRun` call.
///
/// `MockManagedRunLifecycle::start_managed_run` deterministically returns
/// `managed-run-for-<thread>`.
/// Deriving the expected id from its captured request ensures this verifies the
/// real thread/run handoff instead of relying on a stale generated-thread
/// fixture value.
fn assert_direct_terminal_for_prepared_run(
    handles: &MockSessionHandles,
    outcome: TerminalOutcome,
    failure_code: &str,
    assertion_message: &str,
) {
    let thread_id = prepared_thread_id(handles);
    assert_single_user_message_on_prepared_thread(handles, &thread_id);
    let receipts = handles.terminal_outcome_captures.lock().unwrap();
    assert_eq!(receipts.len(), 1, "{assertion_message}");
    let (authorization, receipt) = &receipts[0];
    assert_eq!(
        authorization, "Bearer gateway-terminalizer-token",
        "{assertion_message}: terminalization must use the scoped workload token, not a user bearer"
    );
    assert_eq!(
        receipt.run_id,
        format!("managed-run-for-{thread_id}"),
        "{assertion_message}"
    );
    assert_eq!(
        receipt.source,
        ManagedRunSource::GatewayDirect as i32,
        "{assertion_message}"
    );
    assert_eq!(receipt.outcome, outcome as i32, "{assertion_message}");
    assert_eq!(receipt.failure_code, failure_code, "{assertion_message}");

    let heartbeats = handles.heartbeat_captures.lock().unwrap();
    assert!(
        heartbeats.len() >= 2,
        "{assertion_message}: Gateway must heartbeat before provider dispatch and before its terminal receipt"
    );
    for (authorization, heartbeat) in heartbeats.iter() {
        assert_eq!(
            authorization, "Bearer gateway-terminalizer-token",
            "{assertion_message}: heartbeat must use the scoped workload token, not a user bearer"
        );
        assert_eq!(
            heartbeat.run_id,
            format!("managed-run-for-{thread_id}"),
            "{assertion_message}: heartbeat must be bound to the prepared run"
        );
        assert_eq!(
            heartbeat.source,
            ManagedRunSource::GatewayDirect as i32,
            "{assertion_message}: direct inference must not heartbeat another producer's run"
        );
    }
}

fn sse_frames(body: &str) -> Vec<(Option<u64>, String, serde_json::Value)> {
    body.split("\n\n")
        .filter_map(|frame| {
            let event = frame
                .lines()
                .find_map(|line| line.strip_prefix("event: "))?;
            let id = frame
                .lines()
                .find_map(|line| line.strip_prefix("id: "))
                .and_then(|id| id.parse().ok());
            let data = frame
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .and_then(|data| serde_json::from_str(data).ok())
                .unwrap_or(serde_json::Value::Null);
            Some((id, event.to_owned(), data))
        })
        .collect()
}

/// The plain answer, end to end: the streamed model call, then the overhead calls the gateway makes
/// after it (two self-consistency draws, a thread title and follow-ups), one `done`, one completed run.
#[tokio::test]
#[serial_test::serial]
async fn plain_answer_streams_its_cassette_and_completes_the_run() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    // The cassette records the self-consistency draws, so an operator's "off" must not drop them.
    std::env::set_var("MODEL_GATEWAY_SELF_CONSISTENCY", "on");
    let source = std::fs::read_to_string(cassette_dir().join("01-plain-answer.jsonl"))
        .expect("the shipped cassette is readable");
    let replay = ReplayInference::new(Cassette::parse(&source).expect("the cassette parses"));
    let channel = mp_inference_replay::serve(replay.clone())
        .await
        .expect("the replay fake serves on loopback");
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(InferenceCoreClient::new(channel), session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"content":"hi","model":"m"}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let frames = sse_frames(&String::from_utf8(body.to_vec()).unwrap());
    let events: Vec<&str> = frames.iter().map(|(_, event, _)| event.as_str()).collect();
    assert_eq!(
        events.last().copied(),
        Some("done"),
        "the stream closes with done: {events:?}"
    );
    assert_eq!(
        events,
        ["connected", "chunk", "title", "follow_ups", "done"],
        "the plain answer's frames, in order"
    );
    let answer: String = frames
        .iter()
        .filter(|(_, event, _)| event == "chunk")
        .filter_map(|(_, _, data)| data["delta"].as_str())
        .collect();
    assert_eq!(answer, "PONG", "the streamed answer is the cassette's");
    assert_eq!(
        replay.finish(),
        Ok(()),
        "every recorded model call was served and matched"
    );
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Completed,
        "",
        "plain answer",
    );
}
