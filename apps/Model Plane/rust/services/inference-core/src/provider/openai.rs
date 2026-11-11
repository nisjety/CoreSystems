//! OpenAI-compatible provider — calls a configurable base URL with SSE streaming.

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::effort::{self, Family, ModelAliases, Resolved, Wire};
use super::model_record;
use super::zdr::ZdrAttestation;
use super::{
    narrow_f64, EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelInfo,
    ProviderError, ProviderRouter,
};

const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";

/// Switch for sending `reasoning_effort` and raising the completion ceiling by
/// the level's allowance. Off unless the literal `true`, read once at startup.
const REASONING_EFFORT_ENV: &str = "INFERENCE_CORE_OPENAI_REASONING_EFFORT";

/// Whether an explicit env value turns the switch on. Pure so the policy is
/// unit-testable without touching the process environment; a stray `1` or `on`
/// must not start spending reasoning allowance.
fn parse_enabled(raw: Option<&str>) -> bool {
    raw.map(str::trim) == Some("true")
}

/// Switch for sending `prompt_cache_key` to the first-party `OpenAI` endpoint.
/// Off unless the literal `true`, read once at startup.
const PROMPT_CACHE_KEY_ENV: &str = "INFERENCE_OPENAI_PROMPT_CACHE_KEY";

fn prompt_cache_key_from_env() -> bool {
    parse_enabled(std::env::var(PROMPT_CACHE_KEY_ENV).ok().as_deref())
}

/// The reasoning-effort switch and the deployment aliases that tell an
/// operator-named Azure deployment which model it really is.
#[derive(Clone, Default)]
struct EffortSettings {
    enabled: bool,
    aliases: ModelAliases,
}

impl EffortSettings {
    fn from_env() -> Self {
        let enabled = parse_enabled(std::env::var(REASONING_EFFORT_ENV).ok().as_deref());
        Self {
            enabled,
            aliases: if enabled {
                ModelAliases::from_env()
            } else {
                ModelAliases::default()
            },
        }
    }
}

/// What the request body needs to know about reasoning for one model.
#[derive(Clone, Copy)]
struct ReasoningPlan {
    /// The model takes `max_completion_tokens` and no `temperature`.
    reasoning_model: bool,
    /// The level to send, set only when the switch is on and the model has a
    /// wire for it.
    effort: Option<Resolved>,
}

impl ReasoningPlan {
    /// The behaviour from before the ladder: no effort, the name-prefix test.
    fn legacy(model: &str) -> Self {
        Self {
            reasoning_model: is_reasoning_model(model),
            effort: None,
        }
    }
}

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
    /// Reasoning-effort switch and deployment aliases.
    effort: EffortSettings,
    /// Prompt-cache routing key switch; see `sends_prompt_cache_key`.
    prompt_cache_key: bool,
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
            effort: EffortSettings::from_env(),
            prompt_cache_key: prompt_cache_key_from_env(),
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
            effort: EffortSettings::from_env(),
            prompt_cache_key: prompt_cache_key_from_env(),
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
            effort: EffortSettings::from_env(),
            prompt_cache_key: prompt_cache_key_from_env(),
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

    /// Whether to ask this deployment for token logprobs.
    ///
    /// Three gates, all of which must hold, because a deployment that rejects
    /// the parameter fails the completion outright rather than ignoring it:
    /// the operator kill-switch, the model (reasoning models reject it), and
    /// the endpoint flavor. `UnifiedAzureAi` is excluded deliberately — the
    /// MaaS catalog behind it (Cohere Command A, DeepSeek, …) is
    /// OpenAI-*shaped* but not OpenAI, and none of it is verified to honor
    /// `logprobs`; an unverified 400 there would take out chat for those
    /// models to gain a quality signal.
    fn wants_logprobs(&self, model: &str) -> bool {
        matches!(
            self.flavor,
            OpenAiFlavor::OpenAi { .. } | OpenAiFlavor::Azure { .. }
        ) && super::logprobs::model_supports_logprobs(model)
            && super::logprobs::logprobs_enabled()
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

    /// How this request treats reasoning. With the switch off it is the
    /// name-prefix test alone, so the body is exactly what it was before the
    /// ladder; with it on, the support table decides, so an `o4` model gets the
    /// completion-token shape and an aliased Azure deployment is judged by the
    /// model behind it rather than by its operator-chosen name.
    fn reasoning_plan(&self, req: &InferRequest) -> ReasoningPlan {
        let legacy = ReasoningPlan::legacy(&req.model);
        if !self.effort.enabled {
            return legacy;
        }
        let support = self.effort.aliases.support(Family::OpenAi, &req.model);
        ReasoningPlan {
            reasoning_model: legacy.reasoning_model || support.wire == Wire::OpenAiReasoningEffort,
            effort: effort::resolve(req.reasoning_effort, support),
        }
    }

    /// The body for one attempt. The routing key is added only when
    /// [`Self::sends_prompt_cache_key`] allows it, so every other request gets
    /// exactly what [`build_request_body`] returns.
    fn request_body(
        &self,
        req: &InferRequest,
        stream: bool,
        request_logprobs: bool,
        plan: ReasoningPlan,
    ) -> serde_json::Value {
        let mut body = build_request_body(req, stream, request_logprobs, plan);
        if self.sends_prompt_cache_key(req) {
            body["prompt_cache_key"] = serde_json::json!(req.cache_scope_key);
        }
        body
    }

    /// Whether this request may carry the prompt-cache routing key. Azure's
    /// acceptance of the key is not yet verified, so only the first-party
    /// endpoint gets it. A ZDR request never does, the same rule the Anthropic
    /// adapter applies to `cache_control`.
    fn sends_prompt_cache_key(&self, req: &InferRequest) -> bool {
        let first_party = match &self.flavor {
            OpenAiFlavor::OpenAi { api_base } => {
                api_base.trim_end_matches('/') == DEFAULT_OPENAI_BASE
            }
            OpenAiFlavor::Azure { .. } | OpenAiFlavor::UnifiedAzureAi { .. } => false,
        };
        self.prompt_cache_key
            && first_party
            && self.provider_id.is_none()
            && !req.zdr
            && !req.cache_scope_key.is_empty()
    }

    /// POST the chat completion and return the successful response.
    ///
    /// A 400 that names `reasoning_effort` means this deployment (an Azure
    /// api-version or model that predates the parameter) refused the level, not
    /// the request, so it is retried once without it. The plan then carries no
    /// effort, which bounds the loop to a single retry.
    async fn post_chat(
        &self,
        req: &InferRequest,
        stream: bool,
    ) -> Result<reqwest::Response, ProviderError> {
        let url = self.chat_completions_url(&req.model);
        let request_logprobs = self.wants_logprobs(&req.model);
        let mut plan = self.reasoning_plan(req);
        if let Some(resolved) = plan.effort {
            info!(
                request_id = %req.request_id,
                model = %req.model,
                wire = ?resolved.wire,
                requested = resolved.requested.as_str(),
                applied = resolved.applied.as_str(),
                clamped = resolved.clamped(),
                "effort_resolved"
            );
        }

        loop {
            let body = self.request_body(req, stream, request_logprobs, plan);
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

            if response.status().is_success() {
                return Ok(response);
            }

            let status = response.status();
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "no body".to_owned());
            if plan.effort.is_some() && rejects_reasoning_effort(status, &text) {
                warn!(
                    request_id = %req.request_id,
                    model = %req.model,
                    "deployment rejected reasoning_effort; retrying once without it"
                );
                plan.effort = None;
                continue;
            }
            return Err(crate::provider::overflow::classify_http_failure(
                status, &text,
            ));
        }
    }
}

