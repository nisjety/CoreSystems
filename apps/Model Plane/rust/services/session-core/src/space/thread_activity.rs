//! Thread activity summary: backfill and pending probe (build plan WS15-S5).
//!
//! Migration 0078 adds the summary columns to `threads`, a trigger on `messages` that keeps
//! them current for every new message, and `thread_activity_recompute(thread_id)`. It does not
//! fill the threads that already existed: a full-table pass would block startup. This task
//! fills them in bounded batches, outside any migration, and [`backfill_pending`] tells a
//! reader (`ListSpaceTimeline`, WS15-S6; the attention read, WS11-S2) whether any thread that
//! has messages is still unfilled, so it can refuse with `summary_backfill_pending` instead of
//! reading a half-filled table.
//!
//! The summary holds ids, counts and a role word only, so the backfill reads and writes no
//! message content and needs no retention decision. It is idempotent and safe to run on every
//! replica at once: each rebuild takes the thread row lock first.

use std::time::Duration;

use anyhow::Result;
use sqlx::PgPool;
use tracing::{info, warn};

/// Threads rebuilt per transaction.
pub(crate) const BACKFILL_BATCH: i64 = 500;
/// Pause between batches, so the backfill never starves appenders.
const BATCH_PAUSE: Duration = Duration::from_millis(50);
/// Pause before retrying after a failed batch.
const RETRY_PAUSE: Duration = Duration::from_secs(5);

/// Rebuilds one batch of unfilled threads that have messages. Returns how many were rebuilt;
/// zero means the backfill is complete.
pub(crate) async fn backfill_batch(pool: &PgPool, batch: i64) -> Result<u64> {
    let rebuilt = sqlx::query(
        "SELECT thread_activity_recompute(b.id)
           FROM (SELECT t.id FROM threads t
                  WHERE t.last_message_seq IS NULL
                    AND EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id)
                  ORDER BY t.id
                  LIMIT $1) b",
    )
    .bind(batch)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(rebuilt)
}

/// True while any thread that has messages still has no summary. Readers of the summary refuse
/// while this is true.
#[allow(dead_code)] // read by ListSpaceTimeline (WS15-S6) and the attention read (WS11-S2)
pub(crate) async fn backfill_pending(pool: &PgPool) -> Result<bool> {
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM threads t
                         WHERE t.last_message_seq IS NULL
                           AND EXISTS (SELECT 1 FROM messages m WHERE m.thread_id = t.id))",
    )
    .fetch_one(pool)
    .await?;
    Ok(pending)
}

/// The backfill loop: batches until none is left, then returns. A failed batch is retried after
/// a pause and never ends the task early; progress is kept because every batch commits alone.
pub(crate) async fn run_backfill(pool: PgPool) {
    let mut total: u64 = 0;
    loop {
        match backfill_batch(&pool, BACKFILL_BATCH).await {
            Ok(0) => {
                info!(threads = total, "thread activity backfill complete");
                return;
            }
            Ok(rebuilt) => {
                total += rebuilt;
                tokio::time::sleep(BATCH_PAUSE).await;
            }
            Err(error) => {
                warn!(%error, "thread activity backfill batch failed; retrying");
                tokio::time::sleep(RETRY_PAUSE).await;
            }
        }
    }
}
