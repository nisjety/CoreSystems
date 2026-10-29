use async_trait::async_trait;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use uuid::Uuid;
use xero_mcp::{
    backend::{Backend, ConnectionInfo, Failure, Query, Response},
    config::AccessScope,
    jobs::{Destination, ExtractArgs, ExtractionPlan, Job, JobRegistry},
};
const TENANT: &str = "5f3c9a2e-0000-0000-0000-000000000000";
struct Fixture;
#[async_trait]
impl Backend for Fixture {
    async fn connections(&self) -> Result<Vec<ConnectionInfo>, Failure> {
        Ok(vec![])
    }
    async fn fetch(&self, _: &Query) -> Result<Response, Failure> {
        unreachable!()
    }
    async fn extract(
        &self,
        _: Uuid,
        _: &str,
        _: &ExtractionPlan,
        _: Destination,
        _: &Path,
    ) -> Result<Value, Failure> {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        Ok(json!({"complete":true}))
    }
}
fn arguments(key: &str) -> ExtractArgs {
    serde_json::from_value(json!({"tenant_id":TENANT,"destination":"local","idempotency_key":key,"plan":{"kind":"accounting","entities":["invoices"]}})).unwrap()
}

#[tokio::test]
async fn retries_reuse_receipts_restart_and_other_clients_cannot_read_jobs() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    let backend = Arc::new(Fixture);
    let first = registry
        .start(backend.clone(), &scope, arguments("same"))
        .await
        .unwrap();
    assert_eq!(
        registry
            .start(backend.clone(), &scope, arguments("same"))
            .await
            .unwrap()
            .job_id,
        first.job_id
    );
    let mut changed = arguments("same");
    changed.destination = Destination::Gcs;
    assert_eq!(
        registry
            .start(backend.clone(), &scope, changed)
            .await
            .unwrap_err()
            .code,
        "idempotency_conflict"
    );
    let other = AccessScope::new("other", vec![TENANT.into()], true, false, false).unwrap();
    assert!(registry.get(&other, first.job_id, false).await.is_err());
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if registry
                .get(&scope, first.job_id, false)
                .await
                .unwrap()
                .status
                == "completed"
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let restarted = Arc::new(JobRegistry::new(directory.path().into()));
    assert_eq!(
        restarted
            .start(backend, &scope, arguments("same"))
            .await
            .unwrap()
            .job_id,
        first.job_id
    );
    assert_eq!(
        restarted
            .get(&scope, first.job_id, false)
            .await
            .unwrap()
            .status,
        "completed"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file = std::fs::File::open(
            directory
                .path()
                .join("jobs")
                .join(format!("{}.json", first.idempotency_digest)),
        )
        .unwrap();
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
}

#[tokio::test]
async fn cancellation_and_retention_permissions_are_enforced() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    let zdr = AccessScope {
        zero_data_retention: true,
        ..scope.clone()
    };
    assert!(registry
        .start(Arc::new(Fixture), &zdr, arguments("zdr"))
        .await
        .is_err());
    let job = registry
        .start(Arc::new(Fixture), &scope, arguments("cancel"))
        .await
        .unwrap();
    registry.get(&scope, job.job_id, true).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            if registry
                .get(&scope, job.job_id, false)
                .await
                .unwrap()
                .status
                == "cancelled"
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn mixed_reports_unknown_modes_bad_dates_and_global_identity_extraction_are_rejected() {
    for plan in [
        json!({"kind":"accounting","entities":["invoices","report_balance_sheet"],"mode":"reports","as_of":"2026-10-03"}),
        json!({"kind":"accounting","entities":["invoices"],"mode":"unknown"}),
        json!({"kind":"accounting","entities":["invoices"],"mode":"backfill","from":"2026-10-03","to":"2025-01-01"}),
        json!({"kind":"extended","family":"identity"}),
        json!({"kind":"extended","family":"finance"}),
    ] {
        assert!(serde_json::from_value::<ExtractionPlan>(plan)
            .unwrap()
            .validate()
            .is_err());
    }
}

