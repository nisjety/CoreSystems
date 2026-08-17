//! Unit tests for the fallback chain: deterministic provider ordering and retry behavior.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use inference_core::config::InferenceConfig;
use inference_core::provider::fallback::{FallbackChain, ProviderRouterDyn};
use inference_core::provider::{
    ChatMessage, EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelInfo,
    ProviderError,
};
use tokio::sync::mpsc;

/// A mock provider that fails a configurable number of times before succeeding.
struct MockProvider {
    name: String,
    fail_count: AtomicU32,
    fail_until: u32,
}

impl MockProvider {
    fn new(name: &str, fail_until: u32) -> Self {
        Self {
            name: name.to_owned(),
            fail_count: AtomicU32::new(0),
            fail_until,
        }
    }
}

#[async_trait::async_trait]
impl ProviderRouterDyn for MockProvider {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        if count < self.fail_until {
            return Err(ProviderError::Http(format!(
                "{} attempt {} failed",
                self.name, count
            )));
        }
        Ok(InferResponse {
            request_id: req.request_id.clone(),
            content: format!("from {}", self.name),
            model_used: req.model.clone(),
            stop_reason: "end_turn".to_owned(),
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        })
    }

    async fn infer_stream_dyn(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let (tx, rx) = mpsc::channel(1);
        let request_id = req.request_id.clone();
        let model = req.model.clone();
        let name = self.name.clone();
        tokio::spawn(async move {
            let _ = tx
                .send(InferChunk {
                    request_id,
                    delta: format!("stream from {name}"),
                    done: true,
                    model_used: model,
                    input_tokens: 0,
                    output_tokens: 0,
                })
                .await;
        });
        Ok(rx)
    }

    async fn create_embedding_dyn(
        &self,
        req: &EmbedRequest,
    ) -> Result<EmbedResponse, ProviderError> {
        Ok(EmbedResponse {
            request_id: req.request_id.clone(),
            vector: vec![0.1, 0.2],
            model_used: req.model.clone(),
            provider_used: self.name.clone(),
        })
    }

    fn list_models_dyn(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: format!("{}-embedding", self.name),
            provider: self.name.clone(),
            modality: "embedding".to_owned(),
            streaming: false,
            ..Default::default()
        }]
    }
}

fn sample_request() -> InferRequest {
    InferRequest {
        request_id: "test-req".to_owned(),
        provider_hint: String::new(),
        model: "test-model".to_owned(),
        messages: vec![ChatMessage {
            role: "user".to_owned(),
            content: "hello".to_owned(),
            name: String::new(),
        }],
        temperature: 0.5,
        max_tokens: 100,
        structured_output_schema: None,
        zdr: false,
        ..Default::default()
    }
}

fn sample_embedding_request(provider_hint: &str) -> EmbedRequest {
    EmbedRequest {
        request_id: "embed-req".to_owned(),
        provider_hint: provider_hint.to_owned(),
        text: "hello".to_owned(),
        model: "embedding-model".to_owned(),
        zdr: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn first_provider_succeeds() {
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = vec![(
        "primary".to_owned(),
        Arc::new(MockProvider::new("primary", 0)),
    )];
    let chain = FallbackChain::new_with_providers(providers, 3);

    let result = chain.infer(&sample_request()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().content, "from primary");
}

#[tokio::test]
async fn falls_back_to_second_provider() {
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = vec![
        (
            "primary".to_owned(),
            Arc::new(MockProvider::new("primary", 100)),
        ), // always fails
        (
            "secondary".to_owned(),
            Arc::new(MockProvider::new("secondary", 0)),
        ),
    ];
    let chain = FallbackChain::new_with_providers(providers, 2);

    let result = chain.infer(&sample_request()).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().content, "from secondary");
}

#[tokio::test]
async fn all_providers_exhausted() {
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = vec![
        (
            "primary".to_owned(),
            Arc::new(MockProvider::new("primary", 100)),
        ),
        (
            "secondary".to_owned(),
            Arc::new(MockProvider::new("secondary", 100)),
        ),
    ];
    let chain = FallbackChain::new_with_providers(providers, 2);

    let result = chain.infer(&sample_request()).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ProviderError::AllExhausted { attempts } => {
            assert_eq!(attempts, 4); // 2 providers x 2 retries
        }
        other => panic!("expected AllExhausted, got {other:?}"),
    }
}

#[tokio::test]
async fn retries_within_provider_before_fallback() {
    // Primary fails once, succeeds on second attempt
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = vec![
        (
            "primary".to_owned(),
            Arc::new(MockProvider::new("primary", 1)),
        ),
        (
            "secondary".to_owned(),
            Arc::new(MockProvider::new("secondary", 0)),
        ),
    ];
    let chain = FallbackChain::new_with_providers(providers, 3);

    let result = chain.infer(&sample_request()).await;
    assert!(result.is_ok());
    // Should succeed on primary's second attempt, never reaching secondary
    assert_eq!(result.unwrap().content, "from primary");
}

#[tokio::test]
async fn provider_hint_selects_matching_provider_for_embeddings() {
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = vec![
        (
            "primary".to_owned(),
            Arc::new(MockProvider::new("primary", 0)),
        ),
        (
            "secondary".to_owned(),
            Arc::new(MockProvider::new("secondary", 0)),
        ),
    ];
    let chain = FallbackChain::new_with_providers(providers, 3);

    let result = chain
        .create_embedding(&sample_embedding_request("secondary"))
        .await
        .expect("embedding");

    assert_eq!(result.provider_used, "secondary");
    assert_eq!(result.vector, vec![0.1, 0.2]);
}

