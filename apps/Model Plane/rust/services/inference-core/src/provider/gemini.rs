//! Gemini chat on Vertex AI, over the native `generateContent` and
//! `streamGenerateContent` surface.
//!
//! The provider is dark: [`crate::provider::fallback`] registers it only when
//! `VERTEX_GEMINI_ENABLED` is `true` and the project, an EU location, a credentials
//! file and a model list are all present. It declares itself EU-resident and never
//! ZDR. Images are refused with a typed error before any call, and so is a streamed
//! request that offers tools, since a stream cannot carry a function call. A request
//! that needs a refused feature moves on to a provider that can serve it, instead of
//! losing the feature on the way.

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::effort::{self, Family, Level, ModelAliases, Resolved, Wire};
use super::gemini_schema;
use super::model_record;
use super::overflow;
use super::vertex_auth::VertexTokenProvider;
use super::{
    features_for_model, provider_http_client, InferChunk, InferRequest, InferResponse, ModelFamily,
    ModelInfo, PrivacyTier, ProviderCapabilities, ProviderError, ProviderRouter, Residency,
    ToolCall, ToolDefinition,
};
use crate::config::{is_vertex_eu_location, vertex_residency, VertexGeminiConfig};

/// The registry id this provider is addressed by.
const PROVIDER_ID: &str = "vertex-gemini";
/// Finish reasons that refuse an answer for safety or policy, as Vertex names them.
const SAFETY_FINISH_REASONS: &[&str] = &[
    "SAFETY",
    "RECITATION",
    "BLOCKLIST",
    "PROHIBITED_CONTENT",
    "SPII",
    "IMAGE_SAFETY",
];

/// One Vertex AI endpoint serving the configured Gemini models.
pub struct GeminiProvider {
    client: reqwest::Client,
    tokens: Arc<VertexTokenProvider>,
    project: String,
    location: String,
    /// `https://aiplatform.eu.rep.googleapis.com` for the EU multi-region, and
    /// `https://<location>-aiplatform.googleapis.com` for a regional location.
    base_url: String,
    models: Vec<String>,
}

impl GeminiProvider {
    /// The provider these settings describe, or `None` when registration is refused.
    /// Every refusal is logged here, so the chain only has to skip it. A missing or
    /// unusable credentials file refuses registration, never boot.
    #[must_use]
    pub fn from_config(gemini: &VertexGeminiConfig) -> Option<Self> {
        if !gemini.enabled {
            info!(
                provider = PROVIDER_ID,
                "provider not registered: VERTEX_GEMINI_ENABLED is not true"
            );
            return None;
        }
        if gemini.project.is_empty()
            || gemini.location.is_empty()
            || gemini.credentials_file.is_empty()
            || gemini.models.is_empty()
        {
            warn!(
                provider = PROVIDER_ID,
                "provider not registered: project, location, credentials file or model list is missing"
            );
            return None;
        }
        if !is_vertex_eu_location(&gemini.location) {
            warn!(
                provider = PROVIDER_ID,
                location = %gemini.location,
                "provider not registered: the location is outside the EU allowlist"
            );
            return None;
        }
        let tokens = match VertexTokenProvider::from_file(&gemini.credentials_file) {
            Ok(tokens) => tokens,
            Err(error) => {
                warn!(
                    provider = PROVIDER_ID,
                    error = %error,
                    "provider not registered: the credentials file was refused"
                );
                return None;
            }
        };
        Some(Self {
            client: provider_http_client(),
            tokens: Arc::new(tokens),
            project: gemini.project.clone(),
            location: gemini.location.clone(),
            base_url: endpoint_base(&gemini.location),
            models: gemini.models.clone(),
        })
    }

    /// The URL of one method on one model. The chain has matched the model against
    /// this provider's own catalogue, so it is a plain configured id.
    fn method_url(&self, model: &str, method: &str) -> String {
        format!(
            "{}/v1/projects/{}/locations/{}/publishers/google/models/{model}:{method}",
            self.base_url, self.project, self.location
        )
    }

    /// Send one call with a fresh bearer token. Returns the response once its status
    /// is a success, and a typed error otherwise.
    async fn post(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, ProviderError> {
        let token = self.tokens.access_token().await?;
        let response = self
            .client
            .post(url)
            .bearer_auth(token)
            .json(body)
            .send()
            .await
            .map_err(|error| ProviderError::Http(error.to_string()))?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(1);
            return Err(ProviderError::RateLimited {
                retry_after_ms: retry_after.saturating_mul(1_000),
            });
        }
        if !status.is_success() {
            let text = response
                .text()
                .await
                .unwrap_or_else(|_| "no body".to_owned());
            return Err(classify_failure(status, &text));
        }
        Ok(response)
    }

    /// The provider with its endpoint at `base_url`, for tests that run the wire
    /// format against a mock server.
    #[cfg(test)]
    fn for_test(base_url: &str, tokens: VertexTokenProvider, models: &[&str]) -> Self {
        Self {
            client: provider_http_client(),
            tokens: Arc::new(tokens),
            project: "test-project".to_owned(),
            location: "europe-west4".to_owned(),
            base_url: base_url.to_owned(),
            models: models.iter().map(|model| (*model).to_owned()).collect(),
        }
    }
}

