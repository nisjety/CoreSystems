//! D2-055 (answer half of U33): `/v1/answer` and `/v1/answer/stream` gate every search-result URL
//! with the request's web policy BEFORE the page is fetched.
//!
//! Through the real router, on the auth dev bypass. The page fetcher, the search provider and the
//! Model Plane are doubles that count their calls:
//!
//! - a denied host is never fetched, never reaches the model, and is reported on its citation as
//!   `failed` / `policy_denied` (typed, no host, no rule); the allowed host is fetched as before;
//! - the default policy changes nothing: both results are fetched;
//! - a ZDR request is still refused before search, fetch and model, and leaves no answer-cache
//!   entry (the cache half needs Redis; it is skipped without one unless `REQUIRE_CONTRACT`).

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::answer_status::AnswerPipelineStatus;
use quarry_edge::policy_baseline::{PolicyBaseline, BASELINE_ENV};
use quarry_edge::state::AppState;
use quarry_runtime::answer::{AnswerPipeline, MarkdownFetcher};
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::mp_client::ModelPlaneClient;
use quarry_runtime::serp::{SearchOptions, SearchProvider, SearchResult};
use quarry_runtime::EventSink;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod support;

const BEARER: &str = "answer-gate-dev-token";
const ALLOWED_URL: &str = "https://allowed.example.org/oslo";
const DENIED_URL: &str = "https://denied.example.net/secret-page";
const ALLOWED_TEXT: &str = "Oslo is the capital of Norway and its largest city.";
const DENIED_SNIPPET: &str = "DENIEDSNIPPET the policy refused this host";

fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    });
}

struct TwoResults {
    calls: AtomicUsize,
}

#[async_trait]
impl SearchProvider for TwoResults {
    async fn search(&self, _query: &str, _opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![
            SearchResult {
                url: ALLOWED_URL.into(),
                title: Some("Allowed".into()),
                snippet: Some("allowed snippet".into()),
                rank: 1,
                provider: "fake".into(),
                ..Default::default()
            },
            SearchResult {
                url: DENIED_URL.into(),
                title: Some("Denied".into()),
                snippet: Some(DENIED_SNIPPET.into()),
                rank: 2,
                provider: "fake".into(),
                ..Default::default()
            },
        ])
    }

    fn name(&self) -> &str {
        "fake"
    }
}

/// Records every URL it is asked to fetch. Every page is a real-looking body.
#[derive(Default)]
struct RecordingFetcher {
    fetched: Mutex<Vec<String>>,
}

impl RecordingFetcher {
    fn urls(&self) -> Vec<String> {
        self.fetched.lock().unwrap().clone()
    }
}

#[async_trait]
impl MarkdownFetcher for RecordingFetcher {
    async fn fetch_markdown(&self, url: &str, _zdr: quarry_core::zdr::ZdrMode) -> Option<String> {
        self.fetched.lock().unwrap().push(url.to_owned());
        Some(ALLOWED_TEXT.repeat(4))
    }
}

struct NoPageDriver;

#[async_trait]
impl Driver for NoPageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
        Err(QuarryError::new(ErrorCode::DriverFailed, "no page fetches here"))
    }
}

struct Edge {
    addr: SocketAddr,
    search: Arc<TwoResults>,
    fetcher: Arc<RecordingFetcher>,
    mp: MockServer,
}

impl Edge {
    async fn start(baseline: Option<&str>, redis: Option<redis::aio::ConnectionManager>) -> Self {
        init();
        let mp = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/invoke"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "request_id": "r1",
                "content": "Oslo is the capital of Norway.",
                "model_used": "model-double"
            })))
            .mount(&mp)
            .await;
        let client = ModelPlaneClient::new(&mp.uri())
            .expect("model plane client")
            .with_bearer_token("model-dev-token");
        let search = Arc::new(TwoResults {
            calls: AtomicUsize::new(0),
        });
        let fetcher = Arc::new(RecordingFetcher::default());
        let pipeline = AnswerPipeline::new(
            search.clone(),
            fetcher.clone(),
            quarry_runtime::ai_formats::AiFormatRunner::new(Arc::new(client)),
        );
        let (tx, mut rx) = mpsc::channel(256);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let policy = match baseline {
            Some(json) => {
                PolicyBaseline::from_lookup(|name| (name == BASELINE_ENV).then(|| json.to_owned()))
                    .expect("the test baseline is a valid layer")
            }
            None => PolicyBaseline::default(),
        };
        let state = AppState {
            answer_pipeline: Some(Arc::new(pipeline)),
            answer_status: AnswerPipelineStatus::Ready,
            policy,
            redis,
            #[cfg(feature = "browser-agent")]
            agent_driver: support::governed_agent_driver(),
            ..support::base_state(Arc::new(NoPageDriver), EventSink::new(tx))
        };
        let app = quarry_edge::routes::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            addr,
            search,
            fetcher,
            mp,
        }
    }

    async fn post(&self, route: &str, body: Value) -> (u16, String) {
        let response = reqwest::Client::new()
            .post(format!("http://{}{route}", self.addr))
            .bearer_auth(BEARER)
            .json(&body)
            .send()
            .await
            .expect("request reaches the edge");
        let status = response.status().as_u16();
        (status, response.text().await.unwrap())
    }

    async fn model_bodies(&self) -> Vec<String> {
        self.mp
            .received_requests()
            .await
            .unwrap_or_default()
            .iter()
            .map(|request| String::from_utf8_lossy(&request.body).into_owned())
            .collect()
    }
}

