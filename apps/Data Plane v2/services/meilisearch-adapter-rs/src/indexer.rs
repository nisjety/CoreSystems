//! Postgres → Meilisearch glue: reads the knowledge-unit rows this service is
//! allowed to surface and writes/removes their Meilisearch projection.
//!
//! Deliberately much smaller than `quickwit-adapter-rs::rebuild`: there is no
//! batch/from-scratch rebuild path here (no admin job queue, no rebuild
//! cursor bookkeeping) — see `stream.rs`'s module docs for why that scope cut
//! is safe for a v1. What remains is exactly the two live-update shapes the
//! keyword arm needs: index one thing, and remove one document's things.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

use crate::meilisearch::MeilisearchClient;
use crate::model::{unix_seconds, KeywordDocument};

#[derive(Clone)]
pub struct IndexerContext {
    pub pool: PgPool,
    pub meilisearch: MeilisearchClient,
}

#[derive(sqlx::FromRow)]
struct KnowledgeRow {
    knowledge_id: String,
    document_id: String,
    org_id: String,
    chunk_index: i32,
    text: String,
    content_hash: Option<String>,
    source: String,
    title: String,
    document_metadata: Value,
    knowledge_updated_at: DateTime<Utc>,
}

/// Same eligibility gate as `quickwit-adapter-rs::rebuild::index_knowledge_unit_by_id`
/// (embedding done, document live, not a restricted ZDR classification) — the
/// keyword arm must never surface a chunk the other lexical/dense arms
/// wouldn't, or "search finds it, retrieve can't" becomes a confusing new
/// failure mode instead of the existing, consistent one.
const KNOWLEDGE_UNIT_SQL: &str = "
    SELECT
        ku.knowledge_id,
        ku.document_id,
        ku.org_id,
        ku.chunk_index,
        ku.text,
        ku.content_hash,
        d.source,
        d.title,
        d.metadata AS document_metadata,
        ku.updated_at AS knowledge_updated_at
    FROM knowledge_units ku
    JOIN documents d ON d.document_id = ku.document_id
    WHERE ku.knowledge_id = $1
      AND ku.org_id = $2
      AND ku.embedding_status = 'done'
      AND d.deleted_at IS NULL
      AND LOWER(BTRIM(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
";

const DOCUMENT_KNOWLEDGE_UNITS_SQL: &str = "
    SELECT
        ku.knowledge_id,
        ku.document_id,
        ku.org_id,
        ku.chunk_index,
        ku.text,
        ku.content_hash,
        d.source,
        d.title,
        d.metadata AS document_metadata,
        ku.updated_at AS knowledge_updated_at
    FROM knowledge_units ku
    JOIN documents d ON d.document_id = ku.document_id
    WHERE ku.document_id = $1
      AND ku.org_id = $2
      AND ku.embedding_status = 'done'
      AND d.deleted_at IS NULL
      AND LOWER(BTRIM(d.zdr_classification)) IN ('internal', 'public', 'sensitive')
    ORDER BY ku.chunk_index
";

fn row_to_document(row: KnowledgeRow) -> KeywordDocument {
    let acl_tags = row
        .document_metadata
        .get("acl_tags")
        .or_else(|| row.document_metadata.get("acl"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    KeywordDocument {
        id: row.knowledge_id.clone(),
        org_id: row.org_id,
        document_id: row.document_id,
        knowledge_id: row.knowledge_id,
        chunk_index: row.chunk_index,
        source: row.source,
        title: row.title,
        body: row.text,
        content_hash: row.content_hash,
        acl_tags,
        updated_at: unix_seconds(row.knowledge_updated_at),
    }
}

/// Indexes exactly one knowledge unit, by id. Returns `false` (not an error)
/// when the row doesn't exist yet or isn't eligible — mirrors
/// `index_knowledge_unit_by_id`'s "not embedded yet" tolerance, since
/// `dataplane.knowledge.units.created` can arrive before `embedding_status`
/// flips to `done`.
pub async fn index_knowledge_unit_by_id(
    ctx: &IndexerContext,
    org_id: &str,
    knowledge_id: &str,
) -> anyhow::Result<bool> {
    // Phase 1 RLS: this path serves exactly one org (taken from the verified
    // event envelope), so it reads through an org-scoped transaction. The SQL
    // still binds `org_id` itself — the database policy is a backstop against
    // that filter being dropped or mis-edited later, not a replacement for it.
    let mut tx = pg_org_scope::begin_org_scoped(&ctx.pool, org_id).await?;
    let row = sqlx::query_as::<_, KnowledgeRow>(KNOWLEDGE_UNIT_SQL)
        .bind(knowledge_id)
        .bind(org_id)
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;

    let Some(row) = row else {
        return Ok(false);
    };
    ctx.meilisearch.upsert(&[row_to_document(row)]).await?;
    Ok(true)
}

/// Re-indexes every knowledge unit for one document. Clears the document's
/// prior entries first so a content update (re-chunk) never leaves stale
/// chunks alongside the new ones — the same clear-before-reindex shape
/// `quickwit-adapter-rs::stream::handle_message` uses for
/// `SUBJECT_DOCUMENT_INDEXED`.
pub async fn index_document_knowledge_units(
    ctx: &IndexerContext,
    org_id: &str,
    document_id: &str,
) -> anyhow::Result<usize> {
    ctx.meilisearch
        .delete_by_document(org_id, document_id)
        .await?;

    // Phase 1 RLS: single-org path, same rationale as
    // `index_knowledge_unit_by_id` above.
    let mut tx = pg_org_scope::begin_org_scoped(&ctx.pool, org_id).await?;
    let rows = sqlx::query_as::<_, KnowledgeRow>(DOCUMENT_KNOWLEDGE_UNITS_SQL)
        .bind(document_id)
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;

    let docs: Vec<_> = rows.into_iter().map(row_to_document).collect();
    let count = docs.len();
    ctx.meilisearch.upsert(&docs).await?;
    Ok(count)
}

/// Purges every keyword-index entry for one document. This is the erasure
/// hook: called from the `dataplane.documents.deleted` handler in
/// `stream.rs`, the same subject `documents-api-go`'s `SoftDeleteWithOutbox`
/// publishes for every ordinary document delete AND for the documents an org
/// erasure cascades through — so this one handler covers both without a
/// second, GDPR-fanout-specific consumer. See `stream.rs`'s module docs for
/// the full reasoning (mirrors `quickwit-adapter-rs::gdpr`'s documented
/// scope boundary).
pub async fn delete_document(
    ctx: &IndexerContext,
    org_id: &str,
    document_id: &str,
) -> anyhow::Result<()> {
    ctx.meilisearch
        .delete_by_document(org_id, document_id)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_unit_queries_are_org_scoped_and_exclude_restricted_or_deleted() {
        for sql in [KNOWLEDGE_UNIT_SQL, DOCUMENT_KNOWLEDGE_UNITS_SQL] {
            assert!(sql.contains("ku.org_id = $2"));
            assert!(sql.contains("d.deleted_at IS NULL"));
            assert!(sql.contains("embedding_status = 'done'"));
            assert!(sql.contains("zdr_classification"));
            assert!(sql.contains("d.document_id = ku.document_id"));
        }
    }

    #[test]
    fn document_scan_orders_by_chunk_index_for_deterministic_reindex() {
        assert!(DOCUMENT_KNOWLEDGE_UNITS_SQL.contains("ORDER BY ku.chunk_index"));
    }

    #[test]
    fn row_to_document_maps_acl_tags_from_either_metadata_key() {
        let base = KnowledgeRow {
            knowledge_id: "kid-1".into(),
            document_id: "doc-1".into(),
            org_id: "org-1".into(),
            chunk_index: 2,
            text: "SKU-1 shipped".into(),
            content_hash: Some("hash1".into()),
            source: "upload".into(),
            title: "Invoice".into(),
            document_metadata: serde_json::json!({ "acl_tags": ["finance", "eu-only"] }),
            knowledge_updated_at: DateTime::parse_from_rfc3339("2026-08-07T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        };
        let doc = row_to_document(base);
        assert_eq!(doc.id, "kid-1");
        assert_eq!(doc.org_id, "org-1");
        assert_eq!(doc.document_id, "doc-1");
        assert_eq!(doc.acl_tags, vec!["finance", "eu-only"]);

        let legacy_key = KnowledgeRow {
            knowledge_id: "kid-2".into(),
            document_id: "doc-2".into(),
            org_id: "org-1".into(),
            chunk_index: 0,
            text: "text".into(),
            content_hash: None,
            source: "upload".into(),
            title: "Doc".into(),
            document_metadata: serde_json::json!({ "acl": ["legacy-tag"] }),
            knowledge_updated_at: Utc::now(),
        };
        assert_eq!(row_to_document(legacy_key).acl_tags, vec!["legacy-tag"]);
    }
}
