use axum::{extract::State, routing::post, Json, Router};
use quickwit_adapter_rs::quickwit::QuickwitClient;
use serde_json::Value;
use std::sync::Arc;

async fn search(State(response): State<Arc<Value>>, Json(request): Json<Value>) -> Json<Value> {
    assert_eq!(request["query"], "org_id:\"org-a\"");
    assert_eq!(request["max_hits"], 0);
    Json(response.as_ref().clone())
}

async fn client(response: Value) -> QuickwitClient {
    let app = Router::new()
        .route("/api/v1/test-index/search", post(search))
        .with_state(Arc::new(response));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server");
    });
    QuickwitClient::new(
        format!("http://{address}"),
        "test-index",
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../infra/quickwit/dataplane-corpus-index.yaml"
        ),
    )
    .expect("client")
}

#[tokio::test]
async fn tenant_preflight_reads_count_without_mutating_quickwit() {
    let quickwit = client(serde_json::json!({"num_hits": 0})).await;
    assert_eq!(quickwit.count_documents(Some("org-a")).await.unwrap(), 0);
}

#[tokio::test]
async fn malformed_count_response_fails_closed() {
    let quickwit = client(serde_json::json!({"hits": []})).await;
    assert!(quickwit.count_documents(Some("org-a")).await.is_err());
}
