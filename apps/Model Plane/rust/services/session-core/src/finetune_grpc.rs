//! gRPC handler for the `FinetuneJobs` service.
//!
//! Wave 7 v1 — owns the fine-tuning job state-machine persistence in
//! session-core's existing Postgres. Provider integration (Azure `OpenAI` Files
//! and Fine-tuning Jobs APIs) lives in `model-gateway::finetune_routes` so the
//! gateway holds the Azure credentials, runs the HTTP calls, and writes the
//! resulting `azure_file_id` / `azure_job_id` back through this service.
//!
//! State machine (column `status`):
//!   `queued` → `running` → (`succeeded` | `failed` | `cancelled`)
//!
//! Transitions are not enforced at the DB level — the gateway is the only
//! caller and writes the valid set. `succeeded` requires the gateway to fill
//! in `fine_tuned_model` + `deployment_name` before flipping status.

// tonic::Status is the unavoidable large Err for gRPC; boxing breaks the service-trait contract.
#![allow(clippy::result_large_err)]

use chrono::{DateTime, Utc};
use mp_contracts::model_plane::v1::{
    self as pb,
    finetune_jobs_server::{FinetuneJobs, FinetuneJobsServer},
};
use sqlx::PgPool;
use std::time::Instant;
use tonic::{Request, Response, Status};
use tracing::warn;

const LIST_LIMIT_DEFAULT: i32 = 50;
const LIST_LIMIT_MAX: i32 = 200;
/// Max rows the polling worker may pull per tick. Higher than the user-facing
/// `ListJobs` cap because the poller is an internal caller and we want it to
/// drain in-flight backlog quickly when the gateway has been offline.
const LIST_ACTIVE_LIMIT_MAX: i32 = 500;

/// Convert `chrono::DateTime<Utc>` to a proto timestamp without panicking.
fn to_pb_ts(t: DateTime<Utc>) -> prost_types::Timestamp {
    let secs = t.timestamp();
    let nanos = i32::try_from(t.timestamp_subsec_nanos()).unwrap_or(0);
    prost_types::Timestamp {
        seconds: secs,
        nanos,
    }
}

fn record_metrics(method: &'static str, started: Instant, is_ok: bool) {
    let status = if is_ok { "ok" } else { "error" };
    metrics::counter!(
        "mp_session_finetune_grpc_requests_total",
        "method" => method,
        "status" => status,
    )
    .increment(1);
    metrics::histogram!(
        "mp_session_finetune_grpc_request_duration_seconds",
        "method" => method,
    )
    .record(started.elapsed().as_secs_f64());
}

/// Row shape returned by all read queries. Mirrors the proto field set so the
/// mapping in `row_to_pb` is a pure rename.
#[derive(sqlx::FromRow)]
struct FinetuneJobRow {
    job_id: String,
    org_id: String,
    agent_id: String,
    base_model: String,
    azure_file_id: String,
    azure_job_id: String,
    fine_tuned_model: String,
    deployment_name: String,
    deployment_tier: String,
    status: String,
    error_message: String,
    hyperparameters_json: serde_json::Value,
    training_example_count: i32,
    estimated_cost_usd: f64,
    actual_cost_usd: f64,
    created_by: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

fn row_to_pb(row: FinetuneJobRow) -> pb::FinetuneJob {
    let hyperparameters_json = if row.hyperparameters_json.is_null()
        || row.hyperparameters_json == serde_json::json!({})
    {
        String::new()
    } else {
        row.hyperparameters_json.to_string()
    };
    pb::FinetuneJob {
        job_id: row.job_id,
        org_id: row.org_id,
        agent_id: row.agent_id,
        base_model: row.base_model,
        azure_file_id: row.azure_file_id,
        azure_job_id: row.azure_job_id,
        fine_tuned_model: row.fine_tuned_model,
        deployment_name: row.deployment_name,
        deployment_tier: row.deployment_tier,
        status: row.status,
        error_message: row.error_message,
        created_at: Some(to_pb_ts(row.created_at)),
        updated_at: Some(to_pb_ts(row.updated_at)),
        completed_at: row.completed_at.map(to_pb_ts),
        hyperparameters_json,
        training_example_count: row.training_example_count,
        estimated_cost_usd: row.estimated_cost_usd,
        actual_cost_usd: row.actual_cost_usd,
        created_by: row.created_by,
    }
}

fn parse_hyperparameters(raw: &str) -> Result<serde_json::Value, Status> {
    if raw.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(raw).map_err(|e| {
        Status::invalid_argument(format!("hyperparameters_json must be valid JSON: {e}"))
    })
}

fn validate_status(status: &str) -> Result<(), Status> {
    match status {
        "queued" | "running" | "succeeded" | "failed" | "cancelled" => Ok(()),
        other => Err(Status::invalid_argument(format!(
            "invalid status: {other} (expected queued|running|succeeded|failed|cancelled)"
        ))),
    }
}

pub struct FinetuneJobsService {
    pool: PgPool,
}

impl FinetuneJobsService {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Convenience for `main.rs` so the wiring there mirrors how
    /// `SessionService` and `OrchestrationGrpc` register their tonic servers.
    #[must_use]
    #[allow(dead_code)] // direct constructor retained for isolated service tests
    pub fn into_server(self) -> FinetuneJobsServer<Self> {
        FinetuneJobsServer::new(self)
    }
}

#[tonic::async_trait]
impl FinetuneJobs for FinetuneJobsService {
    async fn create_job(
        &self,
        request: Request<pb::CreateFinetuneJobRequest>,
    ) -> Result<Response<pb::FinetuneJob>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::FinetuneJob>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            let req = request.into_inner();
            // Cross-tenant guard: a user token may only act on its own org;
            // trusted service workers (allowAnyOrg) may span orgs.
            if !caller.is_service() {
                caller.authorize_org(&req.org_id)?;
            }
            if req.job_id.is_empty() {
                return Err(Status::invalid_argument("job_id required"));
            }
            if req.org_id.is_empty() {
                return Err(Status::invalid_argument("org_id required"));
            }
            if req.agent_id.is_empty() {
                return Err(Status::invalid_argument("agent_id required"));
            }
            if req.base_model.is_empty() {
                return Err(Status::invalid_argument("base_model required"));
            }
            let hyperparameters = parse_hyperparameters(&req.hyperparameters_json)?;

