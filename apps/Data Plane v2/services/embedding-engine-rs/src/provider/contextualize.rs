//! Contextual Retrieval — per-chunk situating context, generated before embedding.
//!
//! ## What this is
//!
//! A chunk taken out of a document loses the context that made it findable.
//! "The margin improved to 31%" is unretrievable for "ACME Q2 margin" because
//! neither "ACME" nor "Q2" survives the split. The technique (Anthropic, 2024)
//! is to ask an LLM for one or two sentences situating each chunk inside its
//! document, prepend that to the chunk, and embed *that* — reported to cut
//! retrieval failures by ~35% for contextual embeddings alone.
//!
//! ## Why it lives here and not in the chunker
//!
//! `index-engine-rs`'s chunker module header has long claimed Contextual
//! Retrieval was "wired behind flags". It was not — there were no such flags,
//! and the chunker is the wrong home regardless:
//!
//! * `index_engine_rs::builder::process_document` does its whole build inside
//!   ONE transaction holding `FOR UPDATE` row locks on `documents` and
//!   `knowledge_units`. N per-chunk LLM calls in there would hold those locks
//!   for minutes and serialize every concurrent write to the same document.
//! * The chunker is pure and synchronous, with no HTTP/gRPC client and no
//!   inference credential — reaching a model from it means giving a pure
//!   function a network dependency.
//! * This service already has what the work needs: an org-bound
//!   `aud=inference-core` bearer minter ([`InferenceTokenClient`]), a
//!   model-plane channel, and an async per-chunk pipeline that is already off
//!   the transaction critical path.
//!
//! Most decisively: this is the layer that decides *what text gets embedded*.
//!
//! ## What is embedded vs. what is stored
//!
//! The generated context is prepended only to the text handed to the embedding
//! provider. The chunk text returned to callers and written to the Qdrant
//! payload stays the ORIGINAL — a user's citation must show the document's own
//! words, not a model's preamble about them.
//!
//! The context is persisted ALONE in `knowledge_units.chunk_context`, never the
//! composed string: the composed form is a pure function of the two
//! ([`compose_contextualized`]), and keeping them apart is what lets the lexical
//! arm index the chunk's own words in one field and the context in another. That
//! matters because BM25 sums across fields — indexing a composed string next to
//! the chunk would score the chunk's own terms twice. `content_tsv` is generated
//! over `text` at weight A and `chunk_context` at weight B, so BOTH halves of the
//! technique (contextual embeddings and contextual BM25) come out of this
//! one generation.
//!
//! ## What this module is, and what it is not
//!
//! This is the client: it takes a bearer, builds the request, calls
//! inference-core, retries what is worth retrying, and turns what came back into a
//! [`ContextOutcome`]. What the prompt says, which part of the document is read,
//! what is accepted from the answer and what each result means are decided in
//! [`crate::context`], without a network, so they are tested with the real inputs.
//!
//! Every chunk of a call ends in an explicit outcome. There is no path on which a
//! failure becomes "no context" without saying which failure: an empty answer is
//! `invalid_output`, a refused bearer is `pending` or `failed_transient` with its
//! class, a zero-retention organization is `skipped_zdr` with no provider call
//! made. What the caller does with an outcome (the embed falls back to the raw
//! chunk, the unit's status is written) is the batch pipeline's.
//!
//! ZDR is absolute, not best-effort, and it is enforced in three places so that no
//! one of them is the only line. A restricted document is refused here before its
//! text is read ([`crate::context::DocumentInput::restricted`]). An organization
//! whose bearer says `zdr` makes no call at all. And the request carries the
//! bearer's own flag, so a route that is not attested for zero retention refuses
//! it at inference-core before any provider is reached.
//!
//! ON by default, and kept on (owner ruling of 2026-10-03): `CONTEXTUAL_RETRIEVAL_ENABLED=false`
//! is the incident kill switch and needs a `CONTEXT_RETRIEVAL_KILL_SWITCH_REASON` (see
//! [`Config::contextual_retrieval_state`]). The cost is bounded, metered and reported, not avoided.

use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::Context as _;
use futures::stream::{self, StreamExt};
use inference_client_rs::{InferenceError, InferenceTokenClient, Reason};
use tokio::time::Instant;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use super::model_plane;
use crate::config::Config;
use crate::context::outcome::{inline_retry_wait, outcome_for_failure};
use crate::context::prompt::{build_prompt, input_hash};
use crate::context::validate::validate;
use crate::context::{
    ChunkInput, ChunkResult, ContextOutcome, DocumentInput, Generated, InlineBudget, MeteredCall,
    PendingReason, PreparedDocument, PromptVersion, Usage, Window, WindowConfig, ZdrReason,
};
// The tests of this module (and its children) build documents with it.
#[cfg(test)]
use crate::context::DocumentMeta;

