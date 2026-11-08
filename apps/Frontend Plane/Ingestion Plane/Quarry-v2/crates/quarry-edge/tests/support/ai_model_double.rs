//! A strict double of the Model Plane gateway's `POST /v1/ai/chat`, for the AI
//! format tests of the edge (INNH-1 U3). Declared with
//! `#[path = "support/ai_model_double.rs"] mod ai_model_double;` by each test
//! binary that needs it, so `support/mod.rs` is not edited.
//!
//! Strict like the real route (DEC-17):
//! - 401 unless both `Authorization` and `x-inference-authorization` carry a bearer;
//! - 400 for a body field the gateway does not read, an object-typed
//!   `structured_output_schema` (the gateway's field is an `Option<String>`) or a
//!   `zdr` that is not a bool;
//! - 404 for any other path (`/v1/invoke` among them).
//!
//! It records every accepted request, so a test can say what was sent and what was
//! not, and it counts the requests it had to reject (a test asserts that is zero).

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, Response};
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct Seen {
    pub headers: HashMap<String, String>,
    pub body: Value,
    /// The word after `Task:` in the system message ("extract", "summarize", ...).
    pub task: String,
}

impl Seen {
    pub fn system(&self) -> &str {
        self.body["messages"][0]["content"].as_str().unwrap_or("")
    }

    pub fn user(&self) -> &str {
        self.body["messages"][1]["content"].as_str().unwrap_or("")
    }
}

pub struct Reply {
    pub status: u16,
    pub body: String,
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn status(status: u16) -> Self {
        Self {
            status,
            body: json!({ "error": "scripted" }).to_string(),
            headers: Vec::new(),
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

/// A 200 chat answer.
pub fn chat(content: &str, input_tokens: u64, output_tokens: u64) -> Reply {
    Reply {
        status: 200,
        body: json!({
            "id": "01HTEST",
            "content": content,
            "model_used": "verevon-test",
            "usage": { "input_tokens": input_tokens, "output_tokens": output_tokens },
        })
        .to_string(),
        headers: Vec::new(),
    }
}

type Script = Box<dyn Fn(&Seen) -> Reply + Send + Sync>;

struct Shared {
    script: Script,
    seen: Mutex<Vec<Seen>>,
    rejected: AtomicUsize,
    other_paths: AtomicUsize,
}

pub struct Double {
    pub uri: String,
    shared: Arc<Shared>,
}

impl Double {
    pub fn seen(&self) -> Vec<Seen> {
        self.shared.seen.lock().unwrap().clone()
    }

    /// Accepted chat calls.
    pub fn calls(&self) -> usize {
        self.seen().len()
    }

    /// Every request the double got was well formed and on the chat path.
    pub fn assert_clean(&self) {
        assert_eq!(
            self.shared.rejected.load(Ordering::SeqCst),
            0,
            "the double rejected a request"
        );
        assert_eq!(
            self.shared.other_paths.load(Ordering::SeqCst),
            0,
            "a request reached a path other than /v1/ai/chat"
        );
    }
}

const GATEWAY_FIELDS: [&str; 7] = [
    "messages",
    "model",
    "provider",
    "subscription_connection_id",
    "stream",
    "structured_output_schema",
    "zdr",
];

fn reject(shared: &Shared, status: u16, why: &str) -> Response<Body> {
    shared.rejected.fetch_add(1, Ordering::SeqCst);
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(json!({ "error": why }).to_string()))
        .unwrap()
}

async fn chat_route(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    let bearer = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("Bearer ") && value.len() > 7)
    };
    if !bearer("authorization") || !bearer("x-inference-authorization") {
        return reject(&shared, 401, "both tokens are required");
    }
    let Ok(parsed) = serde_json::from_slice::<Value>(&body) else {
        return reject(&shared, 400, "not json");
    };
    let Some(object) = parsed.as_object() else {
        return reject(&shared, 400, "not an object");
    };
    if let Some(key) = object
        .keys()
        .find(|key| !GATEWAY_FIELDS.contains(&key.as_str()))
    {
        return reject(&shared, 400, &format!("unknown field {key}"));
    }
    let messages_ok = object
        .get("messages")
        .and_then(Value::as_array)
        .is_some_and(|messages| {
            !messages.is_empty()
                && messages
                    .iter()
                    .all(|m| m["role"].is_string() && m["content"].is_string())
        });
    if !messages_ok {
        return reject(&shared, 400, "messages");
    }
    if !matches!(
        object.get("structured_output_schema"),
        None | Some(Value::Null) | Some(Value::String(_))
    ) {
        return reject(&shared, 400, "structured_output_schema must be a string");
    }
    if !matches!(object.get("zdr"), None | Some(Value::Bool(_))) {
        return reject(&shared, 400, "zdr");
    }
    if !object.get("model").is_some_and(Value::is_string) {
        return reject(&shared, 400, "model");
    }
    let system = parsed["messages"][0]["content"].as_str().unwrap_or("");
    let task = system
        .split("Task: ")
        .nth(1)
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect::<String>()
        })
        .unwrap_or_default();
    let seen = Seen {
        headers: headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_str().unwrap_or("").to_owned()))
            .collect(),
        body: parsed.clone(),
        task,
    };
    shared.seen.lock().unwrap().push(seen.clone());
    let reply = (shared.script)(&seen);
    let mut builder = Response::builder()
        .status(reply.status)
        .header("content-type", "application/json");
    for (name, value) in &reply.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    builder.body(Body::from(reply.body)).unwrap()
}

async fn other_route(State(shared): State<Arc<Shared>>) -> Response<Body> {
    shared.other_paths.fetch_add(1, Ordering::SeqCst);
    Response::builder().status(404).body(Body::empty()).unwrap()
}

/// Start the double. `script` answers each accepted chat call.
pub async fn start(script: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Double {
    let shared = Arc::new(Shared {
        script: Box::new(script),
        seen: Mutex::new(Vec::new()),
        rejected: AtomicUsize::new(0),
        other_paths: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route("/v1/ai/chat", post(chat_route))
        .fallback(other_route)
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Double {
        uri: format!("http://{addr}"),
        shared,
    }
}

/// A model that does its job on the page of `tests/ai_formats.rs`.
pub fn standard(seen: &Seen) -> Reply {
    match seen.task.as_str() {
        "extract" => chat(r#"{"title":"Kaffebrenneriet AS","price":199}"#, 1200, 40),
        "summarize" => chat("Et kort sammendrag av siden.", 1000, 12),
        "answer" => chat(
            r#"{"found":true,"answer":"199 kr","quote":"Prisen for en pose er 199 kr."}"#,
            1000,
            20,
        ),
        "quote" => chat(
            r#"{"quotes":["Levering tar to dager.","Prisen for en pose er 199 kr.","Æøå er norske bokstaver"]}"#,
            1000,
            30,
        ),
        other => panic!("unexpected task {other:?}"),
    }
}
