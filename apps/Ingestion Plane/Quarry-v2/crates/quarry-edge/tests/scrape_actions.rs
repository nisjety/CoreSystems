//! WS13.5: scrape-time `actions[]` on `POST /v1/scrape`, through the real router.
//!
//! The switch is read once per process, so this binary turns it on (`on`) before
//! the first request; `tests/scrape_capture.rs` runs with the committed default
//! (`off`) and pins the refusal. No Chromium here: the browser driver is a double
//! that hands back the captures a real browser fetch would carry after the actions
//! ran (outcomes in request order, plus the bytes of a screenshot and a PDF). What
//! this file pins is everything the edge does with them: the wire shape
//! (`data.actions[]`), the stored or inline bytes, the org stamp, the ZDR floor, the
//! refusals, and that a default request is unchanged. The live-browser half is
//! `tests/scrape_actions_live.rs`: an evidence-stack run, ignored by default. The real-token
//! admission (agent label, service principals, the `user` mode) is in
//! `tests/scrape_actions_real_token.rs` and `tests/scrape_actions_real_token_user_mode.rs`.

use std::{net::SocketAddr, sync::Arc};

use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::scrape_actions::{ActionError, ActionOutcome, ActionOutput, ActionStatus};
use quarry_core::QuarryResult;
use quarry_edge::state::AppState;
use quarry_runtime::artifact_store::{ArtifactStore, InMemoryStore};
use quarry_runtime::capture::{ActionOutputBytes, PageCaptures};
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;

mod support;

const TEST_BEARER: &str = "scrape-actions-dev-token";
const ORG: &str = "org_placeholder";
const PAGE: &str = "<html><head><title>Actions page</title></head><body><main><h1>After</h1><p>Some body text for the actions test page, long enough to be extracted as its main content.</p></main></body></html>";
const SECRET_SENTINEL: &str = "SECRET_SENTINEL_123";

/// A driver of either kind. The browser double carries `captures`; the static one
/// answers a plain page.
struct Fake {
    kind: DriverKind,
    captures: Option<PageCaptures>,
    failure: Option<QuarryError>,
}

#[async_trait]
impl Driver for Fake {
    fn kind(&self) -> DriverKind {
        self.kind
    }
    async fn fetch(&self, url: &Url) -> QuarryResult<FetchResponse> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        Ok(FetchResponse {
            status: 200,
            final_url: url.clone(),
            headers: vec![("content-type".into(), "text/html; charset=utf-8".into())],
            body: PAGE.as_bytes().to_vec(),
            duration_ms: 2,
            served_by: self.kind,
            captures: self.captures.clone().map(Box::new),
        })
    }
}

fn outcome(index: usize, kind: &str, output: Option<ActionOutput>) -> ActionOutcome {
    ActionOutcome {
        index,
        kind: kind.to_owned(),
        status: ActionStatus::Ok,
        duration_ms: 3,
        error: None,
        output,
    }
}

/// What the browser fetch carries after `click, screenshot, pdf`.
fn captures_after_click_screenshot_pdf() -> PageCaptures {
    PageCaptures {
        actions: vec![
            outcome(0, "click", None),
            outcome(
                1,
                "screenshot",
                Some(ActionOutput {
                    kind: "screenshot".into(),
                    content_type: "image/png".into(),
                    bytes: 40,
                    width: Some(800),
                    height: Some(600),
                }),
            ),
            outcome(
                2,
                "pdf",
                Some(ActionOutput {
                    kind: "pdf".into(),
                    content_type: "application/pdf".into(),
                    bytes: 24,
                    width: None,
                    height: None,
                }),
            ),
        ],
        action_outputs: vec![
            None,
            Some(ActionOutputBytes {
                content_type: "image/png".into(),
                bytes: vec![0x89; 40],
            }),
            Some(ActionOutputBytes {
                content_type: "application/pdf".into(),
                bytes: vec![0x25; 24],
            }),
        ],
        ..PageCaptures::default()
    }
}

