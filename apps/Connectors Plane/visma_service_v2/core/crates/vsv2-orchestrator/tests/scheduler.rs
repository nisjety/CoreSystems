//! The scheduler runs what is due, records each pass, and retries only what failed.
//!
//! Requires the local compose Postgres (`make up`); see `sync_semantics.rs`. The Redis lease is
//! left out (the scheduler is the only one running), as everywhere else in these tests.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::fake_adapter::{Failure, FakeAdapter, SharedAdapter, Step};
use common::test_db::TestDb;
use common::{account, erp_def};
use sqlx::Row;
use vsv2_common::{TenantId, TenantStatus};
use vsv2_orchestrator::registry::EndpointDefinition;
use vsv2_orchestrator::{
    DispatchTable, EndpointRegistry, NoopAdapter, ScheduledTenant, Scheduler, SyncExecutor, Tier,
    TierSpec,
};

const ACTIVE: &str = "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd";
const HISTORICAL: &str = "9b1fa69b-5568-11ee-960c-025417856183";

struct Fixture {
    db: TestDb,
    fake: Arc<FakeAdapter>,
    exec: Arc<SyncExecutor>,
}

async fn fixture(endpoints: &[&str]) -> Fixture {
    let db = TestDb::create().await;
    let fake = FakeAdapter::single_partition();
    let dir = tempfile::tempdir().expect("tempdir");
    for name in endpoints {
        let def: EndpointDefinition = erp_def(name);
        std::fs::write(
            dir.path().join(format!("{name}.json")),
            serde_json::to_string(&def).expect("serialize"),
        )
        .expect("write endpoint config");
    }
    let registry = EndpointRegistry::load_dir(dir.path()).expect("load registry");
    let exec = Arc::new(SyncExecutor {
        registry,
        dispatch: DispatchTable {
            erp: Box::new(SharedAdapter(Arc::clone(&fake))),
            datamart: Box::new(NoopAdapter {
                source_name: "datamart",
            }),
            expense: Box::new(NoopAdapter {
                source_name: "expense",
            }),
        },
        state: db.state.clone(),
        bq: None,
        local_bronze_enabled: true,
    });
    Fixture { db, fake, exec }
}

fn tenant(id: &str, status: TenantStatus) -> ScheduledTenant {
    ScheduledTenant {
        id: TenantId::new(id),
        name: id[..8].to_string(),
        status,
    }
}

fn spec(tier: Tier, endpoints: &[&str]) -> TierSpec {
    TierSpec {
        tier,
        interval: Duration::from_secs(3600),
        endpoints: endpoints.iter().map(|e| e.to_string()).collect(),
    }
}

fn ok(code: &str) -> Step {
    Step::Records(vec![account(code, "2026-03-10T09:00:00Z")])
}

async fn passes(f: &Fixture) -> Vec<(String, String, String)> {
    sqlx::query("SELECT tier, tenant_id, status FROM vsv2.scheduler_pass ORDER BY started_at")
        .fetch_all(&f.db.state.pg)
        .await
        .expect("query passes")
        .into_iter()
        .map(|r| (r.get("tier"), r.get("tenant_id"), r.get("status")))
        .collect()
}

#[tokio::test]
async fn a_due_tier_runs_once_and_is_not_due_again_straight_away() {
    let f = fixture(&["accounts"]).await;
    f.fake.push("default", ok("1000"));
    let scheduler = Scheduler::new(
        f.exec.clone(),
        vec![tenant(ACTIVE, TenantStatus::Active)],
        2,
        None,
    );
    let specs = [spec(Tier::Hourly, &["accounts"])];

    let first = scheduler.run_due_once(&specs).await.expect("first");
    let second = scheduler.run_due_once(&specs).await.expect("second");

    assert_eq!(first.len(), 1);
    assert_eq!(
        (first[0].attempted, first[0].failed.len(), first[0].records),
        (1, 0, 1)
    );
    assert!(second.is_empty(), "the hour has not passed");
    assert_eq!(
        passes(&f).await,
        [("hourly".into(), ACTIVE.into(), "succeeded".into())]
    );
}

