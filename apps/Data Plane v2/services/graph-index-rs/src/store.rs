use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{
    Claim, Community, Entity, ExtractionResult, MirrorEntity, MirrorRelationship,
    PersistedExtraction, Relationship,
};

pub struct GraphStore {
    pool: PgPool,
}

impl GraphStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Returns chunks only when their canonical document is live and visible
    /// to the whole verified organization. Private/shared grants are not a
    /// graph authorization source in the secure MVP.
    pub async fn load_org_visible_chunks(
        &self,
        org_id: &str,
        document_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        Ok(sqlx::query_as(
            "SELECT ku.knowledge_id, ku.text
             FROM knowledge_units AS ku
             JOIN documents AS d
               ON d.document_id = ku.document_id AND d.org_id = ku.org_id
             WHERE d.document_id = $1 AND d.org_id = $2
               AND d.visibility = 'org' AND d.deleted_at IS NULL",
        )
        .bind(document_id)
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn knowledge_unit_is_org_visible(
        &self,
        org_id: &str,
        knowledge_id: &str,
    ) -> anyhow::Result<bool> {
        let (visible,): (bool,) = sqlx::query_as(
            "SELECT EXISTS (
                SELECT 1 FROM knowledge_units AS ku
                JOIN documents AS d
                  ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                WHERE ku.knowledge_id = $1 AND ku.org_id = $2
                  AND d.visibility = 'org' AND d.deleted_at IS NULL
             )",
        )
        .bind(knowledge_id)
        .bind(org_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(visible)
    }

    pub async fn persist_extraction(
        &self,
        org_id: &str,
        knowledge_id: &str,
        result: &ExtractionResult,
    ) -> anyhow::Result<PersistedExtraction> {
        if !self
            .knowledge_unit_is_org_visible(org_id, knowledge_id)
            .await?
        {
            // Not org-visible → persist nothing. The empty result means the
            // Neo4j mirror is a no-op too, so the read-model inherits this gate
            // (and the upstream restrictive-ZDR drop) with no extra code.
            return Ok(PersistedExtraction::default());
        }
        let source_ref = serde_json::json!([knowledge_id]);

        let mut entity_ids = Vec::new();
        let mut mirror_entities = Vec::new();
        for e in &result.entities {
            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO graph_entities (entity_id, org_id, entity_type, entity_text, confidence, provenance, source_refs)
                 VALUES ($1, $2, $3, $4, $5, 'extracted', $6)
                 ON CONFLICT (entity_id) DO NOTHING"
            )
            .bind(&id)
            .bind(org_id)
            .bind(&e.entity_type)
            .bind(&e.entity_text)
            .bind(e.confidence)
            .bind(&source_ref)
            .execute(&self.pool)
            .await?;
            mirror_entities.push(MirrorEntity {
                entity_id: id.clone(),
                entity_type: e.entity_type.clone(),
                entity_text: e.entity_text.clone(),
                confidence: e.confidence,
            });
            entity_ids.push(id);
        }

        let entity_map: std::collections::HashMap<&str, &str> = result
            .entities
            .iter()
            .zip(entity_ids.iter())
            .map(|(e, id)| (e.entity_text.as_str(), id.as_str()))
            .collect();

        let mut rel_ids = Vec::new();
        let mut mirror_relationships = Vec::new();
        for r in &result.relationships {
            let a_id = entity_map
                .get(r.source_entity.as_str())
                .copied()
                .unwrap_or("");
            let b_id = entity_map
                .get(r.target_entity.as_str())
                .copied()
                .unwrap_or("");
            if a_id.is_empty() || b_id.is_empty() {
                continue;
            }
            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO graph_relationships (rel_id, org_id, entity_a_id, entity_b_id, relation_type, confidence, provenance, source_refs)
                 VALUES ($1, $2, $3, $4, $5, $6, 'extracted', $7)
                 ON CONFLICT (rel_id) DO NOTHING"
            )
            .bind(&id)
            .bind(org_id)
            .bind(a_id)
            .bind(b_id)
            .bind(&r.relation_type)
            .bind(r.confidence)
            .bind(&source_ref)
            .execute(&self.pool)
            .await?;
            mirror_relationships.push(MirrorRelationship {
                rel_id: id.clone(),
                entity_a_id: a_id.to_string(),
                entity_b_id: b_id.to_string(),
                relation_type: r.relation_type.clone(),
                confidence: r.confidence,
            });
            rel_ids.push(id);
        }

        let mut claim_ids = Vec::new();
        for c in &result.claims {
            let linked: Vec<&str> = c
                .related_entities
                .iter()
                .filter_map(|name| entity_map.get(name.as_str()).copied())
                .collect();
            let id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO graph_claims (claim_id, org_id, claim_text, entity_ids, confidence, provenance, source_refs, claim_status)
                 VALUES ($1, $2, $3, $4, $5, 'extracted', $6, 'active')
                 ON CONFLICT (claim_id) DO NOTHING"
            )
            .bind(&id)
            .bind(org_id)
            .bind(&c.claim_text)
            .bind(serde_json::json!(linked))
            .bind(c.confidence)
            .bind(&source_ref)
            .execute(&self.pool)
            .await?;
            claim_ids.push(id);
        }

        Ok(PersistedExtraction {
            entity_ids,
            rel_ids,
            claim_ids,
            mirror_entities,
            mirror_relationships,
        })
    }

    pub async fn persist_text_unit_mappings(
        &self,
        org_id: &str,
        knowledge_id: &str,
        entity_ids: &[String],
        relationship_ids: &[String],
        claim_ids: &[String],
    ) -> anyhow::Result<()> {
        if !self
            .knowledge_unit_is_org_visible(org_id, knowledge_id)
            .await?
        {
            return Ok(());
        }
        for eid in entity_ids {
            sqlx::query(
                "INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id)
                 VALUES ($1, $2, $3, NULL, NULL)
                 ON CONFLICT DO NOTHING",
            )
            .bind(org_id)
            .bind(knowledge_id)
            .bind(eid)
            .execute(&self.pool)
            .await?;
        }
        for rid in relationship_ids {
            sqlx::query(
                "INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id)
                 VALUES ($1, $2, NULL, $3, NULL)
                 ON CONFLICT DO NOTHING",
            )
            .bind(org_id)
            .bind(knowledge_id)
            .bind(rid)
            .execute(&self.pool)
            .await?;
        }
        for cid in claim_ids {
            sqlx::query(
                "INSERT INTO graph_text_units (org_id, knowledge_id, entity_id, rel_id, claim_id)
                 VALUES ($1, $2, NULL, NULL, $3)
                 ON CONFLICT DO NOTHING",
            )
            .bind(org_id)
            .bind(knowledge_id)
            .bind(cid)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Removes the chunk→graph mappings for knowledge IDs orphaned by a content
    /// re-chunk (carried on knowledge.units.deleted). Graph retrieval joins
    /// through graph_text_units to live chunks, so dropping these mappings is
    /// enough to keep superseded content out of results; any now-unreferenced
    /// entities/relationships/claims are pruned by a separate GC pass.
    pub async fn delete_text_unit_mappings(
        &self,
        org_id: &str,
        knowledge_ids: &[String],
    ) -> anyhow::Result<u64> {
        if knowledge_ids.is_empty() {
            return Ok(0);
        }
        let result = sqlx::query(
            "DELETE FROM graph_text_units WHERE org_id = $1 AND knowledge_id = ANY($2)",
        )
        .bind(org_id)
        .bind(knowledge_ids)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn get_entity(
        &self,
        org_id: &str,
        entity_id: &str,
    ) -> anyhow::Result<Option<Entity>> {
        let row = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge WHERE ge.entity_id = $1 AND ge.org_id = $2
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(entity_id)
        .bind(org_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(
            row.map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            }),
        )
    }

    /// D4+D5 spec §3.1 — aggregate graph snapshot for one org.
    /// Returns `(nodes, edges, total_nodes, total_edges)`. Handler uses the
    /// totals vs limits to decide whether to return `200` or `413` so the
    /// caller knows to fall back to paginated endpoints.
    pub async fn snapshot_org_graph(
        &self,
        org_id: &str,
        limit_nodes: i32,
        limit_edges: i32,
    ) -> anyhow::Result<(Vec<Entity>, Vec<Relationship>, i64, i64)> {
        let (n_total,): (i64,) =
            sqlx::query_as(
                "SELECT COUNT(*) FROM graph_entities AS ge WHERE ge.org_id = $1
                 AND EXISTS (
                   SELECT 1 FROM graph_text_units AS gtu
                   JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                   JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                   WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                     AND d.visibility = 'org' AND d.deleted_at IS NULL
                 )",
            )
                .bind(org_id)
                .fetch_one(&self.pool)
                .await?;

        let (e_total,): (i64,) =
            sqlx::query_as(
                "SELECT COUNT(*) FROM graph_relationships AS gr WHERE gr.org_id = $1
                 AND EXISTS (
                   SELECT 1 FROM graph_text_units AS gtu
                   JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                   JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                   WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                     AND d.visibility = 'org' AND d.deleted_at IS NULL
                 )",
            )
                .bind(org_id)
                .fetch_one(&self.pool)
                .await?;

        let node_rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge WHERE ge.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY ge.created_at DESC LIMIT $2"
        )
        .bind(org_id)
        .bind(limit_nodes)
        .fetch_all(&self.pool)
        .await?;

        let edge_rows = sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
             FROM graph_relationships AS gr WHERE gr.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY gr.created_at DESC LIMIT $2"
        )
        .bind(org_id)
        .bind(limit_edges)
        .fetch_all(&self.pool)
        .await?;

        let nodes = node_rows
            .into_iter()
            .map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        let edges = edge_rows
            .into_iter()
            .map(|(rid, oid, a, b, rt, conf, prov, refs)| Relationship {
                rel_id: rid,
                org_id: oid,
                entity_a_id: a,
                entity_b_id: b,
                relation_type: rt,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        Ok((nodes, edges, n_total, e_total))
    }

    pub async fn list_entities_by_type(
        &self,
        org_id: &str,
        entity_type: &str,
        limit: i32,
        offset: i32,
    ) -> anyhow::Result<(Vec<Entity>, i64)> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM graph_entities AS ge WHERE ge.org_id = $1 AND ge.entity_type = $2
             AND EXISTS (
               SELECT 1 FROM graph_text_units AS gtu
               JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
               JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
               WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                 AND d.visibility = 'org' AND d.deleted_at IS NULL
             )",
        )
        .bind(org_id)
        .bind(entity_type)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge WHERE ge.org_id = $1 AND ge.entity_type = $2
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY ge.created_at DESC LIMIT $3 OFFSET $4"
        )
        .bind(org_id)
        .bind(entity_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let entities = rows
            .into_iter()
            .map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        Ok((entities, count))
    }

    pub async fn get_relationships(
        &self,
        org_id: &str,
        entity_id: &str,
        relation_type: Option<&str>,
    ) -> anyhow::Result<Vec<Relationship>> {
        let rows = if let Some(rt) = relation_type {
            sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
                "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
                 FROM graph_relationships AS gr
                 WHERE gr.org_id = $1 AND (gr.entity_a_id = $2 OR gr.entity_b_id = $2) AND gr.relation_type = $3
                   AND EXISTS (
                     SELECT 1 FROM graph_text_units AS gtu
                     JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                     JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                     WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                       AND d.visibility = 'org' AND d.deleted_at IS NULL
                   )"
            )
            .bind(org_id)
            .bind(entity_id)
            .bind(rt)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
                "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
                 FROM graph_relationships AS gr
                 WHERE gr.org_id = $1 AND (gr.entity_a_id = $2 OR gr.entity_b_id = $2)
                   AND EXISTS (
                     SELECT 1 FROM graph_text_units AS gtu
                     JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                     JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                     WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                       AND d.visibility = 'org' AND d.deleted_at IS NULL
                   )"
            )
            .bind(org_id)
            .bind(entity_id)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|(rid, oid, a, b, rt, conf, prov, refs)| Relationship {
                rel_id: rid,
                org_id: oid,
                entity_a_id: a,
                entity_b_id: b,
                relation_type: rt,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect())
    }

    pub async fn get_claims(
        &self,
        org_id: &str,
        entity_id: Option<&str>,
        status: Option<&str>,
    ) -> anyhow::Result<Vec<Claim>> {
        let mut q = String::from(
            "SELECT gc.claim_id, gc.org_id, gc.claim_text, COALESCE(gc.entity_ids, '[]'), COALESCE(gc.confidence, 0), COALESCE(gc.provenance, ''), COALESCE(gc.source_refs, '[]'), COALESCE(gc.contradicted_by_claim_ids, '[]'), COALESCE(gc.claim_status, 'active')
             FROM graph_claims AS gc WHERE gc.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.claim_id = gc.claim_id AND gtu.org_id = gc.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        );
        let mut params: Vec<String> = vec![org_id.to_string()];

        if let Some(eid) = entity_id {
            params.push(eid.to_string());
            q.push_str(&format!(" AND gc.entity_ids @> ${}::jsonb", params.len()));
        }
        if let Some(s) = status {
            params.push(s.to_string());
            q.push_str(&format!(" AND gc.claim_status = ${}", params.len()));
        }

        let mut query = sqlx::query_as::<
            _,
            (
                String,
                String,
                String,
                serde_json::Value,
                f64,
                String,
                serde_json::Value,
                serde_json::Value,
                String,
            ),
        >(&q);
        for p in &params {
            query = query.bind(p);
        }
        let rows = query.fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(
                |(cid, oid, text, eids, conf, prov, refs, contra, st)| Claim {
                    claim_id: cid,
                    org_id: oid,
                    claim_text: text,
                    entity_ids: serde_json::from_value(eids).unwrap_or_default(),
                    confidence: conf,
                    provenance: prov,
                    source_refs: serde_json::from_value(refs).unwrap_or_default(),
                    contradicted_by: serde_json::from_value(contra).unwrap_or_default(),
                    status: st,
                },
            )
            .collect())
    }

    pub async fn get_contradictions(
        &self,
        org_id: &str,
        limit: i32,
        offset: i32,
    ) -> anyhow::Result<(Vec<Claim>, i64)> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM graph_claims AS gc
             WHERE gc.org_id = $1 AND jsonb_array_length(COALESCE(gc.contradicted_by_claim_ids, '[]')) > 0
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.claim_id = gc.claim_id AND gtu.org_id = gc.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(org_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, serde_json::Value, f64, String, serde_json::Value, serde_json::Value, String)>(
            "SELECT gc.claim_id, gc.org_id, gc.claim_text, COALESCE(gc.entity_ids, '[]'), COALESCE(gc.confidence, 0), COALESCE(gc.provenance, ''), COALESCE(gc.source_refs, '[]'), COALESCE(gc.contradicted_by_claim_ids, '[]'), COALESCE(gc.claim_status, 'active')
             FROM graph_claims AS gc
             WHERE gc.org_id = $1 AND jsonb_array_length(COALESCE(gc.contradicted_by_claim_ids, '[]')) > 0
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.claim_id = gc.claim_id AND gtu.org_id = gc.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             ORDER BY gc.created_at DESC LIMIT $2 OFFSET $3"
        )
        .bind(org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let claims = rows
            .into_iter()
            .map(
                |(cid, oid, text, eids, conf, prov, refs, contra, st)| Claim {
                    claim_id: cid,
                    org_id: oid,
                    claim_text: text,
                    entity_ids: serde_json::from_value(eids).unwrap_or_default(),
                    confidence: conf,
                    provenance: prov,
                    source_refs: serde_json::from_value(refs).unwrap_or_default(),
                    contradicted_by: serde_json::from_value(contra).unwrap_or_default(),
                    status: st,
                },
            )
            .collect();

        Ok((claims, count))
    }

    /// All org entity ids whose provenance is live + org-visible (the same
    /// `graph_text_units` → `documents` gate every read uses). One bulk query —
    /// community detection must not do a per-entity N+1.
    pub async fn list_visible_entity_ids(&self, org_id: &str) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query_as::<_, (String,)>(
            "SELECT ge.entity_id FROM graph_entities AS ge
             WHERE ge.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             LIMIT 10000",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(id,)| id).collect())
    }

    /// All org relationship endpoint pairs with live + org-visible provenance.
    /// One bulk query backing the community adjacency build.
    pub async fn list_visible_relationship_pairs(
        &self,
        org_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        Ok(sqlx::query_as::<_, (String, String)>(
            "SELECT gr.entity_a_id, gr.entity_b_id FROM graph_relationships AS gr
             WHERE gr.org_id = $1
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )
             LIMIT 50000",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Atomically replaces the org's derived communities with a fresh detection
    /// run (communities are a derived artifact — delete-and-replace inside one
    /// transaction keeps re-detection idempotent at the set level and never
    /// leaves a half-written state). Membership provenance is inherited: every
    /// member entity id comes from the visibility-gated listing above, and the
    /// read side additionally requires the queried entity set to cover the
    /// community (`retrieval-engine` COMMUNITY_SUMMARY_SQL subset check).
    pub async fn replace_communities(
        &self,
        org_id: &str,
        communities: &[Community],
    ) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM graph_communities WHERE org_id = $1")
            .bind(org_id)
            .execute(&mut *tx)
            .await?;
        for c in communities {
            sqlx::query(
                "INSERT INTO graph_communities (community_id, org_id, entity_ids, summary, level)
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(&c.community_id)
            .bind(org_id)
            .bind(serde_json::json!(c.entity_ids))
            .bind(&c.summary)
            .bind(c.level)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn get_graph_expansion(
        &self,
        org_id: &str,
        seed_ids: &[String],
        max_hops: i32,
        max_entities: i32,
    ) -> anyhow::Result<(Vec<Entity>, Vec<Relationship>)> {
        let mut visited: std::collections::HashSet<String> = seed_ids.iter().cloned().collect();
        let mut frontier: Vec<String> = seed_ids.to_vec();
        let mut all_entities = Vec::new();
        let mut all_rels = Vec::new();

        for _ in 0..max_hops {
            if frontier.is_empty() || all_entities.len() >= max_entities as usize {
                break;
            }
            let mut next_frontier = Vec::new();
            for eid in &frontier {
                let rels = self.get_relationships(org_id, eid, None).await?;
                for rel in &rels {
                    let neighbor = if rel.entity_a_id == *eid {
                        &rel.entity_b_id
                    } else {
                        &rel.entity_a_id
                    };
                    if !visited.contains(neighbor) {
                        visited.insert(neighbor.clone());
                        next_frontier.push(neighbor.clone());
                        if let Some(entity) = self.get_entity(org_id, neighbor).await? {
                            all_entities.push(entity);
                        }
                    }
                }
                all_rels.extend(rels);
            }
            frontier = next_frontier;
        }

        Ok((all_entities, all_rels))
    }

    /// Re-joins a set of entity ids (e.g. from a Neo4j traversal) against the
    /// canonical Postgres graph, returning only entities that are live and
    /// org-visible, plus the relationships whose BOTH endpoints are in the
    /// visible set. This is the security-critical gate that makes Neo4j a pure
    /// topology accelerator: even a stale or over-broad read-model cannot leak,
    /// because provenance/visibility is enforced here from canonical Postgres.
    pub async fn get_subgraph_visible(
        &self,
        org_id: &str,
        entity_ids: &[String],
    ) -> anyhow::Result<(Vec<Entity>, Vec<Relationship>)> {
        if entity_ids.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let entity_rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT ge.entity_id, ge.org_id, ge.entity_type, ge.entity_text, COALESCE(ge.confidence, 0), COALESCE(ge.provenance, ''), COALESCE(ge.source_refs, '[]')
             FROM graph_entities AS ge
             WHERE ge.org_id = $1 AND ge.entity_id = ANY($2)
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.entity_id = ge.entity_id AND gtu.org_id = ge.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(org_id)
        .bind(entity_ids)
        .fetch_all(&self.pool)
        .await?;

        let entities: Vec<Entity> = entity_rows
            .into_iter()
            .map(|(eid, oid, etype, etext, conf, prov, refs)| Entity {
                entity_id: eid,
                org_id: oid,
                entity_type: etype,
                entity_text: etext,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .collect();

        // Only entities that survived the visibility gate may anchor an edge.
        let visible_ids: std::collections::HashSet<&str> =
            entities.iter().map(|e| e.entity_id.as_str()).collect();

        let rel_rows = sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT gr.rel_id, gr.org_id, gr.entity_a_id, gr.entity_b_id, gr.relation_type, COALESCE(gr.confidence, 0), COALESCE(gr.provenance, ''), COALESCE(gr.source_refs, '[]')
             FROM graph_relationships AS gr
             WHERE gr.org_id = $1 AND gr.entity_a_id = ANY($2) AND gr.entity_b_id = ANY($2)
               AND EXISTS (
                 SELECT 1 FROM graph_text_units AS gtu
                 JOIN knowledge_units AS ku ON ku.knowledge_id = gtu.knowledge_id AND ku.org_id = gtu.org_id
                 JOIN documents AS d ON d.document_id = ku.document_id AND d.org_id = ku.org_id
                 WHERE gtu.rel_id = gr.rel_id AND gtu.org_id = gr.org_id
                   AND d.visibility = 'org' AND d.deleted_at IS NULL
               )"
        )
        .bind(org_id)
        .bind(entity_ids)
        .fetch_all(&self.pool)
        .await?;

        let relationships: Vec<Relationship> = rel_rows
            .into_iter()
            .map(|(rid, oid, a, b, rt, conf, prov, refs)| Relationship {
                rel_id: rid,
                org_id: oid,
                entity_a_id: a,
                entity_b_id: b,
                relation_type: rt,
                confidence: conf,
                provenance: prov,
                source_refs: serde_json::from_value(refs).unwrap_or_default(),
            })
            .filter(|r| {
                visible_ids.contains(r.entity_a_id.as_str())
                    && visible_ids.contains(r.entity_b_id.as_str())
            })
            .collect();

        Ok((entities, relationships))
    }
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    const FIXTURE_SQL: &str = r#"
        CREATE TABLE documents (
            document_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, owner_id TEXT NOT NULL,
            visibility TEXT NOT NULL, deleted_at TIMESTAMPTZ
        );
        CREATE TABLE knowledge_units (
            knowledge_id TEXT PRIMARY KEY, document_id TEXT NOT NULL, org_id TEXT NOT NULL,
            text TEXT NOT NULL
        );
        CREATE TABLE graph_entities (
            entity_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, entity_type TEXT NOT NULL,
            entity_text TEXT NOT NULL, confidence DOUBLE PRECISION, provenance TEXT,
            source_refs JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_relationships (
            rel_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, entity_a_id TEXT NOT NULL,
            entity_b_id TEXT NOT NULL, relation_type TEXT NOT NULL,
            confidence DOUBLE PRECISION, provenance TEXT, source_refs JSONB,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_claims (
            claim_id TEXT PRIMARY KEY, org_id TEXT NOT NULL, claim_text TEXT NOT NULL,
            entity_ids JSONB, confidence DOUBLE PRECISION, provenance TEXT,
            source_refs JSONB, contradicted_by_claim_ids JSONB, claim_status TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        );
        CREATE TABLE graph_text_units (
            id BIGSERIAL PRIMARY KEY, org_id TEXT NOT NULL, knowledge_id TEXT NOT NULL,
            entity_id TEXT, rel_id TEXT, claim_id TEXT
        );

        INSERT INTO documents VALUES
            ('doc-org', 'org-a', 'user-a', 'org', NULL),
            ('doc-private', 'org-a', 'user-a', 'private', NULL),
            ('doc-shared', 'org-a', 'user-a', 'shared', NULL),
            ('doc-deleted', 'org-a', 'user-a', 'org', NOW()),
            ('doc-other', 'org-b', 'user-c', 'org', NULL);
        INSERT INTO knowledge_units VALUES
            ('k-org', 'doc-org', 'org-a', 'org visible'),
            ('k-private', 'doc-private', 'org-a', 'private'),
            ('k-shared', 'doc-shared', 'org-a', 'shared'),
            ('k-deleted', 'doc-deleted', 'org-a', 'deleted'),
            ('k-other', 'doc-other', 'org-b', 'other org');
        INSERT INTO graph_entities VALUES
            ('e-org-a', 'org-a', 'Person', 'Org A', 1, 'test', '[]', NOW()),
            ('e-org-b', 'org-a', 'Person', 'Org B', 1, 'test', '[]', NOW()),
            ('e-private', 'org-a', 'Person', 'Private', 1, 'test', '[]', NOW()),
            ('e-shared', 'org-a', 'Person', 'Shared', 1, 'test', '[]', NOW()),
            ('e-deleted', 'org-a', 'Person', 'Deleted', 1, 'test', '[]', NOW()),
            ('e-other', 'org-b', 'Person', 'Other', 1, 'test', '[]', NOW());
        INSERT INTO graph_relationships VALUES
            ('r-org', 'org-a', 'e-org-a', 'e-org-b', 'knows', 1, 'test', '[]', NOW()),
            ('r-private', 'org-a', 'e-private', 'e-private', 'knows', 1, 'test', '[]', NOW()),
            ('r-shared', 'org-a', 'e-shared', 'e-shared', 'knows', 1, 'test', '[]', NOW()),
            ('r-deleted', 'org-a', 'e-deleted', 'e-deleted', 'knows', 1, 'test', '[]', NOW()),
            ('r-other', 'org-b', 'e-other', 'e-other', 'knows', 1, 'test', '[]', NOW());
        INSERT INTO graph_claims VALUES
            ('c-org', 'org-a', 'org claim', '["e-org-a"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-private', 'org-a', 'private claim', '["e-private"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-shared', 'org-a', 'shared claim', '["e-shared"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-deleted', 'org-a', 'deleted claim', '["e-deleted"]', 1, 'test', '[]', '["c-x"]', 'active', NOW()),
            ('c-other', 'org-b', 'other claim', '["e-other"]', 1, 'test', '[]', '["c-x"]', 'active', NOW());
        INSERT INTO graph_text_units (org_id, knowledge_id, entity_id) VALUES
            ('org-a', 'k-org', 'e-org-a'), ('org-a', 'k-org', 'e-org-b'),
            ('org-a', 'k-private', 'e-private'), ('org-a', 'k-shared', 'e-shared'),
            ('org-a', 'k-deleted', 'e-deleted'), ('org-b', 'k-other', 'e-other');
        INSERT INTO graph_text_units (org_id, knowledge_id, rel_id) VALUES
            ('org-a', 'k-org', 'r-org'), ('org-a', 'k-private', 'r-private'),
            ('org-a', 'k-shared', 'r-shared'), ('org-a', 'k-deleted', 'r-deleted'),
            ('org-b', 'k-other', 'r-other');
        INSERT INTO graph_text_units (org_id, knowledge_id, claim_id) VALUES
            ('org-a', 'k-org', 'c-org'), ('org-a', 'k-private', 'c-private'),
            ('org-a', 'k-shared', 'c-shared'), ('org-a', 'k-deleted', 'c-deleted'),
            ('org-b', 'k-other', 'c-other');
    "#;

    async fn fixture() -> (GraphStore, PgPool, String) {
        let database_url = std::env::var("GRAPH_TEST_DATABASE_URL")
            .expect("GRAPH_TEST_DATABASE_URL must point to disposable PostgreSQL");
        assert!(
            (database_url.contains("localhost") || database_url.contains("127.0.0.1"))
                && database_url.contains("/graph_test"),
            "refusing non-local or non-graph_test database"
        );
        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect disposable admin database");
        let schema = format!("graph_visibility_{}", Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}"))
            .execute(&admin)
            .await
            .expect("create isolated graph schema");
        admin.close().await;

        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .expect("connect isolated graph pool");
        sqlx::query(&format!("SET search_path TO {schema}"))
            .execute(&pool)
            .await
            .expect("select isolated graph schema");
        sqlx::raw_sql(FIXTURE_SQL)
            .execute(&pool)
            .await
            .expect("create graph visibility fixture");
        (GraphStore::new(pool.clone()), pool, schema)
    }

    #[tokio::test]
    #[ignore = "requires GRAPH_TEST_DATABASE_URL pointing to disposable PostgreSQL"]
    async fn only_live_org_visible_provenance_is_queryable_for_same_org_users() {
        let (store, pool, schema) = fixture().await;

        for _conceptual_user in ["user-a", "user-b"] {
            let (entities, relationships, entity_total, relationship_total) =
                store.snapshot_org_graph("org-a", 100, 100).await.unwrap();
            assert_eq!(entity_total, 2);
            assert_eq!(relationship_total, 1);
            assert_eq!(entities.len(), 2);
            assert_eq!(relationships.len(), 1);
            assert!(store
                .get_entity("org-a", "e-org-a")
                .await
                .unwrap()
                .is_some());
            for hidden in ["e-private", "e-shared", "e-deleted", "e-other"] {
                assert!(store.get_entity("org-a", hidden).await.unwrap().is_none());
            }
            let (listed, total) = store
                .list_entities_by_type("org-a", "Person", 100, 0)
                .await
                .unwrap();
            assert_eq!((listed.len(), total), (2, 2));
            assert_eq!(
                store
                    .get_relationships("org-a", "e-org-a", None)
                    .await
                    .unwrap()
                    .len(),
                1
            );
            assert!(store
                .get_relationships("org-a", "e-private", None)
                .await
                .unwrap()
                .is_empty());
            let claims = store.get_claims("org-a", None, None).await.unwrap();
            assert_eq!(
                claims
                    .iter()
                    .map(|c| c.claim_id.as_str())
                    .collect::<Vec<_>>(),
                ["c-org"]
            );
            let (contradictions, total) = store.get_contradictions("org-a", 100, 0).await.unwrap();
            assert_eq!(total, 1);
            assert_eq!(contradictions[0].claim_id, "c-org");
            let (expanded, rels) = store
                .get_graph_expansion("org-a", &["e-org-a".into()], 1, 100)
                .await
                .unwrap();
            assert_eq!(expanded[0].entity_id, "e-org-b");
            assert_eq!(rels[0].rel_id, "r-org");
        }

        assert_eq!(
            store
                .load_org_visible_chunks("org-a", "doc-org")
                .await
                .unwrap(),
            vec![("k-org".into(), "org visible".into())]
        );
        for hidden_doc in ["doc-private", "doc-shared", "doc-deleted", "doc-other"] {
            assert!(store
                .load_org_visible_chunks("org-a", hidden_doc)
                .await
                .unwrap()
                .is_empty());
        }

        sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
            .execute(&pool)
            .await
            .expect("drop isolated graph schema");
        pool.close().await;
    }
}
