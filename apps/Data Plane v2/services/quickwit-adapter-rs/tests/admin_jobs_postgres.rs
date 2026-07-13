use std::time::Duration;

use quickwit_adapter_rs::jobs::{
    AdminJobStore, JobSpec, JobStatus, PgAdminJobStore, RebuildStage, Submission,
};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing to disposable PostgreSQL"]
async fn postgres_job_lifecycle_is_durable_resumable_and_append_only() {
    let database_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&database_url)
        .await
        .expect("disposable database");
    let store = PgAdminJobStore::new(
        pool.clone(),
        Duration::from_secs(1),
        Duration::from_secs(30),
    );
    let suffix = Uuid::new_v4().to_string();
    let spec = JobSpec {
        org_id: Some(format!("test-org-{suffix}")),
        global: false,
        clear: false,
        requested_by: format!("requester-{suffix}"),
        approval_id: format!("approval-{suffix}"),
        idempotency_key: format!("idempotency-{suffix}"),
        reason: "disposable PostgreSQL lifecycle validation".into(),
    };

    let Submission::Created(job) = store.submit(spec.clone()).await.expect("submit") else {
        panic!("created job");
    };
    assert!(matches!(
        store.submit(spec).await.expect("idempotent replay"),
        Submission::Existing(_)
    ));
    let approved = store
        .approve(&job.job_id, &format!("approver-{suffix}"))
        .await
        .expect("approve");
    assert_eq!(approved.status, JobStatus::Approved);
    let claimed = store
        .claim_next(&format!("runner-{suffix}"))
        .await
        .expect("claim")
        .expect("claimed job");
    assert_eq!(claimed.job_id, job.job_id);
    for stage in RebuildStage::ALL.iter().copied() {
        store
            .record_checkpoint(&job.job_id, &format!("runner-{suffix}"), stage)
            .await
            .expect("checkpoint");
    }
    store
        .complete(&job.job_id, &format!("runner-{suffix}"))
        .await
        .expect("complete");
    assert_eq!(
        store.get(&job.job_id).await.unwrap().unwrap().status,
        JobStatus::Succeeded
    );

    let audit_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM quickwit_admin_job_audit WHERE job_id=$1")
            .bind(&job.job_id)
            .fetch_one(&pool)
            .await
            .expect("audit count");
    assert_eq!(audit_count, 10);
    assert!(
        sqlx::query("UPDATE quickwit_admin_job_audit SET action='tampered' WHERE job_id=$1")
            .bind(&job.job_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE quickwit_admin_jobs SET reason='tampered' WHERE job_id=$1")
            .bind(&job.job_id)
            .execute(&pool)
            .await
            .is_err()
    );
}
