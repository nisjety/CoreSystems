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
//! over `text` at weight A and `chunk_context` at weight B, so BOTH halves of
//! the technique (contextual embeddings and contextual BM25) come out of this
//! one generation.
//!
//! ## Failure and ZDR posture
//!
//! Best-effort by construction: any failure (mint, transport, empty answer,
//! timeout) falls back to embedding the raw chunk. A quality enhancement must
//! never be able to fail an ingest.
//!
//! ZDR is absolute, not best-effort: a restricted document's text is never
//! sent, and the guard is applied by the caller refusing to even ask (see
//! `batch::contextualize_items`), so there is no path where a retaining model
//! provider sees restricted content.
//!
//! OFF by default (`CONTEXTUAL_RETRIEVAL_ENABLED`). This spends one LLM call
//! per chunk on every first-time embed; that is a real per-tenant cost and
//! deserves an explicit decision rather than arriving with a deploy.

use std::time::Duration;

use anyhow::Context as _;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

use super::inference_auth::{InferenceBearer, InferenceTokenClient, RetentionPosture};
use super::model_plane;
use crate::config::Config;

/// Bumped whenever the prompt below changes in a way that alters the generated
/// context. Persisted with the text so a later audit can tell which prompt
/// produced a given stored context, and so a future refresh pass can find
/// entries built by an older prompt.
pub const PROMPT_VERSION: &str = "ctx-v1";

/// The instruction sent per chunk. Deliberately constrains the model to
/// *situating* information only: a summary or an interpretation would add
/// tokens that compete with the chunk's own terms in the embedding, which is
/// the opposite of the goal.
fn build_prompt(document_text: &str, chunk_text: &str) -> String {
    format!(
        "Here is a document:\n\n<document>\n{document_text}\n</document>\n\n\
         Here is a chunk taken from that document:\n\n<chunk>\n{chunk_text}\n</chunk>\n\n\
         Give one or two short sentences that situate this chunk within the \
         document, so the chunk can be found by search. Name the concrete \
         entities, dates, sections, or subjects the chunk refers to only \
         implicitly. Do not summarise the chunk, do not interpret it, and do \
         not add anything that is not in the document. Reply with the situating \
         sentences alone and nothing else."
    )
}

/// Joins the generated context to the chunk to form the text that gets embedded.
///
/// A blank line separator, and context first: the retrieval query matches
/// against the whole string, and leading context is what makes an
/// otherwise-anonymous chunk match an entity-bearing query.
pub fn compose_contextualized(context: &str, chunk_text: &str) -> String {
    format!("{}\n\n{}", context.trim(), chunk_text)
}

#[derive(Clone)]
pub struct Contextualizer {
    client: model_plane::v1::inference_core_client::InferenceCoreClient<Channel>,
    token_client: InferenceTokenClient,
    model: String,
    provider_hint: String,
    /// Hard cap on how much document text goes into the prompt. A document is
    /// arbitrary user content and can be megabytes; without a cap one oversized
    /// upload would blow the model's context window (a hard error) or quietly
    /// cost hundreds of times what a normal document does.
    max_document_chars: usize,
    max_context_tokens: i32,
    timeout: Duration,
    concurrency: usize,
    retry_attempts: u32,
    retry_base: Duration,
}

/// Why one chunk's context generation failed.
///
/// The split exists so the retry wrapper can distinguish "the provider is busy"
/// from "this request is malformed". Collapsing both into one error type is what
/// made the original code retry-blind.
enum ContextError {
    /// Inference answered with a gRPC status. Retryable iff `is_transient`.
    Status(tonic::Status),
    /// A local failure — never retryable, because nothing upstream will change.
    Local(anyhow::Error),
}

/// Whether a failed inference call is worth retrying.
///
/// `ResourceExhausted` is the one that matters in practice — it is how a
/// provider rate limit arrives, and a bulk backfill generates them in bursts.
/// `Unavailable` and `DeadlineExceeded` cover a restarting inference pod and a
/// slow provider respectively.
///
/// Everything else is deliberately terminal. Retrying `InvalidArgument`,
/// `PermissionDenied`, or `Unauthenticated` would burn the whole backoff budget
/// re-sending a request that cannot succeed, turning a fast clear failure into a
/// slow one and delaying every other chunk behind it.
fn is_transient(code: tonic::Code) -> bool {
    matches!(
        code,
        tonic::Code::ResourceExhausted | tonic::Code::Unavailable | tonic::Code::DeadlineExceeded
    )
}

