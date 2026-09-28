//! Unit tests for the fallback chain: deterministic provider ordering and retry behavior.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

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
            compaction_summary: String::new(),
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
                    compaction_summary: String::new(),
                    reasoning_delta: String::new(),
                    request_id,
                    delta: format!("stream from {name}"),
                    done: true,
                    model_used: model,
                    input_tokens: 0,
                    output_tokens: 0,
                    stop_reason: "end_turn".to_owned(),
                    provider_used: String::new(),
                    residency: String::new(),
                    token_confidence: None,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                    provider_timing: None,
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

/// A mock provider whose calls take a fixed time: `delay` before a unary
/// answer (or failure), and for streams a delta at `delay` and the final
/// chunk at `2 * delay`. Tests run it on tokio's paused clock, so every
/// duration below is exact.
struct TimedProvider {
    delay: Duration,
    /// Every call fails with an HTTP error after `delay`.
    fail: bool,
    /// Rate-limit this many calls before answering.
    rate_limit_first: AtomicU32,
    /// Token counts to report; `(0, 0)` reports no usage, like the
    /// subscription broker.
    tokens: (i32, i32),
}

impl TimedProvider {
    fn ok(delay_ms: u64) -> Self {
        Self {
            delay: Duration::from_millis(delay_ms),
            fail: false,
            rate_limit_first: AtomicU32::new(0),
            tokens: (10, 5),
        }
    }

    fn failing(delay_ms: u64) -> Self {
        Self {
            fail: true,
            ..Self::ok(delay_ms)
        }
    }
}

#[async_trait::async_trait]
impl ProviderRouterDyn for TimedProvider {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        if self
            .rate_limit_first
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(ProviderError::RateLimited {
                retry_after_ms: 1_000,
            });
        }
        tokio::time::sleep(self.delay).await;
        if self.fail {
            return Err(ProviderError::Http("upstream reset".to_owned()));
        }
        Ok(InferResponse {
            request_id: req.request_id.clone(),
            content: "timed".to_owned(),
            model_used: req.model.clone(),
            stop_reason: "end_turn".to_owned(),
            input_tokens: self.tokens.0,
            output_tokens: self.tokens.1,
            ..Default::default()
        })
    }

    async fn infer_stream_dyn(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        let (tx, rx) = mpsc::channel(4);
        let delay = self.delay;
        let (input_tokens, output_tokens) = self.tokens;
        let chunk = move |request_id: &str, delta: &str, done: bool| InferChunk {
            compaction_summary: String::new(),
            request_id: request_id.to_owned(),
            delta: delta.to_owned(),
            done,
            model_used: String::new(),
            input_tokens: if done { input_tokens } else { 0 },
            output_tokens: if done { output_tokens } else { 0 },
            stop_reason: if done {
                "end_turn".to_owned()
            } else {
                String::new()
            },
            reasoning_delta: String::new(),
            provider_used: String::new(),
            residency: String::new(),
            token_confidence: None,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            provider_timing: None,
        };
        let request_id = req.request_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(chunk(&request_id, "hei", false)).await;
            tokio::time::sleep(delay).await;
            let _ = tx.send(chunk(&request_id, "", true)).await;
        });
        Ok(rx)
    }

    async fn create_embedding_dyn(
        &self,
        _req: &EmbedRequest,
    ) -> Result<EmbedResponse, ProviderError> {
        Err(ProviderError::UnsupportedModel("chat only".to_owned()))
    }

    fn list_models_dyn(&self) -> Vec<ModelInfo> {
        Vec::new()
    }
}

fn timed_chain(providers: Vec<(&str, TimedProvider)>) -> FallbackChain {
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = providers
        .into_iter()
        .map(|(name, provider)| {
            (
                name.to_owned(),
                Arc::new(provider) as Arc<dyn ProviderRouterDyn>,
            )
        })
        .collect();
    FallbackChain::new_with_providers(providers, 1)
}

