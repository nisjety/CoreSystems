//! `/ready` and the local browser (WS3.F): an unhealthy browser takes the edge
//! out of rotation, a healthy one is reported, a driver without a local browser
//! leaves the body exactly as it was, and `/health` never changes.
#![cfg(feature = "browser-agent")]

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quarry_browser::{BrowserHealth, BrowserHealthState, BrowserSession, SlotHealth};
use quarry_core::error::{ErrorCode, QuarryError, QuarryResult};
use quarry_core::lease::BrowserLease;
use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::Value;
use tokio::sync::mpsc;

mod support;

/// A driver that only reports a fixed health.
struct FixedHealth(BrowserHealth);

fn no_browser() -> QuarryError {
    QuarryError::new(ErrorCode::DriverFailed, "test driver has no browser")
}

#[async_trait::async_trait]
impl quarry_browser::BrowserDriver for FixedHealth {
    async fn acquire(&self, _lease: &BrowserLease) -> QuarryResult<BrowserSession> {
        Err(no_browser())
    }
    async fn release(&self, _session: BrowserSession) -> QuarryResult<()> {
        Ok(())
    }
    async fn health(&self) -> BrowserHealth {
        self.0.clone()
    }
    async fn goto(&self, _session: &BrowserSession, _url: &str) -> QuarryResult<()> {
        Err(no_browser())
    }
    async fn content(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
        Err(no_browser())
    }
    async fn screenshot(
        &self,
        _session: &BrowserSession,
        _full_page: bool,
    ) -> QuarryResult<bytes::Bytes> {
        Err(no_browser())
    }
    async fn pdf(&self, _session: &BrowserSession) -> QuarryResult<bytes::Bytes> {
        Err(no_browser())
    }
}

fn slot(status: &str, healthy: Option<bool>) -> SlotHealth {
    SlotHealth {
        slot: 0,
        status: status.to_owned(),
        generation: 1,
        sessions: 2,
        pages: 3,
        healthy,
        probe_failures: if healthy == Some(false) { 2 } else { 0 },
    }
}

async fn spawn_edge(agent_driver: Arc<dyn quarry_browser::BrowserDriver>) -> SocketAddr {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let state = AppState {
        agent_driver,
        ..support::base_state(driver, EventSink::new(tx))
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn get_text(addr: SocketAddr, path: &str) -> (u16, String) {
    let response = reqwest::Client::new()
        .get(format!("http://{addr}{path}"))
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

async fn get_json(addr: SocketAddr, path: &str) -> (u16, Value) {
    let (status, text) = get_text(addr, path).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

#[tokio::test]
async fn unhealthy_browser_answers_503_browser_unhealthy() {
    let health = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(false))]);
    assert_eq!(health.state, BrowserHealthState::Unhealthy);
    let addr = spawn_edge(Arc::new(FixedHealth(health))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["status"], "not_ready");
    assert_eq!(body["reason"], "browser_unhealthy");
    assert_eq!(body["browser"]["state"], "unhealthy");
    assert_eq!(body["browser"]["sessions_open"], 2);
    assert_eq!(body["browser"]["slots"][0]["status"], "ready");
    assert_eq!(body["browser"]["slots"][0]["healthy"], false);
    assert_eq!(get_text(addr, "/health").await.0, 200);

    // A dead browser counts the same.
    let dead = BrowserHealth::from_slots(40, 0, vec![slot("dead", Some(false))]);
    let addr = spawn_edge(Arc::new(FixedHealth(dead))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["reason"], "browser_unhealthy");
    assert_eq!(get_text(addr, "/health").await.0, 200);
}

#[tokio::test]
async fn healthy_and_idle_browsers_answer_200_with_a_browser_field() {
    let healthy = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(true))]);
    assert_eq!(healthy.state, BrowserHealthState::Healthy);
    let addr = spawn_edge(Arc::new(FixedHealth(healthy))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "ready");
    assert_eq!(body["durable"], true);
    assert_eq!(body["browser"]["state"], "healthy");
    assert_eq!(body["browser"]["max_sessions"], 40);
    assert_eq!(body["browser"]["slots"][0]["pages"], 3);
    assert_eq!(get_text(addr, "/health").await.0, 200);

    // Never launched is ready: the first run launches the browser. This is
    // the real driver, constructed and never used.
    let addr = spawn_edge(Arc::new(
        quarry_browser::chromiumoxide::ChromiumoxideDriver::new(),
    ))
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["browser"]["state"], "idle");
    assert_eq!(body["browser"]["sessions_open"], 0);
}

#[tokio::test]
async fn not_applicable_leaves_the_ready_body_byte_identical() {
    let addr = spawn_edge(Arc::new(FixedHealth(BrowserHealth::not_applicable()))).await;
    let (status, text) = get_text(addr, "/ready").await;
    assert_eq!(status, 200);
    assert_eq!(text, r#"{"durable":true,"status":"ready"}"#);
    assert_eq!(get_text(addr, "/health").await.0, 200);
}
