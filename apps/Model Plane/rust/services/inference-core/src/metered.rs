//! Metering for every `InferenceCore` RPC, in one place.
//!
//! [`MeteredCore`] wraps the real service and implements the same generated
//! trait, so each of its RPCs is counted whatever its handler does and a new RPC
//! cannot ship unmetered: adding a method to the proto stops this file
//! compiling until the method is delegated here.
//!
//! What a call is counted under:
//!
//! * the **caller**: `authorize` in `grpc.rs` publishes the verified principal
//!   into a per-request [`CallerSlot`] right after the credential verifies, so
//!   a service that is then refused on scope or tenant is still counted under
//!   its own id. The slot is write-only from the service's side: nothing is ever
//!   trusted from it, and a call that never verified stays `unauthenticated`.
//! * the **serving model and provider**: read from the response (`model_used`,
//!   `provider_used`), never from the request, so a caller cannot name a label.
//!   A failed call reports `unknown`; a catalogue read reports `none`.
//! * the **outcome**: `ok`, the typed provider-error kind, or the gRPC code. See
//!   [`crate::metrics`].
//!
//! A streamed answer is counted when the stream ends, once: `ok` on the final
//! chunk (with its tokens), the failure's outcome on an error, `stream_incomplete`
//! when it ends with no final chunk or the provider flagged the turn so, and
//! `cancelled` when the consumer drops it first. A unary call whose caller gives
//! up while the provider is still working is `cancelled` too: the provider may
//! already be spending on it. A response the chain answered from its
//! completed-response cache is counted as a call and adds no tokens, because no
//! provider was reached and the tokens it carries were counted when the call
//! that filled the cache was served.

#![allow(clippy::result_large_err)]

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};

use mp_contracts::model_plane::v1::{self as pb, inference_core_server::InferenceCore};
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::auth::AuthenticatedPrincipal;
use crate::metrics;

/// The verified caller of one request, filled by `authorize`.
#[derive(Clone, Default)]
struct CallerSlot(Arc<OnceLock<String>>);

impl CallerSlot {
    fn label(&self) -> &str {
        self.0
            .get()
            .map_or(metrics::UNAUTHENTICATED, String::as_str)
    }
}

/// Record the verified caller of the request whose `extensions` these are, for
/// the metering decorator to label the call with. A no-op when the service runs
/// without the decorator (a unit test that calls `authorize` directly).
pub(crate) fn note_caller(extensions: &tonic::Extensions, principal: &AuthenticatedPrincipal) {
    if let Some(slot) = extensions.get::<CallerSlot>() {
        // The first verification wins; `authorize` runs once per request.
        let _ = slot.0.set(metrics::caller_label(principal));
    }
}

/// What a response says about who served it.
pub(crate) struct ServedBy<'a> {
    model: &'a str,
    provider: &'a str,
    input_tokens: i32,
    output_tokens: i32,
}

/// A response (or stream chunk) the meter can attribute.
pub(crate) trait Served {
    /// The serving model, provider and tokens; `None` for a response that
    /// serves no model, such as a catalogue.
    fn served(&self) -> Option<ServedBy<'_>>;

    /// Whether this ends the call. Always true for a unary response; for a
    /// stream, only the final chunk carries the model, provider and tokens.
    fn is_final(&self) -> bool {
        true
    }

    /// The outcome of a call that ended with this response.
    fn outcome(&self) -> &'static str {
        "ok"
    }

    /// Whether no provider was reached for this response: it came out of the
    /// completed-response cache. The stored copy still carries the token counts
    /// of the call that filled the cache; they were counted when that call was
    /// served, and counting them again would charge a cache hit as a provider
    /// call. The call itself is still counted.
    fn was_cached(&self) -> bool {
        false
    }
}

macro_rules! served_by_model_and_provider {
    ($($response:ty),+ $(,)?) => {
        $(impl Served for $response {
            fn served(&self) -> Option<ServedBy<'_>> {
                Some(ServedBy {
                    model: &self.model_used,
                    provider: &self.provider_used,
                    input_tokens: 0,
                    output_tokens: 0,
                })
            }
        })+
    };
}

macro_rules! served_with_usage {
    ($($response:ty),+ $(,)?) => {
        $(impl Served for $response {
            fn served(&self) -> Option<ServedBy<'_>> {
                Some(ServedBy {
                    model: &self.model_used,
                    provider: &self.provider_used,
                    input_tokens: self.input_tokens,
                    output_tokens: self.output_tokens,
                })
            }
        })+
    };
}