            let row: FinetuneJobRow = sqlx::query_as(
                "INSERT INTO finetune_jobs (
                    job_id, org_id, agent_id, base_model,
                    azure_file_id, azure_job_id,
                    hyperparameters_json, training_example_count,
                    estimated_cost_usd, created_by, status
                ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'queued')
                ON CONFLICT (job_id) DO NOTHING
                RETURNING
                    job_id, org_id, agent_id, base_model,
                    azure_file_id, azure_job_id, fine_tuned_model, deployment_name,
                    deployment_tier, status, error_message,
                    hyperparameters_json, training_example_count,
                    estimated_cost_usd::float8 AS estimated_cost_usd,
                    actual_cost_usd::float8 AS actual_cost_usd,
                    created_by, created_at, updated_at, completed_at",
            )
            .bind(&req.job_id)
            .bind(&req.org_id)
            .bind(&req.agent_id)
            .bind(&req.base_model)
            .bind(&req.azure_file_id)
            .bind(&req.azure_job_id)
            .bind(&hyperparameters)
            .bind(req.training_example_count)
            .bind(req.estimated_cost_usd)
            .bind(&req.created_by)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::already_exists(format!("job_id {} exists", req.job_id)))?;

            Ok(Response::new(row_to_pb(row)))
        }
        .await;
        record_metrics("create_job", started, result.is_ok());
        result
    }

    async fn get_job(
        &self,
        request: Request<pb::GetFinetuneJobRequest>,
    ) -> Result<Response<pb::FinetuneJob>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::FinetuneJob>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            let req = request.into_inner();
            if !caller.is_service() {
                caller.authorize_org(&req.org_id)?;
            }
            if req.job_id.is_empty() || req.org_id.is_empty() {
                return Err(Status::invalid_argument("job_id and org_id required"));
            }
            let row: FinetuneJobRow = sqlx::query_as(
                "SELECT
                    job_id, org_id, agent_id, base_model,
                    azure_file_id, azure_job_id, fine_tuned_model, deployment_name,
                    deployment_tier, status, error_message,
                    hyperparameters_json, training_example_count,
                    estimated_cost_usd::float8 AS estimated_cost_usd,
                    actual_cost_usd::float8 AS actual_cost_usd,
                    created_by, created_at, updated_at, completed_at
                 FROM finetune_jobs
                 WHERE job_id = $1 AND org_id = $2",
            )
            .bind(&req.job_id)
            .bind(&req.org_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found(format!("finetune job {} not found", req.job_id)))?;

            Ok(Response::new(row_to_pb(row)))
        }
        .await;
        record_metrics("get_job", started, result.is_ok());
        result
    }

    async fn list_jobs(
        &self,
        request: Request<pb::ListFinetuneJobsRequest>,
    ) -> Result<Response<pb::ListFinetuneJobsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListFinetuneJobsResponse>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            let req = request.into_inner();
            if !caller.is_service() {
                caller.authorize_org(&req.org_id)?;
            }
            if req.org_id.is_empty() {
                return Err(Status::invalid_argument("org_id required"));
            }
            let limit = if req.limit <= 0 {
                LIST_LIMIT_DEFAULT
            } else {
                req.limit.min(LIST_LIMIT_MAX)
            };
            let offset = req.offset.max(0);

            // Total count first — small index-only scan; cheap.
            let total: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM finetune_jobs
                 WHERE org_id = $1
                   AND ($2 = '' OR agent_id = $2)
                   AND ($3 = '' OR status = $3)",
            )
            .bind(&req.org_id)
            .bind(&req.agent_id)
            .bind(&req.status)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            let rows: Vec<FinetuneJobRow> = sqlx::query_as(
                "SELECT
                    job_id, org_id, agent_id, base_model,
                    azure_file_id, azure_job_id, fine_tuned_model, deployment_name,
                    deployment_tier, status, error_message,
                    hyperparameters_json, training_example_count,
                    estimated_cost_usd::float8 AS estimated_cost_usd,
                    actual_cost_usd::float8 AS actual_cost_usd,
                    created_by, created_at, updated_at, completed_at
                 FROM finetune_jobs
                 WHERE org_id = $1
                   AND ($2 = '' OR agent_id = $2)
                   AND ($3 = '' OR status = $3)
                 ORDER BY created_at DESC
                 LIMIT $4 OFFSET $5",
            )
            .bind(&req.org_id)
            .bind(&req.agent_id)
            .bind(&req.status)
            .bind(i64::from(limit))
            .bind(i64::from(offset))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(pb::ListFinetuneJobsResponse {
                jobs: rows.into_iter().map(row_to_pb).collect(),
                total: i32::try_from(total).unwrap_or(i32::MAX),
            }))
        }
        .await;
        record_metrics("list_jobs", started, result.is_ok());
        result
    }

    async fn list_active_jobs(
        &self,
        request: Request<pb::ListActiveFinetuneJobsRequest>,
    ) -> Result<Response<pb::ListActiveFinetuneJobsResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::ListActiveFinetuneJobsResponse>, Status> = async {
            // Cross-org worker scan (no org filter) — restricted to service
            // principals (the polling worker); never a user token.
            let caller = crate::auth::identity(&request)?;
            if !caller.is_service() {
                return Err(Status::permission_denied(
                    "list_active_jobs is restricted to the finetune polling worker",
                ));
            }
            let req = request.into_inner();
            let limit = if req.limit <= 0 {
                LIST_ACTIVE_LIMIT_MAX
            } else {
                req.limit.min(LIST_ACTIVE_LIMIT_MAX)
            };

            // Polling worker scope: only return rows that are both in flight
            // AND have a provider job id. Rows without azure_job_id are
            // operator-tracked placeholders that don't get polled.
            let rows: Vec<FinetuneJobRow> = sqlx::query_as(
                "SELECT
                    job_id, org_id, agent_id, base_model,
                    azure_file_id, azure_job_id, fine_tuned_model, deployment_name,
                    deployment_tier, status, error_message,
                    hyperparameters_json, training_example_count,
                    estimated_cost_usd::float8 AS estimated_cost_usd,
                    actual_cost_usd::float8 AS actual_cost_usd,
                    created_by, created_at, updated_at, completed_at
                 FROM finetune_jobs
                 WHERE status IN ('queued', 'running')
                   AND azure_job_id <> ''
                 ORDER BY updated_at ASC
                 LIMIT $1",
            )
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(pb::ListActiveFinetuneJobsResponse {
                jobs: rows.into_iter().map(row_to_pb).collect(),
            }))
        }
        .await;
        record_metrics("list_active_jobs", started, result.is_ok());
        result
    }

    async fn get_org_monthly_spend(
        &self,
        request: Request<pb::GetOrgMonthlySpendRequest>,
    ) -> Result<Response<pb::GetOrgMonthlySpendResponse>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::GetOrgMonthlySpendResponse>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            let req = request.into_inner();
            if !caller.is_service() {
                caller.authorize_org(&req.org_id)?;
            }
            if req.org_id.is_empty() {
                return Err(Status::invalid_argument("org_id required"));
            }

            // Conservative ceiling: estimate for in-flight rows (we don't
            // know the final cost yet) + actual for terminal rows (the
            // polling worker has written the real number). Same calendar
            // month UTC so monthly budgets reset cleanly at month boundary.
            let row: (Option<f64>, i64) = sqlx::query_as(
                "SELECT
                    COALESCE(SUM(
                        CASE
                            WHEN status IN ('queued', 'running')
                                THEN estimated_cost_usd
                            ELSE actual_cost_usd
                        END
                    )::float8, 0.0) AS total_usd,
                    COUNT(*) AS job_count
                 FROM finetune_jobs
                 WHERE org_id = $1
                   AND created_at >= date_trunc('month', now() AT TIME ZONE 'utc')",
            )
            .bind(&req.org_id)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

            Ok(Response::new(pb::GetOrgMonthlySpendResponse {
                total_usd: row.0.unwrap_or(0.0),
                job_count: i32::try_from(row.1).unwrap_or(i32::MAX),
            }))
        }
        .await;
        record_metrics("get_org_monthly_spend", started, result.is_ok());
        result
    }

    async fn update_job_status(
        &self,
        request: Request<pb::UpdateFinetuneJobStatusRequest>,
    ) -> Result<Response<pb::FinetuneJob>, Status> {
        let started = Instant::now();
        let result: Result<Response<pb::FinetuneJob>, Status> = async {
            let caller = crate::auth::identity(&request)?;
            let req = request.into_inner();
            if !caller.is_service() {
                caller.authorize_org(&req.org_id)?;
            }
            if req.job_id.is_empty() || req.org_id.is_empty() {
                return Err(Status::invalid_argument("job_id and org_id required"));
            }
            validate_status(&req.status)?;

            // TOCTOU guard: a `cancelled` transition must never overwrite a
            // row that already reached a terminal state (`succeeded`,
            // `failed`, or `cancelled`). Poller transitions (running →
            // succeeded/failed) remain unrestricted so the worker can still
            // record outcomes. If the guard fires, RETURNING is empty and
            // the handler surfaces 404, which the cancel route maps to 409.
            let row: FinetuneJobRow = sqlx::query_as(
                "UPDATE finetune_jobs SET
                    status = $3,
                    error_message = $4,
                    fine_tuned_model = CASE WHEN $5 <> '' THEN $5 ELSE fine_tuned_model END,
                    deployment_name = CASE WHEN $6 <> '' THEN $6 ELSE deployment_name END,
                    actual_cost_usd = CASE WHEN $7 > 0 THEN $7 ELSE actual_cost_usd END,
                    completed_at = CASE WHEN $8 THEN now() ELSE completed_at END,
                    deployment_tier = CASE WHEN $9 <> '' THEN $9 ELSE deployment_tier END,
                    updated_at = now()
                 WHERE job_id = $1 AND org_id = $2
                   AND ($3 <> 'cancelled' OR status NOT IN ('succeeded','failed','cancelled'))
                 RETURNING
                    job_id, org_id, agent_id, base_model,
                    azure_file_id, azure_job_id, fine_tuned_model, deployment_name,
                    deployment_tier, status, error_message,
                    hyperparameters_json, training_example_count,
                    estimated_cost_usd::float8 AS estimated_cost_usd,
                    actual_cost_usd::float8 AS actual_cost_usd,
                    created_by, created_at, updated_at, completed_at",
            )
            .bind(&req.job_id)
            .bind(&req.org_id)
            .bind(&req.status)
            .bind(&req.error_message)
            .bind(&req.fine_tuned_model)
            .bind(&req.deployment_name)
            .bind(req.actual_cost_usd)
            .bind(req.set_completed)
            .bind(&req.deployment_tier)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                warn!(error = %e, "finetune update_job_status failed");
                Status::internal(e.to_string())
            })?
            .ok_or_else(|| Status::not_found(format!("finetune job {} not found", req.job_id)))?;

            Ok(Response::new(row_to_pb(row)))
        }
        .await;
        record_metrics("update_job_status", started, result.is_ok());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_status_accepts_lifecycle_states() {
        for s in ["queued", "running", "succeeded", "failed", "cancelled"] {
            assert!(validate_status(s).is_ok(), "{s} should be accepted");
        }
    }

    #[test]
    fn validate_status_rejects_unknown() {
        let err = validate_status("paused").unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("paused"));
    }

    #[test]
    fn parse_hyperparameters_empty_returns_empty_object() {
        let v = parse_hyperparameters("").unwrap();
        assert_eq!(v, serde_json::json!({}));
    }

    #[test]
    fn parse_hyperparameters_passes_through_valid_json() {
        let v = parse_hyperparameters(r#"{"epochs":3}"#).unwrap();
        assert_eq!(v, serde_json::json!({"epochs": 3}));
    }

    #[test]
    fn parse_hyperparameters_rejects_invalid_json() {
        let err = parse_hyperparameters("not json").unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn to_pb_ts_round_trip_seconds() {
        let t = DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap();
        let ts = to_pb_ts(t);
        assert_eq!(ts.seconds, 1_700_000_000);
        assert_eq!(ts.nanos, 0);
    }
}