#[async_trait::async_trait]
impl ProviderRouter for GeminiProvider {
    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        refuse_unsupported(req)?;
        let body = build_request_body(req)?;
        let json: serde_json::Value = self
            .post(&self.method_url(&req.model, "generateContent"), &body)
            .await?
            .json()
            .await
            .map_err(|error| ProviderError::InvalidResponse(error.to_string()))?;
        parse_response(&req.request_id, &req.model, &json)
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        refuse_unsupported(req)?;
        // A stream carries text deltas only, so a function call the model made would be
        // lost. Refused before any call, so the chain walks on.
        if !req.tools.is_empty() {
            return Err(ProviderError::RequestRejected {
                detail: "feature_unsupported: streamed_tools".to_owned(),
            });
        }
        let body = build_request_body(req)?;
        let url = format!(
            "{}?alt=sse",
            self.method_url(&req.model, "streamGenerateContent")
        );
        let response = self.post(&url, &body).await?;
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(relay_stream(
            response,
            req.request_id.clone(),
            req.model.clone(),
            tx,
        ));
        Ok(rx)
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        let caps = self.capabilities();
        let privacy_tier = PrivacyTier::classify(&caps);
        let residency_label = if caps.residency == Residency::Global {
            String::new()
        } else {
            caps.residency.as_str().to_owned()
        };
        let aliases = ModelAliases::default();
        let provider_features = caps.feature_flags();
        self.models
            .iter()
            .map(|id| {
                let record = model_record::advertised(
                    Family::Gemini,
                    &aliases,
                    id,
                    caps.max_context_tokens,
                    caps.max_output_tokens,
                );
                ModelInfo {
                    id: id.clone(),
                    provider: PROVIDER_ID.to_owned(),
                    modality: "chat".to_owned(),
                    streaming: true,
                    features: features_for_model(&caps, &provider_features, id),
                    cheap: false,
                    privacy_tier,
                    residency_label: residency_label.clone(),
                    effort_levels: aliases.support(Family::Gemini, id).levels.to_vec(),
                    context_window_tokens: record.context_window_tokens,
                    max_output_tokens: record.max_output_tokens,
                    lifecycle: record.lifecycle,
                    successor: record.successor,
                    priority_tier: record.priority_tier,
                }
            })
            .collect()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: PROVIDER_ID.to_owned(),
            aliases: vec!["gemini".to_owned()],
            model_family: ModelFamily::Gemini,
            residency: vertex_residency(&self.location),
            exclusive_catalog: true,
            supports_tools: true,
            supports_vision: false,
            // Thinking reaches the wire only through the effort switch, so it is advertised
            // only while that switch is on.
            supports_thinking: effort::gemini_effort_enabled(),
            supports_streaming: true,
            supports_embeddings: false,
            // Never ZDR. No Vertex retention attestation is read, and implicit caching
            // and abuse logging are not attested as zero-retention. The chain skips this
            // provider on every ZDR request because of this constant.
            supports_zdr: false,
            modalities: vec!["chat".to_owned()],
            // No provider-level ceiling: the per-model record table carries the limits.
            max_context_tokens: 0,
            max_output_tokens: 0,
        }
    }
}

/// The host for a Vertex location. The EU multi-region has its own host; a regional
/// location shares one pattern.
fn endpoint_base(location: &str) -> String {
    if location == "eu" {
        "https://aiplatform.eu.rep.googleapis.com".to_owned()
    } else {
        format!("https://{location}-aiplatform.googleapis.com")
    }
}

/// Refuse what this adapter does not send, before any network call. Each refusal is
/// typed so the chain moves on to a provider that can serve the request.
fn refuse_unsupported(req: &InferRequest) -> Result<(), ProviderError> {
    if !req.image_parts.is_empty() {
        return Err(ProviderError::VisionUnsupported(
            "vertex-gemini does not send images yet".to_owned(),
        ));
    }
    Ok(())
}

