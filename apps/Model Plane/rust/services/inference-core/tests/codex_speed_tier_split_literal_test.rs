//! `INFERENCE_SPEED_TIER_SPLIT` is on only for the literal `true` (verification plan 3.4).
//!
//! The switch is read once per process, so this binary sets a value that is not the
//! literal (`True`) before the first call. Every test here must then see the off
//! behaviour, byte for byte the same as with the switch unset: Quick implies the
//! priority tier on a priority-capable model, and `speed_tier` alone asks for nothing.
//! (Surrounding whitespace is trimmed, as every inference-core switch reader does.)

use std::sync::Once;

use inference_core::provider::codex_subscription::CodexSubscriptionProvider;
use inference_core::provider::{ChatMessage, InferRequest, ProviderRouter};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn switch_not_literal() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("INFERENCE_SPEED_TIER_SPLIT", "True"));
}

async fn broker(model: &str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/internal/model-subscriptions/openai-codex/infer"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": { "response": { "requestId": "r", "content": "hello", "modelUsed": model } }
        })))
        .mount(&server)
        .await;
    server
}

fn request(model: &str, budget: i32, speed_tier: &str) -> InferRequest {
    InferRequest {
        request_id: "r".to_owned(),
        org_id: "org-1".to_owned(),
        user_id: "user-1".to_owned(),
        subscription_connection_id: "conn-1".to_owned(),
        model: model.to_owned(),
        messages: vec![ChatMessage {
            compaction_summary: String::new(),
            role: "user".to_owned(),
            content: "hello".to_owned(),
            name: String::new(),
        }],
        thinking_budget_tokens: budget,
        speed_tier: speed_tier.to_owned(),
        ..InferRequest::default()
    }
}

async fn send(model: &str, budget: i32, speed_tier: &str) -> Value {
    switch_not_literal();
    let server = broker(model).await;
    let provider = CodexSubscriptionProvider::new(server.uri(), "test-key", vec![model.to_owned()])
        .expect("provider");
    provider
        .infer(&request(model, budget, speed_tier))
        .await
        .expect("served");
    let requests = server.received_requests().await.expect("recorded");
    serde_json::from_slice(&requests[0].body).expect("json body")
}

#[tokio::test]
async fn a_non_literal_value_keeps_quick_implying_priority() {
    let body = send("gpt-6-astra", 1_024, "").await;
    assert_eq!(body["reasoningEffort"], "low");
    assert_eq!(body["serviceTier"], "priority", "{body}");
}

#[tokio::test]
async fn a_non_literal_value_ignores_speed_tier_on_its_own() {
    // With the split on this would be priority; off, high effort and no prefer_priority ask for none.
    let body = send("gpt-6-astra", 4_096, "priority").await;
    assert_eq!(body["reasoningEffort"], "high");
    assert!(body.get("serviceTier").is_none(), "{body}");
}
