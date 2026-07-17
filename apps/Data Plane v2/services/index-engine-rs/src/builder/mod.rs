use sqlx::PgPool;

use crate::chunker::{chunk_text, ChunkConfig};
use crate::fingerprint::{content_hash, stable_chunk_id};
use crate::normalizer::normalize;

#[derive(Debug, Clone)]
pub struct DocumentEvent {
    pub document_id: String,
    pub org_id: String,
    pub title: String,
    pub source: String,
    pub doc_type: String,
    pub user_id: Option<String>,
    pub idempotency_key: String,
    pub zdr: bool,
}

#[derive(Debug)]
pub struct BuildResult {
    #[allow(dead_code)] // surfaced via Debug + future API responses
    pub document_id: String,
    #[allow(dead_code)] // surfaced via Debug + future API responses
    pub chunks_created: usize,
    pub knowledge_ids: Vec<String>,
    // Knowledge IDs that existed before this (re)build but no longer do —
    // their vectors must be purged from Qdrant to avoid stale retrieval hits
    // after a content update. Empty on first build.
    #[allow(dead_code)] // persisted atomically to index_deletion_outbox
    pub orphaned_knowledge_ids: Vec<String>,
}

// orphaned_ids returns the old knowledge IDs that are absent from the new set.
// Because knowledge IDs are content-derived (stable_chunk_id), unchanged chunks
// keep their ID across a rebuild; only changed or removed chunks orphan.
fn orphaned_ids(old_kids: &[String], new_kids: &[String]) -> Vec<String> {
    let new_set: std::collections::HashSet<&str> = new_kids.iter().map(String::as_str).collect();
    old_kids
        .iter()
        .filter(|k| !new_set.contains(k.as_str()))
        .cloned()
        .collect()
}

fn canonical_document_is_indexable(deleted: bool, zdr_classification: &str) -> bool {
    if deleted {
        return false;
    }
    matches!(
        zdr_classification.trim().to_ascii_lowercase().as_str(),
        "internal" | "public" | "sensitive"
    )
}

/// The `(content_hash, knowledge_id)` pair assigned to a chunk. Both are pure
/// functions of the document id, the chunk's position, and its text, so a
/// chunk's durable identity never depends on existing database rows. This is
/// what lets the ingest loop INSERT unconditionally instead of issuing a
/// per-chunk existence/dedup SELECT.
fn chunk_identity(document_id: &str, chunk_index: usize, text: &str) -> (String, String) {
    let hash = content_hash(text);
    let kid = stable_chunk_id(document_id, chunk_index, &hash);
    (hash, kid)
}

