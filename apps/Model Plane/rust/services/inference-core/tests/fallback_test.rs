//! Unit tests for the fallback chain: deterministic provider ordering and retry behavior.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

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
    }
}

fn sample_embedding_request(provider_hint: &str) -> EmbedRequest {
    EmbedRequest {
        request_id: "embed-req".to_owned(),
        provider_hint: provider_hint.to_owned(),
        text: "hello".to_owned(),
        model: "embedding-model".to_owned(),
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
