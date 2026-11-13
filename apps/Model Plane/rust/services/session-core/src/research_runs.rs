//! Research runs: the durable, addressable research phase of a chat turn (Quarry v2 E4.6, slice Q4-062b;
//! plan 3.4 E4.6 scope (b)).
//!
//! A research run is one row in `research_runs` (migration 0129). A client addresses it by id to read its status,
//! partial results and stop reason, and a second research run on the same thread is refused (`BeginError::Active`,
//! which the gRPC service answers as aborted `research_run_active`, naming the run only to its owner). The store
//! functions come first; the gRPC service [`ResearchRunGrpc`] (`ResearchRunService`, at the end of this file) is their
//! first caller. It is behind `RESEARCH_RUNS_ENABLED` (literal true only), and while that is off every RPC answers
//! `unavailable` with `research_runs_disabled`. Begin records a run and runs nothing: the execution loop stays
//! in-process and reports to the service as it goes. No model is called and no content is sent from here. The gateway
//! that calls the service is gated by `MODEL_GATEWAY_RESEARCH_RUNS` in a later slice, so until then only the gRPC
//! service calls the store.
//!
//! Status: `running` is the only state that moves. Every write is guarded on `status = 'running'`, so an ended run
//! never changes again (see [`Status::may_move_to`]).
//!
//! Zero Data Retention: the caller passes `zdr` as `zdr() || zdr_waiver().is_some()`, the same test the event trigger
//! RPCs apply. A ZDR or waived run keeps ids, status, codes and counters only. `partial` and `report_artifact_id` are
//! dropped for such a row inside the same statement that would have written them, and the table's CHECK refuses them
//! too.
//!
//! Erasure: the thread erase (`grpc::delete_thread_rows`, which the retention sweep also calls) deletes a thread's
//! rows, and the org purge (`gdpr::purge_organization_data`) deletes an organisation's rows. The held-scope list names
//! the table, so a legal hold that blocks a thread erase keeps it.

use chrono::{DateTime, TimeDelta, Utc};
use mp_contracts::model_plane::v1::{self as pb, research_run_service_server::ResearchRunService};
use mp_contracts::zdr_refusal::Refusal;
use mp_ids::new_ulid;
use sqlx::PgPool;
use std::time::Instant;
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Status as RpcStatus};

use crate::auth::{authorize_operation, identity, VerifiedIdentity};
use crate::grpc::{record_metrics, to_proto_timestamp};
use crate::orchestration_grpc::{json_to_struct, struct_to_json};
use crate::run_phase::{ACTIVE_RUN_TRAILER, ERROR_CODE_TRAILER};

/// The stop reason a run gets when its deadline passes before it ends.
pub(crate) const STOP_REASON_DEADLINE: &str = "deadline_exceeded";

/// The status of a research run. The words are the table's CHECK vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Running,
    Completed,
    Stopped,
    Failed,
    Cancelled,
    Expired,
}

impl Status {
    /// The stored word.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    /// The status a stored word names, or `None` for any other word.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "stopped" => Self::Stopped,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            "expired" => Self::Expired,
            _ => return None,
        })
    }

    /// Every status but `Running` ends a run.
    pub(crate) const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }

    /// The whole state machine: a running run may end in any terminal status, and nothing else moves. A terminal
    /// status is final.
    pub(crate) const fn may_move_to(self, next: Self) -> bool {
        matches!(self, Self::Running) && next.is_terminal()
    }
}

/// The budget a run was given. `None` means the dimension was not set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Budget {
    pub(crate) pages: Option<i32>,
    pub(crate) sub_queries: Option<i32>,
    pub(crate) usd_micro: Option<i64>,
}

/// One stored research run, as the store reads it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ResearchRun {
    /// The addressable id (`rsr_` and a ULID). A client reads the run by it, and a 409 names it.
    pub(crate) id: String,
    pub(crate) org_id: String,
    pub(crate) user_id: String,
    pub(crate) thread_id: String,
    /// The chat session run this research phase belongs to.
    pub(crate) run_id: String,
    pub(crate) request_id: Option<String>,
    pub(crate) status: Status,
    pub(crate) stop_reason: Option<String>,
    pub(crate) halt_code: Option<String>,
    pub(crate) zdr: bool,
    pub(crate) budget: Budget,
    pub(crate) spent_usd_micro: i64,
    pub(crate) pages_read: i32,
    pub(crate) sources_found: i32,
    pub(crate) partial: Option<serde_json::Value>,
    pub(crate) report_artifact_id: Option<String>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) deadline_at: DateTime<Utc>,
}

/// The columns a run is read with, in [`RunRow`]'s order.
const RUN_COLUMNS: &str = "id, org_id, user_id, thread_id, run_id, request_id, status, stop_reason, halt_code, \
     zdr, budget_pages, budget_sub_queries, budget_usd_micro, spent_usd_micro, pages_read, sources_found, partial, \
     report_artifact_id, created_at, updated_at, deadline_at";

