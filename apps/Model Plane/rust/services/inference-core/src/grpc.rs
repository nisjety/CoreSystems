//! gRPC server implementing the `InferenceCore` service on :9092.
// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use mp_contracts::model_plane::v1::{
    self as pb,
    inference_core_server::{InferenceCore, InferenceCoreServer},
};
use tonic::{Request, Response, Status};
use tracing::info;

use crate::auth::{AuthenticatedPrincipal, JwtVerifier};
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
use crate::provider::timing::HandlerClock;
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
            // Venice-style per-model privacy disclosure. The residency label
            // stays empty for an undeclared (Global) provider so "no commitment
            // claimed" never renders as a real guarantee.
            privacy_tier: model.privacy_tier.as_wire_i32(),
            residency: model.residency_label,
        }
    }
}

/// Map a wire `PrivacyTier` onto the internal enum. Unknown numerics fail
/// closed with `invalid_argument`: a NEWER client naming a tier this build
/// does not know must never be honored as if it had asked for nothing.
fn tier_from_wire(value: i32) -> Result<provider::PrivacyTier, Status> {
    provider::PrivacyTier::from_wire(value)
        .ok_or_else(|| Status::invalid_argument(format!("unknown privacy tier value: {value}")))
}

pub(crate) struct InferenceService {
    auth: JwtVerifier,
    chain: FallbackChain,
    speech: SpeechChain,
    translation: TranslationChain,
    vision: VisionChain,
    doc_intel: DocIntelChain,
    language: LanguageAnalyticsChain,
    realtime: RealtimeChain,
    video: VideoChain,
}

trait TenantRequest {
    fn org_id(&self) -> Option<&str>;
    fn is_catalog(&self) -> bool;
}

macro_rules! tenant_requests {
    ($($request:ty),+ $(,)?) => {
        $(impl TenantRequest for $request {
            fn org_id(&self) -> Option<&str> {
                Some(&self.org_id)
            }

            fn is_catalog(&self) -> bool {
                false
            }
        })+
    };
}

tenant_requests!(
    pb::InferRequest,
    pb::CreateEmbeddingRequest,
    pb::SynthesizeSpeechRequest,
    pb::TranscribeSpeechRequest,
    pb::TranslateTextRequest,
    pb::BatchTranslateTextRequest,
    pb::DetectTextLanguageRequest,
    pb::GenerateImageRequest,
    pb::AnalyzeImageRequest,
    pb::ExtractImageTextRequest,
    pb::AnalyzeDocumentRequest,
    pb::AnalyzeLanguageRequest,
    pb::CreateRealtimeSessionRequest,
    pb::CreateVideoGenerationJobRequest,
    pb::GetVideoGenerationJobRequest,
    pb::StreamVideoGenerationContentRequest,
);

macro_rules! catalog_requests {
    ($($request:ty),+ $(,)?) => {
        $(impl TenantRequest for $request {
            fn org_id(&self) -> Option<&str> {
                None
            }

            fn is_catalog(&self) -> bool {
                true
            }
        })+
    };
}

catalog_requests!(
    pb::ListModelsRequest,
    pb::ListSpeechVoicesRequest,
    pb::ListTranslationLanguagesRequest,
);

impl InferenceService {
    async fn authorize<T: TenantRequest>(
        &self,
        request: &Request<T>,
    ) -> Result<AuthenticatedPrincipal, Status> {
        let principal = self.auth.authenticate(request).await?;
        if request.get_ref().is_catalog() {
            principal.authorize_catalog()?;
        } else {
            principal.authorize_invoke()?;
        }
        if let Some(org_id) = request.get_ref().org_id() {
            principal.authorize_org(org_id)?;
        }
        Ok(principal)
    }
}

/// Refuse a modality with no independently attested zero-retention deployment
/// whenever zero retention applies: through the caller token's own claim, or
/// through `request_zdr`, the request's own flag. Contracts that carry no such
/// flag pass `false`; the token claim still applies to them.
fn reject_unattested_zdr_modality(
    principal: &AuthenticatedPrincipal,
    request_zdr: bool,
    operation: &'static str,
) -> Result<(), Status> {
    if principal.effective_zdr(request_zdr) {
        return Err(Status::failed_precondition(format!(
            "ZDR {operation} is unavailable because no provider deployment is independently attested"
        )));
    }
    Ok(())
}

