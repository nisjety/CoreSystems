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
    SaveCheckpointRequest, SaveCheckpointResponse, SessionMessage, SpeechVoiceInfo,
    StartManagedRunRequest, StartManagedRunResponse, StartRunRequest, StartRunResponse,
    StartScheduledRunRequest, StreamVideoGenerationContentRequest,
    StreamVideoGenerationContentResponse, SynthesizeSpeechRequest, SynthesizeSpeechResponse,
    TerminalOutcome, TranscribeSpeechRequest, TranscribeSpeechResponse, TranslateTextRequest,
    TranslateTextResponse, TranslationDetection, TranslationLanguageInfo,
};
use mp_events::publisher::InMemoryPublisher;
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

const TEST_AUTH_KID: &str = "model-gateway-zdr-route-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";

#[derive(Default)]
struct MockOk {
    captured_messages: Option<CapturedMessages>,
    captured_calls: Option<CapturedCalls>,
    captured_zdr_requests: Option<CapturedZdrRequests>,
    infer_call_count: Option<Arc<AtomicUsize>>,
    /// Tool calls the next unary `infer` calls propose, one round each; once
    /// they run out, `infer` proposes none, which ends the tool phase.
    planned_rounds: Option<Arc<Mutex<std::collections::VecDeque<Vec<mpv1::ToolCall>>>>>,
    /// `infer_stream` closes after its first delta, without a final chunk.
    stream_without_final: bool,
    /// A private source review (its id carries `-source-review-`) is answered
    /// with one report call that finds no factual assertion in any segment,
    /// without taking one of the planned rounds.
    approve_source_reviews: bool,
    /// Every unary `infer`: its request id, the tool names it offered and its
    /// messages.
    captured_offers: Option<CapturedOffers>,
    /// Called with the request id of each source review answered through
    /// `approve_source_reviews`, before its approval is returned.
    on_source_review: Option<SourceReviewHook>,
    /// The `cache-control` metadata of every unary `infer`, by request id.
    captured_cache_control: Option<CapturedCacheControl>,
    /// Refuse the calls at this point with this status, as inference-core
    /// relays a ChatGPT-subscription refusal. With a refusal planned, a
    /// unary answer on the subscription route also names the subscription's
    /// model and provider, as the broker's does.
    refusal: Option<MockRefusal>,
}

/// Where [`MockOk::refusal`] refuses, and with what.
#[derive(Clone)]
struct MockRefusal {
    at: RefusalPoint,
    status: Status,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefusalPoint {
    /// Every `infer_stream`: the streamed answer.
    AnswerStream,
    /// Every unary `infer`.
    Unary,
    /// A unary `infer` that offers tools and is not a source review: a
    /// tool-decision round.
    ToolRound,
    /// A private source review.
    SourceReview,
}

type CapturedCacheControl = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// What [`MockOk::on_source_review`] runs.
type SourceReviewHook = Arc<dyn Fn(&str) + Send + Sync>;

impl MockOk {
    fn capturing(captured_messages: CapturedMessages) -> Self {
        Self {
            captured_messages: Some(captured_messages),
            captured_calls: None,
            captured_zdr_requests: None,
            infer_call_count: None,
            planned_rounds: None,
            stream_without_final: false,
            approve_source_reviews: false,
            captured_offers: None,
            on_source_review: None,
            captured_cache_control: None,
            refusal: None,
        }
    }

    fn capturing_calls(captured_calls: CapturedCalls) -> Self {
        Self {
            captured_messages: None,
            captured_calls: Some(captured_calls),
            captured_zdr_requests: None,
            infer_call_count: None,
            planned_rounds: None,
            stream_without_final: false,
            approve_source_reviews: false,
            captured_offers: None,
            on_source_review: None,
            captured_cache_control: None,
            refusal: None,
        }
    }

    fn capturing_zdr(captured_zdr_requests: CapturedZdrRequests) -> Self {
        Self {
            captured_messages: None,
            captured_calls: None,
            captured_zdr_requests: Some(captured_zdr_requests),
            infer_call_count: None,
            planned_rounds: None,
            stream_without_final: false,
            approve_source_reviews: false,
            captured_offers: None,
            on_source_review: None,
            captured_cache_control: None,
            refusal: None,
        }
    }

    fn counting(infer_call_count: Arc<AtomicUsize>) -> Self {
        Self {
            captured_messages: None,
            captured_calls: None,
            captured_zdr_requests: None,
            infer_call_count: Some(infer_call_count),
            planned_rounds: None,
            stream_without_final: false,
            approve_source_reviews: false,
            captured_offers: None,
            on_source_review: None,
            captured_cache_control: None,
            refusal: None,
        }
    }

    fn planning(rounds: Vec<Vec<mpv1::ToolCall>>) -> Self {
        Self {
            planned_rounds: Some(Arc::new(Mutex::new(rounds.into()))),
            ..Self::default()
        }
    }

    fn capture(&self, call: &'static str, request: &InferRequest) {
        if let Some(infer_call_count) = &self.infer_call_count {
            infer_call_count.fetch_add(1, Ordering::SeqCst);
        }
        let messages = || -> Vec<(String, String)> {
            request
                .messages
                .iter()
                .map(|message| (message.role.clone(), message.content.clone()))
                .collect()
        };
        if let Some(captured_messages) = &self.captured_messages {
            captured_messages.lock().unwrap().push(messages());
        }
        if let Some(captured_calls) = &self.captured_calls {
            captured_calls.lock().unwrap().push((call, messages()));
        }
        if let Some(captured_zdr_requests) = &self.captured_zdr_requests {
            captured_zdr_requests
                .lock()
                .unwrap()
                .push(("chat", request.zdr));
        }
    }
}

#[tonic::async_trait]
impl InferenceCore for MockOk {
    type InferStreamStream = MockStream;
    type StreamVideoGenerationContentStream = MockVideoContentStream;
    async fn infer(&self, request: TReq<InferRequest>) -> Result<Response<InferResponse>, Status> {
        let cache_control = request
            .metadata()
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let request = request.into_inner();
        if let Some(seen) = &self.captured_cache_control {
            seen.lock().unwrap().push((request.request_id.clone(), cache_control));
        }
        self.capture("infer", &request);
        if let Some(offers) = &self.captured_offers {
            offers.lock().unwrap().push((
                request.request_id.clone(),
                request.tools.iter().map(|tool| tool.name.clone()).collect(),
                request
                    .messages
                    .iter()
                    .map(|message| (message.role.clone(), message.content.clone()))
                    .collect(),
            ));
        }
        if let Some(refusal) = &self.refusal {
            let review = request.request_id.contains("-source-review-");
            let refused = match refusal.at {
                RefusalPoint::AnswerStream => false,
                RefusalPoint::Unary => true,
                RefusalPoint::ToolRound => !request.tools.is_empty() && !review,
                RefusalPoint::SourceReview => review,
            };
            if refused {
                return Err(refusal.status.clone());
            }
        }
        if self.approve_source_reviews && request.request_id.contains("-source-review-") {
            if let Some(hook) = &self.on_source_review {
                hook(&request.request_id);
            }
            let data: serde_json::Value =
                serde_json::from_str(&request.messages[1].content).unwrap();
            // A reviewer answers only the segments it is assigned; the rest
            // are context.
            let assigned = data["reviewIndexes"].as_array();
            let checks: Vec<_> = data["segments"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|segment| assigned.is_none_or(|assigned| assigned.contains(&segment["index"])))
                .map(|segment| {
                    serde_json::json!({"index": segment["index"], "status": "no_assertions", "evidence": [], "reason": ""})
                })
                .collect();
            let mut response = Response::new(InferResponse {
                model_used: "mock".into(),
                stop_reason: "tool_use".into(),
                tool_calls: vec![mpv1::ToolCall {
                    id: "review".into(),
                    name: "report_source_review".into(),
                    arguments_json: serde_json::json!({ "checks": checks }).to_string(),
                }],
                ..Default::default()
            });
            // A private report must prove it bypassed the response cache.
            response
                .metadata_mut()
                .insert("cache-control", "no-store".parse().unwrap());
            return Ok(response);
        }
        let tool_calls = self
            .planned_rounds
            .as_ref()
            .and_then(|rounds| rounds.lock().unwrap().pop_front())
            .unwrap_or_default();
        let (content, stop_reason) = if tool_calls.is_empty() {
            ("hello", "stop")
        } else {
            ("", "tool_use")
        };
        // A subscription round must name the pinned model and provider, or
        // the gateway rejects it as a route mismatch.
        let (model_used, provider_used) =
            if self.refusal.is_some() && !request.subscription_connection_id.is_empty() {
                (request.model.clone(), "openai-codex-subscription".to_owned())
            } else {
                ("mock".to_owned(), String::new())
            };
        let mut response = Response::new(InferResponse {
            compaction_summary: String::new(),
            request_id: String::new(),
            content: content.into(),
            model_used,
            stop_reason: stop_reason.into(),
            input_tokens: 1,
            output_tokens: 1,
            provider_used,
            residency: String::new(),
            token_confidence: None,
            tool_calls,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            provider_timing: None,
        });
        // A private repair or source-review report must prove it bypassed
        // the response cache; a scripted planned round answering either
        // (instead of `approve_source_reviews`'s own auto-approval, which
        // already sets this) needs it set here too.
        if request.request_id.contains("-artifact-repair-") || request.request_id.contains("-source-review-") {
            response
                .metadata_mut()
                .insert("cache-control", "no-store".parse().unwrap());
        }
        Ok(response)
    }
    async fn infer_stream(
        &self,
        request: TReq<InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        let request = request.into_inner();
        self.capture("infer_stream", &request);
        // A subscription review offers no tools (the broker returns its
        // report as structured output), so it is streamed like an answer.
        let review = request.request_id.contains("-source-review-");
        if let Some(refusal) = self.refusal.as_ref().filter(|refusal| match refusal.at {
            RefusalPoint::AnswerStream => !review,
            RefusalPoint::SourceReview => review,
            RefusalPoint::Unary | RefusalPoint::ToolRound => false,
        }) {
            return Err(refusal.status.clone());
        }
        let mut chunks = vec![
            Ok(InferChunk {
                compaction_summary: String::new(),
                reasoning_delta: String::new(),
                request_id: "req-stream-ok".into(),
                delta: "hel".into(),
                done: false,
                model_used: "mock".into(),
                input_tokens: 0,
                output_tokens: 0,
                stop_reason: String::new(),
                provider_used: String::new(),
                residency: String::new(),
                token_confidence: None,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                provider_timing: None,
            }),
            Ok(InferChunk {
                compaction_summary: String::new(),
                reasoning_delta: String::new(),
                request_id: "req-stream-ok".into(),
                delta: "lo".into(),
                done: true,
                model_used: "mock".into(),
                input_tokens: 3,
                output_tokens: 2,
                stop_reason: "end_turn".to_owned(),
                provider_used: String::new(),
                residency: String::new(),
                token_confidence: None,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
                provider_timing: None,
            }),
        ];
        if self.stream_without_final {
            chunks.truncate(1);
        }
        Ok(Response::new(Box::pin(futures::stream::iter(chunks))))
    }

