//! Sequential fallback chain — tries providers in order with bounded retries.

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{
    anthropic::AnthropicProvider, openai::OpenAiProvider, EmbedRequest, EmbedResponse, InferChunk,
    InferRequest, InferResponse, ModelInfo, ProviderError, ProviderRouter,
};
use crate::cache::PromptCache;
use crate::config::InferenceConfig;

/// A boxed, type-erased provider.
type BoxedProvider = Arc<dyn ProviderRouterDyn>;

/// Object-safe version of `ProviderRouter` for dynamic dispatch in the fallback chain.
#[async_trait::async_trait]
pub trait ProviderRouterDyn: Send + Sync {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError>;
    async fn infer_stream_dyn(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError>;
    async fn create_embedding_dyn(
        &self,
        req: &EmbedRequest,
    ) -> Result<EmbedResponse, ProviderError>;
    fn list_models_dyn(&self) -> Vec<ModelInfo>;
}

#[async_trait::async_trait]
impl<T: ProviderRouter + 'static> ProviderRouterDyn for T {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        self.infer(req).await
    }

    async fn infer_stream_dyn(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        self.infer_stream(req).await
    }

    async fn create_embedding_dyn(
        &self,
        req: &EmbedRequest,
    ) -> Result<EmbedResponse, ProviderError> {
        self.create_embedding(req).await
    }

    fn list_models_dyn(&self) -> Vec<ModelInfo> {
        self.list_models()
    }
}

/// Sequential fallback chain with bounded retries per provider.
#[derive(Clone)]
pub struct FallbackChain {
    providers: Vec<(String, BoxedProvider)>,
    max_retries: u32,
    cache: Arc<PromptCache>,
}

impl FallbackChain {
    /// Build a fallback chain from configuration.
    pub fn from_config(cfg: &InferenceConfig) -> Self {
        let mut providers: Vec<(String, BoxedProvider)> = Vec::new();
        let has_explicit_azure = cfg
            .provider_order
            .iter()
            .any(|name| matches!(name.as_str(), "azure" | "azure-openai"));

        for name in &cfg.provider_order {
            match name.as_str() {
                "azure" | "azure-openai" => {
                    if let (Some(endpoint), Some(key)) =
                        (&cfg.azure_openai_endpoint, &cfg.azure_openai_api_key)
                    {
                        if let Ok(p) = OpenAiProvider::new_azure(
                            key.clone(),
                            endpoint.clone(),
                            cfg.azure_openai_api_version.clone(),
                        ) {
                            let p = p.with_model_catalog(
                                cfg.azure_openai_chat_deployments.clone(),
                                cfg.azure_openai_embedding_deployments.clone(),
                            );
                            providers.push(("azure-openai".to_owned(), Arc::new(p)));
                            info!(provider = "azure-openai", "provider registered");
                        }
                    }
                }
                "anthropic" => {
                    if let Some(key) = &cfg.anthropic_api_key {
                        if let Ok(p) = AnthropicProvider::new(key.clone()) {
                            providers.push(("anthropic".to_owned(), Arc::new(p)));
                            info!(provider = "anthropic", "provider registered");
                        }
                    }
                }
                "openai" => {
                    let mut registered_azure = false;
                    if !has_explicit_azure {
                        if let (Some(endpoint), Some(key)) =
                            (&cfg.azure_openai_endpoint, &cfg.azure_openai_api_key)
                        {
                            if let Ok(p) = OpenAiProvider::new_azure(
                                key.clone(),
                                endpoint.clone(),
                                cfg.azure_openai_api_version.clone(),
                            ) {
                                let p = p.with_model_catalog(
                                    cfg.azure_openai_chat_deployments.clone(),
                                    cfg.azure_openai_embedding_deployments.clone(),
                                );
                                providers.push(("azure-openai".to_owned(), Arc::new(p)));
                                info!(provider = "azure-openai", "provider registered");
                                registered_azure = true;
                            }
                        }
                    }

                    if !registered_azure {
                        if let Some(key) = &cfg.openai_api_key {
                            if let Ok(p) =
                                OpenAiProvider::new(key.clone(), cfg.openai_api_base.clone())
                            {
                                let p = p.with_model_catalog(
                                    cfg.openai_chat_models.clone(),
                                    cfg.openai_embedding_models.clone(),
                                );
                                providers.push(("openai".to_owned(), Arc::new(p)));
                                info!(provider = "openai", "provider registered");
                            }
                        }
                    }
                }
                other => {
                    warn!(provider = %other, "unknown provider in config, skipping");
                }
            }
        }

        Self {
            providers,
            max_retries: cfg.max_retries_per_provider,
            cache: Arc::new(PromptCache::new(cfg.cache_ttl_secs)),
        }
    }