macro_rules! serves_no_model {
    ($($response:ty),+ $(,)?) => {
        $(impl Served for $response {
            fn served(&self) -> Option<ServedBy<'_>> {
                None
            }
        })+
    };
}

served_by_model_and_provider!(
    pb::CreateEmbeddingResponse,
    pb::SynthesizeSpeechResponse,
    pb::TranscribeSpeechResponse,
    pb::TranslateTextResponse,
    pb::BatchTranslateTextResponse,
    pb::DetectTextLanguageResponse,
    pb::GenerateImageResponse,
    pb::ExtractImageTextResponse,
    pb::AnalyzeDocumentResponse,
    pb::AnalyzeLanguageResponse,
    pb::CreateRealtimeSessionResponse,
    pb::CreateVideoGenerationJobResponse,
    pb::GetVideoGenerationJobResponse,
);

// `input_tokens` already includes the prompt-cache read and write legs, so the
// separate `cache_*` fields are deliberately not added again.
served_with_usage!(pb::AnalyzeImageResponse);

impl Served for pb::InferResponse {
    fn served(&self) -> Option<ServedBy<'_>> {
        Some(ServedBy {
            model: &self.model_used,
            provider: &self.provider_used,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
        })
    }

    /// The chain stamps a response it answered from its completed-response cache
    /// with `response_cache_hit` in the timing it reports.
    fn was_cached(&self) -> bool {
        self.provider_timing
            .as_ref()
            .is_some_and(|timing| timing.response_cache_hit)
    }
}

serves_no_model!(
    pb::ListModelsResponse,
    pb::ListSpeechVoicesResponse,
    pb::ListTranslationLanguagesResponse,
);

impl Served for pb::InferChunk {
    fn served(&self) -> Option<ServedBy<'_>> {
        Some(ServedBy {
            model: &self.model_used,
            provider: &self.provider_used,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
        })
    }

    fn is_final(&self) -> bool {
        self.done
    }

    /// A connection that broke before any proper end reaches the consumer as a
    /// final chunk flagged `stream_incomplete`; that is not a successful answer.
    fn outcome(&self) -> &'static str {
        if self.stop_reason == "stream_incomplete" {
            "stream_incomplete"
        } else {
            "ok"
        }
    }
}

impl Served for pb::StreamVideoGenerationContentResponse {
    fn served(&self) -> Option<ServedBy<'_>> {
        Some(ServedBy {
            model: "",
            provider: &self.provider_used,
            input_tokens: 0,
            output_tokens: 0,
        })
    }

    fn is_final(&self) -> bool {
        self.done
    }
}

fn non_negative(tokens: i32) -> u64 {
    u64::try_from(tokens).unwrap_or(0)
}

/// One RPC being counted, exactly once: by the first of its answer, its failure
/// or its end. When none of those ever happens because the caller dropped the
/// call first, it is counted as `cancelled` when it is dropped.
struct Call {
    rpc: &'static str,
    caller: CallerSlot,
    settled: AtomicBool,
}

impl Call {
    /// Start counting `rpc`, handing the request on with its caller slot.
    fn begin<T>(rpc: &'static str, mut request: Request<T>) -> (Self, Request<T>) {
        let caller = CallerSlot::default();
        request.extensions_mut().insert(caller.clone());
        (
            Self {
                rpc,
                caller,
                settled: AtomicBool::new(false),
            },
            request,
        )
    }

    /// Claim the one count this call gets. False when it has been counted.
    fn settle(&self) -> bool {
        !self.settled.swap(true, Ordering::AcqRel)
    }

    fn finish<T: Served>(self, result: Result<Response<T>, Status>) -> Result<Response<T>, Status> {
        match &result {
            Ok(response) => self.answered(response.get_ref()),
            Err(status) => self.failed(status),
        }
        result
    }

    fn finish_stream<T, S>(
        self,
        result: Result<Response<S>, Status>,
    ) -> Result<Response<Tapped<T>>, Status>
    where
        T: Served + Send + 'static,
        S: Stream<Item = Result<T, Status>> + Send + 'static,
    {
        match result {
            Ok(response) => Ok(response.map(|stream| Tapped::new(self, Box::pin(stream)))),
            Err(status) => {
                self.failed(&status);
                Err(status)
            }
        }
    }

