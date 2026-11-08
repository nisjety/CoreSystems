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

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000 + secs, 0).expect("valid test time")
    }

    fn row(scope: &'static str, revision: Option<i64>, retention: &'static str) -> GateRow<'static> {
        GateRow {
            org_id: "org-1",
            scope,
            audience_revision: revision,
            retention_class: retention,
        }
    }

    /// A subscriber that may see revision 5 and below, with a lease that has 60 s left.
    fn ctx() -> GateContext<'static> {
        GateContext {
            org_id: "org-1",
            now: at(0),
            lease_until: at(60),
            decision_ceiling: 5,
            decision_issued_at: at(-10),
            revoked_at: None,
            caller_zdr: false,
            decision_zdr: false,
        }
    }

    #[test]
    fn gate_delivers_stubs_or_closes_per_row() {
        let c = ctx();
        let space = |rev, ret| row("space", rev, ret);
        // Deliver: a Space row of a persistent or waived run at or below the ceiling.
        assert_eq!(gate(&space(Some(5), "persistent"), &c), GateOutcome::Deliver);
        assert_eq!(gate(&space(Some(1), "persistent"), &c), GateOutcome::Deliver);
        assert_eq!(gate(&space(Some(5), "zdr_waived"), &c), GateOutcome::Deliver);
        // Newer than the decision authorizes: the stream must re-authorize, not skip.
        assert_eq!(
            gate(&space(Some(6), "persistent"), &c),
            GateOutcome::Close(CloseCode::Reauthorize)
        );
        // A row that cannot be judged is hidden, never delivered.
        assert_eq!(gate(&space(None, "persistent"), &c), GateOutcome::Stub);
        // An unknown scope (a later migration widens the CHECK) and an unknown retention class
        // are stubbed so the client's seq stays contiguous.
        assert_eq!(gate(&row("reaction", Some(1), "persistent"), &c), GateOutcome::Stub);
        assert_eq!(gate(&space(Some(1), "zdr"), &c), GateOutcome::Stub);
        assert_eq!(gate(&space(Some(1), ""), &c), GateOutcome::Stub);
    }

    #[test]
    fn gate_closes_the_whole_stream_before_judging_a_row() {
        // Another org: closed as a revocation, even for a row that would be hidden anyway.
        let other_org = GateRow { org_id: "org-2", ..row("reaction", None, "zdr") };
        assert_eq!(gate(&other_org, &ctx()), GateOutcome::Close(CloseCode::MembershipRevoked));

        // Lease: expired exactly at lease_until, not one second after.
        let mut c = ctx();
        c.now = at(60);
        assert_eq!(
            gate(&row("space", Some(1), "persistent"), &c),
            GateOutcome::Close(CloseCode::LeaseExpired)
        );
        c.now = at(59);
        assert_eq!(gate(&row("space", Some(1), "persistent"), &c), GateOutcome::Deliver);
        // A hidden row does not postpone the close.
        c.now = at(61);
        assert_eq!(
            gate(&row("reaction", None, "zdr"), &c),
            GateOutcome::Close(CloseCode::LeaseExpired)
        );
    }

    #[test]
    fn gate_revocation_uses_the_fail_closed_skew_window() {
        let mut c = ctx();
        // Issued at -10 s: a revocation up to 5 s before that still closes (clock skew).
        c.revoked_at = Some(at(-10));
        assert_eq!(
            gate(&row("space", Some(1), "persistent"), &c),
            GateOutcome::Close(CloseCode::MembershipRevoked)
        );
        c.revoked_at = Some(at(-15));
        assert_eq!(
            gate(&row("space", Some(1), "persistent"), &c),
            GateOutcome::Close(CloseCode::MembershipRevoked),
            "exactly at the skew edge still closes"
        );
        // A revocation older than the decision minus the skew predates the decision: the
        // decision was minted for a re-added member, so it does not close.
        c.revoked_at = Some(at(-16));
        assert_eq!(gate(&row("space", Some(1), "persistent"), &c), GateOutcome::Deliver);
        // A later revocation closes.
        c.revoked_at = Some(at(30));
        assert_eq!(
            gate(&row("space", Some(1), "persistent"), &c),
            GateOutcome::Close(CloseCode::MembershipRevoked)
        );
    }

    #[test]
    fn gate_zdr_takes_precedence_over_every_other_verdict() {
        let any_row = row("space", Some(1), "persistent");
        for (caller_zdr, decision_zdr) in [(true, false), (false, true), (true, true)] {
            let mut c = ctx();
            c.caller_zdr = caller_zdr;
            c.decision_zdr = decision_zdr;
            assert_eq!(gate(&any_row, &c), GateOutcome::Close(CloseCode::ZdrPollOnly));
            // Even when the lease is expired and the member is revoked.
            c.now = at(100);
            c.revoked_at = Some(at(50));
            assert_eq!(gate(&any_row, &c), GateOutcome::Close(CloseCode::ZdrPollOnly));
            let foreign = GateRow { org_id: "org-9", ..row("space", Some(1), "persistent") };
            assert_eq!(gate(&foreign, &c), GateOutcome::Close(CloseCode::ZdrPollOnly));
        }
    }

    #[test]
    fn run_state_maps_the_closed_vocabulary_and_everything_else_to_other() {
        for word in ["queued", "running", "awaiting_approval", "completed", "failed", "cancelled"] {
            assert_eq!(run_state(word), word);
        }
        for other in ["", "Running", "canceled", "parked", "timed_out", "zdr", " queued"] {
            assert_eq!(run_state(other), "other", "{other:?}");
        }
    }

    #[test]
    fn parse_notification_reads_ids_only_and_rejects_anything_malformed() {
        let wake = parse_notification(r#"{"o":"org-1","s":"space-1","q":42,"g":3}"#);
        assert_eq!(
            wake,
            Some(Wake::Row {
                org_id: "org-1".to_owned(),
                space_ref: "space-1".to_owned(),
                seq: 42,
                generation: 3,
            })
        );
        // Extra keys are ignored; the wake never carries them on.
        assert!(parse_notification(r#"{"o":"a","s":"b","q":1,"g":1,"text":"hi"}"#).is_some());
        for bad in [
            "",
            "not json",
            "[]",
            "{}",
            r#"{"o":"a","s":"b","q":1}"#,
            r#"{"o":"a","s":"b","g":1}"#,
            r#"{"o":1,"s":"b","q":1,"g":1}"#,
            r#"{"o":"a","s":"b","q":"1","g":1}"#,
            r#"{"o":"a","s":"b","q":1.5,"g":1}"#,
        ] {
            assert_eq!(parse_notification(bad), None, "{bad:?}");
        }
    }

    #[tokio::test]
    async fn wakes_fan_out_to_every_subscriber_and_tolerate_none() {
        let wakes = SpaceWakes::new();
        // No receivers is normal while no stream is open.
        wakes.publish(Wake::All);
        let mut first = wakes.subscribe();
        let mut second = wakes.subscribe();
        let row = parse_notification(r#"{"o":"o","s":"s","q":7,"g":1}"#).expect("wake");
        wakes.publish(row.clone());
        assert_eq!(first.recv().await.expect("first"), row);
        assert_eq!(second.recv().await.expect("second"), row);
    }

    #[test]
    fn env_switches_default_off_and_retention_is_at_least_one_day() {
        // These two variables are read by nothing else under test, so one test owns them.
        let saved = (
            std::env::var(EMIT_ENV).ok(),
            std::env::var(RETENTION_DAYS_ENV).ok(),
        );
        std::env::remove_var(EMIT_ENV);
        assert!(!emit_enabled_from_env(), "unset means off");
        for (value, expected) in [
            ("true", true),
            ("TRUE", true),
            ("1", true),
            (" 1 ", true),
            ("yes", false),
            ("on", false),
            ("0", false),
            ("", false),
            ("2", false),
        ] {
            std::env::set_var(EMIT_ENV, value);
            assert_eq!(emit_enabled_from_env(), expected, "{value:?}");
        }
        std::env::remove_var(RETENTION_DAYS_ENV);
        assert_eq!(retention_days_from_env(), DEFAULT_RETENTION_DAYS);
        for (value, expected) in [("30", 30), (" 7 ", 7), ("0", 1), ("-5", 1), ("abc", 14), ("", 14)] {
            std::env::set_var(RETENTION_DAYS_ENV, value);
            assert_eq!(retention_days_from_env(), expected, "{value:?}");
        }
        match saved.0 {
            Some(value) => std::env::set_var(EMIT_ENV, value),
            None => std::env::remove_var(EMIT_ENV),
        }
        match saved.1 {
            Some(value) => std::env::set_var(RETENTION_DAYS_ENV, value),
            None => std::env::remove_var(RETENTION_DAYS_ENV),
        }
    }
}

#[cfg(test)]
mod db_tests {
    //! Real-Postgres proof of the event log (`--ignored`, one thread at a time). Every test uses
    //! its own org and Space, switches the log on for the test database and cleans up after itself.

    use super::*;
    use crate::space::test_support::{
        clean_space, connect, seed_space_message, seed_space_run, seed_space_thread, set_event_log,
    };

    const NEEDS_DB: &str = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)";

    fn ids(prefix: &str) -> (String, String) {
        let suffix = mp_ids::new_ulid();
        (format!("org-{prefix}-{suffix}"), format!("space-{prefix}-{suffix}"))
    }

    async fn log_seqs(pool: &PgPool, org: &str, space: &str) -> Vec<i64> {
        sqlx::query_scalar("SELECT seq FROM space_event_log WHERE org_id = $1 AND space_ref = $2 ORDER BY seq")
            .bind(org)
            .bind(space)
            .fetch_all(pool)
            .await
            .expect("read log")
    }

    async fn head(pool: &PgPool, org: &str, space: &str) -> Option<(i64, i64, i64)> {
        sqlx::query_as(
            "SELECT head_seq, min_seq, generation FROM space_event_heads WHERE org_id = $1 AND space_ref = $2",
        )
        .bind(org)
        .bind(space)
        .fetch_optional(pool)
        .await
        .expect("read head")
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_seq_is_contiguous_under_20_concurrent_writers() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("conc");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        let mut writers = Vec::new();
        for n in 0..20 {
            let (pool, space, thread) = (pool.clone(), space.clone(), thread.clone());
            writers.push(tokio::spawn(async move {
                seed_space_message(&pool, &space, &thread, &format!("m-{space}-{n}"), "user", Some("member-2"))
                    .await;
            }));
        }
        for writer in writers {
            writer.await.expect("writer task");
        }
        let seqs = log_seqs(&pool, &org, &space).await;
        // One thread.created plus twenty message.appended, numbered 1..=21 with no hole.
        assert_eq!(seqs, (1..=21).collect::<Vec<i64>>(), "{NEEDS_DB}");
        let (head_seq, min_seq, _) = head(&pool, &org, &space).await.expect("head");
        assert_eq!((head_seq, min_seq), (21, 1));
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_rollback_leaves_no_hole() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("roll");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        // A message inside a transaction that rolls back takes no seq.
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(
            "INSERT INTO messages (id, thread_id, role, content, space_id, recipient_audience_ref, \
             recipient_audience_revision, recipient_audience_hash, authority_revision, \
             resource_authorization_ref, author_subject_id) \
             VALUES ($1, $2, 'user', 'x', $3, 'audience-1', 1, 'sha256:audience-1', 1, 'resource-1', 'member-2')",
        )
        .bind(format!("m-rolled-{space}"))
        .bind(&thread)
        .bind(&space)
        .execute(&mut *tx)
        .await
        .expect("insert");
        tx.rollback().await.expect("rollback");
        seed_space_message(&pool, &space, &thread, &format!("m-kept-{space}"), "user", Some("member-2")).await;
        assert_eq!(log_seqs(&pool, &org, &space).await, vec![1, 2]);
        assert_eq!(head(&pool, &org, &space).await.map(|h| h.0), Some(2));
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_notify_fires_only_after_commit() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("notify");
        let thread = format!("thread-{space}");
        let mut listener = PgListener::connect_with(&pool).await.expect("listener");
        listener.listen(NOTIFY_CHANNEL).await.expect("listen");
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id, origin, space_id, space_decision_ref, \
             recipient_audience_ref, recipient_audience_revision, recipient_audience_hash, \
             privacy_policy_ref, resource_authorization_ref, authority_revision) \
             VALUES ($1, $1, $2, 'owner-1', 'space', $3, 'decision-1', 'audience-1', 1, 'sha256:audience-1', \
             'privacy-1', 'resource-1', 1)",
        )
        .bind(&thread)
        .bind(&org)
        .bind(&space)
        .execute(&mut *tx)
        .await
        .expect("insert thread");
        // Uncommitted: nothing is delivered, whatever the deferred trigger has queued.
        let early = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let notification = listener.recv().await.expect("recv");
                if notification.payload().contains(&space) {
                    return notification;
                }
            }
        })
        .await;
        assert!(early.is_err(), "a NOTIFY arrived before COMMIT");
        tx.commit().await.expect("commit");
        let notification = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notification = listener.recv().await.expect("recv");
                if notification.payload().contains(&space) {
                    return notification;
                }
            }
        })
        .await
        .expect("a NOTIFY after COMMIT");
        match parse_notification(notification.payload()).expect("payload decodes") {
            Wake::Row { org_id, space_ref, seq, generation } => {
                assert_eq!((org_id.as_str(), space_ref.as_str(), seq), (org.as_str(), space.as_str(), 1));
                assert!(generation >= 1);
            }
            Wake::All => panic!("a row wake was expected"),
        }
        // The payload is ids only.
        assert!(!notification.payload().contains("canary"));
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_trigger_failure_is_swallowed_and_counted() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("fail");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        // Plant a row at the next seq so the trigger's log INSERT hits the primary key.
        sqlx::query("UPDATE space_event_heads SET head_seq = 5 WHERE org_id = $1 AND space_ref = $2")
            .bind(&org)
            .bind(&space)
            .execute(&pool)
            .await
            .expect("move head");
        sqlx::query(
            "INSERT INTO space_event_log (org_id, space_ref, seq, generation, kind, actor_type) \
             VALUES ($1, $2, 6, 1, 'message.appended', 'user')",
        )
        .bind(&org)
        .bind(&space)
        .execute(&pool)
        .await
        .expect("plant row");
        let before: i64 =
            sqlx::query_scalar("SELECT count(*) FROM space_event_errors WHERE source = 'messages_insert'")
                .fetch_one(&pool)
                .await
                .expect("count errors");
        // The post itself commits; the event does not.
        let message_id = format!("m-fail-{space}");
        seed_space_message(&pool, &space, &thread, &message_id, "user", Some("member-2")).await;
        let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM messages WHERE id = $1")
            .bind(&message_id)
            .fetch_one(&pool)
            .await
            .expect("count message");
        assert_eq!(stored, 1, "a realtime defect must never block a post");
        let after: Vec<(String, String)> =
            sqlx::query_as("SELECT source, sqlstate FROM space_event_errors WHERE source = 'messages_insert'")
                .fetch_all(&pool)
                .await
                .expect("read errors");
        assert_eq!(after.len() as i64, before + 1);
        assert!(after.iter().any(|(_, state)| state == "23505"), "{after:?}");
        // The swallowed row rolled its head increment back: no hole is left behind.
        assert_eq!(head(&pool, &org, &space).await.map(|h| h.0), Some(5));
        // The error rows carry a source word and a sqlstate, never an id.
        let leaked: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM space_event_errors WHERE source LIKE '%' || $1 || '%'",
        )
        .bind(&space)
        .fetch_one(&pool)
        .await
        .expect("count leaks");
        assert_eq!(leaked, 0);
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_refs_allowlist_rejects_an_unknown_key() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("refs");
        let emit = |refs: &'static str| {
            let (pool, org, space) = (pool.clone(), org.clone(), space.clone());
            async move {
                sqlx::query_scalar::<_, Option<i64>>(
                    "SELECT space_event_emit($1, $2, 'thread.updated', 'system', NULL, NULL, NULL, NULL, 1, 'persistent', $3::jsonb)",
                )
                .bind(&org)
                .bind(&space)
                .bind(serde_json::from_str::<serde_json::Value>(refs).expect("json"))
                .fetch_one(&pool)
                .await
            }
        };
        assert_eq!(emit(r#"{"fields":["title"]}"#).await.expect("allowed key"), Some(1));
        assert!(emit(r#"{"title":"leaked words"}"#).await.is_err(), "text keys are refused");
        assert!(emit(r#"["fields"]"#).await.is_err(), "refs must be an object");
        // A zero-retention class is refused by the chokepoint, not an error.
        let zdr: Option<i64> = sqlx::query_scalar(
            "SELECT space_event_emit($1, $2, 'thread.updated', 'system', NULL, NULL, NULL, NULL, 1, 'zdr', '{}'::jsonb)",
        )
        .bind(&org)
        .bind(&space)
        .fetch_one(&pool)
        .await
        .expect("zdr emit");
        assert_eq!(zdr, None);
        // The 512-byte CHECK on refs is the third gate.
        let big = format!("{{\"fields\":[\"{}\"]}}", "x".repeat(600));
        let oversize = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT space_event_emit($1, $2, 'thread.updated', 'system', NULL, NULL, NULL, NULL, 1, 'persistent', $3::jsonb)",
        )
        .bind(&org)
        .bind(&space)
        .bind(serde_json::from_str::<serde_json::Value>(&big).expect("json"))
        .fetch_one(&pool)
        .await;
        assert!(oversize.is_err());
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_sweeper_moves_min_seq() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("sweep");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        for n in 0..3 {
            seed_space_message(&pool, &space, &thread, &format!("m-{space}-{n}"), "user", Some("member-2")).await;
        }
        assert_eq!(log_seqs(&pool, &org, &space).await, vec![1, 2, 3, 4]);
        // Age the first two rows past the window.
        sqlx::query(
            "UPDATE space_event_log SET created_at = now() - interval '30 days' \
             WHERE org_id = $1 AND space_ref = $2 AND seq <= 2",
        )
        .bind(&org)
        .bind(&space)
        .execute(&pool)
        .await
        .expect("age rows");
        let deleted = sweep_once(&pool, 14).await.expect("sweep");
        assert!(deleted >= 2, "deleted {deleted}");
        assert_eq!(log_seqs(&pool, &org, &space).await, vec![3, 4]);
        let (head_seq, min_seq, _) = head(&pool, &org, &space).await.expect("head");
        assert_eq!((head_seq, min_seq), (4, 3), "min_seq follows the oldest surviving row");
        // Everything aged out: min_seq moves above the head, seq itself is not reused.
        sqlx::query("UPDATE space_event_log SET created_at = now() - interval '30 days' WHERE org_id = $1 AND space_ref = $2")
            .bind(&org)
            .bind(&space)
            .execute(&pool)
            .await
            .expect("age all");
        sweep_once(&pool, 14).await.expect("sweep all");
        assert!(log_seqs(&pool, &org, &space).await.is_empty());
        let (head_seq, min_seq, _) = head(&pool, &org, &space).await.expect("head");
        assert_eq!((head_seq, min_seq), (4, 5));
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_suppress_prevents_rows_during_purge() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("purge");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        seed_space_message(&pool, &space, &thread, &format!("m-{space}"), "user", Some("member-2")).await;
        assert_eq!(log_seqs(&pool, &org, &space).await.len(), 2);
        // The erasure shape: purge the log, then delete the data, in one transaction. The
        // deferred delete trigger fires at COMMIT and must find the suppress setting still on.
        let mut tx = pool.begin().await.expect("begin");
        purge_space(&mut tx, &org, &space).await.expect("purge");
        sqlx::query("DELETE FROM messages WHERE thread_id = $1")
            .bind(&thread)
            .execute(&mut *tx)
            .await
            .expect("delete messages");
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread)
            .execute(&mut *tx)
            .await
            .expect("delete thread");
        tx.commit().await.expect("commit");
        assert!(log_seqs(&pool, &org, &space).await.is_empty(), "no row survives or is re-created");
        assert!(head(&pool, &org, &space).await.is_none(), "the head is gone too");
        // Outside a purge the same delete leaves a tombstone: suppression is per transaction.
        let thread2 = format!("thread2-{space}");
        seed_space_thread(&pool, &org, &space, &thread2, "owner-1").await;
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread2)
            .execute(&pool)
            .await
            .expect("plain delete");
        let kinds: Vec<String> =
            sqlx::query_scalar("SELECT kind FROM space_event_log WHERE org_id = $1 AND space_ref = $2 ORDER BY seq")
                .bind(&org)
                .bind(&space)
                .fetch_all(&pool)
                .await
                .expect("kinds");
        assert!(kinds.iter().any(|kind| kind == "thread.deleted"), "{kinds:?}");
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_no_row_for_a_zdr_run() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("zdr");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        let run_kinds = |pool: PgPool, org: String, space: String| async move {
            sqlx::query_as::<_, (String, Option<String>)>(
                "SELECT kind, run_id FROM space_event_log WHERE org_id = $1 AND space_ref = $2 AND kind = 'run.state_changed' ORDER BY seq",
            )
            .bind(&org)
            .bind(&space)
            .fetch_all(&pool)
            .await
            .expect("run rows")
        };
        let zdr_run = format!("run-zdr-{space}");
        seed_space_run(&pool, &org, &space, &thread, &zdr_run, "queued", "zdr").await;
        sqlx::query("UPDATE runs SET status = 'running' WHERE id = $1")
            .bind(&zdr_run)
            .execute(&pool)
            .await
            .expect("move zdr run");
        assert!(run_kinds(pool.clone(), org.clone(), space.clone()).await.is_empty(), "a zdr run leaves no row");
        // A persistent run does: one row on insert, one per state-word move, none for a no-op.
        let run = format!("run-ok-{space}");
        seed_space_run(&pool, &org, &space, &thread, &run, "queued", "persistent").await;
        sqlx::query("UPDATE runs SET status = 'running' WHERE id = $1")
            .bind(&run)
            .execute(&pool)
            .await
            .expect("move run");
        sqlx::query("UPDATE runs SET status = 'running', goal = 'again' WHERE id = $1")
            .bind(&run)
            .execute(&pool)
            .await
            .expect("no-op status");
        let rows = run_kinds(pool.clone(), org.clone(), space.clone()).await;
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.iter().all(|(_, id)| id.as_deref() == Some(run.as_str())));
        let states: Vec<serde_json::Value> = sqlx::query_scalar(
            "SELECT refs FROM space_event_log WHERE org_id = $1 AND space_ref = $2 AND kind = 'run.state_changed' ORDER BY seq",
        )
        .bind(&org)
        .bind(&space)
        .fetch_all(&pool)
        .await
        .expect("refs");
        assert_eq!(states[0]["state"], "queued");
        assert_eq!(states[1]["state"], "running");
        assert_eq!(states[1]["prev"], "queued");
        // Never text: only closed words and ids.
        for refs in &states {
            let keys: Vec<&str> = refs.as_object().expect("object").keys().map(String::as_str).collect();
            assert!(keys.iter().all(|k| ["state", "prev", "agent_id", "parent_run_id"].contains(k)), "{keys:?}");
        }
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_replay_after_thread_delete_reads_redacted() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, true).await;
        let (org, space) = ids("redact");
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        seed_space_message(&pool, &space, &thread, &format!("m-{space}"), "user", Some("member-2")).await;
        let mut tx = pool.begin().await.expect("begin");
        sqlx::query("DELETE FROM messages WHERE thread_id = $1")
            .bind(&thread)
            .execute(&mut *tx)
            .await
            .expect("delete messages");
        sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(&thread)
            .execute(&mut *tx)
            .await
            .expect("delete thread");
        tx.commit().await.expect("commit");
        type Replayed = (i64, String, String, Option<String>, Option<String>, Option<String>);
        let rows: Vec<Replayed> = sqlx::query_as(
            "SELECT seq, kind, actor_type, actor_subject_id, thread_id, message_id FROM space_event_log \
             WHERE org_id = $1 AND space_ref = $2 ORDER BY seq",
        )
        .bind(&org)
        .bind(&space)
        .fetch_all(&pool)
        .await
        .expect("replay");
        // thread.created and message.appended read as redacted (ids and actor erased, seq kept),
        // then the tombstone keeps only the thread id.
        assert_eq!(rows.len(), 3, "{rows:?}");
        for (_, kind, actor_type, actor, thread_id, message_id) in &rows[..2] {
            assert_eq!(kind, "redacted");
            assert_eq!(actor_type, "system");
            assert!(actor.is_none() && thread_id.is_none() && message_id.is_none());
        }
        assert_eq!(rows[2].1, "thread.deleted");
        assert_eq!(rows[2].4.as_deref(), Some(thread.as_str()));
        assert!(rows[2].3.is_none() && rows[2].5.is_none());
        // Subject erasure drops the actor id from the surviving rows too.
        let thread2 = format!("thread2-{space}");
        seed_space_thread(&pool, &org, &space, &thread2, "owner-1").await;
        seed_space_message(&pool, &space, &thread2, &format!("m2-{space}"), "user", Some("member-3")).await;
        let forgotten: i64 = sqlx::query_scalar("SELECT space_event_forget_subject($1, 'member-3')")
            .bind(&org)
            .fetch_one(&pool)
            .await
            .expect("forget");
        assert_eq!(forgotten, 1);
        clean_space(&pool, &org, &space).await;
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_run_state_agrees_with_events_run_state() {
        let Some(pool) = connect().await else { return };
        for status in [
            "queued", "running", "awaiting_approval", "completed", "failed", "cancelled", "parked",
            "canceled", "", "Running", "timed_out",
        ] {
            let sql: String = sqlx::query_scalar("SELECT space_event_run_state($1)")
                .bind(status)
                .fetch_one(&pool)
                .await
                .expect("sql run state");
            assert_eq!(sql, run_state(status), "status {status:?}");
        }
    }

    #[tokio::test]
    #[ignore = "needs DB harness (DATABASE_URL to a disposable *test* Postgres)"]
    async fn space_event_sync_flag_bumps_the_generation_only_on_an_off_to_on_move() {
        let Some(pool) = connect().await else { return };
        set_event_log(&pool, false).await;
        let (org, space) = ids("flag");
        set_event_log(&pool, true).await;
        let thread = format!("thread-{space}");
        seed_space_thread(&pool, &org, &space, &thread, "owner-1").await;
        let (head_seq, min_seq, generation) = head(&pool, &org, &space).await.expect("head");
        assert_eq!((head_seq, min_seq), (1, 1));
        // Already on: nothing moves.
        assert!(!sync_flag(&pool, true).await.expect("sync on"));
        // Off, and nothing is written while off.
        assert!(!sync_flag(&pool, false).await.expect("sync off"));
        seed_space_message(&pool, &space, &thread, &format!("m-{space}"), "user", Some("member-2")).await;
        assert_eq!(log_seqs(&pool, &org, &space).await, vec![1], "no row while off");
        // Off to on: generation up, min_seq above the old head, seq stays monotonic.
        assert!(sync_flag(&pool, true).await.expect("sync on again"));
        let (head_seq, min_seq, new_generation) = head(&pool, &org, &space).await.expect("head");
        assert_eq!(new_generation, generation + 1);
        assert_eq!((head_seq, min_seq), (1, 2));
        clean_space(&pool, &org, &space).await;
    }
}
