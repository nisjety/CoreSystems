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

pub async fn temporal_search(
    pool: &PgPool,
    org_id: &str,
    before: Option<&str>,
    after: Option<&str>,
    limit: i32,
) -> anyhow::Result<Vec<TimelineEntry>> {
    let mut q = String::from(
        "SELECT d.document_id, d.title, d.source, d.created_at::TEXT, d.status,
                (SELECT COUNT(*) FROM knowledge_units ku WHERE ku.document_id = d.document_id)
         FROM documents d
         WHERE d.org_id = $1 AND d.deleted_at IS NULL",
    );
    let mut params: Vec<String> = vec![org_id.to_string()];

    // All params bind as text, so cast in SQL: timestamptz for the date bounds
    // and bigint for LIMIT (Postgres rejects a text LIMIT — this was the 500).
    if let Some(b) = before {
        params.push(b.to_string());
        q.push_str(&format!(" AND d.created_at < ${}::timestamptz", params.len()));
    }
    if let Some(a) = after {
        params.push(a.to_string());
        q.push_str(&format!(" AND d.created_at > ${}::timestamptz", params.len()));
    }

    q.push_str(" ORDER BY d.created_at DESC");
    params.push(limit.to_string());
    q.push_str(&format!(" LIMIT ${}::bigint", params.len()));

    let mut query = sqlx::query_as::<_, (String, String, String, String, String, i64)>(&q);
    for p in &params {
        query = query.bind(p);
    }

    let rows = query.fetch_all(pool).await?;

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
) -> anyhow::Result<Option<crate::trace::TraceDetail>> {
    crate::trace::get_trace(pool, trace_id, "").await
}
