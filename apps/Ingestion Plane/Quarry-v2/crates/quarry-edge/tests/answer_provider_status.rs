//! INNH-8a: answer synthesis says why it is unavailable.
//!
//! `POST /v1/answer`, `POST /v1/answer/stream` and `include_answer` on `POST /v1/search`, through
//! the real router. Four things are pinned:
//!
//! - a pipeline that is not wired answers 501 with the FIRST closed gate as `reason` and a hint
//!   that names the switch, and `include_answer` names the same reason as `answer_reason`
//!   instead of a silent `answer: null`;
//! - a pipeline that is wired but whose Model Plane call is refused is a 503 with
//!   `model_plane_auth_refused` (never a 403 on the caller), whether Auth Core refused the mint or
//!   the gateway answered 403, and a Model Plane that fails keeps its 502 and gains a reason;
//! - `include_answer` is metered like `/v1/answer` (one `quarry.answer.synth`, never on a cache
//!   hit, a refusal, a failure or a ZDR request) and is skipped under ZDR with no model call;
//! - a failed synthesis is never cached (needs Redis; skipped without one, like
//!   `tests/cache_policy.rs`, unless `REQUIRE_CONTRACT` is set).
//!
//! The model is a wiremock double of the gateway's `POST /v1/invoke`; the edge runs on the auth
//! dev bypass. Result pages are unreachable on purpose: the pipeline falls back to the search
//! snippet, which is enough to make a source.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::Duration;

use async_trait::async_trait;
use quarry_core::error::{ErrorCode, QuarryError};
use quarry_core::output::DriverKind;
use quarry_core::QuarryResult;
use quarry_edge::answer_status::AnswerPipelineStatus;
use quarry_edge::state::AppState;
use quarry_runtime::answer::{AnswerPipeline, SimpleHttpMarkdownFetcher};
use quarry_runtime::driver::Driver;
use quarry_runtime::fetch::FetchResponse;
use quarry_runtime::mp_client::ModelPlaneClient;
use quarry_runtime::serp::{SearchOptions, SearchProvider, SearchResult};
use quarry_runtime::service_tokens::{ServiceBearer, ServiceTokenProvider, ServiceTokenRequest};
use quarry_runtime::{usage_metrics, EventSink, UsageEvent, UsageMeter};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod support;

const BEARER: &str = "answer-status-dev-token";
const PROVIDER_TEXT: &str = "gateway text that must not reach the caller";

fn init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
    });
}

// ---- doubles --------------------------------------------------------------

/// A SERP double. Two results with snippets, or none; counts its calls.
struct FakeSearch {
    calls: AtomicUsize,
    empty: bool,
}

impl FakeSearch {
    fn new(empty: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            empty,
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SearchProvider for FakeSearch {
    async fn search(&self, _query: &str, _opts: &SearchOptions) -> QuarryResult<Vec<SearchResult>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.empty {
            return Ok(Vec::new());
        }
        // Port 9 refuses the connection, so the page fetch fails fast and the snippet stands in.
        Ok((1..=2)
            .map(|rank| SearchResult {
                url: format!("http://127.0.0.1:9/page-{rank}"),
                title: Some(format!("Page {rank}")),
                snippet: Some(
                    "Oslo is the capital of Norway and its largest city, with about 700000 people."
                        .into(),
                ),
                rank,
                provider: "fake".into(),
                ..Default::default()
            })
            .collect())
    }

    fn name(&self) -> &str {
        "fake"
    }
}

#[derive(Default)]
struct RecordingMeter {
    events: Mutex<Vec<UsageEvent>>,
}

#[async_trait]
impl UsageMeter for RecordingMeter {
    async fn meter(&self, event: UsageEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// The edge never fetches a page through its own driver on these routes.
struct NoPageDriver;

#[async_trait]
impl Driver for NoPageDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Static
    }

