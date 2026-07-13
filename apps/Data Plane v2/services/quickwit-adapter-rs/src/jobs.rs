use std::time::Duration;

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Requested,
    Approved,
    Running,
    Succeeded,
    Failed,
}

impl JobStatus {
    fn parse(value: &str) -> Result<Self, JobError> {
        match value {
            "requested" => Ok(Self::Requested),
            "approved" => Ok(Self::Approved),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(JobError::Storage),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i16)]
pub enum RebuildStage {
    EmptyScopePreflight = 1,
    KnowledgeUnits = 2,
    WikiVersions = 3,
    SourceObjects = 4,
    RetrievalLogs = 5,
    WikiSourceLogs = 6,
}

impl RebuildStage {
    pub const ALL: &'static [Self] = &[
        Self::EmptyScopePreflight,
        Self::KnowledgeUnits,
        Self::WikiVersions,
        Self::SourceObjects,
        Self::RetrievalLogs,
        Self::WikiSourceLogs,
    ];

    fn from_checkpoint(value: i16) -> Result<Option<Self>, JobError> {
        match value {
            0 => Ok(None),
            1 => Ok(Some(Self::EmptyScopePreflight)),
            2 => Ok(Some(Self::KnowledgeUnits)),
            3 => Ok(Some(Self::WikiVersions)),
            4 => Ok(Some(Self::SourceObjects)),
            5 => Ok(Some(Self::RetrievalLogs)),
            6 => Ok(Some(Self::WikiSourceLogs)),
            _ => Err(JobError::Storage),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub org_id: Option<String>,
    pub global: bool,
    pub clear: bool,
    pub requested_by: String,
    pub approval_id: String,
    pub idempotency_key: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminJob {
    pub job_id: String,
    pub org_id: Option<String>,
    pub global: bool,
    pub clear: bool,
    pub requested_by: String,
    pub approved_by: Option<String>,
    pub approval_id: String,
    pub idempotency_key: String,
    pub reason: String,
    pub status: JobStatus,
    pub checkpoint: Option<RebuildStage>,
    pub batch_stage: Option<RebuildStage>,
    pub batch_cursor: Option<String>,
    pub lease_owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub event_id: String,
    pub job_id: String,
    pub actor: String,
    pub action: String,
    pub org_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Submission {
    Created(AdminJob),
    Existing(AdminJob),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobError {
    NotFound,
    InvalidState,
    IdempotencyConflict,
    RateLimited,
    SeparationOfDuties,
    UnsafeClear,
    Storage,
    Execution,
}

#[async_trait]
pub trait AdminJobStore: Send + Sync {
    async fn submit(&self, spec: JobSpec) -> Result<Submission, JobError>;
    async fn get(&self, job_id: &str) -> Result<Option<AdminJob>, JobError>;
    async fn approve(&self, job_id: &str, actor: &str) -> Result<AdminJob, JobError>;
    async fn claim_next(&self, runner_id: &str) -> Result<Option<AdminJob>, JobError>;
    async fn renew_lease(&self, job_id: &str, runner_id: &str) -> Result<(), JobError>;
    async fn record_checkpoint(
        &self,
        job_id: &str,
        runner_id: &str,
        stage: RebuildStage,
    ) -> Result<(), JobError>;
    async fn complete(&self, job_id: &str, runner_id: &str) -> Result<(), JobError>;
    async fn fail(&self, job_id: &str, runner_id: &str, reason: &str) -> Result<(), JobError>;
}

#[async_trait]
pub trait JobExecutor: Send + Sync {
    async fn execute_stage(&self, job: &AdminJob, stage: RebuildStage) -> Result<(), String>;
}

pub async fn execute_claimed_job<S: AdminJobStore + ?Sized, E: JobExecutor + ?Sized>(
    store: &S,
    executor: &E,
    runner_id: &str,
    job: AdminJob,
) -> Result<(), JobError> {
    let completed_checkpoint = job.checkpoint;
    for stage in RebuildStage::ALL.iter().copied() {
        if completed_checkpoint.is_some_and(|checkpoint| stage <= checkpoint) {
            continue;
        }
        let execution = executor.execute_stage(&job, stage);
        tokio::pin!(execution);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        let result = loop {
            tokio::select! {
                result = &mut execution => break result,
                _ = heartbeat.tick() => store.renew_lease(&job.job_id, runner_id).await?,
            }
        };
        if let Err(error) = result {
            store.fail(&job.job_id, runner_id, &error).await?;
            return Err(JobError::Execution);
        }
        store
            .record_checkpoint(&job.job_id, runner_id, stage)
            .await?;
    }
    store.complete(&job.job_id, runner_id).await
}

struct InMemoryState {
    jobs: HashMap<String, AdminJob>,
    order: Vec<String>,
    submitted_at: HashMap<String, std::time::Instant>,
    leases: HashMap<String, (String, std::time::Instant)>,
    audit: Vec<AuditEntry>,
}

#[doc(hidden)]
pub struct InMemoryAdminJobStore {
    state: Mutex<InMemoryState>,
    rate_window: Duration,
    max_concurrency: usize,
    lease_duration: Duration,
}

impl InMemoryAdminJobStore {
    pub fn new(rate_window: Duration, max_concurrency: usize) -> Self {
        Self {
            state: Mutex::new(InMemoryState {
                jobs: HashMap::new(),
                order: Vec::new(),
                submitted_at: HashMap::new(),
                leases: HashMap::new(),
                audit: Vec::new(),
            }),
            rate_window,
            max_concurrency: max_concurrency.max(1),
            lease_duration: Duration::from_secs(60),
        }
    }

    pub async fn audit_entries(&self) -> Vec<AuditEntry> {
        self.state.lock().expect("admin job state").audit.clone()
    }

    pub async fn expire_lease_for_test(&self, job_id: &str) {
        if let Some((runner, lease)) = self
            .state
            .lock()
            .expect("admin job state")
            .leases
            .get_mut(job_id)
        {
            *runner = String::new();
            *lease = std::time::Instant::now() - Duration::from_secs(1);
        }
    }
}

fn append_audit(state: &mut InMemoryState, job: &AdminJob, actor: &str, action: &str) {
    state.audit.push(AuditEntry {
        event_id: Uuid::new_v4().to_string(),
        job_id: job.job_id.clone(),
        actor: actor.to_string(),
        action: action.to_string(),
        org_id: job.org_id.clone(),
        occurred_at: Utc::now(),
    });
}

#[async_trait]
impl AdminJobStore for InMemoryAdminJobStore {
    async fn submit(&self, spec: JobSpec) -> Result<Submission, JobError> {
        if spec.clear {
            return Err(JobError::UnsafeClear);
        }
        let mut state = self.state.lock().map_err(|_| JobError::Storage)?;
        if let Some(existing) = state
            .jobs
            .values()
            .find(|job| job.idempotency_key == spec.idempotency_key && job.org_id == spec.org_id)
            .cloned()
        {
            let identical = existing.global == spec.global
                && existing.clear == spec.clear
                && existing.requested_by == spec.requested_by
                && existing.approval_id == spec.approval_id
                && existing.reason == spec.reason;
            return if identical {
                Ok(Submission::Existing(existing))
            } else {
                Err(JobError::IdempotencyConflict)
            };
        }
        let rate_key = spec.org_id.clone().unwrap_or_else(|| "__global__".into());
        if state
            .submitted_at
            .get(&rate_key)
            .is_some_and(|last| last.elapsed() < self.rate_window)
        {
            return Err(JobError::RateLimited);
        }
        let job = AdminJob {
            job_id: Uuid::new_v4().to_string(),
            org_id: spec.org_id,
            global: spec.global,
            clear: spec.clear,
            requested_by: spec.requested_by,
            approved_by: None,
            approval_id: spec.approval_id,
            idempotency_key: spec.idempotency_key,
            reason: spec.reason,
            status: JobStatus::Requested,
            checkpoint: None,
            batch_stage: None,
            batch_cursor: None,
            lease_owner: None,
        };
        state
            .submitted_at
            .insert(rate_key, std::time::Instant::now());
        state.order.push(job.job_id.clone());
        state.jobs.insert(job.job_id.clone(), job.clone());
        append_audit(&mut state, &job, &job.requested_by, "requested");
        Ok(Submission::Created(job))
    }

    async fn get(&self, job_id: &str) -> Result<Option<AdminJob>, JobError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| JobError::Storage)?
            .jobs
            .get(job_id)
            .cloned())
    }

    async fn approve(&self, job_id: &str, actor: &str) -> Result<AdminJob, JobError> {
        let mut state = self.state.lock().map_err(|_| JobError::Storage)?;
        let job = state.jobs.get(job_id).cloned().ok_or(JobError::NotFound)?;
        if job.requested_by == actor {
            return Err(JobError::SeparationOfDuties);
        }
        if job.status != JobStatus::Requested {
            return Err(JobError::InvalidState);
        }
        let mut approved = job;
        approved.approved_by = Some(actor.to_string());
        approved.status = JobStatus::Approved;
        state.jobs.insert(job_id.to_string(), approved.clone());
        append_audit(&mut state, &approved, actor, "approved");
        Ok(approved)
    }

    async fn claim_next(&self, runner_id: &str) -> Result<Option<AdminJob>, JobError> {
        let mut state = self.state.lock().map_err(|_| JobError::Storage)?;
        let now = std::time::Instant::now();
        state.leases.retain(|_, (_, until)| *until > now);
        if state.leases.len() >= self.max_concurrency {
            return Ok(None);
        }
        let job_id = state.order.iter().find(|job_id| {
            state.jobs.get(*job_id).is_some_and(|job| {
                job.status == JobStatus::Approved
                    || (job.status == JobStatus::Running && !state.leases.contains_key(*job_id))
            })
        });
        let Some(job_id) = job_id.cloned() else {
            return Ok(None);
        };
        let claimed = {
            let job = state.jobs.get_mut(&job_id).ok_or(JobError::Storage)?;
            job.status = JobStatus::Running;
            job.lease_owner = Some(runner_id.to_string());
            job.clone()
        };
        state
            .leases
            .insert(job_id, (runner_id.to_string(), now + self.lease_duration));
        append_audit(&mut state, &claimed, runner_id, "claimed");
        Ok(Some(claimed))
    }

    async fn record_checkpoint(
        &self,
        job_id: &str,
        runner_id: &str,
        stage: RebuildStage,
    ) -> Result<(), JobError> {
        let mut state = self.state.lock().map_err(|_| JobError::Storage)?;
        require_lease(&state, job_id, runner_id)?;
        let checkpointed = {
            let job = state.jobs.get_mut(job_id).ok_or(JobError::NotFound)?;
            if job.status != JobStatus::Running
                || job.checkpoint.is_some_and(|checkpoint| stage <= checkpoint)
            {
                return Err(JobError::InvalidState);
            }
            job.checkpoint = Some(stage);
            job.batch_stage = None;
            job.batch_cursor = None;
            job.clone()
        };
        append_audit(&mut state, &checkpointed, runner_id, "checkpointed");
        Ok(())
    }

    async fn renew_lease(&self, job_id: &str, runner_id: &str) -> Result<(), JobError> {
        let mut state = self.state.lock().map_err(|_| JobError::Storage)?;
        require_lease(&state, job_id, runner_id)?;
        state.leases.insert(
            job_id.to_string(),
            (
                runner_id.to_string(),
                std::time::Instant::now() + self.lease_duration,
            ),
        );
        Ok(())
    }

    async fn complete(&self, job_id: &str, runner_id: &str) -> Result<(), JobError> {
        let mut state = self.state.lock().map_err(|_| JobError::Storage)?;
        require_lease(&state, job_id, runner_id)?;
        let completed = {
            let job = state.jobs.get_mut(job_id).ok_or(JobError::NotFound)?;
            if job.checkpoint != Some(RebuildStage::WikiSourceLogs) {
                return Err(JobError::InvalidState);
            }
            job.status = JobStatus::Succeeded;
            job.lease_owner = None;
            job.clone()
        };
        state.leases.remove(job_id);
        append_audit(&mut state, &completed, runner_id, "succeeded");
        Ok(())
    }

    async fn fail(&self, job_id: &str, runner_id: &str, reason: &str) -> Result<(), JobError> {
        let mut state = self.state.lock().map_err(|_| JobError::Storage)?;
        require_lease(&state, job_id, runner_id)?;
        let failed = {
            let job = state.jobs.get_mut(job_id).ok_or(JobError::NotFound)?;
            job.status = JobStatus::Failed;
            job.lease_owner = None;
            job.clone()
        };
        state.leases.remove(job_id);
        let _ = reason;
        append_audit(&mut state, &failed, runner_id, "failed");
        Ok(())
    }
}

fn require_lease(state: &InMemoryState, job_id: &str, runner_id: &str) -> Result<(), JobError> {
    if state
        .leases
        .get(job_id)
        .is_some_and(|(owner, until)| owner == runner_id && *until > std::time::Instant::now())
    {
        Ok(())
    } else {
        Err(JobError::InvalidState)
    }
}

#[derive(Clone)]
pub struct PgAdminJobStore {
    pool: PgPool,
    rate_window_seconds: i64,
    lease_seconds: i64,
}

impl PgAdminJobStore {
    pub fn new(pool: PgPool, rate_window: Duration, lease_duration: Duration) -> Self {
        Self {
            pool,
            rate_window_seconds: rate_window.as_secs().min(i64::MAX as u64) as i64,
            lease_seconds: lease_duration.as_secs().min(i64::MAX as u64) as i64,
        }
    }
}

#[derive(FromRow)]
struct JobRow {
    job_id: String,
    org_id: Option<String>,
    global: bool,
    clear: bool,
    requested_by: String,
    approved_by: Option<String>,
    approval_id: String,
    idempotency_key: String,
    reason: String,
    status: String,
    checkpoint: i16,
    batch_stage: i16,
    batch_cursor: Option<String>,
    lease_owner: Option<String>,
}

impl TryFrom<JobRow> for AdminJob {
    type Error = JobError;

    fn try_from(row: JobRow) -> Result<Self, Self::Error> {
        Ok(Self {
            job_id: row.job_id,
            org_id: row.org_id,
            global: row.global,
            clear: row.clear,
            requested_by: row.requested_by,
            approved_by: row.approved_by,
            approval_id: row.approval_id,
            idempotency_key: row.idempotency_key,
            reason: row.reason,
            status: JobStatus::parse(&row.status)?,
            checkpoint: RebuildStage::from_checkpoint(row.checkpoint)?,
            batch_stage: RebuildStage::from_checkpoint(row.batch_stage)?,
            batch_cursor: row.batch_cursor,
            lease_owner: row.lease_owner,
        })
    }
}

const JOB_COLUMNS: &str = "job_id, org_id, global, clear, requested_by, approved_by, approval_id, idempotency_key, reason, status, checkpoint, batch_stage, batch_cursor, lease_owner";
const JOB_COLUMNS_J: &str = "j.job_id, j.org_id, j.global, j.clear, j.requested_by, j.approved_by, j.approval_id, j.idempotency_key, j.reason, j.status, j.checkpoint, j.batch_stage, j.batch_cursor, j.lease_owner";

#[async_trait]
impl AdminJobStore for PgAdminJobStore {
    async fn submit(&self, spec: JobSpec) -> Result<Submission, JobError> {
        if spec.clear {
            return Err(JobError::UnsafeClear);
        }
        let mut tx = self.pool.begin().await.map_err(|_| JobError::Storage)?;
        let scope_key = spec.org_id.as_deref().unwrap_or("__global__");
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(scope_key)
            .execute(&mut *tx)
            .await
            .map_err(|_| JobError::Storage)?;
        let existing_sql = format!(
            "SELECT {JOB_COLUMNS} FROM quickwit_admin_jobs WHERE scope_key = COALESCE($1, '__global__') AND idempotency_key = $2"
        );
        if let Some(row) = sqlx::query_as::<_, JobRow>(&existing_sql)
            .bind(&spec.org_id)
            .bind(&spec.idempotency_key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| JobError::Storage)?
        {
            let existing = AdminJob::try_from(row)?;
            let identical = existing.global == spec.global
                && existing.clear == spec.clear
                && existing.requested_by == spec.requested_by
                && existing.approval_id == spec.approval_id
                && existing.reason == spec.reason;
            return if identical {
                Ok(Submission::Existing(existing))
            } else {
                Err(JobError::IdempotencyConflict)
            };
        }
        let rate_limited: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM quickwit_admin_jobs WHERE scope_key = COALESCE($1, '__global__') AND created_at > NOW() - make_interval(secs => $2))",
        )
        .bind(&spec.org_id)
        .bind(self.rate_window_seconds as f64)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| JobError::Storage)?;
        if rate_limited {
            return Err(JobError::RateLimited);
        }
        let job_id = Uuid::new_v4().to_string();
        let insert_sql = format!(
            "INSERT INTO quickwit_admin_jobs (job_id, org_id, global, clear, requested_by, approval_id, idempotency_key, reason) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING {JOB_COLUMNS}"
        );
        let row = sqlx::query_as::<_, JobRow>(&insert_sql)
            .bind(&job_id)
            .bind(&spec.org_id)
            .bind(spec.global)
            .bind(spec.clear)
            .bind(&spec.requested_by)
            .bind(&spec.approval_id)
            .bind(&spec.idempotency_key)
            .bind(&spec.reason)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| JobError::Storage)?;
        append_pg_audit(
            &mut tx,
            &job_id,
            spec.org_id.as_deref(),
            &spec.requested_by,
            "requested",
        )
        .await?;
        tx.commit().await.map_err(|_| JobError::Storage)?;
        Ok(Submission::Created(AdminJob::try_from(row)?))
    }

    async fn get(&self, job_id: &str) -> Result<Option<AdminJob>, JobError> {
        let sql = format!("SELECT {JOB_COLUMNS} FROM quickwit_admin_jobs WHERE job_id = $1");
        sqlx::query_as::<_, JobRow>(&sql)
            .bind(job_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(|_| JobError::Storage)?
            .map(AdminJob::try_from)
            .transpose()
    }

    async fn approve(&self, job_id: &str, actor: &str) -> Result<AdminJob, JobError> {
        let mut tx = self.pool.begin().await.map_err(|_| JobError::Storage)?;
        let requester: Option<String> = sqlx::query_scalar(
            "SELECT requested_by FROM quickwit_admin_jobs WHERE job_id=$1 AND status='requested' FOR UPDATE",
        )
        .bind(job_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| JobError::Storage)?;
        let requester = requester.ok_or(JobError::InvalidState)?;
        if requester == actor {
            return Err(JobError::SeparationOfDuties);
        }
        let sql = format!(
            "UPDATE quickwit_admin_jobs SET approved_by=$2,status='approved',updated_at=NOW() WHERE job_id=$1 AND status='requested' AND requested_by<>$2 RETURNING {JOB_COLUMNS}"
        );
        let row = sqlx::query_as::<_, JobRow>(&sql)
            .bind(job_id)
            .bind(actor)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| JobError::Storage)?
            .ok_or(JobError::InvalidState)?;
        append_pg_audit(&mut tx, job_id, row.org_id.as_deref(), actor, "approved").await?;
        tx.commit().await.map_err(|_| JobError::Storage)?;
        AdminJob::try_from(row)
    }

    async fn claim_next(&self, runner_id: &str) -> Result<Option<AdminJob>, JobError> {
        let mut tx = self.pool.begin().await.map_err(|_| JobError::Storage)?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('quickwit-admin-claim'))")
            .execute(&mut *tx)
            .await
            .map_err(|_| JobError::Storage)?;
        let sql = format!(
            "WITH candidate AS (SELECT job_id FROM quickwit_admin_jobs WHERE (status='approved' OR (status='running' AND lease_until<NOW())) AND NOT EXISTS (SELECT 1 FROM quickwit_admin_jobs active WHERE active.status='running' AND active.lease_until>=NOW()) ORDER BY created_at FOR UPDATE SKIP LOCKED LIMIT 1) UPDATE quickwit_admin_jobs j SET status='running',lease_owner=$1,lease_until=NOW()+make_interval(secs=>$2),attempts=attempts+1,updated_at=NOW() FROM candidate c WHERE j.job_id=c.job_id RETURNING {JOB_COLUMNS_J}"
        );
        let row = sqlx::query_as::<_, JobRow>(&sql)
            .bind(runner_id)
            .bind(self.lease_seconds as f64)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| JobError::Storage)?;
        let Some(row) = row else {
            tx.commit().await.map_err(|_| JobError::Storage)?;
            return Ok(None);
        };
        append_pg_audit(
            &mut tx,
            &row.job_id,
            row.org_id.as_deref(),
            runner_id,
            "claimed",
        )
        .await?;
        tx.commit().await.map_err(|_| JobError::Storage)?;
        Ok(Some(AdminJob::try_from(row)?))
    }

    async fn record_checkpoint(
        &self,
        job_id: &str,
        runner_id: &str,
        stage: RebuildStage,
    ) -> Result<(), JobError> {
        let mut tx = self.pool.begin().await.map_err(|_| JobError::Storage)?;
        let row: Option<Option<String>> = sqlx::query_scalar(
            "UPDATE quickwit_admin_jobs SET checkpoint=$3,batch_stage=0,batch_cursor=NULL,lease_until=NOW()+make_interval(secs=>$4),updated_at=NOW() WHERE job_id=$1 AND status='running' AND lease_owner=$2 AND lease_until>NOW() AND checkpoint<$3 RETURNING org_id",
        )
        .bind(job_id).bind(runner_id).bind(stage as i16).bind(self.lease_seconds as f64)
        .fetch_optional(&mut *tx).await.map_err(|_| JobError::Storage)?
        ;
        let org_id = row.ok_or(JobError::InvalidState)?;
        append_pg_audit(
            &mut tx,
            job_id,
            org_id.as_deref(),
            runner_id,
            "checkpointed",
        )
        .await?;
        tx.commit().await.map_err(|_| JobError::Storage)
    }

    async fn renew_lease(&self, job_id: &str, runner_id: &str) -> Result<(), JobError> {
        let updated = sqlx::query(
            "UPDATE quickwit_admin_jobs SET lease_until=NOW()+make_interval(secs=>$3),updated_at=NOW() WHERE job_id=$1 AND status='running' AND lease_owner=$2 AND lease_until>NOW()",
        )
        .bind(job_id)
        .bind(runner_id)
        .bind(self.lease_seconds as f64)
        .execute(&self.pool)
        .await
        .map_err(|_| JobError::Storage)?;
        if updated.rows_affected() == 1 {
            Ok(())
        } else {
            Err(JobError::InvalidState)
        }
    }

    async fn complete(&self, job_id: &str, runner_id: &str) -> Result<(), JobError> {
        transition_terminal(&self.pool, job_id, runner_id, "succeeded", None).await
    }

    async fn fail(&self, job_id: &str, runner_id: &str, reason: &str) -> Result<(), JobError> {
        transition_terminal(&self.pool, job_id, runner_id, "failed", Some(reason)).await
    }
}

