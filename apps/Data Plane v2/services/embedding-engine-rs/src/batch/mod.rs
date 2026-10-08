//! The batch pipeline: turn a buffer of units into contexts, vectors and
//! announcements, in an order that cannot leave a vector that does not match its
//! context.
//!
//! One batch is what the stream consumer fetched: units of several organizations at
//! once, any of which may be a redelivery. The steps, in this order:
//!
//! 1. **Contexts** ([`contexts::ensure`]), before anything is embedded, because they decide
//!    what text is embedded. First, what each organization's operator policy says
//!    (`org_processing_policy`, [`crate::policy`]): an organization that is off gets no provider
//!    call and a `skipped_policy` status on its units. The unit's stored state is read; a context made from the same
//!    inputs is reused (no provider call); then the background budget ([`crate::budget`], CR-T9b),
//!    for an organization that is about to be asked for a context: alert-only and silent by default,
//!    and, only when an operator set it, `throttle` (one chunk at a time, the rest wait) or `stop`
//!    (a `skipped_budget` status and no provider call); what must be generated is generated inside the
//!    inline time box, several documents at a time; and every result is WRITTEN AHEAD:
//!    the context and its provenance in one statement, or a status that says why there is
//!    none. Then what the database holds is read back and put on the items. From here on
//!    the pipeline works from the persisted context and from nothing else.
//! 2. **Embed** `compose(persisted context, text)`; a unit with no context embeds its raw text.
//! 3. **Store the vectors**, each with the `source` of its document as the database has it (read
//!    once, before anything is embedded): the payload key a corpus-scoped search filters on.
//! 4. **Mark the units done**, in one statement that also records which model embedded
//!    them and the hash of the context each vector was built from (`NULL` for a raw
//!    vector). Nothing writes a unit after this, so `updated_at` equals `embedded_at`
//!    and the stale-embedding detector sees no change that is not one.
//! 5. **Announce** `documents.indexed` for each document with no unit left to embed. That is
//!    the keyword index's sync, and it comes after the contexts are written. Then, when the usage
//!    ledger is off, the version 1 cost event.
//!
//! The usage ledger (CR-T10, [`usage`]) is not a step: it is recorded where the money is spent.
//! The contextualization calls are recorded inside step 1, as soon as they are made and before its
//! own database writes, and each embedding request as soon as it returns, so a failure later in the
//! batch cannot lose spend that has happened.
//!
//! The order is the reason every failure converges. A failure after step 1 leaves the
//! contexts in the database, so the broker's redelivery reuses them and generates
//! nothing; a failure after step 3 leaves vectors that the retry overwrites with the
//! same values. A vector can never be built from a context that is not stored, and a
//! stored context can only be missing a vector until the unit is marked done.
//!
//! The pipeline never writes `embedding_status = 'failed'` (D18): a context that could not
//! be made is a `context_status`, never an embedding failure, and the terminal marking of
//! an embedding that cannot be made belongs to `stream::run_consumer`, which alone knows
//! the broker's delivery count.

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use anyhow::Context;
use sqlx::PgPool;

use crate::budget::BudgetGate;
use crate::context::prompt::stored_context_hash;
use crate::policy::PolicyReader;
use crate::provider::contextualize::Contextualizer;
use crate::qdrant_writer::EmbeddingPoint;
use event_envelope_rs::EventSigner;
use usage_ledger_rs::LedgerSink;

mod contexts;
mod generate;
mod plan;
mod ports;
mod schedule;
mod store;
// `pub(crate)`: the reconciler's tests (CR-T9a) run on the same rig, with the same real
// token client, real policy table and recording fakes, instead of a second copy of them.
#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod tests;
mod usage;

pub(crate) use ports::{Embedder, EventSink, PortFuture, VectorSink};
// The reconciler reads and extends the pipeline's own retry schedule instead of keeping a
// second ladder that agrees with it until somebody changes a number (see `schedule`).
pub(crate) use schedule::ladder_step;
pub(crate) use store::ensure_schema;
pub(crate) use usage::usage_signer;

// §17.3.3 — named subjects, lint-checked. See infra/nats/SUBJECTS.md.
const SUBJECT_DOC_INDEXED: &str = "dataplane.documents.indexed";
// Pre-existing inline literal, extracted while here: `check-subjects.sh`
// rejects `publish("dataplane.…")` with a bare string.
const SUBJECT_COST_LEDGER: &str = "dataplane.cost.ledger";

