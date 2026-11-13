//! The platform catalogue overlay on the routes the caller does not name (the sibling of V2-inf-1): the intent
//! layer (`verevon-*`), the platform role table (`verevon-role`), the tool ladder and the automatic provider default
//! never serve a model the overlay hides, and a cached answer for a model that is now hidden is not served.
//!
//! This binary runs with `INFERENCE_CATALOGUE_OVERLAY=true` and `INFERENCE_ROUTE_NOTE=true`: each switch is read once
//! per process, so every test sets them before the first chain call. The switch-off path is pinned in
//! `catalogue_overlay_routing_off_test.rs`.
//! Only the providers are doubles; the chain, its intent layer, role table and admission are the real ones.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Once};

use inference_core::provider::catalogue_overlay::CatalogueOverlay;
use inference_core::provider::fallback::{FallbackChain, HidingOverlay, ProviderRouterDyn};
use inference_core::provider::intent::VerevonMode;
use inference_core::provider::role::{RoleCall, ROLE_MODEL_ID};
use inference_core::provider::routing_policy::{
    ComplexityModels, ModeTable, RoleEntry, RolePurpose, RoutingPolicy,
};
use inference_core::provider::{
    EmbedRequest, EmbedResponse, InferChunk, InferRequest, InferResponse, ModelFamily, ModelInfo,
    ProviderCapabilities, ProviderError,
};
use tokio::sync::mpsc;

/// The models each test's provider lists. `gpt-5-mini` is the balance cell at every complexity, `gpt-4o-mini`
/// the first tool-ladder rung and the cheap fallback, and `gpt-5-nano` the last rung.
const OPENAI: &[&str] = &["gpt-5-nano", "gpt-5-mini", "gpt-4o-mini"];

/// A retired Claude id, its successor, and a model the Claude provider also lists.
const CLAUDE: &[&str] = &[
    "claude-sonnet-4-20250514",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
];

type Seen = Arc<Mutex<Vec<String>>>;

fn switch_on() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("INFERENCE_CATALOGUE_OVERLAY", "true");
        std::env::set_var("INFERENCE_ROUTE_NOTE", "true");
    });
}

/// A provider that lists `models`, serves the ids of its family, and records each model it is asked for.
struct Recording {
    family: ModelFamily,
    models: &'static [&'static str],
    exclusive: bool,
    seen: Seen,
}

impl Recording {
    /// An OpenAI-family provider with an open catalogue: it serves any id that is not a Claude or Gemini one.
    fn openai(seen: &Seen) -> Self {
        Self {
            family: ModelFamily::OpenAiCompatible,
            models: OPENAI,
            exclusive: false,
            seen: Arc::clone(seen),
        }
    }

    /// An OpenAI-family provider that serves only `models`, so any other id cannot be served by it.
    fn openai_exclusive(models: &'static [&'static str], seen: &Seen) -> Self {
        Self {
            family: ModelFamily::OpenAiCompatible,
            models,
            exclusive: true,
            seen: Arc::clone(seen),
        }
    }

    fn anthropic(seen: &Seen) -> Self {
        Self {
            family: ModelFamily::Anthropic,
            models: CLAUDE,
            exclusive: false,
            seen: Arc::clone(seen),
        }
    }
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
            model_family: self.family,
            exclusive_catalog: self.exclusive,
            ..ProviderCapabilities::default()
        }
    }
}

/// A chain over `providers`, with `policy` stored as the live routing policy.
fn chain_with(providers: Vec<(&str, Recording)>, policy: RoutingPolicy) -> FallbackChain {
    switch_on();
    let providers: Vec<(String, Arc<dyn ProviderRouterDyn>)> = providers
        .into_iter()
        .map(|(name, provider)| {
            (
                name.to_owned(),
                Arc::new(provider) as Arc<dyn ProviderRouterDyn>,
            )
        })
        .collect();
    let chain = FallbackChain::new_with_providers(providers, 1);
    chain.policy_handle().store(Arc::new(policy));
    chain
}

