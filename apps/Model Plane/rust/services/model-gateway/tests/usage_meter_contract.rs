//! The run-cost meter as it stands before WS27 changes it. Each test pins today's
//! behaviour, and the slice that changes that behaviour flips the test on purpose:
//!
//! - `tool_round_tokens_are_absent_from_the_final_usage_frame` flips in WS27-S2, when
//!   the tool-decision calls are metered into the turn's usage.
//! - `unknown_model_priced_from_default_row` flipped in WS27-S1, when a model the
//!   catalogue does not list stopped taking the `default` row's price; it is now
//!   `unknown_model_is_unpriced_not_default_priced`.

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
    pricing::{CostBasis, PricingCache},
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
}

#[tonic::async_trait]
impl InferenceCore for ScriptedProvider {
    type InferStreamStream = MockStream;
    type StreamVideoGenerationContentStream = MockVideoStream;

    async fn infer(
        &self,
        _: TReq<mpv1::InferRequest>,
    ) -> Result<Response<mpv1::InferResponse>, Status> {
        self.unary_calls.fetch_add(1, Ordering::SeqCst);
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
        _: TReq<mpv1::InferRequest>,
    ) -> Result<Response<MockStream>, Status> {
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
    async fn list_models(
        &self,
        _: TReq<mpv1::ListModelsRequest>,
    ) -> Result<Response<mpv1::ListModelsResponse>, Status> {
        unimplemented_rpc("list_models")
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
        unimplemented_rpc("analyze_image")
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
        Ok(Response::new(mpv1::AppendMessageResponse { sequence: 1 }))
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
    unary_calls: Arc<AtomicUsize>,
    _auth_core: MockServer,
    _jwks: MockServer,
}

async fn start_gateway(rounds: Vec<Vec<mpv1::ToolCall>>) -> Gateway {
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
    let provider = ScriptedProvider {
        rounds: Mutex::new(rounds.into()),
        unary_calls: unary_calls.clone(),
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
        unary_calls,
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

/// Today the tool-decision calls report tokens that nothing sums: the loop reads them
/// for a debug line, and the final `usage` frame states the answer's counts alone.
/// WS27-S2 makes the frame the turn's total across every provider call.
#[tokio::test]
#[serial_test::serial]
async fn tool_round_tokens_are_absent_from_the_final_usage_frame() {
    let round = vec![mpv1::ToolCall {
        id: "call-count".into(),
        name: "count_words".into(),
        arguments_json: r#"{"texts":["Hei på deg"]}"#.into(),
    }];
    let gateway = start_gateway(vec![round]).await;
    let frames = stream_turn(
        &gateway,
        r#"{"content":"Hvor mange ord er det?","model":"m","features":["tools","usage"]}"#,
    )
    .await;

    let usage: Vec<&serde_json::Value> = frames
        .iter()
        .filter(|(event, _)| event == "usage")
        .map(|(_, data)| data)
        .collect();
    assert_eq!(usage.len(), 1, "one usage frame per answer: {frames:?}");
    assert!(
        frames
            .iter()
            .any(|(event, data)| event == "tool_call" && data["name"] == "count_words"),
        "the scripted round proposed count_words: {frames:?}"
    );
    // The thread title and the follow-up suggestions come after the answer, so the
    // count is a floor: the decision round and the round that proposes nothing are two.
    let unary_calls = gateway.unary_calls.load(Ordering::SeqCst);
    assert!(
        unary_calls >= 2,
        "a decision round and the round proposing nothing ran: {unary_calls} unary calls"
    );
    assert_eq!(usage[0]["input_tokens"], ANSWER_INPUT, "{}", usage[0]);
    assert_eq!(usage[0]["output_tokens"], ANSWER_OUTPUT, "{}", usage[0]);
    clear_auth_env();
}

/// A model the catalogue does not list used to be priced at the `default` row, so a
/// confident figure reached the stream and the ledger for a model whose price is
/// unknown. Its cost is unknown instead, and a listed model is still priced.
#[tokio::test]
async fn unknown_model_is_unpriced_not_default_priced() {
    let catalogue = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/pricing"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "rates": [
                { "model": "default", "input_per_million": 3.0, "output_per_million": 15.0 },
                { "model": "gpt-4o-mini", "input_per_million": 0.15, "output_per_million": 0.6 },
            ],
        })))
        .mount(&catalogue)
        .await;
    let pricing = PricingCache::new(Some(catalogue.uri()), reqwest::Client::new());

    let unknown = pricing
        .cost_usd_for(
            CostBasis::Metered,
            "a-model-the-catalogue-does-not-list",
            1_000,
            500,
            0,
            0,
        )
        .await;
    assert_eq!(unknown, None, "the default row is not this model's price");

    let listed = pricing
        .cost_usd_for(CostBasis::Metered, "gpt-4o-mini", 1_000, 500, 0, 0)
        .await
        .expect("a listed model is priced");
    let want = 1_000.0 / 1_000_000.0 * 0.15 + 500.0 / 1_000_000.0 * 0.6;
    assert!((listed - want).abs() < 1e-12, "got {listed}, want {want}");
}
