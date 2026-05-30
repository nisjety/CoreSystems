use sqlx::PgPool;
use uuid::Uuid;

use crate::model::{Claim, Community, Entity, ExtractionResult, Relationship};

pub struct GraphStore {
    pool: PgPool,
}

impl GraphStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn persist_extraction(
        &self,
        org_id: &str,
        knowledge_id: &str,
        result: &ExtractionResult,
    ) -> anyhow::Result<(Vec<String>, Vec<String>, Vec<String>)> {
        let source_ref = serde_json::json!([knowledge_id]);

        let mut entity_ids = Vec::new();
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
            entity_ids.push(id);
        }

        let entity_map: std::collections::HashMap<&str, &str> = result
            .entities
            .iter()
            .zip(entity_ids.iter())
            .map(|(e, id)| (e.entity_text.as_str(), id.as_str()))
            .collect();

        let mut rel_ids = Vec::new();
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

        Ok((entity_ids, rel_ids, claim_ids))
    }

    pub async fn persist_text_unit_mappings(
        &self,
        org_id: &str,
        knowledge_id: &str,
        entity_ids: &[String],
        relationship_ids: &[String],
        claim_ids: &[String],
    ) -> anyhow::Result<()> {
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
            "SELECT entity_id, org_id, entity_type, entity_text, COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]')
             FROM graph_entities WHERE entity_id = $1 AND org_id = $2"
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
            sqlx::query_as("SELECT COUNT(*) FROM graph_entities WHERE org_id = $1")
                .bind(org_id)
                .fetch_one(&self.pool)
                .await?;

        let (e_total,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM graph_relationships WHERE org_id = $1")
                .bind(org_id)
                .fetch_one(&self.pool)
                .await?;

        let node_rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT entity_id, org_id, entity_type, entity_text, COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]')
             FROM graph_entities WHERE org_id = $1
             ORDER BY created_at DESC LIMIT $2"
        )
        .bind(org_id)
        .bind(limit_nodes)
        .fetch_all(&self.pool)
        .await?;

        let edge_rows = sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT rel_id, org_id, entity_a_id, entity_b_id, relation_type, COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]')
             FROM graph_relationships WHERE org_id = $1
             ORDER BY created_at DESC LIMIT $2"
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
            "SELECT COUNT(*) FROM graph_entities WHERE org_id = $1 AND entity_type = $2",
        )
        .bind(org_id)
        .bind(entity_type)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, String, f64, String, serde_json::Value)>(
            "SELECT entity_id, org_id, entity_type, entity_text, COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]')
             FROM graph_entities WHERE org_id = $1 AND entity_type = $2
             ORDER BY created_at DESC LIMIT $3 OFFSET $4"
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
                "SELECT rel_id, org_id, entity_a_id, entity_b_id, relation_type, COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]')
                 FROM graph_relationships WHERE org_id = $1 AND (entity_a_id = $2 OR entity_b_id = $2) AND relation_type = $3"
            )
            .bind(org_id)
            .bind(entity_id)
            .bind(rt)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as::<_, (String, String, String, String, String, f64, String, serde_json::Value)>(
                "SELECT rel_id, org_id, entity_a_id, entity_b_id, relation_type, COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]')
                 FROM graph_relationships WHERE org_id = $1 AND (entity_a_id = $2 OR entity_b_id = $2)"
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
            "SELECT claim_id, org_id, claim_text, COALESCE(entity_ids, '[]'), COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]'), COALESCE(contradicted_by_claim_ids, '[]'), COALESCE(claim_status, 'active')
             FROM graph_claims WHERE org_id = $1"
        );
        let mut params: Vec<String> = vec![org_id.to_string()];

        if let Some(eid) = entity_id {
            params.push(eid.to_string());
            q.push_str(&format!(" AND entity_ids @> ${}::jsonb", params.len()));
        }
        if let Some(s) = status {
            params.push(s.to_string());
            q.push_str(&format!(" AND claim_status = ${}", params.len()));
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
            "SELECT COUNT(*) FROM graph_claims WHERE org_id = $1 AND jsonb_array_length(COALESCE(contradicted_by_claim_ids, '[]')) > 0"
        )
        .bind(org_id)
        .fetch_one(&self.pool)
        .await?;

        let rows = sqlx::query_as::<_, (String, String, String, serde_json::Value, f64, String, serde_json::Value, serde_json::Value, String)>(
            "SELECT claim_id, org_id, claim_text, COALESCE(entity_ids, '[]'), COALESCE(confidence, 0), COALESCE(provenance, ''), COALESCE(source_refs, '[]'), COALESCE(contradicted_by_claim_ids, '[]'), COALESCE(claim_status, 'active')
             FROM graph_claims WHERE org_id = $1 AND jsonb_array_length(COALESCE(contradicted_by_claim_ids, '[]')) > 0
             ORDER BY created_at DESC LIMIT $2 OFFSET $3"
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

    #[allow(dead_code)] // called by detect_communities; that entry point lands in a later phase
    pub async fn save_community(&self, community: &Community) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO graph_communities (community_id, org_id, entity_ids, summary, level)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (community_id) DO UPDATE SET entity_ids = $3, summary = $4, level = $5",
        )
        .bind(&community.community_id)
        .bind(&community.org_id)
        .bind(serde_json::json!(&community.entity_ids))
        .bind(&community.summary)
        .bind(community.level)
        .execute(&self.pool)
        .await?;
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
}
