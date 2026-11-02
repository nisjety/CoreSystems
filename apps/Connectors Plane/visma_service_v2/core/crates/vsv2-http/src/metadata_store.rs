use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{postgres::PgRow, types::Json, Postgres, QueryBuilder, Row};
use std::fmt;
use uuid::Uuid;

use crate::sync_runner::{EndpointSyncFailure, SyncExecutionSummary};
use vsv2_common::{Source, TenantId};
use vsv2_state::{Checkpoint, CheckpointKey, RunStatus, StateStore, SyncRun};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunsQuery {
    pub tenant: TenantId,
    pub endpoint: Option<String>,
    pub limit: u32,
    pub started_after: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateScheduleInput {
    pub tenant: TenantId,
    pub name: String,
    pub cron: String,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSchedule {
    pub schedule_id: Uuid,
    pub tenant: TenantId,
    pub name: String,
    pub cron: String,
    pub endpoints: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncOperationKind {
    Backfill,
    ScheduleTrigger,
}

impl SyncOperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncOperationKind::Backfill => "backfill",
            SyncOperationKind::ScheduleTrigger => "schedule_trigger",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSyncOperationInput {
    pub kind: SyncOperationKind,
    pub tenant: TenantId,
    pub schedule_id: Option<Uuid>,
    pub endpoints: Vec<String>,
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedSyncOperation {
    pub operation_id: Uuid,
    pub kind: SyncOperationKind,
    pub tenant: TenantId,
    pub schedule_id: Option<Uuid>,
    pub endpoints: Vec<String>,
    pub date_from: Option<DateTime<Utc>>,
    pub date_to: Option<DateTime<Utc>>,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub records_synced: u64,
    pub attempted_endpoints: usize,
    pub succeeded_endpoints: usize,
    pub failed_endpoints: Vec<EndpointSyncFailure>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub enum CreateSyncOperationError {
    Conflict(Box<PersistedSyncOperation>),
    Store(anyhow::Error),
}

impl fmt::Display for CreateSyncOperationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CreateSyncOperationError::Conflict(existing) => write!(
                f,
                "active {} operation already exists: {}",
                existing.kind.as_str(),
                existing.operation_id
            ),
            CreateSyncOperationError::Store(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CreateSyncOperationError {}

#[async_trait]
pub trait MetadataStore: Send + Sync {
    async fn list_runs(&self, query: RunsQuery) -> anyhow::Result<Vec<SyncRun>>;

    async fn list_checkpoints(
        &self,
        tenant: &TenantId,
        endpoint: Option<&str>,
        source: Option<Source>,
    ) -> anyhow::Result<Vec<Checkpoint>>;

    async fn create_schedule(
        &self,
        input: CreateScheduleInput,
    ) -> anyhow::Result<PersistedSchedule>;

    async fn list_schedules(
        &self,
        tenant: Option<&TenantId>,
    ) -> anyhow::Result<Vec<PersistedSchedule>>;

    async fn get_schedule(&self, schedule_id: Uuid) -> anyhow::Result<Option<PersistedSchedule>>;

    async fn create_sync_operation(
        &self,
        input: CreateSyncOperationInput,
    ) -> Result<PersistedSyncOperation, CreateSyncOperationError>;

    async fn get_sync_operation(
        &self,
        operation_id: Uuid,
    ) -> anyhow::Result<Option<PersistedSyncOperation>>;

    async fn mark_sync_operation_running(&self, operation_id: Uuid) -> anyhow::Result<()>;

    async fn complete_sync_operation(
        &self,
        operation_id: Uuid,
        summary: &SyncExecutionSummary,
    ) -> anyhow::Result<()>;

    async fn fail_sync_operation(
        &self,
        operation_id: Uuid,
        error: &str,
        attempted_endpoints: usize,
    ) -> anyhow::Result<()>;

    async fn fail_stale_sync_operations(&self, message: &str) -> anyhow::Result<u64>;
}

#[async_trait]
impl MetadataStore for StateStore {
    async fn list_runs(&self, query: RunsQuery) -> anyhow::Result<Vec<SyncRun>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT run_id, tenant_id, endpoint, source, status, started_at, finished_at, \
             records_in, records_written, error \
             FROM vsv2.sync_run WHERE tenant_id = ",
        );
        builder.push_bind(query.tenant.as_str());

        if let Some(endpoint) = &query.endpoint {
            builder.push(" AND endpoint = ");
            builder.push_bind(endpoint);
        }

        if let Some(started_after) = query.started_after {
            builder.push(" AND started_at >= ");
            builder.push_bind(started_after);
        }

        builder.push(" ORDER BY started_at DESC LIMIT ");
        builder.push_bind(i64::from(query.limit));

        let rows = builder
            .build()
            .fetch_all(&self.pg)
            .await
            .context("list sync runs")?;

        rows.into_iter().map(map_run_row).collect()
    }

    async fn list_checkpoints(
        &self,
        tenant: &TenantId,
        endpoint: Option<&str>,
        source: Option<Source>,
    ) -> anyhow::Result<Vec<Checkpoint>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT tenant_id, endpoint, source, partition_key, last_modified_watermark, \
             last_updated_in_dm, last_id, last_sync_at, records_seen \
             FROM vsv2.sync_checkpoint WHERE tenant_id = ",
        );
        builder.push_bind(tenant.as_str());

        if let Some(endpoint) = endpoint {
            builder.push(" AND endpoint = ");
            builder.push_bind(endpoint);
        }

        if let Some(source) = source {
            builder.push(" AND source = ");
            builder.push_bind(source.as_str());
        }

        builder.push(" ORDER BY endpoint, source, partition_key");

        let rows = builder
            .build()
            .fetch_all(&self.pg)
            .await
            .context("list checkpoints")?;

        rows.into_iter().map(map_checkpoint_row).collect()
    }

    async fn create_schedule(
        &self,
        input: CreateScheduleInput,
    ) -> anyhow::Result<PersistedSchedule> {
        let schedule_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO vsv2.sync_schedule \
             (schedule_id, tenant_id, name, cron_expression, endpoints) \
             VALUES ($1, $2, $3, $4, $5) \
             RETURNING schedule_id, tenant_id, name, cron_expression, endpoints, created_at, disabled_at",
        )
        .bind(schedule_id)
        .bind(input.tenant.as_str())
        .bind(&input.name)
        .bind(&input.cron)
        .bind(Json(&input.endpoints))
        .fetch_one(&self.pg)
        .await
        .context("create schedule")?;

        map_schedule_row(row)
    }

    async fn list_schedules(
        &self,
        tenant: Option<&TenantId>,
    ) -> anyhow::Result<Vec<PersistedSchedule>> {
        let mut builder = QueryBuilder::<Postgres>::new(
            "SELECT schedule_id, tenant_id, name, cron_expression, endpoints, created_at, disabled_at \
             FROM vsv2.sync_schedule",
        );

        if let Some(tenant) = tenant {
            builder.push(" WHERE tenant_id = ");
            builder.push_bind(tenant.as_str());
        }

        builder.push(" ORDER BY created_at DESC");

        let rows = builder
            .build()
            .fetch_all(&self.pg)
            .await
            .context("list schedules")?;

        rows.into_iter().map(map_schedule_row).collect()
    }

    async fn get_schedule(&self, schedule_id: Uuid) -> anyhow::Result<Option<PersistedSchedule>> {
        let row = sqlx::query(
            "SELECT schedule_id, tenant_id, name, cron_expression, endpoints, created_at, disabled_at \
             FROM vsv2.sync_schedule WHERE schedule_id = $1",
        )
        .bind(schedule_id)
        .fetch_optional(&self.pg)
        .await
        .context("get schedule")?;

        row.map(map_schedule_row).transpose()
    }

    async fn create_sync_operation(
        &self,
        input: CreateSyncOperationInput,
    ) -> Result<PersistedSyncOperation, CreateSyncOperationError> {
        let operation_id = Uuid::new_v4();
        let row = sqlx::query(
            "INSERT INTO vsv2.sync_operation \
             (operation_id, kind, tenant_id, schedule_id, endpoints, date_from, date_to, status) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
             RETURNING operation_id, kind, tenant_id, schedule_id, endpoints, date_from, date_to, \
                       status, created_at, started_at, finished_at, records_synced, \
                       attempted_endpoints, succeeded_endpoints, failed_endpoints, error",
        )
        .bind(operation_id)
        .bind(input.kind.as_str())
        .bind(input.tenant.as_str())
        .bind(input.schedule_id)
        .bind(Json(&input.endpoints))
        .bind(input.date_from)
        .bind(input.date_to)
        .bind(RunStatus::Pending.as_str())
        .fetch_one(&self.pg)
        .await;

        match row {
            Ok(row) => map_sync_operation_row(row).map_err(CreateSyncOperationError::Store),
            Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("23505") => {
                let existing = find_active_sync_operation(self, input.kind, input.schedule_id)
                    .await
                    .map_err(CreateSyncOperationError::Store)?
                    .ok_or_else(|| {
                        CreateSyncOperationError::Store(anyhow::anyhow!(
                            "sync operation conflict without active row"
                        ))
                    })?;
                Err(CreateSyncOperationError::Conflict(Box::new(existing)))
            }
            Err(error) => Err(CreateSyncOperationError::Store(
                anyhow::Error::new(error).context("create sync operation"),
            )),
        }
    }

    async fn get_sync_operation(
        &self,
        operation_id: Uuid,
    ) -> anyhow::Result<Option<PersistedSyncOperation>> {
        let row = sqlx::query(
            "SELECT operation_id, kind, tenant_id, schedule_id, endpoints, date_from, date_to, \
                    status, created_at, started_at, finished_at, records_synced, \
                    attempted_endpoints, succeeded_endpoints, failed_endpoints, error \
             FROM vsv2.sync_operation WHERE operation_id = $1",
        )
        .bind(operation_id)
        .fetch_optional(&self.pg)
        .await
        .context("get sync operation")?;

        row.map(map_sync_operation_row).transpose()
    }

    async fn mark_sync_operation_running(&self, operation_id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE vsv2.sync_operation \
             SET status = $2, started_at = COALESCE(started_at, NOW()), error = NULL \
             WHERE operation_id = $1",
        )
        .bind(operation_id)
        .bind(RunStatus::Running.as_str())
        .execute(&self.pg)
        .await
        .context("mark sync operation running")?;

        Ok(())
    }

    async fn complete_sync_operation(
        &self,
        operation_id: Uuid,
        summary: &SyncExecutionSummary,
    ) -> anyhow::Result<()> {
        let status = if summary.failed_endpoints.is_empty() {
            RunStatus::Succeeded
        } else if summary.succeeded_endpoints() == 0 {
            RunStatus::Failed
        } else {
            RunStatus::PartialSuccess
        };

        sqlx::query(
            "UPDATE vsv2.sync_operation \
             SET status = $2, finished_at = NOW(), records_synced = $3, attempted_endpoints = $4, \
                 succeeded_endpoints = $5, failed_endpoints = $6, error = NULL \
             WHERE operation_id = $1",
        )
        .bind(operation_id)
        .bind(status.as_str())
        .bind(i64::try_from(summary.records_synced).context("records_synced exceeds i64")?)
        .bind(
            i32::try_from(summary.attempted_endpoints)
                .context("attempted_endpoints exceeds i32")?,
        )
        .bind(
            i32::try_from(summary.succeeded_endpoints())
                .context("succeeded_endpoints exceeds i32")?,
        )
        .bind(Json(&summary.failed_endpoints))
        .execute(&self.pg)
        .await
        .context("complete sync operation")?;

        Ok(())
    }

    async fn fail_sync_operation(
        &self,
        operation_id: Uuid,
        error: &str,
        attempted_endpoints: usize,
    ) -> anyhow::Result<()> {
        let failed_endpoints: Vec<EndpointSyncFailure> = Vec::new();
        sqlx::query(
            "UPDATE vsv2.sync_operation \
             SET status = $2, finished_at = NOW(), records_synced = 0, attempted_endpoints = $3, \
                 succeeded_endpoints = 0, failed_endpoints = $4, error = $5 \
             WHERE operation_id = $1",
        )
        .bind(operation_id)
        .bind(RunStatus::Failed.as_str())
        .bind(i32::try_from(attempted_endpoints).context("attempted_endpoints exceeds i32")?)
        .bind(Json(&failed_endpoints))
        .bind(vsv2_common::text::storable_text(error))
        .execute(&self.pg)
        .await
        .context("fail sync operation")?;

        Ok(())
    }

    async fn fail_stale_sync_operations(&self, message: &str) -> anyhow::Result<u64> {
        let result = sqlx::query(
            "UPDATE vsv2.sync_operation \
             SET status = $1, finished_at = NOW(), error = $2 \
             WHERE status IN ('pending', 'running')",
        )
        .bind(RunStatus::Failed.as_str())
        .bind(message)
        .execute(&self.pg)
        .await
        .context("fail stale sync operations")?;

        Ok(result.rows_affected())
    }
}

