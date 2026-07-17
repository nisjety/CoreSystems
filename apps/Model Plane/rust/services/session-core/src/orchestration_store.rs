//! CRUD store for orchestration tables (plans, `plan_steps`, todos, approvals,
//! `subagent_edges`) introduced in migration `0003_orchestration_tables.sql`.
//!
//! All IDs are caller-supplied ULID-prefixed text (`plan_`, `step_`, `todo_`,
//! `appr_`, `run_`). Ordinals on `plan_steps` and `todos` are auto-assigned by
//! BEFORE INSERT triggers when passed as `0` (single-writer-per-parent
//! invariant).

#![allow(dead_code)] // store CRUD layer wired up incrementally as orchestration features land

use anyhow::{bail, Result};
use serde_json::Value as JsonValue;
use sqlx::types::chrono::{DateTime, Utc};
use tracing::info;

use crate::store::Pool;

// ---------------------------------------------------------------------------
// Plans
// ---------------------------------------------------------------------------

/// Insert a new plan row. Returns the row id.
///
/// # Errors
///
/// Returns an error if the insert fails.
#[allow(clippy::too_many_arguments)]
pub async fn create_plan(
    pool: &Pool,
    id: &str,
    thread_id: &str,
    run_id: Option<&str>,
    goal: &str,
    org_id: &str,
    user_id: &str,
    metadata: &JsonValue,
) -> Result<String> {
    sqlx::query(
        "INSERT INTO plans (id, thread_id, run_id, status, goal, org_id, user_id, metadata) \
         VALUES ($1, $2, $3, 'draft', $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(thread_id)
    .bind(run_id)
    .bind(goal)
    .bind(org_id)
    .bind(user_id)
    .bind(metadata)
    .execute(pool)
    .await?;

    info!(plan_id = %id, "plan created");
    Ok(id.to_owned())
}

/// Update plan status and bump `updated_at`.
///
/// # Errors
///
/// Returns an error if the update fails.
pub async fn update_plan_status(pool: &Pool, id: &str, status: &str) -> Result<()> {
    sqlx::query("UPDATE plans SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

/// List plans for a thread, newest first.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_plans_by_thread(pool: &Pool, thread_id: &str) -> Result<Vec<(String, String)>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, status FROM plans WHERE thread_id = $1 ORDER BY created_at DESC, id DESC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Plan steps
// ---------------------------------------------------------------------------

/// Append a plan step. Pass `ordinal = 0` to let the trigger auto-assign.
/// Returns the assigned ordinal.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub async fn append_plan_step(
    pool: &Pool,
    id: &str,
    plan_id: &str,
    kind: &str,
    payload: &JsonValue,
    metadata: &JsonValue,
) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO plan_steps (id, plan_id, ordinal, kind, status, payload, metadata) \
         VALUES ($1, $2, 0, $3, 'pending', $4, $5) RETURNING ordinal",
    )
    .bind(id)
    .bind(plan_id)
    .bind(kind)
    .bind(payload)
    .bind(metadata)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Update plan step status.
///
/// # Errors
///
/// Returns an error if the update fails.
pub async fn update_step_status(pool: &Pool, id: &str, status: &str) -> Result<()> {
    sqlx::query("UPDATE plan_steps SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

/// List plan steps in ordinal order.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_steps_by_plan(
    pool: &Pool,
    plan_id: &str,
) -> Result<Vec<(String, i64, String, String)>> {
    let rows = sqlx::query_as(
        "SELECT id, ordinal, kind, status FROM plan_steps WHERE plan_id = $1 ORDER BY ordinal ASC",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Todos
// ---------------------------------------------------------------------------

/// Append a todo. Pass `ordinal = 0` to let the trigger auto-assign.
/// Returns the assigned ordinal.
///
/// # Errors
///
/// Returns an error if the insert fails.
#[allow(clippy::too_many_arguments)]
pub async fn append_todo(
    pool: &Pool,
    id: &str,
    plan_id: &str,
    thread_id: Option<&str>,
    content: &str,
    priority: &str,
    metadata: &JsonValue,
) -> Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO todos (id, plan_id, thread_id, ordinal, content, status, priority, metadata) \
         VALUES ($1, $2, $3, 0, $4, 'pending', $5, $6) RETURNING ordinal",
    )
    .bind(id)
    .bind(plan_id)
    .bind(thread_id)
    .bind(content)
    .bind(priority)
    .bind(metadata)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Update todo status.
///
/// # Errors
///
/// Returns an error if the update fails.
pub async fn update_todo_status(pool: &Pool, id: &str, status: &str) -> Result<()> {
    sqlx::query("UPDATE todos SET status = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(())
}

/// List todos for a plan in ordinal order.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_todos_by_plan(
    pool: &Pool,
    plan_id: &str,
) -> Result<Vec<(String, i64, String, String)>> {
    let rows = sqlx::query_as(
        "SELECT id, ordinal, status, content FROM todos WHERE plan_id = $1 ORDER BY ordinal ASC",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

/// Request an approval. Honours the partial-unique
/// `(org_id, idempotency_key) WHERE idempotency_key <> ''` constraint via
/// `ON CONFLICT DO NOTHING`. Returns the inserted id, or `None` if the
/// idempotency key collided.
///
/// # Errors
///
/// Returns an error if the insert fails for reasons other than conflict.
#[allow(clippy::too_many_arguments)]
pub async fn request_approval(
    pool: &Pool,
    id: &str,
    run_id: &str,
    plan_id: Option<&str>,
    kind: &str,
    requested_by: &str,
    org_id: &str,
    user_id: &str,
    idempotency_key: &str,
    metadata: &JsonValue,
    expires_at: Option<DateTime<Utc>>,
) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "INSERT INTO approvals (id, run_id, plan_id, kind, status, requested_by, \
         org_id, user_id, idempotency_key, metadata, expires_at) \
         VALUES ($1, $2, $3, $4, 'requested', $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (org_id, user_id, idempotency_key) WHERE idempotency_key <> '' DO NOTHING \
         RETURNING id",
    )
    .bind(id)
    .bind(run_id)
    .bind(plan_id)
    .bind(kind)
    .bind(requested_by)
    .bind(org_id)
    .bind(user_id)
    .bind(idempotency_key)
    .bind(metadata)
    .bind(expires_at)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| r.0))
}

/// Result of a durable approval decision transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalDecisionWrite {
    pub updated: bool,
    pub delivery_id: Option<String>,
}

