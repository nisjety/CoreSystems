//! Crawl and batch usage events are keyed by Control's job id (decision 3.11, D2-033).
//!
//! A control stub plays the part of `POST /v1/jobs`: it answers 201 with a new job for an
//! `Idempotency-Key` it has not seen, 200 with the same job for one it has (the replay), and
//! can be told to refuse. A recording meter keeps every event the edge hands to billing.
//! The same cases run with the browser-metrics switch `off` (the legacy rules) and `on`
//! (the corrected rules), because the key and the "no job, no bill" rule hold in both.
//!
//! What is pinned: the first create bills once, under `quarry:<job id>:<metric>`; a replay
//! bills nothing extra; a refused or failed handoff bills nothing; a client that reuses one
//! `X-Request-Id` for different jobs is billed for each of them (the request id plays no
//! part in the key); two calls with no key make two jobs and two events. The synchronous
//! routes (search, map, legacy extract) are untouched and keep billing each retry.

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU16, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use async_trait::async_trait;
use axum::{extract::State, http::HeaderMap, http::StatusCode, routing::post, Json, Router};
use quarry_edge::state::AppState;
use quarry_runtime::{
    fetch::StaticDriver, usage_metrics, BrowserMetricsMode, EventSink, UsageEvent, UsageMeter,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

mod support;

const TEST_BEARER: &str = "billing-handoff-dev-token";

/// Keeps every usage event and reports the browser-metrics mode it was built with.
struct RecordingMeter {
    mode: BrowserMetricsMode,
    events: Mutex<Vec<UsageEvent>>,
}

impl RecordingMeter {
    fn new(mode: BrowserMetricsMode) -> Arc<Self> {
        Arc::new(Self {
            mode,
            events: Mutex::new(Vec::new()),
        })
    }

    fn events(&self) -> Vec<UsageEvent> {
        self.events.lock().unwrap().clone()
    }
}

#[async_trait]
impl UsageMeter for RecordingMeter {
    async fn meter(&self, event: UsageEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn browser_metrics_mode(&self) -> BrowserMetricsMode {
        self.mode
    }
}

/// Stand-in for control's job create. `refuse` is the status to answer with instead of a job
/// (0 = accept).
#[derive(Default)]
struct Control {
    by_key: Mutex<HashMap<String, String>>,
    next: AtomicUsize,
    refuse: AtomicU16,
    posts: AtomicUsize,
}

async fn create_job(
    State(control): State<Arc<Control>>,
    headers: HeaderMap,
    Json(_body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    control.posts.fetch_add(1, Ordering::SeqCst);
    let refuse = control.refuse.load(Ordering::SeqCst);
    if refuse == 429 {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "data": null,
                "error": {
                    "code": "RATE_LIMITED",
                    "message": "this organization has too many jobs waiting",
                    "details": { "reason": "org_backlog_full", "limit": 2, "queued": 2, "retry_after_seconds": 30 }
                }
            })),
        );
    }
    if refuse != 0 {
        return (
            StatusCode::from_u16(refuse).unwrap(),
            Json(json!({ "error": "refused" })),
        );
    }
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    if let Some(key) = &key {
        if let Some(existing) = control.by_key.lock().unwrap().get(key) {
            return (
                StatusCode::OK,
                Json(json!({ "data": { "id": existing, "created_at": 1_700_000_000_000_i64 } })),
            );
        }
    }
    let id = format!("job_{}", control.next.fetch_add(1, Ordering::SeqCst) + 1);
    if let Some(key) = key {
        control.by_key.lock().unwrap().insert(key, id.clone());
    }
    (
        StatusCode::CREATED,
        Json(json!({ "data": { "id": id, "created_at": 1_700_000_000_000_i64 } })),
    )
}

struct Harness {
    edge: SocketAddr,
    control: Arc<Control>,
    meter: Arc<RecordingMeter>,
}

impl Harness {
    async fn start(mode: BrowserMetricsMode) -> Self {
        let control = Arc::new(Control::default());
        let app = Router::new()
            .route("/v1/jobs", post(create_job))
            .with_state(control.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let control_addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        std::env::set_var("QUARRY_EDGE_AUTH_DEV_BYPASS", "1");
        let (tx, mut rx) = mpsc::channel(64);
        tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let driver = Arc::new(StaticDriver::new(Duration::from_secs(5), "quarry-test").unwrap());
        let meter = RecordingMeter::new(mode);
        let state = AppState {
            control_base_url: format!("http://{control_addr}"),
            usage: meter.clone(),
            ..support::base_state(driver, EventSink::new(tx))
        };
        let app = quarry_edge::routes::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let edge = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { edge, control, meter }
    }

    /// One call to a handoff route. `key` is the `Idempotency-Key`, `request_id` the caller's
    /// `X-Request-Id`.
    async fn call(&self, route: Route, key: Option<&str>, request_id: Option<&str>) -> (u16, Value) {
        let mut request = reqwest::Client::new()
            .post(format!("http://{}{}", self.edge, route.path()))
            .bearer_auth(TEST_BEARER)
            .json(&route.body());
        if let Some(key) = key {
            request = request.header("idempotency-key", key);
        }
        if let Some(id) = request_id {
            request = request.header("x-request-id", id);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        (status, response.json().await.unwrap())
    }
}

#[derive(Clone, Copy)]
enum Route {
    Crawl,
    Batch,
}

impl Route {
    fn path(self) -> &'static str {
        match self {
            Route::Crawl => "/v1/crawl",
            Route::Batch => "/v1/batch",
        }
    }

    fn body(self) -> Value {
        match self {
            Route::Crawl => json!({ "url": "https://example.com/" }),
            Route::Batch => json!({ "urls": ["https://example.com/a", "https://example.com/b"] }),
        }
    }

    fn metric(self) -> &'static str {
        match self {
            Route::Crawl => usage_metrics::CRAWL_SEED,
            Route::Batch => usage_metrics::BATCH_URL,
        }
    }

