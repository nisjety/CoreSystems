//! The reads behind `vsv2 monitor`: one consistent, read-only snapshot of what the scheduler and
//! the open-set reconciliation recorded.
//!
//! Nothing here writes. The connection's sessions default to read-only and the reads run in one
//! `REPEATABLE READ, READ ONLY` transaction, so a mistake in a query cannot change the state it
//! reports on, and the passes and runs it returns were all true at the same moment. That moment's
//! clock (the database's `NOW()`, which also stamps `scheduler_pass.started_at` and
//! `heartbeat_at`) comes back with the rows, so ages do not depend on the clock of the host that
//! runs the monitor.
//!
//! The rows are pruned, not the whole history: see [`SnapshotRequest`]. The checks that use them
//! live in `vsv2-cli` (`monitor/evaluate.rs`) and take plain rows, so they need no database.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgRow};
use sqlx::{ConnectOptions, Connection, Row};
use tracing::debug;
use uuid::Uuid;
use vsv2_common::{Error, Result};

use crate::run_history::RunStatus;
use crate::scheduler_pass::PassStatus;

/// How long to wait for a connection before the database counts as unreadable.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// One row of `vsv2.scheduler_pass`: a pass of the scheduler over a tenant's endpoints of a tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassRow {
    pub pass_id: Uuid,
    pub tenant_id: String,
    /// `hourly`, `daily` or `full`.
    pub tier: String,
    pub status: PassStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Last sign of life of the process running the pass (equals `started_at` until it beats).
    pub heartbeat_at: DateTime<Utc>,
    /// Endpoints that failed or were not reached.
    pub failed_names: Vec<String>,
    /// The process that ran the pass died; the scheduler marked it failed at a later poll.
    pub abandoned: bool,
}

/// One finished row of `vsv2.sync_run` for an endpoint the monitor asked for (the open-set runs
/// are recorded under `<endpoint>:open_set`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub run_id: Uuid,
    pub tenant_id: String,
    pub endpoint: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// What [`read_snapshot`] returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// `NOW()` of the database when the snapshot was taken.
    pub database_time: DateTime<Utc>,
    pub passes: Vec<PassRow>,
    pub runs: Vec<RunRow>,
}

/// How much of the history [`read_snapshot`] reads.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotRequest<'a> {
    /// Finished passes to read per tenant and tier, newest first: the stretch a check looks back
    /// over for consecutive failures. Besides these, every tenant and tier gets its newest
    /// succeeded pass and its oldest pass (however old), and every pass still `running` is read.
    pub passes_per_tier: i64,
    /// `sync_run.endpoint` values to read, e.g. `customer_invoices:open_set`.
    pub run_endpoints: &'a [String],
    /// Finished runs to read per tenant and endpoint, newest first.
    pub runs_per_endpoint: i64,
    /// Runs that started longer ago than this are not read (the table has no endpoint index, the
    /// start time bounds the scan).
    pub run_lookback_days: i32,
}

/// The passes that matter to the checks (see [`SnapshotRequest::passes_per_tier`]), in one query.
const PASSES_SQL: &str = "\
WITH finished AS ( \
    SELECT pass_id, \
           row_number() OVER (PARTITION BY tenant_id, tier \
                              ORDER BY started_at DESC, pass_id) AS rn \
    FROM vsv2.scheduler_pass WHERE status <> 'running' \
), wanted AS ( \
    SELECT pass_id FROM finished WHERE rn <= $1 \
    UNION \
    (SELECT DISTINCT ON (tenant_id, tier) pass_id FROM vsv2.scheduler_pass \
      WHERE status = 'succeeded' \
      ORDER BY tenant_id, tier, started_at DESC, pass_id) \
    UNION \
    (SELECT DISTINCT ON (tenant_id, tier) pass_id FROM vsv2.scheduler_pass \
      ORDER BY tenant_id, tier, started_at, pass_id) \
    UNION \
    SELECT pass_id FROM vsv2.scheduler_pass WHERE status = 'running' \
) \
SELECT p.pass_id, p.tenant_id, p.tier, p.status, p.started_at, p.finished_at, p.heartbeat_at, \
       p.failed_names, p.abandoned \
FROM vsv2.scheduler_pass p JOIN wanted USING (pass_id) \
ORDER BY p.tenant_id, p.tier, p.started_at DESC, p.pass_id";

/// The newest finished runs of the endpoints asked for. A run still `running` is not read: a
/// scheduler that died mid-run shows in its pass, and the row is closed at the next start.
const RUNS_SQL: &str = "\
SELECT run_id, tenant_id, endpoint, status, started_at, finished_at FROM ( \
    SELECT run_id, tenant_id, endpoint, status, started_at, finished_at, \
           row_number() OVER (PARTITION BY tenant_id, endpoint \
                              ORDER BY started_at DESC, run_id) AS rn \
    FROM vsv2.sync_run \
    WHERE endpoint = ANY($1) \
      AND status IN ('succeeded', 'partial_success', 'failed') \
      AND started_at > NOW() - make_interval(days => $2) \
) r WHERE rn <= $3 \
ORDER BY tenant_id, endpoint, started_at DESC, run_id";

