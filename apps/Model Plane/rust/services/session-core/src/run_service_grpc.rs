//! gRPC handler for the `RunService` — the run read model + cancel path.
//!
//! session-core owns run metadata (the `runs` table + the append-only `events`
//! table). This service exposes that durable record to the runs-history UI:
//!
//! * `GetRun`   — one run's full [`pb::RunDetail`], with `steps_completed`
//!   derived from the run's `STEP_COMPLETED` event ordinals and
//!   `checkpoint_index` from its checkpoint count.
//! * `ListRuns` — a thread's runs, org-scoped, newest-first, with an
//!   `after_run_id` ULID cursor + `limit` and a `has_more` flag.
//! * `CancelRun` — flips a non-terminal run to `cancelled`, org-scoped. The
//!   durable cancel *event* fan-out is driven separately by the gateway's
//!   `RUN_CANCEL_REQUESTED` NATS publish; this is the authoritative status flip
//!   so the read model reflects the cancellation immediately.
//!
//! Token usage (`input_tokens` / `output_tokens`) is not tracked in session-core
//! — it lives on the inference path — so those fields are reported as 0 here.

// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use mp_contracts::model_plane::v1::{
    self as pb,
    run_service_server::{RunService, RunServiceServer},
};
use sqlx::PgPool;
use std::time::Instant;
use tonic::{Request, Response, Status};

use crate::auth::{
    authorize_operation, authorize_owner_row, authorize_run_action_authority_service,
    authorize_scheduled_step_authority_service, authorize_scheduled_step_service, identity,
    OwnerIntent, VerifiedIdentity,
};
use crate::orchestration_grpc::json_to_struct;

async fn authorize_run_owner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    run_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    let owner: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM runs WHERE id = $1")
            .bind(run_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id) = owner.ok_or_else(|| Status::not_found("run not found"))?;
    authorize_owner_row(caller, &org_id, &user_id, intent)
}

async fn authorize_thread_owner(
    pool: &PgPool,
    caller: &VerifiedIdentity,
    thread_id: &str,
    intent: OwnerIntent,
) -> Result<(), Status> {
    let owner: Option<(String, String)> =
        sqlx::query_as("SELECT org_id, user_id FROM threads WHERE id = $1")
            .bind(thread_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    let (org_id, user_id) = owner.ok_or_else(|| Status::not_found("thread not found"))?;
    authorize_owner_row(caller, &org_id, &user_id, intent)
}

/// Hard cap on `ListRuns.limit` so a hostile or buggy caller cannot ask for an
/// unbounded scan. Mirrors the bounded-page convention used across the cores.
const MAX_LIST_LIMIT: i64 = 200;
const DEFAULT_LIST_LIMIT: i64 = 50;

fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
    let status = if is_ok { "ok" } else { "error" };
    metrics::counter!(
        "mp_session_run_service_grpc_requests_total",
        "method" => method,
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "mp_session_run_service_grpc_request_duration_seconds",
        "method" => method,
    )
    .record(started.elapsed().as_secs_f64());
}

/// One `runs` row plus the two derived counters (`steps_completed`,
/// `checkpoint_index`) projected by correlated sub-selects. Timestamps are
/// projected as Unix seconds so they map onto `prost_types::Timestamp` without
/// timezone gymnastics; `metadata` is the raw JSONB column.
#[derive(sqlx::FromRow)]
struct RunRow {
    id: String,
    thread_id: String,
    parent_run_id: Option<String>,
    agent_id: String,
    status: String,
    mode: String,
    goal: String,
    final_output: Option<String>,
    error: Option<String>,
    metadata: serde_json::Value,
    created_at: i64,
    updated_at: i64,
    steps_completed: i64,
    checkpoint_index: i64,
}

/// The deliberately content-free run/thread projection used only by Control's
/// exact action-authorizer before it issues an owner-targeted effect decision.
/// A query must satisfy both immutable run context and the currently stored
/// thread context; a legacy, terminal, service-owned, or partially scoped row
/// simply does not become an authority source.
#[derive(sqlx::FromRow)]
struct RunActionAuthorityRow {
    run_id: String,
    org_id: String,
    subject_id: String,
    thread_id: String,
    run_status: String,
    space_id: Option<String>,
    recipient_audience_ref: Option<String>,
    recipient_audience_revision: Option<i64>,
    recipient_audience_hash: Option<String>,
    privacy_policy_ref: Option<String>,
    thread_resource_authorization_ref: Option<String>,
    authority_revision: Option<i64>,
}