    fn answered<T: Served>(&self, response: &T) {
        if !self.settle() {
            return;
        }
        let service = self.caller.label();
        match response.served() {
            Some(by) => {
                metrics::record_request(
                    self.rpc,
                    service,
                    by.model,
                    by.provider,
                    response.outcome(),
                );
                if !response.was_cached() {
                    metrics::record_tokens(
                        service,
                        by.model,
                        by.provider,
                        non_negative(by.input_tokens),
                        non_negative(by.output_tokens),
                    );
                }
            }
            None => metrics::record_request(self.rpc, service, metrics::NONE, metrics::NONE, "ok"),
        }
    }

    fn failed(&self, status: &Status) {
        self.ended(&metrics::outcome_of(status));
    }

    fn ended(&self, outcome: &str) {
        if !self.settle() {
            return;
        }
        metrics::record_request(
            self.rpc,
            self.caller.label(),
            metrics::UNKNOWN,
            metrics::UNKNOWN,
            outcome,
        );
    }
}

impl Drop for Call {
    /// A call dropped before it answered (the caller gave up, or the connection
    /// broke) is still a call: the provider may already be spending on it. A
    /// call that was counted is not counted again.
    fn drop(&mut self) {
        self.ended("cancelled");
    }
}

/// A server stream that counts its call exactly once, when it ends.
pub struct Tapped<T> {
    inner: Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>,
    call: Call,
    counted: bool,
}

impl<T> Tapped<T> {
    fn new(call: Call, inner: Pin<Box<dyn Stream<Item = Result<T, Status>> + Send>>) -> Self {
        Self {
            inner,
            call,
            counted: false,
        }
    }

    fn count_once(&mut self, count: impl FnOnce(&Call)) {
        if !self.counted {
            self.counted = true;
            count(&self.call);
        }
    }
}

impl<T: Served> Stream for Tapped<T> {
    type Item = Result<T, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        let polled = this.inner.as_mut().poll_next(cx);
        match &polled {
            Poll::Ready(Some(Ok(item))) if item.is_final() => {
                this.count_once(|call| call.answered(item));
            }
            Poll::Ready(Some(Err(status))) => this.count_once(|call| call.failed(status)),
            Poll::Ready(None) => this.count_once(|call| call.ended("stream_incomplete")),
            _ => {}
        }
        polled
    }
}

impl<T> Drop for Tapped<T> {
    fn drop(&mut self) {
        self.count_once(|call| call.ended("cancelled"));
    }
}

/// The `InferenceCore` service with every RPC metered.
pub struct MeteredCore<S> {
    inner: S,
}

