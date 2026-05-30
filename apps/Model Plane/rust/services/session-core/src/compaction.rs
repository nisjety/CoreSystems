//! Periodic checkpoint compaction loop.
//!
//! For each run that has new `STEP_COMPLETED`/`ACTION_COMPLETED` events beyond
//! the last checkpoint, insert a synthesized checkpoint. The `ordinal` column
//! is filled by the BEFORE INSERT trigger `trg_checkpoints_ordinal` (MAX+1 per
//! `run_id`), so we omit it from the INSERT.

use crate::store::Pool;
use anyhow::Result;
use metrics::{counter, histogram};
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub async fn run(pool: Pool) -> Result<()> {
    let interval_secs = std::env::var("COMPACTION_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(60u64);
    let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!(interval_secs, "compaction loop started");

    loop {
        tick.tick().await;
        let start = Instant::now();
        match compact_once(&pool).await {
            Ok(n) => {
                counter!("mp_session_compaction_runs_total", "status" => "ok").increment(1);
                histogram!("mp_session_compaction_duration_seconds")
                    .record(start.elapsed().as_secs_f64());
                if n > 0 {
                    info!(rolled = n, "compaction cycle complete");
                }
            }
            Err(e) => {
                counter!("mp_session_compaction_runs_total", "status" => "error").increment(1);
                histogram!("mp_session_compaction_duration_seconds")
                    .record(start.elapsed().as_secs_f64());
                warn!(error = %e, "compaction cycle failed");
            }
        }
    }
}

pub async fn compact_once(pool: &Pool) -> Result<i64> {
    let n = sqlx::query_scalar::<_, i64>(
        r"
        WITH candidates AS (
            SELECT e.run_id, MAX(e.step_ordinal) AS max_step
            FROM events e
            WHERE e.event_type IN ('STEP_COMPLETED', 'ACTION_COMPLETED')
              AND e.step_ordinal IS NOT NULL
              AND e.run_id IS NOT NULL
            GROUP BY e.run_id
        ),
        last_ck AS (
            SELECT DISTINCT ON (run_id) run_id, created_at
            FROM checkpoints
            ORDER BY run_id, ordinal DESC NULLS LAST
        ),
        need AS (
            SELECT c.run_id, c.max_step
            FROM candidates c
            LEFT JOIN last_ck lc ON lc.run_id = c.run_id
            WHERE lc.created_at IS NULL
               OR EXISTS (
                   SELECT 1 FROM events e2
                   WHERE e2.run_id = c.run_id
                     AND e2.ts > lc.created_at
                     AND e2.event_type IN ('STEP_COMPLETED', 'ACTION_COMPLETED')
               )
        ),
        ins AS (
            INSERT INTO checkpoints (id, run_id, state)
            SELECT gen_random_uuid()::text,
                   n.run_id,
                   convert_to(jsonb_build_object('max_step', n.max_step)::text, 'UTF8')
            FROM need n
            RETURNING 1
        )
        SELECT COUNT(*)::BIGINT FROM ins
        ",
    )
    .fetch_one(pool)
    .await?;
    Ok(n)
}
