//! Postgres-backed durable `RequestQueue` + run-checkpoint storage.
//!
//! Cycle 20 / cluster #1.
//!
//! Wraps `quarry_request_queues`, `quarry_queue_items`, and
//! `quarry_run_checkpoints` (see `migrations/0001_request_queue.sql`).
//!
//! ## Why this exists
//!
//! `InMemoryRequestQueue` loses every queued item when the edge process
//! restarts. For crawls that span hours and across re-deploys, that's a
//! data-loss bug — restarting the orchestrator while a crawl is mid-run
//! orphans every in-flight URL and forces a full re-fetch.
//!
//! This impl persists every state change to Postgres and uses
//! `SELECT FOR UPDATE SKIP LOCKED` to coordinate concurrent workers
//! safely without a separate broker.
//!
//! ## Tenant isolation
//!
//! Every method takes an `org_id` parameter that the edge handler sets
//! from the verified JWT claim. The composite unique index on
//! `(org_id, request_id)` prevents request-ID collisions across tenants
//! and the `pop()` query is org-scoped so a worker never picks up a
//! peer-tenant's URL.
//!
//! ## Gated
//!
//! Behind the `postgres-queue` cargo feature so default builds don't
//! drag in `sqlx` / `libpq` toolchain.

#![cfg(feature = "postgres-queue")]

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgPool;
use uuid::Uuid;

use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};

use crate::crawl_frontier::FrontierCheckpoint;
use crate::request_queue::{Priority, QueueStats, QueuedRequest, RequestQueue};

/// Convert `sqlx::Error` → `QuarryError` with an `Internal` code. Caller
/// supplies a short label so the log line is useful.
fn db_err(label: &str, e: sqlx::Error) -> QuarryError {
    QuarryError::new(
        ErrorCode::Internal,
        format!("postgres {label}: {e}"),
    )
}

fn priority_to_smallint(p: Priority) -> i16 {
    match p {
        Priority::Low => 0,
        Priority::Default => 1,
        Priority::High => 2,
    }
}

fn smallint_to_priority(v: i16) -> Priority {
    match v {
        0 => Priority::Low,
        2 => Priority::High,
        _ => Priority::Default,
    }
}

/// Durable Postgres-backed request queue.
///
/// One instance per (process, queue). Construct via [`PostgresRequestQueue::bind`]
/// — it both resolves the queue row by name+org and ensures it exists.
#[derive(Clone)]
pub struct PostgresRequestQueue {
    pool: PgPool,
    queue_id: Uuid,
    org_id: String,
    visibility_timeout: Duration,
}

impl PostgresRequestQueue {
    /// Resolve (or create) the queue row for `(org_id, name)` and return
    /// a handle. Visibility-timeout governs how long an `in_flight` item
    /// can sit before the reaper considers it stranded.
    pub async fn bind(
        pool: PgPool,
        org_id: impl Into<String>,
        name: impl AsRef<str>,
        kind: impl AsRef<str>,
        visibility_timeout: Duration,
    ) -> QuarryResult<Self> {
        let org_id = org_id.into();
        let name = name.as_ref();
        let kind = kind.as_ref();

        // Look up or create the queue row. We don't `ON CONFLICT DO
        // UPDATE` because the queue row carries config the caller
        // shouldn't accidentally overwrite — instead, separate UPDATE.
        let existing: Option<(Uuid,)> = sqlx::query_as(
            "SELECT queue_id FROM quarry_request_queues \
             WHERE org_id = $1 AND name = $2 AND deleted_at IS NULL \
             LIMIT 1",
        )
        .bind(&org_id)
        .bind(name)
        .fetch_optional(&pool)
        .await
        .map_err(|e| db_err("bind:select", e))?;

        let queue_id = match existing {
            Some((id,)) => id,
            None => {
                let new_id = Uuid::new_v4();
                sqlx::query(
                    "INSERT INTO quarry_request_queues (queue_id, org_id, name, kind) \
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(new_id)
                .bind(&org_id)
                .bind(name)
                .bind(kind)
                .execute(&pool)
                .await
                .map_err(|e| db_err("bind:insert", e))?;
                new_id
            }
        };

        Ok(Self {
            pool,
            queue_id,
            org_id,
            visibility_timeout,
        })
    }

    /// Persist a checkpoint snapshot for the given run. Idempotent: the
    /// row is UPSERTed per (org_id, run_id), and `version` is bumped on
    /// every save so dashboards can show staleness.
    pub async fn save_checkpoint(
        &self,
        run_id: &str,
        checkpoint: &FrontierCheckpoint,
    ) -> QuarryResult<()> {
        let state = serde_json::to_value(checkpoint).map_err(|e| {
            QuarryError::new(
                ErrorCode::Internal,
                format!("serialize checkpoint failed: {e}"),
            )
        })?;
        sqlx::query(
            "INSERT INTO quarry_run_checkpoints (run_id, org_id, queue_id, state, version, saved_at) \
             VALUES ($1, $2, $3, $4, 1, NOW()) \
             ON CONFLICT (org_id, run_id) DO UPDATE SET \
                 state    = EXCLUDED.state, \
                 version  = quarry_run_checkpoints.version + 1, \
                 saved_at = NOW(), \
                 queue_id = EXCLUDED.queue_id",
        )
        .bind(run_id)
        .bind(&self.org_id)
        .bind(self.queue_id)
        .bind(&state)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("save_checkpoint", e))?;
        Ok(())
    }

