//! ChatGPT subscription-backed inference through Integration Core.
//!
//! This adapter never accepts, stores, or forwards a ChatGPT OAuth token.
//! Integration Core owns the official Codex app-server's managed auth state;
//! Model Plane supplies only its service credential plus an opaque, scoped
//! connection id from the authenticated inference request.

use std::sync::Arc;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::warn;

use super::{
    ChatMessage, InferChunk, InferRequest, InferResponse, ModelFamily, ModelInfo, PrivacyTier,
    ProviderCapabilities, ProviderError, ProviderRouter, Residency,
};

const PROVIDER_ID: &str = "openai-codex-subscription";

#[derive(Clone)]
pub struct CodexSubscriptionProvider {
    client: reqwest::Client,
    integration_core_url: Arc<str>,
    internal_api_key: Arc<str>,
    models: Vec<String>,
}

impl CodexSubscriptionProvider {
    pub fn new(
        integration_core_url: impl Into<String>,
        internal_api_key: impl Into<String>,
        models: Vec<String>,
    ) -> Result<Self, ProviderError> {
        let integration_core_url = integration_core_url.into();
        let internal_api_key = internal_api_key.into();
        if integration_core_url.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "CODEX_SUBSCRIPTION_INTEGRATION_CORE_URL is empty".to_owned(),
            ));
        }
        if internal_api_key.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "CODEX_SUBSCRIPTION_INTERNAL_API_KEY is empty".to_owned(),
            ));
        }
        if models.is_empty() {
            return Err(ProviderError::Unavailable(
                "CODEX_SUBSCRIPTION_MODELS must list at least one enabled model".to_owned(),
            ));
        }
        Ok(Self {
            client: super::provider_http_client(),
            integration_core_url: Arc::from(integration_core_url.trim_end_matches('/')),
            internal_api_key: Arc::from(internal_api_key),
            models,
        })
    }

    fn url(&self) -> String {
        format!(
            "{}/internal/model-subscriptions/openai-codex/infer",
            self.integration_core_url
        )
    }

    fn request_body<'a>(&self, req: &'a InferRequest) -> Result<BrokerRequest<'a>, ProviderError> {
        if req.subscription_connection_id.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "subscription_connection_id is required for openai-codex-subscription".to_owned(),
            ));
        }
        if req.org_id.trim().is_empty() || req.user_id.trim().is_empty() {
            return Err(ProviderError::Unavailable(
                "verified organization and user scope are required for a subscription connection"
                    .to_owned(),
            ));
        }
        if !self.models.iter().any(|model| model == &req.model) {
            return Err(ProviderError::UnsupportedModel(format!(
                "model `{}` is not enabled for openai-codex-subscription",
                req.model
            )));
        }
        let (messages, output_schema) = super::codex_subscription_tools::prepare(req)?;
        Ok(BrokerRequest {
            organization_id: &req.org_id,
            user_id: &req.user_id,
            connection_id: &req.subscription_connection_id,
            request_id: &req.request_id,
            model: &req.model,
            messages,
            output_schema,
            max_tokens: req.max_tokens,
            reasoning_effort: codex_reasoning_effort(req.thinking_budget_tokens),
            service_tier: codex_service_tier(&req.model, req.thinking_budget_tokens, req.prefer_priority_service_tier),
        })
    }

    fn stream_url(&self) -> String {
        format!("{}/stream", self.url())
    }
}

/// Codex models otherwise choose their own default reasoning effort (medium
/// for most subscription models). Normal and quick chat should favor latency;
/// only the gateway's explicit deep profile asks Codex for heavier reasoning.
fn codex_reasoning_effort(thinking_budget_tokens: i32) -> &'static str {
    if thinking_budget_tokens > 1_024 {
        "high"
    } else {
        "low"
    }
}