pub struct BatchItem {
    pub knowledge_id: String,
    pub document_id: String,
    pub org_id: String,
    pub chunk_index: i32,
    pub text: String,
    /// True when the owning document is `zdr_classification = 'restricted'`
    /// (Zero Data Retention). Sourced from the `documents` row at enqueue time
    /// (see `stream`). Drives the embed egress guard: a restricted doc must not
    /// egress to a retaining (direct-Azure) embedding provider, and its text is
    /// never read for a context.
    pub zdr: bool,
    pub user_id: Option<String>,
    /// P2-3: the owning document's `document_date` (source content's own
    /// last-modified time), sourced from the `documents` row at enqueue time
    /// alongside `zdr`. `None` for the common case of a document with no
    /// connector-supplied date -- the retrieval decay stage treats that as no
    /// penalty, not maximum penalty.
    pub document_date: Option<chrono::DateTime<chrono::Utc>>,
    /// Contextual Retrieval: the sentences situating this chunk inside its document
    /// that are STORED for it (`knowledge_units.chunk_context`), or `None` when it has
    /// none.
    ///
    /// Whatever a caller puts here is ignored: the pipeline sets it from the database
    /// after the write-ahead (see [`contexts::ensure`]), because a vector built from a
    /// context that is not stored is a vector that cannot be reproduced. The stream
    /// builds items with `None`.
    ///
    /// The CONTEXT ALONE, not the chunk with the context prepended. The composed
    /// form is a pure function of this plus `text` (`compose_contextualized`), so
    /// materialising it here would just duplicate the chunk. Storing them apart is
    /// also what lets the lexical arm index the chunk's own words in exactly one
    /// field and the context in another — BM25 sums across fields, so a composed
    /// string indexed next to the chunk would score the chunk's terms twice.
    ///
    /// `text` stays the document's own words and remains what the Qdrant payload
    /// carries and what callers are shown, so a citation never displays a model's
    /// preamble. See `provider::contextualize`.
    pub chunk_context: Option<String>,
}

/// What a batch is run with. One struct, not eight arguments, because the
/// reconciler builds the same one and calls the same pipeline: there is one
/// implementation of "make these units' contexts and vectors right", not two that
/// agree for now.
pub(crate) struct Pipeline<'a> {
    pub provider: &'a dyn Embedder,
    pub vectors: &'a dyn VectorSink,
    pub pool: &'a PgPool,
    pub collection: &'a str,
    pub events: &'a dyn EventSink,
    pub event_signer: Option<&'a EventSigner>,
    /// `None` when contextual retrieval is switched off: nothing is generated, but what
    /// is already stored still builds the vectors.
    pub contextualizer: Option<&'a Contextualizer>,
    /// What each organization's operator policy says (`org_processing_policy`, cached for
    /// 30 seconds). Consulted only when `contextualizer` is `Some`: the global switch is the
    /// incident kill switch and comes first. An organization whose policy is off gets no
    /// provider call and a `skipped_policy` status on its units.
    pub policy: &'a PolicyReader,
    /// The background LLM budget gate (CR-T9b), one per process and shared by the stream
    /// consumer and the reconciler. Consulted only for an organization that is about to be asked
    /// for a context, and only while generation is on. Alert-only and without a figure by default,
    /// in which case it changes nothing and reads nothing.
    pub budget: &'a BudgetGate,
    /// The usage ledger (CR-T10): where the calls this batch pays for are recorded, as
    /// version 2 events on the durable `DATAPLANE_COST` stream. `None` (the default, until
    /// `USAGE_EMIT_ENABLED=1`) records nothing there, and the version 1 cost event is published
    /// as it always was. Recording never fails a batch.
    pub usage: Option<&'a dyn LedgerSink>,
    /// What this run is for (CR-T9a). The stream consumer runs [`PassKind::Inline`]; the others
    /// are the reconciler's. Two of them change one decision of the pipeline each; every one of
    /// them is a batch that embeds units that were embedded before, which is what the version 1
    /// cost event's key has to know (see step 6 of [`process_batch`]).
    pub pass: PassKind,
}

/// What a run of the pipeline is for.
///
/// The decisions of the pipeline are the same for every run (it is one implementation of
/// "make these units' contexts and vectors right"); a pass kind changes the two that depend
/// on who asked, and nothing else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PassKind {
    /// The stream consumer's batch: units that arrive to be embedded, and that the broker
    /// redelivers whole when a batch fails. A stored context made with another prompt version is
    /// kept (a re-embed does not pay for a rollout), and a `legacy` unit stays `legacy` (inline
    /// code does not know enough about a context of unknown provenance to promote it).
    #[default]
    Inline,
    /// The reconciler's generation and repair runs: the decisions of [`Self::Inline`], on units
    /// that were embedded before. Nothing about the pipeline changes except what a batch means to
    /// the version 1 cost event: it is never a redelivery, so its embedding is a payment of its own
    /// (see step 6 of [`process_batch`]).
    Reconcile,
    /// The reconciler's legacy verification pass. Embedding a `legacy` unit from its stored
    /// context proves its vector reflects it: in the statement that marks the unit done and
    /// records the hash, a `legacy` unit whose recorded hash is the hash of its stored
    /// context becomes `ready`. No model is called (the reconciler passes none), the
    /// context's own provenance stays unknown (`context_model`, `context_input_hash` are not
    /// invented), and the unit is not written again afterwards, so `updated_at` still equals
    /// `embedded_at` and the stale-embedding detector sees no change that is not one.
    VerifyLegacy,
    /// The reconciler's prompt refresh (`CONTEXT_REFRESH_ON_PROMPT_CHANGE`, capped). A stored
    /// context made with a prompt version other than the one in force for the organization is
    /// regenerated; the old context stays in force until the new one is written, so a failed
    /// regeneration costs nothing.
    RefreshPrompt,
}

