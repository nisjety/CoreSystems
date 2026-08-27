use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use anyhow::Context;
use sqlx::PgPool;

use crate::provider::contextualize::Contextualizer;
use crate::provider::EmbeddingProvider;
use crate::qdrant_writer::{self, EmbeddingPoint};
use event_envelope_rs::EventSigner;
use qdrant_client::Qdrant;

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
    /// egress to a retaining (direct-Azure) embedding provider.
    pub zdr: bool,
    pub user_id: Option<String>,
    /// P2-3: the owning document's `document_date` (source content's own
    /// last-modified time), sourced from the `documents` row at enqueue time
    /// alongside `zdr`. `None` for the common case of a document with no
    /// connector-supplied date -- the retrieval decay stage treats that as no
    /// penalty, not maximum penalty.
    pub document_date: Option<chrono::DateTime<chrono::Utc>>,
    /// Contextual Retrieval: LLM-generated sentences situating this chunk
    /// inside its document. Populated by [`contextualize_items`] and `None`
    /// whenever the feature is off, the document is restricted (ZDR), or
    /// generation failed.
    ///
    /// The CONTEXT ALONE, not the chunk with the context prepended. The
    /// composed form is a pure function of this plus `text`
    /// (`compose_contextualized`), so materialising it here would just
    /// duplicate the chunk. Storing them apart is also what lets the lexical
    /// arm index the chunk's own words in exactly one field and the context in
    /// another — BM25 sums across fields, so a composed string indexed next to
    /// the chunk would score the chunk's terms twice.
    ///
    /// `text` stays the document's own words and remains what the Qdrant
    /// payload carries and what callers are shown, so a citation never displays
    /// a model's preamble. See `provider::contextualize`.
    pub chunk_context: Option<String>,
}

