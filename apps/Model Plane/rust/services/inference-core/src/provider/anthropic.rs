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

    body
}

/// Parse the Anthropic response JSON into our unified response type.
fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn parse_response(request_id: &str, json: &serde_json::Value) -> InferResponse {
    let content = json["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|block| block["text"].as_str())
        .unwrap_or("")
        .to_owned();

    let model_used = json["model"].as_str().unwrap_or("unknown").to_owned();
    let stop_reason = json["stop_reason"]
        .as_str()
        .unwrap_or("end_turn")
        .to_owned();
    let input_tokens = to_i32_or_max(json["usage"]["input_tokens"].as_i64().unwrap_or(0));
    let output_tokens = to_i32_or_max(json["usage"]["output_tokens"].as_i64().unwrap_or(0));

    InferResponse {
        request_id: request_id.to_owned(),
        content,
        model_used,
        stop_reason,
        input_tokens,
        output_tokens,
    }
}

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl ProviderRouter for AnthropicProvider {
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
