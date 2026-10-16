//! A crawl or batch the control plane refuses because the organization's queue
//! is full reaches the caller as a typed, retryable 429 with the queue numbers,
//! not as a 502 from an "upstream" that was in fact working.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{http::StatusCode, routing::post, Json, Router};
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod support;

const TEST_BEARER: &str = "job-admission-dev-token";

async fn spawn_control(status: StatusCode, body: Value) -> String {
    let app = Router::new().route(
        "/v1/jobs",
        post(move || {
            let body = body.clone();
            async move { (status, Json(body)) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn spawn_edge(control_url: String) -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        control_base_url: control_url,
        ..support::base_state(driver, EventSink::new(tx))
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

fn backlog_full_body() -> Value {
    json!({
        "data": null,
        "meta": { "request_id": "req_ctl" },
        "error": {
            "code": "RATE_LIMITED",
            "message": "this organization has too many jobs waiting",
            "details": { "reason": "org_backlog_full", "limit": 2, "queued": 2, "retry_after_seconds": 30 },
            "retryable": false
        }
    })
}

#[tokio::test]
async fn handoff_maps_control_429_to_rate_limited_with_details() {
    let control = spawn_control(StatusCode::TOO_MANY_REQUESTS, backlog_full_body()).await;
    let addr = spawn_edge(control).await;

    for (path, body) in [
        ("/v1/crawl", json!({ "url": "https://example.com/" })),
        ("/v1/batch", json!({ "urls": ["https://example.com/a", "https://example.com/b"] })),
    ] {
        let (status, envelope) = post_json(addr, path, body).await;
        assert_eq!(status, 429, "{path}: {envelope}");
        let error = &envelope["error"];
        assert_eq!(error["code"], "RATE_LIMITED", "{path}");
        assert_eq!(error["details"]["reason"], "org_backlog_full", "{path}");
        assert_eq!(error["details"]["limit"], 2, "{path}");
        assert_eq!(error["details"]["queued"], 2, "{path}");
        assert_eq!(error["details"]["retry_after_seconds"], 30, "{path}");
    }
}

#[tokio::test]
async fn another_control_429_or_failure_is_still_an_upstream_error() {
    let control = spawn_control(StatusCode::TOO_MANY_REQUESTS, json!({ "error": "slow down" })).await;
    let addr = spawn_edge(control).await;
    let (status, envelope) = post_json(addr, "/v1/crawl", json!({ "url": "https://example.com/" })).await;
    assert_eq!(status, 502, "{envelope}");
    assert_eq!(envelope["error"]["code"], "UPSTREAM_BLOCKED");
}
