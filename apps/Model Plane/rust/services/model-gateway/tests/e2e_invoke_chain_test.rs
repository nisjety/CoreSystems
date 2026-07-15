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
use mp_contracts::model_plane::v1::{
    inference_core_client::InferenceCoreClient,
    inference_core_server::{InferenceCore, InferenceCoreServer},
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
    GetVideoGenerationJobResponse, InferChunk, InferRequest, InferResponse, LanguageAnalysisResult,
    ListAgentSkillsRequest, ListAgentSkillsResponse, ListConversationRequest,
    ListConversationResponse, ListModelsRequest, ListModelsResponse, ListSpeechVoicesRequest,
    ListSpeechVoicesResponse, ListThreadsRequest, ListThreadsResponse,
    ListTranslationLanguagesRequest, ListTranslationLanguagesResponse, ModelInfo,
    ReplayThreadRequest, ReserveToolActionRequest, ReserveToolActionResponse,
    SaveCheckpointRequest, SaveCheckpointResponse, SessionMessage, SpeechVoiceInfo,
    StartRunRequest, StartRunResponse, StreamVideoGenerationContentRequest,
    StreamVideoGenerationContentResponse, SynthesizeSpeechRequest, SynthesizeSpeechResponse,
    TranscribeSpeechRequest, TranscribeSpeechResponse, TranslateTextRequest, TranslateTextResponse,
    TranslationDetection, TranslationLanguageInfo,
};
use mp_events::publisher::InMemoryPublisher;
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio::net::TcpListener;
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
}

impl MockOk {
    fn capturing(captured_messages: CapturedMessages) -> Self {
        Self {
            captured_messages: Some(captured_messages),
            captured_zdr_requests: None,
        }
    }

    fn capturing_zdr(captured_zdr_requests: CapturedZdrRequests) -> Self {
        Self {
            captured_messages: None,
            captured_zdr_requests: Some(captured_zdr_requests),
        }
    }

