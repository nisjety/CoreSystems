//! `sitemap: include|only` needs the crawl scope (INNH-6a).
//!
//! A crawl run with `QUARRY_EDGE__CRAWL_SCOPE_MODE=off` is a legacy crawl: the orchestrator
//! sends no scope to check a sitemap's URLs against, so the handoff refuses the option
//! instead of seeding unchecked URLs. The scope switch is read once per process, hence
//! this binary of its own.

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use axum::{extract::State, routing::post, Json, Router};
use quarry_edge::state::AppState;
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod support;

use support::fixture_site::FixtureSiteDriver;

const TEST_BEARER: &str = "crawl-scope-off-dev-token";

type Recorded = Arc<Mutex<Vec<Value>>>;

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
    std::env::set_var("QUARRY_EDGE__CRAWL_SITEMAP", "on");
    std::env::set_var("QUARRY_EDGE__CRAWL_SCOPE_MODE", "off");
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let state = AppState {
        control_base_url: control_url,
        ..support::base_state(
            Arc::new(FixtureSiteDriver::standard_site()),
            EventSink::new(tx),
        )
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
async fn sitemap_needs_the_crawl_scope() {
    let (control, recorded) = spawn_control().await;
    let addr = spawn_edge(control).await;
    for mode in ["include", "only"] {
        let (status, body) = crawl(
            addr,
            json!({ "url": "https://example.com/", "sitemap": mode }),
        )
        .await;
        assert_eq!(status, 400, "{body}");
        let field = &body["error"]["details"]["fields"][0];
        assert_eq!(field["path"], "sitemap");
        assert_eq!(field["code"], "unsupported_option");
    }
    assert!(
        recorded.lock().unwrap().is_empty(),
        "nothing was handed off"
    );

    // A crawl that does not ask for the sitemap still runs with the scope off.
    let (status, body) = crawl(
        addr,
        json!({ "url": "https://example.com/", "sitemap": "skip" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(recorded.lock().unwrap().len(), 1);
}