#[derive(Debug, sqlx::FromRow)]
struct RunRow {
    id: String,
    org_id: String,
    user_id: String,
    thread_id: String,
    run_id: String,
    request_id: Option<String>,
    status: String,
    stop_reason: Option<String>,
    halt_code: Option<String>,
    zdr: bool,
    budget_pages: Option<i32>,
    budget_sub_queries: Option<i32>,
    budget_usd_micro: Option<i64>,
    spent_usd_micro: i64,
    pages_read: i32,
    sources_found: i32,
    partial: Option<serde_json::Value>,
    report_artifact_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deadline_at: DateTime<Utc>,
}

impl RunRow {
    fn into_run(self) -> Result<ResearchRun, sqlx::Error> {
        let status = Status::parse(&self.status).ok_or_else(|| {
            sqlx::Error::Protocol(format!("research run {} holds an unknown status", self.id))
        })?;
        Ok(ResearchRun {
            id: self.id,
            org_id: self.org_id,
            user_id: self.user_id,
            thread_id: self.thread_id,
            run_id: self.run_id,
            request_id: self.request_id,
            status,
            stop_reason: self.stop_reason,
            halt_code: self.halt_code,
            zdr: self.zdr,
            budget: Budget {
                pages: self.budget_pages,
                sub_queries: self.budget_sub_queries,
                usd_micro: self.budget_usd_micro,
            },
            spent_usd_micro: self.spent_usd_micro,
            pages_read: self.pages_read,
            sources_found: self.sources_found,
            partial: self.partial,
            report_artifact_id: self.report_artifact_id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deadline_at: self.deadline_at,
        })
    }
}

/// What `begin` records. `zdr` is the caller's retention test (see the module docs).
#[derive(Debug, Clone, Copy)]
pub(crate) struct NewRun<'a> {
    pub(crate) org_id: &'a str,
    pub(crate) user_id: &'a str,
    pub(crate) thread_id: &'a str,
    /// The chat session run this research phase belongs to.
    pub(crate) run_id: &'a str,
    pub(crate) request_id: Option<&'a str>,
    pub(crate) zdr: bool,
    pub(crate) budget: Budget,
    pub(crate) deadline_at: DateTime<Utc>,
}

/// Why `begin` did not start a run.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BeginError {
    /// The thread already has a running research run. `run_id` is that run's addressable id (the `id` column) and
    /// `user_id` its owner. The gRPC service names the run to its owner only (see [`begin_status`]).
    #[error("a research run is already running on this thread: {run_id}")]
    Active { run_id: String, user_id: String },
    /// The thread is not in the caller's organisation, does not belong to the run's owner, or is gone. The gRPC service
    /// answers it as not found (see [`OWNED_THREAD`]).
    #[error("the research run's thread is not found")]
    ThreadNotFound,
    #[error(transparent)]
    Store(#[from] sqlx::Error),
}

/// A progress report. The counters only move up, so a report that arrives late cannot lower them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Progress<'a> {
    pub(crate) pages_read: i32,
    pub(crate) sources_found: i32,
    pub(crate) spent_usd_micro: i64,
    /// The partial results so far. `Some` replaces the stored value and `None` keeps it. Dropped for a ZDR run.
    pub(crate) partial: Option<&'a serde_json::Value>,
}

/// How a run ends: its terminal status and the codes, partial results and report artifact it ends with.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunEnd<'a> {
    /// Must be terminal. `Running` is refused with [`FinishError::NotTerminal`].
    pub(crate) status: Status,
    pub(crate) stop_reason: Option<&'a str>,
    pub(crate) halt_code: Option<&'a str>,
    /// The final partial results, under the same rule as [`Progress::partial`].
    pub(crate) partial: Option<&'a serde_json::Value>,
    /// The report artifact the run wrote. Dropped for a ZDR run.
    pub(crate) report_artifact_id: Option<&'a str>,
}

/// Why `finish` did not end a run.
#[derive(Debug, thiserror::Error)]
pub(crate) enum FinishError {
    #[error("a research run ends in a terminal status, not running")]
    NotTerminal,
    #[error(transparent)]
    Store(#[from] sqlx::Error),
}

/// What a progress report or an end did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteOutcome {
    /// The run was running, and the write applied.
    Applied,
    /// The run had already ended or expired, or it is not in this organisation. Nothing was written.
    NotRunning,
}

fn outcome(rows: u64) -> WriteOutcome {
    if rows == 0 {
        WriteOutcome::NotRunning
    } else {
        WriteOutcome::Applied
    }
}

