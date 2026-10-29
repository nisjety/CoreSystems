//! The reads behind `vsv2 monitor` (`vsv2_state::monitor`) against a real Postgres: which rows
//! come back, in what shape, and that the connection cannot write.
//!
//! Requires the local compose Postgres (`make up`); see `common/mod.rs`.

mod common;

use chrono::{Duration, Utc};
use common::{TestDb, ACTIVE, OTHER};
use uuid::Uuid;
use vsv2_state::monitor::{
    connect_read_only, read_snapshot, snapshot_of, Snapshot, SnapshotRequest,
};
use vsv2_state::run_history::RunStatus;
use vsv2_state::scheduler_pass::{self, PassStatus};

fn request(labels: &[String]) -> SnapshotRequest<'_> {
    SnapshotRequest {
        passes_per_tier: 3,
        run_endpoints: labels,
        runs_per_endpoint: 2,
        run_lookback_days: 30,
    }
}

fn labels() -> Vec<String> {
    vec![
        "customer_invoices:open_set".to_string(),
        "supplier_invoices:open_set".to_string(),
    ]
}

async fn snapshot(db: &TestDb, request: &SnapshotRequest<'_>) -> Snapshot {
    snapshot_of(&db.url, request)
        .await
        .expect("read the snapshot")
}

fn ids(snapshot: &Snapshot, tenant: &str, tier: &str) -> Vec<Uuid> {
    snapshot
        .passes
        .iter()
        .filter(|pass| pass.tenant_id == tenant && pass.tier == tier)
        .map(|pass| pass.pass_id)
        .collect()
}

#[tokio::test]
async fn the_snapshot_keeps_what_the_checks_need_and_nothing_more_of_a_long_history() {
    let db = TestDb::create().await;
    let oldest = db.succeeded(ACTIVE, "hourly", Duration::hours(100)).await;
    let mut failures = Vec::new();
    for hours_ago in [80, 70, 60, 50, 40, 30, 20, 10] {
        failures.push(
            db.failed(ACTIVE, "hourly", Duration::hours(hours_ago), &["customers"])
                .await,
        );
    }
    let running = db
        .running(ACTIVE, "hourly", Duration::hours(1), Duration::seconds(20))
        .await;
    let daily = db
        .failed(ACTIVE, "daily", Duration::hours(7), &["accounts"])
        .await;
    let elsewhere = db.succeeded(OTHER, "hourly", Duration::hours(2)).await;
    let labels = labels();

    let snapshot = snapshot(&db, &request(&labels)).await;

    assert_eq!(
        ids(&snapshot, ACTIVE, "hourly"),
        [
            running,     // every running pass
            failures[7], // the three newest finished passes: 10, 20 and 30 hours ago
            failures[6],
            failures[5],
            oldest, // the newest succeeded pass, which is also the oldest pass
        ],
        "newest first"
    );
    assert_eq!(ids(&snapshot, ACTIVE, "daily"), [daily]);
    assert_eq!(ids(&snapshot, OTHER, "hourly"), [elsewhere]);
    assert_eq!(snapshot.passes.len(), 7);
}

#[tokio::test]
async fn the_oldest_pass_of_a_tier_is_kept_however_many_came_after() {
    let db = TestDb::create().await;
    let first = db
        .failed(ACTIVE, "full", Duration::days(20), &["customers"])
        .await;
    for days_ago in 1..=10 {
        db.failed(ACTIVE, "full", Duration::days(days_ago), &["customers"])
            .await;
    }
    let labels = labels();

    let snapshot = snapshot(&db, &request(&labels)).await;

    let kept = ids(&snapshot, ACTIVE, "full");
    assert_eq!(
        kept.len(),
        4,
        "three newest finished passes and the first one"
    );
    assert_eq!(kept.last(), Some(&first));
}