fn sample_request() -> InferRequest {
    InferRequest {
        request_id: "test-req".to_owned(),
        provider_hint: String::new(),
        model: "test-model".to_owned(),
        messages: vec![ChatMessage {
            compaction_summary: String::new(),
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

#[tokio::test(start_paused = true)]
async fn unary_timing_covers_the_provider_call() {
    let chain = timed_chain(vec![("primary", TimedProvider::ok(30))]);

    let response = chain.infer(&sample_request()).await.expect("served");

    let timing = response
        .provider_timing
        .expect("a served call carries its timing");
    assert!(timing.provider >= Duration::from_millis(30), "{timing:?}");
    assert_eq!(
        timing.provider,
        Duration::from_millis(30),
        "the paused clock makes it exact"
    );
    assert_eq!(timing.attempts, 1);
    assert_eq!(timing.failed_attempts, Duration::ZERO);
    assert_eq!(timing.retry_wait, Duration::ZERO);
    assert_eq!(
        timing.routing,
        Duration::ZERO,
        "no time passes before the attempt"
    );
    assert!(!timing.response_cache_hit);
    assert!(timing.usage_reported);
    assert_eq!(timing.first_token, None, "unary calls have no first token");
}

#[tokio::test(start_paused = true)]
async fn a_failed_first_provider_is_charged_separately_from_the_serving_one() {
    let chain = timed_chain(vec![
        ("primary", TimedProvider::failing(5)),
        ("secondary", TimedProvider::ok(30)),
    ]);

    let response = chain
        .infer(&sample_request())
        .await
        .expect("secondary serves");

    let timing = response.provider_timing.expect("timing");
    assert_eq!(timing.attempts, 2);
    assert!(timing.failed_attempts > Duration::ZERO, "{timing:?}");
    assert_eq!(timing.failed_attempts, Duration::from_millis(5));
    assert_eq!(
        timing.provider,
        Duration::from_millis(30),
        "only the serving call"
    );
    assert_eq!(
        timing.routing,
        Duration::ZERO,
        "routing ends at the FIRST attempt"
    );
}

#[tokio::test(start_paused = true)]
async fn a_retry_after_wait_is_recorded_as_retry_wait() {
    let provider = TimedProvider::ok(10);
    provider.rate_limit_first.store(1, Ordering::SeqCst);
    let chain = timed_chain(vec![("primary", provider)]);

    let response = chain
        .infer(&sample_request())
        .await
        .expect("served after the wait");

    let timing = response.provider_timing.expect("timing");
    assert_eq!(
        timing.retry_wait,
        Duration::from_secs(1),
        "the provider's retry-after"
    );
    assert_eq!(timing.attempts, 2, "the throttled call and the serving one");
    assert_eq!(
        timing.failed_attempts,
        Duration::ZERO,
        "the 429 came back at once"
    );
    assert_eq!(timing.provider, Duration::from_millis(10));
}

#[tokio::test(start_paused = true)]
async fn a_response_cache_hit_reports_no_provider_time() {
    let chain = timed_chain(vec![("primary", TimedProvider::ok(30))]);
    let first = chain.infer(&sample_request()).await.expect("served");
    assert!(!first.provider_timing.expect("timing").response_cache_hit);

    let cached = chain.infer(&sample_request()).await.expect("cached");

    let timing = cached
        .provider_timing
        .expect("a cache hit still carries timing");
    assert!(timing.response_cache_hit);
    assert_eq!(timing.attempts, 0, "not the stored copy's attempt count");
    assert_eq!(timing.provider, Duration::ZERO);
    assert!(
        timing.usage_reported,
        "the cached response keeps its reported counts"
    );
}

#[tokio::test(start_paused = true)]
async fn stream_timing_marks_the_first_token_and_the_final_chunk() {
    let mut provider = TimedProvider::ok(20);
    provider.tokens = (0, 0);
    let chain = timed_chain(vec![("primary", provider)]);

    let mut rx = chain
        .infer_stream(&sample_request())
        .await
        .expect("stream opens");
    let mut chunks = Vec::new();
    while let Some(chunk) = rx.recv().await {
        let done = chunk.done;
        chunks.push(chunk);
        if done {
            break;
        }
    }

    assert_eq!(chunks.len(), 2);
    assert!(
        chunks[0].provider_timing.is_none(),
        "deltas carry no timing"
    );
    let timing = chunks[1]
        .provider_timing
        .clone()
        .expect("the final chunk carries timing");
    assert_eq!(timing.accept, Duration::ZERO, "the mock accepts at once");
    assert_eq!(timing.first_token, Some(Duration::from_millis(20)));
    assert_eq!(timing.provider, Duration::from_millis(40));
    assert_eq!(timing.attempts, 1);
    assert!(
        !timing.usage_reported,
        "no token counts means unknown usage"
    );
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
        codex_subscription_integration_core_url: None,
        codex_subscription_internal_api_key: None,
        codex_subscription_models: vec![],
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
        azure_cohere_endpoint: None,
        azure_cohere_api_key: None,
        azure_cohere_api_version: "2024-05-01-preview".to_owned(),
        azure_cohere_deployment: "cohere-command-a-plus".to_owned(),
        azure_cohere_region: None,
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
        codex_subscription_integration_core_url: None,
        codex_subscription_internal_api_key: None,
        codex_subscription_models: vec![],
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
        azure_cohere_endpoint: None,
        azure_cohere_api_key: None,
        azure_cohere_api_version: "2024-05-01-preview".to_owned(),
        azure_cohere_deployment: "cohere-command-a-plus".to_owned(),
        azure_cohere_region: None,
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

#[test]
fn cohere_registers_alongside_azure_openai_without_stealing_its_traffic() {
    // "cohere"/"azure-cohere" is Command A Plus (the live successor to the
    // retired "Command R+" name) fronted through Azure AI Foundry's unified
    // inference route. It must coexist with azure-openai — same
    // ModelFamily::OpenAiCompatible family — without either one absorbing the
    // other's models. See `provider_serves_model`'s exclusive_catalog gate.
    let cfg = InferenceConfig {
        provider_order: vec!["azure".to_owned(), "cohere".to_owned()],
        anthropic_api_key: None,
        openai_api_base: None,
        openai_api_key: None,
        codex_subscription_integration_core_url: None,
        codex_subscription_internal_api_key: None,
        codex_subscription_models: vec![],
        azure_openai_endpoint: Some("https://example.openai.azure.com".to_owned()),
        azure_openai_api_key: Some("test-key".to_owned()),
        azure_openai_api_version: "2025-01-01-preview".to_owned(),
        openai_chat_models: vec![],
        openai_embedding_models: vec![],
        azure_openai_chat_deployments: vec!["gpt-4o-mini".to_owned()],
        azure_openai_embedding_deployments: vec![],
        azure_anthropic_endpoint: None,
        azure_anthropic_api_key: None,
        azure_anthropic_deployments: vec![],
        azure_cohere_endpoint: Some("https://core-ai-rg.services.ai.azure.com/models".to_owned()),
        azure_cohere_api_key: Some("cohere-key".to_owned()),
        azure_cohere_api_version: "2024-05-01-preview".to_owned(),
        azure_cohere_deployment: "cohere-command-a-plus".to_owned(),
        azure_cohere_region: None,
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
        allow_global_residency_providers: true,
        allow_non_eu_embedding: false,
    };

    let chain = FallbackChain::from_config(&cfg);
    assert_eq!(chain.provider_count(), 2);

    let cohere_models = chain.list_models("", "cohere");
    assert_eq!(cohere_models.len(), 1);
    assert_eq!(cohere_models[0].id, "cohere-command-a-plus");

    // Azure OpenAI keeps serving its own catalog untouched.
    let azure_models = chain.list_models("", "azure-openai");
    assert_eq!(azure_models.len(), 1);
    assert_eq!(azure_models[0].id, "gpt-4o-mini");
}