    /// Read the latest checkpoint for a run, or `None` if it never saved.
    pub async fn load_checkpoint(
        &self,
        run_id: &str,
    ) -> QuarryResult<Option<FrontierCheckpoint>> {
        let row: Option<(Value,)> = sqlx::query_as(
            "SELECT state FROM quarry_run_checkpoints \
             WHERE org_id = $1 AND run_id = $2",
        )
        .bind(&self.org_id)
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("load_checkpoint", e))?;

        match row {
            None => Ok(None),
            Some((state,)) => {
                let cp: FrontierCheckpoint = serde_json::from_value(state).map_err(|e| {
                    QuarryError::new(
                        ErrorCode::Internal,
                        format!("deserialize checkpoint failed: {e}"),
                    )
                })?;
                Ok(Some(cp))
            }
        }
    }

    /// Record a retry decision for audit. Append-only; failures here
    /// log-warn instead of bubbling — losing an audit row is preferable
    /// to failing the underlying retry.
    pub async fn record_retry(
        &self,
        request_id: &str,
        run_id: Option<&str>,
        attempt: i32,
        error_code: &str,
        retry_class: &str,
        delay_ms: i32,
        note: Option<&str>,
    ) {
        let result = sqlx::query(
            "INSERT INTO quarry_retry_events \
             (org_id, request_id, run_id, attempt, error_code, retry_class, delay_ms, note) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(&self.org_id)
        .bind(request_id)
        .bind(run_id)
        .bind(attempt)
        .bind(error_code)
        .bind(retry_class)
        .bind(delay_ms)
        .bind(note)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::warn!(error = %e, request_id, "record_retry failed");
        }
    }

    /// Exposes the queue's bound UUID — useful for logging + dashboards.
    pub fn queue_id(&self) -> Uuid {
        self.queue_id
    }
}