    async fn create_embedding(
        &self,
        request: TReq<CreateEmbeddingRequest>,
    ) -> Result<Response<CreateEmbeddingResponse>, Status> {
        if let Some(captured_zdr_requests) = &self.captured_zdr_requests {
            captured_zdr_requests
                .lock()
                .unwrap()
                .push(("embedding", request.into_inner().zdr));
        }
        Ok(Response::new(CreateEmbeddingResponse {
            request_id: "embed-ok".into(),
            vector: vec![0.1, 0.2],
            model_used: "mock-embedding".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn list_models(
        &self,
        _: TReq<ListModelsRequest>,
    ) -> Result<Response<ListModelsResponse>, Status> {
        Ok(Response::new(ListModelsResponse {
            models: vec![ModelInfo {
                id: "mock-embedding".into(),
                provider: "mock".into(),
                modality: "embedding".into(),
                streaming: false,
                features: Vec::new(),
                privacy_tier: 0,
                residency: String::new(),
            }],
        }))
    }

    async fn synthesize_speech(
        &self,
        _: TReq<SynthesizeSpeechRequest>,
    ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
        Ok(Response::new(SynthesizeSpeechResponse {
            request_id: "speech-ok".into(),
            audio: b"audio".to_vec(),
            format: "mp3".into(),
            model_used: "mock-tts".into(),
            provider_used: "mock".into(),
            duration_ms: 0,
        }))
    }

    async fn transcribe_speech(
        &self,
        _: TReq<TranscribeSpeechRequest>,
    ) -> Result<Response<TranscribeSpeechResponse>, Status> {
        Ok(Response::new(TranscribeSpeechResponse {
            request_id: "stt-ok".into(),
            text: "hello".into(),
            detected_language: "en".into(),
            confidence: 1.0,
            model_used: "mock-stt".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn list_speech_voices(
        &self,
        _: TReq<ListSpeechVoicesRequest>,
    ) -> Result<Response<ListSpeechVoicesResponse>, Status> {
        Ok(Response::new(ListSpeechVoicesResponse {
            voices: vec![SpeechVoiceInfo {
                id: "alloy".into(),
                name: "Alloy".into(),
                language: "*".into(),
                gender: "Neutral".into(),
                provider: "mock".into(),
            }],
        }))
    }

    async fn translate_text(
        &self,
        _: TReq<TranslateTextRequest>,
    ) -> Result<Response<TranslateTextResponse>, Status> {
        Ok(Response::new(TranslateTextResponse {
            request_id: "translate-ok".into(),
            translated_text: "hei".into(),
            detected_language: "en".into(),
            confidence: 1.0,
            model_used: "mock-translation".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn batch_translate_text(
        &self,
        _: TReq<BatchTranslateTextRequest>,
    ) -> Result<Response<BatchTranslateTextResponse>, Status> {
        Ok(Response::new(BatchTranslateTextResponse {
            request_id: "batch-translate-ok".into(),
            translations: Vec::new(),
            model_used: "mock-translation".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn detect_text_language(
        &self,
        _: TReq<DetectTextLanguageRequest>,
    ) -> Result<Response<DetectTextLanguageResponse>, Status> {
        Ok(Response::new(DetectTextLanguageResponse {
            request_id: "detect-ok".into(),
            detections: vec![TranslationDetection {
                language: "en".into(),
                confidence: 1.0,
                is_translation_supported: true,
            }],
            model_used: "mock-translation".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn list_translation_languages(
        &self,
        _: TReq<ListTranslationLanguagesRequest>,
    ) -> Result<Response<ListTranslationLanguagesResponse>, Status> {
        Ok(Response::new(ListTranslationLanguagesResponse {
            languages: vec![TranslationLanguageInfo {
                code: "en".into(),
                name: "English".into(),
                native_name: "English".into(),
                direction: "ltr".into(),
            }],
        }))
    }

    async fn generate_image(
        &self,
        _: TReq<GenerateImageRequest>,
    ) -> Result<Response<GenerateImageResponse>, Status> {
        Ok(Response::new(GenerateImageResponse {
            request_id: "image-ok".into(),
            images: vec![GeneratedImage {
                url: String::new(),
                b64_json: "aW1hZ2U=".into(),
                revised_prompt: "mock image".into(),
            }],
            model_used: "mock-image".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn analyze_image(
        &self,
        _: TReq<AnalyzeImageRequest>,
    ) -> Result<Response<AnalyzeImageResponse>, Status> {
        Ok(Response::new(AnalyzeImageResponse {
            request_id: "vision-ok".into(),
            description: "mock description".into(),
            model_used: "mock-vision".into(),
            provider_used: "mock".into(),
            input_tokens: 1,
            output_tokens: 1,
        }))
    }

    async fn extract_image_text(
        &self,
        _: TReq<ExtractImageTextRequest>,
    ) -> Result<Response<ExtractImageTextResponse>, Status> {
        Ok(Response::new(ExtractImageTextResponse {
            request_id: "ocr-ok".into(),
            text: "mock text".into(),
            page_count: 0,
            model_used: "mock-ocr".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn analyze_document(
        &self,
        _: TReq<AnalyzeDocumentRequest>,
    ) -> Result<Response<AnalyzeDocumentResponse>, Status> {
        Ok(Response::new(AnalyzeDocumentResponse {
            request_id: "document-ok".into(),
            status: "succeeded".into(),
            content: "invoice text".into(),
            fields_json: r#"{"VendorName":"ACME"}"#.into(),
            tables_json: "[]".into(),
            paragraphs: vec!["invoice text".into()],
            raw_json: "{}".into(),
            pages_processed: 1,
            confidence: 0.9,
            model_used: "prebuilt-invoice".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn analyze_language(
        &self,
        _: TReq<AnalyzeLanguageRequest>,
    ) -> Result<Response<AnalyzeLanguageResponse>, Status> {
        Ok(Response::new(AnalyzeLanguageResponse {
            request_id: "language-ok".into(),
            operation: "sentiment".into(),
            results: vec![LanguageAnalysisResult {
                id: "1".into(),
                sentiment: "positive".into(),
                confidence_scores_json: r#"{"positive":0.9}"#.into(),
                sentences_json: "[]".into(),
                entities_json: "[]".into(),
                key_phrases: Vec::new(),
                redacted_text: String::new(),
                detected_language_name: String::new(),
                detected_language_code: String::new(),
                confidence: 0.0,
                summary: String::new(),
                raw_json: "{}".into(),
                content_safety_json: String::new(),
            }],
            model_used: "mock-language".into(),
            provider_used: "mock".into(),
        }))
    }

    async fn create_realtime_session(
        &self,
        _: TReq<CreateRealtimeSessionRequest>,
    ) -> Result<Response<CreateRealtimeSessionResponse>, Status> {
        Ok(Response::new(CreateRealtimeSessionResponse {
            request_id: "realtime-ok".into(),
            session_id: "sess_mock".into(),
            client_secret: "ek_mock".into(),
            websocket_url: "wss://example.test/v1/realtime?model=mock-realtime".into(),
            expires_at: 1234,
            model_used: "mock-realtime".into(),
            provider_used: "mock".into(),
            voice: "alloy".into(),
        }))
    }

    async fn create_video_generation_job(
        &self,
        _: TReq<CreateVideoGenerationJobRequest>,
    ) -> Result<Response<CreateVideoGenerationJobResponse>, Status> {
        Ok(Response::new(CreateVideoGenerationJobResponse {
            request_id: "video-ok".into(),
            job_id: "job_mock".into(),
            status: "queued".into(),
            model_used: "mock-sora".into(),
            provider_used: "mock".into(),
            raw_json: r#"{"id":"job_mock","status":"queued"}"#.into(),
        }))
    }

    async fn get_video_generation_job(
        &self,
        _: TReq<GetVideoGenerationJobRequest>,
    ) -> Result<Response<GetVideoGenerationJobResponse>, Status> {
        Ok(Response::new(GetVideoGenerationJobResponse {
            request_id: "video-status-ok".into(),
            job_id: "job_mock".into(),
            status: "succeeded".into(),
            generation_id: "gen_mock".into(),
            video_url: "https://example.test/video.mp4".into(),
            error: String::new(),
            model_used: "mock-sora".into(),
            provider_used: "mock".into(),
            raw_json: r#"{"id":"job_mock","status":"succeeded"}"#.into(),
        }))
    }

    async fn stream_video_generation_content(
        &self,
        _: TReq<StreamVideoGenerationContentRequest>,
    ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
        Ok(Response::new(Box::pin(futures::stream::iter(vec![
            Ok(StreamVideoGenerationContentResponse {
                request_id: "video-content-ok".into(),
                generation_id: "gen_mock".into(),
                data: b"mock-video".to_vec(),
                done: false,
                content_type: "video/mp4".into(),
                content_length: 10,
                provider_used: "mock".into(),
            }),
            Ok(StreamVideoGenerationContentResponse {
                request_id: "video-content-ok".into(),
                generation_id: "gen_mock".into(),
                data: Vec::new(),
                done: true,
                content_type: "video/mp4".into(),
                content_length: 10,
                provider_used: "mock".into(),
            }),
        ]))))
    }
}

struct MockDown;
#[tonic::async_trait]
impl InferenceCore for MockDown {
    type InferStreamStream = MockStream;
    type StreamVideoGenerationContentStream = MockVideoContentStream;
    async fn infer(&self, _: TReq<InferRequest>) -> Result<Response<InferResponse>, Status> {
        Err(Status::unavailable("down"))
    }
    async fn infer_stream(
        &self,
        _: TReq<InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn create_embedding(
        &self,
        _: TReq<CreateEmbeddingRequest>,
    ) -> Result<Response<CreateEmbeddingResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn list_models(
        &self,
        _: TReq<ListModelsRequest>,
    ) -> Result<Response<ListModelsResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn synthesize_speech(
        &self,
        _: TReq<SynthesizeSpeechRequest>,
    ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn transcribe_speech(
        &self,
        _: TReq<TranscribeSpeechRequest>,
    ) -> Result<Response<TranscribeSpeechResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn list_speech_voices(
        &self,
        _: TReq<ListSpeechVoicesRequest>,
    ) -> Result<Response<ListSpeechVoicesResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn translate_text(
        &self,
        _: TReq<TranslateTextRequest>,
    ) -> Result<Response<TranslateTextResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn batch_translate_text(
        &self,
        _: TReq<BatchTranslateTextRequest>,
    ) -> Result<Response<BatchTranslateTextResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn detect_text_language(
        &self,
        _: TReq<DetectTextLanguageRequest>,
    ) -> Result<Response<DetectTextLanguageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn list_translation_languages(
        &self,
        _: TReq<ListTranslationLanguagesRequest>,
    ) -> Result<Response<ListTranslationLanguagesResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn generate_image(
        &self,
        _: TReq<GenerateImageRequest>,
    ) -> Result<Response<GenerateImageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn analyze_image(
        &self,
        _: TReq<AnalyzeImageRequest>,
    ) -> Result<Response<AnalyzeImageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn extract_image_text(
        &self,
        _: TReq<ExtractImageTextRequest>,
    ) -> Result<Response<ExtractImageTextResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn analyze_document(
        &self,
        _: TReq<AnalyzeDocumentRequest>,
    ) -> Result<Response<AnalyzeDocumentResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn analyze_language(
        &self,
        _: TReq<AnalyzeLanguageRequest>,
    ) -> Result<Response<AnalyzeLanguageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn create_realtime_session(
        &self,
        _: TReq<CreateRealtimeSessionRequest>,
    ) -> Result<Response<CreateRealtimeSessionResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn create_video_generation_job(
        &self,
        _: TReq<CreateVideoGenerationJobRequest>,
    ) -> Result<Response<CreateVideoGenerationJobResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn get_video_generation_job(
        &self,
        _: TReq<GetVideoGenerationJobRequest>,
    ) -> Result<Response<GetVideoGenerationJobResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn stream_video_generation_content(
        &self,
        _: TReq<StreamVideoGenerationContentRequest>,
    ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
        Err(Status::unavailable("down"))
    }
}

/// Streaming RPC (`InferStream`) is unavailable, but the non-streaming `Infer`
/// works. This drives the real fallback chain wired in `8ac31cfb`: the endpoint
/// reveals `Infer`'s real content in chunks and closes with a real `done`,
/// instead of the bare empty-done stub that masked outages.
struct MockStreamDown;
#[tonic::async_trait]
impl InferenceCore for MockStreamDown {
    type InferStreamStream = MockStream;
    type StreamVideoGenerationContentStream = MockVideoContentStream;

    async fn infer(&self, _: TReq<InferRequest>) -> Result<Response<InferResponse>, Status> {
        // A real unary answer takes seconds. A short delay keeps that shape, so
        // a test client can go away BEFORE the fallback starts revealing it.
        tokio::time::sleep(Duration::from_millis(300)).await;
        Ok(Response::new(InferResponse {
            compaction_summary: String::new(),
            request_id: String::new(),
            content: "hello".into(),
            model_used: "mock".into(),
            stop_reason: "stop".into(),
            input_tokens: 1,
            output_tokens: 1,
            provider_used: String::new(),
            residency: String::new(),
            token_confidence: None,
            tool_calls: Vec::new(),
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            provider_timing: None,
        }))
    }

    async fn infer_stream(
        &self,
        _: TReq<InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        Err(Status::unavailable("stream down"))
    }

    async fn create_embedding(
        &self,
        _: TReq<CreateEmbeddingRequest>,
    ) -> Result<Response<CreateEmbeddingResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn list_models(
        &self,
        _: TReq<ListModelsRequest>,
    ) -> Result<Response<ListModelsResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn synthesize_speech(
        &self,
        _: TReq<SynthesizeSpeechRequest>,
    ) -> Result<Response<SynthesizeSpeechResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn transcribe_speech(
        &self,
        _: TReq<TranscribeSpeechRequest>,
    ) -> Result<Response<TranscribeSpeechResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn list_speech_voices(
        &self,
        _: TReq<ListSpeechVoicesRequest>,
    ) -> Result<Response<ListSpeechVoicesResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn translate_text(
        &self,
        _: TReq<TranslateTextRequest>,
    ) -> Result<Response<TranslateTextResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn batch_translate_text(
        &self,
        _: TReq<BatchTranslateTextRequest>,
    ) -> Result<Response<BatchTranslateTextResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn detect_text_language(
        &self,
        _: TReq<DetectTextLanguageRequest>,
    ) -> Result<Response<DetectTextLanguageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn list_translation_languages(
        &self,
        _: TReq<ListTranslationLanguagesRequest>,
    ) -> Result<Response<ListTranslationLanguagesResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn generate_image(
        &self,
        _: TReq<GenerateImageRequest>,
    ) -> Result<Response<GenerateImageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn analyze_image(
        &self,
        _: TReq<AnalyzeImageRequest>,
    ) -> Result<Response<AnalyzeImageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn extract_image_text(
        &self,
        _: TReq<ExtractImageTextRequest>,
    ) -> Result<Response<ExtractImageTextResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn analyze_document(
        &self,
        _: TReq<AnalyzeDocumentRequest>,
    ) -> Result<Response<AnalyzeDocumentResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn analyze_language(
        &self,
        _: TReq<AnalyzeLanguageRequest>,
    ) -> Result<Response<AnalyzeLanguageResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn create_realtime_session(
        &self,
        _: TReq<CreateRealtimeSessionRequest>,
    ) -> Result<Response<CreateRealtimeSessionResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn create_video_generation_job(
        &self,
        _: TReq<CreateVideoGenerationJobRequest>,
    ) -> Result<Response<CreateVideoGenerationJobResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn get_video_generation_job(
        &self,
        _: TReq<GetVideoGenerationJobRequest>,
    ) -> Result<Response<GetVideoGenerationJobResponse>, Status> {
        Err(Status::unavailable("down"))
    }

    async fn stream_video_generation_content(
        &self,
        _: TReq<StreamVideoGenerationContentRequest>,
    ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
        Err(Status::unavailable("down"))
    }
}

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
        Ok(Response::new(AppendMessageResponse { sequence: 1 }))
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
        let req = request.into_inner();
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
        Ok(Response::new(ListConversationResponse { messages }))
    }

    async fn list_threads(
        &self,
        _: TReq<ListThreadsRequest>,
    ) -> Result<Response<ListThreadsResponse>, Status> {
        Ok(Response::new(ListThreadsResponse { threads: vec![] }))
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

/// A mock Execution Core whose `RunAgent` answers `PermissionDenied` — one of
/// the codes `is_confirmed_agent_dispatch_rejection` treats as a CONFIRMED
/// pre-dispatch rejection. Every other method is unreachable on the agentic
/// chat path, so they stub out.
struct RejectingExecutionCore;

#[tonic::async_trait]
impl ExecutionCore for RejectingExecutionCore {
    async fn run_agent(
        &self,
        _: TReq<mpv1::RunAgentRequest>,
    ) -> Result<Response<mpv1::RunAgentResponse>, Status> {
        Err(Status::permission_denied(
            "run agent refused before it began",
        ))
    }

    async fn execute_step(
        &self,
        _: TReq<mpv1::ExecuteStepRequest>,
    ) -> Result<Response<mpv1::ExecuteStepResponse>, Status> {
        Err(Status::unimplemented("execute_step not needed in test"))
    }

    async fn resume_run(
        &self,
        _: TReq<mpv1::ResumeRunRequest>,
    ) -> Result<Response<mpv1::ResumeRunResponse>, Status> {
        Err(Status::unimplemented("resume_run not needed in test"))
    }

    async fn cancel_run(
        &self,
        _: TReq<mpv1::CancelRunRequest>,
    ) -> Result<Response<mpv1::CancelRunResponse>, Status> {
        Err(Status::unimplemented("cancel_run not needed in test"))
    }

    async fn pause_run(
        &self,
        _: TReq<mpv1::PauseRunRequest>,
    ) -> Result<Response<mpv1::PauseRunResponse>, Status> {
        Err(Status::unimplemented("pause_run not needed in test"))
    }

    async fn execute_scheduled_step(
        &self,
        _: TReq<mpv1::ExecuteScheduledStepRequest>,
    ) -> Result<Response<mpv1::ExecuteScheduledStepResponse>, Status> {
        Err(Status::unimplemented(
            "execute_scheduled_step not needed in test",
        ))
    }
}

/// A listener that completes the TCP accept and then drops the socket without
/// ever speaking gRPC. The client's connection is therefore ESTABLISHED before
/// it dies, so the resulting transport error carries no connect-phase errno and
/// `dispatch_never_reached_execution_core` cannot prove non-delivery. The
/// accept loop keeps running for the lifetime of the test so a retry cannot
/// fall back to `ECONNREFUSED` and be classified as undelivered after all.
/// An execution client pointed at a port that is guaranteed to be CLOSED.
///
/// Binding to :0 and immediately dropping the listener reserves an address the
/// OS just confirmed is free, so the connect fails with ECONNREFUSED
/// deterministically. The alternative — relying on `make_state`'s default
/// `http://localhost:9093` being unbound — is a hidden dependency on the local
/// Model Plane stack being DOWN: 9093 is execution-core's published port, so
/// with the stack up the gateway reached the real service and got a
/// deterministic refusal (`agent_dispatch_rejected`) instead of an unreachable
/// transport error. Proven 2026-08-26 by stopping the container: the test went
/// from failing to passing with no code change.
async fn unreachable_execution_client() -> ExecutionCoreClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    ExecutionCoreClient::new(
        tonic::transport::Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect_lazy(),
    )
}

async fn spawn_connection_dropping_execution_mock() -> ExecutionCoreClient<tonic::transport::Channel>
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        drop(socket);
                    });
                }
                Err(_) => break,
            }
        }
    });
    ExecutionCoreClient::new(
        Endpoint::from_shared(format!("http://{addr}"))
            .unwrap()
            .connect_lazy(),
    )
}

async fn spawn_rejecting_execution_mock() -> ExecutionCoreClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(ExecutionCoreServer::new(RejectingExecutionCore))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let ch = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    ExecutionCoreClient::new(ch)
}

async fn spawn_mock<S: InferenceCore>(svc: S) -> InferenceCoreClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(InferenceCoreServer::new(svc))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let ch = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    InferenceCoreClient::new(ch)
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

async fn signed_model_token(zdr: bool) -> (wiremock::MockServer, String) {
    let now = token_now();
    signed_claims_token(serde_json::json!({
        "sub": "user-zdr",
        "iss": "https://auth.test/model",
        "aud": "model-gateway",
        "exp": now + 300,
        "nbf": now - 5,
        "org_id": "org-zdr",
        "user_id": "user-zdr",
        "principal_type": "user",
        "zdr": zdr
    }))
    .await
}

async fn signed_service_model_token(scopes: &[&str]) -> (wiremock::MockServer, String) {
    let now = token_now();
    signed_claims_token(serde_json::json!({
        "sub": "service:model-worker",
        "iss": "https://auth.test/model",
        "aud": "model-gateway",
        "exp": now + 300,
        "nbf": now - 5,
        "org_id": "org-zdr",
        "principal_type": "service",
        "service_id": "service:model-worker",
        "reason": "invoke bounded model primitive",
        "scopes": scopes,
        "zdr": true
    }))
    .await
}

#[derive(Clone)]
struct DelegatedUserTokens {
    data_plane: String,
    session: String,
    inference: String,
    execution: String,
}

async fn signed_delegated_user_tokens(
    org_id: &str,
    user_id: &str,
) -> (wiremock::MockServer, DelegatedUserTokens) {
    let dev_bypass = std::env::var("MODEL_GATEWAY_AUTH_DEV_BYPASS").ok();
    let now = token_now();
    let claims = |audience: &str| {
        serde_json::json!({
            "sub": user_id,
            "iss": TEST_AUTH_ISSUER,
            "aud": audience,
            "exp": now + 300,
            "nbf": now - 5,
            "org_id": org_id,
            "user_id": user_id,
            "principal_type": "user",
            "zdr": true
        })
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

/// Exercise a normal authenticated chat path with every required downstream
/// bearer except Data Plane. This is intentionally not the ZDR preflight: it
/// reaches `StartManagedRun` first so the regression proves the prepared run is
/// terminalized on the known grounding credential error.
fn with_delegated_user_tokens_without_data_plane(
    app: Router,
    tokens: DelegatedUserTokens,
) -> Router {
    app.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let tokens = tokens.clone();
            async move {
                let headers = request.headers_mut();
                for (name, token) in [
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

async fn authenticated_user_router(
    app: Router,
    org_id: &str,
    user_id: &str,
) -> (Router, wiremock::MockServer) {
    let (jwks, tokens) = signed_delegated_user_tokens(org_id, user_id).await;
    (with_delegated_user_tokens(app, tokens), jwks)
}

async fn signed_delegated_service_inference_token(
    scopes: &[&str],
) -> (wiremock::MockServer, String) {
    let now = token_now();
    std::env::set_var("INFERENCE_CORE_AUTH_AUDIENCE", "inference-core");
    signed_claims_token(serde_json::json!({
        "sub": "service:model-worker",
        "iss": TEST_AUTH_ISSUER,
        "aud": "inference-core",
        "exp": now + 300,
        "nbf": now - 5,
        "org_id": "org-zdr",
        "principal_type": "service",
        "service_id": "service:model-worker",
        "reason": "invoke bounded model primitive",
        "scopes": scopes,
        "zdr": true
    }))
    .await
}

fn with_delegated_inference_token(app: Router, token: String) -> Router {
    app.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let token = token.clone();
            async move {
                request.headers_mut().insert(
                    "x-inference-authorization",
                    HeaderValue::from_str(&format!("Bearer {token}"))
                        .expect("signed JWT is valid HTTP header content"),
                );
                next.run(request).await
            }
        },
    ))
}

fn clear_model_auth_env() {
    std::env::remove_var("AUTH_CORE_JWKS_URL");
    std::env::remove_var("AUTH_CORE_ISSUER");
    std::env::remove_var("AUTH_CORE_AUDIENCE");
    std::env::remove_var("MODEL_GATEWAY_AUTH_DEV_BYPASS");
}

struct SessionMockClients {
    session: SessionCoreClient<tonic::transport::Channel>,
    lifecycle: ManagedRunLifecycleClient<tonic::transport::Channel>,
}

async fn spawn_session_mock(svc: MockSessionCore) -> SessionMockClients {
    let lifecycle = MockManagedRunLifecycle {
        handles: svc.handles.clone(),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(SessionCoreServer::new(svc))
            .add_service(ManagedRunLifecycleServer::new(lifecycle))
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
        lifecycle: ManagedRunLifecycleClient::new(ch),
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
    let mut state = AppState::new();
    state.publisher = publisher.clone();
    state.inference_client = client;
    state.managed_run_client = session_clients.lifecycle;
    state.session_client = session_clients.session;
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

async fn call_zdr_unary_routes(app: axum::Router, bearer: &str, request_zdr: bool) {
    let chat_body = serde_json::json!({
        "messages": [{"role": "user", "content": "ephemeral prompt"}],
        "model": "mock",
        "zdr": request_zdr
    });
    let chat_request = Request::builder()
        .method("POST")
        .uri("/v1/ai/chat")
        .header(AUTHORIZATION, format!("Bearer {bearer}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(chat_body.to_string()))
        .unwrap();
    let chat_response = app.clone().oneshot(chat_request).await.unwrap();
    assert_eq!(chat_response.status(), StatusCode::OK);

    let embedding_body = serde_json::json!({
        "input": "ephemeral text",
        "model": "mock-embedding",
        "zdr": request_zdr
    });
    let embedding_request = Request::builder()
        .method("POST")
        .uri("/v1/ai/embeddings")
        .header(AUTHORIZATION, format!("Bearer {bearer}"))
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(embedding_body.to_string()))
        .unwrap();
    let embedding_response = app.oneshot(embedding_request).await.unwrap();
    assert_eq!(embedding_response.status(), StatusCode::OK);
}

async fn route_status(
    app: axum::Router,
    method: &str,
    uri: &str,
    bearer: &str,
    body: &str,
    delegated_data_bearer: Option<&str>,
) -> StatusCode {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Bearer {bearer}"))
        .header(CONTENT_TYPE, "application/json");
    if let Some(delegated_data_bearer) = delegated_data_bearer {
        request = request.header(
            "x-data-plane-authorization",
            format!("Bearer {delegated_data_bearer}"),
        );
    }
    app.oneshot(request.body(Body::from(body.to_owned())).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
#[serial_test::serial]
async fn ai_unary_routes_forward_request_zdr_to_chat_and_embeddings() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    let captured = CapturedZdrRequests::default();
    let client = spawn_mock(MockOk::capturing_zdr(captured.clone())).await;
    let (mock_session, _) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _) = make_state(client, session_client).await;

    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    call_zdr_unary_routes(app, "dev", true).await;

    assert_eq!(
        *captured.lock().unwrap(),
        vec![("chat", true), ("embedding", true)]
    );
    clear_model_auth_env();
}

#[tokio::test]
#[serial_test::serial]
async fn ai_unary_routes_cannot_downgrade_signed_zdr_posture() {
    let (_jwks, token) = signed_model_token(true).await;
    let captured = CapturedZdrRequests::default();
    let client = spawn_mock(MockOk::capturing_zdr(captured.clone())).await;
    let (mock_session, _) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _) = make_state(client, session_client).await;

    let (app, _delegated_jwks) =
        authenticated_user_router(build_router(state, None), "org-zdr", "user-zdr").await;
    call_zdr_unary_routes(app, &token, false).await;

    assert_eq!(
        *captured.lock().unwrap(),
        vec![("chat", true), ("embedding", true)]
    );
    clear_model_auth_env();
}

#[tokio::test]
#[serial_test::serial]
async fn scoped_service_token_reaches_only_chat_and_embeddings_with_monotonic_zdr() {
    let (_jwks, token) = signed_service_model_token(&["models:invoke"]).await;
    let captured = CapturedZdrRequests::default();
    let client = spawn_mock(MockOk::capturing_zdr(captured.clone())).await;
    let (mock_session, _) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _) = make_state(client, session_client).await;

    let (_inference_jwks, inference_token) =
        signed_delegated_service_inference_token(&["models:invoke"]).await;
    let app = with_delegated_inference_token(build_router(state, None), inference_token);
    call_zdr_unary_routes(app, &token, false).await;

    assert_eq!(
        *captured.lock().unwrap(),
        vec![("chat", true), ("embedding", true)]
    );
    clear_model_auth_env();
}

#[tokio::test]
#[serial_test::serial]
async fn service_machine_routes_require_their_exact_scope() {
    let (_chat_jwks, chat_wrong_scope) = signed_service_model_token(&["runs:submit"]).await;
    let (_embedding_jwks, embedding_wrong_scope) =
        signed_service_model_token(&["runs:submit"]).await;
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _) = make_state(client, session_client).await;
    let app = build_router(state, None);

    assert_eq!(
        route_status(
            app.clone(),
            "POST",
            "/v1/ai/chat",
            &chat_wrong_scope,
            r#"{"messages":[{"role":"user","content":"q"}],"model":"mock"}"#,
            None,
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        route_status(
            app,
            "POST",
            "/v1/ai/embeddings",
            &embedding_wrong_scope,
            r#"{"input":"q","model":"mock-embedding"}"#,
            None,
        )
        .await,
        StatusCode::FORBIDDEN
    );
    clear_model_auth_env();
}

#[tokio::test]
#[serial_test::serial]
async fn service_token_is_denied_on_user_delegated_retrieval_and_session_routes() {
    let (_jwks, token) = signed_service_model_token(&["models:invoke"]).await;
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _) = make_state(client, session_client).await;
    let app = build_router(state, None);

    for (method, uri, body, delegated) in [
        ("POST", "/v1/invoke", r#"{"content":"q"}"#, None),
        ("GET", "/v1/threads", "", None),
        ("POST", "/v1/retrieval", r#"{"query":"q"}"#, None),
        (
            "POST",
            "/v1/chat/documents",
            r#"{"content":"q"}"#,
            Some(token.as_str()),
        ),
    ] {
        assert_eq!(
            route_status(app.clone(), method, uri, &token, body, delegated).await,
            StatusCode::FORBIDDEN,
            "service route must be denied: {method} {uri}"
        );
    }
    clear_model_auth_env();
}

#[tokio::test]
#[serial_test::serial]
async fn ai_images_routes_forward_to_inference_core() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let generate_req = Request::builder()
        .method("POST")
        .uri("/v1/ai/images")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"prompt":"draw a boat"}"#))
        .unwrap();
    let generate_resp = app.clone().oneshot(generate_req).await.unwrap();
    assert_eq!(generate_resp.status(), StatusCode::OK);
    let generate_body = to_bytes(generate_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let generate_json: serde_json::Value = serde_json::from_slice(&generate_body).unwrap();
    assert_eq!(generate_json["object"], "image.generation");
    assert_eq!(generate_json["provider_used"], "mock");

    let ocr_req = Request::builder()
        .method("POST")
        .uri("/v1/ai/images/ocr")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"content_base64":"aW1hZ2U="}"#))
        .unwrap();
    let ocr_resp = app.clone().oneshot(ocr_req).await.unwrap();
    assert_eq!(ocr_resp.status(), StatusCode::OK);
    let ocr_body = to_bytes(ocr_resp.into_body(), usize::MAX).await.unwrap();
    let ocr_json: serde_json::Value = serde_json::from_slice(&ocr_body).unwrap();
    assert_eq!(ocr_json["object"], "image.ocr");
    assert_eq!(ocr_json["text"], "mock text");

    let doc_req = Request::builder()
        .method("POST")
        .uri("/v1/ai/documents/invoices")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"url":"https://example.com/invoice.pdf"}"#))
        .unwrap();
    let doc_resp = app.clone().oneshot(doc_req).await.unwrap();
    assert_eq!(doc_resp.status(), StatusCode::OK);
    let doc_body = to_bytes(doc_resp.into_body(), usize::MAX).await.unwrap();
    let doc_json: serde_json::Value = serde_json::from_slice(&doc_body).unwrap();
    assert_eq!(doc_json["object"], "document.analysis");
    assert_eq!(doc_json["fields"]["VendorName"], "ACME");

    let language_req = Request::builder()
        .method("POST")
        .uri("/v1/ai/language/sentiment")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"text":"I like this"}"#))
        .unwrap();
    let language_resp = app.clone().oneshot(language_req).await.unwrap();
    assert_eq!(language_resp.status(), StatusCode::OK);
    let language_body = to_bytes(language_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let language_json: serde_json::Value = serde_json::from_slice(&language_body).unwrap();
    assert_eq!(language_json["object"], "language.sentiment");
    assert_eq!(language_json["results"][0]["sentiment"], "positive");

    let realtime_req = Request::builder()
        .method("POST")
        .uri("/v1/ai/realtime/session")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"model":"mock-realtime","voice":"alloy"}"#))
        .unwrap();
    let realtime_resp = app.clone().oneshot(realtime_req).await.unwrap();
    assert_eq!(realtime_resp.status(), StatusCode::OK);
    let realtime_body = to_bytes(realtime_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let realtime_json: serde_json::Value = serde_json::from_slice(&realtime_body).unwrap();
    assert_eq!(realtime_json["object"], "realtime.session");
    assert_eq!(realtime_json["client_secret"], "ek_mock");
    assert_eq!(realtime_json["provider_used"], "mock");

    let video_generate_req = Request::builder()
        .method("POST")
        .uri("/v1/ai/video/generate")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"prompt":"a fjord at sunrise"}"#))
        .unwrap();
    let video_generate_resp = app.clone().oneshot(video_generate_req).await.unwrap();
    assert_eq!(video_generate_resp.status(), StatusCode::OK);
    let video_generate_body = to_bytes(video_generate_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let video_generate_json: serde_json::Value =
        serde_json::from_slice(&video_generate_body).unwrap();
    assert_eq!(video_generate_json["object"], "video.generation.job");
    assert_eq!(video_generate_json["job_id"], "job_mock");

    let video_status_req = Request::builder()
        .method("GET")
        .uri("/v1/ai/video/jobs/job_mock")
        .header(AUTHORIZATION, "Bearer dev")
        .body(Body::empty())
        .unwrap();
    let video_status_resp = app.clone().oneshot(video_status_req).await.unwrap();
    assert_eq!(video_status_resp.status(), StatusCode::OK);
    let video_status_body = to_bytes(video_status_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let video_status_json: serde_json::Value = serde_json::from_slice(&video_status_body).unwrap();
    assert_eq!(video_status_json["status"], "succeeded");
    assert_eq!(video_status_json["generation_id"], "gen_mock");

    let video_content_req = Request::builder()
        .method("GET")
        .uri("/v1/ai/video/generations/gen_mock/content")
        .header(AUTHORIZATION, "Bearer dev")
        .body(Body::empty())
        .unwrap();
    let video_content_resp = app.oneshot(video_content_req).await.unwrap();
    assert_eq!(video_content_resp.status(), StatusCode::OK);
    assert_eq!(
        video_content_resp
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("video/mp4")
    );
    let video_content_body = to_bytes(video_content_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&video_content_body[..], b"mock-video");
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_emits_ingress_and_usage_on_success() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"content":"hi","model":"m"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let drained = publisher.drain();
    assert!(drained.iter().any(|(s, _)| s.starts_with("mp.v1.ingress.")));
    assert!(drained.iter().any(|(s, _)| s.starts_with("mp.v1.usage.")));
    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        session_handles.append_message_count.load(Ordering::SeqCst),
        2
    );
    assert_eq!(managed_start_count(&session_handles), 1);
    let captures = session_handles.append_captures.lock().unwrap();
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].0, "user");
    assert_eq!(captures[1].0, "assistant");
    drop(captures);
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Completed,
        "",
        "ordinary unary chat must close its one durable run exactly once",
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_fails_closed_before_provider_when_initial_managed_heartbeat_is_unavailable() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let inference_calls = Arc::new(AtomicUsize::new(0));
    let client = spawn_mock(MockOk::counting(Arc::clone(&inference_calls))).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    session_handles.fail_heartbeat.store(true, Ordering::SeqCst);
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let request = Request::builder()
        .method("POST")
        .uri("/v1/invoke")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"must not reach provider","model":"m"}"#,
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(
        inference_calls.load(Ordering::SeqCst),
        0,
        "an unavailable initial heartbeat must block provider dispatch"
    );
    assert_eq!(managed_start_count(&session_handles), 1);
    assert!(
        publisher.drain().is_empty(),
        "ingress is not accepted before liveness"
    );
    assert!(
        session_handles
            .terminal_outcome_captures
            .lock()
            .unwrap()
            .is_empty(),
        "Gateway must not manufacture a terminal receipt after it cannot establish ownership"
    );
    let heartbeats = session_handles.heartbeat_captures.lock().unwrap();
    assert_eq!(
        heartbeats.len(),
        1,
        "only the required initial heartbeat runs"
    );
    assert_eq!(heartbeats[0].0, "Bearer gateway-terminalizer-token");
    assert_ne!(
        heartbeats[0].0, "Bearer dev",
        "a delegated user bearer must never reach the heartbeat RPC"
    );
    clear_model_auth_env();
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_returns_502_when_inference_unavailable() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockDown).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"content":"hi","model":"m"}"#))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let drained = publisher.drain();
    assert!(drained.iter().any(|(s, _)| s.starts_with("mp.v1.ingress.")));
    assert!(!drained.iter().any(|(s, _)| s.starts_with("mp.v1.usage.")));
    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        session_handles.append_message_count.load(Ordering::SeqCst),
        1
    );
    assert_eq!(managed_start_count(&session_handles), 1);
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Failed,
        "provider_unavailable",
        "ordinary unary inference failure must close its prepared run as failed",
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_emits_stream_and_usage_on_success() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
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
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: chunk"));
    assert!(body.contains("event: done"));
    assert!(body.contains("\"delta\":\"hel\""));
    assert!(body.contains("\"done\":true"));

    let drained = publisher.drain();
    assert!(drained.iter().any(|(s, _)| s == "mp.v1.stream.opened"));
    assert!(drained.iter().any(|(s, _)| s == "mp.v1.stream.closed"));
    assert!(drained.iter().any(|(s, _)| s.starts_with("mp.v1.usage.")));
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Completed,
        "",
        "ordinary streamed chat must close its durable run after its assistant turn",
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_never_presents_or_replays_done_before_terminal_receipt() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    session_handles
        .fail_terminal_outcome
        .store(true, Ordering::SeqCst);
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
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

    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: chunk"), "{body}");
    assert!(body.contains("session_terminalization_failed"), "{body}");
    assert!(
        !body.contains("event: done"),
        "a terminal-receipt failure must never be presented as a completed stream: {body}"
    );

    let request_id = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            value
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .expect("connected SSE event must expose its request id");
    // Buffers are addressed by verified identity + request id so one tenant
    // cannot replay another's stream; this router authenticates as
    // org_placeholder/user_placeholder.
    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    let replay = stream_buffers.replay_after(&buffer_key, None).await;
    assert!(replay.found);
    assert!(
        replay.done.is_none(),
        "a reconnect must not receive done before Session Core acknowledges terminal state"
    );
    // Q15: this failure path used to return without finishing the buffer, so a
    // resume tailed the finished stream for the whole inactivity window. It is
    // now marked ended, and a client that already saw the error is told at
    // once that nothing more will come.
    assert!(
        replay.ended.is_some(),
        "the failed stream's buffer is finished: {body}"
    );
    let last_frame = replay.deltas.last().expect("frames were buffered").seq;
    assert_eq!(
        replay.deltas.last().map(|frame| frame.event.as_str()),
        Some("error"),
        "the terminal frame is the last one buffered"
    );
    let resume = Request::builder()
        .method("GET")
        .uri(format!("/v1/invoke/resume/{request_id}"))
        .header(AUTHORIZATION, "Bearer dev")
        .header("last-event-id", last_frame.to_string())
        .body(Body::empty())
        .unwrap();
    let resumed = app.oneshot(resume).await.unwrap();
    assert_eq!(resumed.status(), StatusCode::OK);
    let resumed = tokio::time::timeout(
        Duration::from_secs(10),
        to_bytes(resumed.into_body(), usize::MAX),
    )
    .await
    .expect("a resume of an ended stream must close, not tail it")
    .unwrap();
    let resumed = String::from_utf8(resumed.to_vec()).unwrap();
    assert!(!resumed.contains("event: done"), "{resumed}");

    let drained = publisher.drain();
    assert!(
        !drained
            .iter()
            .any(|(subject, _)| subject == "mp.v1.stream.closed"),
        "terminal-success telemetry must follow terminal receipt"
    );
    assert!(
        !drained
            .iter()
            .any(|(subject, _)| subject.starts_with("mp.v1.usage.")),
        "usage success telemetry must follow terminal receipt"
    );
}

/// chat-parity §3b (resume): a client disconnect must NOT cancel the run. The
/// producer used to treat any `tx.send` failure identically to a deliberate
/// cancel (`cancelled = true; break`), so a reader dropping mid-stream — a
/// closed tab, a reload — silently truncated the answer and never persisted it
/// or finished the resume buffer, exactly like the naive "detach, don't
/// cancel" design an earlier review rejected for conflating the two signals.
///
/// Dropping the response body's data stream here drops the `mpsc::Receiver`
/// backing the SSE channel, so every subsequent `tx.send` in the producer
/// really does fail — deterministically reproducing a disconnect without any
/// wall-clock race. `MockOk::default()`'s two chunks ("hel" not-done, "lo"
/// done) still have to flow through the persist + terminalize + buffer-finish
/// tail after that: this asserts they do.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_completes_and_persists_after_the_client_disconnects_mid_stream() {
    use futures::StreamExt as _;
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
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
    let mut frames = resp.into_body().into_data_stream();
    let first = frames
        .next()
        .await
        .expect("an SSE frame")
        .expect("a readable SSE frame");
    let first = String::from_utf8(first.to_vec()).unwrap();
    assert!(first.contains("event: connected"), "{first}");
    let request_id = first
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            value
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .expect("connected SSE event must expose its request id");

    // The client goes away right here — never reads the "hel"/"lo" chunks or
    // the terminal `done`. Dropping `frames` drops the response body, which
    // drops the channel's Receiver.
    drop(frames);

    // The producer runs in a spawned task independent of the response body's
    // lifetime, so wait for it to reach its terminal branch. Poll for the
    // assistant append rather than sleeping a fixed span: the turn's post-stream
    // work (grounding, confidence scoring, persistence) has grown over time and
    // a hardcoded sleep silently becomes a false failure the moment the turn
    // outgrows it — which is exactly what a flat 300 ms did. The deadline is a
    // failure bound, not an expected duration; the loop exits as soon as the
    // append lands AND the terminal receipt that follows it is recorded —
    // asserting the receipt right after the append raced the producer's next
    // RPC and failed intermittently even before the Q15 resume changes.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if session_handles.append_captures.lock().unwrap().len() >= 2
            && !session_handles
                .terminal_outcome_captures
                .lock()
                .unwrap()
                .is_empty()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the assistant answer must persist even though nobody was listening for it; \
             saw only {:?} within the deadline",
            session_handles.append_captures.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let thread_id = prepared_thread_id(&session_handles);
    let appended = session_handles.append_captures.lock().unwrap();
    assert_eq!(
        appended.as_slice(),
        [
            ("user".to_owned(), thread_id.clone()),
            ("assistant".to_owned(), thread_id),
        ],
        "the assistant answer must persist even though nobody was listening for it"
    );
    drop(appended);

    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Completed,
        "",
        "a disconnected client must still see its run terminalize Completed, not Cancelled",
    );

    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    // The terminal frame is buffered last — after the title and follow-up
    // frames, so its cursor follows theirs. Wait for it the same way as for
    // the append above instead of racing the producer's final steps.
    let replay = loop {
        let replay = stream_buffers.replay_after(&buffer_key, None).await;
        if replay.done.is_some() || std::time::Instant::now() >= deadline {
            break replay;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(replay.found, "the disconnected run must still be resumable");
    // Reconstruct the answer the way a resuming client does: each buffered
    // frame carries the SSE event name plus its `data:` payload verbatim, so the
    // text comes out of the `chunk` frames' JSON rather than a raw text field.
    // (The buffer used to store bare text, which is why a reconnect replayed the
    // answer and lost every rich event — parity doc §4.1.)
    let full: String = replay
        .deltas
        .iter()
        .filter(|frame| frame.event == "chunk")
        .filter_map(|frame| serde_json::from_str::<serde_json::Value>(&frame.data).ok())
        .filter_map(|payload| {
            payload
                .get("delta")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(
        full, "hello",
        "every delta must have buffered even though the reader was gone"
    );
    assert!(
        replay.deltas.iter().all(|frame| !frame.event.is_empty()),
        "every buffered frame must carry its event name, or a resume cannot          replay it as anything but a chunk"
    );
    assert!(
        replay.done.is_some(),
        "the buffer must be finished so a reconnect replays a real done, not a stalled spinner"
    );

    let drained = publisher.drain();
    assert!(
        drained
            .iter()
            .any(|(subject, _)| subject == "mp.v1.stream.closed"),
        "terminal-success telemetry must still fire for a disconnected-but-completed run"
    );
}

/// Q15 resume: the buffered terminal `done` must carry the cursor it was
/// actually sent with, after every frame buffered before it. `finish` used to be
/// recorded before usage, title and follow-ups took the next ids, so a client
/// that had seen any of those frames and then reconnected with its
/// `Last-Event-ID` never received `done` (the cursor filter dropped it) and
/// tailed a finished stream until the inactivity limit settled a completed
/// answer as stopped.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_done_cursor_follows_every_buffered_frame() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
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
        .body(Body::from(
            r#"{"content":"hi","model":"m","features":["usage"]}"#,
        ))
        .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: done"), "{body}");
    let request_id = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            value
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .expect("connected SSE event must expose its request id");
    let live_done_id = body
        .split("\n\n")
        .find(|frame| frame.lines().any(|line| line == "event: done"))
        .and_then(|frame| frame.lines().find_map(|line| line.strip_prefix("id: ")))
        .map(str::to_owned)
        .expect("the live done frame carries a resume cursor");

    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    let replay = stream_buffers.replay_after(&buffer_key, None).await;
    let last_frame = replay
        .deltas
        .iter()
        .map(|frame| frame.seq)
        .max()
        .expect("the completed stream buffered its frames");
    let done = replay.done.expect("the completed stream is finished");
    assert!(
        done.seq > last_frame,
        "done (seq {}) must follow every buffered frame (last seq {last_frame}); frames: {:?}",
        done.seq,
        replay
            .deltas
            .iter()
            .map(|frame| (frame.seq, frame.event.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        done.seq.to_string(),
        live_done_id,
        "the buffered terminal cursor must be the id the live done frame was sent with"
    );
    let after_last_frame = stream_buffers
        .replay_after(&buffer_key, Some(last_frame))
        .await;
    assert!(
        after_last_frame.done.is_some(),
        "a client that saw the last buffered frame must still receive done on resume"
    );
}

/// `(id, event, data)` of every named frame in an SSE body, in order.
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

/// Q14: a direct turn reports its phase timing once, as the last frame before
/// `done`, in the `usage` family. A resumed stream replays the same frame, and
/// the usage envelope carries a summary beside an unchanged `latency_ms`.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_sends_turn_timing_just_before_done_and_replays_it() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let send = |body: &'static str| {
        let app = app.clone();
        async move {
            let request = Request::builder()
                .method("POST")
                .uri("/v1/invoke/stream")
                .header(AUTHORIZATION, "Bearer dev")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap();
            let response = app.oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            String::from_utf8(body.to_vec()).unwrap()
        }
    };

    let body =
        send(r#"{"content":"Hva sier Fjordhavn-planen?","model":"m","features":["usage"]}"#).await;
    let frames = sse_frames(&body);
    let names: Vec<&str> = frames.iter().map(|(_, event, _)| event.as_str()).collect();
    let done_at = names
        .iter()
        .position(|name| *name == "done")
        .expect("the turn completes");
    assert_eq!(
        names[done_at - 1],
        "turn_timing",
        "timing is the last frame before done: {names:?}"
    );
    assert_eq!(
        names.iter().filter(|name| **name == "turn_timing").count(),
        1,
        "{names:?}"
    );
    let request_id = frames[0].2["request_id"]
        .as_str()
        .expect("connected carries the request id")
        .to_owned();
    let (timing_id, _, timing) = &frames[done_at - 1];
    assert_eq!(timing["request_id"], request_id.as_str());
    assert_eq!(timing["outcome"], "completed");
    let server_ms = timing["server_ms"].as_u64().unwrap();
    let provider_ms = timing["provider_ms"].as_u64().unwrap();
    assert!(provider_ms <= server_ms, "{timing}");
    assert_eq!(
        timing["orchestration_ms"].as_u64(),
        Some(server_ms - provider_ms)
    );
    assert!(
        timing["counts"]["provider_calls"].as_u64().unwrap() >= 1,
        "{timing}"
    );
    assert!(
        timing["marks"]["connected_ms"].is_u64() && timing["marks"]["durable_ms"].is_u64(),
        "{timing}"
    );
    assert_eq!(
        timing["tokens_reported"], false,
        "the mock reports no provider timing"
    );
    let phases: Vec<&str> = timing["phases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|span| span["phase"].as_str())
        .collect();
    for expected in [
        "admission",
        "session_prepare",
        "context",
        "answer_stream",
        "commit_lock",
        "durable_append",
        "terminal_ack",
        "publish",
    ] {
        assert!(
            phases.contains(&expected),
            "{expected} missing from {phases:?}"
        );
    }
    let text = timing.to_string();
    assert!(
        !text.contains("Fjordhavn") && !text.contains("hel"),
        "no prompt or answer text: {text}"
    );

    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    let replay = stream_buffers.replay_after(&buffer_key, None).await;
    let buffered = replay
        .deltas
        .iter()
        .find(|frame| frame.event == "turn_timing")
        .expect("the timing frame is buffered for resume");
    assert_eq!(Some(buffered.seq), *timing_id);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&buffered.data).unwrap(),
        *timing
    );
    assert_eq!(
        replay.done.expect("finished").seq,
        buffered.seq + 1,
        "done follows it directly"
    );
    let resumed = stream_buffers
        .replay_after(&buffer_key, Some(buffered.seq - 1))
        .await;
    assert!(resumed
        .deltas
        .iter()
        .any(|frame| frame.event == "turn_timing"));
    assert!(resumed.done.is_some());

    let usage = &frames
        .iter()
        .find(|(_, event, _)| event == "usage")
        .expect("usage event")
        .2;
    let Some((_, envelope)) = publisher
        .drain()
        .into_iter()
        .find(|(subject, _)| subject == "mp.v1.usage.org_placeholder")
    else {
        panic!("no usage envelope was published");
    };
    assert!(
        envelope.payload["timing"]["server_ms"].is_u64(),
        "{}",
        envelope.payload
    );
    assert!(
        envelope.payload["timing"].get("phases").is_none(),
        "the envelope has no span list"
    );
    assert_eq!(
        envelope.payload["latency_ms"], usage["latency_ms"],
        "latency_ms keeps its meaning"
    );

    // Gated exactly like `usage`: a client that did not ask sees no timing.
    let plain = send(r#"{"content":"Hva sier Fjordhavn-planen?","model":"m","features":[]}"#).await;
    assert!(plain.contains("event: done"), "{plain}");
    assert!(!plain.contains("event: turn_timing"), "{plain}");
}

/// Q14 defect 3: a failed tool phase used to leave no timing at all. The
/// timing frame now precedes the terminal error, with the failed outcome.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_sends_turn_timing_before_a_failed_tool_phase_error() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "results": [{ "url": "https://example.test/result", "title": "Result",
                "snippet": "Audited result", "source": "mock", "score": 0.99 }] }
        })))
        .mount(&quarry)
        .await;
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    session_handles
        .fail_reserve_tool_action
        .store(true, Ordering::SeqCst);
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(5),
    });
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let request = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"latest price for audited result","model":"m","thread_id":"thread-audit-timing","features":["citations","usage"],"browse_web":true}"#,
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    let frames = sse_frames(&body);
    let names: Vec<&str> = frames.iter().map(|(_, event, _)| event.as_str()).collect();
    let error_at = names
        .iter()
        .position(|name| *name == "error")
        .expect("the tool phase fails");
    assert_eq!(frames[error_at].2["code"], "audit_persistence_failed");
    assert_eq!(names[error_at - 1], "turn_timing", "{names:?}");
    assert!(!names.contains(&"done"), "{names:?}");
    let timing = &frames[error_at - 1].2;
    assert_eq!(timing["outcome"], "failed");
    let dispatch = timing["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|span| span["phase"] == "dispatch")
        .expect("the forced search dispatch is timed");
    assert_eq!(dispatch["tools"], serde_json::json!(["web_search"]));
}

/// Shared contract item 2: a turn on the `ChatGPT` subscription route is never
/// priced from API list prices. Its usage event keeps `cost_usd` at 0.0 for
/// older clients and says `cost_basis: "subscription"`, and the ledger
/// envelope carries the same basis beside the real token counts. The mock's
/// final chunk names no serving provider, as an artifact turn's synthetic
/// answer does, so the route alone has to decide.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_reports_the_cost_basis_of_its_route() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    for (body, basis, cost) in [
        (
            r#"{"content":"hi","model":"gpt-5.6-terra","provider":"openai-codex-subscription","subscription_connection_id":"conn-1","features":["usage"]}"#,
            "subscription",
            serde_json::json!(0.0),
        ),
        // No pricing catalogue in the test state: a metered cost is unknown.
        (
            r#"{"content":"hi","model":"m","features":["usage"]}"#,
            "metered",
            serde_json::Value::Null,
        ),
    ] {
        let client = spawn_mock(MockOk::default()).await;
        let (mock_session, _session_handles) = MockSessionCore::new();
        let session_client = spawn_session_mock(mock_session).await;
        let (state, publisher) = make_state(client, session_client).await;
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
            .body(Body::from(body))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{basis}");
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        let Some(data) = body
            .split("\n\n")
            .find(|frame| frame.lines().any(|line| line == "event: usage"))
            .and_then(|frame| frame.lines().find_map(|line| line.strip_prefix("data: ")))
        else {
            panic!("{basis}: no usage event in {body}");
        };
        let usage: serde_json::Value = serde_json::from_str(data).expect("usage payload is JSON");
        assert_eq!(usage["cost_basis"], basis, "{usage}");
        assert_eq!(usage["cost_usd"], cost, "{usage}");
        assert_eq!(usage["input_tokens"], 3, "the counts stay real: {usage}");

        let Some((_, envelope)) = publisher
            .drain()
            .into_iter()
            .find(|(subject, _)| subject == "mp.v1.usage.org_placeholder")
        else {
            panic!("{basis}: no usage envelope was published");
        };
        assert_eq!(envelope.payload["cost_basis"], basis);
        assert_eq!(envelope.payload["input_tokens"], 3);
    }
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_terminalizes_prepared_run_when_grounding_bearer_is_missing() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let (delegated_jwks, tokens) =
        signed_delegated_user_tokens("org_placeholder", "user_placeholder").await;
    let app = with_delegated_user_tokens_without_data_plane(build_router(state, None), tokens);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"ground this safely","model":"m","features":["rag"]}"#,
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("data_plane_auth_required"), "{body}");
    assert!(!body.contains("event: done"), "{body}");
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Failed,
        "inference_failed",
        "a known post-managed-start grounding credential error must close the direct run",
    );
    drop(delegated_jwks);
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_agentic_reuses_the_prepared_session_run() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    // Explicit, not inherited: the default points at 9093, which is
    // execution-core's published port, so this assertion silently depended
    // on the local Model Plane stack being DOWN.
    state.execution_client = unreachable_execution_client().await;
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
        .body(Body::from(
            r#"{"content":"plan this safely","model":"m","features":["agentic"]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: connected"), "{body}");
    assert!(body.contains("event: error"), "{body}");
    // `unreachable`: `execution_client` points at a port the OS just confirmed
    // is free (see `unreachable_execution_client`), so RunAgent fails in the
    // CONNECT phase with `ECONNREFUSED`. `dispatch_never_reached_execution_core` can prove from
    // that errno that the request never left the gateway, so the prepared run
    // is terminalized (nothing is running to finish it) while still being
    // reported retryable — the runner being down is a transport outage, not a
    // verdict on the request. The other two arms have their own tests:
    // `..._terminalizes_on_confirmed_dispatch_rejection` (deterministic
    // refusal, NOT retryable) and `..._leaves_an_ambiguous_dispatch_alone`
    // (connection broken after it was established, so non-delivery is
    // unprovable and the run must not be touched).
    assert!(body.contains("agent_dispatch_unreachable"), "{body}");
    assert!(
        !body.contains("agent_dispatch_unavailable"),
        "a provably undelivered dispatch must not be reported as an unknown outcome: {body}"
    );
    assert!(
        !body.contains("event: done"),
        "a failed RunAgent dispatch must not be reported as a completed agent run: {body}"
    );

    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        1,
        "agentic chat must create exactly one thread"
    );
    assert_eq!(
        managed_start_count(&session_handles),
        1,
        "agentic chat must dispatch exactly the prepared run"
    );
    assert_eq!(
        session_handles.append_message_count.load(Ordering::SeqCst),
        1,
        "agentic chat must persist its user turn exactly once; execution owns any assistant turn"
    );
    let prepared_thread_id = prepared_thread_id(&session_handles);
    assert_single_user_message_on_prepared_thread(&session_handles, &prepared_thread_id);
    // Nothing was delivered, so nothing is running: the prepared run must be
    // closed rather than stranded in `running` with no worker to finish it.
    let receipts = session_handles.terminal_outcome_captures.lock().unwrap();
    assert_eq!(
        receipts.len(),
        1,
        "a provably undelivered dispatch must close the prepared run, not strand it"
    );
    let (authorization, receipt) = &receipts[0];
    assert_eq!(
        authorization, "Bearer gateway-terminalizer-token",
        "terminalization must use the scoped workload token, not a user bearer"
    );
    assert_eq!(
        receipt.run_id,
        format!("managed-run-for-{prepared_thread_id}")
    );
    assert_eq!(receipt.outcome, TerminalOutcome::Failed as i32);
    assert_eq!(
        receipt.source,
        ManagedRunSource::GatewayAgentDispatchRejected as i32,
        "the gateway's dispatch is the producer that failed either way"
    );
    // The durable half of the split: same source and outcome as a refused
    // dispatch, distinguished only by this code, so an operator reading the
    // receipt can tell a transient runner outage (retry the run) from a request
    // the runner deliberately refused (fix the request).
    assert_eq!(
        receipt.failure_code, "dispatch_unreachable",
        "an undelivered dispatch must not be recorded as a refusal"
    );
}

/// chat-wave2 scope spec: the per-turn retrieval-source filter has no
/// equivalent in execution-core's own agentic tool set — `agentic_run_stream`
/// is a separate pipeline that is never given `retrieval_scope` and offers
/// its own built-in + MCP + client tools with no per-turn source filtering.
/// A restrictive scope combined with Plan mode must therefore be rejected
/// up front (`source_scope_conflict`), exactly like the pre-existing
/// `conversation_only` + `agentic` conflict — never silently accepted and
/// then quietly ignored by the agentic path, which would offer sources the
/// user explicitly excluded with no enforcement and no receipt.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_rejects_agentic_combined_with_a_restrictive_retrieval_scope() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.execution_client = unreachable_execution_client().await;
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
        .body(Body::from(
            r#"{"content":"plan this safely","model":"m","features":["agentic"],"retrieval_scope":"knowledge_only"}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: error"), "{body}");
    assert!(body.contains("source_scope_conflict"), "{body}");
    assert!(
        !body.contains("event: connected"),
        "the conflict must be rejected before a durable run is even prepared: {body}"
    );
    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        0,
        "a rejected source-scope conflict must never create a durable thread"
    );
}

/// The ambiguous arm: a connection that is ESTABLISHED and then broken, rather
/// than refused outright. The failure carries no connect-phase errno, so
/// non-delivery is unprovable — Execution Core may have accepted the run before
/// the link died — and the prepared run must therefore be left exactly as it
/// is. Terminalizing here would mark a possibly-live run failed.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_agentic_leaves_an_ambiguous_dispatch_alone() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.execution_client = spawn_connection_dropping_execution_mock().await;
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
        .body(Body::from(
            r#"{"content":"plan this safely","model":"m","features":["agentic"]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: connected"), "{body}");
    assert!(body.contains("agent_dispatch_unavailable"), "{body}");
    assert!(
        !body.contains("agent_dispatch_unreachable"),
        "a broken established connection does not prove the request was undelivered: {body}"
    );
    assert!(
        !body.contains("agent_dispatch_rejected"),
        "an ambiguous transport outcome is not a confirmed rejection: {body}"
    );
    assert!(
        !body.contains("event: done"),
        "an ambiguous dispatch must not be reported as a completed agent run: {body}"
    );

    assert!(
        session_handles
            .terminal_outcome_captures
            .lock()
            .unwrap()
            .is_empty(),
        "an unprovable dispatch outcome must leave the prepared run alone, not terminalize it"
    );
}

/// The `rejected` half of the dispatch contract, which
/// `invoke_stream_agentic_reuses_the_prepared_session_run` cannot reach: a
/// mock Execution Core that answers `PermissionDenied` is a CONFIRMED
/// pre-dispatch rejection (`is_confirmed_agent_dispatch_rejection`), so
/// nothing is running anywhere. Leaving the prepared run open would strand it
/// in `running` forever with no worker to finish it, so it must be
/// terminalized — with the scoped workload token, naming the producer that
/// actually failed.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_agentic_terminalizes_on_confirmed_dispatch_rejection() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.execution_client = spawn_rejecting_execution_mock().await;
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
        .body(Body::from(
            r#"{"content":"plan this safely","model":"m","features":["agentic"]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: connected"), "{body}");
    assert!(body.contains("agent_dispatch_rejected"), "{body}");
    assert!(
        !body.contains("agent_dispatch_unavailable"),
        "a confirmed rejection must not be reported as an ambiguous outage: {body}"
    );
    assert!(
        !body.contains("event: done"),
        "a rejected RunAgent dispatch must not be reported as a completed agent run: {body}"
    );

    let prepared_thread_id = prepared_thread_id(&session_handles);
    let receipts = session_handles.terminal_outcome_captures.lock().unwrap();
    assert_eq!(
        receipts.len(),
        1,
        "a confirmed pre-dispatch rejection must close the prepared run, not strand it"
    );
    let (authorization, receipt) = &receipts[0];
    assert_eq!(
        authorization, "Bearer gateway-terminalizer-token",
        "terminalization must use the scoped workload token, not a user bearer"
    );
    assert_eq!(
        receipt.run_id,
        format!("managed-run-for-{prepared_thread_id}")
    );
    assert_eq!(
        receipt.source,
        ManagedRunSource::GatewayAgentDispatchRejected as i32,
        "the receipt must name the producer that actually failed"
    );
    assert_eq!(receipt.outcome, TerminalOutcome::Failed as i32);
    assert_eq!(receipt.failure_code, "dispatch_rejected");
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_recovers_when_client_thread_id_is_not_durable_yet() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    *session_handles.append_missing_thread_once.lock().unwrap() =
        Some("thread-client-provisional".to_owned());
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
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
        .body(Body::from(
            r#"{"content":"mitt navn er ima","model":"m","thread_id":"thread-client-provisional","session_key":"thread-client-provisional"}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: connected"), "{body}");
    assert!(
        body.contains(r#""thread_id":"thread-thread-client-provisional""#),
        "{body}"
    );
    assert!(body.contains("event: done"), "{body}");

    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        1
    );
    assert_eq!(
        session_handles.append_message_count.load(Ordering::SeqCst),
        3
    );
    assert_eq!(managed_start_count(&session_handles), 1);
    assert_eq!(
        prepared_thread_id(&session_handles),
        "thread-thread-client-provisional".to_owned()
    );
    let appended = session_handles.append_captures.lock().unwrap();
    assert_eq!(
        appended.as_slice(),
        [
            (
                "user".to_owned(),
                "thread-thread-client-provisional".to_owned()
            ),
            (
                "assistant".to_owned(),
                "thread-thread-client-provisional".to_owned()
            ),
        ]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_includes_thread_history_and_persists_assistant() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let captured_messages = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk::capturing(captured_messages.clone())).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    session_handles.conversation.lock().unwrap().extend([
        (
            "user".to_owned(),
            "thread-existing".to_owned(),
            "kan du gi meg svaret på model plane og hva den er?".to_owned(),
        ),
        (
            "assistant".to_owned(),
            "thread-existing".to_owned(),
            "Model Plane owns reasoning, sessions, runs, inference, and tools.".to_owned(),
        ),
    ]);
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
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
        .body(Body::from(
            r#"{"content":"hva var det igjen?","model":"m","thread_id":"thread-existing"}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: done"));

    let captured = captured_messages.lock().unwrap();
    let messages = captured.first().expect("infer_stream should be called");
    assert!(messages.iter().any(|(role, content)| {
        role == "assistant" && content.contains("Model Plane owns reasoning")
    }));
    assert_eq!(
        messages.last(),
        Some(&("user".to_owned(), "hva var det igjen?".to_owned()))
    );
    drop(captured);

    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        0
    );
    assert_eq!(managed_start_count(&session_handles), 1);
    let appended = session_handles.append_captures.lock().unwrap();
    assert_eq!(
        appended.as_slice(),
        [
            ("user".to_owned(), "thread-existing".to_owned()),
            ("assistant".to_owned(), "thread-existing".to_owned()),
        ]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_uses_context_assembly_segments_before_inference() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let captured_messages = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk::capturing(captured_messages.clone())).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    *session_handles.context_segments.lock().unwrap() = Some(vec![
        ContextSegment {
            kind: "thread".to_owned(),
            content: "assistant: Model Plane owns reasoning, sessions, runs, inference, and tools."
                .to_owned(),
            estimated_tokens: 12,
        },
        ContextSegment {
            kind: "thread".to_owned(),
            content: "user: contact ima@example.com about context".to_owned(),
            estimated_tokens: 8,
        },
        ContextSegment {
            kind: "episodic".to_owned(),
            content: "Previous note from contact ima@example.com about context".to_owned(),
            estimated_tokens: 8,
        },
        ContextSegment {
            kind: "retrieval".to_owned(),
            content: "Data Plane retrieval says Model Plane owns inference and execution context."
                .to_owned(),
            estimated_tokens: 12,
        },
        ContextSegment {
            kind: "knowledge".to_owned(),
            content: "LLM wiki entry: Model Plane routes model runs through gateway adapters."
                .to_owned(),
            estimated_tokens: 12,
        },
        ContextSegment {
            kind: "graph".to_owned(),
            content: "GraphRAG: Model Plane -> session-core -> inference-core.".to_owned(),
            estimated_tokens: 10,
        },
        ContextSegment {
            kind: "prompt".to_owned(),
            content: "contact ima@example.com about context".to_owned(),
            estimated_tokens: 8,
        },
    ]);
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
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
        .body(Body::from(
            r#"{"content":"contact ima@example.com about context","model":"m","thread_id":"thread-assembly","features":["pii"]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: done"));

    let captured = captured_messages.lock().unwrap();
    let messages = captured.first().expect("infer_stream should be called");
    // `temporal_awareness_message()` is inserted unconditionally
    // at position 0 so the model always knows the real date, which shifts the
    // context-assembly block to index 1. Asserting the temporal message's
    // presence rather than tolerating it, so a future change that drops it fails
    // here instead of silently letting the model answer time-sensitive questions
    // from its training snapshot.
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[0].0, "system");
    assert!(messages[0].1.contains("Today's real date is"));
    assert_eq!(messages[1].0, "system");
    assert!(messages[1].1.contains("Verevon context assembly"));
    assert!(messages[1].1.contains("[thread]"));
    assert!(messages[1].1.contains("Model Plane owns reasoning"));
    assert!(messages[1].1.contains("[episodic]"));
    assert!(messages[1].1.contains("[redacted-email]"));
    assert!(messages[1].1.contains("[retrieval]"));
    assert!(messages[1].1.contains("Data Plane retrieval"));
    assert!(messages[1].1.contains("[knowledge]"));
    assert!(messages[1].1.contains("LLM wiki entry"));
    assert!(messages[1].1.contains("[graph]"));
    assert!(messages[1].1.contains("GraphRAG"));
    assert!(!messages[1].1.contains("[prompt]"));
    assert!(!messages[1].1.contains("ima@example.com"));
    // Four, not three: `RESPONSE_DISCIPLINE_NOTICE` is appended unconditionally
    // to the tail of the leading system run, so it lands after the assembly
    // block and before the user turn. Pinned rather than tolerated so a change
    // that drops it — or that reorders it ahead of the assembly block it is
    // meant to qualify — fails here.
    assert_eq!(messages[2].0, "system");
    assert!(messages[2].1.contains("Response discipline:"));
    assert_eq!(
        messages[3],
        (
            "user".to_owned(),
            "contact [redacted-email] about context".to_owned()
        )
    );
    drop(captured);

    assert_eq!(
        session_handles
            .context_assembly_count
            .load(Ordering::SeqCst),
        1
    );
    let context_requests = session_handles.context_assembly_requests.lock().unwrap();
    assert_eq!(context_requests.len(), 1);
    assert_eq!(context_requests[0].0, "thread-assembly");
    // Managed-run lifecycle: the assembly is requested for the managed run id.
    assert_eq!(context_requests[0].1, "managed-run-for-thread-assembly");
    assert!(context_requests[0].2 >= 512);
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_search_turn_keeps_prior_user_name_context() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");

    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(body_string_contains("Ima name meaning"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "results": [{
                    "url": "https://names.test/ima",
                    "title": "Ima name meaning",
                    "snippet": "Ima can be interpreted as a short personal name with meanings that vary by language and culture. In Japanese it is written with characters meaning now or the present moment, while in several West African naming traditions it is given as a second daughter's name.",
                    "source": "mock",
                    "score": 0.98
                }]
            }
        })))
        .mount(&quarry)
        .await;

    let captured_messages = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk::capturing(captured_messages.clone())).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(5),
    });
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let name_req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"mitt navn er ima","model":"m","thread_id":"thread-name-memory"}"#,
        ))
        .unwrap();
    let name_resp = app.clone().oneshot(name_req).await.unwrap();
    assert_eq!(name_resp.status(), StatusCode::OK);
    let name_body = to_bytes(name_resp.into_body(), usize::MAX).await.unwrap();
    assert!(String::from_utf8(name_body.to_vec())
        .unwrap()
        .contains("event: done"));

    let search_req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"kan du i dag finne ut hva navnet mitt betyr?","model":"m","thread_id":"thread-name-memory","features":["tools","citations"],"tools":[{"name":"web_search","description":"Search the web","parameters_json":"{\"type\":\"object\"}"}]}"#,
        ))
        .unwrap();
    let search_resp = app.oneshot(search_req).await.unwrap();
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX).await.unwrap();
    let search_body = String::from_utf8(search_body.to_vec()).unwrap();
    assert!(search_body.contains("event: citation"));
    assert!(search_body.contains("https://names.test/ima"));

