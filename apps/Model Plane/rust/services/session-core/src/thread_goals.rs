//! `ThreadGoalService`: the thread goal store (Wave 3 EP-7 S1).
//!
//! A thread goal is a standing objective the owner sets on one of their threads, with optional
//! success criteria. model-gateway reads it on every agentic turn of the thread and recites it to
//! the model as data (`RunContext.goal_recitation`); the goal bar in the chat thread sets, edits,
//! pauses and clears it. The record is the owner's own text, so it lives here beside the thread
//! and is erased with it (`delete_thread_rows`, the org purge, and the cascade).
//!
//! Owner decisions taken by their recommendation (EP-7 section 3): the goal bar lives in the chat
//! thread (D1); a refusal names the feature `thread_goal` (D7); Space threads are refused (D8);
//! objective at most 2,000 characters, at most 8 criteria of 500, `max_turns` default 10 and at
//! most 50, a token budget of at most 5,000,000, one open goal per thread (D11); the model never
//! writes its goal (D12).
//!
//! Refused: a zero-retention or waived credential (every RPC: no such content may be here), a
//! service caller writing (a goal is set by its owner), another person's thread, a Space thread,
//! a thread holding a waived run, a second open goal (`already_exists` `thread_goal_exists` unless
//! `replace`), a stale revision (`aborted` `thread_goal_revision_conflict`) and a change to a
//! closed goal (`failed_precondition` `thread_goal_closed`). This RPC writes only `active`,
//! `paused` and `cleared`; `met` and the stop states belong to the EP-7 S2/S3 code paths. The
//! switch is `SESSION_CORE_THREAD_GOALS_ENABLED`, default off; while it is off every RPC answers
//! `unavailable` `thread_goals_disabled`.
//!
//! The budget ledger (EP-7 S2) rides the same switch: `ClaimGoalTurn` admits one continuation turn
//! before its run is prepared (one in flight per goal, under the goal's row lock),
//! `BindGoalTurnRun` names the run once it exists, and `SettleGoalTurn` debits the run's metered
//! usage, idempotent on the run, and moves the goal to `budget_reached` or `turns_reached` when a
//! cap is hit. Owner decision D2 taken by its recommendation: only metered tokens count (a
//! subscription turn spends none), so every goal also has its turn cap; the judge's tokens count;
//! at 100% no new turn is claimed. A claim not settled within [`CLAIM_LEASE_MINUTES`] (a crashed
//! replica) is settled `lease_expired` by the next claim and still counts as a turn, so a turn
//! that keeps dying can never loop past the cap.

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{self as pb, thread_goal_service_server::ThreadGoalService};
use mp_ids::new_ulid;
use sqlx::{PgPool, Postgres, Transaction};
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{
    authorize_operation, authorize_thread_owner_row, identity, OwnerIntent, VerifiedIdentity,
};
use crate::grpc::{record_metrics, to_proto_timestamp};

/// D11: the longest objective, in characters (the table's CHECK).
pub(crate) const MAX_OBJECTIVE_CHARS: usize = 2_000;
/// D11: the most success criteria a goal keeps (the table's CHECK).
pub(crate) const MAX_CRITERIA: usize = 8;
/// D11: the longest success criterion, in characters.
pub(crate) const MAX_CRITERION_CHARS: usize = 500;
/// D11: the turn cap a goal takes when the caller names none.
pub(crate) const DEFAULT_MAX_TURNS: u32 = 10;
/// D11: the highest turn cap (the table's CHECK).
pub(crate) const MAX_TURNS: u32 = 50;
/// D11: the largest metered-token budget (the table's CHECK).
pub(crate) const MAX_TOKEN_BUDGET: u64 = 5_000_000;

/// The refusal codes a caller can act on. Strings, as the other session-core refusals are; the
/// gateway maps them onto its typed envelope.
pub(crate) const GOAL_EXISTS: &str = "thread_goal_exists";
pub(crate) const REVISION_CONFLICT: &str = "thread_goal_revision_conflict";
pub(crate) const SPACE_UNSUPPORTED: &str = "thread_goal_space_unsupported";
pub(crate) const WAIVED_THREAD: &str = "thread_goal_waived_thread";
pub(crate) const GOAL_CLOSED: &str = "thread_goal_closed";
pub(crate) const INVALID_GOAL: &str = "invalid_thread_goal";
const DISABLED: &str = "thread_goals_disabled";
/// The budget ledger's refusals (EP-7 S2), each a `failed_precondition` message.
pub(crate) const NOT_ACTIVE: &str = "thread_goal_not_active";
pub(crate) const TURN_IN_FLIGHT: &str = "thread_goal_turn_in_flight";
pub(crate) const BUDGET_REACHED: &str = "thread_goal_budget_reached";
pub(crate) const TURNS_REACHED: &str = "thread_goal_turns_reached";
pub(crate) const TURN_MISMATCH: &str = "thread_goal_turn_mismatch";

/// The statuses of a goal that is still open: at most one per thread (the partial unique index).
const OPEN_STATUSES: [&str; 2] = ["active", "paused"];

/// D11: how long a claimed turn holds its goal before the next claim may settle it as
/// `lease_expired`.
pub(crate) const CLAIM_LEASE_MINUTES: i32 = 30;

/// The outcomes a settle may name; `lease_expired` is written only by a claim.
const SETTLE_OUTCOMES: [&str; 7] = [
    "completed",
    "failed",
    "awaiting_approval",
    "parked",
    "awaiting_input",
    "cancelled",
    "unknown",
];

/// Where a settled turn's usage came from (the table's CHECK).
const USAGE_SOURCES: [&str; 4] = ["run_response", "progress", "ledger", "none"];

/// The most tokens one settle may add per count. Far above any real run, it keeps the cached
/// counters far from a BIGINT overflow whatever a caller sends.
const MAX_TURN_TOKENS: u64 = 1_000_000_000_000;

/// One switch for the store and for the goal facts attention reads (EP-7 S5 named it first), read
/// where it is used with the strict parser, so only the literal `true` turns it on.
pub(crate) fn thread_goals_enabled() -> bool {
    crate::attention::goals_enabled()
}

/// The goal RPCs. They hold the pool and nothing else; the rules are in this module.
pub struct ThreadGoalGrpc {
    pub(crate) pool: PgPool,
}

#[tonic::async_trait]
impl ThreadGoalService for ThreadGoalGrpc {
    async fn set_thread_goal(
        &self,
        request: Request<pb::SetThreadGoalRequest>,
    ) -> Result<Response<pb::ThreadGoalResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            // `session:write` refuses a zero-retention credential before anything is read.
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !thread_goals_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            set_thread_goal(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("set_thread_goal", started, result.is_ok());
        result
    }

    async fn get_thread_goal(
        &self,
        request: Request<pb::GetThreadGoalRequest>,
    ) -> Result<Response<pb::ThreadGoalResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            refuse_retention_exempt(&caller)?;
            authorize_operation(&caller, "session:read")?;
            if !thread_goals_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            get_thread_goal(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("get_thread_goal", started, result.is_ok());
        result
    }

    async fn update_thread_goal(
        &self,
        request: Request<pb::UpdateThreadGoalRequest>,
    ) -> Result<Response<pb::ThreadGoalResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !thread_goals_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            update_thread_goal(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("update_thread_goal", started, result.is_ok());
        result
    }

    async fn claim_goal_turn(
        &self,
        request: Request<pb::ClaimGoalTurnRequest>,
    ) -> Result<Response<pb::ClaimGoalTurnResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !thread_goals_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            claim_goal_turn(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("claim_goal_turn", started, result.is_ok());
        result
    }