    async fn fetch(&self, _url: &Url) -> QuarryResult<FetchResponse> {
        Err(QuarryError::new(
            ErrorCode::DriverFailed,
            "no page fetches here",
        ))
    }
}

/// Auth Core refusing to mint the edge's Model Plane token (the registry has no grant yet).
struct RefusingMint;

#[async_trait]
impl ServiceTokenProvider for RefusingMint {
    async fn token_for_org(
        &self,
        _org_id: &str,
        _request: &ServiceTokenRequest,
        _force_refresh: bool,
    ) -> QuarryResult<ServiceBearer> {
        Err(QuarryError::new(
            ErrorCode::Forbidden,
            format!("Auth Core service-token request returned 403 ({PROVIDER_TEXT})"),
        ))
    }

    async fn invalidate(&self, _org_id: &str, _request: &ServiceTokenRequest) {}
}

// ---- the edge -------------------------------------------------------------

#[derive(Clone, Copy)]
enum Mp {
    /// The gateway answers the invoke call.
    Answers,
    /// The gateway answers with this status.
    Status(u16),
    /// Auth Core refuses the mint, so no call is made.
    MintRefused,
}

enum Mode {
    /// No pipeline; the boot status says which gate is closed.
    Closed(AnswerPipelineStatus),
    /// A wired pipeline over a model double.
    Wired {
        mp: Mp,
        pipeline_sees_no_sources: bool,
    },
}

struct Edge {
    addr: SocketAddr,
    usage: Arc<RecordingMeter>,
    route_search: Arc<FakeSearch>,
    pipeline_search: Arc<FakeSearch>,
    mp: MockServer,
}

impl Edge {
    async fn start(mode: Mode) -> Self {
        Self::start_with_redis(mode, None).await
    }

    async fn start_with_redis(mode: Mode, redis: Option<redis::aio::ConnectionManager>) -> Self {
        init();
        let mp = MockServer::start().await;
        let route_search = FakeSearch::new(false);
        let (answer_pipeline, answer_status, pipeline_search) = match mode {
            Mode::Closed(status) => (None, status, FakeSearch::new(false)),
            Mode::Wired {
                mp: behaviour,
                pipeline_sees_no_sources,
            } => {
                match behaviour {
                    Mp::Answers => mount_answer(&mp).await,
                    Mp::Status(code) => {
                        Mock::given(method("POST"))
                            .and(path("/v1/invoke"))
                            .respond_with(
                                ResponseTemplate::new(code).set_body_string(PROVIDER_TEXT),
                            )
                            .mount(&mp)
                            .await
                    }
                    Mp::MintRefused => {}
                }
                let mut client = ModelPlaneClient::new(&mp.uri()).expect("model plane client");
                client = match behaviour {
                    Mp::MintRefused => client.with_token_provider(Arc::new(RefusingMint)),
                    _ => client.with_bearer_token("model-dev-token"),
                };
                let pipeline_search = FakeSearch::new(pipeline_sees_no_sources);
                let pipeline = AnswerPipeline::new(
                    pipeline_search.clone(),
                    Arc::new(SimpleHttpMarkdownFetcher::new()),
                    quarry_runtime::ai_formats::AiFormatRunner::new(Arc::new(client)),
                );
                (
                    Some(Arc::new(pipeline)),
                    AnswerPipelineStatus::Ready,
                    pipeline_search,
                )
            }
        };
        let usage = Arc::new(RecordingMeter::default());
        let (tx, mut rx) = mpsc::channel(256);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let state = AppState {
            search: Some(route_search.clone()),
            usage: usage.clone(),
            answer_pipeline,
            answer_status,
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
            usage,
            route_search,
            pipeline_search,
            mp,
        }
    }

    async fn post(&self, route: &str, body: Value) -> (u16, Value) {
        let response = reqwest::Client::new()
            .post(format!("http://{}{route}", self.addr))
            .bearer_auth(BEARER)
            .json(&body)
            .send()
            .await
            .expect("request reaches the edge");
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    fn metered(&self, metric: &str) -> Vec<UsageEvent> {
        self.usage
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.metric == metric)
            .cloned()
            .collect()
    }