/// Connect for monitoring: one connection (no pool, so a refused connection is reported as
/// such at once instead of being retried until a pool gives up), read-only sessions, and a
/// bounded wait.
///
/// Statement logging is off, so a slow query cannot put a warning on the stdout that carries the
/// report. The connection announces itself as `vsv2-monitor` in `pg_stat_activity`.
pub async fn connect_read_only(dsn: &str) -> Result<PgConnection> {
    let options = dsn
        .parse::<PgConnectOptions>()
        .map_err(|e| Error::StateStore(format!("invalid database URL: {e}")))?
        .application_name("vsv2-monitor")
        .disable_statement_logging();
    let mut conn = tokio::time::timeout(CONNECT_TIMEOUT, PgConnection::connect_with(&options))
        .await
        .map_err(|_| {
            Error::StateStore(format!(
                "cannot connect: no answer within {} s",
                CONNECT_TIMEOUT.as_secs()
            ))
        })?
        .map_err(|e| Error::StateStore(format!("cannot connect: {e}")))?;
    sqlx::query("SET default_transaction_read_only = on")
        .execute(&mut conn)
        .await
        .map_err(read_error("make the session read-only"))?;
    Ok(conn)
}

fn read_error(what: &'static str) -> impl FnOnce(sqlx::Error) -> Error {
    move |e| Error::StateStore(format!("{what}: {e}"))
}

/// Connect, read one snapshot, and disconnect.
pub async fn snapshot_of(dsn: &str, request: &SnapshotRequest<'_>) -> Result<Snapshot> {
    let mut conn = connect_read_only(dsn).await?;
    let snapshot = read_snapshot(&mut conn, request).await;
    if let Err(e) = conn.close().await {
        debug!(err = %e, "monitor: the connection did not close cleanly");
    }
    snapshot
}

/// Read the passes, the open-set runs and the database clock in one read-only transaction.
pub async fn read_snapshot(
    conn: &mut PgConnection,
    request: &SnapshotRequest<'_>,
) -> Result<Snapshot> {
    let mut tx = conn
        .begin()
        .await
        .map_err(read_error("start the read transaction"))?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(read_error("make the transaction read-only"))?;
    sqlx::query("SET LOCAL statement_timeout = '30s'")
        .execute(&mut *tx)
        .await
        .map_err(read_error("bound the statements"))?;

    let database_time: DateTime<Utc> = sqlx::query_scalar("SELECT NOW()")
        .fetch_one(&mut *tx)
        .await
        .map_err(read_error("read the database clock"))?;
    let pass_rows = sqlx::query(PASSES_SQL)
        .bind(request.passes_per_tier)
        .fetch_all(&mut *tx)
        .await
        .map_err(read_error("read vsv2.scheduler_pass"))?;
    let run_rows = sqlx::query(RUNS_SQL)
        .bind(request.run_endpoints)
        .bind(request.run_lookback_days)
        .bind(request.runs_per_endpoint)
        .fetch_all(&mut *tx)
        .await
        .map_err(read_error("read vsv2.sync_run"))?;
    // Nothing was written, so there is nothing to commit; the transaction just ends.
    tx.rollback()
        .await
        .map_err(read_error("end the read transaction"))?;

    Ok(Snapshot {
        database_time,
        passes: pass_rows
            .iter()
            .map(pass_from_row)
            .collect::<std::result::Result<_, _>>()
            .map_err(read_error("decode vsv2.scheduler_pass"))?,
        runs: run_rows
            .iter()
            .map(run_from_row)
            .collect::<std::result::Result<_, _>>()
            .map_err(read_error("decode vsv2.sync_run"))?,
    })
}

fn pass_from_row(row: &PgRow) -> std::result::Result<PassRow, sqlx::Error> {
    let names: serde_json::Value = row.try_get("failed_names")?;
    Ok(PassRow {
        pass_id: row.try_get("pass_id")?,
        tenant_id: row.try_get("tenant_id")?,
        tier: row.try_get("tier")?,
        status: pass_status(&row.try_get::<String, _>("status")?),
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        heartbeat_at: row.try_get("heartbeat_at")?,
        failed_names: failed_names(&names),
        abandoned: row.try_get("abandoned")?,
    })
}

fn run_from_row(row: &PgRow) -> std::result::Result<RunRow, sqlx::Error> {
    Ok(RunRow {
        run_id: row.try_get("run_id")?,
        tenant_id: row.try_get("tenant_id")?,
        endpoint: row.try_get("endpoint")?,
        status: run_status(&row.try_get::<String, _>("status")?),
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
    })
}

fn pass_status(text: &str) -> PassStatus {
    match text {
        "succeeded" => PassStatus::Succeeded,
        "failed" => PassStatus::Failed,
        _ => PassStatus::Running,
    }
}

/// [`RUNS_SQL`] admits only the three finished statuses; anything else cannot arrive.
fn run_status(text: &str) -> RunStatus {
    match text {
        "succeeded" => RunStatus::Succeeded,
        "partial_success" => RunStatus::PartialSuccess,
        _ => RunStatus::Failed,
    }
}

fn failed_names(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn statuses_map_to_the_enums_the_writers_use() {
        assert_eq!(pass_status("running"), PassStatus::Running);
        assert_eq!(pass_status("succeeded"), PassStatus::Succeeded);
        assert_eq!(pass_status("failed"), PassStatus::Failed);
        assert_eq!(run_status("succeeded"), RunStatus::Succeeded);
        assert_eq!(run_status("partial_success"), RunStatus::PartialSuccess);
        assert_eq!(run_status("failed"), RunStatus::Failed);
    }

    #[test]
    fn failed_names_keeps_the_strings_and_tolerates_other_json() {
        assert_eq!(
            failed_names(&json!(["customers", 7, "shipments", null])),
            ["customers", "shipments"]
        );
        assert!(failed_names(&json!({"not": "an array"})).is_empty());
        assert!(failed_names(&json!([])).is_empty());
    }
}