#[allow(clippy::too_many_arguments)] // one optional collaborator added to an existing wide seam
pub async fn process_batch(
    items: &mut [BatchItem],
    provider: &EmbeddingProvider,
    qdrant: &Qdrant,
    pool: &PgPool,
    collection: &str,
    nats: &async_nats::Client,
    event_signer: Option<&EventSigner>,
    contextualizer: Option<&Contextualizer>,
) -> anyhow::Result<()> {
    if items.is_empty() {
        return Ok(());
    }

    // 0. Contextual Retrieval, before embedding because it changes what gets
    //    embedded. Infallible and best-effort: on any failure the affected item
    //    keeps `chunk_context = None` and its raw chunk is embedded, so an
    //    inference outage degrades retrieval quality without failing an ingest.
    if let Some(contextualizer) = contextualizer {
        contextualize_items(items, contextualizer, pool).await;
    }

    let kid_list: Vec<String> = items.iter().map(|i| i.knowledge_id.clone()).collect();
    let doc_ids: Vec<String> = items.iter().map(|i| i.document_id.clone()).collect();

    // 1. Embed
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
    let vectors = match embed_items_by_org(items, provider).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(err = %e, "embedding batch failed; unit left retryable");
            return Err(e);
        }
    };

    // 2. Upsert to Qdrant
    let points: Vec<EmbeddingPoint> = items
        .iter()
        .zip(vectors)
        .map(|(item, vec)| {
            let mut metadata = HashMap::new();
            // P2-3: threaded through as a plain RFC3339 string, matching every
            // other value in this map -- qdrant_writer's generic passthrough
            // (`for (k, v) in p.metadata { payload.insert(k, StringValue(v)) }`)
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
                metadata,
            }
        })
        .collect();

    qdrant_writer::upsert_vectors(qdrant, collection, points).await?;

    // 3. Mark done in Postgres
    mark_units_done(pool, &kid_list).await?;

    // 3b. Persist the situating context that produced these vectors.
    //
    // This is the LEXICAL half of Contextual Retrieval, not just bookkeeping:
    // `content_tsv` is generated over `text` (weight A) and `chunk_context`
    // (weight B), so writing the column is what makes the sparse arm contextual.
    // One generation feeds both arms.
    //
    // Best-effort and AFTER the vectors are live, because the two halves fail
    // independently: if this write fails the vectors are still contextual and
    // retrieval still works, just with a non-contextual lexical arm for these
    // units. Failing the batch instead would throw away good vectors over a
    // recoverable write.
    //
    // No staleness hazard against the vector: the context and the vector are
    // generated in the same pass from the same document text, so they always
    // agree with each other. A later edit elsewhere in the document makes both
    // equally old — exactly the freshness contract the vector already had.
    persist_chunk_context(pool, items).await;

    // 4. Check if documents fully indexed
    let indexed_docs = check_documents_indexed(pool, &doc_ids).await?;
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
        let _ = nats
            .publish(SUBJECT_DOC_INDEXED, event_payload.into())
            .await;
    }

    // 5. Publish cost ledger event
    let cost_idempotency_key =
        make_idempotency_key("embed.cost", &kid_list.join(","), provider.model_name());
    let mut cost_groups: HashMap<(&str, Option<&str>, bool), (usize, usize)> = HashMap::new();
    for item in items.iter() {
        let group = cost_groups
            .entry((item.org_id.as_str(), item.user_id.as_deref(), item.zdr))
            .or_default();
        group.0 += 1;
        // Bill the text that was actually EMBEDDED, not the raw chunk.
        // Contextual Retrieval makes the embedded string longer than
        // `item.text`, so measuring the chunk would under-report every
        // contextualized unit's embedding cost.
        group.1 += embedded_text(item).len() / 4;
    }
    for ((org_id, user_id, zdr), (count, estimated_tokens)) in cost_groups {
        let cost_event = serde_json::json!({
            "event_type": "embedding", "model": provider.model_name(),
            "provider": provider.provider_name(), "count": count,
            "estimated_tokens": estimated_tokens, "org_id": org_id,
            "user_id": user_id, "zdr": zdr,
            "idempotency_key": cost_idempotency_key,
        });
        let payload = encode_outbound_event(
            event_signer,
            SUBJECT_COST_LEDGER,
            org_id,
            user_id,
            zdr,
            &cost_event,
        )?;
        let _ = nats.publish(SUBJECT_COST_LEDGER, payload.into()).await;
    }

    tracing::info!(
        count = items.len(),
        documents = ?doc_ids.iter().collect::<std::collections::HashSet<_>>(),
        "batch embedded"
    );

    Ok(())
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

/// Contextual Retrieval pass over a mixed-org batch.
///
/// Infallible on purpose — every failure mode resolves to "leave
/// `chunk_context` as `None`", which the embed step already handles by
/// using the raw chunk. Propagating an error here would let a model-provider
/// blip fail an ingest that has a perfectly good non-contextual path.
///
/// ## ZDR
///
/// Restricted (`zdr = true`) items are excluded before anything is read or
/// sent. The exclusion happens at the *selection* step rather than inside the
/// provider, so a restricted document's content is never even loaded for this
/// purpose, let alone put in a prompt.
///
/// ## Why documents are fetched here rather than carried on the item
///
/// One document produces many chunks, so the document text would be duplicated
/// once per chunk if the stream consumer attached it at enqueue time — for a
/// megabyte document with 400 chunks that is hundreds of megabytes of buffer.
/// Fetching once per distinct document at use time keeps it to one copy.
/// The exact string handed to the embedding provider for one unit.
///
/// A single function so the embed call and the cost estimate cannot disagree
/// about what was embedded — the bug that would otherwise appear here is billing
/// the bare chunk while embedding the longer contextualized string.
fn embedded_text(item: &BatchItem) -> String {
    match item.chunk_context.as_deref() {
        Some(context) => {
            crate::provider::contextualize::compose_contextualized(context, &item.text)
        }
        None => item.text.clone(),
    }
}

