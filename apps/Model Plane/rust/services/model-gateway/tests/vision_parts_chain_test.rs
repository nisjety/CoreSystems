//! Images as parts of the turn, through the whole gateway chain (WS08-S4).
//!
//! A real `/v1/invoke/stream` turn against a scripted inference-core: the provider
//! records every `InferRequest` it is sent, so the tests read what the model would
//! have seen. The infrastructure (signed bearers, session and lifecycle doubles) is
//! the one `usage_meter_contract.rs` uses.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use axum::{
    body::{to_bytes, Body},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE},
        Request, StatusCode,
    },
    Router,
};
use model_gateway::{
    http_routes::build_router,
    state::{AppState, DynPublisher},
};
use mp_contracts::model_plane::v1 as mpv1;
use mp_contracts::model_plane::v1::{
    inference_core_client::InferenceCoreClient,
    inference_core_server::{InferenceCore, InferenceCoreServer},
    managed_run_lifecycle_client::ManagedRunLifecycleClient,
    managed_run_lifecycle_server::{ManagedRunLifecycle, ManagedRunLifecycleServer},
    session_core_client::SessionCoreClient,
    session_core_server::{SessionCore, SessionCoreServer},
};
use mp_events::publisher::InMemoryPublisher;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    transport::{Channel, Endpoint, Server},
    Request as TReq, Response, Status,
};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

/// Two tiny PNG-typed payloads: "hi" and "ho". The gateway judges the declared type and the
/// encoding, never the picture.
const IMAGE_A: &str = "aGk=";
const IMAGE_B: &str = "aG8=";
const QUESTION: &str = "Hva er forskjellen pa disse bildene?";