    async fn model_calls(&self) -> usize {
        self.mp.received_requests().await.unwrap_or_default().len()
    }
}

async fn mount_answer(mp: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/invoke"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "request_id": "r1",
            "content": "Oslo is the capital of Norway.",
            "model_used": "model-double"
        })))
        .mount(mp)
        .await;
}

fn wired(mp: Mp) -> Mode {
    Mode::Wired {
        mp,
        pipeline_sees_no_sources: false,
    }
}

fn query(tag: &str) -> String {
    format!("capital of norway {tag}")
}

// ---- 501 names the first closed gate --------------------------------------

#[tokio::test]
async fn a_pipeline_that_is_not_wired_is_a_501_that_names_the_first_closed_gate() {
    for (status, reason, names) in [
        (
            AnswerPipelineStatus::SynthesisDisabled,
            "synthesis_disabled",
            "QUARRY_EDGE__ANSWER_SYNTHESIS",
        ),
        (
            AnswerPipelineStatus::NoSearchProvider,
            "no_search_provider",
            "SEARXNG_URL",
        ),
        (
            AnswerPipelineStatus::NoModelPlaneUrl,
            "no_model_plane_url",
            "QUARRY_EDGE__MODEL_PLANE_URL",
        ),
        (
            AnswerPipelineStatus::ModelClientInitFailed,
            "model_client_init_failed",
            "QUARRY_EDGE__MODEL_PLANE_URL",
        ),
        // The QB-T17 boot gate refused a switch that is on (no granted model-invoke row).
        (
            AnswerPipelineStatus::ModelPathRefused,
            "model_path_refused",
            "QUARRY_EDGE__ANSWER_SYNTHESIS",
        ),
    ] {
        let edge = Edge::start(Mode::Closed(status)).await;
        for route in ["/v1/answer", "/v1/answer/stream"] {
            let (code, body) = edge.post(route, json!({ "query": query("501") })).await;
            assert_eq!(code, 501, "{route} with {reason}");
            assert_eq!(body["code"], "UNSUPPORTED", "{route} with {reason}");
            assert_eq!(body["reason"], reason, "{route} with {reason}");
            let hint = body["hint"].as_str().expect("a hint");
            assert!(
                hint.contains(names),
                "{route}: hint {hint:?} should name {names}"
            );
            assert!(hint.contains("docs/RUNBOOKS.md"), "{route}: {hint:?}");
        }
        assert_eq!(edge.model_calls().await, 0);
    }
}

#[tokio::test]
async fn include_answer_without_a_pipeline_says_why_instead_of_a_silent_null() {
    for (status, reason) in [
        (
            AnswerPipelineStatus::SynthesisDisabled,
            "synthesis_disabled",
        ),
        (AnswerPipelineStatus::NoSearchProvider, "no_search_provider"),
        (AnswerPipelineStatus::NoModelPlaneUrl, "no_model_plane_url"),
        (
            AnswerPipelineStatus::ModelClientInitFailed,
            "model_client_init_failed",
        ),
        (AnswerPipelineStatus::ModelPathRefused, "model_path_refused"),
    ] {
        let edge = Edge::start(Mode::Closed(status)).await;
        let (code, body) = edge
            .post(
                "/v1/search",
                json!({ "query": query("search"), "include_answer": true }),
            )
            .await;
        assert_eq!(code, 200, "search still succeeds: {body}");
        assert_eq!(body["count"], 2);
        assert_eq!(body["answer_status"], "unavailable", "{reason}");
        assert_eq!(body["answer_reason"], reason);
        assert!(body.get("answer").is_none(), "no answer key: {body}");
        assert!(edge.metered(usage_metrics::ANSWER_SYNTH).is_empty());
        // The search itself is still metered once.
        assert_eq!(edge.metered(usage_metrics::SEARCH_QUERY).len(), 1);
    }
}

#[tokio::test]
async fn a_search_that_did_not_ask_for_an_answer_carries_neither_new_field() {
    let edge = Edge::start(Mode::Closed(AnswerPipelineStatus::SynthesisDisabled)).await;
    let (code, body) = edge
        .post("/v1/search", json!({ "query": query("plain") }))
        .await;
    assert_eq!(code, 200);
    assert!(body.get("answer_status").is_none(), "{body}");
    assert!(body.get("answer_reason").is_none(), "{body}");
    assert!(body.get("answer").is_none(), "{body}");
}

