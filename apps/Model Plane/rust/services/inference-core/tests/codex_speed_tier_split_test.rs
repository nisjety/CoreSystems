//! The Codex speed tier split from Quick (W5-095-a), with `INFERENCE_SPEED_TIER_SPLIT=true`.
//!
//! The switch is read once per process, so this binary turns it on before the first
//! call and every test here runs with it on. The off behaviour (Quick implies priority
//! on the five priority-capable models) is pinned by the adapter's own unit tests,
//! which run with the switch unset.

use std::sync::Once;

use inference_core::provider::codex_subscription::CodexSubscriptionProvider;
use inference_core::provider::effort::{self, Family};
use inference_core::provider::model_record;
use inference_core::provider::{ChatMessage, InferRequest, ProviderRouter};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PRIORITY_MODELS: [&str; 5] = [
    "gpt-6-astra",
    "gpt-5.6-sol",
    "gpt-5.6-terra",
    "gpt-5.6-luna",
    "gpt-5.5",
];

fn switch_on() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("INFERENCE_SPEED_TIER_SPLIT", "true"));
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

/// Sends one call and returns the body the broker saw and the tier the response echoed.
async fn send(model: &str, budget: i32, speed_tier: &str) -> (Value, String) {
    switch_on();
    let server = broker(model).await;
    let provider = CodexSubscriptionProvider::new(server.uri(), "test-key", vec![model.to_owned()])
        .expect("provider");
    let response = provider
        .infer(&request(model, budget, speed_tier))
        .await
        .expect("served");
    let requests = server.received_requests().await.expect("recorded");
    let body: Value = serde_json::from_slice(&requests[0].body).expect("json body");
    (body, response.service_tier_used)
}

#[test]
fn the_record_declares_priority_only_for_the_five_codex_models() {
    let aliases = effort::ModelAliases::default();
    for model in PRIORITY_MODELS {
        assert!(
            model_record::advertised(Family::Codex, &aliases, model, 0, 0).priority_tier,
            "{model}"
        );
        assert!(
            model_record::priority_tier_listed(Family::Codex, model),
            "{model}"
        );
    }
    for model in ["gpt-5.3-codex-spark", "gpt-4o-mini"] {
        assert!(
            !model_record::advertised(Family::Codex, &aliases, model, 0, 0).priority_tier,
            "{model}"
        );
    }
    // The tier belongs to the Codex records only.
    assert!(!model_record::priority_tier_listed(
        Family::OpenAi,
        "gpt-5.5"
    ));
}

#[test]
fn the_listing_advertises_priority_tier() {
    let mut models: Vec<String> = PRIORITY_MODELS.iter().map(|m| (*m).to_owned()).collect();
    models.push("gpt-5.3-codex-spark".to_owned());
    let provider =
        CodexSubscriptionProvider::new("http://integration", "key", models).expect("provider");
    for info in provider.list_models() {
        let expected = PRIORITY_MODELS.contains(&info.id.as_str());
        assert_eq!(info.priority_tier, expected, "{}", info.id);
    }
}

#[tokio::test]
async fn split_on_priority_is_chosen_by_speed_tier_independent_of_effort() {
    // High effort with speed_tier priority: priority, which Quick alone used to decide.
    let (body, used) = send("gpt-6-astra", 4_096, "priority").await;
    assert_eq!(body["reasoningEffort"], "high");
    assert_eq!(body["serviceTier"], "priority");
    assert_eq!(used, "priority");
}

#[tokio::test]
async fn split_on_quick_no_longer_implies_priority() {
    let (body, used) = send("gpt-6-astra", 1_024, "").await;
    assert_eq!(body["reasoningEffort"], "low");
    assert!(body.get("serviceTier").is_none(), "{body}");
    assert_eq!(used, "");
    let (body, _) = send("gpt-6-astra", 1_024, "standard").await;
    assert!(body.get("serviceTier").is_none(), "{body}");
}

#[tokio::test]
async fn split_on_a_model_without_the_tier_never_gets_priority() {
    let (body, used) = send("gpt-5.3-codex-spark", 4_096, "priority").await;
    assert!(body.get("serviceTier").is_none(), "{body}");
    assert_eq!(used, "");
}