type Captured = Arc<Mutex<Vec<(&'static str, mpv1::InferRequest)>>>;

/// Tokens each tool-decision call reports. No call of this kind reaches the turn's
/// usage today.
const DECISION_INPUT: i32 = 1_000;
const DECISION_OUTPUT: i32 = 100;
/// Tokens the streamed answer reports: the counts the final `usage` frame states.
const ANSWER_INPUT: i32 = 42;
const ANSWER_OUTPUT: i32 = 7;
/// The terminal step a direct (non-agent) chat turn closes its run with.
const DIRECT_TERMINAL_STEP: &str = "model-gateway-direct-inference-final";
/// The identity the dev bypass gives every ingress bearer. The gateway refuses a
/// delegated bearer whose user or org differs from it, so each bearer names it too.
const DEV_ORG: &str = "org_placeholder";
const DEV_USER: &str = "user_placeholder";
const TEST_AUTH_KID: &str = "usage-meter-contract";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";

type MockStream = Pin<Box<dyn futures::Stream<Item = Result<mpv1::InferChunk, Status>> + Send>>;
type MockVideoStream = Pin<
    Box<
        dyn futures::Stream<Item = Result<mpv1::StreamVideoGenerationContentResponse, Status>>
            + Send,
    >,
>;
type MockReplayStream = Pin<Box<dyn futures::Stream<Item = Result<mpv1::Event, Status>> + Send>>;

/// An RPC this turn never reaches answers `UNIMPLEMENTED`.
fn unimplemented_rpc<T>(rpc: &'static str) -> Result<Response<T>, Status> {
    Err(Status::unimplemented(rpc))
}

/// The provider. Each unary call takes the next scripted round of tool calls; once the
/// script runs out, a call proposes none, which ends the tool phase.
struct ScriptedProvider {
    rounds: Mutex<VecDeque<Vec<mpv1::ToolCall>>>,
    unary_calls: Arc<AtomicUsize>,
    /// Every request the gateway sent, tagged with the RPC that carried it.
    captured: Captured,
    /// The catalogue `ListModels` answers with.
    models: Vec<mpv1::ModelInfo>,
    /// How many times the vision-only `AnalyzeImage` diversion was called.
    analyze_calls: Arc<AtomicUsize>,
}

#[tonic::async_trait]
impl InferenceCore for ScriptedProvider {
    type InferStreamStream = MockStream;
    type StreamVideoGenerationContentStream = MockVideoStream;

    async fn infer(
        &self,
        request: TReq<mpv1::InferRequest>,
    ) -> Result<Response<mpv1::InferResponse>, Status> {
        self.unary_calls.fetch_add(1, Ordering::SeqCst);
        self.captured
            .lock()
            .unwrap()
            .push(("infer", request.into_inner()));
        let tool_calls = self.rounds.lock().unwrap().pop_front().unwrap_or_default();
        Ok(Response::new(mpv1::InferResponse {
            model_used: "mock".into(),
            stop_reason: if tool_calls.is_empty() {
                "end_turn"
            } else {
                "tool_use"
            }
            .into(),
            input_tokens: DECISION_INPUT,
            output_tokens: DECISION_OUTPUT,
            tool_calls,
            ..Default::default()
        }))
    }

    async fn infer_stream(
        &self,
        request: TReq<mpv1::InferRequest>,
    ) -> Result<Response<MockStream>, Status> {
        self.captured
            .lock()
            .unwrap()
            .push(("infer_stream", request.into_inner()));
        let first = mpv1::InferChunk {
            delta: "hel".into(),
            model_used: "mock".into(),
            ..Default::default()
        };
        let last = mpv1::InferChunk {
            delta: "lo".into(),
            done: true,
            model_used: "mock".into(),
            stop_reason: "end_turn".into(),
            input_tokens: ANSWER_INPUT,
            output_tokens: ANSWER_OUTPUT,
            ..Default::default()
        };
        Ok(Response::new(Box::pin(futures::stream::iter(vec![
            Ok(first),
            Ok(last),
        ]))))
    }

    async fn create_embedding(
        &self,
        _: TReq<mpv1::CreateEmbeddingRequest>,
    ) -> Result<Response<mpv1::CreateEmbeddingResponse>, Status> {
        unimplemented_rpc("create_embedding")
    }
    async fn get_model_access(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::GetModelAccessRequest>,
    ) -> Result<tonic::Response<mp_contracts::model_plane::v1::GetModelAccessResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("get_model_access not used"))
    }

    async fn list_models(
        &self,
        _: TReq<mpv1::ListModelsRequest>,
    ) -> Result<Response<mpv1::ListModelsResponse>, Status> {
        Ok(Response::new(mpv1::ListModelsResponse {
            models: self.models.clone(),
            access: None,
        }))
    }
    async fn synthesize_speech(
        &self,
        _: TReq<mpv1::SynthesizeSpeechRequest>,
    ) -> Result<Response<mpv1::SynthesizeSpeechResponse>, Status> {
        unimplemented_rpc("synthesize_speech")
    }
    async fn transcribe_speech(
        &self,
        _: TReq<mpv1::TranscribeSpeechRequest>,
    ) -> Result<Response<mpv1::TranscribeSpeechResponse>, Status> {
        unimplemented_rpc("transcribe_speech")
    }
    async fn list_speech_voices(
        &self,
        _: TReq<mpv1::ListSpeechVoicesRequest>,
    ) -> Result<Response<mpv1::ListSpeechVoicesResponse>, Status> {
        unimplemented_rpc("list_speech_voices")
    }
    async fn translate_text(
        &self,
        _: TReq<mpv1::TranslateTextRequest>,
    ) -> Result<Response<mpv1::TranslateTextResponse>, Status> {
        unimplemented_rpc("translate_text")
    }
    async fn batch_translate_text(
        &self,
        _: TReq<mpv1::BatchTranslateTextRequest>,
    ) -> Result<Response<mpv1::BatchTranslateTextResponse>, Status> {
        unimplemented_rpc("batch_translate_text")
    }
    async fn detect_text_language(
        &self,
        _: TReq<mpv1::DetectTextLanguageRequest>,
    ) -> Result<Response<mpv1::DetectTextLanguageResponse>, Status> {
        unimplemented_rpc("detect_text_language")
    }
    async fn list_translation_languages(
        &self,
        _: TReq<mpv1::ListTranslationLanguagesRequest>,
    ) -> Result<Response<mpv1::ListTranslationLanguagesResponse>, Status> {
        unimplemented_rpc("list_translation_languages")
    }
    async fn generate_image(
        &self,
        _: TReq<mpv1::GenerateImageRequest>,
    ) -> Result<Response<mpv1::GenerateImageResponse>, Status> {
        unimplemented_rpc("generate_image")
    }
    async fn analyze_image(
        &self,
        _: TReq<mpv1::AnalyzeImageRequest>,
    ) -> Result<Response<mpv1::AnalyzeImageResponse>, Status> {
        self.analyze_calls.fetch_add(1, Ordering::SeqCst);
        Ok(Response::new(mpv1::AnalyzeImageResponse {
            description: "a vision-only description".into(),
            model_used: "mock-vision".into(),
            ..Default::default()
        }))
    }
    async fn extract_image_text(
        &self,
        _: TReq<mpv1::ExtractImageTextRequest>,
    ) -> Result<Response<mpv1::ExtractImageTextResponse>, Status> {
        unimplemented_rpc("extract_image_text")
    }
    async fn analyze_document(
        &self,
        _: TReq<mpv1::AnalyzeDocumentRequest>,
    ) -> Result<Response<mpv1::AnalyzeDocumentResponse>, Status> {
        unimplemented_rpc("analyze_document")
    }
    async fn analyze_language(
        &self,
        _: TReq<mpv1::AnalyzeLanguageRequest>,
    ) -> Result<Response<mpv1::AnalyzeLanguageResponse>, Status> {
        unimplemented_rpc("analyze_language")
    }
    async fn create_realtime_session(
        &self,
        _: TReq<mpv1::CreateRealtimeSessionRequest>,
    ) -> Result<Response<mpv1::CreateRealtimeSessionResponse>, Status> {
        unimplemented_rpc("create_realtime_session")
    }
    async fn create_video_generation_job(
        &self,
        _: TReq<mpv1::CreateVideoGenerationJobRequest>,
    ) -> Result<Response<mpv1::CreateVideoGenerationJobResponse>, Status> {
        unimplemented_rpc("create_video_generation_job")
    }
    async fn get_video_generation_job(
        &self,
        _: TReq<mpv1::GetVideoGenerationJobRequest>,
    ) -> Result<Response<mpv1::GetVideoGenerationJobResponse>, Status> {
        unimplemented_rpc("get_video_generation_job")
    }
    async fn stream_video_generation_content(
        &self,
        _: TReq<mpv1::StreamVideoGenerationContentRequest>,
    ) -> Result<Response<MockVideoStream>, Status> {
        unimplemented_rpc("stream_video_generation_content")
    }
}

