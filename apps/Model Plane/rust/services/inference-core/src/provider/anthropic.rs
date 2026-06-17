//! Anthropic Claude provider.
//!
//! Two flavors share one Anthropic Messages request/response codec:
//! * **Direct** — `https://api.anthropic.com/v1/messages` (first-party).
//! * **Azure Foundry** — the Claude deployments on an Azure AI Foundry
//!   resource, served at `https://<resource>.services.ai.azure.com/anthropic/v1/messages`.
//!   Verified live: the body is the *native* Anthropic Messages shape and auth
//!   is the **same `x-api-key` header** as the direct API (the `api-key` header
//!   and an `api-version` query param both 401 — do not add them). This is the
//!   fix for the direct API returning 400 "credit balance too low".

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{InferChunk, InferRequest, InferResponse, ModelInfo, ProviderError, ProviderRouter};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Default Claude model used when a request leaves the model unspecified
/// ("Velion Auto"). The fallback chain substitutes this when Anthropic is the
/// provider serving an unpinned request, so chat works against an
/// Anthropic-only deployment with no client- or operator-chosen model.
pub(crate) const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-20250514";

/// Default Claude deployment for the Azure Foundry flavor when a request leaves
/// the model unspecified. Names the cheapest chat-capable Claude deployment so
/// an unpinned Claude request stays economical.
pub(crate) const DEFAULT_AZURE_ANTHROPIC_MODEL: &str = "claude-haiku-4-5";

/// Which Anthropic Messages endpoint this provider targets.
#[derive(Clone)]
enum AnthropicFlavor {
    /// First-party `api.anthropic.com`.
    Direct,
    /// Azure AI Foundry resource. `endpoint` is the resource base
    /// (e.g. `https://<resource>.services.ai.azure.com`); the Messages route
    /// `/anthropic/v1/messages` is appended. `models` is the deployed Claude
    /// catalog used for `list_models`.
    Azure {
        endpoint: String,
        models: Vec<String>,
    },
}

/// Anthropic Claude inference provider.
#[derive(Clone)]
pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    flavor: AnthropicFlavor,
}

impl AnthropicProvider {
    /// Create a new direct (`api.anthropic.com`) Anthropic provider.
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
            flavor: AnthropicFlavor::Direct,
        })
    }

    /// Create an Azure AI Foundry Anthropic provider.
    ///
    /// `endpoint` is the resource base (the `services.ai.azure.com` host —
    /// `cognitiveservices.azure.com` 401s for the Anthropic route). The Claude
    /// Messages route and `x-api-key` auth are appended at call time.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Unavailable`] if `api_key` or `endpoint` is empty.
    pub fn new_azure(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
        models: Vec<String>,
    ) -> Result<Self, ProviderError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(ProviderError::Unavailable(
                "AZURE_ANTHROPIC_API_KEY is empty".to_owned(),
            ));
        }
        let endpoint = endpoint.into();
        if endpoint.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "AZURE_ANTHROPIC_ENDPOINT is empty".to_owned(),
            ));
        }
        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            flavor: AnthropicFlavor::Azure { endpoint, models },
        })
    }

    /// The Messages API URL for the active flavor.
    fn messages_url(&self) -> String {
        match &self.flavor {
            AnthropicFlavor::Direct => ANTHROPIC_API_URL.to_owned(),
            AnthropicFlavor::Azure { endpoint, .. } => {
                format!("{}/anthropic/v1/messages", endpoint.trim_end_matches('/'))
            }
        }
    }

    /// Provider name reported in logs and `list_models`.
    fn provider_name(&self) -> &'static str {
        match &self.flavor {
            AnthropicFlavor::Direct => "anthropic",
            AnthropicFlavor::Azure { .. } => "azure-anthropic",
        }
    }
}

/// True for the economy Claude tier (Haiku) so the UI can group cheap models.
fn is_cheap_claude(model: &str) -> bool {
    model.to_ascii_lowercase().contains("haiku")
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

    /// Advertise the Claude chat catalog so `/v1/models` is populated (the SPA
    /// model picker reads this). Direct uses the first-party model ids; Azure
    /// Foundry uses the deployed Claude deployment names from config.
    fn list_models(&self) -> Vec<ModelInfo> {
        let provider = self.provider_name().to_owned();
        let ids: Vec<String> = match &self.flavor {
            AnthropicFlavor::Direct => [
                DEFAULT_ANTHROPIC_MODEL,
                "claude-opus-4-20250514",
                "claude-3-5-haiku-20241022",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect(),
            AnthropicFlavor::Azure { models, .. } => models.clone(),
        };
        ids.into_iter()
            .map(|id| {
                let cheap = is_cheap_claude(&id);
                ModelInfo {
                    id,
                    provider: provider.clone(),
                    modality: "chat".to_owned(),
                    streaming: true,
                    features: vec![
                        "tools".to_owned(),
                        "vision".to_owned(),
                        "reasoning".to_owned(),
                    ],
                    cheap,
                }
            })
            .collect()
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        let body = build_request_body(req);

        let response = self
            .client
            .post(self.messages_url())
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

        info!(model = %req.model, provider = self.provider_name(), "infer completed");
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
            .post(self.messages_url())
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

#[cfg(test)]
mod flavor_tests {
    use super::{is_cheap_claude, AnthropicProvider, ProviderRouter};

    #[test]
    fn direct_flavor_uses_first_party_messages_url() {
        let p = AnthropicProvider::new("k").expect("direct provider");
        assert_eq!(p.messages_url(), "https://api.anthropic.com/v1/messages");
        assert_eq!(p.provider_name(), "anthropic");
    }

    #[test]
    fn azure_flavor_appends_anthropic_messages_route() {
        let p = AnthropicProvider::new_azure(
            "k",
            "https://cloude-ai-resource.services.ai.azure.com/",
            vec!["claude-haiku-4-5".to_owned()],
        )
        .expect("azure provider");
        // Trailing slash on the endpoint must not double up.
        assert_eq!(
            p.messages_url(),
            "https://cloude-ai-resource.services.ai.azure.com/anthropic/v1/messages",
        );
        assert_eq!(p.provider_name(), "azure-anthropic");
    }

    #[test]
    fn azure_list_models_uses_configured_deployments_with_cheap_flag() {
        let p = AnthropicProvider::new_azure(
            "k",
            "https://cloude-ai-resource.services.ai.azure.com",
            vec!["claude-haiku-4-5".to_owned(), "claude-opus-4-8".to_owned()],
        )
        .expect("azure provider");
        let models = p.list_models();
        assert_eq!(models.len(), 2);
        assert!(models.iter().all(|m| m.provider == "azure-anthropic"));
        let haiku = models.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
        assert!(haiku.cheap, "haiku is the economy tier");
        let opus = models.iter().find(|m| m.id == "claude-opus-4-8").unwrap();
        assert!(!opus.cheap, "opus is not cheap");
    }

    #[test]
    fn empty_azure_key_or_endpoint_is_rejected() {
        assert!(AnthropicProvider::new_azure("", "https://x", vec![]).is_err());
        assert!(AnthropicProvider::new_azure("k", "   ", vec![]).is_err());
    }

    #[test]
    fn cheap_classifier_matches_only_haiku() {
        assert!(is_cheap_claude("claude-haiku-4-5"));
        assert!(is_cheap_claude("claude-3-5-haiku-20241022"));
        assert!(!is_cheap_claude("claude-opus-4-8"));
        assert!(!is_cheap_claude("claude-sonnet-4-6"));
    }
}