/// The policy the tests share. The intent layer is on: `verevon-balance` resolves to `gpt-5-mini` at every
/// complexity, its tool ladder is `gpt-4o-mini` then `gpt-5-nano`, and the cheap fallback is `gpt-4o-mini`.
/// `hidden` is the platform overlay's hidden set.
fn shared_policy(hidden: &[&str]) -> RoutingPolicy {
    let row = |model: &str| ComplexityModels {
        simple: model.to_owned(),
        moderate: model.to_owned(),
        complex: model.to_owned(),
    };
    RoutingPolicy {
        enabled: true,
        cheap_fallback: "gpt-4o-mini".to_owned(),
        tool_fallback_ladder: vec!["gpt-4o-mini".to_owned(), "gpt-5-nano".to_owned()],
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

/// [`shared_policy`] with a role table entry for `Title` naming `candidates` in order.
fn policy_with_title(hidden: &[&str], candidates: &[&str]) -> RoutingPolicy {
    let mut policy = shared_policy(hidden);
    policy.roles.insert(
        RolePurpose::Title,
        RoleEntry {
            candidates: candidates.iter().map(|id| (*id).to_owned()).collect(),
            effort: None,
        },
    );
    policy
}

fn request(model: &str) -> InferRequest {
    InferRequest {
        request_id: format!("routing-{model}"),
        org_id: "org-1".to_owned(),
        user_id: "user-1".to_owned(),
        model: model.to_owned(),
        ..Default::default()
    }
}

fn title_role(fallback_model: &str) -> RoleCall {
    RoleCall {
        purpose: Some(RolePurpose::Title),
        fallback_model: fallback_model.to_owned(),
    }
}

fn shared() -> Seen {
    Arc::new(Mutex::new(Vec::new()))
}

fn calls(seen: &Seen) -> Vec<String> {
    seen.lock().unwrap().clone()
}

/// A role call whose table candidate the platform overlay hides is not chosen: the next candidate serves it.
#[tokio::test]
async fn a_role_candidate_the_overlay_hides_is_not_chosen() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        policy_with_title(&["gpt-5-nano"], &["gpt-5-nano", "gpt-4o-mini"]),
    );
    let (response, _) = chain
        .infer_role_noted(&request(ROLE_MODEL_ID), &title_role(""), false)
        .await
        .expect("the visible candidate serves the title");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(
        calls(&seen),
        ["gpt-4o-mini"],
        "the hidden candidate was called"
    );
}

/// A role call with no table entry runs on its turn model, unless the overlay hides that model: then the cheap
/// fallback answers.
#[tokio::test]
async fn a_hidden_turn_model_gives_way_to_the_cheap_fallback() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-5-mini"]),
    );
    let (response, _) = chain
        .infer_role_noted(&request(ROLE_MODEL_ID), &title_role("gpt-5-mini"), false)
        .await
        .expect("the cheap fallback serves the title");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(
        calls(&seen),
        ["gpt-4o-mini"],
        "the hidden turn model was called"
    );
}

/// The cheap fallback is taken without the question, so the walk must refuse it when the overlay hides it: a role
/// call nothing visible can serve ends as `model_hidden`, and no provider is called.
#[tokio::test]
async fn a_hidden_cheap_fallback_is_refused_before_any_provider() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-4o-mini"]),
    );
    let error = chain
        .infer_role_noted(&request(ROLE_MODEL_ID), &title_role(""), false)
        .await
        .expect_err("a hidden cheap fallback is refused");
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    assert!(calls(&seen).is_empty(), "a provider was called");
}

/// `verevon-balance` whose intent cell the overlay hides is served by the next visible tool-ladder rung, and the
/// hidden cell is never called.
#[tokio::test]
async fn a_hidden_intent_cell_is_served_by_the_next_visible_rung() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-5-mini"]),
    );
    let response = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the first visible rung serves it");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(calls(&seen), ["gpt-4o-mini"], "the hidden cell was called");
}

/// A tool-ladder rung the overlay hides is never called, and it does not use up one of the ladder's steps.
#[tokio::test]
async fn a_hidden_ladder_rung_is_never_called() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-5-mini", "gpt-4o-mini"]),
    );
    let response = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the last visible rung serves it");
    assert_eq!(response.model_used, "gpt-5-nano");
    assert_eq!(calls(&seen), ["gpt-5-nano"], "a hidden model was called");
}

/// An intent call with every candidate hidden is refused as `model_hidden` before any provider is called.
#[tokio::test]
async fn an_intent_call_with_nothing_visible_left_is_refused_as_hidden() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-5-mini", "gpt-4o-mini", "gpt-5-nano"]),
    );
    let error = chain
        .infer(&request("verevon-balance"))
        .await
        .expect_err("nothing visible serves it");
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    assert!(calls(&seen).is_empty(), "a provider was called");
}

/// The stream path skips a hidden intent cell the same way: the first visible rung is the one opened.
#[tokio::test]
async fn a_hidden_intent_cell_is_not_opened_on_the_stream_path() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-5-mini"]),
    );
    let _stream = chain
        .infer_stream(&request("verevon-balance"))
        .await
        .expect("the first visible rung streams it");
    assert_eq!(calls(&seen), ["gpt-4o-mini"], "the hidden cell was opened");
}