/// Decide an approval (granted/denied/expired). Stamps `decided_at = now()`.
/// The tenant predicate and requested-state compare-and-set are deliberately
/// part of the write itself so a read/check/write race cannot cross tenants or
/// overwrite an already-recorded operator decision. A successful transition
/// to `granted` inserts its identifier-only delivery record in the same
/// transaction. If that insert fails, dropping the uncommitted transaction
/// rolls the approval update back.
///
/// # Errors
///
/// Returns an error if the update fails.
pub async fn decide_approval(
    pool: &Pool,
    id: &str,
    org_id: &str,
    user_id: Option<&str>,
    status: &str,
    decided_by: &str,
    decision_reason: &str,
) -> Result<ApprovalDecisionWrite> {
    if org_id.trim().is_empty() {
        bail!("org_id is required");
    }
    if decided_by.trim().is_empty() {
        bail!("decided_by is required");
    }
    if !matches!(status, "granted" | "denied" | "timed_out") {
        bail!("unsupported approval decision status");
    }

    let mut transaction = pool.begin().await?;
    let updated = sqlx::query_as::<_, (String,)>(DECIDE_APPROVAL_SQL)
        .bind(id)
        .bind(org_id)
        .bind(status)
        .bind(decided_by)
        .bind(decision_reason)
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await?;
    let updated = updated.is_some();

    let delivery_id = if status == "granted" && updated {
        let delivery_id = format!("approval_delivery_{}", mp_ids::new_ulid());
        let inserted = sqlx::query_as::<_, (String,)>(INSERT_APPROVAL_DELIVERY_OUTBOX_SQL)
            .bind(&delivery_id)
            .bind(id)
            .bind(org_id)
            .bind(user_id)
            .fetch_one(&mut *transaction)
            .await?;
        Some(inserted.0)
    } else if status == "granted" {
        sqlx::query_as::<_, (String,)>(GET_APPROVAL_DELIVERY_SQL)
            .bind(id)
            .bind(org_id)
            .bind(user_id)
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| row.0)
    } else {
        None
    };

    transaction.commit().await?;
    Ok(ApprovalDecisionWrite {
        updated,
        delivery_id,
    })
}