/// Backoff before retry `attempt` (0-based) for the chunk at `chunk_index`.
///
/// Exponential in the attempt, plus a deterministic per-chunk offset. The offset
/// is what keeps `concurrency` chunks that were rate-limited by the same upstream
/// burst from retrying in lockstep and re-triggering the same limit; spreading
/// them across the step is the point. It is derived from the chunk index rather
/// than drawn randomly so the schedule is reproducible in tests — `rand` is a
/// dev-dependency here, and decorrelating concurrent retries within one document
/// is the only property the jitter needs.
fn retry_backoff(base: Duration, attempt: u32, chunk_index: usize) -> Duration {
    let step = base.saturating_mul(1u32 << attempt.min(6));
    let spread = step / 4;
    let offset = if spread.is_zero() {
        Duration::ZERO
    } else {
        // `% 4` then scaled, so the offset stays inside one quarter-step.
        spread * ((chunk_index % 4) as u32) / 4
    };
    step + offset
}

impl Contextualizer {
    /// `Ok(None)` when the feature is off — an absent contextualizer is the
    /// normal state, not an error, so callers treat it as "embed raw text".
    pub fn from_config(cfg: &Config) -> anyhow::Result<Option<Self>> {
        if !cfg.contextual_retrieval_enabled {
            return Ok(None);
        }
        if cfg.contextual_retrieval_model.trim().is_empty() {
            anyhow::bail!(
                "CONTEXTUAL_RETRIEVAL_MODEL is required when CONTEXTUAL_RETRIEVAL_ENABLED=true"
            );
        }
        let timeout = Duration::from_millis(cfg.contextual_retrieval_timeout_ms.max(1));
        // Reuses this service's existing inference credential and retention
        // posture — the same principal already used for model-plane embedding,
        // so enabling this grants no authority the service did not already hold.
        //
        // Strict in a real deployment: an operator who asked for contextual
        // retrieval and supplied no credential should be told at startup, not
        // discover months later that nothing was ever contextualized. Standalone
        // startup relaxes it exactly as the embedding provider does — the client
        // stays unusable, so mints fail, so every chunk falls back to raw text
        // rather than making an unauthenticated inference call.
        let posture = RetentionPosture::parse(&cfg.model_plane_inference_retention_posture)?;
        let token_client = if super::standalone_startup() {
            InferenceTokenClient::new_allow_unconfigured(
                &cfg.model_plane_inference_token_url,
                &cfg.model_plane_inference_token_issuer,
                &cfg.model_plane_inference_service_id,
                &cfg.model_plane_inference_service_api_key,
                posture,
            )?
        } else {
            InferenceTokenClient::new(
                &cfg.model_plane_inference_token_url,
                &cfg.model_plane_inference_token_issuer,
                &cfg.model_plane_inference_service_id,
                &cfg.model_plane_inference_service_api_key,
                posture,
            )?
        };
        let channel = Endpoint::from_shared(cfg.model_plane_ai_core_grpc_url.clone())
            .with_context(|| {
                format!(
                    "invalid MODEL_PLANE_AI_CORE_GRPC_URL `{}`",
                    cfg.model_plane_ai_core_grpc_url
                )
            })?
            .connect_timeout(timeout)
            .timeout(timeout)
            .connect_lazy();
        Ok(Some(Self {
            client: model_plane::v1::inference_core_client::InferenceCoreClient::new(channel),
            token_client,
            model: cfg.contextual_retrieval_model.trim().to_string(),
            provider_hint: cfg.contextual_retrieval_provider_hint.trim().to_string(),
            max_document_chars: cfg.contextual_retrieval_max_document_chars,
            max_context_tokens: cfg.contextual_retrieval_max_tokens,
            timeout,
            concurrency: cfg.contextual_retrieval_concurrency.max(1),
            retry_attempts: cfg.contextual_retrieval_retry_attempts,
            retry_base: Duration::from_millis(cfg.contextual_retrieval_retry_base_ms.max(1)),
        }))
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Contextualize every chunk of ONE document.
    ///
    /// Returns one entry per input chunk, positionally aligned: `Some(context)`
    /// where generation succeeded, `None` where it did not. `None` is a normal
    /// outcome, not a failure to propagate — the caller embeds the raw chunk.
    /// The whole method is infallible for exactly that reason: there is no error
    /// a caller could usefully act on that is not already expressed by `None`.
    ///
    /// One bearer is minted for the whole document rather than one per chunk:
    /// the token is already scoped to precisely this org and audience, so
    /// per-chunk minting would add a round trip each without narrowing
    /// authority. A mint failure yields all-`None` — every chunk falls back,
    /// and the ingest proceeds.
    ///
    /// The bearer never leaves this module. That is deliberate: an API where
    /// the caller holds the credential invites it being reused for a request
    /// this service never authorized.
    pub async fn contextualize_document(
        &self,
        org_id: &str,
        document_text: &str,
        chunks: &[&str],
    ) -> Vec<Option<String>> {
        if chunks.is_empty() {
            return Vec::new();
        }
        let bearer = match self.token_client.mint(org_id).await {
            Ok(bearer) => bearer,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    org_id,
                    chunks = chunks.len(),
                    "contextual retrieval: bearer mint failed; embedding raw chunks"
                );
                return vec![None; chunks.len()];
            }
        };