/// The session half of Session Core that a direct chat turn reaches: its thread, the
/// saved messages, the audit of each tool call and the run's lifecycle. Its other RPCs
/// belong to agent, scheduled and Space paths this turn never takes.
struct ScriptedSession;

#[tonic::async_trait]
impl SessionCore for ScriptedSession {
    type ReplayThreadStream = MockReplayStream;

    async fn record_scheduled_run_result(
        &self,
        _: tonic::Request<mp_contracts::model_plane::v1::RecordScheduledRunResultRequest>,
    ) -> Result<Response<mp_contracts::model_plane::v1::RecordScheduledRunResultResponse>, Status> {
        unimplemented_rpc("record_scheduled_run_result")
    }

    async fn create_thread(
        &self,
        _: TReq<mpv1::CreateThreadRequest>,
    ) -> Result<Response<mpv1::CreateThreadResponse>, Status> {
        Ok(Response::new(mpv1::CreateThreadResponse {
            thread_id: "thread-usage-meter".into(),
            created_at: None,
        }))
    }
    async fn append_message(
        &self,
        _: TReq<mpv1::AppendMessageRequest>,
    ) -> Result<Response<mpv1::AppendMessageResponse>, Status> {
        Ok(Response::new(mpv1::AppendMessageResponse { sequence: 1, ..Default::default() }))
    }
    async fn start_run(
        &self,
        _: TReq<mpv1::StartRunRequest>,
    ) -> Result<Response<mpv1::StartRunResponse>, Status> {
        unimplemented_rpc("start_run")
    }
    async fn start_scheduled_run(
        &self,
        _: TReq<mpv1::StartScheduledRunRequest>,
    ) -> Result<Response<mpv1::StartRunResponse>, Status> {
        unimplemented_rpc("start_scheduled_run")
    }
    async fn prepare_scheduled_run_thread(
        &self,
        _: TReq<mpv1::PrepareScheduledRunThreadRequest>,
    ) -> Result<Response<mpv1::PrepareScheduledRunThreadResponse>, Status> {
        unimplemented_rpc("prepare_scheduled_run_thread")
    }
    async fn prepare_user_owned_thread(
        &self,
        _: TReq<mpv1::PrepareUserOwnedThreadRequest>,
    ) -> Result<Response<mpv1::PrepareUserOwnedThreadResponse>, Status> {
        unimplemented_rpc("prepare_user_owned_thread")
    }
    async fn complete_step(
        &self,
        _: TReq<mpv1::CompleteStepRequest>,
    ) -> Result<Response<mpv1::CompleteStepResponse>, Status> {
        unimplemented_rpc("complete_step")
    }
    async fn reserve_tool_action(
        &self,
        _: TReq<mpv1::ReserveToolActionRequest>,
    ) -> Result<Response<mpv1::ReserveToolActionResponse>, Status> {
        Ok(Response::new(mpv1::ReserveToolActionResponse {
            created: true,
        }))
    }
    async fn finalize_tool_action(
        &self,
        _: TReq<mpv1::FinalizeToolActionRequest>,
    ) -> Result<Response<mpv1::FinalizeToolActionResponse>, Status> {
        Ok(Response::new(mpv1::FinalizeToolActionResponse {
            updated: true,
        }))
    }
    async fn save_checkpoint(
        &self,
        _: TReq<mpv1::SaveCheckpointRequest>,
    ) -> Result<Response<mpv1::SaveCheckpointResponse>, Status> {
        unimplemented_rpc("save_checkpoint")
    }
    async fn save_loop_checkpoint(
        &self,
        _: TReq<mpv1::SaveLoopCheckpointRequest>,
    ) -> Result<Response<mpv1::SaveLoopCheckpointResponse>, Status> {
        unimplemented_rpc("save_loop_checkpoint")
    }
    async fn claim_loop_checkpoint(
        &self,
        _: TReq<mpv1::ClaimLoopCheckpointRequest>,
    ) -> Result<Response<mpv1::ClaimLoopCheckpointResponse>, Status> {
        unimplemented_rpc("claim_loop_checkpoint")
    }
    async fn close_loop_checkpoint(
        &self,
        _: TReq<mpv1::CloseLoopCheckpointRequest>,
    ) -> Result<Response<mpv1::CloseLoopCheckpointResponse>, Status> {
        unimplemented_rpc("close_loop_checkpoint")
    }
    async fn list_tool_actions(
        &self,
        _: TReq<mpv1::ListToolActionsRequest>,
    ) -> Result<Response<mpv1::ListToolActionsResponse>, Status> {
        unimplemented_rpc("list_tool_actions")
    }
    async fn replay_thread(
        &self,
        _: TReq<mpv1::ReplayThreadRequest>,
    ) -> Result<Response<MockReplayStream>, Status> {
        unimplemented_rpc("replay_thread")
    }
    async fn get_context_assembly(
        &self,
        _: TReq<mpv1::GetContextAssemblyRequest>,
    ) -> Result<Response<mpv1::GetContextAssemblyResponse>, Status> {
        unimplemented_rpc("get_context_assembly")
    }
    async fn compact_now(
        &self,
        _: TReq<mpv1::CompactNowRequest>,
    ) -> Result<Response<mpv1::CompactNowResponse>, Status> {
        unimplemented_rpc("compact_now")
    }
    async fn upsert_agent_skill(
        &self,
        _: TReq<mpv1::UpsertAgentSkillRequest>,
    ) -> Result<Response<mpv1::UpsertAgentSkillResponse>, Status> {
        unimplemented_rpc("upsert_agent_skill")
    }
    async fn list_agent_skills(
        &self,
        _: TReq<mpv1::ListAgentSkillsRequest>,
    ) -> Result<Response<mpv1::ListAgentSkillsResponse>, Status> {
        unimplemented_rpc("list_agent_skills")
    }
    async fn set_agent_skill_enabled(
        &self,
        _: TReq<mpv1::SetAgentSkillEnabledRequest>,
    ) -> Result<Response<mpv1::SetAgentSkillEnabledResponse>, Status> {
        unimplemented_rpc("set_agent_skill_enabled")
    }
    async fn list_conversation(
        &self,
        _: TReq<mpv1::ListConversationRequest>,
    ) -> Result<Response<mpv1::ListConversationResponse>, Status> {
        unimplemented_rpc("list_conversation")
    }
    async fn list_threads(
        &self,
        _: TReq<mpv1::ListThreadsRequest>,
    ) -> Result<Response<mpv1::ListThreadsResponse>, Status> {
        unimplemented_rpc("list_threads")
    }
    async fn update_thread_presentation(
        &self,
        _: TReq<mpv1::UpdateThreadPresentationRequest>,
    ) -> Result<Response<mpv1::UpdateThreadPresentationResponse>, Status> {
        unimplemented_rpc("update_thread_presentation")
    }
    async fn archive_thread(
        &self,
        _: TReq<mpv1::ArchiveThreadRequest>,
    ) -> Result<Response<mpv1::ArchiveThreadResponse>, Status> {
        unimplemented_rpc("archive_thread")
    }
    async fn archive_threads(
        &self,
        _: TReq<mpv1::ArchiveThreadsRequest>,
    ) -> Result<Response<mpv1::ArchiveThreadsResponse>, Status> {
        unimplemented_rpc("archive_threads")
    }
    async fn delete_thread(
        &self,
        _: TReq<mpv1::DeleteThreadRequest>,
    ) -> Result<Response<mpv1::DeleteThreadResponse>, Status> {
        unimplemented_rpc("delete_thread")
    }
    async fn delete_threads(
        &self,
        _: TReq<mpv1::DeleteThreadsRequest>,
    ) -> Result<Response<mpv1::DeleteThreadsResponse>, Status> {
        unimplemented_rpc("delete_threads")
    }
    async fn delete_space_threads(
        &self,
        _: TReq<mpv1::DeleteSpaceThreadsRequest>,
    ) -> Result<Response<mpv1::DeleteSpaceThreadsResponse>, Status> {
        unimplemented_rpc("delete_space_threads")
    }
    async fn set_run_mode(
        &self,
        _: TReq<mpv1::SetRunModeRequest>,
    ) -> Result<Response<mpv1::SetRunModeResponse>, Status> {
        unimplemented_rpc("set_run_mode")
    }
    async fn claim_scheduled_step(
        &self,
        _: TReq<mpv1::ClaimScheduledStepRequest>,
    ) -> Result<Response<mpv1::ClaimScheduledStepResponse>, Status> {
        unimplemented_rpc("claim_scheduled_step")
    }
    async fn record_scheduled_step_receipt(
        &self,
        _: TReq<mpv1::RecordScheduledStepReceiptRequest>,
    ) -> Result<Response<mpv1::RecordScheduledStepReceiptResponse>, Status> {
        unimplemented_rpc("record_scheduled_step_receipt")
    }
    async fn record_run_context(
        &self,
        _: TReq<mpv1::RecordRunContextRequest>,
    ) -> Result<Response<mpv1::RecordRunContextResponse>, Status> {
        unimplemented_rpc("record_run_context")
    }
}