    let captured = captured_messages.lock().unwrap();
    // A Search-enabled turn (`features: ["tools"]` + web_search) makes TWO model
    // calls: the forced web_search injects results, then the remaining built-in
    // agent tools (`fetch_url`, `knowledge_search`) are still advertised, so
    // The exact call count is deliberately NOT asserted. Thread-title and
    // follow-up generation are inference calls too, so the total moves whenever
    // an unrelated feature adds one — and `captured.last()` is one of those
    // rather than the answer. Find the prompt carrying the grounded tool context,
    // which is what this test is actually about.
    let search_messages = captured
        .iter()
        .find(|set| set.iter().any(|(_, c)| c.contains("Current request:")))
        .expect("an inference call should have carried the forced-search context");
    assert!(search_messages
        .iter()
        .any(|(role, content)| { role == "user" && content.contains("mitt navn er ima") }));
    assert!(search_messages.iter().any(|(role, content)| {
        role == "user" && content.contains("finne ut hva navnet mitt betyr?")
    }));
    assert!(search_messages.iter().any(|(_, content)| {
        content.contains("Current request: kan du i dag finne ut hva navnet mitt betyr?")
    }));
    assert!(search_messages.iter().any(|(_, content)| {
        content.contains("do not ask for information already present in the conversation")
    }));
    drop(captured);

