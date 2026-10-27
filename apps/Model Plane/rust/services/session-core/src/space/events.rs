//! Space event log: the Rust side (build plan WS02-S1).
//!
//! The log itself is written by deferred constraint triggers (migration 0072), not by
//! Rust call sites. This module owns what the database cannot:
//!
//! - [`sync_flag`]: writes `SPACE_REALTIME_EMIT` into `space_event_settings` at boot and
//!   bumps the generation on an off-to-on move, so a client that saw an interrupted log
//!   resnapshots instead of trusting a hole.
//! - [`run_listener`] / [`SpaceWakes`]: one `LISTEN space_events` connection that fans a
//!   wake out in process. A wake carries ids only; the table is the source of truth, so a
//!   lost NOTIFY costs latency, never correctness.
//! - [`run_retention_sweeper`]: deletes rows older than the retention window and moves
//!   `min_seq`, and exports the swallowed-trigger-error counts.
//! - [`gate`]: the pure per-delivery authority check the stream (WS02-S3) applies to every
//!   row and every heartbeat. Nothing here is an authorization input by itself; the verified
//!   Control decision is.
//! - [`run_state`]: the Rust mirror of the SQL `space_event_run_state`, the one place the
//!   run-state word is derived from `runs.status`.
//! - [`purge_space`]: the erasure hook used by Space deletion.

use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgListener;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{info, warn};

/// Env switch for writing log rows. Off unless exactly `true` or `1`.
pub(crate) const EMIT_ENV: &str = "SPACE_REALTIME_EMIT";
/// Retention window of the log, in days.
pub(crate) const RETENTION_DAYS_ENV: &str = "SPACE_EVENT_LOG_RETENTION_DAYS";
const DEFAULT_RETENTION_DAYS: i64 = 14;
const NOTIFY_CHANNEL: &str = "space_events";
const SWEEP_INTERVAL: Duration = Duration::from_secs(15 * 60);
const SWEEP_BATCH: i64 = 5000;
const WAKE_CAPACITY: usize = 1024;

/// Whether the log is written. Off by default (release artifacts); compose dev sets it.
pub(crate) fn emit_enabled_from_env() -> bool {
    std::env::var(EMIT_ENV).is_ok_and(|value| {
        let value = value.trim();
        value.eq_ignore_ascii_case("true") || value == "1"
    })
}

/// Retention window in days, at least one.
pub(crate) fn retention_days_from_env() -> i64 {
    std::env::var(RETENTION_DAYS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .map_or(DEFAULT_RETENTION_DAYS, |days| days.max(1))
}

/// Writes the flag into `space_event_settings`. An off-to-on move bumps every Space's
/// generation and moves `min_seq` above the old head (seq itself stays monotonic), so older
/// cursors read as too old. Returns true when the generation was bumped.
pub(crate) async fn sync_flag(pool: &PgPool, enabled: bool) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let previous: Option<bool> = sqlx::query_scalar(
        "SELECT enabled FROM space_event_settings WHERE singleton FOR UPDATE",
    )
    .fetch_optional(&mut *tx)
    .await?;
    let previous = previous.unwrap_or(false);
    if previous == enabled {
        tx.commit().await?;
        return Ok(false);
    }
    sqlx::query(
        "INSERT INTO space_event_settings (singleton, enabled, updated_at)
         VALUES (TRUE, $1, now())
         ON CONFLICT (singleton) DO UPDATE SET enabled = EXCLUDED.enabled, updated_at = now()",
    )
    .bind(enabled)
    .execute(&mut *tx)
    .await?;
    let bumped = enabled && !previous;
    if bumped {
        sqlx::query(
            "UPDATE space_event_heads
                SET generation = generation + 1, min_seq = head_seq + 1",
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    info!(enabled, bumped, "space event log flag synced");
    Ok(bumped)
}

/// The closed run-state vocabulary stored in `refs.state`. Mirror of the SQL function
/// `space_event_run_state`; when the run-state enum changes (audit conflict C17) both
/// change together, and a db test owes their agreement.
#[allow(dead_code)] // first caller is the stream's parity check (WS02-S3 / db test)
pub(crate) fn run_state(status: &str) -> &'static str {
    match status {
        "queued" => "queued",
        "running" => "running",
        "awaiting_approval" => "awaiting_approval",
        "completed" => "completed",
        "failed" => "failed",
        "cancelled" => "cancelled",
        _ => "other",
    }
}

/// One wake: something was committed to a Space's log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// A row at `seq` of `generation` was committed for this Space.
    Row {
        org_id: String,
        space_ref: String,
        seq: i64,
        generation: i64,
    },
    /// The listener (re)connected: notifications may have been missed, so every
    /// subscriber re-reads its Space from its own cursor.
    All,
}