/// The most documents of one batch that are contextualized at once. A bound on what
/// one batch can ask of the provider (it is multiplied by the per-document
/// concurrency), checked at startup.
pub const MAX_DOCUMENT_CONCURRENCY: usize = 32;

/// The provider asked for when nothing else is configured. A hint pins the route:
/// without one, inference-core may serve the call from any provider in its chain.
pub const DEFAULT_PROVIDER_HINT: &str = "azure-openai";
/// The residency floor asked for when nothing else is configured.
pub const DEFAULT_MIN_RESIDENCY: &str = "eu";
/// The tokens inference-core's `Residency::parse` accepts. Anything else is
/// rejected there with INVALID_ARGUMENT before any provider is tried, which would
/// refuse every call; it is refused at startup instead.
const RESIDENCY_TOKENS: [&str; 6] = ["global", "eu", "eea", "norway", "no", "sovereign"];

/// A window shorter than this has no prefix a provider could cache (Azure's
/// automatic prompt cache needs 1,024 tokens, about 4,000 characters), so asking
/// the first chunk alone to warm it would only add a round trip of latency.
const PREFIX_CACHE_MIN_BYTES: usize = 4_096;

const OUTPUT_TOKENS_MIN: i32 = 16;
const OUTPUT_TOKENS_MAX: i32 = 1_024;

/// Everything the engine reads from configuration, validated.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub model: String,
    pub provider_hint: String,
    pub min_residency: String,
    pub prompt_version: PromptVersion,
    pub window: WindowConfig,
    pub max_tokens: i32,
    pub timeout: Duration,
    pub concurrency: usize,
    pub retry_attempts: u32,
    pub retry_base: Duration,
    pub inline_budget: InlineBudget,
    /// How many documents of one batch are contextualized at the same time.
    pub document_concurrency: usize,
}

impl Settings {
    /// Read and check the configuration. Every refusal names the variable, so a
    /// startup failure says what to fix.
    pub(crate) fn from_config(cfg: &Config) -> anyhow::Result<Self> {
        let model = cfg.contextual_retrieval_model.trim().to_string();
        if model.is_empty() {
            anyhow::bail!(
                "CONTEXTUAL_RETRIEVAL_MODEL is required when CONTEXTUAL_RETRIEVAL_ENABLED=true"
            );
        }
        // An empty value takes the default rather than meaning "no floor": compose
        // passes every variable, empty when unset, and an empty residency floor
        // would silently remove the guarantee. Weaker is spelled out (`global`).
        let or_default = |value: &str, default: &str| {
            let value = value.trim();
            let chosen = if value.is_empty() { default } else { value };
            chosen.to_string()
        };
        let provider_hint = or_default(
            &cfg.contextual_retrieval_provider_hint,
            DEFAULT_PROVIDER_HINT,
        );
        let min_residency = or_default(
            &cfg.contextual_retrieval_min_residency,
            DEFAULT_MIN_RESIDENCY,
        )
        .to_ascii_lowercase();
        if !RESIDENCY_TOKENS.contains(&min_residency.as_str()) {
            anyhow::bail!(
                "CONTEXTUAL_RETRIEVAL_MIN_RESIDENCY `{min_residency}` is not a residency inference-core accepts (one of {}); every call would be refused",
                RESIDENCY_TOKENS.join(", ")
            );
        }
        let prompt_version = PromptVersion::parse(&or_default(
            &cfg.contextual_retrieval_prompt_version,
            crate::context::prompt::PROMPT_V1,
        ))
        .map_err(|error| anyhow::anyhow!("CONTEXTUAL_RETRIEVAL_PROMPT_VERSION: {error}"))?;
        let window =
            WindowConfig::new(cfg.contextual_retrieval_window_chars).map_err(anyhow::Error::msg)?;
        if window.window_chars() > cfg.contextual_retrieval_max_document_chars {
            anyhow::bail!(
                "CONTEXTUAL_RETRIEVAL_WINDOW_CHARS ({}) must not exceed CONTEXTUAL_RETRIEVAL_MAX_DOCUMENT_CHARS ({}), the most document text a prompt may carry",
                window.window_chars(),
                cfg.contextual_retrieval_max_document_chars
            );
        }
        let max_tokens = cfg.contextual_retrieval_max_tokens;
        if !(OUTPUT_TOKENS_MIN..=OUTPUT_TOKENS_MAX).contains(&max_tokens) {
            anyhow::bail!(
                "CONTEXTUAL_RETRIEVAL_MAX_TOKENS must be between {OUTPUT_TOKENS_MIN} and {OUTPUT_TOKENS_MAX}, got {max_tokens}"
            );
        }
        let inline_budget = InlineBudget::new(
            cfg.context_inline_document_budget_ms,
            cfg.context_inline_budget_ms,
        )
        .map_err(anyhow::Error::msg)?;
        let document_concurrency = cfg.context_inline_document_concurrency;
        if !(1..=MAX_DOCUMENT_CONCURRENCY).contains(&document_concurrency) {
            anyhow::bail!(
                "CONTEXT_INLINE_DOCUMENT_CONCURRENCY must be between 1 and {MAX_DOCUMENT_CONCURRENCY}, got {document_concurrency}: it is how many documents of one batch are contextualized at once, so the most provider calls one batch has in flight is this times CONTEXTUAL_RETRIEVAL_CONCURRENCY"
            );
        }
        Ok(Self {
            model,
            provider_hint,
            min_residency,
            prompt_version,
            window,
            max_tokens,
            timeout: Duration::from_millis(cfg.contextual_retrieval_timeout_ms.max(1)),
            concurrency: cfg.contextual_retrieval_concurrency.max(1),
            retry_attempts: cfg.contextual_retrieval_retry_attempts,
            retry_base: Duration::from_millis(cfg.contextual_retrieval_retry_base_ms.max(1)),
            inline_budget,
            document_concurrency,
        })
    }
}

