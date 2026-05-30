use sqlx::PgPool;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WikiSearchResult {
    pub page_id: String,
    pub title: String,
    pub path: String,
    pub snippet: String,
    pub version_id: Option<String>,
    pub backlinks: Vec<String>,
}

pub async fn wiki_search(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    limit: i32,
) -> anyhow::Result<Vec<WikiSearchResult>> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<String>, Option<String>, Option<serde_json::Value>)>(
        "SELECT p.page_id, p.title, p.path, p.current_version_id,
                (SELECT LEFT(v.content, 300) FROM wiki_page_versions v WHERE v.version_id = p.current_version_id),
                p.backlinks
         FROM wiki_pages p
         WHERE p.org_id = $1
           AND (p.title ILIKE '%' || $2 || '%' OR p.path ILIKE '%' || $2 || '%'
                OR EXISTS (SELECT 1 FROM wiki_page_versions v
                           WHERE v.page_id = p.page_id
                             AND to_tsvector('english', v.content) @@ plainto_tsquery('english', $2)))
         ORDER BY p.updated_at DESC
         LIMIT $3"
    )
    .bind(org_id)
    .bind(query)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(pid, title, path, vid, snippet, bl)| WikiSearchResult {
            page_id: pid,
            title,
            path,
            snippet: snippet.unwrap_or_default(),
            version_id: vid,
            backlinks: bl
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
        })
        .collect())
}