impl<S> MeteredCore<S> {
    #[must_use]
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

#[tonic::async_trait]
impl<S: InferenceCore> InferenceCore for MeteredCore<S> {
    async fn infer(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<pb::InferResponse>, Status> {
        let (call, request) = Call::begin("infer", request);
        call.finish(self.inner.infer(request).await)
    }

    type InferStreamStream = Tapped<pb::InferChunk>;

    async fn infer_stream(
        &self,
        request: Request<pb::InferRequest>,
    ) -> Result<Response<Self::InferStreamStream>, Status> {
        let (call, request) = Call::begin("infer_stream", request);
        call.finish_stream(self.inner.infer_stream(request).await)
    }

    async fn create_embedding(
        &self,
        request: Request<pb::CreateEmbeddingRequest>,
    ) -> Result<Response<pb::CreateEmbeddingResponse>, Status> {
        let (call, request) = Call::begin("create_embedding", request);
        call.finish(self.inner.create_embedding(request).await)
    }

    async fn list_models(
        &self,
        request: Request<pb::ListModelsRequest>,
    ) -> Result<Response<pb::ListModelsResponse>, Status> {
        let (call, request) = Call::begin("list_models", request);
        call.finish(self.inner.list_models(request).await)
    }

    async fn synthesize_speech(
        &self,
        request: Request<pb::SynthesizeSpeechRequest>,
    ) -> Result<Response<pb::SynthesizeSpeechResponse>, Status> {
        let (call, request) = Call::begin("synthesize_speech", request);
        call.finish(self.inner.synthesize_speech(request).await)
    }

    async fn transcribe_speech(
        &self,
        request: Request<pb::TranscribeSpeechRequest>,
    ) -> Result<Response<pb::TranscribeSpeechResponse>, Status> {
        let (call, request) = Call::begin("transcribe_speech", request);
        call.finish(self.inner.transcribe_speech(request).await)
    }

    async fn list_speech_voices(
        &self,
        request: Request<pb::ListSpeechVoicesRequest>,
    ) -> Result<Response<pb::ListSpeechVoicesResponse>, Status> {
        let (call, request) = Call::begin("list_speech_voices", request);
        call.finish(self.inner.list_speech_voices(request).await)
    }

    async fn translate_text(
        &self,
        request: Request<pb::TranslateTextRequest>,
    ) -> Result<Response<pb::TranslateTextResponse>, Status> {
        let (call, request) = Call::begin("translate_text", request);
        call.finish(self.inner.translate_text(request).await)
    }

    async fn batch_translate_text(
        &self,
        request: Request<pb::BatchTranslateTextRequest>,
    ) -> Result<Response<pb::BatchTranslateTextResponse>, Status> {
        let (call, request) = Call::begin("batch_translate_text", request);
        call.finish(self.inner.batch_translate_text(request).await)
    }

    async fn detect_text_language(
        &self,
        request: Request<pb::DetectTextLanguageRequest>,
    ) -> Result<Response<pb::DetectTextLanguageResponse>, Status> {
        let (call, request) = Call::begin("detect_text_language", request);
        call.finish(self.inner.detect_text_language(request).await)
    }

    async fn list_translation_languages(
        &self,
        request: Request<pb::ListTranslationLanguagesRequest>,
    ) -> Result<Response<pb::ListTranslationLanguagesResponse>, Status> {
        let (call, request) = Call::begin("list_translation_languages", request);
        call.finish(self.inner.list_translation_languages(request).await)
    }

    async fn generate_image(
        &self,
        request: Request<pb::GenerateImageRequest>,
    ) -> Result<Response<pb::GenerateImageResponse>, Status> {
        let (call, request) = Call::begin("generate_image", request);
        call.finish(self.inner.generate_image(request).await)
    }

    async fn analyze_image(
        &self,
        request: Request<pb::AnalyzeImageRequest>,
    ) -> Result<Response<pb::AnalyzeImageResponse>, Status> {
        let (call, request) = Call::begin("analyze_image", request);
        call.finish(self.inner.analyze_image(request).await)
    }

    async fn extract_image_text(
        &self,
        request: Request<pb::ExtractImageTextRequest>,
    ) -> Result<Response<pb::ExtractImageTextResponse>, Status> {
        let (call, request) = Call::begin("extract_image_text", request);
        call.finish(self.inner.extract_image_text(request).await)
    }

    async fn analyze_document(
        &self,
        request: Request<pb::AnalyzeDocumentRequest>,
    ) -> Result<Response<pb::AnalyzeDocumentResponse>, Status> {
        let (call, request) = Call::begin("analyze_document", request);
        call.finish(self.inner.analyze_document(request).await)
    }

    async fn analyze_language(
        &self,
        request: Request<pb::AnalyzeLanguageRequest>,
    ) -> Result<Response<pb::AnalyzeLanguageResponse>, Status> {
        let (call, request) = Call::begin("analyze_language", request);
        call.finish(self.inner.analyze_language(request).await)
    }

    async fn create_realtime_session(
        &self,
        request: Request<pb::CreateRealtimeSessionRequest>,
    ) -> Result<Response<pb::CreateRealtimeSessionResponse>, Status> {
        let (call, request) = Call::begin("create_realtime_session", request);
        call.finish(self.inner.create_realtime_session(request).await)
    }

    async fn create_video_generation_job(
        &self,
        request: Request<pb::CreateVideoGenerationJobRequest>,
    ) -> Result<Response<pb::CreateVideoGenerationJobResponse>, Status> {
        let (call, request) = Call::begin("create_video_generation_job", request);
        call.finish(self.inner.create_video_generation_job(request).await)
    }

    async fn get_video_generation_job(
        &self,
        request: Request<pb::GetVideoGenerationJobRequest>,
    ) -> Result<Response<pb::GetVideoGenerationJobResponse>, Status> {
        let (call, request) = Call::begin("get_video_generation_job", request);
        call.finish(self.inner.get_video_generation_job(request).await)
    }

    type StreamVideoGenerationContentStream = Tapped<pb::StreamVideoGenerationContentResponse>;

    async fn stream_video_generation_content(
        &self,
        request: Request<pb::StreamVideoGenerationContentRequest>,
    ) -> Result<Response<Self::StreamVideoGenerationContentStream>, Status> {
        let (call, request) = Call::begin("stream_video_generation_content", request);
        call.finish_stream(self.inner.stream_video_generation_content(request).await)
    }
}

#[cfg(test)]
mod tests {
    use metrics_exporter_prometheus::PrometheusBuilder;
    use tokio_stream::StreamExt;

