use std::{sync::Arc, time::Duration};

use futures_util::StreamExt;
use quarry_core::output::DriverKind;
use quarry_edge::state::AppState;
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::{artifact_store::InMemoryStore, fetch::FetchResponse, Driver, EventSink};
use quarry_security::preflight::DefaultEngine;
use tokio::sync::mpsc;
use url::Url;

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
    let mut drivers = DriverRegistry::new(DriverKind::Static);
    drivers.register(static_driver.clone());
    let state = AppState {
        readiness: quarry_edge::state::ReadinessState {
            durable: true,
            reason: None,
        },
        receipts: Arc::new(quarry_runtime::InMemoryStepReceiptStore::new()),
        grant_validator: Arc::new(quarry_runtime::NoopGrantValidator),
        require_browser_grants: false,
        driver: static_driver,
        drivers,
        http3: None,
        security: Arc::new(DefaultEngine::new().with_allow_private_hosts(true)),
        artifacts: Arc::new(InMemoryStore::new()),
        control_base_url: String::new(),
        redis: None,
        cache: None,
        event_sink,
        ingest: None,
        profiles: Arc::new(quarry_browser::session::InMemoryProfileStore::new()),
        search: None,
        vector_index: None,
        searxng_url: None,
        model_plane_url: None,
        model_plane_token: None,
        service_token_provider: None,
        answer_pipeline: None,
        local_index: None,
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: None,
        internal_signer: None,
        page_renderer: None,
        visual_processor: None,
        #[cfg(feature = "postgres-queue")]
        event_history: None,
        #[cfg(feature = "postgres-queue")]
        baseline_store: None,
        #[cfg(feature = "postgres-queue")]
        queue_pool: None,
        usage: std::sync::Arc::new(quarry_runtime::NoopUsageMeter),
        #[cfg(feature = "browser-agent")]
        agent_driver: Arc::new(quarry_browser::chromiumoxide::ChromiumoxideDriver::new()),
        #[cfg(feature = "browser-agent")]
        agent_runs: quarry_edge::agent_routes::new_runs(),
    };

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
