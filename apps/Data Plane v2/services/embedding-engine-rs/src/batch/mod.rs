use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use anyhow::Context;
use sqlx::PgPool;

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
}

pub async fn process_batch(
    items: &[BatchItem],
    provider: &EmbeddingProvider,
    qdrant: &Qdrant,
    pool: &PgPool,
    collection: &str,
    nats: &async_nats::Client,
    event_signer: Option<&EventSigner>,
) -> anyhow::Result<()> {
    if items.is_empty() {
        return Ok(());
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
        .zip(vectors.into_iter())
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
    for item in items {
        let group = cost_groups
            .entry((item.org_id.as_str(), item.user_id.as_deref(), item.zdr))
            .or_default();
        group.0 += 1;
        group.1 += item.text.len() / 4;
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

async fn embed_items_by_org(
    items: &[BatchItem],
    provider: &EmbeddingProvider,
) -> anyhow::Result<Vec<Vec<f32>>> {
    // Group by (org_id, zdr) so a restricted-doc batch carries the ZDR signal
    // distinctly from a non-restricted batch for the same org — the embed
    // egress guard then fires only for the restricted group.
    let mut groups: HashMap<(&str, bool), Vec<(usize, String)>> = HashMap::new();
    for (index, item) in items.iter().enumerate() {
        groups
            .entry((item.org_id.as_str(), item.zdr))
            .or_default()
            .push((index, item.text.clone()));
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
        for ((index, _), vector) in group.into_iter().zip(vectors.into_iter()) {
            vectors_by_index[index] = Some(vector);
        }
    }

    vectors_by_index
        .into_iter()
        .map(|vector| vector.context("missing embedding vector"))
        .collect()
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