/// The `generateContent` body for `req`. System messages move to
/// `systemInstruction`, assistant turns become `model` turns, and every other role
/// is sent as `user`, the way the tool loops frame tool results.
fn build_request_body(req: &InferRequest) -> Result<serde_json::Value, ProviderError> {
    let mut system: Vec<&str> = Vec::new();
    let mut contents = Vec::new();
    for message in &req.messages {
        // Vertex refuses an empty text part, so an empty message is left out.
        if message.content.is_empty() {
            continue;
        }
        if message.role == "system" {
            system.push(&message.content);
            continue;
        }
        let role = if message.role == "assistant" {
            "model"
        } else {
            "user"
        };
        contents.push(serde_json::json!({
            "role": role,
            "parts": [{ "text": message.content }],
        }));
    }
    let mut body = serde_json::json!({
        "contents": contents,
        "generationConfig": { "temperature": req.temperature },
    });
    let thinking = resolve_effort(req)
        .and_then(|resolved| thinking_config(&resolved).map(|config| (resolved.applied, config)));
    if req.max_tokens > 0 {
        // Thinking spends the same output ceiling as the answer, so the ceiling rises by the
        // level's allowance, as the other adapters raise theirs.
        let ceiling = thinking.as_ref().map_or(req.max_tokens, |(applied, _)| {
            effort::raised_ceiling(req.max_tokens, *applied)
        });
        body["generationConfig"]["maxOutputTokens"] = serde_json::json!(ceiling);
    }
    if let Some((_, config)) = thinking {
        body["generationConfig"]["thinkingConfig"] = config;
    }
    if let Some(text) = req
        .structured_output_schema
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        // A function call and a JSON answer are not sent together, so a request that offers
        // tools is refused rather than sent in a shape Vertex would reject.
        if !req.tools.is_empty() {
            return Err(ProviderError::RequestRejected {
                detail: "feature_unsupported: structured_output_with_tools".to_owned(),
            });
        }
        let schema = gemini_schema::response_schema(text).map_err(|reason| {
            ProviderError::RequestRejected {
                detail: format!("feature_unsupported: structured_output ({})", reason.as_str()),
            }
        })?;
        body["generationConfig"]["responseMimeType"] = serde_json::json!("application/json");
        body["generationConfig"]["responseSchema"] = schema;
    }
    if let Some((tools, tool_config)) = tool_members(req)? {
        body["tools"] = tools;
        body["toolConfig"] = tool_config;
    }
    if !system.is_empty() {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{ "text": system.join("\n\n") }],
        });
    }
    Ok(body)
}

/// The effort the request asks for, resolved against this model's Gemini support. `None`
/// when no effort is asked for, or the model has no effort wire (the switch is off).
fn resolve_effort(req: &InferRequest) -> Option<Resolved> {
    let requested = req
        .reasoning_effort
        .or_else(|| effort::from_legacy_budget(req.thinking_budget_tokens));
    effort::resolve(
        requested,
        ModelAliases::default().support(Family::Gemini, &req.model),
    )
}

/// The `thinkingConfig` a resolved level renders to: a budget for the 2.5 generation, a
/// level name for the 3 generation. The declared rows never carry another level, so the
/// `None` arm only guards a row added without its wire.
fn thinking_config(resolved: &Resolved) -> Option<serde_json::Value> {
    match resolved.wire {
        Wire::GeminiThinkingBudget => Some(serde_json::json!({
            "thinkingBudget": effort::allowance(resolved.applied),
        })),
        Wire::GeminiThinkingLevel => match resolved.applied {
            Level::Low => Some(serde_json::json!({ "thinkingLevel": "LOW" })),
            Level::High => Some(serde_json::json!({ "thinkingLevel": "HIGH" })),
            _ => None,
        },
        _ => None,
    }
}

/// The `tools` and `toolConfig` members for the tools `req` offers, or `None` when it offers
/// none. A tool whose schema cannot be expressed is left out and counted. A forced choice
/// that then names no offered tool is refused, rather than answered with another choice.
fn tool_members(
    req: &InferRequest,
) -> Result<Option<(serde_json::Value, serde_json::Value)>, ProviderError> {
    if req.tools.is_empty() {
        return Ok(None);
    }
    let declarations = tool_declarations(&req.tools);
    if declarations.is_empty() {
        return if forced_choice(&req.tool_choice) {
            Err(refused_choice())
        } else {
            Ok(None)
        };
    }
    let offered: Vec<&str> = declarations
        .iter()
        .filter_map(|declaration| declaration["name"].as_str())
        .collect();
    let config = function_calling_config(&req.tool_choice, &offered)?;
    Ok(Some((
        serde_json::json!([{ "functionDeclarations": declarations }]),
        serde_json::json!({ "functionCallingConfig": config }),
    )))
}

/// The function declarations for `tools`. A tool whose schema cannot be expressed is left out
/// and counted under its reason. Its schema is never logged.
fn tool_declarations(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .filter_map(|tool| match gemini_schema::parameters(&tool.parameters_json) {
            Ok(parameters) => {
                let mut declaration = serde_json::json!({ "name": tool.name });
                if !tool.description.is_empty() {
                    declaration["description"] = serde_json::json!(tool.description);
                }
                if let Some(parameters) = parameters {
                    declaration["parameters"] = parameters;
                }
                Some(declaration)
            }
            Err(reason) => {
                gemini_schema::count_dropped_tool(reason);
                warn!(
                    tool = %tool.name,
                    reason = reason.as_str(),
                    "tool left out: its schema cannot be expressed for vertex-gemini"
                );
                None
            }
        })
        .collect()
}

/// The `functionCallingConfig` for a `tool_choice`. The keywords map to a mode, and a named
/// tool is forced through an allowlist holding only that tool.
fn function_calling_config(
    choice: &str,
    offered: &[&str],
) -> Result<serde_json::Value, ProviderError> {
    Ok(match choice {
        "" | "auto" => serde_json::json!({ "mode": "AUTO" }),
        "none" => serde_json::json!({ "mode": "NONE" }),
        "required" => serde_json::json!({ "mode": "ANY" }),
        name if offered.contains(&name) => serde_json::json!({
            "mode": "ANY",
            "allowedFunctionNames": [name],
        }),
        _ => return Err(refused_choice()),
    })
}