#[test]
fn list_models_filters_by_modality_and_provider() {
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = vec![
        (
            "primary".to_owned(),
            Arc::new(MockProvider::new("primary", 0)),
        ),
        (
            "secondary".to_owned(),
            Arc::new(MockProvider::new("secondary", 0)),
        ),
    ];
    let chain = FallbackChain::new_with_providers(providers, 3);

    let models = chain.list_models("embedding", "primary");

    assert_eq!(models.len(), 1);
    assert_eq!(models[0].provider, "primary");
}

#[test]
fn provider_order_accepts_azure_alias_without_duplicate_openai_fallback() {
    let cfg = InferenceConfig {
        provider_order: vec![
            "azure".to_owned(),
            "anthropic".to_owned(),
            "openai".to_owned(),
        ],
        anthropic_api_key: None,
        openai_api_base: None,
        openai_api_key: None,
        azure_openai_endpoint: Some("https://example.openai.azure.com".to_owned()),
        azure_openai_api_key: Some("test-key".to_owned()),
        azure_openai_api_version: "2025-01-01-preview".to_owned(),
        openai_chat_models: vec!["gpt-test".to_owned()],
        openai_embedding_models: vec!["embed-test".to_owned()],
        azure_openai_chat_deployments: vec!["azure-chat".to_owned()],
        azure_openai_embedding_deployments: vec!["azure-embed".to_owned()],
        azure_anthropic_endpoint: None,
        azure_anthropic_api_key: None,
        azure_anthropic_deployments: vec![],
        max_retries_per_provider: 1,
        cache_ttl_secs: 60,
        verevon_intent_enabled: false,
        cost_core_url: None,
        verevon_intent_budget_usd: 50.0,
        session_core_url: None,
        router_policy_refresh_secs: 60,
        azure_openai_region: None,
        azure_anthropic_region: None,
        azure_openai_zdr: None,
        azure_anthropic_zdr: None,
        azure_openai_deployment_type: None,
        allow_global_deployment: false,
        // These fixtures configure no region, so the Azure provider classifies as
        // Global. The residency gate has its own tests; opt in here so these keep
        // testing model routing rather than dying on the gate.
        allow_global_residency_providers: true,
        allow_non_eu_embedding: false,
    };

    let chain = FallbackChain::from_config(&cfg);

    assert_eq!(chain.provider_count(), 1);
    assert_eq!(chain.list_models("", "azure-openai")[0].id, "azure-chat");
}

#[test]
fn azure_anthropic_registers_and_advertises_claude_catalog() {
    // The `anthropic` slot resolves to the Azure Foundry Claude resource when
    // AZURE_ANTHROPIC_* is configured (the direct key is out of credit).
    let cfg = InferenceConfig {
        provider_order: vec!["azure".to_owned(), "anthropic".to_owned()],
        anthropic_api_key: Some("direct-but-broke".to_owned()),
        openai_api_base: None,
        openai_api_key: None,
        azure_openai_endpoint: Some("https://example.openai.azure.com".to_owned()),
        azure_openai_api_key: Some("test-key".to_owned()),
        azure_openai_api_version: "2025-01-01-preview".to_owned(),
        openai_chat_models: vec![],
        openai_embedding_models: vec![],
        azure_openai_chat_deployments: vec!["model-router".to_owned(), "gpt-4o-mini".to_owned()],
        azure_openai_embedding_deployments: vec![],
        azure_anthropic_endpoint: Some(
            "https://cloude-ai-resource.services.ai.azure.com".to_owned(),
        ),
        azure_anthropic_api_key: Some("azure-claude-key".to_owned()),
        azure_anthropic_deployments: vec![
            "claude-haiku-4-5".to_owned(),
            "claude-opus-4-8".to_owned(),
        ],
        max_retries_per_provider: 1,
        cache_ttl_secs: 60,
        verevon_intent_enabled: false,
        cost_core_url: None,
        verevon_intent_budget_usd: 50.0,
        session_core_url: None,
        router_policy_refresh_secs: 60,
        azure_openai_region: None,
        azure_anthropic_region: None,
        azure_openai_zdr: None,
        azure_anthropic_zdr: None,
        azure_openai_deployment_type: None,
        allow_global_deployment: false,
        // These fixtures configure no region, so the Azure provider classifies as
        // Global. The residency gate has its own tests; opt in here so these keep
        // testing model routing rather than dying on the gate.
        allow_global_residency_providers: true,
        allow_non_eu_embedding: false,
    };

    let chain = FallbackChain::from_config(&cfg);

    // azure-openai + azure-anthropic (direct anthropic is NOT registered because
    // the Azure flavor took the slot).
    assert_eq!(chain.provider_count(), 2);

    // The Claude catalog is advertised under the azure-anthropic provider, with
    // haiku flagged cheap (carried in features as "cheap" at the gRPC edge).
    let claude = chain.list_models("", "azure-anthropic");
    assert_eq!(claude.len(), 2);
    assert!(claude.iter().all(|m| m.provider == "azure-anthropic"));
    let haiku = claude.iter().find(|m| m.id == "claude-haiku-4-5").unwrap();
    assert!(haiku.cheap);

    // model-router is the azure-openai default and is flagged cheap.
    let chat = chain.list_models("", "azure-openai");
    let router = chat.iter().find(|m| m.id == "model-router").unwrap();
    assert!(router.cheap);
}