/// An answer cached while its model was visible is not served once the overlay hides that model: the request
/// runs again and the visible rung answers.
#[tokio::test]
async fn a_cached_answer_from_a_model_now_hidden_is_not_served() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&[]),
    );
    let first = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the cell serves it while visible");
    assert_eq!(first.model_used, "gpt-5-mini");

    chain
        .policy_handle()
        .store(Arc::new(shared_policy(&["gpt-5-mini"])));
    let second = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the visible rung serves it");
    assert_eq!(
        second.model_used, "gpt-4o-mini",
        "the cached answer from the hidden model was served"
    );
    assert_eq!(calls(&seen), ["gpt-5-mini", "gpt-4o-mini"]);
}

/// A role candidate that is a retired Claude id is judged by the model that serves it: a successor the overlay
/// hides refuses the candidate, and the next candidate serves the title.
#[tokio::test]
async fn a_retired_role_candidate_is_judged_by_its_successor() {
    let seen = shared();
    let chain = chain_with(
        vec![("anthropic", Recording::anthropic(&seen))],
        policy_with_title(
            &["claude-sonnet-5-5"],
            &["claude-sonnet-4-20250514", "claude-haiku-5-5"],
        ),
    );
    let (response, _) = chain
        .infer_role_noted(&request(ROLE_MODEL_ID), &title_role(""), false)
        .await
        .expect("the visible candidate serves the title");
    assert_eq!(response.model_used, "claude-haiku-5-5");
    assert_eq!(
        calls(&seen),
        ["claude-haiku-5-5"],
        "the retired candidate was called"
    );
}

/// A stored answer that a tool-ladder rung served is not served once the overlay hides that rung. The cache key
/// names the intent cell, which stays visible, so the stored answer is judged by the model that answered it.
#[tokio::test]
async fn a_cached_rung_answer_the_overlay_now_hides_is_not_served() {
    let seen = shared();
    // The provider does not list the balance cell, so the cell cannot be served and the first rung answers.
    let chain = chain_with(
        vec![(
            "openai",
            Recording::openai_exclusive(&["gpt-4o-mini", "gpt-5-nano"], &seen),
        )],
        shared_policy(&[]),
    );
    let first = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the first rung serves it");
    assert_eq!(first.model_used, "gpt-4o-mini");

    chain
        .policy_handle()
        .store(Arc::new(shared_policy(&["gpt-4o-mini"])));
    let second = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the visible rung serves it");
    assert_eq!(
        second.model_used, "gpt-5-nano",
        "the cached answer from the hidden rung was served"
    );
    assert_eq!(calls(&seen), ["gpt-4o-mini", "gpt-5-nano"]);
}

/// An automatic call (no model named, or `default`) is served the provider's default, and the walk sends that default
/// as the model. When the overlay hides it and no other provider can answer, the call is refused as `model_hidden`
/// before any provider is called.
#[tokio::test]
async fn an_automatic_call_is_not_served_a_hidden_provider_default() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-4o-mini"]),
    );
    for model in ["", "default"] {
        let error = chain
            .infer(&request(model))
            .await
            .expect_err("the hidden provider default is refused");
        assert!(
            matches!(error, ProviderError::ModelHidden { .. }),
            "{model:?}: {error:?}"
        );
    }
    assert!(calls(&seen).is_empty(), "a provider was called");
}

/// The stream path refuses an automatic call whose provider default the overlay hides, before any stream is opened.
#[tokio::test]
async fn an_automatic_stream_is_not_opened_on_a_hidden_provider_default() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-4o-mini"]),
    );
    let error = chain
        .infer_stream(&request(""))
        .await
        .expect_err("the hidden provider default is refused on the stream path");
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    assert!(calls(&seen).is_empty(), "a stream was opened");
}

/// An automatic call moves on past a provider whose default the overlay hides: the next provider, whose default is
/// visible, serves it on both paths, and the hidden default is never called.
#[tokio::test]
async fn an_automatic_call_moves_on_to_a_provider_whose_default_is_visible() {
    let seen = shared();
    let chain = chain_with(
        vec![
            ("openai", Recording::openai(&seen)),
            ("anthropic", Recording::anthropic(&seen)),
        ],
        shared_policy(&["gpt-4o-mini"]),
    );
    let response = chain
        .infer(&request(""))
        .await
        .expect("the visible default serves it");
    assert_eq!(response.model_used, "claude-sonnet-5-5");
    assert_eq!(
        calls(&seen),
        ["claude-sonnet-5-5"],
        "the hidden default was called"
    );
    let _stream = chain
        .infer_stream(&request(""))
        .await
        .expect("the visible default streams it");
    assert_eq!(
        calls(&seen),
        ["claude-sonnet-5-5", "claude-sonnet-5-5"],
        "the hidden default was opened on the stream path"
    );
}