/// Durable scheduled-run bindings projected for Control's per-step authority
/// refresh. The metadata is intentionally the only source for schedule/fire
/// identity: callers cannot turn a valid prepared thread into a different
/// workload by restating those values.
#[derive(sqlx::FromRow)]
struct ScheduledStepAuthorityRow {
    run_id: String,
    thread_id: String,
    org_id: String,
    space_id: String,
    run_status: String,
    metadata: serde_json::Value,
}

fn valid_sha256_digest(value: &str) -> bool {
    let value = value.trim();
    value.len() == "sha256:".len() + 64
        && value.starts_with("sha256:")
        && value["sha256:".len()..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn scheduled_metadata_string(metadata: &serde_json::Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn project_scheduled_step_authority(
    row: ScheduledStepAuthorityRow,
    req: &pb::ResolveScheduledStepAuthorityRequest,
) -> Option<pb::ResolveScheduledStepAuthorityResponse> {
    if is_terminal(&row.run_status)
        || row.space_id.trim().is_empty()
        || row.run_id != req.run_id
        || row.thread_id != req.thread_id
        || row.org_id != req.org_id
        || req.step_id != format!("{}:step:{}", req.run_id, req.step_index)
        || req.idempotency_key.trim().is_empty()
        || !valid_sha256_digest(&req.template_digest)
        || !valid_sha256_digest(&req.policy_digest)
    {
        return None;
    }

    let source = scheduled_metadata_string(&row.metadata, "source")?;
    let schedule_id = scheduled_metadata_string(&row.metadata, "schedule_id")?;
    let fire_key = scheduled_metadata_string(&row.metadata, "fire_key")?;
    let space_id = scheduled_metadata_string(&row.metadata, "space_id")?;
    let subject_id = scheduled_metadata_string(&row.metadata, "subject_id")?;
    let template_digest = scheduled_metadata_string(&row.metadata, "template_digest")?;
    let policy_digest = scheduled_metadata_string(&row.metadata, "policy_digest")?;
    let idempotency_key = scheduled_metadata_string(&row.metadata, "idempotency_key")?;
    if source != "scheduled_run"
        || space_id != row.space_id.trim()
        || schedule_id != req.schedule_id.trim()
        || fire_key != req.fire_key.trim()
        || template_digest != req.template_digest.trim()
        || policy_digest != req.policy_digest.trim()
        || idempotency_key != req.idempotency_key.trim()
    {
        return None;
    }

    Some(pb::ResolveScheduledStepAuthorityResponse {
        resolved: true,
        run_id: row.run_id,
        thread_id: row.thread_id,
        org_id: row.org_id,
        subject_id,
        space_id,
        schedule_id,
        fire_key,
        template_digest,
        policy_digest,
        step_id: req.step_id.clone(),
        step_index: req.step_index,
        idempotency_key,
        run_status: row.run_status,
    })
}

/// Project a query row only when it is safe for Control to use as the durable
/// context source for a future action decision. This function intentionally
/// returns `None` rather than a partial projection: the caller must fail
/// closed and re-resolve current Control facts, not infer missing authority.
fn project_run_action_authority(
    row: RunActionAuthorityRow,
) -> Option<pb::ResolveRunActionAuthorityResponse> {
    if matches!(
        row.run_status.as_str(),
        "completed" | "failed" | "cancelled"
    ) || row.subject_id.starts_with("service:")
    {
        return None;
    }
    let space_id = row.space_id?.trim().to_owned();
    let recipient_audience_ref = row.recipient_audience_ref?.trim().to_owned();
    let recipient_audience_hash = row.recipient_audience_hash?.trim().to_owned();
    let privacy_policy_ref = row.privacy_policy_ref?.trim().to_owned();
    let thread_resource_authorization_ref =
        row.thread_resource_authorization_ref?.trim().to_owned();
    let recipient_audience_revision = u64::try_from(row.recipient_audience_revision?).ok()?;
    let authority_revision = u64::try_from(row.authority_revision?).ok()?;
    if space_id.is_empty()
        || recipient_audience_ref.is_empty()
        || recipient_audience_hash.is_empty()
        || privacy_policy_ref.is_empty()
        || thread_resource_authorization_ref.is_empty()
        || recipient_audience_revision == 0
        || authority_revision == 0
    {
        return None;
    }
    Some(pb::ResolveRunActionAuthorityResponse {
        resolved: true,
        run_id: row.run_id,
        org_id: row.org_id,
        subject_id: row.subject_id,
        thread_id: row.thread_id,
        space_id,
        recipient_audience_ref,
        recipient_audience_revision,
        recipient_audience_hash,
        privacy_policy_ref,
        thread_resource_authorization_ref,
        authority_revision,
        run_status: row.run_status,
    })
}

fn unresolved_run_action_authority() -> pb::ResolveRunActionAuthorityResponse {
    pb::ResolveRunActionAuthorityResponse {
        resolved: false,
        ..Default::default()
    }
}

/// Column projection shared by `GetRun` and `ListRuns`. `steps_completed` is the
/// highest `step_ordinal` recorded for the run (0 when none) and
/// `checkpoint_index` is the run's checkpoint count, both via correlated
/// sub-selects so a single round trip hydrates the whole `RunDetail`.
const RUN_SELECT: &str = "SELECT
        r.id,
        r.thread_id,
        r.parent_run_id,
        r.agent_id,
        r.status,
        r.mode,
        r.goal,
        r.final_output,
        r.error,
        r.metadata,
        extract(epoch from r.created_at)::bigint AS created_at,
        extract(epoch from r.updated_at)::bigint AS updated_at,
        COALESCE((
            SELECT MAX(e.step_ordinal)
            FROM events e
            WHERE e.run_id = r.id AND e.step_ordinal IS NOT NULL
        ), 0)::bigint AS steps_completed,
        COALESCE((
            SELECT COUNT(*)
            FROM checkpoints c
            WHERE c.run_id = r.id
        ), 0)::bigint AS checkpoint_index
     FROM runs r";

#[allow(clippy::cast_sign_loss)]
fn row_to_detail(row: RunRow) -> pb::RunDetail {
    // The derived counters are non-negative by construction (MAX over a
    // non-negative ordinal, COUNT(*)), so the unsigned cast cannot wrap.
    let steps_completed = u32::try_from(row.steps_completed).unwrap_or(u32::MAX);
    let checkpoint_index = u32::try_from(row.checkpoint_index).unwrap_or(u32::MAX);

    pb::RunDetail {
        run_id: row.id,
        thread_id: row.thread_id,
        parent_run_id: row.parent_run_id.unwrap_or_default(),
        agent_id: row.agent_id,
        status: row.status,
        mode: row.mode,
        goal: row.goal,
        final_output: row.final_output.unwrap_or_default(),
        error: row.error.unwrap_or_default(),
        checkpoint_index,
        steps_completed,
        // Token usage is owned by the inference path, not session-core.
        input_tokens: 0,
        output_tokens: 0,
        created_at: Some(prost_types::Timestamp {
            seconds: row.created_at,
            nanos: 0,
        }),
        updated_at: Some(prost_types::Timestamp {
            seconds: row.updated_at,
            nanos: 0,
        }),
        metadata: json_to_struct(&row.metadata),
    }
}

/// Statuses a run can no longer leave. `CancelRun` rejects these so a completed
/// or already-cancelled run is never silently "re-cancelled".
fn is_terminal(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

pub struct RunServiceImpl {
    pool: PgPool,
}

impl RunServiceImpl {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Convenience for `grpc.rs` so the wiring mirrors how the other services in
    /// this crate register their tonic servers.
    #[must_use]
    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub fn into_server(self) -> RunServiceServer<Self> {
        RunServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl RunService for RunServiceImpl {
    async fn get_run(
        &self,
        request: Request<pb::GetRunRequest>,
    ) -> Result<Response<pb::RunDetail>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::RunDetail>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Read).await?;

            let row: Option<RunRow> = sqlx::query_as(&format!("{RUN_SELECT} WHERE r.id = $1"))
                .bind(&req.run_id)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let row =
                row.ok_or_else(|| Status::not_found(format!("run not found: {}", req.run_id)))?;
            Ok(Response::new(row_to_detail(row)))
        }
        .await;
        record_metrics("get_run", started, result.is_ok());
        result
    }

    async fn get_scheduled_step_context(
        &self,
        request: Request<pb::GetScheduledStepContextRequest>,
    ) -> Result<Response<pb::ScheduledStepContext>, Status> {
        let caller = identity(&request)?;
        authorize_operation(&caller, crate::auth::SCHEDULED_STEP_SCOPE)?;
        authorize_scheduled_step_service(&caller)?;
        let req = request.into_inner();
        if req.run_id.trim().is_empty()
            || req.thread_id.trim().is_empty()
            || req.org_id.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "scheduled-step context bindings are required",
            ));
        }
        caller.authorize_org(&req.org_id)?;
        let row: Option<(String, String, String, String, String)> = sqlx::query_as(
            "SELECT r.id, r.thread_id, r.org_id, r.goal, r.status
             FROM runs r
             JOIN threads t ON t.id = r.thread_id
             WHERE r.id = $1 AND r.thread_id = $2 AND r.org_id = $3
               AND r.user_id = $4 AND COALESCE(t.space_id, '') <> ''
               AND COALESCE(r.metadata->>'source', '') = 'scheduled_run'",
        )
        .bind(&req.run_id)
        .bind(&req.thread_id)
        .bind(&req.org_id)
        .bind(crate::auth::system_run_owners()[0])
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
        let Some((run_id, thread_id, org_id, goal, status)) = row else {
            return Err(Status::not_found("scheduled run context not found"));
        };
        Ok(Response::new(pb::ScheduledStepContext {
            run_id,
            thread_id,
            org_id,
            goal,
            status,
        }))
    }

    async fn resolve_scheduled_step_authority(
        &self,
        request: Request<pb::ResolveScheduledStepAuthorityRequest>,
    ) -> Result<Response<pb::ResolveScheduledStepAuthorityResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ResolveScheduledStepAuthorityResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_scheduled_step_authority_service(&caller)?;
            let req = request.into_inner();
            if req.run_id.trim().is_empty()
                || req.thread_id.trim().is_empty()
                || req.org_id.trim().is_empty()
                || req.schedule_id.trim().is_empty()
                || req.fire_key.trim().is_empty()
                || req.step_id.trim().is_empty()
                || req.idempotency_key.trim().is_empty()
                || !valid_sha256_digest(&req.template_digest)
                || !valid_sha256_digest(&req.policy_digest)
            {
                return Err(Status::invalid_argument(
                    "scheduled-step authority bindings are required",
                ));
            }
            if req.step_id != format!("{}:step:{}", req.run_id, req.step_index) {
                return Err(Status::invalid_argument("scheduled step_id is invalid"));
            }
            caller.authorize_org(&req.org_id)?;

            // Only a prepared, service-owned scheduled run can answer this
            // query. The exact metadata comparison below prevents a caller
            // from reusing a valid thread for another schedule or template.
            let row: Option<ScheduledStepAuthorityRow> = sqlx::query_as(
                "SELECT r.id AS run_id, r.thread_id, r.org_id,
                        COALESCE(t.space_id, '') AS space_id,
                        r.status AS run_status, r.metadata
                 FROM runs r
                 JOIN threads t ON t.id = r.thread_id AND t.org_id = r.org_id
                 WHERE r.id = $1 AND r.thread_id = $2 AND r.org_id = $3
                   AND r.user_id = $4
                   AND COALESCE(r.metadata->>'source', '') = 'scheduled_run'",
            )
            .bind(req.run_id.trim())
            .bind(req.thread_id.trim())
            .bind(req.org_id.trim())
            .bind(crate::auth::system_run_owners()[0])
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "scheduled-step authority lookup failed");
                Status::unavailable("scheduled-step authority unavailable")
            })?;

            let response = row
                .and_then(|row| project_scheduled_step_authority(row, &req))
                .unwrap_or_else(|| pb::ResolveScheduledStepAuthorityResponse {
                    resolved: false,
                    ..Default::default()
                });
            Ok(Response::new(response))
        }
        .await;
        record_metrics("resolve_scheduled_step_authority", started, result.is_ok());
        result
    }

    /// Org-scoped listing of runs with no human owner.
    ///
    /// `list_runs` is thread-scoped, and a system run lives in a thread its own
    /// workload owns, so no person's thread listing can reach it — the run would
    /// exist and be fully authorized yet be unreachable by construction. This is
    /// the only way to see them.
    ///
    /// Read-only by design. The owner set is bound from the same constant the
    /// authorization predicate reads (`auth::system_run_owners`), not a `LIKE
    /// 'service:%'` pattern, so a row this query returns is exactly a row
    /// [`authorize_owner_row`] would admit for reading.
    async fn list_system_runs(
        &self,
        request: Request<pb::ListSystemRunsRequest>,
    ) -> Result<Response<pb::ListRunsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListRunsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            // The org boundary is absolute here exactly as everywhere else: a
            // system run is org-readable, never cross-org readable.
            caller.authorize_org(&req.org_id)?;

            let limit = clamp_limit(req.limit);
            let fetch = limit + 1;

            let owners: Vec<String> = crate::auth::system_run_owners()
                .iter()
                .map(|owner| (*owner).to_owned())
                .collect();

            let mut query = format!("{RUN_SELECT} WHERE r.org_id = $1");
            query.push_str(" AND r.user_id = ANY($2)");
            query.push_str(" AND ($3 = '' OR r.status = $3)");
            query.push_str(" AND ($4 = '' OR r.id < $4)");
            query.push_str(" ORDER BY r.id DESC LIMIT $5");

            let rows: Vec<RunRow> = sqlx::query_as(&query)
                .bind(&req.org_id)
                .bind(&owners)
                .bind(&req.status_filter)
                .bind(&req.after_run_id)
                .bind(fetch)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let has_more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
            let runs: Vec<pb::RunDetail> = rows
                .into_iter()
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .map(row_to_detail)
                .collect();
            Ok(Response::new(pb::ListRunsResponse { runs, has_more }))
        }
        .await;
        record_metrics("list_system_runs", started, result.is_ok());
        result
    }

    async fn list_runs(
        &self,
        request: Request<pb::ListRunsRequest>,
    ) -> Result<Response<pb::ListRunsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListRunsResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }
            authorize_thread_owner(&self.pool, &caller, &req.thread_id, OwnerIntent::Read).await?;

            let limit = clamp_limit(req.limit);
            // Fetch one extra row to compute `has_more` without a second query.
            let fetch = limit + 1;

            // Runs are ordered newest-first by ULID id (lexicographic ULID order
            // == creation order). The cursor pages strictly older than the last
            // id the caller saw. Optional status filter; empty = all statuses.
            let mut query = format!("{RUN_SELECT} WHERE r.thread_id = $1");
            query.push_str(" AND ($2 = '' OR r.status = $2)");
            query.push_str(" AND ($3 = '' OR r.id < $3)");
            query.push_str(" ORDER BY r.id DESC LIMIT $4");

            let rows: Vec<RunRow> = sqlx::query_as(&query)
                .bind(&req.thread_id)
                .bind(&req.status_filter)
                .bind(&req.after_run_id)
                .bind(fetch)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let has_more = i64::try_from(rows.len()).unwrap_or(i64::MAX) > limit;
            let runs: Vec<pb::RunDetail> = rows
                .into_iter()
                .take(usize::try_from(limit).unwrap_or(usize::MAX))
                .map(row_to_detail)
                .collect();

            Ok(Response::new(pb::ListRunsResponse { runs, has_more }))
        }
        .await;
        record_metrics("list_runs", started, result.is_ok());
        result
    }

    async fn cancel_run(
        &self,
        request: Request<pb::CancelRunRequest>,
    ) -> Result<Response<pb::CancelRunResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::CancelRunResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:write")?;
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }
            authorize_run_owner(&self.pool, &caller, &req.run_id, OwnerIntent::Mutate).await?;

            // Read the current status to reject a re-cancel of a terminal run
            // before flipping it. A missing run is a 404.
            let current: Option<(String,)> =
                sqlx::query_as("SELECT status FROM runs WHERE id = $1")
                    .bind(&req.run_id)
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|e| Status::internal(e.to_string()))?;

            let (status,) = current
                .ok_or_else(|| Status::not_found(format!("run not found: {}", req.run_id)))?;

            if is_terminal(&status) {
                // Already terminal — not cancellable. Report not-cancelled rather
                // than erroring so a double-cancel is idempotent at the UI.
                return Ok(Response::new(pb::CancelRunResponse { cancelled: false }));
            }

            sqlx::query(
                "UPDATE runs
                 SET status = 'cancelled', ended_at = now(), updated_at = now()
                 WHERE id = $1 AND status NOT IN ('completed', 'failed', 'cancelled')",
            )
            .bind(&req.run_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(pb::CancelRunResponse { cancelled: true }))
        }
        .await;
        record_metrics("cancel_run", started, result.is_ok());
        result
    }

    async fn resolve_run_owner(
        &self,
        request: Request<pb::ResolveRunOwnerRequest>,
    ) -> Result<Response<pb::ResolveRunOwnerResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ResolveRunOwnerResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_operation(&caller, "session:read")?;
            let req = request.into_inner();
            if req.run_id.trim().is_empty()
                || req.org_id.trim().is_empty()
                || req.user_id.trim().is_empty()
            {
                return Err(Status::invalid_argument(
                    "run_id, org_id, and user_id are required",
                ));
            }
            caller.authorize_org(&req.org_id)?;
            if !caller.is_service() {
                caller.authorize_user(&req.user_id)?;
            }
            let (authorized,): (bool,) = sqlx::query_as(
                "SELECT EXISTS(SELECT 1 FROM runs WHERE id = $1 AND org_id = $2 AND user_id = $3)",
            )
            .bind(&req.run_id)
            .bind(&req.org_id)
            .bind(&req.user_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "durable run ownership lookup failed");
                Status::unavailable("run ownership unavailable")
            })?;
            Ok(Response::new(pb::ResolveRunOwnerResponse { authorized }))
        }
        .await;
        record_metrics("resolve_run_owner", started, result.is_ok());
        result
    }

    async fn resolve_run_action_authority(
        &self,
        request: Request<pb::ResolveRunActionAuthorityRequest>,
    ) -> Result<Response<pb::ResolveRunActionAuthorityResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ResolveRunActionAuthorityResponse>, Status> = async {
            let caller = identity(&request)?;
            authorize_run_action_authority_service(&caller)?;
            let req = request.into_inner();
            if req.run_id.trim().is_empty() || req.org_id.trim().is_empty() {
                return Err(Status::invalid_argument("run_id and org_id are required"));
            }
            caller.authorize_org(&req.org_id)?;

            // `runs` inherits its Space bindings from the owner-bound thread in
            // its creation transaction. Requiring the two complete projections
            // to remain equal makes a stale or partial legacy row unavailable
            // instead of letting a caller choose which context to trust.
            let row: Option<RunActionAuthorityRow> = sqlx::query_as(
                "SELECT r.id AS run_id, r.org_id, r.user_id AS subject_id,
                        r.thread_id, r.status AS run_status,
                        r.space_id, r.recipient_audience_ref, r.recipient_audience_revision,
                        r.recipient_audience_hash, r.privacy_policy_ref,
                        r.resource_authorization_ref AS thread_resource_authorization_ref,
                        r.authority_revision
                 FROM runs r
                 JOIN threads t
                   ON t.id = r.thread_id AND t.org_id = r.org_id AND t.user_id = r.user_id
                 WHERE r.id = $1 AND r.org_id = $2
                   AND r.space_id IS NOT DISTINCT FROM t.space_id
                   AND r.recipient_audience_ref IS NOT DISTINCT FROM t.recipient_audience_ref
                   AND r.recipient_audience_revision IS NOT DISTINCT FROM t.recipient_audience_revision
                   AND r.recipient_audience_hash IS NOT DISTINCT FROM t.recipient_audience_hash
                   AND r.privacy_policy_ref IS NOT DISTINCT FROM t.privacy_policy_ref
                   AND r.resource_authorization_ref IS NOT DISTINCT FROM t.resource_authorization_ref
                   AND r.authority_revision IS NOT DISTINCT FROM t.authority_revision",
            )
            .bind(&req.run_id)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|error| {
                tracing::error!(%error, "run action authority lookup failed");
                Status::unavailable("run action authority unavailable")
            })?;

            let response = row
                .and_then(project_run_action_authority)
                .unwrap_or_else(unresolved_run_action_authority);
            Ok(Response::new(response))
        }
        .await;
        record_metrics("resolve_run_action_authority", started, result.is_ok());
        result
    }
}

