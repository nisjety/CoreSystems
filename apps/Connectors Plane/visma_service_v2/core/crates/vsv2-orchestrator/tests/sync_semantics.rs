//! Sync-semantics contract for `SyncExecutor` over the local Postgres bronze sink.
//!
//! These tests pin the properties the DataMart cutover depends on: the same
//! data can be synced twice without duplicates, and a failure at any stage
//! (fetch, validation, write) leaves the checkpoint exactly where it was so a
//! restart resumes from the last good state.
//!
//! They run against a throwaway database on the local compose Postgres; start
//! it with `make up`. No Visma or BigQuery access is involved.

mod common;

use common::fake_adapter::{Failure, FakeAdapter, Step};
use common::{account, erp_def, partitioned_erp_def, snapshot, utc, Harness, TENANT_A, TENANT_B};
use serde_json::json;
use vsv2_common::Error;

const TEN_MINUTES_OVERLAP: chrono::Duration = chrono::Duration::minutes(10);

// ── idempotency ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn resyncing_identical_data_creates_no_duplicates() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    let batch = vec![
        account("1000", "2026-03-01T10:00:00Z"),
        account("2000", "2026-03-01T10:05:00Z"),
        account("3000", "2026-03-01T10:10:00Z"),
    ];
    h.fake.push("default", Step::Records(batch.clone()));
    h.fake.push("default", Step::Records(batch));

    assert_eq!(h.sync().await.expect("first sync"), 3);
    let first = h.bronze().await;
    assert_eq!(h.sync().await.expect("second sync"), 3);
    let second = h.bronze().await;

    assert_eq!(second.len(), 3, "re-syncing must not add rows");
    for (before, after) in first.iter().zip(&second) {
        assert_eq!(before.record_key, after.record_key);
        assert_eq!(
            before.first_seen_at, after.first_seen_at,
            "first_seen_at must survive a re-sync"
        );
        assert_ne!(
            before.last_run_id, after.last_run_id,
            "last_run_id must point at the run that saw the row last"
        );
    }

    let runs = h.runs().await;
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|r| r.status == "succeeded"));
    assert!(runs
        .iter()
        .all(|r| r.records_in == 3 && r.records_written == 3));
}

#[tokio::test]
async fn overlapping_fetches_update_in_place_and_resume_from_watermark_minus_overlap() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![
            account("1000", "2026-03-01T10:00:00Z"),
            account("2000", "2026-03-01T10:15:00Z"),
            account("3000", "2026-03-01T10:30:00Z"),
        ]),
    );
    // The overlap re-delivers 3000 (now modified) plus genuinely new rows.
    let mut modified = account("3000", "2026-03-01T10:35:00Z");
    modified["description"] = json!("renamed upstream");
    h.fake.push(
        "default",
        Step::Records(vec![
            modified,
            account("4000", "2026-03-01T10:40:00Z"),
            account("5000", "2026-03-01T10:45:00Z"),
        ]),
    );

    h.sync().await.expect("first sync");
    h.sync().await.expect("second sync");

    let rows = h.bronze().await;
    assert_eq!(rows.len(), 5, "3000 is updated in place, not duplicated");
    let renamed = rows
        .iter()
        .find(|r| r.payload["accountCD"] == "3000")
        .expect("row 3000");
    assert_eq!(renamed.payload["description"], "renamed upstream");

    let calls = h.fake.calls();
    assert_eq!(calls[0].watermark, None, "first sync is a full fetch");
    assert_eq!(
        calls[1].watermark,
        Some(utc("2026-03-01T10:30:00Z") - TEN_MINUTES_OVERLAP),
        "second sync starts 10 minutes before the stored watermark"
    );
    assert_eq!(
        h.checkpoint()
            .await
            .expect("checkpoint")
            .last_modified_watermark,
        Some(utc("2026-03-01T10:45:00Z"))
    );
}

#[tokio::test]
async fn rows_absent_from_a_later_fetch_are_retained() {
    // Characterization: the local bronze sink is upsert-only. An incremental
    // sync never removes rows, so deletions can only arrive as explicit
    // tombstone records. Local bronze therefore cannot prove deletion parity.
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![
            account("1000", "2026-03-01T10:00:00Z"),
            account("2000", "2026-03-01T10:05:00Z"),
            account("3000", "2026-03-01T10:10:00Z"),
        ]),
    );
    h.fake.push(
        "default",
        Step::Records(vec![account("1000", "2026-03-01T10:20:00Z")]),
    );

    h.sync().await.expect("first sync");
    h.sync().await.expect("second sync");

    assert_eq!(h.bronze().await.len(), 3);
}

