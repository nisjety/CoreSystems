//! Neo4j-backed graph read-model client.
//!
//! Neo4j mirrors the **canonical Postgres graph** (`graph_entities` /
//! `graph_relationships`) and is used ONLY for fast native multi-hop Cypher
//! traversal. It is a rebuildable read-model — the same role Qdrant/Quickwit
//! play for vectors/sparse — and is **never** an authorization source: callers
//! re-join provenance + org-visibility in Postgres before anything leaves the
//! plane (see `docs/graphrag-neo4j-plan.md` §5.2). Every statement is scoped by
//! an `org_id` predicate so cross-org traversal is impossible by construction.
//!
//! Phase 2 provides connection + idempotent schema bootstrap. The dual-write
//! (Phase 3) and multi-hop traversal (Phase 4) land in later phases.

use std::collections::HashMap;

use anyhow::Context;
use neo4rs::{query, BoltType, ConfigBuilder, Graph};

use crate::config::Config;
use crate::model::{MirrorEntity, MirrorRelationship};

/// Idempotent schema bootstrap. `entity_id` uniqueness makes `MERGE (e:Entity
/// {entity_id})` deduplicating (re-processing a chunk never forks a node — the
/// reference architecture's `CREATE`-with-random-UUID duplication bug is avoided
/// by reusing the Postgres PK). The `org_id` indexes back org-scoped traversal
/// and seed lookups. `IF NOT EXISTS` makes every statement safe on every boot.
pub const SCHEMA_STATEMENTS: &[&str] = &[
    "CREATE CONSTRAINT entity_id_unique IF NOT EXISTS \
     FOR (e:Entity) REQUIRE e.entity_id IS UNIQUE",
    "CREATE INDEX entity_org IF NOT EXISTS FOR (e:Entity) ON (e.org_id)",
    "CREATE INDEX entity_org_text IF NOT EXISTS FOR (e:Entity) ON (e.org_id, e.entity_text)",
    "CREATE INDEX rel_org IF NOT EXISTS FOR ()-[r:REL]-() ON (r.org_id)",
];

/// Idempotent entity mirror. `MERGE` on the Postgres PK (`entity_id`) means
/// re-processing a chunk updates in place instead of forking a node; `org_id`
/// is written from the scalar `$org_id` param so every node carries its tenant.
const MERGE_ENTITIES_CYPHER: &str = "UNWIND $entities AS e \
     MERGE (n:Entity {entity_id: e.entity_id}) \
     SET n.org_id = $org_id, n.entity_type = e.entity_type, \
         n.entity_text = e.entity_text, n.confidence = e.confidence, \
         n.updated_at = timestamp()";

/// Idempotent relationship mirror. Both endpoints are MATCHed **within the same
/// org** (`org_id: $org_id`) so an edge can never bridge tenants even if the
/// payload were malformed; `MERGE` on `rel_id` keeps it idempotent.
const MERGE_RELATIONSHIPS_CYPHER: &str = "UNWIND $rels AS r \
     MATCH (a:Entity {entity_id: r.entity_a_id, org_id: $org_id}) \
     MATCH (b:Entity {entity_id: r.entity_b_id, org_id: $org_id}) \
     MERGE (a)-[e:REL {rel_id: r.rel_id}]->(b) \
     SET e.org_id = $org_id, e.relation_type = r.relation_type, \
         e.confidence = r.confidence, e.updated_at = timestamp()";

/// Shapes one entity as a Bolt map for the `UNWIND $entities` batch.
fn entity_row(e: &MirrorEntity) -> BoltType {
    let mut m: HashMap<String, BoltType> = HashMap::new();
    m.insert("entity_id".into(), e.entity_id.clone().into());
    m.insert("entity_type".into(), e.entity_type.clone().into());
    m.insert("entity_text".into(), e.entity_text.clone().into());
    m.insert("confidence".into(), e.confidence.into());
    m.into()
}

/// Shapes one relationship as a Bolt map for the `UNWIND $rels` batch.
fn rel_row(r: &MirrorRelationship) -> BoltType {
    let mut m: HashMap<String, BoltType> = HashMap::new();
    m.insert("rel_id".into(), r.rel_id.clone().into());
    m.insert("entity_a_id".into(), r.entity_a_id.clone().into());
    m.insert("entity_b_id".into(), r.entity_b_id.clone().into());
    m.insert("relation_type".into(), r.relation_type.clone().into());
    m.insert("confidence".into(), r.confidence.into());
    m.into()
}

/// Bolt client over the Neo4j graph read-model. Cheap to clone (the inner
/// `Graph` holds a connection pool behind an `Arc`).
#[derive(Clone)]
pub struct Neo4jClient {
    graph: Graph,
}

impl Neo4jClient {
    /// Connects using the deployment config. The caller is responsible for the
    /// fail-closed secret check (`NEO4J_ENABLED` without a password); a
    /// connection/handshake error here propagates so the caller can degrade to
    /// the Postgres fallback rather than crash the service.
    pub async fn connect(cfg: &Config) -> anyhow::Result<Self> {
        let config = ConfigBuilder::default()
            .uri(cfg.neo4j_url.clone())
            .user(cfg.neo4j_user.clone())
            .password(cfg.neo4j_password.clone())
            .db(cfg.neo4j_database.clone())
            .build()
            .context("build neo4j config")?;
        let graph = Graph::connect(config)
            .await
            .context("connect to neo4j graph read-model")?;
        Ok(Self { graph })
    }

