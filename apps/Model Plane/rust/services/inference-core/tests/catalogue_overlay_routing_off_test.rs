//! The platform catalogue overlay's switch-off path: with `INFERENCE_CATALOGUE_OVERLAY` unset the stored overlay is
//! ignored, and the routes the caller does not name serve what they served before the overlay existed.
//!
//! This binary clears the switch before its first chain call, because the switch is read once per process. The
//! switch-on path is pinned in `catalogue_overlay_routing_test.rs`.

use std::sync::{Arc, Mutex, Once};

use inference_core::provider::catalogue_overlay::CatalogueOverlay;
use inference_core::provider::fallback::{FallbackChain, ProviderRouterDyn};
use inference_core::provider::role::{RoleCall, ROLE_MODEL_ID};
use inference_core::provider::routing_policy::{
    ComplexityModels, ModeTable, RoleEntry, RolePurpose, RoutingPolicy,
};
use inference_core::provider::{
    EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelFamily, ModelInfo,
    ProviderCapabilities, ProviderError,
};
use tokio::sync::mpsc;

const OPENAI: &[&str] = &["gpt-5-nano", "gpt-5-mini", "gpt-4o-mini"];

type Seen = Arc<Mutex<Vec<String>>>;

fn switch_off() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| std::env::remove_var("INFERENCE_CATALOGUE_OVERLAY"));
}

/// An OpenAI-family provider with an open catalogue that records each model it is asked for.
struct Recording {
    seen: Seen,
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
        OPENAI
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
            model_family: ModelFamily::OpenAiCompatible,
            ..ProviderCapabilities::default()
        }
    }
}

/// A chain over one provider, with `policy` stored as the live routing policy.
fn chain_with(seen: &Seen, policy: RoutingPolicy) -> FallbackChain {
    switch_off();
    let provider: Arc<dyn ProviderRouterDyn> = Arc::new(Recording {
        seen: Arc::clone(seen),
    });
    let chain = FallbackChain::new_with_providers(vec![("openai".to_owned(), provider)], 1);
    chain.policy_handle().store(Arc::new(policy));
    chain
}

/// The intent layer on, `verevon-balance` resolving to `gpt-5-mini`, with the overlay hiding `hidden`.
fn policy(hidden: &[&str]) -> RoutingPolicy {
    let row = |model: &str| ComplexityModels {
        simple: model.to_owned(),
        moderate: model.to_owned(),
        complex: model.to_owned(),
    };
    RoutingPolicy {
        enabled: true,
        cheap_fallback: "gpt-4o-mini".to_owned(),
        tool_fallback_ladder: vec!["gpt-4o-mini".to_owned()],
        table: ModeTable {
            budget: row("gpt-5-nano"),
            balance: row("gpt-5-mini"),
            genius: row("gpt-5-mini"),
        },
        catalogue_overlay: CatalogueOverlay {
            hidden: hidden.iter().map(|id| (*id).to_owned()).collect(),
            ..CatalogueOverlay::default()
        },
        ..RoutingPolicy::default()
    }
}

fn request(model: &str) -> InferRequest {
    InferRequest {
        request_id: format!("routing-off-{model}"),
        org_id: "org-1".to_owned(),
        user_id: "user-1".to_owned(),
        model: model.to_owned(),
        ..Default::default()
    }
}

fn calls(seen: &Seen) -> Vec<String> {
    seen.lock().unwrap().clone()
}

/// With the switch off a hidden intent cell is served as it was: the overlay is dark.
#[tokio::test]
async fn with_the_switch_off_a_hidden_intent_cell_is_served_as_before() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let chain = chain_with(&seen, policy(&["gpt-5-mini"]));
    let response = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("served");
    assert_eq!(response.model_used, "gpt-5-mini");
    assert_eq!(calls(&seen), ["gpt-5-mini"]);
}

/// With the switch off an automatic call is served the provider default even when the overlay hides it, as before:
/// the default is not checked against the overlay on either path.
#[tokio::test]
async fn with_the_switch_off_an_automatic_call_is_served_a_hidden_default_as_before() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let chain = chain_with(&seen, policy(&["gpt-4o-mini"]));
    let response = chain.infer(&request("")).await.expect("served");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(calls(&seen), ["gpt-4o-mini"]);
    let _stream = chain
        .infer_stream(&request(""))
        .await
        .expect("the stream is opened as before");
    assert_eq!(calls(&seen), ["gpt-4o-mini", "gpt-4o-mini"]);
}

/// With the switch off a `verevon-*` call with the intent layer off is served the provider default even when the
/// overlay hides it, as before.
#[tokio::test]
async fn with_the_switch_off_a_verevon_call_is_served_a_hidden_default_as_before() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let mut policy = policy(&["gpt-4o-mini"]);
    policy.enabled = false;
    let chain = chain_with(&seen, policy);
    let response = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("served");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(calls(&seen), ["gpt-4o-mini"]);
}

/// With the switch off a hidden role candidate is chosen as it was: the role table does not read the overlay.
#[tokio::test]
async fn with_the_switch_off_a_hidden_role_candidate_is_chosen_as_before() {
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let mut policy = policy(&["gpt-5-nano"]);
    policy.roles.insert(
        RolePurpose::Title,
        RoleEntry {
            candidates: vec!["gpt-5-nano".to_owned(), "gpt-4o-mini".to_owned()],
            effort: None,
        },
    );
    let chain = chain_with(&seen, policy);
    let (response, _) = chain
        .infer_role_noted(
            &request(ROLE_MODEL_ID),
            &RoleCall {
                purpose: Some(RolePurpose::Title),
                fallback_model: String::new(),
            },
            false,
        )
        .await
        .expect("served");
    assert_eq!(response.model_used, "gpt-5-nano");
    assert_eq!(calls(&seen), ["gpt-5-nano"]);
}
