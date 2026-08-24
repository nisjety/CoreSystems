//! OpenAI-compatible provider — calls a configurable base URL with SSE streaming.

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::zdr::ZdrAttestation;
use super::{
    narrow_f64, EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelInfo,
    ProviderError, ProviderRouter,
};

const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";

/// Default chat model used when a request leaves the model unspecified
/// ("Verevon Auto"). The fallback chain substitutes this when OpenAI/Azure is
/// the provider serving an unpinned request.
pub(crate) const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";

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
    /// Azure AI Foundry's unified "AI Model Inference API" — the route `MaaS`
    /// deployments (Cohere Command A Plus, `DeepSeek`, etc.) speak in this
    /// account, confirmed live: `POST {endpoint}/chat/completions?api-version=…`
    /// with `api-key` auth and the deployment name in the JSON body's `model`
    /// field, not the URL path. Distinct from both existing flavors: it needs
    /// Azure's `api-key` header (not `OpenAi`'s bearer token) but `OpenAi`'s
    /// path-only URL shape (not Azure's `/openai/deployments/{model}/...`).
    /// The request/response bodies are genuinely OpenAI-chat-completions-shaped
    /// (verified against a real deployment), so this reuses every existing
    /// builder/parser below rather than adding a new module.
    UnifiedAzureAi {
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
    /// Evidence-bound ZDR attestation for this exact deployment, or `None` when
    /// the operator makes no ZDR claim.
    zdr: Option<Arc<ZdrAttestation>>,
    /// Registry id override. `None` uses the flavor-derived name (`openai` /
    /// `azure-openai`); a third-party OpenAI-compatible endpoint sets its own so
    /// it is addressable alongside them instead of colliding on one id.
    provider_id: Option<String>,
    /// The strongest residency guarantee this endpoint honors.
    residency: super::Residency,
    /// Whether this endpoint serves only its declared catalog.
    exclusive_catalog: bool,
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
            client: crate::provider::provider_http_client(),
            api_key,
            flavor: OpenAiFlavor::OpenAi { api_base },
            chat_models: vec!["gpt-4o-mini".to_owned(), "gpt-5-mini".to_owned()],
            embedding_models: vec![
                "text-embedding-3-small".to_owned(),
                "text-embedding-3-large".to_owned(),
            ],
            zdr: None,
            provider_id: None,
            residency: super::Residency::Global,
            exclusive_catalog: false,
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
            client: crate::provider::provider_http_client(),
            api_key,
            flavor: OpenAiFlavor::Azure {
                endpoint,
                api_version,
            },
            chat_models: Vec::new(),
            embedding_models: Vec::new(),
            zdr: None,
            provider_id: None,
            residency: super::Residency::Global,
            exclusive_catalog: false,
        })
    }

    /// Create a provider for Azure AI Foundry's unified "AI Model Inference
    /// API" — the wire shape `MaaS` deployments (Cohere Command A Plus, etc.)
    /// speak, distinct from classic Azure `OpenAI` deployments. Confirmed
    /// live: `POST {endpoint}/chat/completions?api-version=…` with `api-key`
    /// auth and the deployment name in the request body's `model` field.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::Unavailable`] if `api_key` or `endpoint` is empty.
    pub fn new_azure_ai_unified(
        api_key: impl Into<String>,
        endpoint: impl Into<String>,
        api_version: impl Into<String>,
    ) -> Result<Self, ProviderError> {
        let api_key = api_key.into();
        if api_key.is_empty() {
            return Err(ProviderError::Unavailable(
                "azure ai unified provider api key is empty".to_owned(),
            ));
        }

        let endpoint = endpoint.into();
        if endpoint.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "azure ai unified provider endpoint is empty".to_owned(),
            ));
        }

        let api_version = api_version.into();
        let api_version = if api_version.trim().is_empty() {
            "2024-05-01-preview".to_owned()
        } else {
            api_version
        };

        Ok(Self {
            client: crate::provider::provider_http_client(),
            api_key,
            flavor: OpenAiFlavor::UnifiedAzureAi {
                endpoint,
                api_version,
            },
            chat_models: Vec::new(),
            embedding_models: Vec::new(),
            zdr: None,
            provider_id: None,
            residency: super::Residency::Global,
            exclusive_catalog: false,
        })
    }

    /// Attach an evidence-bound ZDR attestation to this exact deployment.
    ///
    /// Replaces the previous `with_zdr_confirmed(bool)`: a boolean recorded only
    /// that someone typed `true`, which survives a copy-pasted `.env` and a stale
    /// deployment. The attestation names the resource, the retention-exception
    /// approval, its effective date and its reviewer, and is digest-bound over
    /// all four — see [`ZdrAttestation`]. Direct `OpenAI` routes are never
    /// promoted implicitly.
    #[must_use]
    pub fn with_zdr_attestation(mut self, attestation: Option<Arc<ZdrAttestation>>) -> Self {
        self.zdr = attestation;
        self
    }

    /// Register this endpoint under its own id, with its own residency and a
    /// catalog it must not stray outside.
    ///
    /// This is what makes a second OpenAI-compatible provider addressable. Before
    /// it, every OpenAI-shaped provider registered as `openai`/`azure-openai`,
    /// matched the same hints, and the first one registered won every non-Claude
    /// model — so a sovereign endpoint could not coexist with Azure at all.
    #[must_use]
    pub fn with_identity(
        mut self,
        provider_id: impl Into<String>,
        residency: super::Residency,
        exclusive_catalog: bool,
    ) -> Self {
        self.provider_id = Some(provider_id.into());
        self.residency = residency;
        self.exclusive_catalog = exclusive_catalog;
        self
    }

    /// Declare this endpoint's residency without changing its id.
    #[must_use]
    pub const fn with_residency(mut self, residency: super::Residency) -> Self {
        self.residency = residency;
        self
    }

    /// `provider_hint` synonyms that should resolve to this provider.
    ///
    /// Only the built-in flavors carry synonyms: `openai`/`azure` are historical
    /// spellings callers already send for the Azure deployment. A custom endpoint
    /// gets none, so its id is the only way to address it and it can never absorb
    /// a hint meant for Azure.
    fn hint_aliases(&self) -> Vec<String> {
        if self.provider_id.is_some() {
            return Vec::new();
        }
        match &self.flavor {
            OpenAiFlavor::OpenAi { .. } | OpenAiFlavor::UnifiedAzureAi { .. } => Vec::new(),
            OpenAiFlavor::Azure { .. } => vec!["openai".to_owned(), "azure".to_owned()],
        }
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
            OpenAiFlavor::UnifiedAzureAi {
                endpoint,
                api_version,
            } => format!(
                "{}/chat/completions?api-version={}",
                endpoint.trim_end_matches('/'),
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
            OpenAiFlavor::UnifiedAzureAi {
                endpoint,
                api_version,
            } => format!(
                "{}/embeddings?api-version={}",
                endpoint.trim_end_matches('/'),
                api_version
            ),
        }
    }

    fn provider_name(&self) -> &str {
        if let Some(id) = &self.provider_id {
            return id;
        }
        match &self.flavor {
            OpenAiFlavor::OpenAi { .. } => "openai",
            OpenAiFlavor::Azure { .. } => "azure-openai",
            OpenAiFlavor::UnifiedAzureAi { .. } => "azure-ai-unified",
        }
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.flavor {
            OpenAiFlavor::OpenAi { .. } => request.bearer_auth(&self.api_key),
            OpenAiFlavor::Azure { .. } | OpenAiFlavor::UnifiedAzureAi { .. } => {
                request.header("api-key", &self.api_key)
            }
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

    if stream {
        // Ask OpenAI/Azure to emit a trailing usage-only chunk so streamed turns
        // report real token counts (otherwise usage is omitted from streams and
        // the gateway's `usage`/`done` events carry 0). Parsed in infer_stream.
        body["stream_options"] = serde_json::json!({ "include_usage": true });
    }

    if is_reasoning_model(&req.model) {
        body["max_completion_tokens"] = serde_json::json!(req.max_tokens);
    } else {
        body["max_tokens"] = serde_json::json!(req.max_tokens);
        body["temperature"] = serde_json::json!(req.temperature);
    }

    if let Some(schema) = &req.structured_output_schema {
        if !schema.is_empty() {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(schema) {
                // OpenAI's `response_format: json_schema` requires a NAMED wrapper
                // `{ name, schema, strict? }`. Passing the bare schema 400s with
                // "Missing required parameter: 'response_format.json_schema.name'".
                // Accept either a bare schema object or an already-wrapped one, and
                // always ensure a regex-valid `name` is present. `strict` is left
                // unset (lenient) so schemas with optional fields are not rejected.
                let json_schema = if parsed.get("schema").is_some() {
                    let mut wrapper = parsed;
                    if wrapper.get("name").is_none() {
                        wrapper["name"] = serde_json::json!("structured_output");
                    }
                    wrapper
                } else {
                    serde_json::json!({
                        "name": "structured_output",
                        "schema": parsed,
                    })
                };
                body["response_format"] = serde_json::json!({
                    "type": "json_schema",
                    "json_schema": json_schema,
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

/// Strip Cohere Command A's `<|START_TEXT|>`/`<|END_TEXT|>` sentinels from a
/// non-streamed `message.content` string (confirmed live: every response
/// wraps its visible answer in these; neither GPT-4o/GPT-5 nor Claude do
/// this). Each side is stripped independently so a `max_tokens`-truncated
/// response — which may carry the opening sentinel with no closing one —
/// still comes out clean.
fn strip_cohere_text_sentinels(content: &str) -> String {
    let without_prefix = content.strip_prefix("<|START_TEXT|>").unwrap_or(content);
    without_prefix
        .strip_suffix("<|END_TEXT|>")
        .unwrap_or(without_prefix)
        .to_owned()
}

/// Classify a deployment id into a UI modality group. The Azure chat-deployment
/// catalog (`AZURE_OPENAI_CHAT_DEPLOYMENTS`) may legitimately include
/// image/video/transcribe deployments on the same resource; group them so the
/// SPA can present "image", "video", "transcribe" sections without the gateway
/// having to know per-model knowledge.
fn model_modality(model: &str) -> String {
    let m = model.to_ascii_lowercase();
    if m.contains("image") || m.starts_with("dall-e") || m.starts_with("mai-image") {
        "image".to_owned()
    } else if m.contains("sora") || m.contains("video") {
        "video".to_owned()
    } else if m.contains("transcribe") || m.contains("whisper") {
        "transcribe".to_owned()
    } else if m.contains("embedding") {
        "embedding".to_owned()
    } else {
        "chat".to_owned()
    }
}

/// True for low-cost / economy deployments so the UI can group "cheap" models
/// and pick a cheap default. Covers the mini/nano tiers and `model-router`
/// (which itself routes to the cheapest capable model).
fn is_cheap_model(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m == "model-router"
        || m.contains("mini")
        || m.contains("nano")
        || m.contains("deepseek")
        || m.contains("embedding")
}

#[allow(clippy::too_many_lines)]
#[async_trait::async_trait]
impl ProviderRouter for OpenAiProvider {
    fn capabilities(&self) -> super::ProviderCapabilities {
        // GPT-4o / GPT-5 / o-series: tools, vision, reasoning, streaming, and
        // a first-party embeddings API. Conservative context/output bounds.
        // The unified-Azure-AI flavor fronts MaaS chat deployments (e.g.
        // Cohere Command A Plus) with no vision/embeddings arm of their own.
        let (
            supports_vision,
            supports_embeddings,
            modalities,
            max_context_tokens,
            max_output_tokens,
        ) = match &self.flavor {
            OpenAiFlavor::UnifiedAzureAi { .. } => {
                (false, false, vec!["chat".to_owned()], 128_000, 8_192)
            }
            OpenAiFlavor::OpenAi { .. } | OpenAiFlavor::Azure { .. } => (
                true,
                true,
                vec![
                    "chat".to_owned(),
                    "vision".to_owned(),
                    "embeddings".to_owned(),
                ],
                128_000,
                16_384,
            ),
        };
        super::ProviderCapabilities {
            provider_id: self.provider_name().to_owned(),
            aliases: self.hint_aliases(),
            model_family: super::ModelFamily::OpenAiCompatible,
            residency: self.residency,
            exclusive_catalog: self.exclusive_catalog,
            supports_tools: true,
            supports_vision,
            supports_thinking: true,
            supports_streaming: true,
            supports_embeddings,
            supports_zdr: self.zdr.is_some(),
            modalities,
            max_context_tokens,
            max_output_tokens,
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

        let raw_content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        let content = if matches!(self.flavor, OpenAiFlavor::UnifiedAzureAi { .. }) {
            strip_cohere_text_sentinels(&raw_content)
        } else {
            raw_content
        };
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
            // Provenance is stamped by the fallback chain, not the raw adapter.
            provider_used: String::new(),
            residency: String::new(),
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
        let strip_text_sentinels = matches!(self.flavor, OpenAiFlavor::UnifiedAzureAi { .. });
        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            let mut bytes_stream = response.bytes_stream();
            let mut buffer = String::new();
            // Captured from the trailing usage-only chunk (include_usage=true).
            // We must NOT terminate on `finish_reason` — that chunk arrives
            // first; the usage chunk (empty choices) comes after it, then [DONE].
            let mut input_tokens = 0i32;
            let mut output_tokens = 0i32;
            let mut model_used = model.clone();

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
                            let _ = tx
                                .send(InferChunk {
                                    request_id: request_id.clone(),
                                    delta: String::new(),
                                    done: true,
                                    model_used: model_used.clone(),
                                    input_tokens,
                                    output_tokens,
                                    provider_used: String::new(),
                                    residency: String::new(),
                                })
                                .await;
                            return;
                        }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(usage) = json["usage"].as_object() {
                                if let Some(p) = usage
                                    .get("prompt_tokens")
                                    .and_then(serde_json::Value::as_i64)
                                {
                                    input_tokens = to_i32_or_max(p);
                                }
                                if let Some(c) = usage
                                    .get("completion_tokens")
                                    .and_then(serde_json::Value::as_i64)
                                {
                                    output_tokens = to_i32_or_max(c);
                                }
                            }
                            if let Some(m) = json["model"].as_str() {
                                model_used = m.to_owned();
                            }
                            let delta = json["choices"][0]["delta"]["content"]
                                .as_str()
                                .unwrap_or("")
                                .to_owned();
                            // Command A Plus streams `<|START_TEXT|>`/`<|END_TEXT|>`
                            // as their own isolated delta chunks bracketing the
                            // visible answer (confirmed live) — drop them rather
                            // than forward the literal sentinel to the caller.
                            let delta = if strip_text_sentinels
                                && matches!(delta.as_str(), "<|START_TEXT|>" | "<|END_TEXT|>")
                            {
                                String::new()
                            } else {
                                delta
                            };
                            // Stream content as it arrives (done=false). The
                            // terminal `done` (with real token counts) is emitted
                            // only on [DONE] / stream end so the usage chunk is read.
                            if !delta.is_empty() {
                                let chunk = InferChunk {
                                    request_id: request_id.clone(),
                                    delta,
                                    done: false,
                                    model_used: model_used.clone(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    provider_used: String::new(),
                                    residency: String::new(),
                                };
                                if tx.send(chunk).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            }

            // Stream ended without an explicit [DONE] — still emit a terminal
            // done carrying whatever usage we captured.
            let _ = tx
                .send(InferChunk {
                    request_id,
                    delta: String::new(),
                    done: true,
                    model_used,
                    input_tokens,
                    output_tokens,
                    provider_used: String::new(),
                    residency: String::new(),
                })
                .await;
        });

        Ok(rx)
    }

    async fn create_embedding(&self, req: &EmbedRequest) -> Result<EmbedResponse, ProviderError> {
        if req.text.trim().is_empty() {
            return Err(ProviderError::InvalidResponse(
                "embedding input text is empty".to_owned(),
            ));
        }

        let include_model = matches!(
            &self.flavor,
            OpenAiFlavor::OpenAi { .. } | OpenAiFlavor::UnifiedAzureAi { .. }
        );
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
        let caps = self.capabilities();
        let chat_features = caps.feature_flags();
        // Venice-style per-model privacy disclosure from this deployment's
        // declared residency + ZDR attestation. An undeclared (Global)
        // residency stays an empty label: "no commitment claimed".
        let privacy_tier = super::PrivacyTier::classify(&caps);
        let residency_label = if caps.residency == super::Residency::Global {
            String::new()
        } else {
            caps.residency.as_str().to_owned()
        };
        self.chat_models
            .iter()
            .map(|id| ModelInfo {
                id: id.clone(),
                provider: provider.clone(),
                modality: model_modality(id),
                streaming: true,
                features: chat_features.clone(),
                cheap: is_cheap_model(id),
                privacy_tier,
                residency_label: residency_label.clone(),
            })
            .chain(self.embedding_models.iter().map(|id| ModelInfo {
                id: id.clone(),
                provider: provider.clone(),
                modality: "embedding".to_owned(),
                streaming: false,
                // Embeddings are an economy modality; flag so the UI can default
                // to a cheap embedder.
                cheap: true,
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
    fn unified_azure_ai_chat_url_puts_model_in_body_not_path() {
        let provider = OpenAiProvider::new_azure_ai_unified(
            "key",
            "https://core-ai-rg.services.ai.azure.com/models/",
            "2024-05-01-preview",
        )
        .expect("unified azure ai provider");

        // Deliberately NOT parameterized by model, unlike the classic Azure
        // flavor above — the deployment name belongs in the JSON body only.
        assert_eq!(
            provider.chat_completions_url("cohere-command-a-plus"),
            "https://core-ai-rg.services.ai.azure.com/models/chat/completions?api-version=2024-05-01-preview",
        );
    }

    #[test]
    fn unified_azure_ai_embeddings_url_puts_model_in_body_not_path() {
        let provider = OpenAiProvider::new_azure_ai_unified(
            "key",
            "https://core-ai-rg.services.ai.azure.com/models",
            "2024-05-01-preview",
        )
        .expect("unified azure ai provider");

        assert_eq!(
            provider.embeddings_url("embed-v-4-0"),
            "https://core-ai-rg.services.ai.azure.com/models/embeddings?api-version=2024-05-01-preview",
        );
    }

    #[test]
    fn unified_azure_ai_uses_api_key_header_not_bearer_token() {
        let provider = OpenAiProvider::new_azure_ai_unified(
            "secret-key",
            "https://core-ai-rg.services.ai.azure.com/models",
            "2024-05-01-preview",
        )
        .expect("unified azure ai provider");

        let built = provider
            .apply_auth(provider.client.get("https://example.invalid"))
            .build()
            .expect("request builds");
        assert_eq!(
            built.headers().get("api-key").and_then(|v| v.to_str().ok()),
            Some("secret-key"),
        );
        assert!(built
            .headers()
            .get(reqwest::header::AUTHORIZATION)
            .is_none());
    }

    #[test]
    fn strip_cohere_text_sentinels_removes_both_markers() {
        assert_eq!(
            strip_cohere_text_sentinels("<|START_TEXT|>Red  \nBlue  \nGreen<|END_TEXT|>"),
            "Red  \nBlue  \nGreen",
        );
    }

    #[test]
    fn strip_cohere_text_sentinels_handles_truncated_response_missing_end_marker() {
        // A max_tokens-truncated response can carry the opening sentinel with
        // no closing one — each side strips independently, so this must not
        // silently drop the (still useful, if incomplete) visible text.
        assert_eq!(
            strip_cohere_text_sentinels("<|START_TEXT|>Red  \nBl"),
            "Red  \nBl",
        );
    }

    #[test]
    fn strip_cohere_text_sentinels_is_a_noop_on_plain_content() {
        assert_eq!(strip_cohere_text_sentinels("OK"), "OK");
    }

    #[test]
    fn unified_azure_ai_carries_no_hint_aliases() {
        let provider = OpenAiProvider::new_azure_ai_unified(
            "key",
            "https://core-ai-rg.services.ai.azure.com/models",
            "2024-05-01-preview",
        )
        .expect("unified azure ai provider")
        .with_identity("cohere", super::super::Residency::Global, true)
        .with_model_catalog(vec!["cohere-command-a-plus".to_owned()], Vec::new());

        let caps = provider.capabilities();
        assert_eq!(caps.provider_id, "cohere");
        assert!(caps.aliases.is_empty());
        assert!(caps.exclusive_catalog);
        assert!(!caps.supports_embeddings);
        assert!(!caps.supports_vision);
        assert_eq!(caps.modalities, vec!["chat".to_owned()]);
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
            zdr: false,
            ..Default::default()
        };

        let openai_body = build_embedding_request_body(&req, true);
        assert_eq!(openai_body["model"], "text-embedding-3-small");
        assert_eq!(openai_body["input"], "hello");

        let azure_body = build_embedding_request_body(&req, false);
        assert!(azure_body.get("model").is_none());
        assert_eq!(azure_body["input"], "hello");
    }
}
