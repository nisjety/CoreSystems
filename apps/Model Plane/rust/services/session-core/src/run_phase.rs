//! Honest run phase and the per-thread run guard (Space build plan WS09-S1).
//!
//! Two things live here because they read the same fact: a run that Session Core
//! has started and not yet ended.
//!
//! * **Derived phase.** Session Core never writes `runs.status = 'running'`: a run
//!   is inserted `queued` and moves only to a terminal state. A live turn is
//!   therefore `queued` for its whole life, which a room shows as "Queued".
//!   [`run_phase_sql!`] derives the honest phase: a `queued` run whose one managed
//!   terminalization obligation is still open (`open` or `processing`) and inside
//!   its deadline is `running`. Nothing is written; readers select the expression.
//! * **Thread run guard.** A Space thread carries at most one live root run. The
//!   guard runs inside the start transaction, under a per-thread advisory lock, so
//!   two concurrent starts cannot both pass. A refused start is gRPC `Aborted`
//!   with the trailers [`ERROR_CODE_TRAILER`] = `thread_run_active` and
//!   [`ACTIVE_RUN_TRAILER`]. The check is a liveness predicate, never a bare
//!   "has a queued run": a producer that crashed leaves its obligation to expire at
//!   `MANAGED_DEADLINE_SECS`, and the guard releases then (or earlier by a Stop).
//!
//! The guard is read once at startup ([`set_thread_run_guard`]); it is a safety
//! restriction (it only refuses), never an authorization input.

use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::{Postgres, Transaction};
use tonic::metadata::MetadataValue;
use tonic::Status;

/// Env var that switches the guard. Unset means ON (the guard must ship ON before
/// member posting); `false`, `0`, `off` or `no` is the kill switch.
pub(crate) const THREAD_RUN_GUARD_ENV: &str = "SPACE_RUN_THREAD_GUARD";

/// Wire code of a refused start, on the gRPC trailer and in the SSE error frame.
pub(crate) const THREAD_RUN_ACTIVE_CODE: &str = "thread_run_active";

/// gRPC metadata key carrying the typed error code of a refusal.
pub(crate) const ERROR_CODE_TRAILER: &str = "x-mp-error-code";

/// gRPC metadata key carrying the id of the run that holds the thread.
pub(crate) const ACTIVE_RUN_TRAILER: &str = "x-mp-active-run-id";

/// SQL expression for a run's honest phase, over a `runs` row aliased `$alias`.
/// A macro expanding to a literal so it composes inside `concat!` constants.
macro_rules! run_phase_sql {
    ($alias:literal) => {
        concat!(
            "CASE WHEN ",
            $alias,
            ".status = 'queued' AND EXISTS (
                SELECT 1 FROM managed_run_terminalization_outbox AS rp_o
                WHERE rp_o.run_id = ",
            $alias,
            ".id
                  AND rp_o.state IN ('open', 'processing')
                  AND rp_o.deadline_at > now()
            ) THEN 'running' ELSE ",
            $alias,
            ".status END"
        )
    };
}
pub(crate) use run_phase_sql;

/// [`run_phase_sql!`] over the alias `r`, for a standalone query.
#[allow(dead_code)] // the DB tests (`run_phase_pg_*`) and later readers (WS10) use it
pub(crate) const RUN_PHASE_SQL: &str = run_phase_sql!("r");

static THREAD_RUN_GUARD: AtomicBool = AtomicBool::new(true);

/// Set the guard. Called once at startup from [`guard_enabled_from_env`].
pub(crate) fn set_thread_run_guard(enabled: bool) {
    THREAD_RUN_GUARD.store(enabled, Ordering::Relaxed);
}

/// Whether the guard refuses a second live root run on a Space thread.
pub(crate) fn thread_run_guard_enabled() -> bool {
    THREAD_RUN_GUARD.load(Ordering::Relaxed)
}

/// The guard setting from the environment: ON unless explicitly switched off.
pub(crate) fn guard_enabled_from_env() -> bool {
    match std::env::var(THREAD_RUN_GUARD_ENV) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "false" | "0" | "off" | "no"
        ),
        Err(_) => true,
    }
}

/// The live root run of a Space thread, if any. `FOR UPDATE` is not needed: the
/// advisory lock serializes every start on the thread.
pub(crate) const ACTIVE_ROOT_RUN_SQL: &str = "SELECT r.id
       FROM runs AS r
       JOIN threads AS t ON t.id = r.thread_id AND t.org_id = r.org_id
       JOIN managed_run_terminalization_outbox AS o ON o.run_id = r.id
      WHERE r.thread_id = $1
        AND r.org_id = $2
        AND r.parent_run_id IS NULL
        AND t.space_id IS NOT NULL
        AND r.status IN ('queued', 'running', 'awaiting_approval')
        AND o.configured_source IN ('gateway_direct', 'execution_agent')
        AND o.state IN ('open', 'processing')
        AND o.deadline_at > now()
      ORDER BY r.created_at DESC
      LIMIT 1";

/// The typed refusal of a start on a thread whose root run is still live.
pub(crate) fn thread_run_active_status(active_run_id: &str) -> Status {
    let mut status = Status::aborted("thread already has an active run");
    status.metadata_mut().insert(
        ERROR_CODE_TRAILER,
        MetadataValue::from_static(THREAD_RUN_ACTIVE_CODE),
    );
    if let Ok(value) = MetadataValue::try_from(active_run_id) {
        status.metadata_mut().insert(ACTIVE_RUN_TRAILER, value);
    }
    status
}

/// Refuse a start on a Space thread that already has a live root run.
///
/// Call inside the start transaction, after the idempotent start-key branch (a
/// replay of the same key returns its existing run and is never refused) and
/// after the thread id is resolved, before the run row is inserted. A no-op when
/// the guard is off. A non-Space thread, a terminal or expired run, a child run
/// and a browser-sourced run never hold the thread (see [`ACTIVE_ROOT_RUN_SQL`]).
#[allow(clippy::result_large_err)]
pub(crate) async fn ensure_thread_idle(
    transaction: &mut Transaction<'_, Postgres>,
    org_id: &str,
    thread_id: &str,
) -> Result<(), Status> {
    if !thread_run_guard_enabled() {
        return Ok(());
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('thread-run-slot' || chr(31) || $1, 0))")
        .bind(thread_id)
        .execute(&mut **transaction)
        .await
        .map_err(|error| Status::internal(format!("thread run guard lock failed: {error}")))?;
    let active: Option<String> = sqlx::query_scalar(ACTIVE_ROOT_RUN_SQL)
        .bind(thread_id)
        .bind(org_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| Status::internal(format!("thread run guard check failed: {error}")))?;
    match active {
        Some(run_id) => Err(thread_run_active_status(&run_id)),
        None => Ok(()),
    }
}

#[cfg(test)]
#[path = "run_phase_tests.rs"]
mod tests;