pub(crate) async fn process_batch(
    items: Vec<BatchItem>,
    pipeline: &Pipeline<'_>,
) -> anyhow::Result<()> {
    let Pipeline {
        provider,
        vectors: qdrant,
        pool,
        collection,
        events: nats,
        event_signer,
        contextualizer: _,
        policy: _,
        budget: _,
        usage: ledger,
        pass,
    } = *pipeline;
    if items.is_empty() {
        return Ok(());
    }
    let started = std::time::Instant::now();

    // 1. Contexts, written ahead. Infallible per unit: a unit whose context cannot be
    //    made is embedded raw with a status that says why; only an unreachable database
    //    fails the batch.
    let (items, report) = contexts::ensure(pipeline, items).await?;
    // The calls the context phase made are spend that has happened. They were recorded inside
    // `ensure`, as soon as they were made, so that a database that fails the write-ahead
    // cannot lose them with the batch (a redelivery generates again and pays again).
    let mut usage_lost = report.usage_lost;
    if items.is_empty() {
        return Ok(());
    }

    let doc_ids: Vec<String> = items.iter().map(|i| i.document_id.clone()).collect();

    // The corpus each document is from, as the database has it now. Read before the
    // embedding is paid for, so a database that cannot answer fails the batch while nothing
    // has been spent; and read here rather than taken from the event, which can be minutes
    // old and which a caller could get wrong: the stamp on the point is the document's.
    let sources = document_sources(pool, &items).await?;

    // 2. Embed
    //
    // D18: this arm deliberately does NOT write `embedding_status = 'failed'`.
    // It used to, on the very first error — but JetStream still redelivers this
    // message up to `max_delivery_attempts`, so the row recorded a *terminal*
    // failure while retries were genuinely still in flight. A transient
    // model-plane blip therefore looked permanent to every reader
    // (`sparse.rs` excludes `'failed'`, so the content silently left the
    // lexical arm), and nothing ever corrected it back. Terminal marking now
    // belongs to `stream::run_consumer`, which is the only layer that knows
    // the delivery count and can tell "attempt 1 of 5" from "exhausted".
    //
    // The requests that returned are recorded whether or not the batch then succeeds: a
    // failure after them (or of a later group) redelivers the batch and pays for those
    // texts again, and that second payment is a second event.
    let mut embed_calls = Vec::new();
    let embedded = embed_items_by_org(&items, provider, &mut embed_calls).await;
    if let Some(ledger) = ledger {
        let events = usage::embed_events(
            &embed_calls,
            provider.model_name(),
            provider.provider_name(),
        );
        usage_lost += usage::publish(ledger, &events).await;
    }
    let vectors = match embedded {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(err = %e, "embedding batch failed; unit left retryable");
            return Err(e);
        }
    };

    // 3. Upsert to Qdrant
    let points = build_points(&items, vectors, &sources);
    let unstamped = points.iter().filter(|point| point.source.is_none()).count();
    if unstamped > 0 {
        // Content-free on purpose: a count. Such a point is written without a `source` and
        // a corpus-scoped search treats it as outside every corpus; `qdrant-backfill-source`
        // stamps it once its document can be found.
        tracing::warn!(
            points = unstamped,
            "points written without a source: their document was not found, or has a blank source"
        );
    }

    qdrant.upsert(collection, points).await?;

    // 4. Mark done in Postgres: the last write of every unit in this batch, with what
    //    the vector was built from. The hash is of the stored context exactly as
    //    stored, which is the string the coverage view compares it with.
    mark_units_done(pool, &items, provider.model_name(), pass).await?;

    // 5. Check if documents fully indexed. This is the keyword index's sync and it
    //    comes after the contexts are in the database: the adapter re-indexes each
    //    unit with the context its vector was built from.
    let indexed_docs = check_documents_indexed(pool, &items).await?;
    for doc in &indexed_docs {
        let idempotency_key = make_idempotency_key("doc.indexed", &doc.document_id, &doc.org_id);
        let event = serde_json::json!({
            "document_id": doc.document_id,
            "org_id": doc.org_id,
            "title": doc.title,
            "embedding_model": provider.model_name(),
            "embedding_provider": provider.provider_name(),
            // Phase 3 freshness: the moment this doc became retrievable. The
            // gateway/UI use this to flip an Indexing→Ready signal honestly.
            "embedded_at": chrono::Utc::now().to_rfc3339(),
            "idempotency_key": idempotency_key,
            "user_id": items.iter().find(|item| item.document_id == doc.document_id).and_then(|item| item.user_id.as_deref()),
            "zdr": items.iter().find(|item| item.document_id == doc.document_id).is_some_and(|item| item.zdr),
        });
        let event_item = items
            .iter()
            .find(|item| item.document_id == doc.document_id)
            .context("indexed document missing source event authority")?;
        let event_payload = encode_outbound_event(
            event_signer,
            SUBJECT_DOC_INDEXED,
            &doc.org_id,
            event_item.user_id.as_deref(),
            event_item.zdr,
            &event,
        )?;
        if let Err(error) = nats.publish(SUBJECT_DOC_INDEXED, event_payload).await {
            tracing::warn!(%error, document_id = %doc.document_id, "documents.indexed was not published");
        }
    }

    // 6. The version 1 cost event, while the usage ledger is off. With the ledger on, the
    //    embedding requests were recorded where they were made (above) and this event would
    //    book the same embeddings a second time.
    //
    //    One event, and one idempotency key, per (organization, user, zero-retention) group.
    //    The key used to be one value for the whole batch while the consumer stores it behind
    //    the organization only, so a second user of an organization in the same batch was
    //    silently dropped as a duplicate (see `usage::v1_cost_events`).
    //
    //    The key is a function of the group's units, so that the broker's redelivery of a batch is
    //    the same event. A reconciler batch is never a redelivery: it embeds units that were
    //    embedded before (a repaired vector, a verified legacy context, a context generated for a
    //    unit that was embedded raw), which is a second payment for the same units, and the
    //    consumer drops a key it has already stored (`ON CONFLICT DO NOTHING`, behind the
    //    organization). So every batch that is not the stream's gets a key of its own.
    if ledger.is_none() {
        let reembed_run = (pass != PassKind::Inline).then(|| uuid::Uuid::new_v4().to_string());
        for mut event in usage::v1_cost_events(
            &items,
            provider.model_name(),
            provider.provider_name(),
            |item| embedded_text(item).len(),
        ) {
            if let Some(run) = &reembed_run {
                event.idempotency_key =
                    make_idempotency_key("embed.cost.reembed", &event.idempotency_key, run);
                event.payload["idempotency_key"] =
                    serde_json::Value::from(event.idempotency_key.clone());
            }
            let payload = encode_outbound_event(
                event_signer,
                SUBJECT_COST_LEDGER,
                &event.org_id,
                event.user_id.as_deref(),
                event.zdr,
                &event.payload,
            )?;
            if let Err(error) = nats.publish(SUBJECT_COST_LEDGER, payload).await {
                tracing::warn!(%error, "cost ledger event was not published");
            }
        }
    }

    for document in &report.metered {
        tracing::debug!(
            org_id = %document.org_id,
            document_id = %document.document_id,
            provider_calls = document.calls.len(),
            "contextual retrieval: provider calls made for a document in this batch"
        );
    }
    tracing::info!(
        count = items.len(),
        documents = ?doc_ids.iter().collect::<std::collections::HashSet<_>>(),
        dropped = report.dropped,
        contexts_kept = report.kept,
        contexts_generated = report.generated,
        contexts_lost_race = report.lost_race,
        contexts_left = report.left,
        contexts_budget_stopped = report.budget_stopped,
        contexts_budget_deferred = report.budget_deferred,
        context_outcomes = %render(&report.by_status),
        provider_calls = report.provider_calls(),
        input_tokens = report.billed_tokens().0,
        output_tokens = report.billed_tokens().1,
        cache_read_input_tokens = report.billed_tokens().2,
        usage_events_lost = usage_lost,
        elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        "batch embedded"
    );

    Ok(())
}