#[tokio::test]
async fn a_failed_endpoint_is_retried_alone_once_the_retry_delay_has_passed() {
    let f = fixture(&["accounts", "branches"]).await;
    f.fake.push("default", ok("1000"));
    f.fake.push("default", Step::Fail(Failure::Upstream(500)));
    let scheduler = Scheduler::new(
        f.exec.clone(),
        vec![tenant(ACTIVE, TenantStatus::Active)],
        2,
        None,
    );
    let specs = [spec(Tier::Hourly, &["accounts", "branches"])];

    let first = scheduler.run_due_once(&specs).await.expect("first");
    assert_eq!(first[0].failed, ["branches"]);
    assert!(
        scheduler
            .run_due_once(&specs)
            .await
            .expect("too soon")
            .is_empty(),
        "15 minutes have not passed"
    );

    sqlx::query("UPDATE vsv2.scheduler_pass SET started_at = started_at - interval '20 minutes'")
        .execute(&f.db.state.pg)
        .await
        .expect("age the pass");
    f.fake.push("default", ok("2000"));
    let retry = scheduler.run_due_once(&specs).await.expect("retry");

    assert_eq!(retry.len(), 1);
    assert_eq!(retry[0].attempted, 1, "only the endpoint that failed");
    assert!(retry[0].failed.is_empty());
    assert_eq!(
        f.fake.calls().len(),
        3,
        "accounts, branches, then branches again"
    );
    let statuses: Vec<String> = passes(&f).await.into_iter().map(|p| p.2).collect();
    assert_eq!(statuses, ["failed", "succeeded"]);
    assert!(
        scheduler
            .run_due_once(&specs)
            .await
            .expect("after retry")
            .is_empty(),
        "the retry does not restart the hour"
    );
}

#[tokio::test]
async fn the_full_pass_ignores_checkpoints_and_the_hourly_pass_does_not() {
    let f = fixture(&["accounts"]).await;
    f.fake.push("default", ok("1000"));
    f.exec
        .sync_one(&TenantId::new(ACTIVE), "accounts")
        .await
        .expect("first sync");
    let scheduler = Scheduler::new(
        f.exec.clone(),
        vec![tenant(ACTIVE, TenantStatus::Active)],
        2,
        None,
    );

    f.fake.push("default", ok("1000"));
    scheduler
        .run_due_once(&[spec(Tier::Hourly, &["accounts"])])
        .await
        .expect("hourly");
    f.fake.push("default", ok("1000"));
    scheduler
        .run_due_once(&[spec(Tier::Full, &["accounts"])])
        .await
        .expect("full");

    let calls = f.fake.calls();
    assert!(
        calls[1].watermark.is_some(),
        "an hourly pass resumes from the checkpoint"
    );
    assert_eq!(calls[2].watermark, None, "a full pass asks for everything");
}

#[tokio::test]
async fn a_historical_tenant_gets_the_full_pass_and_nothing_else() {
    let f = fixture(&["accounts"]).await;
    for code in ["1", "2", "3"] {
        f.fake.push("default", ok(code));
    }
    let scheduler = Scheduler::new(
        f.exec.clone(),
        vec![
            tenant(ACTIVE, TenantStatus::Active),
            tenant(HISTORICAL, TenantStatus::Historical),
        ],
        1,
        None,
    );
    let specs = [
        spec(Tier::Hourly, &["accounts"]),
        spec(Tier::Full, &["accounts"]),
    ];

    let reports = scheduler.run_due_once(&specs).await.expect("run");

    assert_eq!(reports.len(), 3, "hourly for one tenant, full for both");
    let recorded = passes(&f).await;
    assert!(!recorded
        .iter()
        .any(|p| p.0 == "hourly" && p.1 == HISTORICAL));
    assert!(recorded.iter().any(|p| p.0 == "full" && p.1 == HISTORICAL));
}

