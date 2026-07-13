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

const GRAPH_ENTITY_SQL: &str = "SELECT entity_id, entity_text, entity_type, COALESCE(confidence, 0)
     FROM graph_entities
     WHERE org_id = $1 AND to_tsvector('english', entity_text) @@ plainto_tsquery('english', $2)
       AND ($4::text IS NULL OR (
           jsonb_array_length(COALESCE(graph_entities.source_refs, '[]')) > 0
           AND NOT EXISTS (
               SELECT 1 FROM jsonb_array_elements_text(
                   COALESCE(graph_entities.source_refs, '[]')
               ) AS sr(knowledge_id)
               LEFT JOIN knowledge_units ku
                 ON ku.knowledge_id = sr.knowledge_id AND ku.org_id = graph_entities.org_id
               LEFT JOIN documents d
                 ON d.document_id = ku.document_id AND d.org_id = graph_entities.org_id
               WHERE ku.knowledge_id IS NULL OR d.document_id IS NULL
                  OR d.deleted_at IS NOT NULL
                  OR NOT COALESCE(
                      d.owner_id = $4 OR d.visibility = 'org' OR d.document_id = ANY($5),
                      FALSE
                  )
           )
       ))
     ORDER BY ts_rank_cd(to_tsvector('english', entity_text), plainto_tsquery('english', $2)) DESC
     LIMIT $3";

const GRAPH_RELATIONSHIP_SQL: &str =
    "SELECT r.rel_id, r.entity_a_id, r.entity_b_id, r.relation_type,
            CASE WHEN r.entity_a_id = $1 THEN b.entity_text ELSE a.entity_text END,
            COALESCE(r.confidence, 0)
     FROM graph_relationships r
     JOIN graph_entities a ON r.entity_a_id = a.entity_id AND a.org_id = r.org_id
     JOIN graph_entities b ON r.entity_b_id = b.entity_id AND b.org_id = r.org_id
     WHERE r.org_id = $2 AND (r.entity_a_id = $1 OR r.entity_b_id = $1)
       AND ($3::text IS NULL OR (
           jsonb_array_length(COALESCE(
               CASE WHEN r.entity_a_id = $1 THEN b.source_refs ELSE a.source_refs END,
               '[]'
           )) > 0
           AND NOT EXISTS (
               SELECT 1 FROM jsonb_array_elements_text(COALESCE(
                   CASE WHEN r.entity_a_id = $1 THEN b.source_refs ELSE a.source_refs END,
                   '[]'
               )) AS other_sr(knowledge_id)
               LEFT JOIN knowledge_units other_ku
                 ON other_ku.knowledge_id = other_sr.knowledge_id AND other_ku.org_id = r.org_id
               LEFT JOIN documents other_d
                 ON other_d.document_id = other_ku.document_id AND other_d.org_id = r.org_id
               WHERE other_ku.knowledge_id IS NULL OR other_d.document_id IS NULL
                  OR other_d.deleted_at IS NOT NULL
                  OR NOT COALESCE(
                      other_d.owner_id = $3 OR other_d.visibility = 'org'
                          OR other_d.document_id = ANY($4),
                      FALSE
                  )
           )
           AND jsonb_array_length(COALESCE(r.source_refs, '[]')) > 0
           AND NOT EXISTS (
               SELECT 1 FROM jsonb_array_elements_text(
                   COALESCE(r.source_refs, '[]')
               ) AS rel_sr(knowledge_id)
               LEFT JOIN knowledge_units rel_ku
                 ON rel_ku.knowledge_id = rel_sr.knowledge_id AND rel_ku.org_id = r.org_id
               LEFT JOIN documents rel_d
                 ON rel_d.document_id = rel_ku.document_id AND rel_d.org_id = r.org_id
               WHERE rel_ku.knowledge_id IS NULL OR rel_d.document_id IS NULL
                  OR rel_d.deleted_at IS NOT NULL
                  OR NOT COALESCE(
                      rel_d.owner_id = $3 OR rel_d.visibility = 'org'
                          OR rel_d.document_id = ANY($4),
                      FALSE
                  )
           )
       ))
     LIMIT 20";