    async fn bind_goal_turn_run(
        &self,
        request: Request<pb::BindGoalTurnRunRequest>,
    ) -> Result<Response<pb::BindGoalTurnRunResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !thread_goals_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            let goal_auto = claim_guard::goal_auto_requested(request.metadata());
            bind_goal_turn_run(&self.pool, &caller, request.into_inner(), goal_auto)
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("bind_goal_turn_run", started, result.is_ok());
        result
    }

    async fn settle_goal_turn(
        &self,
        request: Request<pb::SettleGoalTurnRequest>,
    ) -> Result<Response<pb::ThreadGoalResponse>, Status> {
        let started = Instant::now();
        let result = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            refuse_retention_exempt(&caller)?;
            if !thread_goals_enabled() {
                return Err(Status::unavailable(DISABLED));
            }
            settle_goal_turn(&self.pool, &caller, request.into_inner())
                .await
                .map(Response::new)
        }
        .await;
        record_metrics("settle_goal_turn", started, result.is_ok());
        result
    }
}

/// A zero-retention or waived credential never reaches the store (defence in depth: the gateway
/// refuses both first). `authorize_operation` lets a ZDR credential read, and a waived bearer's
/// `zdr` claim is false, so both are named here (DEC-32: a waived run counts as ZDR for every new
/// durable store).
fn refuse_retention_exempt(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.zdr() || caller.zdr_waiver().is_some() {
        return Err(mp_contracts::zdr_refusal::Refusal::DURABLE_MUTATION_FORBIDDEN
            .grpc_status(tonic::Code::FailedPrecondition));
    }
    Ok(())
}

/// A goal is the owner's own aim: a service principal may not set or change one, even in the
/// owner's org, so no workload can steer what every later turn is told.
fn refuse_service_writer(caller: &VerifiedIdentity) -> Result<(), Status> {
    if caller.is_service() {
        return Err(Status::permission_denied("a thread goal is set by its owner"));
    }
    Ok(())
}

/// The pump guard every goal turn claim runs, and the audit mark of a pump-sent turn (EP-7 S4).
mod claim_guard;

fn invalid(detail: &str) -> Status {
    Status::invalid_argument(format!("{INVALID_GOAL}: {detail}"))
}

/// The objective trimmed and checked against D11.
fn check_objective(raw: &str) -> Result<String, Status> {
    let objective = raw.trim();
    let chars = objective.chars().count();
    if chars == 0 || chars > MAX_OBJECTIVE_CHARS {
        return Err(invalid("objective must be 1 to 2000 characters"));
    }
    Ok(objective.to_owned())
}

/// The criteria trimmed, blank lines dropped, and checked against D11.
fn check_criteria(raw: &[String]) -> Result<Vec<String>, Status> {
    let criteria: Vec<String> = raw
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if criteria.len() > MAX_CRITERIA {
        return Err(invalid("at most 8 success criteria"));
    }
    if criteria
        .iter()
        .any(|line| line.chars().count() > MAX_CRITERION_CHARS)
    {
        return Err(invalid("a success criterion is at most 500 characters"));
    }
    Ok(criteria)
}

/// The turn cap: 0 takes the default, anything else must be 1 to 50.
fn check_max_turns(raw: u32, default_when_zero: bool) -> Result<i32, Status> {
    let turns = if raw == 0 && default_when_zero {
        DEFAULT_MAX_TURNS
    } else {
        raw
    };
    if !(1..=MAX_TURNS).contains(&turns) {
        return Err(invalid("max_turns must be 1 to 50"));
    }
    i32::try_from(turns).map_err(|_| invalid("max_turns out of range"))
}

/// A token budget: 0 is none, anything else at most 5,000,000.
fn check_token_budget(raw: u64) -> Result<Option<i64>, Status> {
    if raw == 0 {
        return Ok(None);
    }
    if raw > MAX_TOKEN_BUDGET {
        return Err(invalid("token_budget is at most 5000000"));
    }
    i64::try_from(raw)
        .map(Some)
        .map_err(|_| invalid("token_budget out of range"))
}

/// A cost budget in micro-USD: 0 is none.
fn check_cost_budget(raw: u64) -> Result<Option<i64>, Status> {
    if raw == 0 {
        return Ok(None);
    }
    i64::try_from(raw)
        .map(Some)
        .map_err(|_| invalid("cost_budget_usd_micros out of range"))
}

/// The status an update may write. `met` and the stop states come only from the budget ledger and
/// the judge (EP-7 S2/S3), never from a person's request.
fn check_update_status(raw: &str) -> Result<&'static str, Status> {
    match raw.trim() {
        "active" => Ok("active"),
        "paused" => Ok("paused"),
        "cleared" => Ok("cleared"),
        _ => Err(invalid("status must be active, paused or cleared")),
    }
}

/// The fields an update may name, in `UpdateThreadGoalRequest`'s words.
const UPDATE_PATHS: [&str; 6] = [
    "objective",
    "success_criteria",
    "token_budget",
    "cost_budget_usd_micros",
    "max_turns",
    "status",
];

fn check_update_paths(paths: &[String]) -> Result<(), Status> {
    if paths.is_empty() {
        return Err(invalid("update_paths is required"));
    }
    if let Some(unknown) = paths
        .iter()
        .find(|path| !UPDATE_PATHS.contains(&path.as_str()))
    {
        // The path is a field name the caller wrote; a bounded echo names the mistake.
        let name: String = unknown.chars().take(64).collect();
        return Err(invalid(&format!("unknown update path {name}")));
    }
    Ok(())
}

/// The compare-and-set: the revision the caller saw must be the revision now.
fn check_revision(current: i64, expected: u64) -> Result<(), Status> {
    if u64::try_from(current).ok() != Some(expected) {
        return Err(Status::aborted(REVISION_CONFLICT));
    }
    Ok(())
}

/// The thread's owner and Space, read for an owner check.
struct ThreadOwner {
    org_id: String,
    user_id: String,
    space_id: Option<String>,
}

impl ThreadOwner {
    fn space(&self) -> &str {
        self.space_id.as_deref().unwrap_or_default()
    }

    fn in_space(&self) -> bool {
        !self.space().trim().is_empty()
    }
}

async fn read_thread<'e, E>(executor: E, thread_id: &str) -> Result<ThreadOwner, Status>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let row: Option<(String, String, Option<String>)> =
        sqlx::query_as("SELECT org_id, user_id, space_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(executor)
            .await
            .map_err(internal)?;
    let (org_id, user_id, space_id) = row.ok_or_else(|| Status::not_found("thread not found"))?;
    Ok(ThreadOwner {
        org_id,
        user_id,
        space_id,
    })
}

/// The write-side thread checks, in one place for set and update: same org (else the thread is
/// not found), the owner's own personal thread, and no waived run on it.
async fn authorize_thread_write(
    tx: &mut Transaction<'_, Postgres>,
    caller: &VerifiedIdentity,
    org_id: &str,
    thread_id: &str,
) -> Result<ThreadOwner, Status> {
    let thread = read_thread(&mut **tx, thread_id).await?;
    if thread.org_id != org_id {
        return Err(Status::not_found("thread not found"));
    }
    authorize_thread_owner_row(
        caller,
        &thread.org_id,
        &thread.user_id,
        thread.space(),
        OwnerIntent::Mutate,
    )?;
    // D8: a shared room's goal needs the Control shared-Space decision family first.
    if thread.in_space() {
        return Err(Status::failed_precondition(SPACE_UNSUPPORTED));
    }
    if thread_is_waived(tx, thread_id).await? {
        return Err(Status::failed_precondition(WAIVED_THREAD));
    }
    Ok(thread)
}

/// Whether the thread holds a run whose content was waived from retention (DEC-32). The same
/// predicate as the fork's and the artifact store's.
async fn thread_is_waived(
    tx: &mut Transaction<'_, Postgres>,
    thread_id: &str,
) -> Result<bool, Status> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE thread_id = $1 AND retention_class = 'zdr_waived')",
    )
    .bind(thread_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(internal)
}