#[async_trait]
impl RequestQueue for PostgresRequestQueue {
    async fn enqueue(
        &self,
        request_id: String,
        url: String,
        priority: Priority,
        payload: Value,
    ) -> QuarryResult<bool> {
        // `ON CONFLICT (org_id, request_id) DO NOTHING` gives us
        // idempotency: a producer that retries enqueue with the same
        // `request_id` doesn't double-insert. We use `RETURNING` to
        // distinguish "inserted" (returns 1 row) from "no-op".
        let inserted: Option<(String,)> = sqlx::query_as(
            "INSERT INTO quarry_queue_items \
             (request_id, queue_id, org_id, url, priority, payload) \
             VALUES ($1, $2, $3, $4, $5, $6) \
             ON CONFLICT (org_id, request_id) DO NOTHING \
             RETURNING request_id",
        )
        .bind(&request_id)
        .bind(self.queue_id)
        .bind(&self.org_id)
        .bind(&url)
        .bind(priority_to_smallint(priority))
        .bind(&payload)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| db_err("enqueue", e))?;
        Ok(inserted.is_some())
    }

    async fn pop(&self) -> QuarryResult<Option<QueuedRequest>> {
        // SELECT FOR UPDATE SKIP LOCKED lets N concurrent workers race
        // on the same queue without any waiting for a row lock — each
        // skips rows already claimed by a peer. We mutate the chosen
        // row to `in_flight` inside the same transaction so the next
        // call from any process never sees it as available.
        let mut tx = self.pool.begin().await.map_err(|e| db_err("pop:begin", e))?;

        let row: Option<(String, String, i16, Value, DateTime<Utc>, i32)> = sqlx::query_as(
            "SELECT request_id, url, priority, payload, enqueued_at, attempt \
             FROM quarry_queue_items \
             WHERE org_id = $1 AND queue_id = $2 AND status = 'queued' \
             ORDER BY priority DESC, enqueued_at ASC \
             LIMIT 1 \
             FOR UPDATE SKIP LOCKED",
        )
        .bind(&self.org_id)
        .bind(self.queue_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| db_err("pop:select", e))?;

        let Some((rid, url, prio, payload, enq, attempt)) = row else {
            tx.commit().await.map_err(|e| db_err("pop:commit_empty", e))?;
            return Ok(None);
        };

        let deadline_at = Utc::now()
            + chrono::Duration::from_std(self.visibility_timeout).unwrap_or_else(|_| {
                // Should never happen — visibility_timeout is bounded by
                // a u64 of seconds. Fall back to 30s if the cast fails.
                chrono::Duration::seconds(30)
            });

        sqlx::query(
            "UPDATE quarry_queue_items \
             SET status = 'in_flight', \
                 in_flight_since = NOW(), \
                 visibility_deadline_at = $3, \
                 attempt = attempt + 1 \
             WHERE org_id = $1 AND request_id = $2",
        )
        .bind(&self.org_id)
        .bind(&rid)
        .bind(deadline_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| db_err("pop:update", e))?;

        tx.commit().await.map_err(|e| db_err("pop:commit", e))?;

        Ok(Some(QueuedRequest {
            request_id: rid,
            url,
            priority: smallint_to_priority(prio),
            payload,
            enqueued_at: enq,
            attempt: (attempt as u32) + 1, // we bumped it above
        }))
    }

    async fn ack(&self, request_id: &str) -> QuarryResult<()> {
        sqlx::query(
            "UPDATE quarry_queue_items \
             SET status = 'acked', acked_at = NOW(), visibility_deadline_at = NULL \
             WHERE org_id = $1 AND request_id = $2 AND status = 'in_flight'",
        )
        .bind(&self.org_id)
        .bind(request_id)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("ack", e))?;
        Ok(())
    }

    async fn reap_expired(&self) -> QuarryResult<u64> {
        // Move every `in_flight` row whose visibility deadline elapsed
        // back to `queued`. We don't reset `attempt` — the workers count
        // up so a poison-pill URL eventually trips fail_permanently.
        let result = sqlx::query(
            "UPDATE quarry_queue_items \
             SET status = 'queued', in_flight_since = NULL, visibility_deadline_at = NULL \
             WHERE org_id = $1 \
               AND queue_id = $2 \
               AND status = 'in_flight' \
               AND visibility_deadline_at < NOW()",
        )
        .bind(&self.org_id)
        .bind(self.queue_id)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("reap_expired", e))?;
        Ok(result.rows_affected())
    }

    async fn fail_permanently(&self, request_id: &str, reason: &str) -> QuarryResult<()> {
        sqlx::query(
            "UPDATE quarry_queue_items \
             SET status = 'failed', failed_at = NOW(), failure_reason = $3, \
                 visibility_deadline_at = NULL \
             WHERE org_id = $1 AND request_id = $2",
        )
        .bind(&self.org_id)
        .bind(request_id)
        .bind(reason)
        .execute(&self.pool)
        .await
        .map_err(|e| db_err("fail_permanently", e))?;
        Ok(())
    }

    async fn stats(&self) -> QuarryResult<QueueStats> {
        // Single round-trip with FILTER aggregates so we don't pay for
        // N separate SELECTs.
        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT \
                 COUNT(*) FILTER (WHERE status = 'queued')    AS queued, \
                 COUNT(*) FILTER (WHERE status = 'in_flight') AS in_flight, \
                 COUNT(*)                                       AS total_enqueued, \
                 COUNT(*) FILTER (WHERE status = 'acked')     AS total_acked, \
                 0::BIGINT                                      AS total_requeued \
             FROM quarry_queue_items \
             WHERE org_id = $1 AND queue_id = $2",
        )
        .bind(&self.org_id)
        .bind(self.queue_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| db_err("stats", e))?;
        let (queued, in_flight, total_enqueued, total_acked, total_requeued) = row;
        Ok(QueueStats {
            queued: queued.max(0) as usize,
            in_flight: in_flight.max(0) as usize,
            total_enqueued: total_enqueued.max(0) as u64,
            total_acked: total_acked.max(0) as u64,
            total_requeued: total_requeued.max(0) as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// All tests in this module require a live Postgres reachable via
    /// `DATABASE_URL` (e.g. via testcontainers or a developer-local
    /// instance). Without that var set, the tests print a single
    /// skip-line and pass — so default CI (which doesn't expose a DB)
    /// stays green while local dev gets real coverage on opt-in.
    fn database_url() -> Option<String> {
        std::env::var("DATABASE_URL").ok().filter(|s| !s.is_empty())
    }

    async fn fresh_pool() -> Option<PgPool> {
        let url = database_url()?;
        let pool = PgPool::connect(&url).await.ok()?;
        // Run migrations every test (they're idempotent).
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply");
        Some(pool)
    }

    /// Drop and recreate the queue tables between tests so we don't
    /// leak rows. Cheap because tables are small in tests.
    async fn clean(pool: &PgPool) {
        for tbl in [
            "quarry_retry_events",
            "quarry_queue_items",
            "quarry_run_checkpoints",
            "quarry_request_queues",
        ] {
            sqlx::query(&format!("TRUNCATE TABLE {tbl} CASCADE"))
                .execute(pool)
                .await
                .ok();
        }
    }

    #[tokio::test]
    async fn enqueue_then_pop_roundtrips() {
        let Some(pool) = fresh_pool().await else {
            eprintln!("skipping: DATABASE_URL not set");
            return;
        };
        clean(&pool).await;

        let q = PostgresRequestQueue::bind(
            pool.clone(),
            "org_alpha",
            "test-queue",
            "crawl",
            Duration::from_secs(30),
        )
        .await
        .expect("bind");

        let inserted = q
            .enqueue("r1".into(), "https://a/".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        assert!(inserted, "first enqueue should insert");

        // Idempotent: same request_id should NOT re-insert.
        let again = q
            .enqueue("r1".into(), "https://a/".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        assert!(!again, "duplicate enqueue should be no-op");

        let popped = q.pop().await.unwrap().expect("pop returns a row");
        assert_eq!(popped.request_id, "r1");
        assert_eq!(popped.url, "https://a/");
        assert_eq!(popped.attempt, 1);

        // Second pop drains the queue.
        let empty = q.pop().await.unwrap();
        assert!(empty.is_none());
    }

    #[tokio::test]
    async fn priority_ordering_pops_high_first() {
        let Some(pool) = fresh_pool().await else { return; };
        clean(&pool).await;

        let q = PostgresRequestQueue::bind(
            pool.clone(),
            "org_alpha",
            "prio-queue",
            "crawl",
            Duration::from_secs(30),
        )
        .await
        .unwrap();

        q.enqueue("low".into(), "https://l/".into(), Priority::Low, json!({}))
            .await
            .unwrap();
        q.enqueue("hi".into(), "https://h/".into(), Priority::High, json!({}))
            .await
            .unwrap();
        q.enqueue("mid".into(), "https://m/".into(), Priority::Default, json!({}))
            .await
            .unwrap();

        // Order should be: hi, mid, low.
        assert_eq!(q.pop().await.unwrap().unwrap().request_id, "hi");
        assert_eq!(q.pop().await.unwrap().unwrap().request_id, "mid");
        assert_eq!(q.pop().await.unwrap().unwrap().request_id, "low");
    }

    #[tokio::test]
    async fn reap_expired_returns_in_flight_to_queued() {
        let Some(pool) = fresh_pool().await else { return; };
        clean(&pool).await;

        let q = PostgresRequestQueue::bind(
            pool.clone(),
            "org_alpha",
            "reap-queue",
            "crawl",
            // Tight timeout so the test doesn't wait.
            Duration::from_millis(50),
        )
        .await
        .unwrap();

        q.enqueue("x".into(), "https://x/".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        let _ = q.pop().await.unwrap().unwrap();
        // Worker "dies" without acking. Wait past visibility timeout.
        tokio::time::sleep(Duration::from_millis(120)).await;
        let reaped = q.reap_expired().await.unwrap();
        assert_eq!(reaped, 1);
        // Next pop must return the same request_id with attempt=2.
        let again = q.pop().await.unwrap().unwrap();
        assert_eq!(again.request_id, "x");
        assert_eq!(again.attempt, 2);
    }

    #[tokio::test]
    async fn tenant_isolation_pop_skips_other_orgs() {
        let Some(pool) = fresh_pool().await else { return; };
        clean(&pool).await;

        let q_a = PostgresRequestQueue::bind(
            pool.clone(),
            "org_alpha",
            "tenant-queue",
            "crawl",
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        let q_b = PostgresRequestQueue::bind(
            pool.clone(),
            "org_beta",
            "tenant-queue",
            "crawl",
            Duration::from_secs(30),
        )
        .await
        .unwrap();
        // Same logical queue name on different orgs → distinct rows
        // (queue_id is per-org via the bind() upsert).
        assert_ne!(q_a.queue_id(), q_b.queue_id());

        q_a.enqueue("a1".into(), "https://a/".into(), Priority::Default, json!({}))
            .await
            .unwrap();
        // org_b's queue must NOT see org_a's request.
        let nothing = q_b.pop().await.unwrap();
        assert!(nothing.is_none(), "tenant leak: org_b saw org_a's item");
    }

    #[tokio::test]
    async fn checkpoint_save_load_roundtrip() {
        let Some(pool) = fresh_pool().await else { return; };
        clean(&pool).await;

        let q = PostgresRequestQueue::bind(
            pool.clone(),
            "org_alpha",
            "cp-queue",
            "crawl",
            Duration::from_secs(30),
        )
        .await
        .unwrap();

        let cp = FrontierCheckpoint {
            config: crate::crawl_frontier::FrontierConfigSnapshot {
                max_depth: Some(3),
                max_pages: Some(100),
                allow_external: false,
                seed_host: Some("example.com".into()),
                seed_hosts: vec!["example.com".into()],
                include_patterns: vec![],
                exclude_patterns: vec![],
            },
            queue: vec![],
            seen: vec!["https://example.com/".into()],
            visited_count: 1,
        };

        q.save_checkpoint("run_42", &cp).await.unwrap();
        let loaded = q.load_checkpoint("run_42").await.unwrap().expect("loaded");
        assert_eq!(loaded.visited_count, 1);
        assert_eq!(loaded.seen, vec!["https://example.com/"]);

        // Second save bumps the version and overwrites state.
        let cp2 = FrontierCheckpoint {
            visited_count: 7,
            ..cp.clone()
        };
        q.save_checkpoint("run_42", &cp2).await.unwrap();
        let loaded2 = q.load_checkpoint("run_42").await.unwrap().unwrap();
        assert_eq!(loaded2.visited_count, 7);
    }
}