/// With the intent layer off (`enabled = false`), a `verevon-*` id is answered by the provider default, as an
/// automatic id is. The overlay hides that default, so the call is refused on both paths and no provider is called.
#[tokio::test]
async fn a_verevon_mode_with_the_intent_layer_off_is_not_served_a_hidden_default() {
    let seen = shared();
    let mut policy = shared_policy(&["gpt-4o-mini"]);
    policy.enabled = false;
    let chain = chain_with(vec![("openai", Recording::openai(&seen))], policy);
    let error = chain
        .infer(&request("verevon-balance"))
        .await
        .expect_err("the hidden provider default is refused");
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    let error = chain
        .infer_stream(&request("verevon-balance"))
        .await
        .expect_err("the hidden provider default is refused on the stream path");
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "stream: {error:?}"
    );
    assert!(calls(&seen).is_empty(), "a provider was called");
}

/// With the intent layer off, a `verevon-*` call whose provider default the overlay hides moves on to the next
/// provider whose default is visible, as an automatic call does.
#[tokio::test]
async fn a_verevon_mode_with_the_intent_layer_off_moves_on_to_a_visible_default() {
    let seen = shared();
    let mut policy = shared_policy(&["gpt-4o-mini"]);
    policy.enabled = false;
    let chain = chain_with(
        vec![
            ("openai", Recording::openai(&seen)),
            ("anthropic", Recording::anthropic(&seen)),
        ],
        policy,
    );
    let response = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the visible default serves it");
    assert_eq!(response.model_used, "claude-sonnet-5-5");
    assert_eq!(
        calls(&seen),
        ["claude-sonnet-5-5"],
        "the hidden default was called"
    );
}

/// A policy with no tool ladder: an intent pick has no rung after it, so the walk has nothing else to try.
fn ladderless_policy(hidden: &[&str]) -> RoutingPolicy {
    let mut policy = shared_policy(hidden);
    policy.tool_fallback_ladder.clear();
    policy
}

/// A hidden intent pick with no ladder is served by the policy's cheap fallback, a visible model the walk can try, so
/// the call does not end as `model_hidden` while that model is visible.
#[tokio::test]
async fn a_ladderless_hidden_intent_pick_is_served_by_the_cheap_fallback() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        ladderless_policy(&["gpt-5-mini"]),
    );
    let response = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the visible cheap fallback serves it");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(calls(&seen), ["gpt-4o-mini"], "the hidden pick was called");
}

/// The stream path takes the same substitute: the cheap fallback is the model opened.
#[tokio::test]
async fn a_ladderless_hidden_intent_pick_is_opened_on_the_cheap_fallback() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        ladderless_policy(&["gpt-5-mini"]),
    );
    let _stream = chain
        .infer_stream(&request("verevon-balance"))
        .await
        .expect("the visible cheap fallback streams it");
    assert_eq!(calls(&seen), ["gpt-4o-mini"], "the hidden pick was opened");
}

/// A ladderless intent pick with no visible candidate left is refused as `model_hidden` before any provider is called.
#[tokio::test]
async fn a_ladderless_hidden_intent_pick_with_nothing_visible_is_refused_as_hidden() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        ladderless_policy(&["gpt-5-mini", "gpt-4o-mini"]),
    );
    let error = chain
        .infer(&request("verevon-balance"))
        .await
        .expect_err("nothing visible serves it");
    assert!(
        matches!(error, ProviderError::ModelHidden { .. }),
        "{error:?}"
    );
    assert!(calls(&seen).is_empty(), "a provider was called");
}

/// A hidden id with surrounding whitespace hides the model it names: the intent cell is not served, the first rung is.
#[tokio::test]
async fn a_padded_hidden_id_hides_the_model_it_names() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&[" gpt-5-mini "]),
    );
    let response = chain
        .infer(&request("verevon-balance"))
        .await
        .expect("the first visible rung serves it");
    assert_eq!(response.model_used, "gpt-4o-mini");
    assert_eq!(
        calls(&seen),
        ["gpt-4o-mini"],
        "the padded hidden id did not hide the cell"
    );
}

