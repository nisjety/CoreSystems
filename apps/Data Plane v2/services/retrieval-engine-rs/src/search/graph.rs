use sqlx::PgPool;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphExpansionResult {
    pub entity_id: String,
    pub entity_text: String,
    pub entity_type: String,
    pub confidence: f64,
    pub relationships: Vec<GraphRelationshipHit>,
    pub claims: Vec<GraphClaimHit>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphRelationshipHit {
    pub rel_id: String,
    pub other_entity_id: String,
    pub other_entity_text: String,
    pub relation_type: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GraphClaimHit {
    pub claim_id: String,
    pub claim_text: String,
    pub confidence: f64,
    pub status: String,
}

pub async fn graph_expansion_search(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    max_entities: i32,
) -> anyhow::Result<Vec<GraphExpansionResult>> {
    let entity_rows = sqlx::query_as::<_, (String, String, String, f64)>(
        "SELECT entity_id, entity_text, entity_type, COALESCE(confidence, 0)
         FROM graph_entities
         WHERE org_id = $1 AND to_tsvector('english', entity_text) @@ plainto_tsquery('english', $2)
         ORDER BY ts_rank_cd(to_tsvector('english', entity_text), plainto_tsquery('english', $2)) DESC
         LIMIT $3"
    )
    .bind(org_id)
    .bind(query)
    .bind(max_entities)
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();

    for (eid, etext, etype, econf) in &entity_rows {
        let rels = sqlx::query_as::<_, (String, String, String, String, String, f64)>(
            "SELECT r.rel_id, r.entity_a_id, r.entity_b_id, r.relation_type,
                    CASE WHEN r.entity_a_id = $1 THEN b.entity_text ELSE a.entity_text END,
                    COALESCE(r.confidence, 0)
             FROM graph_relationships r
             JOIN graph_entities a ON r.entity_a_id = a.entity_id
             JOIN graph_entities b ON r.entity_b_id = b.entity_id
             WHERE r.org_id = $2 AND (r.entity_a_id = $1 OR r.entity_b_id = $1)
             LIMIT 20",
        )
        .bind(eid)
        .bind(org_id)
        .fetch_all(pool)
        .await?;

        let relationships: Vec<GraphRelationshipHit> = rels
            .iter()
            .map(|(rid, aid, bid, rtype, other_text, conf)| {
                let other_id = if aid == eid { bid.clone() } else { aid.clone() };
                GraphRelationshipHit {
                    rel_id: rid.clone(),
                    other_entity_id: other_id,
                    other_entity_text: other_text.clone(),
                    relation_type: rtype.clone(),
                    confidence: *conf,
                }
            })
            .collect();

        let claim_rows = sqlx::query_as::<_, (String, String, f64, String)>(
            "SELECT claim_id, claim_text, COALESCE(confidence, 0), COALESCE(claim_status, 'active')
             FROM graph_claims
             WHERE org_id = $1 AND entity_ids @> $2::jsonb
             LIMIT 10",
        )
        .bind(org_id)
        .bind(serde_json::json!([eid]))
        .fetch_all(pool)
        .await?;

        let claims: Vec<GraphClaimHit> = claim_rows
            .iter()
            .map(|(cid, ctext, conf, st)| GraphClaimHit {
                claim_id: cid.clone(),
                claim_text: ctext.clone(),
                confidence: *conf,
                status: st.clone(),
            })
            .collect();

        results.push(GraphExpansionResult {
            entity_id: eid.clone(),
            entity_text: etext.clone(),
            entity_type: etype.clone(),
            confidence: *econf,
            relationships,
            claims,
        });
    }

    Ok(results)
}

pub async fn community_summary_search(
    pool: &PgPool,
    entity_ids: &[String],
    org_id: &str,
) -> anyhow::Result<Vec<CommunitySummary>> {
    if entity_ids.is_empty() {
        return Ok(vec![]);
    }

    let mut summaries = Vec::new();
    for eid in entity_ids {
        let rows = sqlx::query_as::<_, (String, String, serde_json::Value, Option<String>, i32)>(
            "SELECT community_id, org_id, entity_ids, summary, level
             FROM graph_communities
             WHERE org_id = $1 AND entity_ids @> $2::jsonb
             LIMIT 5",
        )
        .bind(org_id)
        .bind(serde_json::json!([eid]))
        .fetch_all(pool)
        .await?;

        for (cid, _oid, eids, summary, level) in rows {
            summaries.push(CommunitySummary {
                community_id: cid,
                entity_ids: serde_json::from_value(eids).unwrap_or_default(),
                summary,
                level,
            });
        }
    }

    summaries.sort_by(|a, b| a.community_id.cmp(&b.community_id));
    summaries.dedup_by(|a, b| a.community_id == b.community_id);
    Ok(summaries)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommunitySummary {
    pub community_id: String,
    pub entity_ids: Vec<String>,
    pub summary: Option<String>,
    pub level: i32,
}