pub async fn process_document(
    pool: &PgPool,
    event: &DocumentEvent,
    chunk_config: &ChunkConfig,
) -> anyhow::Result<BuildResult> {
    if event.zdr {
        anyhow::bail!("restrictive-ZDR document cannot enter durable indexing");
    }
    let mut tx = pool.begin().await?;

    // The event is only a notification. Re-authorize the current canonical row
    // while holding its row lock, then keep every chunk/outbox read and write in
    // this transaction. A delayed event therefore cannot resurrect content after
    // a concurrent delete or restrictive-ZDR transition.
    let canonical: Option<(String, bool, String)> = sqlx::query_as(
        r#"
        SELECT content, deleted_at IS NOT NULL, zdr_classification
        FROM documents
        WHERE document_id = $1 AND org_id = $2
        FOR UPDATE
        "#,
    )
    .bind(&event.document_id)
    .bind(&event.org_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some((content, deleted, zdr_classification)) = canonical else {
        tracing::warn!(document_id = %event.document_id, "canonical document missing; stale indexing event ignored");
        tx.rollback().await?;
        return Ok(BuildResult {
            document_id: event.document_id.clone(),
            chunks_created: 0,
            knowledge_ids: vec![],
            orphaned_knowledge_ids: vec![],
        });
    };
    if !canonical_document_is_indexable(deleted, &zdr_classification) {
        tracing::info!(document_id = %event.document_id, "deleted or restrictive canonical document; stale indexing event ignored");
        tx.rollback().await?;
        return Ok(BuildResult {
            document_id: event.document_id.clone(),
            chunks_created: 0,
            knowledge_ids: vec![],
            orphaned_knowledge_ids: vec![],
        });
    }

    let normalized = normalize(&content);
    let chunks = chunk_text(&normalized, chunk_config);

    // Capture old chunk IDs up front so an update that produces zero chunks
    // (e.g. content cleared) still purges the prior vectors.
    let old_kids: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT knowledge_id, chunk_index, content_hash FROM knowledge_units WHERE document_id = $1 AND org_id = $2 FOR UPDATE",
    )
    .bind(&event.document_id)
    .bind(&event.org_id)
    .fetch_all(&mut *tx)
    .await
    .unwrap_or_default();
    let old_kid_ids: Vec<String> = old_kids.iter().map(|(kid, _, _)| kid.clone()).collect();

    if chunks.is_empty() {
        tracing::warn!(document_id = %event.document_id, "no chunks produced");
        if !old_kids.is_empty() {
            crate::outbox::enqueue_intent(
                &mut tx,
                &event.org_id,
                &event.document_id,
                &old_kid_ids,
                event.user_id.as_deref(),
                &event.idempotency_key,
                event.zdr,
            )
            .await?;
            sqlx::query("DELETE FROM knowledge_units WHERE document_id = $1 AND org_id = $2")
                .bind(&event.document_id)
                .bind(&event.org_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        return Ok(BuildResult {
            document_id: event.document_id.clone(),
            chunks_created: 0,
            knowledge_ids: vec![],
            // Every prior chunk is now orphaned (document has no content).
            orphaned_knowledge_ids: old_kid_ids,
        });
    }

    let reindex = !old_kids.is_empty();

    sqlx::query("DELETE FROM knowledge_units WHERE document_id = $1 AND org_id = $2")
        .bind(&event.document_id)
        .bind(&event.org_id)
        .execute(&mut *tx)
        .await?;

    let mut knowledge_ids = Vec::with_capacity(chunks.len());

    for chunk in &chunks {
        // knowledge_id is a pure function of (document_id, chunk_index,
        // content_hash); no database state is consulted here. Two per-chunk
        // SELECTs used to run at this point — an existence COUNT(*) on
        // knowledge_id, and a near-duplicate ("Phase 5 cost graft") lookup for a
        // prior 'done' unit at the same (document_id, chunk_index). Both were
        // unreachable at runtime: the unconditional
        // `DELETE FROM knowledge_units WHERE document_id = $1 AND org_id = $2`
        // above runs in THIS transaction, so each query could only observe the
        // post-delete state — the COUNT was always 0, and the 'done' lookup
        // always returned no rows (rows re-inserted below are 'pending'). Every
        // chunk therefore always fell through to the INSERT. Computing the
        // identity in-memory preserves that outcome exactly while removing two
        // round-trips per chunk.
        let (hash, kid) = chunk_identity(&event.document_id, chunk.index, &chunk.text);

        let metadata = serde_json::json!({
            "title": event.title,
            "source": event.source,
            "type": event.doc_type,
            "chunk_tokens": chunk.estimated_tokens,
        });

        sqlx::query(
            r#"
            INSERT INTO knowledge_units (
                knowledge_id, document_id, org_id, chunk_index, text,
                embedding_status, content_hash, chunk_version, metadata
            ) VALUES ($1, $2, $3, $4, $5, 'pending', $6, '1', $7)
            ON CONFLICT (knowledge_id) DO NOTHING
            "#,
        )
        .bind(&kid)
        .bind(&event.document_id)
        .bind(&event.org_id)
        .bind(chunk.index as i32)
        .bind(&chunk.text)
        .bind(&hash)
        .bind(&metadata)
        .execute(&mut *tx)
        .await?;

        knowledge_ids.push(kid);
    }

    // Persist chunk lineage on reindex
    if reindex {
        for (old_kid, old_idx, old_hash) in &old_kids {
            let new_kid = knowledge_ids
                .get(*old_idx as usize)
                .unwrap_or(&knowledge_ids[0]);
            sqlx::query(
                r#"
                INSERT INTO chunk_lineage (
                    document_id, old_knowledge_id, new_knowledge_id,
                    old_chunk_index, old_content_hash, reason
                ) VALUES ($1, $2, $3, $4, $5, 'reindex')
                ON CONFLICT DO NOTHING
                "#,
            )
            .bind(&event.document_id)
            .bind(old_kid)
            .bind(new_kid)
            .bind(old_idx)
            .bind(old_hash)
            .execute(&mut *tx)
            .await?;
        }
        tracing::info!(
            document_id = %event.document_id,
            old_chunks = old_kids.len(),
            new_chunks = knowledge_ids.len(),
            "chunk lineage recorded"
        );
    }

    // Update document status to processing
    sqlx::query(
        "UPDATE documents SET status = 'processing' WHERE document_id = $1 AND org_id = $2",
    )
    .bind(&event.document_id)
    .bind(&event.org_id)
    .execute(&mut *tx)
    .await?;

    tracing::info!(
        document_id = %event.document_id,
        chunks = chunks.len(),
        "knowledge units built"
    );

    let orphaned_knowledge_ids = orphaned_ids(&old_kid_ids, &knowledge_ids);
    if !orphaned_knowledge_ids.is_empty() {
        crate::outbox::enqueue_intent(
            &mut tx,
            &event.org_id,
            &event.document_id,
            &orphaned_knowledge_ids,
            event.user_id.as_deref(),
            &event.idempotency_key,
            event.zdr,
        )
        .await?;
    }
    tx.commit().await?;

    Ok(BuildResult {
        document_id: event.document_id.clone(),
        chunks_created: chunks.len(),
        knowledge_ids,
        orphaned_knowledge_ids,
    })
}

// Non-test helper items intentionally follow this module.
#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::{
        canonical_document_is_indexable, chunk_identity, orphaned_ids, process_document,
        DocumentEvent,
    };
    use crate::chunker::ChunkConfig;

    #[test]
    fn canonical_document_gate_fails_closed_for_deleted_restricted_and_unknown_rows() {
        for classification in ["internal", "public", "sensitive"] {
            assert!(canonical_document_is_indexable(false, classification));
        }
        for classification in ["restricted", "", "future-policy", " RESTRICTED "] {
            assert!(!canonical_document_is_indexable(false, classification));
        }
        assert!(!canonical_document_is_indexable(true, "internal"));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn delayed_events_for_deleted_or_restricted_documents_do_zero_durable_work() {
        let database_url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must point to disposable PostgreSQL");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("disposable postgres");
        sqlx::raw_sql(
            r#"
            CREATE TABLE documents (
              document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, content TEXT NOT NULL,
              deleted_at TIMESTAMPTZ, zdr_classification TEXT NOT NULL
            );
            CREATE TABLE knowledge_units (
              knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL
            );
            CREATE TABLE index_deletion_outbox (outbox_id BIGSERIAL PRIMARY KEY);
            INSERT INTO documents VALUES
              ('fixture-restricted','fixture-org','canonical restricted content',NULL,'restricted'),
              ('fixture-unknown','fixture-org','canonical unknown content',NULL,'future-policy'),
              ('fixture-deleted','fixture-org','canonical deleted content',NOW(),'internal');
            INSERT INTO knowledge_units VALUES
              ('existing-restricted','fixture-restricted','fixture-org'),
              ('existing-unknown','fixture-unknown','fixture-org'),
              ('existing-deleted','fixture-deleted','fixture-org');
            "#,
        )
        .execute(&pool)
        .await
        .expect("minimal disposable schema");

        for document_id in ["fixture-restricted", "fixture-unknown", "fixture-deleted"] {
            let result = process_document(
                &pool,
                &DocumentEvent {
                    document_id: document_id.into(),
                    org_id: "fixture-org".into(),
                    title: "stale event title".into(),
                    source: "fixture".into(),
                    doc_type: "text".into(),
                    user_id: Some("fixture-user".into()),
                    idempotency_key: format!("stale-{document_id}"),
                    zdr: false,
                },
                &ChunkConfig::default(),
            )
            .await
            .expect("stale event is acknowledged as a no-op");
            assert_eq!(result.chunks_created, 0);
            assert!(result.knowledge_ids.is_empty());
            assert!(result.orphaned_knowledge_ids.is_empty());
        }

        let counts: (i64, i64) = sqlx::query_as(
            "SELECT
               (SELECT COUNT(*) FROM knowledge_units),
               (SELECT COUNT(*) FROM index_deletion_outbox)",
        )
        .fetch_one(&pool)
        .await
        .expect("durable counts");
        assert_eq!(counts, (3, 0));
    }

    #[test]
    fn orphaned_ids_returns_removed_chunks() {
        let old = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let new = vec!["a".to_string(), "c".to_string()]; // b removed/changed
        assert_eq!(orphaned_ids(&old, &new), vec!["b".to_string()]);
    }

    #[test]
    fn orphaned_ids_empty_when_all_retained() {
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "b".to_string(), "d".to_string()];
        assert!(orphaned_ids(&old, &new).is_empty());
    }

    #[test]
    fn orphaned_ids_all_when_none_retained() {
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["x".to_string()];
        assert_eq!(
            orphaned_ids(&old, &new),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn orphaned_ids_empty_on_first_build() {
        let old: Vec<String> = vec![];
        let new = vec!["a".to_string()];
        assert!(orphaned_ids(&old, &new).is_empty());
    }

    #[test]
    fn chunk_identity_is_pure_and_index_scoped() {
        // Deterministic: identical inputs produce an identical (hash, id) pair,
        // so the loop's knowledge_ids are a pure function of the chunks.
        let (h1, k1) = chunk_identity("doc-1", 0, "hello world");
        let (h2, k2) = chunk_identity("doc-1", 0, "hello world");
        assert_eq!((h1.as_str(), k1.as_str()), (h2.as_str(), k2.as_str()));

        // Same text at a different chunk_index yields a different knowledge_id,
        // so distinct chunks in one build never share an id — the removed
        // per-chunk "skip if exists" COUNT(*) could not have deduped within a
        // single build even before the DELETE made it unreachable.
        let (_, k_next_index) = chunk_identity("doc-1", 1, "hello world");
        assert_ne!(k1, k_next_index);

        // Different content yields a different hash and id.
        let (h_diff, k_diff) = chunk_identity("doc-1", 0, "different content");
        assert_ne!(h1, h_diff);
        assert_ne!(k1, k_diff);
    }
}
