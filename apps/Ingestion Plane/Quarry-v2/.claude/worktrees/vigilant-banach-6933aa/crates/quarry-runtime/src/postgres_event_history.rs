//! Postgres-backed durable job-history event log.
//!
//! Cycle 24 / cluster #7.
//!
//! Persists `JobHistoryEvent` rows so the frontend stops simulating
//! phases — a reconnected client can fetch the full event sequence
//! for a run via `list_events` / `replay_window`.
//!
//! Schema: `crates/quarry-runtime/migrations/0003_job_history.sql`.

#![cfg(feature = "postgres-queue")]

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::ids::kinds::RunKind;
use quarry_core::job_history::{JobHistoryEvent, JobStage, JobStatus};
use quarry_core::resources::JobResourceKind;

fn db_err(label: &str, e: sqlx::Error) -> QuarryError {
    QuarryError::new(ErrorCode::Internal, format!("postgres {label}: {e}"))
}

/// Append-only durable event log for runs. One row per
/// `JobHistoryEvent`; `(run_id, seq)` is the primary key so a
/// duplicate emit by a buggy producer fails fast instead of
/// corrupting the per-run total order.
#[derive(Clone)]
pub struct PostgresEventHistory {
    pool: PgPool,
}

impl PostgresEventHistory {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Append one event. Producers MUST assign monotonically
    /// increasing `seq` per `run_id`; duplicate `(run_id, seq)`
    /// returns a Conflict error so the caller can detect the bug.
    pub async fn record(&self, event: &JobHistoryEvent) -> QuarryResult<()> {
        let result = sqlx::query(
            "INSERT INTO quarry_job_history \
             (event_id, run_id, org_id, kind, stage, status, seq, \
              completed, total, discovered, queued, retries, blocks, eta, ts, payload) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)",
        )
        .bind(event_id_for(event))
        .bind(event.run_id.to_string())
        .bind(&event.org_id)
        .bind(event.kind.as_str())
        .bind(event.stage.as_str())
        .bind(status_str(event.status))
        .bind(event.seq as i64)
        .bind(event.completed as i32)
        .bind(event.total.map(|n| n as i32))
        .bind(event.discovered as i32)
        .bind(event.queued as i32)
        .bind(event.retries as i32)
        .bind(event.blocks as i32)
        .bind(event.eta)
        .bind(event.timestamp)
        .bind(&event.payload)
        .execute(&self.pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(sqlx::Error::Database(db)) if db.code().as_deref() == Some("23505") => {
                // Unique violation on (run_id, seq) — the producer
                // emitted a duplicate. Surface as a typed error.
                Err(QuarryError::new(
                    ErrorCode::BadRequest,
                    format!(
                        "duplicate event: run_id={} seq={} (producer bug)",
                        event.run_id, event.seq
                    ),
                ))
            }
            Err(e) => Err(db_err("record", e)),
        }
    }

    /// List events for one run in sequence order. `limit` clamps to
    /// 1000 to bound response size.
    pub async fn list_events(
        &self,
        org_id: &str,
        run_id: &RunKind,
        limit: u32,
    ) -> QuarryResult<Vec<JobHistoryEvent>> {
        let lim = limit.clamp(1, 1000) as i64;
        let rows = sqlx::query(
            "SELECT event_id, run_id, org_id, kind, stage, status, seq, \
                    completed, total, discovered, queued, retries, blocks, eta, ts, payload \
             FROM quarry_job_history \
             WHERE org_id = $1 AND run_id = $2 \
             ORDER BY seq ASC \
             LIMIT $3",
        )
        .bind(org_id)
        .bind(run_id.to_string())
        .bind(lim)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_err("list_events", e))?;

