//! Periodic, additive checkpoint compaction loop.
//!
//! For each run that has new `STEP_COMPLETED`/`ACTION_COMPLETED` events beyond
//! its deterministic step watermark, insert a synthesized checkpoint containing
//! only that watermark. Fixed-width deterministic IDs and semantic conflict
//! verification make concurrent cycles idempotent without allowing a foreign
//! row to preclaim the automatic-checkpoint namespace. The `ordinal` column is
//! filled by the existing BEFORE INSERT trigger; targeted ordinal conflicts
//! are retried because other checkpoint writers use the same MAX+1 trigger.

use crate::store::Pool;
use anyhow::Result;
use metrics::{counter, histogram};
use std::{
    future::Future,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tracing::{info, warn};

const DEFAULT_RETRY_ATTEMPTS: u32 = 3;
const DEFAULT_RETRY_BASE_DELAY: Duration = Duration::from_millis(250);
const DEFAULT_RETRY_MAX_DELAY: Duration = Duration::from_secs(2);
const DEFAULT_INTERVAL_SECS: u64 = 60;
const MAX_INTERVAL_SECS: u64 = 86_400;
const MAX_CHECKPOINTS_PER_CYCLE: i64 = 100;
pub(crate) const AUTO_CHECKPOINT_PREFIX: &str = "auto-compact-v1:";
#[cfg(test)]
const MAX_CHECKPOINT_ID_LEN: usize = AUTO_CHECKPOINT_PREFIX.len() + 64;
const STATEMENT_TIMEOUT: Duration = Duration::from_secs(5);
const SET_STATEMENT_TIMEOUT_SQL: &str = "SELECT set_config('statement_timeout', $1, true)";
static JITTER_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CompactionResult {
    inserted: i64,
    semantic_conflicts: i64,
}

#[derive(Clone, Copy, Debug)]
struct RetryPolicy {
    max_attempts: u32,
    base_delay: Duration,
    max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_RETRY_ATTEMPTS,
            base_delay: DEFAULT_RETRY_BASE_DELAY,
            max_delay: DEFAULT_RETRY_MAX_DELAY,
        }
    }
}

impl RetryPolicy {
    fn delay_after(self, failed_attempt: u32, jitter_seed: u64) -> Duration {
        let shift = failed_attempt.saturating_sub(1).min(31);
        let multiplier = 1_u32.checked_shl(shift).unwrap_or(u32::MAX);
        let exponential = self
            .base_delay
            .saturating_mul(multiplier)
            .min(self.max_delay);
        let jitter_cap = (exponential / 5).min(self.max_delay.saturating_sub(exponential));
        let jitter_cap_ms = u64::try_from(jitter_cap.as_millis()).unwrap_or(u64::MAX);
        let jitter_ms = if jitter_cap_ms == 0 {
            0
        } else {
            jitter_seed % (jitter_cap_ms + 1)
        };
        exponential.saturating_add(Duration::from_millis(jitter_ms))
    }
}

const COMPACT_SQL: &str = r#"
        WITH watermarks AS (
            SELECT e.run_id, MAX(e.step_ordinal) AS max_step
            FROM events e
            JOIN runs r ON r.id = e.run_id
            WHERE e.event_type IN ('STEP_COMPLETED', 'ACTION_COMPLETED')
              AND e.step_ordinal > 0
              AND e.run_id IS NOT NULL
              AND NOT (COALESCE(r.metadata, '{}'::jsonb) @> '{"zdr": true}'::jsonb)
            GROUP BY e.run_id
        ),
        identified AS (
            SELECT c.run_id,
                   c.max_step,
                   'auto-compact-v1:'
                       || encode(
                            sha256(convert_to(c.run_id || ':' || c.max_step::text, 'UTF8')),
                            'hex'
                          ) AS checkpoint_id,
                   convert_to(
                       jsonb_build_object('max_step', c.max_step)::text,
                       'UTF8'
                   ) AS legacy_state,
                   convert_to(
                       jsonb_build_object(
                           'kind', 'auto_compact_v1',
                           'max_step', c.max_step,
                           'run_id', c.run_id
                       )::text,
                       'UTF8'
                   ) AS expected_state
            FROM watermarks c
        ),
        need AS (
            SELECT i.run_id, i.max_step, i.checkpoint_id, i.expected_state
            FROM identified i
            WHERE NOT EXISTS (
                SELECT 1
                FROM checkpoints ck
                WHERE ck.run_id = i.run_id
                  AND (ck.state = i.legacy_state OR ck.state = i.expected_state)
            )
            ORDER BY i.run_id
            LIMIT $1
        ),
        attempted AS (
            SELECT COUNT(*)::BIGINT AS count FROM need
        ),
        upserted AS (
            INSERT INTO checkpoints (id, run_id, state)
            SELECT n.checkpoint_id,
                   n.run_id,
                   n.expected_state
            FROM need n
            ON CONFLICT (id) DO UPDATE
                SET state = checkpoints.state
                WHERE checkpoints.run_id = EXCLUDED.run_id
                  AND checkpoints.state = EXCLUDED.state
            RETURNING (xmax = 0) AS inserted
        )
        SELECT attempted.count,
               COUNT(upserted.inserted)::BIGINT AS accepted,
               COUNT(*) FILTER (WHERE upserted.inserted)::BIGINT AS inserted
        FROM attempted
        LEFT JOIN upserted ON true
        GROUP BY attempted.count
        "#;

