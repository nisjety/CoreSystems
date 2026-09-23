use super::*;
use crate::provider::{ChatMessage, ToolDefinition};
use serde_json::json;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn request() -> InferRequest {
    InferRequest {
        model: "claude-sonnet-4-6".into(),
        max_tokens: 512,
        zdr: true,
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: "Pinned: never send the order".into(),
                name: String::new(),
                compaction_summary: String::new(),
            },
            ChatMessage {
                role: "user".into(),
                content: "Original source: 9 packed, 3 pending".into(),
                name: String::new(),
                compaction_summary: String::new(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "Draft answer".into(),
                name: String::new(),
                compaction_summary: "9 packed; 3 pending; K3 is current".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "Make the same draft warmer".into(),
                name: String::new(),
                compaction_summary: String::new(),
            },
        ],
        ..InferRequest::default()
    }
}

#[test]
fn checkpoint_round_trip_preserves_original_history_pins_and_follow_up() {
    let req = request();
    let body = build_request_body_with_context_management(&req, true);
    assert_eq!(body["system"], req.messages[0].content);
    assert_eq!(body["messages"][0]["content"], req.messages[1].content);
    assert_eq!(
        body["messages"][1]["content"][0],
        json!({"type":"compaction", "content": req.messages[2].compaction_summary})
    );
    assert_eq!(body["messages"][1]["content"][1]["text"], "Draft answer");
    assert_eq!(body["messages"][2]["content"], "Make the same draft warmer");
    // OpenAI-compatible serialization must never leak Anthropic-only fields.
    assert!(serde_json::to_value(&req.messages).unwrap()[2]
        .get("compaction_summary")
        .is_none());
}

#[test]
fn native_off_haiku_and_unknown_models_replay_original_text_without_checkpoint() {
    for (model, enabled) in [
        ("claude-sonnet-4-6", false),
        ("claude-haiku-4-5", true),
        ("claude-unknown", true),
    ] {
        let mut req = request();
        req.model = model.into();
        let body = build_request_body_with_context_management(&req, enabled);
        assert_eq!(body["messages"][0]["content"], req.messages[1].content);
        assert_eq!(body["messages"][1]["content"], "Draft answer");
        assert!(!body["context_management"]["edits"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|e| e["type"] == "compact_20260112"));
    }
}

#[test]
fn decision_rounds_do_not_create_unpersisted_checkpoints() {
    let mut req = request();
    req.tools.push(ToolDefinition {
        name: "create_artifact".into(),
        description: "Draft".into(),
        parameters_json: "{}".into(),
    });
    let body = build_request_body_with_context_management(&req, true);
    assert!(!body["context_management"]["edits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["type"] == "compact_20260112"));
}

#[test]
fn unary_checkpoint_is_separate_and_compaction_cost_is_counted_once() {
    let response = parse_response(
        "r",
        &json!({"model":"claude-sonnet-4-6", "content":[
            {"type":"compaction", "content":"K3 is current"}, {"type":"text", "text":"Customer draft"}
        ], "usage":{"input_tokens":25,"output_tokens":5,"iterations":[
            {"type":"compaction", "input_tokens":150000,"output_tokens":2000},
            {"type":"message", "input_tokens":25,"output_tokens":5,"cache_read_input_tokens":8000}
        ]}}),
    );
    assert_eq!(response.content, "Customer draft");
    assert_eq!(response.compaction_summary, "K3 is current");
    assert_eq!(response.input_tokens, 158025);
    assert_eq!(response.output_tokens, 2005);
    assert_eq!(response.cache_read_input_tokens, 8000);
}

#[tokio::test]
async fn streamed_checkpoint_never_becomes_answer_text_and_returns_on_final_chunk() {
    let server = MockServer::start().await;
    let events = [
        json!({"type":"message_start","message":{"usage":{"input_tokens":150001}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"compaction","content":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"compaction_delta","content":"K3 is current"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Customer draft"}}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ];
    let body = events
        .iter()
        .map(|e| format!("data: {e}\n\n"))
        .collect::<String>();
    Mock::given(method("POST"))
        .and(path("/anthropic/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;
    let provider = AnthropicProvider::new_azure("test-key", server.uri(), vec![]).unwrap();
    // Non-ZDR here: mock deployment has no separately configured ZDR endpoint.
    let mut req = request();
    req.zdr = false;
    let mut stream = provider.infer_stream(&req).await.unwrap();
    let first = stream.recv().await.unwrap();
    assert_eq!(first.delta, "Customer draft");
    assert!(first.compaction_summary.is_empty());
    let final_chunk = stream.recv().await.unwrap();
    assert!(final_chunk.done);
    assert!(final_chunk.delta.is_empty());
    assert_eq!(final_chunk.compaction_summary, "K3 is current");
    assert!(stream.recv().await.is_none());
}