/// The expiry of one thread's runs past their deadline, inside `begin`'s transaction.
const EXPIRE_IN_THREAD: &str = "UPDATE research_runs
    SET status = 'expired', stop_reason = $1, updated_at = now()
  WHERE status = 'running' AND deadline_at < now() AND org_id = $2 AND thread_id = $3";

/// The expiry of every run past its deadline, for the sweep.
const EXPIRE_STALE: &str = "UPDATE research_runs
    SET status = 'expired', stop_reason = $1, updated_at = now()
  WHERE status = 'running' AND deadline_at < now()";

/// Starts a research run on a thread, or refuses with the id of the run already running there.
///
/// One transaction. Its first statement is the thread's owner check, which locks the thread's row (see
/// [`OWNED_THREAD`]), so the check and the insert are atomic with respect to a thread erase. A per-thread advisory lock
/// makes concurrent begins for the thread take turns; the thread's running runs past their deadline are expired; and if
/// a running run remains, the new one is refused. The partial unique index is the backstop, so a begin that somehow
/// raced past the lock fails on the insert rather than leaving two running runs.
pub(crate) async fn begin(pool: &PgPool, new: NewRun<'_>) -> Result<ResearchRun, BeginError> {
    let mut tx = pool.begin().await?;
    let owned: Option<(String,)> = sqlx::query_as(OWNED_THREAD)
        .bind(new.thread_id)
        .bind(new.org_id)
        .bind(new.user_id)
        .fetch_optional(&mut *tx)
        .await?;
    if owned.is_none() {
        return Err(BeginError::ThreadNotFound);
    }
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('research-run-slot' || chr(31) || $1::text, 0))")
        .bind(new.thread_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(EXPIRE_IN_THREAD)
        .bind(STOP_REASON_DEADLINE)
        .bind(new.org_id)
        .bind(new.thread_id)
        .execute(&mut *tx)
        .await?;
    let running: Option<(String, String)> = sqlx::query_as(
        "SELECT id, user_id FROM research_runs
          WHERE org_id = $1 AND thread_id = $2 AND status = 'running'
          LIMIT 1",
    )
    .bind(new.org_id)
    .bind(new.thread_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((run_id, user_id)) = running {
        // Committed, so the expiry above stands. It only ends runs whose deadline has passed.
        tx.commit().await?;
        return Err(BeginError::Active { run_id, user_id });
    }
    let id = format!("rsr_{}", new_ulid());
    let row: RunRow = sqlx::query_as(&format!(
        "INSERT INTO research_runs
             (id, org_id, user_id, thread_id, run_id, request_id, status, zdr,
              budget_pages, budget_sub_queries, budget_usd_micro, deadline_at)
         VALUES ($1, $2, $3, $4, $5, $6, 'running', $7, $8, $9, $10, $11)
         RETURNING {RUN_COLUMNS}"
    ))
    .bind(&id)
    .bind(new.org_id)
    .bind(new.user_id)
    .bind(new.thread_id)
    .bind(new.run_id)
    .bind(new.request_id)
    .bind(new.zdr)
    .bind(new.budget.pages)
    .bind(new.budget.sub_queries)
    .bind(new.budget.usd_micro)
    .bind(new.deadline_at)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row.into_run()?)
}

/// Records a progress report for a run of an organisation. `NotRunning` once the run has ended.
pub(crate) async fn record_progress(
    pool: &PgPool,
    org_id: &str,
    id: &str,
    progress: Progress<'_>,
) -> Result<WriteOutcome, sqlx::Error> {
    let rows = sqlx::query(
        "UPDATE research_runs
            SET pages_read = GREATEST(pages_read, $3),
                sources_found = GREATEST(sources_found, $4),
                spent_usd_micro = GREATEST(spent_usd_micro, $5),
                partial = CASE WHEN zdr THEN NULL ELSE COALESCE($6::jsonb, partial) END,
                updated_at = now()
          WHERE id = $1 AND org_id = $2 AND status = 'running'",
    )
    .bind(id)
    .bind(org_id)
    .bind(progress.pages_read)
    .bind(progress.sources_found)
    .bind(progress.spent_usd_micro)
    .bind(progress.partial)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(outcome(rows))
}

