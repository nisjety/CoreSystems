//! Postgres-backed baseline + diff store for change tracking.
//!
//! Cycle 30 / cluster #9 deferral from cycle 26.
//!
//! Wire shapes in `quarry_core::change_history`. Schema in
//! `migrations/0004_baselines.sql`.

#![cfg(feature = "postgres-queue")]

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;

use quarry_core::change_history::{BaselineSnapshot, ChangeRecord, ChangeStatus, DiffRecord};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::ArtifactKind;

fn db_err(label: &str, e: sqlx::Error) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("postgres {label}: {e}"))
}

/// Durable baseline chain + diff store. Methods are all org-scoped
/// at the SQL level; the trait doesn't take `org_id` as a constructor
/// param because the same instance serves every tenant in production
/// — the JWT claim flows through the handler.
#[derive(Clone)]
pub struct PostgresBaselineStore {
    pool: PgPool,
}

impl PostgresBaselineStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Persist a new baseline. `prev_baseline_id` should be the
    /// most-recent baseline for `(org_id, source_url)` to maintain
    /// the singly-linked chain.
    pub async fn save_baseline(&self, b: &BaselineSnapshot) -> QuarryResult<()> {
        sqlx::query(
            "INSERT INTO quarry_baselines \
             (baseline_id, org_id, source_url, fingerprint, artifact_id, \
              prev_baseline_id, run_id, captured_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             ON CONFLICT (baseline_id) DO NOTHING",
        )
        .bind(&b.baseline_id)
        .bind(&b.org_id)
        .bind(&b.source_url)
        .bind(&b.fingerprint)
        .bind(b.artifact_id.as_ref().map(|a| a.to_string()))
        .bind(b.prev_baseline_id.as_deref())
        .bind(b.run_id.as_ref().map(|r| r.to_string()))
        .bind(b.captured_at)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("save_baseline", e))?;
        Ok(())
    }

    /// Latest baseline for a URL. Returns `None` if Quarry has never
    /// captured this URL for the org.
    pub async fn load_latest(
        &self,
        org_id: &str,
        source_url: &str,
    ) -> QuarryResult<Option<BaselineSnapshot>> {
        let row = sqlx::query(
            "SELECT baseline_id, org_id, source_url, fingerprint, artifact_id, \
                    prev_baseline_id, run_id, captured_at \
             FROM quarry_baselines \
             WHERE org_id = $1 AND source_url = $2 \
             ORDER BY captured_at DESC \
             LIMIT 1",
        )
        .bind(org_id)
        .bind(source_url)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("load_latest", e))?;
        row.map(row_to_baseline).transpose()
    }

    /// Walks the chain newest-first up to `limit` entries.
    pub async fn load_history(
        &self,
        org_id: &str,
        source_url: &str,
        limit: u32,
    ) -> QuarryResult<Vec<BaselineSnapshot>> {
        let lim = limit.clamp(1, 1_000) as i64;
        let rows = sqlx::query(
            "SELECT baseline_id, org_id, source_url, fingerprint, artifact_id, \
                    prev_baseline_id, run_id, captured_at \
             FROM quarry_baselines \
             WHERE org_id = $1 AND source_url = $2 \
             ORDER BY captured_at DESC \
             LIMIT $3",
        )
        .bind(org_id)
        .bind(source_url)
        .bind(lim)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_err("load_history", e))?;
        rows.into_iter().map(row_to_baseline).collect()
    }

    /// Compares the fresh fingerprint to the latest baseline, returns
    /// a `ChangeRecord`. Does NOT persist the new baseline — that's
    /// the caller's choice (they may want to skip persistence for
    /// unchanged pages to save storage).
    pub async fn compare_snapshot(
        &self,
        org_id: &str,
        source_url: &str,
        fresh_fingerprint: &str,
    ) -> QuarryResult<ChangeRecord> {
        let prev = self.load_latest(org_id, source_url).await?;
        let now = Utc::now();
        let status = match prev.as_ref() {
            None => ChangeStatus::New,
            Some(p) if p.fingerprint == fresh_fingerprint => ChangeStatus::Unchanged,
            Some(_) => ChangeStatus::Changed,
        };
        Ok(ChangeRecord {
            source_url: source_url.to_string(),
            org_id: org_id.to_string(),
            status,
            new_baseline: None, // caller fills in after save
            prev_baseline: prev,
            diff_id: None,
            checked_at: now,
        })
    }

    /// Persist a computed diff between two baselines.
    pub async fn create_diff_record(&self, d: &DiffRecord) -> QuarryResult<()> {
        sqlx::query(
            "INSERT INTO quarry_change_diffs \
             (diff_id, org_id, from_baseline_id, to_baseline_id, source_url, \
              format, artifact_id, summary, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
             ON CONFLICT (diff_id) DO NOTHING",
        )
        .bind(&d.diff_id)
        .bind(&d.org_id)
        .bind(&d.from_baseline_id)
        .bind(&d.to_baseline_id)
        .bind(&d.source_url)
        .bind(&d.format)
        .bind(d.artifact_id.to_string())
        .bind(d.summary.as_deref())
        .bind(d.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("create_diff", e))?;
        Ok(())
    }
}

