//! gRPC server implementing the `InferenceCore` service on :9092.
// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use mp_contracts::model_plane::v1::{
    self as pb,
    inference_core_server::{InferenceCore, InferenceCoreServer},
};
use tonic::{Request, Response, Status};
use tracing::info;

use crate::provider;
use crate::provider::doc_intel::{AnalyzeDocumentRequest, DocIntelChain};
use crate::provider::fallback::FallbackChain;
use crate::provider::language::{
    LanguageAnalyticsChain, LanguageAnalyticsRequest, LanguageOperation,
};
use crate::provider::realtime::{RealtimeChain, RealtimeSessionRequest};
use crate::provider::speech::{
    AudioFormat, SpeechChain, SpeechSynthesisRequest, SpeechTranscriptionRequest,
};
use crate::provider::translation::{
    BatchTranslationRequest, LanguageDetectionRequest, TranslationChain, TranslationInputItem,
    TranslationRequest,
};
use crate::provider::video::{
    VideoChain, VideoContentRequest, VideoGenerationRequest, VideoJobStatusRequest,
};
use crate::provider::vision::{AnalyzeImageRequest, GenerateImageRequest, VisionChain};
use crate::streaming;

const MAX_TTS_TEXT_LEN: usize = 5_000;
const MAX_STT_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const MAX_TRANSLATE_TEXT_LEN: usize = 10_000;
const MAX_DETECT_TEXT_LEN: usize = 1_000;
const MAX_BATCH_TRANSLATE_ITEMS: usize = 100;
const MAX_IMAGE_PROMPT_LEN: usize = 4_000;
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_GENERATED_IMAGES: u32 = 4;
const MAX_DOCUMENT_BYTES: usize = 50 * 1024 * 1024;
const MAX_REALTIME_INSTRUCTIONS_LEN: usize = 8_000;
const MAX_VIDEO_PROMPT_LEN: usize = 4_000;
const MAX_VIDEO_DURATION_SECONDS: u32 = 20;
const MAX_VIDEO_VARIANTS: u32 = 4;

/// Convert an internal [`provider::ModelInfo`] into the wire [`pb::ModelInfo`].
///
/// The proto `ModelInfo` has no dedicated cost field, so the internal `cheap`
/// flag is surfaced as a `"cheap"` entry appended to `features`. The gateway
/// passes `/v1/models` through verbatim, so the SPA can group economy models
/// (and pick a cheap default) by testing `features.includes("cheap")` without
/// any proto change.
impl From<provider::ModelInfo> for pb::ModelInfo {
    fn from(model: provider::ModelInfo) -> Self {
        let mut features = model.features;
        if model.cheap {
            features.push("cheap".to_owned());
        }
        Self {
            id: model.id,
            provider: model.provider,
            modality: model.modality,
            streaming: model.streaming,
            features,
        }
    }
}

pub struct InferenceService {
    chain: FallbackChain,
    speech: SpeechChain,
    translation: TranslationChain,
    vision: VisionChain,
    doc_intel: DocIntelChain,
    language: LanguageAnalyticsChain,
    realtime: RealtimeChain,
    video: VideoChain,
}