/// Ends a run of an organisation with a terminal status.
///
/// `FinishError::NotTerminal` when `end.status` is `Running`. `WriteOutcome::NotRunning` when the run has already ended
/// (a second end, an expiry, a thread erase) and nothing changed.
pub(crate) async fn finish(
    pool: &PgPool,
    org_id: &str,
    id: &str,
    end: RunEnd<'_>,
) -> Result<WriteOutcome, FinishError> {
    if !Status::Running.may_move_to(end.status) {
        return Err(FinishError::NotTerminal);
    }
    let rows = sqlx::query(
        "UPDATE research_runs
            SET status = $3,
                stop_reason = $4,
                halt_code = $5,
                partial = CASE WHEN zdr THEN NULL ELSE COALESCE($6::jsonb, partial) END,
                report_artifact_id = CASE WHEN zdr THEN NULL ELSE COALESCE($7, report_artifact_id) END,
                updated_at = now()
          WHERE id = $1 AND org_id = $2 AND status = 'running'",
    )
    .bind(id)
    .bind(org_id)
    .bind(end.status.as_str())
    .bind(end.stop_reason)
    .bind(end.halt_code)
    .bind(end.partial)
    .bind(end.report_artifact_id)
    .execute(pool)
    .await?
    .rows_affected();
    Ok(outcome(rows))
}

/// Reads one run of an organisation. Another organisation's run reads as absent. The caller checks that the run belongs
/// to its user.
pub(crate) async fn get(
    pool: &PgPool,
    org_id: &str,
    id: &str,
) -> Result<Option<ResearchRun>, sqlx::Error> {
    let row: Option<RunRow> = sqlx::query_as(&format!(
        "SELECT {RUN_COLUMNS} FROM research_runs WHERE id = $1 AND org_id = $2"
    ))
    .bind(id)
    .bind(org_id)
    .fetch_optional(pool)
    .await?;
    row.map(RunRow::into_run).transpose()
}

/// The run of a thread that is still running and inside its deadline, if there is one.
pub(crate) async fn active_for_thread(
    pool: &PgPool,
    org_id: &str,
    thread_id: &str,
) -> Result<Option<ResearchRun>, sqlx::Error> {
    let row: Option<RunRow> = sqlx::query_as(&format!(
        "SELECT {RUN_COLUMNS} FROM research_runs
          WHERE org_id = $1 AND thread_id = $2 AND status = 'running' AND deadline_at >= now()"
    ))
    .bind(org_id)
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;
    row.map(RunRow::into_run).transpose()
}

/// Ends every running run whose deadline has passed, in every organisation. Returns how many it ended.
pub(crate) async fn expire_stale(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let rows = sqlx::query(EXPIRE_STALE)
        .bind(STOP_REASON_DEADLINE)
        .execute(pool)
        .await?
        .rows_affected();
    Ok(rows)
}

/// How often the expiry sweep ends the runs past their deadline. A run whose finish was lost reads as running for at
/// most this long after its deadline.
const EXPIRY_SWEEP_SECONDS: u64 = 5;

