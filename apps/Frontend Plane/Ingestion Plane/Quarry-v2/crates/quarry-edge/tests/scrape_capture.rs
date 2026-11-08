//! WS13.4b: scrape-time captures on `POST /v1/scrape` (contract and refusal
//! paths that need no browser).
//!
//! The capture switch is read once per process, and this binary never sets
//! it, so these tests run with the committed default (`off`). What they pin:
//! the flag-off refusal is a field error, and the stream route refuses capture
//! formats instead of dropping them. The browser paths (the 501 when the
//! browser cannot capture, the screenshot bytes, the ZDR inline bytes) need a
//! real Chromium and are evidence, not unit tests; see the W2-U45 hand-over.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use quarry_edge::state::AppState;
use quarry_runtime::{fetch::StaticDriver, EventSink};
use serde_json::Value;
use tokio::sync::mpsc;

mod support;

const TEST_BEARER: &str = "scrape-capture-dev-token";

async fn spawn_app() -> SocketAddr {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

    let (tx, mut rx) = mpsc::channel(64);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let event_sink = EventSink::new(tx);

    let static_driver =
        Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let state = AppState {
        #[cfg(feature = "browser-agent")]
        agent_driver: support::governed_agent_driver(),
        ..support::base_state(static_driver, event_sink)
    };
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    addr
}

fn authed(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.bearer_auth(TEST_BEARER)
}

#[tokio::test]
async fn capture_disabled_is_a_field_error() {
    // The committed default is `QUARRY_EDGE__SCRAPE_CAPTURE=off`: a screenshot
    // request is refused at the offending path, before any fetch.
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = authed(client.post(format!("http://{addr}/v1/scrape")).json(
        &serde_json::json!({
            "url": "https://example.com/",
            "formats": ["markdown", { "type": "screenshot", "fullPage": true }],
        }),
    ))
    .send()
    .await
    .unwrap();

    assert_eq!(resp.status().as_u16(), 400, "a capture with the switch off is a 400");
    let body: Value = resp.json().await.unwrap();
    let fields = body["error"]["details"]["fields"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        fields
            .iter()
            .any(|field| field["code"] == "feature_disabled" && field["path"] == "formats"),
        "expected a feature_disabled field error at formats, got {body}"
    );
}

#[tokio::test]
async fn actions_disabled_is_a_field_error() {
    // WS13.5: the committed default is QUARRY_EDGE__SCRAPE_ACTIONS=off. A request
    // with actions is refused at the actions path before any fetch, for every caller.
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = authed(client.post(format!("http://{addr}/v1/scrape")).json(
        &serde_json::json!({
            "url": "https://example.com/",
            "actions": [{ "type": "click", "selector": "#go" }],
        }),
    ))
    .send()
    .await
    .unwrap();

    assert_eq!(resp.status().as_u16(), 400, "actions with the switch off is a 400");
    let body: Value = resp.json().await.unwrap();
    let fields = body["error"]["details"]["fields"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        fields
            .iter()
            .any(|field| field["code"] == "feature_disabled" && field["path"] == "actions"),
        "expected a feature_disabled field error at actions, got {body}"
    );
}

#[tokio::test]
async fn scrape_stream_refuses_capture_formats_instead_of_dropping_them() {
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = authed(client.post(format!("http://{addr}/v1/scrape/stream")).json(
        &serde_json::json!({
            "url": "https://example.com/",
            "formats": ["pdf"],
        }),
    ))
    .send()
    .await
    .unwrap();

    let text = resp.text().await.unwrap();
    assert!(
        text.contains("unsupported_in_stream"),
        "the stream must refuse a pdf format with a typed reason, got {text}"
    );
}

#[tokio::test]
async fn stream_capture_refusal_does_not_touch_a_plain_markdown_stream() {
    // A request without a capture format is not refused by the new check.
    let addr = spawn_app().await;
    let client = reqwest::Client::new();

    let resp = authed(client.post(format!("http://{addr}/v1/scrape/stream")).json(
        &serde_json::json!({
            "url": "not a url",
            "formats": ["markdown"],
        }),
    ))
    .send()
    .await
    .unwrap();

    let text = resp.text().await.unwrap();
    assert!(
        !text.contains("unsupported_in_stream"),
        "a markdown-only stream request is not a capture refusal, got {text}"
    );
}