/// Set a goal on a thread. The switch and the session scope are the RPC's to check, so the store
/// itself can be tested without either.
pub(crate) async fn set_thread_goal(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::SetThreadGoalRequest,
) -> Result<pb::ThreadGoalResponse, Status> {
    refuse_service_writer(caller)?;
    let org_id = req.org_id.trim();
    let thread_id = req.thread_id.trim();
    if org_id.is_empty() || thread_id.is_empty() {
        return Err(Status::invalid_argument("org_id and thread_id are required"));
    }
    let objective = check_objective(&req.objective)?;
    let criteria = check_criteria(&req.success_criteria)?;
    let max_turns = check_max_turns(req.max_turns, true)?;
    let token_budget = check_token_budget(req.token_budget)?;
    let cost_budget = check_cost_budget(req.cost_budget_usd_micros)?;
    caller.authorize_org(org_id)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    // One goal writer per thread at a time: the open-goal read, the replace and the insert are one
    // decision. The partial unique index still holds if this lock were ever skipped.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended('thread_goals:' || $1, 0))")
        .bind(thread_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let thread = authorize_thread_write(&mut tx, caller, org_id, thread_id).await?;

    let open: Option<String> = sqlx::query_scalar(
        "SELECT id FROM thread_goals WHERE thread_id = $1 AND status IN ('active', 'paused')",
    )
    .bind(thread_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if let Some(open_id) = open {
        if !req.replace {
            return Err(Status::already_exists(GOAL_EXISTS));
        }
        sqlx::query(
            "UPDATE thread_goals
             SET status = 'cleared', status_reason = 'replaced', revision = revision + 1,
                 updated_at = now(), closed_at = now()
             WHERE id = $1",
        )
        .bind(&open_id)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    }

    let row: GoalRow = sqlx::query_as(&format!(
        "INSERT INTO thread_goals
         (id, thread_id, org_id, user_id, objective, success_criteria, token_budget,
          cost_budget_usd_micros, max_turns, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'active')
         RETURNING {GOAL_COLUMNS}"
    ))
    .bind(format!("goal_{}", new_ulid()))
    .bind(thread_id)
    .bind(&thread.org_id)
    .bind(&thread.user_id)
    .bind(&objective)
    .bind(sqlx::types::Json(&criteria))
    .bind(token_budget)
    .bind(cost_budget)
    .bind(max_turns)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| match &error {
        // Only reachable if the advisory lock were bypassed: the index is the last word.
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            Status::already_exists(GOAL_EXISTS)
        }
        _ => Status::internal(error.to_string()),
    })?;
    tx.commit().await.map_err(internal)?;
    Ok(found(row))
}

/// Read the thread's latest goal that was not cleared. A thread with none answers `found: false`,
/// so the goal bar can ask on every thread without an error path.
pub(crate) async fn get_thread_goal(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::GetThreadGoalRequest,
) -> Result<pb::ThreadGoalResponse, Status> {
    let org_id = req.org_id.trim();
    let thread_id = req.thread_id.trim();
    if org_id.is_empty() || thread_id.is_empty() {
        return Err(Status::invalid_argument("org_id and thread_id are required"));
    }
    caller.authorize_org(org_id)?;
    let thread = read_thread(pool, thread_id).await?;
    if thread.org_id != org_id {
        return Err(Status::not_found("thread not found"));
    }
    authorize_thread_owner_row(
        caller,
        &thread.org_id,
        &thread.user_id,
        thread.space(),
        OwnerIntent::Read,
    )?;
    // The same "latest" as the attention listing (`attention::fetch_goals`), so the bar and the
    // rail never disagree about which goal a thread has.
    let row: Option<GoalRow> = sqlx::query_as(&format!(
        "SELECT {GOAL_COLUMNS} FROM thread_goals
         WHERE thread_id = $1 AND org_id = $2
         ORDER BY updated_at DESC, created_at DESC, id DESC LIMIT 1"
    ))
    .bind(thread_id)
    .bind(org_id)
    .fetch_optional(pool)
    .await
    .map_err(internal)?;
    Ok(match row {
        Some(row) if row.status != "cleared" => found(row),
        _ => pb::ThreadGoalResponse {
            goal: None,
            found: false,
        },
    })
}

/// Change an open goal under a compare-and-set on its revision.
pub(crate) async fn update_thread_goal(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::UpdateThreadGoalRequest,
) -> Result<pb::ThreadGoalResponse, Status> {
    refuse_service_writer(caller)?;
    let org_id = req.org_id.trim();
    let goal_id = req.goal_id.trim();
    if org_id.is_empty() || goal_id.is_empty() {
        return Err(Status::invalid_argument("org_id and goal_id are required"));
    }
    check_update_paths(&req.update_paths)?;
    caller.authorize_org(org_id)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    let current: Option<GoalRow> = sqlx::query_as(&format!(
        "SELECT {GOAL_COLUMNS} FROM thread_goals WHERE id = $1 AND org_id = $2 FOR UPDATE"
    ))
    .bind(goal_id)
    .bind(org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    let mut goal = current.ok_or_else(|| Status::not_found("thread goal not found"))?;
    authorize_thread_write(&mut tx, caller, org_id, &goal.thread_id).await?;
    check_revision(goal.revision, req.expected_revision)?;
    if !OPEN_STATUSES.contains(&goal.status.as_str()) {
        return Err(Status::failed_precondition(GOAL_CLOSED));
    }

    let wants = |path: &str| req.update_paths.iter().any(|named| named == path);
    if wants("objective") {
        goal.objective = check_objective(&req.objective)?;
    }
    if wants("success_criteria") {
        goal.success_criteria = sqlx::types::Json(check_criteria(&req.success_criteria)?);
    }
    if wants("token_budget") {
        goal.token_budget = check_token_budget(req.token_budget)?;
    }
    if wants("cost_budget_usd_micros") {
        goal.cost_budget_usd_micros = check_cost_budget(req.cost_budget_usd_micros)?;
    }
    if wants("max_turns") {
        goal.max_turns = check_max_turns(req.max_turns, false)?;
    }
    let status = if wants("status") {
        check_update_status(&req.status)?
    } else if goal.status == "paused" {
        "paused"
    } else {
        "active"
    };

    let row: GoalRow = sqlx::query_as(&format!(
        "UPDATE thread_goals
         SET objective = $2, success_criteria = $3, token_budget = $4,
             cost_budget_usd_micros = $5, max_turns = $6, status = $7,
             status_reason = CASE WHEN $7 = status THEN status_reason ELSE '' END,
             revision = revision + 1, updated_at = now(),
             closed_at = CASE WHEN $7 = 'cleared' THEN now() ELSE NULL END
         WHERE id = $1
         RETURNING {GOAL_COLUMNS}"
    ))
    .bind(&goal.id)
    .bind(&goal.objective)
    .bind(&goal.success_criteria)
    .bind(goal.token_budget)
    .bind(goal.cost_budget_usd_micros)
    .bind(goal.max_turns)
    .bind(status)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(if row.status == "cleared" {
        // A cleared goal reads as no goal, here as on every read.
        pb::ThreadGoalResponse {
            goal: None,
            found: false,
        }
    } else {
        found(row)
    })
}

/// What a goal's counters say about its caps: the stop state and its closed reason, or `None`
/// while the goal may take another turn. A spent budget is named before the turn cap, so a goal
/// that hit both reads as out of budget. A cap that is not set never stops the goal.
fn stop_state(goal: &GoalRow) -> Option<(&'static str, &'static str)> {
    if goal
        .token_budget
        .is_some_and(|budget| goal.tokens_used >= budget)
    {
        return Some(("budget_reached", "token_budget"));
    }
    if goal
        .cost_budget_usd_micros
        .is_some_and(|budget| goal.cost_used_usd_micros >= budget)
    {
        return Some(("budget_reached", "cost_budget"));
    }
    if goal.turns_used >= goal.max_turns {
        return Some(("turns_reached", "max_turns"));
    }
    None
}

/// The refusal a claim answers for a goal in `stop` state.
fn stop_refusal(stop: &str) -> Status {
    Status::failed_precondition(if stop == "turns_reached" {
        TURNS_REACHED
    } else {
        BUDGET_REACHED
    })
}

/// Whether a settled turn counts toward `max_turns`. Every turn that started a run counts, however
/// it ended, so a run that keeps failing still exhausts the cap; a turn cancelled before any run
/// existed (a refused or abandoned start) spent nothing and does not.
fn turn_counts(run_bound: bool, outcome: &str) -> bool {
    run_bound || outcome != "cancelled"
}

/// What a goal has left of a budget, never below zero.
fn remaining(budget: Option<i64>, used: i64) -> u64 {
    budget.map_or(0, |budget| {
        u64::try_from(budget.saturating_sub(used)).unwrap_or(0)
    })
}

fn check_outcome(raw: &str) -> Result<&'static str, Status> {
    let outcome = raw.trim();
    SETTLE_OUTCOMES
        .iter()
        .copied()
        .find(|known| *known == outcome)
        .ok_or_else(|| invalid("outcome is not a known run outcome"))
}