    let appended = session_handles.append_captures.lock().unwrap();
    assert_eq!(
        appended.as_slice(),
        [
            ("user".to_owned(), "thread-name-memory".to_owned()),
            ("assistant".to_owned(), "thread-name-memory".to_owned()),
            ("user".to_owned(), "thread-name-memory".to_owned()),
            ("assistant".to_owned(), "thread-name-memory".to_owned()),
        ]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_browse_web_flag_forces_search_without_tool_array() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");

    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .and(body_string_contains("Claude Opus 4.8 official"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "results": [{
                    "url": "https://www.anthropic.com/claude/opus",
                    "title": "Claude Opus",
                    "snippet": "Claude Opus 4.8 is Anthropic's most capable model, positioned above Sonnet and Haiku for tasks that reward deeper reasoning. It leads the family on long-horizon agentic work, code generation and analysis benchmarks published alongside the release.",
                    "source": "mock",
                    "score": 0.99
                }]
            }
        })))
        .mount(&quarry)
        .await;

    let captured_messages = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk::capturing(captured_messages.clone())).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(5),
    });
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
        .body(Body::from(
            r#"{"content":"latest Claude Opus 4.8 official","model":"m","thread_id":"thread-browse-flag","features":["citations"],"browse_web":true}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: citation"));
    assert!(body.contains("https://www.anthropic.com/claude/opus"));

    let captured = captured_messages.lock().unwrap();
    // NOT `captured.last()`: thread-title and follow-up generation are inference
    // calls too, so the final captured prompt is one of those rather than the
    // answer's. Find the prompt that carries the tool results — that is the one
    // this test is about.
    //
    // The needle spans the provenance label rather than stopping at the arrow:
    // `append_tool_outcomes` renders `- web_search [source: ...] → ...`, so a
    // bare `"web_search →"` matches nothing and this `expect` fires even though
    // the results were carried correctly.
    let messages = captured
        .iter()
        .find(|set| set.iter().any(|(_, c)| c.contains("web_search [source:")))
        .expect("an inference call should have carried the web_search results");
    assert!(messages
        .iter()
        .any(|(_, content)| content.contains("Claude Opus 4.8")));

    let reservations = session_handles.reserve_tool_action_captures.lock().unwrap();
    assert_eq!(reservations.len(), 1);
    // The managed-run lifecycle owns this run, so the reservation carries the
    // managed id rather than the plain one.
    assert_eq!(reservations[0].run_id, "managed-run-for-thread-browse-flag");
    assert_eq!(reservations[0].tool, "web_search");
    assert!(reservations[0].action_id.starts_with("inline-"));
    let finalizations = session_handles
        .finalize_tool_action_captures
        .lock()
        .unwrap();
    assert_eq!(finalizations.len(), 1);
    assert_eq!(finalizations[0].action_id, reservations[0].action_id);
    assert_eq!(finalizations[0].outcome, "completed");
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_opens_the_response_before_the_tool_phase_finishes() {
    use futures::StreamExt as _;
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");

    // A deliberately slow tool phase. The handler used to await the whole phase
    // before returning, and axum starts the HTTP response only once it does — so
    // the client saw nothing at all until every round had finished. With a
    // 12-round budget that is a dead spinner for the length of the phase.
    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_json(serde_json::json!({
                    "data": {
                        "results": [{
                            "url": "https://example.test/slow",
                            "title": "Slow",
                            "snippet": "slow result",
                            "source": "mock",
                            "score": 0.5
                        }]
                    }
                })),
        )
        .mount(&quarry)
        .await;

    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(10),
    });
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
        .body(Body::from(
            r#"{"content":"latest slow lookup","model":"m","thread_id":"thread-early-open","features":["citations"],"browse_web":true}"#,
        ))
        .unwrap();

    let started = std::time::Instant::now();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut frames = resp.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_millis(1_500), frames.next())
        .await
        .expect("the response must open before the slow tool phase completes")
        .expect("an SSE frame")
        .expect("a readable SSE frame");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the first frame waited for the whole tool phase"
    );
    let first = String::from_utf8(first.to_vec()).unwrap();
    assert!(first.contains("event: connected"), "{first}");

    // The tool phase really was slow: without this the early frame above could
    // pass vacuously on a mock that answered instantly.
    let drained = tokio::time::timeout(Duration::from_secs(20), async {
        while frames.next().await.is_some() {}
    })
    .await;
    assert!(drained.is_ok(), "the stream must still run to completion");
    assert!(
        started.elapsed() >= Duration::from_secs(3),
        "the tool phase did not actually block, so the early frame proves nothing"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_fails_closed_when_tool_audit_intent_cannot_be_persisted() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");

    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "results": [{
                    "url": "https://example.test/result",
                    "title": "Result",
                    "snippet": "Audited result",
                    "source": "mock",
                    "score": 0.99
                }]
            }
        })))
        .mount(&quarry)
        .await;

    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    session_handles
        .fail_reserve_tool_action
        .store(true, Ordering::SeqCst);
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(5),
    });
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let request = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"latest price for audited result","model":"m","thread_id":"thread-audit-down","features":["citations"],"browse_web":true}"#,
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("audit_persistence_failed"));
    assert!(!body.contains("event: citation"));
    assert!(session_handles
        .reserve_tool_action_captures
        .lock()
        .unwrap()
        .is_empty());
    assert!(session_handles
        .finalize_tool_action_captures
        .lock()
        .unwrap()
        .is_empty());
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Failed,
        "inference_failed",
        "a known audit-precondition failure after managed start must close the prepared direct run",
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_leaves_a_durable_reservation_when_tool_audit_finalization_fails() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");

    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "results": [{
                    "url": "https://example.test/result",
                    "title": "Result",
                    "snippet": "Audited result",
                    "source": "mock",
                    "score": 0.99
                }]
            }
        })))
        .mount(&quarry)
        .await;

    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    session_handles
        .fail_finalize_tool_action
        .store(true, Ordering::SeqCst);
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(5),
    });
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let request = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"latest price for audited result","model":"m","thread_id":"thread-audit-finalize-down","features":["citations"],"browse_web":true}"#,
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("audit_persistence_failed"));
    assert!(!body.contains("event: citation"));
    assert_eq!(
        session_handles
            .reserve_tool_action_captures
            .lock()
            .unwrap()
            .len(),
        1
    );
    assert!(session_handles
        .finalize_tool_action_captures
        .lock()
        .unwrap()
        .is_empty());
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Failed,
        "inference_failed",
        "a known audit-finalization failure after managed start must close the prepared direct run",
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_generate_image_emits_attachment_and_persists() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
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
        .body(Body::from(
            r#"{"content":"lag et bilde av det","model":"gpt-4o-mini","thread_id":"thread-image","generate_image":true,"features":["artifacts"]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: artifact"));
    assert!(body.contains("event: attachment"));
    assert!(body.contains("data:image/png;base64,aW1hZ2U="));
    assert!(body.contains(
        "I generated an image artifact: generated-image.png for prompt: lag et bilde av det"
    ));
    assert!(body.contains("event: done"));

    let appended = session_handles.append_captures.lock().unwrap();
    assert_eq!(
        appended.as_slice(),
        [
            ("user".to_owned(), "thread-image".to_owned()),
            ("assistant".to_owned(), "thread-image".to_owned()),
        ]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_runs_search_image_followup_sequence_with_context() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");

    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "results": [{
                    "url": "https://verevon.test/model-plane",
                    "title": "Model Plane",
                    "snippet": "Model Plane owns reasoning, sessions, inference, tools, and cost controls. It is the plane that hosts the model gateway and execution core, brokers every provider call, and enforces the per-organisation budget and retention posture on each turn.",
                    "source": "mock",
                    "score": 0.99
                }]
            }
        })))
        .mount(&quarry)
        .await;

    let captured_calls = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk::capturing_calls(captured_calls.clone())).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(5),
    });
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let search_req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"kan du gi meg nyeste svaret på model plane og hva den er?","model":"m","thread_id":"thread-sequence","features":["tools","citations"],"tools":[{"name":"web_search","description":"Search the web","parameters_json":"{\"type\":\"object\"}"}]}"#,
        ))
        .unwrap();
    let search_resp = app.clone().oneshot(search_req).await.unwrap();
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX).await.unwrap();
    let search_body = String::from_utf8(search_body.to_vec()).unwrap();
    assert!(search_body.contains("event: citation"));
    assert!(search_body.contains("https://verevon.test/model-plane"));

    let image_req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"lag et bilde av det","model":"gpt-4o-mini","thread_id":"thread-sequence","generate_image":true,"features":["artifacts"]}"#,
        ))
        .unwrap();
    let image_resp = app.clone().oneshot(image_req).await.unwrap();
    assert_eq!(image_resp.status(), StatusCode::OK);
    let image_body = to_bytes(image_resp.into_body(), usize::MAX).await.unwrap();
    let image_body = String::from_utf8(image_body.to_vec()).unwrap();
    assert!(image_body.contains("event: attachment"));
    assert!(image_body.contains("data:image/png;base64,aW1hZ2U="));
    assert!(image_body.contains(
        "I generated an image artifact: generated-image.png for prompt: lag et bilde av det"
    ));

    *session_handles.context_segments.lock().unwrap() = Some(vec![ContextSegment {
        kind: "thread".to_owned(),
        content: "user: kan du gi meg svaret på model plane og hva den er?".to_owned(),
        estimated_tokens: 12,
    }]);

    let followup_req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"hva snakket vi om, og lagde vi et bilde?","model":"m","thread_id":"thread-sequence","features":["tools","citations"],"tools":[{"name":"web_search","description":"Search the web","parameters_json":"{\"type\":\"object\"}"}]}"#,
        ))
        .unwrap();
    let followup_resp = app.oneshot(followup_req).await.unwrap();
    assert_eq!(followup_resp.status(), StatusCode::OK);
    let followup_body = to_bytes(followup_resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let followup_body = String::from_utf8(followup_body.to_vec()).unwrap();
    assert!(followup_body.contains("event: done"));

    let captured = captured_calls.lock().unwrap();
    // Not an exact count: title and follow-up generation add inference calls, so
    // the total shifts with unrelated features. What matters is that the search
    // turn's tool results reached a prompt, and that the image turn's artifact
    // reached a later one.
    assert!(
        captured
            .iter()
            .any(|(_, set)| set.iter().any(|(_, content)| {
                content.contains("Tool results")
                    && content.contains("https://verevon.test/model-plane")
            })),
        "the forced search results should have reached an inference prompt",
    );
    // The follow-up turn carries the image reply into two prompts: the
    // tool-decision `infer`, which deliberately ends on the tool-selection
    // system message (`tool_decision_messages`), and the streamed answer. The
    // assertions below are about the prompt the answer is written from.
    let followup_messages = captured
        .iter()
        .filter(|(call, _)| *call == "infer_stream")
        .map(|(_, set)| set)
        .find(|set| {
            set.iter().any(|(role, content)| {
                role == "assistant" && content.contains("I generated an image artifact")
            })
        })
        .expect("the image artifact should have reached the follow-up turn's streamed prompt");
    assert!(followup_messages.iter().any(|(role, content)| {
        role == "assistant"
            && content
                .contains("I generated an image artifact: generated-image.png for prompt: lag et bilde av det")
    }));
    assert!(followup_messages.iter().any(|(role, content)| {
        role == "system" && content.contains("the assistant already generated an image artifact")
    }));
    // The follow-up ("lag et bilde av det") no longer forces a web search, and
    // should not: it is an image request with nothing time-sensitive in it, and
    // the Search gate now asks whether the answer could be stale rather than
    // searching on every tool-enabled turn. Under the old always-force behaviour
    // this turn ran a pointless web lookup before generating a picture.
    assert!(
        !followup_messages
            .iter()
            .any(|(_, content)| content.contains("Tool results")),
        "an image request should not drag a forced web search along with it",
    );
    assert!(followup_messages
        .iter()
        .any(|(role, content)| { role == "user" && content.contains("svaret på model plane") }));
    assert!(followup_messages.iter().any(|(role, content)| {
        role == "user" && content.contains("hva snakket vi om, og lagde vi et bilde?")
    }));
    let last = followup_messages
        .last()
        .expect("follow-up turn should infer");
    assert_eq!(last.0.as_str(), "user");
    // The current question is what the model must answer last. It used to be a
    // "Tool results" block because every tool-enabled turn forced a search;
    // with the staleness gate the prompt ends on the question itself.
    assert_eq!(last.1, "hva snakket vi om, og lagde vi et bilde?");
    drop(captured);

    let appended = session_handles.append_captures.lock().unwrap();
    assert_eq!(appended.len(), 6);
    assert!(appended
        .iter()
        .all(|(_, thread_id)| thread_id == "thread-sequence"));
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_falls_back_to_infer_when_stream_unavailable() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    // InferStream is unavailable but non-streaming Infer works: the endpoint
    // must reveal Infer's real content in chunks and close with a real `done`
    // (the 8ac31cfb fallback) — never a bare empty-done stub.
    let client = spawn_mock(MockStreamDown).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
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
    let body = String::from_utf8(body.to_vec()).unwrap();
    // Real content from the Infer fallback, streamed as chunks then a real done.
    assert!(body.contains("event: chunk"));
    assert!(body.contains("\"delta\":\"hello\""));
    assert!(body.contains("event: done"));
    assert!(body.contains("\"done\":true"));
    assert!(!body.contains("event: error"));

    let drained = publisher.drain();
    assert!(drained.iter().any(|(s, _)| s == "mp.v1.stream.opened"));
    assert!(drained.iter().any(|(s, _)| s == "mp.v1.stream.closed"));
    assert!(drained.iter().any(|(s, _)| s.starts_with("mp.v1.usage.")));
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Completed,
        "",
        "fallback success must close the same prepared direct-inference run",
    );
}

/// Q15 provider failure + disconnect: with the streaming RPC unavailable the
/// gateway reveals a unary `Infer` answer instead. A client that went away
/// there used to CANCEL the run and discard the provider's finished answer —
/// the opposite of the streaming path's detach contract — and the fallback's
/// chunks restarted at cursor 0, colliding with frames already buffered. Its
/// `usage` frame was also sent live only, so a reload lost the tokens, latency,
/// confidence and cost basis of a fallback-answered turn.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_fallback_detaches_on_disconnect_and_persists_the_answer() {
    use futures::StreamExt as _;
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockStreamDown).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
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
        .body(Body::from(
            r#"{"content":"hi","model":"m","features":["usage"]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut frames = resp.into_body().into_data_stream();
    let first = frames
        .next()
        .await
        .expect("an SSE frame")
        .expect("a readable SSE frame");
    let first = String::from_utf8(first.to_vec()).unwrap();
    assert!(first.contains("event: connected"), "{first}");
    let request_id = first
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            value
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .expect("connected SSE event must expose its request id");
    // Reload while the unary answer is still being produced.
    drop(frames);

    // Poll for the append AND the terminal receipt that follows it; either
    // alone would race the producer's next step.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if session_handles.append_captures.lock().unwrap().len() >= 2
            && !session_handles
                .terminal_outcome_captures
                .lock()
                .unwrap()
                .is_empty()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the fallback answer must persist and terminalize even though the client went away; saw {:?}",
            session_handles.append_captures.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Completed,
        "",
        "a disconnect during the fallback is a detach, not a cancel",
    );

    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    // The terminal frame is buffered after the durable terminal receipt and
    // telemetry; wait for it rather than racing the producer's last steps.
    let replay = loop {
        let replay = stream_buffers.replay_after(&buffer_key, None).await;
        if replay.done.is_some() || std::time::Instant::now() >= deadline {
            break replay;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert!(
        replay.found,
        "the detached fallback must still be resumable"
    );
    let text: String = replay
        .deltas
        .iter()
        .filter(|frame| frame.event == "chunk")
        .filter_map(|frame| serde_json::from_str::<serde_json::Value>(&frame.data).ok())
        .filter_map(|payload| {
            payload
                .get("delta")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(text, "hello");
    let seqs: Vec<u64> = replay.deltas.iter().map(|frame| frame.seq).collect();
    assert!(
        seqs.windows(2).all(|pair| pair[0] < pair[1]),
        "every buffered frame needs its own increasing cursor: {seqs:?}"
    );
    let usage = replay
        .deltas
        .iter()
        .find(|frame| frame.event == "usage")
        .expect("a reload must replay the fallback turn's usage frame");
    let usage_payload: serde_json::Value =
        serde_json::from_str(&usage.data).expect("usage frame carries JSON");
    assert_eq!(usage_payload["input_tokens"], 1, "{usage_payload}");
    assert_eq!(usage_payload["output_tokens"], 1, "{usage_payload}");
    let done = replay
        .done
        .as_ref()
        .expect("a reload must replay a real done");
    assert!(
        usage.seq < done.seq,
        "done (seq {}) must follow the buffered usage frame (seq {})",
        done.seq,
        usage.seq
    );
}

/// Q15 cancel before publication: a Stop that reaches the gateway while the
/// answer is still being produced must end the turn as `stopped` with nothing
/// persisted. On the unary fallback (streaming RPC down) the Stop used to be
/// invisible — that path read only a failed send as a cancel — so the answer
/// was persisted and completed behind a browser showing "Stoppet". The tool
/// decision itself now observes the Stop too, instead of waiting it out.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_stop_before_publication_persists_nothing() {
    use futures::StreamExt as _;
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockStreamDown).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
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
        .body(Body::from(
            r#"{"content":"hi","model":"m","features":["tools"]}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut frames = resp.into_body().into_data_stream();
    let first = frames
        .next()
        .await
        .expect("an SSE frame")
        .expect("a readable SSE frame");
    let first = String::from_utf8(first.to_vec()).unwrap();
    assert!(first.contains("event: connected"), "{first}");
    let request_id = first
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            value
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .expect("connected SSE event must expose its request id");

    let cancel = Request::builder()
        .method("POST")
        .uri(format!("/v1/invoke/{request_id}/cancel"))
        .header(AUTHORIZATION, "Bearer dev")
        .body(Body::empty())
        .unwrap();
    let cancelled = app.clone().oneshot(cancel).await.unwrap();
    assert_eq!(
        cancelled.status(),
        StatusCode::ACCEPTED,
        "the running stream accepts the Stop"
    );

    let mut rest = String::new();
    let drained = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(frame) = frames.next().await {
            rest.push_str(&String::from_utf8_lossy(
                &frame.expect("a readable SSE frame"),
            ));
        }
    })
    .await;
    assert!(drained.is_ok(), "the stopped stream must end");
    assert!(rest.contains("event: stopped"), "{rest}");
    assert!(
        !rest.contains("event: done"),
        "a stopped turn must never complete: {rest}"
    );
    assert!(
        !rest.contains("\"delta\":\"hello\""),
        "no answer text after the Stop: {rest}"
    );
    let appended = session_handles.append_captures.lock().unwrap();
    assert_eq!(
        appended
            .iter()
            .filter(|(role, _)| role == "assistant")
            .count(),
        0,
        "nothing may be published after the Stop: {appended:?}"
    );
    drop(appended);
    assert!(
        session_handles
            .terminal_outcome_captures
            .lock()
            .unwrap()
            .is_empty(),
        "a stopped run is cancelled, never terminalized Completed"
    );
}

/// Q15 durable cancel versus the stream: POST /v1/orchestration/runs/{id}/cancel
/// cancels the run in Session Core, but the stream producing it went on, saved
/// its answer onto the cancelled run and then failed to terminalize it. The
/// route now also trips the stream's own flag through
/// `CancelRegistry::cancel_run_for` (tested with the route in `http_routes`);
/// this pins that the chat stream is registered under its run id, so that
/// call stops it before anything is published.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_stops_when_its_run_is_cancelled_durably() {
    use futures::StreamExt as _;
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockStreamDown).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let cancels = state.cancels.clone();
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
        .body(Body::from(
            r#"{"content":"hi","model":"m","features":["tools"]}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut frames = resp.into_body().into_data_stream();
    let first = frames
        .next()
        .await
        .expect("an SSE frame")
        .expect("a readable SSE frame");
    let first = String::from_utf8(first.to_vec()).unwrap();
    let run_id = sse_frames(&first)
        .into_iter()
        .find_map(|(_, event, data)| {
            (event == "connected").then(|| data["run_id"].as_str().map(str::to_owned))?
        })
        .expect("connected SSE event must expose its run id");

    assert!(
        !cancels.cancel_run_for(&run_id, "org_placeholder", "user_other"),
        "another user's durable cancel never reaches this stream"
    );
    assert!(
        cancels.cancel_run_for(&run_id, "org_placeholder", "user_placeholder"),
        "the live stream is found by its run id"
    );

    let mut rest = String::new();
    let drained = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(frame) = frames.next().await {
            rest.push_str(&String::from_utf8_lossy(
                &frame.expect("a readable SSE frame"),
            ));
        }
    })
    .await;
    assert!(drained.is_ok(), "the stopped stream must end");
    assert!(rest.contains("event: stopped"), "{rest}");
    assert!(!rest.contains("event: done"), "{rest}");
    assert!(!rest.contains("session_terminalization_failed"), "{rest}");
    let appended = session_handles.append_captures.lock().unwrap();
    let saved = appended.iter().filter(|(role, _)| role == "assistant");
    assert_eq!(
        saved.count(),
        0,
        "nothing may be saved onto the cancelled run: {appended:?}"
    );
    drop(appended);
    let receipts = session_handles.terminal_outcome_captures.lock().unwrap();
    assert!(
        receipts.is_empty(),
        "a cancelled run is never terminalized Completed"
    );
}

/// Q15 save versus a finished run: Session Core can end a run while its answer
/// is produced (a durable Stop handled by another replica, or its own
/// deadline). The stream used to save the answer anyway and only then fail to
/// terminalize. It now confirms the run is live right before the save: nothing
/// is saved, the client gets a terminal error, no second outcome is attempted,
/// and the replay buffer is finished.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_does_not_save_onto_a_run_that_ended_before_the_save() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    // The heartbeat before any provider work still finds the run live.
    session_handles
        .terminal_from_heartbeat
        .store(2, Ordering::SeqCst);
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
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
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: error"), "{body}");
    assert!(body.contains("session_liveness_failed"), "{body}");
    assert!(!body.contains("event: done"), "{body}");
    assert!(
        !body.contains("session_terminalization_failed"),
        "an already-ended run is not terminalized again: {body}"
    );

    let appended = session_handles.append_captures.lock().unwrap();
    let saved = appended.iter().filter(|(role, _)| role == "assistant");
    assert_eq!(
        saved.count(),
        0,
        "nothing may be saved onto a run Session Core already ended: {appended:?}"
    );
    drop(appended);
    let receipts = session_handles.terminal_outcome_captures.lock().unwrap();
    assert!(
        receipts.is_empty(),
        "no terminal receipt is attempted for a run that is already terminal"
    );
    drop(receipts);

    let request_id = sse_frames(&body)
        .into_iter()
        .find_map(|(_, _, data)| data["request_id"].as_str().map(str::to_owned))
        .expect("the stream exposes its request id");
    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    let replay = stream_buffers.replay_after(&buffer_key, None).await;
    assert!(replay.done.is_none(), "a failed turn never replays as done");
    assert!(replay.ended.is_some(), "the replay buffer is finished");
    assert_eq!(
        replay.deltas.last().map(|frame| frame.event.as_str()),
        Some("error")
    );
}

/// Q15 terminal frames: a provider stream that closed without its final chunk
/// was the one exit that sent no terminal frame at all, so the browser saw a
/// stream stop mid-answer and a resume had nothing to settle on. It now ends
/// on an honest error, buffered, with the buffer finished.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_reports_a_provider_stream_that_ends_without_its_final_chunk() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk {
        stream_without_final: true,
        ..MockOk::default()
    })
    .await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
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
    let response = app.oneshot(req).await.unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    let frames = sse_frames(&body);
    let (_, last, error) = frames.last().expect("frames");
    assert_eq!(last, "error", "{body}");
    assert_eq!(error["code"], "inference_stream_ended_without_terminal");
    assert!(!body.contains("event: done"), "{body}");

    let request_id = error["request_id"].as_str().expect("request id").to_owned();
    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    let replay = stream_buffers.replay_after(&buffer_key, None).await;
    assert!(replay.done.is_none());
    assert!(replay.ended.is_some(), "the replay buffer is finished");
    assert_eq!(
        replay.deltas.last().map(|frame| frame.event.as_str()),
        Some("error")
    );
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Failed,
        "inference_failed",
        "a truncated provider stream closes its run as failed",
    );
}

/// Q15 tool failure beside a revised document: a failed read switched off the
/// fixed completion text for a held document answer, so the chat text about
/// the saved revision became unchecked model prose. With the search failing
/// on every attempt, the chat text must still be exactly the receipt's text.
#[tokio::test]
#[serial_test::serial]
async fn a_failed_search_keeps_the_fixed_completion_text_of_a_revised_document() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(ResponseTemplate::new(504))
        .expect(3)
        .mount(&quarry)
        .await;
    let revised = "# Plan\n\nEn kort innledning.\n\nLevering fredag.";
    let client = spawn_mock(MockOk::planning(vec![vec![
        mpv1::ToolCall {
            id: "call-search".into(),
            name: "web_search".into(),
            arguments_json: serde_json::json!({ "query": "leveringstid Oslo" }).to_string(),
        },
        mpv1::ToolCall {
            id: "call-update".into(),
            name: "update_artifact".into(),
            arguments_json: serde_json::json!({ "id": "plan", "content": revised }).to_string(),
        },
    ]]))
    .await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(10),
    });
    state.artifact_versions.seed_persisted(
        "thread-revision",
        "plan",
        "Plan",
        "# Plan\n\nLevering fredag.",
        1,
    );
    state.artifact_versions.remember_kind(
        "thread-revision",
        "plan",
        model_gateway::artifacts::ArtifactKind::Document,
    );
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
        .body(Body::from(
            r#"{"content":"Oppdater planen med en kort innledning","model":"m","thread_id":"thread-revision","features":["tools","artifacts","usage"],"browse_web":true}"#,
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: done"), "{body}");
    let frames = sse_frames(&body);

    let search = frames
        .iter()
        .find(|(_, event, data)| event == "tool_result" && data["id"] == "call-search")
        .unwrap_or_else(|| panic!("the search reports its failure: {body}"));
    assert_eq!(search.2["status"], "error", "{body}");
    let chat: String = frames
        .iter()
        .filter(|(_, event, _)| event == "chunk")
        .filter_map(|(_, _, data)| data["delta"].as_str())
        .collect();
    let receipt = frames
        .iter()
        .find(|(_, event, _)| event == "result_receipt")
        .map(|(_, _, data)| data["receipt"].clone())
        .unwrap_or_else(|| panic!("the revision is completed with its receipt: {body}"));
    assert_eq!(receipt["scope"], "artifact_revision_binding", "{receipt}");
    assert_eq!(receipt["version"], 2, "{receipt}");
    assert!(chat.contains("versjon 1 til 2"), "{chat}");
    assert_eq!(
        receipt["summaryHash"],
        model_gateway::result_validation::content_hash(&chat),
        "the chat text is exactly the receipt's text: {chat}"
    );
    let saved = session_handles.conversation.lock().unwrap();
    let answer = saved
        .iter()
        .rev()
        .find(|(role, _, _)| role == "assistant")
        .map(|(_, _, content)| content.clone())
        .expect("the revision's answer is saved");
    assert_eq!(answer, chat, "the saved text is the same checked text");
}

/// The project journey's second prompt (REGI), unchanged.
const NOTE_PROMPT: &str = "Lag en kort intern status til prosjektleder med de to viktigste risikoene og beslutningen som må tas først. Maks 100 ord. Ikke send den.";
const NOTE_ATTACHMENT: &str = "\n\n--- VEDLEGG: kildepakke.md ---\nNora eier planen.\nTeknisk avklaring må være ferdig før piloten kan bekreftes.\n--- SLUTT PÅ VEDLEGG: kildepakke.md ---";
const JOURNEY_PLAN: &str = "# Prosjektplan\n\nEt kort utkast til prosjektplan.";
const READ_INVITATION: &str = "Read the relevant prior artifact if needed before drafting";
/// The new-note guidance the decision rounds received before the prefetch,
/// which switching the prefetch off restores.
const PREVIOUS_NOTE_GUIDANCE: &str = "Create the requested short status/memo as a NEW artifact with its own distinct id and title. Preserve every existing artifact and its current version. Read the relevant prior artifact if needed before drafting; do not replace the project plan or report with the new note.";
/// How the gateway's prefetched read is introduced to the model.
const GATEWAY_READ_HEADER: &str = "Tool results for a read the gateway made before your first decision; you did not call this tool. The gateway read the sole prior artifact for you.";

/// What [`run_conversation_only_turn`] observed: the SSE frames, the audited
/// reservations and finalizations, and every unary call.
type ConversationTurn = (
    Vec<(Option<u64>, String, serde_json::Value)>,
    Vec<ReserveToolActionRequest>,
    Vec<FinalizeToolActionRequest>,
    Vec<CapturedOffer>,
);

/// One conversation-only turn in `thread-note`, whose thread already holds
/// `artifacts` (id, content). The planned rounds answer the tool decisions
/// and every source review approves. Returns the SSE frames, the audited
/// reservations and finalizations, and every unary call's (id, offered
/// tools, messages).
async fn run_conversation_only_turn(
    content: &str,
    artifacts: &[(&str, &str)],
    rounds: Vec<Vec<mpv1::ToolCall>>,
) -> ConversationTurn {
    run_conversation_only_turn_with(
        LatencyLevers::ALL_ON,
        AuthorGuidance::default(),
        content,
        artifacts,
        rounds,
    )
    .await
}

/// [`run_conversation_only_turn`] with these latency levers and this author
/// guidance on the state.
async fn run_conversation_only_turn_with(
    levers: LatencyLevers,
    author_guidance: AuthorGuidance,
    content: &str,
    artifacts: &[(&str, &str)],
    rounds: Vec<Vec<mpv1::ToolCall>>,
) -> ConversationTurn {
    run_note_thread_turn(NoteThreadTurn {
        levers,
        author_guidance,
        review_proposal_rule: ReviewProposalRule::default(),
        content,
        artifacts,
        rounds,
        queued_during_first_review: None,
        follow_ups: &[],
    })
    .await
}

/// A conversation-only turn in `thread-note`, as [`run_note_thread_turn`]
/// plays it.
struct NoteThreadTurn<'a> {
    levers: LatencyLevers,
    author_guidance: AuthorGuidance,
    /// `AppState::review_proposal_rule`.
    review_proposal_rule: ReviewProposalRule,
    /// The user's message, before [`NOTE_ATTACHMENT`] is appended.
    content: &'a str,
    /// What the thread already holds, as (id, content).
    artifacts: &'a [(&'a str, &'a str)],
    rounds: Vec<Vec<mpv1::ToolCall>>,
    /// Sent as mid-run user input while the turn's first source review runs,
    /// the way the queue route enqueues it; the tool loop delivers it at the
    /// next round boundary.
    queued_during_first_review: Option<&'a str>,
    /// Later user turns in the same thread, sent in order on the SAME gateway
    /// state after the first turn is durable, without a new attachment (the
    /// sources come from the thread history, as in the product). The frames
    /// returned are the last turn's; the offers span every turn.
    follow_ups: &'a [&'a str],
}