/// Whether one unit may be contextualized at all.
///
/// A standalone predicate rather than an inline `continue` so the ZDR exclusion
/// is a named, directly-testable rule instead of a condition inside a loop. It
/// is the ONLY place restricted content is filtered out of this path, so it is
/// worth being able to point at.
///
/// * `zdr` — a restricted document's text must never reach a model provider.
///   Excluding here, at selection, means its content is not even read from
///   Postgres for this purpose, let alone placed in a prompt.
/// * empty `document_id` — there is no document to situate the chunk within.
fn contextualization_eligible(item: &BatchItem) -> bool {
    !item.zdr && !item.document_id.is_empty()
}

async fn contextualize_items(
    items: &mut [BatchItem],
    contextualizer: &Contextualizer,
    pool: &PgPool,
) {
    // Group eligible item indices by their owning (document_id, org_id). The
    // org is part of the key, never assumed: `process_batch` deliberately mixes
    // tenants in one buffer, so a document_id alone is not an identity here.
    let mut by_document: HashMap<(String, String), Vec<usize>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        if !contextualization_eligible(item) {
            continue;
        }
        by_document
            .entry((item.document_id.clone(), item.org_id.clone()))
            .or_default()
            .push(index);
    }
    if by_document.is_empty() {
        return;
    }

    let doc_ids: Vec<String> = by_document.keys().map(|(doc, _)| doc.clone()).collect();
    let org_ids: Vec<String> = by_document.keys().map(|(_, org)| org.clone()).collect();

    // Every row is matched on BOTH document_id and org_id via the paired
    // unnest, so this cannot return another tenant's document even though the
    // batch spans tenants and therefore cannot run in one org-scoped
    // transaction (see `stream`'s note on why `process_batch` must not be
    // org-scoped).
    let rows: Vec<(String, String, Option<String>)> = match sqlx::query_as(
        r#"
        SELECT d.document_id, d.org_id, d.content
        FROM documents d
        JOIN unnest($1::text[], $2::text[]) AS t(document_id, org_id)
          ON d.document_id = t.document_id AND d.org_id = t.org_id
        WHERE d.deleted_at IS NULL
        "#,
    )
    .bind(&doc_ids)
    .bind(&org_ids)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(
                error = %e,
                documents = by_document.len(),
                "contextual retrieval: document content lookup failed; embedding raw chunks"
            );
            return;
        }
    };

    let mut contents: HashMap<(String, String), String> = HashMap::new();
    for (document_id, org_id, content) in rows {
        if let Some(content) = content.filter(|c| !c.trim().is_empty()) {
            contents.insert((document_id, org_id), content);
        }
    }

    for ((document_id, org_id), indices) in by_document {
        let Some(document_text) = contents.get(&(document_id.clone(), org_id.clone())) else {
            // Content missing, empty, or the document was deleted between
            // enqueue and now. Nothing to situate the chunk against.
            continue;
        };
        let chunks: Vec<&str> = indices
            .iter()
            .map(|index| items[*index].text.as_str())
            .collect();
        let contexts = contextualizer
            .contextualize_document(&org_id, document_text, &chunks)
            .await;
        if contexts.len() != indices.len() {
            // The provider contract is one entry per chunk; a mismatch would
            // mean attaching one chunk's context to another, so drop the whole
            // document's contexts rather than risk a misalignment.
            tracing::error!(
                document_id = %document_id,
                expected = indices.len(),
                got = contexts.len(),
                "contextual retrieval: misaligned context count; embedding raw chunks"
            );
            continue;
        }
        let mut applied = 0usize;
        for (index, context) in indices.into_iter().zip(contexts) {
            if let Some(context) = context {
                items[index].chunk_context = Some(context);
                applied += 1;
            }
        }
        tracing::debug!(
            document_id = %document_id,
            applied,
            "contextual retrieval: chunks contextualized"
        );
    }
}