#[tokio::test]
async fn damaged_receipts_fail_closed_instead_of_repeating_extraction() {
    let directory = tempfile::tempdir().unwrap();
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let job = registry
        .start(Arc::new(Fixture), &scope, arguments("damaged"))
        .await
        .unwrap();
    registry.get(&scope, job.job_id, true).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while registry
            .get(&scope, job.job_id, false)
            .await
            .unwrap()
            .status
            != "cancelled"
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    std::fs::write(
        directory
            .path()
            .join("jobs")
            .join(format!("{}.json", job.idempotency_digest)),
        b"invalid receipt",
    )
    .unwrap();
    let restarted = Arc::new(JobRegistry::new(directory.path().into()));
    assert_eq!(
        restarted
            .start(Arc::new(Fixture), &scope, arguments("damaged"))
            .await
            .unwrap_err()
            .code,
        "job_storage_unavailable"
    );
}

#[tokio::test]
async fn sensitive_job_receipts_recheck_current_permission() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let allowed = AccessScope::new("first", vec![TENANT.into()], true, true, false).unwrap();
    let revoked = AccessScope {
        allow_sensitive: false,
        ..allowed.clone()
    };
    let args = serde_json::from_value(json!({"tenant_id":TENANT,"destination":"local","idempotency_key":"sensitive","plan":{"kind":"extended","family":"bankfeeds"}})).unwrap();
    let job = registry
        .start(Arc::new(Fixture), &allowed, args)
        .await
        .unwrap();
    assert_eq!(
        registry
            .get(&revoked, job.job_id, false)
            .await
            .unwrap_err()
            .code,
        "sensitive_access_required"
    );
    registry.get(&allowed, job.job_id, true).await.unwrap();
}

struct Held {
    started: tokio::sync::Notify,
    finish: tokio::sync::Notify,
}
#[async_trait]
impl Backend for Held {
    async fn connections(&self) -> Result<Vec<ConnectionInfo>, Failure> {
        Ok(vec![])
    }
    async fn fetch(&self, _: &Query) -> Result<Response, Failure> {
        unreachable!()
    }
    async fn extract(
        &self,
        _: Uuid,
        _: &str,
        _: &ExtractionPlan,
        _: Destination,
        _: &Path,
    ) -> Result<Value, Failure> {
        self.started.notify_one();
        self.finish.notified().await;
        Ok(json!({"complete":true}))
    }
}
#[tokio::test]
async fn receipt_write_failures_are_reported_instead_of_durable_success() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    let backend = Arc::new(Held {
        started: tokio::sync::Notify::new(),
        finish: tokio::sync::Notify::new(),
    });
    let job = registry
        .start(backend.clone(), &scope, arguments("blocked-receipt"))
        .await
        .unwrap();
    backend.started.notified().await;
    let path = directory
        .path()
        .join("jobs")
        .join(format!("{}.json", job.idempotency_digest));
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    backend.finish.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let current = registry.get(&scope, job.job_id, false).await.unwrap();
            if current.status != "running" {
                assert_eq!(current.status, "receipt_unavailable");
                assert_eq!(current.error.unwrap()["code"], "job_receipt_unavailable");
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn completed_history_is_evicted_from_memory_without_losing_receipt_lookup() {
    let directory = tempfile::tempdir().unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    let backend = Arc::new(Fixture);
    let first = registry
        .start(backend.clone(), &scope, arguments("history-0"))
        .await
        .unwrap();
    for index in 0..130 {
        let job = registry
            .start(
                backend.clone(),
                &scope,
                arguments(&format!("history-{index}")),
            )
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while registry
                .get(&scope, job.job_id, false)
                .await
                .unwrap()
                .status
                != "completed"
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    assert_eq!(
        registry
            .get(&scope, first.job_id, false)
            .await
            .unwrap()
            .status,
        "completed"
    );
}

#[tokio::test]
async fn index_failure_before_start_does_not_consume_the_idempotency_key() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("jobs")).unwrap();
    let index = directory.path().join("jobs/by-id");
    std::fs::write(&index, b"blocked index directory").unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    assert!(registry
        .start(Arc::new(Fixture), &scope, arguments("pre-start-failure"))
        .await
        .is_err());
    std::fs::remove_file(index).unwrap();
    let job = registry
        .start(Arc::new(Fixture), &scope, arguments("pre-start-failure"))
        .await
        .unwrap();
    assert_eq!(job.status, "queued");
    registry.get(&scope, job.job_id, true).await.unwrap();
}