const DENY_NET: &str = r#"{"domains":{"deny":["example.net"]}}"#;

/// The `data:` payload of the first SSE frame named `event`.
fn sse_event(body: &str, event: &str) -> Option<Value> {
    let mut lines = body.lines().map(str::trim_end);
    while let Some(line) = lines.next() {
        if line == format!("event: {event}") {
            for next in lines.by_ref() {
                if let Some(data) = next.strip_prefix("data:") {
                    return serde_json::from_str(data.trim()).ok();
                }
            }
        }
    }
    None
}

fn citation_for<'a>(citations: &'a [Value], url: &str) -> &'a Value {
    citations
        .iter()
        .find(|citation| citation["url"] == url)
        .unwrap_or_else(|| panic!("no citation for {url}: {citations:?}"))
}

fn assert_denied_citation(citation: &Value) {
    assert_eq!(citation["outcome"]["kind"], "failed", "{citation}");
    assert_eq!(citation["outcome"]["reason"]["code"], "policy_denied", "{citation}");
    // The reason is a closed code: no host, rule or layer name rides on it.
    let shown = citation["outcome"].to_string();
    assert!(!shown.contains("example.net"), "{shown}");
}

// ---- /v1/answer -----------------------------------------------------------

#[tokio::test]
async fn answer_never_fetches_a_denied_host_and_still_fetches_the_allowed_one() {
    let edge = Edge::start(Some(DENY_NET), None).await;
    let (status, text) = edge
        .post("/v1/answer", json!({ "query": "capital of norway" }))
        .await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();

    assert_eq!(
        edge.fetcher.urls(),
        vec![ALLOWED_URL.to_owned()],
        "only the allowed URL is fetched"
    );
    assert_eq!(body["sources_used"], 1, "{body}");
    assert_eq!(body["sources_skipped"], 1, "{body}");
    let citations = body["citations"].as_array().unwrap();
    assert_eq!(citation_for(citations, ALLOWED_URL)["outcome"]["kind"], "fetched");
    assert_denied_citation(citation_for(citations, DENIED_URL));

    // The denied result's snippet does not stand in for the refused page: it never reaches the model.
    let sent = edge.model_bodies().await.join("\n");
    assert!(!sent.contains(DENIED_SNIPPET), "denied snippet reached the model");
    assert!(!sent.contains("secret-page"), "denied URL reached the model");
    assert!(sent.contains("Oslo is the capital"), "the allowed page reached the model");
}

#[tokio::test]
async fn answer_under_the_default_policy_fetches_every_result() {
    let edge = Edge::start(None, None).await;
    let (status, text) = edge
        .post("/v1/answer", json!({ "query": "capital of norway" }))
        .await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        edge.fetcher.urls(),
        vec![ALLOWED_URL.to_owned(), DENIED_URL.to_owned()],
        "the default policy is today's behaviour"
    );
    assert_eq!(body["sources_used"], 2, "{body}");
    assert_eq!(body["sources_skipped"], 0, "{body}");
}

