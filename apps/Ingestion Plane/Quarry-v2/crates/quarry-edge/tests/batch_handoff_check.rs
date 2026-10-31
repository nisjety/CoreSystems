//! WS14.S5: `POST /v1/batch` checks every URL before a job exists.
//!
//! A control stub records the job the edge hands off, so these tests assert what
//! the orchestrator would receive: nothing is forwarded when every URL is refused
//! (400 with `details.rejected`), and a partial batch forwards only the accepted
//! URLs and reports the rest in `HandoffAck.rejected`.

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

const TEST_BEARER: &str = "batch-handoff-check-dev-token";

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

async fn batch(addr: SocketAddr, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{addr}/v1/batch"))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.json().await.unwrap())
}

#[tokio::test]
async fn a_batch_with_no_crawlable_url_is_refused_and_nothing_is_forwarded() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, body) = batch(
        addr,
        json!({ "urls": ["file:///etc/passwd", "ftp://example.com/x"] }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let rejected = body["error"]["details"]["rejected"].as_array().unwrap();
    assert_eq!(rejected.len(), 2);
    assert!(rejected.iter().all(|r| r["code"] == "not_crawlable"));
    assert!(recorded.lock().unwrap().is_empty(), "no job is created");
}

#[tokio::test]
async fn a_partial_batch_forwards_only_the_accepted_urls_and_reports_the_rest() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, body) = batch(
        addr,
        json!({
            "urls": [
                "https://example.com/docs/a",
                "https://example.com/docs/a#x",
                "file:///etc/passwd",
                "http://127.0.0.1/private/secret",
                "https://example.com/docs/b"
            ]
        }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let rejected = body["data"]["rejected"].as_array().unwrap();
    let codes: Vec<(&str, &str)> = rejected
        .iter()
        .map(|r| (r["code"].as_str().unwrap(), r["url"].as_str().unwrap()))
        .collect();
    assert!(codes.contains(&("duplicate", "https://example.com/docs/a#x")), "{codes:?}");
    assert!(codes.contains(&("not_crawlable", "file:///etc/passwd")), "{codes:?}");
    assert!(
        codes.contains(&("security_rejected", "http://127.0.0.1/private/secret")),
        "{codes:?}"
    );

    let jobs = recorded.lock().unwrap();
    assert_eq!(jobs.len(), 1);
    let params = &jobs[0]["params"];
    assert_eq!(
        params["urls"],
        json!(["https://example.com/docs/a", "https://example.com/docs/b"])
    );
    assert_eq!(params["robots"], "respect");
}

#[tokio::test]
async fn a_clean_batch_has_no_rejected_list() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;

    let (status, body) = batch(addr, json!({ "urls": ["https://example.com/docs/a"] })).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["data"].get("rejected").is_none(), "{body}");
    assert_eq!(recorded.lock().unwrap().len(), 1);
}