#[tokio::test]
async fn explicitly_retried_queued_receipt_recovers_the_committed_job() {
    let directory = tempfile::tempdir().unwrap();
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    let registry = Arc::new(JobRegistry::new(directory.path().into()));
    let queued = registry
        .start(Arc::new(Fixture), &scope, arguments("queued-retry"))
        .await
        .unwrap();
    registry.get(&scope, queued.job_id, true).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while registry
            .get(&scope, queued.job_id, false)
            .await
            .unwrap()
            .status
            != "cancelled"
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    // Simulate a visible pre-start receipt after ambiguous publication failure.
    std::fs::write(
        directory
            .path()
            .join("jobs")
            .join(format!("{}.json", queued.idempotency_digest)),
        serde_json::to_vec(&queued).unwrap(),
    )
    .unwrap();
    let restarted = Arc::new(JobRegistry::new(directory.path().into()));
    let mut wrong = arguments("queued-retry");
    wrong.destination = Destination::Gcs;
    assert_eq!(
        restarted
            .start(Arc::new(Fixture), &scope, wrong)
            .await
            .unwrap_err()
            .code,
        "idempotency_conflict"
    );
    let recovered = restarted
        .start(Arc::new(Fixture), &scope, arguments("queued-retry"))
        .await
        .unwrap();
    assert_eq!(recovered.job_id, queued.job_id);
    assert_eq!(recovered.status, "queued");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while restarted
            .get(&scope, queued.job_id, false)
            .await
            .unwrap()
            .status
            != "completed"
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cold_cancellation_and_same_key_recovery_are_serialized() {
    let directory = tempfile::tempdir().unwrap();
    let scope = AccessScope::new("first", vec![TENANT.into()], true, false, false).unwrap();
    std::fs::create_dir_all(directory.path().join("jobs/by-id")).unwrap();
    for index in 0..8 {
        let args = arguments(&format!("cold-cancel-{index}"));
        let job = Job {
            job_id: Uuid::new_v4(),
            client_id: scope.client_id.clone(),
            tenant_id: TENANT.into(),
            status: "queued".into(),
            created_at: chrono::Utc::now().to_rfc3339(),
            finished_at: None,
            destination: args.destination,
            sensitive: false,
            result: None,
            error: None,
            idempotency_digest: format!(
                "{:x}",
                Sha256::digest(format!("first:{TENANT}:{}", args.idempotency_key))
            ),
            plan_digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&(args.destination, &args.plan)).unwrap())
            ),
        };
        // Slow the cold read so the retry overlaps receipt recovery on another worker.
        let mut receipt = vec![b' '; 8 * 1024 * 1024];
        receipt.extend(serde_json::to_vec(&job).unwrap());
        std::fs::write(
            directory
                .path()
                .join("jobs")
                .join(format!("{}.json", job.idempotency_digest)),
            receipt,
        )
        .unwrap();
        std::fs::write(
            directory
                .path()
                .join("jobs/by-id")
                .join(format!("{}.ref", job.job_id)),
            &job.idempotency_digest,
        )
        .unwrap();
        let registry = Arc::new(JobRegistry::new(directory.path().into()));
        let cancellation = {
            let registry = registry.clone();
            let scope = scope.clone();
            tokio::spawn(async move { registry.get(&scope, job.job_id, true).await.unwrap() })
        };
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let retry = registry
            .start(
                Arc::new(Held {
                    started: tokio::sync::Notify::new(),
                    finish: tokio::sync::Notify::new(),
                }),
                &scope,
                args,
            )
            .await
            .unwrap();
        let cancellation = cancellation.await.unwrap();
        assert_eq!(retry.job_id, cancellation.job_id);
        assert_ne!(cancellation.status, "interrupted");
        if cancellation.status == "cancelled" {
            assert_eq!(
                retry.status, "cancelled",
                "A confirmed cold cancellation must not be replayed"
            );
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while registry
                .get(&scope, retry.job_id, false)
                .await
                .unwrap()
                .status
                != "cancelled"
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
