use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Utc};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

use vsv2_common::{Source, TenantId};
use vsv2_state::{Checkpoint, RunStatus, SyncRun};

use crate::metadata_store::{
    CreateScheduleInput, CreateSyncOperationError, CreateSyncOperationInput, PersistedSchedule,
    PersistedSyncOperation, RunsQuery, SyncOperationKind,
};
use crate::state::AppState;
use crate::sync_runner::{EndpointSyncFailure, SyncExecutionSummary, SyncRunner};

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct EndpointSummary {
    pub endpoint_name: String,
    pub source: String,
    pub enabled: bool,
    pub modified_field: Option<String>,
    pub sync_pattern: Option<String>,
    pub tier: Option<u8>,
    pub datamart_ready: bool,
}

#[derive(Debug, Serialize)]
pub struct EndpointsResponse {
    pub endpoints: Vec<EndpointSummary>,
}

#[derive(Debug, Deserialize, Default)]
pub struct EndpointsQuery {
    pub tenant: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunsResponse {
    pub runs: Vec<SyncRun>,
}

#[derive(Debug, Serialize)]
pub struct CheckpointsResponse {
    pub checkpoints: Vec<Checkpoint>,
}

#[derive(Debug, Serialize)]
pub struct SchedulesResponse {
    pub schedules: Vec<PersistedSchedule>,
}

#[derive(Debug, Deserialize)]
pub struct BackfillRequest {
    pub tenant: String,
    pub endpoints: Vec<String>,
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct BackfillAcceptedResponse {
    pub backfill_id: Uuid,
    pub tenant: String,
    pub endpoints: Vec<String>,
    pub status: RunStatus,
    pub poll_url: String,
    pub runs_url: String,
}

#[derive(Debug, Serialize)]
pub struct BackfillStatusResponse {
    pub backfill_id: Uuid,
    pub tenant: String,
    pub endpoints: Vec<String>,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub records_synced: u64,
    pub attempted_endpoints: usize,
    pub succeeded_endpoints: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed_endpoints: Vec<FailedEndpointResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub runs_url: String,
}

#[derive(Debug, Serialize)]
pub struct TriggerScheduleAcceptedResponse {
    pub trigger_id: Uuid,
    pub schedule_id: Uuid,
    pub tenant: String,
    pub endpoints: Vec<String>,
    pub status: RunStatus,
    pub poll_url: String,
    pub runs_url: String,
}

#[derive(Debug, Serialize)]
pub struct TriggerScheduleStatusResponse {
    pub trigger_id: Uuid,
    pub schedule_id: Uuid,
    pub tenant: String,
    pub endpoints: Vec<String>,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub records_synced: u64,
    pub attempted_endpoints: usize,
    pub succeeded_endpoints: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed_endpoints: Vec<FailedEndpointResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub runs_url: String,
}

#[derive(Debug, Serialize)]
pub struct FailedEndpointResponse {
    pub endpoint: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct RunsQueryParams {
    pub tenant: String,
    pub endpoint: Option<String>,
    pub limit: Option<u32>,
    pub started_after: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CheckpointsQueryParams {
    pub tenant: String,
    pub endpoint: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SchedulesQueryParams {
    pub tenant: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateScheduleRequest {
    pub tenant: String,
    pub name: String,
    pub cron: String,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TriggerScheduleRequest {
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
}

pub async fn health(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    if state.require_pg_for_health() && state.pg().is_none() {
        tracing::error!("health: postgres pool is required but unavailable");
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    if let Some(pg) = state.pg() {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(pg)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "health: postgres probe failed");
                StatusCode::SERVICE_UNAVAILABLE
            })?;
    }

    Ok(Json(HealthResponse { status: "ok" }))
}

pub async fn list_endpoints(
    State(state): State<AppState>,
    Query(query): Query<EndpointsQuery>,
) -> Result<Json<EndpointsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let tenant = query.tenant.as_deref().map(parse_tenant_id).transpose()?;

    let mut endpoints = Vec::with_capacity(state.registry().iter().count());
    for def in state.registry().iter() {
        let requested_source = match tenant.as_ref() {
            Some(tenant) => {
                if let Some(pg) = state.pg() {
                    if let Some(flag) =
                        vsv2_state::endpoint_flags::get_flag(pg, tenant, &def.endpoint_name)
                            .await
                            .map_err(|error| {
                                internal_error("endpoint flag lookup failed")(anyhow::Error::new(
                                    error,
                                ))
                            })?
                    {
                        flag
                    } else {
                        def.effective_source(tenant)
                    }
                } else {
                    def.effective_source(tenant)
                }
            }
            None => def.source,
        };
        let source = def.live_source(requested_source).map_err(|error| {
            internal_error("endpoint source resolution failed")(anyhow::Error::new(error))
        })?;

        endpoints.push(EndpointSummary {
            endpoint_name: def.endpoint_name.clone(),
            source: source.as_str().to_string(),
            enabled: def.enabled,
            modified_field: def.modified_field.clone(),
            sync_pattern: def.sync_pattern.clone(),
            tier: def.tier,
            datamart_ready: def.datamart.as_ref().is_some_and(|binding| binding.ready),
        });
    }

    endpoints.sort_by(|left, right| left.endpoint_name.cmp(&right.endpoint_name));

    Ok(Json(EndpointsResponse { endpoints }))
}

pub async fn get_backfill(
    State(state): State<AppState>,
    Path(backfill_id): Path<Uuid>,
) -> Result<Json<BackfillStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let metadata_store = metadata_store(&state)?;
    let backfill = metadata_store
        .get_sync_operation(backfill_id)
        .await
        .map_err(internal_error("backfill lookup failed"))?
        .filter(|operation| operation.kind == SyncOperationKind::Backfill)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "backfill_not_found",
                    message: format!("backfill {backfill_id} was not found"),
                }),
            )
        })?;

    Ok(Json(map_backfill_status(&backfill)))
}

