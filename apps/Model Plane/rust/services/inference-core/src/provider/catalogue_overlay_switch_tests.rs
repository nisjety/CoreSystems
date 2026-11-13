//! The two overlay switches (`INFERENCE_CATALOGUE_OVERLAY`, `INFERENCE_ORG_OVERLAY`):
//! only the literal `true` turns them on, and with the platform switch off a hidden
//! model is served exactly as before. The lib test process never sets either switch.

use std::sync::{Arc, Mutex};

use super::*;
use crate::provider::fallback::{FallbackChain, ProviderRouterDyn};
use crate::provider::routing_policy::RoutingPolicy;
use crate::provider::{
    EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelFamily, ModelInfo,
    ProviderCapabilities, ProviderError,
};

#[test]
fn both_overlay_switches_are_on_only_for_the_literal_true() {
    assert!(catalogue_overlay_enabled_from(Some("true")));
    assert!(org_overlay_enabled_from(Some("true")));
    for raw in [
        None,
        Some(""),
        Some("1"),
        Some("TRUE"),
        Some("True"),
        Some("on"),
        Some("false"),
    ] {
        assert!(!catalogue_overlay_enabled_from(raw), "{raw:?}");
        assert!(!org_overlay_enabled_from(raw), "{raw:?}");
    }
}

struct Recording {
    seen: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl ProviderRouterDyn for Recording {
    async fn infer_dyn(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        self.seen.lock().unwrap().push(req.model.clone());
        Ok(InferResponse {
            request_id: req.request_id.clone(),
            model_used: req.model.clone(),
            ..Default::default()
        })
    }

    async fn infer_stream_dyn(
        &self,
        _req: &InferRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<InferChunk>, ProviderError> {
        Err(ProviderError::Unavailable("stub".to_owned()))
    }

    async fn create_embedding_dyn(&self, _: &EmbedRequest) -> Result<EmbedResponse, ProviderError> {
        Err(ProviderError::UnsupportedModel("embedding".to_owned()))
    }

    fn list_models_dyn(&self) -> Vec<ModelInfo> {
        vec![ModelInfo {
            id: "claude-opus-5-5".to_owned(),
            modality: "chat".to_owned(),
            ..Default::default()
        }]
    }

    fn capabilities_dyn(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            model_family: ModelFamily::Anthropic,
            ..ProviderCapabilities::default()
        }
    }
}

#[tokio::test]
async fn with_the_switch_off_a_hidden_model_and_an_alias_change_nothing() {
    assert!(
        !catalogue_overlay_enabled(),
        "the lib test process never sets INFERENCE_CATALOGUE_OVERLAY"
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    let provider: Arc<dyn ProviderRouterDyn> = Arc::new(Recording { seen: seen.clone() });
    let chain = FallbackChain::new_with_providers(vec![("anthropic".to_owned(), provider)], 1);
    let mut policy = RoutingPolicy {
        enabled: false,
        ..RoutingPolicy::default()
    };
    policy
        .catalogue_overlay
        .hidden
        .insert("claude-opus-5-5".to_owned());
    policy
        .catalogue_overlay
        .aliases
        .insert("team-best".to_owned(), "claude-opus-5-5".to_owned());
    chain.policy_handle().store(Arc::new(policy));

    let request = InferRequest {
        request_id: "off-1".to_owned(),
        model: "claude-opus-5-5".to_owned(),
        ..Default::default()
    };
    chain.infer(&request).await.expect("served as before");
    // An alias is not resolved with the switch off: it reaches the chain as the id the caller sent.
    let alias = InferRequest {
        request_id: "off-2".to_owned(),
        model: "team-best".to_owned(),
        ..Default::default()
    };
    let _ = chain.infer(&alias).await;
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.first().map(String::as_str), Some("claude-opus-5-5"));
    assert!(
        !seen.iter().skip(1).any(|model| model == "claude-opus-5-5"),
        "{seen:?}"
    );
}
