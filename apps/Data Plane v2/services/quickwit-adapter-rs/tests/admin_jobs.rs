use std::{sync::Arc, time::Duration};

use quickwit_adapter_rs::jobs::{
    self, execute_claimed_job, AdminJobStore, InMemoryAdminJobStore, JobError, JobExecutor,
    JobSpec, JobStatus, RebuildStage, Submission,
};

fn tenant_spec(key: &str) -> JobSpec {
    JobSpec {
        org_id: Some("org-a".into()),
        global: false,
        clear: false,
        requested_by: "operator-a".into(),
        approval_id: "approval-a".into(),
        idempotency_key: key.into(),
        reason: "tenant search recovery".into(),
    }
}

#[tokio::test]
async fn submission_is_tenant_scoped_idempotent_and_rate_bounded() {
    let store = InMemoryAdminJobStore::new(Duration::from_secs(60), 1);
    let first = store.submit(tenant_spec("key-a")).await.expect("submit");
    let replay = store.submit(tenant_spec("key-a")).await.expect("replay");
    assert!(matches!(first, Submission::Created(_)));
    assert!(matches!(replay, Submission::Existing(_)));

    assert_eq!(
        store.submit(tenant_spec("key-b")).await,
        Err(JobError::RateLimited)
    );

    let mut conflicting = tenant_spec("key-a");
    conflicting.reason = "different operation".into();
    assert_eq!(
        store.submit(conflicting).await,
        Err(JobError::IdempotencyConflict)
    );
}

#[tokio::test]
async fn approval_is_two_person_and_audited_before_claim() {
    let store = InMemoryAdminJobStore::new(Duration::ZERO, 1);
    let Submission::Created(job) = store.submit(tenant_spec("key-a")).await.expect("submit") else {
        panic!("new job");
    };
    assert_eq!(store.claim_next("runner-a").await.expect("claim"), None);
    assert_eq!(
        store.approve(&job.job_id, "operator-a").await,
        Err(JobError::SeparationOfDuties)
    );
    let approved = store
        .approve(&job.job_id, "approver-b")
        .await
        .expect("approve");
    assert_eq!(approved.status, JobStatus::Approved);

    let audit = store.audit_entries().await;
    assert_eq!(audit.len(), 2);
    assert_eq!(audit[0].action, "requested");
    assert_eq!(audit[1].action, "approved");
    assert_ne!(audit[0].event_id, audit[1].event_id);
}

#[tokio::test]
async fn claims_are_concurrency_bounded_and_expired_leases_resume_from_checkpoint() {
    let store = InMemoryAdminJobStore::new(Duration::ZERO, 1);
    let first = submit_and_approve(&store, "key-a").await;
    let _second = submit_and_approve(&store, "key-b").await;

    let claimed = store
        .claim_next("runner-a")
        .await
        .expect("claim")
        .expect("job");
    assert_eq!(claimed.job_id, first.job_id);
    assert_eq!(store.claim_next("runner-b").await.expect("bounded"), None);

    store
        .record_checkpoint(&claimed.job_id, "runner-a", RebuildStage::WikiVersions)
        .await
        .expect("checkpoint");
    store.expire_lease_for_test(&claimed.job_id).await;
    let resumed = store
        .claim_next("runner-b")
        .await
        .expect("reclaim")
        .expect("resumed job");
    assert_eq!(resumed.checkpoint, Some(RebuildStage::WikiVersions));
}

#[derive(Default)]
struct RecordingExecutor {
    stages: tokio::sync::Mutex<Vec<RebuildStage>>,
}

struct FailingExecutor;

#[async_trait::async_trait]
impl JobExecutor for FailingExecutor {
    async fn execute_stage(
        &self,
        _job: &jobs::AdminJob,
        _stage: RebuildStage,
    ) -> Result<(), String> {
        Err("sanitized executor failure".into())
    }
}

#[async_trait::async_trait]
impl JobExecutor for RecordingExecutor {
    async fn execute_stage(
        &self,
        _job: &jobs::AdminJob,
        stage: RebuildStage,
    ) -> Result<(), String> {
        self.stages.lock().await.push(stage);
        Ok(())
    }
}