pub async fn get_schedule_trigger(
    State(state): State<AppState>,
    Path((schedule_id, trigger_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<TriggerScheduleStatusResponse>, (StatusCode, Json<ErrorResponse>)> {
    let metadata_store = metadata_store(&state)?;
    let operation = metadata_store
        .get_sync_operation(trigger_id)
        .await
        .map_err(internal_error("schedule trigger lookup failed"))?
        .filter(|operation| {
            operation.kind == SyncOperationKind::ScheduleTrigger
                && operation.schedule_id == Some(schedule_id)
        })
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "schedule_trigger_not_found",
                    message: format!(
                        "schedule trigger {trigger_id} for schedule {schedule_id} was not found"
                    ),
                }),
            )
        })?;

    Ok(Json(map_schedule_trigger_status(&operation, schedule_id)))
}

pub async fn list_runs(
    State(state): State<AppState>,
    Query(query): Query<RunsQueryParams>,
) -> Result<Json<RunsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let metadata_store = metadata_store(&state)?;
    let tenant = parse_tenant_id(&query.tenant)?;
    let limit = validate_limit(query.limit)?;

    let runs = metadata_store
        .list_runs(RunsQuery {
            tenant,
            endpoint: query.endpoint,
            limit,
            started_after: query.started_after,
        })
        .await
        .map_err(internal_error("run query failed"))?;

    Ok(Json(RunsResponse { runs }))
}

pub async fn list_checkpoints(
    State(state): State<AppState>,
    Query(query): Query<CheckpointsQueryParams>,
) -> Result<Json<CheckpointsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let metadata_store = metadata_store(&state)?;
    let tenant = parse_tenant_id(&query.tenant)?;
    let source = query.source.as_deref().map(parse_source).transpose()?;

    let checkpoints = metadata_store
        .list_checkpoints(&tenant, query.endpoint.as_deref(), source)
        .await
        .map_err(internal_error("checkpoint query failed"))?;

    Ok(Json(CheckpointsResponse { checkpoints }))
}

pub async fn create_schedule(
    State(state): State<AppState>,
    Json(request): Json<CreateScheduleRequest>,
) -> Result<(StatusCode, Json<PersistedSchedule>), (StatusCode, Json<ErrorResponse>)> {
    let metadata_store = metadata_store(&state)?;
    let tenant = parse_tenant_id(&request.tenant)?;

    if request.name.trim().is_empty() {
        return Err(bad_request("name must not be empty"));
    }

    if request.name.trim().len() > 255 {
        return Err(bad_request("name must not exceed 255 characters"));
    }

    if request.cron.trim().is_empty() {
        return Err(bad_request("cron must not be empty"));
    }

    validate_cron_expression(request.cron.trim())?;

    let endpoints = validate_endpoint_selection(state.registry(), &request.endpoints)?;
    let schedule = metadata_store
        .create_schedule(CreateScheduleInput {
            tenant,
            name: request.name.trim().to_string(),
            cron: request.cron.trim().to_string(),
            endpoints,
        })
        .await
        .map_err(internal_error("schedule creation failed"))?;

    Ok((StatusCode::CREATED, Json(schedule)))
}

