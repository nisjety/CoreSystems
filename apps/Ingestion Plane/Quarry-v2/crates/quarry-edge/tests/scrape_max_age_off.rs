//! INNH-7: `max_age` with the committed default, the switch OFF.
//!
//! Nothing here sets `QUARRY_EDGE__SCRAPE_MAX_AGE`: this binary is what every stack
//! that has not opted in runs. A request that carries `max_age` is a 400
//! `feature_disabled` at `max_age` (never a 200 that silently refetches), nothing is
//! fetched, and a request without it behaves exactly as before. `tests/scrape_max_age.rs`
//! runs the same router with the switch on. No Redis is needed.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};

use async_trait::async_trait;
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::state::AppState;
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;

mod support;

const TEST_BEARER: &str = "scrape-max-age-off-dev-token";
const PAGE: &str = "<html><head><title>Page</title></head><body><main><h1>Heading</h1><p>Some body text for the switch-off test page, long enough to be extracted as its main content.</p></main></body></html>";

struct PageDriver {
    fetches: Arc<AtomicUsize>,
}

#[async_trait]
impl Driver for PageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        self.fetches.fetch_add(1, Ordering::SeqCst);
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: PAGE.as_bytes().to_vec(),
            duration_ms: 2,
            served_by: DriverKind::Static,
            captures: None,
        })
    }
}

struct Edge {
    addr: SocketAddr,
    fetches: Arc<AtomicUsize>,
}

async fn spawn_edge() -> Edge {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::remove_var("QUARRY_EDGE__SCRAPE_MAX_AGE");
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    });
    let (tx, mut rx) = mpsc::channel(256);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let fetches = Arc::new(AtomicUsize::new(0));
    let state = AppState {
        #[cfg(feature = "browser-agent")]
        agent_driver: support::governed_agent_driver(),
        ..support::base_state(
            Arc::new(PageDriver {
                fetches: fetches.clone(),
            }),
            EventSink::new(tx),
        )
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Edge { addr, fetches }
}

async fn post(edge: &Edge, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{}/v1/scrape", edge.addr))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

#[tokio::test]
async fn max_age_is_a_400_feature_disabled_at_max_age_and_nothing_is_fetched() {
    let edge = spawn_edge().await;
    for key in ["max_age", "maxAge"] {
        let (status, body) = post(
            &edge,
            json!({ "url": "https://www.example.com/", key: 60_000 }),
        )
        .await;
        assert_eq!(status, 400, "{key}: {body}");
        let fields = body["error"]["details"]["fields"]
            .as_array()
            .expect("fields");
        assert!(
            fields
                .iter()
                .any(|field| field["path"] == "max_age" && field["code"] == "feature_disabled"),
            "no feature_disabled at max_age for {key}: {body}"
        );
    }
    // Zero is a value too: the switch is about the key, not the number.
    let (status, _) = post(&edge, json!({ "url": "https://www.example.com/", "max_age": 0 })).await;
    assert_eq!(status, 400);
    assert_eq!(edge.fetches.load(Ordering::SeqCst), 0, "nothing was fetched");
}

#[tokio::test]
async fn a_scrape_without_max_age_is_unchanged() {
    let edge = spawn_edge().await;
    let (status, body) = post(
        &edge,
        json!({ "url": "https://www.example.com/", "formats": ["markdown"] }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let options = &body["data"]["effective_options"];
    assert_eq!(options["cache_state"], "bypass");
    for key in ["max_age", "cached_at", "cache_age_ms"] {
        assert!(options.get(key).is_none(), "`{key}` on a plain scrape: {body}");
    }
    assert!(body["data"]["warnings"].as_array().map_or(true, Vec::is_empty));
    assert_eq!(edge.fetches.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_cache_policy_with_only_max_age_s_is_accepted() {
    // The mode used to be required, so this was a 400 invalid_request. It is
    // independent of the max_age switch.
    let edge = spawn_edge().await;
    let (status, body) = post(
        &edge,
        json!({ "url": "https://www.example.com/", "cache": { "max_age_s": 60 } }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(edge.fetches.load(Ordering::SeqCst), 1);
}
