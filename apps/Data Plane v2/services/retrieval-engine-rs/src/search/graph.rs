use std::collections::HashMap;

use sqlx::PgPool;

use crate::pipeline::types::ScoredCandidate;

/// SQL for the fused graph ARM: unlike `graph_expansion_search` (which returns
/// entity/relationship/claim objects for the standalone `/v1/retrieve/graph`
/// endpoint), this returns **chunk candidates** grounded in the graph so the
/// arm can be RRF-fused with dense/sparse/wiki/visual (the "entities → candidates
/// shape adapter"). It surfaces chunks of entities matching the query (hop 0)
/// AND chunks of their 1-hop relationship neighbours (hop 1) — the graph's
/// value-add over pure text match. Ordered hop-then-rank so direct hits precede
/// connected ones. Org-scoped throughout; only `visibility = 'org'` provenance
/// (the only content the graph is built from) is eligible, and the pipeline's
/// step-6 canonical gate still re-filters every candidate afterwards.
const GRAPH_ARM_SQL: &str = "WITH matched AS (
        SELECT ge.entity_id,
               ts_rank_cd(to_tsvector('simple', ge.entity_text), plainto_tsquery('simple', $2)) AS rank
        FROM graph_entities ge
        WHERE ge.org_id = $1
          AND to_tsvector('simple', ge.entity_text) @@ plainto_tsquery('simple', $2)
        ORDER BY rank DESC
        LIMIT 25
     ),
     connected AS (
        SELECT entity_id, rank, 0 AS hop FROM matched
        UNION
        SELECT CASE WHEN gr.entity_a_id = m.entity_id THEN gr.entity_b_id ELSE gr.entity_a_id END AS entity_id,
               m.rank, 1 AS hop
        FROM matched m
        JOIN graph_relationships gr
          ON gr.org_id = $1 AND (gr.entity_a_id = m.entity_id OR gr.entity_b_id = m.entity_id)
     ),
     ranked AS (
        SELECT ku.knowledge_id, ku.document_id, ku.text,
               MIN(c.hop) AS hop, MAX(c.rank) AS rank
        FROM connected c
        JOIN graph_text_units gtu ON gtu.entity_id = c.entity_id AND gtu.org_id = $1
        JOIN knowledge_units ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = $1
        JOIN documents d ON d.document_id = ku.document_id AND d.org_id = $1
        WHERE d.deleted_at IS NULL AND d.visibility = 'org'
        GROUP BY ku.knowledge_id, ku.document_id, ku.text
     )
     SELECT knowledge_id, document_id, text
     FROM ranked
     ORDER BY hop ASC, rank DESC
     LIMIT $3";

fn graph_arm_sql() -> &'static str {
    GRAPH_ARM_SQL
}

/// Seed-entity resolution for the REMOTE (deep multi-hop) graph arm: the
/// org-scoped entities whose text matches the query, best-first. These ids seed
/// graph-index's `/v1/graph/traverse`.
const SEED_ENTITY_SQL: &str = "SELECT ge.entity_id
     FROM graph_entities ge
     WHERE ge.org_id = $1
       AND to_tsvector('simple', ge.entity_text) @@ plainto_tsquery('simple', $2)
     ORDER BY ts_rank_cd(to_tsvector('simple', ge.entity_text), plainto_tsquery('simple', $2)) DESC
     LIMIT $3";

/// Entities → chunks grounding for the remote arm. Takes the traversed
/// `(entity_id, hop)` pairs and maps them to org-visible, live chunks via
/// `graph_text_units`, keeping each chunk's MINIMUM hop so direct matches
/// (hop 0 seeds) outrank distant neighbours. Same visibility posture as
/// `GRAPH_ARM_SQL`: only `visibility = 'org'` provenance is eligible, and the
/// pipeline's step-6 canonical gate re-filters afterwards.
const CHUNKS_FOR_ENTITIES_SQL: &str =
    "SELECT ku.knowledge_id, ku.document_id, ku.text, MIN(e.hop)::int AS hop
     FROM UNNEST($2::text[], $3::int[]) AS e(entity_id, hop)
     JOIN graph_text_units gtu ON gtu.entity_id = e.entity_id AND gtu.org_id = $1
     JOIN knowledge_units ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = $1
     JOIN documents d ON d.document_id = ku.document_id AND d.org_id = $1
     WHERE d.deleted_at IS NULL AND d.visibility = 'org'
     GROUP BY ku.knowledge_id, ku.document_id, ku.text
     ORDER BY MIN(e.hop) ASC
     LIMIT $4";

fn seed_entity_sql() -> &'static str {
    SEED_ENTITY_SQL
}

fn chunks_for_entities_sql() -> &'static str {
    CHUNKS_FOR_ENTITIES_SQL
}