        use futures::stream::StreamExt;
        futures::stream::iter(chunks.iter().enumerate().map(|(index, chunk)| {
            let bearer = bearer.clone();
            async move {
                self.context_for_chunk_with_retry(&bearer, org_id, document_text, chunk, index)
                    .await
            }
        }))
        // `buffered`, not `buffer_unordered`: the results are consumed
        // positionally against the chunk list, so order is load-bearing —
        // an unordered stream would attach one chunk's context to another.
        .buffered(self.concurrency)
        .collect()
        .await
    }

    /// `context_for_chunk`, retrying transient inference failures.
    ///
    /// Always resolves to `Option<String>`: a chunk that cannot be
    /// contextualized falls back to raw embedding, which is correct behaviour
    /// rather than an error to propagate. What the retries buy is that a
    /// *recoverable* upstream condition — nearly always a rate limit during a
    /// bulk backfill — stops silently costing coverage.
    async fn context_for_chunk_with_retry(
        &self,
        bearer: &InferenceBearer,
        org_id: &str,
        document_text: &str,
        chunk_text: &str,
        chunk_index: usize,
    ) -> Option<String> {
        let mut attempt = 0u32;
        loop {
            match self
                .context_for_chunk(bearer, org_id, document_text, chunk_text)
                .await
            {
                Ok(context) => return context,
                Err(ContextError::Local(e)) => {
                    tracing::warn!(
                        error = %e,
                        org_id,
                        chunk_index,
                        "contextual retrieval: generation failed; embedding raw chunk"
                    );
                    return None;
                }
                Err(ContextError::Status(status)) => {
                    let retryable = is_transient(status.code()) && attempt < self.retry_attempts;
                    if !retryable {
                        tracing::warn!(
                            code = %status.code(),
                            message = %status.message(),
                            org_id,
                            chunk_index,
                            attempts = attempt + 1,
                            transient = is_transient(status.code()),
                            "contextual retrieval: generation failed; embedding raw chunk"
                        );
                        return None;
                    }
                    let delay = retry_backoff(self.retry_base, attempt, chunk_index);
                    tracing::debug!(
                        code = %status.code(),
                        org_id,
                        chunk_index,
                        attempt = attempt + 1,
                        delay_ms = delay.as_millis(),
                        "contextual retrieval: transient inference failure; retrying"
                    );
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        }
    }

    /// Generate the situating context for one chunk.
    ///
    /// Returns `Ok(None)` when the model replied with nothing usable — treated
    /// as "no context available" rather than an error, because the fallback
    /// (embed the raw chunk) is exactly what the caller does on `Err` anyway,
    /// and distinguishing them only matters for logging.
    async fn context_for_chunk(
        &self,
        bearer: &InferenceBearer,
        org_id: &str,
        document_text: &str,
        chunk_text: &str,
    ) -> Result<Option<String>, ContextError> {
        // Truncate on a char boundary, never a byte offset: document content is
        // Norwegian-first in this corpus, so a byte slice would panic on the
        // first `ø` that straddles the cap.
        let bounded: String = document_text
            .chars()
            .take(self.max_document_chars)
            .collect();
        let request = model_plane::v1::InferRequest {
            request_id: Uuid::new_v4().to_string(),
            org_id: org_id.to_string(),
            model: self.model.clone(),
            provider_hint: self.provider_hint.clone(),
            messages: vec![model_plane::v1::ChatMessage {
                role: "user".to_string(),
                content: build_prompt(&bounded, chunk_text),
                name: String::new(),
            }],
            // Zero temperature: the same chunk in the same document must
            // produce the same context, or a reindex silently changes what got
            // embedded for reasons unrelated to the content.
            temperature: 0.0,
            max_tokens: self.max_context_tokens,
            structured_output_schema: String::new(),
            // This path never carries restricted content — the caller filters
            // ZDR items out before asking — so the flag is false rather than
            // plumbed. A restricted document reaching here would be a caller
            // bug, and `batch` asserts against it.
            zdr: false,
            tools: Vec::new(),
            tool_choice: String::new(),
        };

        let mut req = tonic::Request::new(request);
        req.set_timeout(self.timeout);
        let bearer_value: MetadataValue<_> = format!("Bearer {}", bearer.as_str())
            .parse()
            .context("inference bearer is not a valid header value")
            .map_err(ContextError::Local)?;
        req.metadata_mut().insert("authorization", bearer_value);

        // The `Status` is carried intact rather than flattened to a message, so
        // the retry wrapper can read its code. Only the code and message are
        // ever logged, never the payload: a provider error body can echo the
        // prompt, and the prompt contains document text.
        let response = self
            .client
            .clone()
            .infer(req)
            .await
            .map_err(ContextError::Status)?
            .into_inner();

        let context = response.content.trim();
        if context.is_empty() {
            return Ok(None);
        }
        Ok(Some(context.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_recoverable_inference_statuses_are_retried() {
        // A rate limit is the whole reason this exists: bulk contextualization
        // of a 1,164-chunk corpus lost 300 chunks of coverage to
        // ResourceExhausted, silently, because every failure fell back to raw.
        assert!(is_transient(tonic::Code::ResourceExhausted));
        assert!(is_transient(tonic::Code::Unavailable));
        assert!(is_transient(tonic::Code::DeadlineExceeded));

        // Retrying these would spend the entire backoff budget re-sending a
        // request that cannot succeed, delaying every chunk queued behind it.
        assert!(!is_transient(tonic::Code::InvalidArgument));
        assert!(!is_transient(tonic::Code::PermissionDenied));
        assert!(!is_transient(tonic::Code::Unauthenticated));
        assert!(!is_transient(tonic::Code::NotFound));
        assert!(!is_transient(tonic::Code::Internal));
    }

    #[test]
    fn backoff_doubles_and_spans_the_providers_retry_window() {
        let base = Duration::from_millis(2_000);
        // Attempt n waits 2^n * base, so 4 attempts span 2+4+8+16 = 30s —
        // covering the 30-second Retry-After this Azure deployment returns.
        let total: Duration = (0..4).map(|a| retry_backoff(base, a, 0)).sum();
        assert_eq!(total, Duration::from_millis(30_000));
        assert_eq!(retry_backoff(base, 0, 0), Duration::from_millis(2_000));
        assert_eq!(retry_backoff(base, 3, 0), Duration::from_millis(16_000));
    }

    #[test]
    fn concurrent_chunks_do_not_retry_in_lockstep() {
        let base = Duration::from_millis(2_000);
        // Chunks rate-limited by one upstream burst must not all come back at
        // the same instant and re-trigger the same limit.
        let delays: Vec<Duration> = (0..4).map(|i| retry_backoff(base, 0, i)).collect();
        let distinct: std::collections::HashSet<u128> =
            delays.iter().map(|d| d.as_millis()).collect();
        assert_eq!(distinct.len(), 4, "offsets collided: {delays:?}");

        // The spread stays inside one quarter-step, so jitter never reorders
        // attempts across backoff generations.
        for d in &delays {
            assert!(
                *d >= base && *d < base + base / 4,
                "{d:?} escaped its step"
            );
        }
    }

    #[test]
    fn backoff_saturates_rather_than_overflowing() {
        // A misconfigured attempt count must not panic on the shift.
        let base = Duration::from_secs(1);
        for attempt in [6u32, 7, 32, u32::MAX] {
            let d = retry_backoff(base, attempt, 1);
            assert!(d >= base, "attempt {attempt} produced {d:?}");
        }
    }

    #[test]
    fn the_prompt_carries_both_the_document_and_the_chunk() {
        let prompt = build_prompt("full document body", "the chunk");
        assert!(prompt.contains("<document>\nfull document body\n</document>"));
        assert!(prompt.contains("<chunk>\nthe chunk\n</chunk>"));
        // The instruction must forbid summarising: a summary competes with the
        // chunk's own terms in the embedding instead of adding findable ones.
        assert!(prompt.contains("Do not summarise"));
    }

    #[test]
    fn context_is_prepended_and_the_chunk_survives_verbatim() {
        let composed = compose_contextualized("  This is from ACME's Q2 report.  ", "Margin: 31%.");
        assert_eq!(
            composed,
            "This is from ACME's Q2 report.\n\nMargin: 31%."
        );
        assert!(
            composed.ends_with("Margin: 31%."),
            "the chunk must be preserved exactly, not rewritten"
        );
    }

    /// A byte-offset truncation would panic partway through a multi-byte char.
    /// The corpus is Norwegian-first, so this is the common case, not an edge.
    #[test]
    fn document_truncation_never_splits_a_multibyte_character() {
        let norwegian = "ø".repeat(1000);
        for cap in [0_usize, 1, 7, 999, 1000, 5000] {
            let bounded: String = norwegian.chars().take(cap).collect();
            assert_eq!(bounded.chars().count(), cap.min(1000));
            // Round-trips as valid UTF-8 by construction; the assertion is that
            // building it did not panic and no char was cut in half.
            assert!(bounded.chars().all(|c| c == 'ø'));
        }
    }

    /// Only three env vars are required by `Config`; everything this module
    /// reads is `#[serde(default)]`, which is exactly what the off-by-default
    /// assertions below are checking.
    fn minimal_config() -> Config {
        envy::from_iter::<_, Config>(vec![
            (
                "DATABASE_URL".to_string(),
                "postgres://test.invalid/test".to_string(),
            ),
            ("NATS_URL".to_string(), "nats://test.invalid:4222".to_string()),
            ("QDRANT_URL".to_string(), "http://test.invalid:6334".to_string()),
        ])
        .expect("minimal config")
    }

    #[test]
    fn a_disabled_config_yields_no_contextualizer_rather_than_an_error() {
        let cfg = minimal_config();
        assert!(
            !cfg.contextual_retrieval_enabled,
            "contextual retrieval must default to OFF: it spends one inference \
             call per chunk and must never switch on by deploying"
        );
        let built = Contextualizer::from_config(&cfg).expect("disabled is not an error");
        assert!(built.is_none());
    }

    #[test]
    fn enabling_without_a_model_fails_closed() {
        let cfg = Config {
            contextual_retrieval_enabled: true,
            contextual_retrieval_model: "   ".to_string(),
            ..minimal_config()
        };
        let err = match Contextualizer::from_config(&cfg) {
            Ok(_) => panic!("an enabled contextualizer with no model must fail"),
            Err(err) => err,
        };
        assert!(
            err.to_string().contains("CONTEXTUAL_RETRIEVAL_MODEL"),
            "unexpected error: {err}"
        );
    }

    /// Concurrency is clamped to at least 1: `buffered(0)` yields nothing
    /// forever, so a misconfigured `0` would hang every ingest rather than
    /// degrade it.
    // `tokio::test` because building the contextualizer calls
    // `Endpoint::connect_lazy`, which registers with the Tokio reactor even
    // though it opens no connection.
    #[tokio::test]
    async fn zero_concurrency_is_clamped_rather_than_stalling_the_stream() {
        let cfg = Config {
            contextual_retrieval_enabled: true,
            contextual_retrieval_model: "claude-haiku-4-5-20251001".to_string(),
            contextual_retrieval_concurrency: 0,
            // A real credential is required outside standalone startup; the
            // value is never used because nothing here reaches the network.
            model_plane_inference_service_api_key: "test-key-0123456789".to_string(),
            ..minimal_config()
        };
        let built = Contextualizer::from_config(&cfg)
            .expect("valid config")
            .expect("enabled");
        assert_eq!(built.concurrency, 1);
    }

    #[tokio::test]
    async fn an_empty_chunk_list_makes_no_calls_and_mints_nothing() {
        let cfg = Config {
            contextual_retrieval_enabled: true,
            contextual_retrieval_model: "claude-haiku-4-5-20251001".to_string(),
            // Unreachable on purpose: reaching the network at all would surface
            // as a hang/error instead of the empty vec asserted below.
            model_plane_ai_core_grpc_url: "http://127.0.0.1:1".to_string(),
            model_plane_inference_service_api_key: "test-key-0123456789".to_string(),
            ..minimal_config()
        };
        let built = Contextualizer::from_config(&cfg)
            .expect("valid config")
            .expect("enabled");
        let out = built.contextualize_document("org-1", "doc body", &[]).await;
        assert!(out.is_empty());
    }
}
