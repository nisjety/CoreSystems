//! OpenAI-compatible provider — calls a configurable base URL with SSE streaming.

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{
    narrow_f64, EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelInfo,
    ProviderError, ProviderRouter,
};

const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";

/// OpenAI-compatible inference provider.
#[derive(Clone)]
enum OpenAiFlavor {
    OpenAi {
        api_base: String,
    },
    Azure {
        endpoint: String,
        api_version: String,
    },
}

/// OpenAI-compatible inference provider.
#[derive(Clone)]
pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    flavor: OpenAiFlavor,
    chat_models: Vec<String>,
    embedding_models: Vec<String>,
}

impl OpenAiProvider {
    /// Create a new OpenAI-compatible provider.
    ///
    /// # Errors
    ///
    /// Returns an error if the API key is empty.
    pub fn new(
        api_key: impl Into<String>,
        api_base: Option<String>,
    ) -> Result<Self, ProviderError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(ProviderError::Unavailable(
                "OPENAI_API_KEY is empty".to_owned(),
            ));
        }

        let api_base = api_base
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_OPENAI_BASE.to_owned());

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            flavor: OpenAiFlavor::OpenAi { api_base },
            chat_models: vec!["gpt-4o-mini".to_owned(), "gpt-5-mini".to_owned()],
            embedding_models: vec![
                "text-embedding-3-small".to_owned(),
                "text-embedding-3-large".to_owned(),
            ],
        })
    }

    /// Create an Azure `OpenAI` provider.
    ///
    /// Azure uses deployment names in the URL and the `api-key` header
    /// instead of `OpenAI`'s `/v1/chat/completions` + bearer token shape.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Unavailable`] if `api_key` or `endpoint` is empty.
    pub fn new_azure(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
        api_version: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(ProviderError::Unavailable(
                "AZURE_OPENAI_API_KEY is empty".to_owned(),
            ));
        }

        let endpoint = endpoint.into();
        if endpoint.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "AZURE_OPENAI_ENDPOINT is empty".to_owned(),
            ));
        }

        let api_version = api_version.into();
        let api_version = if api_version.trim().is_empty() {
            "2025-01-01-preview".to_owned()
        } else {
            api_version
        };

        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            flavor: OpenAiFlavor::Azure {
                endpoint,
                api_version,
            },
            chat_models: Vec::new(),
            embedding_models: Vec::new(),
        })
    }

    /// Override the startup model/deployment catalogue returned by `ListModels`.
    #[must_use]
    pub fn with_model_catalog(
        mut self,
        chat_models: Vec<String>,
        embedding_models: Vec<String>,
    ) -> Self {
        self.chat_models = chat_models;
        self.embedding_models = embedding_models;
        self
    }

    fn chat_completions_url(&self, model: &str) -> String {
        match &self.flavor {
            OpenAiFlavor::OpenAi { api_base } => {
                format!("{}/chat/completions", api_base.trim_end_matches('/'))
            }
            OpenAiFlavor::Azure {
                endpoint,
                api_version,
            } => format!(
                "{}/openai/deployments/{}/chat/completions?api-version={}",
                endpoint.trim_end_matches('/'),
                model,
                api_version
            ),
        }
    }

    fn embeddings_url(&self, model: &str) -> String {
        match &self.flavor {
            OpenAiFlavor::OpenAi { api_base } => {
                format!("{}/embeddings", api_base.trim_end_matches('/'))
            }
            OpenAiFlavor::Azure {
                endpoint,
                api_version,
            } => format!(
                "{}/openai/deployments/{}/embeddings?api-version={}",
                endpoint.trim_end_matches('/'),
                model,
                api_version
            ),
        }
    }

    fn provider_name(&self) -> &'static str {
        match &self.flavor {
            OpenAiFlavor::OpenAi { .. } => "openai",
            OpenAiFlavor::Azure { .. } => "azure-openai",
        }
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.flavor {
            OpenAiFlavor::OpenAi { .. } => request.bearer_auth(&self.api_key),
            OpenAiFlavor::Azure { .. } => request.header("api-key", &self.api_key),
        }
    }
}

fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// Build the `OpenAI` chat completions request body.
fn build_request_body(req: &InferRequest, stream: bool) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        .map(|m| {
            let mut msg = serde_json::json!({
                "role": m.role,
                "content": m.content,
            });
            if !m.name.is_empty() {
                msg["name"] = serde_json::Value::String(m.name.clone());
            }
            msg
        })
        .collect();

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "stream": stream,
    });

    if is_reasoning_model(&req.model) {
        body["max_completion_tokens"] = serde_json::json!(req.max_tokens);
    } else {
        body["max_tokens"] = serde_json::json!(req.max_tokens);
        body["temperature"] = serde_json::json!(req.temperature);
    }

    if let Some(schema) = &req.structured_output_schema {
        if !schema.is_empty() {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(schema) {
                body["response_format"] = serde_json::json!({
                    "type": "json_schema",
                    "json_schema": parsed,
                });
            }
        }
    }

    // chat-parity §2 function-calling: translate tool definitions to the
    // OpenAI `tools`/`tool_choice` shape. Empty → omitted (plain completion).
    if !req.tools.is_empty() {
        let tools: Vec<serde_json::Value> = req
            .tools
            .iter()
            .map(|t| {
                let params = serde_json::from_str::<serde_json::Value>(&t.parameters_json)
                    .unwrap_or_else(|_| serde_json::json!({ "type": "object", "properties": {} }));
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": params,
                    }
                })
            })
            .collect();
        body["tools"] = serde_json::Value::Array(tools);
        let choice = if req.tool_choice.is_empty() {
            "auto"
        } else {
            req.tool_choice.as_str()
        };
        body["tool_choice"] = match choice {
            "auto" | "none" | "required" => serde_json::json!(choice),
            name => serde_json::json!({ "type": "function", "function": { "name": name } }),
        };
    }

    body
}