/// Clamp a wire `limit` (0 = caller left it unset) into `[1, MAX_LIST_LIMIT]`.
#[allow(clippy::cast_possible_wrap)]
fn clamp_limit(raw: u32) -> i64 {
    let requested = if raw == 0 {
        DEFAULT_LIST_LIMIT
    } else {
        i64::from(raw)
    };
    requested.clamp(1, MAX_LIST_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_limit_defaults_when_zero() {
        assert_eq!(clamp_limit(0), DEFAULT_LIST_LIMIT);
    }

    #[test]
    fn clamp_limit_caps_at_max() {
        assert_eq!(clamp_limit(10_000), MAX_LIST_LIMIT);
    }

    #[test]
    fn clamp_limit_passes_through_in_range() {
        assert_eq!(clamp_limit(25), 25);
    }

    #[test]
    fn is_terminal_covers_done_failed_cancelled() {
        assert!(is_terminal("completed"));
        assert!(is_terminal("failed"));
        assert!(is_terminal("cancelled"));
        assert!(!is_terminal("running"));
        assert!(!is_terminal("queued"));
        assert!(!is_terminal("awaiting_approval"));
    }

    fn scheduled_step_request() -> pb::ResolveScheduledStepAuthorityRequest {
        pb::ResolveScheduledStepAuthorityRequest {
            run_id: "run-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            org_id: "org-1".to_owned(),
            schedule_id: "schedule-1".to_owned(),
            fire_key: "fire-1".to_owned(),
            template_digest: format!("sha256:{}", "a".repeat(64)),
            policy_digest: format!("sha256:{}", "b".repeat(64)),
            step_id: "run-1:step:0".to_owned(),
            step_index: 0,
            idempotency_key: "fire-1:step:0".to_owned(),
        }
    }

    fn scheduled_step_row() -> ScheduledStepAuthorityRow {
        ScheduledStepAuthorityRow {
            run_id: "run-1".to_owned(),
            thread_id: "thread-1".to_owned(),
            org_id: "org-1".to_owned(),
            space_id: "space-1".to_owned(),
            run_status: "running".to_owned(),
            metadata: serde_json::json!({
                "source": "scheduled_run",
                "schedule_id": "schedule-1",
                "fire_key": "fire-1",
                "space_id": "space-1",
                "subject_id": "user-1",
                "template_digest": format!("sha256:{}", "a".repeat(64)),
                "policy_digest": format!("sha256:{}", "b".repeat(64)),
                "idempotency_key": "fire-1:step:0",
            }),
        }
    }

    #[test]
    fn scheduled_step_authority_requires_exact_prepared_metadata() {
        let request = scheduled_step_request();
        let response = project_scheduled_step_authority(scheduled_step_row(), &request)
            .expect("exact prepared metadata must resolve");
        assert!(response.resolved);
        assert_eq!(response.subject_id, "user-1");
        assert_eq!(response.space_id, "space-1");
    }

    #[test]
    fn scheduled_step_authority_rejects_retargeted_fire_or_idempotency() {
        let mut request = scheduled_step_request();
        request.fire_key = "other-fire".to_owned();
        assert!(project_scheduled_step_authority(scheduled_step_row(), &request).is_none());

        let mut request = scheduled_step_request();
        request.idempotency_key = "other-idempotency".to_owned();
        assert!(project_scheduled_step_authority(scheduled_step_row(), &request).is_none());
    }

    fn valid_run_action_authority_row() -> RunActionAuthorityRow {
        RunActionAuthorityRow {
            run_id: "run_1".to_owned(),
            org_id: "org_1".to_owned(),
            subject_id: "user_1".to_owned(),
            thread_id: "thread_1".to_owned(),
            run_status: "running".to_owned(),
            space_id: Some("space_1".to_owned()),
            recipient_audience_ref: Some("audience_1".to_owned()),
            recipient_audience_revision: Some(3),
            recipient_audience_hash: Some("sha256:audience".to_owned()),
            privacy_policy_ref: Some("privacy_1".to_owned()),
            thread_resource_authorization_ref: Some("thread-resource_1".to_owned()),
            authority_revision: Some(7),
        }
    }

    #[test]
    fn run_action_authority_projection_is_content_free_and_complete() {
        let response = project_run_action_authority(valid_run_action_authority_row())
            .expect("complete active human run is resolvable");
        assert!(response.resolved);
        assert_eq!(response.run_id, "run_1");
        assert_eq!(response.subject_id, "user_1");
        assert_eq!(
            response.thread_resource_authorization_ref,
            "thread-resource_1"
        );
        assert_eq!(response.authority_revision, 7);
        // The response contract intentionally has no goal, transcript, tool
        // input, output, approval, or bearer field to accidentally retain.
        assert_eq!(response.run_status, "running");
    }

    #[test]
    fn run_action_authority_projection_fails_closed_for_terminal_system_or_partial_rows() {
        let mut terminal = valid_run_action_authority_row();
        terminal.run_status = "completed".to_owned();
        assert!(project_run_action_authority(terminal).is_none());

        let mut system = valid_run_action_authority_row();
        system.subject_id = "service:orchestrator-core".to_owned();
        assert!(project_run_action_authority(system).is_none());

        let mut partial = valid_run_action_authority_row();
        partial.recipient_audience_revision = Some(0);
        assert!(project_run_action_authority(partial).is_none());
    }

    #[test]
    fn row_to_detail_maps_columns_and_derives_counters() {
        let row = RunRow {
            id: "run_01".to_owned(),
            thread_id: "thr_01".to_owned(),
            parent_run_id: None,
            agent_id: "general-v1".to_owned(),
            status: "running".to_owned(),
            mode: "execute".to_owned(),
            goal: "do the thing".to_owned(),
            final_output: None,
            error: None,
            metadata: serde_json::json!({"source": "run_start"}),
            created_at: 1_700_000_000,
            updated_at: 1_700_000_050,
            steps_completed: 3,
            checkpoint_index: 2,
        };
        let detail = row_to_detail(row);
        assert_eq!(detail.run_id, "run_01");
        assert_eq!(detail.thread_id, "thr_01");
        assert_eq!(detail.parent_run_id, "");
        assert_eq!(detail.steps_completed, 3);
        assert_eq!(detail.checkpoint_index, 2);
        assert_eq!(detail.input_tokens, 0);
        assert_eq!(detail.output_tokens, 0);
        assert_eq!(detail.created_at.unwrap().seconds, 1_700_000_000);
        assert!(detail.metadata.is_some());
    }

    #[test]
    fn row_to_detail_flattens_optional_text_to_empty() {
        let row = RunRow {
            id: "run_02".to_owned(),
            thread_id: "thr_02".to_owned(),
            parent_run_id: Some("run_parent".to_owned()),
            agent_id: "general-v1".to_owned(),
            status: "completed".to_owned(),
            mode: "plan".to_owned(),
            goal: "g".to_owned(),
            final_output: Some("the answer".to_owned()),
            error: None,
            metadata: serde_json::Value::Null,
            created_at: 1,
            updated_at: 2,
            steps_completed: 0,
            checkpoint_index: 0,
        };
        let detail = row_to_detail(row);
        assert_eq!(detail.parent_run_id, "run_parent");
        assert_eq!(detail.final_output, "the answer");
        assert_eq!(detail.error, "");
        // Null JSONB metadata maps to no Struct rather than an empty one.
        assert!(detail.metadata.is_none());
    }
}