/// `SynthesizeSpeech` for an authorized caller. A ZDR request, by token or by
/// its own flag, is refused before any provider sees the text.
async fn synthesize_speech_as(
    speech: &SpeechChain,
    principal: &AuthenticatedPrincipal,
    req: pb::SynthesizeSpeechRequest,
) -> Result<pb::SynthesizeSpeechResponse, Status> {
    reject_unattested_zdr_modality(principal, req.zdr, "speech synthesis")?;
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

    let result = speech
        .synthesize(&internal_req)
        .await
        .map_err(provider_error_to_status)?;

    Ok(pb::SynthesizeSpeechResponse {
        request_id: req.request_id,
        audio: result.audio_bytes,
        format: result.format.as_wire().to_owned(),
        model_used: result.model_used,
        provider_used: result.provider_used,
        duration_ms: result.duration_ms,
    })
}

/// `TranscribeSpeech` for an authorized caller. A ZDR request, by token or by
/// its own flag, is refused before any provider sees the audio.
async fn transcribe_speech_as(
    speech: &SpeechChain,
    principal: &AuthenticatedPrincipal,
    req: pb::TranscribeSpeechRequest,
) -> Result<pb::TranscribeSpeechResponse, Status> {
    reject_unattested_zdr_modality(principal, req.zdr, "speech transcription")?;
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

    let result = speech
        .transcribe(&internal_req)
        .await
        .map_err(provider_error_to_status)?;

    Ok(pb::TranscribeSpeechResponse {
        request_id: req.request_id,
        text: result.text,
        detected_language: result.language,
        confidence: result.confidence,
        model_used: result.model_used,
        provider_used: result.provider_used,
    })
}

