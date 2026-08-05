use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use anyhow::Context;
use sqlx::PgPool;

use crate::provider::EmbeddingProvider;
use crate::qdrant_writer::{self, EmbeddingPoint};
use event_envelope_rs::EventSigner;
use qdrant_client::Qdrant;

// §17.3.3 — named subject, lint-checked. See infra/nats/SUBJECTS.md.
const SUBJECT_DOC_INDEXED: &str = "dataplane.documents.indexed";

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
    let vectors = match embed_items_by_org(items, provider).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(err = %e, "embedding batch failed");
            mark_units_failed(pool, &kid_list, &e.to_string()).await?;
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
            "dataplane.cost.ledger",
            org_id,
            user_id,
            zdr,
            &cost_event,
        )?;
        let _ = nats.publish("dataplane.cost.ledger", payload.into()).await;
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
        "UPDATE knowledge_units SET embedding_status = 'done', embedded_at = NOW() WHERE knowledge_id = ANY($1)",
    )
    .bind(knowledge_ids)
    .execute(pool)
    .await?;
    Ok(())
}

async fn mark_units_failed(
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

struct IndexedDoc {
    document_id: String,
    org_id: String,
    title: String,
}

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
            let row = sqlx::query_as::<_, (String, String, String)>(
                "UPDATE documents SET status = 'indexed' WHERE document_id = $1 AND status != 'indexed' RETURNING document_id, org_id, title",
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
