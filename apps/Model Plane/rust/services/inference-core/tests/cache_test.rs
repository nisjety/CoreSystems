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
        ..Default::default()
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
        ..Default::default()
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

#[test]
fn tools_change_the_cache_key() {
    // A request with the same messages but a tool available must NOT collide
    // with a tool-less one — otherwise it would be served the wrong (tool-less)
    // cached completion.
    let cache = PromptCache::new(300);
    let plain = make_request("gpt-4o-mini", "same question");
    cache.put(&plain, &make_response("plain answer"));

    let mut with_tool = make_request("gpt-4o-mini", "same question");
    with_tool.tools = vec![inference_core::provider::ToolDefinition {
        name: "web_search".to_owned(),
        description: "search".to_owned(),
        parameters_json: "{}".to_owned(),
    }];
    assert!(
        cache.get(&with_tool).is_none(),
        "tool presence must change the cache key"
    );
    // The original tool-less request still hits.
    assert!(cache.get(&plain).is_some());
}

#[test]
fn structured_output_schema_changes_the_cache_key() {
    let cache = PromptCache::new(300);
    let plain = make_request("gpt-4o-mini", "same question");
    cache.put(&plain, &make_response("plain answer"));

    let mut with_schema = make_request("gpt-4o-mini", "same question");
    with_schema.structured_output_schema = Some("{\"type\":\"object\"}".to_owned());
    assert!(
        cache.get(&with_schema).is_none(),
        "structured-output schema must change the cache key"
    );
}