type InferFuture<'a> = Pin<
    Box<dyn Future<Output = Result<model_plane::v1::InferResponse, tonic::Status>> + Send + 'a>,
>;

/// The one call this client makes. A seam so the engine can be driven by a port
/// that records what it was sent and answers with real statuses, without a server.
pub(crate) trait InferencePort: Send + Sync + 'static {
    fn infer(&self, request: tonic::Request<model_plane::v1::InferRequest>) -> InferFuture<'_>;
}

struct GrpcInference {
    client: model_plane::v1::inference_core_client::InferenceCoreClient<Channel>,
}

impl InferencePort for GrpcInference {
    fn infer(&self, request: tonic::Request<model_plane::v1::InferRequest>) -> InferFuture<'_> {
        let mut client = self.client.clone();
        Box::pin(async move { client.infer(request).await.map(tonic::Response::into_inner) })
    }
}

/// The lazily-connected channel to inference-core. Opens no connection until the
/// first call, so building one needs a Tokio runtime and nothing else.
fn grpc_inference(cfg: &Config, settings: &Settings) -> anyhow::Result<GrpcInference> {
    let channel = Endpoint::from_shared(cfg.model_plane_ai_core_grpc_url.clone())
        .with_context(|| {
            format!(
                "invalid MODEL_PLANE_AI_CORE_GRPC_URL `{}`",
                cfg.model_plane_ai_core_grpc_url
            )
        })?
        .connect_timeout(settings.timeout)
        .timeout(settings.timeout)
        .connect_lazy();
    Ok(GrpcInference {
        client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
    })
}

#[derive(Clone)]
pub struct Contextualizer {
    port: Arc<dyn InferencePort>,
    token_client: InferenceTokenClient,
    settings: Arc<Settings>,
}

/// What every chunk of one call shares.
struct Run<'a> {
    prepared: &'a PreparedDocument<'a>,
    chunks: &'a [ChunkInput<'a>],
    windows: &'a [Window<'a>],
    deadline: Option<Instant>,
    /// An outcome that is the answer for every remaining chunk (see
    /// [`ContextOutcome::is_systemic`]), once any chunk has found it out.
    systemic: Mutex<Option<ContextOutcome>>,
}

impl Run<'_> {
    fn systemic(&self) -> Option<ContextOutcome> {
        self.systemic
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn note_systemic(&self, outcome: &ContextOutcome) {
        let mut slot = self.systemic.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(outcome.clone());
        }
    }

    /// `None` when there is no deadline; `Some(ZERO)` when it has passed.
    fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
    }

    fn expired(&self) -> bool {
        self.remaining().is_some_and(|left| left.is_zero())
    }
}