// ---- a wired pipeline whose model call is refused or fails ----------------

#[tokio::test]
async fn a_refused_model_call_is_a_503_and_never_a_403_on_the_caller() {
    for (label, mp) in [
        ("gateway 403", Mp::Status(403)),
        ("mint refused", Mp::MintRefused),
    ] {
        let edge = Edge::start(wired(mp)).await;
        let (code, body) = edge
            .post("/v1/answer", json!({ "query": query("refused") }))
            .await;
        assert_eq!(code, 503, "{label}: {body}");
        assert_eq!(body["code"], "UNAVAILABLE", "{label}");
        assert_eq!(body["reason"], "model_plane_auth_refused", "{label}");
        assert!(
            body["hint"].as_str().unwrap().contains("model-gateway"),
            "{label}: {body}"
        );
        let wire = body.to_string();
        assert!(
            !wire.contains(PROVIDER_TEXT),
            "{label}: provider text must stay out of the body: {wire}"
        );
        // The stream route reports the same closed reason in its error frame.
        let (code, body) = edge
            .post(
                "/v1/answer/stream",
                json!({ "query": query("refused-stream") }),
            )
            .await;
        assert_eq!(code, 200, "{label}: an SSE response");
        let text = body.as_str().unwrap_or_default().to_owned();
        assert!(text.contains("event: error"), "{label}: {text}");
        assert!(text.contains("model_plane_auth_refused"), "{label}: {text}");
        assert!(
            edge.metered(usage_metrics::ANSWER_SYNTH).is_empty(),
            "{label}: a refused call is not metered"
        );
    }
}

#[tokio::test]
async fn a_refused_model_call_on_search_is_unavailable_with_the_same_reason_and_is_not_metered() {
    for (label, mp) in [
        ("gateway 403", Mp::Status(403)),
        ("mint refused", Mp::MintRefused),
    ] {
        let edge = Edge::start(wired(mp)).await;
        let (code, body) = edge
            .post(
                "/v1/search",
                json!({ "query": query("search-refused"), "include_answer": true }),
            )
            .await;
        assert_eq!(code, 200, "{label}: {body}");
        assert_eq!(body["answer_status"], "unavailable", "{label}");
        assert_eq!(body["answer_reason"], "model_plane_auth_refused", "{label}");
        assert!(body.get("answer").is_none(), "{label}: {body}");
        assert!(!body.to_string().contains(PROVIDER_TEXT));
        assert!(
            edge.metered(usage_metrics::ANSWER_SYNTH).is_empty(),
            "{label}"
        );
    }
}

#[tokio::test]
async fn a_model_plane_failure_keeps_its_502_and_gains_a_reason() {
    let edge = Edge::start(wired(Mp::Status(500))).await;
    let (code, body) = edge
        .post("/v1/answer", json!({ "query": query("fails") }))
        .await;
    assert_eq!(code, 502, "{body}");
    // The wire name CONTRACTS.md and ERROR_REASONS.md promise, not the Rust identifier upper-cased.
    assert_eq!(body["code"], "DRIVER_FAILED", "{body}");
    assert_eq!(body["reason"], "model_plane_error");
    let (code, body) = edge
        .post(
            "/v1/search",
            json!({ "query": query("fails-search"), "include_answer": true }),
        )
        .await;
    assert_eq!(code, 200);
    assert_eq!(body["answer_status"], "unavailable");
    assert_eq!(body["answer_reason"], "model_plane_error");
}

// ---- a healthy synthesis, metering, ZDR -----------------------------------