/// [`run_conversation_only_turn`] for any [`NoteThreadTurn`].
async fn run_note_thread_turn(turn: NoteThreadTurn<'_>) -> ConversationTurn {
    let NoteThreadTurn { levers, author_guidance, review_proposal_rule, content, artifacts, rounds, queued_during_first_review, follow_ups } = turn;
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let offers: CapturedOffers = Arc::new(Mutex::new(Vec::new()));
    // The state, and so its queue, exists only after the mock is spawned.
    let queue: Arc<std::sync::OnceLock<model_gateway::queued_input::QueuedInputRegistry>> =
        Arc::new(std::sync::OnceLock::new());
    let enqueued: Arc<Mutex<Option<model_gateway::queued_input::EnqueueOutcome>>> =
        Arc::new(Mutex::new(None));
    let on_source_review = queued_during_first_review.map(|text| {
        let (queue, enqueued, text) = (queue.clone(), enqueued.clone(), text.to_owned());
        Arc::new(move |review_id: &str| {
            let mut enqueued = enqueued.lock().unwrap();
            if enqueued.is_some() {
                return;
            }
            // `{stream ULID}-{call id}-source-review-{hash}`.
            let request_id = review_id.split('-').next().expect("a review request id");
            *enqueued = Some(queue.get().expect("the state exists").enqueue_for(
                request_id,
                "org_placeholder",
                "user_placeholder",
                &text,
            ));
        }) as SourceReviewHook
    });
    let client = spawn_mock(MockOk {
        planned_rounds: Some(Arc::new(Mutex::new(rounds.into()))),
        approve_source_reviews: true,
        captured_offers: Some(offers.clone()),
        on_source_review,
        ..MockOk::default()
    })
    .await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.latency_levers = levers;
    state.author_guidance = author_guidance;
    state.review_proposal_rule = review_proposal_rule;
    assert!(queue.set(state.queued_inputs.clone()).is_ok());
    // Without a readable PII policy the turn is redacted, which also collapses
    // the attachment markers a source-bounded turn is recognised by.
    let capability_core = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/safety"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "policies": [] })),
        )
        .mount(&capability_core)
        .await;
    state.capability_core_base_url = capability_core.uri();
    for (id, text) in artifacts {
        state
            .artifact_versions
            .seed_persisted("thread-note", id, id, text, 1);
        state.artifact_versions.remember_kind(
            "thread-note",
            id,
            model_gateway::artifacts::ArtifactKind::Document,
        );
    }
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    // The delegated bearer Capability Core's policy read needs (default audience).
    let now = token_now();
    let capability = encode_test_token(
        &serde_json::json!({
            "sub": "user_placeholder", "iss": TEST_AUTH_ISSUER, "aud": "capability-core",
            "exp": now + 300, "nbf": now - 5, "org_id": "org_placeholder",
            "user_id": "user_placeholder", "principal_type": "user", "zdr": true
        }),
        &zdr_route_test_keypair().0,
    );
    let app = app.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let capability = capability.clone();
            async move {
                request.headers_mut().insert(
                    "x-capability-authorization",
                    HeaderValue::from_str(&format!("Bearer {capability}")).unwrap(),
                );
                next.run(request).await
            }
        },
    ));
    let first = format!("{content}{NOTE_ATTACHMENT}");
    let mut body = String::new();
    for content in std::iter::once(first.as_str()).chain(follow_ups.iter().copied()) {
        let req = Request::builder()
            .method("POST")
            .uri("/v1/invoke/stream")
            .header(AUTHORIZATION, "Bearer dev")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({
                    "content": content,
                    "model": "m",
                    "thread_id": "thread-note",
                    "features": ["tools", "artifacts", "usage", "conversation_only"],
                })
                .to_string(),
            ))
            .unwrap();
        let response = app.clone().oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(body.contains("event: done"), "{body}");
    }
    let reserved = session_handles
        .reserve_tool_action_captures
        .lock()
        .unwrap()
        .clone();
    let finalized = session_handles
        .finalize_tool_action_captures
        .lock()
        .unwrap()
        .clone();
    let offers = offers.lock().unwrap().clone();
    if queued_during_first_review.is_some() {
        let enqueued = enqueued.lock().unwrap().clone();
        assert!(
            matches!(enqueued, Some(model_gateway::queued_input::EnqueueOutcome::Queued { .. })),
            "the correction was queued during the review: {enqueued:?}"
        );
    }
    (sse_frames(&body), reserved, finalized, offers)
}

fn create_note_round() -> Vec<mpv1::ToolCall> {
    vec![mpv1::ToolCall {
        id: "call-note".into(),
        name: "create_artifact".into(),
        arguments_json: serde_json::json!({
            "id": "status", "kind": "document", "title": "Status", "content": "# Status\n\nKort status.",
        })
        .to_string(),
    }]
}

/// Tool-decision rounds only: not source reviews, titles or other calls.
fn decision_rounds(offers: &[CapturedOffer]) -> Vec<&CapturedOffer> {
    offers
        .iter()
        .filter(|(_, tools, _)| tools.iter().any(|tool| tool == "create_artifact"))
        .collect()
}

/// The project journey's note turn, whose thread holds only the plan. The
/// gateway reads the plan once, through the audited read, before the first
/// model round. That round receives the read's result as the model's own read
/// would add it, under a header saying the gateway made the read; it is
/// offered only `create_artifact`, and is no longer invited to read.
#[tokio::test]
#[serial_test::serial]
async fn a_new_note_reads_its_sole_plan_once_before_the_first_model_round() {
    let (frames, reserved, finalized, offers) = run_conversation_only_turn(
        NOTE_PROMPT,
        &[("plan", JOURNEY_PLAN)],
        vec![create_note_round()],
    )
    .await;
    assert_eq!(
        reserved
            .iter()
            .map(|action| action.tool.as_str())
            .collect::<Vec<_>>(),
        ["read_artifact", "create_artifact"],
        "one audited read, then the note"
    );
    assert_eq!(finalized.len(), 2);
    assert_eq!(finalized[0].action_id, reserved[0].action_id);
    assert!(finalized.iter().all(|action| action.outcome == "completed"));

    let rounds = decision_rounds(&offers);
    assert_eq!(rounds.len(), 1, "no model round is spent on the read");
    let (_, tools, messages) = rounds[0];
    assert_eq!(
        tools,
        &["create_artifact"],
        "round 1 goes straight to the note"
    );
    let read = format!("Current content of artifact 'plan' (v1):\n\n{JOURNEY_PLAN}");
    let results: Vec<_> = messages
        .iter()
        .filter(|(_, content)| content.contains(&read))
        .collect();
    assert_eq!(results.len(), 1, "{messages:?}");
    assert_eq!(results[0].0, "user");
    assert!(
        results[0].1.starts_with(GATEWAY_READ_HEADER)
            && results[0].1.contains("- read_artifact [source: "),
        "the read appears as a read_artifact tool result the gateway made: {}",
        results[0].1
    );
    assert!(!results[0].1.contains("your previous request"));
    assert!(!messages
        .iter()
        .any(|(_, content)| content.contains(READ_INVITATION)));

    let position = |event: &str, name: &str| {
        frames
            .iter()
            .position(|(_, kind, data)| kind == event && data["name"] == name)
    };
    let read_call = position("tool_call", "read_artifact").expect("the read is shown as a step");
    assert!(read_call < position("tool_call", "create_artifact").expect("the note is written"));
    let read_id = frames[read_call].2["id"].clone();
    assert!(frames.iter().any(|(_, kind, data)| kind == "tool_result"
        && data["id"] == read_id
        && data["status"] == "ok"));
    assert!(frames
        .iter()
        .any(|(_, kind, data)| kind == "artifact" && data["id"] == "status"));
}

/// Other turns keep today's behaviour: a note among several artifacts, and a
/// question in a thread whose only artifact is the plan, get no prefetch, and
/// their first round still offers the model's own read.
#[tokio::test]
#[serial_test::serial]
async fn turns_without_a_sole_note_source_keep_the_model_decided_read() {
    let (_, reserved, _, offers) = run_conversation_only_turn(
        NOTE_PROMPT,
        &[
            ("plan", JOURNEY_PLAN),
            ("budsjett", "# Budsjett\n\nIkke oppgitt."),
        ],
        vec![create_note_round()],
    )
    .await;
    assert_eq!(
        reserved
            .iter()
            .map(|action| action.tool.as_str())
            .collect::<Vec<_>>(),
        ["create_artifact"]
    );
    let rounds = decision_rounds(&offers);
    assert_eq!(rounds.len(), 1);
    let (_, tools, messages) = rounds[0];
    assert!(
        tools.iter().any(|tool| tool == "read_artifact"),
        "{tools:?}"
    );
    assert!(messages
        .iter()
        .any(|(_, content)| content.contains(READ_INVITATION)));
    assert!(!messages
        .iter()
        .any(|(_, content)| content.contains("Current content of artifact")));

    let (_, reserved, _, offers) =
        run_conversation_only_turn("Hvem eier planen?", &[("plan", JOURNEY_PLAN)], Vec::new())
            .await;
    assert!(reserved.is_empty(), "no read is audited for a question");
    let first = offers
        .iter()
        .find(|(_, tools, _)| tools.iter().any(|tool| tool == "read_artifact"))
        .expect("the question's first round offers the read");
    assert!(!first
        .2
        .iter()
        .any(|(_, content)| content.contains("Current content of artifact")));
}

/// `MODEL_GATEWAY_SOLE_NOTE_PREFETCH` off: the journey's note turn goes back
/// to the model-decided read. The first round is offered the read and told to
/// read as before; the model's read comes back as the results of its previous
/// request; only then is the round offered `create_artifact` alone. The first
/// round's messages are exactly the prefetching turn's first round without
/// the gateway's read and with the previous guidance.
#[tokio::test]
#[serial_test::serial]
async fn with_the_prefetch_off_the_model_reads_its_sole_plan_as_before() {
    let read_plan_round = vec![mpv1::ToolCall {
        id: "call-read".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "plan" }).to_string(),
    }];
    let off = LatencyLevers {
        sole_note_prefetch: false,
        ..LatencyLevers::ALL_ON
    };
    let (frames, reserved, finalized, offers) = run_conversation_only_turn_with(
        off,
        AuthorGuidance::default(),
        NOTE_PROMPT,
        &[("plan", JOURNEY_PLAN)],
        vec![read_plan_round, create_note_round()],
    )
    .await;
    assert_eq!(
        reserved
            .iter()
            .map(|action| action.tool.as_str())
            .collect::<Vec<_>>(),
        ["read_artifact", "create_artifact"],
        "the model's read, then the note"
    );
    assert!(finalized.iter().all(|action| action.outcome == "completed"));

    let rounds = decision_rounds(&offers);
    assert_eq!(rounds.len(), 2, "a model round is spent on the read");
    let (_, first_tools, first) = rounds[0];
    assert!(
        first_tools.iter().any(|tool| tool == "read_artifact"),
        "{first_tools:?}"
    );
    assert!(first
        .iter()
        .any(|(role, content)| role == "system" && content == PREVIOUS_NOTE_GUIDANCE));
    assert!(!first
        .iter()
        .any(|(_, content)| content.contains("Current content of artifact")));

    let (_, second_tools, second) = rounds[1];
    assert_eq!(
        second_tools,
        &["create_artifact"],
        "the sole read still narrows the offer"
    );
    let read = format!("Current content of artifact 'plan' (v1):\n\n{JOURNEY_PLAN}");
    let results: Vec<_> = second
        .iter()
        .filter(|(_, content)| content.contains(&read))
        .collect();
    assert_eq!(results.len(), 1, "{second:?}");
    assert!(
        results[0]
            .1
            .starts_with("Tool results for your previous request"),
        "{}",
        results[0].1
    );
    assert!(!offers
        .iter()
        .flat_map(|(_, _, messages)| messages)
        .any(|(_, content)| content.contains(GATEWAY_READ_HEADER)));
    assert!(frames
        .iter()
        .any(|(_, kind, data)| kind == "tool_call" && data["name"] == "read_artifact"));

    // The same turn with the prefetch on. Its first round carries the
    // gateway's read and the guidance without the read invitation; take
    // those two away and it is the off turn's first round, message for
    // message.
    let (_, _, _, on_offers) = run_conversation_only_turn(
        NOTE_PROMPT,
        &[("plan", JOURNEY_PLAN)],
        vec![create_note_round()],
    )
    .await;
    let on_rounds = decision_rounds(&on_offers);
    let mut restored = on_rounds[0].2.clone();
    let gateway_read = restored
        .iter()
        .position(|(_, content)| content.starts_with(GATEWAY_READ_HEADER))
        .expect("the prefetching turn carries the gateway's read");
    restored.remove(gateway_read);
    let guidance = restored
        .iter_mut()
        .find(|(role, content)| {
            role == "system" && content.starts_with("Create the requested short status/memo")
        })
        .expect("the new-note guidance");
    assert_ne!(guidance.1, PREVIOUS_NOTE_GUIDANCE);
    PREVIOUS_NOTE_GUIDANCE.clone_into(&mut guidance.1);
    assert_eq!(&restored, first);
}

/// A plan-draft request of the kind any project sends; not a recorded prompt.
const PLAN_PROMPT: &str = "Gjør møtenotatet om til en prosjektplan for lagerflyttingen. Lag oppgaver med ansvarlig rolle, avhengigheter og ferdigkriterium. Lever planen som et utkast; ikke send varsler.";

fn create_plan_round() -> Vec<mpv1::ToolCall> {
    vec![mpv1::ToolCall {
        id: "call-plan".into(),
        name: "create_artifact".into(),
        arguments_json: serde_json::json!({
            "id": "plan", "kind": "document", "title": "Prosjektplan", "content": "# Prosjektplan\n\nEt kort utkast.",
        })
        .to_string(),
    }]
}

/// `AppState::author_guidance` selects the plan and no-word-count
/// instructions a plan draft's first decision round is sent, and changes
/// nothing else in that round. The source review is sent neither version.
#[tokio::test]
#[serial_test::serial]
async fn the_author_guidance_on_the_state_selects_what_the_plan_author_is_sent() {
    let mut first_rounds = Vec::new();
    for guidance in AuthorGuidance::ALL {
        let (_, reserved, _, offers) = run_conversation_only_turn_with(
            LatencyLevers::ALL_ON,
            guidance,
            PLAN_PROMPT,
            &[],
            vec![create_plan_round()],
        )
        .await;
        assert_eq!(
            reserved
                .iter()
                .map(|action| action.tool.as_str())
                .collect::<Vec<_>>(),
            ["create_artifact"],
            "{guidance:?}"
        );
        let rounds = decision_rounds(&offers);
        let (_, _, first) = rounds.first().expect("a decision round");
        for version in AuthorGuidance::ALL {
            for text in [version.project_plan_draft(), version.no_word_count()] {
                let sent = first
                    .iter()
                    .filter(|(role, content)| role == "system" && content == text)
                    .count();
                assert_eq!(
                    sent,
                    usize::from(version == guidance),
                    "{guidance:?} round, {version:?} text: {text}"
                );
            }
        }
        let reviews: Vec<_> = offers
            .iter()
            .filter(|(id, _, _)| id.contains("-source-review-"))
            .collect();
        assert!(!reviews.is_empty(), "the plan is source-reviewed");
        for (_, _, review) in reviews {
            for version in AuthorGuidance::ALL {
                assert!(!review.iter().any(|(_, content)| {
                    content.contains(version.project_plan_draft())
                        || content.contains(version.no_word_count())
                }));
            }
        }
        first_rounds.push(first.clone());
    }
    let [v1, v2] = <[_; 2]>::try_from(first_rounds).expect("one round per version");
    let restored: Vec<(String, String)> = v2
        .into_iter()
        .map(|(role, content)| {
            let content = [
                AuthorGuidance::V2.project_plan_draft(),
                AuthorGuidance::V2.no_word_count(),
            ]
            .iter()
            .zip([
                AuthorGuidance::V1.project_plan_draft(),
                AuthorGuidance::V1.no_word_count(),
            ])
            .find(|(new, _)| **new == content)
            .map_or(content, |(_, old)| old.to_owned());
            (role, content)
        })
        .collect();
    assert_eq!(restored, v1);
}

/// A plan draft whose conflict bullet asserts an approval no source states and
/// ends with a v2-style labelled proposal.
const PLAN_WITH_QUALIFIED_DEFECT: &str =
    "# Prosjektplan\n\n- Konflikt: flyttingen 22. september er vedtatt. Forslag: avklar datoen.";

/// `AppState::author_guidance` also sets how far a qualifier word exempts a
/// line from the local guards of the turn's source check. Under v1 the
/// bullet's trailing "Forslag" exempts the whole line, so the unsupported
/// approval goes to the semantic review. Under v2 the local guard rejects it
/// first, and the segment is repaired before any review sees it.
#[tokio::test]
#[serial_test::serial]
async fn the_author_guidance_on_the_state_sets_the_local_guards_qualifier_scope() {
    let create = vec![mpv1::ToolCall {
        id: "call-plan".into(),
        name: "create_artifact".into(),
        arguments_json: serde_json::json!({
            "id": "plan", "kind": "document", "title": "Prosjektplan", "content": PLAN_WITH_QUALIFIED_DEFECT,
        })
        .to_string(),
    }];
    let repair = vec![mpv1::ToolCall {
        id: "repair".into(),
        name: "report_segment_repairs".into(),
        arguments_json: serde_json::json!({ "repairs": [{
            "index": 1, "text": "- Konflikt: flyttingen 22. september er et forslag. Forslag: avklar datoen.",
        }] })
        .to_string(),
    }];
    for guidance in AuthorGuidance::ALL {
        let mut rounds = vec![create.clone()];
        if guidance == AuthorGuidance::V2 {
            rounds.push(repair.clone());
        }
        let (_, reserved, _, offers) = run_conversation_only_turn_with(
            LatencyLevers::ALL_ON,
            guidance,
            PLAN_PROMPT,
            &[],
            rounds,
        )
        .await;
        assert_eq!(
            reserved
                .iter()
                .map(|action| action.tool.as_str())
                .collect::<Vec<_>>(),
            ["create_artifact"],
            "{guidance:?}"
        );
        let repairs = offers
            .iter()
            .filter(|(id, _, _)| id.contains("-artifact-repair-"))
            .count();
        let defect_reviewed = offers
            .iter()
            .filter(|(id, _, _)| id.contains("-source-review-"))
            .any(|(_, _, messages)| {
                messages
                    .iter()
                    .any(|(_, content)| content.contains("22. september er vedtatt"))
            });
        let reviewed = offers
            .iter()
            .any(|(id, _, _)| id.contains("-source-review-"));
        assert!(reviewed, "{guidance:?}: the plan is source-reviewed");
        match guidance {
            AuthorGuidance::V1 => {
                assert_eq!(repairs, 0, "v1 exempts the line");
                assert!(defect_reviewed, "v1 leaves the defect to the review");
            }
            AuthorGuidance::V2 => {
                assert_eq!(repairs, 1, "v2 rejects the defect locally");
                assert!(!defect_reviewed, "v2 repairs it before any review");
            }
        }
        // Neither the review nor the repair receives any version's author text.
        for (id, _, messages) in offers
            .iter()
            .filter(|(id, _, _)| id.contains("-source-review-") || id.contains("-artifact-repair-"))
        {
            for version in AuthorGuidance::ALL {
                assert!(
                    !messages.iter().any(|(_, content)| {
                        content.contains(version.project_plan_draft())
                            || content.contains(version.no_word_count())
                    }),
                    "{guidance:?}: {id} carries {version:?} author guidance"
                );
            }
        }
    }
}

/// `AppState::review_proposal_rule` reaches every source review of the turn
/// and nothing else. On, each `-source-review-` offer carries the rule once,
/// in its system instruction; no author round and no `-artifact-repair-` offer
/// carries it in either state, and off no call does. The checked plan
/// completes either way, and its receipt keeps the checker id and names the
/// rule only when on.
#[tokio::test]
#[serial_test::serial]
async fn the_review_proposal_rule_on_the_state_reaches_every_review_and_nothing_else() {
    let create = vec![mpv1::ToolCall {
        id: "call-plan".into(),
        name: "create_artifact".into(),
        arguments_json: serde_json::json!({
            "id": "plan", "kind": "document", "title": "Prosjektplan", "content": PLAN_WITH_QUALIFIED_DEFECT,
        })
        .to_string(),
    }];
    // v2's local guard rejects the defect, so the author repair runs too.
    let repair = vec![mpv1::ToolCall {
        id: "repair".into(),
        name: "report_segment_repairs".into(),
        arguments_json: serde_json::json!({ "repairs": [{
            "index": 1, "text": "- Konflikt: flyttingen 22. september er et forslag. Forslag: avklar datoen.",
        }] })
        .to_string(),
    }];
    let carried = |messages: &[(String, String)]| -> usize {
        messages
            .iter()
            .map(|(_, content)| content.matches(PROPOSAL_RULE).count())
            .sum()
    };
    for rule in ReviewProposalRule::ALL {
        let (frames, reserved, _, offers) = run_note_thread_turn(NoteThreadTurn {
            levers: LatencyLevers::ALL_ON,
            author_guidance: AuthorGuidance::V2,
            review_proposal_rule: rule,
            content: PLAN_PROMPT,
            artifacts: &[],
            rounds: vec![create.clone(), repair.clone()],
            queued_during_first_review: None,
            follow_ups: &[],
        })
        .await;
        assert_eq!(
            reserved
                .iter()
                .map(|action| action.tool.as_str())
                .collect::<Vec<_>>(),
            ["create_artifact"],
            "{rule:?}: the checked plan completes"
        );
        let reviews: Vec<_> = offers
            .iter()
            .filter(|(id, _, _)| id.contains("-source-review-"))
            .collect();
        assert!(!reviews.is_empty(), "{rule:?}: the plan is source-reviewed");
        for (id, _, messages) in &reviews {
            assert_eq!(carried(messages), usize::from(rule == ReviewProposalRule::On), "{rule:?}: {id}");
            assert_eq!(messages[0].0, "system");
            assert_eq!(
                messages[0].1.matches(PROPOSAL_RULE).count(),
                usize::from(rule == ReviewProposalRule::On),
                "{rule:?}: {id} carries it in its instruction"
            );
        }
        let repairs = offers
            .iter()
            .filter(|(id, _, _)| id.contains("-artifact-repair-"))
            .count();
        assert_eq!(repairs, 1, "{rule:?}: the author repair runs");
        assert!(!decision_rounds(&offers).is_empty(), "{rule:?}: the author has rounds");
        for (id, _, messages) in offers
            .iter()
            .filter(|(id, _, _)| !id.contains("-source-review-"))
        {
            assert_eq!(carried(messages), 0, "{rule:?}: {id} is no source review");
        }
        let source_reviews: Vec<serde_json::Value> = frames
            .iter()
            .filter(|(_, event, _)| event == "result_receipt")
            .flat_map(|(_, _, data)| data["receipt"]["artifactChecks"].as_array().cloned().unwrap_or_default())
            .map(|check| check["sourceReview"].clone())
            .collect();
        assert!(!source_reviews.is_empty(), "{rule:?}: the receipt carries the source review");
        for review in source_reviews {
            assert_eq!(review["checker"], model_gateway::source_validation::CHECKER, "{rule:?}");
            match rule {
                ReviewProposalRule::Off => assert!(review.get("reviewRule").is_none(), "{review}"),
                ReviewProposalRule::On => assert_eq!(review["reviewRule"], PROPOSAL_RULE_ID, "{review}"),
            }
        }
    }
}

/// A report request of the kind any business sends, with a small CSV of its
/// own; not a recorded prompt or source.
const CSV_REPORT_PROMPT: &str = "Lag en kort rapport om endringen fra uke 1 til uke 2 basert på vedlagt CSV.\n\n--- VEDLEGG: tall.csv ---\nweek,revenue,cost\n1,100.00,60.00\n2,120.00,66.00\n--- SLUTT PÅ VEDLEGG: tall.csv ---";

fn create_report_round() -> Vec<mpv1::ToolCall> {
    vec![mpv1::ToolCall {
        id: "call-report".into(),
        name: "create_artifact".into(),
        arguments_json: serde_json::json!({
            "id": "report", "kind": "document", "title": "Rapport", "content": "# Rapport\n\nEt kort utkast.",
        })
        .to_string(),
    }]
}