/// The expiry sweep loop: once per cadence it ends every running run past its deadline, in every organisation, and logs
/// how many it ended. `main` starts it only while the switch is on, under `supervise_background`, and a failed pass is
/// logged before the next one.
pub(crate) async fn run_expiry_sweep(pool: PgPool) -> anyhow::Result<()> {
    let interval = std::time::Duration::from_secs(EXPIRY_SWEEP_SECONDS);
    loop {
        tokio::time::sleep(interval).await;
        match expire_stale(&pool).await {
            Ok(0) => {}
            Ok(ended) => tracing::info!(ended, "research runs past their deadline were expired"),
            Err(error) => tracing::warn!(%error, "research run expiry sweep failed"),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ResearchRunService (Quarry v2 E4.6, slice Q4-062b): the gRPC face of the store above.
//
// Every RPC takes its org from the verified caller, never from the request, and its owner from the verified caller
// too: the person for a user, the service's own principal for a service (so a service reads only the rows it
// created), and the key principal for an api key. A run of another owner is not found. Writes refuse a ZDR token
// (`session:write`, as every durable write does); a ZDR or waived run is stored with `zdr = true` and keeps no content.
// ---------------------------------------------------------------------------------------------------------------------

/// The switch. Only the literal `true` (case-insensitive, surrounding whitespace allowed) turns the service on.
pub(crate) const ENABLED_ENV: &str = "RESEARCH_RUNS_ENABLED";
/// Why every RPC answers `unavailable` while the switch is off.
const DISABLED: &str = "research_runs_disabled";
/// The trailer code of a begin refused because the thread already has a running run (gRPC `Aborted`).
pub(crate) const ACTIVE_CODE: &str = "research_run_active";
/// The prefix of an argument refusal, so a caller can tell the kind of refusal from its text.
const INVALID_RUN: &str = "invalid_research_run";
/// The longest id, in characters: the table's CHECK on every id column.
const MAX_ID_CHARS: usize = 128;
/// The longest stop reason or halt code, in characters: the table's CHECK on both columns.
const MAX_WORD_CHARS: usize = 64;
/// The deadline a run gets when it names none, in seconds.
const DEFAULT_DEADLINE_SECS: u32 = 900;
/// The shortest deadline a run may name, in seconds.
const MIN_DEADLINE_SECS: u32 = 60;
/// The longest deadline a run may name, in seconds.
const MAX_DEADLINE_SECS: u32 = 3_600;
/// The largest partial result a run keeps, as encoded JSON, in bytes.
const MAX_PARTIAL_BYTES: usize = 1024 * 1024;

/// Whether the service answers. Read where it is used, with the parser the other switches use.
pub(crate) fn research_runs_enabled() -> bool {
    mp_contracts::loop_fold::switch_from(std::env::var(ENABLED_ENV).ok().as_deref())
}

/// The research run RPCs. They hold the pool and nothing else; the rules are in this module.
pub struct ResearchRunGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl ResearchRunService for ResearchRunGrpc {
    async fn begin(
        &self,
        request: Request<pb::BeginRequest>,
    ) -> Result<Response<pb::BeginResponse>, RpcStatus> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // `session:write` refuses a zero-retention credential before anything is read.
            authorize_operation(&caller, "session:write")?;
            if !research_runs_enabled() {
                return Err(RpcStatus::unavailable(DISABLED));
            }
            begin_run(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("begin_research_run", started, result.is_ok());
        result
    }

    async fn progress(
        &self,
        request: Request<pb::ProgressRequest>,
    ) -> Result<Response<pb::ProgressResponse>, RpcStatus> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            if !research_runs_enabled() {
                return Err(RpcStatus::unavailable(DISABLED));
            }
            progress_run(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("progress_research_run", started, result.is_ok());
        result
    }

    async fn finish(
        &self,
        request: Request<pb::FinishRequest>,
    ) -> Result<Response<pb::FinishResponse>, RpcStatus> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            if !research_runs_enabled() {
                return Err(RpcStatus::unavailable(DISABLED));
            }
            finish_run(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("finish_research_run", started, result.is_ok());
        result
    }

    async fn get(
        &self,
        request: Request<pb::GetRequest>,
    ) -> Result<Response<pb::GetResponse>, RpcStatus> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            if !research_runs_enabled() {
                return Err(RpcStatus::unavailable(DISABLED));
            }
            get_run(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_research_run", started, result.is_ok());
        result
    }

    async fn get_active(
        &self,
        request: Request<pb::GetActiveRequest>,
    ) -> Result<Response<pb::GetActiveResponse>, RpcStatus> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            if !research_runs_enabled() {
                return Err(RpcStatus::unavailable(DISABLED));
            }
            get_active_run(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_active_research_run", started, result.is_ok());
        result
    }
}

/// The owner a run row carries for this caller: the person for a user, the key principal for an api key, and the
/// service's own principal for a service. A caller with no owner may not hold a run.
fn owner_of(caller: &VerifiedIdentity) -> Result<&str, RpcStatus> {
    let owner = if caller.is_service() {
        Some(caller.principal_id())
    } else {
        caller.owner_id()
    };
    owner
        .filter(|owner| !owner.trim().is_empty())
        .ok_or_else(|| RpcStatus::permission_denied("a research run belongs to its owner"))
}

fn invalid(detail: &str) -> RpcStatus {
    RpcStatus::invalid_argument(format!("{INVALID_RUN}: {detail}"))
}

fn internal(error: sqlx::Error) -> RpcStatus {
    RpcStatus::internal(error.to_string())
}

/// A text field trimmed and checked to be 1 to `max_chars` characters.
fn bounded(raw: &str, max_chars: usize, field: &str) -> Result<String, RpcStatus> {
    let value = raw.trim();
    let chars = value.chars().count();
    if chars == 0 || chars > max_chars {
        return Err(invalid(&format!(
            "{field} must be 1 to {max_chars} characters"
        )));
    }
    Ok(value.to_owned())
}

/// A text field that is empty (absent) or 1 to `max_chars` characters after trimming.
fn optional_bounded(raw: &str, max_chars: usize, field: &str) -> Result<Option<String>, RpcStatus> {
    if raw.trim().is_empty() {
        return Ok(None);
    }
    bounded(raw, max_chars, field).map(Some)
}

/// The budget a run asks for. A dimension left unset stays unset, and a set one may not be negative.
fn check_budget(raw: Option<&pb::ResearchBudget>) -> Result<Budget, RpcStatus> {
    let budget = raw.map_or_else(Budget::default, |raw| Budget {
        pages: raw.max_pages,
        sub_queries: raw.max_sub_queries,
        usd_micro: raw.usd_ceiling_micro,
    });
    let negative = budget.pages.is_some_and(|value| value < 0)
        || budget.sub_queries.is_some_and(|value| value < 0)
        || budget.usd_micro.is_some_and(|value| value < 0);
    if negative {
        return Err(invalid("budget values must not be negative"));
    }
    Ok(budget)
}

/// The counters a request carries. They only move up in the store, and a negative one is refused.
fn check_counters(
    pages_read: i32,
    sources_found: i32,
    spent_usd_micro: i64,
) -> Result<(), RpcStatus> {
    if pages_read < 0 || sources_found < 0 || spent_usd_micro < 0 {
        return Err(invalid("counters must not be negative"));
    }
    Ok(())
}

/// The status a run ends in: one of the terminal words. `running` is not an end.
fn check_terminal_status(raw: &str) -> Result<Status, RpcStatus> {
    match Status::parse(raw.trim()) {
        Some(status) if status.is_terminal() => Ok(status),
        _ => Err(invalid(
            "status must be completed, stopped, failed, cancelled or expired",
        )),
    }
}

/// The partial results a request carries, as stored JSON. Refused past [`MAX_PARTIAL_BYTES`] once encoded.
fn partial_from(raw: Option<&prost_types::Struct>) -> Result<Option<serde_json::Value>, RpcStatus> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let value = struct_to_json(raw);
    // An encoding that fails leaves the size unknown, so the partial is refused rather than kept.
    let encoded = serde_json::to_vec(&value).map_or(usize::MAX, |bytes| bytes.len());
    if encoded > MAX_PARTIAL_BYTES {
        return Err(invalid(&format!(
            "partial must be at most {MAX_PARTIAL_BYTES} bytes once encoded"
        )));
    }
    Ok(Some(value))
}

/// The deadline a run's seconds name, counted from now. Zero takes [`DEFAULT_DEADLINE_SECS`].
fn deadline_from_now(seconds: u32) -> Result<DateTime<Utc>, RpcStatus> {
    let seconds = if seconds == 0 {
        DEFAULT_DEADLINE_SECS
    } else {
        seconds
    };
    if !(MIN_DEADLINE_SECS..=MAX_DEADLINE_SECS).contains(&seconds) {
        return Err(invalid(&format!(
            "deadline_seconds must be {MIN_DEADLINE_SECS} to {MAX_DEADLINE_SECS}"
        )));
    }
    let delta = TimeDelta::try_seconds(i64::from(seconds))
        .ok_or_else(|| invalid("deadline_seconds is out of range"))?;
    Utc::now()
        .checked_add_signed(delta)
        .ok_or_else(|| invalid("deadline_seconds is out of range"))
}

/// The refusal of a partial result or a report artifact on a ZDR run: a run that keeps no content stores none.
fn zdr_content_refused() -> RpcStatus {
    Refusal::DURABLE_MUTATION_FORBIDDEN.grpc_status(tonic::Code::FailedPrecondition)
}

/// The typed refusal of a begin on a thread whose research run is still running. The trailer names the code, and the
/// id of that run when the caller may see it (see [`begin_status`]).
fn research_run_active_status(active_run_id: Option<&str>) -> RpcStatus {
    let mut status = RpcStatus::aborted("thread already has an active research run");
    status
        .metadata_mut()
        .insert(ERROR_CODE_TRAILER, MetadataValue::from_static(ACTIVE_CODE));
    if let Some(value) = active_run_id.and_then(|id| MetadataValue::try_from(id).ok()) {
        status.metadata_mut().insert(ACTIVE_RUN_TRAILER, value);
    }
    status
}

/// The refusal of a begin. The running run's id goes out only to that run's owner: another owner's run is not named,
/// since the thread's run is not confirmed to anyone else (as [`get_active_run`] does). The refusal still says that the
/// thread is busy.
fn begin_status(error: BeginError, owner: &str) -> RpcStatus {
    match error {
        BeginError::Active { run_id, user_id } if user_id == owner => {
            research_run_active_status(Some(run_id.as_str()))
        }
        BeginError::Active { .. } => research_run_active_status(None),
        BeginError::ThreadNotFound => RpcStatus::not_found("thread not found"),
        BeginError::Store(error) => internal(error),
    }
}

fn finish_status(error: FinishError) -> RpcStatus {
    match error {
        FinishError::NotTerminal => invalid("status must be a terminal status"),
        FinishError::Store(error) => internal(error),
    }
}

/// The stored run as the wire message. The partial results are the stored object, when the run keeps any.
fn run_to_proto(run: ResearchRun) -> pb::ResearchRun {
    pb::ResearchRun {
        id: run.id,
        org_id: run.org_id,
        user_id: run.user_id,
        thread_id: run.thread_id,
        run_id: run.run_id,
        request_id: run.request_id.unwrap_or_default(),
        status: run.status.as_str().to_owned(),
        stop_reason: run.stop_reason.unwrap_or_default(),
        halt_code: run.halt_code.unwrap_or_default(),
        zdr: run.zdr,
        budget: Some(pb::ResearchBudget {
            max_pages: run.budget.pages,
            max_sub_queries: run.budget.sub_queries,
            usd_ceiling_micro: run.budget.usd_micro,
        }),
        spent_usd_micro: run.spent_usd_micro,
        pages_read: run.pages_read,
        sources_found: run.sources_found,
        partial: run.partial.as_ref().and_then(json_to_struct),
        report_artifact_id: run.report_artifact_id.unwrap_or_default(),
        created_at: Some(to_proto_timestamp(run.created_at)),
        updated_at: Some(to_proto_timestamp(run.updated_at)),
        deadline_at: Some(to_proto_timestamp(run.deadline_at)),
    }
}

/// The run of `id` when the caller owns it. A run of another owner, and a run that does not exist, are both not found.
async fn owned_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    id: &str,
) -> Result<ResearchRun, RpcStatus> {
    let owner = owner_of(caller)?;
    match get(pool, caller.org_id(), id).await.map_err(internal)? {
        Some(run) if run.user_id == owner => Ok(run),
        _ => Err(RpcStatus::not_found("research run not found")),
    }
}

/// The thread lookup of a begin. It runs first, inside the begin's transaction: the thread must be in the caller's
/// org and belong to the run's owner, or it reads as not found. `FOR KEY SHARE` holds the thread's row until the
/// transaction ends. The thread erase takes `FOR UPDATE` on the same row before it deletes the thread's research
/// runs, so the two take turns. A begin that locks the row first commits its run before the erase deletes runs,
/// and the erase deletes it with the rest. An erase that locked the row first has committed by the time the lookup
/// runs, and the thread reads as absent. A message append takes this same lock through its foreign key, so a begin
/// does not wait on an append.
///
/// A run takes its thread's one running slot, and only its owner can see or end the run. A begin on another owner's
/// thread would hold that slot where the thread's owner cannot reach it. So another owner's thread reads as not found,
/// as another owner's run does (see [`owned_run`]). A ZDR turn's thread is a metadata-only row of its owner too, so the
/// same rule holds for it.
const OWNED_THREAD: &str = "SELECT id FROM threads WHERE id = $1 AND org_id = $2 AND user_id = $3 FOR KEY SHARE";

/// Begins a research run for the caller on a thread the caller owns, or refuses with the id of the run already running
/// there.
async fn begin_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::BeginRequest,
) -> Result<pb::BeginResponse, RpcStatus> {
    let owner = owner_of(caller)?;
    let thread_id = bounded(&req.thread_id, MAX_ID_CHARS, "thread_id")?;
    let run_id = bounded(&req.run_id, MAX_ID_CHARS, "run_id")?;
    let request_id = optional_bounded(&req.request_id, MAX_ID_CHARS, "request_id")?;
    let budget = check_budget(req.budget.as_ref())?;
    let deadline_at = deadline_from_now(req.deadline_seconds)?;
    // A ZDR or waived caller always gets a ZDR run. The request can add to that, and never take it away.
    let zdr = req.zdr || caller.zdr() || caller.zdr_waiver().is_some();
    let run = begin(
        pool,
        NewRun {
            org_id: caller.org_id(),
            user_id: owner,
            thread_id: &thread_id,
            run_id: &run_id,
            request_id: request_id.as_deref(),
            zdr,
            budget,
            deadline_at,
        },
    )
    .await
    .map_err(|error| begin_status(error, owner))?;
    Ok(pb::BeginResponse {
        run: Some(run_to_proto(run)),
    })
}