impl Contextualizer {
    /// `Ok(None)` only while the incident kill switch is thrown, and a thrown switch has a
    /// reason on record (`Config::contextual_retrieval_state`): callers then embed the raw text.
    pub fn from_config(cfg: &Config) -> anyhow::Result<Option<Self>> {
        if !cfg.contextual_retrieval_state()?.is_on() {
            return Ok(None);
        }
        let settings = Settings::from_config(cfg)?;
        // Reuses this service's existing inference credential and retention
        // posture — the same principal already used for model-plane embedding,
        // so enabling this grants no authority the service did not already hold.
        //
        // Strict in a real deployment: an operator who asked for contextual
        // retrieval and supplied no credential should be told at startup, not
        // discover months later that nothing was ever contextualized. Standalone
        // startup relaxes it exactly as the embedding provider does — the client
        // stays unusable, so mints fail, so every chunk gets a refused-bearer
        // outcome rather than an unauthenticated inference call.
        let token_client = super::token_client_from_config(cfg)?;
        let port = grpc_inference(cfg, &settings)?;
        tracing::info!(
            model = %settings.model,
            prompt_version = settings.prompt_version.as_str(),
            provider_hint = %settings.provider_hint,
            min_residency = %settings.min_residency,
            window_chars = settings.window.window_chars(),
            inline_document_budget_ms = settings.inline_budget.document.as_millis() as u64,
            inline_batch_budget_ms = settings.inline_budget.batch.as_millis() as u64,
            concurrency = settings.concurrency,
            "contextual retrieval configured"
        );
        Ok(Some(Self::new(Arc::new(port), token_client, settings)))
    }

    pub(crate) fn new(
        port: Arc<dyn InferencePort>,
        token_client: InferenceTokenClient,
        settings: Settings,
    ) -> Self {
        Self {
            port,
            token_client,
            settings: Arc::new(settings),
        }
    }

    pub fn model_name(&self) -> &str {
        &self.settings.model
    }

    /// The prompt version used when an organization has not pinned another.
    #[allow(dead_code)] // consumed by the write-ahead pipeline (CR-T8)
    pub fn prompt_version(&self) -> PromptVersion {
        self.settings.prompt_version
    }

    /// How the inline phase is time-boxed.
    pub fn inline_budget(&self) -> InlineBudget {
        self.settings.inline_budget
    }

    /// How many documents of one batch are contextualized at the same time.
    pub fn document_concurrency(&self) -> usize {
        self.settings.document_concurrency
    }

    /// Whether Auth Core says the organization is zero-retention NOW: one bearer mint for the
    /// feature, and its `zdr` claim read. This is how the reconciler (CR-T9a) rechecks the units
    /// that were skipped for zero retention, at most once an hour per organization: it asks the
    /// same authority the generation asks, and calls no model. `Err` is what a refused or
    /// unresolved mint means (a wait, never a skip and never a send), by the same table as a
    /// failed call.
    pub async fn org_is_zero_retention(&self, org_id: &str) -> Result<bool, ContextOutcome> {
        match self
            .token_client
            .bearer(org_id, Reason::ContextualRetrieval)
            .await
        {
            Ok(bearer) => Ok(bearer.zdr()),
            Err(error) => Err(outcome_for_failure(&InferenceError::from(error))),
        }
    }