const DECIDE_APPROVAL_SQL: &str =
    "UPDATE approvals SET status = $3, decided_by = $4, decision_reason = $5, \
     decided_at = now() WHERE id = $1 AND org_id = $2 AND status = 'requested' \
     AND ($6::text IS NULL OR user_id = $6) \
     RETURNING id";

const INSERT_APPROVAL_DELIVERY_OUTBOX_SQL: &str = "INSERT INTO approval_delivery_outbox \
         (delivery_id, approval_id, run_id, org_id, user_id) \
     SELECT $1, id, run_id, org_id, user_id FROM approvals \
     WHERE id = $2 AND org_id = $3 AND status = 'granted' \
       AND ($4::text IS NULL OR user_id = $4) \
     RETURNING delivery_id";

const GET_APPROVAL_DELIVERY_SQL: &str = "SELECT approval_delivery_outbox.delivery_id \
     FROM approval_delivery_outbox \
     JOIN approvals ON approvals.id = approval_delivery_outbox.approval_id \
     WHERE approvals.id = $1 AND approvals.org_id = $2 \
       AND ($3::text IS NULL OR approvals.user_id = $3)";

/// List approvals for a run.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_approvals_by_run(
    pool: &Pool,
    run_id: &str,
) -> Result<Vec<(String, String, String)>> {
    let rows = sqlx::query_as(
        "SELECT id, kind, status FROM approvals WHERE run_id = $1 ORDER BY requested_at ASC",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Subagent edges
// ---------------------------------------------------------------------------

/// Attach a child run to a parent. Composite PK `(parent_run_id, child_run_id)`
/// plus the `UNIQUE(child_run_id)` invariant ensure one parent per child.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub async fn attach_subagent(
    pool: &Pool,
    parent_run_id: &str,
    child_run_id: &str,
    role: &str,
    metadata: &JsonValue,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO subagent_edges (parent_run_id, child_run_id, role, status, metadata) \
         VALUES ($1, $2, $3, 'attached', $4)",
    )
    .bind(parent_run_id)
    .bind(child_run_id)
    .bind(role)
    .bind(metadata)
    .execute(pool)
    .await?;
    Ok(())
}