#[tokio::test]
async fn a_row_carries_its_status_names_heartbeat_and_abandoned_flag() {
    let db = TestDb::create().await;
    let failed = db
        .failed(
            ACTIVE,
            "hourly",
            Duration::minutes(40),
            &["customers", "shipments"],
        )
        .await;
    let stuck = db
        .running(ACTIVE, "daily", Duration::hours(2), Duration::minutes(30))
        .await;
    // The scheduler's own cleanup produces an abandoned pass: failed, with `abandoned` set.
    let abandoned = db
        .running(ACTIVE, "full", Duration::hours(2), Duration::minutes(30))
        .await;
    let marked = scheduler_pass::abandon_stale(&db.state.pg, Duration::minutes(10))
        .await
        .expect("abandon stale passes");
    assert_eq!(marked, 2, "both running passes were silent for 30 minutes");
    let fresh = db
        .running(OTHER, "hourly", Duration::minutes(3), Duration::seconds(5))
        .await;
    let labels = labels();

    let snapshot = snapshot(&db, &request(&labels)).await;

    let row = |id: Uuid| {
        snapshot
            .passes
            .iter()
            .find(|pass| pass.pass_id == id)
            .unwrap_or_else(|| panic!("pass {id} was not read"))
    };
    let failed = row(failed);
    assert_eq!(failed.status, PassStatus::Failed);
    assert_eq!(failed.failed_names, ["customers", "shipments"]);
    assert!(!failed.abandoned);
    assert!(failed.finished_at.is_some());
    let stuck = row(stuck);
    assert_eq!(
        (stuck.status, stuck.abandoned),
        (PassStatus::Failed, true),
        "the scheduler marked it abandoned"
    );
    assert_eq!(row(abandoned).status, PassStatus::Failed);
    let fresh = row(fresh);
    assert_eq!(fresh.status, PassStatus::Running);
    assert!(fresh.finished_at.is_none());
    assert!(
        snapshot.database_time - fresh.heartbeat_at < Duration::seconds(30),
        "the heartbeat is the one the pass beat last"
    );
}

#[tokio::test]
async fn the_snapshot_clock_is_the_databases_and_agrees_with_this_machine_to_the_second() {
    let db = TestDb::create().await;
    let labels = labels();

    let snapshot = snapshot(&db, &request(&labels)).await;

    let skew = (Utc::now() - snapshot.database_time).num_seconds().abs();
    assert!(
        skew < 5,
        "database clock and local clock differ by {skew} s"
    );
}

#[tokio::test]
async fn open_set_runs_are_finished_ones_inside_the_lookback_newest_first_per_endpoint() {
    let db = TestDb::create().await;
    let too_old = db
        .open_set_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Succeeded,
            Duration::days(40),
        )
        .await;
    let failed = db
        .open_set_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Failed,
            Duration::hours(5),
        )
        .await;
    let partial = db
        .open_set_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::PartialSuccess,
            Duration::hours(4),
        )
        .await;
    let succeeded = db
        .open_set_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Succeeded,
            Duration::hours(3),
        )
        .await;
    let in_progress = db
        .open_set_run(
            ACTIVE,
            "customer_invoices",
            RunStatus::Running,
            Duration::hours(1),
        )
        .await;
    let supplier = db
        .open_set_run(
            ACTIVE,
            "supplier_invoices",
            RunStatus::Succeeded,
            Duration::hours(6),
        )
        .await;
    let other_tenant = db
        .open_set_run(
            OTHER,
            "customer_invoices",
            RunStatus::Failed,
            Duration::hours(2),
        )
        .await;
    let not_asked_for = db
        .open_set_run(
            ACTIVE,
            "customers",
            RunStatus::Succeeded,
            Duration::hours(1),
        )
        .await;
    let labels = labels();

    let snapshot = snapshot(&db, &request(&labels)).await;

    let read = |tenant: &str, endpoint: &str| -> Vec<(Uuid, RunStatus)> {
        snapshot
            .runs
            .iter()
            .filter(|run| run.tenant_id == tenant && run.endpoint == endpoint)
            .map(|run| (run.run_id, run.status))
            .collect()
    };
    assert_eq!(
        read(ACTIVE, "customer_invoices:open_set"),
        [
            (succeeded, RunStatus::Succeeded),
            (partial, RunStatus::PartialSuccess)
        ],
        "the two newest finished runs; the failed one, the old one and the running one are out"
    );
    assert_eq!(
        read(ACTIVE, "supplier_invoices:open_set"),
        [(supplier, RunStatus::Succeeded)]
    );
    assert_eq!(
        read(OTHER, "customer_invoices:open_set"),
        [(other_tenant, RunStatus::Failed)]
    );
    let all: Vec<Uuid> = snapshot.runs.iter().map(|run| run.run_id).collect();
    for left_out in [too_old, failed, in_progress, not_asked_for] {
        assert!(!all.contains(&left_out), "{left_out} should not be read");
    }
    assert_eq!(snapshot.runs.len(), 4);
}

