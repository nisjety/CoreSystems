//! `vsv2 monitor`, the real binary, against a real Postgres.
//!
//! What they guard: a healthy pipeline exits 0, and each way a pipeline goes wrong (a stale tier, a
//! pass whose heartbeat stopped, an endpoint that keeps failing, a tenant that never ran, a
//! scheduler that never started, an unreadable database) shows up in the report with the exit code
//! a cron job can act on; no password reaches stdout or stderr; and the command writes nothing.
//! The webhook is in `monitor_webhook.rs`, the reads behind the report in `monitor_state.rs`.
//!
//! Requires the local compose Postgres (`make up`); see `common/mod.rs`.

mod common;

use chrono::Duration;
use common::{
    finding, pipeline_with_one_problem, with_severity, Fixture, Run, TestDb, ACTIVE, HISTORICAL,
    OPEN_SET, OTHER,
};
use vsv2_state::RunStatus;

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(tenant, check)| (tenant.to_string(), check.to_string()))
        .collect()
}

// ── healthy ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn a_healthy_pipeline_exits_0_and_every_finding_is_ok() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 0, "{}", outcome.everything());
    let report = outcome.json();
    assert_eq!(report["summary"]["status"], "ok");
    assert_eq!(report["summary"]["exit_code"], 0);
    assert_eq!(report["summary"]["problems"], 0);
    assert_eq!(report["summary"]["warnings"], 0);
    assert_eq!(report["summary"]["tenants"], 2);
    assert!(report["summary"]["error"].is_null());
    assert_eq!(
        report["findings"].as_array().expect("findings").len(),
        12,
        "nine checks for the active tenant, three for the historical one"
    );
    assert!(with_severity(&report, "problem").is_empty());
    assert!(with_severity(&report, "warning").is_empty());
}

#[tokio::test]
async fn the_text_report_of_a_healthy_pipeline_says_ok_and_exits_0() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;
    let text = Run {
        flags: Vec::new(),
        ..Run::new(&fixture, &db.url)
    }
    .go()
    .await;

    assert_eq!(outcome.code, 0);
    assert_eq!(text.code, 0, "{}", text.everything());
    assert!(
        text.stdout.starts_with("vsv2 monitor: OK at "),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("OK: Aqua Pharma AS, Consult AS"),
        "{}",
        text.stdout
    );
    assert!(text.stderr.is_empty(), "{}", text.stderr);
}

// ── what goes wrong ────────────────────────────────────────────────────────

#[tokio::test]
async fn a_stale_hourly_pass_is_a_problem_and_exits_1() {
    let db = TestDb::create().await;
    db.succeeded(ACTIVE, "hourly", Duration::hours(5)).await;
    db.succeeded(ACTIVE, "daily", Duration::hours(3)).await;
    db.succeeded(ACTIVE, "full", Duration::days(2)).await;
    for endpoint in OPEN_SET {
        db.open_set_run(ACTIVE, endpoint, RunStatus::Succeeded, Duration::hours(5))
            .await;
    }
    db.healthy_historical(HISTORICAL).await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    let report = outcome.json();
    assert_eq!(
        with_severity(&report, "problem"),
        pairs(&[(ACTIVE, "freshness_hourly")])
    );
    let stale = finding(&report, ACTIVE, "freshness_hourly");
    assert_eq!(stale["tenant_name"], "Aqua Pharma AS");
    let detail = stale["detail"].as_str().expect("detail");
    assert!(detail.contains("started 5 h ago"), "{detail}");
    assert!(detail.contains("limit 3 h"), "{detail}");
    assert_eq!(report["summary"]["status"], "problem");
    assert_eq!(report["summary"]["exit_code"], 1);
}

#[tokio::test]
async fn a_stale_hourly_pass_is_judged_by_the_limit_on_the_command_line() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    let fixture = Fixture::standard();

    let lenient = Run::new(&fixture, &db.url).go().await;
    let strict = Run::new(&fixture, &db.url)
        .flags(&["--hourly-max-age-mins", "10"])
        .go()
        .await;

    assert_eq!(lenient.code, 0);
    assert_eq!(strict.code, 1, "{}", strict.everything());
    assert_eq!(
        with_severity(&strict.json(), "problem"),
        pairs(&[(ACTIVE, "freshness_hourly")])
    );
}