fn check_usage_source(raw: &str) -> Result<&'static str, Status> {
    let source = raw.trim();
    if source.is_empty() {
        return Ok("run_response");
    }
    USAGE_SOURCES
        .iter()
        .copied()
        .find(|known| *known == source)
        .ok_or_else(|| invalid("usage_source is not a known source"))
}

/// The cost basis as the table holds it: a closed lower-case token, or empty.
fn check_cost_basis(raw: &str) -> Result<String, Status> {
    let basis = raw.trim();
    if basis.len() > 32 || !basis.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
        return Err(invalid("cost_basis is not a closed token"));
    }
    Ok(basis.to_owned())
}

/// A token count bounded by [`MAX_TURN_TOKENS`], as the counters' signed type.
fn bounded_tokens(raw: u64) -> i64 {
    i64::try_from(raw.min(MAX_TURN_TOKENS)).unwrap_or(i64::MAX)
}

/// The goal, locked, for one of its own turn writes: the caller must own its thread. A bind or a
/// settle does not repeat the claim's Space and waiver checks, so the accounting of a turn the
/// claim admitted is never refused afterwards.
async fn lock_owned_goal(
    tx: &mut Transaction<'_, Postgres>,
    caller: &VerifiedIdentity,
    org_id: &str,
    goal_id: &str,
) -> Result<GoalRow, Status> {
    let goal: GoalRow = sqlx::query_as(&format!(
        "SELECT {GOAL_COLUMNS} FROM thread_goals WHERE id = $1 AND org_id = $2 FOR UPDATE"
    ))
    .bind(goal_id)
    .bind(org_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)?
    .ok_or_else(|| Status::not_found("thread goal not found"))?;
    let thread = read_thread(&mut **tx, &goal.thread_id).await?;
    if thread.org_id != org_id {
        return Err(Status::not_found("thread goal not found"));
    }
    authorize_thread_owner_row(
        caller,
        &thread.org_id,
        &thread.user_id,
        thread.space(),
        OwnerIntent::Mutate,
    )?;
    Ok(goal)
}

/// Move an open goal to a stop state; a goal already closed is left as it is.
async fn close_goal(
    tx: &mut Transaction<'_, Postgres>,
    goal_id: &str,
    status: &str,
    reason: &str,
) -> Result<Option<GoalRow>, Status> {
    sqlx::query_as(&format!(
        "UPDATE thread_goals
         SET status = $2, status_reason = $3, revision = revision + 1,
             updated_at = now(), closed_at = now()
         WHERE id = $1 AND status IN ('active', 'paused')
         RETURNING {GOAL_COLUMNS}"
    ))
    .bind(goal_id)
    .bind(status)
    .bind(reason)
    .fetch_optional(&mut **tx)
    .await
    .map_err(internal)
}

/// Admit one continuation turn toward an active goal (EP-7 S2). Refused while another turn of the
/// goal is in flight and once a cap is reached; reaching a cap here (a budget lowered below what was
/// spent, or a lease that expired into the cap) also closes the goal, so the bar shows why.
pub(crate) async fn claim_goal_turn(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::ClaimGoalTurnRequest,
) -> Result<pb::ClaimGoalTurnResponse, Status> {
    refuse_service_writer(caller)?;
    let org_id = req.org_id.trim();
    let goal_id = req.goal_id.trim();
    if org_id.is_empty() || goal_id.is_empty() {
        return Err(Status::invalid_argument("org_id and goal_id are required"));
    }
    caller.authorize_org(org_id)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    let mut goal: GoalRow = sqlx::query_as(&format!(
        "SELECT {GOAL_COLUMNS} FROM thread_goals WHERE id = $1 AND org_id = $2 FOR UPDATE"
    ))
    .bind(goal_id)
    .bind(org_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?
    .ok_or_else(|| Status::not_found("thread goal not found"))?;
    // The full write checks of a set: a claim starts new spending on the thread.
    authorize_thread_write(&mut tx, caller, org_id, &goal.thread_id).await?;
    if req.expected_revision != 0 {
        check_revision(goal.revision, req.expected_revision)?;
    }
    if goal.status != "active" {
        return Err(Status::failed_precondition(NOT_ACTIVE));
    }
    // EP-7 S4: no goal turn starts while a decision waits on the owner, nor past the hourly rate.
    claim_guard::refuse_unsettled_claim(&mut tx, &goal.thread_id, &goal.id).await?;

    // A claim whose lease ran out belongs to a turn nobody will settle (a crashed replica). It is
    // settled here, with no usage, and counts as a turn.
    let expired = sqlx::query(
        "UPDATE thread_goal_turns
         SET settled_at = now(), outcome = 'lease_expired', usage_source = 'none', counted = true
         WHERE goal_id = $1 AND settled_at IS NULL AND claim_expires_at <= now()",
    )
    .bind(&goal.id)
    .execute(&mut *tx)
    .await
    .map_err(internal)?
    .rows_affected();
    if expired > 0 {
        let expired = i32::try_from(expired).unwrap_or(i32::MAX);
        sqlx::query(
            "UPDATE thread_goals SET turns_used = turns_used + $2, updated_at = now() WHERE id = $1",
        )
        .bind(&goal.id)
        .bind(expired)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
        goal.turns_used = goal.turns_used.saturating_add(expired);
    }

    let in_flight: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM thread_goal_turns WHERE goal_id = $1 AND settled_at IS NULL)",
    )
    .bind(&goal.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    if in_flight {
        // The expiry above is kept: it is true whatever this claim answers.
        tx.commit().await.map_err(internal)?;
        return Err(Status::failed_precondition(TURN_IN_FLIGHT));
    }
    if let Some((stop, reason)) = stop_state(&goal) {
        close_goal(&mut tx, &goal.id, stop, reason).await?;
        tx.commit().await.map_err(internal)?;
        return Err(stop_refusal(stop));
    }

    let turn_no: i32 = sqlx::query_scalar(
        "INSERT INTO thread_goal_turns (goal_id, turn_no, org_id, claim_expires_at)
         SELECT $1, COALESCE(MAX(turn_no), 0) + 1, $2, now() + make_interval(mins => $3)
         FROM thread_goal_turns WHERE goal_id = $1
         RETURNING turn_no",
    )
    .bind(&goal.id)
    .bind(&goal.org_id)
    .bind(CLAIM_LEASE_MINUTES)
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok(pb::ClaimGoalTurnResponse {
        turn_no: u32::try_from(turn_no).unwrap_or_default(),
        tokens_remaining: remaining(goal.token_budget, goal.tokens_used),
        cost_remaining_usd_micros: remaining(
            goal.cost_budget_usd_micros,
            goal.cost_used_usd_micros,
        ),
        token_budget_set: goal.token_budget.is_some(),
        cost_budget_set: goal.cost_budget_usd_micros.is_some(),
    })
}