/// The run lifecycle a direct turn opens, heartbeats and closes with a receipt.
struct ScriptedLifecycle;

#[tonic::async_trait]
impl ManagedRunLifecycle for ScriptedLifecycle {
    async fn start_managed_run(
        &self,
        request: TReq<mpv1::StartManagedRunRequest>,
    ) -> Result<Response<mpv1::StartManagedRunResponse>, Status> {
        let thread_id = request.into_inner().thread_id;
        Ok(Response::new(mpv1::StartManagedRunResponse {
            run_id: format!("managed-run-for-{thread_id}"),
            created_at: None,
            terminal_step_id: DIRECT_TERMINAL_STEP.into(),
            already_started: false,
            thread_id,
        }))
    }
    async fn record_terminal_outcome(
        &self,
        request: TReq<mpv1::RecordTerminalOutcomeRequest>,
    ) -> Result<Response<mpv1::RecordTerminalOutcomeResponse>, Status> {
        Ok(Response::new(terminal_receipt(
            &request.into_inner().run_id,
        )))
    }
    async fn heartbeat_managed_run(
        &self,
        _: TReq<mpv1::HeartbeatManagedRunRequest>,
    ) -> Result<Response<mpv1::HeartbeatManagedRunResponse>, Status> {
        Ok(Response::new(mpv1::HeartbeatManagedRunResponse {
            renewed_until: None,
            already_terminal: false,
        }))
    }
    async fn record_run_output(
        &self,
        _: TReq<mpv1::RecordRunOutputRequest>,
    ) -> Result<Response<mpv1::RecordRunOutputResponse>, Status> {
        unimplemented_rpc("record_run_output")
    }
    async fn complete_direct_run_with_message(
        &self,
        request: TReq<mpv1::CompleteDirectRunWithMessageRequest>,
    ) -> Result<Response<mpv1::CompleteDirectRunWithMessageResponse>, Status> {
        let run_id = request.into_inner().run_id;
        Ok(Response::new(mpv1::CompleteDirectRunWithMessageResponse {
            already_terminal: false,
            receipt: Some(terminal_receipt(&run_id)),
            message_sequence: 1,
        }))
    }
}

