//! A model the platform catalogue overlay hides is refused on infer, not only left
//! out of the listing (GAP-W4-BYO-hidden-infer, W4-BYO-1).
//!
//! This binary runs with `INFERENCE_CATALOGUE_OVERLAY=true`: the switch is read once
//! per process, so every test sets it before the first chain call. Only the providers
//! are doubles; the chain and its admission are the real ones.

use std::sync::{Arc, Mutex, Once};

use inference_core::provider::catalogue_overlay::CatalogueOverlay;
use inference_core::provider::fallback::{FallbackChain, ProviderRouterDyn};
use inference_core::provider::routing_policy::RoutingPolicy;
use inference_core::provider::{
    EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelFamily, ModelInfo,
    ProviderCapabilities, ProviderError,
};
use tokio::sync::mpsc;

fn switch_on() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::set_var("INFERENCE_CATALOGUE_OVERLAY", "true"));
}

/// A provider that records every model it was asked for.
struct Recording {
    family: ModelFamily,
    models: Vec<&'static str>,
    seen: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl ProviderRouterDyn for Recording {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        self.seen.lock().unwrap().push(req.model.clone());
        Ok(InferResponse {
            request_id: req.request_id.clone(),
            content: "answer".to_owned(),
            model_used: req.model.clone(),
            stop_reason: "end_turn".to_owned(),
            ..Default::default()
        })
    }

    async fn infer_stream_dyn(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        self.seen.lock().unwrap().push(req.model.clone());
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn create_embedding_dyn(&self, _: &EmbedRequest) -> Result<EmbedResponse, ProviderError> {
        Err(ProviderError::UnsupportedModel("embedding".to_owned()))
    }

    fn list_models_dyn(&self) -> Vec<ModelInfo> {
        self.models
            .iter()
            .map(|id| ModelInfo {
                id: (*id).to_owned(),
                modality: "chat".to_owned(),
                streaming: true,
                ..Default::default()
            })
            .collect()
    }

    fn capabilities_dyn(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: "anthropic".to_owned(),
            model_family: self.family,
            ..ProviderCapabilities::default()
        }
    }
}

fn chain_hiding(
    hidden: &[&str],
    aliases: &[(&str, &str)],
) -> (FallbackChain, Arc<Mutex<Vec<String>>>) {
    switch_on();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider: Arc<dyn ProviderRouterDyn> = Arc::new(Recording {
        family: ModelFamily::Anthropic,
        models: vec!["claude-sonnet-5-5", "claude-opus-5-5", "claude-haiku-5-5"],
        seen: Arc::clone(&seen),
    });
    let chain = FallbackChain::new_with_providers(vec![("anthropic".to_owned(), provider)], 1);
    let mut policy = RoutingPolicy {
        enabled: false,
        ..RoutingPolicy::default()
    };
    policy.catalogue_overlay = CatalogueOverlay {
        hidden: hidden.iter().map(|id| (*id).to_owned()).collect(),
        order: Vec::new(),
        aliases: aliases
            .iter()
            .map(|(alias, target)| ((*alias).to_owned(), (*target).to_owned()))
            .collect(),
    };
    chain.policy_handle().store(Arc::new(policy));
    (chain, seen)
}

fn request(model: &str) -> InferRequest {
    InferRequest {
        request_id: format!("hidden-{model}"),
        org_id: "org-1".to_owned(),
        user_id: "user-1".to_owned(),
        model: model.to_owned(),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_hidden_model_named_on_infer_is_refused_before_any_provider() {
    let (chain, seen) = chain_hiding(&["claude-opus-5-5"], &[]);
    let error = chain.infer(&request("claude-opus-5-5")).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    // Case does not let it through.
    let error = chain.infer(&request("Claude-Opus-5-5")).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    let error = chain
        .infer_stream(&request("claude-opus-5-5"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "stream: {error:?}"
    );
    assert!(seen.lock().unwrap().is_empty(), "no provider was called");
}

#[tokio::test]
async fn a_visible_model_is_still_served() {
    let (chain, seen) = chain_hiding(&["claude-opus-5-5"], &[]);
    chain
        .infer(&request("claude-sonnet-5-5"))
        .await
        .expect("served");
    assert_eq!(seen.lock().unwrap().as_slice(), ["claude-sonnet-5-5"]);
}

#[tokio::test]
async fn an_alias_that_targets_a_hidden_model_is_refused() {
    let (chain, seen) = chain_hiding(&["claude-opus-5-5"], &[("team-best", "claude-opus-5-5")]);
    let error = chain.infer(&request("team-best")).await.unwrap_err();
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn an_alias_to_a_visible_model_serves_its_target() {
    let (chain, seen) = chain_hiding(&[], &[("team-fast", "claude-haiku-5-5")]);
    chain.infer(&request("team-fast")).await.expect("served");
    assert_eq!(seen.lock().unwrap().as_slice(), ["claude-haiku-5-5"]);
}

/// A retired Claude id is answered by its successor. When the overlay hides the
/// successor, naming the retired id must not reach it: the check is by the id that
/// would really be served.
#[tokio::test]
async fn a_retired_id_whose_successor_is_hidden_is_refused() {
    let (chain, seen) = chain_hiding(&["claude-sonnet-5-5"], &[]);
    let outcome = chain.infer(&request("claude-sonnet-4-20250514")).await;
    assert!(
        matches!(outcome, Err(ProviderError::ModelHidden { .. })),
        "the hidden successor was served: {outcome:?}, provider saw {:?}",
        seen.lock().unwrap()
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the hidden model was called"
    );
    // The stream path admits the same way, before any provider is called.
    let error = chain
        .infer_stream(&request("claude-sonnet-4-20250514"))
        .await
        .unwrap_err();
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "stream: {error:?}"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "the hidden model was called on the stream path"
    );
}

/// A retired Claude id whose successor the overlay leaves visible is still answered by that
/// successor, so the check above refuses only what the overlay hides.
#[tokio::test]
async fn a_retired_id_whose_successor_is_visible_is_served_as_it() {
    let (chain, seen) = chain_hiding(&["claude-opus-5-5"], &[]);
    chain
        .infer(&request("claude-sonnet-4-20250514"))
        .await
        .expect("served");
    assert_eq!(seen.lock().unwrap().as_slice(), ["claude-sonnet-5-5"]);
}
