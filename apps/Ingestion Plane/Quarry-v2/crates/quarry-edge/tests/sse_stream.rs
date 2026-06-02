use std::{sync::Arc, time::Duration};

use futures_util::StreamExt;
use quarry_core::output::DriverKind;
use quarry_edge::state::AppState;
use quarry_runtime::driver_registry::DriverRegistry;
use quarry_runtime::{artifact_store::InMemoryStore, fetch::StaticDriver, EventSink};
use quarry_security::preflight::DefaultEngine;
use tokio::sync::mpsc;
use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

const TEST_BEARER: &str = "sse-stream-dev-token";

#[tokio::test]
async fn sse_streams_page_fetched_and_artifact_written() {
    std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<html><body><a href=\"/x\">x</a></body></html>"),
        )
        .mount(&server)
        .await;

    let (tx, mut rx) = mpsc::channel(1024);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let event_sink = EventSink::new(tx);

    let static_driver =
        Arc::new(StaticDriver::new(Duration::from_secs(10), "quarry-test").unwrap());
    let mut drivers = DriverRegistry::new(DriverKind::Static);
    drivers.register(static_driver.clone());
    let state = AppState {
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
        searxng_url: None,
        model_plane_url: None,
        model_plane_token: None,
        answer_pipeline: None,
        local_index: None,
        policy: quarry_runtime::RunPolicy::default(),
        scheduler: None,
        internal_signer: None,
        #[cfg(feature = "postgres-queue")]
        event_history: None,
        usage: std::sync::Arc::new(quarry_runtime::NoopUsageMeter),
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
        .json(&serde_json::json!({ "url": server.uri() }))
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
