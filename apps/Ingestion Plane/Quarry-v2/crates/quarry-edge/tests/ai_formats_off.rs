//! INNH-1 U3: the AI formats with the committed default, the switch OFF.
//!
//! Nothing here sets `QUARRY_EDGE__AI_FORMATS`: this binary is what every stack
//! that has not opted in runs. Even with a Model Plane URL and a credential
//! configured, an AI format is a 400 `feature_disabled` at its own index, nothing
//! is fetched and no model is called. The stream refusal does not depend on the
//! switch. `tests/ai_formats.rs` runs the same router with the switch on.

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

#[path = "support/ai_model_double.rs"]
mod ai_model_double;

use ai_model_double::{standard, Double};

const TEST_BEARER: &str = "ai-formats-off-dev-token";
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

/// An edge that has a model URL and a dev credential, as a stack that is set up
/// for the Model Plane but has not turned the formats on.
async fn spawn_edge(double: &Double) -> Edge {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::remove_var("QUARRY_EDGE__AI_FORMATS");
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    });
    let (tx, mut rx) = mpsc::channel(256);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let fetches = Arc::new(AtomicUsize::new(0));
    let state = AppState {
        model_plane_url: Some(double.uri.clone()),
        model_plane_token: Some("model-dev-token".to_owned()),
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

async fn post(edge: &Edge, path: &str, body: Value) -> (u16, String) {
    let response = reqwest::Client::new()
        .post(format!("http://{}{path}", edge.addr))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    (response.status().as_u16(), response.text().await.unwrap())
}

fn schema() -> Value {
    json!({ "type": "object", "properties": { "title": { "type": "string" } } })
}

#[tokio::test]
async fn every_ai_format_is_a_400_feature_disabled_at_its_own_index() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    for (formats, path) in [
        (
            json!(["markdown", { "type": "json", "schema": schema() }]),
            "formats[1]",
        ),
        (json!(["markdown", "summary"]), "formats[1]"),
        (
            json!(["markdown", { "type": "summary", "max_chars": 300 }]),
            "formats[1]",
        ),
        (
            json!([{ "type": "question", "question": "What?" }]),
            "formats[0]",
        ),
        (
            json!(["html", "markdown", { "type": "highlights", "query": "x" }]),
            "formats[2]",
        ),
    ] {
        let (status, text) = post(
            &edge,
            "/v1/scrape",
            json!({ "url": "https://www.example.com/", "formats": formats }),
        )
        .await;
        assert_eq!(status, 400, "{text}");
        let body: Value = serde_json::from_str(&text).unwrap();
        let fields = body["error"]["details"]["fields"]
            .as_array()
            .expect("fields");
        assert!(
            fields
                .iter()
                .any(|field| field["path"] == path && field["code"] == "feature_disabled"),
            "no feature_disabled at {path}: {text}"
        );
        assert!(
            !text.contains("reserved_format"),
            "the old code is gone: {text}"
        );
    }
    assert_eq!(
        edge.fetches.load(Ordering::SeqCst),
        0,
        "nothing was fetched"
    );
    assert_eq!(model.calls(), 0, "no model was called");
}

#[tokio::test]
async fn a_scrape_without_ai_formats_is_unchanged() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    let (status, text) = post(
        &edge,
        "/v1/scrape",
        json!({ "url": "https://www.example.com/", "formats": ["markdown", "html"] }),
    )
    .await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    for key in ["json", "summary", "answer", "highlights", "ai"] {
        assert!(body["data"].get(key).is_none(), "`{key}` on a plain scrape");
    }
    assert!(body["data"]["markdown"].as_str().is_some());
    assert_eq!(model.calls(), 0);
}

#[tokio::test]
async fn the_stream_refusal_does_not_depend_on_the_switch() {
    let model = ai_model_double::start(standard).await;
    let edge = spawn_edge(&model).await;
    let (status, text) = post(
        &edge,
        "/v1/scrape/stream",
        json!({ "url": "https://www.example.com/", "formats": ["summary"] }),
    )
    .await;
    assert_eq!(
        status, 200,
        "an SSE stream answers 200 and carries the error as an event"
    );
    assert!(
        text.contains("event: error") && text.contains("unsupported_in_stream"),
        "{text}"
    );
    assert_eq!(edge.fetches.load(Ordering::SeqCst), 0);
}