#[tokio::test]
async fn a_healthy_include_answer_is_ok_and_metered_once() {
    let edge = Edge::start(wired(Mp::Answers)).await;
    let (code, body) = edge
        .post(
            "/v1/search",
            json!({ "query": query("healthy"), "include_answer": true }),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["answer_status"], "ok");
    assert!(body.get("answer_reason").is_none(), "{body}");
    assert!(body["answer"].as_str().unwrap().contains("Oslo"), "{body}");
    assert!(!body["citations"].as_array().unwrap().is_empty());
    assert_eq!(edge.model_calls().await, 1);

    let synth = edge.metered(usage_metrics::ANSWER_SYNTH);
    assert_eq!(synth.len(), 1, "one model call, one ANSWER_SYNTH");
    assert_eq!(synth[0].quantity, 1.0);
    assert_eq!(synth[0].metadata["via"], "search.include_answer");
    assert_eq!(synth[0].metadata["model"], "model-double");
    assert!(synth[0].metadata["sources_used"].as_u64().unwrap() >= 1);
    let search = edge.metered(usage_metrics::SEARCH_QUERY);
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].metadata["answer_status"], "ok");
    assert_ne!(
        synth[0].event_id, search[0].event_id,
        "distinct billing ids"
    );
}

#[tokio::test]
async fn an_answer_with_no_usable_source_is_no_sources_and_calls_no_model() {
    let edge = Edge::start(Mode::Wired {
        mp: Mp::Answers,
        pipeline_sees_no_sources: true,
    })
    .await;
    let (code, body) = edge
        .post(
            "/v1/search",
            json!({ "query": query("nothing"), "include_answer": true }),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["answer_status"], "no_sources");
    assert_eq!(body["answer"], "");
    assert_eq!(edge.model_calls().await, 0);
    assert!(edge.metered(usage_metrics::ANSWER_SYNTH).is_empty());
    assert_eq!(edge.pipeline_search.calls(), 1);
}

#[tokio::test]
async fn include_answer_under_zdr_is_skipped_with_no_model_call_and_no_meter() {
    let edge = Edge::start(wired(Mp::Answers)).await;
    let (code, body) = edge
        .post(
            "/v1/search",
            json!({ "query": query("zdr"), "include_answer": true, "zdr": true }),
        )
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["answer_status"], "skipped_zdr");
    assert!(body.get("answer").is_none(), "{body}");
    assert!(body.get("answer_reason").is_none(), "{body}");
    assert_eq!(body["count"], 2, "the search itself still runs");
    assert_eq!(edge.model_calls().await, 0, "no page text goes to a model");
    assert_eq!(
        edge.pipeline_search.calls(),
        0,
        "the answer pipeline never starts"
    );
    assert!(edge.metered(usage_metrics::ANSWER_SYNTH).is_empty());
}

/// ADR-4: the model hop carries no ZDR posture, so a ZDR answer is refused BEFORE the pipeline
/// starts. The gateway double sees no request, and neither search provider is asked.
#[tokio::test]
async fn a_zdr_answer_is_refused_before_the_pipeline_starts_and_nothing_reaches_the_model() {
    let edge = Edge::start(wired(Mp::Answers)).await;
    for route in ["/v1/answer", "/v1/answer/stream"] {
        let (code, body) = edge
            .post(route, json!({ "query": query("zdr-answer"), "zdr": true }))
            .await;
        assert_eq!(code, 403, "{route}: {body}");
        assert_eq!(body["code"], "FORBIDDEN", "{route}");
        assert_eq!(body["reason"], "zdr_unavailable", "{route}");
        assert!(body["error"].as_str().unwrap().contains("ZDR"), "{route}: {body}");
        assert!(body["hint"].as_str().unwrap().contains("ZDR"), "{route}: {body}");
        assert!(!body.to_string().contains("capital of norway"), "{route}: the query stays out");
    }
    assert_eq!(edge.model_calls().await, 0, "the gateway double saw no request");
    assert_eq!(edge.pipeline_search.calls(), 0, "no search, so no page fetch");
    assert_eq!(edge.route_search.calls(), 0);
    assert!(edge.metered(usage_metrics::ANSWER_SYNTH).is_empty());

    // The same request without ZDR still runs: the refusal is the posture, not the route.
    let (code, body) = edge
        .post("/v1/answer", json!({ "query": query("not-zdr") }))
        .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(edge.model_calls().await, 1);
}