/// Records a progress report on a running run the caller owns. A report that arrives after the run ended applies
/// nothing and says so.
async fn progress_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ProgressRequest,
) -> Result<pb::ProgressResponse, RpcStatus> {
    let id = bounded(&req.id, MAX_ID_CHARS, "id")?;
    check_counters(req.pages_read, req.sources_found, req.spent_usd_micro)?;
    let partial = partial_from(req.partial.as_ref())?;
    let run = owned_run(pool, caller, &id).await?;
    if run.zdr && partial.is_some() {
        return Err(zdr_content_refused());
    }
    let outcome = record_progress(
        pool,
        caller.org_id(),
        &id,
        Progress {
            pages_read: req.pages_read,
            sources_found: req.sources_found,
            spent_usd_micro: req.spent_usd_micro,
            partial: partial.as_ref(),
        },
    )
    .await
    .map_err(internal)?;
    Ok(pb::ProgressResponse {
        applied: outcome == WriteOutcome::Applied,
    })
}

/// Ends a run the caller owns with a terminal status. The counters go in first, since they only move up, and a run
/// that has already ended takes no change at all.
async fn finish_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::FinishRequest,
) -> Result<pb::FinishResponse, RpcStatus> {
    let id = bounded(&req.id, MAX_ID_CHARS, "id")?;
    let status = check_terminal_status(&req.status)?;
    let stop_reason = optional_bounded(&req.stop_reason, MAX_WORD_CHARS, "stop_reason")?;
    let halt_code = optional_bounded(&req.halt_code, MAX_WORD_CHARS, "halt_code")?;
    let report_artifact_id =
        optional_bounded(&req.report_artifact_id, MAX_ID_CHARS, "report_artifact_id")?;
    check_counters(req.pages_read, req.sources_found, req.spent_usd_micro)?;
    let partial = partial_from(req.partial.as_ref())?;
    let run = owned_run(pool, caller, &id).await?;
    if run.zdr && (partial.is_some() || report_artifact_id.is_some()) {
        return Err(zdr_content_refused());
    }
    record_progress(
        pool,
        caller.org_id(),
        &id,
        Progress {
            pages_read: req.pages_read,
            sources_found: req.sources_found,
            spent_usd_micro: req.spent_usd_micro,
            partial: None,
        },
    )
    .await
    .map_err(internal)?;
    let outcome = finish(
        pool,
        caller.org_id(),
        &id,
        RunEnd {
            status,
            stop_reason: stop_reason.as_deref(),
            halt_code: halt_code.as_deref(),
            partial: partial.as_ref(),
            report_artifact_id: report_artifact_id.as_deref(),
        },
    )
    .await
    .map_err(finish_status)?;
    Ok(pb::FinishResponse {
        applied: outcome == WriteOutcome::Applied,
    })
}