/// True when a failed completion is the deployment refusing `reasoning_effort`.
fn rejects_reasoning_effort(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::BAD_REQUEST && body.contains("reasoning_effort")
}

fn to_i32_or_max(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

/// The prompt-cache legs of an OpenAI-family `usage` object, as `(read, write)`.
///
/// Chat Completions reports them in `prompt_tokens_details`: `cached_tokens`,
/// plus `cache_write_tokens` from GPT-5.6 on. The Responses API names the same
/// object `input_tokens_details`, which is read when the first is absent. A
/// missing or `null` field counts as 0.
///
/// `OpenAI` counts both legs inside `prompt_tokens`, which is the fold-in the
/// gateway's pricing expects, so `input_tokens` is left as reported and the
/// legs are capped to it: reads first, then writes to what remains. A
/// malformed report can then never make the uncached share negative.
fn openai_cache_tokens(usage: &serde_json::Value) -> (i32, i32) {
    let Some(details) = ["prompt_tokens_details", "input_tokens_details"]
        .iter()
        .map(|key| &usage[*key])
        .find(|details| details.is_object())
    else {
        return (0, 0);
    };
    let count = |value: &serde_json::Value| value.as_i64().unwrap_or(0).max(0);
    let prompt = count(if usage["prompt_tokens"].is_null() {
        &usage["input_tokens"]
    } else {
        &usage["prompt_tokens"]
    });
    let read = count(&details["cached_tokens"]).min(prompt);
    let write = count(&details["cache_write_tokens"]).min(prompt - read);
    (to_i32_or_max(read), to_i32_or_max(write))
}

/// This chunk's `finish_reason`, if the model set one on this choice.
///
/// OpenAI-family streaming carries it at `choices[0].finish_reason` — `null`
/// on every content-bearing chunk, then a real value (`stop`, `length`,
/// `tool_calls`, `content_filter`, `function_call`) on the chunk right
/// before the trailing usage-only one. Verified against
/// developers.openai.com's streaming-events reference, not assumed.
fn openai_stream_finish_reason(json: &serde_json::Value) -> Option<&str> {
    json["choices"][0]["finish_reason"].as_str()
}

/// Build the `OpenAI` chat completions request body.
/// The `content` of the message at `index`: the plain string it always was, or,
/// when images ride with it, the text part followed by one `image_url` part per
/// image as a data URL. An empty text is left out rather than sent as an empty part.
fn message_content(req: &InferRequest, index: usize, text: &str) -> serde_json::Value {
    let mut images = super::images_for_message(req, index).peekable();
    if images.peek().is_none() {
        return serde_json::Value::String(text.to_owned());
    }
    let mut parts = Vec::new();
    if !text.is_empty() {
        parts.push(serde_json::json!({ "type": "text", "text": text }));
    }
    parts.extend(images.map(|image| {
        serde_json::json!({
            "type": "image_url",
            "image_url": { "url": format!("data:{};base64,{}", image.mime_type, image.base64()) },
        })
    }));
    serde_json::Value::Array(parts)
}

fn build_request_body(
    req: &InferRequest,
    stream: bool,
    request_logprobs: bool,
    reasoning: ReasoningPlan,
) -> serde_json::Value {
    let messages: Vec<serde_json::Value> = req
        .messages
        .iter()
        .enumerate()
        .map(|(index, m)| {
            let mut msg = serde_json::json!({
                "role": m.role,
                "content": message_content(req, index, &m.content),
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

    if reasoning.reasoning_model {
        // A reasoning model spends its ceiling thinking, so a level raises the
        // ceiling by its allowance; otherwise Deep would only buy an empty answer.
        let ceiling = reasoning.effort.map_or(req.max_tokens, |resolved| {
            effort::raised_ceiling(req.max_tokens, resolved.applied)
        });
        body["max_completion_tokens"] = serde_json::json!(ceiling);
        if let Some(resolved) = reasoning.effort {
            body["reasoning_effort"] = serde_json::json!(resolved.applied.as_str());
        }
    } else {
        body["max_tokens"] = serde_json::json!(req.max_tokens);
        body["temperature"] = serde_json::json!(req.temperature);
    }

    // Ask for the per-token probabilities of the answer, which the caller turns
    // into a `TokenConfidence` — the only signal in the stack that reports how
    // sure the MODEL was rather than how well-backed the answer was. Whether it
    // is safe to ask is the caller's call (`wants_logprobs`): a deployment that
    // rejects the parameter 400s the whole completion. `top_logprobs` is
    // deliberately not requested — the alternatives the model considered are
    // far more data than the aggregate needs, and cost response size on every
    // turn.
    if request_logprobs {
        body["logprobs"] = serde_json::json!(true);
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
                let params = super::tool_parameters(&t.name, &t.parameters_json);
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
            supports_zdr: self.zdr.as_deref().is_some_and(ZdrAttestation::in_force_now),
            modalities,
            max_context_tokens,
            max_output_tokens,
        }
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        let response = self.post_chat(req, false).await?;

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
        let (cache_read_input_tokens, cache_creation_input_tokens) =
            openai_cache_tokens(&json["usage"]);
        let tool_calls = parse_tool_calls(&json);
        // `None` whenever we did not ask, the deployment ignored the parameter,
        // or the field was unusable — never a fabricated value. The question is
        // passed so the summary can separate the answer's claims from the part
        // that merely restates the prompt.
        let token_confidence = super::logprobs::from_completion(
            &json,
            super::logprobs::question_from_messages(&req.messages),
        );

        info!(model = %req.model, provider = "openai", "infer completed");

        Ok(InferResponse {
            compaction_summary: String::new(),
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
            token_confidence,
            // Already inside `input_tokens`; see `openai_cache_tokens`.
            cache_read_input_tokens,
            cache_creation_input_tokens,
            // Stamped by the fallback chain, not the raw adapter.
            provider_timing: None,
            service_tier_used: String::new(),
        })
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let response = self.post_chat(req, true).await?;

        let request_id = req.request_id.clone();
        let model = req.model.clone();
        let requested_logprobs = self.wants_logprobs(&req.model);
        // Cloned in so the streamed summary can tell the answer's claims apart
        // from the framing that restates the question.
        let question = super::logprobs::question_from_messages(&req.messages).to_owned();
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
            // The prompt-cache legs arrive in the same usage chunk.
            let mut cache_read_input_tokens = 0i32;
            let mut cache_creation_input_tokens = 0i32;
            let mut model_used = model.clone();
            // Set from the choice's own `finish_reason` once a content-bearing
            // chunk carries one (OpenAI-family streams put it on the last
            // choice before the usage-only trailer). Stays empty if the loop
            // exits without ever seeing one -- the tail-chunk fallback below
            // treats that as an incomplete stream, not a natural completion.
            let mut stop_reason = String::new();
            // Token logprobs arrive the way content does — a few per chunk — so
            // the answer-level summary only exists once the stream ends. Fed
            // unconditionally: when we did not request them, every chunk
            // contributes nothing and the summary stays `None`.
            let mut token_logprobs = super::logprobs::LogprobAccumulator::for_question(&question);

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
                            // Operating this signal means being able to see
                            // which half is missing when it is absent: did we
                            // ask, and did the provider answer.
                            info!(
                                request_id = %request_id,
                                requested_logprobs,
                                token_certainty_tokens = token_logprobs.summarize().map_or(0, |s| s.token_count),
                                "stream token certainty"
                            );
                            let _ = tx
                                .send(InferChunk {
                                    compaction_summary: String::new(),
                                    reasoning_delta: String::new(),
                                    request_id: request_id.clone(),
                                    delta: String::new(),
                                    done: true,
                                    model_used: model_used.clone(),
                                    input_tokens,
                                    output_tokens,
                                    stop_reason: if stop_reason.is_empty() {
                                        "end_turn".to_owned()
                                    } else {
                                        stop_reason.clone()
                                    },
                                    // Provenance is stamped by the fallback
                                    // chain, not the raw adapter.
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    token_confidence: token_logprobs.summarize(),
                                    // From the usage chunk; 0 when the stream
                                    // carried none.
                                    cache_read_input_tokens,
                                    cache_creation_input_tokens,
                                    provider_timing: None,
                                    service_tier_used: String::new(),
                                })
                                .await;
                            return;
                        }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            token_logprobs.absorb(&json);
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
                                (cache_read_input_tokens, cache_creation_input_tokens) =
                                    openai_cache_tokens(&json["usage"]);
                            }
                            if let Some(m) = json["model"].as_str() {
                                model_used = m.to_owned();
                            }
                            if let Some(reason) = openai_stream_finish_reason(&json) {
                                stop_reason = reason.to_owned();
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
                                    compaction_summary: String::new(),
                                    request_id: request_id.clone(),
                                    delta,
                                    done: false,
                                    model_used: model_used.clone(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    stop_reason: String::new(),
                                    reasoning_delta: String::new(),
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    // Answer-level; carried on the final chunk.
                                    token_confidence: None,
                                    cache_read_input_tokens: 0,
                                    cache_creation_input_tokens: 0,
                                    provider_timing: None,
                                    service_tier_used: String::new(),
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
            // done carrying whatever usage we captured. This is the abnormal
            // path (a transport error above `break`s into it, and so does a
            // connection that just closes early): a finish_reason seen on a
            // real content chunk still wins, but absent that, "end_turn"
            // would misreport an incomplete answer as a clean one.
            let _ = tx
                .send(InferChunk {
                    compaction_summary: String::new(),
                    reasoning_delta: String::new(),
                    request_id,
                    delta: String::new(),
                    done: true,
                    model_used,
                    input_tokens,
                    output_tokens,
                    stop_reason: if stop_reason.is_empty() {
                        "stream_incomplete".to_owned()
                    } else {
                        stop_reason
                    },
                    // Provenance is stamped by the fallback chain, not the raw
                    // adapter.
                    provider_used: String::new(),
                    residency: String::new(),
                    // Whatever the truncated stream did deliver. The summary
                    // covers the tokens that actually arrived, which is what
                    // the caller scored.
                    token_confidence: token_logprobs.summarize(),
                    cache_read_input_tokens,
                    cache_creation_input_tokens,
                    provider_timing: None,
                    service_tier_used: String::new(),
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
            return Err(crate::provider::overflow::classify_http_failure(
                status, &text,
            ));
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
        let provider_features = caps.feature_flags();
        // Venice-style per-model privacy disclosure from this deployment's
        // declared residency + ZDR attestation. An undeclared (Global)
        // residency stays an empty label: "no commitment claimed".
        let privacy_tier = super::PrivacyTier::classify(&caps);
        let residency_label = if caps.residency == super::Residency::Global {
            String::new()
        } else {
            caps.residency.as_str().to_owned()
        };
        let aliases = &self.effort.aliases;
        self.chat_models
            .iter()
            .map(|id| {
                let record = model_record::advertised(
                    Family::OpenAi,
                    aliases,
                    id,
                    caps.max_context_tokens,
                    caps.max_output_tokens,
                );
                ModelInfo {
                    id: id.clone(),
                    provider: provider.clone(),
                    modality: model_modality(id),
                    streaming: true,
                    features: super::features_for_model(&caps, &provider_features, id),
                    cheap: is_cheap_model(id),
                    privacy_tier,
                    residency_label: residency_label.clone(),
                    // Advertised only while the adapter actually sends the level.
                    effort_levels: if self.effort.enabled {
                        self.effort
                            .aliases
                            .support(Family::OpenAi, id)
                            .levels
                            .to_vec()
                    } else {
                        Vec::new()
                    },
                    context_window_tokens: record.context_window_tokens,
                    max_output_tokens: record.max_output_tokens,
                    lifecycle: record.lifecycle,
                    successor: record.successor,
                    priority_tier: record.priority_tier,
                }
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

    #[test]
    fn stream_finish_reason_reads_the_choices_zero_path() {
        // Shape verified against developers.openai.com's streaming-events
        // reference: null on a content-bearing chunk, a real value on the
        // chunk right before the trailing usage-only one.
        let content_chunk = serde_json::json!({
            "choices": [{"index": 0, "delta": {"content": "hei"}, "finish_reason": null}]
        });
        assert_eq!(openai_stream_finish_reason(&content_chunk), None);

        let final_chunk = serde_json::json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "length"}]
        });
        assert_eq!(openai_stream_finish_reason(&final_chunk), Some("length"));

        let missing_choices = serde_json::json!({"usage": {"prompt_tokens": 3}});
        assert_eq!(openai_stream_finish_reason(&missing_choices), None);
    }

    fn make_request(model: &str) -> InferRequest {
        InferRequest {
            thinking_budget_tokens: 0,
            request_id: "req-1".to_owned(),
            provider_hint: String::new(),
            model: model.to_owned(),
            messages: vec![crate::provider::ChatMessage {
                compaction_summary: String::new(),
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

    /// The body as it was before the ladder: the name-prefix test, no effort.
    fn legacy_body(req: &InferRequest, stream: bool, request_logprobs: bool) -> serde_json::Value {
        build_request_body(
            req,
            stream,
            request_logprobs,
            ReasoningPlan::legacy(&req.model),
        )
    }

    fn image(message_index: usize, mime: &str, bytes: &[u8]) -> crate::provider::ImagePart {
        crate::provider::ImagePart {
            message_index,
            data: std::sync::Arc::from(bytes),
            mime_type: mime.to_owned(),
        }
    }

    #[test]
    fn openai_body_emits_image_url_part_for_indexed_message() {
        let mut req = make_request("gpt-4o");
        req.messages.insert(
            0,
            crate::provider::ChatMessage {
                compaction_summary: String::new(),
                role: "system".to_owned(),
                content: "be brief".to_owned(),
                name: String::new(),
            },
        );
        // Index 1 is the user message once the system message leads.
        req.image_parts = vec![image(1, "image/png", b"one"), image(1, "image/jpeg", b"two")];
        let body = legacy_body(&req, false, false);

        assert_eq!(body["messages"][0]["content"], "be brief", "an image-free message stays a string");
        let parts = body["messages"][1]["content"].as_array().expect("parts");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], serde_json::json!({"type": "text", "text": "hello"}));
        assert_eq!(parts[1]["type"], "image_url");
        assert_eq!(parts[1]["image_url"]["url"], "data:image/png;base64,b25l");
        assert_eq!(parts[2]["image_url"]["url"], "data:image/jpeg;base64,dHdv");
    }

    #[test]
    fn no_image_parts_is_byte_identical_to_today() {
        let req = make_request("gpt-4o");
        let body = legacy_body(&req, false, false);
        assert_eq!(body["messages"][0], serde_json::json!({"role": "user", "content": "hello"}));
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
        let body = legacy_body(&req, false, false);
        assert_eq!(body["tools"][0]["type"], "function");
        assert_eq!(body["tools"][0]["function"]["name"], "search_web");
        assert_eq!(body["tools"][0]["function"]["parameters"]["type"], "object");
        assert_eq!(body["tool_choice"], "auto");
    }

    #[test]
    fn build_request_body_omits_tools_when_empty() {
        let body = legacy_body(&make_request("gpt-4o"), false, false);
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

    /// An attestation that was valid when the process booted (validated against
    /// a date inside its window) but whose review-by date has since passed.
    fn attestation_that_lapsed_after_boot() -> Arc<ZdrAttestation> {
        use crate::provider::zdr::{canonical_digest, RawAttestation};
        let mut raw = RawAttestation {
            resource_id: "/subscriptions/abc/rg/eu/example".to_owned(),
            approval_ref: "MAM-2020-0001".to_owned(),
            effective_date: "2020-01-01".to_owned(),
            review_by: "2020-06-01".to_owned(),
            reviewer: "ima@aquatiq.com".to_owned(),
            digest: String::new(),
        };
        raw.digest = canonical_digest(&raw);
        Arc::new(
            ZdrAttestation::validate(
                "AZURE_OPENAI",
                &raw,
                chrono::NaiveDate::from_ymd_opt(2020, 3, 1).expect("static date"),
            )
            .expect("in force on the boot date"),
        )
    }

    /// A running process re-checks the date: once review-by has passed, the
    /// provider stops advertising ZDR without a restart.
    #[test]
    fn review_by_passes_while_running_stops_granting() {
        let provider = OpenAiProvider::new_azure(
            "key",
            "https://example.openai.azure.com/",
            "2025-01-01-preview",
        )
        .expect("azure provider")
        .with_zdr_attestation(Some(attestation_that_lapsed_after_boot()));
        assert!(
            !provider.capabilities().supports_zdr,
            "an attestation past its review-by date must not advertise ZDR"
        );
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
        let body = legacy_body(&make_request("gpt-5-mini"), false, false);

        assert_eq!(body["max_completion_tokens"], 1024);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn standard_models_use_chat_completion_shape() {
        let body = legacy_body(&make_request("gpt-4o-mini"), false, false);

        assert_eq!(body["max_tokens"], 1024);
        let temperature = body["temperature"].as_f64().expect("temperature");
        assert!((temperature - 0.7).abs() < 0.0001);
        assert!(body.get("max_completion_tokens").is_none());
    }

    /// The request-side half of the token-certainty signal. Opt-in per call
    /// because a deployment that does not support the parameter fails the whole
    /// completion — the gating lives in `wants_logprobs`, and the body builder
    /// only obeys.
    #[test]
    fn logprobs_are_requested_only_when_asked_for() {
        let with = legacy_body(&make_request("gpt-4o-mini"), false, true);
        assert_eq!(with["logprobs"], true);
        // The alternatives the model considered are far more data than the
        // aggregate needs, on every turn.
        assert!(with.get("top_logprobs").is_none());

        let without = legacy_body(&make_request("gpt-4o-mini"), false, false);
        assert!(without.get("logprobs").is_none());

        // Streaming asks the same way; the summary is accumulated across chunks.
        let streamed = legacy_body(&make_request("gpt-4o-mini"), true, true);
        assert_eq!(streamed["logprobs"], true);
        assert_eq!(streamed["stream"], true);
    }

    #[test]
    fn cache_tokens_read_the_plain_openai_shape() {
        let usage = serde_json::json!({
            "prompt_tokens": 1566,
            "completion_tokens": 20,
            "prompt_tokens_details": {"cached_tokens": 1408, "audio_tokens": 0}
        });
        assert_eq!(openai_cache_tokens(&usage), (1408, 0));
    }

    #[test]
    fn cache_tokens_read_the_gpt_5_6_write_leg() {
        // Azure GPT-5.6 reports cache writes beside the reads.
        let usage = serde_json::json!({
            "prompt_tokens": 2000,
            "completion_tokens": 50,
            "prompt_tokens_details": {"cached_tokens": 1024, "cache_write_tokens": 512}
        });
        assert_eq!(openai_cache_tokens(&usage), (1024, 512));
    }

    #[test]
    fn cache_tokens_fall_back_to_the_responses_api_details() {
        let usage = serde_json::json!({
            "input_tokens": 300,
            "output_tokens": 9,
            "input_tokens_details": {"cached_tokens": 256}
        });
        assert_eq!(openai_cache_tokens(&usage), (256, 0));
    }

    #[test]
    fn missing_or_null_cache_details_count_as_zero() {
        for usage in [
            serde_json::json!({"prompt_tokens": 10, "completion_tokens": 2}),
            serde_json::json!({"prompt_tokens": 10, "prompt_tokens_details": null}),
            serde_json::json!({
                "prompt_tokens": 10,
                "prompt_tokens_details": {"cached_tokens": null, "cache_write_tokens": null}
            }),
            serde_json::Value::Null,
        ] {
            assert_eq!(openai_cache_tokens(&usage), (0, 0), "{usage}");
        }
    }

    #[test]
    fn cache_legs_never_exceed_the_prompt_they_are_part_of() {
        let usage = serde_json::json!({
            "prompt_tokens": 100,
            "prompt_tokens_details": {"cached_tokens": 150, "cache_write_tokens": 20}
        });
        assert_eq!(openai_cache_tokens(&usage), (100, 0));

        let usage = serde_json::json!({
            "prompt_tokens": 100,
            "prompt_tokens_details": {"cached_tokens": 90, "cache_write_tokens": 20}
        });
        assert_eq!(
            openai_cache_tokens(&usage),
            (90, 10),
            "writes get what the reads left"
        );
    }

    mod cached_tokens_over_http {
        use super::*;
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        async fn provider_for(response: ResponseTemplate) -> (MockServer, OpenAiProvider) {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(response)
                .mount(&server)
                .await;
            let provider = OpenAiProvider::new("k", Some(server.uri())).expect("provider");
            (server, provider)
        }

        #[tokio::test]
        async fn unary_infer_reports_both_cache_legs() {
            let (_server, provider) =
                provider_for(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id": "chatcmpl-1",
                    "model": "gpt-4o-mini-2024-07-18",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "ok"},
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 2000,
                        "completion_tokens": 5,
                        "prompt_tokens_details": {"cached_tokens": 1024, "cache_write_tokens": 512}
                    }
                })))
                .await;

            let response = provider
                .infer(&make_request("gpt-4o-mini"))
                .await
                .expect("infer");

            assert_eq!(
                response.input_tokens, 2000,
                "input stays as reported, legs inside it"
            );
            assert_eq!(response.cache_read_input_tokens, 1024);
            assert_eq!(response.cache_creation_input_tokens, 512);
        }

        fn sse(with_done: bool) -> String {
            let mut body = String::new();
            for event in [
                r#"{"id":"c1","model":"gpt-4o-mini-2024-07-18","choices":[{"index":0,"delta":{"content":"Hei"},"finish_reason":null}]}"#,
                r#"{"id":"c1","model":"gpt-4o-mini-2024-07-18","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
                r#"{"id":"c1","model":"gpt-4o-mini-2024-07-18","choices":[],"usage":{"prompt_tokens":1566,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":1408}}}"#,
            ] {
                body.push_str("data: ");
                body.push_str(event);
                body.push_str("\n\n");
            }
            if with_done {
                body.push_str("data: [DONE]\n\n");
            }
            body
        }

        async fn final_chunk(with_done: bool) -> InferChunk {
            let (_server, provider) = provider_for(
                ResponseTemplate::new(200).set_body_raw(sse(with_done), "text/event-stream"),
            )
            .await;
            let mut rx = provider
                .infer_stream(&make_request("gpt-4o-mini"))
                .await
                .expect("stream opens");
            while let Some(chunk) = rx.recv().await {
                if chunk.done {
                    return chunk;
                }
            }
            panic!("the stream ended without a final chunk");
        }

        #[tokio::test]
        async fn a_streamed_usage_chunk_reaches_the_final_chunk() {
            let done = final_chunk(true).await;
            assert_eq!(done.input_tokens, 1566);
            assert_eq!(done.cache_read_input_tokens, 1408);
            assert_eq!(done.cache_creation_input_tokens, 0);
            assert_eq!(done.stop_reason, "stop");
        }

        #[tokio::test]
        async fn a_stream_without_done_still_reports_the_cache_legs() {
            let done = final_chunk(false).await;
            assert_eq!(done.input_tokens, 1566);
            assert_eq!(done.cache_read_input_tokens, 1408);
            assert_eq!(done.stop_reason, "stop", "a real finish_reason still wins");
        }
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

    /// A request the gateway has scoped: the key is the hash it derives from ids.
    fn scoped_request(zdr: bool) -> InferRequest {
        InferRequest {
            zdr,
            cache_scope_key: "0123456789abcdef0123456789abcdef".to_owned(),
            ..make_request("gpt-4o-mini")
        }
    }

    /// The body `post_chat` sends for `req` on this provider.
    fn sent_body(provider: &OpenAiProvider, req: &InferRequest, stream: bool) -> serde_json::Value {
        provider.request_body(req, stream, false, provider.reasoning_plan(req))
    }

    #[test]
    fn prompt_cache_key_sent_only_when_enabled_and_not_zdr() {
        let req = scoped_request(false);
        let mut provider = OpenAiProvider::new("key", None).expect("provider");

        // Off is the body from before the key existed, whatever the request carries.
        assert_eq!(
            sent_body(&provider, &req, false).to_string(),
            legacy_body(&req, false, false).to_string()
        );

        provider.prompt_cache_key = true;
        assert_eq!(
            sent_body(&provider, &req, false)["prompt_cache_key"],
            req.cache_scope_key.as_str()
        );

        let keyless = InferRequest {
            cache_scope_key: String::new(),
            ..scoped_request(false)
        };
        assert!(sent_body(&provider, &keyless, false).get("prompt_cache_key").is_none());

        // A trailing slash names the same endpoint; any other endpoint does not get the key.
        let trailing_slash = format!("{DEFAULT_OPENAI_BASE}/");
        let mut slash = OpenAiProvider::new("key", Some(trailing_slash)).expect("provider");
        slash.prompt_cache_key = true;
        assert!(sent_body(&slash, &req, false).get("prompt_cache_key").is_some());

        let mut elsewhere = [
            OpenAiProvider::new("key", Some("https://gateway.example.test/v1".to_owned()))
                .expect("provider"),
            OpenAiProvider::new_azure("key", "https://verevon.openai.azure.com", "")
                .expect("provider"),
            OpenAiProvider::new_azure_ai_unified("key", "https://verevon.example.test", "")
                .expect("provider"),
            OpenAiProvider::new("key", None)
                .expect("provider")
                .with_identity("sovereign", crate::provider::Residency::Global, false),
        ];
        for other in &mut elsewhere {
            other.prompt_cache_key = true;
            assert!(
                sent_body(other, &req, false).get("prompt_cache_key").is_none(),
                "only the first-party endpoint takes the key"
            );
        }
    }

    #[test]
    fn openai_zdr_body_has_no_prompt_cache_key() {
        let mut provider = OpenAiProvider::new("key", None).expect("provider");
        provider.prompt_cache_key = true;

        // Both the answer and the stream go through `post_chat`.
        for stream in [false, true] {
            let open = scoped_request(false);
            assert!(
                sent_body(&provider, &open, stream).get("prompt_cache_key").is_some(),
                "the same request without ZDR carries the key"
            );

            let req = scoped_request(true);
            let body = sent_body(&provider, &req, stream);
            assert!(body.get("prompt_cache_key").is_none());
            assert_eq!(body.to_string(), legacy_body(&req, stream, false).to_string());
        }
    }

    mod reasoning_effort {
        use super::*;
        use crate::provider::effort::Level;

        /// A provider with the switch on and the given `deployment=base` aliases.
        fn provider(aliases: &str) -> OpenAiProvider {
            let mut provider = OpenAiProvider::new("key", None).expect("provider");
            provider.effort = EffortSettings {
                enabled: true,
                aliases: ModelAliases::parse(aliases),
            };
            provider
        }

        fn request(model: &str, level: Option<Level>) -> InferRequest {
            InferRequest {
                reasoning_effort: level,
                ..make_request(model)
            }
        }

        fn body_for(provider: &OpenAiProvider, req: &InferRequest) -> serde_json::Value {
            build_request_body(req, false, false, provider.reasoning_plan(req))
        }

        #[test]
        fn only_the_literal_true_enables_the_switch() {
            assert!(parse_enabled(Some("true")));
            assert!(parse_enabled(Some(" true ")));
            for off in ["1", "on", "yes", "TRUE", "enabled", ""] {
                assert!(!parse_enabled(Some(off)), "{off:?} must leave it off");
            }
            assert!(!parse_enabled(None));
        }

        #[test]
        fn reasoning_effort_sent_for_gpt5_family_only() {
            let provider = provider("");
            for model in ["gpt-5-mini", "gpt-5", "gpt-5-nano", "gpt-5.1"] {
                let body = body_for(&provider, &request(model, Some(Level::Medium)));
                assert_eq!(body["reasoning_effort"], "medium", "{model}");
            }
            for model in ["gpt-4o-mini", "gpt-4.1", "gpt-5-chat-latest"] {
                let body = body_for(&provider, &request(model, Some(Level::Medium)));
                assert!(body.get("reasoning_effort").is_none(), "{model}");
            }
        }

        #[test]
        fn quick_on_gpt5_mini_clamps_and_sends_low() {
            let provider = provider("");

            let quick = body_for(&provider, &request("gpt-5-mini", Some(Level::Low)));
            assert_eq!(quick["reasoning_effort"], "low");

            // `xhigh` is above everything the family accepts: the strongest
            // supported level is sent, never a rejected value.
            let req = request("gpt-5-mini", Some(Level::XHigh));
            let plan = provider.reasoning_plan(&req);
            let resolved = plan.effort.expect("resolved");
            assert!(resolved.clamped());
            assert_eq!(resolved.applied, Level::High);
            assert_eq!(body_for(&provider, &req)["reasoning_effort"], "high");
        }

        #[test]
        fn standard_body_is_byte_identical_to_before() {
            let provider = provider("");
            for model in ["gpt-5-mini", "o3-mini", "gpt-4o-mini", "gpt-5-chat-latest"] {
                let req = request(model, None);
                assert_eq!(
                    body_for(&provider, &req).to_string(),
                    legacy_body(&req, false, false).to_string(),
                    "{model}: no level requested must leave the body as it was"
                );
            }

            // With the switch off a requested level changes nothing either.
            let mut off = provider.clone();
            off.effort.enabled = false;
            let req = request("gpt-5-mini", Some(Level::High));
            assert_eq!(
                body_for(&off, &req).to_string(),
                legacy_body(&req, false, false).to_string()
            );
        }

        #[test]
        fn non_reasoning_model_never_gets_the_key() {
            let provider = provider("");
            let body = body_for(&provider, &request("gpt-4o-mini", Some(Level::High)));
            assert!(body.get("reasoning_effort").is_none());
            assert!(body.get("max_completion_tokens").is_none());
            assert_eq!(body["max_tokens"], 1024);
            assert!(body.get("temperature").is_some());
        }

        #[test]
        fn azure_deployment_alias_resolves_to_its_base_model() {
            let req = request("prod-chat", Some(Level::High));

            let aliased = body_for(&provider("prod-chat=gpt-5-mini"), &req);
            assert_eq!(aliased["reasoning_effort"], "high");
            assert!(aliased.get("max_completion_tokens").is_some());
            assert_eq!(aliased["model"], "prod-chat", "the deployment name stays");

            // Without the alias the name says nothing about the model, so the
            // body is the legacy one rather than a guess.
            let unaliased = body_for(&provider(""), &req);
            assert!(unaliased.get("reasoning_effort").is_none());
            assert_eq!(
                unaliased.to_string(),
                legacy_body(&req, false, false).to_string()
            );
        }

        #[test]
        fn o4_uses_max_completion_tokens() {
            let provider = provider("");
            let body = body_for(&provider, &request("o4-mini", None));
            assert_eq!(body["max_completion_tokens"], 1024);
            assert!(body.get("max_tokens").is_none());
            assert!(body.get("temperature").is_none());

            // Switch off: the name-prefix test never knew `o4`.
            let off = legacy_body(&request("o4-mini", None), false, false);
            assert_eq!(off["max_tokens"], 1024);
        }

        #[test]
        fn allowance_raises_max_completion_tokens_not_max_tokens() {
            let provider = provider("");
            for (level, expected) in [
                (Level::Low, 1024 + 1_024),
                (Level::Medium, 1024 + 2_048),
                (Level::High, 1024 + 4_096),
            ] {
                let body = body_for(&provider, &request("gpt-5-mini", Some(level)));
                assert_eq!(body["max_completion_tokens"], expected, "{level:?}");
                assert!(body.get("max_tokens").is_none());
            }

            // The total is bounded, and a ceiling the caller set above the cap
            // is never lowered.
            let mut near = request("o3", Some(Level::High));
            near.max_tokens = 30_000;
            assert_eq!(
                body_for(&provider, &near)["max_completion_tokens"],
                effort::TOTAL_TOKEN_CAP
            );
            let mut above = request("o3", Some(Level::High));
            above.max_tokens = 40_000;
            assert_eq!(body_for(&provider, &above)["max_completion_tokens"], 40_000);
        }

        #[test]
        fn catalogue_lists_levels_only_while_the_switch_is_on() {
            let models = |provider: OpenAiProvider| {
                provider.with_model_catalog(
                    vec!["gpt-5-mini".to_owned(), "gpt-4o-mini".to_owned()],
                    Vec::new(),
                )
            };

            let on = models(provider("")).list_models();
            assert_eq!(
                on[0].effort_levels,
                vec![Level::Minimal, Level::Low, Level::Medium, Level::High]
            );
            assert!(on[1].effort_levels.is_empty());

            let mut off = provider("");
            off.effort.enabled = false;
            assert!(models(off)
                .list_models()
                .iter()
                .all(|model| model.effort_levels.is_empty()));
        }

        #[test]
        fn only_a_400_naming_the_parameter_is_retried() {
            use reqwest::StatusCode;
            let named = r#"{"error":{"message":"Unrecognized request argument supplied: reasoning_effort"}}"#;
            assert!(rejects_reasoning_effort(StatusCode::BAD_REQUEST, named));
            assert!(!rejects_reasoning_effort(
                StatusCode::BAD_REQUEST,
                "bad json"
            ));
            assert!(!rejects_reasoning_effort(
                StatusCode::INTERNAL_SERVER_ERROR,
                named
            ));
        }

        fn completion() -> serde_json::Value {
            serde_json::json!({
                "model": "gpt-5-mini",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 3, "completion_tokens": 2},
            })
        }

        #[tokio::test]
        async fn a_rejected_level_is_retried_once_without_the_key() {
            use wiremock::matchers::{method, path};
            use wiremock::{Mock, MockServer, ResponseTemplate};

            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .and(|request: &wiremock::Request| {
                    String::from_utf8_lossy(&request.body).contains("reasoning_effort")
                })
                .respond_with(ResponseTemplate::new(400).set_body_string(
                    r#"{"error":{"message":"Unrecognized request argument: reasoning_effort"}}"#,
                ))
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(200).set_body_json(completion()))
                .mount(&server)
                .await;

            let mut provider = OpenAiProvider::new("key", Some(server.uri())).expect("provider");
            provider.effort.enabled = true;

            let response = provider
                .infer(&request("gpt-5-mini", Some(Level::High)))
                .await
                .expect("the retry succeeds");
            assert_eq!(response.content, "ok");

            let sent = server.received_requests().await.expect("recorded");
            assert_eq!(sent.len(), 2, "one attempt with the key, one retry without");
            assert!(String::from_utf8_lossy(&sent[0].body).contains("reasoning_effort"));
            assert!(!String::from_utf8_lossy(&sent[1].body).contains("reasoning_effort"));
        }

        #[tokio::test]
        async fn an_unrelated_400_is_not_retried() {
            use wiremock::matchers::{method, path};
            use wiremock::{Mock, MockServer, ResponseTemplate};

            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/chat/completions"))
                .respond_with(ResponseTemplate::new(400).set_body_string("invalid tool schema"))
                .mount(&server)
                .await;

            let mut provider = OpenAiProvider::new("key", Some(server.uri())).expect("provider");
            provider.effort.enabled = true;

            let result = provider
                .infer(&request("gpt-5-mini", Some(Level::High)))
                .await;
            assert!(result.is_err());
            let sent = server.received_requests().await.expect("recorded");
            assert_eq!(sent.len(), 1);
        }
    }
}