/// The route note of an intent call the overlay moved names the mode, but no budget move and no reason for the model
/// the intent layer chose, which is not the one served.
#[tokio::test]
async fn the_route_note_of_a_call_the_overlay_moved_names_no_budget_move() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&["gpt-5-mini"]),
    );
    let (response, note) = chain
        .infer_noted(&request("verevon-balance"), false)
        .await
        .expect("the next visible rung serves it");
    assert_eq!(response.model_used, "gpt-4o-mini");
    let note = note.expect("the route note is on");
    assert_eq!(note.mode, Some(VerevonMode::Balance));
    assert_eq!(
        note.reason, "",
        "the note gives the reason of a model that was not served"
    );
    assert!(!note.tier_shifted && !note.cheap_fallback_forced);
}

/// An intent pick no overlay hides keeps the note it always had: its mode, and the reason the posture gives. No
/// cost-core budget is wired in this binary, so the posture is unknown.
#[tokio::test]
async fn an_intent_pick_no_overlay_hides_keeps_its_route_note() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&[]),
    );
    let (response, note) = chain
        .infer_noted(&request("verevon-balance"), false)
        .await
        .expect("the cell serves it");
    assert_eq!(response.model_used, "gpt-5-mini");
    let note = note.expect("the route note is on");
    assert_eq!(note.mode, Some(VerevonMode::Balance));
    assert_eq!(note.reason, "posture_unknown");
}

/// The access verdict GetModelAccess answers with is `hiding_overlay`, and it judges a model as admission does
/// (V3-inf-1): a hidden id is hidden whatever its case or padding, and an alias is judged by the model it stands for.
#[tokio::test]
async fn the_access_verdict_reports_a_model_the_platform_overlay_hides() {
    let seen = shared();
    let mut policy = shared_policy(&["gpt-5-mini", " gpt-5-nano "]);
    policy.catalogue_overlay.aliases = BTreeMap::from([
        ("hidden-alias".to_owned(), "gpt-5-mini".to_owned()),
        ("visible-alias".to_owned(), "gpt-4o-mini".to_owned()),
    ]);
    let chain = chain_with(vec![("openai", Recording::openai(&seen))], policy);
    assert_eq!(
        chain.hiding_overlay("org-1", "gpt-5-mini").await,
        Some(HidingOverlay::Platform)
    );
    assert_eq!(
        chain.hiding_overlay("org-1", " GPT-5-MINI ").await,
        Some(HidingOverlay::Platform)
    );
    assert_eq!(
        chain.hiding_overlay("org-1", "gpt-5-nano").await,
        Some(HidingOverlay::Platform),
        "a padded hidden entry did not hide the model it names"
    );
    assert_eq!(
        chain.hiding_overlay("org-1", "hidden-alias").await,
        Some(HidingOverlay::Platform),
        "an alias of a hidden model was not judged by its target"
    );
    assert_eq!(chain.hiding_overlay("org-1", "visible-alias").await, None);
    assert_eq!(chain.hiding_overlay("org-1", "gpt-4o-mini").await, None);
    assert!(calls(&seen).is_empty(), "a provider was called");
}

/// A retired Claude id is served as its successor, so the verdict judges it by the successor (V3-inf-1).
#[tokio::test]
async fn the_access_verdict_judges_a_retired_claude_id_by_its_successor() {
    let seen = shared();
    let chain = chain_with(
        vec![("anthropic", Recording::anthropic(&seen))],
        shared_policy(&["claude-sonnet-5-5"]),
    );
    assert_eq!(
        chain
            .hiding_overlay("org-1", "claude-sonnet-4-20250514")
            .await,
        Some(HidingOverlay::Platform),
        "a retired id whose successor is hidden was not refused"
    );
    assert_eq!(
        chain.hiding_overlay("org-1", "claude-haiku-5-5").await,
        None
    );
}

/// The ids the routing layer answers for itself are never judged by an overlay (V3-inf-1): an automatic call, an
/// intent mode and the role table each pick a model and the walk judges that pick. The overlay here also lists those
/// ids as hidden, which only the reserved-id check keeps from refusing them, and the verdict still names none of them.
#[tokio::test]
async fn the_access_verdict_leaves_the_ids_the_routing_layer_answers_for_itself() {
    let seen = shared();
    let chain = chain_with(
        vec![("openai", Recording::openai(&seen))],
        shared_policy(&[
            "gpt-4o-mini",
            "gpt-5-mini",
            "verevon-balance",
            ROLE_MODEL_ID,
        ]),
    );
    for id in ["", "verevon-balance", ROLE_MODEL_ID] {
        assert_eq!(chain.hiding_overlay("org-1", id).await, None, "{id:?}");
    }
    assert!(calls(&seen).is_empty(), "a provider was called");
}
