//! A fake quarry-edge `/v1/agent/*` on `127.0.0.1:0`, driven through the real
//! `QuarryAgentClient`. It records every request body and header it receives and
//! replays scripted step responses.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, post};
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
    closed: u32,
    step_counter: u32,
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

    /// How many times the run was released.
    pub fn closed_runs(&self) -> u32 {
        self.inner.lock().expect("lock").closed
    }
}

fn envelope(data: Value) -> Value {
    json!({"data": data, "meta": {}, "error": null})
}

async fn start_run(State(inner): State<Shared>, Json(body): Json<Value>) -> impl IntoResponse {
    let mut guard = inner.lock().expect("lock");
    guard.start_requests.push(body);
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
    Path(_run_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let mut guard = inner.lock().expect("lock");
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

async fn close_run(State(inner): State<Shared>, Path(_run_id): Path<String>) -> StatusCode {
    inner.lock().expect("lock").closed += 1;
    StatusCode::OK
}
