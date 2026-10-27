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
use crate::entitlement::{tier_from_label, RequestAccess, Resolved};
use crate::provider;
use crate::provider::doc_intel::{AnalyzeDocumentRequest, DocIntelChain};
use crate::provider::fallback::{is_unspecified_model, FallbackChain};
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
/// Images one chat request may carry as parts of its messages.
const MAX_IMAGE_PARTS: usize = 4;
/// Media types a chat image part may declare; the set both provider bodies accept.
const IMAGE_PART_MIME_TYPES: &[&str] = &["image/png", "image/jpeg", "image/webp", "image/gif"];
/// Floor of the largest request tonic decodes. The 4 MiB default would refuse a
/// request carrying even two images long before `MAX_IMAGE_BYTES` could apply.
const MAX_REQUEST_DECODING_BYTES: usize = 16 * 1024 * 1024;
/// Room for the rest of a request around its attachment bytes.
const REQUEST_DECODING_HEADROOM_BYTES: usize = 8 * 1024 * 1024;
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
            effort_levels: model
                .effort_levels
                .into_iter()
                .map(|level| level.as_str().to_owned())
                .collect(),
        }
    }
}

/// Map a wire `ReasoningEffort` onto the internal ladder. UNSPECIFIED is "no
/// level" so the legacy budget keeps deciding; an unknown numeric fails closed
/// with `invalid_argument`, because a NEWER client naming a level this build
/// cannot interpret must not have its request silently weakened to "none".
fn effort_from_wire(value: i32) -> Result<Option<provider::effort::Level>, Status> {
    use provider::effort::Level;
    match pb::ReasoningEffort::try_from(value) {
        Ok(pb::ReasoningEffort::Unspecified) => Ok(None),
        Ok(pb::ReasoningEffort::Minimal) => Ok(Some(Level::Minimal)),
        Ok(pb::ReasoningEffort::Low) => Ok(Some(Level::Low)),
        Ok(pb::ReasoningEffort::Medium) => Ok(Some(Level::Medium)),
        Ok(pb::ReasoningEffort::High) => Ok(Some(Level::High)),
        Ok(pb::ReasoningEffort::Xhigh) => Ok(Some(Level::XHigh)),
        Ok(pb::ReasoningEffort::Max) => Ok(Some(Level::Max)),
        Err(_) => Err(Status::invalid_argument(format!(
            "unknown reasoning effort value: {value}"
        ))),
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
    pb::GetModelAccessRequest,
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
        // From here the call has a verified caller, whatever the checks below
        // decide: the metering decorator counts it under that service.
        crate::metered::note_caller(request.extensions(), &principal);
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
/// flag pass `false`; the token claim still applies to them. A modality is never
/// the route a connection-bound waiver names, so the posture check (Z-16) runs
/// first: a waived or mixed-posture caller is refused before any ZDR gate.
fn reject_unattested_zdr_modality(
    principal: &AuthenticatedPrincipal,
    request_zdr: bool,
    operation: &'static str,
) -> Result<(), Status> {
    principal.enforce_non_chat_zdr_posture(request_zdr)?;
    if principal.effective_zdr(request_zdr) {
        return Err(zdr_unavailable_status(format!(
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

        let access = self
            .chain
            .entitlement()
            .access_for(&principal)
            .await
            .map_err(provider_error_to_status)?;
        let internal_req = with_access(to_internal_request(&req, &principal)?, access);

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

        let access = self
            .chain
            .entitlement()
            .access_for(&principal)
            .await
            .map_err(provider_error_to_status)?;
        let internal_req = with_access(to_internal_request(&req, &principal)?, access);

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
        // An embedding is not the chat route a waiver names: the posture check (Z-16)
        // refuses a waived caller here, before any provider is named.
        principal.enforce_non_chat_zdr_posture(req.zdr)?;
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
                // A residency, ZDR or tier refusal is a precondition the caller
                // can act on, not an internal fault: typed FAILED_PRECONDITION.
                embedding_error_to_status(e)
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
        let principal = self.authorize(&request).await?;
        let resolved = self
            .chain
            .entitlement()
            .resolve(&principal)
            .await
            .map_err(provider_error_to_status)?;
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

        // The caller's organisation policy applies to chat models only; every
        // other modality passes unfiltered. Without a policy answer to apply
        // (off, a service caller, an unreadable answer in shadow) the response is
        // what it was before the policy existed.
        let access = match resolved {
            Resolved::Access(access) => Some(catalogue_access(&access, &mut models)),
            Resolved::Off | Resolved::Exempt | Resolved::Unreadable => None,
        };

        Ok(Response::new(pb::ListModelsResponse { models, access }))
    }

    async fn get_model_access(
        &self,
        request: Request<pb::GetModelAccessRequest>,
    ) -> Result<Response<pb::GetModelAccessResponse>, Status> {
        let principal = self.authorize(&request).await?;
        let resolved = self
            .chain
            .entitlement()
            .resolve(&principal)
            .await
            .map_err(provider_error_to_status)?;
        model_access_response(
            &self.chain.list_models("chat", ""),
            request.into_inner(),
            resolved,
        )
        .map(Response::new)
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

/// The images of a chat request, checked against the messages they ride with.
///
/// Every limit is a refusal, never a trim: a request that names a fifth image,
/// an oversize or empty one, a type outside the allowlist, or a message that is
/// not a user message, is not a request the caller meant, and answering it from
/// what is left would be a silent loss. Only sizes and indexes appear in the
/// message, never bytes.
///
/// # Errors
/// Returns `invalid_argument` naming the first limit a part breaks.
fn image_parts_from_wire(req: &pb::InferRequest) -> Result<Vec<provider::ImagePart>, Status> {
    if req.image_parts.len() > MAX_IMAGE_PARTS {
        return Err(Status::invalid_argument(format!(
            "at most {MAX_IMAGE_PARTS} image parts per request"
        )));
    }
    req.image_parts
        .iter()
        .map(|part| {
            let index = part.message_index as usize;
            let message = req.messages.get(index).ok_or_else(|| {
                Status::invalid_argument(format!("image part names message {index}, which does not exist"))
            })?;
            if !message.role.eq_ignore_ascii_case("user") {
                return Err(Status::invalid_argument(format!(
                    "image part {index} must ride on a user message"
                )));
            }
            if part.data.is_empty() || part.data.len() > MAX_IMAGE_BYTES {
                return Err(Status::invalid_argument(format!(
                    "image part {index} must be between 1 byte and {} MB",
                    MAX_IMAGE_BYTES / (1024 * 1024)
                )));
            }
            if !IMAGE_PART_MIME_TYPES.contains(&part.mime_type.as_str()) {
                return Err(Status::invalid_argument(format!(
                    "image part {index} has an unsupported mime type"
                )));
            }
            Ok(provider::ImagePart {
                message_index: index,
                data: std::sync::Arc::from(part.data.as_ref()),
                mime_type: part.mime_type.clone(),
            })
        })
        .collect()
}

/// Convert the public contract to the provider contract. Tenant, user, and
/// issuer-enforced ZDR posture come only from the verified principal.
///
/// The route the request names is checked against that posture first (Z-16): a
/// connection-bound waiver reaches its own provider and connection and nothing
/// else, so a request that names any other route never reaches a provider.
///
/// # Errors
/// Returns `invalid_argument` when the caller names a privacy tier this build
/// cannot interpret, and the posture refusal when the route is not allowed.
fn to_internal_request(
    req: &pb::InferRequest,
    principal: &AuthenticatedPrincipal,
) -> Result<provider::InferRequest, Status> {
    principal.enforce_zdr_route(&req.provider_hint, &req.subscription_connection_id, req.zdr)?;
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
        reasoning_effort: effort_from_wire(req.reasoning_effort)?,
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
        image_parts: image_parts_from_wire(req)?,
        prompt_tiers: req
            .prompt_tiers
            .as_ref()
            .map(|plan| mp_contracts::prompt_tier::PromptTierPlan {
                stable: plan.stable_messages as usize,
                context: plan.context_messages as usize,
            })
            .unwrap_or_default(),
        cache_scope_key: req.cache_scope_key.clone(),
        org_id: principal.org_id.clone(),
        user_id: principal.budget_user_id(),
        // The verified caller's own token, forwarded so the intent layer's
        // cost-core budget check authenticates as the caller (cost-core pins
        // org/user to the token's claims). Never serialized into provider
        // bodies — see the field's doc comment.
        caller_bearer: crate::provider::Bearer::new(principal.bearer()),
        access: None,
    })
}

/// Attach the caller's entitlement to a provider request. When the policy is
/// applied, its minimum privacy tier joins the request's own as the stronger of
/// the two, so the chain's existing tier gate enforces it and nothing new
/// decides a provider.
fn with_access(
    mut request: provider::InferRequest,
    access: Option<std::sync::Arc<RequestAccess>>,
) -> provider::InferRequest {
    if let Some(access) = &access {
        let floor = access.min_privacy_tier();
        if floor > request.min_privacy_tier {
            request.min_privacy_tier = floor;
        }
    }
    request.access = access;
    request
}

/// The policy a `GetModelAccess` draft carries, in the matcher's form.
fn policy_of_draft(doc: &pb::ModelPolicyDoc) -> mp_contracts::model_entitlement::Policy {
    use mp_contracts::model_entitlement::{Policy, SurfaceDefault};
    let lower = |values: &[String]| -> Vec<String> {
        values
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .collect()
    };
    let tier = match provider::PrivacyTier::from_wire(doc.min_privacy_tier) {
        Some(provider::PrivacyTier::Unspecified) => "",
        Some(tier) => tier.label(),
        // A tier this build does not know is the strongest, never no minimum.
        None => provider::PrivacyTier::Sovereign.label(),
    };
    Policy {
        allow_all: doc.allow_mode.trim().eq_ignore_ascii_case("all"),
        allow: lower(&doc.allow),
        deny: lower(&doc.deny),
        max_effort: doc.max_effort.trim().to_ascii_lowercase(),
        min_privacy_tier: tier.to_owned(),
        defaults: doc
            .defaults
            .iter()
            .map(|(surface, default)| {
                (
                    surface.trim().to_ascii_lowercase(),
                    SurfaceDefault {
                        model: default.model.trim().to_ascii_lowercase(),
                        effort: default.effort.trim().to_ascii_lowercase(),
                    },
                )
            })
            .collect(),
    }
}

/// Filter a catalogue by the caller's policy and describe what was done. Under
/// shadow nothing is removed: the catalogue stays whole and only says that the
/// policy is not applied.
fn catalogue_access(access: &RequestAccess, models: &mut Vec<pb::ModelInfo>) -> pb::ModelAccessMeta {
    let mut hidden = 0_u32;
    if access.enforced() {
        models.retain(|model| {
            let keep = model.modality != "chat" || access.check(&model.id, &model.provider).is_ok();
            hidden += u32::from(!keep);
            keep
        });
    }
    let resolved = access.resolved();
    let policy = resolved.policy.as_ref();
    let default = policy.and_then(|policy| policy.defaults.get("chat"));
    pb::ModelAccessMeta {
        state: resolved.state.as_str().to_owned(),
        enforced: access.enforced(),
        revision: resolved.revision,
        restricted: resolved.restricted(),
        hidden_count: hidden,
        default_model: default.map(|d| d.model.clone()).unwrap_or_default(),
        default_effort: default.map(|d| d.effort.clone()).unwrap_or_default(),
        max_effort: policy.map(|p| p.max_effort.clone()).unwrap_or_default(),
    }
}

/// Whether a chat model's deployment carries a zero-retention attestation: the
/// `zdr` feature family `ProviderCapabilities::feature_flags` derives from
/// `supports_zdr`, or a tier that only a verified ZDR contract reaches
/// (`PrivacyTier::classify`). It is a fact about the deployment, so the policy
/// never changes it; the Frontend BFF reads the same rule off `/v1/models`.
fn attests_zdr(model: &provider::ModelInfo) -> bool {
    model.features.iter().any(|feature| feature == "zdr")
        || matches!(
            model.privacy_tier,
            provider::PrivacyTier::ZdrContractual | provider::PrivacyTier::Sovereign
        )
}

/// The answer to a `GetModelAccess`: what the policy says about the caller, one
/// model and, for a caller who can manage the policy, a draft and the catalogue.
///
/// The verdict is the policy's whether or not it is applied; `enforced` says
/// whether it is. A draft or a catalogue is the policy manager's: for any other
/// caller it is refused, and when nothing was resolved (off, an unreadable
/// answer in shadow) it is unavailable rather than answered from nothing.
fn model_access_response(
    chat: &[provider::ModelInfo],
    req: pb::GetModelAccessRequest,
    resolved: Resolved,
) -> Result<pb::GetModelAccessResponse, Status> {
    use mp_contracts::model_entitlement::{Refusal, ResolvedAccess, State};
    let manage = req.draft.is_some() || req.include_catalogue;
    let (mut access, enforced) = match &resolved {
        Resolved::Access(access) => (access.resolved().clone(), access.enforced()),
        Resolved::Off | Resolved::Unreadable if manage => {
            return Err(Refusal::unavailable().grpc_status())
        }
        Resolved::Off | Resolved::Unreadable | Resolved::Exempt => {
            (ResolvedAccess::unrestricted(), false)
        }
    };
    if manage && !access.can_manage {
        return Err(Status::permission_denied(
            "the model policy can be read in full only by those who manage it",
        ));
    }
    if let Some(draft) = &req.draft {
        access.policy = Some(policy_of_draft(draft));
        if access.state == State::None {
            access.state = State::Org;
        }
    }

    let (model_allowed, reason) = if is_unspecified_model(&req.model) {
        (true, "")
    } else {
        let provider = if req.provider_hint.trim().is_empty() {
            chat.iter()
                .find(|known| known.id.eq_ignore_ascii_case(req.model.trim()))
                .map_or("", |known| known.provider.as_str())
        } else {
            req.provider_hint.as_str()
        };
        match access.permits(&req.model, provider) {
            Ok(()) => (true, ""),
            Err(reason) => (false, reason.as_str()),
        }
    };
    let catalogue = if req.include_catalogue {
        chat.iter()
            .map(|known| {
                let verdict = access.permits(&known.id, &known.provider);
                pb::ModelAccessEntry {
                    id: known.id.clone(),
                    provider: known.provider.clone(),
                    entitled: verdict.is_ok(),
                    reason: verdict.err().map_or(String::new(), |r| r.as_str().to_owned()),
                    // From the unfiltered chat list, whatever the draft refuses.
                    zdr_attested: attests_zdr(known),
                }
            })
            .collect()
    } else {
        Vec::new()
    };

    let policy = access.policy.as_ref();
    let default = policy.and_then(|policy| policy.defaults.get(req.surface.trim().to_ascii_lowercase().as_str()));
    Ok(pb::GetModelAccessResponse {
        request_id: req.request_id,
        state: access.state.as_str().to_owned(),
        enforced,
        revision: access.revision,
        role: access.role.clone(),
        restricted: access.restricted(),
        model_allowed,
        reason: reason.to_owned(),
        default_model: default.map(|d| d.model.clone()).unwrap_or_default(),
        default_effort: default.map(|d| d.effort.clone()).unwrap_or_default(),
        max_effort: policy.map(|p| p.max_effort.clone()).unwrap_or_default(),
        min_privacy_tier: policy
            .map_or(provider::PrivacyTier::Unspecified, |p| tier_from_label(&p.min_privacy_tier))
            .as_wire_i32(),
        can_manage: access.can_manage,
        catalogue,
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
    // Every RPC is counted by the decorator, so none can ship unmetered.
    let grpc_service = InferenceCoreServer::new(crate::metered::MeteredCore::new(service))
        .max_decoding_message_size(request_decoding_limit_bytes());
    let (mut health_reporter, health_service) = tonic_health::server::health_reporter();
    health_reporter
        .set_serving::<InferenceCoreServer<crate::metered::MeteredCore<InferenceService>>>()
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

/// Value of [`PROVIDER_ERROR_KIND_TRAILER`] when a request required zero data
/// retention and no matching deployment has an independently verified ZDR
/// contract. Refused before any provider is called.
///
/// The three posture kinds below are a frozen vocabulary: callers (the Data
/// Plane client, the gateway's refusal catalogue) key on the exact strings and
/// a rename is a breaking change. They ride a `FAILED_PRECONDITION` whose
/// message is unchanged, so a caller that has not adopted the trailer behaves
/// exactly as it did when these were message-only.
pub const PROVIDER_ERROR_ZDR_UNAVAILABLE: &str = "zdr_unavailable";

/// Value of [`PROVIDER_ERROR_KIND_TRAILER`] when no matching deployment meets
/// the requested residency floor, or an embedding region is outside the EU
/// boundary. Refused before any provider is called.
pub const PROVIDER_ERROR_RESIDENCY_UNAVAILABLE: &str = "residency_unavailable";

/// Value of [`PROVIDER_ERROR_KIND_TRAILER`] when a request carried images and no
/// matching provider and model accepts them. Refused before any provider is
/// called, so the images are never dropped to answer from the text.
pub const PROVIDER_ERROR_VISION_UNSUPPORTED: &str = "vision_unsupported";

/// Value of [`PROVIDER_ERROR_KIND_TRAILER`] when no configured provider
/// satisfies the requested minimum privacy tier. Refused before any provider
/// is called.
pub const PROVIDER_ERROR_TIER_UNAVAILABLE: &str = "tier_unavailable";

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

/// A posture refusal on the wire: the caller asked for a guarantee (verified
/// ZDR, an EU residency floor, a minimum privacy tier) that no configured
/// provider can honor, so no provider was called.
///
/// All three share `FAILED_PRECONDITION` on purpose, because the caller can act
/// on it (adjust the request, or opt in) rather than retry blindly, and a
/// consumer that predates the kind trailer keeps reading the code it always
/// read. The `kind` is what tells the three apart, so no caller has to match
/// the message text. The message is the refusal's own, content-free sentence.
fn posture_refusal_status(kind: &'static str, message: String) -> Status {
    typed_status(tonic::Code::FailedPrecondition, message, kind, None, None)
}

/// The zero-retention refusal: the chain's own, and the one every non-chat
/// modality returns when ZDR applies and no deployment of it is attested.
fn zdr_unavailable_status(message: String) -> Status {
    posture_refusal_status(PROVIDER_ERROR_ZDR_UNAVAILABLE, message)
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
        // The provider refused the request itself. Sending it again cannot change the answer,
        // so the code says so: `invalid_argument`, not the `unavailable` that asks the caller to
        // come back later. No kind trailer: the kind vocabulary is pinned by the committed status
        // contract, which this change does not regenerate.
        provider::ProviderError::RequestRejected { detail } => Status::invalid_argument(detail),
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
        // The three posture rejections: one code, three kinds. See
        // `posture_refusal_status`.
        provider::ProviderError::ResidencyViolation(message) => {
            posture_refusal_status(PROVIDER_ERROR_RESIDENCY_UNAVAILABLE, message)
        }
        provider::ProviderError::ZdrUnavailable(message) => zdr_unavailable_status(message),
        provider::ProviderError::TierUnavailable(message) => {
            posture_refusal_status(PROVIDER_ERROR_TIER_UNAVAILABLE, message)
        }
        provider::ProviderError::VisionUnsupported(message) => {
            posture_refusal_status(PROVIDER_ERROR_VISION_UNSUPPORTED, message)
        }
        // The two entitlement refusals. `PERMISSION_DENIED` and `UNAVAILABLE`,
        // never `FAILED_PRECONDITION`: the gateway reads that status as a
        // posture or ZDR refusal. The status is the catalogue's own.
        provider::ProviderError::ModelNotEntitled { model, reason } => {
            mp_contracts::model_entitlement::Refusal::not_entitled(&model, reason).grpc_status()
        }
        provider::ProviderError::EntitlementUnavailable => {
            mp_contracts::model_entitlement::Refusal::unavailable().grpc_status()
        }
    }
}

/// A `CreateEmbedding` failure on the wire.
///
/// A posture refusal is typed exactly as it is on every other path. Anything
/// else stays `INTERNAL`, as it always has: the embedding clients retry on that
/// code, and re-coding it here would change what they do.
fn embedding_error_to_status(error: provider::ProviderError) -> Status {
    match error {
        posture @ (provider::ProviderError::ResidencyViolation(_)
        | provider::ProviderError::ZdrUnavailable(_)
        | provider::ProviderError::TierUnavailable(_)) => provider_error_to_status(posture),
        other => Status::internal(other.to_string()),
    }
}

/// Largest request tonic decodes. A request carrying attachments holds their
/// bytes raw (gRPC has no base64), so the floor is raised to the declared
/// attachment total (`CHAT_ATTACHMENT_MAX_TOTAL_BYTES`, the number model-gateway
/// and the BFF derive their body limits from) plus room for the rest of the
/// request. Unset, it is the floor: nothing changes until the cap is raised.
fn request_decoding_limit_bytes() -> usize {
    decoding_limit_with(&|key| std::env::var(key).ok())
}

fn decoding_limit_with(get: &dyn Fn(&str) -> Option<String>) -> usize {
    get("CHAT_ATTACHMENT_MAX_TOTAL_BYTES")
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|total| *total > 0)
        .map_or(MAX_REQUEST_DECODING_BYTES, |total| {
            MAX_REQUEST_DECODING_BYTES.max(total.saturating_add(REQUEST_DECODING_HEADROOM_BYTES))
        })
}

#[cfg(test)]
mod decoding_limit_tests {
    use super::{decoding_limit_with, MAX_REQUEST_DECODING_BYTES, REQUEST_DECODING_HEADROOM_BYTES};

    fn limit(total: Option<&str>) -> usize {
        decoding_limit_with(&|_| total.map(str::to_owned))
    }

    #[test]
    fn unset_or_unreadable_totals_keep_the_floor() {
        assert_eq!(limit(None), MAX_REQUEST_DECODING_BYTES);
        assert_eq!(limit(Some("0")), MAX_REQUEST_DECODING_BYTES);
        assert_eq!(limit(Some("lots")), MAX_REQUEST_DECODING_BYTES);
        // Today's declared total (3 MiB) fits under the floor.
        assert_eq!(limit(Some("3145728")), MAX_REQUEST_DECODING_BYTES);
    }

    #[test]
    fn a_raised_total_raises_the_limit_to_hold_it() {
        let total = 64 * 1024 * 1024;
        assert_eq!(
            limit(Some(&total.to_string())),
            total + REQUEST_DECODING_HEADROOM_BYTES
        );
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

    fn effort_request(value: i32) -> pb::InferRequest {
        pb::InferRequest {
            org_id: "org-signed".to_owned(),
            reasoning_effort: value,
            ..Default::default()
        }
    }

    #[test]
    fn proto_level_roundtrips_to_internal() {
        use provider::effort::Level;
        let principal = AuthenticatedPrincipal::for_test("org-signed", Some("user-signed"), false);
        let cases = [
            (pb::ReasoningEffort::Unspecified, None),
            (pb::ReasoningEffort::Minimal, Some(Level::Minimal)),
            (pb::ReasoningEffort::Low, Some(Level::Low)),
            (pb::ReasoningEffort::Medium, Some(Level::Medium)),
            (pb::ReasoningEffort::High, Some(Level::High)),
            (pb::ReasoningEffort::Xhigh, Some(Level::XHigh)),
            (pb::ReasoningEffort::Max, Some(Level::Max)),
        ];
        for (wire, internal) in cases {
            let mapped = to_internal_request(&effort_request(wire as i32), &principal)
                .expect("every known level maps onto the internal contract");
            assert_eq!(mapped.reasoning_effort, internal, "{wire:?}");
        }
    }

    #[test]
    fn the_prompt_tier_plan_maps_to_counts_and_absent_means_none() {
        let principal = AuthenticatedPrincipal::for_test("org-signed", Some("user-signed"), false);
        let mut request = effort_request(0);
        let absent = to_internal_request(&request, &principal).expect("maps");
        assert_eq!(absent.prompt_tiers, Default::default());

        request.prompt_tiers = Some(pb::PromptTierPlan {
            stable_messages: 2,
            context_messages: 3,
        });
        let mapped = to_internal_request(&request, &principal).expect("maps");
        assert_eq!((mapped.prompt_tiers.stable, mapped.prompt_tiers.context), (2, 3));
    }

    #[test]
    fn a_level_leaves_the_legacy_budget_input_untouched() {
        let principal = AuthenticatedPrincipal::for_test("org-signed", Some("user-signed"), false);
        let mut request = effort_request(pb::ReasoningEffort::High as i32);
        request.thinking_budget_tokens = 1_024;
        let mapped = to_internal_request(&request, &principal).expect("maps");
        // Field 13 stays a legacy input; "the level wins" is decided where the
        // level is resolved, not by erasing the caller's own number here.
        assert_eq!(mapped.thinking_budget_tokens, 1_024);
        assert_eq!(mapped.reasoning_effort, Some(provider::effort::Level::High));
    }

    #[test]
    fn unknown_enum_value_is_invalid_argument() {
        let principal = AuthenticatedPrincipal::for_test("org-signed", Some("user-signed"), false);
        for value in [7, 99, -1] {
            let error = to_internal_request(&effort_request(value), &principal)
                .expect_err("a level this build cannot interpret must not be weakened");
            assert_eq!(error.code(), tonic::Code::InvalidArgument, "{value}");
        }
    }

    #[test]
    fn model_info_carries_effort_levels_as_canonical_names() {
        use provider::effort::Level;
        let wire: pb::ModelInfo = provider::ModelInfo {
            effort_levels: vec![Level::Low, Level::Medium, Level::High, Level::XHigh],
            ..provider::ModelInfo::default()
        }
        .into();
        assert_eq!(wire.effort_levels, ["low", "medium", "high", "xhigh"]);

        let none: pb::ModelInfo = provider::ModelInfo::default().into();
        assert!(none.effort_levels.is_empty());
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

    /// A refusal of the request reaches the caller as an invalid argument carrying the
    /// provider's own text, with no retry timing and no kind trailer. The code is what says the
    /// same request must not be sent again.
    #[test]
    fn a_provider_rejection_is_invalid_argument_with_no_retry_timing() {
        let detail = "400 Bad Request: tools.15.custom.name: String should match pattern";
        let status = provider_error_to_status(provider::ProviderError::RequestRejected {
            detail: detail.to_owned(),
        });
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert_eq!(status.message(), detail);
        assert_eq!(trailer(&status, RETRY_AFTER_MS_TRAILER), None);
        assert_eq!(trailer(&status, PROVIDER_ERROR_KIND_TRAILER), None);
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

    /// The three posture refusals keep the code they always had and gain the
    /// kind a caller reads instead of the prose. The kind strings are a frozen
    /// vocabulary (the Data Plane client and the refusal catalogue key on
    /// them), so they are spelled as literals here and not through the
    /// constants: renaming one must fail this test, not silently follow.
    #[test]
    fn a_posture_refusal_is_failed_precondition_with_its_frozen_kind() {
        for (error, message, kind) in [
            (
                provider::ProviderError::ZdrUnavailable("zdr gate".to_owned()),
                "zdr gate",
                "zdr_unavailable",
            ),
            (
                provider::ProviderError::ResidencyViolation("residency gate".to_owned()),
                "residency gate",
                "residency_unavailable",
            ),
            (
                provider::ProviderError::TierUnavailable("tier gate".to_owned()),
                "tier gate",
                "tier_unavailable",
            ),
        ] {
            let status = provider_error_to_status(error);
            assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{kind}");
            assert_eq!(
                status.message(),
                message,
                "{kind}: the message is unchanged"
            );
            assert_eq!(trailer(&status, "x-mp-provider-error"), Some(kind));
            assert_eq!(trailer(&status, "x-mp-retry-after-ms"), None, "{kind}");
            assert_eq!(trailer(&status, "x-mp-resets-at"), None, "{kind}");
        }
    }

    /// A non-chat modality refused under ZDR names the same kind as the chat
    /// chain does, whether ZDR came from the token or from the request's flag.
    #[test]
    fn a_zdr_modality_refusal_names_its_kind() {
        for (issuer_zdr, request_zdr) in [(true, false), (false, true), (true, true)] {
            let principal =
                AuthenticatedPrincipal::for_test("org-zdr", Some("user-zdr"), issuer_zdr);
            let status = reject_unattested_zdr_modality(&principal, request_zdr, "translation")
                .expect_err("an unattested modality is refused under ZDR");
            let case = format!("issuer_zdr={issuer_zdr} request_zdr={request_zdr}");
            assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{case}");
            assert!(status.message().contains("translation"), "{case}");
            assert_eq!(
                trailer(&status, "x-mp-provider-error"),
                Some("zdr_unavailable"),
                "{case}"
            );
        }
    }

    /// The committed status contract: what the Data Plane client (and any other
    /// caller) may rely on, generated from the code under test rather than
    /// written by hand. Regenerate with `MP_UPDATE_GOLDEN=1`; the repo-level
    /// `scripts/tests/zdr-contract-sync.test.sh` then needs its pinned digest
    /// updated, which is the deliberate second step of changing the contract.
    const STATUS_CONTRACT_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../api/contracts/infer-statuses.json"
    );

    /// One row per way a call can fail, built from the real mappers.
    fn status_contract_cases() -> Vec<(String, &'static str, Status)> {
        use provider::{ProviderError as E, SubscriptionRefusal as S};
        let resets_at = chrono::DateTime::parse_from_rfc3339("2026-09-26T01:21:00.750+02:00")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let principal = AuthenticatedPrincipal::for_test("org-zdr", Some("user-zdr"), true);
        let mut cases: Vec<(String, &'static str, Status)> = Vec::new();
        let mut add = |id: &str, source: &'static str, status: Status| {
            cases.push((id.to_owned(), source, status));
        };
        for error in [
            E::Http("upstream reset".to_owned()),
            E::Unavailable("provider unavailable".to_owned()),
            E::SubscriptionFailed("subscription_broker_failed".to_owned()),
            E::InvalidResponse("bad response".to_owned()),
            E::UnsupportedModel("model".to_owned()),
            E::AllExhausted { attempts: 3 },
            E::TooLong {
                detail: "413: too long".to_owned(),
            },
            E::RateLimited {
                retry_after_ms: 1_500,
            },
            E::ZdrUnavailable(
                "no matching provider deployment has verified ZDR support".to_owned(),
            ),
            E::ResidencyViolation(
                "no matching provider deployment meets the residency floor".to_owned(),
            ),
            E::TierUnavailable("no configured provider satisfies privacy tier".to_owned()),
            E::ModelNotEntitled {
                model: "gpt-5".to_owned(),
                reason: mp_contracts::model_entitlement::Reason::ExplicitlyDenied,
            },
            E::EntitlementUnavailable,
        ] {
            let source = status_contract_source(&error);
            let id = status_contract_id(source);
            add(&id, source, provider_error_to_status(error));
        }
        for (reason, id) in [
            (S::UsageLimit, "subscription_usage_limit"),
            (S::ReauthRequired, "subscription_reauth_required"),
            (S::UpstreamUnavailable, "subscription_upstream_unavailable"),
            (S::BadRequest, "subscription_bad_request"),
        ] {
            for (suffix, retry_after_ms, resets_at) in [
                ("with_timing", Some(52_260_000), Some(resets_at)),
                ("without_timing", None, None),
            ] {
                add(
                    &format!("{id}_{suffix}"),
                    "ProviderError::SubscriptionRefused",
                    provider_error_to_status(E::SubscriptionRefused {
                        reason,
                        retry_after_ms,
                        resets_at,
                    }),
                );
            }
        }
        add(
            "modality_refused_under_zdr",
            "reject_unattested_zdr_modality",
            reject_unattested_zdr_modality(&principal, false, "translation")
                .expect_err("an unattested modality is refused under ZDR"),
        );
        // CreateEmbedding types the three posture refusals like every other
        // path and keeps INTERNAL for the rest (its clients retry on it).
        for (id, error) in [
            (
                "embedding_zdr_unavailable",
                E::ZdrUnavailable("no matching provider deployment has verified ZDR".to_owned()),
            ),
            (
                "embedding_residency_unavailable",
                E::ResidencyViolation("embedding region is outside the EU boundary".to_owned()),
            ),
            (
                "embedding_tier_unavailable",
                E::TierUnavailable("no configured provider satisfies the tier".to_owned()),
            ),
            (
                "embedding_provider_http",
                E::Http("upstream reset".to_owned()),
            ),
        ] {
            add(
                id,
                "embedding_error_to_status",
                embedding_error_to_status(error),
            );
        }
        cases
    }

    // -- model entitlement (ADR-0005, WS10-S3) ------------------------------

    fn applied(mode: mp_contracts::model_entitlement::Mode, policy: serde_json::Value) -> RequestAccess {
        let body = serde_json::json!({
            "state": "role",
            "revision": 9,
            "role": "member",
            "can_manage": false,
            "policy": policy,
            "sources": [{"layer": "role_override", "role": "member"}],
        });
        let resolved = mp_contracts::model_entitlement::ResolvedAccess::from_body(&body)
            .expect("a readable body");
        RequestAccess::new(std::sync::Arc::new(resolved), mode, "org-1")
    }

    fn catalogue() -> Vec<pb::ModelInfo> {
        let model = |id: &str, provider: &str, modality: &str| pb::ModelInfo {
            id: id.to_owned(),
            provider: provider.to_owned(),
            modality: modality.to_owned(),
            ..Default::default()
        };
        vec![
            model("gpt-5", "openai", "chat"),
            model("gpt-5-mini", "openai", "chat"),
            model("claude-sonnet-4-5", "anthropic", "chat"),
            model("text-embedding-3", "openai", "embedding"),
        ]
    }

    fn ids(models: &[pb::ModelInfo]) -> Vec<&str> {
        models.iter().map(|model| model.id.as_str()).collect()
    }

    #[test]
    fn an_enforced_catalogue_loses_denied_chat_models_and_keeps_every_other_modality() {
        use mp_contracts::model_entitlement::Mode;
        let policy = serde_json::json!({
            "allow_mode": "listed", "allow": ["gpt-5-mini"], "deny": [],
            "max_effort": "medium",
            "defaults": {"chat": {"model": "gpt-5-mini", "effort": "low"}},
        });
        let mut models = catalogue();
        let meta = catalogue_access(&applied(Mode::Enforce, policy.clone()), &mut models);
        // The embedding model is not a chat model: the policy does not touch it.
        assert_eq!(ids(&models), ["gpt-5-mini", "text-embedding-3"]);
        assert_eq!(meta.hidden_count, 2);
        assert!(meta.enforced && meta.restricted);
        assert_eq!(meta.state, "role");
        assert_eq!(meta.revision, 9);
        assert_eq!(meta.default_model, "gpt-5-mini");
        assert_eq!(meta.default_effort, "low");
        assert_eq!(meta.max_effort, "medium");

        // Shadow reports the policy and removes nothing.
        let mut models = catalogue();
        let meta = catalogue_access(&applied(Mode::Shadow, policy), &mut models);
        assert_eq!(models.len(), 4);
        assert!(!meta.enforced);
        assert_eq!(meta.hidden_count, 0);
    }

    #[test]
    fn a_provider_selector_filters_the_catalogue_by_the_provider_of_each_model() {
        use mp_contracts::model_entitlement::Mode;
        let policy = serde_json::json!({"allow_mode": "all", "allow": [], "deny": ["provider:anthropic"]});
        let mut models = catalogue();
        catalogue_access(&applied(Mode::Enforce, policy), &mut models);
        assert_eq!(ids(&models), ["gpt-5", "gpt-5-mini", "text-embedding-3"]);
    }

    fn chat_models() -> Vec<provider::ModelInfo> {
        catalogue()
            .into_iter()
            .filter(|model| model.modality == "chat")
            .map(|model| provider::ModelInfo {
                id: model.id,
                provider: model.provider,
                modality: model.modality,
                ..Default::default()
            })
            .collect()
    }

    fn access_request(model: &str) -> pb::GetModelAccessRequest {
        pb::GetModelAccessRequest {
            request_id: "r1".to_owned(),
            org_id: "org-1".to_owned(),
            model: model.to_owned(),
            surface: "chat".to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn get_model_access_judges_one_model_and_looks_its_provider_up_in_the_catalogue() {
        use mp_contracts::model_entitlement::Mode;
        let policy = serde_json::json!({
            "allow_mode": "listed", "allow": ["provider:openai"], "deny": ["gpt-5"],
            "min_privacy_tier": "eu_resident",
            "defaults": {"chat": {"model": "gpt-5-mini", "effort": "high"}},
        });
        let resolved = || Resolved::Access(std::sync::Arc::new(applied(Mode::Enforce, policy.clone())));

        let denied = model_access_response(&chat_models(), access_request("gpt-5"), resolved()).unwrap();
        assert!(!denied.model_allowed);
        assert_eq!(denied.reason, "explicitly_denied");
        assert!(denied.enforced && denied.restricted);
        assert_eq!(denied.state, "role");
        assert_eq!(denied.role, "member");
        assert_eq!(denied.default_model, "gpt-5-mini");
        assert_eq!(denied.default_effort, "high");
        assert_eq!(denied.min_privacy_tier, provider::PrivacyTier::EuResident.as_wire_i32());
        assert!(denied.catalogue.is_empty(), "the catalogue is asked for, not volunteered");

        // Allowed through its provider's selector, which only the catalogue knows.
        let allowed = model_access_response(&chat_models(), access_request("GPT-5-Mini"), resolved()).unwrap();
        assert!(allowed.model_allowed);
        assert_eq!(allowed.reason, "");

        // An automatic request is never judged as a model.
        for model in ["", "verevon-auto", "default"] {
            let auto = model_access_response(&chat_models(), access_request(model), resolved()).unwrap();
            assert!(auto.model_allowed, "{model:?}");
        }
    }

    #[test]
    fn get_model_access_says_it_is_not_enforced_when_nothing_was_resolved() {
        for resolved in [Resolved::Off, Resolved::Exempt, Resolved::Unreadable] {
            let answer =
                model_access_response(&chat_models(), access_request("gpt-5"), resolved).unwrap();
            assert!(!answer.enforced);
            assert!(!answer.restricted);
            assert!(answer.model_allowed);
            assert_eq!(answer.state, "none");
        }
    }

    #[test]
    fn a_draft_and_the_catalogue_are_the_policy_managers_and_need_a_resolved_answer() {
        use mp_contracts::model_entitlement::{Mode, ResolvedAccess};
        let manager = |mode| {
            let body = serde_json::json!({
                "state": "org", "revision": 2, "role": "owner", "can_manage": true,
                "policy": {"allow_mode": "all", "allow": [], "deny": []},
                "sources": [{"layer": "org_default"}],
            });
            Resolved::Access(std::sync::Arc::new(RequestAccess::new(
                std::sync::Arc::new(ResolvedAccess::from_body(&body).unwrap()),
                mode,
                "org-1",
            )))
        };
        let draft = pb::ModelPolicyDoc {
            allow_mode: "listed".to_owned(),
            allow: vec!["GPT-5-Mini".to_owned()],
            ..Default::default()
        };
        let request = pb::GetModelAccessRequest {
            draft: Some(draft),
            include_catalogue: true,
            ..access_request("")
        };

        // The draft is judged in place of the stored policy.
        let answer = model_access_response(&chat_models(), request.clone(), manager(Mode::Enforce)).unwrap();
        let entitled: Vec<(&str, bool)> = answer
            .catalogue
            .iter()
            .map(|entry| (entry.id.as_str(), entry.entitled))
            .collect();
        assert_eq!(
            entitled,
            [("gpt-5", false), ("gpt-5-mini", true), ("claude-sonnet-4-5", false)]
        );
        assert_eq!(answer.catalogue[0].reason, "not_in_allow_list");
        assert!(answer.restricted, "the draft restricts");
        assert!(answer.can_manage);

        // A member may not ask for either.
        let member = Resolved::Access(std::sync::Arc::new(applied(
            Mode::Enforce,
            serde_json::json!({"allow_mode": "all", "allow": [], "deny": []}),
        )));
        let refused = model_access_response(&chat_models(), request.clone(), member).unwrap_err();
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);
        let service = model_access_response(&chat_models(), request.clone(), Resolved::Exempt).unwrap_err();
        assert_eq!(service.code(), tonic::Code::PermissionDenied);

        // With nothing resolved there is nothing to answer from.
        for resolved in [Resolved::Off, Resolved::Unreadable] {
            let status = model_access_response(&chat_models(), request.clone(), resolved).unwrap_err();
            assert_eq!(status.code(), tonic::Code::Unavailable);
            assert_eq!(
                trailer(&status, "x-mp-provider-error"),
                Some("model_entitlement_unavailable")
            );
        }
    }

    #[test]
    fn the_verdicts_say_which_chat_models_are_zdr_attested_whatever_the_draft_refuses() {
        use mp_contracts::model_entitlement::{Mode, ResolvedAccess};
        let manager = Resolved::Access(std::sync::Arc::new(RequestAccess::new(
            std::sync::Arc::new(
                ResolvedAccess::from_body(&serde_json::json!({
                    "state": "org", "revision": 2, "role": "owner", "can_manage": true,
                    "policy": {"allow_mode": "all", "allow": [], "deny": []},
                    "sources": [{"layer": "org_default"}],
                }))
                .unwrap(),
            ),
            Mode::Enforce,
            "org-1",
        )));
        let chat = |id: &str, features: &[&str], tier: provider::PrivacyTier| provider::ModelInfo {
            id: id.to_owned(),
            provider: "openai".to_owned(),
            modality: "chat".to_owned(),
            features: features
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect(),
            privacy_tier: tier,
            ..Default::default()
        };
        let models = vec![
            // Attested by the feature the provider advertises.
            chat(
                "gpt-by-feature",
                &["zdr"],
                provider::PrivacyTier::Unspecified,
            ),
            // Attested by a tier only a verified ZDR contract reaches.
            chat("gpt-by-tier", &[], provider::PrivacyTier::ZdrContractual),
            chat("claude-sovereign", &[], provider::PrivacyTier::Sovereign),
            // EU residency alone is not a zero-retention attestation.
            chat("gpt-eu", &["tools"], provider::PrivacyTier::EuResident),
            chat("gpt-open", &[], provider::PrivacyTier::Global),
        ];
        // The draft allows only the open model, so the attested ones are refused.
        let request = pb::GetModelAccessRequest {
            draft: Some(pb::ModelPolicyDoc {
                allow_mode: "listed".to_owned(),
                allow: vec!["gpt-open".to_owned()],
                ..Default::default()
            }),
            include_catalogue: true,
            ..access_request("")
        };

        let answer = model_access_response(&models, request, manager).unwrap();
        let verdicts: Vec<(&str, bool, bool)> = answer
            .catalogue
            .iter()
            .map(|entry| (entry.id.as_str(), entry.entitled, entry.zdr_attested))
            .collect();
        assert_eq!(
            verdicts,
            [
                ("gpt-by-feature", false, true),
                ("gpt-by-tier", false, true),
                ("claude-sovereign", false, true),
                ("gpt-eu", false, false),
                ("gpt-open", true, false),
            ]
        );
    }

    #[test]
    fn the_entitlement_refusals_reach_the_wire_as_permission_denied_and_unavailable() {
        use provider::ProviderError as E;
        let denied = provider_error_to_status(E::ModelNotEntitled {
            model: "gpt-5".to_owned(),
            reason: mp_contracts::model_entitlement::Reason::NotInAllowList,
        });
        assert_eq!(denied.code(), tonic::Code::PermissionDenied);
        assert_eq!(trailer(&denied, "x-mp-provider-error"), Some("model_not_entitled"));
        assert_eq!(trailer(&denied, "x-mp-entitlement-reason"), Some("not_in_allow_list"));
        // The model id is named in the message and never in a trailer.
        assert!(denied.message().contains("gpt-5"));

        let unavailable = provider_error_to_status(E::EntitlementUnavailable);
        assert_eq!(unavailable.code(), tonic::Code::Unavailable);
        assert_eq!(
            trailer(&unavailable, "x-mp-provider-error"),
            Some("model_entitlement_unavailable")
        );
        // Never the status the gateway reads as a posture or ZDR refusal.
        for status in [&denied, &unavailable] {
            assert_ne!(status.code(), tonic::Code::FailedPrecondition);
        }
    }

    #[test]
    fn the_policy_tier_joins_the_requests_own_as_the_stronger_of_the_two() {
        use mp_contracts::model_entitlement::Mode;
        let policy = serde_json::json!({
            "allow_mode": "all", "allow": [], "deny": [], "min_privacy_tier": "eu_resident",
        });
        let access = std::sync::Arc::new(applied(Mode::Enforce, policy.clone()));
        let weaker = with_access(provider::InferRequest::default(), Some(access.clone()));
        assert_eq!(weaker.min_privacy_tier, provider::PrivacyTier::EuResident);
        let stronger = with_access(
            provider::InferRequest {
                min_privacy_tier: provider::PrivacyTier::Sovereign,
                ..Default::default()
            },
            Some(access),
        );
        assert_eq!(stronger.min_privacy_tier, provider::PrivacyTier::Sovereign);
        // Shadow and no answer leave the request untouched.
        let shadow = with_access(
            provider::InferRequest::default(),
            Some(std::sync::Arc::new(applied(Mode::Shadow, policy))),
        );
        assert_eq!(shadow.min_privacy_tier, provider::PrivacyTier::Unspecified);
        assert!(with_access(provider::InferRequest::default(), None).access.is_none());
    }

    #[test]
    fn an_embedding_posture_refusal_is_typed_and_any_other_failure_stays_internal() {
        use provider::ProviderError as E;
        for (error, kind) in [
            (E::ZdrUnavailable("zdr".to_owned()), "zdr_unavailable"),
            (
                E::ResidencyViolation("residency".to_owned()),
                "residency_unavailable",
            ),
            (E::TierUnavailable("tier".to_owned()), "tier_unavailable"),
        ] {
            let message = match &error {
                E::ZdrUnavailable(m) | E::ResidencyViolation(m) | E::TierUnavailable(m) => {
                    m.clone()
                }
                _ => unreachable!(),
            };
            let status = embedding_error_to_status(error);
            assert_eq!(status.code(), tonic::Code::FailedPrecondition, "{kind}");
            assert_eq!(status.message(), message, "{kind}");
            assert_eq!(trailer(&status, "x-mp-provider-error"), Some(kind));
        }
        for error in [
            E::Http("upstream reset".to_owned()),
            E::RateLimited {
                retry_after_ms: 1_000,
            },
            E::AllExhausted { attempts: 2 },
        ] {
            let status = embedding_error_to_status(error);
            assert_eq!(status.code(), tonic::Code::Internal);
            assert!(
                status.metadata().is_empty(),
                "an INTERNAL embedding failure is untyped"
            );
        }
    }

    fn image_request(messages: &[&str], parts: Vec<pb::InferImagePart>) -> pb::InferRequest {
        pb::InferRequest {
            messages: messages
                .iter()
                .map(|role| pb::ChatMessage {
                    role: (*role).to_owned(),
                    content: "look".to_owned(),
                    ..Default::default()
                })
                .collect(),
            image_parts: parts,
            ..Default::default()
        }
    }

    fn image_part(message_index: u32, bytes: usize, mime: &str) -> pb::InferImagePart {
        pb::InferImagePart {
            message_index,
            data: vec![7; bytes].into(),
            mime_type: mime.to_owned(),
        }
    }

    #[test]
    fn image_parts_map_onto_the_internal_request_by_original_index() {
        let principal = AuthenticatedPrincipal::for_test("org-a", Some("user-a"), false);
        let request = image_request(
            &["system", "user"],
            vec![image_part(1, 3, "image/png"), image_part(1, 5, "image/webp")],
        );
        let mapped = to_internal_request(&request, &principal).expect("maps");
        assert_eq!(mapped.image_parts.len(), 2);
        assert_eq!(mapped.image_parts[0].message_index, 1);
        assert_eq!(mapped.image_parts[1].data.len(), 5);
        assert_eq!(mapped.image_parts[1].mime_type, "image/webp");

        let text_only = to_internal_request(&image_request(&["user"], Vec::new()), &principal)
            .expect("maps");
        assert!(text_only.image_parts.is_empty());
    }

    #[test]
    fn image_parts_that_break_a_limit_are_refused_never_trimmed() {
        let principal = AuthenticatedPrincipal::for_test("org-a", Some("user-a"), false);
        let refused = |request: pb::InferRequest| {
            let status = to_internal_request(&request, &principal).expect_err("refused");
            assert_eq!(status.code(), tonic::Code::InvalidArgument, "{}", status.message());
            status.message().to_owned()
        };
        let five = (0..5).map(|_| image_part(0, 1, "image/png")).collect();
        assert!(refused(image_request(&["user"], five)).contains("at most 4"));
        assert!(refused(image_request(&["user"], vec![image_part(1, 1, "image/png")]))
            .contains("does not exist"));
        assert!(refused(image_request(
            &["user", "assistant"],
            vec![image_part(1, 1, "image/png")]
        ))
        .contains("user message"));
        assert!(refused(image_request(&["user"], vec![image_part(0, 0, "image/png")]))
            .contains("between 1 byte"));
        assert!(refused(image_request(
            &["user"],
            vec![image_part(0, MAX_IMAGE_BYTES + 1, "image/png")]
        ))
        .contains("between 1 byte"));
        let message = refused(image_request(&["user"], vec![image_part(0, 1, "image/svg+xml")]));
        assert!(message.contains("unsupported mime"));
        assert!(!message.contains("svg"), "the refusal never echoes caller text");

        // The limits themselves are accepted.
        let four = (0..4).map(|_| image_part(0, 1, "image/gif")).collect();
        assert!(to_internal_request(&image_request(&["user"], four), &principal).is_ok());
        assert!(to_internal_request(
            &image_request(&["user"], vec![image_part(0, MAX_IMAGE_BYTES, "image/jpeg")]),
            &principal
        )
        .is_ok());
    }

    #[test]
    fn a_vision_refusal_is_failed_precondition_with_its_own_kind() {
        let status = provider_error_to_status(provider::ProviderError::VisionUnsupported(
            "no vision".to_owned(),
        ));
        assert_eq!(status.code(), tonic::Code::FailedPrecondition);
        assert_eq!(status.message(), "no vision");
        // A literal, not the constant: renaming the kind must fail this test.
        assert_eq!(trailer(&status, "x-mp-provider-error"), Some("vision_unsupported"));
        assert_eq!(trailer(&status, "x-mp-retry-after-ms"), None);
    }

    /// Exhaustive on purpose: a new `ProviderError` variant does not compile
    /// until it is given a name here, so it cannot reach the wire without
    /// reaching the committed contract.
    fn status_contract_source(error: &provider::ProviderError) -> &'static str {
        use provider::ProviderError as E;
        match error {
            E::Http(_) => "ProviderError::Http",
            E::RateLimited { .. } => "ProviderError::RateLimited",
            E::SubscriptionRefused { .. } => "ProviderError::SubscriptionRefused",
            E::InvalidResponse(_) => "ProviderError::InvalidResponse",
            E::Unavailable(_) => "ProviderError::Unavailable",
            E::SubscriptionFailed(_) => "ProviderError::SubscriptionFailed",
            E::TooLong { .. } => "ProviderError::TooLong",
            E::RequestRejected { .. } => "ProviderError::RequestRejected",
            E::AllExhausted { .. } => "ProviderError::AllExhausted",
            E::UnsupportedModel(_) => "ProviderError::UnsupportedModel",
            E::ResidencyViolation(_) => "ProviderError::ResidencyViolation",
            E::ZdrUnavailable(_) => "ProviderError::ZdrUnavailable",
            E::TierUnavailable(_) => "ProviderError::TierUnavailable",
            E::VisionUnsupported(_) => "ProviderError::VisionUnsupported",
            E::ModelNotEntitled { .. } => "ProviderError::ModelNotEntitled",
            E::EntitlementUnavailable => "ProviderError::EntitlementUnavailable",
        }
    }

    fn status_contract_id(source: &str) -> String {
        let name = source.trim_start_matches("ProviderError::");
        let mut id = String::new();
        for (index, ch) in name.chars().enumerate() {
            if ch.is_ascii_uppercase() && index > 0 {
                id.push('_');
            }
            id.push(ch.to_ascii_lowercase());
        }
        format!("provider_{id}")
    }

    fn grpc_code_name(code: tonic::Code) -> &'static str {
        match code {
            tonic::Code::InvalidArgument => "INVALID_ARGUMENT",
            tonic::Code::PermissionDenied => "PERMISSION_DENIED",
            tonic::Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
            tonic::Code::FailedPrecondition => "FAILED_PRECONDITION",
            tonic::Code::Unimplemented => "UNIMPLEMENTED",
            tonic::Code::Internal => "INTERNAL",
            tonic::Code::Unavailable => "UNAVAILABLE",
            other => panic!("the status contract has no name for {other:?}; add it"),
        }
    }

    #[derive(serde::Serialize)]
    struct StatusContract {
        contract: &'static str,
        version: u32,
        trailer_keys: std::collections::BTreeMap<&'static str, &'static str>,
        kinds: Vec<String>,
        cases: Vec<StatusContractCase>,
    }

    #[derive(serde::Serialize)]
    struct StatusContractCase {
        id: String,
        source: &'static str,
        grpc_code: &'static str,
        grpc_code_number: i32,
        trailers: std::collections::BTreeMap<String, String>,
    }

    /// The contract as bytes: pretty JSON, LF endings, one trailing newline.
    /// Built from structs and `BTreeMap`s so the byte order never depends on
    /// which `serde_json` features another crate in the build turned on.
    fn render_status_contract() -> String {
        let mut kinds = std::collections::BTreeSet::new();
        let mut cases = Vec::new();
        for (id, source, status) in status_contract_cases() {
            let trailers: std::collections::BTreeMap<String, String> = status
                .metadata()
                .iter()
                .filter_map(|entry| match entry {
                    tonic::metadata::KeyAndValueRef::Ascii(key, value) => {
                        Some((key.as_str().to_owned(), value.to_str().ok()?.to_owned()))
                    }
                    tonic::metadata::KeyAndValueRef::Binary(..) => None,
                })
                .collect();
            if let Some(kind) = trailers.get("x-mp-provider-error") {
                kinds.insert(kind.clone());
            }
            cases.push(StatusContractCase {
                id,
                source,
                grpc_code: grpc_code_name(status.code()),
                grpc_code_number: status.code() as i32,
                trailers,
            });
        }
        let contract = StatusContract {
            contract: "inference-core.infer-statuses",
            version: 1,
            trailer_keys: [
                ("kind", "x-mp-provider-error"),
                ("resets_at", "x-mp-resets-at"),
                ("retry_after_ms", "x-mp-retry-after-ms"),
            ]
            .into_iter()
            .collect(),
            kinds: kinds.into_iter().collect(),
            cases,
        };
        let mut text = serde_json::to_string_pretty(&contract).expect("the contract serializes");
        text.push('\n');
        text
    }

    #[test]
    fn the_committed_status_contract_is_what_the_code_produces() {
        let generated = render_status_contract();
        if std::env::var("MP_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
            std::fs::create_dir_all(std::path::Path::new(STATUS_CONTRACT_PATH).parent().unwrap())
                .expect("the contracts directory");
            std::fs::write(STATUS_CONTRACT_PATH, &generated).expect("write the status contract");
            return;
        }
        let committed = std::fs::read_to_string(STATUS_CONTRACT_PATH).unwrap_or_else(|error| {
            panic!(
                "cannot read {STATUS_CONTRACT_PATH} ({error}). Generate it with \
                 MP_UPDATE_GOLDEN=1 cargo test -p inference-core --lib status_contract, \
                 then pin its digest in scripts/tests/zdr-contract-sync.test.sh."
            )
        });
        assert!(
            !committed.contains('\r'),
            "the status contract must keep LF line endings (see its .gitattributes)"
        );
        assert_eq!(
            committed, generated,
            "the committed status contract differs from what the code produces. If the change \
             is intended, regenerate it with MP_UPDATE_GOLDEN=1 and update the pinned digest \
             in scripts/tests/zdr-contract-sync.test.sh."
        );
    }
}

/// The metered call path under the real rules (real tokens, real provider
/// configuration, rendered Prometheus text). A file of its own, so the tests that
/// build a whole service stay out of this one.
#[cfg(test)]
mod real_rules_tests;

/// The inference boundary under a connection-bound zero-retention waiver (Z-16): the
/// route the waiver binds is the only route it reaches, and a waiver never covers a
/// non-chat call or a persistent token of a ZDR org. A file of its own, like
/// `real_rules_tests`.
#[cfg(test)]
mod zdr_waiver_route_tests;

/// The zero-data-retention refusals of the modality surface are typed through the
/// catalogue: every handler that refuses a modality under ZDR does it through
/// `reject_unattested_zdr_modality`, every operation it names comes back as the
/// frozen `zdr_unavailable` kind, and that kind is the catalogue's code.
#[cfg(test)]
mod zdr_refusal_catalogue {
    use super::*;
    use mp_contracts::zdr_refusal::{code, Refusal};

    /// The gRPC handlers that refuse a modality under ZDR, by function name. Each
    /// must call `reject_unattested_zdr_modality` in its body.
    const REFUSING_HANDLERS: [&str; 14] = [
        "synthesize_speech_as",
        "transcribe_speech_as",
        "translate_text",
        "batch_translate_text",
        "detect_text_language",
        "generate_image",
        "analyze_image",
        "extract_image_text",
        "analyze_document",
        "analyze_language",
        "create_realtime_session",
        "create_video_generation_job",
        "get_video_generation_job",
        "stream_video_generation_content",
    ];

    /// The operations those handlers name. A client sees each in the message.
    const OPERATIONS: [&str; 13] = [
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
    ];

    /// This file's code, without its test module.
    fn production_source() -> &'static str {
        include_str!("grpc.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the file starts with code")
    }

    #[test]
    fn every_refusing_handler_refuses_through_the_one_helper() {
        let source = production_source();
        for handler in REFUSING_HANDLERS {
            let start = source
                .find(&format!("fn {handler}("))
                .unwrap_or_else(|| panic!("{handler} is declared in grpc.rs"));
            let window = &source[start..(start + 1500).min(source.len())];
            assert!(
                window.contains("reject_unattested_zdr_modality("),
                "{handler} refuses a modality under ZDR without reject_unattested_zdr_modality"
            );
        }
    }

    #[test]
    fn every_refused_modality_is_the_catalogue_refusal_with_its_operation_named() {
        for operation in OPERATIONS {
            let principal = AuthenticatedPrincipal::for_test("org-zdr", Some("user-zdr"), true);
            let status =
                reject_unattested_zdr_modality(&principal, false, operation).expect_err(operation);
            assert_eq!(
                status.code(),
                tonic::Code::FailedPrecondition,
                "{operation}"
            );
            assert_eq!(
                Refusal::from_status(&status),
                Some(Refusal::UNAVAILABLE),
                "{operation}"
            );
            assert!(
                status.message().contains(operation),
                "{operation}: {}",
                status.message()
            );
        }
    }

    #[test]
    fn the_frozen_kind_is_the_catalogue_code() {
        assert_eq!(PROVIDER_ERROR_ZDR_UNAVAILABLE, code::UNAVAILABLE);
    }

    #[test]
    fn an_unrestricted_caller_is_not_refused_on_any_modality() {
        let principal = AuthenticatedPrincipal::for_test("org-a", Some("user-a"), false);
        for operation in OPERATIONS {
            assert!(
                reject_unattested_zdr_modality(&principal, false, operation).is_ok(),
                "{operation}"
            );
        }
    }
}