/// `AppState::author_guidance` selects the preamble in front of the local CSV
/// evidence the author is sent: the selected version's preamble, a newline
/// and the evidence, and no other version's. The source review is sent
/// neither preamble.
#[tokio::test]
#[serial_test::serial]
async fn the_author_guidance_on_the_state_selects_the_csv_preamble() {
    for guidance in AuthorGuidance::ALL {
        let (_, reserved, _, offers) = run_conversation_only_turn_with(
            LatencyLevers::ALL_ON,
            guidance,
            CSV_REPORT_PROMPT,
            &[],
            vec![create_report_round()],
        )
        .await;
        assert_eq!(
            reserved
                .iter()
                .map(|action| action.tool.as_str())
                .collect::<Vec<_>>(),
            ["create_artifact"],
            "{guidance:?}"
        );
        let rounds = decision_rounds(&offers);
        let (_, _, first) = rounds.first().expect("a decision round");
        let evidence: Vec<(AuthorGuidance, &str)> = first
            .iter()
            .filter(|(role, _)| role == "system")
            .flat_map(|(_, content)| {
                AuthorGuidance::ALL.into_iter().filter_map(move |version| {
                    content
                        .strip_prefix(version.computed_csv_preamble())
                        .and_then(|rest| rest.strip_prefix('\n'))
                        .map(|json| (version, json))
                })
            })
            .collect();
        assert_eq!(evidence.len(), 1, "{guidance:?}: {evidence:?}");
        let (version, json) = evidence[0];
        assert_eq!(version, guidance);
        let json: serde_json::Value = serde_json::from_str(json).expect("the evidence is JSON");
        assert_eq!(json[0]["checker"], "csv-revenue-cost-v2");
        let reviews: Vec<_> = offers
            .iter()
            .filter(|(id, _, _)| id.contains("-source-review-"))
            .collect();
        assert!(!reviews.is_empty(), "the report is source-reviewed");
        for (_, _, review) in reviews {
            for version in AuthorGuidance::ALL {
                assert!(!review
                    .iter()
                    .any(|(_, content)| content.contains(version.computed_csv_preamble())));
            }
        }
    }
}

/// A correction the user sends while the plan is being checked arrives at
/// the next round boundary, after the plan's write was accepted. That round
/// is offered the update, and nothing sent after the correction forbids it:
/// v2's single-write rule yields to a newer user message, and v1 has none.
#[tokio::test]
#[serial_test::serial]
async fn a_correction_queued_during_the_check_may_still_update_the_accepted_draft() {
    const CORRECTION: &str = "Legg til en egen kolonne for risiko i planen.";
    const RULE: &str = "do not create or update that same deliverable again";
    for guidance in AuthorGuidance::ALL {
        let (_, reserved, _, offers) = run_note_thread_turn(NoteThreadTurn {
            levers: LatencyLevers::ALL_ON,
            author_guidance: guidance,
            review_proposal_rule: ReviewProposalRule::default(),
            content: PLAN_PROMPT,
            artifacts: &[],
            rounds: vec![create_plan_round()],
            queued_during_first_review: Some(CORRECTION),
            follow_ups: &[],
        })
        .await;
        assert_eq!(
            reserved
                .iter()
                .map(|action| action.tool.as_str())
                .collect::<Vec<_>>(),
            ["create_artifact"],
            "{guidance:?}"
        );
        let rounds = decision_rounds(&offers);
        assert_eq!(rounds.len(), 2, "{guidance:?}: the delivered correction opens a round");
        let (_, tools, after) = rounds[1];
        assert!(tools.iter().any(|tool| tool == "update_artifact"), "{guidance:?}: {tools:?}");
        let pause = after
            .iter()
            .position(|(role, content)| {
                role == "system" && content == model_gateway::queued_input::QUEUED_INPUT_PAUSE
            })
            .expect("the pause is delivered");
        let correction = after
            .iter()
            .position(|(role, content)| role == "user" && content == CORRECTION)
            .expect("the correction is delivered");
        assert!(pause < correction);
        let words = after
            .iter()
            .position(|(role, content)| role == "system" && content == guidance.no_word_count())
            .expect("the no-word-count guidance is still sent");
        assert!(correction < words, "{guidance:?}");
        let rules: Vec<_> = after[correction..]
            .iter()
            .filter_map(|(_, content)| content.split_once(RULE).map(|(_, rule)| rule))
            .collect();
        assert_eq!(rules.len(), usize::from(guidance == AuthorGuidance::V2), "{guidance:?}");
        for rule in rules {
            assert!(
                rule.starts_with(" in this turn unless a newer user message asks you to change it;"),
                "{guidance:?}: the rule forbids the correction's rewrite: {rule}"
            );
        }
    }
}

/// What the rewrite round below proposes: the accepted report again, under
/// its own id, with other text.
const REWRITTEN_REPORT: &str = "# Rapport\n\nEt omskrevet utkast.";
/// Told to the author once a new document is accepted for the instruction.
const ACCEPTED_DOCUMENT_RESULT: &str = "This deliverable is complete for the current request";
const ACCEPTED_DOCUMENT_NOTICE: &str = "The requested document has already been written in this turn";

fn write_round(id: &str, tool: &str, artifact: &str, content: &str) -> Vec<mpv1::ToolCall> {
    vec![mpv1::ToolCall {
        id: id.into(),
        name: tool.into(),
        arguments_json: serde_json::json!({
            "id": artifact, "kind": "document", "title": artifact, "content": content,
        })
        .to_string(),
    }]
}

/// The source reviews of a turn whose candidate carried `text`.
fn reviews_of<'a>(offers: &'a [CapturedOffer], text: &str) -> Vec<&'a CapturedOffer> {
    offers
        .iter()
        .filter(|(id, _, messages)| {
            id.contains("-source-review-") && messages.iter().any(|(_, content)| content.contains(text))
        })
        .collect()
}

fn chat_text(frames: &[(Option<u64>, String, serde_json::Value)]) -> String {
    frames
        .iter()
        .filter(|(_, event, _)| event == "chunk")
        .filter_map(|(_, _, data)| data["delta"].as_str())
        .collect()
}

/// Journeys 02 and 03 (live, 2026-09-26) surfaced this for a campaign and a
/// project plan; the general case is any plain report or note with no
/// further ask. Once its one accepted write is staged, nothing later in this
/// turn could do more than rewrite it, so the round that wrote it ends the
/// phase directly: unlike a narrower prompt shape, a query-only CSV report
/// needs no second decision round to confirm nothing else is needed.
#[tokio::test]
#[serial_test::serial]
async fn a_new_report_with_no_further_ask_ends_the_tool_phase_in_one_round() {
    let (frames, reserved, _, offers) = run_conversation_only_turn(
        CSV_REPORT_PROMPT,
        &[],
        vec![
            create_report_round(),
            // Never reached: the phase already ended after the accepted write.
            write_round("call-rewrite", "create_artifact", "report", REWRITTEN_REPORT),
        ],
    )
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact"],
        "only the accepted write is dispatched"
    );
    assert_eq!(decision_rounds(&offers).len(), 1, "no round after the accepted report");
    assert!(!reviews_of(&offers, "Et kort utkast").is_empty(), "the report is reviewed");
    assert!(reviews_of(&offers, "Et omskrevet utkast").is_empty(), "the unreached rewrite is never reviewed");
    let artifacts: Vec<_> = frames.iter().filter(|(_, event, _)| event == "artifact").collect();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].2["content"], "# Rapport\n\nEt kort utkast.");
    assert_eq!(chat_text(&frames), "Utkastet er klart for gjennomgang i Resultat.");
    let receipt = frames
        .iter()
        .find(|(_, event, _)| event == "result_receipt")
        .map(|(_, _, data)| data["receipt"].clone())
        .expect("the report completes with its receipt");
    let checks = receipt["artifactChecks"].as_array().cloned().unwrap_or_default();
    assert_eq!(checks.len(), 1, "{receipt}");
    assert_eq!(checks[0]["scopedRevisionComplete"], true, "{receipt}");
}

/// The same-id case survives even when something else in that first round
/// (here a read) keeps the phase from ending immediately: the round that
/// proposes the rewrite still never reaches a check or dispatch for it.
#[tokio::test]
#[serial_test::serial]
async fn a_same_id_rewrite_in_a_round_kept_open_by_a_read_is_still_dropped() {
    let read = mpv1::ToolCall {
        id: "call-read".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "budsjett" }).to_string(),
    };
    let (frames, reserved, _, offers) = run_conversation_only_turn(
        CSV_REPORT_PROMPT,
        &[("budsjett", "# Budsjett\n\nIkke oppgitt.")],
        vec![
            [create_report_round(), vec![read]].concat(),
            write_round("call-rewrite", "create_artifact", "report", REWRITTEN_REPORT),
        ],
    )
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact"],
        "only the accepted write and the read are dispatched"
    );
    let rounds = decision_rounds(&offers);
    assert_eq!(rounds.len(), 2, "the read kept a second round open");
    assert!(reviews_of(&offers, "Et omskrevet utkast").is_empty(), "the rewrite is never reviewed");
    let artifacts: Vec<_> = frames.iter().filter(|(_, event, _)| event == "artifact").collect();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].2["content"], "# Rapport\n\nEt kort utkast.");
    // What the rewrite round was told: the result says the document is
    // complete, and the drafting guidance gives way to the same notice.
    let (_, _, second) = rounds[1];
    assert!(second.iter().any(|(role, content)| role == "user" && content.contains(ACCEPTED_DOCUMENT_RESULT)));
    assert_eq!(second.last().map(|(role, content)| (role.as_str(), content.starts_with(ACCEPTED_DOCUMENT_NOTICE))),
        Some(("system", true)), "{second:?}");
}

/// The rewrite guard's generalisation: a same-turn write of a brand-new
/// second document, under its own id and title, is dropped exactly like a
/// same-id rewrite once one new document has already been accepted and
/// nobody's instruction asked for a second one.
#[tokio::test]
#[serial_test::serial]
async fn a_second_new_document_under_its_own_id_and_title_is_also_dropped() {
    let read = mpv1::ToolCall {
        id: "call-read".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "budsjett" }).to_string(),
    };
    let (frames, reserved, _, offers) = run_conversation_only_turn(
        CSV_REPORT_PROMPT,
        &[("budsjett", "# Budsjett\n\nIkke oppgitt.")],
        vec![
            [create_report_round(), vec![read]].concat(),
            write_round("call-second", "create_artifact", "notat", "# Notat\n\nEt helt annet dokument om noe annet."),
        ],
    )
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact"],
        "the second, differently-named document is never dispatched"
    );
    assert_eq!(decision_rounds(&offers).len(), 2, "the read kept a second round open");
    assert!(reviews_of(&offers, "Et helt annet dokument").is_empty(), "never reviewed either");
    let artifacts: Vec<_> = frames.iter().filter(|(_, event, _)| event == "artifact").collect();
    assert_eq!(artifacts.len(), 1, "only the one document the instruction asked for exists");
    assert_eq!(artifacts[0].2["content"], "# Rapport\n\nEt kort utkast.");
}

fn finish_call() -> mpv1::ToolCall {
    mpv1::ToolCall {
        id: "call-finish".into(),
        name: "finish_tool_phase".into(),
        arguments_json: "{}".into(),
    }
}

/// A finish sent in the same round as the new document's write holds once
/// that write is accepted and staged: no result is left for the model to
/// read, so no further round is asked for, which could only rewrite it.
#[tokio::test]
#[serial_test::serial]
async fn a_finish_sent_with_the_accepted_write_ends_the_tool_phase() {
    let (frames, reserved, _, offers) = run_conversation_only_turn(
        CSV_REPORT_PROMPT,
        &[],
        vec![
            [create_report_round(), vec![finish_call()]].concat(),
            write_round("call-rewrite", "create_artifact", "report", REWRITTEN_REPORT),
        ],
    )
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact"]
    );
    assert_eq!(decision_rounds(&offers).len(), 1, "the finish holds");
    assert!(reviews_of(&offers, "Et omskrevet utkast").is_empty());
    assert_eq!(chat_text(&frames), "Utkastet er klart for gjennomgang i Resultat.");
}

/// A finish that came with another call keeps waiting for that call's result:
/// here a read the model has yet to see, so the next round still runs.
#[tokio::test]
#[serial_test::serial]
async fn a_finish_sent_with_a_read_still_waits_for_the_read_result() {
    let read = mpv1::ToolCall {
        id: "call-read".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "budsjett" }).to_string(),
    };
    let (_, reserved, _, offers) = run_conversation_only_turn(
        CSV_REPORT_PROMPT,
        &[("budsjett", "# Budsjett\n\nIkke oppgitt.")],
        vec![[create_report_round(), vec![read, finish_call()]].concat()],
    )
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact"]
    );
    assert_eq!(decision_rounds(&offers).len(), 2, "the read's result reaches a round");
}

/// A correction the user queues while the new report is being checked is a
/// newer user message: it reaches the next round, which may still change the
/// accepted document, and that update is checked and dispatched as usual.
#[tokio::test]
#[serial_test::serial]
async fn a_correction_queued_during_the_check_still_gets_its_round_after_an_accepted_document() {
    const CORRECTION: &str = "Legg til en kort konklusjon i rapporten.";
    let (_, reserved, _, offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CSV_REPORT_PROMPT,
        artifacts: &[],
        rounds: vec![
            create_report_round(),
            write_round("call-conclusion", "update_artifact", "report", "# Rapport\n\nEt kort utkast.\n\nEn kort konklusjon."),
        ],
        queued_during_first_review: Some(CORRECTION),
        follow_ups: &[],
    })
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "update_artifact"],
        "the correction's update is dispatched"
    );
    assert!(!reviews_of(&offers, "En kort konklusjon").is_empty(), "and checked");
    let rounds = decision_rounds(&offers);
    assert!(rounds.len() >= 2);
    let (_, tools, after) = rounds[1];
    assert!(tools.iter().any(|tool| tool == "update_artifact"), "{tools:?}");
    let correction = after
        .iter()
        .position(|(role, content)| role == "user" && content == CORRECTION)
        .expect("the correction is delivered");
    assert!(
        !after[correction..].iter().any(|(_, content)| content.starts_with(ACCEPTED_DOCUMENT_NOTICE)),
        "nothing after the correction forbids its change"
    );
}

/// A campaign request of the kind any business sends, with a brief of its
/// own; not a recorded prompt or source.
const CAMPAIGN_PROMPT: &str = "Bruk kampanjebriefen og språkprofilen til å skrive to Kampanjeinnlegg og én e-post om verkstedet. Lever alt som utkast; ikke publiser, planlegg utsendelser eller send noe.\n\n--- VEDLEGG: brief.md ---\nLinkedIn: to innlegg, 20–40 ord hver.\nE-post: brødtekst på 30–50 ord.\nSnakk til «dere».\nIkke oppgi nettadresse.\n--- SLUTT PÅ VEDLEGG: brief.md ---";

/// Both posts and the email inside the brief's ranges.
const CAMPAIGN: &str = "# Kampanjeutkast\n\n**Status:** Utkast til gjennomgang\n\n## Kampanjeinnlegg: verkstedet\n\n**Status:** Utkast\n\nDere som driver et lite verksted kan nå be om produktarket vårt. Arket beskriver verktøyet, fargene og garantien slik de står i kildene, uten påstander som ikke er dokumentert.\n\n## Kampanjeinnlegg: arbeidsplassen\n\nSnakk med oss om arbeidsplassen deres. Vi forteller gjerne hva produktarket sier om arm, lysnivåer og farger, og dere bestemmer selv om det passer rommet deres.\n\n## E-post\n\n**Emne:** Produktarket for verkstedet\n\n**Brødtekst:**\n\nTakk for at dere vil vite mer om verkstedet. Dere kan be om produktarket når det passer, eller snakke med oss om arbeidsplassen deres. Arket viser egenskapene slik de er beskrevet i kildene, og dere får svar fra oss.\n\n### Kilder\n\nBriefen og produktarket.";

/// A new campaign document holding every piece its adopted brief enumerates,
/// each checked, is the whole deliverable. The phase ends after that write,
/// with no decision round that could only rewrite it.
#[tokio::test]
#[serial_test::serial]
async fn an_accepted_campaign_with_every_brief_piece_ends_the_tool_phase() {
    let (frames, reserved, _, offers) = run_conversation_only_turn(
        CAMPAIGN_PROMPT,
        &[],
        vec![
            write_round("call-campaign", "create_artifact", "kampanje", CAMPAIGN),
            write_round("call-rewrite", "create_artifact", "kampanje", &CAMPAIGN.replace("lite verksted", "lite snekkerverksted")),
        ],
    )
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact"]
    );
    assert_eq!(decision_rounds(&offers).len(), 1, "no round after the accepted campaign");
    assert!(!reviews_of(&offers, "lite verksted").is_empty(), "the campaign is reviewed");
    assert!(reviews_of(&offers, "snekkerverksted").is_empty());
    let receipt = frames
        .iter()
        .find(|(_, event, _)| event == "result_receipt")
        .map(|(_, _, data)| data["receipt"].clone())
        .expect("the campaign completes with its receipt");
    let checks = receipt["artifactChecks"].as_array().cloned().unwrap_or_default();
    assert_eq!(checks.len(), 1, "{receipt}");
    assert_eq!(checks[0]["scopedRevisionComplete"], true, "{receipt}");
    assert_eq!(checks[0]["sourceReview"]["documentChecks"].as_array().map(Vec::len), Some(3), "{receipt}");
    assert_eq!(chat_text(&frames), "Utkastet er klart for gjennomgang i Resultat.");
}

/// The campaign's early end still waits at the round boundary: a correction
/// queued while it is checked gets its round and its update.
#[tokio::test]
#[serial_test::serial]
async fn a_correction_queued_during_the_campaign_check_still_gets_its_round() {
    const CORRECTION: &str = "Gjør det andre innlegget litt mer konkret.";
    let revised = CAMPAIGN.replace("Snakk med oss om arbeidsplassen deres.", "Snakk med oss om arbeidsplassen og bordene deres.");
    let (_, reserved, _, offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", CAMPAIGN),
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: Some(CORRECTION),
        follow_ups: &[],
    })
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "update_artifact"]
    );
    assert!(!reviews_of(&offers, "bordene deres").is_empty(), "the correction's update is checked");
    let rounds = decision_rounds(&offers);
    assert!(rounds.len() >= 2);
    assert!(rounds[1].2.iter().any(|(role, content)| role == "user" && content == CORRECTION));
}

/// [`CAMPAIGN`] with dated post headings, so a later turn can name one post.
fn dated_campaign() -> String {
    CAMPAIGN
        .replace("## Kampanjeinnlegg: verkstedet", "## LinkedIn 21. oktober")
        .replace("## Kampanjeinnlegg: arbeidsplassen", "## LinkedIn 23. oktober")
}

/// A campaign accepted in one turn, then one dated post revised in the next,
/// on the same gateway state. Returns the revision turn's frames and the
/// revision's source-review payloads.
async fn revise_one_dated_post(levers: LatencyLevers) -> (Vec<(Option<u64>, String, serde_json::Value)>, Vec<serde_json::Value>) {
    let campaign = dated_campaign();
    let revised = campaign.replace("Snakk med oss om arbeidsplassen deres.", "Snakk med oss om arbeidsplassen og bordene deres.");
    let (frames, _, _, offers) = run_note_thread_turn(NoteThreadTurn {
        levers,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: None,
        follow_ups: &["Gjør innlegget for 23. oktober mer konkret."],
    })
    .await;
    let payloads = reviews_of(&offers, "bordene deres")
        .into_iter()
        .map(|(_, _, messages)| serde_json::from_str(&messages[1].1).expect("a review payload"))
        .collect();
    (frames, payloads)
}

/// Owner ruling 2026-09-26, end to end: the accepted review of the first
/// turn survives that turn's durable commit, so the revision turn sends the
/// reviewer only the segments it must review again and records the reuse.
/// (Live, the review was left in the run's private workspace and every
/// revision re-reviewed the whole document.)
#[tokio::test]
#[serial_test::serial]
async fn a_revision_turn_reuses_the_review_accepted_in_the_previous_turn() {
    let (frames, payloads) = revise_one_dated_post(LatencyLevers::ALL_ON).await;
    assert_eq!(payloads.len(), 1, "one review of the revision: {payloads:?}");
    let assigned = payloads[0]["reviewIndexes"].as_array().cloned().expect("only some segments are assigned");
    let segments = payloads[0]["segments"].as_array().map(Vec::len).unwrap();
    assert!(!assigned.is_empty() && assigned.len() < segments, "{assigned:?} of {segments}");
    let receipt = frames
        .iter()
        .find(|(_, event, _)| event == "result_receipt")
        .map(|(_, _, data)| data["receipt"].clone())
        .expect("the revision completes with its receipt");
    let review = &receipt["artifactChecks"][0]["sourceReview"];
    assert_eq!(review["reusedSegments"].as_u64(), Some((segments - assigned.len()) as u64), "{receipt}");
    assert_eq!(review["reusedFromReceipt"]["version"], 1, "{receipt}");
}

/// The switch off: the same revision is reviewed in full, with no reuse.
#[tokio::test]
#[serial_test::serial]
async fn with_the_reuse_switch_off_the_revision_turn_reviews_everything() {
    let levers = LatencyLevers { revision_review_reuse: false, ..LatencyLevers::ALL_ON };
    let (frames, payloads) = revise_one_dated_post(levers).await;
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert!(payloads[0].get("reviewIndexes").is_none(), "{:?}", payloads[0]);
    let receipt = frames
        .iter()
        .find(|(_, event, _)| event == "result_receipt")
        .map(|(_, _, data)| data["receipt"].clone())
        .expect("the revision completes with its receipt");
    let review = &receipt["artifactChecks"][0]["sourceReview"];
    assert_eq!(review["reusedSegments"].as_u64(), Some(0), "{receipt}");
    assert!(review["reusedFromReceipt"].is_null(), "{receipt}");
}

/// Owner ruling 2026-09-26, the within-turn half: reuse applies ONLY on a
/// cross-turn revision, "repairs WITHIN a turn keep the independent COMPLETE
/// recheck; reuse never applies there". A correction queued while the SAME
/// turn's own first review still runs (delivered mid-run, at the next round
/// boundary) names the exact dated piece the cross-turn test above targets —
/// but this is one turn, not two, so the second round must never read back
/// the first round's own review, which at that point has not been durably
/// committed at all. (Live bug reproduced before the fix: the second round's
/// review carried `reviewIndexes` for only 4 of 13 segments, reusing 9 from
/// its own first round, because the lookup asked the turn's own live
/// per-round store instead of a snapshot frozen at the turn's start.)
#[tokio::test]
#[serial_test::serial]
async fn a_same_turn_queued_correction_never_reuses_this_turns_own_review() {
    let campaign = dated_campaign();
    let revised = campaign.replace(
        "Snakk med oss om arbeidsplassen deres.",
        "Snakk med oss om arbeidsplassen og bordene deres.",
    );
    let (_, _, _, offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: Some("Gjør innlegget for 23. oktober mer konkret."),
        follow_ups: &[],
    })
    .await;
    let payloads: Vec<serde_json::Value> = reviews_of(&offers, "bordene deres")
        .into_iter()
        .map(|(_, _, messages)| serde_json::from_str(&messages[1].1).expect("a review payload"))
        .collect();
    assert_eq!(payloads.len(), 1, "{payloads:?}");
    assert!(
        payloads[0].get("reviewIndexes").is_none(),
        "same-turn reuse must never apply, however the two rounds are triggered: {:?}",
        payloads[0]
    );
}

/// The sole-artifact prefetch (`MODEL_GATEWAY_SOLE_NOTE_PREFETCH`) extends to
/// a scoped revision of the thread's sole artifact, not only a new note's
/// source: the campaign accepted in the first turn is the thread's only
/// artifact, and "Gjør innlegget for 23. oktober mer konkret." is a
/// recognized scoped revision of it, so the gateway reads it once, through
/// the audited read, before the revision turn's own first model round —
/// which then goes straight to `update_artifact`, exactly as a new note goes
/// straight to `create_artifact` after its own prefetch.
#[tokio::test]
#[serial_test::serial]
async fn a_scoped_revision_of_the_sole_artifact_is_prefetched_before_its_first_model_round() {
    let campaign = dated_campaign();
    let revised = campaign.replace(
        "Snakk med oss om arbeidsplassen deres.",
        "Snakk med oss om arbeidsplassen og bordene deres.",
    );
    let (frames, reserved, finalized, offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: None,
        follow_ups: &["Gjør innlegget for 23. oktober mer konkret."],
    })
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact", "update_artifact"],
        "the revision turn's own read is audited between the campaign's write and its revision"
    );
    assert_eq!(finalized.len(), 3);
    assert_eq!(finalized[1].action_id, reserved[1].action_id);
    assert!(finalized.iter().all(|action| action.outcome == "completed"));

    // No model round is spent on the read: the first decision round to offer
    // `update_artifact` already carries the gateway's read, and goes straight
    // to the revision (a later round may still confirm nothing else is
    // needed, exactly as an ordinary revision does independent of the read).
    let revision_rounds: Vec<&CapturedOffer> = offers
        .iter()
        .filter(|(_, tools, _)| tools.iter().any(|tool| tool == "update_artifact"))
        .collect();
    assert!(!revision_rounds.is_empty());
    let (_, _, messages) = revision_rounds[0];
    let read = format!("Current content of artifact 'kampanje' (v1):\n\n{campaign}");
    let results: Vec<_> = messages
        .iter()
        .filter(|(_, content)| content.contains(&read))
        .collect();
    assert_eq!(results.len(), 1, "{messages:?}");
    assert_eq!(results[0].0, "user");
    assert!(
        results[0].1.starts_with(GATEWAY_READ_HEADER) && results[0].1.contains("- read_artifact [source: "),
        "the read appears as a read_artifact tool result the gateway made: {}",
        results[0].1
    );
    assert!(!results[0].1.contains("your previous request"));
    assert!(!messages.iter().any(|(_, content)| content.contains(READ_INVITATION)));

    let position = |event: &str, name: &str| {
        frames
            .iter()
            .position(|(_, kind, data)| kind == event && data["name"] == name)
    };
    let read_call = position("tool_call", "read_artifact").expect("the read is shown as a step");
    assert!(read_call < position("tool_call", "update_artifact").expect("the revision is written"));
}

/// A real model that decides to re-read the sole artifact anyway, right after
/// the gateway's own prefetch handed it that artifact's complete content,
/// gets no second audited read: unlike the scripted rounds every other test
/// here plays (which never propose a call the round does not offer), this
/// round's own tool list has `read_artifact` narrowed away once the prefetch
/// supplied the content, exactly as the note path narrows down to
/// `create_artifact` once its own prefetch lands. The repeat call is left for
/// the model to see failed rather than silently dropped, so it can recover
/// (`Tool is not available for this turn's source scope.`), and the turn
/// still completes on the following round's `update_artifact`.
#[tokio::test]
#[serial_test::serial]
async fn a_repeated_read_after_the_scoped_revision_prefetch_is_never_dispatched_again() {
    let campaign = dated_campaign();
    let revised = campaign.replace(
        "Snakk med oss om arbeidsplassen deres.",
        "Snakk med oss om arbeidsplassen og bordene deres.",
    );
    let read_again = mpv1::ToolCall {
        id: "call-read-again".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "kampanje" }).to_string(),
    };
    let (frames, reserved, finalized, offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            vec![read_again],
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: None,
        follow_ups: &["Gjør innlegget for 23. oktober mer konkret."],
    })
    .await;
    // Exactly three audited actions: the create, the gateway's own prefetch
    // read, and the eventual update. The model's repeated read never becomes
    // a fourth — it is rejected before it ever reaches audited dispatch.
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact", "update_artifact"],
        "the repeated read must not be dispatched or audited a second time"
    );
    assert_eq!(finalized.len(), 3);
    assert!(finalized.iter().all(|action| action.outcome == "completed"));

    // The round that received the repeated call is the one right after the
    // prefetch — it still offers `update_artifact`, but not `read_artifact`.
    let revision_rounds: Vec<&CapturedOffer> = offers
        .iter()
        .filter(|(_, tools, _)| tools.iter().any(|tool| tool == "update_artifact"))
        .collect();
    assert!(!revision_rounds.is_empty());
    let (_, first_tools, _) = revision_rounds[0];
    assert!(
        !first_tools.iter().any(|tool| tool == "read_artifact"),
        "read_artifact must be withdrawn once the sole artifact's content was already prefetched: {first_tools:?}"
    );

    // The repeated call still shows up as a step (so the model sees it was
    // rejected rather than vanish silently), but its result is an error, not
    // a second successful read.
    assert!(
        frames
            .iter()
            .any(|(_, kind, data)| kind == "tool_call" && data["id"] == "call-read-again"),
        "the repeated call is still shown as a step"
    );
    let repeated_result = frames
        .iter()
        .find(|(_, kind, data)| kind == "tool_result" && data["id"] == "call-read-again")
        .expect("the repeated call gets a result");
    assert_eq!(repeated_result.2["status"], "error", "{:?}", repeated_result.2);
}