/// Insert a `running` pass whose heartbeat is `silent_minutes` old.
async fn running_pass(f: &Fixture, tier: &str, retry_of: Option<uuid::Uuid>, silent_minutes: i32) {
    sqlx::query(
        "INSERT INTO vsv2.scheduler_pass (pass_id, tier, tenant_id, status, retry_of, started_at, heartbeat_at) \
         VALUES (gen_random_uuid(), $1, $2, 'running', $3, \
                 NOW() - interval '2 hours', NOW() - make_interval(mins => $4))",
    )
    .bind(tier)
    .bind(ACTIVE)
    .bind(retry_of)
    .bind(silent_minutes)
    .execute(&f.db.state.pg)
    .await
    .expect("insert running pass");
}

#[tokio::test]
async fn a_pass_whose_process_stopped_beating_is_marked_failed_and_abandoned() {
    let f = fixture(&["accounts"]).await;
    running_pass(&f, "hourly", None, 30).await;

    let abandoned = vsv2_state::scheduler_pass::abandon_stale(
        &f.db.state.pg,
        vsv2_orchestrator::scheduler::STALE_PASS_AFTER,
    )
    .await
    .expect("abandon");

    assert_eq!(abandoned, 1);
    assert_eq!(passes(&f).await[0].2, "failed");
}

#[tokio::test]
async fn a_pass_that_is_still_beating_is_not_abandoned_however_old_it_is() {
    let f = fixture(&["accounts"]).await;
    running_pass(&f, "hourly", None, 1).await;

    let abandoned = vsv2_state::scheduler_pass::abandon_stale(
        &f.db.state.pg,
        vsv2_orchestrator::scheduler::STALE_PASS_AFTER,
    )
    .await
    .expect("abandon");

    assert_eq!(abandoned, 0, "started two hours ago, but beat a minute ago");
    assert_eq!(passes(&f).await[0].2, "running");
}

#[tokio::test]
async fn a_pass_a_restart_killed_runs_again_at_the_next_poll_not_after_its_interval() {
    let f = fixture(&["accounts"]).await;
    f.fake.push("default", ok("1000"));
    running_pass(&f, "hourly", None, 30).await;
    let scheduler = Scheduler::new(
        f.exec.clone(),
        vec![tenant(ACTIVE, TenantStatus::Active)],
        2,
        None,
    );

    let reports = scheduler
        .run_due_once(&[spec(Tier::Hourly, &["accounts"])])
        .await
        .expect("run");

    assert_eq!(reports.len(), 1, "the dead pass is replaced straight away");
    let statuses: Vec<String> = passes(&f).await.into_iter().map(|p| p.2).collect();
    assert_eq!(statuses, ["failed", "succeeded"]);
}

#[tokio::test]
async fn a_retry_a_restart_killed_still_knows_which_endpoints_it_was_retrying() {
    let f = fixture(&["accounts", "branches"]).await;
    f.fake.push("default", Step::Fail(Failure::Upstream(500)));
    f.fake.push("default", ok("2000"));
    let scheduler = Scheduler::new(
        f.exec.clone(),
        vec![tenant(ACTIVE, TenantStatus::Active)],
        2,
        None,
    );
    let specs = [spec(Tier::Hourly, &["accounts", "branches"])];
    let first = scheduler.run_due_once(&specs).await.expect("first");
    assert_eq!(first[0].failed, ["accounts"]);
    let failed_pass: uuid::Uuid =
        sqlx::query_scalar("SELECT pass_id FROM vsv2.scheduler_pass WHERE status = 'failed'")
            .fetch_one(&f.db.state.pg)
            .await
            .expect("failed pass");

    // 20 minutes later a retry was running and its process died.
    sqlx::query("UPDATE vsv2.scheduler_pass SET started_at = started_at - interval '20 minutes'")
        .execute(&f.db.state.pg)
        .await
        .expect("age the pass");
    running_pass(&f, "hourly", Some(failed_pass), 30).await;

    f.fake.push("default", ok("1000"));
    let reports = scheduler.run_due_once(&specs).await.expect("run");

    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].attempted, 1,
        "only the endpoint that was failing, not the whole tier"
    );
}