fn terminal_receipt(run_id: &str) -> mpv1::RecordTerminalOutcomeResponse {
    mpv1::RecordTerminalOutcomeResponse {
        run_id: run_id.to_owned(),
        source: mpv1::ManagedRunSource::GatewayDirect as i32,
        terminal_step_id: DIRECT_TERMINAL_STEP.to_owned(),
        step_index: 1,
        receipt_id: "receipt-usage-meter".to_owned(),
        applied_at: None,
        already_applied: false,
        reconciliation_required: false,
    }
}

async fn connect(addr: SocketAddr) -> Channel {
    Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap()
}

async fn spawn_provider(provider: ScriptedProvider) -> InferenceCoreClient<Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(InferenceCoreServer::new(provider))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    InferenceCoreClient::new(connect(addr).await)
}

/// Session Core serves both services on one port, as the gateway reaches them.
async fn spawn_session() -> Channel {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(SessionCoreServer::new(ScriptedSession))
            .add_service(ManagedRunLifecycleServer::new(ScriptedLifecycle))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    connect(addr).await
}

/// The key the delegated bearers are signed with. It is generated once per test binary,
/// so no key material lives in the repository. Returns the private key as PEM and the
/// public key's JWK `n` and `e`.
fn test_signing_key() -> &'static (String, String, String) {
    use base64::Engine as _;
    use rsa::{
        pkcs8::{EncodePrivateKey, LineEnding},
        traits::PublicKeyParts,
    };

    static KEY: std::sync::OnceLock<(String, String, String)> = std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        let mut rng = rand::thread_rng();
        let private_key = rsa::RsaPrivateKey::new(&mut rng, 2048).expect("RSA key generation");
        let public_key = rsa::RsaPublicKey::from(&private_key);
        let jwk_part =
            |part: Vec<u8>| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(part);
        (
            private_key
                .to_pkcs8_pem(LineEnding::LF)
                .expect("private key PEM")
                .to_string(),
            jwk_part(public_key.n().to_bytes_be()),
            jwk_part(public_key.e().to_bytes_be()),
        )
    })
}