#[tokio::test]
async fn tenants_are_isolated_in_bronze_and_checkpoints() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![account("1000", "2026-03-01T10:00:00Z")]),
    );
    let mut other = account("1000", "2026-03-02T09:00:00Z");
    other["description"] = json!("tenant b's account");
    h.fake.push("default", Step::Records(vec![other]));

    h.sync_tenant(TENANT_A).await.expect("tenant a");
    h.sync_tenant(TENANT_B).await.expect("tenant b");

    let a = h.bronze_for(TENANT_A).await;
    let b = h.bronze_for(TENANT_B).await;
    assert_eq!((a.len(), b.len()), (1, 1), "same key, one row per tenant");
    assert_eq!(a[0].payload["description"], "account 1000");
    assert_eq!(b[0].payload["description"], "tenant b's account");

    assert_eq!(
        h.checkpoint_for(TENANT_A, "default")
            .await
            .and_then(|c| c.last_modified_watermark),
        Some(utc("2026-03-01T10:00:00Z"))
    );
    assert_eq!(
        h.checkpoint_for(TENANT_B, "default")
            .await
            .and_then(|c| c.last_modified_watermark),
        Some(utc("2026-03-02T09:00:00Z"))
    );
}

// ── failures never move the checkpoint ───────────────────────────────────────

#[tokio::test]
async fn nul_bytes_in_upstream_data_do_not_fail_the_endpoint() {
    // An inventory item note held a NUL; jsonb rejects \u0000 and the whole endpoint failed.
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    let mut dirty = account("1000", "2026-03-01T10:00:00Z");
    dirty["description"] = json!("before\u{0}after");
    h.fake.push(
        "default",
        Step::Records(vec![dirty, account("2000", "2026-03-01T10:05:00Z")]),
    );

    assert_eq!(h.sync().await.expect("sync must survive a NUL byte"), 2);

    let rows = h.bronze().await;
    assert_eq!(rows.len(), 2);
    let stored = rows
        .iter()
        .find(|r| r.record_key.contains("1000"))
        .expect("row 1000 is stored");
    assert_eq!(stored.payload["description"], "before\u{FFFD}after");
}

#[tokio::test]
async fn failure_with_binary_error_text_is_still_recorded_as_failed() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push("default", Step::Fail(Failure::BinaryBody));

    let err = h.sync().await.expect_err("the fetch fails");

    assert!(
        matches!(err, Error::UpstreamStatus { status: 502, .. }),
        "the caller must see the upstream error, not a state-store error: {err}"
    );
    let runs = h.runs().await;
    assert_eq!(runs.len(), 1);
    assert_eq!(
        runs[0].status, "failed",
        "a NUL byte in the error text must not leave the run stuck as running"
    );
    let recorded = runs[0].error.as_deref().expect("error text is recorded");
    assert!(recorded.contains("binary"), "{recorded}");
    assert!(!recorded.contains('\0'));
}

