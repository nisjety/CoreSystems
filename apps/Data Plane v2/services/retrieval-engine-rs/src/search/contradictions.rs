use sqlx::PgPool;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContradictionResult {
    pub claim_id: String,
    pub claim_text: String,
    pub confidence: f64,
    pub contradicted_by: Vec<String>,
    pub entity_ids: Vec<String>,
    pub source_refs: Vec<String>,
}

pub async fn search_contradictions(
    pool: &PgPool,
    org_id: &str,
    query: Option<&str>,
    limit: i32,
) -> anyhow::Result<Vec<ContradictionResult>> {
    let rows = if let Some(q) = query {
        sqlx::query_as::<
            _,
            (
                String,
                String,
                f64,
                serde_json::Value,
                serde_json::Value,
                serde_json::Value,
            ),
        >(
            "SELECT claim_id, claim_text, COALESCE(confidence, 0),
                    COALESCE(contradicted_by_claim_ids, '[]'),
                    COALESCE(entity_ids, '[]'),
                    COALESCE(source_refs, '[]')
             FROM graph_claims
             WHERE org_id = $1
               AND jsonb_array_length(COALESCE(contradicted_by_claim_ids, '[]')) > 0
               AND to_tsvector('english', claim_text) @@ plainto_tsquery('english', $2)
             ORDER BY confidence DESC
             LIMIT $3",
        )
        .bind(org_id)
        .bind(q)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<
            _,
            (
                String,
                String,
                f64,
                serde_json::Value,
                serde_json::Value,
                serde_json::Value,
            ),
        >(
            "SELECT claim_id, claim_text, COALESCE(confidence, 0),
                    COALESCE(contradicted_by_claim_ids, '[]'),
                    COALESCE(entity_ids, '[]'),
                    COALESCE(source_refs, '[]')
             FROM graph_claims
             WHERE org_id = $1
               AND jsonb_array_length(COALESCE(contradicted_by_claim_ids, '[]')) > 0
             ORDER BY confidence DESC
             LIMIT $2",
        )
        .bind(org_id)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    Ok(rows
        .into_iter()
        .map(
            |(cid, text, conf, contra, eids, refs)| ContradictionResult {
                claim_id: cid,
                claim_text: text,
                confidence: conf,
                contradicted_by: serde_json::from_value(contra).unwrap_or_default(),
                entity_ids: serde_json::from_value(eids).unwrap_or_default(),
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            },
        )
        .collect())
}