/// Detach a child run from its parent. Sets `status = 'detached'` and stamps
/// `detached_at = now()`.
///
/// # Errors
///
/// Returns an error if the update fails.
pub async fn detach_subagent(pool: &Pool, parent_run_id: &str, child_run_id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE subagent_edges SET status = 'detached', detached_at = now() \
         WHERE parent_run_id = $1 AND child_run_id = $2",
    )
    .bind(parent_run_id)
    .bind(child_run_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// List child runs for a parent, oldest attachment first.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_children(
    pool: &Pool,
    parent_run_id: &str,
) -> Result<Vec<(String, String, String)>> {
    let rows = sqlx::query_as(
        "SELECT child_run_id, role, status FROM subagent_edges \
         WHERE parent_run_id = $1 ORDER BY attached_at ASC",
    )
    .bind(parent_run_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ============================================================================
// Read-side row types and queries (used by the gRPC layer)
// ============================================================================

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct PlanRow {
    pub id: String,
    pub thread_id: String,
    pub run_id: Option<String>,
    pub status: String,
    pub goal: String,
    pub org_id: String,
    pub user_id: String,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct PlanStepRow {
    pub id: String,
    pub plan_id: String,
    pub ordinal: i64,
    pub kind: String,
    pub status: String,
    pub payload: JsonValue,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct TodoRow {
    pub id: String,
    pub plan_id: String,
    pub thread_id: Option<String>,
    pub ordinal: i64,
    pub content: String,
    pub status: String,
    pub priority: String,
    pub metadata: JsonValue,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct ApprovalRow {
    pub id: String,
    pub run_id: String,
    pub plan_id: Option<String>,
    pub kind: String,
    pub status: String,
    pub requested_by: String,
    pub decided_by: String,
    pub decision_reason: String,
    pub org_id: String,
    pub user_id: String,
    pub idempotency_key: String,
    pub metadata: JsonValue,
    pub requested_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct SubagentEdgeRow {
    pub parent_run_id: String,
    pub child_run_id: String,
    pub role: String,
    pub status: String,
    pub metadata: JsonValue,
    pub attached_at: DateTime<Utc>,
    pub detached_at: Option<DateTime<Utc>>,
}

/// Fetch a single plan by id.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_plan(pool: &Pool, id: &str) -> Result<Option<PlanRow>> {
    let row = sqlx::query_as::<_, PlanRow>(
        "SELECT id, thread_id, run_id, status, goal, org_id, user_id, metadata, \
                created_at, updated_at \
         FROM plans WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch a single plan step by id.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_plan_step(pool: &Pool, id: &str) -> Result<Option<PlanStepRow>> {
    let row = sqlx::query_as::<_, PlanStepRow>(
        "SELECT id, plan_id, ordinal, kind, status, payload, metadata, \
                created_at, updated_at \
         FROM plan_steps WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch a single todo by id.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_todo(pool: &Pool, id: &str) -> Result<Option<TodoRow>> {
    let row = sqlx::query_as::<_, TodoRow>(
        "SELECT t.id, t.plan_id, p.thread_id AS thread_id, t.ordinal, t.content, \
                t.status, t.priority, t.metadata, t.created_at, t.updated_at \
         FROM todos t JOIN plans p ON p.id = t.plan_id \
         WHERE t.id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch a single approval by id.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_approval(pool: &Pool, id: &str) -> Result<Option<ApprovalRow>> {
    let row = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, run_id, plan_id, kind, status, requested_by, decided_by, \
                decision_reason, org_id, user_id, idempotency_key, metadata, \
                requested_at, decided_at, expires_at \
         FROM approvals WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch a single approval only when it belongs to `org_id`.
///
/// # Errors
///
/// Returns an error when the tenant is empty or the query fails.
pub async fn get_approval_for_org(
    pool: &Pool,
    id: &str,
    org_id: &str,
    user_id: Option<&str>,
) -> Result<Option<ApprovalRow>> {
    if org_id.trim().is_empty() {
        bail!("org_id is required");
    }

    let row = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, run_id, plan_id, kind, status, requested_by, decided_by, \
                decision_reason, org_id, user_id, idempotency_key, metadata, \
                requested_at, decided_at, expires_at \
         FROM approvals WHERE id = $1 AND org_id = $2 \
         AND ($3::text IS NULL OR user_id = $3)",
    )
    .bind(id)
    .bind(org_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Fetch a single approval by its `(org_id, idempotency_key)` pair. Used to
/// resolve the existing durable record when an idempotent re-request hit the
/// `ON CONFLICT ... DO NOTHING` no-op path and so returned no new id. (D-1)
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_approval_by_idempotency_key(
    pool: &Pool,
    org_id: &str,
    user_id: &str,
    idempotency_key: &str,
) -> Result<Option<ApprovalRow>> {
    let row = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, run_id, plan_id, kind, status, requested_by, decided_by, \
                decision_reason, org_id, user_id, idempotency_key, metadata, \
                requested_at, decided_at, expires_at \
         FROM approvals WHERE org_id = $1 AND user_id = $2 AND idempotency_key = $3",
    )
    .bind(org_id)
    .bind(user_id)
    .bind(idempotency_key)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// List plans associated with a run id.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_plans_by_run(pool: &Pool, run_id: &str) -> Result<Vec<PlanRow>> {
    let rows = sqlx::query_as::<_, PlanRow>(
        "SELECT id, thread_id, run_id, status, goal, org_id, user_id, metadata, \
                created_at, updated_at \
         FROM plans WHERE run_id = $1 ORDER BY created_at ASC",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List all steps for a plan (full row), ordered by ordinal.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_steps_by_plan_full(pool: &Pool, plan_id: &str) -> Result<Vec<PlanStepRow>> {
    let rows = sqlx::query_as::<_, PlanStepRow>(
        "SELECT id, plan_id, ordinal, kind, status, payload, metadata, \
                created_at, updated_at \
         FROM plan_steps WHERE plan_id = $1 ORDER BY ordinal ASC",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List all todos for a plan (full row), ordered by ordinal.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_todos_by_plan_full(pool: &Pool, plan_id: &str) -> Result<Vec<TodoRow>> {
    let rows = sqlx::query_as::<_, TodoRow>(
        "SELECT t.id, t.plan_id, p.thread_id AS thread_id, t.ordinal, t.content, \
                t.status, t.priority, t.metadata, t.created_at, t.updated_at \
         FROM todos t JOIN plans p ON p.id = t.plan_id \
         WHERE t.plan_id = $1 ORDER BY t.ordinal ASC",
    )
    .bind(plan_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List all todos that belong to plans on a given thread.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_todos_by_thread(pool: &Pool, thread_id: &str) -> Result<Vec<TodoRow>> {
    let rows = sqlx::query_as::<_, TodoRow>(
        "SELECT t.id, t.plan_id, p.thread_id AS thread_id, t.ordinal, t.content, \
                t.status, t.priority, t.metadata, t.created_at, t.updated_at \
         FROM todos t JOIN plans p ON p.id = t.plan_id \
         WHERE p.thread_id = $1 ORDER BY t.created_at ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List approvals for a run (full row), most recent first.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_approvals_by_run_full(pool: &Pool, run_id: &str) -> Result<Vec<ApprovalRow>> {
    let rows = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, run_id, plan_id, kind, status, requested_by, decided_by, \
                decision_reason, org_id, user_id, idempotency_key, metadata, \
                requested_at, decided_at, expires_at \
         FROM approvals WHERE run_id = $1 ORDER BY requested_at DESC",
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List approvals for a run, constrained at the database boundary to one
/// tenant.
///
/// # Errors
///
/// Returns an error when the tenant is empty or the query fails.
pub async fn list_approvals_by_run_full_for_org(
    pool: &Pool,
    run_id: &str,
    org_id: &str,
    user_id: Option<&str>,
) -> Result<Vec<ApprovalRow>> {
    if org_id.trim().is_empty() {
        bail!("org_id is required");
    }

    let rows = sqlx::query_as::<_, ApprovalRow>(
        "SELECT id, run_id, plan_id, kind, status, requested_by, decided_by, \
                decision_reason, org_id, user_id, idempotency_key, metadata, \
                requested_at, decided_at, expires_at \
         FROM approvals WHERE run_id = $1 AND org_id = $2 \
         AND ($3::text IS NULL OR user_id = $3) \
         ORDER BY requested_at DESC",
    )
    .bind(run_id)
    .bind(org_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// List all PENDING (status = `'requested'`) approvals for one tenant, oldest
/// first. Empty tenants are rejected rather than interpreted as a wildcard.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_pending_approvals(
    pool: &Pool,
    org_id: &str,
    user_id: Option<&str>,
) -> Result<Vec<ApprovalRow>> {
    if org_id.trim().is_empty() {
        bail!("org_id is required");
    }

    let rows = sqlx::query_as::<_, ApprovalRow>(LIST_PENDING_APPROVALS_SQL)
        .bind(org_id)
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

const LIST_PENDING_APPROVALS_SQL: &str =
    "SELECT id, run_id, plan_id, kind, status, requested_by, decided_by, \
            decision_reason, org_id, user_id, idempotency_key, metadata, \
            requested_at, decided_at, expires_at \
     FROM approvals \
     WHERE status = 'requested' AND org_id = $1 \
     AND ($2::text IS NULL OR user_id = $2) \
     ORDER BY requested_at ASC";

/// Recursively walk subagent lineage for all runs on a thread.
///
/// Seeds with the runs whose `thread_id` matches and traverses outgoing
/// `subagent_edges` to enumerate the full descendant tree.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_lineage_by_thread(pool: &Pool, thread_id: &str) -> Result<Vec<SubagentEdgeRow>> {
    let rows = sqlx::query_as::<_, SubagentEdgeRow>(
        "WITH RECURSIVE lineage AS ( \
             SELECT e.parent_run_id, e.child_run_id, e.role, e.status, \
                    e.metadata, e.attached_at, e.detached_at \
             FROM subagent_edges e \
             JOIN runs r ON r.id = e.parent_run_id \
             WHERE r.thread_id = $1 \
             UNION \
             SELECT e.parent_run_id, e.child_run_id, e.role, e.status, \
                    e.metadata, e.attached_at, e.detached_at \
             FROM subagent_edges e \
             JOIN lineage l ON l.child_run_id = e.parent_run_id \
         ) \
         SELECT parent_run_id, child_run_id, role, status, metadata, \
                attached_at, detached_at \
         FROM lineage ORDER BY attached_at ASC",
    )
    .bind(thread_id)
    .fetch_all(pool)
    .await?;
    info!(thread_id, count = rows.len(), "list_lineage_by_thread");
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[test]
    fn approval_decision_query_is_tenant_scoped_and_compare_and_set() {
        assert!(DECIDE_APPROVAL_SQL.contains("id = $1 AND org_id = $2"));
        assert!(DECIDE_APPROVAL_SQL.contains("($6::text IS NULL OR user_id = $6)"));
        assert!(DECIDE_APPROVAL_SQL.contains("status = 'requested'"));
        assert!(DECIDE_APPROVAL_SQL.contains("RETURNING id"));
    }

    #[test]
    fn pending_approval_query_has_no_empty_org_wildcard() {
        assert!(LIST_PENDING_APPROVALS_SQL.contains("org_id = $1"));
        assert!(LIST_PENDING_APPROVALS_SQL.contains("($2::text IS NULL OR user_id = $2)"));
        assert!(!LIST_PENDING_APPROVALS_SQL.contains("$1 = ''"));
    }

    #[test]
    fn approval_idempotency_is_user_scoped() {
        let migration = include_str!("../migrations/0011_identity_scoping.sql");
        assert!(migration.contains("ON approvals (org_id, user_id, idempotency_key)"));
    }

    #[test]
    fn approval_delivery_outbox_is_one_per_approval_and_content_free() {
        let migration = include_str!("../migrations/0014_approval_delivery_outbox.sql");
        assert!(migration.contains("approval_id TEXT NOT NULL UNIQUE"));
        assert!(
            !migration.contains("'delivered'"),
            "outbox must not represent a completed continuation before a receipt exists"
        );
        assert!(
            !migration.contains("delivered_at"),
            "outbox must not imply a completed continuation before a receipt exists"
        );
        assert!(migration.contains("run_id TEXT NOT NULL"));
        assert!(migration.contains("org_id TEXT NOT NULL"));
        assert!(migration.contains("user_id TEXT NOT NULL"));
        for forbidden in ["payload", "prompt", "reason", "content", "metadata"] {
            assert!(
                !migration.contains(forbidden),
                "approval delivery outbox must not persist {forbidden}"
            );
        }
    }

    #[test]
    fn approval_delivery_outbox_has_a_leased_claim_contract() {
        let migration = include_str!("../migrations/0014_approval_delivery_outbox.sql");
        for required in [
            "lease_owner TEXT",
            "lease_token_hash TEXT",
            "lease_expires_at TIMESTAMPTZ",
            "last_failure_code TEXT",
        ] {
            assert!(
                migration.contains(required),
                "approval delivery outbox must define {required}"
            );
        }
    }

    #[test]
    fn granted_decision_outbox_insert_is_scoped_and_has_no_conflict_bypass() {
        assert!(INSERT_APPROVAL_DELIVERY_OUTBOX_SQL.contains("status = 'granted'"));
        assert!(INSERT_APPROVAL_DELIVERY_OUTBOX_SQL.contains("id = $2 AND org_id = $3"));
        assert!(!INSERT_APPROVAL_DELIVERY_OUTBOX_SQL.contains("ON CONFLICT"));
        assert!(GET_APPROVAL_DELIVERY_SQL.contains("approvals.org_id = $2"));
        assert!(GET_APPROVAL_DELIVERY_SQL.contains("user_id = $3"));
    }

    #[tokio::test]
    async fn approval_store_rejects_empty_scope_before_database_access() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .expect("syntactically valid test URL");

        let org_error =
            decide_approval(&pool, "appr_1", "", Some("user_1"), "granted", "user_1", "")
                .await
                .expect_err("empty org must fail before querying Postgres");
        assert_eq!(org_error.to_string(), "org_id is required");

        let actor_error = decide_approval(
            &pool,
            "appr_1",
            "org_1",
            Some("user_1"),
            "granted",
            "   ",
            "",
        )
        .await
        .expect_err("empty actor must fail before querying Postgres");
        assert_eq!(actor_error.to_string(), "decided_by is required");
    }
}