/// Parse the chat-completion `message.tool_calls` into the internal
/// [`ToolCall`] shape. Returns empty when the model produced a plain answer.
fn parse_tool_calls(json: &serde_json::Value) -> Vec<super::ToolCall> {
    json["choices"][0]["message"]["tool_calls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    let id = tc["id"].as_str()?.to_owned();
                    let name = tc["function"]["name"].as_str()?.to_owned();
                    let arguments_json = tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_owned();
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

fn build_embedding_request_body(req: &EmbedRequest, include_model: bool) -> serde_json::Value {
    let mut body = serde_json::json!({
        "input": req.text,
    });
    if include_model {
        body["model"] = serde_json::Value::String(req.model.clone());
    }
    body
}

fn is_reasoning_model(model: &str) -> bool {
    let normalized = model.to_ascii_lowercase();
    normalized.starts_with("gpt-5") || normalized.starts_with("o1") || normalized.starts_with("o3")
}

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl ProviderRouter for OpenAiProvider {
    fn capabilities(&self) -> super::ProviderCapabilities {
        // GPT-4o / GPT-5 / o-series: tools, vision, reasoning, streaming, and
        // a first-party embeddings API. Conservative context/output bounds.
        super::ProviderCapabilities {
            supports_tools: true,
            supports_vision: true,
            supports_thinking: true,
            supports_streaming: true,
            supports_embeddings: true,
            modalities: vec![
                "chat".to_owned(),
                "vision".to_owned(),
                "embeddings".to_owned(),
            ],
            max_context_tokens: 128_000,
            max_output_tokens: 16_384,
        }
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        let body = build_request_body(req, false);
        let url = self.chat_completions_url(&req.model);

        let response = self
            .apply_auth(self.client.post(&url))
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
                .unwrap_or(1);
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

        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let model_used = json["model"].as_str().unwrap_or("unknown").to_owned();
        let stop_reason = json["choices"][0]["finish_reason"]
            .as_str()
            .unwrap_or("stop")
            .to_owned();
        let input_tokens = to_i32_or_max(json["usage"]["prompt_tokens"].as_i64().unwrap_or(0));
        let output_tokens = to_i32_or_max(json["usage"]["completion_tokens"].as_i64().unwrap_or(0));
        let tool_calls = parse_tool_calls(&json);

        info!(model = %req.model, provider = "openai", "infer completed");

        Ok(InferResponse {
            request_id: req.request_id.clone(),
            content,
            model_used,
            stop_reason,
            input_tokens,
            output_tokens,
            tool_calls,
        })
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let body = build_request_body(req, true);
        let url = self.chat_completions_url(&req.model);

        let response = self
            .apply_auth(self.client.post(&url))
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
                .unwrap_or(1);
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
                            let delta = json["choices"][0]["delta"]["content"]
                                .as_str()
                                .unwrap_or("")
                                .to_owned();
                            let finish = json["choices"][0]["finish_reason"].as_str().unwrap_or("");
                            let done = finish == "stop" || finish == "length";

                            let chunk = InferChunk {
                                request_id: request_id.clone(),
                                delta,
                                done,
                                model_used: json["model"].as_str().unwrap_or(&model).to_owned(),
                                input_tokens: 0,
                                output_tokens: 0,
                            };
                            if tx.send(chunk).await.is_err() {
                                return;
                            }
                            if done {
                                return;
                            }
                        }
                    }
                }
            }

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

    async fn create_embedding(&self, req: &EmbedRequest) -> Result<EmbedResponse, ProviderError> {
        if req.text.trim().is_empty() {
            return Err(ProviderError::InvalidResponse(
                "embedding input text is empty".to_owned(),
            ));
        }

        let include_model = matches!(&self.flavor, OpenAiFlavor::OpenAi { .. });
        let body = build_embedding_request_body(req, include_model);
        let url = self.embeddings_url(&req.model);

        let response = self
            .apply_auth(self.client.post(&url))
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
                .unwrap_or(1);
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

        let values = json["data"][0]["embedding"].as_array().ok_or_else(|| {
            ProviderError::InvalidResponse("missing data[0].embedding array".to_owned())
        })?;
        let vector = values
            .iter()
            .map(|value| {
                value.as_f64().map(narrow_f64).ok_or_else(|| {
                    ProviderError::InvalidResponse(
                        "embedding vector contains non-number".to_owned(),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let model_used = json["model"].as_str().unwrap_or(&req.model).to_owned();
        info!(model = %req.model, provider = self.provider_name(), dims = vector.len(), "embedding completed");

        Ok(EmbedResponse {
            request_id: req.request_id.clone(),
            vector,
            model_used,
            provider_used: self.provider_name().to_owned(),
        })
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        let provider = self.provider_name().to_owned();
        // chat-parity §2: advertise the model's supported feature families so
        // the client can gate the opt-in `features[]` per model.
        let chat_features = self.capabilities().feature_flags();
        self.chat_models
            .iter()
            .map(|id| ModelInfo {
                id: id.clone(),
                provider: provider.clone(),
                modality: "chat".to_owned(),
                streaming: true,
                features: chat_features.clone(),
            })
            .chain(self.embedding_models.iter().map(|id| ModelInfo {
                id: id.clone(),
                provider: provider.clone(),
                modality: "embedding".to_owned(),
                streaming: false,
                ..Default::default()
            }))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(model: &str) -> InferRequest {
        InferRequest {
            request_id: "req-1".to_owned(),
            provider_hint: String::new(),
            model: model.to_owned(),
            messages: vec![crate::provider::ChatMessage {
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            temperature: 0.7,
            max_tokens: 1024,
            structured_output_schema: None,
            zdr: false,
            ..Default::default()
        }
    }

    #[test]
    fn build_request_body_includes_tools_and_choice() {
        let mut req = make_request("gpt-4o");
        req.tools = vec![super::super::ToolDefinition {
            name: "search_web".to_owned(),
            description: "Search the web".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"q":{"type":"string"}}}"#.to_owned(),
        }];
        req.tool_choice = "auto".to_owned();
        let body = build_request_body(&req, false);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "search_web");
        assert_eq!(body["tools"][0]["function"]["parameters"]["type"], "object");
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn build_request_body_omits_tools_when_empty() {
        let body = build_request_body(&make_request("gpt-4o"), false);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn parse_tool_calls_extracts_function_calls() {
        let json = serde_json::json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": { "name": "search_web", "arguments": "{\"q\":\"rust\"}" }
                    }]
                }
            }]
        });
        let calls = parse_tool_calls(&json);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "search_web");
        assert_eq!(calls[0].arguments_json, "{\"q\":\"rust\"}");
    }

    #[test]
    fn parse_tool_calls_empty_for_plain_answer() {
        let json = serde_json::json!({ "choices": [{ "message": { "content": "hi" } }] });
        assert!(parse_tool_calls(&json).is_empty());
    }

    #[test]
    fn azure_url_uses_deployment_and_api_version() {
        let provider = OpenAiProvider::new_azure(
            "key",
            "https://example.openai.azure.com/",
            "2025-01-01-preview",
        )
        .expect("azure provider");

        assert_eq!(
            provider.chat_completions_url("gpt-4o-mini"),
            "https://example.openai.azure.com/openai/deployments/gpt-4o-mini/chat/completions?api-version=2025-01-01-preview",
        );
    }

    #[test]
    fn azure_embedding_url_uses_deployment_and_api_version() {
        let provider = OpenAiProvider::new_azure(
            "key",
            "https://example.openai.azure.com/",
            "2025-01-01-preview",
        )
        .expect("azure provider");

        assert_eq!(
            provider.embeddings_url("text-embedding-3-large"),
            "https://example.openai.azure.com/openai/deployments/text-embedding-3-large/embeddings?api-version=2025-01-01-preview",
        );
    }

    #[test]
    fn gpt5_uses_completion_token_limit_shape() {
        let body = build_request_body(&make_request("gpt-5-mini"), false);

        assert_eq!(body["max_completion_tokens"], 1024);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn standard_models_use_chat_completion_shape() {
        let body = build_request_body(&make_request("gpt-4o-mini"), false);

        assert_eq!(body["max_tokens"], 1024);
        let temperature = body["temperature"].as_f64().expect("temperature");
        assert!((temperature - 0.7).abs() < 0.0001);
        assert!(body.get("max_completion_tokens").is_none());
    }

    #[test]
    fn embedding_body_uses_model_only_for_openai_shape() {
        let req = EmbedRequest {
            request_id: "req-1".to_owned(),
            provider_hint: String::new(),
            text: "hello".to_owned(),
            model: "text-embedding-3-small".to_owned(),
        };

        let openai_body = build_embedding_request_body(&req, true);
        assert_eq!(openai_body["model"], "text-embedding-3-small");
        assert_eq!(openai_body["input"], "hello");

        let azure_body = build_embedding_request_body(&req, false);
        assert!(azure_body.get("model").is_none());
        assert_eq!(azure_body["input"], "hello");
    }
}
