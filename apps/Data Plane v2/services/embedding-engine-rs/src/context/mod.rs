//! Contextual retrieval's pure logic: what is asked, what part of the document the
//! model reads, what is accepted from the answer, and what each result means.
//!
//! The client that calls inference-core is `crate::provider::contextualize`. It is
//! small on purpose; everything that decides something lives here, without a
//! network, so it can be tested with the real inputs and the real rules instead of
//! a stand-in for a server:
//!
//! * [`prompt`]: the prompt in its versions (`ctx-v1` byte for byte as it shipped,
//!   `ctx-v2` behind a pin), the context composed into the embedded text, and the
//!   hash of what a context was generated from.
//! * [`window`]: the part of a document one chunk is read in, character-safe.
//! * [`validate`]: what is kept of the model's answer, and why anything is not.
//! * [`outcome`]: the closed set of results, their `context_status` and
//!   `context_error_class`, and the one table that turns a decoded failure into
//!   one.
//!
//! This task defined the outcome contract; the write-ahead batch pipeline (CR-T8),
//! the reconciler (CR-T9a) and the budget gate (CR-T9b) are its consumers and have all
//! landed. The allowance below is what is left of the time they had not: a few items
//! that only the tests call (and that the operator's `context-backfill`, which compiles
//! this module without using all of it, does not), seven of them when the last consumer
//! landed, found by taking the allowance off. Narrowing it to those items, or removing
//! them, belongs to whoever next edits them.
#![allow(dead_code)]

use std::time::Duration;

use tokio::time::Instant;

pub mod outcome;
pub mod prompt;
#[cfg(test)]
pub(crate) mod testing;
pub mod validate;
pub mod window;

pub use outcome::{
    ChunkResult, ContextOutcome, ErrorClass, Generated, MeteredCall, PendingReason, Usage,
    ZdrReason,
};
pub use prompt::{DocumentMeta, PromptVersion};
pub use window::{Window, WindowConfig, WindowedDocument};

/// The longest the inline phase may spend on one batch: half of the consumer's
/// `ack_wait` (60 s, `stream::create_consumer`). A batch also embeds, writes to
/// Qdrant and Postgres, and publishes after contextualizing; spending most of the
/// ack window on generation would redeliver the message mid-work and pay for the
/// same contexts twice. The signed event envelope's 120 s time to live is the other
/// bound and is looser.
pub const INLINE_BATCH_BUDGET_CEILING: Duration = Duration::from_secs(30);

/// How the inline phase is time-boxed.
///
/// `batch` of zero means no generation happens inline at all: every unit is
/// embedded raw and left `pending` for the reconciler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InlineBudget {
    pub document: Duration,
    pub batch: Duration,
}

impl InlineBudget {
    pub fn new(document_ms: u64, batch_ms: u64) -> Result<Self, String> {
        let document = Duration::from_millis(document_ms);
        let batch = Duration::from_millis(batch_ms);
        if batch > INLINE_BATCH_BUDGET_CEILING {
            return Err(format!(
                "CONTEXT_INLINE_BUDGET_MS is {batch_ms} but must not exceed {} ms: it is half of \
                 the 60 s JetStream ack_wait, and generation that runs longer redelivers the \
                 message and pays for the same contexts twice",
                INLINE_BATCH_BUDGET_CEILING.as_millis()
            ));
        }
        if !batch.is_zero() && document.is_zero() {
            return Err(
                "CONTEXT_INLINE_DOCUMENT_BUDGET_MS must be greater than 0 when CONTEXT_INLINE_BUDGET_MS is"
                    .to_string(),
            );
        }
        if document > batch && !batch.is_zero() {
            return Err(format!(
                "CONTEXT_INLINE_DOCUMENT_BUDGET_MS ({document_ms}) must not exceed \
                 CONTEXT_INLINE_BUDGET_MS ({batch_ms})"
            ));
        }
        Ok(Self { document, batch })
    }

    /// Start the clock for one batch.
    pub fn start(&self, now: Instant) -> BatchClock {
        BatchClock {
            batch_deadline: now + self.batch,
            document: self.document,
        }
    }
}

/// The deadlines of one batch in flight.
#[derive(Clone, Copy, Debug)]
pub struct BatchClock {
    batch_deadline: Instant,
    document: Duration,
}

impl BatchClock {
    /// When the batch's inline time is up.
    pub fn batch_deadline(&self) -> Instant {
        self.batch_deadline
    }

    /// The deadline for a document that starts now: its own budget, but never past
    /// the batch's, so a slow document cannot take the time a later one needs.
    pub fn document_deadline(&self, now: Instant) -> Instant {
        (now + self.document).min(self.batch_deadline)
    }
}

/// One document's worth of work.
#[derive(Clone, Copy, Debug)]
pub struct DocumentInput<'a> {
    /// The organization whose content this is. The bearer is minted for it.
    pub org_id: &'a str,
    /// For log correlation only; never sent anywhere.
    pub document_id: &'a str,
    pub meta: DocumentMeta<'a>,
    /// The document's full text.
    pub text: &'a str,
    /// The document is classified restricted (`zdr_classification = 'restricted'`).
    /// Nothing of it is read, windowed or sent.
    pub restricted: bool,
    /// An organization's pin of the prompt version. `None` uses the service's.
    pub prompt_version: Option<PromptVersion>,
    /// How many chunks the document has, when known. Makes the position of a chunk
    /// whose text cannot be found exact instead of estimated.
    pub total_chunks: Option<usize>,
}

/// One chunk of a document.
#[derive(Clone, Copy, Debug)]
pub struct ChunkInput<'a> {
    pub index: i32,
    pub text: &'a str,
}

/// A document, windowed and ready: what the caller needs to decide reuse (the input
/// hash of each chunk) before it pays for generation.
pub struct PreparedDocument<'a> {
    pub(crate) input: DocumentInput<'a>,
    pub(crate) version: PromptVersion,
    pub(crate) windows: WindowedDocument<'a>,
}

impl<'a> PreparedDocument<'a> {
    pub(crate) fn new(
        input: DocumentInput<'a>,
        service_version: PromptVersion,
        window: WindowConfig,
    ) -> Self {
        Self {
            version: input.prompt_version.unwrap_or(service_version),
            windows: WindowedDocument::new(input.text, window),
            input,
        }
    }

    pub fn version(&self) -> PromptVersion {
        self.version
    }

    pub fn input(&self) -> &DocumentInput<'a> {
        &self.input
    }

    pub fn windows(&self) -> &WindowedDocument<'a> {
        &self.windows
    }

    /// The window one chunk is read in.
    pub fn window_for(&self, chunk: &ChunkInput<'_>) -> Window<'_> {
        self.windows
            .window_for(chunk.text, chunk.index, self.input.total_chunks)
    }
}

#[cfg(test)]
mod tests;
