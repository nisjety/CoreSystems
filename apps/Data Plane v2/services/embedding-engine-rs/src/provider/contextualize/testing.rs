//! A model for the tests of the batch pipeline.
//!
//! It lives here, beside the engine, because the engine's seam (`InferencePort`) and
//! the wire types it speaks are private to this module and its tests. It is a port
//! and nothing more: it records what it was sent and answers as it is told, per
//! tenant, so a test can see which organization's chunk reached the provider, when,
//! and how many at once. Every rule that decides what an answer or a failure MEANS is
//! the real one: the answers it gives pass through the real validator, and the
//! failures it gives are the statuses inference-core's own contract pins, decoded by
//! the real decoder.

#![allow(clippy::result_large_err)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use futures::future::BoxFuture;
use sha2::{Digest, Sha256};

use super::*;
use crate::context::testing::golden_status;

/// One request that reached the provider.
#[derive(Clone, Debug)]
pub(crate) struct ModelCall {
    pub org_id: String,
    /// The chunk the prompt asks about (found between its `<chunk>` tags).
    pub chunk: String,
    pub zdr: bool,
}

/// What the provider does with a request.
#[derive(Clone, Debug)]
pub(crate) enum Behaviour {
    /// Answers, at once, with a context derived from the chunk.
    Answer,
    /// Answers after this long.
    AnswerAfter(Duration),
    /// Never answers.
    Hang,
    /// Fails with the status that inference-core's contract pins under this id.
    Fail(&'static str),
}

type Hook = Arc<dyn Fn(ModelCall) -> BoxFuture<'static, ()> + Send + Sync>;

pub(crate) struct FakeModel {
    calls: Mutex<Vec<ModelCall>>,
    by_org: Mutex<HashMap<String, Behaviour>>,
    default: Mutex<Behaviour>,
    in_flight: AtomicUsize,
    peak: AtomicUsize,
    before_answer: Mutex<Option<Hook>>,
}

/// Counts a request as in flight until the future that serves it is dropped, whether
/// it finished or was cancelled by a deadline.
struct InFlight(Arc<FakeModel>);

impl InFlight {
    fn enter(model: &Arc<FakeModel>) -> Self {
        let now = model.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        model.peak.fetch_max(now, Ordering::SeqCst);
        Self(model.clone())
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl FakeModel {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            by_org: Mutex::new(HashMap::new()),
            default: Mutex::new(Behaviour::Answer),
            in_flight: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            before_answer: Mutex::new(None),
        })
    }

    pub(crate) fn set_default(&self, behaviour: Behaviour) {
        *self.default.lock().unwrap_or_else(PoisonError::into_inner) = behaviour;
    }

    pub(crate) fn set_for_org(&self, org_id: &str, behaviour: Behaviour) {
        self.by_org
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(org_id.to_owned(), behaviour);
    }

    /// Run `hook` when a request arrives, before it is answered: how a test makes
    /// something else happen in the middle of a generation (another worker finishing
    /// the same unit first).
    pub(crate) fn on_call(&self, hook: Hook) {
        *self
            .before_answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(hook);
    }

    pub(crate) fn calls(&self) -> Vec<ModelCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn call_count(&self) -> usize {
        self.calls().len()
    }

    pub(crate) fn calls_for(&self, org_id: &str) -> Vec<ModelCall> {
        self.calls()
            .into_iter()
            .filter(|call| call.org_id == org_id)
            .collect()
    }

    /// The most requests that were ever in flight at once.
    pub(crate) fn peak_in_flight(&self) -> usize {
        self.peak.load(Ordering::SeqCst)
    }

    fn behaviour_for(&self, org_id: &str) -> Behaviour {
        self.by_org
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(org_id)
            .cloned()
            .unwrap_or_else(|| {
                self.default
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .clone()
            })
    }
}

/// The context this model answers for a chunk. A function of the chunk alone, so a
/// test knows what a unit's context must be without asking the model, and two
/// different chunks never share one.
pub(crate) fn context_for(chunk: &str) -> String {
    let digest = Sha256::digest(chunk.as_bytes());
    let lead: String = chunk.chars().take(28).collect();
    format!(
        "Situerer avsnittet «{lead}» ({:02x}{:02x}{:02x}) i dokumentet.",
        digest[0], digest[1], digest[2]
    )
}

fn chunk_of(request: &model_plane::v1::InferRequest) -> String {
    let prompt = request
        .messages
        .first()
        .map(|message| message.content.as_str())
        .unwrap_or_default();
    prompt
        .split("<chunk>\n")
        .nth(1)
        .and_then(|tail| tail.split("\n</chunk>").next())
        .unwrap_or_default()
        .to_owned()
}

fn answer(content: &str) -> model_plane::v1::InferResponse {
    model_plane::v1::InferResponse {
        request_id: String::new(),
        content: content.to_string(),
        model_used: "gpt-4o-mini-2024-07-18".to_string(),
        stop_reason: "stop".to_string(),
        input_tokens: 612,
        output_tokens: 48,
        provider_used: "azure-openai".to_string(),
        residency: "eu".to_string(),
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        provider_timing: Some(model_plane::v1::ProviderTiming {
            usage_reported: true,
            response_cache_hit: false,
            attempts: 1,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The port the engine is built over. It has to be an `Arc<FakeModel>` because the
/// requests it serves count themselves while they are in flight.
pub(crate) struct ModelPort(pub(crate) Arc<FakeModel>);

impl InferencePort for ModelPort {
    fn infer(&self, request: tonic::Request<model_plane::v1::InferRequest>) -> InferFuture<'_> {
        let model = self.0.clone();
        let chunk = chunk_of(request.get_ref());
        let call = ModelCall {
            org_id: request.get_ref().org_id.clone(),
            chunk: chunk.clone(),
            zdr: request.get_ref().zdr,
        };
        let request_id = request.get_ref().request_id.clone();
        model
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call.clone());
        let behaviour = model.behaviour_for(&call.org_id);
        let hook = model
            .before_answer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Box::pin(async move {
            let _in_flight = InFlight::enter(&model);
            if let Some(hook) = hook {
                hook(call).await;
            }
            let mut response = match behaviour {
                Behaviour::Answer => answer(&context_for(&chunk)),
                Behaviour::AnswerAfter(wait) => {
                    tokio::time::sleep(wait).await;
                    answer(&context_for(&chunk))
                }
                Behaviour::Hang => std::future::pending::<model_plane::v1::InferResponse>().await,
                Behaviour::Fail(id) => return Err(golden_status(id)),
            };
            response.request_id = request_id;
            Ok(response)
        })
    }
}