/// True when `choice` forces a call: any choice other than auto or none.
fn forced_choice(choice: &str) -> bool {
    !matches!(choice, "" | "auto" | "none")
}

fn refused_choice() -> ProviderError {
    ProviderError::RequestRejected {
        detail: "feature_unsupported: tool_choice".to_owned(),
    }
}

/// The answer to a unary call. A blocked prompt or a safety finish is a typed
/// refusal, never an empty answer.
fn parse_response(
    request_id: &str,
    model: &str,
    json: &serde_json::Value,
) -> Result<InferResponse, ProviderError> {
    let candidate = &json["candidates"][0];
    if let Some(reason) = safety_block(json, candidate) {
        return Err(ProviderError::RequestRejected {
            detail: format!("safety_blocked: {reason}"),
        });
    }
    if candidate.is_null() {
        return Err(ProviderError::InvalidResponse(
            "vertex gemini answer has no candidate".to_owned(),
        ));
    }
    let counts = usage(json);
    Ok(InferResponse {
        request_id: request_id.to_owned(),
        content: answer_text(candidate),
        model_used: model.to_owned(),
        stop_reason: stop_reason(candidate["finishReason"].as_str()),
        input_tokens: counts.input,
        output_tokens: counts.output,
        cache_read_input_tokens: counts.cache_read,
        tool_calls: function_calls(candidate),
        ..InferResponse::default()
    })
}

/// The reason a response refuses its answer for safety: the prompt was blocked, or
/// the candidate finished on a safety reason.
fn safety_block(json: &serde_json::Value, candidate: &serde_json::Value) -> Option<String> {
    if let Some(reason) = json["promptFeedback"]["blockReason"].as_str() {
        return Some(reason.to_owned());
    }
    candidate["finishReason"]
        .as_str()
        .filter(|reason| SAFETY_FINISH_REASONS.contains(reason))
        .map(str::to_owned)
}

/// The answer text of a candidate: its text parts in order, leaving out thought
/// summaries.
fn answer_text(candidate: &serde_json::Value) -> String {
    candidate["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| !part["thought"].as_bool().unwrap_or(false))
        .filter_map(|part| part["text"].as_str())
        .collect()
}

/// The function calls of a candidate, in the order the model made them. Vertex returns no
/// call id, so one is synthesised from the position, which keeps it stable for the same
/// answer. A thought signature on the part is not carried: no loop replays native
/// function-call parts.
fn function_calls(candidate: &serde_json::Value) -> Vec<ToolCall> {
    candidate["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("functionCall"))
        .enumerate()
        .map(|(index, call)| ToolCall {
            id: format!("gemini-call-{index}"),
            name: call["name"].as_str().unwrap_or_default().to_owned(),
            arguments_json: match call.get("args") {
                Some(args) if !args.is_null() => args.to_string(),
                _ => "{}".to_owned(),
            },
        })
        .collect()
}

/// The stop reason in the proto's vocabulary. A unary answer with no finish reason
/// reads as a normal end, as the Anthropic adapter reads it.
fn stop_reason(finish: Option<&str>) -> String {
    match finish {
        None | Some("STOP") => "end_turn".to_owned(),
        Some("MAX_TOKENS") => "max_tokens".to_owned(),
        Some(other) => other.to_ascii_lowercase(),
    }
}

struct Usage {
    input: i32,
    output: i32,
    cache_read: i32,
}

/// Token counts from `usageMetadata`. The prompt count already includes cached
/// content, so the cached part is reported separately rather than added again.
/// Thought tokens are billed as output, so they count there.
fn usage(json: &serde_json::Value) -> Usage {
    let meta = &json["usageMetadata"];
    let count = |field: &str| {
        meta[field]
            .as_i64()
            .map_or(0, |value| i32::try_from(value).unwrap_or(i32::MAX))
    };
    Usage {
        input: count("promptTokenCount"),
        output: count("candidatesTokenCount").saturating_add(count("thoughtsTokenCount")),
        cache_read: count("cachedContentTokenCount"),
    }
}

fn delta_chunk(request_id: &str, model: &str, delta: String) -> InferChunk {
    InferChunk {
        compaction_summary: String::new(),
        request_id: request_id.to_owned(),
        delta,
        done: false,
        model_used: model.to_owned(),
        input_tokens: 0,
        output_tokens: 0,
        stop_reason: String::new(),
        reasoning_delta: String::new(),
        provider_used: String::new(),
        residency: String::new(),
        token_confidence: None,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        provider_timing: None,
        service_tier_used: String::new(),
    }
}

/// Relay one `streamGenerateContent` response. Each text delta goes out as it
/// arrives. The final chunk carries the stop reason and the usage the stream reported
/// last, and a stream that ends without a finish reason is `stream_incomplete`.
async fn relay_stream(
    response: reqwest::Response,
    request_id: String,
    model: String,
    tx: mpsc::Sender<InferChunk>,
) {
    let mut bytes = response.bytes_stream();
    // Bytes, not text: a line is decoded only once its newline has arrived, so a
    // multi-byte character split across two network chunks is never mangled.
    let mut buffer: Vec<u8> = Vec::new();
    let mut finish: Option<String> = None;
    let mut blocked = false;
    let mut totals = Usage {
        input: 0,
        output: 0,
        cache_read: 0,
    };
    while let Some(next) = bytes.next().await {
        let Ok(chunk) = next else { break };
        buffer.extend_from_slice(&chunk);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let raw: Vec<u8> = buffer.drain(..=newline).collect();
            let line = String::from_utf8_lossy(&raw);
            let Some(data) = line.trim().strip_prefix("data:") else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data.trim()) else {
                continue;
            };
            let candidate = &json["candidates"][0];
            if safety_block(&json, candidate).is_some() {
                blocked = true;
            }
            let text = answer_text(candidate);
            if !text.is_empty()
                && tx
                    .send(delta_chunk(&request_id, &model, text))
                    .await
                    .is_err()
            {
                return;
            }
            if let Some(reason) = candidate["finishReason"].as_str() {
                finish = Some(reason.to_owned());
            }
            if json["usageMetadata"].is_object() {
                totals = usage(&json);
            }
        }
    }
    let stop = if blocked {
        "content_filter".to_owned()
    } else {
        finish.as_deref().map_or_else(
            || "stream_incomplete".to_owned(),
            |reason| stop_reason(Some(reason)),
        )
    };
    let _ = tx
        .send(InferChunk {
            compaction_summary: String::new(),
            request_id,
            delta: String::new(),
            done: true,
            model_used: model,
            input_tokens: totals.input,
            output_tokens: totals.output,
            stop_reason: stop,
            reasoning_delta: String::new(),
            provider_used: String::new(),
            residency: String::new(),
            token_confidence: None,
            cache_read_input_tokens: totals.cache_read,
            cache_creation_input_tokens: 0,
            provider_timing: None,
            service_tier_used: String::new(),
        })
        .await;
}

