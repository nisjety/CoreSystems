//! History of the scheduler's passes over a tenant's endpoints (`vsv2.scheduler_pass`).

use chrono::{DateTime, Utc};
use sqlx::postgres::PgPool;
use sqlx::Row;
use uuid::Uuid;
use vsv2_common::{Error, Result, TenantId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassStatus {
    Running,
    Succeeded,
    Failed,
}

impl PassStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "succeeded" => Self::Succeeded,
            "failed" => Self::Failed,
            _ => Self::Running,
        }
    }
}

/// The most recent pass of a tier over a tenant, and when the last complete (non-retry) one began.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PassHistory {
    pub last: Option<LastPass>,
    pub last_complete_started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastPass {
    pub pass_id: Uuid,
    pub status: PassStatus,
    pub started_at: DateTime<Utc>,
    /// Last sign of life from the process running the pass (equals `started_at` until it beats).
    pub heartbeat_at: DateTime<Utc>,
    /// Endpoints that failed or were not reached; the next retry covers exactly these.
    pub failed_names: Vec<String>,
}

pub async fn start_pass(
    pool: &PgPool,
    pass_id: Uuid,
    tier: &str,
    tenant: &TenantId,
    retry_of: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO vsv2.scheduler_pass (pass_id, tier, tenant_id, status, retry_of) \
         VALUES ($1, $2, $3, 'running', $4)",
    )
    .bind(pass_id)
    .bind(tier)
    .bind(tenant.as_str())
    .bind(retry_of)
    .execute(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?;
    Ok(())
}

/// Tell the other readers of the table that the process running `pass_id` is alive.
pub async fn heartbeat(pool: &PgPool, pass_id: Uuid) -> Result<()> {
    sqlx::query("UPDATE vsv2.scheduler_pass SET heartbeat_at = NOW() WHERE pass_id = $1")
        .bind(pass_id)
        .execute(pool)
        .await
        .map_err(|e| Error::StateStore(e.to_string()))?;
    Ok(())
}

pub async fn finish_pass(
    pool: &PgPool,
    pass_id: Uuid,
    status: PassStatus,
    attempted: usize,
    failed_names: &[String],
    records: u64,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE vsv2.scheduler_pass \
         SET status = $2, finished_at = NOW(), attempted_endpoints = $3, \
             failed_names = $4, records_synced = $5, error = $6, abandoned = FALSE \
         WHERE pass_id = $1",
    )
    .bind(pass_id)
    .bind(status.as_str())
    .bind(attempted as i32)
    .bind(serde_json::json!(failed_names))
    .bind(records as i64)
    .bind(error)
    .execute(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?;
    Ok(())
}

pub async fn history(pool: &PgPool, tier: &str, tenant: &TenantId) -> Result<PassHistory> {
    let last = sqlx::query(
        "SELECT pass_id, status, started_at, heartbeat_at, failed_names FROM vsv2.scheduler_pass \
         WHERE tier = $1 AND tenant_id = $2 ORDER BY started_at DESC LIMIT 1",
    )
    .bind(tier)
    .bind(tenant.as_str())
    .fetch_optional(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?
    .map(|row| {
        let names: serde_json::Value = row.get("failed_names");
        LastPass {
            pass_id: row.get("pass_id"),
            status: PassStatus::parse(&row.get::<String, _>("status")),
            started_at: row.get("started_at"),
            heartbeat_at: row.get("heartbeat_at"),
            failed_names: names
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        }
    });
    let last_complete_started_at: Option<DateTime<Utc>> = sqlx::query(
        "SELECT MAX(started_at) AS started FROM vsv2.scheduler_pass \
         WHERE tier = $1 AND tenant_id = $2 AND retry_of IS NULL AND NOT abandoned",
    )
    .bind(tier)
    .bind(tenant.as_str())
    .fetch_one(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?
    .get("started");
    Ok(PassHistory {
        last,
        last_complete_started_at,
    })
}

/// Mark passes still `running` whose heartbeat is older than `silent_for` as failed and
/// `abandoned`: the process that ran them is gone.
///
/// An abandoned pass does not count as the tier's last complete pass, so the tier is due again
/// straight away. An abandoned retry keeps the endpoint names of the pass it was retrying.
pub async fn abandon_stale(pool: &PgPool, silent_for: chrono::Duration) -> Result<u64> {
    let result = sqlx::query(
        "UPDATE vsv2.scheduler_pass AS s \
         SET status = 'failed', finished_at = NOW(), abandoned = TRUE, \
             failed_names = COALESCE( \
                 (SELECT p.failed_names FROM vsv2.scheduler_pass p WHERE p.pass_id = s.retry_of), \
                 '[]'::jsonb), \
             error = 'abandoned: the scheduler ended without finishing this pass' \
         WHERE s.status = 'running' \
           AND s.heartbeat_at < NOW() - make_interval(secs => $1::double precision)",
    )
    .bind(silent_for.num_seconds())
    .execute(pool)
    .await
    .map_err(|e| Error::StateStore(e.to_string()))?;
    Ok(result.rows_affected())
}
