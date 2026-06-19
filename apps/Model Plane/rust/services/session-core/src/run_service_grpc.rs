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

use crate::orchestration_grpc::json_to_struct;

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
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }

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

    async fn list_runs(
        &self,
        request: Request<pb::ListRunsRequest>,
    ) -> Result<Response<pb::ListRunsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListRunsResponse>, Status> = async {
            let req = request.into_inner();
            if req.thread_id.is_empty() {
                return Err(Status::invalid_argument("thread_id is required"));
            }

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

            let has_more = rows.len() as i64 > limit;
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
            let req = request.into_inner();
            if req.run_id.is_empty() {
                return Err(Status::invalid_argument("run_id is required"));
            }

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
