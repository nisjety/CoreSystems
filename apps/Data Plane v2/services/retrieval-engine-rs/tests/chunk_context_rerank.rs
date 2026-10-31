//! CR-T15: what the reranker actually receives. The real `RerankClient` posts over HTTP to a
//! localhost stand-in for the Cohere route, which keeps every raw request body. The only double is
//! the provider. The request, its serialization and the document composition are production code.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use retrieval_engine::pipeline::types::ScoredCandidate;
use retrieval_engine::search::rerank::RerankClient;

/// Every request body the stand-in received, verbatim, in arrival order.
#[derive(Clone, Default)]
struct Received(Arc<Mutex<Vec<String>>>);

async fn answer(State(received): State<Received>, body: Bytes) -> Json<Value> {
    received
        .0
        .lock()
        .expect("the record is not poisoned")
        .push(String::from_utf8(body.to_vec()).expect("the request body is UTF-8"));
    Json(json!({
        "results": [
            { "index": 0, "relevance_score": 0.9 },
            { "index": 1, "relevance_score": 0.5 }
        ]
    }))
}

async fn start_provider() -> (String, Received) {
    let received = Received::default();
    let app = Router::new()
        .route("/v2/rerank", post(answer))
        .with_state(received.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the provider stand-in");
    let addr: SocketAddr = listener.local_addr().expect("local address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}/v2/rerank"), received)
}

fn candidate(text: &str, context: Option<&str>) -> ScoredCandidate {
    let mut candidate = ScoredCandidate {
        knowledge_id: format!("k-{text}"),
        document_id: "d1".to_string(),
        text: text.to_string(),
        dense_score: 0.0,
        sparse_score: 0.0,
        rerank_score: 0.0,
        final_score: 0.0,
        chunk_index: 0,
        metadata: HashMap::new(),
    };
    candidate.attach_chunk_context(context.map(str::to_owned));
    candidate
}

fn client(endpoint: &str, include_context: bool) -> RerankClient {
    RerankClient::with_endpoint("test-key", "rerank-test", endpoint, false)
        .with_chunk_context_input(include_context)
}

fn bodies(received: &Received) -> Vec<String> {
    received
        .0
        .lock()
        .expect("the record is not poisoned")
        .clone()
}

/// Flag off: the provider receives the raw chunk texts, byte for byte the body the code always sent.
/// A context on a candidate is not consulted at all.
#[tokio::test]
async fn flag_off_posts_the_raw_chunk_texts_byte_for_byte() {
    let (endpoint, received) = start_provider().await;
    let candidates = [
        candidate("Margin: 31%.", Some("Fra ACMEs Q2-rapport.")),
        candidate("Lønn og pensjon.", None),
    ];
    client(&endpoint, false)
        .rerank("margin", &candidates, 2)
        .await
        .expect("the stand-in answers");

    let bodies = bodies(&received);
    assert_eq!(bodies.len(), 1, "one provider call");
    assert_eq!(
        bodies[0],
        r#"{"model":"rerank-test","query":"margin","documents":["Margin: 31%.","Lønn og pensjon."],"top_n":2}"#
    );
}

/// Flag on: each chunk reaches the provider with its context in force ahead of it. A candidate with
/// no context is sent as its raw text.
#[tokio::test]
async fn flag_on_sends_each_chunk_with_its_context_ahead_of_it() {
    let (endpoint, received) = start_provider().await;
    let candidates = [
        candidate("Margin: 31%.", Some("  Fra ACMEs Q2-rapport.  ")),
        candidate("Lønn og pensjon.", None),
    ];
    client(&endpoint, true)
        .rerank("margin", &candidates, 2)
        .await
        .expect("the stand-in answers");

    let bodies = bodies(&received);
    assert_eq!(bodies.len(), 1, "one provider call");
    let body: Value = serde_json::from_str(&bodies[0]).expect("the body is JSON");
    assert_eq!(body["query"], "margin");
    assert_eq!(
        body["documents"],
        json!(["Fra ACMEs Q2-rapport.\n\nMargin: 31%.", "Lønn og pensjon."])
    );
}

/// A rerank above the client's split point is sharded, and every shard is composed, not only the
/// first. The shards arrive concurrently, so the check is over the set of documents.
#[tokio::test]
async fn flag_on_composes_every_shard_of_a_large_rerank() {
    let (endpoint, received) = start_provider().await;
    let candidates: Vec<ScoredCandidate> = (0..60)
        .map(|index| {
            let context = format!("Kontekst {index}.");
            candidate(&format!("chunk {index}"), Some(context.as_str()))
        })
        .collect();
    client(&endpoint, true)
        .rerank("q", &candidates, 5)
        .await
        .expect("the stand-in answers");

    let bodies = bodies(&received);
    assert!(
        bodies.len() >= 2,
        "60 candidates are sharded: {}",
        bodies.len()
    );
    let mut documents: Vec<String> = Vec::new();
    for body in &bodies {
        let value: Value = serde_json::from_str(body).expect("the body is JSON");
        for document in value["documents"].as_array().expect("documents") {
            documents.push(document.as_str().expect("a string document").to_owned());
        }
    }
    assert_eq!(documents.len(), 60, "every candidate is sent exactly once");
    for index in 0..60 {
        let composed = format!("Kontekst {index}.\n\nchunk {index}");
        assert!(documents.contains(&composed), "{composed} was not sent");
    }
}

/// The context is an input to the provider and never part of the answer: the candidates the rerank
/// returns serialize without it, so no response, cache entry or trace can carry it.
#[tokio::test]
async fn the_context_never_comes_back_in_the_answer() {
    const MARKER: &str = "CONTEXT-MARKER-9d1e";
    let (endpoint, _received) = start_provider().await;
    let context = format!("{MARKER} Fra ACMEs Q2-rapport.");
    let candidates = [candidate("Margin: 31%.", Some(context.as_str()))];
    let reranked = client(&endpoint, true)
        .rerank("margin", &candidates, 1)
        .await
        .expect("the stand-in answers");
    assert_eq!(reranked.len(), 1);
    let wire = serde_json::to_string(&reranked).expect("candidates serialize");
    assert!(!wire.contains(MARKER), "{wire}");
}