#[tokio::test]
async fn fetch_failure_keeps_checkpoint_and_restart_resumes_from_last_good_state() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![
            account("1000", "2026-03-01T10:00:00Z"),
            account("2000", "2026-03-01T10:30:00Z"),
        ]),
    );
    h.fake.push("default", Step::Fail(Failure::Upstream(502)));
    h.fake.push(
        "default",
        Step::Records(vec![account("3000", "2026-03-01T11:00:00Z")]),
    );

    h.sync().await.expect("first sync");
    let good_checkpoint = snapshot(&h.checkpoint().await);
    let good_rows = h.bronze().await;

    let err = h.sync().await.expect_err("second sync must fail");
    assert!(
        matches!(err, Error::UpstreamStatus { status: 502, .. }),
        "unexpected error: {err}"
    );
    assert_eq!(
        snapshot(&h.checkpoint().await),
        good_checkpoint,
        "a failed fetch must leave the checkpoint untouched"
    );
    let rows_after_failure = h.bronze().await;
    assert_eq!(rows_after_failure.len(), good_rows.len());
    assert!(
        rows_after_failure
            .iter()
            .zip(&good_rows)
            .all(|(a, b)| a.last_run_id == b.last_run_id),
        "a failed run must not touch bronze rows"
    );

    h.sync().await.expect("restart after failure");

    let calls = h.fake.calls();
    let resume_from = Some(utc("2026-03-01T10:30:00Z") - TEN_MINUTES_OVERLAP);
    assert_eq!(calls[1].watermark, resume_from);
    assert_eq!(
        calls[2].watermark, resume_from,
        "restart resumes from the last good checkpoint, not from the failed attempt"
    );
    assert_eq!(
        h.checkpoint()
            .await
            .expect("checkpoint")
            .last_modified_watermark,
        Some(utc("2026-03-01T11:00:00Z"))
    );
    assert_eq!(h.bronze().await.len(), 3);

    let runs = h.runs().await;
    let statuses: Vec<&str> = runs.iter().map(|r| r.status.as_str()).collect();
    assert_eq!(statuses, ["succeeded", "failed", "succeeded"]);
    assert!(
        runs[1].error.as_deref().is_some_and(|e| e.contains("502")),
        "failed run must record why it failed: {:?}",
        runs[1].error
    );
}

#[tokio::test]
async fn rate_limited_fetch_keeps_checkpoint_and_records_a_429_metric() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![account("1000", "2026-03-01T10:00:00Z")]),
    );
    h.fake.push("default", Step::Fail(Failure::RateLimited));

    h.sync().await.expect("first sync");
    let good_checkpoint = snapshot(&h.checkpoint().await);

    let err = h.sync().await.expect_err("rate-limited sync must fail");
    assert!(matches!(err, Error::RateLimited { .. }), "got: {err}");
    assert_eq!(snapshot(&h.checkpoint().await), good_checkpoint);

    let metrics = h.page_metrics().await;
    assert_eq!(metrics.len(), 1, "one failed-page metric: {metrics:?}");
    let (status, retry_after) = metrics[0];
    assert_eq!(status, 429);
    assert!(
        retry_after.is_some_and(|secs| (25..=30).contains(&secs)),
        "retry-after should reflect the cooldown: {retry_after:?}"
    );
}

#[tokio::test]
async fn failure_in_one_partition_commits_nothing_for_any_partition() {
    let h = Harness::new(
        partitioned_erp_def("accounts"),
        FakeAdapter::with_partitions(&["ledger=A", "ledger=B"]),
    )
    .await;
    h.fake.push(
        "ledger=A",
        Step::Records(vec![account("1000", "2026-03-01T10:00:00Z")]),
    );
    h.fake.push("ledger=B", Step::Fail(Failure::Upstream(503)));
    h.fake.push(
        "ledger=A",
        Step::Records(vec![account("1000", "2026-03-01T10:00:00Z")]),
    );
    h.fake.push(
        "ledger=B",
        Step::Records(vec![account("1000", "2026-03-01T11:00:00Z")]),
    );

    h.sync().await.expect_err("partition B fails");

    assert_eq!(
        h.checkpoint_row_count().await,
        0,
        "no partition may checkpoint when any partition failed"
    );
    assert!(h.bronze().await.is_empty(), "nothing may be written either");
    assert_eq!(h.runs().await[0].status, "failed");

    h.sync().await.expect("retry succeeds");

    let calls = h.fake.calls();
    assert_eq!(
        calls[2].watermark, None,
        "partition A re-fetches from scratch: a healthy partition makes no \
         progress while a sibling keeps failing"
    );
    assert_eq!(
        h.bronze().await.len(),
        2,
        "same account code, one row per ledger"
    );
    assert_eq!(
        h.checkpoint_partition("ledger=A")
            .await
            .and_then(|c| c.last_modified_watermark),
        Some(utc("2026-03-01T10:00:00Z"))
    );
    assert_eq!(
        h.checkpoint_partition("ledger=B")
            .await
            .and_then(|c| c.last_modified_watermark),
        Some(utc("2026-03-01T11:00:00Z"))
    );
}