fn click_screenshot_pdf() -> Value {
    json!([
        { "type": "click", "selector": "#swap" },
        { "type": "screenshot" },
        { "type": "pdf" },
    ])
}

struct Edge {
    addr: SocketAddr,
    store: Arc<InMemoryStore>,
    events: Arc<std::sync::Mutex<Vec<quarry_core::event::Event>>>,
}

/// An edge with a static double and, when `browser` is `Some`, a browser double.
async fn spawn_edge(browser: Option<Fake>) -> Edge {
    // Read once per process by `scrape_actions::actions_mode`: set before any request.
    static MODE: std::sync::Once = std::sync::Once::new();
    MODE.call_once(|| {
        std::env::set_var("QUARRY_EDGE__SCRAPE_ACTIONS", "on");
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    });

    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tx, mut rx) = mpsc::channel(256);
    let sink = events.clone();
    tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            sink.lock().unwrap().push(event);
        }
    });
    let static_driver = Arc::new(Fake {
        kind: DriverKind::Static,
        captures: None,
        failure: None,
    });
    let store = Arc::new(InMemoryStore::new());
    let mut state = AppState {
        artifacts: store.clone(),
        #[cfg(feature = "browser-agent")]
        agent_driver: support::governed_agent_driver(),
        ..support::base_state(static_driver, EventSink::new(tx))
    };
    if let Some(browser) = browser {
        state.drivers.register(Arc::new(browser));
    }
    let app = quarry_edge::routes::router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Edge { addr, store, events }
}

fn browser_with(captures: PageCaptures) -> Fake {
    Fake {
        kind: DriverKind::Browser,
        captures: Some(captures),
        failure: None,
    }
}

