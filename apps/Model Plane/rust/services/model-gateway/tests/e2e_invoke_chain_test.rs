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
    http_routes::build_router,
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
type CapturedZdrRequests = Arc<Mutex<Vec<(&'static str, bool)>>>;

const TEST_AUTH_KID: &str = "model-gateway-zdr-route-test";
const TEST_AUTH_ISSUER: &str = "https://auth.test/model";

#[derive(Default)]
struct MockOk {
    captured_messages: Option<CapturedMessages>,
    captured_zdr_requests: Option<CapturedZdrRequests>,
    infer_call_count: Option<Arc<AtomicUsize>>,
}

impl MockOk {
    fn capturing(captured_messages: CapturedMessages) -> Self {
        Self {
            captured_messages: Some(captured_messages),
            captured_zdr_requests: None,
            infer_call_count: None,
        }
    }

    fn capturing_zdr(captured_zdr_requests: CapturedZdrRequests) -> Self {
        Self {
            captured_messages: None,
            captured_zdr_requests: Some(captured_zdr_requests),
            infer_call_count: None,
        }
    }

    fn counting(infer_call_count: Arc<AtomicUsize>) -> Self {
        Self {
            captured_messages: None,
            captured_zdr_requests: None,
            infer_call_count: Some(infer_call_count),
        }
    }

    fn capture(&self, request: &InferRequest) {
        if let Some(infer_call_count) = &self.infer_call_count {
            infer_call_count.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(captured_messages) = &self.captured_messages {
            captured_messages.lock().unwrap().push(
                request
                    .messages
                    .iter()
                    .map(|message| (message.role.clone(), message.content.clone()))
                    .collect(),
            );
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
        let request = request.into_inner();
        self.capture(&request);
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
        }))
    }
    async fn infer_stream(
        &self,
        request: TReq<InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        let request = request.into_inner();
        self.capture(&request);
        Ok(Response::new(Box::pin(futures::stream::iter(vec![
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
            }),
        ]))))
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
        self.handles
            .heartbeat_captures
            .lock()
            .unwrap()
            .push((authorization.clone(), request.clone()));
        if authorization != "Bearer gateway-terminalizer-token" {
            return Err(Status::unauthenticated(
                "managed heartbeat requires the scoped service credential",
            ));
        }
        let _ = managed_source(request.source)?;
        if self.handles.fail_heartbeat.load(Ordering::SeqCst) {
            return Err(Status::unavailable("managed heartbeat unavailable"));
        }
        Ok(Response::new(HeartbeatManagedRunResponse {
            renewed_until: None,
            already_terminal: false,
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

    let response = app.oneshot(req).await.unwrap();
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
    // append lands.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if session_handles.append_captures.lock().unwrap().len() >= 2 {
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
    let replay = stream_buffers.replay_after(&buffer_key, None).await;
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

    let captured = captured_messages.lock().unwrap();
    // Not an exact count: title and follow-up generation add inference calls, so
    // the total shifts with unrelated features. What matters is that the search
    // turn's tool results reached a prompt, and that the image turn's artifact
    // reached a later one.
    assert!(
        captured.iter().any(|set| set.iter().any(|(_, content)| {
            content.contains("Tool results") && content.contains("https://verevon.test/model-plane")
        })),
        "the forced search results should have reached an inference prompt",
    );
    let followup_messages = captured
        .iter()
        .find(|set| {
            set.iter().any(|(role, content)| {
                role == "assistant" && content.contains("I generated an image artifact")
            })
        })
        .expect("the image artifact should have reached a later prompt");
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