fn row_to_baseline(row: sqlx::postgres::PgRow) -> QuarryResult<BaselineSnapshot> {
    let artifact_id_s: Option<String> = row.try_get("artifact_id").ok();
    let artifact_id = artifact_id_s
        .as_deref()
        .and_then(|s| s.parse::<ArtifactKind>().ok());
    let run_id_s: Option<String> = row.try_get("run_id").ok();
    let run_id = run_id_s.as_deref().and_then(|s| s.parse().ok());
    Ok(BaselineSnapshot {
        baseline_id: row
            .try_get::<String, _>("baseline_id")
            .map_err(|e| db_err("row.baseline_id", e))?,
        org_id: row
            .try_get::<String, _>("org_id")
            .map_err(|e| db_err("row.org_id", e))?,
        source_url: row
            .try_get::<String, _>("source_url")
            .map_err(|e| db_err("row.source_url", e))?,
        fingerprint: row
            .try_get::<String, _>("fingerprint")
            .map_err(|e| db_err("row.fingerprint", e))?,
        artifact_id,
        prev_baseline_id: row.try_get("prev_baseline_id").ok(),
        run_id,
        captured_at: row
            .try_get::<DateTime<Utc>, _>("captured_at")
            .map_err(|e| db_err("row.captured_at", e))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database_url() -> Option<String> {
        std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())
    }

    async fn fresh_pool() -> Option<PgPool> {
        let url = database_url()?;
        let pool = PgPool::connect(&url).await.ok()?;
        sqlx::migrate!("./migrations").run(&pool).await.ok();
        Some(pool)
    }

    async fn clean(pool: &PgPool) {
        let _ = sqlx::query("TRUNCATE TABLE quarry_change_diffs, quarry_baselines CASCADE")
            .execute(pool)
            .await;
    }

    fn baseline(org: &str, url: &str, fp: &str, prev: Option<&str>) -> BaselineSnapshot {
        BaselineSnapshot {
            baseline_id: format!("bln_{}", quarry_core::ids::kinds::ArtifactKind::new()),
            org_id: org.into(),
            source_url: url.into(),
            fingerprint: fp.into(),
            artifact_id: None,
            prev_baseline_id: prev.map(String::from),
            run_id: None,
            captured_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn save_then_load_latest_returns_most_recent() {
        let Some(pool) = fresh_pool().await else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };
        clean(&pool).await;
        let store = PostgresBaselineStore::new(pool);

        let first = baseline("org_a", "https://x/", "blake3:a", None);
        store.save_baseline(&first).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        let second = baseline("org_a", "https://x/", "blake3:b", Some(&first.baseline_id));
        store.save_baseline(&second).await.unwrap();

        let latest = store
            .load_latest("org_a", "https://x/")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.fingerprint, "blake3:b");
        assert_eq!(
            latest.prev_baseline_id.as_deref(),
            Some(first.baseline_id.as_str())
        );
    }

    #[tokio::test]
    async fn compare_snapshot_returns_correct_status() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresBaselineStore::new(pool);

        // No baseline yet → New.
        let r = store
            .compare_snapshot("org_a", "https://x/", "fp_new")
            .await
            .unwrap();
        assert_eq!(r.status, ChangeStatus::New);
        assert!(r.prev_baseline.is_none());

        // Save + compare with matching fingerprint → Unchanged.
        let b = baseline("org_a", "https://x/", "fp_v1", None);
        store.save_baseline(&b).await.unwrap();
        let r = store
            .compare_snapshot("org_a", "https://x/", "fp_v1")
            .await
            .unwrap();
        assert_eq!(r.status, ChangeStatus::Unchanged);

        // Different fingerprint → Changed.
        let r = store
            .compare_snapshot("org_a", "https://x/", "fp_v2")
            .await
            .unwrap();
        assert_eq!(r.status, ChangeStatus::Changed);
    }

    #[tokio::test]
    async fn tenant_isolation_cross_org_load_returns_none() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresBaselineStore::new(pool);
        let b = baseline("org_a", "https://x/", "fp", None);
        store.save_baseline(&b).await.unwrap();

        // Same URL, different org → must NOT see it.
        let none = store.load_latest("org_b", "https://x/").await.unwrap();
        assert!(none.is_none(), "cross-org load leaked");
    }

    #[tokio::test]
    async fn load_history_orders_newest_first() {
        let Some(pool) = fresh_pool().await else {
            return;
        };
        clean(&pool).await;
        let store = PostgresBaselineStore::new(pool);

        for i in 0..3 {
            let b = baseline("org_a", "https://x/", &format!("fp_{i}"), None);
            store.save_baseline(&b).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        }

        let hist = store.load_history("org_a", "https://x/", 10).await.unwrap();
        assert_eq!(hist.len(), 3);
        // Newest-first = fp_2 → fp_1 → fp_0.
        assert_eq!(hist[0].fingerprint, "fp_2");
        assert_eq!(hist[2].fingerprint, "fp_0");
    }
}
