//! §16.1.4 — per-(org, agent) retrieval configuration lookup.
//!
//! Model Plane sends `agent_id` in the request (or `X-Agent-Retrieval-Config`
//! header). The engine looks up the row in `agent_retrieval_configs`. If
//! none exists, weights fall back to the org default which falls back to
//! the global config defaults.
//!
//! Reads are hot, so we wrap them in a `moka` future cache with a 60s TTL.
//! Mutations to the table should bump that org's version (§16.2.2) so
//! callers see fresh config within one read-side cache window.

use std::time::Duration;

use moka::future::Cache;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentRetrievalConfig {
    pub weights: serde_json::Value,
    pub rerank: bool,
}

static CACHE: Lazy<Cache<(String, String), Option<AgentRetrievalConfig>>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(10_000)
        .time_to_live(Duration::from_secs(60))
        .build()
});

/// Returns the row for `(org_id, agent_id)` or `None` if no override
/// exists. Postgres failures degrade to `None` so the engine just uses
/// whatever the caller's defaults are.
pub async fn lookup(pool: &PgPool, org_id: &str, agent_id: &str) -> Option<AgentRetrievalConfig> {
    let key = (org_id.to_string(), agent_id.to_string());
    if let Some(cached) = CACHE.get(&key).await {
        return cached;
    }
    // Phase 1 RLS: config is per-(org, agent) and the org comes from the
    // verified caller claims, so the lookup reads through an org-scoped
    // transaction. The SQL still binds `org_id` itself — the database policy
    // is a backstop, not a replacement. Error handling is unchanged: anything
    // that fails here (including opening the scoped transaction) still
    // degrades to `None` so the caller falls back to its own defaults.
    let row = async {
        let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
        let row: Option<(serde_json::Value, bool)> = sqlx::query_as(
            "SELECT weights, rerank FROM agent_retrieval_configs
             WHERE org_id = $1 AND agent_id = $2",
        )
        .bind(org_id)
        .bind(agent_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<_, anyhow::Error>(row)
    }
    .await;

    let val = match row {
        Ok(Some((weights, rerank))) => Some(AgentRetrievalConfig { weights, rerank }),
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(error = %e, org_id, agent_id, "agent_retrieval_configs lookup failed");
            None
        }
    };
    CACHE.insert(key, val.clone()).await;
    val
}
