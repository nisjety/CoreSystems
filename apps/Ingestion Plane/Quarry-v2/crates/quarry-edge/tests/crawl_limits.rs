//! `POST /v1/crawl` bounds every crawl before the orchestrator sees it.
//!
//! A control stub records the job the edge hands off, so these tests assert
//! what the orchestrator would receive: a finite page and depth bound for every
//! request (zero used to mean unlimited), clamped values reported back to the
//! caller, and ZDR callers still refused.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{extract::State, routing::post, Json, Router};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod support;

const TEST_BEARER: &str = "crawl-limits-dev-token";

type Recorded = Arc<Mutex<Vec<Value>>>;

/// Stand-in for quarry-control's `POST /v1/jobs`: records the posted body and
/// answers with a job id.
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

async fn spawn_edge(control_url: String) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let static_driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: control_url,
        ..support::base_state(static_driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn crawl(addr: SocketAddr, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/crawl"))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

#[tokio::test]
async fn a_crawl_with_no_limits_takes_the_defaults() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, body) = crawl(addr, json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 200, "{body}");
    let limits = &body["data"]["limits"];
    assert_eq!(limits["max_pages"]["effective"], 1000);
    assert_eq!(limits["max_pages"]["clamped_by"], json!(["default"]));
    assert_eq!(limits["max_depth"]["effective"], 5);

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs.len(), 1);
    let params = &jobs[0]["params"];
    assert_eq!(params["max_pages"], 1000);
    assert_eq!(params["max_depth"], 5);
    assert!(params["max_pages_requested"].is_null());
}

#[tokio::test]
async fn zero_max_pages_is_the_default_never_unlimited() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, body) = crawl(
        addr,
        json!({ "url": "https://example.com/", "max_pages": 0, "max_depth": 0 }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["limits"]["max_pages"]["effective"], 1000);
    assert_eq!(body["data"]["limits"]["max_pages"]["requested"], 0);

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs[0]["params"]["max_pages"], 1000);
    assert_eq!(jobs[0]["params"]["max_depth"], 5);
    assert_eq!(jobs[0]["params"]["max_pages_requested"], 0);
}

#[tokio::test]
async fn a_request_above_the_ceiling_is_clamped_and_reported() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, body) = crawl(
        addr,
        json!({ "url": "https://example.com/", "max_pages": 900000, "max_depth": 99 }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let limits = &body["data"]["limits"];
    assert_eq!(limits["max_pages"]["effective"], 5000);
    assert_eq!(limits["max_pages"]["requested"], 900000);
    assert_eq!(limits["max_pages"]["clamped_by"], json!(["hard"]));
    assert_eq!(limits["max_depth"]["effective"], 10);

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs[0]["params"]["max_pages"], 5000);
    assert_eq!(jobs[0]["params"]["max_depth"], 10);
}

#[tokio::test]
async fn an_in_range_request_is_forwarded_as_asked() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, body) = crawl(
        addr,
        json!({ "url": "https://example.com/", "max_pages": 25, "max_depth": 2 }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["data"]["limits"]["max_pages"]["clamped_by"], json!([]));

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs[0]["params"]["max_pages"], 25);
    assert_eq!(jobs[0]["params"]["max_depth"], 2);
}

#[tokio::test]
async fn a_zdr_crawl_is_still_refused() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, _) = crawl(addr, json!({ "url": "https://example.com/", "zdr": true })).await;
    assert_eq!(status, 403);
    assert!(recorded.lock().unwrap().is_empty(), "no job is handed off");
}
