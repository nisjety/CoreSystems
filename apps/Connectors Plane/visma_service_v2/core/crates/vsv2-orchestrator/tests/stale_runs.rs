//! Runs whose process died must not stay `running` forever.
//!
//! A `kill -9` (or an OOM kill, or a crashed container) leaves the `sync_run` row at `running`
//! because only the dying process could have finished it. Operators then see a run that never
//! ends. `abandon_stale_runs` marks such rows failed once they are older than any plausible
//! live run, and leaves live and finished runs alone.
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`.

mod common;

use chrono::{Duration, Utc};
use common::test_db::TestDb;
use uuid::Uuid;
use vsv2_common::{Source, TenantId};
use vsv2_state::{RunStatus, StateStore, SyncRun};

async fn insert(state: &StateStore, endpoint: &str, status: RunStatus, age: Duration) -> Uuid {
    let run_id = Uuid::new_v4();
    state
        .insert_run(&SyncRun {
            run_id,
            tenant: TenantId::new("tenant-a"),
            endpoint: endpoint.into(),
            source: Source::ErpRest,
            status,
            started_at: Utc::now() - age,
            finished_at: None,
            records_in: 0,
            records_written: 0,
            error: None,
        })
        .await
        .expect("insert run");
    run_id
}

async fn status_and_error(db: &TestDb, run_id: Uuid) -> (String, Option<String>) {
    let row: (String, Option<String>) =
        sqlx::query_as("SELECT status, error FROM vsv2.sync_run WHERE run_id = $1")
            .bind(run_id)
            .fetch_one(&db.state.pg)
            .await
            .expect("read run");
    row
}

#[tokio::test]
async fn an_old_running_run_is_marked_failed_with_a_reason() {
    let db = TestDb::create().await;
    let killed = insert(
        &db.state,
        "customers",
        RunStatus::Running,
        Duration::hours(7),
    )
    .await;

    let reaped = db
        .state
        .abandon_stale_runs(Duration::hours(6))
        .await
        .expect("abandon stale runs");

    assert_eq!(reaped, 1);
    let (status, error) = status_and_error(&db, killed).await;
    assert_eq!(status, "failed");
    assert!(
        error.as_deref().is_some_and(|e| e.contains("abandoned")),
        "the row must say why it failed: {error:?}"
    );
    let finished: Option<chrono::DateTime<Utc>> =
        sqlx::query_scalar("SELECT finished_at FROM vsv2.sync_run WHERE run_id = $1")
            .bind(killed)
            .fetch_one(&db.state.pg)
            .await
            .expect("read finished_at");
    assert!(
        finished.is_some(),
        "finished_at is set so the run has an end"
    );
}

#[tokio::test]
async fn a_recent_running_run_is_left_alone() {
    let db = TestDb::create().await;
    let live = insert(
        &db.state,
        "customers",
        RunStatus::Running,
        Duration::minutes(3),
    )
    .await;

    let reaped = db
        .state
        .abandon_stale_runs(Duration::hours(6))
        .await
        .expect("abandon stale runs");

    assert_eq!(reaped, 0);
    assert_eq!(status_and_error(&db, live).await.0, "running");
}

#[tokio::test]
async fn finished_runs_are_never_touched_however_old() {
    let db = TestDb::create().await;
    let ok = insert(&db.state, "a", RunStatus::Succeeded, Duration::days(30)).await;
    let failed = insert(&db.state, "b", RunStatus::Failed, Duration::days(30)).await;

    let reaped = db
        .state
        .abandon_stale_runs(Duration::hours(6))
        .await
        .expect("abandon stale runs");

    assert_eq!(reaped, 0);
    assert_eq!(status_and_error(&db, ok).await.0, "succeeded");
    assert_eq!(status_and_error(&db, failed).await, ("failed".into(), None));
}

#[tokio::test]
async fn a_second_call_finds_nothing_more_to_reap() {
    let db = TestDb::create().await;
    insert(
        &db.state,
        "customers",
        RunStatus::Running,
        Duration::hours(9),
    )
    .await;

    assert_eq!(
        db.state
            .abandon_stale_runs(Duration::hours(6))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.state
            .abandon_stale_runs(Duration::hours(6))
            .await
            .unwrap(),
        0
    );
}