/// `name=count,name=count`, for one log line. Content-free by construction: the names are
/// the closed status vocabulary.
fn render(counts: &std::collections::BTreeMap<&'static str, usize>) -> String {
    counts
        .iter()
        .map(|(name, count)| format!("{name}={count}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn encode_outbound_event(
    signer: Option<&EventSigner>,
    subject: &str,
    org_id: &str,
    user_id: Option<&str>,
    zdr: bool,
    value: &serde_json::Value,
) -> anyhow::Result<Vec<u8>> {
    let raw = serde_json::to_vec(value)?;
    match signer {
        Some(signer) => Ok(signer.sign(subject, org_id, user_id, zdr, &raw)?),
        None => Ok(raw),
    }
}

/// The exact string handed to the embedding provider for one unit.
///
/// A single function so the embed call and the cost estimate cannot disagree
/// about what was embedded — the bug that would otherwise appear here is billing
/// the bare chunk while embedding the longer contextualized string.
fn embedded_text(item: &BatchItem) -> String {
    match item.chunk_context.as_deref() {
        Some(context) => crate::context::prompt::compose_contextualized(context, &item.text),
        None => item.text.clone(),
    }
}

async fn embed_items_by_org(
    items: &[BatchItem],
    provider: &dyn Embedder,
    spent: &mut Vec<usage::EmbedCall>,
) -> anyhow::Result<Vec<Vec<f32>>> {
    // Group by (org_id, zdr) so a restricted-doc batch carries the ZDR signal
    // distinctly from a non-restricted batch for the same org — the embed
    // egress guard then fires only for the restricted group.
    //
    // Contextual Retrieval: the EMBEDDED text is the chunk with its situating
    // context prepended, composed here rather than stored composed, from the
    // context the database holds. `item.text` remains what is persisted and
    // shown — only the vector reflects the added context.
    let mut groups: HashMap<(&str, bool), Vec<(usize, String)>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        let embed_text = embedded_text(item);
        groups
            .entry((item.org_id.as_str(), item.zdr))
            .or_default()
            .push((index, embed_text));
    }

    let mut vectors_by_index: Vec<Option<Vec<f32>>> = vec![None; items.len()];
    for ((org_id, zdr), group) in groups {
        let texts: Vec<String> = group.iter().map(|(_, text)| text.clone()).collect();
        let vectors = provider.embed_batch(org_id, &texts, zdr).await?;
        // The request returned, so it was paid for: recorded before its answer is checked.
        // The estimate is the length of what was sent, as it has always been.
        spent.push(usage::EmbedCall {
            org_id: org_id.to_owned(),
            zdr,
            call_id: uuid::Uuid::new_v4().to_string(),
            texts: group.len(),
            estimated_tokens: texts.iter().map(|text| text.len() as u64 / 4).sum(),
        });
        if vectors.len() != group.len() {
            anyhow::bail!(
                "embedding provider returned {} vectors for {} texts",
                vectors.len(),
                group.len()
            );
        }
        for ((index, _), vector) in group.into_iter().zip(vectors) {
            vectors_by_index[index] = Some(vector);
        }
    }

    vectors_by_index
        .into_iter()
        .map(|vector| vector.context("missing embedding vector"))
        .collect()
}

/// `documents.source` of the documents the batch's units belong to, keyed by
/// `(document_id, org_id)`. A document the database does not have under that organisation is
/// not in the map.
async fn document_sources(
    pool: &PgPool,
    items: &[BatchItem],
) -> anyhow::Result<HashMap<(String, String), String>> {
    let pairs: Vec<(&str, &str)> = items
        .iter()
        .map(|item| (item.document_id.as_str(), item.org_id.as_str()))
        .collect();
    let mut conn = pool
        .acquire()
        .await
        .context("acquire a connection to read the documents' sources")?;
    crate::source_stamp::load(&mut conn, &pairs)
        .await
        .context("read the documents' sources")
}

/// The points of a batch as they are stored: what the retrieval engine reads, plus the
/// `source` of each unit's document so a corpus-scoped search can be answered from the
/// vector store alone.
///
/// `text` is the document's own words (never the composition with the context), the
/// organisation is the unit's, and the source is looked up under that same organisation: a
/// document id that exists only under another organisation yields no stamp.
fn build_points(
    items: &[BatchItem],
    vectors: Vec<Vec<f32>>,
    sources: &HashMap<(String, String), String>,
) -> Vec<EmbeddingPoint> {
    items
        .iter()
        .zip(vectors)
        .map(|(item, vec)| {
            let mut metadata = HashMap::new();
            // P2-3: threaded through as a plain RFC3339 string, matching every
            // other value in this map -- qdrant_writer's generic passthrough
            // takes String, not a typed timestamp.
            if let Some(date) = item.document_date {
                metadata.insert("document_date".to_string(), date.to_rfc3339());
            }
            EmbeddingPoint {
                knowledge_id: item.knowledge_id.clone(),
                document_id: item.document_id.clone(),
                org_id: item.org_id.clone(),
                chunk_index: item.chunk_index,
                text: item.text.clone(),
                vector: vec,
                // A blank source is no stamp (the writer would drop it): say so here, so the
                // count of unstamped points in the batch's log line is true.
                source: sources
                    .get(&(item.document_id.clone(), item.org_id.clone()))
                    .filter(|source| crate::source_stamp::stampable(source).is_some())
                    .cloned(),
                metadata,
            }
        })
        .collect()
}

/// Mark the batch's units embedded and record what each vector was built from.
///
/// The hash is of the context exactly as the database holds it (`stored_context_hash`),
/// the string the coverage view compares `embedded_context_hash` with, and `NULL` for a
/// unit embedded raw. Keyed by knowledge id AND organization: the batch spans
/// organizations on a connection where row-level security is inert, so the predicate is the
/// only tenant check there is.
async fn mark_units_done(
    pool: &PgPool,
    items: &[BatchItem],
    embedding_model: &str,
    pass: PassKind,
) -> anyhow::Result<()> {
    let done: Vec<store::Done<'_>> = items
        .iter()
        .map(|item| store::Done {
            unit: store::UnitRef {
                knowledge_id: &item.knowledge_id,
                org_id: &item.org_id,
            },
            context_hash: item.chunk_context.as_deref().map(stored_context_hash),
        })
        .collect();
    let mut conn = pool
        .acquire()
        .await
        .context("acquire a connection to mark the units done")?;
    store::mark_done_with(
        &mut conn,
        &done,
        embedding_model,
        pass == PassKind::VerifyLegacy,
    )
    .await
    .context("mark the units done")?;
    Ok(())
}

/// Write the terminal `embedding_status = 'failed'`.
///
/// D18: `pub(crate)` and called from **one** place — `stream::run_consumer`'s
/// dead-letter arm, immediately before the DLQ publish and the ack. Those
/// three actions are one decision ("this delivery is over"), and keeping them
/// adjacent is what stops the database from claiming a terminal outcome that
/// the broker has not reached yet. Do not call this from the batch pipeline.
pub(crate) async fn mark_units_failed(
    pool: &PgPool,
    knowledge_ids: &[String],
    error: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE knowledge_units SET embedding_status = 'failed', error_message = $2 WHERE knowledge_id = ANY($1)",
    )
    .bind(knowledge_ids)
    .bind(error)
    .execute(pool)
    .await?;
    Ok(())
}

