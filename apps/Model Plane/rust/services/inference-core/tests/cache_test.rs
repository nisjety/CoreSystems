//! Unit tests for prompt cache: hit, miss, eviction, and key isolation.

use inference_core::cache::PromptCache;
use inference_core::provider::{ChatMessage, InferRequest, InferResponse};

fn make_request(model: &str, content: &str) -> InferRequest {
    InferRequest {
        request_id: "req-1".to_owned(),
        provider_hint: String::new(),
        model: model.to_owned(),
        messages: vec![ChatMessage {
            role: "user".to_owned(),
            content: content.to_owned(),
            name: String::new(),
        }],
        temperature: 0.7,
        max_tokens: 1024,
        structured_output_schema: None,
        zdr: false,
    }
}

fn make_response(content: &str) -> InferResponse {
    InferResponse {
        request_id: "req-1".to_owned(),
        content: content.to_owned(),
        model_used: "test-model".to_owned(),
        stop_reason: "end_turn".to_owned(),
        input_tokens: 10,
        output_tokens: 5,
    }
}

#[test]
fn miss_on_empty_cache() {
    let cache = PromptCache::new(300);
    let req = make_request("model-a", "hello");
    assert!(cache.get(&req).is_none());
    assert!(cache.is_empty());
}

#[test]
fn hit_after_put() {
    let cache = PromptCache::new(300);
    let req = make_request("model-a", "hello");
    let resp = make_response("world");

    cache.put(&req, &resp);
    assert_eq!(cache.len(), 1);

    let cached = cache.get(&req);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().content, "world");
}

#[test]
fn different_models_are_isolated() {
    let cache = PromptCache::new(300);
    let req_a = make_request("model-a", "hello");
    let req_b = make_request("model-b", "hello");

    cache.put(&req_a, &make_response("from-a"));

    assert!(cache.get(&req_a).is_some());
    assert!(cache.get(&req_b).is_none());
}

#[test]
fn different_messages_are_isolated() {
    let cache = PromptCache::new(300);
    let req_hello = make_request("model-a", "hello");
    let req_goodbye = make_request("model-a", "goodbye");

    cache.put(&req_hello, &make_response("hi"));

    assert!(cache.get(&req_hello).is_some());
    assert!(cache.get(&req_goodbye).is_none());
}

#[test]
fn ttl_eviction() {
    let cache = PromptCache::new(0); // 0 second TTL = immediate expiry
    let req = make_request("model-a", "hello");
    let resp = make_response("world");

    cache.put(&req, &resp);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Entry should be expired
    assert!(cache.get(&req).is_none());
}