    /// Runs the idempotent constraint/index bootstrap. Safe on every boot.
    pub async fn ensure_schema(&self) -> anyhow::Result<()> {
        for stmt in SCHEMA_STATEMENTS {
            self.graph
                .run(query(stmt))
                .await
                .with_context(|| format!("neo4j schema statement failed: {stmt}"))?;
        }
        Ok(())
    }

    /// Mirrors just-persisted entities + relationships into the read-model.
    ///
    /// Idempotent (`MERGE` on the Postgres PKs) and org-scoped (`org_id` is set
    /// on every node/edge). Called ONLY with rows that already cleared the
    /// canonical Postgres org-visibility + restrictive-ZDR gates in
    /// `persist_extraction` / the consumer, so this method needs no ZDR logic of
    /// its own. Callers treat any error as non-fatal: Postgres stays canonical
    /// and the read-model is rebuildable.
    pub async fn merge_extraction(
        &self,
        org_id: &str,
        entities: &[MirrorEntity],
        relationships: &[MirrorRelationship],
    ) -> anyhow::Result<()> {
        if entities.is_empty() {
            return Ok(());
        }
        let entity_rows: Vec<BoltType> = entities.iter().map(entity_row).collect();
        self.graph
            .run(
                query(MERGE_ENTITIES_CYPHER)
                    .param("org_id", org_id)
                    .param("entities", entity_rows),
            )
            .await
            .context("neo4j merge entities")?;

        if !relationships.is_empty() {
            let rel_rows: Vec<BoltType> = relationships.iter().map(rel_row).collect();
            self.graph
                .run(
                    query(MERGE_RELATIONSHIPS_CYPHER)
                        .param("org_id", org_id)
                        .param("rels", rel_rows),
                )
                .await
                .context("neo4j merge relationships")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_is_idempotent_org_scoped_and_deduplicating() {
        // Every statement must be re-runnable on boot.
        assert!(
            SCHEMA_STATEMENTS
                .iter()
                .all(|s| s.contains("IF NOT EXISTS")),
            "every schema statement must be idempotent"
        );
        // MERGE de-duplication depends on the uniqueness constraint.
        assert!(
            SCHEMA_STATEMENTS
                .iter()
                .any(|s| s.contains("REQUIRE e.entity_id IS UNIQUE")),
            "entity_id uniqueness constraint is required for idempotent MERGE"
        );
        // Org scoping must be indexed on both nodes and edges.
        assert!(SCHEMA_STATEMENTS
            .iter()
            .any(|s| s.contains("(e:Entity) ON (e.org_id)")));
        assert!(SCHEMA_STATEMENTS
            .iter()
            .any(|s| s.contains("[r:REL]-() ON (r.org_id)")));
    }

    #[test]
    fn merge_cypher_is_idempotent_and_org_scoped() {
        // Idempotency: MERGE on the Postgres PKs, never CREATE.
        assert!(MERGE_ENTITIES_CYPHER.contains("MERGE (n:Entity {entity_id: e.entity_id})"));
        assert!(!MERGE_ENTITIES_CYPHER.contains("CREATE ("));
        assert!(MERGE_RELATIONSHIPS_CYPHER.contains("MERGE (a)-[e:REL {rel_id: r.rel_id}]->(b)"));
        assert!(!MERGE_RELATIONSHIPS_CYPHER.contains("CREATE ("));
        // Org scoping: every node/edge gets org_id, and edge endpoints are
        // matched within the same org so an edge can't bridge tenants.
        assert!(MERGE_ENTITIES_CYPHER.contains("n.org_id = $org_id"));
        assert!(MERGE_RELATIONSHIPS_CYPHER.contains("e.org_id = $org_id"));
        assert!(MERGE_RELATIONSHIPS_CYPHER.contains("{entity_id: r.entity_a_id, org_id: $org_id}"));
        assert!(MERGE_RELATIONSHIPS_CYPHER.contains("{entity_id: r.entity_b_id, org_id: $org_id}"));
    }

    #[test]
    fn entity_row_carries_all_fields() {
        let e = MirrorEntity {
            entity_id: "e1".into(),
            entity_type: "Person".into(),
            entity_text: "Ada".into(),
            confidence: 0.9,
        };
        match entity_row(&e) {
            BoltType::Map(m) => {
                for key in ["entity_id", "entity_type", "entity_text", "confidence"] {
                    assert!(
                        m.value.keys().any(|k| k.value == key),
                        "entity row missing key {key}"
                    );
                }
            }
            other => panic!("entity_row must produce a Bolt map, got {other:?}"),
        }
    }

    #[test]
    fn rel_row_carries_all_fields() {
        let r = MirrorRelationship {
            rel_id: "r1".into(),
            entity_a_id: "e1".into(),
            entity_b_id: "e2".into(),
            relation_type: "works_at".into(),
            confidence: 0.8,
        };
        match rel_row(&r) {
            BoltType::Map(m) => {
                for key in [
                    "rel_id",
                    "entity_a_id",
                    "entity_b_id",
                    "relation_type",
                    "confidence",
                ] {
                    assert!(
                        m.value.keys().any(|k| k.value == key),
                        "rel row missing key {key}"
                    );
                }
            }
            other => panic!("rel_row must produce a Bolt map, got {other:?}"),
        }
    }
}