async fn append_pg_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    job_id: &str,
    org_id: Option<&str>,
    actor: &str,
    action: &str,
) -> Result<(), JobError> {
    sqlx::query("INSERT INTO quickwit_admin_job_audit (event_id,job_id,org_id,actor,action) VALUES ($1,$2,$3,$4,$5)")
        .bind(Uuid::new_v4().to_string()).bind(job_id).bind(org_id).bind(actor).bind(action)
        .execute(&mut **tx).await.map_err(|_| JobError::Storage)?;
    Ok(())
}

async fn transition_terminal(
    pool: &PgPool,
    job_id: &str,
    runner_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), JobError> {
    let mut tx = pool.begin().await.map_err(|_| JobError::Storage)?;
    let required_checkpoint = if status == "succeeded" {
        RebuildStage::WikiSourceLogs as i16
    } else {
        0
    };
    let row: Option<Option<String>> = sqlx::query_scalar(
        "UPDATE quickwit_admin_jobs SET status=$3,last_error=$4,lease_owner=NULL,lease_until=NULL,updated_at=NOW() WHERE job_id=$1 AND status='running' AND lease_owner=$2 AND lease_until>NOW() AND ($5=0 OR checkpoint=$5) RETURNING org_id",
    )
    .bind(job_id).bind(runner_id).bind(status).bind(error).bind(required_checkpoint)
    .fetch_optional(&mut *tx).await.map_err(|_| JobError::Storage)?;
    let org_id = row.ok_or(JobError::InvalidState)?;
    append_pg_audit(&mut tx, job_id, org_id.as_deref(), runner_id, status).await?;
    tx.commit().await.map_err(|_| JobError::Storage)
}