fn map_run_row(row: PgRow) -> anyhow::Result<SyncRun> {
    Ok(SyncRun {
        run_id: row.try_get("run_id")?,
        tenant: TenantId::new(row.try_get::<String, _>("tenant_id")?),
        endpoint: row.try_get("endpoint")?,
        source: parse_source(&row.try_get::<String, _>("source")?)?,
        status: parse_run_status(&row.try_get::<String, _>("status")?)?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        records_in: row.try_get("records_in")?,
        records_written: row.try_get("records_written")?,
        error: row.try_get("error")?,
    })
}

fn map_checkpoint_row(row: PgRow) -> anyhow::Result<Checkpoint> {
    let tenant = TenantId::new(row.try_get::<String, _>("tenant_id")?);
    let endpoint = row.try_get::<String, _>("endpoint")?;
    let source = parse_source(&row.try_get::<String, _>("source")?)?;
    let partition_key = row.try_get::<String, _>("partition_key")?;

    Ok(Checkpoint {
        key: CheckpointKey::for_partition(tenant, endpoint, source, partition_key),
        last_modified_watermark: row.try_get("last_modified_watermark")?,
        last_updated_in_dm: row.try_get("last_updated_in_dm")?,
        last_id: row.try_get("last_id")?,
        last_sync_at: row.try_get("last_sync_at")?,
        records_seen: row.try_get("records_seen")?,
    })
}