/// Whether a redelivery has exhausted its budget and the outcome is terminal.
///
/// D18: extracted so the "is this actually the last attempt?" rule is one
/// named, unit-tested predicate instead of an inline comparison duplicated
/// next to every DLQ publish. `delivered` is JetStream's 1-based count of
/// deliveries *including* the current one, so attempt N of N is terminal.
pub(crate) fn delivery_is_terminal(delivered: u32, max_delivery_attempts: u32) -> bool {
    // A misconfigured zero must not mean "never retry, fail immediately" — that
    // would restore the exact D18 behaviour by accident.
    let budget = max_delivery_attempts.max(1);
    delivered >= budget
}

/// Which of the batch's documents have no unit left to embed, marked `indexed`.
/// See [`store::indexed_documents`] for why an already-indexed document is
/// announced again.
async fn check_documents_indexed(
    pool: &PgPool,
    items: &[BatchItem],
) -> anyhow::Result<Vec<store::Indexed>> {
    let pairs: Vec<(&str, &str)> = items
        .iter()
        .map(|item| (item.document_id.as_str(), item.org_id.as_str()))
        .collect();
    let mut conn = pool
        .acquire()
        .await
        .context("acquire a connection to check the documents")?;
    Ok(store::indexed_documents(&mut conn, &pairs).await?)
}

