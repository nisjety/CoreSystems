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

const CONTRADICTIONS_SQL: &str = "SELECT gc.claim_id, gc.claim_text, COALESCE(gc.confidence, 0),
            COALESCE(gc.contradicted_by_claim_ids, '[]'),
            COALESCE(gc.entity_ids, '[]'),
            COALESCE(gc.source_refs, '[]')
     FROM graph_claims gc
     WHERE gc.org_id = $1
       AND jsonb_array_length(COALESCE(gc.contradicted_by_claim_ids, '[]')) > 0
       AND ($4::text IS NULL
            OR to_tsvector('simple', gc.claim_text) @@ websearch_to_tsquery('simple', $4))
       AND ($2::text IS NULL OR (
           jsonb_array_length(COALESCE(gc.source_refs, '[]')) > 0
           AND NOT EXISTS (
               SELECT 1
               FROM jsonb_array_elements_text(COALESCE(gc.source_refs, '[]')) AS sr(knowledge_id)
               LEFT JOIN knowledge_units ku
                 ON ku.knowledge_id = sr.knowledge_id AND ku.org_id = gc.org_id
               LEFT JOIN documents d
                 ON d.document_id = ku.document_id AND d.org_id = gc.org_id
               WHERE ku.knowledge_id IS NULL OR d.document_id IS NULL
                  OR d.deleted_at IS NOT NULL
                  OR NOT COALESCE(
                      d.owner_id = $2 OR d.visibility = 'org' OR d.document_id = ANY($3),
                      FALSE
                  )
           )
           AND NOT EXISTS (
               SELECT 1
               FROM jsonb_array_elements_text(
                   COALESCE(gc.contradicted_by_claim_ids, '[]')
               ) AS contradicted(claim_id)
               LEFT JOIN graph_claims other
                 ON other.claim_id = contradicted.claim_id AND other.org_id = gc.org_id
               WHERE other.claim_id IS NULL
                  OR jsonb_array_length(COALESCE(other.source_refs, '[]')) = 0
                  OR EXISTS (
                      SELECT 1
                      FROM jsonb_array_elements_text(
                          COALESCE(other.source_refs, '[]')
                      ) AS other_sr(knowledge_id)
                      LEFT JOIN knowledge_units other_ku
                        ON other_ku.knowledge_id = other_sr.knowledge_id
                       AND other_ku.org_id = gc.org_id
                      LEFT JOIN documents other_d
                        ON other_d.document_id = other_ku.document_id
                       AND other_d.org_id = gc.org_id
                      WHERE other_ku.knowledge_id IS NULL OR other_d.document_id IS NULL
                         OR other_d.deleted_at IS NOT NULL
                         OR NOT COALESCE(
                             other_d.owner_id = $2 OR other_d.visibility = 'org'
                                 OR other_d.document_id = ANY($3),
                             FALSE
                         )
                  )
           )
           AND NOT EXISTS (
               SELECT 1
               FROM jsonb_array_elements_text(COALESCE(gc.entity_ids, '[]')) AS member(entity_id)
               LEFT JOIN graph_entities ge
                 ON ge.entity_id = member.entity_id AND ge.org_id = gc.org_id
               WHERE ge.entity_id IS NULL
                  OR jsonb_array_length(COALESCE(ge.source_refs, '[]')) = 0
                  OR EXISTS (
                      SELECT 1
                      FROM jsonb_array_elements_text(
                          COALESCE(ge.source_refs, '[]')
                      ) AS entity_sr(knowledge_id)
                      LEFT JOIN knowledge_units entity_ku
                        ON entity_ku.knowledge_id = entity_sr.knowledge_id
                       AND entity_ku.org_id = gc.org_id
                      LEFT JOIN documents entity_d
                        ON entity_d.document_id = entity_ku.document_id
                       AND entity_d.org_id = gc.org_id
                      WHERE entity_ku.knowledge_id IS NULL OR entity_d.document_id IS NULL
                         OR entity_d.deleted_at IS NOT NULL
                         OR NOT COALESCE(
                             entity_d.owner_id = $2 OR entity_d.visibility = 'org'
                                 OR entity_d.document_id = ANY($3),
                             FALSE
                         )
                  )
           )
       ))
     ORDER BY gc.confidence DESC
     LIMIT $5";

fn contradictions_sql() -> &'static str {
    CONTRADICTIONS_SQL
}

pub async fn search_contradictions(
    pool: &PgPool,
    org_id: &str,
    query: Option<&str>,
    limit: i32,
    viewer: Option<&str>,
    granted_ids: &[String],
) -> anyhow::Result<Vec<ContradictionResult>> {
    // Phase 1 RLS: a contradiction search serves exactly one org (taken from
    // the verified caller claims), so it reads through an org-scoped
    // transaction. The SQL still binds `org_id` itself — the database policy
    // is a backstop against that filter being dropped or mis-edited later,
    // not a replacement for it.
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query_as::<
        _,
        (
            String,
            String,
            f64,
            serde_json::Value,
            serde_json::Value,
            serde_json::Value,
        ),
    >(contradictions_sql())
    .bind(org_id)
    .bind(viewer)
    .bind(granted_ids)
    // OR-joined — see `search::textquery`; ANDed terms made the claim search
    // match nothing, which is why the contradiction category was never
    // emitted. `$4` is an OPTIONAL predicate (`$4::text IS NULL OR ...`), so a
    // query that sanitizes to nothing binds NULL — "no text filter" — rather
    // than an empty tsquery that would match no claim at all.
    .bind(
        query
            .map(crate::search::textquery::fts_disjunction)
            .filter(|q| !q.is_empty()),
    )
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contradiction_query_requires_every_derived_reference_to_be_visible() {
        let sql = contradictions_sql();

        assert!(sql.contains("jsonb_array_elements_text"));
        assert!(sql.contains("JOIN knowledge_units"));
        assert!(sql.contains("JOIN documents"));
        assert!(sql.contains("owner_id = $2"));
        assert!(sql.contains("visibility = 'org'"));
        assert!(sql.contains("document_id = ANY($3)"));
        assert!(sql.contains("contradicted_by_claim_ids"));
        assert!(sql.contains("entity_ids"));
        assert!(sql.contains("NOT EXISTS"));
        assert!(!sql.contains("visibility IN ('org', 'shared')"));
    }
}