        rows.into_iter().map(row_to_event).collect()
    }

    /// Replay every event between `from_ts` and `to_ts` for an org,
    /// across all runs. Used for org-wide audit replays and dashboard
    /// rebuilds. Bounded by `limit` (capped at 5000).
    pub async fn replay_window(
        &self,
        org_id: &str,
        from_ts: DateTime<Utc>,
        to_ts: DateTime<Utc>,
        limit: u32,
    ) -> QuarryResult<Vec<JobHistoryEvent>> {
        if to_ts <= from_ts {
            return Err(QuarryError::new(
                ErrorCode::BadRequest,
                "to_ts must be strictly after from_ts",
            ));
        }
        let lim = limit.clamp(1, 5000) as i64;
        let rows = sqlx::query(
            "SELECT event_id, run_id, org_id, kind, stage, status, seq, \
                    completed, total, discovered, queued, retries, blocks, eta, ts, payload \
             FROM quarry_job_history \
             WHERE org_id = $1 AND ts >= $2 AND ts <= $3 \
             ORDER BY ts ASC \
             LIMIT $4",
        )
        .bind(org_id)
        .bind(from_ts)
        .bind(to_ts)
        .bind(lim)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| db_err("replay_window", e))?;

        rows.into_iter().map(row_to_event).collect()
    }
}

fn status_str(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Ok => "ok",
        JobStatus::Warn => "warn",
        JobStatus::Error => "error",
    }
}

fn parse_status(s: &str) -> JobStatus {
    match s {
        "warn" => JobStatus::Warn,
        "error" => JobStatus::Error,
        _ => JobStatus::Ok,
    }
}

fn parse_stage(s: &str) -> JobStage {
    match s {
        "queued" => JobStage::Queued,
        "starting" => JobStage::Starting,
        "running" => JobStage::Running,
        "finalizing" => JobStage::Finalizing,
        "completed" => JobStage::Completed,
        "failed" => JobStage::Failed,
        "cancelled" => JobStage::Cancelled,
        // Unknown stage = future-version row we can't represent;
        // default to Running so dashboards render something useful
        // rather than 500.
        _ => JobStage::Running,
    }
}

fn parse_kind(s: &str) -> JobResourceKind {
    JobResourceKind::from_path_segment(s).unwrap_or(JobResourceKind::Crawl)
}

fn row_to_event(row: sqlx::postgres::PgRow) -> QuarryResult<JobHistoryEvent> {
    let run_id_s: String = row.try_get("run_id").map_err(|e| db_err("row.run_id", e))?;
    let run_id: RunKind = run_id_s.parse().map_err(|e: quarry_core::error::QuarryError| {
        QuarryError::new(ErrorCode::Internal, format!("run_id parse: {e}"))
    })?;
    let total: Option<i32> = row.try_get("total").ok();
    Ok(JobHistoryEvent {
        run_id,
        org_id: row.try_get("org_id").map_err(|e| db_err("row.org_id", e))?,
        kind: parse_kind(
            row.try_get::<String, _>("kind")
                .map_err(|e| db_err("row.kind", e))?
                .as_str(),
        ),
        stage: parse_stage(
            row.try_get::<String, _>("stage")
                .map_err(|e| db_err("row.stage", e))?
                .as_str(),
        ),
        status: parse_status(
            row.try_get::<String, _>("status")
                .map_err(|e| db_err("row.status", e))?
                .as_str(),
        ),
        seq: row
            .try_get::<i64, _>("seq")
            .map_err(|e| db_err("row.seq", e))? as u64,
        completed: row
            .try_get::<i32, _>("completed")
            .map_err(|e| db_err("row.completed", e))? as u32,
        total: total.map(|n| n as u32),
        discovered: row
            .try_get::<i32, _>("discovered")
            .map_err(|e| db_err("row.discovered", e))? as u32,
        queued: row
            .try_get::<i32, _>("queued")
            .map_err(|e| db_err("row.queued", e))? as u32,
        retries: row
            .try_get::<i32, _>("retries")
            .map_err(|e| db_err("row.retries", e))? as u32,
        blocks: row
            .try_get::<i32, _>("blocks")
            .map_err(|e| db_err("row.blocks", e))? as u32,
        eta: row.try_get("eta").ok(),
        timestamp: row.try_get("ts").map_err(|e| db_err("row.ts", e))?,
        payload: row
            .try_get::<serde_json::Value, _>("payload")
            .unwrap_or(serde_json::Value::Null),
    })
}

