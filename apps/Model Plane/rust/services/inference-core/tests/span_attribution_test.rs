//! Provider-attempt spans must follow their own call across awaits.
//!
//! Regression test for the fallback chain holding `span.enter()` across a
//! provider `.await`. The entered guard stays on the thread while the call is
//! parked, so any other request polled in between logged under the parked
//! request's id, and the parked request's own later logs carried whichever
//! span was entered last. Production saw it as "infer succeeded" lines without
//! their request id and unrelated connection logs attributed to one request.
//!
//! Kept in its own test binary: the recorder below is installed as the
//! thread's default subscriber, and a test running in parallel on another
//! thread could otherwise cache a callsite's interest against the no-op
//! dispatcher first.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use inference_core::provider::fallback::{FallbackChain, ProviderRouterDyn};
use inference_core::provider::{
    ChatMessage, EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelInfo,
    ProviderError,
};
use tokio::sync::mpsc;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

/// One logged event: its message, the `marker` field the provider stamps with
/// the request it is actually serving, and the `request_id` of the innermost
/// entered span that has one.
#[derive(Debug, Clone)]
struct Logged {
    message: String,
    marker: Option<String>,
    span_request_id: Option<String>,
}

/// A minimal single-threaded subscriber: enough span bookkeeping to answer
/// "which request's span was current when this event fired".
#[derive(Default)]
struct Recorder {
    next_id: AtomicU64,
    span_request_ids: Mutex<HashMap<u64, Option<String>>>,
    entered: Mutex<Vec<u64>>,
    events: Mutex<Vec<Logged>>,
}

#[derive(Default)]
struct Fields {
    message: Option<String>,
    marker: Option<String>,
    request_id: Option<String>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.record_debug(field, &value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}").trim_matches('"').to_owned();
        match field.name() {
            "message" => self.message = Some(rendered),
            "marker" => self.marker = Some(rendered),
            "request_id" => self.request_id = Some(rendered),
            _ => {}
        }
    }
}

struct Shared(Arc<Recorder>);

impl Subscriber for Shared {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let id = self.0.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        self.0
            .span_request_ids
            .lock()
            .unwrap()
            .insert(id, fields.request_id);
        Id::from_u64(id)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let span_ids = self.0.span_request_ids.lock().unwrap();
        let span_request_id = self
            .0
            .entered
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|id| span_ids.get(id).cloned().flatten());
        self.0.events.lock().unwrap().push(Logged {
            message: fields.message.unwrap_or_default(),
            marker: fields.marker,
            span_request_id,
        });
    }

    fn enter(&self, span: &Id) {
        self.0.entered.lock().unwrap().push(span.into_u64());
    }

    fn exit(&self, span: &Id) {
        let mut entered = self.0.entered.lock().unwrap();
        if let Some(at) = entered.iter().rposition(|id| *id == span.into_u64()) {
            entered.remove(at);
        }
    }
}

/// A provider that parks twice and logs in between, the way a real adapter
/// awaits its HTTP call and logs as it goes.
struct YieldingProvider;

#[async_trait::async_trait]
impl ProviderRouterDyn for YieldingProvider {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        tokio::task::yield_now().await;
        tracing::info!(marker = %req.request_id, "provider working");
        tokio::task::yield_now().await;
        Ok(InferResponse {
            request_id: req.request_id.clone(),
            content: "ok".to_owned(),
            model_used: req.model.clone(),
            stop_reason: "end_turn".to_owned(),
            input_tokens: 1,
            output_tokens: 1,
            ..Default::default()
        })
    }

    async fn infer_stream_dyn(
        &self,
        _req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        Err(ProviderError::UnsupportedModel("unary only".to_owned()))
    }

    async fn create_embedding_dyn(
        &self,
        _req: &EmbedRequest,
    ) -> Result<EmbedResponse, ProviderError> {
        Err(ProviderError::UnsupportedModel("chat only".to_owned()))
    }

    fn list_models_dyn(&self) -> Vec<ModelInfo> {
        Vec::new()
    }
}

fn request(request_id: &str) -> InferRequest {
    InferRequest {
        request_id: request_id.to_owned(),
        model: "test-model".to_owned(),
        messages: vec![ChatMessage {
            compaction_summary: String::new(),
            role: "user".to_owned(),
            // Distinct prompts, so neither call can be served from the other's
            // cached response.
            content: format!("hello from {request_id}"),
            name: String::new(),
        }],
        max_tokens: 16,
        ..Default::default()
    }
}

#[tokio::test]
async fn interleaved_calls_log_under_their_own_request_span() {
    let recorder = Arc::new(Recorder::default());
    let _default = tracing::subscriber::set_default(Shared(Arc::clone(&recorder)));
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> =
        vec![("primary".to_owned(), Arc::new(YieldingProvider))];
    let chain = FallbackChain::new_with_providers(providers, 1);

    let (req_a, req_b) = (request("req-a"), request("req-b"));
    // Polled in one task, so the two calls interleave at every yield.
    let (a, b) = tokio::join!(chain.infer(&req_a), chain.infer(&req_b));
    a.expect("req-a served");
    b.expect("req-b served");

    let events = recorder.events.lock().unwrap().clone();
    let working: Vec<&Logged> = events
        .iter()
        .filter(|event| event.message == "provider working")
        .collect();
    assert_eq!(working.len(), 2, "{events:#?}");
    for event in &working {
        assert_eq!(
            event.span_request_id, event.marker,
            "a provider log must sit under the span of the request it serves: {events:#?}"
        );
    }

    let mut succeeded: Vec<Option<&str>> = events
        .iter()
        .filter(|event| event.message == "infer succeeded")
        .map(|event| event.span_request_id.as_deref())
        .collect();
    succeeded.sort_unstable();
    assert_eq!(
        succeeded,
        vec![Some("req-a"), Some("req-b")],
        "each outcome line carries its own request id: {events:#?}"
    );
}