#[tonic::async_trait]
impl InferenceCore for InferenceService {
    async fn infer(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<pb::InferResponse>, Status> {
        let (org_id, user_id) = tenant_from_metadata(request.metadata());
        let req = request.into_inner();

        let internal_req = to_internal_request(&req, org_id, user_id);

        let result = self
            .chain
            .infer(&internal_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(pb::InferResponse {
            request_id: result.request_id,
            content: result.content,
            model_used: result.model_used,
            stop_reason: result.stop_reason,
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
            tool_calls: result
                .tool_calls
                .into_iter()
                .map(|tc| pb::ToolCall {
                    id: tc.id,
                    name: tc.name,
                    arguments_json: tc.arguments_json,
                })
                .collect(),
        }))
    }

    type InferStreamStream = tokio_stream::wrappers::ReceiverStream<Result<pb::InferChunk, Status>>;
    type StreamVideoGenerationContentStream = tokio_stream::wrappers::ReceiverStream<
        Result<pb::StreamVideoGenerationContentResponse, Status>,
    >;

    async fn infer_stream(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        let (org_id, user_id) = tenant_from_metadata(request.metadata());
        let req = request.into_inner();

        let internal_req = to_internal_request(&req, org_id, user_id);

        let rx = self
            .chain
            .infer_stream(&internal_req)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let grpc_rx = streaming::bridge_to_grpc(rx);

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            grpc_rx,
        )))
    }

    async fn create_embedding(
        &self,
        request: Request<pb::CreateEmbeddingRequest>,
    ) -> Result<Response<pb::CreateEmbeddingResponse>, Status> {
        let req = request.into_inner();
        // Capture identifiers before they move into the internal request so a
        // failure is never silent (Phase 3 B-spike: the embedding error path had
        // no server-side log, so a provider-hint mismatch surfaced only as an
        // opaque client-side Status).
        let request_id = req.request_id.clone();
        let provider_hint = req.provider_hint.clone();
        let model = req.model.clone();
        let internal_req = provider::EmbedRequest {
            request_id: req.request_id,
            provider_hint: req.provider_hint,
            text: req.text,
            model: req.model,
        };

        let result = self
            .chain
            .create_embedding(&internal_req)
            .await
            .map_err(|e| {
                tracing::error!(
                    request_id = %request_id,
                    provider_hint = %provider_hint,
                    model = %model,
                    error = %e,
                    "create_embedding failed"
                );
                Status::internal(e.to_string())
            })?;

        Ok(Response::new(pb::CreateEmbeddingResponse {
            request_id: result.request_id,
            vector: result.vector,
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn list_models(
        &self,
        request: Request<pb::ListModelsRequest>,
    ) -> Result<Response<pb::ListModelsResponse>, Status> {
        let req = request.into_inner();
        let mut models: Vec<_> = self
            .chain
            .list_models(&req.modality, &req.provider)
            .into_iter()
            .map(pb::ModelInfo::from)
            .collect();
        models.extend(
            self.speech
                .list_models(&req.modality, &req.provider)
                .into_iter()
                .map(pb::ModelInfo::from),
        );
        models.extend(
            self.translation
                .list_models(&req.modality, &req.provider)
                .into_iter()
                .map(pb::ModelInfo::from),
        );
        models.extend(
            self.vision
                .list_models(&req.modality, &req.provider)
                .into_iter()
                .map(pb::ModelInfo::from),
        );
        models.extend(
            self.doc_intel
                .list_models(&req.modality, &req.provider)
                .into_iter()
                .map(pb::ModelInfo::from),
        );
        models.extend(
            self.language
                .list_models(&req.modality, &req.provider)
                .into_iter()
                .map(pb::ModelInfo::from),
        );
        models.extend(
            self.realtime
                .list_models(&req.modality, &req.provider)
                .into_iter()
                .map(pb::ModelInfo::from),
        );
        models.extend(
            self.video
                .list_models(&req.modality, &req.provider)
                .into_iter()
                .map(pb::ModelInfo::from),
        );

        Ok(Response::new(pb::ListModelsResponse { models }))
    }

    async fn synthesize_speech(
        &self,
        request: Request<pb::SynthesizeSpeechRequest>,
    ) -> Result<Response<pb::SynthesizeSpeechResponse>, Status> {
        let req = request.into_inner();
        if req.text.trim().is_empty() {
            return Err(Status::invalid_argument("text is required"));
        }
        if req.text.len() > MAX_TTS_TEXT_LEN {
            return Err(Status::invalid_argument(format!(
                "text exceeds {MAX_TTS_TEXT_LEN}-char cap"
            )));
        }

        let internal_req = SpeechSynthesisRequest {
            request_id: req.request_id.clone(),
            provider_hint: req.provider_hint,
            text: req.text,
            voice: req.voice,
            format: AudioFormat::from_wire(&req.format),
            model: req.model,
            language: req.language,
        };

        let result = self
            .speech
            .synthesize(&internal_req)
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::SynthesizeSpeechResponse {
            request_id: req.request_id,
            audio: result.audio_bytes,
            format: result.format.as_wire().to_owned(),
            model_used: result.model_used,
            provider_used: result.provider_used,
            duration_ms: result.duration_ms,
        }))
    }

    async fn transcribe_speech(
        &self,
        request: Request<pb::TranscribeSpeechRequest>,
    ) -> Result<Response<pb::TranscribeSpeechResponse>, Status> {
        let req = request.into_inner();
        if req.audio.is_empty() {
            return Err(Status::invalid_argument("audio is required"));
        }
        if req.audio.len() > MAX_STT_AUDIO_BYTES {
            return Err(Status::invalid_argument(format!(
                "audio exceeds {}-MB cap",
                MAX_STT_AUDIO_BYTES / (1024 * 1024)
            )));
        }

        let internal_req = SpeechTranscriptionRequest {
            request_id: req.request_id.clone(),
            provider_hint: req.provider_hint,
            audio_bytes: req.audio,
            format: req.format,
            model: req.model,
            language: req.language,
        };

        let result = self
            .speech
            .transcribe(&internal_req)
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::TranscribeSpeechResponse {
            request_id: req.request_id,
            text: result.text,
            detected_language: result.language,
            confidence: result.confidence,
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn list_speech_voices(
        &self,
        request: Request<pb::ListSpeechVoicesRequest>,
    ) -> Result<Response<pb::ListSpeechVoicesResponse>, Status> {
        let req = request.into_inner();
        let voices = self
            .speech
            .list_voices(&req.provider, &req.language)
            .into_iter()
            .map(|voice| pb::SpeechVoiceInfo {
                id: voice.id,
                name: voice.name,
                language: voice.language,
                gender: voice.gender,
                provider: voice.provider,
            })
            .collect();

        Ok(Response::new(pb::ListSpeechVoicesResponse { voices }))
    }

    async fn translate_text(
        &self,
        request: Request<pb::TranslateTextRequest>,
    ) -> Result<Response<pb::TranslateTextResponse>, Status> {
        let req = request.into_inner();
        validate_translate_text(&req.text)?;
        validate_target_language(&req.target_language)?;

        let result = self
            .translation
            .translate(&TranslationRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                text: req.text,
                source_language: req.source_language,
                target_language: req.target_language,
                model: req.model,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::TranslateTextResponse {
            request_id: req.request_id,
            translated_text: result.translated_text,
            detected_language: result.detected_language,
            confidence: result.confidence,
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn batch_translate_text(
        &self,
        request: Request<pb::BatchTranslateTextRequest>,
    ) -> Result<Response<pb::BatchTranslateTextResponse>, Status> {
        let req = request.into_inner();
        validate_target_language(&req.target_language)?;
        if req.items.is_empty() {
            return Err(Status::invalid_argument("items are required"));
        }
        if req.items.len() > MAX_BATCH_TRANSLATE_ITEMS {
            return Err(Status::invalid_argument(format!(
                "items exceed {MAX_BATCH_TRANSLATE_ITEMS}-item cap"
            )));
        }

        let items: Vec<TranslationInputItem> = req
            .items
            .into_iter()
            .map(|item| {
                validate_translate_text(&item.text)?;
                Ok(TranslationInputItem {
                    id: item.id,
                    text: item.text,
                })
            })
            .collect::<Result<_, Status>>()?;

        let result = self
            .translation
            .batch_translate(&BatchTranslationRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                items,
                source_language: req.source_language,
                target_language: req.target_language,
                model: req.model,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::BatchTranslateTextResponse {
            request_id: req.request_id,
            translations: result
                .translations
                .into_iter()
                .map(|item| pb::TranslationResult {
                    id: item.id,
                    original_text: item.original_text,
                    translated_text: item.translated_text,
                    detected_language: item.detected_language,
                    confidence: item.confidence,
                })
                .collect(),
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn detect_text_language(
        &self,
        request: Request<pb::DetectTextLanguageRequest>,
    ) -> Result<Response<pb::DetectTextLanguageResponse>, Status> {
        let req = request.into_inner();
        if req.text.trim().is_empty() {
            return Err(Status::invalid_argument("text is required"));
        }
        if req.text.len() > MAX_DETECT_TEXT_LEN {
            return Err(Status::invalid_argument(format!(
                "text exceeds {MAX_DETECT_TEXT_LEN}-char cap"
            )));
        }

        let result = self
            .translation
            .detect_language(&LanguageDetectionRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                text: req.text,
                model: req.model,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::DetectTextLanguageResponse {
            request_id: req.request_id,
            detections: result
                .detections
                .into_iter()
                .map(|detection| pb::TranslationDetection {
                    language: detection.language,
                    confidence: detection.confidence,
                    is_translation_supported: detection.is_translation_supported,
                })
                .collect(),
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn list_translation_languages(
        &self,
        request: Request<pb::ListTranslationLanguagesRequest>,
    ) -> Result<Response<pb::ListTranslationLanguagesResponse>, Status> {
        let req = request.into_inner();
        let languages = self
            .translation
            .list_languages(&req.provider)
            .await
            .into_iter()
            .map(|language| pb::TranslationLanguageInfo {
                code: language.code,
                name: language.name,
                native_name: language.native_name,
                direction: language.direction,
            })
            .collect();

        Ok(Response::new(pb::ListTranslationLanguagesResponse {
            languages,
        }))
    }

    async fn generate_image(
        &self,
        request: Request<pb::GenerateImageRequest>,
    ) -> Result<Response<pb::GenerateImageResponse>, Status> {
        let req = request.into_inner();
        validate_image_prompt(&req.prompt)?;
        if req.n > MAX_GENERATED_IMAGES {
            return Err(Status::invalid_argument(format!(
                "n exceeds {MAX_GENERATED_IMAGES}-image cap"
            )));
        }

        let result = self
            .vision
            .generate_image(&GenerateImageRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                prompt: req.prompt,
                model: req.model,
                size: req.size,
                quality: req.quality,
                n: req.n.max(1),
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::GenerateImageResponse {
            request_id: req.request_id,
            images: result
                .images
                .into_iter()
                .map(|image| pb::GeneratedImage {
                    url: image.url,
                    b64_json: image.b64_json,
                    revised_prompt: image.revised_prompt,
                })
                .collect(),
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn analyze_image(
        &self,
        request: Request<pb::AnalyzeImageRequest>,
    ) -> Result<Response<pb::AnalyzeImageResponse>, Status> {
        let req = request.into_inner();
        validate_image_prompt(&req.prompt)?;
        validate_image_input(&req.image_url, req.image_data.len())?;

        let result = self
            .vision
            .analyze_image(&AnalyzeImageRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                image_url: req.image_url,
                image_data: req.image_data,
                mime_type: req.mime_type,
                prompt: req.prompt,
                model: req.model,
                max_tokens: req.max_tokens,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::AnalyzeImageResponse {
            request_id: req.request_id,
            description: result.description,
            model_used: result.model_used,
            provider_used: result.provider_used,
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
        }))
    }

    async fn extract_image_text(
        &self,
        request: Request<pb::ExtractImageTextRequest>,
    ) -> Result<Response<pb::ExtractImageTextResponse>, Status> {
        let req = request.into_inner();
        validate_image_input(&req.image_url, req.image_data.len())?;

        let result = self
            .vision
            .extract_text(&AnalyzeImageRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                image_url: req.image_url,
                image_data: req.image_data,
                mime_type: req.mime_type,
                prompt: String::new(),
                model: req.model,
                max_tokens: 2_048,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::ExtractImageTextResponse {
            request_id: req.request_id,
            text: result.text,
            page_count: result.page_count,
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn analyze_document(
        &self,
        request: Request<pb::AnalyzeDocumentRequest>,
    ) -> Result<Response<pb::AnalyzeDocumentResponse>, Status> {
        let req = request.into_inner();
        validate_document_input(&req.document_url, req.document_data.len())?;

        let result = self
            .doc_intel
            .analyze_document(&AnalyzeDocumentRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                document_url: req.document_url,
                document_data: req.document_data,
                model: req.model,
                pages: req.pages,
                locale: req.locale,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::AnalyzeDocumentResponse {
            request_id: req.request_id,
            status: result.status,
            content: result.content,
            fields_json: result.fields_json,
            tables_json: result.tables_json,
            paragraphs: result.paragraphs,
            raw_json: result.raw_json,
            pages_processed: result.pages_processed,
            confidence: result.confidence,
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn analyze_language(
        &self,
        request: Request<pb::AnalyzeLanguageRequest>,
    ) -> Result<Response<pb::AnalyzeLanguageResponse>, Status> {
        let req = request.into_inner();
        validate_language_texts(&req.texts)?;
        let operation =
            LanguageOperation::from_wire(&req.operation).map_err(provider_error_to_status)?;

        let result = self
            .language
            .analyze(&LanguageAnalyticsRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                operation,
                texts: req.texts,
                language: req.language,
                model: req.model,
                sentence_count: req.sentence_count,
                summary_kind: req.summary_kind,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::AnalyzeLanguageResponse {
            request_id: req.request_id,
            operation: result.operation.as_wire().to_owned(),
            results: result
                .results
                .into_iter()
                .map(|item| pb::LanguageAnalysisResult {
                    id: item.id,
                    sentiment: item.sentiment,
                    confidence_scores_json: item.confidence_scores_json,
                    sentences_json: item.sentences_json,
                    entities_json: item.entities_json,
                    key_phrases: item.key_phrases,
                    redacted_text: item.redacted_text,
                    detected_language_name: item.detected_language_name,
                    detected_language_code: item.detected_language_code,
                    confidence: item.confidence,
                    summary: item.summary,
                    raw_json: item.raw_json,
                    content_safety_json: item.content_safety_json,
                })
                .collect(),
            model_used: result.model_used,
            provider_used: result.provider_used,
        }))
    }

    async fn create_realtime_session(
        &self,
        request: Request<pb::CreateRealtimeSessionRequest>,
    ) -> Result<Response<pb::CreateRealtimeSessionResponse>, Status> {
        let req = request.into_inner();
        validate_realtime_session(&req)?;

        let result = self
            .realtime
            .create_session(&RealtimeSessionRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                model: req.model,
                voice: req.voice,
                instructions: req.instructions,
                input_audio_format: req.input_audio_format,
                output_audio_format: req.output_audio_format,
                turn_detection_type: req.turn_detection_type,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::CreateRealtimeSessionResponse {
            request_id: result.request_id,
            session_id: result.session_id,
            client_secret: result.client_secret,
            websocket_url: result.websocket_url,
            expires_at: result.expires_at,
            model_used: result.model_used,
            provider_used: result.provider_used,
            voice: result.voice,
        }))
    }

    async fn create_video_generation_job(
        &self,
        request: Request<pb::CreateVideoGenerationJobRequest>,
    ) -> Result<Response<pb::CreateVideoGenerationJobResponse>, Status> {
        let req = request.into_inner();
        validate_video_generation(&req)?;

        let result = self
            .video
            .create_job(&VideoGenerationRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                prompt: req.prompt,
                width: req.width,
                height: req.height,
                duration_seconds: req.duration_seconds,
                n_variants: req.n_variants,
                model: req.model,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::CreateVideoGenerationJobResponse {
            request_id: result.request_id,
            job_id: result.job_id,
            status: result.status,
            model_used: result.model_used,
            provider_used: result.provider_used,
            raw_json: result.raw_json,
        }))
    }

    async fn get_video_generation_job(
        &self,
        request: Request<pb::GetVideoGenerationJobRequest>,
    ) -> Result<Response<pb::GetVideoGenerationJobResponse>, Status> {
        let req = request.into_inner();
        if req.job_id.trim().is_empty() {
            return Err(Status::invalid_argument("job_id is required"));
        }

        let result = self
            .video
            .get_job_status(&VideoJobStatusRequest {
                request_id: req.request_id.clone(),
                provider_hint: req.provider_hint,
                job_id: req.job_id,
                model: req.model,
            })
            .await
            .map_err(provider_error_to_status)?;

        Ok(Response::new(pb::GetVideoGenerationJobResponse {
            request_id: result.request_id,
            job_id: result.job_id,
            status: result.status,
            generation_id: result.generation_id,
            video_url: result.video_url,
            error: result.error,
            model_used: result.model_used,
            provider_used: result.provider_used,
            raw_json: result.raw_json,
        }))
    }

    async fn stream_video_generation_content(
        &self,
        request: Request<pb::StreamVideoGenerationContentRequest>,
    ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
        let req = request.into_inner();
        if req.generation_id.trim().is_empty() {
            return Err(Status::invalid_argument("generation_id is required"));
        }

        let rx = self
            .video
            .stream_generation_content(&VideoContentRequest {
                request_id: req.request_id,
                provider_hint: req.provider_hint,
                generation_id: req.generation_id,
                model: req.model,
            })
            .await
            .map_err(provider_error_to_status)?;

        let (tx, grpc_rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(chunk) = rx.recv().await {
                let result = chunk
                    .map(|chunk| pb::StreamVideoGenerationContentResponse {
                        request_id: chunk.request_id,
                        generation_id: chunk.generation_id,
                        data: chunk.data,
                        done: chunk.done,
                        content_type: chunk.content_type,
                        content_length: chunk.content_length,
                        provider_used: chunk.provider_used,
                    })
                    .map_err(provider_error_to_status);
                if tx.send(result).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            grpc_rx,
        )))
    }
}

/// Extract the tenant scope (org id, user id) from gRPC request metadata for
/// the Velion intent layer's budget check. The gateway forwards these as
/// `x-org-id` / `x-user-id`; both default to empty when absent (the budget gate
/// then degrades to an `Unknown` posture — see `provider::intent`).
fn tenant_from_metadata(md: &tonic::metadata::MetadataMap) -> (String, String) {
    let get = |keys: &[&str]| -> String {
        for key in keys {
            if let Some(value) = md.get(*key).and_then(|v| v.to_str().ok()) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_owned();
                }
            }
        }
        String::new()
    };
    let org_id = get(&["x-org-id", "x-velion-org-id", "organization-id"]);
    let user_id = get(&["x-user-id", "x-velion-user-id"]);
    (org_id, user_id)
}

/// Convert a proto `InferRequest` to an internal `InferRequest`. `org_id`/
/// `user_id` come from gRPC metadata (see `tenant_from_metadata`).
fn to_internal_request(
    req: &pb::InferRequest,
    org_id: String,
    user_id: String,
) -> provider::InferRequest {
    let messages = req
        .messages
        .iter()
        .map(|m| provider::ChatMessage {
            role: m.role.clone(),
            content: m.content.clone(),
            name: m.name.clone(),
        })
        .collect();

    let tools = req
        .tools
        .iter()
        .map(|t| provider::ToolDefinition {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters_json: t.parameters_json.clone(),
        })
        .collect();

    provider::InferRequest {
        request_id: req.request_id.clone(),
        provider_hint: req.provider_hint.clone(),
        model: req.model.clone(),
        messages,
        temperature: req.temperature,
        max_tokens: req.max_tokens,
        structured_output_schema: if req.structured_output_schema.is_empty() {
            None
        } else {
            Some(req.structured_output_schema.clone())
        },
        zdr: req.zdr,
        tools,
        tool_choice: req.tool_choice.clone(),
        // Prefer the gateway's JWT-derived body `org_id` (not client-spoofable);
        // fall back to gRPC metadata for direct callers that don't set it. The
        // proto carries no user_id, so the budget check's user scope comes from
        // metadata only (empty → cost-core's org-wide "__org__" key).
        org_id: if req.org_id.trim().is_empty() {
            org_id
        } else {
            req.org_id.clone()
        },
        user_id,
    }
}

/// Bundle of every provider chain required to construct an [`InferenceService`].
pub struct ProviderChains {
    pub chain: FallbackChain,
    pub speech: SpeechChain,
    pub translation: TranslationChain,
    pub vision: VisionChain,
    pub doc_intel: DocIntelChain,
    pub language: LanguageAnalyticsChain,
    pub realtime: RealtimeChain,
    pub video: VideoChain,
}

/// Start the gRPC server with explicit provider chains.
///
/// # Errors
///
/// Returns an error if the server fails to bind.
pub async fn serve_with_providers(chains: ProviderChains) -> anyhow::Result<()> {
    let addr = "0.0.0.0:9092".parse()?;
    info!("gRPC listening on :9092");

    let ProviderChains {
        chain,
        speech,
        translation,
        vision,
        doc_intel,
        language,
        realtime,
        video,
    } = chains;

    tonic::transport::Server::builder()
        .add_service(InferenceCoreServer::new(InferenceService {
            chain,
            speech,
            translation,
            vision,
            doc_intel,
            language,
            realtime,
            video,
        }))
        .serve(addr)
        .await?;

    Ok(())
}

fn validate_translate_text(text: &str) -> Result<(), Status> {
    if text.trim().is_empty() {
        return Err(Status::invalid_argument("text is required"));
    }
    if text.len() > MAX_TRANSLATE_TEXT_LEN {
        return Err(Status::invalid_argument(format!(
            "text exceeds {MAX_TRANSLATE_TEXT_LEN}-char cap"
        )));
    }
    Ok(())
}

fn validate_target_language(target_language: &str) -> Result<(), Status> {
    if target_language.trim().is_empty() {
        return Err(Status::invalid_argument("target_language is required"));
    }
    Ok(())
}

fn validate_image_prompt(prompt: &str) -> Result<(), Status> {
    if prompt.trim().is_empty() {
        return Err(Status::invalid_argument("prompt is required"));
    }
    if prompt.len() > MAX_IMAGE_PROMPT_LEN {
        return Err(Status::invalid_argument(format!(
            "prompt exceeds {MAX_IMAGE_PROMPT_LEN}-char cap"
        )));
    }
    Ok(())
}

fn validate_image_input(image_url: &str, image_bytes_len: usize) -> Result<(), Status> {
    let has_url = !image_url.trim().is_empty();
    let has_bytes = image_bytes_len > 0;
    if has_url == has_bytes {
        return Err(Status::invalid_argument(
            "exactly one of image_url or image_data is required",
        ));
    }
    if image_bytes_len > MAX_IMAGE_BYTES {
        return Err(Status::invalid_argument(format!(
            "image_data exceeds {}-MB cap",
            MAX_IMAGE_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

fn validate_document_input(document_url: &str, document_bytes_len: usize) -> Result<(), Status> {
    let has_url = !document_url.trim().is_empty();
    let has_bytes = document_bytes_len > 0;
    if has_url == has_bytes {
        return Err(Status::invalid_argument(
            "exactly one of document_url or document_data is required",
        ));
    }
    if document_bytes_len > MAX_DOCUMENT_BYTES {
        return Err(Status::invalid_argument(format!(
            "document_data exceeds {}-MB cap",
            MAX_DOCUMENT_BYTES / (1024 * 1024)
        )));
    }
    Ok(())
}

fn validate_language_texts(texts: &[String]) -> Result<(), Status> {
    if texts.is_empty() {
        return Err(Status::invalid_argument("texts are required"));
    }
    if texts.len() > 25 {
        return Err(Status::invalid_argument("texts exceed 25-item cap"));
    }
    if texts.iter().any(|text| text.trim().is_empty()) {
        return Err(Status::invalid_argument(
            "texts must not contain empty items",
        ));
    }
    if texts.iter().map(String::len).sum::<usize>() > 100_000 {
        return Err(Status::invalid_argument(
            "texts exceed 100000-char total cap",
        ));
    }
    Ok(())
}

fn validate_realtime_session(req: &pb::CreateRealtimeSessionRequest) -> Result<(), Status> {
    if req.instructions.len() > MAX_REALTIME_INSTRUCTIONS_LEN {
        return Err(Status::invalid_argument(format!(
            "instructions exceed {MAX_REALTIME_INSTRUCTIONS_LEN}-char cap"
        )));
    }
    if req.voice.len() > 64 {
        return Err(Status::invalid_argument("voice exceeds 64-char cap"));
    }
    if req.input_audio_format.len() > 64 || req.output_audio_format.len() > 64 {
        return Err(Status::invalid_argument("audio format exceeds 64-char cap"));
    }
    if req.turn_detection_type.len() > 64 {
        return Err(Status::invalid_argument(
            "turn_detection_type exceeds 64-char cap",
        ));
    }
    Ok(())
}

fn validate_video_generation(req: &pb::CreateVideoGenerationJobRequest) -> Result<(), Status> {
    if req.prompt.trim().is_empty() {
        return Err(Status::invalid_argument("prompt is required"));
    }
    if req.prompt.len() > MAX_VIDEO_PROMPT_LEN {
        return Err(Status::invalid_argument(format!(
            "prompt exceeds {MAX_VIDEO_PROMPT_LEN}-char cap"
        )));
    }
    if req.duration_seconds > MAX_VIDEO_DURATION_SECONDS {
        return Err(Status::invalid_argument(format!(
            "duration_seconds exceeds {MAX_VIDEO_DURATION_SECONDS}-second cap"
        )));
    }
    if req.n_variants > MAX_VIDEO_VARIANTS {
        return Err(Status::invalid_argument(format!(
            "n_variants exceeds {MAX_VIDEO_VARIANTS}-variant cap"
        )));
    }
    if req.width > 0 && !req.width.is_multiple_of(8) {
        return Err(Status::invalid_argument("width must be divisible by 8"));
    }
    if req.height > 0 && !req.height.is_multiple_of(8) {
        return Err(Status::invalid_argument("height must be divisible by 8"));
    }
    Ok(())
}

fn provider_error_to_status(error: provider::ProviderError) -> Status {
    match error {
        provider::ProviderError::InvalidResponse(message) => Status::invalid_argument(message),
        provider::ProviderError::UnsupportedModel(message) => Status::unimplemented(message),
        provider::ProviderError::Unavailable(message) | provider::ProviderError::Http(message) => {
            Status::unavailable(message)
        }
        provider::ProviderError::AllExhausted { attempts } => {
            Status::unavailable(format!("all providers exhausted after {attempts} attempts"))
        }
        provider::ProviderError::RateLimited { retry_after_ms } => {
            Status::resource_exhausted(format!("rate limited: retry after {retry_after_ms}ms"))
        }
    }
}