/// Derive a deterministic `event_id` from `(run_id, seq)`. Two
/// records of the same event produce the same id — the `(run_id,
/// seq)` PK on `quarry_job_history` would catch the duplicate anyway,
/// but having an explicit id is friendlier for log correlation.
///
/// Free function (not an inherent impl) because `JobHistoryEvent`
/// lives in `quarry-core` and Rust's orphan rules forbid adding
/// methods to it from `quarry-runtime`.
fn event_id_for(event: &JobHistoryEvent) -> String {
    format!("evt:{}:{}", event.run_id, event.seq)
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
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply");
        Some(pool)
    }

    async fn clean(pool: &PgPool) {
        let _ = sqlx::query("TRUNCATE TABLE quarry_job_history")
            .execute(pool)
            .await;
    }

    #[tokio::test]
    async fn record_and_list_preserves_order() {
        let Some(pool) = fresh_pool().await else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };
        clean(&pool).await;
        let store = PostgresEventHistory::new(pool);
        let run_id: RunKind = quarry_core::ids::Id::new();

        for seq in 1..=5u64 {
            let mut e = JobHistoryEvent::new(
                run_id.clone(),
                "org_a",
                JobResourceKind::Crawl,
                JobStage::Running,
                JobStatus::Ok,
                seq,
            );
            e.completed = seq as u32 * 2;
            store.record(&e).await.unwrap();
        }

        let events = store.list_events("org_a", &run_id, 100).await.unwrap();
        assert_eq!(events.len(), 5);
        // Order is by seq ascending.
        for (i, e) in events.iter().enumerate() {
            assert_eq!(e.seq, (i as u64) + 1);
            assert_eq!(e.completed, ((i as u32) + 1) * 2);
        }
    }

    #[tokio::test]
    async fn duplicate_seq_returns_bad_request() {
        let Some(pool) = fresh_pool().await else { return; };
        clean(&pool).await;
        let store = PostgresEventHistory::new(pool);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let e = JobHistoryEvent::new(
            run_id.clone(),
            "org_a",
            JobResourceKind::Crawl,
            JobStage::Running,
            JobStatus::Ok,
            1,
        );
        store.record(&e).await.unwrap();
        // Second record with same seq must fail.
        let err = store.record(&e).await.unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }

    #[tokio::test]
    async fn tenant_isolation_cross_org_list_returns_empty() {
        let Some(pool) = fresh_pool().await else { return; };
        clean(&pool).await;
        let store = PostgresEventHistory::new(pool);
        let run_id: RunKind = quarry_core::ids::Id::new();
        let e = JobHistoryEvent::new(
            run_id.clone(),
            "org_a",
            JobResourceKind::Crawl,
            JobStage::Running,
            JobStatus::Ok,
            1,
        );
        store.record(&e).await.unwrap();

        // org_b querying the same run_id MUST get an empty list.
        let cross = store.list_events("org_b", &run_id, 100).await.unwrap();
        assert!(cross.is_empty(), "cross-org list leaked");
    }

    #[tokio::test]
    async fn replay_window_filters_by_timestamp() {
        let Some(pool) = fresh_pool().await else { return; };
        clean(&pool).await;
        let store = PostgresEventHistory::new(pool);

        let earlier = Utc::now() - chrono::Duration::hours(2);
        let later = Utc::now();

        let mut e_early = JobHistoryEvent::new(
            quarry_core::ids::Id::new(),
            "org_a",
            JobResourceKind::Crawl,
            JobStage::Running,
            JobStatus::Ok,
            1,
        );
        e_early.timestamp = earlier - chrono::Duration::minutes(5);
        store.record(&e_early).await.unwrap();

        let mut e_mid = JobHistoryEvent::new(
            quarry_core::ids::Id::new(),
            "org_a",
            JobResourceKind::Crawl,
            JobStage::Running,
            JobStatus::Ok,
            1,
        );
        e_mid.timestamp = Utc::now() - chrono::Duration::minutes(30);
        store.record(&e_mid).await.unwrap();

        // Replay only the last hour.
        let window = store
            .replay_window(
                "org_a",
                Utc::now() - chrono::Duration::hours(1),
                Utc::now(),
                100,
            )
            .await
            .unwrap();
        assert_eq!(window.len(), 1, "only mid should match the window");
    }

    #[tokio::test]
    async fn replay_window_rejects_inverted_range() {
        let Some(pool) = fresh_pool().await else { return; };
        let store = PostgresEventHistory::new(pool);
        let now = Utc::now();
        let err = store
            .replay_window(
                "org_a",
                now,
                now - chrono::Duration::hours(1),
                100,
            )
            .await
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