/// Name the run a claimed, unsettled turn started. A turn binds one run, and a run one turn. A
/// pump-sent turn (`goal_auto`, EP-7 S4) also stamps its run for audit.
pub(crate) async fn bind_goal_turn_run(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::BindGoalTurnRunRequest,
    goal_auto: bool,
) -> Result<pb::BindGoalTurnRunResponse, Status> {
    refuse_service_writer(caller)?;
    let org_id = req.org_id.trim();
    let goal_id = req.goal_id.trim();
    let run_id = req.run_id.trim();
    if org_id.is_empty() || goal_id.is_empty() || run_id.is_empty() || req.turn_no == 0 {
        return Err(Status::invalid_argument(
            "org_id, goal_id, turn_no and run_id are required",
        ));
    }
    let turn_no = i32::try_from(req.turn_no).map_err(|_| invalid("turn_no out of range"))?;
    caller.authorize_org(org_id)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    let goal = lock_owned_goal(&mut tx, caller, org_id, goal_id).await?;
    let bound = sqlx::query(
        "UPDATE thread_goal_turns SET run_id = $3
         WHERE goal_id = $1 AND turn_no = $2 AND settled_at IS NULL
           AND (run_id IS NULL OR run_id = $3)",
    )
    .bind(&goal.id)
    .bind(turn_no)
    .bind(run_id)
    .execute(&mut *tx)
    .await
    .map_err(|error| match &error {
        // The run already settles another turn of this goal.
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            Status::failed_precondition(TURN_MISMATCH)
        }
        _ => internal(error),
    })?
    .rows_affected();
    if bound == 0 {
        return Err(Status::failed_precondition(TURN_MISMATCH));
    }
    if goal_auto {
        claim_guard::stamp_goal_auto(&mut tx, run_id, org_id).await?;
    }
    tx.commit().await.map_err(internal)?;
    Ok(pb::BindGoalTurnRunResponse {})
}

/// The turn row a settle reads.
#[derive(Debug, sqlx::FromRow)]
struct TurnRow {
    run_id: Option<String>,
    outcome: Option<String>,
    usage_source: String,
}

/// Settle a claimed turn from its run's usage, then apply the stop rules. Idempotent: a settled
/// turn is answered as it stands, except a turn the next claim settled `lease_expired` with no
/// usage, whose late settle adds the run's usage once (the turn was already counted).
pub(crate) async fn settle_goal_turn(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    req: pb::SettleGoalTurnRequest,
) -> Result<pb::ThreadGoalResponse, Status> {
    refuse_service_writer(caller)?;
    let org_id = req.org_id.trim();
    let goal_id = req.goal_id.trim();
    if org_id.is_empty() || goal_id.is_empty() || req.turn_no == 0 {
        return Err(Status::invalid_argument(
            "org_id, goal_id and turn_no are required",
        ));
    }
    let turn_no = i32::try_from(req.turn_no).map_err(|_| invalid("turn_no out of range"))?;
    let outcome = check_outcome(&req.outcome)?;
    let usage_source = check_usage_source(&req.usage_source)?;
    let cost_basis = check_cost_basis(&req.cost_basis)?;
    let input = bounded_tokens(req.metered_input_tokens);
    let output = bounded_tokens(req.metered_output_tokens);
    let judge = bounded_tokens(req.judge_tokens);
    // -1 (any negative) is "could not be priced": stored as NULL and adding nothing.
    let cost = (req.cost_usd_micros >= 0)
        .then(|| req.cost_usd_micros.min(bounded_tokens(MAX_TURN_TOKENS)));
    let run_id = req.run_id.trim();
    caller.authorize_org(org_id)?;

    let mut tx = pool.begin().await.map_err(internal)?;
    let goal = lock_owned_goal(&mut tx, caller, org_id, goal_id).await?;
    let turn: TurnRow = sqlx::query_as(
        "SELECT run_id, outcome, usage_source FROM thread_goal_turns
         WHERE goal_id = $1 AND turn_no = $2 FOR UPDATE",
    )
    .bind(&goal.id)
    .bind(turn_no)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?
    .ok_or_else(|| Status::not_found("thread goal turn not found"))?;
    if let Some(bound) = turn.run_id.as_deref() {
        if !run_id.is_empty() && bound != run_id {
            return Err(Status::failed_precondition(TURN_MISMATCH));
        }
    }
    let run_bound = turn.run_id.is_some() || !run_id.is_empty();
    let late = turn.outcome.as_deref() == Some("lease_expired") && turn.usage_source == "none";
    if turn.outcome.is_some() && !late {
        // Settled already: a retried settle changes nothing.
        tx.commit().await.map_err(internal)?;
        return Ok(found(goal));
    }
    // A late settle keeps the turn counted once (the claim counted it); a first settle counts it
    // by the rule of [`turn_counts`].
    let counts = !late && turn_counts(run_bound, outcome);
    sqlx::query(
        "UPDATE thread_goal_turns
         SET settled_at = COALESCE(settled_at, now()), outcome = $3,
             run_id = COALESCE(run_id, NULLIF($4, '')),
             metered_input_tokens = $5, metered_output_tokens = $6, cost_usd_micros = $7,
             cost_basis = $8, judge_tokens = $9, usage_source = $10,
             counted = counted OR $11
         WHERE goal_id = $1 AND turn_no = $2",
    )
    .bind(&goal.id)
    .bind(turn_no)
    .bind(outcome)
    .bind(run_id)
    .bind(input)
    .bind(output)
    .bind(cost)
    .bind(&cost_basis)
    .bind(judge)
    .bind(usage_source)
    .bind(counts)
    .execute(&mut *tx)
    .await
    .map_err(|error| match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
            Status::failed_precondition(TURN_MISMATCH)
        }
        _ => internal(error),
    })?;

    let tokens = input.saturating_add(output).saturating_add(judge);
    let mut row: GoalRow = sqlx::query_as(&format!(
        "UPDATE thread_goals
         SET tokens_used = tokens_used + $2,
             cost_used_usd_micros = cost_used_usd_micros + COALESCE($3, 0),
             turns_used = turns_used + $4, updated_at = now()
         WHERE id = $1
         RETURNING {GOAL_COLUMNS}"
    ))
    .bind(&goal.id)
    .bind(tokens)
    .bind(cost)
    .bind(i32::from(counts))
    .fetch_one(&mut *tx)
    .await
    .map_err(internal)?;
    // A goal paused while its turn ran still stops on a spent cap: what was spent is spent.
    if OPEN_STATUSES.contains(&row.status.as_str()) {
        if let Some((stop, reason)) = stop_state(&row) {
            if let Some(closed) = close_goal(&mut tx, &row.id, stop, reason).await? {
                row = closed;
            }
        }
    }
    tx.commit().await.map_err(internal)?;
    Ok(found(row))
}

fn found(row: GoalRow) -> pb::ThreadGoalResponse {
    pb::ThreadGoalResponse {
        goal: Some(row.into_proto()),
        found: true,
    }
}

/// The columns a goal is read with, in [`GoalRow`]'s order.
const GOAL_COLUMNS: &str = "id, thread_id, org_id, user_id, objective, success_criteria, \
     token_budget, cost_budget_usd_micros, max_turns, status, status_reason, revision, \
     created_at, updated_at, tokens_used, cost_used_usd_micros, turns_used";

#[derive(Debug, sqlx::FromRow)]
struct GoalRow {
    id: String,
    thread_id: String,
    org_id: String,
    user_id: String,
    objective: String,
    success_criteria: sqlx::types::Json<Vec<String>>,
    token_budget: Option<i64>,
    cost_budget_usd_micros: Option<i64>,
    max_turns: i32,
    status: String,
    status_reason: String,
    revision: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    // The budget ledger's cached counters (EP-7 S2, migration 0092).
    tokens_used: i64,
    cost_used_usd_micros: i64,
    turns_used: i32,
}

