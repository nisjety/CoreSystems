//! Gemini chat on Vertex AI, over the native `generateContent` and
//! `streamGenerateContent` surface.
//!
//! The provider is dark: [`crate::provider::fallback`] registers it only when
//! `VERTEX_GEMINI_ENABLED` is `true` and the project, an EU location, a credentials
//! file and a model list are all present. It declares itself EU-resident and never
//! ZDR. Tools, images and structured output are refused with a typed error before
//! any call, so a request that needs them moves on to a provider that can serve
//! it, instead of losing them on the way.

use std::sync::Arc;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::effort::{Family, ModelAliases};
use super::model_record;
use super::overflow;
use super::vertex_auth::VertexTokenProvider;
use super::{
    features_for_model, provider_http_client, InferChunk, InferRequest, InferResponse, ModelFamily,
    ModelInfo, PrivacyTier, ProviderCapabilities, ProviderError, ProviderRouter, Residency,
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
        let body = build_request_body(req);
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
        let body = build_request_body(req);
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
                    effort_levels: Vec::new(),
                    context_window_tokens: record.context_window_tokens,
                    max_output_tokens: record.max_output_tokens,
                    lifecycle: record.lifecycle,
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
            supports_tools: false,
            supports_vision: false,
            supports_thinking: false,
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

/// Refuse what this adapter does not send yet, before any network call. Each refusal
/// is typed so the chain moves on to a provider that can serve the request.
fn refuse_unsupported(req: &InferRequest) -> Result<(), ProviderError> {
    if !req.image_parts.is_empty() {
        return Err(ProviderError::VisionUnsupported(
            "vertex-gemini does not send images yet".to_owned(),
        ));
    }
    if req
        .structured_output_schema
        .as_deref()
        .is_some_and(|schema| !schema.is_empty())
    {
        return Err(ProviderError::RequestRejected {
            detail: "feature_unsupported: structured_output".to_owned(),
        });
    }
    if !req.tools.is_empty() {
        return Err(ProviderError::RequestRejected {
            detail: "feature_unsupported: tools".to_owned(),
        });
    }
    Ok(())
}

/// The `generateContent` body for `req`. System messages move to
/// `systemInstruction`, assistant turns become `model` turns, and every other role
/// is sent as `user`, the way the tool loops frame tool results.
fn build_request_body(req: &InferRequest) -> serde_json::Value {
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
    if req.max_tokens > 0 {
        body["generationConfig"]["maxOutputTokens"] = serde_json::json!(req.max_tokens);
    }
    if !system.is_empty() {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{ "text": system.join("\n\n") }],
        });
    }
    body
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
        ]));

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
    fn structured_output_refused_until_sanitiser() {
        let mut req = request(vec![("user", "Give JSON")]);
        req.structured_output_schema = Some(r#"{"type":"object"}"#.to_owned());
        let error = refuse_unsupported(&req).expect_err("refused");
        assert!(
            matches!(&error, ProviderError::RequestRejected { detail }
                if detail == "feature_unsupported: structured_output"),
            "{error:?}"
        );
    }

    #[test]
    fn tools_are_refused_until_the_tool_wire_lands() {
        let mut req = request(vec![("user", "Search")]);
        req.tools = vec![ToolDefinition {
            name: "search".to_owned(),
            description: String::new(),
            parameters_json: "{}".to_owned(),
        }];
        let error = refuse_unsupported(&req).expect_err("refused");
        assert!(
            matches!(&error, ProviderError::RequestRejected { detail }
                if detail == "feature_unsupported: tools"),
            "{error:?}"
        );
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
