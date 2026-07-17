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

use anyhow::Context;
use neo4rs::{query, ConfigBuilder, Graph};

use crate::config::Config;

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
}
