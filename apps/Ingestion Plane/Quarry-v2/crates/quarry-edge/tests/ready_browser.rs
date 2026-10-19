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

/// An edge whose registry also carries a scrape-path browser driver, as
/// main.rs builds it (a second local browser behind `BrowserDriverAdapter`).
async fn spawn_edge_with_scrape_browser(
    agent_driver: Arc<dyn quarry_browser::BrowserDriver>,
    scrape_driver: Arc<dyn quarry_browser::BrowserDriver + Send + Sync>,
) -> SocketAddr {
    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
    let base = support::base_state(driver, EventSink::new(tx));
    let mut drivers = base.drivers.clone();
    drivers.register(Arc::new(quarry_runtime::BrowserDriverAdapter::new(
        scrape_driver,
        Arc::new(quarry_runtime::lease_pool::RuntimeLeasePool::new(2)),
    )));
    let state = AppState {
        agent_driver,
        drivers,
        ..base
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

#[tokio::test]
async fn an_unhealthy_scrape_browser_answers_503_beside_a_healthy_agent_browser() {
    let healthy = BrowserHealth::from_slots(40, 1, vec![slot("ready", Some(true))]);
    let unhealthy = BrowserHealth::from_slots(40, 0, vec![slot("dead", Some(false))]);
    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(healthy.clone())),
        Arc::new(FixedHealth(unhealthy.clone())),
    )
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert_eq!(body["reason"], "browser_unhealthy");
    assert_eq!(body["browser"]["state"], "healthy");
    assert_eq!(body["scrape_browser"]["state"], "unhealthy");
    assert_eq!(get_text(addr, "/health").await.0, 200);

    // Both healthy: 200 with both objects.
    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(healthy.clone())),
        Arc::new(FixedHealth(healthy)),
    )
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["scrape_browser"]["state"], "healthy");

    // A scrape driver without a local browser (a remote provider) is left out.
    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(unhealthy)),
        Arc::new(FixedHealth(BrowserHealth::not_applicable())),
    )
    .await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 503, "{body}");
    assert!(body.get("scrape_browser").is_none(), "{body}");
}

#[tokio::test]
async fn a_degraded_browser_is_ready() {
    let degraded = BrowserHealth::from_slots(
        40,
        1,
        vec![
            slot("ready", Some(true)),
            SlotHealth {
                slot: 1,
                ..slot("dead", Some(false))
            },
        ],
    );
    assert_eq!(degraded.state, BrowserHealthState::Degraded);
    let addr = spawn_edge(Arc::new(FixedHealth(degraded))).await;
    let (status, body) = get_json(addr, "/ready").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["browser"]["state"], "degraded");
}

/// The `/ready` body exactly as docs/openapi.yaml documents it: every key
/// the edge sends is documented (`deny_unknown_fields`), every required
/// key is there, and the state values are the documented ones.
mod documented {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Ready {
        pub status: String,
        pub durable: Option<bool>,
        pub reason: Option<String>,
        pub browser: Option<Browser>,
        pub scrape_browser: Option<Browser>,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Browser {
        pub state: State,
        pub max_sessions: u64,
        pub sessions_open: u64,
        pub slots: Vec<Slot>,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum State {
        Idle,
        Healthy,
        Degraded,
        Unhealthy,
    }

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct Slot {
        pub slot: u64,
        pub status: SlotStatus,
        pub generation: u64,
        pub sessions: u64,
        pub pages: u64,
        pub healthy: Option<bool>,
        pub probe_failures: u64,
    }

    #[derive(Debug, Deserialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum SlotStatus {
        Starting,
        Ready,
        Retiring,
        Dead,
    }

    /// The keys above, as they must appear in the openapi `/ready` block.
    pub const KEYS: &[&str] = &[
        "status",
        "durable",
        "reason",
        "browser",
        "scrape_browser",
        "state",
        "max_sessions",
        "sessions_open",
        "slots",
        "slot",
        "generation",
        "sessions",
        "pages",
        "healthy",
        "probe_failures",
    ];
}

#[tokio::test]
async fn ready_bodies_decode_into_exactly_the_documented_keys() {
    let decode = |text: &str| -> documented::Ready {
        serde_json::from_str(text).unwrap_or_else(|e| panic!("undocumented /ready body {text}: {e}"))
    };
    let healthy = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(true))]);
    let unhealthy = BrowserHealth::from_slots(40, 2, vec![slot("ready", Some(false))]);

    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(healthy.clone())),
        Arc::new(FixedHealth(healthy)),
    )
    .await;
    let (status, text) = get_text(addr, "/ready").await;
    assert_eq!(status, 200);
    let ready = decode(&text);
    assert_eq!(ready.status, "ready");
    assert_eq!(ready.durable, Some(true));
    assert!(ready.reason.is_none());
    let browser = ready.browser.expect("browser");
    assert_eq!(browser.state, documented::State::Healthy);
    assert_eq!(browser.slots[0].status, documented::SlotStatus::Ready);
    assert_eq!(
        ready.scrape_browser.expect("scrape_browser").state,
        documented::State::Healthy
    );

    let addr = spawn_edge_with_scrape_browser(
        Arc::new(FixedHealth(unhealthy)),
        Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
    )
    .await;
    let (status, text) = get_text(addr, "/ready").await;
    assert_eq!(status, 503);
    let ready = decode(&text);
    assert_eq!(ready.status, "not_ready");
    assert_eq!(ready.reason.as_deref(), Some("browser_unhealthy"));
    assert_eq!(ready.browser.expect("browser").state, documented::State::Unhealthy);
    // The real driver, never launched.
    assert_eq!(
        ready.scrape_browser.expect("scrape_browser").state,
        documented::State::Idle
    );

    let addr = spawn_edge(Arc::new(FixedHealth(BrowserHealth::not_applicable()))).await;
    let (status, text) = get_text(addr, "/ready").await;
    assert_eq!(status, 200);
    let ready = decode(&text);
    assert!(ready.browser.is_none() && ready.scrape_browser.is_none());

    // And the openapi /ready block names each of these keys, as JSON, not
    // the old text/plain body.
    let openapi = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/openapi.yaml"
    ))
    .expect("docs/openapi.yaml");
    let start = openapi.find("\n  /ready:\n").expect("the /ready path");
    let rest = &openapi[start + 1..];
    let end = rest[2..].find("\n  /").map_or(rest.len(), |i| i + 2);
    let block = &rest[..end];
    assert!(!block.contains("text/plain"), "/ready is documented as JSON");
    for key in documented::KEYS {
        assert!(
            block.contains(&format!(" {key}:")),
            "docs/openapi.yaml /ready does not document `{key}`"
        );
    }
    for value in ["idle", "healthy", "degraded", "unhealthy", "browser_unhealthy", "draining"] {
        assert!(block.contains(value), "docs/openapi.yaml /ready does not name `{value}`");
    }
}
