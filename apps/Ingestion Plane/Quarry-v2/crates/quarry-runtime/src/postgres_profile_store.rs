//! Postgres-backed `ProfileStore`.
//!
//! Cycle 24 / cluster #6.
//!
//! Why this exists: the existing `InMemoryProfileStore` and
//! `S3ProfileStore` cover dev + bulk archival, but the production
//! hot-path needs:
//!   1. **Restart-safe** session restore (in-memory loses everything
//!      on rolling deploy).
//!   2. **Multi-instance** consistency (S3 list is eventually-consistent
//!      — two edges can see different profile sets for tens of
//!      seconds after a write).
//!   3. **Per-tenant isolation** that's enforced by the schema, not
//!      by hopeful code paths.
//!
//! Postgres gives us all three. The schema lives at
//! `crates/quarry-runtime/migrations/0002_profiles.sql`.
//!
//! Gated behind the `postgres-queue` feature so default builds don't
//! pull `sqlx` / `libpq`.

#![cfg(feature = "postgres-queue")]

use async_trait::async_trait;
use sqlx::postgres::PgPool;
use sqlx::Row;

use quarry_browser::session::{ProfileStore, SessionSnapshot};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::ProfileKind;

fn db_err(label: &str, e: sqlx::Error) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("postgres {label}: {e}"))
}

/// Durable, multi-instance-safe `ProfileStore`.
///
/// All methods take an explicit `org_id` so the schema's PK
/// `(org_id, profile_id)` is hit on every operation. The verified JWT
/// org from the edge handler is the only value ever passed in
/// production (P0 / cluster #auth+tenancy enforces this upstream).
#[derive(Clone)]
pub struct PostgresProfileStore {
    pool: PgPool,
}

impl PostgresProfileStore {
    /// Construct from an existing pool. Migrations are applied
    /// idempotently by the surrounding runtime — we don't run them
    /// inline so multiple components sharing the pool don't race on
    /// the migration table.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ProfileStore for PostgresProfileStore {
    async fn save(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
        snapshot: &SessionSnapshot,
    ) -> QuarryResult<()> {
        let json = serde_json::to_value(snapshot).map_err(|e| {
            QuarryError::new(ErrorCode::Internal, format!("snapshot encode failed: {e}"))
        })?;
        sqlx::query(
            "INSERT INTO quarry_profiles (org_id, profile_id, snapshot, created_at, updated_at) \
             VALUES ($1, $2, $3, NOW(), NOW()) \
             ON CONFLICT (org_id, profile_id) DO UPDATE SET \
                 snapshot   = EXCLUDED.snapshot, \
                 updated_at = NOW()",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .bind(&json)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("save", e))?;
        Ok(())
    }

    async fn load(
        &self,
        org_id: &str,
        profile_id: &ProfileKind,
    ) -> QuarryResult<Option<SessionSnapshot>> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT snapshot FROM quarry_profiles \
             WHERE org_id = $1 AND profile_id = $2",
        )
        .bind(org_id)
        .bind(profile_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("load", e))?;

        match row {
            None => Ok(None),
            Some((json,)) => {
                let snap: SessionSnapshot = serde_json::from_value(json).map_err(|e| {
                    QuarryError::new(ErrorCode::Internal, format!("snapshot decode failed: {e}"))
                })?;
                Ok(Some(snap))
            }
        }
    }

    async fn delete(&self, org_id: &str, profile_id: &ProfileKind) -> QuarryResult<()> {
        sqlx::query("DELETE FROM quarry_profiles WHERE org_id = $1 AND profile_id = $2")
            .bind(org_id)
            .bind(profile_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| db_err("delete", e))?;
        Ok(())
    }

    async fn list(&self, org_id: &str) -> QuarryResult<Vec<ProfileKind>> {
        // ORDER BY updated_at DESC uses the partial index defined in
        // 0002_profiles.sql — single index seek + range scan, no full
        // table walk even on a multi-tenant DB.
        let rows = sqlx::query(
            "SELECT profile_id FROM quarry_profiles \
             WHERE org_id = $1 \
             ORDER BY updated_at DESC",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_err("list", e))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let s: String = row.try_get(0).map_err(|e| db_err("list:row", e))?;
            if let Ok(id) = s.parse::<ProfileKind>() {
                out.push(id);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gracefully skip when no DATABASE_URL is set — default CI stays
    /// green without a Postgres dependency. Local dev pointing at a
    /// disposable Postgres exercises the real wire.
    fn database_url() -> Option<String> {
        std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())
    }

    async fn fresh_pool() -> Option<PgPool> {
        let url = database_url()?;
        let pool = PgPool::connect(&url).await.ok()?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply");
        Some(pool)
    }

    async fn clean(pool: &PgPool) {
        let _ = sqlx::query("TRUNCATE TABLE quarry_profiles")
            .execute(pool)
            .await;
    }

    fn snap() -> SessionSnapshot {
        SessionSnapshot {
            cookies: vec![],
            local_storage: vec![("k".into(), "v".into())],
            session_storage: vec![],
            indexed_db: vec![],
            user_agent: Some("test/1.0".into()),
            viewport: None,
            locale: Some("en-US".into()),
            timezone: None,
        }
    }

    #[tokio::test]
    async fn save_load_roundtrip() {
        let Some(pool) = fresh_pool().await else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();
        store.save("org_a", &id, &snap()).await.unwrap();

        let loaded = store.load("org_a", &id).await.unwrap().unwrap();
        assert_eq!(loaded.user_agent.as_deref(), Some("test/1.0"));
        assert_eq!(loaded.local_storage.len(), 1);
    }

    #[tokio::test]
    async fn save_is_upsert_on_repeated_id() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();

        store.save("org_a", &id, &snap()).await.unwrap();
        // Second save with different content should overwrite, not insert.
        let mut other = snap();
        other.user_agent = Some("test/2.0".into());
        store.save("org_a", &id, &other).await.unwrap();

        let loaded = store.load("org_a", &id).await.unwrap().unwrap();
        assert_eq!(loaded.user_agent.as_deref(), Some("test/2.0"));
        // list MUST return exactly 1 row (upsert, not duplicate).
        let ids = store.list("org_a").await.unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[tokio::test]
    async fn tenant_isolation_cross_org_load_returns_none() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);
        let id: ProfileKind = quarry_core::ids::Id::new();

        store.save("org_a", &id, &snap()).await.unwrap();

        // The same profile_id under a DIFFERENT org MUST be invisible
        // (it's actually inaccessible because the PK is composite).
        let none = store.load("org_b", &id).await.unwrap();
        assert!(none.is_none(), "cross-org load leaked");

        let none_list = store.list("org_b").await.unwrap();
        assert!(none_list.is_empty(), "cross-org list leaked");

        // Cross-org delete must not affect org_a's data.
        store.delete("org_b", &id).await.unwrap();
        let still_there = store.load("org_a", &id).await.unwrap();
        assert!(still_there.is_some(), "cross-org delete corrupted org_a");
    }

    #[tokio::test]
    async fn list_orders_by_recent_update() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresProfileStore::new(pool);

        let a: ProfileKind = quarry_core::ids::Id::new();
        let b: ProfileKind = quarry_core::ids::Id::new();
        store.save("org_a", &a, &snap()).await.unwrap();
        // Tick to ensure distinct updated_at values.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        store.save("org_a", &b, &snap()).await.unwrap();

        let ids = store.list("org_a").await.unwrap();
        assert_eq!(ids.len(), 2);
        // Most-recent first → b before a.
        assert_eq!(ids[0].to_string(), b.to_string());
        assert_eq!(ids[1].to_string(), a.to_string());
    }
}