const GRAPH_CLAIM_SQL: &str =
    "SELECT claim_id, claim_text, COALESCE(confidence, 0), COALESCE(claim_status, 'active')
     FROM graph_claims
     WHERE org_id = $1 AND entity_ids @> $2::jsonb
       AND ($3::text IS NULL OR (
           jsonb_array_length(COALESCE(graph_claims.source_refs, '[]')) > 0
           AND NOT EXISTS (
               SELECT 1 FROM jsonb_array_elements_text(
                   COALESCE(graph_claims.source_refs, '[]')
               ) AS sr(knowledge_id)
               LEFT JOIN knowledge_units ku
                 ON ku.knowledge_id = sr.knowledge_id AND ku.org_id = graph_claims.org_id
               LEFT JOIN documents d
                 ON d.document_id = ku.document_id AND d.org_id = graph_claims.org_id
               WHERE ku.knowledge_id IS NULL OR d.document_id IS NULL
                  OR d.deleted_at IS NOT NULL
                  OR NOT COALESCE(
                      d.owner_id = $3 OR d.visibility = 'org' OR d.document_id = ANY($4),
                      FALSE
                  )
           )
       ))
     LIMIT 10";

const COMMUNITY_SUMMARY_SQL: &str = "SELECT community_id, org_id, entity_ids, summary, level
     FROM graph_communities
     WHERE org_id = $1 AND entity_ids @> $2::jsonb
       AND jsonb_array_length(COALESCE(entity_ids, '[]')) > 0
       AND NOT EXISTS (
           SELECT 1
           FROM jsonb_array_elements_text(COALESCE(entity_ids, '[]')) AS member(entity_id)
           WHERE NOT (member.entity_id = ANY($3))
       )
     LIMIT 5";

fn graph_entity_sql() -> &'static str {
    GRAPH_ENTITY_SQL
}

fn graph_relationship_sql() -> &'static str {
    GRAPH_RELATIONSHIP_SQL
}

fn graph_claim_sql() -> &'static str {
    GRAPH_CLAIM_SQL
}

fn community_summary_sql() -> &'static str {
    COMMUNITY_SUMMARY_SQL
}

pub async fn graph_expansion_search(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    max_entities: i32,
    // Per-user ownership: when `viewer` is `Some`, an entity/relationship/claim is
    // returned only if at least one of its `source_refs` (knowledge_ids) belongs
    // to a document the viewer can see (owner / org-visible / explicitly granted).
    // `None` → no-op (legacy org-scoped path). Closes the graph-grounding leak so
    // a non-owner never sees graph knowledge extracted from another user's private
    // docs.
    viewer: Option<&str>,
    granted_ids: &[String],
) -> anyhow::Result<Vec<GraphExpansionResult>> {
    let entity_rows = sqlx::query_as::<_, (String, String, String, f64)>(graph_entity_sql())
        .bind(org_id)
        .bind(query)
        .bind(max_entities)
        .bind(viewer)
        .bind(granted_ids)
        .fetch_all(pool)
        .await?;

    let mut results = Vec::new();

    for (eid, etext, etype, econf) in &entity_rows {
        let rels = sqlx::query_as::<_, (String, String, String, String, String, f64)>(
            graph_relationship_sql(),
        )
        .bind(eid)
        .bind(org_id)
        .bind(viewer)
        .bind(granted_ids)
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

        let claim_rows = sqlx::query_as::<_, (String, String, f64, String)>(graph_claim_sql())
            .bind(org_id)
            .bind(serde_json::json!([eid]))
            .bind(viewer)
            .bind(granted_ids)
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
            community_summary_sql(),
        )
        .bind(org_id)
        .bind(serde_json::json!([eid]))
        .bind(entity_ids)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_queries_require_all_sources_and_communities_are_visible_subsets() {
        for sql in [
            graph_entity_sql(),
            graph_relationship_sql(),
            graph_claim_sql(),
        ] {
            assert!(sql.contains("jsonb_array_elements_text"), "{sql}");
            assert!(sql.contains("visibility = 'org'"), "{sql}");
            assert!(sql.contains("NOT EXISTS"), "{sql}");
            assert!(!sql.contains("visibility IN ('org', 'shared')"), "{sql}");
        }

        let community_sql = community_summary_sql();
        assert!(community_sql.contains("jsonb_array_elements_text"));
        assert!(community_sql.contains("NOT (member.entity_id = ANY($3))"));
    }
}
