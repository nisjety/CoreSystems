//! A fake quarry-edge `/v1/agent/*` on `127.0.0.1:0`, driven through the real
//! `QuarryAgentClient`. It records every request body and header it receives and
//! replays scripted step responses.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use execution_core::quarry_agent::QuarryAgentClient;
use serde_json::{json, Value};

/// One scripted answer to `POST /v1/agent/runs/:id/step`.
#[derive(Debug, Clone)]
pub enum StepScript {
    /// 200 with this observation (the envelope `data`).
    Observation(Value),
    /// A non-2xx answer with this JSON body (a Quarry error envelope).
    Error { status: u16, body: Value },
}

#[derive(Default)]
struct Inner {
    start: Option<(u16, Value)>,
    steps: VecDeque<StepScript>,
    start_requests: Vec<Value>,
    step_requests: Vec<Value>,
    step_headers: Vec<Vec<(String, String)>>,
    start_headers: Vec<Vec<(String, String)>>,
    close_headers: Vec<Vec<(String, String)>>,
    closed: u32,
    step_counter: u32,
    /// WS8.G: the run id in the path of each step request.
    step_paths: Vec<String>,
    /// WS8.G: a scripted error for `POST .../attach`.
    attach_error: Option<(u16, Value)>,
    /// WS8.G: `(run_id, body, headers)` of each attach request.
    attach_requests: Vec<(String, Value, Vec<(String, String)>)>,
    /// WS8.G: `(run_id, attach_id, disposition)` of each release.
    releases: Vec<(String, String, String)>,
    /// WS8.G: `(run_id, grant_id)` of each tab listing.
    tab_queries: Vec<(String, String)>,
}

type Shared = Arc<Mutex<Inner>>;