pub async fn list_schedules(
    State(state): State<AppState>,
    Query(query): Query<SchedulesQueryParams>,
) -> Result<Json<SchedulesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let metadata_store = metadata_store(&state)?;
    let tenant = query.tenant.as_deref().map(parse_tenant_id).transpose()?;

    let schedules = metadata_store
        .list_schedules(tenant.as_ref())
        .await
        .map_err(internal_error("schedule query failed"))?;

    Ok(Json(SchedulesResponse { schedules }))
}

/// Enqueues a persisted schedule trigger and returns an async poll handle.
pub async fn trigger_schedule(
    State(state): State<AppState>,
    Path(schedule_id): Path<Uuid>,
    Json(request): Json<TriggerScheduleRequest>,
) -> Result<(StatusCode, Json<TriggerScheduleAcceptedResponse>), (StatusCode, Json<ErrorResponse>)>
{
    validate_date_range(request.date_from, request.date_to)?;

    let metadata_store = metadata_store(&state)?;
    let schedule = metadata_store
        .get_schedule(schedule_id)
        .await
        .map_err(internal_error("schedule lookup failed"))?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "schedule_not_found",
                    message: format!("schedule {schedule_id} was not found"),
                }),
            )
        })?;

    if schedule.disabled_at.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(ErrorResponse {
                error: "schedule_disabled",
                message: format!("schedule {schedule_id} is disabled"),
            }),
        ));
    }

    let runner = sync_runner(&state)?;
    let operation = metadata_store
        .create_sync_operation(CreateSyncOperationInput {
            kind: SyncOperationKind::ScheduleTrigger,
            tenant: schedule.tenant.clone(),
            schedule_id: Some(schedule_id),
            endpoints: schedule.endpoints.clone(),
            date_from: request.date_from,
            date_to: request.date_to,
        })
        .await
        .map_err(|error| match error {
            CreateSyncOperationError::Conflict(existing) => (
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "schedule_trigger_already_running",
                    message: format!(
                        "another schedule trigger is already running; inspect GET /v1/schedules/{}/triggers/{}",
                        existing.schedule_id.unwrap_or(schedule_id),
                        existing.operation_id
                    ),
                }),
            ),
            CreateSyncOperationError::Store(error) => internal_error("schedule trigger creation failed")(error),
        })?;

    spawn_sync_operation(
        metadata_store,
        runner,
        operation.operation_id,
        schedule.tenant,
        schedule.endpoints,
        request.date_from,
        request.date_to,
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(map_schedule_trigger_accepted(&operation, schedule_id)),
    ))
}

/// Enqueues a backfill request and returns an async poll handle.
pub async fn backfill(
    State(state): State<AppState>,
    Json(request): Json<BackfillRequest>,
) -> Result<(StatusCode, Json<BackfillAcceptedResponse>), (StatusCode, Json<ErrorResponse>)> {
    let tenant = parse_tenant_id(&request.tenant)?;
    let endpoints = validate_endpoint_selection(state.registry(), &request.endpoints)?;
    validate_date_range(request.date_from, request.date_to)?;

    let metadata_store = metadata_store(&state)?;
    let runner = sync_runner(&state)?;
    let operation = metadata_store
        .create_sync_operation(CreateSyncOperationInput {
            kind: SyncOperationKind::Backfill,
            tenant: tenant.clone(),
            schedule_id: None,
            endpoints: endpoints.clone(),
            date_from: request.date_from,
            date_to: request.date_to,
        })
        .await
        .map_err(|error| match error {
            CreateSyncOperationError::Conflict(existing) => (
                StatusCode::CONFLICT,
                Json(ErrorResponse {
                    error: "backfill_already_running",
                    message: format!(
                        "another backfill is already running; inspect GET /v1/backfills/{}",
                        existing.operation_id
                    ),
                }),
            ),
            CreateSyncOperationError::Store(error) => {
                internal_error("backfill creation failed")(error)
            }
        })?;

    spawn_sync_operation(
        metadata_store,
        runner,
        operation.operation_id,
        tenant,
        endpoints,
        request.date_from,
        request.date_to,
    );

    Ok((
        StatusCode::ACCEPTED,
        Json(map_backfill_accepted(&operation)),
    ))
}

fn bad_request(message: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: "invalid_request",
            message: message.to_string(),
        }),
    )
}

fn parse_tenant_id(raw: &str) -> Result<TenantId, (StatusCode, Json<ErrorResponse>)> {
    if Uuid::parse_str(raw).is_err() {
        return Err(bad_request("tenant must be a valid UUID"));
    }

    Ok(TenantId::new(raw.to_string()))
}

