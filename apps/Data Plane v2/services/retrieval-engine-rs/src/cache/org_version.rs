//! §16.2.2 — Per-org cache version counter.
//!
//! TTL-based invalidation has a 5-minute stale window: doc updated →
//! NATS event sent → in-flight retrieval already read stale cache.
//! Versioning eliminates that: every cache key embeds the org's current
//! version. The documents-api `bump_org_version(org_id)` outbox call
//! makes a mutation immediately invalidate the org's cache without
//! touching the cache layer.
//!
//! Reads are hot, so we wrap Postgres lookups in a `moka` future cache
//! with a short TTL (30s) — stale-by-30s is acceptable because the
//! whole point is that mutations bump the value, not that reads see
//! the bump in <30s.

use std::time::Duration;

use moka::future::Cache;
use once_cell::sync::Lazy;
use sqlx::PgPool;

static CACHE: Lazy<Cache<String, i64>> = Lazy::new(|| {
    Cache::builder()
        .max_capacity(10_000)
        .time_to_live(Duration::from_secs(30))
        .build()
});

/// Fetch the current org_version, lazily inserting a row (version=1) if
/// the org has never been written. Returns 1 if Postgres is unreachable
/// — degrade to "no versioning" rather than fail the request.
pub async fn current(pool: &PgPool, org_id: &str) -> i64 {
    if let Some(v) = CACHE.get(org_id).await {
        return v;
    }
    // Phase 1 RLS: the counter is per-org and the org comes from the verified
    // caller claims, so the read/lazy-insert runs in an org-scoped transaction.
    // Error handling is unchanged: anything that fails here (including opening
    // the scoped transaction) still degrades to version 1, i.e. "no versioning",
    // rather than failing the request.
    let row = async {
        let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
        let row: (i64,) = sqlx::query_as(
            r#"
        INSERT INTO org_versions (org_id) VALUES ($1)
        ON CONFLICT (org_id) DO UPDATE SET org_id = EXCLUDED.org_id
        RETURNING version
        "#,
        )
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<_, anyhow::Error>(row)
    }
    .await;
    let v = row.map(|r| r.0).unwrap_or(1);
    CACHE.insert(org_id.to_string(), v).await;
    v
}

/// Bump the org's version. Returns the new value. Best-effort: a failure
/// here logs and returns 0 — the caller's primary write should not fail.
pub async fn bump(pool: &PgPool, org_id: &str) -> i64 {
    // Phase 1 RLS: single-org write, same rationale as `current` above. Best
    // effort is preserved exactly — a failure (including opening the scoped
    // transaction) logs and returns 0 so the caller's primary write still
    // succeeds.
    let row = async {
        let mut tx = pg_org_scope::begin_org_scoped(pool, org_id).await?;
        let row: (i64,) = sqlx::query_as(
            r#"
        INSERT INTO org_versions (org_id, version, bumped_at)
            VALUES ($1, 2, NOW())
        ON CONFLICT (org_id) DO UPDATE
            SET version = org_versions.version + 1,
                bumped_at = NOW()
        RETURNING version
        "#,
        )
        .bind(org_id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok::<_, anyhow::Error>(row)
    }
    .await;
    match row {
        Ok((v,)) => {
            CACHE.insert(org_id.to_string(), v).await;
            v
        }
        Err(e) => {
            tracing::warn!(error = %e, org_id, "org_version bump failed");
            0
        }
    }
}
