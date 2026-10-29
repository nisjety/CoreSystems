//! Mock-server contract tests for `MeilisearchClient`, mirroring
//! `quickwit-adapter-rs/tests/quickwit_preflight.rs`'s style: a real HTTP
//! server on an ephemeral port, real requests over the wire, asserting the
//! client sends the auth header and request bodies Meilisearch actually
//! expects — without needing a live Meilisearch instance.

use axum::{
    extract::{Json as JsonExtract, Path, State},
    http::HeaderMap,
    routing::{patch, post},
    Json, Router,
};
use meilisearch_adapter_rs::meilisearch::MeilisearchClient;
use meilisearch_adapter_rs::model::KeywordDocument;
use serde_json::Value;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Captured {
    auth_headers: Vec<Option<String>>,
    settings_bodies: Vec<Value>,
    upsert_bodies: Vec<Value>,
    delete_bodies: Vec<Value>,
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

async fn create_index(
    State(state): State<Arc<Mutex<Captured>>>,
    headers: HeaderMap,
    JsonExtract(_body): JsonExtract<Value>,
) -> axum::http::StatusCode {
    state.lock().unwrap().auth_headers.push(bearer(&headers));
    axum::http::StatusCode::ACCEPTED
}

async fn update_settings(
    State(state): State<Arc<Mutex<Captured>>>,
    headers: HeaderMap,
    Path(_index): Path<String>,
    JsonExtract(body): JsonExtract<Value>,
) -> axum::http::StatusCode {
    let mut s = state.lock().unwrap();
    s.auth_headers.push(bearer(&headers));
    s.settings_bodies.push(body);
    axum::http::StatusCode::ACCEPTED
}

async fn add_documents(
    State(state): State<Arc<Mutex<Captured>>>,
    headers: HeaderMap,
    Path(_index): Path<String>,
    JsonExtract(body): JsonExtract<Value>,
) -> axum::http::StatusCode {
    let mut s = state.lock().unwrap();
    s.auth_headers.push(bearer(&headers));
    s.upsert_bodies.push(body);
    axum::http::StatusCode::ACCEPTED
}

async fn delete_by_filter(
    State(state): State<Arc<Mutex<Captured>>>,
    headers: HeaderMap,
    Path(_index): Path<String>,
    JsonExtract(body): JsonExtract<Value>,
) -> axum::http::StatusCode {
    let mut s = state.lock().unwrap();
    s.auth_headers.push(bearer(&headers));
    s.delete_bodies.push(body);
    axum::http::StatusCode::ACCEPTED
}

async fn health() -> Json<Value> {
    Json(serde_json::json!({ "status": "available" }))
}

async fn server() -> (String, Arc<Mutex<Captured>>) {
    let state = Arc::new(Mutex::new(Captured::default()));
    let app = Router::new()
        .route("/indexes", post(create_index))
        .route("/indexes/{index}/settings", patch(update_settings))
        .route("/indexes/{index}/documents", post(add_documents))
        .route("/indexes/{index}/documents/delete", post(delete_by_filter))
        .route("/health", axum::routing::get(health))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server");
    });
    (format!("http://{address}"), state)
}

fn sample_doc() -> KeywordDocument {
    KeywordDocument {
        id: "kid-1".into(),
        org_id: "org-a".into(),
        document_id: "doc-1".into(),
        knowledge_id: "kid-1".into(),
        chunk_index: 0,
        source: "upload".into(),
        title: "Invoice 2039".into(),
        body: "SKU-88213 shipped".into(),
        content_hash: None,
        acl_tags: vec![],
        updated_at: 1_754_000_000,
    }
}

#[tokio::test]
async fn ensure_index_sends_the_bearer_and_org_id_filterable_setting() {
    let (base_url, state) = server().await;
    let client =
        MeilisearchClient::new(base_url, "dataplane-knowledge", "test-master-key").expect("client");

    client.ensure_index().await.expect("ensure_index");

    let captured = state.lock().unwrap();
    assert!(captured
        .auth_headers
        .iter()
        .all(|h| h.as_deref() == Some("Bearer test-master-key")));
    let settings = &captured.settings_bodies[0];
    let filterable = settings["filterableAttributes"]
        .as_array()
        .expect("filterableAttributes array");
    assert!(filterable.contains(&Value::String("org_id".into())));
    assert!(filterable.contains(&Value::String("document_id".into())));
}

#[tokio::test]
async fn upsert_sends_the_document_batch_as_a_json_array() {
    let (base_url, state) = server().await;
    let client = MeilisearchClient::new(base_url, "dataplane-knowledge", "k").expect("client");

    client.upsert(&[sample_doc()]).await.expect("upsert");

    let captured = state.lock().unwrap();
    let body = &captured.upsert_bodies[0];
    assert_eq!(body[0]["id"], "kid-1");
    assert_eq!(body[0]["org_id"], "org-a");
    assert_eq!(body[0]["body"], "SKU-88213 shipped");
}

#[tokio::test]
async fn upsert_with_no_documents_never_calls_the_server() {
    let (base_url, state) = server().await;
    let client = MeilisearchClient::new(base_url, "dataplane-knowledge", "k").expect("client");

    client.upsert(&[]).await.expect("no-op upsert");

    assert!(state.lock().unwrap().upsert_bodies.is_empty());
}

#[tokio::test]
async fn delete_by_document_sends_an_org_and_document_scoped_filter() {
    let (base_url, state) = server().await;
    let client = MeilisearchClient::new(base_url, "dataplane-knowledge", "k").expect("client");

    client
        .delete_by_document("org-a", "doc-1")
        .await
        .expect("delete_by_document");

    let captured = state.lock().unwrap();
    let filter = captured.delete_bodies[0]["filter"]
        .as_str()
        .expect("filter string");
    assert_eq!(filter, "org_id = \"org-a\" AND document_id = \"doc-1\"");
}

#[tokio::test]
async fn healthy_reports_true_only_on_a_200_from_the_health_endpoint() {
    let (base_url, _state) = server().await;
    let client = MeilisearchClient::new(base_url, "dataplane-knowledge", "k").expect("client");
    assert!(client.healthy().await);

    // An unreachable host must report unhealthy, not panic or hang.
    let dead_client =
        MeilisearchClient::new("http://127.0.0.1:1", "dataplane-knowledge", "k").expect("client");
    assert!(!dead_client.healthy().await);
}