/// Serves the test key's public half as Auth Core's JWKS and points the gateway at it.
/// The returned server must outlive every delegated bearer the gateway verifies.
async fn serve_auth_core_jwks() -> MockServer {
    let (_, n, e) = test_signing_key();
    let jwks = MockServer::start().await;
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
    std::env::set_var(
        "AUTH_CORE_JWKS_URL",
        format!("{}/.well-known/jwks.json", jwks.uri()),
    );
    std::env::set_var("AUTH_CORE_ISSUER", TEST_AUTH_ISSUER);
    std::env::set_var("AUTH_CORE_AUDIENCE", "model-gateway");
    jwks
}

/// The delegated bearers one turn carries beside its ingress bearer, one per plane.
struct DelegatedBearers {
    data_plane: String,
    session: String,
    inference: String,
    execution: String,
}

/// One bearer for `audience`, for the dev bypass identity. Its retention posture is
/// the one the bypass asserts for its ingress bearer: persistent, so `zdr` is false.
fn sign_bearer(audience: &str) -> String {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time")
        .as_secs();
    let claims = serde_json::json!({
        "sub": DEV_USER,
        "iss": TEST_AUTH_ISSUER,
        "aud": audience,
        "exp": now + 300,
        "nbf": now - 5,
        "org_id": DEV_ORG,
        "user_id": DEV_USER,
        "principal_type": "user",
        "zdr": false
    });
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(TEST_AUTH_KID.to_owned());
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(test_signing_key().0.as_bytes()).expect("encoding key"),
    )
    .expect("signed JWT")
}

/// The four delegated bearers of the turn. Each plane's audience is set explicitly, so a
/// developer's environment cannot change what the gateway verifies.
fn sign_delegated_bearers() -> DelegatedBearers {
    std::env::set_var("DATA_PLANE_AUTH_AUDIENCE", "data-plane");
    std::env::set_var("SESSION_CORE_AUTH_AUDIENCE", "session-core");
    std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");
    std::env::set_var("EXECUTION_CORE_AUTH_AUDIENCE", "execution-core");
    DelegatedBearers {
        data_plane: sign_bearer("data-plane"),
        session: sign_bearer("session-core"),
        inference: sign_bearer("inference-core"),
        execution: sign_bearer("execution-core"),
    }
}

fn clear_auth_env() {
    for name in [
        "AUTH_CORE_JWKS_URL",
        "AUTH_CORE_ISSUER",
        "AUTH_CORE_AUDIENCE",
        "MODEL_GATEWAY_AUTH_DEV_BYPASS",
        "DATA_PLANE_AUTH_AUDIENCE",
        "SESSION_CORE_AUTH_AUDIENCE",
        "INFERENCE_CORE_AUTH_AUDIENCE",
        "EXECUTION_CORE_AUTH_AUDIENCE",
    ] {
        std::env::remove_var(name);
    }
}

/// A gateway whose provider follows `rounds` and whose session core accepts the turn.
/// `_auth_core` must outlive the gateway: it mints the credential that terminal
/// receipts are sent with. `_jwks` must outlive it too: it serves the key the delegated
/// bearers verify against.
struct Gateway {
    app: Router,
    bearers: DelegatedBearers,
    captured: Captured,
    analyze_calls: Arc<AtomicUsize>,
    _auth_core: MockServer,
    _jwks: MockServer,
}

