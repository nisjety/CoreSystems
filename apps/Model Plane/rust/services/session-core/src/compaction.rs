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

#[cfg(test)]
mod tests {
    use super::*;

    // PG-gated regression guard for the GOAL invariant "compaction never
    // replaces source": `compact_once` must only ADD a checkpoint and leave
    // every source event intact. The query is INSERT-only today (provable by
    // reading it), so this test LOCKS that in — a future change that adds an
    // event cleanup (e.g. "prune events older than the last checkpoint") would
    // fail here, catching the data-loss regression before it ships.
    //
    // #[ignore]d so plain `cargo test` (no DB) skips it; run with a DB:
    //   DATABASE_URL=… cargo test -p session-core --bin session-core \
    //     compaction_is_additive_never_deletes_events -- --ignored
    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a Postgres with session-core migrations"]
    async fn compaction_is_additive_never_deletes_events() {
        let Ok(url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL unset");
            return;
        };
        let pool = sqlx::PgPool::connect(&url).await.expect("connect pg");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrate");

        let sfx = std::process::id();
        let (thread_id, run_id, org) = (
            format!("ct-t-{sfx}"),
            format!("ct-r-{sfx}"),
            format!("ct-org-{sfx}"),
        );
        sqlx::query("INSERT INTO threads (id,session_key,org_id,user_id) VALUES ($1,$1,$2,'u1')")
            .bind(&thread_id)
            .bind(&org)
            .execute(&pool)
            .await
            .expect("seed thread");
        sqlx::query(
            "INSERT INTO runs (id,thread_id,goal,org_id,user_id) VALUES ($1,$2,'g',$3,'u1')",
        )
        .bind(&run_id)
        .bind(&thread_id)
        .bind(&org)
        .execute(&pool)
        .await
        .expect("seed run");

        // Seed source events; the trigger assigns step_ordinal for STEP_COMPLETED.
        for i in 0..3 {
            sqlx::query(
                "INSERT INTO events (id,event_type,run_id) VALUES ($1,'STEP_COMPLETED',$2)",
            )
            .bind(format!("ct-e-{sfx}-{i}"))
            .bind(&run_id)
            .execute(&pool)
            .await
            .expect("seed event");
        }
        let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id=$1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("count before");
        assert_eq!(count_before, 3);

        // Compact: must add exactly one checkpoint for this fresh run...
        let rolled = compact_once(&pool).await.expect("compact");
        assert!(rolled >= 1, "expected >=1 checkpoint rolled, got {rolled}");
        let ck_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM checkpoints WHERE run_id=$1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("ck count");
        assert_eq!(ck_count, 1, "compaction must add exactly one checkpoint");

        // ...and must NOT have touched any source event (the invariant).
        let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id=$1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("count after");
        assert_eq!(
            count_after, count_before,
            "compaction must never delete source events"
        );

        // Idempotent: no new events since the checkpoint -> no new checkpoint,
        // events still intact.
        let rolled2 = compact_once(&pool).await.expect("compact2");
        assert_eq!(rolled2, 0, "no new events -> no new checkpoint");
        let count_final: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE run_id=$1")
            .bind(&run_id)
            .fetch_one(&pool)
            .await
            .expect("count final");
        assert_eq!(
            count_final, count_before,
            "events still intact after re-compaction"
        );

        // cleanup (checkpoints/events first — checkpoints FK-references runs).
        for q in [
            "DELETE FROM checkpoints WHERE run_id=$1",
            "DELETE FROM events WHERE run_id=$1",
        ] {
            sqlx::query(q).bind(&run_id).execute(&pool).await.ok();
        }
        for q in [
            "DELETE FROM runs WHERE org_id=$1",
            "DELETE FROM threads WHERE org_id=$1",
        ] {
            sqlx::query(q).bind(&org).execute(&pool).await.ok();
        }
    }
}