#[tonic::async_trait]
impl InferenceCore for InferenceService {
    async fn infer(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<pb::InferResponse>, Status> {
        // Read before authorization, so the reported handler time covers it.
        let mut clock = HandlerClock::start();
        let principal = self.authorize(&request).await?;
        let no_store = request.metadata().get("cache-control").is_some_and(|value| value == "no-store");
        let req = request.into_inner();

        let internal_req = to_internal_request(&req, &principal)?;

        clock.enter_chain();
        let result = if no_store {
            self.chain.infer_without_response_cache(&internal_req).await
        } else {
            self.chain.infer(&internal_req).await
        }.map_err(provider_error_to_status)?;

        let mut response = Response::new(infer_response_to_wire(result, &clock));
        if no_store { response.metadata_mut().insert("cache-control", "no-store".parse().expect("static metadata")); }
        Ok(response)
    }

    type InferStreamStream = tokio_stream::wrappers::ReceiverStream<Result<pb::InferChunk, Status>>;
    type StreamVideoGenerationContentStream = tokio_stream::wrappers::ReceiverStream<
        Result<pb::StreamVideoGenerationContentResponse, Status>,
    >;

    async fn infer_stream(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        // Read before authorization, so the reported handler time covers it.
        let mut clock = HandlerClock::start();
        let principal = self.authorize(&request).await?;
        let req = request.into_inner();

        let internal_req = to_internal_request(&req, &principal)?;

        clock.enter_chain();
        let rx = self
            .chain
            .infer_stream(&internal_req)
            .await
            .map_err(provider_error_to_status)?;

        let grpc_rx = streaming::bridge_to_grpc(rx, clock);

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            grpc_rx,
        )))
    }

    async fn create_embedding(
        &self,
        request: Request<pb::CreateEmbeddingRequest>,
    ) -> Result<Response<pb::CreateEmbeddingResponse>, Status> {
        let principal = self.authorize(&request).await?;
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
            // Carry the ZDR signal through; inference-core's provider is Azure
            // today so residency enforcement is Phase-4 — this just threads it
            // so a future EU/ZDR provider can honor it.
            zdr: principal.effective_zdr(req.zdr),
            // Requested residency region. The fallback chain enforces the EU
            // residency gate (deny-by-default) before any network call.
            region: req.region,
            // The embedding contract has no tier field yet; embeddings are
            // served from the same EU-attested Azure deployment today, so no
            // constraint is expressed until that contract grows one.
            min_privacy_tier: provider::PrivacyTier::Unspecified,
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
                // A residency rejection is a precondition the caller can act on
                // (request an EU region / opt in), not an internal fault — map
                // it to FAILED_PRECONDITION rather than INTERNAL.
                match e {
                    provider::ProviderError::ResidencyViolation(msg) => {
                        Status::failed_precondition(msg)
                    }
                    other => Status::internal(other.to_string()),
                }
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
        self.authorize(&request).await?;
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

        // Several providers enumerate the same Azure deployment under different
        // modalities — the chat chain lists `model-router`/`gpt-4o-mini` as
        // `chat` (with full features), while the translation/language/vision/OCR
        // providers re-list those same chat deployments as `translation`,
        // `language_analytics`, `vision`, `ocr` (LLM-prompted fallbacks, with no
        // features). That surfaces the same id several times in the catalog. The
        // SPA fetches this list unfiltered and filters modality client-side, so
        // collapse to one entry per id, keeping the first-seen — the chat chain
        // is aggregated first, so each model keeps its primary (chat) entry with
        // the richest metadata. Dedicated single-modality deployments
        // (gpt-image-1, sora, whisper, azure-translator-v3, prebuilt-*) have
        // distinct ids and are unaffected.
        let mut seen = std::collections::HashSet::new();
        models.retain(|model| seen.insert(model.id.clone()));

        Ok(Response::new(pb::ListModelsResponse { models }))
    }

    async fn synthesize_speech(
        &self,
        request: Request<pb::SynthesizeSpeechRequest>,
    ) -> Result<Response<pb::SynthesizeSpeechResponse>, Status> {
        let principal = self.authorize(&request).await?;
        synthesize_speech_as(&self.speech, &principal, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn transcribe_speech(
        &self,
        request: Request<pb::TranscribeSpeechRequest>,
    ) -> Result<Response<pb::TranscribeSpeechResponse>, Status> {
        let principal = self.authorize(&request).await?;
        transcribe_speech_as(&self.speech, &principal, request.into_inner())
            .await
            .map(Response::new)
    }

    async fn list_speech_voices(
        &self,
        request: Request<pb::ListSpeechVoicesRequest>,
    ) -> Result<Response<pb::ListSpeechVoicesResponse>, Status> {
        self.authorize(&request).await?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "translation")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "translation")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "language detection")?;
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
        self.authorize(&request).await?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "image generation")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "image analysis")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "image text extraction")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "document analysis")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "language analysis")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "realtime session")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "video generation")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "video status")?;
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
        let principal = self.authorize(&request).await?;
        reject_unattested_zdr_modality(&principal, false, "video content")?;
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

/// Project a unary chain result onto the wire. `clock` is the handler's own:
/// the chain's timing is re-anchored on the handler's entry and its `handler`
/// interval closed here, as the response is built.
fn infer_response_to_wire(
    result: provider::InferResponse,
    clock: &HandlerClock,
) -> pb::InferResponse {
    pb::InferResponse {
        request_id: result.request_id,
        content: result.content,
        model_used: result.model_used,
        stop_reason: result.stop_reason,
        input_tokens: result.input_tokens,
        output_tokens: result.output_tokens,
        provider_used: result.provider_used,
        residency: result.residency,
        token_confidence: result.token_confidence.map(Into::into),
        cache_read_input_tokens: result.cache_read_input_tokens,
        cache_creation_input_tokens: result.cache_creation_input_tokens,
        compaction_summary: result.compaction_summary,
        provider_timing: result.provider_timing.map(|mut timing| {
            clock.stamp(&mut timing);
            timing.into()
        }),
        tool_calls: result
            .tool_calls
            .into_iter()
            .map(|tc| pb::ToolCall {
                id: tc.id,
                name: tc.name,
                arguments_json: tc.arguments_json,
            })
            .collect(),
    }
}

