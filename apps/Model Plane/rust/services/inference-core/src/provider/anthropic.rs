//! Anthropic Claude provider — calls `https://api.anthropic.com/v1/messages` with SSE streaming.

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{InferChunk, InferRequest, InferResponse, ProviderError, ProviderRouter};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Claude inference provider.
#[derive(Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
}

impl AnthropicProvider {
    /// Create a new Anthropic provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is empty.
    pub fn new(api_key: impl Into<String>) -> Result<Self, ProviderError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(ProviderError::Unavailable(
                "ANTHROPIC_API_KEY is empty".to_owned(),
            ));
        }

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
        })
    }
}

/// Build the Anthropic messages API request body.
fn build_request_body(req: &InferRequest) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "role": m.role,
                "content": m.content,
            })
        })
        .collect();

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "temperature": req.temperature,
    });

    if let Some(schema) = &req.structured_output_schema {
        if !schema.is_empty() {
            body["metadata"] = serde_json::json!({
                "structured_output_schema": schema,
            });
        }
    }

    // chat-parity §2 function-calling: translate tool definitions to the
    // Anthropic `tools`/`tool_choice` shape. Empty → omitted.
    if !req.tools.is_empty() {
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                let schema = serde_json::from_str::<serde_json::Value>(&t.parameters_json)
                    .unwrap_or_else(|_| serde_json::json!({ "type": "object", "properties": {} }));
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": schema,
                })
            })
            .collect();
        body["tools"] = serde_json::Value::Array(tools);
        let tc = if req.tool_choice.is_empty() {
            "auto"
        } else {
            req.tool_choice.as_str()
        };
        body["tool_choice"] = match tc {
            "auto" => serde_json::json!({ "type": "auto" }),
            "none" => serde_json::json!({ "type": "none" }),
            "required" => serde_json::json!({ "type": "any" }),
            name => serde_json::json!({ "type": "tool", "name": name }),
        };
    }

    body
}