impl GoalRow {
    fn into_proto(self) -> pb::ThreadGoal {
        let unsigned = |value: Option<i64>| value.and_then(|v| u64::try_from(v).ok()).unwrap_or(0);
        pb::ThreadGoal {
            id: self.id,
            thread_id: self.thread_id,
            org_id: self.org_id,
            user_id: self.user_id,
            objective: self.objective,
            success_criteria: self.success_criteria.0,
            token_budget: unsigned(self.token_budget),
            cost_budget_usd_micros: unsigned(self.cost_budget_usd_micros),
            max_turns: u32::try_from(self.max_turns).unwrap_or_default(),
            status: self.status,
            status_reason: self.status_reason,
            revision: u64::try_from(self.revision).unwrap_or_default(),
            created_at: Some(to_proto_timestamp(self.created_at)),
            updated_at: Some(to_proto_timestamp(self.updated_at)),
            tokens_used: u64::try_from(self.tokens_used).unwrap_or_default(),
            cost_used_usd_micros: u64::try_from(self.cost_used_usd_micros).unwrap_or_default(),
            turns_used: u32::try_from(self.turns_used).unwrap_or_default(),
            // The judge (EP-7 S3) fills this; empty until it exists.
            last_verdict: String::new(),
        }
    }
}

fn internal(error: sqlx::Error) -> Status {
    Status::internal(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: &str = "org-thread-goals";
    const OWNER: &str = "user-goal-owner";

    fn set_request(thread: &str) -> pb::SetThreadGoalRequest {
        pb::SetThreadGoalRequest {
            thread_id: thread.to_owned(),
            org_id: ORG.to_owned(),
            objective: "Ship the quarterly report".to_owned(),
            success_criteria: vec!["Numbers reconciled".to_owned()],
            ..Default::default()
        }
    }

    #[test]
    fn thread_goal_limits_rejected() {
        assert!(check_objective("").is_err());
        assert!(check_objective("   ").is_err());
        assert!(check_objective(&"x".repeat(MAX_OBJECTIVE_CHARS)).is_ok());
        let long = check_objective(&"x".repeat(MAX_OBJECTIVE_CHARS + 1)).unwrap_err();
        assert_eq!(long.code(), tonic::Code::InvalidArgument);
        assert!(long.message().starts_with(INVALID_GOAL));

        let nine: Vec<String> = (0..9).map(|i| format!("criterion {i}")).collect();
        assert!(check_criteria(&nine).is_err());
        assert_eq!(check_criteria(&nine[..8]).unwrap().len(), 8);
        assert!(check_criteria(&["x".repeat(MAX_CRITERION_CHARS + 1)]).is_err());
        // Blank lines are dropped before the count, so they never use up a slot.
        let blanks = vec![" ".to_owned(); 12];
        assert!(check_criteria(&blanks).unwrap().is_empty());
    }

    #[test]
    fn thread_goal_turn_cap_and_budgets_follow_d11() {
        assert_eq!(check_max_turns(0, true).unwrap(), 10);
        assert!(check_max_turns(0, false).is_err(), "an update names a real cap");
        assert_eq!(check_max_turns(50, true).unwrap(), 50);
        assert!(check_max_turns(51, true).is_err());
        assert_eq!(check_token_budget(0).unwrap(), None);
        assert_eq!(check_token_budget(MAX_TOKEN_BUDGET).unwrap(), Some(5_000_000));
        assert!(check_token_budget(MAX_TOKEN_BUDGET + 1).is_err());
        assert_eq!(check_cost_budget(0).unwrap(), None);
        assert!(check_cost_budget(u64::MAX).is_err());
    }

    #[test]
    fn thread_goal_update_writes_only_owner_statuses() {
        for status in ["active", "paused", "cleared"] {
            assert_eq!(check_update_status(status).unwrap(), status);
        }
        for status in ["met", "budget_reached", "turns_reached", "blocked", ""] {
            assert!(check_update_status(status).is_err(), "{status}");
        }
    }

    #[test]
    fn thread_goal_update_paths_are_closed_and_required() {
        assert!(check_update_paths(&[]).is_err());
        assert!(check_update_paths(&["objective".to_owned(), "status".to_owned()]).is_ok());
        assert!(check_update_paths(&["user_id".to_owned()]).is_err());
    }

    #[test]
    fn thread_goal_stale_revision_is_a_conflict() {
        assert!(check_revision(3, 3).is_ok());
        let stale = check_revision(4, 3).unwrap_err();
        assert_eq!(stale.code(), tonic::Code::Aborted);
        assert_eq!(stale.message(), REVISION_CONFLICT);
    }

    #[test]
    fn thread_goal_zdr_waived_and_service_callers_refused() {
        assert!(refuse_retention_exempt(&VerifiedIdentity::user_for_test(ORG, OWNER)).is_ok());
        let zdr = VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true);
        assert_eq!(
            refuse_retention_exempt(&zdr).unwrap_err().code(),
            tonic::Code::FailedPrecondition
        );
        let waived = VerifiedIdentity::user_for_test_with_waiver(ORG, OWNER, "conn-1", "ack-1");
        assert!(refuse_retention_exempt(&waived).is_err());
        let service = VerifiedIdentity::service_for_test(ORG, &["session:write"], false);
        assert_eq!(
            refuse_service_writer(&service).unwrap_err().code(),
            tonic::Code::PermissionDenied
        );
    }

    #[test]
    fn thread_goal_switch_reads_literal_true_only() {
        assert!(!mp_contracts::loop_fold::switch_from(None));
        assert!(!mp_contracts::loop_fold::switch_from(Some("1")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("yes")));
        assert!(!mp_contracts::loop_fold::switch_from(Some("on")));
        assert!(mp_contracts::loop_fold::switch_from(Some("true")));
    }

    fn counters(
        token_budget: Option<i64>,
        cost_budget: Option<i64>,
        max_turns: i32,
        used: (i64, i64, i32),
    ) -> GoalRow {
        GoalRow {
            id: "goal_test".to_owned(),
            thread_id: "thread".to_owned(),
            org_id: ORG.to_owned(),
            user_id: OWNER.to_owned(),
            objective: "aim".to_owned(),
            success_criteria: sqlx::types::Json(Vec::new()),
            token_budget,
            cost_budget_usd_micros: cost_budget,
            max_turns,
            status: "active".to_owned(),
            status_reason: String::new(),
            revision: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            tokens_used: used.0,
            cost_used_usd_micros: used.1,
            turns_used: used.2,
        }
    }

    #[test]
    fn thread_goal_stop_state_names_budget_before_turns() {
        assert_eq!(stop_state(&counters(Some(100), None, 10, (99, 0, 9))), None);
        assert_eq!(
            stop_state(&counters(Some(100), None, 10, (100, 0, 1))),
            Some(("budget_reached", "token_budget"))
        );
        assert_eq!(
            stop_state(&counters(None, Some(5_000), 10, (0, 5_001, 1))),
            Some(("budget_reached", "cost_budget"))
        );
        assert_eq!(
            stop_state(&counters(None, None, 3, (10_000_000, 0, 3))),
            Some(("turns_reached", "max_turns")),
            "no budget set: only the turn cap applies (the subscription route)"
        );
        assert_eq!(
            stop_state(&counters(Some(10), None, 3, (10, 0, 3))),
            Some(("budget_reached", "token_budget")),
            "a goal that hit both caps reads as out of budget"
        );
        assert_eq!(stop_refusal("turns_reached").message(), TURNS_REACHED);
        assert_eq!(stop_refusal("budget_reached").message(), BUDGET_REACHED);
    }

    #[test]
    fn thread_goal_turn_counts_every_started_run() {
        for outcome in SETTLE_OUTCOMES {
            assert!(turn_counts(true, outcome), "{outcome} with a run counts");
        }
        assert!(!turn_counts(false, "cancelled"), "a start that never ran is free");
        assert!(turn_counts(false, "failed"));
        assert!(turn_counts(false, "unknown"));
    }

    #[test]
    fn thread_goal_remaining_never_negative() {
        assert_eq!(remaining(None, 10), 0);
        assert_eq!(remaining(Some(100), 40), 60);
        assert_eq!(remaining(Some(100), 140), 0);
    }

    #[test]
    fn thread_goal_settle_inputs_are_closed() {
        assert_eq!(check_outcome(" completed ").unwrap(), "completed");
        assert!(check_outcome("lease_expired").is_err(), "only a claim writes it");
        assert!(check_outcome("done").is_err());
        assert_eq!(check_usage_source("").unwrap(), "run_response");
        assert_eq!(check_usage_source("progress").unwrap(), "progress");
        assert!(check_usage_source("guess").is_err());
        assert_eq!(check_cost_basis("metered").unwrap(), "metered");
        assert_eq!(check_cost_basis("").unwrap(), "");
        assert!(check_cost_basis("Metered; drop").is_err());
        assert_eq!(bounded_tokens(u64::MAX), bounded_tokens(MAX_TURN_TOKENS));
    }

    // The database tests below run with `--ignored` against a disposable Postgres, as the other
    // session-core database tests do. They call the store functions directly, so the switch is not
    // needed; `thread_goal_rpc_is_refused_while_the_switch_is_off` tests the switch.

    async fn connect() -> PgPool {
        let pool =
            PgPool::connect(&std::env::var("DATABASE_URL").expect("disposable database required"))
                .await
                .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed_thread(pool: &PgPool) -> String {
        let thread = new_ulid();
        sqlx::query(
            "INSERT INTO threads (id, session_key, org_id, user_id) VALUES ($1, $1, $2, $3)",
        )
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(pool)
        .await
        .unwrap();
        thread
    }

    async fn rows(pool: &PgPool, thread: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM thread_goals WHERE thread_id = $1")
            .bind(thread)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    fn owner() -> VerifiedIdentity {
        VerifiedIdentity::user_for_test(ORG, OWNER)
    }

    fn update(goal: &pb::ThreadGoal, paths: &[&str]) -> pb::UpdateThreadGoalRequest {
        pb::UpdateThreadGoalRequest {
            goal_id: goal.id.clone(),
            org_id: ORG.to_owned(),
            expected_revision: goal.revision,
            update_paths: paths.iter().map(|path| (*path).to_owned()).collect(),
            ..Default::default()
        }
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn set_thread_goal_once_per_thread() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let goal = set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap()
            .goal
            .unwrap();
        assert!(goal.id.starts_with("goal_"));
        assert_eq!(goal.status, "active");
        assert_eq!(goal.max_turns, DEFAULT_MAX_TURNS);
        assert_eq!(goal.revision, 1);
        let again = set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(again.code(), tonic::Code::AlreadyExists);
        assert_eq!(again.message(), GOAL_EXISTS);
        assert_eq!(rows(&pool, &thread).await, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn set_with_replace_clears_old() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let first = set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap()
            .goal
            .unwrap();
        let mut replace = set_request(&thread);
        replace.objective = "A new aim".to_owned();
        replace.replace = true;
        let second = set_thread_goal(&pool, &owner(), replace)
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_ne!(first.id, second.id);
        let old: String = sqlx::query_scalar("SELECT status FROM thread_goals WHERE id = $1")
            .bind(&first.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(old, "cleared");
        let read = get_thread_goal(
            &pool,
            &owner(),
            pb::GetThreadGoalRequest {
                thread_id: thread,
                org_id: ORG.to_owned(),
            },
        )
        .await
        .unwrap();
        assert_eq!(read.goal.unwrap().objective, "A new aim");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn update_cas_revision_conflict() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let goal = set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap()
            .goal
            .unwrap();
        let mut pause = update(&goal, &["status"]);
        pause.status = "paused".to_owned();
        let paused = update_thread_goal(&pool, &owner(), pause.clone())
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(paused.status, "paused");
        assert_eq!(paused.revision, 2);
        let stale = update_thread_goal(&pool, &owner(), pause).await.unwrap_err();
        assert_eq!(stale.message(), REVISION_CONFLICT);

        let mut clear = update(&paused, &["status"]);
        clear.status = "cleared".to_owned();
        let cleared = update_thread_goal(&pool, &owner(), clear).await.unwrap();
        assert!(!cleared.found, "a cleared goal reads as no goal");
        let mut reopen = update(&paused, &["status"]);
        reopen.expected_revision = 3;
        reopen.status = "active".to_owned();
        let closed = update_thread_goal(&pool, &owner(), reopen).await.unwrap_err();
        assert_eq!(closed.message(), GOAL_CLOSED);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn foreign_user_not_found() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        let goal = set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap()
            .goal
            .unwrap();
        let other = VerifiedIdentity::user_for_test(ORG, "user-goal-other");
        assert!(set_thread_goal(&pool, &other, set_request(&thread)).await.is_err());
        let read = pb::GetThreadGoalRequest {
            thread_id: thread.clone(),
            org_id: ORG.to_owned(),
        };
        assert!(get_thread_goal(&pool, &other, read).await.is_err());
        let mut pause = update(&goal, &["status"]);
        pause.status = "paused".to_owned();
        assert!(update_thread_goal(&pool, &other, pause).await.is_err());
        let foreign_org = VerifiedIdentity::user_for_test("org-elsewhere", OWNER);
        let mut elsewhere = set_request(&thread);
        elsewhere.org_id = "org-elsewhere".to_owned();
        assert_eq!(
            set_thread_goal(&pool, &foreign_org, elsewhere)
                .await
                .unwrap_err()
                .code(),
            tonic::Code::NotFound
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn space_thread_refused() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        sqlx::query(
            "UPDATE threads SET space_id = 'space-goal', origin = 'space', space_decision_ref = 'dec-t', recipient_audience_ref = 'aud-t', privacy_policy_ref = 'pp-t', resource_authorization_ref = 'ra-t', authority_revision = 1, recipient_audience_revision = 1, recipient_audience_hash = 'test-audience' WHERE id = $1",
        )
        .bind(&thread)
        .execute(&pool)
        .await
        .unwrap();
        let refused = set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), SPACE_UNSUPPORTED);
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn zdr_and_waived_credentials_refused() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        for caller in [
            VerifiedIdentity::user_for_test_with_zdr(ORG, OWNER, true),
            VerifiedIdentity::user_for_test_with_waiver(ORG, OWNER, "conn-1", "ack-1"),
        ] {
            let mut request = Request::new(set_request(&thread));
            request.extensions_mut().insert(caller);
            let refused = ThreadGoalGrpc { pool: pool.clone() }
                .set_thread_goal(request)
                .await
                .unwrap_err();
            assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        }
        // A thread that holds a waived run refuses the owner's own persistent credential too.
        sqlx::query(
            "INSERT INTO runs (id, thread_id, goal, org_id, user_id, status, metadata,
                               retention_class, waiver_ack_id, waiver_connection_id)
             VALUES ($1, $2, 'g', $3, $4, 'completed', '{}'::jsonb,
                     'zdr_waived', 'ack-1', 'conn-1')",
        )
        .bind(new_ulid())
        .bind(&thread)
        .bind(ORG)
        .bind(OWNER)
        .execute(&pool)
        .await
        .unwrap();
        let waived = set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap_err();
        assert_eq!(waived.message(), WAIVED_THREAD);
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn thread_delete_cascades_goal() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        crate::grpc::delete_thread_rows(&mut tx, &thread).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn gdpr_erasure_deletes_goals() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        set_thread_goal(&pool, &owner(), set_request(&thread))
            .await
            .unwrap();
        // The purge names the table (the thread cascade is the second line of defence).
        let summary = crate::gdpr::purge_organization_data(&pool, ORG, None)
            .await
            .unwrap();
        assert!(summary.thread_goals >= 1);
        assert_eq!(rows(&pool, &thread).await, 0);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn thread_goal_rpc_is_refused_while_the_switch_is_off() {
        let pool = connect().await;
        let thread = seed_thread(&pool).await;
        // The suite runs with the switch unset, which is the default this test proves.
        let mut request = Request::new(set_request(&thread));
        request.extensions_mut().insert(owner());
        let refused = ThreadGoalGrpc { pool: pool.clone() }
            .set_thread_goal(request)
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::Unavailable);
        assert_eq!(refused.message(), DISABLED);
    }

    async fn goal_with(pool: &PgPool, token_budget: u64, max_turns: u32) -> pb::ThreadGoal {
        let thread = seed_thread(pool).await;
        let mut request = set_request(&thread);
        request.token_budget = token_budget;
        request.max_turns = max_turns;
        set_thread_goal(pool, &owner(), request)
            .await
            .unwrap()
            .goal
            .unwrap()
    }

    fn claim(goal: &pb::ThreadGoal) -> pb::ClaimGoalTurnRequest {
        pb::ClaimGoalTurnRequest {
            goal_id: goal.id.clone(),
            org_id: ORG.to_owned(),
            expected_revision: 0,
        }
    }

    fn settle(
        goal: &pb::ThreadGoal,
        turn_no: u32,
        run_id: &str,
        tokens: u64,
    ) -> pb::SettleGoalTurnRequest {
        pb::SettleGoalTurnRequest {
            goal_id: goal.id.clone(),
            org_id: ORG.to_owned(),
            turn_no,
            run_id: run_id.to_owned(),
            outcome: "completed".to_owned(),
            metered_input_tokens: tokens,
            metered_output_tokens: 0,
            cost_usd_micros: -1,
            cost_basis: "metered".to_owned(),
            judge_tokens: 0,
            usage_source: "run_response".to_owned(),
        }
    }

    async fn bind(pool: &PgPool, goal: &pb::ThreadGoal, turn_no: u32, run_id: &str) {
        bind_goal_turn_run(
            pool,
            &owner(),
            pb::BindGoalTurnRunRequest {
                goal_id: goal.id.clone(),
                org_id: ORG.to_owned(),
                turn_no,
                run_id: run_id.to_owned(),
            },
            false,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn claim_is_single_in_flight() {
        let pool = connect().await;
        let goal = goal_with(&pool, 0, 10).await;
        let first = claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        assert_eq!(first.turn_no, 1);
        assert!(!first.token_budget_set);
        let second = claim_goal_turn(&pool, &owner(), claim(&goal))
            .await
            .unwrap_err();
        assert_eq!(second.code(), tonic::Code::FailedPrecondition);
        assert_eq!(second.message(), TURN_IN_FLIGHT);
        bind(&pool, &goal, 1, "run-a").await;
        settle_goal_turn(&pool, &owner(), settle(&goal, 1, "run-a", 10))
            .await
            .unwrap();
        let next = claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        assert_eq!(next.turn_no, 2);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn expired_claim_settles_lease_expired_then_reclaims() {
        let pool = connect().await;
        let goal = goal_with(&pool, 0, 10).await;
        claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        sqlx::query(
            "UPDATE thread_goal_turns SET claim_expires_at = now() - interval '1 minute'
             WHERE goal_id = $1",
        )
        .bind(&goal.id)
        .execute(&pool)
        .await
        .unwrap();
        let again = claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        assert_eq!(again.turn_no, 2);
        let (outcome, counted): (String, bool) = sqlx::query_as(
            "SELECT outcome, counted FROM thread_goal_turns WHERE goal_id = $1 AND turn_no = 1",
        )
        .bind(&goal.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(outcome, "lease_expired");
        assert!(counted, "an expired turn still counts toward the cap");
        // The late settle of the expired turn adds its usage once, and no second turn.
        let late = settle_goal_turn(&pool, &owner(), settle(&goal, 1, "run-late", 500))
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(late.tokens_used, 500);
        assert_eq!(late.turns_used, 1);
        let replay = settle_goal_turn(&pool, &owner(), settle(&goal, 1, "run-late", 500))
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(replay.tokens_used, 500);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn settle_idempotent_on_run_id() {
        let pool = connect().await;
        let goal = goal_with(&pool, 0, 10).await;
        claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        bind(&pool, &goal, 1, "run-x").await;
        let once = settle_goal_turn(&pool, &owner(), settle(&goal, 1, "run-x", 1_000))
            .await
            .unwrap()
            .goal
            .unwrap();
        let twice = settle_goal_turn(&pool, &owner(), settle(&goal, 1, "run-x", 1_000))
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(once.tokens_used, 1_000);
        assert_eq!(twice.tokens_used, 1_000);
        assert_eq!(twice.turns_used, 1);
        let other_run = settle_goal_turn(&pool, &owner(), settle(&goal, 1, "run-y", 1))
            .await
            .unwrap_err();
        assert_eq!(other_run.message(), TURN_MISMATCH);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn budget_reached_moves_status() {
        let pool = connect().await;
        let goal = goal_with(&pool, 30_000, 10).await;
        let admitted = claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        assert!(admitted.token_budget_set);
        assert_eq!(admitted.tokens_remaining, 30_000);
        bind(&pool, &goal, 1, "run-big").await;
        let settled = settle_goal_turn(&pool, &owner(), settle(&goal, 1, "run-big", 31_000))
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(settled.status, "budget_reached");
        assert_eq!(settled.status_reason, "token_budget");
        assert_eq!(settled.revision, goal.revision + 1);
        let refused = claim_goal_turn(&pool, &owner(), claim(&goal))
            .await
            .unwrap_err();
        assert_eq!(refused.message(), NOT_ACTIVE);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn turns_reached_moves_status() {
        let pool = connect().await;
        let goal = goal_with(&pool, 0, 1).await;
        claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        bind(&pool, &goal, 1, "run-one").await;
        let mut failed = settle(&goal, 1, "run-one", 0);
        // A failed run without usage still counts (register D2-005).
        failed.outcome = "failed".to_owned();
        failed.usage_source = "none".to_owned();
        let settled = settle_goal_turn(&pool, &owner(), failed)
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(settled.status, "turns_reached");
        assert_eq!(settled.turns_used, 1);
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn cancelled_start_without_run_is_not_a_turn() {
        let pool = connect().await;
        let goal = goal_with(&pool, 0, 1).await;
        claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap();
        let mut cancelled = settle(&goal, 1, "", 0);
        cancelled.outcome = "cancelled".to_owned();
        cancelled.usage_source = "none".to_owned();
        let settled = settle_goal_turn(&pool, &owner(), cancelled)
            .await
            .unwrap()
            .goal
            .unwrap();
        assert_eq!(settled.status, "active");
        assert_eq!(settled.turns_used, 0);
        assert_eq!(
            claim_goal_turn(&pool, &owner(), claim(&goal)).await.unwrap().turn_no,
            2
        );
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL to a disposable Postgres with session-core migrations"]
    async fn paused_goal_refuses_claim() {
        let pool = connect().await;
        let goal = goal_with(&pool, 0, 10).await;
        let mut pause = update(&goal, &["status"]);
        pause.status = "paused".to_owned();
        update_thread_goal(&pool, &owner(), pause).await.unwrap();
        let refused = claim_goal_turn(&pool, &owner(), claim(&goal))
            .await
            .unwrap_err();
        assert_eq!(refused.code(), tonic::Code::FailedPrecondition);
        assert_eq!(refused.message(), NOT_ACTIVE);
        let other = VerifiedIdentity::user_for_test(ORG, "user-goal-other");
        assert!(claim_goal_turn(&pool, &other, claim(&goal)).await.is_err());
    }
}