    /// Create a fallback chain for testing with explicit providers.
    #[allow(dead_code)]
    pub fn new_with_providers(providers: Vec<(String, BoxedProvider)>, max_retries: u32) -> Self {
        Self {
            providers,
            max_retries,
            cache: Arc::new(PromptCache::new(300)),
        }
    }

    #[allow(dead_code)]
    /// Total number of registered providers.
    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    fn provider_matches(name: &str, hint: &str) -> bool {
        let hint = hint.trim().to_ascii_lowercase();
        hint.is_empty()
            || hint == name
            || (hint == "openai" && name == "azure-openai")
            || (hint == "azure" && name == "azure-openai")
    }

    /// Perform unary inference with fallback and caching.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every provider and retry is exhausted.
    pub async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        // Check cache first
        if let Some(cached) = self.cache.get(req) {
            info!(request_id = %req.request_id, "cache hit");
            return Ok(cached);
        }

        let mut total_attempts: u32 = 0;

        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            for attempt in 1..=self.max_retries {
                total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let _enter = span.enter();

                match provider.infer_dyn(req).await {
                    Ok(response) => {
                        self.cache.put(req, &response);
                        info!(
                            provider = %name,
                            attempt = attempt,
                            model_used = %response.model_used,
                            "infer succeeded"
                        );
                        return Ok(response);
                    }
                    Err(ProviderError::RateLimited { retry_after_ms }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            retry_after_ms = retry_after_ms,
                            "rate limited, moving to next provider"
                        );
                        break; // Skip remaining retries for this provider
                    }
                    Err(e) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %e,
                            "provider attempt failed"
                        );
                    }
                }
            }
        }

        Err(ProviderError::AllExhausted {
            attempts: total_attempts,
        })
    }

    /// Perform streaming inference with fallback (no caching for streams).
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every provider and retry is exhausted.
    pub async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let mut total_attempts: u32 = 0;

        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            for attempt in 1..=self.max_retries {
                total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_stream_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let _enter = span.enter();

                match provider.infer_stream_dyn(req).await {
                    Ok(rx) => {
                        info!(
                            provider = %name,
                            attempt = attempt,
                            "infer_stream started"
                        );
                        return Ok(rx);
                    }
                    Err(ProviderError::RateLimited { retry_after_ms }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            retry_after_ms = retry_after_ms,
                            "rate limited, moving to next provider"
                        );
                        break;
                    }
                    Err(e) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %e,
                            "provider stream attempt failed"
                        );
                    }
                }
            }
        }

        Err(ProviderError::AllExhausted {
            attempts: total_attempts,
        })
    }

    /// Create an embedding with provider fallback.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every matching provider fails.
    pub async fn create_embedding(
        &self,
        req: &EmbedRequest,
    ) -> Result<EmbedResponse, ProviderError> {
        let mut total_attempts: u32 = 0;

        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            for attempt in 1..=self.max_retries {
                total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_embedding_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let _enter = span.enter();

                match provider.create_embedding_dyn(req).await {
                    Ok(response) => {
                        info!(
                            provider = %name,
                            attempt = attempt,
                            model_used = %response.model_used,
                            dims = response.vector.len(),
                            "embedding succeeded"
                        );
                        return Ok(response);
                    }
                    Err(ProviderError::UnsupportedModel(message)) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %message,
                            "provider does not support embedding request"
                        );
                        break;
                    }
                    Err(ProviderError::RateLimited { retry_after_ms }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            retry_after_ms = retry_after_ms,
                            "rate limited, moving to next provider"
                        );
                        break;
                    }
                    Err(e) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            error = %e,
                            "provider embedding attempt failed"
                        );
                    }
                }
            }
        }

        Err(ProviderError::AllExhausted {
            attempts: total_attempts,
        })
    }

    /// Return models from every registered provider, optionally filtered.
    #[must_use]
    pub fn list_models(&self, modality: &str, provider: &str) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .filter(|(name, _)| Self::provider_matches(name, provider))
            .flat_map(|(_, provider)| provider.list_models_dyn())
            .filter(|model| modality.is_empty() || model.modality == modality)
            .collect()
    }
}