#[tokio::test]
async fn a_database_without_the_schema_is_an_error_that_names_the_table() {
    let db = TestDb::create_unmigrated().await;
    let labels = labels();

    let error = snapshot_of(&db.url, &request(&labels))
        .await
        .expect_err("there is nothing to read");

    assert!(error.to_string().contains("scheduler_pass"), "{error}");
}

#[tokio::test]
async fn the_monitors_connection_cannot_write() {
    let db = TestDb::create().await;
    let mut conn = connect_read_only(&db.url).await.expect("connect");

    let insert = sqlx::query(
        "INSERT INTO vsv2.scheduler_pass (pass_id, tier, tenant_id, status) \
         VALUES ($1, 'hourly', $2, 'running')",
    )
    .bind(Uuid::new_v4())
    .bind(ACTIVE)
    .execute(&mut conn)
    .await
    .expect_err("a read-only session refuses an INSERT");
    let update = sqlx::query("UPDATE vsv2.scheduler_pass SET abandoned = TRUE")
        .execute(&mut conn)
        .await
        .expect_err("or an UPDATE");
    let delete = sqlx::query("DELETE FROM vsv2.sync_run")
        .execute(&mut conn)
        .await
        .expect_err("or a DELETE");

    for refusal in [insert, update, delete] {
        assert!(refusal.to_string().contains("read-only"), "{refusal}");
    }
    assert_eq!(db.count("scheduler_pass").await, 0);
}

#[tokio::test]
async fn reading_a_snapshot_leaves_the_connection_read_only_and_the_tables_alone() {
    let db = TestDb::create().await;
    let stuck = db
        .running(ACTIVE, "hourly", Duration::hours(2), Duration::minutes(30))
        .await;
    let labels = labels();
    let mut conn = connect_read_only(&db.url).await.expect("connect");

    let first = read_snapshot(&mut conn, &request(&labels))
        .await
        .expect("first read");
    let second = read_snapshot(&mut conn, &request(&labels))
        .await
        .expect("second read");

    assert_eq!(first.passes, second.passes, "a read changes nothing");
    assert_eq!(first.passes[0].pass_id, stuck);
    assert_eq!(
        first.passes[0].status,
        PassStatus::Running,
        "not abandoned by the read"
    );
}

#[tokio::test]
async fn a_database_url_that_is_not_one_and_a_host_that_refuses_are_errors_without_the_url() {
    let invalid = connect_read_only("this is not a url hunter2pw")
        .await
        .expect_err("not a URL");
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("address").port()
    };
    let started = std::time::Instant::now();
    let refused = connect_read_only(&format!("postgres://u:hunter2pw@127.0.0.1:{port}/db"))
        .await
        .expect_err("nothing listens there");

    assert!(
        invalid.to_string().contains("invalid database URL"),
        "{invalid}"
    );
    assert!(refused.to_string().contains("cannot connect"), "{refused}");
    assert!(
        refused.to_string().contains("Connection refused"),
        "{refused}"
    );
    assert!(!invalid.to_string().contains("hunter2pw"), "{invalid}");
    assert!(!refused.to_string().contains("hunter2pw"), "{refused}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "a refused connection is reported at once, not retried until a timeout"
    );
}