fn make_idempotency_key(prefix: &str, a: &str, b: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    prefix.hash(&mut hasher);
    a.hash(&mut hasher);
    b.hash(&mut hasher);
    let hash = hasher.finish();
    let mut out = String::with_capacity(prefix.len() + 17);
    out.push_str(prefix);
    out.push('-');
    let _ = write!(out, "{hash:016x}");
    out
}

#[cfg(test)]
mod contextual_retrieval_tests {
    use super::*;

    fn item() -> BatchItem {
        BatchItem {
            knowledge_id: "kid-1".to_string(),
            document_id: "doc-1".to_string(),
            org_id: "org-1".to_string(),
            chunk_index: 0,
            text: "Margin improved to 31%.".to_string(),
            zdr: false,
            user_id: None,
            document_date: None,
            chunk_context: None,
        }
    }

    /// Contextualization must change what is EMBEDDED without changing what is
    /// stored or shown — a citation has to render the document's own words.
    #[test]
    fn the_embedded_text_is_contextualized_but_the_stored_text_is_not() {
        let mut unit = item();
        let original = unit.text.clone();
        unit.chunk_context = Some("From ACME's Q2 report.".to_string());

        // What the embedder sees — the same helper `embed_items_by_org` uses.
        let embedded = embedded_text(&unit);
        assert!(embedded.starts_with("From ACME's Q2 report."));
        assert!(embedded.ends_with(&original));

        // What gets stored in the Qdrant payload and returned to callers.
        assert_eq!(unit.text, original, "the chunk text must be untouched");
    }

    /// With no generated context, the embedded text must be byte-identical to
    /// the chunk — the feature being off cannot perturb existing behaviour.
    #[test]
    fn without_a_generated_context_the_embedded_text_is_the_raw_chunk() {
        let unit = item();
        assert_eq!(embedded_text(&unit), unit.text);
    }

    /// Billing follows the embedded text, not the chunk: a contextualized unit
    /// genuinely costs more to embed, and the ledger must say so.
    #[test]
    fn the_cost_estimate_counts_the_contextualized_length() {
        let raw = item();
        let mut contextualized = item();
        contextualized.chunk_context =
            Some("A much longer situating preamble naming the report and period.".to_string());
        let estimate = |unit: &BatchItem| embedded_text(unit).len() / 4;
        assert!(
            estimate(&contextualized) > estimate(&raw),
            "a contextualized unit must not be billed as if it were the bare chunk"
        );
    }
}

#[cfg(test)]
mod source_payload_tests {
    use super::*;

    fn unit(knowledge_id: &str, document_id: &str, org_id: &str, chunk_index: i32) -> BatchItem {
        BatchItem {
            knowledge_id: knowledge_id.to_string(),
            document_id: document_id.to_string(),
            org_id: org_id.to_string(),
            chunk_index,
            text: format!("chunk {chunk_index} of {document_id}"),
            zdr: false,
            user_id: None,
            document_date: None,
            chunk_context: Some("A context that is never stored in the payload.".to_string()),
        }
    }

    fn sources(entries: &[(&str, &str, &str)]) -> HashMap<(String, String), String> {
        entries
            .iter()
            .map(|(document_id, org_id, source)| {
                (
                    ((*document_id).to_string(), (*org_id).to_string()),
                    (*source).to_string(),
                )
            })
            .collect()
    }

    fn vectors(count: usize) -> Vec<Vec<f32>> {
        (0..count).map(|index| vec![index as f32]).collect()
    }