/// In-process fan-out of wakes. A receiver that lags treats the lag as [`Wake::All`].
#[derive(Clone)]
pub(crate) struct SpaceWakes {
    tx: broadcast::Sender<Wake>,
}

impl SpaceWakes {
    pub(crate) fn new() -> Self {
        let (tx, _rx) = broadcast::channel(WAKE_CAPACITY);
        Self { tx }
    }

    #[allow(dead_code)] // first subscriber is the SpaceEvents stream (WS02-S3)
    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Wake> {
        self.tx.subscribe()
    }

    fn publish(&self, wake: Wake) {
        // No receivers is normal while no stream is open.
        let _ = self.tx.send(wake);
    }
}

fn parse_notification(payload: &str) -> Option<Wake> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    Some(Wake::Row {
        org_id: value.get("o")?.as_str()?.to_owned(),
        space_ref: value.get("s")?.as_str()?.to_owned(),
        seq: value.get("q")?.as_i64()?,
        generation: value.get("g")?.as_i64()?,
    })
}

/// Holds one `LISTEN` connection and publishes a wake per committed row. Returns an error
/// when the connection fails; the caller's supervised loop restarts it, and the restart
/// publishes [`Wake::All`].
pub(crate) async fn run_listener(pool: PgPool, wakes: SpaceWakes) -> Result<()> {
    let mut listener = PgListener::connect_with(&pool).await?;
    listener.listen(NOTIFY_CHANNEL).await?;
    info!(channel = NOTIFY_CHANNEL, "space event listener subscribed");
    wakes.publish(Wake::All);
    loop {
        match listener.try_recv().await? {
            Some(notification) => match parse_notification(notification.payload()) {
                Some(wake) => wakes.publish(wake),
                None => warn!("space event notification could not be decoded"),
            },
            None => {
                // The connection dropped and was re-established: anything committed in the
                // gap was not delivered.
                warn!("space event listener reconnected; waking every subscriber");
                wakes.publish(Wake::All);
            }
        }
    }
}

/// Deletes one batch of rows older than `retention_days` and moves `min_seq` of the Spaces
/// it touched. Lock order is log rows, then head, the same as the thread-delete trigger.
/// Returns how many rows it deleted.
pub(crate) async fn sweep_once(pool: &PgPool, retention_days: i64) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let deleted: Vec<(String, String)> = sqlx::query_as(
        "WITH doomed AS (
             SELECT org_id, space_ref, seq FROM space_event_log
              WHERE created_at < now() - make_interval(days => $1::int)
              ORDER BY created_at
              LIMIT $2
                FOR UPDATE SKIP LOCKED
         )
         DELETE FROM space_event_log l
           USING doomed d
          WHERE l.org_id = d.org_id AND l.space_ref = d.space_ref AND l.seq = d.seq
          RETURNING l.org_id, l.space_ref",
    )
    .bind(i32::try_from(retention_days).unwrap_or(i32::MAX))
    .bind(SWEEP_BATCH)
    .fetch_all(&mut *tx)
    .await?;
    let count = deleted.len() as u64;

    let mut spaces = deleted;
    spaces.sort_unstable();
    spaces.dedup();
    let (orgs, space_refs): (Vec<String>, Vec<String>) = spaces.into_iter().unzip();
    if !orgs.is_empty() {
        sqlx::query(
            "UPDATE space_event_heads h
                SET min_seq = GREATEST(
                    h.min_seq,
                    COALESCE(
                        (SELECT min(l.seq) FROM space_event_log l
                          WHERE l.org_id = h.org_id AND l.space_ref = h.space_ref),
                        h.head_seq + 1))
               FROM unnest($1::text[], $2::text[]) AS s(org_id, space_ref)
              WHERE h.org_id = s.org_id AND h.space_ref = s.space_ref",
        )
        .bind(&orgs)
        .bind(&space_refs)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(count)
}

/// Prunes old swallowed-error rows and exports the remaining counts per source word.
async fn report_errors(pool: &PgPool, retention_days: i64) -> Result<()> {
    sqlx::query("DELETE FROM space_event_errors WHERE at < now() - make_interval(days => $1::int)")
        .bind(i32::try_from(retention_days).unwrap_or(i32::MAX))
        .execute(pool)
        .await?;
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT source, count(*) FROM space_event_errors GROUP BY source")
            .fetch_all(pool)
            .await?;
    for (source, count) in rows {
        // Source words are the fixed set written by the trigger functions.
        metrics::gauge!("mp_session_space_event_errors", "source" => source).set(count as f64);
    }
    Ok(())
}

