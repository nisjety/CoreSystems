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
        if !req.tools.is_empty() || req.structured_output_schema.is_some() {
            return Err(ProviderError::UnsupportedModel(
                "openai-codex-subscription supports text-only inference; tools and structured output are unavailable"
                    .to_owned(),
            ));
        }
        if !self.models.iter().any(|model| model == &req.model) {
            return Err(ProviderError::UnsupportedModel(format!(
                "model `{}` is not enabled for openai-codex-subscription",
                req.model
            )));
        }
        Ok(BrokerRequest {
            organization_id: &req.org_id,
            user_id: &req.user_id,
            connection_id: &req.subscription_connection_id,
            request_id: &req.request_id,
            model: &req.model,
            messages: &req.messages,
            max_tokens: req.max_tokens,
            reasoning_effort: codex_reasoning_effort(req.thinking_budget_tokens),
            service_tier: codex_service_tier(&req.model, req.thinking_budget_tokens),
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

/// The composer already exposes Quick as an explicit latency/usage choice.
/// Codex calls that mode `priority`; it is available only on the subscription
/// models that advertise a speed tier. Standard and Deep keep normal usage.
fn codex_service_tier(model: &str, thinking_budget_tokens: i32) -> Option<&'static str> {
    if thinking_budget_tokens != 1_024 {
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
    messages: &'a [ChatMessage],
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
            supports_tools: false,
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
        Ok(InferResponse {
            request_id: if result.request_id.is_empty() {
                req.request_id.clone()
            } else {
                result.request_id
            },
            content: result.content,
            model_used: if result.model_used.is_empty() {
                req.model.clone()
            } else {
                result.model_used
            },
            stop_reason: "end_turn".to_owned(),
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
                                    request_id: if event.request_id.is_empty() {
                                        request_id.clone()
                                    } else {
                                        event.request_id
                                    },
                                    delta: String::new(),
                                    done: true,
                                    model_used: if event.model_used.is_empty() {
                                        model.clone()
                                    } else {
                                        event.model_used
                                    },
                                    input_tokens: 0,
                                    output_tokens: 0,
                                    stop_reason: "end_turn".to_owned(),
                                    reasoning_delta: String::new(),
                                    provider_used: String::new(),
                                    residency: String::new(),
                                    token_confidence: None,
                                })
                                .await;
                            return;
                        }
                        "error" => {
                            warn!(code = %event.code, "subscription broker stream failed");
                            let _ = tx
                                .send(InferChunk {
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
    fn enables_priority_service_only_for_quick_compatible_models() {
        assert_eq!(codex_service_tier("gpt-6-astra", 1_024), Some("priority"));
        assert_eq!(codex_service_tier("gpt-5.6-luna", 1_024), Some("priority"));
        assert_eq!(codex_service_tier("gpt-6-astra", 0), None);
        assert_eq!(codex_service_tier("gpt-6-astra", 4_096), None);
        assert_eq!(codex_service_tier("gpt-5.3-codex-spark", 1_024), None);
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