/// Parse Anthropic `tool_use` content blocks into the internal [`ToolCall`].
fn parse_tool_calls(json: &serde_json::Value) -> Vec<super::ToolCall> {
    json["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter(|b| b["type"].as_str() == Some("tool_use"))
                .filter_map(|b| {
                    let id = b["id"].as_str()?.to_owned();
                    let name = b["name"].as_str()?.to_owned();
                    let arguments_json =
                        serde_json::to_string(&b["input"]).unwrap_or_else(|_| "{}".to_owned());
                    Some(super::ToolCall {
                        id,
                        name,
                        arguments_json,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Parse the Anthropic response JSON into our unified response type.
fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn parse_response(request_id: &str, json: &serde_json::Value) -> InferResponse {
    // Concatenate all text blocks (a response may interleave text + tool_use).
    let content = json["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|block| block["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let model_used = json["model"].as_str().unwrap_or("unknown").to_owned();
    let stop_reason = json["stop_reason"]
        .as_str()
        .unwrap_or("end_turn")
        .to_owned();
    let input_tokens = to_i32_or_max(json["usage"]["input_tokens"].as_i64().unwrap_or(0));
    let output_tokens = to_i32_or_max(json["usage"]["output_tokens"].as_i64().unwrap_or(0));
    let tool_calls = parse_tool_calls(json);

    InferResponse {
        request_id: request_id.to_owned(),
        content,
        model_used,
        stop_reason,
        input_tokens,
        output_tokens,
        tool_calls,
    }
}

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl ProviderRouter for AnthropicProvider {
    fn capabilities(&self) -> super::ProviderCapabilities {
        // Claude: tools, vision, extended thinking, streaming; 200k context.
        // No first-party embeddings API.
        super::ProviderCapabilities {
            supports_tools: true,
            supports_vision: true,
            supports_thinking: true,
            supports_streaming: true,
            supports_embeddings: false,
            modalities: vec!["chat".to_owned(), "vision".to_owned()],
            max_context_tokens: 200_000,
            max_output_tokens: 8_192,
        }
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        let body = build_request_body(req);

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        // Check rate-limit headers
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1000);
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after * 1000,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "no body".to_owned());
            return Err(ProviderError::Http(format!("{status}: {text}")));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::InvalidResponse(e.to_string()))?;

        info!(model = %req.model, provider = "anthropic", "infer completed");
        Ok(parse_response(&req.request_id, &json))
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let mut body = build_request_body(req);
        body["stream"] = serde_json::Value::Bool(true);

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Http(e.to_string()))?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(1000);
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after * 1000,
            });
        }

        if !response.status().is_success() {
            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "no body".to_owned());
            return Err(ProviderError::Http(format!("{status}: {text}")));
        }

        let request_id = req.request_id.clone();
        let model = req.model.clone();
        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            // Read SSE events from the response body.
            // Anthropic sends `event: content_block_delta` with `data: {...}` lines.
            let mut bytes_stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_result) = bytes_stream.next().await {
                let bytes = match chunk_result {
                    Ok(b) => b,
                    Err(e) => {
                        warn!(error = %e, "stream read error");
                        break;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&bytes));

                // Process complete SSE lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_owned();
                    buffer = buffer[newline_pos + 1..].to_owned();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            let final_chunk = InferChunk {
                                request_id: request_id.clone(),
                                delta: String::new(),
                                done: true,
                                model_used: model.clone(),
                                input_tokens: 0,
                                output_tokens: 0,
                            };
                            let _ = tx.send(final_chunk).await;
                            return;
                        }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            let event_type = json["type"].as_str().unwrap_or("");

                            if event_type == "content_block_delta" {
                                let delta = json["delta"]["text"].as_str().unwrap_or("").to_owned();
                                let chunk = InferChunk {
                                    request_id: request_id.clone(),
                                    delta,
                                    done: false,
                                    model_used: model.clone(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                };
                                if tx.send(chunk).await.is_err() {
                                    return;
                                }
                            } else if event_type == "message_stop" {
                                let final_chunk = InferChunk {
                                    request_id: request_id.clone(),
                                    delta: String::new(),
                                    done: true,
                                    model_used: model.clone(),
                                    input_tokens: json["usage"]["input_tokens"]
                                        .as_i64()
                                        .map_or(0, to_i32_or_max),
                                    output_tokens: json["usage"]["output_tokens"]
                                        .as_i64()
                                        .map_or(0, to_i32_or_max),
                                };
                                let _ = tx.send(final_chunk).await;
                                return;
                            }
                        }
                    }
                }
            }

            // End of stream without explicit done marker
            let final_chunk = InferChunk {
                request_id,
                delta: String::new(),
                done: true,
                model_used: model,
                input_tokens: 0,
                output_tokens: 0,
            };
            let _ = tx.send(final_chunk).await;
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tool_tests {
    use super::{build_request_body, parse_tool_calls};
    use crate::provider::{InferRequest, ToolDefinition};

    #[test]
    fn build_request_body_includes_tools_in_anthropic_shape() {
        let req = InferRequest {
            model: "claude-sonnet-4-20250514".to_owned(),
            max_tokens: 1024,
            tools: vec![ToolDefinition {
                name: "get_weather".to_owned(),
                description: "Get weather".to_owned(),
                parameters_json: r#"{"type":"object","properties":{"city":{"type":"string"}}}"#
                    .to_owned(),
            }],
            tool_choice: "required".to_owned(),
            ..Default::default()
        };
        let body = build_request_body(&req);
        assert_eq!(body["tools"][0]["name"], "get_weather");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        // "required" maps to Anthropic's "any".
        assert_eq!(body["tool_choice"]["type"], "any");
    }

    #[test]
    fn parse_tool_calls_extracts_tool_use_blocks() {
        let json = serde_json::json!({
            "content": [
                { "type": "text", "text": "Let me check." },
                { "type": "tool_use", "id": "tu_1", "name": "get_weather", "input": { "city": "Oslo" } }
            ]
        });
        let calls = parse_tool_calls(&json);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "tu_1");
        assert_eq!(calls[0].name, "get_weather");
        assert!(calls[0].arguments_json.contains("Oslo"));
    }
}
