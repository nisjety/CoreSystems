//! The alert of `vsv2 monitor`: the webhook is posted only for a problem, with the message Slack
//! and Teams take, and when it cannot be delivered nothing changes but a line on stderr, which
//! never carries the URL.
//!
//! Requires the local compose Postgres (`make up`); see `common/mod.rs`.

mod common;

use chrono::Duration;
use common::{pipeline_with_one_problem, Fixture, Run, TestDb, ACTIVE, HISTORICAL};
use mockito::Matcher;

const TOKEN: &str = "topsecretT0kenValue";

const PROBLEM_BODY: &str = r#"^\{"text":"vsv2 monitor: 1 problem, 0 warnings \(2 tenants\)\\n- Aqua Pharma AS: freshness_hourly: last succeeded hourly pass started 5 h ago .*limit 3 h.*"\}$"#;

#[tokio::test]
async fn a_problem_is_posted_once_to_the_webhook_in_the_environment_and_the_url_stays_private() {
    let (db, fixture) = pipeline_with_one_problem().await;
    let mut server = mockito::Server::new_async().await;
    let path = format!("/services/T0/B0/{TOKEN}");
    let hook = server
        .mock("POST", path.as_str())
        .match_header("content-type", "application/json")
        .match_body(Matcher::Regex(PROBLEM_BODY.to_string()))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}{path}", server.url());

    let outcome = Run::new(&fixture, &db.url)
        .env("VSV2_MONITOR_WEBHOOK_URL", &url)
        .go()
        .await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    hook.assert_async().await;
    assert!(
        !outcome.everything().contains(&url),
        "{}",
        outcome.everything()
    );
    assert!(
        !outcome.everything().contains(TOKEN),
        "{}",
        outcome.everything()
    );
    assert!(
        outcome.stderr.is_empty(),
        "a delivered alert is silent: {}",
        outcome.stderr
    );
}

#[tokio::test]
async fn the_webhook_can_also_be_given_on_the_command_line() {
    let (db, fixture) = pipeline_with_one_problem().await;
    let mut server = mockito::Server::new_async().await;
    let hook = server
        .mock("POST", "/hook")
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/hook", server.url());

    let outcome = Run::new(&fixture, &db.url)
        .flags(&["--webhook-url", &url])
        .go()
        .await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    hook.assert_async().await;
}

#[tokio::test]
async fn nothing_is_posted_when_the_pipeline_is_healthy() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    let fixture = Fixture::standard();
    let mut server = mockito::Server::new_async().await;
    let hook = server
        .mock("POST", "/hook")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    let url = format!("{}/hook", server.url());

    let outcome = Run::new(&fixture, &db.url)
        .env("VSV2_MONITOR_WEBHOOK_URL", &url)
        .go()
        .await;

    assert_eq!(outcome.code, 0, "{}", outcome.everything());
    hook.assert_async().await;
}

#[tokio::test]
async fn nothing_is_posted_for_a_warning_even_under_strict() {
    let db = TestDb::create().await;
    db.healthy(ACTIVE).await;
    db.healthy_historical(HISTORICAL).await;
    db.failed(ACTIVE, "hourly", Duration::minutes(6), &["customers"])
        .await;
    let fixture = Fixture::standard();
    let mut server = mockito::Server::new_async().await;
    let hook = server
        .mock("POST", "/hook")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    let url = format!("{}/hook", server.url());

    let outcome = Run::new(&fixture, &db.url)
        .flags(&["--strict"])
        .env("VSV2_MONITOR_WEBHOOK_URL", &url)
        .go()
        .await;

    assert_eq!(outcome.code, 1, "strict fails the run");
    hook.assert_async().await;
}

#[tokio::test]
async fn a_webhook_that_answers_with_an_error_changes_nothing_but_stderr() {
    let (db, fixture) = pipeline_with_one_problem().await;
    let mut server = mockito::Server::new_async().await;
    let path = format!("/services/T0/B0/{TOKEN}");
    let hook = server
        .mock("POST", path.as_str())
        .with_status(500)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}{path}", server.url());

    let outcome = Run::new(&fixture, &db.url)
        .env("VSV2_MONITOR_WEBHOOK_URL", &url)
        .go()
        .await;

    assert_eq!(
        outcome.code, 1,
        "the exit code is the pipeline's, not the webhook's"
    );
    hook.assert_async().await;
    assert!(
        outcome
            .stderr
            .contains("the alert was not delivered: the webhook answered HTTP 500"),
        "{}",
        outcome.stderr
    );
    assert!(
        !outcome.everything().contains(&url),
        "{}",
        outcome.everything()
    );
    assert!(
        !outcome.everything().contains(TOKEN),
        "{}",
        outcome.everything()
    );
    assert_eq!(
        outcome.json()["summary"]["status"],
        "problem",
        "the report is intact"
    );
}

#[tokio::test]
async fn a_webhook_that_cannot_be_reached_changes_nothing_but_stderr() {
    let (db, fixture) = pipeline_with_one_problem().await;
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("address").port()
    };
    let url = format!("http://127.0.0.1:{port}/services/T0/B0/{TOKEN}");

    let outcome = Run::new(&fixture, &db.url)
        .env("VSV2_MONITOR_WEBHOOK_URL", &url)
        .go()
        .await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    assert!(
        outcome
            .stderr
            .contains("the alert was not delivered: could not connect"),
        "{}",
        outcome.stderr
    );
    assert!(
        !outcome.everything().contains(&url),
        "{}",
        outcome.everything()
    );
    assert!(
        !outcome.everything().contains(TOKEN),
        "{}",
        outcome.everything()
    );
}

#[tokio::test]
async fn a_webhook_url_that_is_not_a_url_is_reported_without_echoing_it() {
    let (db, fixture) = pipeline_with_one_problem().await;
    let bad = format!("not-a-url-{TOKEN}");

    let outcome = Run::new(&fixture, &db.url)
        .env("VSV2_MONITOR_WEBHOOK_URL", &bad)
        .go()
        .await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    assert!(
        outcome.stderr.contains("not a valid URL"),
        "{}",
        outcome.stderr
    );
    assert!(
        !outcome.everything().contains(TOKEN),
        "{}",
        outcome.everything()
    );
}

#[tokio::test]
async fn a_blank_webhook_variable_is_the_same_as_none() {
    let (db, fixture) = pipeline_with_one_problem().await;

    let outcome = Run::new(&fixture, &db.url)
        .env("VSV2_MONITOR_WEBHOOK_URL", "")
        .go()
        .await;

    assert_eq!(outcome.code, 1, "{}", outcome.everything());
    assert!(outcome.stderr.is_empty(), "{}", outcome.stderr);
}

#[tokio::test]
async fn an_unreadable_database_is_posted_too_without_the_reason() {
    let fixture = Fixture::standard();
    let mut server = mockito::Server::new_async().await;
    let hook = server
        .mock("POST", "/hook")
        .match_body(Matcher::Regex(
            r#"^\{"text":"vsv2 monitor: 1 problem, 0 warnings \(2 tenants\)\\n- pipeline: state_database: the state database cannot be read, so nothing was checked"\}$"#
                .to_string(),
        ))
        .with_status(200)
        .expect(1)
        .create_async()
        .await;
    let url = format!("{}/hook", server.url());

    let outcome = Run::new(&fixture, "unused")
        .without_database()
        .env("VSV2_MONITOR_WEBHOOK_URL", &url)
        .go()
        .await;

    assert_eq!(outcome.code, 2, "{}", outcome.everything());
    hook.assert_async().await;
}