/// Several artifacts in the thread mean no prefetch, even for an otherwise
/// recognized scoped revision: the campaign turn also creates a second,
/// unrelated document, so the revision turn's read is left to the model, as
/// for a note among several artifacts.
#[tokio::test]
#[serial_test::serial]
async fn a_scoped_revision_among_several_artifacts_keeps_the_model_decided_read() {
    let campaign = dated_campaign();
    let revised = campaign.replace(
        "Snakk med oss om arbeidsplassen deres.",
        "Snakk med oss om arbeidsplassen og bordene deres.",
    );
    let read_campaign_first = mpv1::ToolCall {
        id: "call-read".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "kampanje" }).to_string(),
    };
    let (_, reserved, _, offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[("budsjett", "# Budsjett\n\nIkke oppgitt.")],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            vec![read_campaign_first],
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: None,
        follow_ups: &["Gjør innlegget for 23. oktober mer konkret."],
    })
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact", "update_artifact"],
        "the model's own read, not a prefetch: no gateway read precedes it"
    );
    let follow_up = "Gjør innlegget for 23. oktober mer konkret.";
    let revision_rounds: Vec<&CapturedOffer> = offers
        .iter()
        .filter(|(_, tools, messages)| {
            tools.iter().any(|tool| tool == "update_artifact")
                && messages.iter().any(|(role, content)| role == "user" && content == follow_up)
        })
        .collect();
    assert!(revision_rounds.len() >= 2, "the model's own read kept a second round open: {revision_rounds:?}");
    let (_, first_tools, first_messages) = revision_rounds[0];
    assert!(first_tools.iter().any(|tool| tool == "read_artifact"), "{first_tools:?}");
    assert!(
        !first_messages.iter().any(|(_, content)| content.contains("Current content of artifact")),
        "no gateway prefetch precedes the model's own read"
    );
}

/// An unrecognized or whole-document instruction on the thread's sole
/// artifact means no prefetch either: the model decides whether to read, as
/// for any other revision.
#[tokio::test]
#[serial_test::serial]
async fn a_whole_document_revision_of_the_sole_artifact_keeps_the_model_decided_read() {
    let campaign = dated_campaign();
    let rewritten = campaign.replace("Kampanjeutkast", "Revidert kampanjeutkast");
    let read_campaign_first = mpv1::ToolCall {
        id: "call-read".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "kampanje" }).to_string(),
    };
    let (_, reserved, _, offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            vec![read_campaign_first],
            write_round("call-rewrite", "update_artifact", "kampanje", &rewritten),
        ],
        queued_during_first_review: None,
        follow_ups: &["Gjør hele kampanjen litt kortere."],
    })
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact", "update_artifact"],
        "a whole-document instruction is never prefetched, so the model's own read is what is audited"
    );
    let follow_up = "Gjør hele kampanjen litt kortere.";
    let revision_rounds: Vec<&CapturedOffer> = offers
        .iter()
        .filter(|(_, tools, messages)| {
            tools.iter().any(|tool| tool == "update_artifact")
                && messages.iter().any(|(role, content)| role == "user" && content == follow_up)
        })
        .collect();
    assert!(revision_rounds.len() >= 2, "the model's own read kept a second round open: {revision_rounds:?}");
    let (_, _, first_messages) = revision_rounds[0];
    assert!(
        !first_messages.iter().any(|(_, content)| content.contains("Current content of artifact")),
        "no gateway prefetch precedes the model's own read"
    );
}

/// `MODEL_GATEWAY_SOLE_NOTE_PREFETCH` off: a scoped revision that WOULD be
/// prefetched with the lever on gets exactly today's behaviour instead — the
/// model's own read, offered and told to read as before — and the request
/// sent to the model is byte-identical to the lever-on turn's first round
/// without the gateway's read and its guidance (mirroring
/// `with_the_prefetch_off_the_model_reads_its_sole_plan_as_before` for the
/// note path).
#[tokio::test]
#[serial_test::serial]
async fn with_the_prefetch_off_a_scoped_revision_gets_the_model_decided_read() {
    let campaign = dated_campaign();
    let revised = campaign.replace(
        "Snakk med oss om arbeidsplassen deres.",
        "Snakk med oss om arbeidsplassen og bordene deres.",
    );
    let read_campaign_first = mpv1::ToolCall {
        id: "call-read".into(),
        name: "read_artifact".into(),
        arguments_json: serde_json::json!({ "id": "kampanje" }).to_string(),
    };
    let off = LatencyLevers { sole_note_prefetch: false, ..LatencyLevers::ALL_ON };
    let (_, reserved, finalized, off_offers) = run_note_thread_turn(NoteThreadTurn {
        levers: off,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            vec![read_campaign_first],
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: None,
        follow_ups: &["Gjør innlegget for 23. oktober mer konkret."],
    })
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact", "read_artifact", "update_artifact"]
    );
    assert!(finalized.iter().all(|action| action.outcome == "completed"));
    let off_rounds: Vec<&CapturedOffer> = off_offers
        .iter()
        .filter(|(_, tools, _)| tools.iter().any(|tool| tool == "update_artifact"))
        .collect();
    assert!(off_rounds.len() >= 2, "a model round is spent on the read: {off_rounds:?}");
    let (_, first_tools, first_off) = off_rounds[0];
    assert!(first_tools.iter().any(|tool| tool == "read_artifact"), "{first_tools:?}");
    assert!(!first_off.iter().any(|(_, content)| content.contains("Current content of artifact")));
    assert!(!off_offers
        .iter()
        .flat_map(|(_, _, messages)| messages)
        .any(|(_, content)| content.contains(GATEWAY_READ_HEADER)));

    // The same turn with the prefetch on. Its only round carries the
    // gateway's read; take that one message away and it is the off turn's
    // FIRST round, message for message — unlike the note path, there is no
    // guidance to swap, since a scoped revision adds none.
    let (_, _, _, on_offers) = run_note_thread_turn(NoteThreadTurn {
        levers: LatencyLevers::ALL_ON,
        author_guidance: AuthorGuidance::default(),
        review_proposal_rule: ReviewProposalRule::default(),
        content: CAMPAIGN_PROMPT,
        artifacts: &[],
        rounds: vec![
            write_round("call-campaign", "create_artifact", "kampanje", &campaign),
            write_round("call-revision", "update_artifact", "kampanje", &revised),
        ],
        queued_during_first_review: None,
        follow_ups: &["Gjør innlegget for 23. oktober mer konkret."],
    })
    .await;
    let on_rounds: Vec<&CapturedOffer> = on_offers
        .iter()
        .filter(|(_, tools, _)| tools.iter().any(|tool| tool == "update_artifact"))
        .collect();
    assert!(!on_rounds.is_empty());
    let mut restored = on_rounds[0].2.clone();
    let gateway_read = restored
        .iter()
        .position(|(_, content)| content.starts_with(GATEWAY_READ_HEADER))
        .expect("the prefetching turn carries the gateway's read");
    restored.remove(gateway_read);
    assert_eq!(&restored, first_off);
}

/// Q15 reload during a quiet phase: a slow first tool call (and, for
/// source-bounded drafts, source review and repair) can leave the resume
/// buffer empty for a long time. Resume used to answer 404 there, and the
/// browser settled the turn as a lost connection while the server went on to
/// finish and publish it. A producer still running for this caller must be
/// resumable, through to its answer and a real `done`.
///
/// The tool phase's own progress used to go out on the live channel only, so
/// even a successful resume showed none of it: the replay must carry the
/// search's `tool_call` before any answer text.
#[tokio::test]
#[serial_test::serial]
async fn invoke_resume_follows_a_live_producer_that_has_buffered_nothing_yet() {
    use futures::StreamExt as _;
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");

    let quarry = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/search"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(3))
                .set_body_json(serde_json::json!({
                    "data": {
                        "results": [{
                            "url": "https://example.test/slow",
                            "title": "Slow",
                            "snippet": "slow result",
                            "source": "mock",
                            "score": 0.5
                        }]
                    }
                })),
        )
        .mount(&quarry)
        .await;

    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.quarry = model_gateway::quarry::Client::new(model_gateway::quarry::Config {
        base_url: quarry.uri(),
        token: "test-token".to_owned(),
        timeout: Duration::from_secs(10),
    });
    let stream_buffers = state.stream_buffers.clone();
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
        .body(Body::from(
            r#"{"content":"latest slow lookup","model":"m","thread_id":"thread-quiet-resume","features":["citations","tools","steps"],"browse_web":true}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut frames = resp.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_millis(1_500), frames.next())
        .await
        .expect("the response must open before the slow tool phase completes")
        .expect("an SSE frame")
        .expect("a readable SSE frame");
    let first = String::from_utf8(first.to_vec()).unwrap();
    assert!(first.contains("event: connected"), "{first}");
    let request_id = first
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|value| {
            value
                .get("request_id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .expect("connected SSE event must expose its request id");
    // The browser reloads in the middle of the quiet tool phase.
    drop(frames);
    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    assert!(
        !stream_buffers.replay_after(&buffer_key, None).await.found,
        "precondition: nothing is buffered yet, which is exactly when resume used to 404"
    );

    let resume = Request::builder()
        .method("GET")
        .uri(format!("/v1/invoke/resume/{request_id}"))
        .header(AUTHORIZATION, "Bearer dev")
        .body(Body::empty())
        .unwrap();
    let resumed = app.clone().oneshot(resume).await.unwrap();
    assert_eq!(
        resumed.status(),
        StatusCode::OK,
        "a producer that is still running must not be reported as gone"
    );
    let body = tokio::time::timeout(
        Duration::from_secs(30),
        to_bytes(resumed.into_body(), usize::MAX),
    )
    .await
    .expect("the resumed stream must end when the producer finishes")
    .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("\"delta\":\"hel\""), "{body}");
    assert!(body.contains("\"delta\":\"lo\""), "{body}");
    assert!(body.contains("event: done"), "{body}");

    let frames = sse_frames(&body);
    let first_chunk = frames
        .iter()
        .position(|(_, event, _)| event == "chunk")
        .expect("the answer is replayed");
    let progress = frames
        .iter()
        .position(|(_, event, _)| event == "tool_call")
        .unwrap_or_else(|| panic!("the tool phase's progress must be replayed: {body}"));
    assert!(progress < first_chunk, "progress before the answer: {body}");
    let (search_id, _, search) = &frames[progress];
    assert_eq!(search["name"], "web_search", "{body}");
    // One cursor for every frame: the progress frame is numbered in sequence
    // with the answer, so a later resume from it skips neither.
    let ids: Vec<u64> = frames.iter().filter_map(|(id, _, _)| *id).collect();
    assert!(search_id.is_some(), "replayed progress carries its id");
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]), "{ids:?}");
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_emits_error_when_inference_fully_unavailable() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    // Both InferStream and the Infer fallback are down: emit an honest `error`
    // event (chat-parity §20), never a fake successful `done`, and publish no
    // usage because no tokens were produced.
    let client = spawn_mock(MockDown).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client).await;
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
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(!body.contains("event: chunk"));
    assert!(!body.contains("event: done"));
    assert!(body.contains("event: error"));
    assert!(body.contains("model_plane_unavailable"));

    let drained = publisher.drain();
    assert!(drained.iter().any(|(s, _)| s == "mp.v1.stream.opened"));
    assert!(drained.iter().any(|(s, _)| s == "mp.v1.stream.closed"));
    assert!(!drained.iter().any(|(s, _)| s.starts_with("mp.v1.usage.")));
    assert_direct_terminal_for_prepared_run(
        &session_handles,
        TerminalOutcome::Failed,
        "provider_unavailable",
        "ordinary streamed inference failure must close its prepared run as failed",
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_replay_determinism() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;

    for _ in 0..2 {
        let (app, _delegated_jwks) = authenticated_user_router(
            build_router(state.clone(), None),
            "org_placeholder",
            "user_placeholder",
        )
        .await;
        let req = Request::builder()
            .method("POST")
            .uri("/v1/invoke")
            .header(AUTHORIZATION, "Bearer dev")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"content":"hi","model":"m","session_key":"sk-1"}"#,
            ))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        2
    );
    assert_eq!(
        session_handles.append_message_count.load(Ordering::SeqCst),
        4
    );
    assert_eq!(managed_start_count(&session_handles), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_propagates_thread_id() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"hi","model":"m","session_key":"sk-x"}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        prepared_thread_id(&session_handles),
        "thread-sk-x".to_owned()
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_reuses_existing_thread() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            r#"{"content":"hi","model":"m","thread_id":"thread-existing"}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    assert_eq!(
        session_handles.create_thread_count.load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        session_handles.append_message_count.load(Ordering::SeqCst),
        2
    );
    assert_eq!(managed_start_count(&session_handles), 1);
    assert_eq!(
        prepared_thread_id(&session_handles),
        "thread-existing".to_owned()
    );
}

/// One ordinary streamed turn ("hel" + "lo") against a Session Core mock set up
/// by `configure`. Returns the SSE body, the mock's durable state and the
/// replay buffers.
async fn run_direct_turn<S: InferenceCore>(
    inference: S,
    body: &'static str,
    configure: impl FnOnce(&MockSessionHandles),
) -> (
    String,
    MockSessionHandles,
    model_gateway::stream_buffer::StreamBufferStore,
) {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(inference).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    configure(&session_handles);
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let stream_buffers = state.stream_buffers.clone();
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
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        String::from_utf8(body.to_vec()).unwrap(),
        session_handles,
        stream_buffers,
    )
}

fn stream_request_id(body: &str) -> String {
    sse_frames(body)
        .into_iter()
        .find_map(|(_, _, data)| data["request_id"].as_str().map(str::to_owned))
        .expect("the stream exposes its request id")
}

fn saved_assistant_answers(handles: &MockSessionHandles) -> Vec<String> {
    handles
        .conversation
        .lock()
        .unwrap()
        .iter()
        .filter(|(role, _, _)| role == "assistant")
        .map(|(_, _, content)| content.clone())
        .collect()
}

fn assistant_append_rpcs(handles: &MockSessionHandles) -> usize {
    handles
        .append_rpc_captures
        .lock()
        .unwrap()
        .iter()
        .filter(|(role, _)| role == "assistant")
        .count()
}

/// W2 atomic completion: a durable cancel that commits after the pre-save
/// heartbeat answered "live" used to find the answer already saved by the
/// separate append, and the receipt then failed. The save and the receipt are
/// now one Session Core transaction, so the run being over means nothing is
/// saved, the client is told the run is no longer active, and no second
/// outcome is attempted.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_saves_nothing_when_the_run_ends_between_the_heartbeat_and_the_save() {
    let (body, handles, stream_buffers) = run_direct_turn(
        MockOk::default(),
        r#"{"content":"hi","model":"m"}"#,
        // Heartbeat 1 precedes provider work, heartbeat 2 is the pre-save
        // check; the cancel commits right after the second answered "live".
        |handles| handles.cancel_after_heartbeat.store(2, Ordering::SeqCst),
    )
    .await;

    assert!(body.contains("event: chunk"), "{body}");
    assert!(body.contains("session_liveness_failed"), "{body}");
    assert!(
        body.contains("The chat run is no longer active. Please retry."),
        "{body}"
    );
    assert!(!body.contains("event: done"), "{body}");
    assert!(
        !body.contains("session_terminalization_failed"),
        "an already-ended run is not terminalized again: {body}"
    );
    assert_eq!(
        saved_assistant_answers(&handles),
        Vec::<String>::new(),
        "nothing may be saved onto a run that a cancel ended"
    );
    assert!(
        handles.terminal_outcome_captures.lock().unwrap().is_empty(),
        "no terminal receipt is recorded or attempted"
    );
    {
        let completions = handles.direct_completion_captures.lock().unwrap();
        assert_eq!(completions.len(), 1, "one atomic attempt, no retry");
        assert_eq!(completions[0].0, "Bearer gateway-terminalizer-token");
        assert_eq!(completions[0].1.content, "hello");
    }

    let request_id = stream_request_id(&body);
    let buffer_key = model_gateway::stream_buffer::scoped_stream_key(
        "org_placeholder",
        "user_placeholder",
        &request_id,
    );
    let replay = stream_buffers.replay_after(&buffer_key, None).await;
    assert!(replay.done.is_none(), "a failed turn never replays as done");
    assert!(replay.ended.is_some(), "the replay buffer is finished");
    assert_eq!(
        replay.deltas.last().map(|frame| frame.event.as_str()),
        Some("error")
    );
}

/// The same race on the unary fallback path (the streaming RPC unavailable).
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_fallback_saves_nothing_when_the_run_ends_between_the_heartbeat_and_the_save()
{
    let (body, handles, _stream_buffers) = run_direct_turn(
        MockStreamDown,
        r#"{"content":"hi","model":"m"}"#,
        |handles| handles.cancel_after_heartbeat.store(2, Ordering::SeqCst),
    )
    .await;

    assert!(body.contains("session_liveness_failed"), "{body}");
    assert!(!body.contains("event: done"), "{body}");
    assert!(!body.contains("session_terminalization_failed"), "{body}");
    assert_eq!(saved_assistant_answers(&handles), Vec::<String>::new());
    assert!(handles.terminal_outcome_captures.lock().unwrap().is_empty());
    assert_eq!(handles.direct_completion_captures.lock().unwrap().len(), 1);
}

/// A lost response after Session Core committed is retried with the same key
/// (the request id), and the retry returns the original result: one saved
/// answer, one receipt, and the turn completes normally.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_retries_a_lost_completion_response_without_a_second_message() {
    let (body, handles, _stream_buffers) = run_direct_turn(
        MockOk::default(),
        r#"{"content":"hi","model":"m"}"#,
        |handles| {
            handles
                .lose_first_completion_response
                .store(true, Ordering::SeqCst);
        },
    )
    .await;

    assert!(body.contains("event: done"), "{body}");
    assert!(!body.contains("event: error"), "{body}");
    let request_id = stream_request_id(&body);
    let completions = handles.direct_completion_captures.lock().unwrap();
    assert_eq!(completions.len(), 2, "one retry after the lost response");
    for (authorization, completion) in completions.iter() {
        assert_eq!(authorization, "Bearer gateway-terminalizer-token");
        assert_eq!(
            completion.idempotency_key, request_id,
            "the retry reuses the request id as its key"
        );
    }
    drop(completions);
    assert_eq!(saved_assistant_answers(&handles), vec!["hello".to_owned()]);
    assert_eq!(assistant_append_rpcs(&handles), 0);
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Completed,
        "",
        "the replayed completion is the one receipt",
    );
}

/// The atomic call rides the Gateway's scoped service credential, so a
/// delegated user bearer that expired during a long turn no longer loses the
/// checked answer with `assistant_persist_failed`.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_saves_the_answer_after_the_delegated_bearer_expired() {
    let (body, handles, _stream_buffers) = run_direct_turn(
        MockOk::default(),
        r#"{"content":"hi","model":"m"}"#,
        |handles| {
            handles
                .expire_delegated_assistant_append
                .store(true, Ordering::SeqCst);
        },
    )
    .await;

    assert!(body.contains("event: done"), "{body}");
    assert!(!body.contains("assistant_persist_failed"), "{body}");
    assert_eq!(saved_assistant_answers(&handles), vec!["hello".to_owned()]);
    assert_eq!(
        assistant_append_rpcs(&handles),
        0,
        "the delegated-bearer append is not used"
    );
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Completed,
        "",
        "the answer and its receipt are durable",
    );
}

/// A Session Core that has not been rebuilt answers UNIMPLEMENTED; the Gateway
/// then saves and completes in two calls, as before, with the same frames.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_uses_two_calls_when_session_core_lacks_the_atomic_completion() {
    let (body, handles, _stream_buffers) = run_direct_turn(
        MockOk::default(),
        r#"{"content":"hi","model":"m"}"#,
        |handles| {
            handles
                .direct_completion_unimplemented
                .store(true, Ordering::SeqCst);
        },
    )
    .await;

    assert!(body.contains("event: done"), "{body}");
    assert!(!body.contains("event: error"), "{body}");
    assert_eq!(handles.direct_completion_captures.lock().unwrap().len(), 1);
    assert_eq!(saved_assistant_answers(&handles), vec!["hello".to_owned()]);
    assert_eq!(
        assistant_append_rpcs(&handles),
        1,
        "the answer went through AppendMessage"
    );
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Completed,
        "",
        "the two-call path still records the one Completed receipt",
    );
}

/// On success the client sees the same frames whichever path saved the turn:
/// the atomic call, or the two calls it replaces (still used while Session
/// Core answers UNIMPLEMENTED). Timing values and the per-turn ids differ
/// between the two turns, so only their presence is compared.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_frames_are_unchanged_by_the_atomic_completion() {
    fn normalized(body: &str) -> Vec<(String, serde_json::Value)> {
        sse_frames(body)
            .into_iter()
            .map(|(_, event, mut data)| {
                if let Some(object) = data.as_object_mut() {
                    // Each turn mints its own thread, run and request.
                    for volatile in [
                        "request_id",
                        "thread_id",
                        "run_id",
                        "latency_ms",
                        "server_ms",
                        "provider_ms",
                        "orchestration_ms",
                        "marks",
                        "phases",
                    ] {
                        if object.contains_key(volatile) {
                            object.insert(volatile.to_owned(), serde_json::Value::Null);
                        }
                    }
                }
                (event, data)
            })
            .collect()
    }
    fn phases(body: &str) -> Vec<String> {
        sse_frames(body)
            .into_iter()
            .find(|(_, event, _)| event == "turn_timing")
            .and_then(|(_, _, data)| data["phases"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|span| span["phase"].as_str().map(str::to_owned))
            .collect()
    }
    let request = r#"{"content":"hi","model":"m","features":["usage"]}"#;
    let (atomic, atomic_handles, _) = run_direct_turn(MockOk::default(), request, |_| {}).await;
    let (two_calls, two_call_handles, _) = run_direct_turn(MockOk::default(), request, |handles| {
        handles
            .direct_completion_unimplemented
            .store(true, Ordering::SeqCst);
    })
    .await;

    assert!(atomic.contains("event: done"), "{atomic}");
    assert_eq!(assistant_append_rpcs(&atomic_handles), 0);
    assert_eq!(assistant_append_rpcs(&two_call_handles), 1);
    assert_eq!(normalized(&atomic), normalized(&two_calls));
    assert_eq!(
        sse_frames(&atomic)
            .iter()
            .map(|(id, _, _)| *id)
            .collect::<Vec<_>>(),
        sse_frames(&two_calls)
            .iter()
            .map(|(id, _, _)| *id)
            .collect::<Vec<_>>(),
        "the same frames carry the same resume ids"
    );
    let atomic_phases = phases(&atomic);
    for expected in ["durable_append", "terminal_ack"] {
        assert!(
            atomic_phases.iter().any(|phase| phase == expected),
            "{expected} missing from {atomic_phases:?}"
        );
    }
    assert_eq!(atomic_phases, phases(&two_calls));
}

/// Rolling deploy: the first attempt committed but its response was lost, and
/// the retry reached a Session Core replica without the atomic call. Falling
/// back to `AppendMessage` there would save the answer a second time, so the
/// turn fails instead, and the one committed answer stays the only one.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_never_saves_twice_when_the_retry_reaches_an_old_session_core() {
    let (body, handles, _stream_buffers) = run_direct_turn(
        MockOk::default(),
        r#"{"content":"hi","model":"m"}"#,
        |handles| {
            handles
                .lose_first_completion_response
                .store(true, Ordering::SeqCst);
            handles
                .retry_reaches_old_replica
                .store(true, Ordering::SeqCst);
        },
    )
    .await;

    assert_eq!(handles.direct_completion_captures.lock().unwrap().len(), 2);
    assert_eq!(
        assistant_append_rpcs(&handles),
        0,
        "no two-call fallback after an ambiguous first attempt"
    );
    assert_eq!(saved_assistant_answers(&handles), vec!["hello".to_owned()]);
    assert!(!body.contains("event: done"), "{body}");
    assert!(body.contains("assistant_persist_failed"), "{body}");
}

/// Session Core refusing the atomic save is final: no retry, no two-call
/// fallback through the delegated bearer, nothing saved, and the run gets its
/// Failed receipt. The first refusal is the 2026-09-24 live one (a ZDR-marked
/// service credential); routing around it would have hidden that the atomic
/// path was broken while quietly bringing back the races it closes.
#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_never_routes_a_refused_completion_around_session_core() {
    for (code, reason) in [
        (
            tonic::Code::FailedPrecondition,
            "ZDR credentials cannot access durable session writes",
        ),
        (
            tonic::Code::PermissionDenied,
            "the assistant message must go to the run's own thread",
        ),
    ] {
        let (body, handles, _stream_buffers) = run_direct_turn(
            MockOk::default(),
            r#"{"content":"hi","model":"m"}"#,
            |handles| {
                *handles.refuse_direct_completion.lock().unwrap() = Some((code, reason));
            },
        )
        .await;

        assert!(
            body.contains("assistant_persist_failed"),
            "{code:?}: {body}"
        );
        assert!(!body.contains("event: done"), "{code:?}: {body}");
        assert_eq!(
            handles.direct_completion_captures.lock().unwrap().len(),
            1,
            "{code:?}: a refusal is not retried"
        );
        assert_eq!(
            assistant_append_rpcs(&handles),
            0,
            "{code:?}: no two-call fallback after a refusal"
        );
        assert_eq!(saved_assistant_answers(&handles), Vec::<String>::new());
        assert_direct_terminal_for_prepared_run(
            &handles,
            TerminalOutcome::Failed,
            "execution_failed",
            "the unsaved turn closes its run as failed",
        );
    }
}