async fn embed_items_by_org(
    items: &[BatchItem],
    provider: &EmbeddingProvider,
) -> anyhow::Result<Vec<Vec<f32>>> {
    // Group by (org_id, zdr) so a restricted-doc batch carries the ZDR signal
    // distinctly from a non-restricted batch for the same org — the embed
    // egress guard then fires only for the restricted group.
    //
    // Contextual Retrieval: the EMBEDDED text is the chunk with its situating
    // context prepended, composed here rather than stored composed. `item.text`
    // remains what is persisted and shown — only the vector reflects the
    // added context.
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

/// Store each unit's situating context. No-op when nothing was contextualized,
/// which is the default state of the whole feature.
///
/// This write is what turns on contextual BM25: `knowledge_units.content_tsv` is
/// a generated column over `text` at weight A and `chunk_context` at weight B,
/// so the lexical arm starts matching the context the moment the column lands —
/// no separate index build and no query change. The dense arm got the same
/// context via the embedding a moment earlier, so both halves of the technique
/// derive from this one generation.
async fn persist_chunk_context(pool: &PgPool, items: &[BatchItem]) {
    let mut kids: Vec<String> = Vec::new();
    let mut contexts: Vec<String> = Vec::new();
    for item in items {
        if let Some(context) = item.chunk_context.as_deref() {
            kids.push(item.knowledge_id.clone());
            contexts.push(context.to_string());
        }
    }
    if kids.is_empty() {
        return;
    }
    let prompt_version = crate::provider::contextualize::PROMPT_VERSION;
    // Keyed by knowledge_id only, which is content-derived and globally unique,
    // so this needs no org predicate of its own — matching the sibling
    // `mark_units_done` above and index-engine's `parent_window_text` refresh.
    let result = sqlx::query(
        r#"
        UPDATE knowledge_units AS k
           SET chunk_context = t.chunk_context,
               context_prompt_version = $3
          FROM unnest($1::text[], $2::text[]) AS t(knowledge_id, chunk_context)
         WHERE k.knowledge_id = t.knowledge_id
        "#,
    )
    .bind(&kids)
    .bind(&contexts)
    .bind(prompt_version)
    .execute(pool)
    .await;
    match result {
        Ok(done) => tracing::debug!(
            rows = done.rows_affected(),
            prompt_version,
            "chunk context persisted (dense + lexical arms now both contextual)"
        ),
        Err(e) => tracing::warn!(
            error = %e,
            units = kids.len(),
            // Worth being precise about the split failure: the vectors ARE
            // contextual (that happened at embed time), only the lexical arm
            // misses out until a re-embed rewrites the column.
            "chunk context not persisted; vectors are live and contextual, but the lexical arm stays non-contextual for these units"
        ),
    }
}

async fn mark_units_done(pool: &PgPool, knowledge_ids: &[String]) -> anyhow::Result<()> {
    sqlx::query(
        // Phase 3 freshness: stamp embedded_at when vectors land in Qdrant so
        // consumers can tell "embedded/retrievable" from "ingested/chunked".
        //
        // D18: no status predicate, and `error_message` is cleared. A row that
        // a previous exhausted attempt marked `'failed'` — or that the D19
        // reconciler re-drove — is reconciled all the way back to a clean
        // `'done'` here, so a healed unit cannot keep advertising a stale
        // error to the quality gates and the stale detector.
        "UPDATE knowledge_units \
            SET embedding_status = 'done', embedded_at = NOW(), error_message = NULL \
          WHERE knowledge_id = ANY($1)",
    )
    .bind(knowledge_ids)
    .execute(pool)
    .await?;
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

struct IndexedDoc {
    document_id: String,
    org_id: String,
    title: String,
}

/// Re-announce is deliberate, not just "leaves `indexed` alone": every
/// downstream consumer of `SUBJECT_DOC_INDEXED` is independently idempotent on
/// a repeat announce of the same document —
/// `graph-index-rs::store` upserts entities/relationships/claims keyed on
/// deterministic, content-derived ids (`ON CONFLICT ... DO UPDATE` /
/// `DO NOTHING`), and Meilisearch/Quickwit upsert by document id. Before this
/// fix, an already-`indexed` document could NEVER be re-announced — the
/// `WHERE status != 'indexed'` guard this replaced meant the event fires
/// exactly once, on the original state transition, and nothing (a graph-index
/// backfill, a new Meilisearch arm needing its own backfill, a DLQ replay of
/// `documents.created`) could ever trigger it again. Two real, independent
/// consequences of that: graph extraction has literally never run in this
/// deployment, and the new Meilisearch keyword arm shipped with zero
/// production documents in its index and no way to backfill them.
async fn check_documents_indexed(
    pool: &PgPool,
    doc_ids: &[String],
) -> anyhow::Result<Vec<IndexedDoc>> {
    let mut indexed = Vec::new();
    let unique_ids: std::collections::HashSet<&String> = doc_ids.iter().collect();

    for doc_id in unique_ids {
        let pending: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM knowledge_units WHERE document_id = $1 AND embedding_status != 'done'",
        )
        .bind(doc_id)
        .fetch_one(pool)
        .await?;

        if pending.0 == 0 {
            // `SET status = 'indexed'` is a no-op write when already set — the
            // row is unconditionally re-selected so RETURNING always produces
            // it, which is what makes re-announcing possible at all.
            let row = sqlx::query_as::<_, (String, String, String)>(
                "UPDATE documents SET status = 'indexed' WHERE document_id = $1 RETURNING document_id, org_id, title",
            )
            .bind(doc_id)
            .fetch_optional(pool)
            .await?;

            if let Some((did, oid, title)) = row {
                indexed.push(IndexedDoc {
                    document_id: did,
                    org_id: oid,
                    title,
                });
            }
        }
    }

    Ok(indexed)
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

    fn item(document_id: &str, zdr: bool) -> BatchItem {
        BatchItem {
            knowledge_id: "kid-1".to_string(),
            document_id: document_id.to_string(),
            org_id: "org-1".to_string(),
            chunk_index: 0,
            text: "Margin improved to 31%.".to_string(),
            zdr,
            user_id: None,
            document_date: None,
            chunk_context: None,
        }
    }

    /// The load-bearing guard: a restricted document's text must never be sent
    /// to a model provider, so a ZDR unit is excluded before its document
    /// content is even read.
    #[test]
    fn a_restricted_unit_is_never_contextualized() {
        assert!(!contextualization_eligible(&item("doc-1", true)));
        assert!(contextualization_eligible(&item("doc-1", false)));
    }

    #[test]
    fn a_unit_with_no_owning_document_is_skipped() {
        // Nothing to situate the chunk within, so there is no context to build.
        assert!(!contextualization_eligible(&item("", false)));
        // ZDR still dominates when both conditions apply.
        assert!(!contextualization_eligible(&item("", true)));
    }

    /// Contextualization must change what is EMBEDDED without changing what is
    /// stored or shown — a citation has to render the document's own words.
    #[test]
    fn the_embedded_text_is_contextualized_but_the_stored_text_is_not() {
        let mut unit = item("doc-1", false);
        let original = unit.text.clone();
        unit.chunk_context = Some("From ACME's Q2 report.".to_string());

        // What the embedder sees — the same helper `embed_items_by_org` uses.
        let embedded = embedded_text(&unit);
        assert!(embedded.starts_with("From ACME's Q2 report."));
        assert!(embedded.ends_with(&original));

        // What gets stored in the Qdrant payload and returned to callers.
        assert_eq!(unit.text, original, "the chunk text must be untouched");
    }

    /// With no context generated, the embedded text must be byte-identical to
    /// the chunk — the feature being off cannot perturb existing behaviour.
    #[test]
    fn without_a_generated_context_the_embedded_text_is_the_raw_chunk() {
        let unit = item("doc-1", false);
        assert_eq!(embedded_text(&unit), unit.text);
    }

    /// Billing follows the embedded text, not the chunk: a contextualized unit
    /// genuinely costs more to embed, and the ledger must say so.
    #[test]
    fn the_cost_estimate_counts_the_contextualized_length() {
        let raw = item("doc-1", false);
        let mut contextualized = item("doc-1", false);
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
                embedded_at      TIMESTAMPTZ
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
        mark_units_done(&pool, &ids).await.expect("mark done");
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

        let ids = vec!["doc-reannounce".to_string()];

        // First call: a genuine transition. Must announce.
        let first = check_documents_indexed(&pool, &ids)
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
        let second = check_documents_indexed(&pool, &ids)
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