    /// Window a document. Cheap (one pass over the text) and needed first: the
    /// input hash of each chunk, with which the caller decides reuse, depends on it.
    pub fn prepare<'a>(&self, input: DocumentInput<'a>) -> PreparedDocument<'a> {
        PreparedDocument::new(input, self.settings.prompt_version, self.settings.window)
    }

    /// What a chunk's context would be generated from. Equal to what a stored
    /// context recorded when it was generated from the same inputs, which is how an
    /// unchanged chunk is recognised and not paid for twice.
    pub fn input_hash(&self, prepared: &PreparedDocument<'_>, chunk: &ChunkInput<'_>) -> String {
        self.hash_for(prepared, &prepared.window_for(chunk).text, chunk.text)
    }

    fn hash_for(
        &self,
        prepared: &PreparedDocument<'_>,
        window_text: &str,
        chunk_text: &str,
    ) -> String {
        input_hash(
            prepared.version,
            &self.settings.model,
            window_text,
            prepared.input.meta.title,
            chunk_text,
        )
    }

    /// Generate the context of each of `chunks`, one result per chunk, in order.
    ///
    /// Infallible: every chunk ends in an outcome (see [`ContextOutcome`]), and a
    /// failure that applies to the whole document is every chunk's outcome.
    ///
    /// * **ZDR.** A restricted document is refused before anything of it is read.
    ///   An organization whose bearer says `zdr` makes no call.
    /// * **Deadline.** `deadline` bounds the whole call. A chunk that has not
    ///   finished by then is [`PendingReason::DeadlineExceeded`] and no call is
    ///   started after it; a retry never sleeps past it. `None` is unbounded (the
    ///   per-call timeout still applies). An already-passed deadline makes no call
    ///   and mints nothing.
    /// * **Bearer.** One is asked for up front to fail fast: if Auth Core issues
    ///   none, every chunk would find that out one slow round trip at a time. After
    ///   that a bearer is taken from the client's cache at the moment of EACH
    ///   provider call and never held across the document, so a document that
    ///   outlives a 60-second tenant token does not carry an expired one.
    /// * **Order.** Chunks that share a window (the same prefix of the prompt) are
    ///   not all started at once: the first goes alone and the rest follow
    ///   concurrently, so the provider's prefix cache is warm for them. A window too
    ///   short to be cached (most documents) is not staged.
    pub async fn contextualize(
        &self,
        prepared: &PreparedDocument<'_>,
        chunks: &[ChunkInput<'_>],
        deadline: Option<Instant>,
    ) -> Vec<ChunkResult> {
        if chunks.is_empty() {
            return Vec::new();
        }
        let org_id = prepared.input.org_id;
        let uniform = |outcome: ContextOutcome| {
            chunks
                .iter()
                .map(|_| ChunkResult::without_calls(outcome.clone()))
                .collect::<Vec<_>>()
        };
        if prepared.input.restricted {
            return self.report(
                prepared,
                uniform(ContextOutcome::skipped_zdr(ZdrReason::Document)),
            );
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return self.report(prepared, uniform(deadline_outcome()));
        }
        match self
            .token_client
            .bearer(org_id, Reason::ContextualRetrieval)
            .await
        {
            Err(error) => {
                let error = InferenceError::from(error);
                return self.report(prepared, uniform(outcome_for_failure(&error)));
            }
            Ok(bearer) if bearer.zdr() => {
                return self.report(
                    prepared,
                    uniform(ContextOutcome::skipped_zdr(ZdrReason::Org)),
                );
            }
            Ok(_) => {}
        }

        let windows: Vec<Window<'_>> = chunks
            .iter()
            .map(|chunk| prepared.window_for(chunk))
            .collect();
        let (leaders, followers) = stages(&windows);
        let run = Run {
            prepared,
            chunks,
            windows: &windows,
            deadline,
            systemic: Mutex::new(None),
        };
        let run = &run;
        let mut results: Vec<Option<ChunkResult>> = vec![None; chunks.len()];
        for stage in [leaders, followers] {
            let done: Vec<(usize, ChunkResult)> = stream::iter(stage)
                .map(|position| async move { (position, self.generate_one(run, position).await) })
                // Unordered, indexed: an ordered buffer would hold a slot behind a
                // slow head and quietly lower the concurrency below what was set.
                .buffer_unordered(self.settings.concurrency)
                .collect()
                .await;
            for (position, result) in done {
                results[position] = Some(result);
            }
        }
        let results = results
            .into_iter()
            .map(|result| {
                result.unwrap_or_else(|| {
                    ChunkResult::without_calls(ContextOutcome::permanent(
                        crate::context::ErrorClass::Internal,
                    ))
                })
            })
            .collect();
        self.report(prepared, results)
    }

    /// One chunk, to an outcome, with every provider call it cost.
    async fn generate_one(&self, run: &Run<'_>, position: usize) -> ChunkResult {
        let mut calls = Vec::new();
        let outcome = self.generate(run, position, &mut calls).await;
        if outcome.is_systemic() {
            run.note_systemic(&outcome);
        }
        ChunkResult { outcome, calls }
    }

    async fn generate(
        &self,
        run: &Run<'_>,
        position: usize,
        calls: &mut Vec<MeteredCall>,
    ) -> ContextOutcome {
        let chunk = &run.chunks[position];
        let org_id = run.prepared.input.org_id;
        // From the PLANNED window, whatever window the call ends up using: the hash
        // names the inputs, and a shrink is a deterministic fallback for the same
        // inputs. Otherwise a document that always needs the shrink would never
        // match its own stored hash and would be paid for again every pass.
        let planned_hash = self.hash_for(run.prepared, &run.windows[position].text, chunk.text);
        let mut window = Some(&run.windows[position]);
        let mut shrunk: Option<Window<'_>> = None;
        let mut attempt = 0u32;
        loop {
            // Another chunk already found the answer for the whole document.
            if let Some(outcome) = run.systemic() {
                return outcome;
            }
            let Some(remaining) = remaining_or_expired(run) else {
                return deadline_outcome();
            };
            // From the client's cache, at the moment of this call.
            let bearer = match self
                .token_client
                .bearer(org_id, Reason::ContextualRetrieval)
                .await
            {
                Ok(bearer) => bearer,
                Err(error) => return outcome_for_failure(&InferenceError::from(error)),
            };
            if bearer.zdr() {
                // The organization turned zero-retention part way through.
                return ContextOutcome::skipped_zdr(ZdrReason::Org);
            }
            let in_use: &Window<'_> = match (&shrunk, window) {
                (Some(shrunk), _) => shrunk,
                (None, Some(planned)) => planned,
                (None, None) => {
                    return ContextOutcome::permanent(crate::context::ErrorClass::Internal)
                }
            };
            let prompt = build_prompt(
                run.prepared.version,
                &run.prepared.input.meta,
                &in_use.text,
                chunk.text,
            );
            let per_call = remaining.map_or(self.settings.timeout, |left| {
                left.min(self.settings.timeout)
            });
            let request =
                match self.build_request(org_id, bearer.zdr(), prompt, bearer.as_str(), per_call) {
                    Ok(request) => request,
                    Err(outcome) => return outcome,
                };

            let call = self.port.infer(request);
            let answered = match tokio::time::timeout(per_call, call).await {
                Ok(answered) => answered,
                Err(_elapsed) if run.expired() => return deadline_outcome(),
                // The per-call timeout ran out while there was still time overall:
                // a slow provider, the same as one that answered DEADLINE_EXCEEDED.
                Err(_elapsed) => Err(tonic::Status::deadline_exceeded("inference call timed out")),
            };

            let error = match answered {
                Ok(response) => {
                    return self.finish(run, response, &planned_hash, calls);
                }
                Err(status) => InferenceError::from_status(&status),
            };

            // The deadline we set ran out inside the call.
            if run.expired()
                && matches!(
                    error.code(),
                    Some(tonic::Code::DeadlineExceeded | tonic::Code::Cancelled)
                )
            {
                return deadline_outcome();
            }
            // Too long: one smaller window around the chunk, then give up.
            if matches!(error.failure(), inference_client_rs::Failure::TooLong) && shrunk.is_none()
            {
                if let Some(smaller) = run.prepared.windows.shrunk_for(
                    chunk.text,
                    chunk.index,
                    run.prepared.input.total_chunks,
                ) {
                    shrunk = Some(smaller);
                    window = None;
                    continue;
                }
            }
            let outcome = outcome_for_failure(&error);
            match inline_retry_wait(
                &error,
                self.settings.retry_base,
                attempt,
                usize::try_from(chunk.index.max(0)).unwrap_or(0),
            ) {
                Some(wait)
                    if attempt < self.settings.retry_attempts
                        && run.remaining().is_none_or(|left| wait < left) =>
                {
                    tracing::debug!(
                        org_id,
                        chunk_index = chunk.index,
                        attempt = attempt + 1,
                        failure = error.failure().kind(),
                        code = ?error.code(),
                        provider_error = error.wire_kind(),
                        wait_ms = wait.as_millis() as u64,
                        "contextual retrieval: transient inference failure; retrying"
                    );
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                // Out of retries, or the wait would not fit: what the chunk is now.
                _ => return outcome,
            }
        }
    }

    /// The request for one chunk. Residency and provider floors are set here, not
    /// left to the router.
    ///
    /// * The provider hint pins the route to the deployment this feature's data
    ///   processing is documented for. A hint of `azure-openai` matches the
    ///   provider whether it is spelled with an underscore or a hyphen.
    /// * The residency floor makes inference-core refuse, before any provider is
    ///   tried, a route that cannot prove it stays inside it. That proof is its
    ///   own configuration (`AZURE_OPENAI_REGION`), and an unset region reads as
    ///   unproven: the refusal comes back as `residency_unavailable` and is an
    ///   outcome of its own, never a silent fallback.
    /// * Restricted content never reaches this request: a restricted document is
    ///   refused before it is read, and an organization whose bearer says `zdr`
    ///   makes no call. The flag is still taken from the bearer, not assumed
    ///   false, so that if either of those ever changed, a route that is not
    ///   attested for zero retention would still refuse it.
    fn build_request(
        &self,
        org_id: &str,
        zdr: bool,
        prompt: String,
        bearer: &str,
        timeout: Duration,
    ) -> Result<tonic::Request<model_plane::v1::InferRequest>, ContextOutcome> {
        let request = model_plane::v1::InferRequest {
            request_id: Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            model: self.settings.model.clone(),
            provider_hint: self.settings.provider_hint.clone(),
            messages: vec![model_plane::v1::ChatMessage {
                role: "user".to_string(),
                content: prompt,
                name: String::new(),
                // Provider-native compaction checkpoint; a one-shot user
                // prompt never carries one.
                compaction_summary: String::new(),
            }],
            // Zero temperature: the same chunk in the same document should
            // produce the same context, or a reindex silently changes what got
            // embedded for reasons unrelated to the content. Providers are not
            // bit-exact, so what makes it reproducible is that the output is
            // persisted and reused by its input hash, not that it is regenerated.
            temperature: 0.0,
            max_tokens: self.settings.max_tokens,
            structured_output_schema: String::new(),
            zdr,
            tools: Vec::new(),
            tool_choice: String::new(),
            // No privacy-tier constraint: the tier is a statement about a
            // provider's ZDR attestation, which `zdr` above already enforces.
            min_privacy_tier: model_plane::v1::PrivacyTier::Unspecified as i32,
            min_residency: self.settings.min_residency.clone(),
            // No extended thinking: right for a cheap, deterministic,
            // temperature-0 task like this one.
            thinking_budget_tokens: 0,
            // Subscription routing belongs to a user's own chat, never to
            // background contextualization: no subscription connection, normal tier.
            subscription_connection_id: String::new(),
            prefer_priority_service_tier: false,
        };
        let mut request = tonic::Request::new(request);
        request.set_timeout(timeout);
        let value: MetadataValue<_> = format!("Bearer {bearer}").parse().map_err(|_| {
            // A JWT is base64url and dots; reaching this is a defect in the mint,
            // and retrying cannot fix it.
            ContextOutcome::permanent(crate::context::ErrorClass::Internal)
        })?;
        request.metadata_mut().insert("authorization", value);
        Ok(request)
    }

    /// An answered call: validate it, and record what it cost either way.
    fn finish(
        &self,
        run: &Run<'_>,
        response: model_plane::v1::InferResponse,
        planned_hash: &str,
        calls: &mut Vec<MeteredCall>,
    ) -> ContextOutcome {
        let usage = usage_of(&response);
        let request_id = response.request_id.clone();
        calls.push(MeteredCall {
            request_id: request_id.clone(),
            model_used: response.model_used.clone(),
            provider_used: response.provider_used.clone(),
            usage,
        });
        match validate(&response.content, &response.stop_reason) {
            Ok(accepted) => ContextOutcome::Ready(Box::new(Generated {
                text: accepted.text,
                usage,
                model_used: response.model_used,
                provider_used: response.provider_used,
                residency: response.residency,
                request_id,
                prompt_version: run.prepared.version,
                input_hash: planned_hash.to_string(),
                cut_at_sentence: accepted.cut_at_sentence,
                meta_opening: accepted.meta_opening,
            })),
            Err(rejection) => rejection.outcome(),
        }
    }

    /// One line per call, content-free: ids, counts, classes and token totals.
    /// Never a prompt, a context, a window or a provider body.
    fn report(
        &self,
        prepared: &PreparedDocument<'_>,
        results: Vec<ChunkResult>,
    ) -> Vec<ChunkResult> {
        let mut by_status: BTreeMap<&'static str, usize> = BTreeMap::new();
        let mut by_class: BTreeMap<&'static str, usize> = BTreeMap::new();
        let (mut input_tokens, mut output_tokens, mut cache_read, mut provider_calls) =
            (0u64, 0u64, 0u64, 0u64);
        let (mut meta_openings, mut cut) = (0usize, 0usize);
        for result in &results {
            *by_status
                .entry(result.outcome.status().as_str())
                .or_default() += 1;
            if let Some(class) = result.outcome.error_class() {
                *by_class.entry(class.as_str()).or_default() += 1;
            }
            if let Some(generated) = result.outcome.generated() {
                meta_openings += usize::from(generated.meta_opening);
                cut += usize::from(generated.cut_at_sentence);
            }
            for call in &result.calls {
                provider_calls += 1;
                if call.usage.is_billable() {
                    input_tokens += u64::from(call.usage.input_tokens);
                    output_tokens += u64::from(call.usage.output_tokens);
                    cache_read += u64::from(call.usage.cache_read_input_tokens);
                }
            }
        }
        let ready = by_status.get("ready").copied().unwrap_or(0);
        let statuses = render(&by_status);
        let classes = render(&by_class);
        if ready == results.len() {
            tracing::debug!(
                org_id = prepared.input.org_id,
                document_id = prepared.input.document_id,
                chunks = results.len(),
                prompt_version = prepared.version.as_str(),
                provider_calls,
                input_tokens,
                output_tokens,
                cache_read_input_tokens = cache_read,
                meta_openings,
                cut_at_sentence = cut,
                "contextual retrieval: chunks contextualized"
            );
        } else {
            let failed = by_status.get("failed_transient").copied().unwrap_or(0)
                + by_status.get("failed_permanent").copied().unwrap_or(0);
            // A refusal, a failure: said once per document with its classes, never
            // once per chunk. A throttled bulk ingest would otherwise write one
            // line per chunk of the corpus.
            if failed > 0 {
                tracing::warn!(
                    org_id = prepared.input.org_id,
                    document_id = prepared.input.document_id,
                    chunks = results.len(),
                    statuses = %statuses,
                    classes = %classes,
                    provider_calls,
                    "contextual retrieval: chunks not contextualized; they are embedded raw"
                );
            } else {
                tracing::info!(
                    org_id = prepared.input.org_id,
                    document_id = prepared.input.document_id,
                    chunks = results.len(),
                    statuses = %statuses,
                    classes = %classes,
                    provider_calls,
                    "contextual retrieval: chunks not contextualized; they are embedded raw"
                );
            }
        }
        results
    }
}