    fn capture(&self, request: &InferRequest) {
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
            request_id: String::new(),
            content: "hello".into(),
            model_used: "mock".into(),
            stop_reason: "stop".into(),
            input_tokens: 1,
            output_tokens: 1,
            tool_calls: Vec::new(),
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
                request_id: "req-stream-ok".into(),
                delta: "hel".into(),
                done: false,
                model_used: "mock".into(),
                input_tokens: 0,
                output_tokens: 0,
            }),
            Ok(InferChunk {
                request_id: "req-stream-ok".into(),
                delta: "lo".into(),
                done: true,
                model_used: "mock".into(),
                input_tokens: 3,
                output_tokens: 2,
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
            request_id: String::new(),
            content: "hello".into(),
            model_used: "mock".into(),
            stop_reason: "stop".into(),
            input_tokens: 1,
            output_tokens: 1,
            tool_calls: Vec::new(),
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
    start_run_count: Arc<AtomicUsize>,
    context_assembly_count: Arc<AtomicUsize>,
    /// (role, `thread_id`) per `append_message` call
    append_captures: Arc<Mutex<Vec<(String, String)>>>,
    conversation: Arc<Mutex<Vec<(String, String, String)>>>,
    context_assembly_requests: Arc<Mutex<Vec<(String, String, u32)>>>,
    context_segments: Arc<Mutex<Option<Vec<ContextSegment>>>>,
    start_run_thread_id: Arc<Mutex<Option<String>>>,
    append_missing_thread_once: Arc<Mutex<Option<String>>>,
    reserve_tool_action_captures: Arc<Mutex<Vec<ReserveToolActionRequest>>>,
    finalize_tool_action_captures: Arc<Mutex<Vec<FinalizeToolActionRequest>>>,
    fail_reserve_tool_action: Arc<AtomicBool>,
    fail_finalize_tool_action: Arc<AtomicBool>,
}

struct MockSessionCore {
    handles: MockSessionHandles,
}

impl MockSessionCore {
    fn new() -> (Self, MockSessionHandles) {
        let handles = MockSessionHandles {
            create_thread_count: Arc::new(AtomicUsize::new(0)),
            append_message_count: Arc::new(AtomicUsize::new(0)),
            start_run_count: Arc::new(AtomicUsize::new(0)),
            context_assembly_count: Arc::new(AtomicUsize::new(0)),
            append_captures: Arc::new(Mutex::new(Vec::new())),
            conversation: Arc::new(Mutex::new(Vec::new())),
            context_assembly_requests: Arc::new(Mutex::new(Vec::new())),
            context_segments: Arc::new(Mutex::new(None)),
            start_run_thread_id: Arc::new(Mutex::new(None)),
            append_missing_thread_once: Arc::new(Mutex::new(None)),
            reserve_tool_action_captures: Arc::new(Mutex::new(Vec::new())),
            finalize_tool_action_captures: Arc::new(Mutex::new(Vec::new())),
            fail_reserve_tool_action: Arc::new(AtomicBool::new(false)),
            fail_finalize_tool_action: Arc::new(AtomicBool::new(false)),
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
        request: TReq<StartRunRequest>,
    ) -> Result<Response<StartRunResponse>, Status> {
        self.handles.start_run_count.fetch_add(1, Ordering::SeqCst);
        let req = request.into_inner();
        *self.handles.start_run_thread_id.lock().unwrap() = Some(req.thread_id.clone());
        Ok(Response::new(StartRunResponse {
            run_id: format!("run-for-{}", req.thread_id),
            created_at: None,
        }))
    }

    async fn complete_step(
        &self,
        _: TReq<CompleteStepRequest>,
    ) -> Result<Response<CompleteStepResponse>, Status> {
        Err(Status::unimplemented(
            "complete_step not needed in this test",
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
                role: role.clone(),
                content: content.clone(),
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

async fn spawn_session_mock<S: SessionCore>(
    svc: S,
) -> SessionCoreClient<tonic::transport::Channel> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        Server::builder()
            .add_service(SessionCoreServer::new(svc))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .ok();
    });
    let ch = Endpoint::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap();
    SessionCoreClient::new(ch)
}

fn make_state(
    client: InferenceCoreClient<tonic::transport::Channel>,
    session_client: SessionCoreClient<tonic::transport::Channel>,
) -> (AppState, Arc<DynPublisher>) {
    let publisher = Arc::new(DynPublisher::InMemory(InMemoryPublisher::new()));
    let mut state = AppState::new();
    state.publisher = publisher.clone();
    state.inference_client = client;
    state.session_client = session_client;
    (state, publisher)
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
    let (state, _) = make_state(client, session_client);

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
    let (state, _) = make_state(client, session_client);

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
    let (state, _) = make_state(client, session_client);

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
    let (state, _) = make_state(client, session_client);
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
    let (state, _) = make_state(client, session_client);
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
    let (state, _) = make_state(client, session_client);
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
    let (state, publisher) = make_state(client, session_client);
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
    assert_eq!(session_handles.start_run_count.load(Ordering::SeqCst), 1);
    let captures = session_handles.append_captures.lock().unwrap();
    assert_eq!(captures.len(), 2);
    assert_eq!(captures[0].0, "user");
    assert_eq!(captures[1].0, "assistant");
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_returns_502_when_inference_unavailable() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockDown).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client);
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
    assert_eq!(session_handles.start_run_count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_emits_stream_and_usage_on_success() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client);
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
    let (state, _publisher) = make_state(client, session_client);
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
    assert_eq!(session_handles.start_run_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        *session_handles.start_run_thread_id.lock().unwrap(),
        Some("thread-thread-client-provisional".to_owned())
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
    let (state, _publisher) = make_state(client, session_client);
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
    assert_eq!(session_handles.start_run_count.load(Ordering::SeqCst), 1);
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
    let (state, _publisher) = make_state(client, session_client);
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
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].0, "system");
    assert!(messages[0].1.contains("Velion context assembly"));
    assert!(messages[0].1.contains("[thread]"));
    assert!(messages[0].1.contains("Model Plane owns reasoning"));
    assert!(messages[0].1.contains("[episodic]"));
    assert!(messages[0].1.contains("[redacted-email]"));
    assert!(messages[0].1.contains("[retrieval]"));
    assert!(messages[0].1.contains("Data Plane retrieval"));
    assert!(messages[0].1.contains("[knowledge]"));
    assert!(messages[0].1.contains("LLM wiki entry"));
    assert!(messages[0].1.contains("[graph]"));
    assert!(messages[0].1.contains("GraphRAG"));
    assert!(!messages[0].1.contains("[prompt]"));
    assert!(!messages[0].1.contains("ima@example.com"));
    assert_eq!(
        messages[1],
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
    assert_eq!(context_requests[0].1, "run-for-thread-assembly");
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
                    "snippet": "Ima can be interpreted as a short personal name with meanings that vary by language and culture.",
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
    let (mut state, _publisher) = make_state(client, session_client);
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
            r#"{"content":"kan du finne ut hva navnet mitt betyr?","model":"m","thread_id":"thread-name-memory","features":["tools","citations"],"tools":[{"name":"web_search","description":"Search the web","parameters_json":"{\"type\":\"object\"}"}]}"#,
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
    // `run_tool_rounds` performs one unary infer (the model declines here)
    // before the final streaming answer. With the preceding plain turn that is
    // 3 captured infer requests in total. The tool-round infer and the final
    // stream see the same messages, so `captured.last()` carries the full
    // grounded context asserted below.
    assert_eq!(captured.len(), 3);
    let search_messages = captured.last().expect("search answer should infer");
    assert!(search_messages
        .iter()
        .any(|(role, content)| { role == "user" && content.contains("mitt navn er ima") }));
    assert!(search_messages.iter().any(|(role, content)| {
        role == "user" && content.contains("kan du finne ut hva navnet mitt betyr?")
    }));
    assert!(search_messages.iter().any(|(_, content)| {
        content.contains("Current request: kan du finne ut hva navnet mitt betyr?")
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
                    "snippet": "Claude Opus 4.8 is Anthropic's most capable model.",
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
    let (mut state, _publisher) = make_state(client, session_client);
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
            r#"{"content":"Claude Opus 4.8 official","model":"m","thread_id":"thread-browse-flag","features":["citations"],"browse_web":true}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("event: citation"));
    assert!(body.contains("https://www.anthropic.com/claude/opus"));

    let captured = captured_messages.lock().unwrap();
    let messages = captured.last().expect("search answer should infer");
    assert!(messages
        .iter()
        .any(|(_, content)| content.contains("web_search →")));
    assert!(messages
        .iter()
        .any(|(_, content)| content.contains("Claude Opus 4.8")));

    let reservations = session_handles.reserve_tool_action_captures.lock().unwrap();
    assert_eq!(reservations.len(), 1);
    assert_eq!(reservations[0].run_id, "run-for-thread-browse-flag");
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
    let (mut state, _publisher) = make_state(client, session_client);
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
            r#"{"content":"search for audited result","model":"m","thread_id":"thread-audit-down","features":["citations"],"browse_web":true}"#,
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
    let (mut state, _publisher) = make_state(client, session_client);
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
            r#"{"content":"search for audited result","model":"m","thread_id":"thread-audit-finalize-down","features":["citations"],"browse_web":true}"#,
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
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_stream_generate_image_emits_attachment_and_persists() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client);
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
                    "url": "https://velion.test/model-plane",
                    "title": "Model Plane",
                    "snippet": "Model Plane owns reasoning, sessions, inference, tools, and cost controls.",
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
    let (mut state, _publisher) = make_state(client, session_client);
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
            r#"{"content":"kan du gi meg svaret på model plane og hva den er?","model":"m","thread_id":"thread-sequence","features":["tools","citations"],"tools":[{"name":"web_search","description":"Search the web","parameters_json":"{\"type\":\"object\"}"}]}"#,
        ))
        .unwrap();
    let search_resp = app.clone().oneshot(search_req).await.unwrap();
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX).await.unwrap();
    let search_body = String::from_utf8(search_body.to_vec()).unwrap();
    assert!(search_body.contains("event: citation"));
    assert!(search_body.contains("https://velion.test/model-plane"));

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
    // Each Search-enabled turn makes two model calls (forced web_search → unary
    // `run_tool_rounds` infer offering fetch_url/knowledge_search → final
    // stream). The two search turns therefore contribute 4 captured infer
    // requests; the image turn never reaches inference. captured[0]/[1] are the
    // first search turn's tool-round + stream, [2]/[3] the follow-up's.
    assert_eq!(captured.len(), 4);
    assert!(captured[0].iter().any(|(_, content)| {
        content.contains("Tool results") && content.contains("https://velion.test/model-plane")
    }));
    let followup_messages = captured.last().unwrap();
    assert!(followup_messages.iter().any(|(role, content)| {
        role == "assistant"
            && content
                .contains("I generated an image artifact: generated-image.png for prompt: lag et bilde av det")
    }));
    assert!(followup_messages.iter().any(|(role, content)| {
        role == "system" && content.contains("the assistant already generated an image artifact")
    }));
    // The follow-up also carries the web_search tool, so the Search gate forces
    // a fresh web lookup for it too: its messages now include their own forced
    // "Tool results" context, appended AFTER the user's follow-up question (so
    // that context block, not the question, is the final message handed to
    // inference).
    assert!(followup_messages
        .iter()
        .any(|(_, content)| content.contains("Tool results")));
    assert!(followup_messages.iter().any(|(role, content)| {
        role == "user" && content.contains("kan du gi meg svaret på model plane")
    }));
    assert!(followup_messages.iter().any(|(role, content)| {
        role == "user" && content.contains("hva snakket vi om, og lagde vi et bilde?")
    }));
    let last = followup_messages
        .last()
        .expect("follow-up turn should infer");
    assert_eq!(last.0.as_str(), "user");
    assert!(last.1.contains("Tool results"));
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
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client);
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
    let (mock_session, _session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, publisher) = make_state(client, session_client);
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
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_replay_determinism() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client);

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
    assert_eq!(session_handles.start_run_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
#[serial_test::serial]
async fn invoke_propagates_thread_id() {
    std::env::set_var("MODEL_GATEWAY_AUTH_DEV_BYPASS", "1");
    std::env::remove_var("ALLOWED_MODELS");
    let client = spawn_mock(MockOk::default()).await;
    let (mock_session, session_handles) = MockSessionCore::new();
    let session_client = spawn_session_mock(mock_session).await;
    let (state, _publisher) = make_state(client, session_client);
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
        *session_handles.start_run_thread_id.lock().unwrap(),
        Some("thread-sk-x".into())
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
    let (state, _publisher) = make_state(client, session_client);
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
    assert_eq!(session_handles.start_run_count.load(Ordering::SeqCst), 1);
    assert_eq!(
        *session_handles.start_run_thread_id.lock().unwrap(),
        Some("thread-existing".into())
    );
}