/// A closed gate does not hide the ZDR fact: a ZDR caller is told 403 ZDR, not the deployment's
/// 501, and learns nothing about which switches this edge has.
#[tokio::test]
async fn a_zdr_answer_is_refused_even_when_the_gate_is_closed() {
    let edge = Edge::start(Mode::Closed(AnswerPipelineStatus::ModelPathRefused)).await;
    for route in ["/v1/answer", "/v1/answer/stream"] {
        let (code, body) = edge
            .post(route, json!({ "query": query("zdr-closed-answer"), "zdr": true }))
            .await;
        assert_eq!(code, 403, "{route}: {body}");
        assert_eq!(body["reason"], "zdr_unavailable", "{route}");
        assert!(!body.to_string().contains("QUARRY_EDGE__ANSWER_SYNTHESIS"), "{body}");
    }
}

#[tokio::test]
async fn zdr_wins_over_a_closed_gate() {
    // Nothing was going to be sent either way; the status says the ZDR fact, not a config fact.
    let edge = Edge::start(Mode::Closed(AnswerPipelineStatus::SynthesisDisabled)).await;
    let (_, body) = edge
        .post(
            "/v1/search",
            json!({ "query": query("zdr-closed"), "include_answer": true, "zdr": true }),
        )
        .await;
    assert_eq!(body["answer_status"], "skipped_zdr");
}

#[tokio::test]
async fn the_answer_route_still_meters_once_and_returns_the_old_shape() {
    let edge = Edge::start(wired(Mp::Answers)).await;
    let (code, body) = edge
        .post("/v1/answer", json!({ "query": query("route") }))
        .await;
    assert_eq!(code, 200, "{body}");
    assert!(body["answer"].as_str().unwrap().contains("Oslo"));
    assert_eq!(body["model"], "model-double");
    assert_eq!(edge.metered(usage_metrics::ANSWER_SYNTH).len(), 1);
    assert_eq!(
        edge.route_search.calls(),
        0,
        "/v1/answer does not use the route's provider"
    );
}

// ---- a failed synthesis is never cached -----------------------------------

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
async fn a_failed_synthesis_is_not_cached_so_the_next_query_sees_the_pipeline_turned_on() {
    let Some(conn) = try_redis().await else {
        assert!(
            std::env::var_os("REQUIRE_CONTRACT").is_none(),
            "REQUIRE_CONTRACT is set but REDIS_URL is unreachable: the answer cache guard cannot be skipped"
        );
        eprintln!("skip: REDIS_URL unreachable");
        return;
    };
    let edge = Edge::start_with_redis(wired(Mp::Status(403)), Some(conn)).await;
    // A fresh query every run: the cache key is the org, the query and the options.
    let q = format!(
        "capital of norway {}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let body = json!({ "query": q, "include_answer": true });

    let (_, first) = edge.post("/v1/search", body.clone()).await;
    assert_eq!(first["answer_status"], "unavailable");
    assert_eq!(edge.route_search.calls(), 1);

    // The gateway now accepts the call. The failure above must not have been cached.
    edge.mp.reset().await;
    mount_answer(&edge.mp).await;
    let (_, second) = edge.post("/v1/search", body.clone()).await;
    assert_eq!(second["answer_status"], "ok", "{second}");
    assert!(second["answer"].as_str().unwrap().contains("Oslo"));
    assert_eq!(
        edge.route_search.calls(),
        2,
        "the failed attempt left no cache entry"
    );

    // The good answer is cached, and a cache hit is not metered again.
    let (_, third) = edge.post("/v1/search", body).await;
    assert_eq!(third["answer_status"], "ok");
    assert_eq!(third["answer"], second["answer"]);
    assert_eq!(edge.route_search.calls(), 2, "served from the cache");
    assert_eq!(edge.metered(usage_metrics::ANSWER_SYNTH).len(), 1);
    assert_eq!(edge.metered(usage_metrics::SEARCH_QUERY).len(), 3);
}