/// Reads one run the caller owns.
async fn get_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::GetRequest,
) -> Result<pb::GetResponse, RpcStatus> {
    let id = bounded(&req.id, MAX_ID_CHARS, "id")?;
    let run = owned_run(pool, caller, &id).await?;
    Ok(pb::GetResponse {
        run: Some(run_to_proto(run)),
    })
}

/// Reads the running run of a thread when the caller owns it. Another owner's run reads as no run, so the thread's
/// run is not confirmed to anyone else.
async fn get_active_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::GetActiveRequest,
) -> Result<pb::GetActiveResponse, RpcStatus> {
    let owner = owner_of(caller)?;
    let thread_id = bounded(&req.thread_id, MAX_ID_CHARS, "thread_id")?;
    let active = active_for_thread(pool, caller.org_id(), &thread_id)
        .await
        .map_err(internal)?;
    Ok(match active.filter(|run| run.user_id == owner) {
        Some(run) => pb::GetActiveResponse {
            run: Some(run_to_proto(run)),
            found: true,
        },
        None => pb::GetActiveResponse {
            run: None,
            found: false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusal a begin by `caller` gets while a run of `run_owner` is running on the thread.
    fn refusal(run_owner: &str, caller: &str) -> RpcStatus {
        begin_status(
            BeginError::Active {
                run_id: "rsr_01HZOWNERCHECK".to_owned(),
                user_id: run_owner.to_owned(),
            },
            caller,
        )
    }

    fn trailer<'a>(status: &'a RpcStatus, key: &str) -> Option<&'a str> {
        status
            .metadata()
            .get(key)
            .and_then(|value| value.to_str().ok())
    }

    #[test]
    fn the_owner_of_the_running_run_is_named_on_the_refusal() {
        let status = refusal("user-a", "user-a");
        assert_eq!(status.code(), tonic::Code::Aborted);
        assert_eq!(trailer(&status, ERROR_CODE_TRAILER), Some(ACTIVE_CODE));
        assert_eq!(trailer(&status, ACTIVE_RUN_TRAILER), Some("rsr_01HZOWNERCHECK"));
    }

    #[test]
    fn another_owners_running_run_is_not_named_on_the_refusal() {
        let status = refusal("user-a", "user-b");
        assert_eq!(status.code(), tonic::Code::Aborted);
        assert_eq!(trailer(&status, ERROR_CODE_TRAILER), Some(ACTIVE_CODE));
        assert_eq!(trailer(&status, ACTIVE_RUN_TRAILER), None);
    }

    #[test]
    fn a_begin_names_only_a_thread_of_its_owner_in_its_org() {
        // The org and the owner are both in the lookup, so another owner's or another org's thread reads as absent.
        assert!(OWNED_THREAD.contains("org_id = $2"));
        assert!(OWNED_THREAD.contains("user_id = $3"));
    }

    #[test]
    fn a_begin_locks_its_thread_row_against_the_erase() {
        // FOR KEY SHARE conflicts with the thread erase's FOR UPDATE on the same row, which is what orders them.
        assert!(OWNED_THREAD.ends_with("FOR KEY SHARE"));
    }

    #[test]
    fn the_expiry_sweep_ends_only_a_running_run_past_its_deadline() {
        // A live run inside its deadline and an ended run are both left alone.
        assert!(EXPIRE_STALE.contains("status = 'running' AND deadline_at < now()"));
        assert!(EXPIRE_STALE.contains("SET status = 'expired'"));
    }
}