#[tokio::test]
async fn an_empty_partition_does_not_disturb_its_own_or_its_siblings_checkpoint() {
    let h = Harness::new(
        partitioned_erp_def("accounts"),
        FakeAdapter::with_partitions(&["ledger=A", "ledger=B"]),
    )
    .await;
    h.fake.push(
        "ledger=A",
        Step::Records(vec![account("1000", "2026-03-01T10:00:00Z")]),
    );
    h.fake.push(
        "ledger=B",
        Step::Records(vec![account("2000", "2026-03-01T11:00:00Z")]),
    );
    h.fake.push(
        "ledger=A",
        Step::Records(vec![account("1000", "2026-03-01T12:00:00Z")]),
    );
    h.fake.push("ledger=B", Step::Records(vec![]));

    h.sync().await.expect("first sync");
    h.sync().await.expect("second sync");

    assert_eq!(
        h.checkpoint_partition("ledger=A")
            .await
            .and_then(|c| c.last_modified_watermark),
        Some(utc("2026-03-01T12:00:00Z")),
        "A advances independently"
    );
    assert_eq!(
        h.checkpoint_partition("ledger=B")
            .await
            .and_then(|c| c.last_modified_watermark),
        Some(utc("2026-03-01T11:00:00Z")),
        "an empty fetch keeps B's watermark"
    );
}

#[tokio::test]
async fn validation_rejection_blocks_write_and_checkpoint_then_recovers() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![
            account("1000", "2026-03-01T10:00:00Z"),
            json!({ "description": "no key", "lastModifiedDateTime": "2026-03-01T10:05:00Z" }),
        ]),
    );
    h.fake.push(
        "default",
        Step::Records(vec![account("1000", "2026-03-01T10:00:00Z")]),
    );

    let err = h.sync().await.expect_err("bad row rejects the batch");
    assert!(
        matches!(&err, Error::Config(msg) if msg.contains("validation failed")),
        "unexpected error: {err}"
    );
    assert!(
        h.bronze().await.is_empty(),
        "one bad row blocks the whole endpoint: the good row is not written"
    );
    assert!(h.checkpoint().await.is_none());
    assert_eq!(h.runs().await[0].status, "failed");

    h.sync().await.expect("clean batch recovers");
    assert_eq!(h.bronze().await.len(), 1);
    assert!(h.checkpoint().await.is_some());
}

#[tokio::test]
async fn bronze_write_failure_rolls_back_the_whole_batch_and_keeps_checkpoint() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![
            account("1000", "2026-03-01T10:00:00Z"),
            account("2000", "2026-03-01T10:10:00Z"),
        ]),
    );
    let doomed_batch = vec![
        account("4000", "2026-03-01T11:00:00Z"),
        account("POISON", "2026-03-01T11:10:00Z"),
        account("5000", "2026-03-01T11:20:00Z"),
    ];
    h.fake.push("default", Step::Records(doomed_batch.clone()));
    h.fake.push("default", Step::Records(doomed_batch));

    h.sync().await.expect("first sync");
    let good_checkpoint = snapshot(&h.checkpoint().await);
    let good_rows = h.bronze().await;

    // Make Postgres reject the middle record of the next batch, mid-transaction.
    sqlx::query(
        "ALTER TABLE vsv2.local_bronze_record \
         ADD CONSTRAINT reject_poison CHECK (record_key NOT LIKE '%POISON%')",
    )
    .execute(&h.db.state.pg)
    .await
    .expect("install poison constraint");

    let err = h.sync().await.expect_err("write must fail");
    assert!(
        matches!(err, Error::StateStore(_)),
        "unexpected error: {err}"
    );

    let rows = h.bronze().await;
    assert_eq!(
        rows.len(),
        good_rows.len(),
        "4000 was inserted before the failure and must have rolled back"
    );
    assert_eq!(snapshot(&h.checkpoint().await), good_checkpoint);
    let runs = h.runs().await;
    assert_eq!(runs[1].status, "failed");
    assert_eq!(runs[1].records_written, 0);

    sqlx::query("ALTER TABLE vsv2.local_bronze_record DROP CONSTRAINT reject_poison")
        .execute(&h.db.state.pg)
        .await
        .expect("remove poison constraint");
    h.sync().await.expect("restart after the write path heals");

    assert_eq!(h.bronze().await.len(), 5);
    assert_eq!(
        h.checkpoint()
            .await
            .expect("checkpoint")
            .last_modified_watermark,
        Some(utc("2026-03-01T11:20:00Z"))
    );
}