    use super::*;

    /// Run `body` against a fresh recorder and return what it rendered. A
    /// current-thread runtime keeps every counter on the thread the recorder is
    /// scoped to.
    fn rendered<F: std::future::Future<Output = ()>>(body: F) -> String {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        ::metrics::with_local_recorder(&recorder, || {
            tokio::runtime::Builder::new_current_thread()
                .build()
                .expect("runtime")
                .block_on(body);
        });
        handle.render()
    }

    fn series(text: &str, name: &str, labels: &[&str]) -> Option<String> {
        text.lines()
            .find(|line| line.starts_with(name) && labels.iter().all(|label| line.contains(label)))
            .and_then(|line| line.rsplit(' ').next())
            .map(str::to_owned)
    }

    fn chunk(done: bool, stop_reason: &str) -> pb::InferChunk {
        pb::InferChunk {
            done,
            model_used: if done {
                "gpt-4o-mini".to_owned()
            } else {
                String::new()
            },
            provider_used: if done {
                "azure-openai".to_owned()
            } else {
                String::new()
            },
            input_tokens: if done { 2000 } else { 0 },
            output_tokens: if done { 5 } else { 0 },
            cache_read_input_tokens: if done { 1024 } else { 0 },
            cache_creation_input_tokens: if done { 512 } else { 0 },
            stop_reason: stop_reason.to_owned(),
            ..pb::InferChunk::default()
        }
    }

    fn tapped(items: Vec<Result<pb::InferChunk, Status>>) -> Tapped<pb::InferChunk> {
        let (call, _request) = Call::begin("infer_stream", Request::new(()));
        Tapped::new(call, Box::pin(tokio_stream::iter(items)))
    }

    #[test]
    fn a_stream_is_counted_once_at_its_final_chunk_with_its_tokens_and_no_cache_double_count() {
        let text = rendered(async {
            let mut stream = tapped(vec![Ok(chunk(false, "")), Ok(chunk(true, "stop"))]);
            while stream.next().await.is_some() {}
            drop(stream);
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &[
                    "rpc=\"infer_stream\"",
                    "outcome=\"ok\"",
                    "model=\"gpt-4o-mini\""
                ]
            )
            .as_deref(),
            Some("1"),
            "one call, counted once: {text}"
        );
        assert_eq!(
            series(&text, "inference_tokens_total", &["direction=\"input\""]).as_deref(),
            Some("2000"),
            "the cache legs are inside input_tokens already: {text}"
        );
        assert_eq!(
            series(&text, "inference_tokens_total", &["direction=\"output\""]).as_deref(),
            Some("5"),
            "{text}"
        );
    }