fn validate_limit(limit: Option<u32>) -> Result<u32, (StatusCode, Json<ErrorResponse>)> {
    let limit = limit.unwrap_or(100);
    if limit == 0 {
        return Err(bad_request("limit must be greater than zero"));
    }

    Ok(limit.min(500))
}

fn parse_source(raw: &str) -> Result<Source, (StatusCode, Json<ErrorResponse>)> {
    serde_json::from_str::<Source>(&format!("\"{raw}\""))
        .map_err(|_| bad_request("source must be one of erp_rest, datamart_graphql, expense_api"))
}

fn validate_endpoint_selection(
    registry: &vsv2_orchestrator::EndpointRegistry,
    endpoints: &[String],
) -> Result<Vec<String>, (StatusCode, Json<ErrorResponse>)> {
    if endpoints.is_empty() {
        return Err(bad_request(
            "endpoints must contain at least one endpoint name or 'all'",
        ));
    }

    if endpoints.len() > 1
        && endpoints
            .iter()
            .any(|endpoint| endpoint.eq_ignore_ascii_case("all"))
    {
        return Err(bad_request(
            "'all' must be the only value when used in endpoints",
        ));
    }

    if endpoints.len() == 1 && endpoints[0].eq_ignore_ascii_case("all") {
        return Ok(vec!["all".to_string()]);
    }

    let invalid: Vec<String> = endpoints
        .iter()
        .filter(|endpoint| registry.get(endpoint).is_none())
        .cloned()
        .collect();

    if !invalid.is_empty() {
        return Err(bad_request(&format!(
            "unknown endpoints: {}",
            invalid.join(", ")
        )));
    }

    Ok(endpoints.to_vec())
}

fn validate_date_range(
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    if let (Some(from), Some(to)) = (date_from, date_to) {
        if from > to {
            return Err(bad_request("date_from must be before or equal to date_to"));
        }
    }

    Ok(())
}

fn validate_cron_expression(cron: &str) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    Schedule::from_str(cron)
        .map(|_| ())
        .map_err(|_| bad_request("cron must be a valid cron expression"))
}

fn metadata_store(
    state: &AppState,
) -> Result<
    std::sync::Arc<dyn crate::metadata_store::MetadataStore>,
    (StatusCode, Json<ErrorResponse>),
> {
    state.metadata_store().cloned().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "metadata_store_unavailable",
                message: "metadata queries are not configured for this server".to_string(),
            }),
        )
    })
}

fn sync_runner(
    state: &AppState,
) -> Result<std::sync::Arc<dyn SyncRunner>, (StatusCode, Json<ErrorResponse>)> {
    state.sync_runner().cloned().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "sync_runner_unavailable",
                message: "sync execution is not configured for this server".to_string(),
            }),
        )
    })
}

async fn run_sync_operation(
    runner: std::sync::Arc<dyn SyncRunner>,
    tenant: TenantId,
    endpoints: Vec<String>,
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
) -> anyhow::Result<SyncExecutionSummary> {
    if endpoints.len() == 1 && endpoints[0].eq_ignore_ascii_case("all") {
        runner.sync_all_window(&tenant, date_from, date_to).await
    } else {
        runner
            .sync_names_window(&tenant, &endpoints, date_from, date_to)
            .await
    }
}

fn internal_error(
    message: &'static str,
) -> impl FnOnce(anyhow::Error) -> (StatusCode, Json<ErrorResponse>) {
    move |error| {
        tracing::error!(error = %error, "{message}");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "internal_error",
                message: message.to_string(),
            }),
        )
    }
}

fn classify_sync_summary(summary: &SyncExecutionSummary) -> (StatusCode, &'static str) {
    if summary.failed_endpoints.is_empty() {
        (StatusCode::OK, "succeeded")
    } else if summary.succeeded_endpoints() == 0 {
        (StatusCode::INTERNAL_SERVER_ERROR, "failed")
    } else {
        (StatusCode::MULTI_STATUS, "partial_success")
    }
}

fn map_failed_endpoints(failures: &[EndpointSyncFailure]) -> Vec<FailedEndpointResponse> {
    failures
        .iter()
        .map(|failure| FailedEndpointResponse {
            endpoint: failure.endpoint.clone(),
            error: failure.error.clone(),
        })
        .collect()
}

fn map_backfill_accepted(operation: &PersistedSyncOperation) -> BackfillAcceptedResponse {
    BackfillAcceptedResponse {
        backfill_id: operation.operation_id,
        tenant: operation.tenant.to_string(),
        endpoints: operation.endpoints.clone(),
        status: operation.status,
        poll_url: format!("/v1/backfills/{}", operation.operation_id),
        runs_url: sync_operation_runs_url(operation),
    }
}

