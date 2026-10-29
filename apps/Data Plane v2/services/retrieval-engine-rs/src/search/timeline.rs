use sqlx::PgPool;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TimelineEntry {
    pub document_id: String,
    pub title: String,
    pub source: String,
    pub created_at: String,
    pub status: String,
    pub chunk_count: i64,
}

const TIMELINE_BASE_SQL: &str =
    "SELECT d.document_id, d.title, d.source, d.created_at::TEXT, d.status,
            (SELECT COUNT(*) FROM knowledge_units ku
             WHERE ku.document_id = d.document_id AND ku.org_id = d.org_id)
     FROM documents d
     WHERE d.org_id = $1 AND d.deleted_at IS NULL
       AND ($2::text IS NULL OR d.owner_id = $2 OR d.visibility = 'org' OR d.document_id = ANY($3))";

fn timeline_base_sql() -> &'static str {
    TIMELINE_BASE_SQL
}

pub async fn temporal_search(
    pool: &PgPool,
    org_id: &str,
    viewer: Option<&str>,
    granted_ids: &[String],
    before: Option<&str>,
    after: Option<&str>,
    limit: i32,
) -> anyhow::Result<Vec<TimelineEntry>> {
    let mut q = String::from(timeline_base_sql());
    let mut next_param = 4;

    // All params bind as text, so cast in SQL: timestamptz for the date bounds
    // and bigint for LIMIT (Postgres rejects a text LIMIT — this was the 500).
    if before.is_some() {
        q.push_str(&format!(" AND d.created_at < ${next_param}::timestamptz"));
        next_param += 1;
    }
    if after.is_some() {
        q.push_str(&format!(" AND d.created_at > ${next_param}::timestamptz"));
        next_param += 1;
    }

    q.push_str(" ORDER BY d.created_at DESC");
    q.push_str(&format!(" LIMIT ${next_param}::bigint"));

    let mut query = sqlx::query_as::<_, (String, String, String, String, String, i64)>(&q)
        .bind(org_id)
        .bind(viewer)
        .bind(granted_ids);
    if let Some(value) = before {
        query = query.bind(value);
    }
    if let Some(value) = after {
        query = query.bind(value);
    }
    query = query.bind(i64::from(limit));

    // Phase 1 RLS: a timeline search serves exactly one org (taken from the
    // verified caller claims), so it reads through an org-scoped transaction.
    // The SQL still binds `org_id` itself — the database policy is a backstop
    // against that filter being dropped or mis-edited later, not a replacement.
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = query.fetch_all(&mut *tx).await?;
    tx.commit().await?;

    Ok(rows
        .into_iter()
        .map(
            |(did, title, source, created, status, chunks)| TimelineEntry {
                document_id: did,
                title,
                source,
                created_at: created,
                status,
                chunk_count: chunks,
            },
        )
        .collect())
}

pub async fn replay_trace(
    pool: &PgPool,
    trace_id: &str,
    org_id: &str,
    actor_user_id: Option<&str>,
) -> anyhow::Result<Option<crate::trace::TraceDetail>> {
    crate::trace::get_trace(pool, trace_id, org_id, actor_user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_query_uses_canonical_visibility_and_tenant_bound_chunk_counts() {
        let sql = timeline_base_sql();

        assert!(sql.contains("d.org_id = $1"));
        assert!(sql.contains("d.deleted_at IS NULL"));
        assert!(sql.contains("d.owner_id = $2"));
        assert!(sql.contains("d.visibility = 'org'"));
        assert!(sql.contains("d.document_id = ANY($3)"));
        assert!(sql.contains("ku.org_id = d.org_id"));
        assert!(!sql.contains("visibility IN ('org', 'shared')"));
    }
}