pub struct FakeQuarry {
    base_url: String,
    inner: Shared,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeQuarry {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeQuarry {
    /// Bind `127.0.0.1:0` and serve.
    pub async fn start() -> Self {
        let inner: Shared = Arc::new(Mutex::new(Inner::default()));
        let app = Router::new()
            .route("/v1/agent/runs", post(start_run))
            .route("/v1/agent/runs/:run_id/step", post(step))
            .route("/v1/agent/runs/:run_id", delete(close_run))
            .route("/v1/agent/runs/:run_id/attach", post(attach_run))
            .route("/v1/agent/runs/:run_id/attach/:attach_id", delete(release_attach))
            .route("/v1/agent/runs/:run_id/tabs", get(list_tabs))
            .with_state(inner.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Self {
            base_url,
            inner,
            task,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// The real client, pointed at this server with a static test token.
    pub fn client(&self) -> QuarryAgentClient {
        QuarryAgentClient::new(self.base_url.clone(), "test-token").expect("client")
    }

    /// Make `POST /v1/agent/runs` answer with this error instead of a run.
    pub fn fail_start(&self, status: u16, body: Value) {
        self.inner.lock().expect("lock").start = Some((status, body));
    }

    pub fn push(&self, script: StepScript) -> &Self {
        self.inner.lock().expect("lock").steps.push_back(script);
        self
    }

    /// A 200 observation of `url` with `text` and these `(ref, role, name, text)`
    /// snapshot targets, snapshot `snap-N` at generation N.
    pub fn push_page(&self, url: &str, title: &str, text: &str, targets: &[(&str, &str, &str, &str)]) -> &Self {
        let mut guard = self.inner.lock().expect("lock");
        guard.step_counter += 1;
        let n = guard.step_counter;
        let targets: Vec<Value> = targets
            .iter()
            .map(|(r, role, name, text)| json!({"ref_id": r, "role": role, "name": name, "text": text}))
            .collect();
        guard.steps.push_back(StepScript::Observation(json!({
            "run_id": "run_fake",
            "step": n,
            "url": url,
            "title": title,
            "snapshot": {"snapshot_id": format!("snap-{n}"), "generation": n, "targets": targets},
            "dom_summary": {"node_count": 10, "text_snippet": text},
        })));
        self
    }

    /// WS12.S4: like [`Self::push_page`] (no targets) with Quarry's redaction
    /// stamp as its `RedactionStamp` serialises it.
    pub fn push_stamped_page(&self, url: &str, title: &str, text: &str, rule_version: &str) -> &Self {
        let mut guard = self.inner.lock().expect("lock");
        guard.step_counter += 1;
        let n = guard.step_counter;
        guard.steps.push_back(StepScript::Observation(json!({
            "run_id": "run_fake",
            "step": n,
            "url": url,
            "title": title,
            "snapshot": {"snapshot_id": format!("snap-{n}"), "generation": n, "targets": []},
            "dom_summary": {"node_count": 10, "text_snippet": text},
            "redaction": {
                "rule_version": rule_version, "profile": "live", "mode": "enforce",
                "values_masked": 0, "by_rule": {}, "probe": "ok", "probe_ms": 1,
                "screenshot": "none", "mask_ms": 0, "oopif_frames": 0, "rects": 0
            },
        })));
        self
    }

    pub fn push_error(&self, status: u16, body: Value) -> &Self {
        self.push(StepScript::Error { status, body })
    }

    pub fn start_requests(&self) -> Vec<Value> {
        self.inner.lock().expect("lock").start_requests.clone()
    }

    /// Every step request body, in arrival order.
    pub fn step_requests(&self) -> Vec<Value> {
        self.inner.lock().expect("lock").step_requests.clone()
    }

    /// The headers (lower-cased names) of each step request.
    pub fn step_headers(&self) -> Vec<Vec<(String, String)>> {
        self.inner.lock().expect("lock").step_headers.clone()
    }

    /// The headers (lower-cased names) of each start request.
    pub fn start_headers(&self) -> Vec<Vec<(String, String)>> {
        self.inner.lock().expect("lock").start_headers.clone()
    }

    /// The headers (lower-cased names) of each release request.
    pub fn close_headers(&self) -> Vec<Vec<(String, String)>> {
        self.inner.lock().expect("lock").close_headers.clone()
    }

    /// How many times the run was released.
    pub fn closed_runs(&self) -> u32 {
        self.inner.lock().expect("lock").closed
    }

    /// WS8.G: the run id each step went to, in order.
    pub fn step_paths(&self) -> Vec<String> {
        self.inner.lock().expect("lock").step_paths.clone()
    }

    /// WS8.G: make `POST .../attach` answer with this error.
    pub fn fail_attach(&self, status: u16, body: Value) {
        self.inner.lock().expect("lock").attach_error = Some((status, body));
    }

    /// WS8.G: every attach request, `(run_id, body, headers)`.
    pub fn attach_requests(&self) -> Vec<(String, Value, Vec<(String, String)>)> {
        self.inner.lock().expect("lock").attach_requests.clone()
    }

    /// WS8.G: every release, `(run_id, attach_id, disposition)`.
    pub fn releases(&self) -> Vec<(String, String, String)> {
        self.inner.lock().expect("lock").releases.clone()
    }

    /// WS8.G: a 409 `USER_TOOK_OVER` exactly as Quarry's delegate lane writes it.
    pub fn push_took_over(&self) -> &Self {
        self.push_error(
            409,
            json!({"data": null, "meta": {"request_id": "r"}, "error": {
                "code": "USER_TOOK_OVER",
                "message": "the person using the browser has taken control of this tab; this action was not run",
                "retryable": false,
                "details": {"scope": "tab", "tab_id": "tab_fake", "tab_epoch": 2, "control_epoch": 3}
            }}),
        )
    }
}

fn envelope(data: Value) -> Value {
    json!({"data": data, "meta": {}, "error": null})
}

fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

async fn start_run(
    State(inner): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut guard = inner.lock().expect("lock");
    guard.start_requests.push(body);
    guard.start_headers.push(header_pairs(&headers));
    if let Some((status, body)) = guard.start.clone() {
        return (
            StatusCode::from_u16(status).expect("status"),
            Json(body),
        );
    }
    (
        StatusCode::OK,
        Json(envelope(json!({"run_id": "run_fake", "lease_id": "lease_fake"}))),
    )
}

async fn step(
    State(inner): State<Shared>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut guard = inner.lock().expect("lock");
    guard.step_paths.push(run_id);
    guard.step_requests.push(body);
    guard.step_headers.push(
        headers
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    value.to_str().unwrap_or_default().to_owned(),
                )
            })
            .collect(),
    );
    match guard.steps.pop_front() {
        Some(StepScript::Observation(observation)) => {
            (StatusCode::OK, Json(envelope(observation)))
        }
        Some(StepScript::Error { status, body }) => (
            StatusCode::from_u16(status).expect("status"),
            Json(body),
        ),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": {"code": "FAKE_QUARRY_SCRIPT_EXHAUSTED", "message": "no scripted step"}})),
        ),
    }
}

async fn close_run(
    State(inner): State<Shared>,
    Path(_run_id): Path<String>,
    headers: HeaderMap,
) -> StatusCode {
    let mut guard = inner.lock().expect("lock");
    guard.closed += 1;
    guard.close_headers.push(header_pairs(&headers));
    StatusCode::OK
}

async fn attach_run(
    State(inner): State<Shared>,
    Path(run_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut guard = inner.lock().expect("lock");
    guard
        .attach_requests
        .push((run_id, body, header_pairs(&headers)));
    if let Some((status, body)) = guard.attach_error.clone() {
        return (StatusCode::from_u16(status).expect("status"), Json(body));
    }
    (
        StatusCode::OK,
        Json(envelope(json!({
            "attach_id": "att_fake",
            "tab_id": "tab_fake",
            "tab_epoch": 1,
            "control_epoch": 2,
            "opened_by": "agent",
            "holder": "agent",
            "allowed_domains": ["example.com"],
        }))),
    )
}

#[derive(serde::Deserialize)]
struct ReleaseQuery {
    disposition: Option<String>,
}

async fn release_attach(
    State(inner): State<Shared>,
    Path((run_id, attach_id)): Path<(String, String)>,
    Query(query): Query<ReleaseQuery>,
) -> impl IntoResponse {
    let disposition = query.disposition.unwrap_or_else(|| "release".to_owned());
    let state = if disposition == "abort" { "aborted" } else { "released" };
    inner
        .lock()
        .expect("lock")
        .releases
        .push((run_id, attach_id.clone(), disposition));
    (
        StatusCode::OK,
        Json(envelope(json!({"attach_id": attach_id, "state": state}))),
    )
}

#[derive(serde::Deserialize)]
struct TabsQuery {
    grant_id: Option<String>,
}

async fn list_tabs(
    State(inner): State<Shared>,
    Path(run_id): Path<String>,
    Query(query): Query<TabsQuery>,
) -> impl IntoResponse {
    inner
        .lock()
        .expect("lock")
        .tab_queries
        .push((run_id, query.grant_id.unwrap_or_default()));
    (
        StatusCode::OK,
        Json(envelope(json!({"tabs": [{"tabId": "tab_fake", "title": null, "url": null, "active": false}]}))),
    )
}
