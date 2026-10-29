//! Integration test: EventSink → EventPublisher → HTTP POST batching.

use quarry_core::event::EventType;
use quarry_core::ids::kinds::RunKind;
use quarry_runtime::{EventPublisher, EventSink};
use tokio::sync::mpsc;
use wiremock::matchers::{header, method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn publisher_posts_batch_with_bearer_auth() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/runs/run_[A-Z0-9]+/events$"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    let (tx, rx) = mpsc::channel(16);
    let client = reqwest::Client::new();
    let publisher = EventPublisher::new(rx, client, mock_server.uri(), "test-key");
    let handle = tokio::spawn(publisher.run());

    let sink = EventSink::new(tx.clone());
    let run_id = RunKind::new();
    for i in 0..3u32 {
        sink.emit(
            run_id.clone(),
            EventType::RunStarted,
            serde_json::json!({ "i": i }),
            format!("k-{i}"),
        )
        .await;
    }

    // Drop all senders so publisher exits cleanly.
    drop(sink);
    drop(tx);

    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .expect("publisher did not exit in time")
        .expect("publisher task panicked");

    let received = mock_server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1, "expected a single batched POST");

    let body: Vec<serde_json::Value> =
        serde_json::from_slice(&received[0].body).expect("body must be a JSON array");
    assert_eq!(body.len(), 3, "batch must contain all 3 events");
    assert_eq!(body[0]["type"], "run_started");
}