fn map_schedule_row(row: PgRow) -> anyhow::Result<PersistedSchedule> {
    Ok(PersistedSchedule {
        schedule_id: row.try_get("schedule_id")?,
        tenant: TenantId::new(row.try_get::<String, _>("tenant_id")?),
        name: row.try_get("name")?,
        cron: row.try_get("cron_expression")?,
        endpoints: row.try_get::<Json<Vec<String>>, _>("endpoints")?.0,
        created_at: row.try_get("created_at")?,
        disabled_at: row.try_get("disabled_at")?,
    })
}

async fn find_active_sync_operation(
    store: &StateStore,
    kind: SyncOperationKind,
    schedule_id: Option<Uuid>,
) -> anyhow::Result<Option<PersistedSyncOperation>> {
    let row = match kind {
        SyncOperationKind::Backfill => sqlx::query(
            "SELECT operation_id, kind, tenant_id, schedule_id, endpoints, date_from, date_to, \
                        status, created_at, started_at, finished_at, records_synced, \
                        attempted_endpoints, succeeded_endpoints, failed_endpoints, error \
                 FROM vsv2.sync_operation \
                 WHERE kind = 'backfill' AND status IN ('pending', 'running') \
                 ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_optional(&store.pg)
        .await
        .context("find active backfill operation")?,
        SyncOperationKind::ScheduleTrigger => sqlx::query(
            "SELECT operation_id, kind, tenant_id, schedule_id, endpoints, date_from, date_to, \
                        status, created_at, started_at, finished_at, records_synced, \
                        attempted_endpoints, succeeded_endpoints, failed_endpoints, error \
                 FROM vsv2.sync_operation \
                 WHERE kind = 'schedule_trigger' AND schedule_id = $1 \
                   AND status IN ('pending', 'running') \
                 ORDER BY created_at DESC LIMIT 1",
        )
        .bind(schedule_id)
        .fetch_optional(&store.pg)
        .await
        .context("find active schedule trigger operation")?,
    };

    row.map(map_sync_operation_row).transpose()
}

fn map_sync_operation_row(row: PgRow) -> anyhow::Result<PersistedSyncOperation> {
    let records_synced = row.try_get::<i64, _>("records_synced")?;
    let attempted_endpoints = row.try_get::<i32, _>("attempted_endpoints")?;
    let succeeded_endpoints = row.try_get::<i32, _>("succeeded_endpoints")?;

    Ok(PersistedSyncOperation {
        operation_id: row.try_get("operation_id")?,
        kind: parse_sync_operation_kind(&row.try_get::<String, _>("kind")?)?,
        tenant: TenantId::new(row.try_get::<String, _>("tenant_id")?),
        schedule_id: row.try_get("schedule_id")?,
        endpoints: row.try_get::<Json<Vec<String>>, _>("endpoints")?.0,
        date_from: row.try_get("date_from")?,
        date_to: row.try_get("date_to")?,
        status: parse_run_status(&row.try_get::<String, _>("status")?)?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        records_synced: u64::try_from(records_synced)
            .context("records_synced must be non-negative")?,
        attempted_endpoints: usize::try_from(attempted_endpoints)
            .context("attempted_endpoints must be non-negative")?,
        succeeded_endpoints: usize::try_from(succeeded_endpoints)
            .context("succeeded_endpoints must be non-negative")?,
        failed_endpoints: row
            .try_get::<Json<Vec<EndpointSyncFailure>>, _>("failed_endpoints")?
            .0,
        error: row.try_get("error")?,
    })
}

fn parse_source(value: &str) -> anyhow::Result<Source> {
    serde_json::from_value::<Source>(Value::String(value.to_string()))
        .with_context(|| format!("invalid source '{value}'"))
}

fn parse_run_status(value: &str) -> anyhow::Result<RunStatus> {
    serde_json::from_value::<RunStatus>(Value::String(value.to_string()))
        .with_context(|| format!("invalid run status '{value}'"))
}

fn parse_sync_operation_kind(value: &str) -> anyhow::Result<SyncOperationKind> {
    match value {
        "backfill" => Ok(SyncOperationKind::Backfill),
        "schedule_trigger" => Ok(SyncOperationKind::ScheduleTrigger),
        _ => anyhow::bail!("invalid sync operation kind '{value}'"),
    }
}
