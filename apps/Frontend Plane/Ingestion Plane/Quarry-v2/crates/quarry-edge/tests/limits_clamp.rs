//! WS6.D-s: the organization's remaining allowance clamps a crawl and a batch.
//!
//! A control stub records every job the edge hands off, and a fake limits
//! resolver stands in for a source that reports allowances (only the static
//! source is built, and it reports none). The tests pin what the orchestrator
//! would receive and what the caller is told: a crawl never runs past what is
//! left, a spent allowance is a 402 with no job, a batch keeps the first URLs
//! that fit, and an unreadable source follows its fail mode.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{extract::State, routing::post, Json, Router};
use quarry_core::error::QuarryError;
use quarry_edge::limits::{
    Allowance, BackendError, FailMode, LimitsBackend, LimitsOrigin, LimitsResolver, OrgLimits,
    PlanCaps, StaticFloor, WithFailMode,
};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod support;

const TEST_BEARER: &str = "limits-clamp-dev-token";

type Recorded = Arc<Mutex<Vec<Value>>>;

/// A resolver that reports fixed remaining allowances for every organization.
struct Fixed(OrgLimits);

#[async_trait]
impl LimitsResolver for Fixed {
    async fn org_limits(&self, _org_id: &str) -> Result<OrgLimits, QuarryError> {
        Ok(self.0.clone())
    }
}

/// A source that is always down.
struct Down;

#[async_trait]
impl LimitsBackend for Down {
    async fn read(&self, _org_id: &str) -> Result<OrgLimits, BackendError> {
        Err(BackendError("unreachable"))
    }
}

fn entitled(pages: Allowance, urls: Allowance) -> OrgLimits {
    OrgLimits {
        origin: LimitsOrigin::Entitlement,
        crawl_pages: pages,
        batch_urls: urls,
        browser_seconds_agent: Allowance::Unconfigured,
        caps: PlanCaps::default(),
    }
}

async fn spawn_control() -> (String, Recorded) {
    async fn create_job(State(recorded): State<Recorded>, Json(body): Json<Value>) -> Json<Value> {
        recorded.lock().unwrap().push(body);
        Json(json!({ "data": { "id": "job_stub", "run_id": null } }))
    }
    let recorded: Recorded = Arc::default();
    let app = Router::new()
        .route("/v1/jobs", post(create_job))
        .with_state(recorded.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}"), recorded)
}

async fn spawn_edge(control_url: String, limits: Arc<dyn LimitsResolver>) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let static_driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: control_url,
        limits,
        ..support::base_state(static_driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn post_json(addr: SocketAddr, path: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}{path}"))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

fn urls(count: usize) -> Value {
    Value::Array(
        (0..count)
            .map(|index| json!(format!("https://example.com/page-{index}")))
            .collect(),
    )
}

#[tokio::test]
async fn a_crawl_is_clamped_to_the_remaining_page_allowance() {
    let (control, recorded) = spawn_control().await;
    let limits = Arc::new(Fixed(entitled(Allowance::Remaining(25), Allowance::Unconfigured)));
    let addr = spawn_edge(control, limits).await;

    let (status, body) = post_json(addr, "/v1/crawl", json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 200, "{body}");
    let pages = &body["data"]["limits"]["max_pages"];
    assert_eq!(pages["effective"], 25);
    assert_eq!(pages["remaining"], 25);
    assert_eq!(pages["clamped_by"], json!(["default", "entitlement"]));

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["params"]["max_pages"], 25);
}