    #[test]
    fn a_stream_the_provider_flagged_incomplete_is_not_counted_ok() {
        let text = rendered(async {
            let mut stream = tapped(vec![
                Ok(chunk(false, "")),
                Ok(chunk(true, "stream_incomplete")),
            ]);
            while stream.next().await.is_some() {}
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["outcome=\"stream_incomplete\""]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
        assert!(
            series(&text, "inference_requests_total", &["outcome=\"ok\""]).is_none(),
            "{text}"
        );
    }

    #[test]
    fn a_stream_that_ends_without_a_final_chunk_is_stream_incomplete() {
        let text = rendered(async {
            let mut stream = tapped(vec![Ok(chunk(false, ""))]);
            while stream.next().await.is_some() {}
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["outcome=\"stream_incomplete\"", "model=\"unknown\""]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
    }

    #[test]
    fn a_stream_error_is_counted_under_its_own_outcome() {
        let text = rendered(async {
            let mut stream = tapped(vec![
                Ok(chunk(false, "")),
                Err(Status::unavailable("broke")),
            ]);
            while stream.next().await.is_some() {}
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["outcome=\"unavailable\""]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
    }

    #[test]
    fn a_stream_dropped_before_its_end_is_counted_as_cancelled_and_only_once() {
        let text = rendered(async {
            let mut stream = tapped(vec![Ok(chunk(false, "")), Ok(chunk(true, "stop"))]);
            assert!(stream.next().await.is_some());
            drop(stream);
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["outcome=\"cancelled\""]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
        assert!(
            series(&text, "inference_tokens_total", &[]).is_none(),
            "{text}"
        );
    }

    #[test]
    fn a_request_the_service_never_verified_is_counted_as_unauthenticated() {
        let text = rendered(async {
            let (call, _request) = Call::begin("infer", Request::new(()));
            let refused: Result<Response<pb::InferResponse>, Status> =
                Err(Status::unauthenticated("invalid credential"));
            let _ = call.finish(refused);
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["service=\"unauthenticated\"", "outcome=\"unauthenticated\""]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
    }

    #[test]
    fn a_catalogue_read_serves_no_model() {
        let text = rendered(async {
            let (call, _request) = Call::begin("list_models", Request::new(()));
            let _ = call.finish(Ok(Response::new(pb::ListModelsResponse::default())));
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &[
                    "rpc=\"list_models\"",
                    "model=\"none\"",
                    "provider=\"none\"",
                    "outcome=\"ok\""
                ]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
    }

    #[test]
    fn the_caller_slot_is_filled_only_by_a_verified_principal() {
        let (call, request) = Call::begin("infer", Request::new(()));
        assert_eq!(call.caller.label(), "unauthenticated");
        let principal = AuthenticatedPrincipal::for_test("org-1", None, false);
        note_caller(request.extensions(), &principal);
        assert_eq!(call.caller.label(), "service:test");
        // A second verification in the same request does not rename the call.
        let user = AuthenticatedPrincipal::for_test("org-1", Some("user-1"), false);
        note_caller(request.extensions(), &user);
        assert_eq!(call.caller.label(), "service:test");
    }

    #[test]
    fn a_call_dropped_before_it_answers_is_counted_as_cancelled_once() {
        let text = rendered(async {
            let (call, request) = Call::begin("infer", Request::new(()));
            let principal = AuthenticatedPrincipal::for_test("org-1", None, false);
            note_caller(request.extensions(), &principal);
            drop(call);
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &[
                    "rpc=\"infer\"",
                    "service=\"service:test\"",
                    "outcome=\"cancelled\""
                ]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
    }

    #[test]
    fn a_call_that_was_counted_is_not_counted_again_when_it_is_dropped() {
        let text = rendered(async {
            let (call, _request) = Call::begin("list_models", Request::new(()));
            let _ = call.finish(Ok(Response::new(pb::ListModelsResponse::default())));
            let (call, _request) = Call::begin("infer", Request::new(()));
            let _ = call.finish::<pb::InferResponse>(Err(Status::unavailable("down")));
        });
        assert!(
            series(
                &text,
                "inference_requests_total",
                &["outcome=\"cancelled\""]
            )
            .is_none(),
            "an answered or failed call is not also cancelled: {text}"
        );
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["rpc=\"list_models\"", "outcome=\"ok\""]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["rpc=\"infer\"", "outcome=\"unavailable\""]
            )
            .as_deref(),
            Some("1"),
            "{text}"
        );
    }

    #[test]
    fn a_response_from_the_cache_is_counted_without_the_tokens_it_carries() {
        let cached = |from_cache: bool| pb::InferResponse {
            model_used: "gpt-4o-mini".to_owned(),
            provider_used: "azure-openai".to_owned(),
            input_tokens: 2000,
            output_tokens: 5,
            provider_timing: Some(pb::ProviderTiming {
                response_cache_hit: from_cache,
                ..pb::ProviderTiming::default()
            }),
            ..pb::InferResponse::default()
        };
        let text = rendered(async {
            for from_cache in [false, true] {
                let (call, _request) = Call::begin("infer", Request::new(()));
                let _ = call.finish(Ok(Response::new(cached(from_cache))));
            }
        });
        assert_eq!(
            series(
                &text,
                "inference_requests_total",
                &["rpc=\"infer\"", "outcome=\"ok\""]
            )
            .as_deref(),
            Some("2"),
            "both answers are calls: {text}"
        );
        assert_eq!(
            series(&text, "inference_tokens_total", &["direction=\"input\""]).as_deref(),
            Some("2000"),
            "the cache hit adds no tokens: {text}"
        );
    }
}