/// Quick opts into priority through its profile. A source reviewer may also
/// request priority explicitly while retaining high reasoning effort. Both
/// paths stay on the same subscription connection and allowed model.
fn codex_service_tier(model: &str, thinking_budget_tokens: i32, prefer_priority: bool) -> Option<&'static str> {
    if thinking_budget_tokens != 1_024 && !prefer_priority {
        return None;
    }
    match model {
        "gpt-6-astra" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" | "gpt-5.5" => {
            Some("priority")
        }
        _ => None,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrokerRequest<'a> {
    organization_id: &'a str,
    user_id: &'a str,
    connection_id: &'a str,
    request_id: &'a str,
    model: &'a str,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_schema: Option<serde_json::Value>,
    max_tokens: i32,
    reasoning_effort: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    service_tier: Option<&'static str>,
}

#[derive(Deserialize)]
struct BrokerEnvelope {
    success: bool,
    data: Option<BrokerData>,
    error: Option<BrokerError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrokerData {
    response: BrokerResponse,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrokerResponse {
    request_id: String,
    content: String,
    model_used: String,
}

#[derive(Deserialize)]
struct BrokerError {
    code: Option<String>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BrokerStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: String,
    #[serde(default)]
    request_id: String,
    #[serde(default)]
    model_used: String,
    #[serde(default)]
    code: String,
}

#[async_trait::async_trait]
impl ProviderRouter for CodexSubscriptionProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: PROVIDER_ID.to_owned(),
            aliases: vec!["chatgpt-codex".to_owned(), "codex-subscription".to_owned()],
            model_family: ModelFamily::OpenAiCompatible,
            // A ChatGPT subscription does not make an EU, sovereign, or ZDR
            // commitment available to this product integration.
            residency: Residency::Global,
            exclusive_catalog: true,
            supports_tools: true,
            supports_vision: false,
            supports_thinking: true,
            supports_streaming: true,
            supports_embeddings: false,
            supports_zdr: false,
            modalities: vec!["chat".to_owned()],
            max_context_tokens: 0,
            max_output_tokens: 0,
        }
    }

    async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        if req.zdr {
            return Err(ProviderError::ZdrUnavailable(
                "ChatGPT subscription execution has no verified ZDR commitment".to_owned(),
            ));
        }
        let body = self.request_body(req)?;
        let response = self
            .client
            .post(self.url())
            .header("X-Internal-API-Key", self.internal_api_key.as_ref())
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                ProviderError::Http(format!("subscription broker request: {error}"))
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after_ms = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map_or(1_000, |seconds| seconds.saturating_mul(1_000));
            return Err(ProviderError::RateLimited { retry_after_ms });
        }
        let envelope: BrokerEnvelope = response.json().await.map_err(|error| {
            ProviderError::InvalidResponse(format!("subscription broker response: {error}"))
        })?;
        if !status.is_success() || !envelope.success {
            let detail = envelope.error.and_then(|error| {
                error
                    .code
                    .or(error.message)
                    .map(|value| value.trim().to_owned())
            });
            return Err(ProviderError::Unavailable(format!(
                "subscription broker rejected the request{}",
                detail
                    .filter(|value| !value.is_empty())
                    .map_or_else(String::new, |value| format!(": {value}"))
            )));
        }
        let result = envelope
            .data
            .ok_or_else(|| {
                ProviderError::InvalidResponse(
                    "subscription broker success response omitted data".to_owned(),
                )
            })?
            .response;
        if result.model_used != req.model {
            return Err(ProviderError::InvalidResponse(
                "subscription broker changed the selected model".into(),
            ));
        }
        let (content, tool_calls) = super::codex_subscription_tools::parse(req, &result.content)?;
        let stop_reason = if tool_calls.is_empty() { "end_turn" } else { "tool_use" }.to_owned();
        Ok(InferResponse {
            tool_calls,
            compaction_summary: String::new(),
            request_id: if result.request_id.is_empty() {
                req.request_id.clone()
            } else {
                result.request_id
            },
            content,
            model_used: if result.model_used.is_empty() {
                req.model.clone()
            } else {
                result.model_used
            },
            stop_reason,
            input_tokens: 0,
            output_tokens: 0,
            ..InferResponse::default()
        })
    }

    async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        if req.zdr {
            return Err(ProviderError::ZdrUnavailable(
                "ChatGPT subscription execution has no verified ZDR commitment".to_owned(),
            ));
        }
        if !req.tools.is_empty() {
            return Err(ProviderError::UnsupportedModel(
                "subscription tool proposals require non-streaming inference".into(),
            ));
        }
        let body = self.request_body(req)?;
        let response = self
            .client
            .post(self.stream_url())
            .header("X-Internal-API-Key", self.internal_api_key.as_ref())
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                ProviderError::Http(format!("subscription broker stream request: {error}"))
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after_ms = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map_or(1_000, |seconds| seconds.saturating_mul(1_000));
            return Err(ProviderError::RateLimited { retry_after_ms });
        }
        if !status.is_success() {
            let envelope: BrokerEnvelope = response.json().await.map_err(|error| {
                ProviderError::InvalidResponse(format!(
                    "subscription broker stream rejection: {error}"
                ))
            })?;
            let detail = envelope.error.and_then(|error| {
                error
                    .code
                    .or(error.message)
                    .map(|value| value.trim().to_owned())
            });
            return Err(ProviderError::Unavailable(format!(
                "subscription broker rejected the stream{}",
                detail
                    .filter(|value| !value.is_empty())
                    .map_or_else(String::new, |value| format!(": {value}"))
            )));
        }

        let request_id = req.request_id.clone();
        let model = req.model.clone();
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            let mut bytes_stream = response.bytes_stream();
            // Keep raw bytes until a complete SSE line arrives. HTTP chunk
            // boundaries may split a multi-byte UTF-8 character.
            let mut buffer = Vec::new();
            while let Some(chunk_result) = bytes_stream.next().await {
                let bytes = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        warn!(error = %error, "subscription broker stream read failed");
                        break;
                    }
                };
                buffer.extend_from_slice(&bytes);
                while let Some(newline_pos) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line_bytes: Vec<u8> = buffer.drain(..=newline_pos).collect();
                    let Ok(line) = std::str::from_utf8(&line_bytes[..newline_pos]) else {
                        warn!("subscription broker emitted an invalid UTF-8 SSE line");
                        continue;
                    };
                    let line = line.trim();
                    let Some(data) = line.strip_prefix("data: ") else {
                        continue;
                    };
                    let Ok(event) = serde_json::from_str::<BrokerStreamEvent>(data) else {
                        continue;
                    };
                    match event.event_type.as_str() {
                        "delta" if !event.delta.is_empty() => {
                            if tx
                                .send(InferChunk {
                                    compaction_summary: String::new(),
                                    request_id: request_id.clone(),
                                    delta: event.delta,
                                    done: false,
                                    model_used: String::new(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    stop_reason: String::new(),
                                    reasoning_delta: String::new(),
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    token_confidence: None,
                                    // The subscription broker's own contract
                                    // carries no cache-usage telemetry (unlike
                                    // Anthropic's first-party API); 0 is exact,
                                    // not a placeholder.
                                    cache_read_input_tokens: 0,
                                    cache_creation_input_tokens: 0,
                                })
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        "done" => {
                            let _ = tx
                                .send(InferChunk {
                                    compaction_summary: String::new(),
                                    request_id: if event.request_id.is_empty() {
                                        request_id.clone()
                                    } else {
                                        event.request_id
                                    },
                                    delta: String::new(),
                                    done: true,
                                    model_used: model.clone(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    stop_reason: if event.model_used == model {
                                        "end_turn"
                                    } else {
                                        "subscription_model_mismatch"
                                    }
                                    .to_owned(),
                                    reasoning_delta: String::new(),
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    token_confidence: None,
                                    cache_read_input_tokens: 0,
                                    cache_creation_input_tokens: 0,
                                })
                                .await;
                            return;
                        }
                        "error" => {
                            warn!(code = %event.code, "subscription broker stream failed");
                            let _ = tx
                                .send(InferChunk {
                                    compaction_summary: String::new(),
                                    request_id: request_id.clone(),
                                    delta: String::new(),
                                    done: true,
                                    model_used: model.clone(),
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    stop_reason: "stream_error".to_owned(),
                                    reasoning_delta: String::new(),
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    token_confidence: None,
                                    cache_read_input_tokens: 0,
                                    cache_creation_input_tokens: 0,
                                })
                                .await;
                            return;
                        }
                        _ => {}
                    }
                }
            }

            let _ = tx
                .send(InferChunk {
                    compaction_summary: String::new(),
                    request_id,
                    delta: String::new(),
                    done: true,
                    model_used: model,
                    input_tokens: 0,
                    output_tokens: 0,
                    stop_reason: "stream_incomplete".to_owned(),
                    reasoning_delta: String::new(),
                    provider_used: String::new(),
                    residency: String::new(),
                    token_confidence: None,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                })
                .await;
        });
        Ok(rx)
    }

    fn list_models(&self) -> Vec<ModelInfo> {
        let caps = self.capabilities();
        self.models
            .iter()
            .map(|model| ModelInfo {
                id: model.clone(),
                provider: PROVIDER_ID.to_owned(),
                modality: "chat".to_owned(),
                streaming: true,
                features: caps.feature_flags(),
                cheap: false,
                privacy_tier: PrivacyTier::classify(&caps),
                residency_label: caps.residency.as_str().to_owned(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[test]
    fn requires_a_connection_and_explicit_model_catalog() {
        assert!(
            CodexSubscriptionProvider::new("http://integration", "service-key", Vec::new())
                .is_err()
        );
        let provider = CodexSubscriptionProvider::new(
            "http://integration",
            "service-key",
            vec!["gpt-codex".to_owned()],
        )
        .expect("provider");
        let request = InferRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            model: "gpt-codex".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            ..InferRequest::default()
        };
        let error = provider
            .request_body(&request)
            .expect_err("missing connection must fail");
        assert!(matches!(error, ProviderError::Unavailable(_)));

        let request = InferRequest {
            subscription_connection_id: "conn-1".to_owned(),
            model: "not-enabled".to_owned(),
            ..request
        };
        let error = provider
            .request_body(&request)
            .expect_err("the subscription catalog must be an allow-list");
        assert!(matches!(error, ProviderError::UnsupportedModel(_)));
    }

    #[test]
    fn maps_gateway_effort_to_codex_reasoning_effort() {
        assert_eq!(codex_reasoning_effort(0), "low");
        assert_eq!(codex_reasoning_effort(1_024), "low");
        assert_eq!(codex_reasoning_effort(4_096), "high");
    }

    #[test]
    fn enables_priority_only_when_requested_on_compatible_models() {
        assert_eq!(codex_service_tier("gpt-6-astra", 1_024, false), Some("priority"));
        assert_eq!(codex_service_tier("gpt-5.6-luna", 1_024, false), Some("priority"));
        assert_eq!(codex_service_tier("gpt-5.6-terra", 4_096, true), Some("priority"));
        assert_eq!(codex_service_tier("gpt-6-astra", 0, false), None);
        assert_eq!(codex_service_tier("gpt-6-astra", 4_096, false), None);
        assert_eq!(codex_service_tier("gpt-5.3-codex-spark", 1_024, true), None);
    }

    #[test]
    fn broker_request_includes_reasoning_effort() {
        let provider = CodexSubscriptionProvider::new(
            "http://integration",
            "service-key",
            vec!["gpt-6-astra".to_owned()],
        )
        .expect("provider");
        let request = InferRequest {
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            subscription_connection_id: "conn-1".to_owned(),
            model: "gpt-6-astra".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            thinking_budget_tokens: 4_096,
            ..InferRequest::default()
        };

        let body = serde_json::to_value(provider.request_body(&request).expect("request body"))
            .expect("serialize request body");
        assert_eq!(body["reasoningEffort"], "high");
        assert!(body.get("serviceTier").is_none());

        let priority_review = InferRequest {
            prefer_priority_service_tier: true,
            ..request.clone()
        };
        let priority_body = serde_json::to_value(
            provider.request_body(&priority_review).expect("priority review body")
        ).expect("serialize priority review body");
        assert_eq!(priority_body["reasoningEffort"], "high");
        assert_eq!(priority_body["serviceTier"], "priority");

        let quick_request = InferRequest {
            thinking_budget_tokens: 1_024,
            ..request
        };
        let quick_body = serde_json::to_value(
            provider
                .request_body(&quick_request)
                .expect("quick request body"),
        )
        .expect("serialize quick request body");
        assert_eq!(quick_body["reasoningEffort"], "low");
        assert_eq!(quick_body["serviceTier"], "priority");
    }

    #[tokio::test]
    async fn tool_proposals_preserve_subscription_scope_through_the_broker() {
        use serde_json::json;
        let server = MockServer::start().await;
        let proposal = json!({"content":"", "toolCalls":[{"name":"create_artifact", "arguments":"{\"content\":\"Hei\"}"}]}).to_string();
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"success":true,"data":{"response":{
                    "requestId":"tool-round","content":proposal,"modelUsed":"gpt-5.6-terra"
                }}}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();
        let request = InferRequest {
            request_id: "tool-round".into(),
            model: "gpt-5.6-terra".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            subscription_connection_id: "connection".into(),
            tool_choice: "required".into(),
            tools: vec![super::super::ToolDefinition {
                name: "create_artifact".into(),
                parameters_json: r#"{"type":"object"}"#.into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let response = provider.infer(&request).await.unwrap();
        assert_eq!(response.model_used, "gpt-5.6-terra");
        assert_eq!(response.tool_calls[0].name, "create_artifact");
        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["organizationId"], "org");
        assert_eq!(body["userId"], "user");
        assert_eq!(body["connectionId"], "connection");
        assert_eq!(body["outputSchema"]["type"], "object");
        assert!(
            body.get("tools").is_none(),
            "the broker does not execute gateway tools"
        );
    }

    #[tokio::test]
    async fn changed_model_is_rejected() {
        use serde_json::json;
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/model-subscriptions/openai-codex/infer"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"success":true,"data":{"response":{
                    "requestId":"test","content":"hello","modelUsed":"another-model"
                }}}),
            ))
            .mount(&server)
            .await;
        let provider =
            CodexSubscriptionProvider::new(server.uri(), "test-key", vec!["gpt-5.6-terra".into()])
                .unwrap();
        let request = InferRequest {
            model: "gpt-5.6-terra".into(),
            org_id: "org".into(),
            user_id: "user".into(),
            provider_hint: PROVIDER_ID.into(),
            subscription_connection_id: "connection".into(),
            ..Default::default()
        };
        assert!(matches!(
            provider.infer(&request).await,
            Err(ProviderError::InvalidResponse(_))
        ));
    }

    #[tokio::test]
    async fn streams_broker_deltas_before_the_done_event() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(
                "/internal/model-subscriptions/openai-codex/infer/stream",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                concat!(
                    "event: subscription.ready\n",
                    "data: {\"type\":\"ready\",\"requestId\":\"req-1\"}\n\n",
                    "event: subscription.delta\n",
                    "data: {\"type\":\"delta\",\"delta\":\"hello \"}\n\n",
                    "event: subscription.delta\n",
                    "data: {\"type\":\"delta\",\"delta\":\"world\"}\n\n",
                    "event: subscription.done\n",
                    "data: {\"type\":\"done\",\"requestId\":\"req-1\",\"modelUsed\":\"gpt-6-astra\"}\n\n"
                ),
                "text/event-stream",
            ))
            .mount(&server)
            .await;

        let provider = CodexSubscriptionProvider::new(
            server.uri(),
            "service-key",
            vec!["gpt-6-astra".to_owned()],
        )
        .expect("provider");
        let request = InferRequest {
            request_id: "req-1".to_owned(),
            org_id: "org-1".to_owned(),
            user_id: "user-1".to_owned(),
            subscription_connection_id: "conn-1".to_owned(),
            model: "gpt-6-astra".to_owned(),
            messages: vec![ChatMessage {
                compaction_summary: String::new(),
                role: "user".to_owned(),
                content: "hello".to_owned(),
                name: String::new(),
            }],
            ..InferRequest::default()
        };

        let mut stream = provider.infer_stream(&request).await.expect("stream");
        let first = stream.recv().await.expect("first delta");
        let second = stream.recv().await.expect("second delta");
        let done = stream.recv().await.expect("done");
        assert_eq!(first.delta, "hello ");
        assert_eq!(second.delta, "world");
        assert!(done.done);
        assert_eq!(done.stop_reason, "end_turn");
        assert_eq!(done.model_used, "gpt-6-astra");
    }
}