#[tokio::test]
async fn a_request_below_the_remainder_is_used_as_asked() {
    let (control, recorded) = spawn_control().await;
    let limits = Arc::new(Fixed(entitled(Allowance::Remaining(500), Allowance::Unconfigured)));
    let addr = spawn_edge(control, limits).await;

    let (status, body) = post_json(
        addr,
        "/v1/crawl",
        json!({ "url": "https://example.com/", "max_pages": 10 }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["limits"]["max_pages"]["effective"], 10);
    assert_eq!(body["data"]["limits"]["max_pages"]["remaining"], 500);
    assert_eq!(body["data"]["limits"]["max_pages"]["clamped_by"], json!([]));
    assert_eq!(recorded.lock().unwrap()[0]["params"]["max_pages"], 10);
}

#[tokio::test]
async fn a_spent_page_allowance_is_a_402_and_creates_no_job() {
    let (control, recorded) = spawn_control().await;
    let limits = Arc::new(Fixed(entitled(Allowance::Remaining(0), Allowance::Unconfigured)));
    let addr = spawn_edge(control, limits).await;

    let (status, body) = post_json(addr, "/v1/crawl", json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 402, "{body}");
    assert_eq!(body["error"]["code"], "BUDGET_EXHAUSTED");
    let details = &body["error"]["details"];
    assert_eq!(details["dimension"], "org_allowance");
    assert_eq!(details["metric"], "quarry.crawl.page");
    assert_eq!(details["limit_source"], "entitlement");
    assert!(recorded.lock().unwrap().is_empty(), "no job is handed off");
}

#[tokio::test]
async fn a_batch_keeps_the_first_urls_that_fit() {
    let (control, recorded) = spawn_control().await;
    let limits = Arc::new(Fixed(entitled(Allowance::Unconfigured, Allowance::Remaining(25))));
    let addr = spawn_edge(control, limits).await;

    let (status, body) = post_json(addr, "/v1/batch", json!({ "urls": urls(40) })).await;
    assert_eq!(status, 200, "{body}");
    let bound = &body["data"]["limits"]["urls"];
    assert_eq!(bound["requested"], 40);
    assert_eq!(bound["accepted"], 25);
    assert_eq!(bound["dropped"], 15);
    assert_eq!(bound["remaining"], 25);
    assert_eq!(bound["clamped_by"], json!(["entitlement"]));
    // Counts only: the acknowledgement names no URL.
    assert!(!bound.to_string().contains("example.com"));

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs.len(), 1);
    let sent = jobs[0]["params"]["urls"].as_array().unwrap();
    assert_eq!(sent.len(), 25);
    assert_eq!(sent[0], "https://example.com/page-0");
    assert_eq!(sent[24], "https://example.com/page-24");
}

#[tokio::test]
async fn a_spent_batch_allowance_is_a_402_and_creates_no_job() {
    let (control, recorded) = spawn_control().await;
    let limits = Arc::new(Fixed(entitled(Allowance::Unconfigured, Allowance::Remaining(0))));
    let addr = spawn_edge(control, limits).await;

    let (status, body) = post_json(addr, "/v1/batch", json!({ "urls": urls(3) })).await;
    assert_eq!(status, 402, "{body}");
    assert_eq!(body["error"]["details"]["dimension"], "org_allowance");
    assert_eq!(body["error"]["details"]["metric"], "quarry.batch.url");
    assert!(recorded.lock().unwrap().is_empty(), "no job is handed off");
}

#[tokio::test]
async fn the_static_floor_changes_nothing() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control, Arc::new(StaticFloor)).await;

    let (status, body) = post_json(addr, "/v1/crawl", json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 200, "{body}");
    let pages = &body["data"]["limits"]["max_pages"];
    assert_eq!(pages["effective"], 1000);
    assert_eq!(pages["clamped_by"], json!(["default"]));
    assert!(pages.get("remaining").is_none(), "an unconfigured allowance reports no remainder");

    let (status, body) = post_json(addr, "/v1/batch", json!({ "urls": urls(40) })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["limits"]["urls"]["accepted"], 40);
    assert_eq!(body["data"]["limits"]["urls"]["dropped"], 0);
    assert_eq!(body["data"]["limits"]["urls"]["clamped_by"], json!([]));
    assert_eq!(recorded.lock().unwrap()[1]["params"]["urls"].as_array().unwrap().len(), 40);
}

#[tokio::test]
async fn an_unreadable_source_serves_the_floor_in_floor_mode() {
    let (control, recorded) = spawn_control().await;
    let limits = Arc::new(WithFailMode::new(Down, FailMode::Floor));
    let addr = spawn_edge(control, limits).await;

    let (status, body) = post_json(addr, "/v1/crawl", json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["limits"]["max_pages"]["effective"], 1000);
    assert_eq!(recorded.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn an_unreadable_source_refuses_in_closed_mode_with_no_job() {
    let (control, recorded) = spawn_control().await;
    let limits = Arc::new(WithFailMode::new(Down, FailMode::Closed));
    let addr = spawn_edge(control, limits).await;

    let (status, body) = post_json(addr, "/v1/crawl", json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["error"]["code"], "RUNTIME_NOT_READY");
    assert_eq!(body["error"]["details"]["reason"], "limits_unreadable");
    assert!(recorded.lock().unwrap().is_empty(), "no job is handed off");
}