/// Classify a non-success response. Overflow keeps its own type; a refusal of the
/// request itself is `RequestRejected`, so the chain does not send the same request
/// here again. The rule matches the Anthropic adapter's.
fn classify_failure(status: reqwest::StatusCode, body: &str) -> ProviderError {
    match overflow::classify_http_failure(status, body) {
        ProviderError::Http(detail) if refuses_request(status) => {
            ProviderError::RequestRejected { detail }
        }
        typed => typed,
    }
}

fn refuses_request(status: reqwest::StatusCode) -> bool {
    status.is_client_error() && !matches!(status.as_u16(), 401 | 403 | 408 | 409 | 429)
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::provider::vertex_auth::test_support::key_json;
    use crate::provider::{ChatMessage, ImagePart, ToolDefinition};

    const MODEL: &str = "gemini-2.5-pro";

    fn request(messages: Vec<(&str, &str)>) -> InferRequest {
        InferRequest {
            request_id: "req-1".to_owned(),
            model: MODEL.to_owned(),
            messages: messages
                .into_iter()
                .map(|(role, content)| ChatMessage {
                    compaction_summary: String::new(),
                    role: role.to_owned(),
                    content: content.to_owned(),
                    name: String::new(),
                })
                .collect(),
            max_tokens: 256,
            temperature: 0.2,
            ..InferRequest::default()
        }
    }

    /// A provider whose token exchange and generation calls go to `server`.
    async fn provider_at(server: &MockServer) -> GeminiProvider {
        let tokens = VertexTokenProvider::from_json(&key_json(&format!("{}/token", server.uri())))
            .expect("key");
        GeminiProvider::for_test(&server.uri(), tokens, &[MODEL])
    }

    async fn mount_token(server: &MockServer) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "tok-test",
                "expires_in": 3600,
            })))
            .mount(server)
            .await;
    }

    fn generate_path(method_name: &str) -> String {
        format!(
            "/v1/projects/test-project/locations/europe-west4/publishers/google/models/{MODEL}:{method_name}"
        )
    }

    #[test]
    fn maps_system_to_system_instruction() {
        let body = build_request_body(&request(vec![
            ("system", "Be brief."),
            ("system", "Answer in Norwegian."),
            ("user", "Hi"),
            ("assistant", "Hei"),
            ("user", "Thanks"),
        ]))
        .expect("a plain body");

        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "Be brief.\n\nAnswer in Norwegian."
        );
        let roles: Vec<&str> = body["contents"]
            .as_array()
            .expect("contents")
            .iter()
            .map(|turn| turn["role"].as_str().expect("role"))
            .collect();
        assert_eq!(roles, ["user", "model", "user"]);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 256);
    }

    #[test]
    fn maps_usage_and_cache_tokens() {
        let json = serde_json::json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "Hei" }] },
                "finishReason": "STOP",
            }],
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 20,
                "thoughtsTokenCount": 7,
                "cachedContentTokenCount": 60,
            },
        });

        let response = parse_response("req-1", MODEL, &json).expect("answer");
        assert_eq!(response.content, "Hei");
        assert_eq!(response.stop_reason, "end_turn");
        assert_eq!(response.input_tokens, 100);
        assert_eq!(response.output_tokens, 27);
        assert_eq!(response.cache_read_input_tokens, 60);
        assert_eq!(response.cache_creation_input_tokens, 0);
    }

    #[test]
    fn safety_block_is_typed() {
        let blocked_prompt = serde_json::json!({
            "promptFeedback": { "blockReason": "SAFETY" },
        });
        let error = parse_response("req-1", MODEL, &blocked_prompt).expect_err("refused");
        assert!(
            matches!(&error, ProviderError::RequestRejected { detail }
                if detail == "safety_blocked: SAFETY"),
            "{error:?}"
        );

        let safety_finish = serde_json::json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "partial" }] },
                "finishReason": "SAFETY",
            }],
        });
        let error = parse_response("req-1", MODEL, &safety_finish).expect_err("refused");
        assert!(
            matches!(error, ProviderError::RequestRejected { .. }),
            "{error:?}"
        );

        let max_tokens = serde_json::json!({
            "candidates": [{
                "content": { "role": "model", "parts": [{ "text": "cut" }] },
                "finishReason": "MAX_TOKENS",
            }],
        });
        let response = parse_response("req-1", MODEL, &max_tokens).expect("not a refusal");
        assert_eq!(response.stop_reason, "max_tokens");
    }

    #[tokio::test]
    async fn image_parts_refuse_vision_unsupported() {
        let mut req = request(vec![("user", "What is this?")]);
        req.image_parts = vec![ImagePart {
            message_index: 0,
            data: Arc::from(vec![1_u8, 2, 3]),
            mime_type: "image/png".to_owned(),
        }];
        // Nothing listens on port 1: a refusal that reached the network would fail
        // differently, so the typed error proves the call was refused first.
        let provider = GeminiProvider::for_test(
            "http://127.0.0.1:1",
            VertexTokenProvider::from_json(&key_json("http://127.0.0.1:1/token")).expect("key"),
            &[MODEL],
        );

        let error = provider.infer(&req).await.expect_err("images are refused");
        assert!(
            matches!(error, ProviderError::VisionUnsupported(_)),
            "{error:?}"
        );
    }

    #[test]
    fn structured_output_goes_through_the_sanitiser() {
        let mut req = request(vec![("user", "Give JSON")]);
        req.structured_output_schema = Some(
            r#"{"type":"object","properties":{"city":{"type":"string"}},"required":["city"],"additionalProperties":false}"#
                .to_owned(),
        );
        let body = build_request_body(&req).expect("a schema the sanitiser expresses");
        assert_eq!(
            body["generationConfig"]["responseMimeType"],
            "application/json"
        );
        assert_eq!(
            body["generationConfig"]["responseSchema"],
            serde_json::json!({
                "type": "OBJECT",
                "properties": {"city": {"type": "STRING"}},
                "required": ["city"],
            })
        );
        // A schema with no fields cannot be expressed, so the call is refused before the network.
        req.structured_output_schema = Some(r#"{"type":"object"}"#.to_owned());
        let error = build_request_body(&req).expect_err("refused");
        assert!(
            matches!(&error, ProviderError::RequestRejected { detail }
                if detail == "feature_unsupported: structured_output (open_object)"),
            "{error:?}"
        );
    }

    fn search_tool() -> ToolDefinition {
        ToolDefinition {
            name: "search".to_owned(),
            description: "Search the index".to_owned(),
            parameters_json: r#"{"type":"object","properties":{"q":{"type":"string"}},"required":["q"]}"#
                .to_owned(),
        }
    }

    #[test]
    fn declared_tools_ride_in_the_body_with_their_choice() {
        let mut req = request(vec![("user", "Find oslo")]);
        req.tools = vec![search_tool()];
        req.tool_choice = "search".to_owned();
        let body = build_request_body(&req).expect("body");
        assert_eq!(
            body["tools"],
            serde_json::json!([{ "functionDeclarations": [{
                "name": "search",
                "description": "Search the index",
                "parameters": {
                    "type": "OBJECT",
                    "properties": {"q": {"type": "STRING"}},
                    "required": ["q"],
                },
            }]}])
        );
        assert_eq!(
            body["toolConfig"],
            serde_json::json!({
                "functionCallingConfig": {"mode": "ANY", "allowedFunctionNames": ["search"]},
            })
        );
    }

    #[test]
    fn unexpressible_schema_drops_only_that_tool() {
        let tool = |name: &str, schema: &str| ToolDefinition {
            name: name.to_owned(),
            description: String::new(),
            parameters_json: schema.to_owned(),
        };
        let declarations = tool_declarations(&[
            tool("search", r#"{"type":"object","properties":{"q":{"type":"string"}}}"#),
            tool(
                "choose",
                r#"{"type":"object","properties":{"v":{"oneOf":[{"type":"string"}]}}}"#,
            ),
            tool("open", ""),
        ]);
        let names: Vec<&str> = declarations
            .iter()
            .filter_map(|declaration| declaration["name"].as_str())
            .collect();
        assert_eq!(names, ["search", "open"]);
        assert!(
            declarations[1].get("parameters").is_none(),
            "a tool with no arguments declares none"
        );
    }

    #[test]
    fn named_tool_choice_maps_to_any_with_allowlist() {
        let offered = ["search", "open"];
        assert_eq!(
            function_calling_config("auto", &offered).expect("auto"),
            serde_json::json!({ "mode": "AUTO" })
        );
        assert_eq!(
            function_calling_config("none", &offered).expect("none"),
            serde_json::json!({ "mode": "NONE" })
        );
        assert_eq!(
            function_calling_config("required", &offered).expect("required"),
            serde_json::json!({ "mode": "ANY" })
        );
        assert_eq!(
            function_calling_config("open", &offered).expect("a named tool"),
            serde_json::json!({ "mode": "ANY", "allowedFunctionNames": ["open"] })
        );
        // A forced choice naming no offered tool is refused: a call forced to a tool that
        // was never declared would fail at Vertex.
        let error = function_calling_config("missing", &offered).expect_err("refused");
        assert!(
            matches!(&error, ProviderError::RequestRejected { detail }
                if detail == "feature_unsupported: tool_choice"),
            "{error:?}"
        );
    }

    #[test]
    fn function_call_maps_to_tool_calls_with_stable_ids() {
        let json = serde_json::json!({
            "candidates": [{
                "content": { "role": "model", "parts": [
                    { "functionCall": { "name": "search", "args": { "q": "oslo" } } },
                    { "text": "" },
                    { "functionCall": { "name": "open", "args": { "id": 7 } } },
                ]},
                "finishReason": "STOP",
            }],
        });
        let first = parse_response("req-1", MODEL, &json).expect("tool round");
        let again = parse_response("req-1", MODEL, &json).expect("tool round");
        let ids: Vec<&str> = first.tool_calls.iter().map(|call| call.id.as_str()).collect();
        assert_eq!(ids, ["gemini-call-0", "gemini-call-1"]);
        assert_eq!(first.tool_calls, again.tool_calls, "the ids are stable for one answer");
        assert_eq!(first.tool_calls[0].name, "search");
        assert_eq!(first.tool_calls[0].arguments_json, r#"{"q":"oslo"}"#);
        assert_eq!(first.tool_calls[1].arguments_json, r#"{"id":7}"#);
        assert!(first.content.is_empty());
    }

    #[test]
    fn thought_signature_is_ignored() {
        let json = serde_json::json!({
            "candidates": [{
                "content": { "role": "model", "parts": [
                    {
                        "functionCall": { "name": "search", "args": { "q": "oslo" } },
                        "thoughtSignature": "c2lnbmF0dXJl",
                    },
                ]},
                "finishReason": "STOP",
            }],
        });
        let response = parse_response("req-1", MODEL, &json).expect("tool round");
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "search");
        assert_eq!(response.tool_calls[0].arguments_json, r#"{"q":"oslo"}"#);
        // No loop replays a native function-call part, so the request never carries one.
        let mut req = request(vec![("user", "Find oslo")]);
        req.tools = vec![search_tool()];
        let body = build_request_body(&req).expect("body");
        assert!(!body.to_string().contains("thoughtSignature"));
    }

    #[tokio::test]
    async fn a_streamed_request_with_tools_is_refused_before_the_call() {
        let mut req = request(vec![("user", "Find oslo")]);
        req.tools = vec![search_tool()];
        // Nothing listens on port 1: a refusal that reached the network would fail differently.
        let provider = GeminiProvider::for_test(
            "http://127.0.0.1:1",
            VertexTokenProvider::from_json(&key_json("http://127.0.0.1:1/token")).expect("key"),
            &[MODEL],
        );
        let Err(error) = provider.infer_stream(&req).await else {
            panic!("a streamed request with tools is refused");
        };
        assert!(
            matches!(&error, ProviderError::RequestRejected { detail }
                if detail == "feature_unsupported: streamed_tools"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_tool_round_sends_declarations_and_reads_the_call() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        Mock::given(method("POST"))
            .and(path(generate_path("generateContent")))
            .and(wiremock::matchers::body_partial_json(serde_json::json!({
                "tools": [{ "functionDeclarations": [{ "name": "search" }] }],
                "toolConfig": { "functionCallingConfig": { "mode": "AUTO" } },
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [{
                    "content": { "role": "model", "parts": [
                        { "functionCall": { "name": "search", "args": { "q": "oslo" } } },
                    ]},
                    "finishReason": "STOP",
                }],
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = provider_at(&server).await;
        let mut req = request(vec![("user", "Find oslo")]);
        req.tools = vec![search_tool()];
        let response = provider.infer(&req).await.expect("tool round");
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "search");
        server.verify().await;
    }

    #[test]
    fn capabilities_declare_eu_and_never_zdr() {
        let provider = GeminiProvider::for_test(
            "http://127.0.0.1:1",
            VertexTokenProvider::from_json(&key_json("http://127.0.0.1:1/token")).expect("key"),
            &[MODEL],
        );
        let caps = provider.capabilities();
        assert_eq!(caps.model_family, ModelFamily::Gemini);
        assert_eq!(caps.residency, Residency::Eu);
        assert!(!caps.supports_zdr);
        assert!(caps.exclusive_catalog);
        assert_eq!(PrivacyTier::classify(&caps), PrivacyTier::EuResident);
    }

    #[test]
    fn lists_the_configured_models_with_sourced_limits() {
        let provider = GeminiProvider::for_test(
            "http://127.0.0.1:1",
            VertexTokenProvider::from_json(&key_json("http://127.0.0.1:1/token")).expect("key"),
            &[MODEL],
        );
        let listed = provider.list_models();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, MODEL);
        assert_eq!(listed[0].context_window_tokens, 1_048_576);
        assert_eq!(listed[0].max_output_tokens, 65_536);
        assert_eq!(
            listed[0].lifecycle,
            Some(model_record::Lifecycle::Deprecated)
        );
        assert!(listed[0].effort_levels.is_empty());
    }

    #[test]
    fn endpoint_is_the_eu_multi_region_host_or_a_regional_host() {
        assert_eq!(
            endpoint_base("eu"),
            "https://aiplatform.eu.rep.googleapis.com"
        );
        assert_eq!(
            endpoint_base("europe-west4"),
            "https://europe-west4-aiplatform.googleapis.com"
        );
    }

    #[tokio::test]
    async fn unary_answer_comes_back_through_generate_content() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        Mock::given(method("POST"))
            .and(path(generate_path("generateContent")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "candidates": [{
                    "content": { "role": "model", "parts": [{ "text": "Hei!" }] },
                    "finishReason": "STOP",
                }],
                "usageMetadata": { "promptTokenCount": 9, "candidatesTokenCount": 2 },
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = provider_at(&server).await;
        let response = provider
            .infer(&request(vec![("user", "Hi")]))
            .await
            .expect("answer");
        assert_eq!(response.content, "Hei!");
        assert_eq!(response.input_tokens, 9);
        assert_eq!(response.output_tokens, 2);
        server.verify().await;
    }

    #[tokio::test]
    async fn stream_assembles_deltas_and_usage() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        let body = concat!(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hel\"}]}}]}\r\n\r\n",
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"lo\"}]},\"finishReason\":\"STOP\"}],",
            "\"usageMetadata\":{\"promptTokenCount\":12,\"candidatesTokenCount\":5,\"cachedContentTokenCount\":4}}\r\n\r\n",
        );
        Mock::given(method("POST"))
            .and(path(generate_path("streamGenerateContent")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .expect(1)
            .mount(&server)
            .await;

        let provider = provider_at(&server).await;
        let mut rx = provider
            .infer_stream(&request(vec![("user", "Hi")]))
            .await
            .expect("stream");
        let mut chunks = Vec::new();
        while let Some(chunk) = rx.recv().await {
            chunks.push(chunk);
        }

        let deltas: Vec<&str> = chunks
            .iter()
            .filter(|chunk| !chunk.done)
            .map(|chunk| chunk.delta.as_str())
            .collect();
        assert_eq!(deltas, ["Hel", "lo"]);
        let last = chunks.last().expect("final chunk");
        assert!(last.done);
        assert_eq!(last.stop_reason, "end_turn");
        assert_eq!(last.input_tokens, 12);
        assert_eq!(last.output_tokens, 5);
        assert_eq!(last.cache_read_input_tokens, 4);
        server.verify().await;
    }

    #[tokio::test]
    async fn a_stream_without_a_finish_reason_is_incomplete() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        let body = "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hel\"}]}}]}\r\n\r\n";
        Mock::given(method("POST"))
            .and(path(generate_path("streamGenerateContent")))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;

        let provider = provider_at(&server).await;
        let mut rx = provider
            .infer_stream(&request(vec![("user", "Hi")]))
            .await
            .expect("stream");
        let mut last = None;
        while let Some(chunk) = rx.recv().await {
            last = Some(chunk);
        }
        let last = last.expect("final chunk");
        assert!(last.done);
        assert_eq!(last.stop_reason, "stream_incomplete");
    }

    #[tokio::test]
    async fn a_rejected_request_is_refused_not_retried() {
        let server = MockServer::start().await;
        mount_token(&server).await;
        Mock::given(method("POST"))
            .and(path(generate_path("generateContent")))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad field"))
            .expect(1)
            .mount(&server)
            .await;

        let provider = provider_at(&server).await;
        let error = provider
            .infer(&request(vec![("user", "Hi")]))
            .await
            .expect_err("a 400 is refused");
        assert!(
            matches!(error, ProviderError::RequestRejected { .. }),
            "{error:?}"
        );
        server.verify().await;
    }
}