pub async fn run(pool: Pool) -> Result<()> {
    let configured_interval = std::env::var("COMPACTION_INTERVAL_SECS").ok();
    let interval_secs = parse_interval_secs(configured_interval.as_deref());
    let mut tick = tokio::time::interval(Duration::from_secs(interval_secs));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    info!(interval_secs, "compaction loop started");
    let retry_policy = RetryPolicy::default();

    loop {
        tick.tick().await;
        let start = Instant::now();
        match compact_with_retry(&pool, retry_policy).await {
            Ok(n) => {
                record_cycle_metrics("ok", start.elapsed());
                if n > 0 {
                    info!(rolled = n, "compaction cycle complete");
                }
            }
            Err(e) => {
                record_cycle_metrics("error", start.elapsed());
                warn!(error = %e, "compaction cycle failed");
            }
        }
    }
}

pub async fn compact_once(pool: &Pool) -> Result<i64> {
    let mut transaction = pool.begin().await?;
    let timeout = format!("{}ms", STATEMENT_TIMEOUT.as_millis());
    sqlx::query_scalar::<_, String>(SET_STATEMENT_TIMEOUT_SQL)
        .bind(timeout)
        .fetch_one(&mut *transaction)
        .await?;
    let (attempted, accepted, inserted) = sqlx::query_as::<_, (i64, i64, i64)>(COMPACT_SQL)
        .bind(MAX_CHECKPOINTS_PER_CYCLE)
        .fetch_one(&mut *transaction)
        .await?;
    let result = match validate_compaction_result(attempted, accepted, inserted) {
        Ok(result) => result,
        Err(error) => {
            transaction.rollback().await?;
            return Err(error);
        }
    };
    if result.semantic_conflicts > 0 {
        counter!("mp_session_compaction_semantic_conflicts_total")
            .increment(u64::try_from(result.semantic_conflicts).unwrap_or(u64::MAX));
        warn!(
            conflicts = result.semantic_conflicts,
            "reserved automatic checkpoint conflicts were isolated"
        );
    }
    transaction.commit().await?;
    Ok(result.inserted)
}

fn validate_compaction_result(
    attempted: i64,
    accepted: i64,
    inserted: i64,
) -> Result<CompactionResult> {
    if attempted < 0 || accepted < 0 || inserted < 0 || accepted > attempted || inserted > accepted
    {
        anyhow::bail!("invalid compaction result counts");
    }
    Ok(CompactionResult {
        inserted,
        semantic_conflicts: attempted - accepted,
    })
}

async fn compact_with_retry(pool: &Pool, policy: RetryPolicy) -> Result<i64> {
    retry_operation(|| compact_once(pool), policy).await
}

async fn retry_operation<T, F, Fut>(mut operation: F, policy: RetryPolicy) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut attempt = 1_u32;
    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) => {
                let Some(reason) = retry_reason(&error) else {
                    return Err(error);
                };
                if attempt >= policy.max_attempts.max(1) {
                    return Err(error);
                }

                let delay = policy.delay_after(attempt, jitter_seed());
                counter!("mp_session_compaction_retries_total", "reason" => reason).increment(1);
                histogram!("mp_session_compaction_retry_delay_seconds").record(delay.as_secs_f64());
                warn!(
                    error = %error,
                    attempt,
                    next_attempt = attempt + 1,
                    delay_ms = delay.as_millis(),
                    reason,
                    "transient compaction failure; retrying"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

fn retry_reason(error: &anyhow::Error) -> Option<&'static str> {
    let sqlx_error = error.downcast_ref::<sqlx::Error>()?;
    match sqlx_error {
        sqlx::Error::Io(_) | sqlx::Error::Tls(_) => Some("transport"),
        sqlx::Error::PoolTimedOut | sqlx::Error::WorkerCrashed | sqlx::Error::BeginFailed => {
            Some("pool")
        }
        sqlx::Error::Database(database) => {
            let code = database.code()?;
            postgres_retry_reason(code.as_ref(), database.constraint())
        }
        _ => None,
    }
}

fn postgres_retry_reason(code: &str, constraint: Option<&str>) -> Option<&'static str> {
    if code.starts_with("08") {
        return Some("connection");
    }
    if code == "23505" && constraint == Some("idx_checkpoints_run_ordinal") {
        return Some("ordinal_contention");
    }
    matches!(
        code,
        "40001" | "40P01" | "55P03" | "57P01" | "57P02" | "57P03" | "53300"
    )
    .then_some("database")
}

fn jitter_seed() -> u64 {
    let clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let sequence = JITTER_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    u64::from(clock.subsec_nanos()) ^ sequence.rotate_left(17)
}

fn record_cycle_metrics(status: &'static str, duration: Duration) {
    counter!("mp_session_compaction_runs_total", "status" => status).increment(1);
    histogram!("mp_session_compaction_duration_seconds").record(duration.as_secs_f64());
}

fn parse_interval_secs(value: Option<&str>) -> u64 {
    value
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|seconds| (1..=MAX_INTERVAL_SECS).contains(seconds))
        .unwrap_or(DEFAULT_INTERVAL_SECS)
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;