/// `Some(remaining)` while there is time (`None` inside when unbounded), `None`
/// when the deadline has passed.
fn remaining_or_expired(run: &Run<'_>) -> Option<Option<Duration>> {
    match run.remaining() {
        Some(left) if left.is_zero() => None,
        other => Some(other),
    }
}

fn deadline_outcome() -> ContextOutcome {
    // Due again at once: the reconciler picks it up, and the unit is embedded raw
    // in the meantime.
    ContextOutcome::pending(PendingReason::DeadlineExceeded, Duration::ZERO)
}

/// Split chunk positions into the first of each cacheable window (asked alone,
/// first) and the rest. Chunks of a window too short to cache are all in the first
/// stage, so nothing waits for nothing.
fn stages(windows: &[Window<'_>]) -> (Vec<usize>, Vec<usize>) {
    let mut seen = HashSet::new();
    let (mut leaders, mut followers) = (Vec::new(), Vec::new());
    for (position, window) in windows.iter().enumerate() {
        if window.text.len() >= PREFIX_CACHE_MIN_BYTES && !seen.insert(window.id) {
            followers.push(position);
        } else {
            leaders.push(position);
        }
    }
    (leaders, followers)
}

fn usage_of(response: &model_plane::v1::InferResponse) -> Usage {
    let tokens = |value: i32| u32::try_from(value.max(0)).unwrap_or(0);
    let timing = response.provider_timing.as_ref();
    let (input_tokens, output_tokens) = (
        tokens(response.input_tokens),
        tokens(response.output_tokens),
    );
    Usage {
        input_tokens,
        output_tokens,
        cache_read_input_tokens: tokens(response.cache_read_input_tokens),
        cache_creation_input_tokens: tokens(response.cache_creation_input_tokens),
        // A server that predates the timing field says nothing: both counts zero
        // then means unknown, as it does on the wire.
        usage_reported: timing.map_or(input_tokens > 0 || output_tokens > 0, |timing| {
            timing.usage_reported
        }),
        response_cache_hit: timing.is_some_and(|timing| timing.response_cache_hit),
    }
}

fn render(counts: &BTreeMap<&'static str, usize>) -> String {
    counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(",")
}

/// The boot-time contract of contextual retrieval (CR-T13), for the process that is about to run
/// it. With the feature on, the settings are valid (a variable out of range names itself) and the
/// service holds the credential its Auth Core token client needs; while the incident kill switch is
/// thrown there is nothing to check, because no model is asked. `main` calls it before anything
/// starts, so a deployment that cannot generate contexts stops there, with a message and a
/// non-zero exit, and does not run for weeks with the feature doing nothing (the consumer builds
/// its own contextualizer later, and a failure there only ended the consumer, quietly).
pub fn check_boot(cfg: &Config) -> anyhow::Result<()> {
    if !cfg.contextual_retrieval_state()?.is_on() {
        return Ok(());
    }
    Settings::from_config(cfg)?;
    super::token_client_from_config(cfg)?;
    Ok(())
}

#[cfg(test)]
mod live_proof;
/// A model for the batch pipeline's tests (a port that records and answers per tenant).
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