/// Convert the public contract to the provider contract. Tenant, user, and
/// issuer-enforced ZDR posture come only from the verified principal.
///
/// # Errors
/// Returns `invalid_argument` when the caller names a privacy tier this build
/// cannot interpret.
fn to_internal_request(
    req: &pb::InferRequest,
    principal: &AuthenticatedPrincipal,
) -> Result<provider::InferRequest, Status> {
    let messages = req
        .messages
        .iter()
        .map(|m| provider::ChatMessage {
            compaction_summary: m.compaction_summary.clone(),
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

    Ok(provider::InferRequest {
        thinking_budget_tokens: req.thinking_budget_tokens,
        prefer_priority_service_tier: req.prefer_priority_service_tier,
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
        zdr: principal.effective_zdr(req.zdr),
        min_privacy_tier: tier_from_wire(req.min_privacy_tier)?,
        tools,
        tool_choice: req.tool_choice.clone(),
        min_residency: req.min_residency.clone(),
        subscription_connection_id: req.subscription_connection_id.clone(),
        org_id: principal.org_id.clone(),
        user_id: principal.budget_user_id(),
        // The verified caller's own token, forwarded so the intent layer's
        // cost-core budget check authenticates as the caller (cost-core pins
        // org/user to the token's claims). Never serialized into provider
        // bodies — see the field's doc comment.
        caller_bearer: crate::provider::Bearer::new(principal.bearer()),
    })
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

/// Canonical service name for the standard gRPC health check. Both this name
/// and the empty overall-server name are public transport probes; every
/// inference RPC remains authenticated in the service implementation.
pub const GRPC_HEALTH_SERVICE_NAME: &str = "model_plane.v1.InferenceCore";

/// Start the authenticated gRPC server with an additive standard health
/// service. Auth configuration and the initial JWKS have already succeeded
/// before this function is called, so `SERVING` never masks an unverifiable
/// listener.
///
/// # Errors
/// Returns an error if the server address cannot be parsed or served.
pub async fn serve_with_providers(chains: ProviderChains, auth: JwtVerifier) -> anyhow::Result<()> {
    serve_with_providers_and_readiness(chains, auth, crate::readiness::GrpcReadiness::new()).await
}

/// Start the authenticated gRPC server and mark the HTTP readiness gate ready
/// only after the listener owns its socket.
///
/// # Errors
///
/// Returns an error when the listener address cannot be parsed, its socket
/// cannot be bound, or the authenticated gRPC server fails while serving.
pub async fn serve_with_providers_and_readiness(
    chains: ProviderChains,
    auth: JwtVerifier,
    readiness: crate::readiness::GrpcReadiness,
) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = "0.0.0.0:9092".parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
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
    let service = InferenceService {
        auth,
        chain,
        speech,
        translation,
        vision,
        doc_intel,
        language,
        realtime,
        video,
    };
    let grpc_service = InferenceCoreServer::new(service);
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<InferenceCoreServer<InferenceService>>()
        .await;

    readiness.mark_bound();
    info!(
        address = %addr,
        health_service = GRPC_HEALTH_SERVICE_NAME,
        "authenticated gRPC listening"
    );
    let result = tonic::transport::Server::builder()
        .add_service(health_service)
        .add_service(grpc_service)
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
        .await;
    readiness.mark_unbound();
    result?;
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

/// gRPC trailer key carrying inference-core's own classification of a provider
/// failure, so downstream services read a type instead of matching prose.
///
/// Additive by design: the Status message still holds the provider's original
/// text, so a caller that has not adopted this key behaves exactly as before.
pub const PROVIDER_ERROR_KIND_TRAILER: &str = "x-mp-provider-error";

/// Value of [`PROVIDER_ERROR_KIND_TRAILER`] for a prompt over the input limit.
pub const PROVIDER_ERROR_TOO_LONG: &str = "too_long";

/// Value of [`PROVIDER_ERROR_KIND_TRAILER`] for a throttled provider. The
/// gateway honours it on the subscription route only.
pub const PROVIDER_ERROR_RATE_LIMITED: &str = "rate_limited";

/// Trailer carrying how long to wait before a retry could succeed, as a whole
/// number of milliseconds. Sent only when the provider said.
pub const RETRY_AFTER_MS_TRAILER: &str = "x-mp-retry-after-ms";

/// Trailer carrying when a subscription's usage limit resets: RFC 3339 UTC to
/// whole seconds. Sent only for a usage limit whose reset is known.
pub const RESETS_AT_TRAILER: &str = "x-mp-resets-at";

/// Add one typed trailer. Every value here is a static code, a number or a
/// formatted timestamp, all valid ASCII; if one ever were not, that signal is
/// simply absent and the status code still applies.
fn insert_trailer(metadata: &mut tonic::metadata::MetadataMap, key: &'static str, value: &str) {
    match value.parse() {
        Ok(value) => {
            metadata.insert(key, value);
        }
        Err(_) => tracing::error!(key, "typed provider error value is not a valid trailer value"),
    }
}

fn too_long_status(detail: &str) -> Status {
    let mut metadata = tonic::metadata::MetadataMap::new();
    insert_trailer(&mut metadata, PROVIDER_ERROR_KIND_TRAILER, PROVIDER_ERROR_TOO_LONG);
    Status::with_metadata(tonic::Code::Unavailable, detail.to_owned(), metadata)
}

/// A typed provider failure: its code, a content-free message, and the kind
/// plus any retry timing as trailers.
fn typed_status(
    code: tonic::Code,
    message: String,
    kind: &str,
    retry_after_ms: Option<u64>,
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Status {
    let mut metadata = tonic::metadata::MetadataMap::new();
    insert_trailer(&mut metadata, PROVIDER_ERROR_KIND_TRAILER, kind);
    if let Some(retry_after_ms) = retry_after_ms {
        insert_trailer(&mut metadata, RETRY_AFTER_MS_TRAILER, &retry_after_ms.to_string());
    }
    if let Some(resets_at) = resets_at {
        insert_trailer(
            &mut metadata,
            RESETS_AT_TRAILER,
            &resets_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
    }
    Status::with_metadata(code, message, metadata)
}

/// A ChatGPT-subscription refusal on the wire.
///
/// Only the usage limit is `ResourceExhausted`. Every other reason stays
/// `Unavailable` on purpose: the gateway reads `Unauthenticated` as a 401 and a
/// confirmed dispatch rejection, and `FailedPrecondition` as a failed check or
/// the ZDR gate, so any consumer that does not read the trailer keeps today's
/// "the service did not answer". The message is the code alone.
fn subscription_refused_status(
    reason: provider::SubscriptionRefusal,
    retry_after_ms: Option<u64>,
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Status {
    let code = match reason {
        provider::SubscriptionRefusal::UsageLimit => tonic::Code::ResourceExhausted,
        provider::SubscriptionRefusal::ReauthRequired
        | provider::SubscriptionRefusal::UpstreamUnavailable
        | provider::SubscriptionRefusal::BadRequest => tonic::Code::Unavailable,
    };
    typed_status(
        code,
        format!("subscription refused: {}", reason.code()),
        reason.code(),
        retry_after_ms,
        resets_at,
    )
}

fn provider_error_to_status(error: provider::ProviderError) -> Status {
    match error {
        provider::ProviderError::InvalidResponse(message) => Status::invalid_argument(message),
        provider::ProviderError::UnsupportedModel(message) => Status::unimplemented(message),
        // A broker failure the adapter could not type keeps the plain
        // `Unavailable` it always had; only the chain treats it differently.
        provider::ProviderError::Unavailable(message)
        | provider::ProviderError::SubscriptionFailed(message)
        | provider::ProviderError::Http(message) => Status::unavailable(message),
        // Code stays `unavailable`, deliberately. An overflow error is
        // retryable in a way an invalid argument is not: FallbackChain can move
        // the request to a larger-context provider, and a prompt too long for
        // an 8k model may well fit a 200k one. Re-coding this to
        // `invalid_argument` would silently remove that recovery path. What is
        // added is the machine-readable type, so a caller no longer has to
        // re-derive it from provider prose.
        provider::ProviderError::TooLong { detail } => too_long_status(&detail),
        provider::ProviderError::AllExhausted { attempts } => {
            Status::unavailable(format!("all providers exhausted after {attempts} attempts"))
        }
        provider::ProviderError::RateLimited { retry_after_ms } => typed_status(
            tonic::Code::ResourceExhausted,
            format!("rate limited: retry after {retry_after_ms}ms"),
            PROVIDER_ERROR_RATE_LIMITED,
            Some(retry_after_ms),
            None,
        ),
        provider::ProviderError::SubscriptionRefused {
            reason,
            retry_after_ms,
            resets_at,
        } => subscription_refused_status(reason, retry_after_ms, resets_at),
        // All three posture rejections share FAILED_PRECONDITION on purpose: the
        // caller asked for a guarantee (EU residency, verified ZDR, or a minimum
        // privacy tier) that no configured provider can honor, so the caller can
        // act on it (adjust the request / opt in) rather than retry blindly.
        provider::ProviderError::ResidencyViolation(message)
        | provider::ProviderError::ZdrUnavailable(message)
        | provider::ProviderError::TierUnavailable(message) => Status::failed_precondition(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::server::NamedService;

    #[test]
    fn health_name_matches_generated_inference_contract() {
        assert_eq!(
            GRPC_HEALTH_SERVICE_NAME,
            <InferenceCoreServer<InferenceService> as NamedService>::NAME
        );
    }

    #[test]
    fn internal_infer_scope_comes_only_from_verified_principal() {
        let principal = AuthenticatedPrincipal::for_test("org-signed", Some("user-signed"), true);
        let request = pb::InferRequest {
            thinking_budget_tokens: 0,
            org_id: "org-signed".to_owned(),
            zdr: false,
            ..Default::default()
        };

        let internal = to_internal_request(&request, &principal)
            .expect("an UNSPECIFIED tier request always maps onto the internal contract");

        assert_eq!(internal.org_id, "org-signed");
        assert_eq!(internal.user_id, "user-signed");
        assert!(internal.zdr, "issuer-enforced ZDR cannot be downgraded");
        // The caller's verified bearer is threaded for the budget check.
        assert_eq!(internal.caller_bearer.as_str(), "test-bearer");
    }

    #[tokio::test(start_paused = true)]
    async fn unary_response_carries_the_chains_timing_on_the_handler_origin() {
        use std::time::Duration;

        let mut clock = HandlerClock::start();
        // Authorization before the chain was entered.
        tokio::time::advance(Duration::from_millis(4)).await;
        clock.enter_chain();
        tokio::time::advance(Duration::from_millis(31)).await;
        let result = provider::InferResponse {
            request_id: "req-unary".to_owned(),
            input_tokens: 12,
            output_tokens: 3,
            provider_timing: Some(provider::ProviderTiming {
                routing: Duration::from_millis(1),
                provider: Duration::from_millis(30),
                attempts: 1,
                usage_reported: true,
                ..provider::ProviderTiming::default()
            }),
            ..provider::InferResponse::default()
        };

        let wire = infer_response_to_wire(result, &clock);

        let timing = wire
            .provider_timing
            .expect("the chain's timing is copied onto the wire");
        assert_eq!(timing.handler_ms, 35);
        assert_eq!(timing.routing_ms, 5, "authorization counts as routing");
        assert_eq!(timing.provider_ms, 30);
        assert_eq!(timing.attempts, 1);
        assert!(timing.usage_reported);
        assert_eq!(wire.request_id, "req-unary");
    }

    #[test]
    fn rpc_contract_classifies_catalogs_separately_from_provider_operations() {
        assert!(pb::ListModelsRequest::default().is_catalog());
        assert!(pb::ListSpeechVoicesRequest::default().is_catalog());
        assert!(pb::ListTranslationLanguagesRequest::default().is_catalog());
        assert!(!pb::InferRequest::default().is_catalog());
        assert!(!pb::CreateEmbeddingRequest::default().is_catalog());
        assert!(!pb::CreateRealtimeSessionRequest::default().is_catalog());
    }

    #[test]
    fn issuer_zdr_blocks_every_unattested_modality_before_provider_dispatch() {
        let principal = AuthenticatedPrincipal::for_test("org-zdr", Some("user-zdr"), true);
        for operation in [
            "speech synthesis",
            "speech transcription",
            "translation",
            "language detection",
            "image generation",
            "image analysis",
            "image text extraction",
            "document analysis",
            "language analysis",
            "realtime session",
            "video generation",
            "video status",
            "video content",
        ] {
            let error = reject_unattested_zdr_modality(&principal, false, operation)
                .expect_err("an unattested modality must make zero provider calls under ZDR");
            assert_eq!(error.code(), tonic::Code::FailedPrecondition, "{operation}");
        }
    }

    /// A speech provider that only counts calls. In the ZDR tests any call at
    /// all means text or audio left for a provider that retains it.
    #[derive(Default)]
    struct CountingSpeech {
        calls: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl crate::provider::speech::SpeechProvider for CountingSpeech {
        fn provider_name(&self) -> &'static str {
            "counting"
        }

        async fn synthesize(
            &self,
            req: &SpeechSynthesisRequest,
        ) -> Result<crate::provider::speech::AudioResult, provider::ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::provider::speech::AudioResult {
                audio_bytes: b"audio".to_vec(),
                format: req.format,
                duration_ms: 0,
                artifact_ref: None,
                model_used: "counting-tts".to_owned(),
                provider_used: "counting".to_owned(),
            })
        }

        async fn transcribe(
            &self,
            _req: &SpeechTranscriptionRequest,
        ) -> Result<crate::provider::speech::TranscriptResult, provider::ProviderError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(crate::provider::speech::TranscriptResult {
                text: "hei".to_owned(),
                language: "nb".to_owned(),
                confidence: 1.0,
                model_used: "counting-stt".to_owned(),
                provider_used: "counting".to_owned(),
            })
        }
    }

    fn counting_speech_chain() -> (SpeechChain, std::sync::Arc<CountingSpeech>) {
        let provider = std::sync::Arc::new(CountingSpeech::default());
        let chain = SpeechChain::new_with_providers(vec![(
            "counting".to_owned(),
            provider.clone() as std::sync::Arc<dyn crate::provider::speech::SpeechProvider>,
        )]);
        (chain, provider)
    }

    fn transcription(zdr: bool) -> pb::TranscribeSpeechRequest {
        pb::TranscribeSpeechRequest {
            request_id: "stt-zdr".to_owned(),
            org_id: "org-a".to_owned(),
            audio: b"webm-bytes".to_vec(),
            format: "webm".to_owned(),
            zdr,
            ..Default::default()
        }
    }

    fn synthesis(zdr: bool) -> pb::SynthesizeSpeechRequest {
        pb::SynthesizeSpeechRequest {
            request_id: "tts-zdr".to_owned(),
            org_id: "org-a".to_owned(),
            text: "Hei".to_owned(),
            format: "mp3".to_owned(),
            zdr,
            ..Default::default()
        }
    }

    /// Temporary chat sends `zdr` on the request while the token carries no
    /// ZDR claim. That flag alone must stop speech before any provider call,
    /// because no speech deployment is attested for zero retention.
    #[tokio::test]
    async fn zdr_speech_is_refused_before_any_provider_call() {
        let (speech, provider) = counting_speech_chain();
        for (issuer_zdr, request_zdr) in [(false, true), (true, false), (true, true)] {
            let principal = AuthenticatedPrincipal::for_test("org-a", Some("user-a"), issuer_zdr);
            let case = format!("issuer_zdr={issuer_zdr} request_zdr={request_zdr}");

            let stt = transcribe_speech_as(&speech, &principal, transcription(request_zdr))
                .await
                .expect_err("ZDR transcription must be refused");
            assert_eq!(stt.code(), tonic::Code::FailedPrecondition, "{case}");
            assert!(stt.message().contains("speech transcription"), "{case}");

            let tts = synthesize_speech_as(&speech, &principal, synthesis(request_zdr))
                .await
                .expect_err("ZDR synthesis must be refused");
            assert_eq!(tts.code(), tonic::Code::FailedPrecondition, "{case}");
            assert!(tts.message().contains("speech synthesis"), "{case}");
        }
        assert_eq!(
            provider.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "no text or audio may reach a speech provider under ZDR"
        );
    }

    #[tokio::test]
    async fn non_zdr_speech_still_reaches_the_provider() {
        let (speech, provider) = counting_speech_chain();
        let principal = AuthenticatedPrincipal::for_test("org-a", Some("user-a"), false);

        let stt = transcribe_speech_as(&speech, &principal, transcription(false))
            .await
            .expect("an ordinary transcription is served");
        assert_eq!(stt.text, "hei");
        assert_eq!(stt.request_id, "stt-zdr");

        let tts = synthesize_speech_as(&speech, &principal, synthesis(false))
            .await
            .expect("an ordinary synthesis is served");
        assert_eq!(tts.audio, b"audio");
        assert_eq!(tts.format, "mp3");

        assert_eq!(provider.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    fn trailer<'a>(status: &'a Status, key: &str) -> Option<&'a str> {
        status.metadata().get(key).and_then(|value| value.to_str().ok())
    }

    /// Each subscription refusal reaches the gateway as its code, its own
    /// gRPC code and only the timing it has, with a message naming the code
    /// alone.
    #[test]
    fn a_subscription_refusal_carries_its_code_and_timing_as_trailers() {
        use provider::SubscriptionRefusal;
        let resets_at = chrono::DateTime::parse_from_rfc3339("2026-09-26T01:21:00.750+02:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        for (reason, code, grpc) in [
            (
                SubscriptionRefusal::UsageLimit,
                "subscription_usage_limit",
                tonic::Code::ResourceExhausted,
            ),
            (
                SubscriptionRefusal::ReauthRequired,
                "subscription_reauth_required",
                tonic::Code::Unavailable,
            ),
            (
                SubscriptionRefusal::UpstreamUnavailable,
                "subscription_upstream_unavailable",
                tonic::Code::Unavailable,
            ),
            (
                SubscriptionRefusal::BadRequest,
                "subscription_bad_request",
                tonic::Code::Unavailable,
            ),
        ] {
            let status = provider_error_to_status(provider::ProviderError::SubscriptionRefused {
                reason,
                retry_after_ms: Some(52_260_000),
                resets_at: Some(resets_at),
            });
            assert_eq!(status.code(), grpc, "{code}");
            assert_eq!(status.message(), format!("subscription refused: {code}"));
            assert_eq!(trailer(&status, PROVIDER_ERROR_KIND_TRAILER), Some(code));
            assert_eq!(trailer(&status, RETRY_AFTER_MS_TRAILER), Some("52260000"));
            assert_eq!(
                trailer(&status, RESETS_AT_TRAILER),
                Some("2026-09-25T23:21:00Z"),
                "UTC, whole seconds"
            );

            let bare = provider_error_to_status(provider::ProviderError::SubscriptionRefused {
                reason,
                retry_after_ms: None,
                resets_at: None,
            });
            assert_eq!(trailer(&bare, PROVIDER_ERROR_KIND_TRAILER), Some(code));
            assert_eq!(trailer(&bare, RETRY_AFTER_MS_TRAILER), None, "{code}");
            assert_eq!(trailer(&bare, RESETS_AT_TRAILER), None, "{code}");
        }
    }

    #[test]
    fn a_rate_limit_is_typed_with_its_retry_after() {
        let status =
            provider_error_to_status(provider::ProviderError::RateLimited { retry_after_ms: 1_500 });
        assert_eq!(status.code(), tonic::Code::ResourceExhausted);
        assert_eq!(status.message(), "rate limited: retry after 1500ms");
        assert_eq!(trailer(&status, PROVIDER_ERROR_KIND_TRAILER), Some("rate_limited"));
        assert_eq!(trailer(&status, RETRY_AFTER_MS_TRAILER), Some("1500"));
        assert_eq!(trailer(&status, RESETS_AT_TRAILER), None);
    }

    /// Context-too-long from the subscription keeps the overflow contract:
    /// `Unavailable`, the `too_long` trailer, and the code as its detail.
    #[test]
    fn a_subscription_overflow_keeps_the_too_long_contract() {
        let status = provider_error_to_status(provider::ProviderError::TooLong {
            detail: "subscription_context_too_long".to_owned(),
        });
        assert_eq!(status.code(), tonic::Code::Unavailable);
        assert_eq!(status.message(), "subscription_context_too_long");
        assert_eq!(trailer(&status, PROVIDER_ERROR_KIND_TRAILER), Some("too_long"));
        assert_eq!(trailer(&status, RETRY_AFTER_MS_TRAILER), None);
    }

    /// A started turn the broker failed without a typed reason reaches the
    /// gateway exactly as before: plain `Unavailable`, no trailer, so it reads
    /// as the generic route failure.
    #[test]
    fn an_untyped_subscription_failure_stays_a_plain_unavailable() {
        let status = provider_error_to_status(provider::ProviderError::SubscriptionFailed(
            "subscription broker stream failed: subscription_broker_failed".to_owned(),
        ));
        assert_eq!(status.code(), tonic::Code::Unavailable);
        assert_eq!(
            status.message(),
            "subscription broker stream failed: subscription_broker_failed"
        );
        assert!(status.metadata().is_empty(), "no typed trailer");
    }
}