async fn start_gateway(
    rounds: Vec<Vec<mpv1::ToolCall>>,
    models: Vec<mpv1::ModelInfo>,
) -> Gateway {
    // The dev bypass accepts the ingress bearer unverified, but a route that extracts
    // a delegated bearer still needs one that verifies. The turn therefore carries
    // four, signed for the identity the bypass gives its ingress bearer. An allowlist
    // from the developer's environment would make the model name in the request depend
    // on the machine.
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let jwks = serve_auth_core_jwks().await;
    let bearers = sign_delegated_bearers();
    let auth_core = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/session-core/internal-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "gateway-terminalizer-token",
            "expiresInSeconds": 300,
            "audience": "session-core",
        })))
        .mount(&auth_core)
        .await;
    let unary_calls = Arc::new(AtomicUsize::new(0));
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let analyze_calls = Arc::new(AtomicUsize::new(0));
    let provider = ScriptedProvider {
        rounds: Mutex::new(rounds.into()),
        unary_calls,
        captured: captured.clone(),
        models,
        analyze_calls: analyze_calls.clone(),
    };
    let session = spawn_session().await;
    let mut state = AppState::new();
    state.publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
    state.inference_client = spawn_provider(provider).await;
    state.session_client = SessionCoreClient::new(session.clone());
    state.managed_run_client = ManagedRunLifecycleClient::new(session);
    state
        .configure_managed_terminalization(
            &auth_core.uri(),
            "model-gateway",
            "test-model-gateway-service-credential",
        )
        .expect("the scoped terminalization credential is configured");
    Gateway {
        app: build_router(state, None),
        bearers,
        captured,
        analyze_calls,
        _auth_core: auth_core,
        _jwks: jwks,
    }
}

/// Streams one turn through `/v1/invoke/stream` and returns its frames as (event, data).
async fn stream_turn(gateway: &Gateway, body: &str) -> Vec<(String, serde_json::Value)> {
    let bearers = &gateway.bearers;
    let request = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .header(
            "x-data-plane-authorization",
            format!("Bearer {}", bearers.data_plane),
        )
        .header(
            "x-session-authorization",
            format!("Bearer {}", bearers.session),
        )
        .header(
            "x-inference-authorization",
            format!("Bearer {}", bearers.inference),
        )
        .header(
            "x-execution-authorization",
            format!("Bearer {}", bearers.execution),
        )
        .body(Body::from(body.to_owned()))
        .unwrap();
    let response = gateway.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    text.split("\n\n")
        .filter_map(|frame| {
            let event = frame
                .lines()
                .find_map(|line| line.strip_prefix("event: "))?;
            let data = frame
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .map_or(serde_json::Value::Null, |data| {
                    serde_json::from_str(data).expect("a frame's data is JSON")
                });
            Some((event.to_owned(), data))
        })
        .collect()
}

fn model(id: &str, features: &[&str]) -> mpv1::ModelInfo {
    mpv1::ModelInfo {
        id: id.to_owned(),
        features: features.iter().map(|feature| (*feature).to_owned()).collect(),
        ..Default::default()
    }
}

fn image_attachment(data: &str) -> serde_json::Value {
    serde_json::json!({"kind": "image", "data_base64": data, "mime_type": "image/png"})
}

fn body_with_images(model: &str, features: &[&str], extra: serde_json::Value) -> String {
    let mut body = serde_json::json!({
        "content": QUESTION,
        "model": model,
        "features": features,
        "attachments": [image_attachment(IMAGE_A), image_attachment(IMAGE_B)],
    });
    for (key, value) in extra.as_object().cloned().unwrap_or_default() {
        body[key] = value;
    }
    body.to_string()
}

fn count_words_round() -> Vec<mpv1::ToolCall> {
    vec![mpv1::ToolCall {
        id: "call-count".into(),
        name: "count_words".into(),
        arguments_json: r#"{"texts":["Hei pa deg"]}"#.into(),
    }]
}

/// The index of the user's own message in a recorded request.
fn question_index(request: &mpv1::InferRequest) -> usize {
    request
        .messages
        .iter()
        .position(|message| message.role == "user" && message.content.contains(QUESTION))
        .expect("the user's question reaches the model")
}

fn clear_flag() {
    std::env::remove_var("CHAT_VISION_AS_PARTS_ENABLED");
    clear_auth_env();
}