#[tokio::test]
async fn a_running_pass_whose_heartbeat_stopped_is_stuck_and_exits_1() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    db.running(ACTIVE, "daily", Duration::hours(2), Duration::minutes(30))
        .await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    let report = outcome.json();
    assert_eq!(
        with_severity(&report, "problem"),
        pairs(&[(ACTIVE, "stuck_pass")])
    );
    let detail = finding(&report, ACTIVE, "stuck_pass")["detail"]
        .as_str()
        .expect("detail")
        .to_string();
    assert!(detail.contains("daily pass begun"), "{detail}");
    assert!(detail.contains("no heartbeat for 30 min"), "{detail}");
}

#[tokio::test]
async fn a_running_pass_that_still_beats_is_not_stuck() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    db.running(ACTIVE, "full", Duration::hours(30), Duration::seconds(20))
        .await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 0, "{}", outcome.everything());
}

#[tokio::test]
async fn an_endpoint_that_failed_in_three_passes_in_a_row_is_a_problem_and_exits_1() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    for minutes_ago in [18, 12, 6] {
        db.failed(
            ACTIVE,
            "hourly",
            Duration::minutes(minutes_ago),
            &["customers"],
        )
        .await;
    }
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    let report = outcome.json();
    assert_eq!(
        with_severity(&report, "problem"),
        pairs(&[(ACTIVE, "failures_hourly")])
    );
    let detail = finding(&report, ACTIVE, "failures_hourly")["detail"]
        .as_str()
        .expect("detail")
        .to_string();
    assert!(detail.contains("customers (3 passes in a row)"), "{detail}");
}

#[tokio::test]
async fn a_single_failure_is_only_a_warning_which_strict_turns_into_exit_1() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    db.failed(
        ACTIVE,
        "hourly",
        Duration::minutes(6),
        &["customers", "shipments"],
    )
    .await;
    let fixture = Fixture::standard();

    let lenient = Run::new(&fixture, &db.url).go().await;
    let strict = Run::new(&fixture, &db.url).flags(&["--strict"]).go().await;

    assert_eq!(lenient.code, 0, "{}", lenient.everything());
    let report = lenient.json();
    assert_eq!(
        with_severity(&report, "warning"),
        pairs(&[(ACTIVE, "failures_hourly")])
    );
    assert_eq!(report["summary"]["status"], "warning");
    let detail = finding(&report, ACTIVE, "failures_hourly")["detail"]
        .as_str()
        .expect("detail")
        .to_string();
    assert!(detail.contains("customers (1 pass in a row)"), "{detail}");
    assert!(detail.contains("shipments (1 pass in a row)"), "{detail}");
    assert_eq!(strict.code, 1, "{}", strict.everything());
    assert_eq!(strict.json()["summary"]["status"], "warning");
    assert_eq!(strict.json()["summary"]["exit_code"], 1);
}

#[tokio::test]
async fn the_number_of_failures_in_a_row_is_set_on_the_command_line() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    db.failed(ACTIVE, "hourly", Duration::minutes(10), &["customers"])
        .await;
    db.failed(ACTIVE, "hourly", Duration::minutes(5), &["customers"])
        .await;
    let fixture = Fixture::standard();

    let default = Run::new(&fixture, &db.url).go().await;
    let touchy = Run::new(&fixture, &db.url)
        .flags(&["--repeat-failures", "2"])
        .go()
        .await;

    assert_eq!(default.code, 0, "two in a row, three are needed");
    assert_eq!(touchy.code, 1, "{}", touchy.everything());
}

#[tokio::test]
async fn an_open_set_run_that_failed_is_a_warning_and_three_in_a_row_are_a_problem() {
    let db = TestDb::create().await;
    db.healthy_historical(HISTORICAL).await;
    db.succeeded(ACTIVE, "hourly", Duration::minutes(20)).await;
    db.succeeded(ACTIVE, "daily", Duration::hours(3)).await;
    db.succeeded(ACTIVE, "full", Duration::days(2)).await;
    db.open_set_run(
        ACTIVE,
        "supplier_invoices",
        RunStatus::Succeeded,
        Duration::hours(5),
    )
    .await;
    db.open_set_run(
        ACTIVE,
        "customer_invoices",
        RunStatus::Failed,
        Duration::hours(5),
    )
    .await;
    let fixture = Fixture::standard();

    let once = Run::new(&fixture, &db.url).go().await;
    db.open_set_run(
        ACTIVE,
        "customer_invoices",
        RunStatus::Failed,
        Duration::hours(3),
    )
    .await;
    db.open_set_run(
        ACTIVE,
        "customer_invoices",
        RunStatus::Failed,
        Duration::hours(1),
    )
    .await;
    let thrice = Run::new(&fixture, &db.url).go().await;

    assert_eq!(once.code, 0, "{}", once.everything());
    assert_eq!(
        with_severity(&once.json(), "warning"),
        pairs(&[(ACTIVE, "open_set_customer_invoices")])
    );
    assert_eq!(thrice.code, 1, "{}", thrice.everything());
    assert_eq!(
        with_severity(&thrice.json(), "problem"),
        pairs(&[(ACTIVE, "open_set_customer_invoices")])
    );
}

