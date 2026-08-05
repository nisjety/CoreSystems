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

const WIKI_SEARCH_SQL: &str = "SELECT p.page_id, p.title, p.path, p.current_version_id,
            LEFT(COALESCE(v.content, ''), 300),
            CASE WHEN $3::text IS NULL THEN p.backlinks ELSE '[]'::jsonb END
     FROM wiki_pages p
     JOIN wiki_page_versions v ON v.version_id = p.current_version_id AND v.page_id = p.page_id
     WHERE p.org_id = $1
       AND p.deleted_at IS NULL
       AND p.page_status = 'published'
       AND (cardinality($4::text[]) = 0 OR p.workspace_id = ANY($4))
       AND (p.title ILIKE '%' || $2 || '%' OR p.path ILIKE '%' || $2 || '%'
            OR to_tsvector('simple', COALESCE(v.content, '')) @@ plainto_tsquery('simple', $2))
       AND ($3::text IS NULL OR (
           (jsonb_array_length(COALESCE(v.source_refs, '[]')) = 0
                AND v.proposed_by_agent IS NULL)
           OR (
               jsonb_array_length(COALESCE(v.source_refs, '[]')) > 0
               AND NOT EXISTS (
                   SELECT 1
                   FROM jsonb_array_elements_text(COALESCE(v.source_refs, '[]')) AS sr(knowledge_id)
                   LEFT JOIN knowledge_units ku
                     ON ku.knowledge_id = sr.knowledge_id AND ku.org_id = p.org_id
                   LEFT JOIN documents d
                     ON d.document_id = ku.document_id AND d.org_id = p.org_id
                   WHERE ku.knowledge_id IS NULL OR d.document_id IS NULL
                      OR d.deleted_at IS NOT NULL
                      OR NOT COALESCE(
                          d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($5),
                          FALSE
                      )
               )
           )
       ))
     ORDER BY p.updated_at DESC
     LIMIT $6";

fn wiki_search_sql() -> &'static str {
    WIKI_SEARCH_SQL
}

pub async fn wiki_search(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    limit: i32,
    viewer: Option<&str>,
    allowed_workspaces: &[String],
    granted_ids: &[String],
) -> anyhow::Result<Vec<WikiSearchResult>> {
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<serde_json::Value>,
        ),
    >(wiki_search_sql())
    .bind(org_id)
    .bind(query)
    .bind(viewer)
    .bind(allowed_workspaces)
    .bind(granted_ids)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_query_enforces_workspace_and_source_visibility() {
        let sql = wiki_search_sql();

        assert!(sql.contains("p.deleted_at IS NULL"));
        assert!(sql.contains("p.page_status = 'published'"));
        assert!(sql.contains("p.workspace_id = ANY($4)"));
        assert!(sql.contains("v.version_id = p.current_version_id"));
        assert!(sql.contains("proposed_by_agent IS NULL"));
        assert!(sql.contains("owner_id = $3"));
        assert!(sql.contains("visibility = 'org'"));
        assert!(sql.contains("document_id = ANY($5)"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(!sql.contains("visibility IN ('org', 'shared')"));
    }
}
