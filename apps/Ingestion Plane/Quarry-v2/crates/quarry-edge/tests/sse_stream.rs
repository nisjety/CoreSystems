use std::sync::Arc;

use futures_util::StreamExt;
use quarry_core::output::DriverKind;
use quarry_runtime::{fetch::FetchResponse, Driver, EventSink};
use tokio::sync::mpsc;
use url::Url;

mod support;

const TEST_BEARER: &str = "sse-stream-dev-token";

/// Returns one canned page instead of performing HTTP.
///
/// This test is about the SSE contract — that a scrape emits `page_fetched`
/// then `artifact_written` — not about fetching. It used to point a real
/// `StaticDriver` at a loopback wiremock server, which stopped working when
/// direct egress gained unconditional DNS pinning: `client_for_decision`
/// preflights every direct fetch through `resolve_public_url`, and loopback is
/// refused there. `DefaultEngine::with_allow_private_hosts(true)` does NOT
/// reach that check — it configures the preflight ENGINE, a separate layer —
/// so the escape hatch this test relied on no longer covers the driver.
///
/// Stubbing the driver is the right fix rather than relaxing the guard: the
/// guard is deliberately unconditional, and a test that needs no network
/// should not have one. `a_direct_fetch_to_a_loopback_target_is_refused_before_any_request`
/// in quarry-runtime covers the refusal itself.
struct StubPageDriver;

#[async_trait::async_trait]
impl Driver for StubPageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, url: &Url) -> quarry_core::error::QuarryResult<FetchResponse> {
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".to_string(), "text/html".to_string())],
            body: b"<html><body><a href=\"/x\">x</a></body></html>".to_vec(),
            duration_ms: 1,
        })
    }
}

#[tokio::test]
async fn sse_streams_page_fetched_and_artifact_written() {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

    let (tx, mut rx) = mpsc::channel(1024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let event_sink = EventSink::new(tx);

    let static_driver = Arc::new(StubPageDriver);
    let state = support::base_state(static_driver, event_sink);

    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{addr}/v1/scrape/stream"))
        .bearer_auth(TEST_BEARER)
        .json(&serde_json::json!({ "url": "https://stub.example/" }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    let mut body = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        body.push_str(std::str::from_utf8(&chunk.unwrap()).unwrap());
        if body.contains("data: done") {
            break;
        }
    }

    assert!(body.contains("event: page_fetched"), "body: {body}");
    assert!(body.contains("event: artifact_written"), "body: {body}");
}