/// Balance plus two images plus a tool round: the picked model, not the vision-only
/// diversion, reads both pictures on every round, and the tools reach it.
#[tokio::test]
#[serial_test::serial]
async fn two_images_ride_every_round_at_the_user_index() {
    std::env::set_var("CHAT_VISION_AS_PARTS_ENABLED", "true");
    let gateway = start_gateway(
        vec![count_words_round()],
        vec![model("gpt-vision", &["tools", "vision"])],
    )
    .await;
    let frames = stream_turn(
        &gateway,
        &body_with_images("verevon-balance", &["tools", "usage"], serde_json::json!({})),
    )
    .await;
    clear_flag();

    assert!(
        frames.iter().any(|(event, _)| event == "done"),
        "the turn completed: {frames:?}"
    );
    assert!(
        frames
            .iter()
            .any(|(event, data)| event == "tool_call" && data["name"] == "count_words"),
        "the tool round ran: {frames:?}"
    );
    assert_eq!(
        gateway.analyze_calls.load(Ordering::SeqCst),
        0,
        "the diversion was not taken"
    );

    let captured = gateway.captured.lock().unwrap();
    let with_images: Vec<_> = captured
        .iter()
        .filter(|(_, request)| !request.image_parts.is_empty())
        .collect();
    let rounds = with_images
        .iter()
        .filter(|(rpc, request)| *rpc == "infer" && !request.tools.is_empty())
        .count();
    assert!(rounds >= 2, "every tool round carries the images: {rounds}");
    assert!(
        with_images.iter().any(|(rpc, _)| *rpc == "infer_stream"),
        "and so does the answer"
    );
    let first_index = question_index(&with_images[0].1);
    for (_, request) in &with_images {
        assert_eq!(request.image_parts.len(), 2);
        let index = question_index(request);
        assert_eq!(index, first_index, "the tool rounds did not move the message");
        for (part, bytes) in request.image_parts.iter().zip([b"hi", b"ho"]) {
            assert_eq!(part.message_index as usize, index);
            assert_eq!(part.data, bytes);
            assert_eq!(part.mime_type, "image/png");
        }
        // The prompt the model reads still has its system blocks ahead of the question.
        assert!(index > 0, "the prompt kept its leading blocks");
    }
}

/// With the switch off the turn is today's: the vision-only diversion answers and no
/// request to the model carries an image part.
#[tokio::test]
#[serial_test::serial]
async fn flag_off_keeps_vision_stream() {
    clear_flag();
    let gateway = start_gateway(vec![], vec![model("gpt-vision", &["tools", "vision"])]).await;
    let frames = stream_turn(
        &gateway,
        &body_with_images("verevon-balance", &["tools"], serde_json::json!({})),
    )
    .await;
    assert!(frames.iter().any(|(event, _)| event == "done"), "{frames:?}");
    assert_eq!(gateway.analyze_calls.load(Ordering::SeqCst), 1);
    assert!(gateway
        .captured
        .lock()
        .unwrap()
        .iter()
        .all(|(_, request)| request.image_parts.is_empty()));
}

/// With the switch on but no model that reads images, the diversion is the fallback
/// rather than a refusal.
#[tokio::test]
#[serial_test::serial]
async fn no_vision_model_falls_back_to_the_diversion() {
    std::env::set_var("CHAT_VISION_AS_PARTS_ENABLED", "true");
    let gateway = start_gateway(vec![], vec![model("deepseek-chat", &["tools"])]).await;
    let frames = stream_turn(
        &gateway,
        &body_with_images("verevon-balance", &["tools"], serde_json::json!({})),
    )
    .await;
    clear_flag();
    assert!(frames.iter().any(|(event, _)| event == "done"), "{frames:?}");
    assert_eq!(gateway.analyze_calls.load(Ordering::SeqCst), 1);
    assert!(gateway
        .captured
        .lock()
        .unwrap()
        .iter()
        .all(|(_, request)| request.image_parts.is_empty()));
}

/// A Do turn with an image is refused typed, before any model call, while the switch is on.
#[tokio::test]
#[serial_test::serial]
async fn a_do_turn_with_an_image_is_refused_typed() {
    std::env::set_var("CHAT_VISION_AS_PARTS_ENABLED", "true");
    let gateway = start_gateway(vec![], vec![model("gpt-vision", &["tools", "vision"])]).await;
    let frames = stream_turn(
        &gateway,
        &body_with_images("verevon-balance", &["agentic", "tools"], serde_json::json!({})),
    )
    .await;
    clear_flag();
    let error = frames
        .iter()
        .find(|(event, _)| event == "error")
        .unwrap_or_else(|| panic!("an error frame: {frames:?}"));
    assert_eq!(error.1["code"], "attachment_do_unsupported", "{frames:?}");
    assert_eq!(gateway.analyze_calls.load(Ordering::SeqCst), 0);
    assert!(gateway.captured.lock().unwrap().is_empty());
}