/// Every 15 minutes: sweep expired rows (all batches), then report swallowed errors. Runs
/// whether or not emitting is on, because retention is a duty of the data already written.
pub(crate) async fn run_retention_sweeper(pool: PgPool) -> Result<()> {
    let retention_days = retention_days_from_env();
    info!(retention_days, "space event log retention sweeper started");
    loop {
        loop {
            let deleted = sweep_once(&pool, retention_days).await?;
            if deleted > 0 {
                info!(deleted, "space event log rows swept");
            }
            if deleted < SWEEP_BATCH as u64 {
                break;
            }
        }
        if let Err(error) = report_errors(&pool, retention_days).await {
            warn!(%error, "space event error report failed");
        }
        tokio::time::sleep(SWEEP_INTERVAL).await;
    }
}

/// Erasure hook. Inside the caller's transaction: suppress the deferred triggers (so the
/// deletes that follow cannot re-create rows after the purge) and delete the Space's log
/// and head. The GUC is transaction-local, so it lasts to commit, where the deferred
/// triggers fire.
pub(crate) async fn purge_space(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    org_id: &str,
    space_ref: &str,
) -> Result<(), sqlx::Error> {
    suppress(tx).await?;
    sqlx::query("DELETE FROM space_event_log WHERE org_id = $1 AND space_ref = $2")
        .bind(org_id)
        .bind(space_ref)
        .execute(&mut **tx)
        .await?;
    sqlx::query("DELETE FROM space_event_heads WHERE org_id = $1 AND space_ref = $2")
        .bind(org_id)
        .bind(space_ref)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Turns the event triggers off for the rest of this transaction.
pub(crate) async fn suppress(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('space_event.suppress', 'on', true)")
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Why a stream must close.
#[allow(dead_code)] // consumed by the SpaceEvents stream (WS02-S3)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseCode {
    LeaseExpired,
    MembershipRevoked,
    Reauthorize,
    ZdrPollOnly,
}

/// What the stream does with one row.
#[allow(dead_code)] // consumed by the SpaceEvents stream (WS02-S3)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GateOutcome {
    Deliver,
    /// Sent as a `hidden` stub so the client's seq stays contiguous.
    Stub,
    Close(CloseCode),
}

/// The row facts the gate looks at.
#[allow(dead_code)]
pub(crate) struct GateRow<'a> {
    pub org_id: &'a str,
    pub scope: &'a str,
    pub audience_revision: Option<i64>,
    pub retention_class: &'a str,
}

/// The subscriber facts the gate looks at, taken from the verified Control decision, the
/// lease and the membership-revocation table.
#[allow(dead_code)]
pub(crate) struct GateContext<'a> {
    pub org_id: &'a str,
    pub now: DateTime<Utc>,
    pub lease_until: DateTime<Utc>,
    /// Highest `recipient_audience_revision` the decision authorizes.
    pub decision_ceiling: i64,
    pub decision_issued_at: DateTime<Utc>,
    /// `revoked_at` of an inactive `space_membership_revocations` row for this subject.
    pub revoked_at: Option<DateTime<Utc>>,
    pub caller_zdr: bool,
    pub decision_zdr: bool,
}

/// Clock-skew tolerance, in the fail-closed direction.
const REVOCATION_SKEW_SECONDS: i64 = 5;

/// Per-delivery gate: applied to every row and at every heartbeat (with the heartbeat's own
/// row facts, or the head's). Pure so it is unit-tested row by row. Order matters: the
/// stream-wide closes come before the per-row verdicts.
#[allow(dead_code)]
pub(crate) fn gate(row: &GateRow<'_>, ctx: &GateContext<'_>) -> GateOutcome {
    if ctx.caller_zdr || ctx.decision_zdr {
        return GateOutcome::Close(CloseCode::ZdrPollOnly);
    }
    if row.org_id != ctx.org_id {
        return GateOutcome::Close(CloseCode::MembershipRevoked);
    }
    if ctx.now >= ctx.lease_until {
        return GateOutcome::Close(CloseCode::LeaseExpired);
    }
    if let Some(revoked_at) = ctx.revoked_at {
        if revoked_at >= ctx.decision_issued_at - chrono::Duration::seconds(REVOCATION_SKEW_SECONDS)
        {
            return GateOutcome::Close(CloseCode::MembershipRevoked);
        }
    }
    if row.scope != "space" {
        return GateOutcome::Stub;
    }
    if !matches!(row.retention_class, "persistent" | "zdr_waived") {
        return GateOutcome::Stub;
    }
    match row.audience_revision {
        // A Space row always has a revision; one without cannot be judged, so it is hidden.
        None => GateOutcome::Stub,
        Some(revision) if revision > ctx.decision_ceiling => {
            GateOutcome::Close(CloseCode::Reauthorize)
        }
        Some(_) => GateOutcome::Deliver,
    }
}