    /// The point of a unit says which kind of origin its document has, so the dense arm can
    /// be scoped to a corpus.
    #[test]
    fn a_point_carries_the_source_of_its_document() {
        let items = vec![
            unit("kid-1", "doc-q", "org-1", 0),
            unit("kid-2", "doc-q", "org-1", 1),
            unit("kid-3", "doc-s", "org-1", 0),
        ];
        let points = build_points(
            &items,
            vectors(3),
            &sources(&[
                ("doc-q", "org-1", "quarry"),
                ("doc-s", "org-1", "sharepoint"),
            ]),
        );

        let found: Vec<(&str, Option<&str>)> = points
            .iter()
            .map(|point| (point.knowledge_id.as_str(), point.source.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                ("kid-1", Some("quarry")),
                ("kid-2", Some("quarry")),
                ("kid-3", Some("sharepoint")),
            ]
        );
    }

    /// The source is looked up under the unit's own organisation. A document id that exists
    /// only under another organisation is not this unit's document: no stamp, never the
    /// other organisation's.
    #[test]
    fn the_source_is_looked_up_under_the_units_own_organisation() {
        let items = vec![
            unit("kid-1", "doc-x", "org-1", 0),
            unit("kid-2", "doc-x", "org-2", 0),
        ];
        let points = build_points(
            &items,
            vectors(2),
            &sources(&[("doc-x", "org-1", "quarry")]),
        );

        assert_eq!(points[0].org_id, "org-1");
        assert_eq!(points[0].source.as_deref(), Some("quarry"));
        assert_eq!(points[1].org_id, "org-2");
        assert_eq!(points[1].source, None);
    }

    /// A unit whose document is not in the database is still stored, without a stamp: a
    /// corpus-scoped search then does not return it, and the backfill stamps it later.
    #[test]
    fn a_unit_whose_document_is_unknown_is_written_without_a_source() {
        let items = vec![unit("kid-1", "doc-gone", "org-1", 0)];
        let points = build_points(&items, vectors(1), &HashMap::new());

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].source, None);
        assert_eq!(points[0].document_id, "doc-gone");
    }

    /// A document whose source is blank has no corpus to stamp: its point carries no source,
    /// and the batch counts it with the unstamped ones (the writer would drop a blank value
    /// either way, so the count must not call it stamped).
    #[test]
    fn a_unit_whose_document_has_a_blank_source_is_written_without_one() {
        let items = vec![
            unit("kid-1", "doc-blank", "org-1", 0),
            unit("kid-2", "doc-space", "org-1", 0),
        ];
        let points = build_points(
            &items,
            vectors(2),
            &sources(&[("doc-blank", "org-1", ""), ("doc-space", "org-1", "  ")]),
        );

        assert_eq!(points[0].source, None);
        assert_eq!(points[1].source, None);
    }

    /// The stamp rides beside the document date and changes nothing else about the point:
    /// the chunk's own words, its tenant and its vector are the unit's.
    #[test]
    fn the_stamp_changes_nothing_else_about_the_point() {
        let mut dated = unit("kid-1", "doc-q", "org-1", 4);
        dated.document_date = Some(
            chrono::DateTime::parse_from_rfc3339("2026-03-14T08:30:00+00:00")
                .expect("a date")
                .with_timezone(&chrono::Utc),
        );
        let items = vec![dated];
        let points = build_points(
            &items,
            vec![vec![0.5, 0.25]],
            &sources(&[("doc-q", "org-1", "quarry")]),
        );

        let point = &points[0];
        assert_eq!(point.source.as_deref(), Some("quarry"));
        assert_eq!(point.org_id, "org-1");
        assert_eq!(point.chunk_index, 4);
        assert_eq!(
            point.text, "chunk 4 of doc-q",
            "the context is not in the payload"
        );
        assert_eq!(point.vector, vec![0.5, 0.25]);
        assert_eq!(
            point.metadata.get("document_date").map(String::as_str),
            Some("2026-03-14T08:30:00+00:00")
        );
        assert!(
            !point.metadata.contains_key("source"),
            "the stamp is its own field, not free-form metadata"
        );
    }
}

#[cfg(test)]
mod terminal_failure_tests {
    use super::*;

    #[test]
    fn a_first_failure_is_never_terminal() {
        // D18 in one assertion: the very first delivery must not be allowed to
        // write `embedding_status = 'failed'`, because JetStream is going to
        // redeliver it four more times.
        assert!(!delivery_is_terminal(1, 5));
        assert!(!delivery_is_terminal(2, 5));
        assert!(!delivery_is_terminal(4, 5));
    }

    #[test]
    fn the_last_attempt_is_terminal() {
        assert!(delivery_is_terminal(5, 5));
        // Defensive: a redelivery that somehow overshoots the budget still
        // terminates rather than looping forever.
        assert!(delivery_is_terminal(6, 5));
    }

    #[test]
    fn an_unknown_delivery_count_is_not_terminal() {
        // `msg.info()` failing yields 0 at the call site. Treating "unknown" as
        // exhausted would mark healthy work permanently failed.
        assert!(!delivery_is_terminal(0, 5));
    }