fn map_backfill_status(operation: &PersistedSyncOperation) -> BackfillStatusResponse {
    BackfillStatusResponse {
        backfill_id: operation.operation_id,
        tenant: operation.tenant.to_string(),
        endpoints: operation.endpoints.clone(),
        status: operation.status,
        created_at: operation.created_at,
        started_at: operation.started_at,
        finished_at: operation.finished_at,
        records_synced: operation.records_synced,
        attempted_endpoints: operation.attempted_endpoints,
        succeeded_endpoints: operation.succeeded_endpoints,
        failed_endpoints: map_failed_endpoints(&operation.failed_endpoints),
        error: operation.error.clone(),
        runs_url: sync_operation_runs_url(operation),
    }
}

fn map_schedule_trigger_accepted(
    operation: &PersistedSyncOperation,
    schedule_id: Uuid,
) -> TriggerScheduleAcceptedResponse {
    TriggerScheduleAcceptedResponse {
        trigger_id: operation.operation_id,
        schedule_id,
        tenant: operation.tenant.to_string(),
        endpoints: operation.endpoints.clone(),
        status: operation.status,
        poll_url: format!(
            "/v1/schedules/{schedule_id}/triggers/{}",
            operation.operation_id
        ),
        runs_url: sync_operation_runs_url(operation),
    }
}

fn map_schedule_trigger_status(
    operation: &PersistedSyncOperation,
    schedule_id: Uuid,
) -> TriggerScheduleStatusResponse {
    TriggerScheduleStatusResponse {
        trigger_id: operation.operation_id,
        schedule_id,
        tenant: operation.tenant.to_string(),
        endpoints: operation.endpoints.clone(),
        status: operation.status,
        created_at: operation.created_at,
        started_at: operation.started_at,
        finished_at: operation.finished_at,
        records_synced: operation.records_synced,
        attempted_endpoints: operation.attempted_endpoints,
        succeeded_endpoints: operation.succeeded_endpoints,
        failed_endpoints: map_failed_endpoints(&operation.failed_endpoints),
        error: operation.error.clone(),
        runs_url: sync_operation_runs_url(operation),
    }
}

fn sync_operation_runs_url(operation: &PersistedSyncOperation) -> String {
    format!(
        "/v1/runs?tenant={}&started_after={}",
        operation.tenant,
        operation
            .created_at
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    )
}

fn spawn_sync_operation(
    metadata_store: std::sync::Arc<dyn crate::metadata_store::MetadataStore>,
    runner: std::sync::Arc<dyn SyncRunner>,
    operation_id: Uuid,
    tenant: TenantId,
    endpoints: Vec<String>,
    date_from: Option<DateTime<Utc>>,
    date_to: Option<DateTime<Utc>>,
) {
    let attempted_endpoints_on_error =
        if endpoints.len() == 1 && endpoints[0].eq_ignore_ascii_case("all") {
            0
        } else {
            endpoints.len()
        };

    tokio::spawn(async move {
        if let Err(error) = metadata_store
            .mark_sync_operation_running(operation_id)
            .await
        {
            tracing::error!(
                operation_id = %operation_id,
                tenant = %tenant,
                error = %error,
                "failed to mark sync operation running"
            );
        }

        match run_sync_operation(runner, tenant.clone(), endpoints, date_from, date_to).await {
            Ok(summary) => {
                let (_, status) = classify_sync_summary(&summary);
                if let Err(error) = metadata_store
                    .complete_sync_operation(operation_id, &summary)
                    .await
                {
                    tracing::error!(
                        operation_id = %operation_id,
                        tenant = %tenant,
                        error = %error,
                        "failed to persist completed sync operation"
                    );
                    return;
                }
                tracing::info!(
                    operation_id = %operation_id,
                    tenant = %tenant,
                    status,
                    records_synced = summary.records_synced,
                    attempted_endpoints = summary.attempted_endpoints,
                    succeeded_endpoints = summary.succeeded_endpoints(),
                    failed_endpoints = summary.failed_endpoints.len(),
                    "sync operation completed asynchronously"
                );
            }
            Err(error) => {
                tracing::error!(
                    operation_id = %operation_id,
                    tenant = %tenant,
                    error = %error,
                    "sync execution failed asynchronously"
                );
                if let Err(persist_error) = metadata_store
                    .fail_sync_operation(
                        operation_id,
                        &error.to_string(),
                        attempted_endpoints_on_error,
                    )
                    .await
                {
                    tracing::error!(
                        operation_id = %operation_id,
                        tenant = %tenant,
                        error = %persist_error,
                        "failed to persist failed sync operation"
                    );
                }
            }
        }
    });
}