#[tokio::test]
async fn a_missing_open_set_run_is_a_problem_once_the_first_hourly_pass_is_a_day_old() {
    let db = TestDb::create().await;
    db.healthy_historical(HISTORICAL).await;
    db.succeeded(ACTIVE, "hourly", Duration::hours(30)).await;
    db.succeeded(ACTIVE, "hourly", Duration::minutes(20)).await;
    db.succeeded(ACTIVE, "daily", Duration::hours(3)).await;
    db.succeeded(ACTIVE, "full", Duration::days(2)).await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    assert_eq!(
        with_severity(&outcome.json(), "problem"),
        pairs(&[
            (ACTIVE, "open_set_customer_invoices"),
            (ACTIVE, "open_set_supplier_invoices")
        ])
    );
}

#[tokio::test]
async fn a_tenant_that_never_ran_is_a_problem_and_the_others_are_judged_on_their_own() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    let fixture = Fixture::two_active();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    let report = outcome.json();
    assert_eq!(
        with_severity(&report, "problem"),
        pairs(&[(OTHER, "tenant_has_run")])
    );
    let never = finding(&report, OTHER, "tenant_has_run");
    assert_eq!(never["tenant_name"], "Aqua Pharma Group AS");
    assert!(never["observed_at"].is_null());
    assert!(never["detail"]
        .as_str()
        .expect("detail")
        .starts_with("never ran"));
}

#[tokio::test]
async fn a_database_without_a_single_pass_says_the_scheduler_has_not_run_once() {
    let db = TestDb::create().await;
    let fixture = Fixture::new(&[
        (ACTIVE, "Aqua Pharma AS", false),
        (OTHER, "Aqua Pharma Group AS", false),
        (HISTORICAL, "Consult AS", true),
    ]);

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    let report = outcome.json();
    let findings = report["findings"].as_array().expect("findings");
    assert_eq!(
        findings.len(),
        1,
        "one finding, not one per tenant: {report:#}"
    );
    assert_eq!(findings[0]["check"], "scheduler_ran");
    assert_eq!(findings[0]["severity"], "problem");
    assert!(findings[0]["tenant_id"].is_null());
    assert!(findings[0]["detail"]
        .as_str()
        .expect("detail")
        .contains("has not run yet"));
    assert_eq!(report["summary"]["tenants"], 3);
}

#[tokio::test]
async fn a_historical_tenant_is_only_expected_to_run_the_full_pass() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.succeeded(HISTORICAL, "full", Duration::days(9)).await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    assert_eq!(
        with_severity(&outcome.json(), "problem"),
        pairs(&[(HISTORICAL, "freshness_full")]),
        "nine days without the weekly pass; no hourly or daily pass is expected"
    );
}

// ── the state cannot be read ───────────────────────────────────────────────

#[tokio::test]
async fn an_unreachable_database_exits_2_says_so_and_keeps_the_password_out() {
    let fixture = Fixture::standard();
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("address").port()
    };
    let dsn = format!("postgres://visma_v2:hunter2pw@127.0.0.1:{port}/visma_v2");

    let outcome = Run::new(&fixture, &dsn).go().await;

    assert_eq!(outcome.code, 2, "{}", outcome.everything());
    let report = outcome.json();
    assert_eq!(report["summary"]["status"], "unreadable");
    assert_eq!(report["summary"]["exit_code"], 2);
    assert!(report["summary"]["error"]
        .as_str()
        .expect("the reason")
        .contains("cannot connect"));
    assert_eq!(report["findings"][0]["check"], "state_database");
    assert!(
        !outcome.everything().contains("hunter2pw"),
        "{}",
        outcome.everything()
    );
    assert!(
        !outcome.everything().contains(&dsn),
        "{}",
        outcome.everything()
    );
}

