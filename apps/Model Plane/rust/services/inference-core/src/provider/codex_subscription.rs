//! ChatGPT subscription-backed inference through Integration Core.
//!
//! This adapter never accepts, stores, or forwards a ChatGPT OAuth token.
//! Integration Core owns the official Codex app-server's managed auth state;
//! Model Plane supplies only its service credential plus an opaque, scoped
//! connection id from the authenticated inference request.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

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
        })
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
            supports_thinking: false,
            // Integration Core currently returns a completed turn. gRPC stream
            // callers receive one final chunk rather than fake token streaming.
            supports_streaming: false,
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
        let response = self.infer(req).await?;
        let (tx, rx) = mpsc::channel(1);
        let _ = tx
            .send(InferChunk {
                request_id: response.request_id,
                delta: response.content,
                done: true,
                model_used: response.model_used,
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                stop_reason: response.stop_reason,
                reasoning_delta: String::new(),
                provider_used: String::new(),
                residency: String::new(),
                token_confidence: None,
            })
            .await;
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
                streaming: false,
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
}
