//! Checkpoint safety when runs overlap in time or in range.
//!
//! The scheduler serialises tenants through a Redis lease, but the HTTP
//! backfill handlers and the `sync` CLI command take no lease, so a backfill
//! can run next to a scheduled sync of the same endpoint. These tests pin what
//! must hold anyway: no duplicate rows, and a checkpoint that never moves
//! backwards (which would silently trigger a large re-fetch).
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`.

mod common;

use common::fake_adapter::{FakeAdapter, Gate, Step};
use common::{account, erp_def, utc, Harness};

#[tokio::test]
async fn overlapping_runs_of_the_same_data_create_no_duplicates() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    let gate = Gate::default();
    let batch = vec![
        account("1000", "2026-03-01T10:00:00Z"),
        account("2000", "2026-03-01T10:30:00Z"),
    ];
    h.fake
        .push("default", Step::Gated(gate.clone(), batch.clone()));
    h.fake.push("default", Step::Records(batch));

    let first = h.sync();
    let second_then_release = async {
        gate.entered.notified().await; // first run is now mid-fetch
        let result = h.sync().await;
        gate.release.notify_one();
        result
    };
    let (first, second) = tokio::join!(first, second_then_release);
    first.expect("first run");
    second.expect("second run");

    assert_eq!(h.bronze().await.len(), 2);
    assert!(h.runs().await.iter().all(|r| r.status == "succeeded"));
}

#[tokio::test]
async fn overlapping_runs_never_move_the_checkpoint_backwards() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    let gate = Gate::default();
    // The first run reads an older snapshot and finishes last.
    h.fake.push(
        "default",
        Step::Gated(gate.clone(), vec![account("1000", "2026-03-01T10:00:00Z")]),
    );
    h.fake.push(
        "default",
        Step::Records(vec![account("2000", "2026-03-01T11:00:00Z")]),
    );

    let slow_stale_run = h.sync();
    let fast_fresh_run_then_release = async {
        gate.entered.notified().await;
        let result = h.sync().await;
        gate.release.notify_one();
        result
    };
    let (slow, fast) = tokio::join!(slow_stale_run, fast_fresh_run_then_release);
    slow.expect("slow run");
    fast.expect("fast run");

    assert_eq!(
        h.checkpoint()
            .await
            .expect("checkpoint")
            .last_modified_watermark,
        Some(utc("2026-03-01T11:00:00Z")),
        "the stale run finished last and must not rewind the fresh run's watermark"
    );
}

#[tokio::test]
async fn bounded_backfill_never_moves_the_checkpoint_backwards() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![account("1000", "2026-03-10T09:00:00Z")]),
    );
    h.fake.push(
        "default",
        Step::Records(vec![account("9000", "2026-02-10T09:00:00Z")]),
    );

    h.sync().await.expect("regular sync");
    h.sync_window(
        Some(utc("2026-02-01T00:00:00Z")),
        Some(utc("2026-02-15T00:00:00Z")),
    )
    .await
    .expect("backfill of an older window");

    assert_eq!(
        h.checkpoint()
            .await
            .expect("checkpoint")
            .last_modified_watermark,
        Some(utc("2026-03-10T09:00:00Z")),
        "backfilling February must not rewind the March watermark"
    );
}