#[tokio::test]
async fn a_database_without_the_schema_exits_2_instead_of_guessing() {
    let db = TestDb::create_unmigrated().await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 2, "{}", outcome.everything());
    let error = outcome.json()["summary"]["error"]
        .as_str()
        .expect("the reason")
        .to_string();
    assert!(error.contains("scheduler_pass"), "{error}");
}

#[tokio::test]
async fn a_missing_database_url_exits_2_and_names_the_variable() {
    let fixture = Fixture::standard();
    let db_url = "unused";

    let outcome = Run::new(&fixture, db_url).without_database().go().await;

    assert_eq!(outcome.code, 2, "{}", outcome.everything());
    let error = outcome.json()["summary"]["error"]
        .as_str()
        .expect("the reason")
        .to_string();
    assert!(error.contains("VSV2_DATABASE_URL is not set"), "{error}");
}

#[tokio::test]
async fn an_unreadable_state_in_text_format_says_so_on_stdout() {
    let fixture = Fixture::standard();

    let outcome = Run {
        flags: Vec::new(),
        ..Run::new(&fixture, "unused").without_database()
    }
    .go()
    .await;

    assert_eq!(outcome.code, 2);
    assert!(
        outcome.stdout.starts_with("vsv2 monitor: UNREADABLE at "),
        "{}",
        outcome.stdout
    );
    assert!(outcome
        .stdout
        .contains("reason: VSV2_DATABASE_URL is not set"));
}

#[tokio::test]
async fn a_tenant_directory_that_cannot_be_read_exits_2_on_stderr() {
    let db = TestDb::create().await;
    let fixture = Fixture::standard();
    let empty = tempfile::tempdir().expect("tempdir");

    let missing = Run::new(&fixture, &db.url)
        .tenants_dir("/nonexistent/vsv2/tenants")
        .go()
        .await;
    let none = Run::new(&fixture, &db.url)
        .tenants_dir(empty.path().to_str().expect("utf-8 path"))
        .go()
        .await;

    assert_eq!(missing.code, 2, "{}", missing.everything());
    assert!(
        missing.stderr.contains("/nonexistent/vsv2/tenants"),
        "{}",
        missing.stderr
    );
    assert_eq!(none.code, 2, "{}", none.everything());
    assert!(none.stderr.contains("no tenant files"), "{}", none.stderr);
}

// ── it only reads ──────────────────────────────────────────────────────────

#[tokio::test]
async fn the_monitor_writes_nothing() {
    let (db, fixture) = pipeline_with_one_problem().await;
    db.running(ACTIVE, "daily", Duration::hours(2), Duration::minutes(30))
        .await;
    let before = (
        db.count("scheduler_pass").await,
        db.count("sync_run").await,
        db.count("sync_checkpoint").await,
        db.count("sync_page_metric").await,
    );
    let heartbeat_before: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT MAX(heartbeat_at) FROM vsv2.scheduler_pass WHERE status = 'running'",
    )
    .fetch_one(&db.state.pg)
    .await
    .expect("read a heartbeat");

    let outcome = Run::new(&fixture, &db.url).go().await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    let after = (
        db.count("scheduler_pass").await,
        db.count("sync_run").await,
        db.count("sync_checkpoint").await,
        db.count("sync_page_metric").await,
    );
    assert_eq!(before, after);
    let still_running: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM vsv2.scheduler_pass \
         WHERE status = 'running' AND heartbeat_at = $1 AND NOT abandoned",
    )
    .bind(heartbeat_before)
    .fetch_one(&db.state.pg)
    .await
    .expect("count running passes");
    assert_eq!(still_running, 1, "a stuck pass is reported, not abandoned");
}

#[tokio::test]
async fn the_report_is_on_stdout_alone_and_clean_json_under_the_default_log_filter() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    let fixture = Fixture::standard();

    let outcome = Run::new(&fixture, &db.url).go().await;

    // `Outcome::json` fails on any log line mixed into stdout; `Run` removes RUST_LOG, so the
    // binary's own default filter (info, with vsv2 at debug) is in force.
    assert_eq!(outcome.json()["summary"]["status"], "ok");
    assert!(outcome.stderr.is_empty(), "{}", outcome.stderr);
}
