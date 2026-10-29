//! An explicit `date_from` window means exactly that window.
//!
//! Scheduled runs resume from the checkpoint. A backfill names a range on purpose, so it must
//! start where the operator said, also when the endpoint has already synced past that point;
//! otherwise "backfill February" silently fetches nothing once March has been synced. The
//! checkpoint is monotone in the database (see `sync_races.rs`), so an old window can no longer
//! drag it backwards.
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`.

mod common;

use common::fake_adapter::{FakeAdapter, Step};
use common::{account, erp_def, utc, Harness};

#[tokio::test]
async fn a_backfill_window_before_the_checkpoint_asks_upstream_for_its_own_range() {
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

    let backfill = &h.fake.calls()[1];
    assert_eq!(
        backfill.watermark,
        Some(utc("2026-02-01T00:00:00Z")),
        "the backfill must ask for February, not for everything after the March checkpoint"
    );
    assert_eq!(backfill.window_end, Some(utc("2026-02-15T00:00:00Z")));
    assert_eq!(h.bronze().await.len(), 2, "the February row is written");
    assert_eq!(
        h.checkpoint()
            .await
            .expect("checkpoint")
            .last_modified_watermark,
        Some(utc("2026-03-10T09:00:00Z")),
        "and the March checkpoint is untouched"
    );
}

#[tokio::test]
async fn a_window_after_the_checkpoint_starts_at_date_from() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![account("1000", "2026-03-10T09:00:00Z")]),
    );
    h.fake.push(
        "default",
        Step::Records(vec![account("2000", "2026-03-21T09:00:00Z")]),
    );

    h.sync().await.expect("regular sync");
    h.sync_window(
        Some(utc("2026-03-20T00:00:00Z")),
        Some(utc("2026-03-25T00:00:00Z")),
    )
    .await
    .expect("forward window");

    assert_eq!(
        h.fake.calls()[1].watermark,
        Some(utc("2026-03-20T00:00:00Z"))
    );
}

#[tokio::test]
async fn a_run_without_date_from_still_resumes_from_the_checkpoint() {
    let h = Harness::new(erp_def("accounts"), FakeAdapter::single_partition()).await;
    h.fake.push(
        "default",
        Step::Records(vec![account("1000", "2026-03-10T09:00:00Z")]),
    );
    h.fake.push("default", Step::Records(vec![]));

    h.sync().await.expect("regular sync");
    // Only an upper bound: the lower bound is still the checkpoint minus the usual overlap.
    h.sync_window(None, Some(utc("2026-03-31T00:00:00Z")))
        .await
        .expect("upper-bounded run");

    assert_eq!(
        h.fake.calls()[1].watermark,
        Some(utc("2026-03-10T09:00:00Z") - chrono::Duration::minutes(10))
    );
}