    fn quantity(self) -> f64 {
        match self {
            Route::Crawl => 1.0,
            Route::Batch => 2.0,
        }
    }
}

const MODES: [BrowserMetricsMode; 2] = [BrowserMetricsMode::Off, BrowserMetricsMode::On];
const ROUTES: [Route; 2] = [Route::Crawl, Route::Batch];

#[tokio::test]
async fn the_first_create_bills_once_under_the_job_id() {
    for mode in MODES {
        for route in ROUTES {
            let h = Harness::start(mode).await;
            let (status, body) = h.call(route, Some("key-1"), Some("req-first")).await;
            assert_eq!(status, 200, "{mode:?} {}: {body}", route.path());
            assert_eq!(body["data"]["job_id"], "job_1");

            let events = h.meter.events();
            assert_eq!(events.len(), 1, "{mode:?} {}: {events:?}", route.path());
            let event = &events[0];
            assert_eq!(
                event.event_id,
                format!("quarry:job_1:{}", route.metric()),
                "{mode:?} {}",
                route.path()
            );
            assert_eq!(event.metric, route.metric());
            assert_eq!(event.quantity, route.quantity());
            assert_eq!(event.org_id, "org_placeholder");
            assert!(
                !event.event_id.contains("req-first"),
                "the request id is not part of the key"
            );
        }
    }
}

#[tokio::test]
async fn a_replay_of_an_idempotency_key_bills_nothing_extra() {
    for mode in MODES {
        for route in ROUTES {
            let h = Harness::start(mode).await;
            let (_, first) = h.call(route, Some("key-1"), None).await;
            let after_first = h.meter.events();
            assert_eq!(after_first.len(), 1, "{mode:?} {}", route.path());

            // The client retries with the same key, three times, each with its own request id.
            for attempt in 0..3 {
                let id = format!("retry-{attempt}");
                let (status, replay) = h.call(route, Some("key-1"), Some(&id)).await;
                assert_eq!(status, 200, "{mode:?} {}: {replay}", route.path());
                assert_eq!(
                    replay["data"]["job_id"], first["data"]["job_id"],
                    "the replay answers with the job control already holds"
                );
            }
            assert_eq!(h.control.posts.load(Ordering::SeqCst), 4, "every retry reached control");
            assert_eq!(
                h.meter.events().len(),
                1,
                "{mode:?} {}: a replay created no job and bills nothing",
                route.path()
            );
        }
    }
}

#[tokio::test]
async fn a_failed_or_refused_handoff_bills_nothing() {
    for mode in MODES {
        for route in ROUTES {
            for refusal in [500_u16, 502, 429] {
                let h = Harness::start(mode).await;
                h.control.refuse.store(refusal, Ordering::SeqCst);
                let (status, body) = h.call(route, Some("key-1"), None).await;
                assert_ne!(status, 200, "{mode:?} {} {refusal}: {body}", route.path());
                assert_eq!(h.control.posts.load(Ordering::SeqCst), 1);
                assert!(
                    h.meter.events().is_empty(),
                    "{mode:?} {} {refusal}: no job was created, so nothing is billed: {:?}",
                    route.path(),
                    h.meter.events()
                );

                // Control recovers: the retry creates the job, and that is billed once.
                h.control.refuse.store(0, Ordering::SeqCst);
                let (status, body) = h.call(route, Some("key-1"), None).await;
                assert_eq!(status, 200, "{body}");
                assert_eq!(h.meter.events().len(), 1, "{mode:?} {} {refusal}", route.path());
            }
        }
    }
}

#[tokio::test]
async fn reusing_a_request_id_does_not_collapse_or_collide_billing() {
    // Before decision 3.11 both events carried the caller's request id, so billing-core saw
    // one id and kept one of them. Each job is billed under its own id now.
    for mode in MODES {
        for route in ROUTES {
            let h = Harness::start(mode).await;
            let (_, a) = h.call(route, Some("key-a"), Some("same-request-id")).await;
            let (_, b) = h.call(route, Some("key-b"), Some("same-request-id")).await;
            assert_ne!(a["data"]["job_id"], b["data"]["job_id"]);

            let events = h.meter.events();
            assert_eq!(events.len(), 2, "{mode:?} {}", route.path());
            assert_ne!(events[0].event_id, events[1].event_id, "{mode:?} {}", route.path());
            for event in &events {
                assert!(!event.event_id.contains("same-request-id"));
            }
        }
    }
}

#[tokio::test]
async fn calls_without_a_key_make_a_job_each_and_bill_each() {
    for mode in MODES {
        for route in ROUTES {
            let h = Harness::start(mode).await;
            h.call(route, None, None).await;
            h.call(route, None, None).await;
            let events = h.meter.events();
            assert_eq!(events.len(), 2, "{mode:?} {}", route.path());
            assert_eq!(events[0].event_id, format!("quarry:job_1:{}", route.metric()));
            assert_eq!(events[1].event_id, format!("quarry:job_2:{}", route.metric()));
        }
    }
}