/// Resolves the query's seed entity ids (org-scoped, best-first) for the remote
/// multi-hop traversal.
pub async fn seed_entities_for_query(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<String>> {
    // Phase 1 RLS: a retrieval request serves exactly one org (taken from the
    // verified caller claims), so this reads through an org-scoped transaction.
    // The SQL still binds `org_id` itself — the database policy is a backstop
    // against that filter being dropped or mis-edited later, not a replacement.
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query_as::<_, (String,)>(seed_entity_sql())
        .bind(org_id)
        .bind(query)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Grounds traversed `(entity_id, hop)` pairs back to chunk candidates,
/// ordered nearest-hop-first. RRF consumes the order; scores stay 0.
pub async fn chunks_for_entities(
    pool: &PgPool,
    org_id: &str,
    entities: &[(String, u8)],
    limit: i64,
) -> anyhow::Result<Vec<ScoredCandidate>> {
    if entities.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = entities.iter().map(|(id, _)| id.clone()).collect();
    let hops: Vec<i32> = entities.iter().map(|(_, h)| i32::from(*h)).collect();
    // Phase 1 RLS: single-org retrieval path, same rationale as
    // `seed_entities_for_query` above.
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query_as::<_, (String, String, String, i32)>(chunks_for_entities_sql())
        .bind(org_id)
        .bind(&ids)
        .bind(&hops)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(rows
        .into_iter()
        .map(|(knowledge_id, document_id, text, _hop)| ScoredCandidate {
            knowledge_id,
            document_id,
            text,
            dense_score: 0.0,
            sparse_score: 0.0,
            rerank_score: 0.0,
            final_score: 0.0,
            chunk_index: 0,
            metadata: HashMap::new(),
        })
        .collect())
}

/// Graph retrieval ARM for RRF fusion. Returns chunk candidates grounded in the
/// query-matching graph neighbourhood, ordered best-first. RRF consumes the
/// order (not the raw scores), so scores are left at 0 and set during fusion.
/// Non-fatal by contract: the orchestrator logs + skips on error.
pub async fn graph_arm_candidates(
    pool: &PgPool,
    query: &str,
    org_id: &str,
    limit: i64,
) -> anyhow::Result<Vec<ScoredCandidate>> {
    // Phase 1 RLS: single-org retrieval path, same rationale as
    // `seed_entities_for_query` above.
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
    let rows = sqlx::query_as::<_, (String, String, String)>(graph_arm_sql())
        .bind(org_id)
        .bind(query)
        .bind(limit)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(rows
        .into_iter()
        .map(|(knowledge_id, document_id, text)| ScoredCandidate {
            knowledge_id,
            document_id,
            text,
            dense_score: 0.0,
            sparse_score: 0.0,
            rerank_score: 0.0,
            final_score: 0.0,
            chunk_index: 0,
            metadata: HashMap::new(),
        })
        .collect())
}

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
     WHERE org_id = $1 AND to_tsvector('simple', entity_text) @@ plainto_tsquery('simple', $2)
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
     ORDER BY ts_rank_cd(to_tsvector('simple', entity_text), plainto_tsquery('simple', $2)) DESC
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
    // Phase 1 RLS: single-org expansion. All three queries below (entities,
    // their relationships, their claims) share one org, so they share ONE
    // scoped transaction rather than paying the set_config/SET ROLE round trip
    // per entity. The SQL still binds `org_id` itself — the database policy is
    // a backstop, not a replacement for the explicit filter.
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;

    let entity_rows = sqlx::query_as::<_, (String, String, String, f64)>(graph_entity_sql())
        .bind(org_id)
        .bind(query)
        .bind(max_entities)
        .bind(viewer)
        .bind(granted_ids)
        .fetch_all(&mut *tx)
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
        .fetch_all(&mut *tx)
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
            .fetch_all(&mut *tx)
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

    tx.commit().await?;

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

    // Phase 1 RLS: single-org lookup. The per-entity loop below shares one
    // scoped transaction rather than opening one per entity.
    let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;

    let mut summaries = Vec::new();
    for eid in entity_ids {
        let rows = sqlx::query_as::<_, (String, String, serde_json::Value, Option<String>, i32)>(
            community_summary_sql(),
        )
        .bind(org_id)
        .bind(serde_json::json!([eid]))
        .bind(entity_ids)
        .fetch_all(&mut *tx)
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

    tx.commit().await?;

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

    #[test]
    fn graph_arm_sql_is_org_scoped_and_grounds_via_graph_neighbourhood() {
        let sql = graph_arm_sql();
        // Org scoping on every graph table it touches.
        assert!(sql.contains("ge.org_id = $1"));
        assert!(sql.contains("gr.org_id = $1"));
        assert!(sql.contains("gtu.org_id = $1"));
        assert!(sql.contains("ku.org_id = $1"));
        assert!(sql.contains("d.org_id = $1"));
        // Only org-visible, live provenance is eligible.
        assert!(sql.contains("d.visibility = 'org'"));
        assert!(sql.contains("d.deleted_at IS NULL"));
        // Grounds candidates through the graph (entities + 1-hop neighbours),
        // not just a text match — that is the arm's reason to exist.
        assert!(sql.contains("graph_relationships"));
        assert!(sql.contains("graph_text_units"));
        assert!(sql.contains("LIMIT $3"));
    }

    #[test]
    fn remote_arm_queries_are_org_scoped_and_hop_ordered() {
        let seed = seed_entity_sql();
        assert!(seed.contains("ge.org_id = $1"));
        assert!(seed.contains("LIMIT $3"));

        let chunks = chunks_for_entities_sql();
        // Org scoping on every joined table.
        assert!(chunks.contains("gtu.org_id = $1"));
        assert!(chunks.contains("ku.org_id = $1"));
        assert!(chunks.contains("d.org_id = $1"));
        // Only org-visible, live provenance.
        assert!(chunks.contains("d.visibility = 'org'"));
        assert!(chunks.contains("d.deleted_at IS NULL"));
        // Nearest-hop-first ordering (min hop per chunk).
        assert!(chunks.contains("MIN(e.hop)"));
        assert!(chunks.contains("ORDER BY MIN(e.hop) ASC"));
        assert!(chunks.contains("LIMIT $4"));
    }
}