async fn post(edge: &Edge, path: &str, body: Value) -> (u16, Value) {
    let response = reqwest::Client::new()
        .post(format!("http://{}{path}", edge.addr))
        .bearer_auth(TEST_BEARER)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

async fn get_bytes(edge: &Edge, path: &str) -> (u16, Vec<u8>) {
    let response = reqwest::Client::new()
        .get(format!("http://{}{path}", edge.addr))
        .bearer_auth(TEST_BEARER)
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    (status, response.bytes().await.unwrap().to_vec())
}

fn scrape_body(actions: Value) -> Value {
    json!({
        "url": "https://www.example.com/",
        "formats": ["markdown"],
        "actions": actions,
    })
}

#[tokio::test]
async fn the_wire_carries_ordered_actions_with_stored_artifacts_and_reads_back_by_id() {
    let edge = spawn_edge(Some(browser_with(captures_after_click_screenshot_pdf()))).await;
    let (status, body) = post(&edge, "/v1/scrape", scrape_body(click_screenshot_pdf())).await;
    assert_eq!(status, 200, "{body}");

    let actions = body["data"]["actions"].as_array().expect("data.actions[]");
    let order: Vec<_> = actions
        .iter()
        .map(|entry| (entry["index"].as_u64().unwrap(), entry["type"].as_str().unwrap()))
        .collect();
    assert_eq!(order, vec![(0, "click"), (1, "screenshot"), (2, "pdf")]);
    assert!(actions.iter().all(|entry| entry["status"] == "ok" && entry["duration_ms"] == 3));
    assert!(actions[0].get("output").is_none(), "a click has no output");

    let shot = &actions[1]["output"];
    assert_eq!(shot["kind"], "screenshot");
    assert_eq!(shot["content_type"], "image/png");
    assert_eq!((shot["bytes"].as_u64(), shot["width"].as_u64(), shot["height"].as_u64()), (Some(40), Some(800), Some(600)));
    assert!(shot.get("data_base64").is_none(), "stored, so not inline");
    let pdf = &actions[2]["output"];
    assert_eq!(pdf["content_type"], "application/pdf");

    // Each stored output reads back, byte for byte, as the request's own org.
    for (output, expected, content_type) in [(shot, 40usize, "image/png"), (pdf, 24usize, "application/pdf")] {
        let id = output["artifact_id"].as_str().expect("a stored output has an artifact id");
        let (status, bytes) = get_bytes(&edge, &format!("/v1/artifacts/{id}")).await;
        assert_eq!(status, 200);
        assert_eq!(bytes.len(), expected);
        let _ = content_type;
    }
    // Another org gets nothing for the same id.
    let id: quarry_core::ids::kinds::ArtifactKind =
        shot["artifact_id"].as_str().unwrap().parse().unwrap();
    assert!(edge.store.get("org_other", &id).await.is_err());

    // The stored record answers `GET /v1/scrape/{id}` with the same entries.
    let scrape_id = body["data"]["scrape_id"].as_str().expect("a record id").to_owned();
    let (status, bytes) = get_bytes(&edge, &format!("/v1/scrape/{scrape_id}")).await;
    assert_eq!(status, 200);
    let record: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(record["data"]["actions"], body["data"]["actions"]);

    // The effective options echo counts only: never a selector or a text.
    let text = body.to_string();
    assert!(!text.contains("#swap"), "a selector never reaches the response");
}

#[tokio::test]
async fn zdr_returns_action_bytes_inline_and_stores_nothing() {
    let edge = spawn_edge(Some(browser_with(captures_after_click_screenshot_pdf()))).await;
    let mut request = scrape_body(click_screenshot_pdf());
    request["zdr"] = json!(true);
    let (status, body) = post(&edge, "/v1/scrape", request).await;
    assert_eq!(status, 200, "{body}");

    let actions = body["data"]["actions"].as_array().expect("data.actions[]");
    let shot = &actions[1]["output"];
    assert!(shot["data_base64"].as_str().is_some_and(|data| !data.is_empty()));
    assert!(shot.get("artifact_id").is_none(), "no stored reference under ZDR");
    assert_eq!(shot["bytes"], 40);
    assert!(actions[2]["output"]["data_base64"].as_str().is_some());
    assert!(body["data"].get("scrape_id").is_none(), "no record under ZDR");

    // The ZDR floor: nothing in the store, no artifact event.
    let listed = edge
        .store
        .list(ORG, &quarry_core::pagination::ListFilter::default())
        .await
        .unwrap();
    assert!(listed.items.is_empty(), "ZDR wrote an artifact: {:?}", listed.items.len());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let events = edge.events.lock().unwrap();
    assert!(
        events
            .iter()
            .all(|event| !matches!(event.event_type, quarry_core::event::EventType::ArtifactWritten)),
        "no artifact event under ZDR"
    );
}

#[tokio::test]
async fn a_default_scrape_has_no_actions_and_is_unchanged() {
    let edge = spawn_edge(Some(browser_with(captures_after_click_screenshot_pdf()))).await;
    let (status, body) = post(&edge, "/v1/scrape", json!({ "url": "https://www.example.com/" })).await;
    assert_eq!(status, 200, "{body}");
    assert!(body["data"].get("actions").is_none(), "no actions key on a request without actions");
    assert!(body["data"].get("captures").is_none());
    // The static double served it: the browser (and its canned actions) was never asked.
    assert_eq!(body["data"]["driver"]["kind"].as_str(), Some("static"));
}

#[tokio::test]
async fn agent_labelled_user_token_cannot_send_actions() {
    let edge = spawn_edge(Some(browser_with(captures_after_click_screenshot_pdf()))).await;
    let mut labelled = scrape_body(click_screenshot_pdf());
    labelled["initiated_by"] = json!("agent");
    let (status, body) = post(&edge, "/v1/scrape", labelled).await;
    assert_eq!(status, 403, "{body}");
    assert_eq!(body["error"]["details"]["reason"], "scrape_interact_scope_required");

    // The same body without the label is served, and an unknown label never widens or 400s.
    let (status, _) = post(&edge, "/v1/scrape", scrape_body(click_screenshot_pdf())).await;
    assert_eq!(status, 200);
    let mut odd = scrape_body(click_screenshot_pdf());
    odd["initiated_by"] = json!("somebody");
    let (status, _) = post(&edge, "/v1/scrape", odd).await;
    assert_eq!(status, 200, "an unknown label is ignored");
}

#[tokio::test]
async fn actions_with_ingest_true_is_a_field_error() {
    let edge = spawn_edge(Some(browser_with(captures_after_click_screenshot_pdf()))).await;
    let mut request = scrape_body(click_screenshot_pdf());
    request["ingest"] = json!(true);
    let (status, body) = post(&edge, "/v1/scrape", request).await;
    assert_eq!(status, 400, "{body}");
    let fields = body["error"]["details"]["fields"].as_array().cloned().unwrap_or_default();
    assert!(fields.iter().any(|field| field["code"] == "not_supported_with_actions"), "{body}");
}

#[tokio::test]
async fn a_denied_action_is_a_field_error_at_its_entry() {
    let edge = spawn_edge(Some(browser_with(PageCaptures::default()))).await;
    let (status, body) = post(
        &edge,
        "/v1/scrape",
        scrape_body(json!([{ "type": "click", "selector": "#a" }, { "type": "evaluate", "script": "1" }])),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    let fields = body["error"]["details"]["fields"].as_array().cloned().unwrap_or_default();
    assert!(
        fields.iter().any(|field| field["code"] == "action_denied" && field["path"] == "actions[1].type"),
        "{body}"
    );
}

#[tokio::test]
async fn actions_without_a_browser_are_a_501_not_a_silent_200() {
    // Only the static double is wired. Before this check the plan fell back to it,
    // which ran the page without the actions and answered 200.
    let edge = spawn_edge(None).await;
    let (status, body) = post(&edge, "/v1/scrape", scrape_body(click_screenshot_pdf())).await;
    assert_eq!(status, 501, "{body}");
    assert_eq!(body["error"]["details"]["reason"], "browser_capability_unavailable");
}

#[tokio::test]
async fn scrape_stream_refuses_actions_instead_of_dropping_them() {
    let edge = spawn_edge(Some(browser_with(PageCaptures::default()))).await;
    let response = reqwest::Client::new()
        .post(format!("http://{}/v1/scrape/stream", edge.addr))
        .bearer_auth(TEST_BEARER)
        .json(&scrape_body(click_screenshot_pdf()))
        .send()
        .await
        .unwrap();
    let text = response.text().await.unwrap();
    assert!(text.contains("unsupported_in_stream"), "{text}");
}

#[tokio::test]
async fn an_aborted_action_returns_the_ordered_outcomes_and_no_typed_text() {
    let failed = ActionOutcome {
        index: 0,
        kind: "write".into(),
        status: ActionStatus::Failed,
        duration_ms: 5,
        error: Some(ActionError {
            code: "selector_not_found".into(),
            message: "the selector matched nothing".into(),
        }),
        output: None,
    };
    let skipped = ActionOutcome {
        index: 1,
        kind: "click".into(),
        status: ActionStatus::Skipped,
        duration_ms: 0,
        error: None,
        output: None,
    };
    let error = QuarryError::new(ErrorCode::DriverFailed, "a scrape action failed").with_details(json!({
        "reason": "action_failed",
        "action_index": 0,
        "action_type": "write",
        "actions": [failed, skipped],
    }));
    let edge = spawn_edge(Some(Fake {
        kind: DriverKind::Browser,
        captures: None,
        failure: Some(error),
    }))
    .await;
    let (status, body) = post(
        &edge,
        "/v1/scrape",
        scrape_body(json!([
            { "type": "write", "selector": "#search", "text": SECRET_SENTINEL },
            { "type": "click", "selector": "#go" },
        ])),
    )
    .await;
    assert_eq!(status, 502, "{body}");
    assert_eq!(body["error"]["details"]["reason"], "action_failed", "{body}");
    let listed = body["error"]["details"]["actions"].as_array().expect("ordered outcomes");
    assert_eq!(listed[0]["status"], "failed");
    assert_eq!(listed[1]["status"], "skipped");
    assert!(!body.to_string().contains(SECRET_SENTINEL), "the typed text never comes back");
}