/// A tool round is a fresh model decision. It opts out of inference-core's
/// response cache, so a quick retry or back-to-back runs of the same brief
/// never replay an earlier identical draft.
#[tokio::test]
#[serial_test::serial]
async fn every_tool_round_opts_out_of_the_inference_response_cache() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let seen: CapturedCacheControl = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk {
        captured_cache_control: Some(seen.clone()),
        ..MockOk::default()
    })
    .await;
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
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
        .body(Body::from(
            r#"{"content":"kan du finne siste nytt om model plane?","model":"m","thread_id":"thread-fresh-round","features":["tools","citations"],"tools":[{"name":"web_search","description":"Search the web","parameters_json":"{\"type\":\"object\"}"}]}"#,
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: done"), "{body}");
    let seen = seen.lock().unwrap();
    // A tool round carries the turn's own request id; the verification pass's
    // independent draws carry `-resample-N`. Titles and follow-up suggestions
    // may still use the cache: a repeated title is harmless.
    let turn_id = seen
        .iter()
        .map(|(request_id, _)| request_id.as_str())
        .min_by_key(|request_id| request_id.len())
        .unwrap_or_else(|| panic!("the turn made no unary infer: {body}"))
        .to_owned();
    let fresh: Vec<_> = seen
        .iter()
        .filter(|(request_id, _)| *request_id == turn_id || request_id.contains("-resample-"))
        .collect();
    assert!(
        fresh.iter().any(|(request_id, _)| *request_id == turn_id),
        "the turn made no tool-decision infer: {seen:?}"
    );
    for (request_id, cache_control) in fresh {
        assert_eq!(
            cache_control.as_deref(),
            Some("no-store"),
            "{request_id} must bypass the response cache"
        );
    }
}

/// Text inference-core never relays, planted in a refusal's status message
/// where a careless reader would pick it up.
const REFUSAL_CANARY: &str = "Try again at 3:05 PM CANARY";

/// A turn on the `ChatGPT` subscription route.
const SUBSCRIPTION_TURN: &str = r#"{"content":"hi","model":"gpt-5.6-terra","provider":"openai-codex-subscription","subscription_connection_id":"conn-1"}"#;

/// The usage-limit refusal inference-core sends: its code and timing as
/// trailers, beside a status message the gateway must never show.
fn subscription_refusal(code: &str, grpc: tonic::Code, timing: &[(&'static str, &str)]) -> Status {
    let mut metadata = tonic::metadata::MetadataMap::new();
    metadata.insert("x-mp-provider-error", code.parse().unwrap());
    for (key, value) in timing {
        metadata.insert(*key, value.parse().unwrap());
    }
    Status::with_metadata(
        grpc,
        format!("subscription refused: {code} {REFUSAL_CANARY}"),
        metadata,
    )
}

fn usage_limit_refusal() -> Status {
    subscription_refusal(
        "subscription_usage_limit",
        tonic::Code::ResourceExhausted,
        &[
            ("x-mp-retry-after-ms", "52260000"),
            ("x-mp-resets-at", "2026-09-26T01:21:00Z"),
        ],
    )
}

const USAGE_LIMIT_MESSAGE: &str = "Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles 26.09.2026 kl. 01:21 UTC. Ingen annen modell ble brukt.";

fn error_frames(body: &str) -> Vec<serde_json::Value> {
    sse_frames(body)
        .into_iter()
        .filter(|(_, name, _)| name == "error")
        .map(|(_, _, data)| data)
        .collect()
}

fn rpc_count(calls: &CapturedCalls, rpc: &str) -> usize {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| *name == rpc)
        .count()
}

/// The live failure: the subscription's usage limit refuses the answer
/// stream. The client gets one `error` frame carrying the reason and its
/// reset, and the gateway neither falls back to the unary `Infer` (which
/// would ask the refused account again) nor shows the status text.
#[tokio::test]
#[serial_test::serial]
async fn a_subscription_refusal_at_stream_open_is_one_error_frame_and_no_fallback() {
    let calls: CapturedCalls = Arc::new(Mutex::new(Vec::new()));
    let (body, handles, _) = run_direct_turn(
        MockOk {
            captured_calls: Some(calls.clone()),
            refusal: Some(MockRefusal {
                at: RefusalPoint::AnswerStream,
                status: usage_limit_refusal(),
            }),
            ..MockOk::default()
        },
        SUBSCRIPTION_TURN,
        |_| {},
    )
    .await;

    let errors = error_frames(&body);
    assert_eq!(errors.len(), 1, "{body}");
    let error = &errors[0];
    assert_eq!(error["code"], "subscription_usage_limit", "{body}");
    assert_eq!(error["retryable"], false);
    assert_eq!(error["resets_at"], "2026-09-26T01:21:00Z");
    assert_eq!(error["retry_after_ms"], 52_260_000);
    assert_eq!(error["retry_at"], "2026-09-26T01:21:00Z", "a known reset is the retry instant");
    assert_eq!(error["message"], USAGE_LIMIT_MESSAGE);
    assert!(!body.contains("event: chunk") && !body.contains("event: done"), "{body}");
    assert!(!body.contains("CANARY"), "the status text never reaches the client: {body}");
    assert_eq!(rpc_count(&calls, "infer_stream"), 1);
    assert_eq!(rpc_count(&calls, "infer"), 0, "no unary fallback: {body}");
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Failed,
        "provider_unavailable",
        "a refused subscription turn closes its run as a provider outage",
    );
    clear_model_auth_env();
}

/// A subscription stream that failed for no typed reason no longer falls
/// back to the unary `Infer` either: that doubled the calls on a failing
/// route and surfaced the raw status text.
#[tokio::test]
#[serial_test::serial]
async fn an_untyped_subscription_stream_failure_is_the_generic_route_failure() {
    let calls: CapturedCalls = Arc::new(Mutex::new(Vec::new()));
    let (body, handles, _) = run_direct_turn(
        MockOk {
            captured_calls: Some(calls.clone()),
            refusal: Some(MockRefusal {
                at: RefusalPoint::AnswerStream,
                status: Status::unavailable(format!(
                    "subscription broker stream failed: subscription_broker_failed {REFUSAL_CANARY}"
                )),
            }),
            ..MockOk::default()
        },
        SUBSCRIPTION_TURN,
        |_| {},
    )
    .await;

    let errors = error_frames(&body);
    assert_eq!(errors.len(), 1, "{body}");
    assert_eq!(errors[0]["code"], "subscription_route_unavailable");
    assert_eq!(errors[0]["retryable"], true);
    assert!(errors[0].get("resets_at").is_none(), "{body}");
    assert_eq!(
        errors[0]["message"],
        "Den valgte ChatGPT-abonnementstilkoblingen kunne ikke fullføre svaret. Prøv igjen om litt. Ingen annen modell ble brukt."
    );
    assert!(!body.contains("CANARY"), "{body}");
    assert_eq!(rpc_count(&calls, "infer"), 0, "no unary fallback: {body}");
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Failed,
        "provider_unavailable",
        "an unanswered subscription turn closes its run as a provider outage",
    );
    clear_model_auth_env();
}

/// Off the subscription route nothing changes: a throttled answer stream
/// still falls back to the unary `Infer`, and is not worded as a
/// subscription refusal.
#[tokio::test]
#[serial_test::serial]
async fn a_throttle_off_the_subscription_route_keeps_the_unary_fallback() {
    let calls: CapturedCalls = Arc::new(Mutex::new(Vec::new()));
    let (body, _handles, _) = run_direct_turn(
        MockOk {
            captured_calls: Some(calls.clone()),
            refusal: Some(MockRefusal {
                at: RefusalPoint::AnswerStream,
                status: subscription_refusal(
                    "rate_limited",
                    tonic::Code::ResourceExhausted,
                    &[("x-mp-retry-after-ms", "1000")],
                ),
            }),
            ..MockOk::default()
        },
        r#"{"content":"hi","model":"m"}"#,
        |_| {},
    )
    .await;
    assert!(body.contains("event: done"), "{body}");
    assert!(body.contains("hello"), "the unary fallback's answer: {body}");
    assert!(error_frames(&body).is_empty(), "{body}");
    assert!(rpc_count(&calls, "infer") >= 1, "the unary fallback answered");
    clear_model_auth_env();
}

/// A tool-decision round refused because the ChatGPT sign-in must be
/// renewed ends the turn on that reason: the frame says to reconnect, a
/// retry is not offered as it stands, and the fixed explanation is saved.
#[tokio::test]
#[serial_test::serial]
async fn a_subscription_refusal_in_a_tool_round_names_its_reason() {
    let calls: CapturedCalls = Arc::new(Mutex::new(Vec::new()));
    let (body, handles, _) = run_direct_turn(
        MockOk {
            captured_calls: Some(calls.clone()),
            refusal: Some(MockRefusal {
                at: RefusalPoint::ToolRound,
                status: subscription_refusal(
                    "subscription_reauth_required",
                    tonic::Code::Unavailable,
                    &[],
                ),
            }),
            ..MockOk::default()
        },
        r#"{"content":"hi","model":"gpt-5.6-terra","provider":"openai-codex-subscription","subscription_connection_id":"conn-1","features":["tools"]}"#,
        |_| {},
    )
    .await;

    let expected = "ChatGPT-tilkoblingen må fornyes. Koble til ChatGPT på nytt under Innstillinger → Integrasjoner, og prøv igjen. Ingen annen modell ble brukt.";
    let errors = error_frames(&body);
    assert_eq!(errors.len(), 1, "{body}");
    assert_eq!(errors[0]["code"], "subscription_reauth_required", "{body}");
    assert_eq!(errors[0]["retryable"], false);
    assert_eq!(errors[0]["message"], expected);
    assert!(!body.contains("CANARY"), "{body}");
    assert_eq!(rpc_count(&calls, "infer"), 1, "one decision round, no retry");
    assert_eq!(rpc_count(&calls, "infer_stream"), 0, "no answer after a refused round");
    assert_eq!(saved_assistant_answers(&handles), vec![expected.to_owned()]);
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Failed,
        "provider_unavailable",
        "a refused tool round closes its run as a provider outage",
    );
    clear_model_auth_env();
}

/// The overflow inference-core declares for the subscription's
/// `subscription_context_too_long`: the `too_long` trailer.
fn subscription_overflow() -> Status {
    let mut metadata = tonic::metadata::MetadataMap::new();
    metadata.insert("x-mp-provider-error", "too_long".parse().unwrap());
    Status::with_metadata(
        tonic::Code::Unavailable,
        format!("subscription_context_too_long {REFUSAL_CANARY}"),
        metadata,
    )
}

/// Tool-decision rounds of a plain turn: every unary `infer` that offered
/// tools and is not a source review.
fn offered_tool_rounds(offers: &[CapturedOffer]) -> usize {
    offers
        .iter()
        .filter(|(request_id, tools, _)| !tools.is_empty() && !request_id.contains("-source-review-"))
        .count()
}

/// A long thread overflows first in the tool decision, the first call that
/// carries the whole history. That is not a route outage: the tool phase ends
/// as on any other route, and the answer stream (which carries no tool
/// schema and can shed old history) answers. The overflow is never reported
/// as the retryable `subscription_route_unavailable`, and the round is not
/// retried.
#[tokio::test]
#[serial_test::serial]
async fn an_overlong_subscription_tool_round_hands_the_turn_to_the_answer_stream() {
    let calls: CapturedCalls = Arc::new(Mutex::new(Vec::new()));
    let offers: CapturedOffers = Arc::new(Mutex::new(Vec::new()));
    let (body, _handles, _) = run_direct_turn(
        MockOk {
            captured_calls: Some(calls.clone()),
            captured_offers: Some(offers.clone()),
            refusal: Some(MockRefusal {
                at: RefusalPoint::ToolRound,
                status: subscription_overflow(),
            }),
            ..MockOk::default()
        },
        r#"{"content":"hi","model":"gpt-5.6-terra","provider":"openai-codex-subscription","subscription_connection_id":"conn-1","features":["tools"]}"#,
        |_| {},
    )
    .await;

    assert!(error_frames(&body).is_empty(), "{body}");
    assert!(body.contains("event: done"), "{body}");
    assert!(!body.contains("subscription_route_unavailable"), "{body}");
    assert!(!body.contains("CANARY"), "{body}");
    assert_eq!(offered_tool_rounds(&offers.lock().unwrap()), 1, "one decision round, not retried");
    assert_eq!(rpc_count(&calls, "infer_stream"), 1, "the answer stream answered");
    let calls = calls.lock().unwrap();
    let (_, answer) = calls
        .iter()
        .find(|(rpc, _)| *rpc == "infer_stream")
        .expect("the answer stream was asked");
    assert!(
        answer
            .iter()
            .any(|(_, content)| content.contains("too long for the model's context window")),
        "the answer is told why the tool phase ended: {answer:?}"
    );
    assert!(
        !answer
            .iter()
            .any(|(_, content)| content.contains("temporary inference failure")),
        "an overflow is not called a temporary failure: {answer:?}"
    );
    clear_model_auth_env();
}

/// A customer reply with a word maximum and a source attachment: a checked
/// document is owed from the first round.
const CHECKED_REPLY_PROMPT: &str =
    "Lag et svarutkast, maks 150 ord. Vis kildene i en egen intern merknad.";

/// A source-bound turn on the subscription route, with the attachment and
/// the PII policy that keeps the attachment readable.
async fn run_source_bound_subscription_turn(
    inference: MockOk,
    prompt: &str,
) -> (String, MockSessionHandles) {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(inference).await;
    let (mock_session, handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    let capability_core = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/safety"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "policies": [] })),
        )
        .mount(&capability_core)
        .await;
    state.capability_core_base_url = capability_core.uri();
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let now = token_now();
    let capability = encode_test_token(
        &serde_json::json!({
            "sub": "user_placeholder", "iss": TEST_AUTH_ISSUER, "aud": "capability-core",
            "exp": now + 300, "nbf": now - 5, "org_id": "org_placeholder",
            "user_id": "user_placeholder", "principal_type": "user", "zdr": true
        }),
        &zdr_route_test_keypair().0,
    );
    let app = app.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let capability = capability.clone();
            async move {
                request.headers_mut().insert(
                    "x-capability-authorization",
                    HeaderValue::from_str(&format!("Bearer {capability}")).unwrap(),
                );
                next.run(request).await
            }
        },
    ));
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "content": format!("{prompt}{NOTE_ATTACHMENT}"),
                "model": "gpt-5.6-terra",
                "provider": "openai-codex-subscription",
                "subscription_connection_id": "conn-1",
                "thread_id": "thread-note",
                "features": ["tools", "artifacts", "usage", "conversation_only"],
            })
            .to_string(),
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (String::from_utf8(body.to_vec()).unwrap(), handles)
}

/// With a checked document owed, the answer stream cannot write it, so an
/// overlong decision round ends the turn on `prompt_too_long`: not retried,
/// no retry offered, closed as the request's own failure, and never blamed
/// on the draft's sources or on the route.
#[tokio::test]
#[serial_test::serial]
async fn an_overlong_subscription_tool_round_owing_a_document_is_prompt_too_long() {
    let calls: CapturedCalls = Arc::new(Mutex::new(Vec::new()));
    let (body, handles) = run_source_bound_subscription_turn(
        MockOk {
            captured_calls: Some(calls.clone()),
            refusal: Some(MockRefusal {
                at: RefusalPoint::ToolRound,
                status: subscription_overflow(),
            }),
            ..MockOk::default()
        },
        CHECKED_REPLY_PROMPT,
    )
    .await;

    let expected = "Samtalen er for lang for ChatGPT-modellen, selv etter komprimering. Start en ny tråd. Ingen annen modell ble brukt.";
    let errors = error_frames(&body);
    assert_eq!(errors.len(), 1, "{body}");
    assert_eq!(errors[0]["code"], "prompt_too_long", "{body}");
    assert_eq!(errors[0]["retryable"], false);
    assert_eq!(errors[0]["message"], expected);
    assert!(!body.contains("event: done") && !body.contains("event: artifact"), "{body}");
    assert!(!body.contains("CANARY"), "{body}");
    assert_eq!(rpc_count(&calls, "infer"), 1, "one decision round, not retried");
    assert_eq!(rpc_count(&calls, "infer_stream"), 0, "no answer without the document");
    assert_eq!(saved_assistant_answers(&handles), vec![expected.to_owned()]);
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Failed,
        "inference_failed",
        "an overlong request closes its run as its own failure, not an outage",
    );
    clear_model_auth_env();
}

/// A source-bound draft whose review the subscription refused: the draft is
/// not published, the explanation names the usage limit and its reset instead
/// of blaming the draft, and no repair or second review is attempted.
#[tokio::test]
#[serial_test::serial]
async fn a_subscription_refusal_during_the_source_review_is_not_blamed_on_the_draft() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let offers: CapturedOffers = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk {
        planned_rounds: Some(Arc::new(Mutex::new(vec![create_plan_round()].into()))),
        approve_source_reviews: true,
        captured_offers: Some(offers.clone()),
        refusal: Some(MockRefusal {
            at: RefusalPoint::SourceReview,
            status: usage_limit_refusal(),
        }),
        ..MockOk::default()
    })
    .await;
    let (mock_session, handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    // Without a readable PII policy the turn is redacted, which also collapses
    // the attachment markers a source-bounded turn is recognised by.
    let capability_core = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/safety"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "policies": [] })),
        )
        .mount(&capability_core)
        .await;
    state.capability_core_base_url = capability_core.uri();
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let now = token_now();
    let capability = encode_test_token(
        &serde_json::json!({
            "sub": "user_placeholder", "iss": TEST_AUTH_ISSUER, "aud": "capability-core",
            "exp": now + 300, "nbf": now - 5, "org_id": "org_placeholder",
            "user_id": "user_placeholder", "principal_type": "user", "zdr": true
        }),
        &zdr_route_test_keypair().0,
    );
    let app = app.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let capability = capability.clone();
            async move {
                request.headers_mut().insert(
                    "x-capability-authorization",
                    HeaderValue::from_str(&format!("Bearer {capability}")).unwrap(),
                );
                next.run(request).await
            }
        },
    ));
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "content": format!("{PLAN_PROMPT}{NOTE_ATTACHMENT}"),
                "model": "gpt-5.6-terra",
                "provider": "openai-codex-subscription",
                "subscription_connection_id": "conn-1",
                "thread_id": "thread-note",
                "features": ["tools", "artifacts", "usage", "conversation_only"],
            })
            .to_string(),
        ))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    let expected = "Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles 26.09.2026 kl. 01:21 UTC. Utkastet er ikke publisert. Ingen annen modell ble brukt.";
    let errors = error_frames(&body);
    assert_eq!(errors.len(), 1, "{body}");
    assert_eq!(errors[0]["code"], "subscription_usage_limit", "{body}");
    assert_eq!(errors[0]["retryable"], false);
    assert_eq!(errors[0]["resets_at"], "2026-09-26T01:21:00Z");
    assert_eq!(errors[0]["message"], expected);
    assert!(!body.contains("event: done"), "{body}");
    assert!(!body.contains("event: artifact"), "the draft is not published: {body}");
    assert!(!body.contains("CANARY"), "{body}");
    // The turn's own accounting: the decision round and one review, with no
    // retried review, repair or answer after the refusal.
    let frames = sse_frames(&body);
    let timing = &frames
        .iter()
        .find(|(_, name, _)| name == "turn_timing")
        .expect("the failed turn reports its timing")
        .2;
    assert_eq!(timing["outcome"], "failed");
    assert_eq!(timing["counts"]["provider_calls"], 2, "{timing}");
    assert_eq!(timing["counts"]["repairs"], 0, "{timing}");
    assert_eq!(
        decision_rounds(&offers.lock().unwrap()).len(),
        1,
        "one decision round"
    );
    assert_eq!(saved_assistant_answers(&handles), vec![expected.to_owned()]);
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Failed,
        "provider_unavailable",
        "a refused source review closes its run as a provider outage",
    );
    clear_model_auth_env();
}

/// The unary route answers a refusal with its own status, a `Retry-After`
/// counted to the reset, and the typed body; never the tonic `Display`,
/// which would print the trailers, nor the status text.
#[tokio::test]
#[serial_test::serial]
async fn invoke_answers_a_subscription_refusal_with_its_status_body_and_retry_after() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let resets_at = (chrono::Utc::now() + chrono::TimeDelta::hours(2))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let client = spawn_mock(MockOk {
        refusal: Some(MockRefusal {
            at: RefusalPoint::Unary,
            status: subscription_refusal(
                "subscription_usage_limit",
                tonic::Code::ResourceExhausted,
                &[("x-mp-resets-at", &resets_at)],
            ),
        }),
        ..MockOk::default()
    })
    .await;
    let (mock_session, handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(SUBSCRIPTION_TURN))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let retry_after: u64 = response
        .headers()
        .get("retry-after")
        .expect("a known reset sets Retry-After")
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (7_190..=7_201).contains(&retry_after),
        "counted to the reset two hours out: {retry_after}"
    );
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("MetadataMap") && !text.contains("CANARY"), "{text}");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["code"], "subscription_usage_limit");
    assert_eq!(body["retryable"], false);
    assert_eq!(body["resets_at"], resets_at.as_str());
    assert_eq!(body["retry_at"], resets_at.as_str(), "the absolute instant, for a proxy that drops the header");
    assert!(body.get("retry_after_ms").is_none(), "{body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("Den tilkoblede ChatGPT-planen har nådd bruksgrensen for Codex. Den tilbakestilles "),
        "{body}"
    );
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Failed,
        "provider_unavailable",
        "a refused unary subscription turn closes its run as a provider outage",
    );
    clear_model_auth_env();
}

/// `/v1/invoke` sends one message, so an overflow on the subscription route
/// is that message being too long: `prompt_too_long` with a 413, no retry
/// offered, worded to shorten the message, and closed as the request's own
/// failure. Before, it was the retryable `subscription_route_unavailable`
/// (a 502 and a provider outage), and every offered retry failed again.
#[tokio::test]
#[serial_test::serial]
async fn invoke_answers_a_subscription_overflow_as_prompt_too_long() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk {
        refusal: Some(MockRefusal {
            at: RefusalPoint::Unary,
            status: subscription_overflow(),
        }),
        ..MockOk::default()
    })
    .await;
    let (mock_session, handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client).await;
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(SUBSCRIPTION_TURN))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(response.headers().get("retry-after").is_none(), "waiting cannot help");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();
    assert!(!text.contains("CANARY"), "{text}");
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["code"], "prompt_too_long");
    assert_eq!(body["retryable"], false);
    assert_eq!(
        body["error"],
        "Meldingen er for lang for ChatGPT-modellen. Kort den ned, og prøv igjen. Ingen annen modell ble brukt."
    );
    assert_direct_terminal_for_prepared_run(
        &handles,
        TerminalOutcome::Failed,
        "inference_failed",
        "an overlong unary subscription request closes as its own failure",
    );
    clear_model_auth_env();
}

// --- In-turn recheck scope on every turn, end to end (owner ruling 2026-09-27). ---

/// [`run_note_thread_turn`], but every "-source-review-" and
/// "-artifact-repair-" inference also just pops the next planned round
/// instead of auto-approving, in the order the checked draft actually calls
/// them -- so a rejection, its private repair and the recheck can be
/// scripted exactly like a tool-decision round.
async fn run_note_thread_turn_with_scripted_review(
    content: &str,
    rounds: Vec<Vec<mpv1::ToolCall>>,
) -> ConversationTurn {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let offers: CapturedOffers = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_mock(MockOk {
        planned_rounds: Some(Arc::new(Mutex::new(rounds.into()))),
        approve_source_reviews: false,
        captured_offers: Some(offers.clone()),
        ..MockOk::default()
    })
    .await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (mut state, _publisher) = make_state(client, session_client).await;
    state.latency_levers = LatencyLevers::ALL_ON;
    let capability_core = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/safety"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "policies": [] })))
        .mount(&capability_core)
        .await;
    state.capability_core_base_url = capability_core.uri();
    let (app, _delegated_jwks) = authenticated_user_router(
        build_router(state, None),
        "org_placeholder",
        "user_placeholder",
    )
    .await;
    let now = token_now();
    let capability = encode_test_token(
        &serde_json::json!({
            "sub": "user_placeholder", "iss": TEST_AUTH_ISSUER, "aud": "capability-core",
            "exp": now + 300, "nbf": now - 5, "org_id": "org_placeholder",
            "user_id": "user_placeholder", "principal_type": "user", "zdr": true
        }),
        &zdr_route_test_keypair().0,
    );
    let app = app.layer(middleware::from_fn(
        move |mut request: Request<Body>, next: Next| {
            let capability = capability.clone();
            async move {
                request.headers_mut().insert(
                    "x-capability-authorization",
                    HeaderValue::from_str(&format!("Bearer {capability}")).unwrap(),
                );
                next.run(request).await
            }
        },
    ));
    let req = Request::builder()
        .method("POST")
        .uri("/v1/invoke/stream")
        .header(AUTHORIZATION, "Bearer dev")
        .header(CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "content": format!("{content}{NOTE_ATTACHMENT}"),
                "model": "m",
                "thread_id": "thread-note",
                "features": ["tools", "artifacts", "usage", "conversation_only"],
            })
            .to_string(),
        ))
        .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(body.contains("event: done"), "{body}");
    let reserved = session_handles.reserve_tool_action_captures.lock().unwrap().clone();
    let finalized = session_handles.finalize_tool_action_captures.lock().unwrap().clone();
    let offers = offers.lock().unwrap().clone();
    (sse_frames(&body), reserved, finalized, offers)
}

/// Owner ruling 2026-09-27, "in-turn recheck scope on every turn", end to
/// end: a first-turn draft whose first full review rejects one segment; the
/// private repair fixes it; the recheck is RESTRICTED to that repaired
/// segment alone (`reviewIndexes`), never resending the untouched intro and
/// the unrelated post -- and the persisted receipt reports them as carried.
#[tokio::test]
#[serial_test::serial]
async fn a_scripted_rejection_narrows_the_in_turn_recheck_end_to_end() {
    let content = "# Plan\nOverview.\n\n## Detail\nPickup is confirmed today.\n\n## Other\nDelivery is confirmed today.\n";
    let create = vec![mpv1::ToolCall {
        id: "call-note".into(),
        name: "create_artifact".into(),
        arguments_json: serde_json::json!({
            "id": "status", "kind": "document", "title": "Status", "content": content,
        })
        .to_string(),
    }];
    let first_review = vec![mpv1::ToolCall {
        id: "review-1".into(),
        name: "report_source_review".into(),
        arguments_json: serde_json::json!({ "checks": [
            {"index": 0, "status": "no_assertions", "evidence": [], "reason": ""},
            {"index": 1, "status": "unsupported", "evidence": [], "reason": "Pickup is not confirmed."},
            {"index": 2, "status": "no_assertions", "evidence": [], "reason": ""},
        ] })
        .to_string(),
    }];
    let repair = vec![mpv1::ToolCall {
        id: "repair-1".into(),
        name: "report_segment_repairs".into(),
        arguments_json: serde_json::json!({ "repairs": [
            {"index": 1, "text": "## Detail\nPickup status is unknown."},
        ] })
        .to_string(),
    }];
    let recheck = vec![mpv1::ToolCall {
        id: "review-2".into(),
        name: "report_source_review".into(),
        arguments_json: serde_json::json!({ "checks": [
            {"index": 1, "status": "supported", "evidence": [{"source": 0, "lines": [0]}], "reason": ""},
        ] })
        .to_string(),
    }];
    let (frames, reserved, _, offers) = run_note_thread_turn_with_scripted_review(
        "Create a short status note.",
        vec![create, first_review, repair, recheck],
    )
    .await;
    assert_eq!(
        reserved.iter().map(|action| action.tool.as_str()).collect::<Vec<_>>(),
        ["create_artifact"],
        "the checked note completes despite the rejection and repair"
    );
    let reviews: Vec<_> = offers.iter().filter(|(id, _, _)| id.contains("-source-review-")).collect();
    assert_eq!(reviews.len(), 2, "the first full review and the restricted recheck");
    let recheck_message = reviews[1].2.iter().rev().find(|(role, _)| role == "user")
        .expect("the recheck's user message").1.clone();
    let recheck_data: serde_json::Value = serde_json::from_str(&recheck_message).unwrap();
    assert_eq!(recheck_data["reviewIndexes"], serde_json::json!([1]),
        "the recheck is restricted to the repaired segment; the untouched intro and post are carried");
    let source_reviews: Vec<serde_json::Value> = frames.iter()
        .filter(|(_, event, _)| event == "result_receipt")
        .flat_map(|(_, _, data)| data["receipt"]["artifactChecks"].as_array().cloned().unwrap_or_default())
        .map(|check| check["sourceReview"].clone())
        .collect();
    assert_eq!(source_reviews.len(), 1);
    let receipt = &source_reviews[0];
    assert_eq!(receipt["reviewedSegments"], 1);
    assert_eq!(receipt["carriedInTurnSegments"], 2);
    assert_eq!(
        receipt["reviewedSegments"].as_u64().unwrap()
            + receipt["reusedSegments"].as_u64().unwrap()
            + receipt["carriedInTurnSegments"].as_u64().unwrap(),
        3
    );
}
