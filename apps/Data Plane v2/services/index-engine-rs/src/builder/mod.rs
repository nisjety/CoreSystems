use sqlx::PgPool;

use crate::chunker::{chunk_text, ChunkConfig};
use crate::fingerprint::{content_hash, stable_chunk_id};
use crate::normalizer::normalize;

#[derive(Debug, Clone)]
pub struct DocumentEvent {
    pub document_id: String,
    pub org_id: String,
    pub content: String,
    pub title: String,
    pub source: String,
    pub doc_type: String,
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

pub async fn process_document(
    pool: &PgPool,
    event: &DocumentEvent,
    chunk_config: &ChunkConfig,
) -> anyhow::Result<BuildResult> {
    // Lifecycle events (documents.created / documents.updated) are notifications
    // and carry no body, so the canonical content lives in Postgres. Fetch it by
    // document_id; fall back to any inline content for publishers that include it.
    let content = if event.content.trim().is_empty() {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT content FROM documents WHERE document_id = $1")
                .bind(&event.document_id)
                .fetch_optional(pool)
                .await?;
        match row {
            Some((c,)) => c,
            None => {
                tracing::warn!(document_id = %event.document_id, "document row not found; no content to index");
                String::new()
            }
        }
    } else {
        event.content.clone()
    };

    let normalized = normalize(&content);
    let chunks = chunk_text(&normalized, chunk_config);

    // Capture old chunk IDs up front so an update that produces zero chunks
    // (e.g. content cleared) still purges the prior vectors.
    let old_kids: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT knowledge_id, chunk_index, content_hash FROM knowledge_units WHERE document_id = $1",
    )
    .bind(&event.document_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    let old_kid_ids: Vec<String> = old_kids.iter().map(|(kid, _, _)| kid.clone()).collect();

    if chunks.is_empty() {
        tracing::warn!(document_id = %event.document_id, "no chunks produced");
        if !old_kids.is_empty() {
            sqlx::query("DELETE FROM knowledge_units WHERE document_id = $1")
                .bind(&event.document_id)
                .execute(pool)
                .await?;
        }
        return Ok(BuildResult {
            document_id: event.document_id.clone(),
            chunks_created: 0,
            knowledge_ids: vec![],
            // Every prior chunk is now orphaned (document has no content).
            orphaned_knowledge_ids: old_kid_ids,
        });
    }

    let reindex = !old_kids.is_empty();

    sqlx::query("DELETE FROM knowledge_units WHERE document_id = $1")
        .bind(&event.document_id)
        .execute(pool)
        .await?;

    let mut knowledge_ids = Vec::with_capacity(chunks.len());

    for chunk in &chunks {
        let hash = content_hash(&chunk.text);
        let kid = stable_chunk_id(&event.document_id, chunk.index, &hash);

        // Check for existing chunk with same hash (dedup across documents)
        let existing = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM knowledge_units WHERE knowledge_id = $1",
        )
        .bind(&kid)
        .fetch_one(pool)
        .await?;

        if existing > 0 {
            tracing::debug!(knowledge_id = %kid, "chunk already exists, skipping");
            knowledge_ids.push(kid);
            continue;
        }

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
        .execute(pool)
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
            .execute(pool)
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
    sqlx::query("UPDATE documents SET status = 'processing' WHERE document_id = $1")
        .bind(&event.document_id)
        .execute(pool)
        .await?;

    tracing::info!(
        document_id = %event.document_id,
        chunks = chunks.len(),
        "knowledge units built"
    );

    let orphaned_knowledge_ids = orphaned_ids(&old_kid_ids, &knowledge_ids);

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
    use super::orphaned_ids;

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
}

pub async fn handle_document_deleted(pool: &PgPool, document_id: &str) -> anyhow::Result<()> {
    let deleted = sqlx::query("DELETE FROM knowledge_units WHERE document_id = $1")
        .bind(document_id)
        .execute(pool)
        .await?;

    tracing::info!(
        document_id,
        rows = deleted.rows_affected(),
        "knowledge units deleted"
    );
    Ok(())
}