#[tokio::test]
async fn answer_with_every_result_denied_fetches_nothing_and_makes_no_model_call() {
    let edge = Edge::start(
        Some(r#"{"domains":{"deny":["example.net","example.org"]}}"#),
        None,
    )
    .await;
    let (status, text) = edge
        .post("/v1/answer", json!({ "query": "capital of norway" }))
        .await;
    assert_eq!(status, 200, "{text}");
    let body: Value = serde_json::from_str(&text).unwrap();
    assert!(edge.fetcher.urls().is_empty());
    assert_eq!(body["answer"], "");
    assert_eq!(body["sources_used"], 0);
    assert_eq!(body["sources_skipped"], 2);
    assert!(edge.model_bodies().await.is_empty(), "no source, no model call");
}

// ---- /v1/answer/stream ----------------------------------------------------

#[tokio::test]
async fn answer_stream_never_fetches_a_denied_host() {
    let edge = Edge::start(Some(DENY_NET), None).await;
    let (status, text) = edge
        .post("/v1/answer/stream", json!({ "query": "capital of norway" }))
        .await;
    assert_eq!(status, 200, "{text}");

    assert_eq!(edge.fetcher.urls(), vec![ALLOWED_URL.to_owned()]);
    let citations = sse_event(&text, "citations").expect("a citations frame");
    let citations = citations.as_array().unwrap();
    assert_eq!(citation_for(citations, ALLOWED_URL)["outcome"]["kind"], "fetched");
    assert_denied_citation(citation_for(citations, DENIED_URL));
    assert!(sse_event(&text, "done").is_some(), "{text}");

    let sent = edge.model_bodies().await.join("\n");
    assert!(!sent.contains(DENIED_SNIPPET), "denied snippet reached the model");
    assert!(!sent.contains("secret-page"), "denied URL reached the model");
}

#[tokio::test]
async fn answer_stream_under_the_default_policy_fetches_every_result() {
    let edge = Edge::start(None, None).await;
    let (status, text) = edge
        .post("/v1/answer/stream", json!({ "query": "capital of norway" }))
        .await;
    assert_eq!(status, 200, "{text}");
    assert_eq!(edge.fetcher.urls().len(), 2);
}

// ---- ZDR ------------------------------------------------------------------

#[tokio::test]
async fn zdr_is_still_refused_before_search_fetch_and_model_on_both_routes() {
    let edge = Edge::start(Some(DENY_NET), None).await;
    for route in ["/v1/answer", "/v1/answer/stream"] {
        let (status, text) = edge
            .post(route, json!({ "query": "capital of norway", "zdr": true }))
            .await;
        assert_eq!(status, 403, "{route}: {text}");
        let body: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["reason"], "zdr_unavailable", "{route}");
    }
    assert_eq!(edge.search.calls.load(Ordering::SeqCst), 0, "no search");
    assert!(edge.fetcher.urls().is_empty(), "no page fetch");
    assert!(edge.model_bodies().await.is_empty(), "no model call");
}

fn redis_url() -> String {
    std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string())
}

async fn try_redis() -> Option<redis::aio::ConnectionManager> {
    let client = redis::Client::open(redis_url()).ok()?;
    tokio::time::timeout(Duration::from_secs(2), client.get_connection_manager())
        .await
        .ok()?
        .ok()
}

#[tokio::test]
async fn a_zdr_stream_writes_no_answer_cache_entry_and_a_standard_one_does() {
    let Some(conn) = try_redis().await else {
        assert!(
            std::env::var_os("REQUIRE_CONTRACT").is_none(),
            "REQUIRE_CONTRACT is set but REDIS_URL is unreachable: the answer cache guard cannot be skipped"
        );
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let cache = quarry_edge::cache::SearchCache::new(conn.clone());
    let edge = Edge::start(Some(DENY_NET), Some(conn)).await;
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let zdr_query = format!("capital of norway zdr {unique}");
    let std_query = format!("capital of norway std {unique}");
    let key = |query: &str| {
        quarry_edge::cache::SearchCache::key("org_placeholder", query, "answer:stream:v1")
    };

    let (status, _) = edge
        .post("/v1/answer/stream", json!({ "query": zdr_query, "zdr": true }))
        .await;
    assert_eq!(status, 403);
    assert!(
        cache.get::<String>(&key(&zdr_query)).await.is_none(),
        "a ZDR request leaves nothing in the answer cache"
    );
    assert!(edge.fetcher.urls().is_empty());

    // Control: the same route without ZDR does write the entry, so the key above is the real one.
    let (status, _) = edge
        .post("/v1/answer/stream", json!({ "query": std_query }))
        .await;
    assert_eq!(status, 200);
    assert!(
        cache.get::<String>(&key(&std_query)).await.is_some(),
        "the control run wrote its answer-cache entry"
    );
    // The gate held on the control run as well.
    assert_eq!(edge.fetcher.urls(), vec![ALLOWED_URL.to_owned()]);
}