    #[test]
    fn a_misconfigured_zero_budget_still_allows_one_attempt() {
        assert!(!delivery_is_terminal(0, 0));
        assert!(delivery_is_terminal(1, 0));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn a_later_success_reconciles_a_terminally_failed_unit() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("connect disposable postgres");

        sqlx::raw_sql(
            "DROP TABLE IF EXISTS knowledge_units;
             CREATE TABLE knowledge_units (
                knowledge_id     TEXT PRIMARY KEY,
                org_id           TEXT NOT NULL,
                document_id      TEXT NOT NULL,
                embedding_status TEXT NOT NULL DEFAULT 'pending',
                error_message    TEXT,
                embedded_at      TIMESTAMPTZ,
                embedding_model  TEXT,
                embedded_context_hash TEXT,
                context_lease_until TIMESTAMPTZ
             );
             INSERT INTO knowledge_units (knowledge_id, org_id, document_id)
             VALUES ('kid-1', 'org-a', 'doc-a');",
        )
        .execute(&pool)
        .await
        .expect("seed schema");

        let ids = vec!["kid-1".to_string()];

        // Exhausted delivery marks it terminally failed...
        mark_units_failed(&pool, &ids, "model-plane embedding failed")
            .await
            .expect("mark failed");
        let (status, error, embedded): (String, Option<String>, Option<chrono::DateTime<chrono::Utc>>) =
            sqlx::query_as(
                "SELECT embedding_status, error_message, embedded_at FROM knowledge_units WHERE knowledge_id = 'kid-1'",
            )
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert_eq!(status, "failed");
        assert_eq!(error.as_deref(), Some("model-plane embedding failed"));
        assert!(embedded.is_none());

        // ...and a later successful attempt reconciles it all the way back,
        // clearing the stale error rather than leaving a `done` row that still
        // advertises a failure.
        let unit = BatchItem {
            knowledge_id: "kid-1".to_string(),
            document_id: "doc-a".to_string(),
            org_id: "org-a".to_string(),
            chunk_index: 0,
            text: "text".to_string(),
            zdr: false,
            user_id: None,
            document_date: None,
            chunk_context: None,
        };
        mark_units_done(
            &pool,
            std::slice::from_ref(&unit),
            "test-embed",
            PassKind::Inline,
        )
        .await
        .expect("mark done");
        let (status, error, embedded): (String, Option<String>, Option<chrono::DateTime<chrono::Utc>>) =
            sqlx::query_as(
                "SELECT embedding_status, error_message, embedded_at FROM knowledge_units WHERE knowledge_id = 'kid-1'",
            )
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert_eq!(status, "done");
        assert_eq!(error, None, "stale error survived a successful re-embed");
        assert!(embedded.is_some());

        sqlx::raw_sql("DROP TABLE IF EXISTS knowledge_units;")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn an_already_indexed_document_is_re_announced_not_silently_skipped() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("connect disposable postgres");

        sqlx::raw_sql(
            "DROP TABLE IF EXISTS knowledge_units;
             DROP TABLE IF EXISTS documents;
             CREATE TABLE documents (
                document_id TEXT PRIMARY KEY,
                org_id      TEXT NOT NULL,
                title       TEXT NOT NULL DEFAULT '',
                status      TEXT NOT NULL DEFAULT 'pending'
             );
             CREATE TABLE knowledge_units (
                knowledge_id     TEXT PRIMARY KEY,
                org_id           TEXT NOT NULL,
                document_id      TEXT NOT NULL,
                embedding_status TEXT NOT NULL DEFAULT 'pending'
             );
             INSERT INTO documents (document_id, org_id, title, status)
             VALUES ('doc-reannounce', 'org-a', 'Re-announce test', 'pending');
             INSERT INTO knowledge_units (knowledge_id, org_id, document_id, embedding_status)
             VALUES ('kid-1', 'org-a', 'doc-reannounce', 'done');",
        )
        .execute(&pool)
        .await
        .expect("seed schema");

        let unit = BatchItem {
            knowledge_id: "kid-1".to_string(),
            document_id: "doc-reannounce".to_string(),
            org_id: "org-a".to_string(),
            chunk_index: 0,
            text: "text".to_string(),
            zdr: false,
            user_id: None,
            document_date: None,
            chunk_context: None,
        };
        let items = std::slice::from_ref(&unit);

        // First call: a genuine transition. Must announce.
        let first = check_documents_indexed(&pool, items)
            .await
            .expect("first check");
        assert_eq!(
            first.len(),
            1,
            "genuine pending->indexed transition must announce"
        );
        assert_eq!(first[0].document_id, "doc-reannounce");

        // Second call: the document is already `indexed` and nothing about it
        // changed. This is exactly the re-drive/backfill scenario (a graph-index
        // rebuild, a new search arm's backfill) — it must announce again, not
        // silently return empty. Before this fix, the `WHERE status !=
        // 'indexed'` guard made this structurally impossible: an
        // already-indexed document could never be re-announced by any caller,
        // ever.
        let second = check_documents_indexed(&pool, items)
            .await
            .expect("second check");
        assert_eq!(
            second.len(),
            1,
            "an already-indexed document must still be re-announceable"
        );
        assert_eq!(second[0].document_id, "doc-reannounce");

        sqlx::raw_sql("DROP TABLE IF EXISTS knowledge_units; DROP TABLE IF EXISTS documents;")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