#[tokio::test]
async fn runner_checkpoints_every_stage_and_only_completes_after_final_stage() {
    let store = Arc::new(InMemoryAdminJobStore::new(Duration::ZERO, 1));
    let job = submit_and_approve(store.as_ref(), "key-a").await;
    let claimed = store
        .claim_next("runner-a")
        .await
        .expect("claim")
        .expect("job");
    let executor = RecordingExecutor::default();
    execute_claimed_job(store.as_ref(), &executor, "runner-a", claimed)
        .await
        .expect("execute");

    let completed = store.get(&job.job_id).await.expect("get").expect("job");
    assert_eq!(completed.status, JobStatus::Succeeded);
    assert_eq!(completed.checkpoint, Some(RebuildStage::WikiSourceLogs));
    assert_eq!(executor.stages.lock().await.as_slice(), RebuildStage::ALL);
    assert_eq!(
        store.audit_entries().await.last().unwrap().action,
        "succeeded"
    );
}

#[tokio::test]
async fn invalid_leases_transitions_and_executor_failures_fail_closed() {
    let store = InMemoryAdminJobStore::new(Duration::ZERO, 1);
    assert_eq!(
        store.approve("missing", "approver-b").await,
        Err(JobError::NotFound)
    );

    let job = submit_and_approve(&store, "key-invalid-state").await;
    assert_eq!(
        store.approve(&job.job_id, "approver-c").await,
        Err(JobError::InvalidState)
    );
    let claimed = store
        .claim_next("runner-a")
        .await
        .expect("claim")
        .expect("job");
    assert_eq!(
        store.renew_lease(&claimed.job_id, "runner-b").await,
        Err(JobError::InvalidState)
    );
    store
        .renew_lease(&claimed.job_id, "runner-a")
        .await
        .expect("renew owned lease");
    assert_eq!(
        store.complete(&claimed.job_id, "runner-a").await,
        Err(JobError::InvalidState)
    );
    assert_eq!(
        store
            .record_checkpoint(
                &claimed.job_id,
                "runner-b",
                RebuildStage::EmptyScopePreflight,
            )
            .await,
        Err(JobError::InvalidState)
    );

    store
        .fail(&claimed.job_id, "runner-a", "operator-requested stop")
        .await
        .expect("fail job");
    assert_eq!(
        store.get(&claimed.job_id).await.unwrap().unwrap().status,
        JobStatus::Failed
    );

    let expected_failure = submit_and_approve(&store, "key-executor-failure").await;
    let failing = store
        .claim_next("runner-b")
        .await
        .expect("claim")
        .expect("job");
    assert_eq!(failing.job_id, expected_failure.job_id);
    assert_eq!(
        execute_claimed_job(&store, &FailingExecutor, "runner-b", failing.clone()).await,
        Err(JobError::Execution)
    );
    assert_eq!(
        store.get(&failing.job_id).await.unwrap().unwrap().status,
        JobStatus::Failed
    );
}

#[tokio::test]
async fn clear_requests_stay_fail_closed_without_safe_quickwit_completion_proof() {
    let store = InMemoryAdminJobStore::new(Duration::ZERO, 1);
    let mut spec = tenant_spec("key-clear");
    spec.clear = true;
    assert_eq!(store.submit(spec).await, Err(JobError::UnsafeClear));
}

#[tokio::test]
async fn global_jobs_require_explicit_break_glass_authorization_upstream() {
    let store = InMemoryAdminJobStore::new(Duration::ZERO, 1);
    let mut spec = tenant_spec("key-global");
    spec.org_id = None;
    spec.global = true;
    let Submission::Created(job) = store.submit(spec).await.expect("global submit") else {
        panic!("new job");
    };
    assert!(job.global);
    assert!(job.org_id.is_none());
}

async fn submit_and_approve(store: &InMemoryAdminJobStore, key: &str) -> jobs::AdminJob {
    let Submission::Created(job) = store.submit(tenant_spec(key)).await.expect("submit") else {
        panic!("new job");
    };
    store
        .approve(&job.job_id, "approver-b")
        .await
        .expect("approve")
}

#[test]
fn durable_schema_enforces_idempotency_clear_containment_and_immutable_audit() {
    let migration =
        include_str!("../../../infra/postgres/migrations/20260711150000_quickwit_admin_jobs.sql");
    assert!(migration.contains("UNIQUE (scope_key, idempotency_key)"));
    assert!(migration.contains("CHECK (clear = FALSE)"));
    assert!(migration.contains("approved_by <> requested_by"));
    assert!(migration.contains("BEFORE UPDATE OR DELETE ON quickwit_admin_job_audit"));
    assert!(migration.contains("quickwit_admin_job_request_immutable"));
    assert!(migration.contains("checkpoint BETWEEN 0 AND 6"));
    assert!(migration.contains("batch_stage BETWEEN 0 AND 6"));
    assert!(migration.contains("batch_stage > checkpoint"));
    assert!(migration.contains("batch_cursor IS NOT NULL"));
}
