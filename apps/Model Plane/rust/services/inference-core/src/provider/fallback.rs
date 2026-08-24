//! Sequential fallback chain — tries providers in order with bounded retries.

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::policy_client::PolicyClient;
use super::routing_policy::RoutingPolicy;
use super::{
    anthropic::AnthropicProvider, endpoint_region_is_non_eu, intent, is_eu_region,
    normalize_region_token, openai::OpenAiProvider, EmbedRequest, EmbedResponse, InferChunk,
    InferRequest, InferResponse, ModelFamily, ModelInfo, PrivacyTier, ProviderCapabilities,
    ProviderError, ProviderRouter, Residency,
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
    fn capabilities_dyn(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }
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

    fn capabilities_dyn(&self) -> ProviderCapabilities {
        self.capabilities()
    }
}

/// Sequential fallback chain with bounded retries per provider.
#[derive(Clone)]
pub struct FallbackChain {
    providers: Vec<(String, BoxedProvider)>,
    max_retries: u32,
    cache: Arc<PromptCache>,
    /// The live Verevon routing policy. Seeded from [`RoutingPolicy::default`] and
    /// hot-swapped by the refresh loop / the HTTP PUT write-through. When
    /// `policy.enabled` is false the intent layer is bypassed.
    policy: Arc<ArcSwap<RoutingPolicy>>,
    /// Client for session-core's `RoutingPolicy` store. Drives the refresh loop
    /// and the write path; `None` when `SESSION_CORE_URL`/`_ADDR` is unset (the
    /// chain then runs forever on the default policy).
    policy_client: Option<Arc<PolicyClient>>,
    /// Best-effort budget signal for the intent layer. `None` disables the gate.
    budget: Option<Arc<intent::BudgetClient>>,
    /// EU embedding residency posture, resolved at boot from config. Drives the
    /// request-time deny-by-default gate in [`FallbackChain::create_embedding`].
    residency: EmbeddingResidency,
    /// Per-provider default chat model, resolved at boot from the configured
    /// deployment catalogs.
    defaults: ProviderDefaults,
    /// What this deployment can actually serve, used to prune tool-ladder rungs.
    deployed: DeployedModels,
    /// Per-provider model catalog, snapshotted once at boot from each provider's
    /// `list_models`.
    ///
    /// Keyed by registry id, unlike [`DeployedModels`], which has one slot per
    /// *family* and therefore cannot distinguish two providers of the same family.
    /// That family-level granularity is why a second OpenAI-compatible provider
    /// could not previously coexist with Azure. Only consulted for providers that
    /// declare `exclusive_catalog`.
    catalogs: std::collections::HashMap<String, Vec<String>>,
}

/// The deployment names that gate each model family on this resource.
///
/// `None` means "not knowable": either no provider of that family registered, or
/// the one that did is a direct vendor API, which accepts any published model id
/// and therefore has no deployment list to check against. Pruning on a guess
/// would delete real fallback options, so `None` prunes nothing.
#[derive(Debug, Clone, Default)]
struct DeployedModels {
    anthropic: Option<Vec<String>>,
    openai: Option<Vec<String>>,
}

/// Default chat model per registered provider, resolved from configuration.
///
/// The Azure entries were compile-time constants, which is wrong for Azure: a
/// model name there is a **deployment** name private to the resource, so a
/// hardcoded default 404s `DeploymentNotFound` on any resource that does not
/// happen to host it. That is precisely how every unspecified-model request died
/// against an operator whose only chat deployment is `gpt-4o-mini` — the chain
/// asked for `model-router`, which exists on some resources and not on theirs.
///
/// The direct (`anthropic` / `openai`) providers keep their compile-time
/// defaults: their model ids are global to the vendor, not per-resource names,
/// so there is no deployment to mismatch.
#[derive(Debug, Clone)]
pub struct ProviderDefaults {
    azure_openai: String,
    azure_anthropic: String,
}

impl Default for ProviderDefaults {
    fn default() -> Self {
        Self {
            azure_openai: AZURE_MODEL_ROUTER.to_owned(),
            azure_anthropic: super::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL.to_owned(),
        }
    }
}

impl ProviderDefaults {
    fn from_config(cfg: &InferenceConfig) -> Self {
        Self {
            azure_openai: azure_default_deployment(
                &cfg.azure_openai_chat_deployments,
                AZURE_MODEL_ROUTER,
            ),
            azure_anthropic: azure_default_deployment(
                &cfg.azure_anthropic_deployments,
                super::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL,
            ),
        }
    }
}

/// Pick the default deployment for an Azure-style provider.
///
/// `preferred` wins when the operator's catalog actually lists it — a resource
/// that really does host Azure's auto-routing `model-router` (or the cheap
/// `claude-haiku-4-5`) should keep getting it. Otherwise the operator's first
/// configured deployment is the only name known to exist. An empty catalog means
/// nothing was configured, so `preferred` is the last resort rather than a
/// guess.
fn azure_default_deployment(configured: &[String], preferred: &str) -> String {
    configured
        .iter()
        .find(|name| name.eq_ignore_ascii_case(preferred))
        .or_else(|| configured.first())
        .map_or_else(|| preferred.to_owned(), String::clone)
}

/// Resolved EU embedding residency posture for the chain.
///
/// `allow_non_eu` is the explicit operator opt-in
/// (`MODEL_PLANE_ALLOW_NON_EU_EMBEDDING`); when it is `false` (the default) any
/// non-EU requested region — or a configured deployment region the startup gate
/// flagged non-EU — is rejected before any network call.
#[derive(Debug, Clone, Default)]
pub struct EmbeddingResidency {
    /// Explicit opt-in to egress embeddings outside the EU residency boundary.
    pub allow_non_eu: bool,
    /// Configured deployment region (`AZURE_OPENAI_REGION`), normalized; empty
    /// when unspecified.
    pub configured_region: String,
}

impl FallbackChain {
    /// EU embedding residency — REQUEST-time deny-by-default gate. Rejects
    /// BEFORE any network call when the resolved region is non-EU and the
    /// operator has not explicitly opted in. The resolved region is the
    /// request's `region` when set, else the configured deployment region.
    /// Mirrors the speech.rs allow-flag shape but REJECTS (does not
    /// warn-and-fallback): an EU/ZDR posture must fail closed.
    fn reject_non_eu_embedding_region(&self, req: &EmbedRequest) -> Result<(), ProviderError> {
        if self.residency.allow_non_eu {
            return Ok(());
        }
        let requested = normalize_region_token(&req.region);
        let resolved = if requested.is_empty() {
            self.residency.configured_region.as_str()
        } else {
            requested.as_str()
        };
        if !is_eu_region(resolved) {
            warn!(
                request_id = %req.request_id,
                region = %resolved,
                "embedding rejected: non-EU residency region and \
                 MODEL_PLANE_ALLOW_NON_EU_EMBEDDING is off"
            );
            return Err(ProviderError::ResidencyViolation(format!(
                "embedding region `{resolved}` is outside the EU residency boundary and \
                 MODEL_PLANE_ALLOW_NON_EU_EMBEDDING is off"
            )));
        }
        Ok(())
    }
}

/// True when the caller didn't pin a model — empty or a "let the gateway pick"
/// sentinel. Such requests resolve to the per-provider default so "Verevon Auto"
/// works against whatever provider is actually configured.
fn is_unspecified_model(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    // Empty / "default", plus any Verevon intent id. The intent layer normally
    // rewrites a `verevon-*` id to a concrete model before the provider loop;
    // treating it as unspecified here is the safety net for when the intent
    // layer is disabled — the id resolves to the provider default instead of
    // being sent verbatim (which would 404 the deployment).
    m.is_empty() || m == "default" || intent::parse_mode(model).is_some()
}

/// True for an Anthropic-family model id (the only models the Anthropic-shaped
/// providers can serve). Used to keep `claude-*` requests off the OpenAI/Azure
/// chat-completions surface (which would 404 the deployment) and vice-versa.
fn is_anthropic_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("claude")
}

/// The Azure `model-router` deployment — Azure auto-routes it to the cheapest
/// capable model. Used as the smart default for unspecified requests when the
/// Azure `OpenAI` provider is serving them.
const AZURE_MODEL_ROUTER: &str = "model-router";

/// Longest a throttled request may wait before the chain is retried.
///
/// Moving to the next provider stays the primary response to a 429 — it is
/// instant and usually succeeds. But when every matching provider is throttled
/// (commonly: only one is configured for the model family) the chain used to
/// discard `retry_after_ms` and fail, turning a few seconds of throttling into a
/// dead turn. Waiting is only the better answer while the wait is short enough
/// that a person is still willing to sit through it; past that, telling them
/// when to come back beats holding a request open. Chat is interactive, so the
/// default is deliberately small.
const DEFAULT_RATE_LIMIT_MAX_WAIT_MS: u64 = 8_000;

/// True when a provider may serve a request requiring `min_privacy_tier`.
///
/// The tier combines the two independent axes — geography and retention — into
/// one comparison: derive the provider's strongest tier from its declared
/// capabilities ([`PrivacyTier::classify`]) and require `>=`. `Unspecified`
/// imposes no constraint, so pre-tier behavior stays byte-identical. A
/// provider below the floor is skipped BEFORE any network call, the same way a
/// non-ZDR provider is skipped when `zdr` is set.
fn provider_meets_privacy_tier(caps: &ProviderCapabilities, min_privacy_tier: PrivacyTier) -> bool {
    min_privacy_tier == PrivacyTier::Unspecified
        || PrivacyTier::classify(caps) >= min_privacy_tier
}

/// The typed exhaustion error for a tier-constrained request whose entire
/// matching chain was skipped. Names the REQUIRED tier so callers see exactly
/// what could not be honored; never a silent downgrade.
fn tier_unavailable_error(required: PrivacyTier) -> ProviderError {
    ProviderError::TierUnavailable(format!(
        "no configured provider satisfies privacy tier `{}`",
        required.label()
    ))
}

fn rate_limit_max_wait() -> Duration {
    static MAX_WAIT: std::sync::OnceLock<Duration> = std::sync::OnceLock::new();
    *MAX_WAIT.get_or_init(|| {
        let ms = std::env::var("INFERENCE_RATE_LIMIT_MAX_WAIT_MS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_RATE_LIMIT_MAX_WAIT_MS);
        Duration::from_millis(ms)
    })
}

/// Throttling seen while walking the provider chain.
///
/// Kept separate from the generic failure count because the two need different
/// answers: a throttled chain is worth waiting for or reporting with a time, and
/// a broken one is not.
#[derive(Debug, Default, Clone, Copy)]
struct ThrottleState {
    /// Shortest `retry_after_ms` any throttled provider asked for — the soonest
    /// moment a retry could plausibly succeed.
    soonest_retry_ms: Option<u64>,
}

impl ThrottleState {
    fn record(&mut self, retry_after_ms: u64) {
        self.soonest_retry_ms = Some(match self.soonest_retry_ms {
            Some(current) => current.min(retry_after_ms),
            None => retry_after_ms,
        });
    }

    fn throttled(self) -> bool {
        self.soonest_retry_ms.is_some()
    }

    /// The wait to honor before retrying the chain, or `None` when the provider's
    /// own retry-after exceeds what an interactive request should absorb.
    fn affordable_wait(self) -> Option<Duration> {
        let wait = Duration::from_millis(self.soonest_retry_ms?);
        (wait <= rate_limit_max_wait()).then_some(wait)
    }

    /// The error a throttled, exhausted chain should return.
    ///
    /// `RateLimited` rather than `AllExhausted` on purpose: it carries the
    /// retry-after, so a caller can say "try again in about a minute" instead of
    /// reporting a generic failure for something that is neither permanent nor
    /// the user's fault.
    fn exhausted_error(self, attempts: u32) -> ProviderError {
        match self.soonest_retry_ms {
            Some(retry_after_ms) => ProviderError::RateLimited { retry_after_ms },
            None => ProviderError::AllExhausted { attempts },
        }
    }
}

/// How far past the resolved model the tool ladder may walk.
///
/// Bounded because each rung costs a full provider walk (up to `max_retries`
/// requests) before the next is tried, and an interactive turn cannot absorb an
/// unbounded search. Four is what it takes to cross the whole Claude family and
/// still reach one `OpenAI` rung on a resource with all four Claude deployments.
const MAX_TOOL_LADDER_STEPS: usize = 4;

/// Whether a registered provider can serve the requested model.
///
/// Anthropic-family providers serve only `claude-*`; `OpenAI`-compatible ones
/// serve everything else. An unspecified model is served by any provider (it
/// resolves to that provider's default). This stops the chain wasting an attempt
/// — and emitting a spurious 404 — by sending a Claude model to the `OpenAI`
/// surface or vice versa.
///
/// The family now comes from the provider's own declaration rather than from
/// `matches!(provider_name, "anthropic" | "azure-anthropic")`. That literal was
/// the reason a third OpenAI-compatible provider could not be added: every such
/// provider registered under one of two known names, so the chain could not tell
/// them apart and the first registered absorbed every non-Claude model.
///
/// A provider that declares `exclusive_catalog` is additionally held to its own
/// catalog. Sovereign providers must: routing an unrecognised model to one either
/// 404s or gets silently served from a brokered upstream outside the residency
/// boundary the tier was sold on — and the latter looks like success.
fn provider_serves_model(
    caps: &ProviderCapabilities,
    catalog: Option<&[String]>,
    model: &str,
) -> bool {
    if is_unspecified_model(model) {
        return true;
    }
    let family_matches = match caps.model_family {
        ModelFamily::Anthropic => is_anthropic_model(model),
        ModelFamily::OpenAiCompatible => !is_anthropic_model(model),
    };
    if !family_matches {
        return false;
    }
    if !caps.exclusive_catalog {
        return true;
    }
    // Exclusive: an empty catalog serves nothing rather than everything. A
    // sovereign provider misconfigured with no catalog must go quiet, not become
    // a wildcard.
    catalog.is_some_and(|models| models.iter().any(|known| known.eq_ignore_ascii_case(model)))
}

impl FallbackChain {
    /// Build a fallback chain from configuration.
    ///
    /// # Panics
    ///
    /// Panics at boot (fail-loud) when an Azure embedding provider is registered
    /// against a non-EU deployment region/endpoint and the operator has not set
    /// `MODEL_PLANE_ALLOW_NON_EU_EMBEDDING` — a misconfigured non-EU embedding
    /// path must never be allowed to serve traffic.
    // Linear provider-registration table plus the intent/budget wiring; reads
    // top-to-bottom and isn't worth fragmenting across helpers.
    #[allow(clippy::too_many_lines)]
    pub fn from_config(cfg: &InferenceConfig) -> Self {
        let mut providers: Vec<(String, BoxedProvider)> = Vec::new();

        // Validated at config load; Arc'd once so each registration site shares
        // one instance rather than cloning four Strings per provider.
        let azure_openai_zdr = cfg.azure_openai_zdr.clone().map(Arc::new);
        let azure_anthropic_zdr = cfg.azure_anthropic_zdr.clone().map(Arc::new);

        // Correlate each attestation with the endpoint it is supposed to cover. The
        // digest cannot do this: it binds the fields to each other, not to a host.
        if let (Some(zdr), Some(endpoint)) = (
            azure_openai_zdr.as_ref(),
            cfg.azure_openai_endpoint.as_deref(),
        ) {
            zdr.warn_on_endpoint_mismatch("azure-openai", endpoint);
        }
        if let (Some(zdr), Some(endpoint)) = (
            azure_anthropic_zdr.as_ref(),
            cfg.azure_anthropic_endpoint.as_deref(),
        ) {
            zdr.warn_on_endpoint_mismatch("azure-anthropic", endpoint);
        }

        let has_explicit_azure = cfg
            .provider_order
            .iter()
            .any(|name| matches!(name.as_str(), "azure" | "azure-openai"));

        // Residency per Azure resource, classified independently.
        //
        // These are separate Azure resources -- the checked-in config points them
        // at core-ai-rg.cognitiveservices.azure.com and
        // cloude-ai-resource.services.ai.azure.com -- so neither may inherit the
        // other's geography. An earlier version of this code did exactly that, and
        // would have declared Foundry Claude EU-resident on the strength of
        // AZURE_OPENAI_REGION alone.
        //
        // `Eu` requires positive evidence: a known-EU region AND, where the
        // deployment type is declared, a non-global one. Unknown is classified
        // `Global` rather than promoted, because "we cannot prove this stays in the
        // EU" and "this stays in the EU" are different claims and only one of them
        // is true. Phase 0's deployment-type gate deliberately only warns when the
        // type is unset; promoting the same silence to an affirmative `Eu` here
        // would turn a known blind spot into a stated guarantee.
        let classify = |region: Option<&str>, declared_global: bool| -> Residency {
            let region_is_eu = region
                .map(normalize_region_token)
                .is_some_and(|region| is_eu_region(&region));
            Residency::classify(region_is_eu, declared_global)
        };
        let azure_declared_global = cfg
            .azure_openai_deployment_type
            .as_deref()
            .is_some_and(crate::config::deployment_type_is_global);
        let azure_openai_residency =
            classify(cfg.azure_openai_region.as_deref(), azure_declared_global);
        // Foundry Claude has no deployment-type variable of its own; its region is
        // the only signal, and absent one it stays `Global`.
        let azure_anthropic_residency = classify(cfg.azure_anthropic_region.as_deref(), false);
        // Cohere has no deployment-type variable either; region is the only signal.
        let azure_cohere_residency = classify(cfg.azure_cohere_region.as_deref(), false);

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
                            let p = p
                                .with_zdr_attestation(azure_openai_zdr.clone())
                                .with_residency(azure_openai_residency)
                                .with_model_catalog(
                                    cfg.azure_openai_chat_deployments.clone(),
                                    cfg.azure_openai_embedding_deployments.clone(),
                                );
                            providers.push(("azure-openai".to_owned(), Arc::new(p)));
                            info!(provider = "azure-openai", "provider registered");
                        }
                    }
                }
                "anthropic" | "azure-anthropic" => {
                    // Prefer the Azure AI Foundry Claude resource when configured
                    // — the direct api.anthropic.com path is out of credit (400
                    // "credit balance too low"), so Azure Foundry is the working
                    // surface. Fall through to the direct provider only when
                    // Azure Anthropic is not configured.
                    let mut registered_azure_anthropic = false;
                    if let (Some(endpoint), Some(key)) =
                        (&cfg.azure_anthropic_endpoint, &cfg.azure_anthropic_api_key)
                    {
                        if let Ok(p) = AnthropicProvider::new_azure(
                            key.clone(),
                            endpoint.clone(),
                            cfg.azure_anthropic_deployments.clone(),
                        ) {
                            let p = p
                                .with_zdr_attestation(azure_anthropic_zdr.clone())
                                .with_residency(azure_anthropic_residency);
                            providers.push(("azure-anthropic".to_owned(), Arc::new(p)));
                            info!(provider = "azure-anthropic", "provider registered");
                            registered_azure_anthropic = true;
                        }
                    }

                    if !registered_azure_anthropic {
                        if let Some(key) = &cfg.anthropic_api_key {
                            if let Ok(p) = AnthropicProvider::new(key.clone()) {
                                providers.push(("anthropic".to_owned(), Arc::new(p)));
                                info!(provider = "anthropic", "provider registered");
                            }
                        }
                    }
                }
                "cohere" | "azure-cohere" => {
                    // exclusive_catalog: true — this endpoint serves only its own
                    // declared deployment, so it can coexist with Azure OpenAI's
                    // wildcard-ish catalog instead of stealing its traffic (see
                    // `provider_serves_model`).
                    if let (Some(endpoint), Some(key)) =
                        (&cfg.azure_cohere_endpoint, &cfg.azure_cohere_api_key)
                    {
                        if let Ok(p) = OpenAiProvider::new_azure_ai_unified(
                            key.clone(),
                            endpoint.clone(),
                            cfg.azure_cohere_api_version.clone(),
                        ) {
                            let p = p
                                .with_identity("cohere", azure_cohere_residency, true)
                                .with_model_catalog(
                                    vec![cfg.azure_cohere_deployment.clone()],
                                    Vec::new(),
                                );
                            providers.push(("cohere".to_owned(), Arc::new(p)));
                            info!(provider = "cohere", "provider registered");
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
                                let p = p
                                    .with_zdr_attestation(azure_openai_zdr.clone())
                                    .with_residency(azure_openai_residency)
                                    .with_model_catalog(
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

        // EU embedding residency — STARTUP fail-loud (deny-by-default).
        //
        // If an Azure OpenAI provider was registered (it owns the embedding
        // path), classify the configured deployment region. The authoritative
        // signal is `AZURE_OPENAI_REGION`; the endpoint host is a fallback
        // heuristic since Azure OpenAI hosts don't carry the region. A non-EU
        // deployment must NOT be allowed to serve embeddings unless the operator
        // explicitly opted in via `MODEL_PLANE_ALLOW_NON_EU_EMBEDDING` — so we
        // abort boot rather than silently registering a non-EU embedding path.
        let configured_region =
            normalize_region_token(cfg.azure_openai_region.as_deref().unwrap_or_default());
        let azure_registered = providers.iter().any(|(name, _)| name == "azure-openai");

        // Global-deployment gate — STARTUP fail-loud (deny-by-default).
        //
        // A `Global`/`GlobalStandard` Azure deployment routes to whichever region
        // has capacity, so data can be at rest in the European geography while
        // *inference* runs anywhere in the world. The region check above cannot
        // see this: an EU-region resource can still host a Global deployment, and
        // the endpoint host carries neither fact. Left unchecked it silently voids
        // the EU-boundary claim for every request, which is precisely the failure
        // this gate exists to make impossible.
        //
        // Unset is a warning rather than a rejection: existing deployments
        // predate the variable, and refusing to boot on absence would take the
        // running stack down for a claim it may not even be making. Explicitly
        // declaring Global is a rejection.
        if azure_registered {
            match cfg.azure_openai_deployment_type.as_deref() {
                Some(declared) if crate::config::deployment_type_is_global(declared) => {
                    assert!(
                        cfg.allow_global_deployment,
                        "global deployment residency: AZURE_OPENAI_DEPLOYMENT_TYPE={declared:?} \
                         processes inference in any Azure region worldwide, which voids the EU \
                         residency claim, and MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT is off. \
                         Refusing to boot. Re-deploy the model as DataZoneStandard (EU) or a \
                         regional Standard deployment, or set \
                         MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT=1 to explicitly accept worldwide \
                         processing."
                    );
                    warn!(
                        deployment_type = %declared,
                        "AZURE_OPENAI_DEPLOYMENT_TYPE is global and MODEL_PLANE_ALLOW_GLOBAL_DEPLOYMENT \
                         is set — inference may be processed outside the EU"
                    );
                }
                Some(declared) => {
                    info!(deployment_type = %declared, "azure openai deployment type declared");
                }
                None => {
                    warn!(
                        "AZURE_OPENAI_DEPLOYMENT_TYPE is unset — cannot prove the Azure OpenAI \
                         deployment keeps inference inside the EU. Set it (e.g. DataZoneStandard) \
                         so the global-deployment gate can enforce the residency claim."
                    );
                }
            }
        }
        if azure_registered && !cfg.allow_non_eu_embedding {
            let region_is_non_eu =
                !configured_region.is_empty() && !is_eu_region(&configured_region);
            let endpoint_is_non_eu = cfg
                .azure_openai_endpoint
                .as_deref()
                .is_some_and(endpoint_region_is_non_eu);
            let configured_region_display = cfg.azure_openai_region.as_deref().unwrap_or("");
            assert!(
                !(region_is_non_eu || endpoint_is_non_eu),
                "EU embedding residency: the configured Azure embedding deployment is \
                 non-EU (AZURE_OPENAI_REGION={configured_region_display:?}, endpoint flagged \
                 non-EU={endpoint_is_non_eu}) and MODEL_PLANE_ALLOW_NON_EU_EMBEDDING is off. \
                 Refusing to boot a non-EU embedding path. Point AZURE_OPENAI_ENDPOINT/\
                 AZURE_OPENAI_REGION at an EU deployment (e.g. swedencentral), or set \
                 MODEL_PLANE_ALLOW_NON_EU_EMBEDDING=1 to explicitly accept the cross-region \
                 transfer."
            );
        }
        let residency = EmbeddingResidency {
            allow_non_eu: cfg.allow_non_eu_embedding,
            configured_region,
        };

        let budget = cfg
            .cost_core_url
            .as_deref()
            .and_then(intent::BudgetClient::new)
            .map(Arc::new);

        // Seed the live policy from the compile-time default, then let the
        // bootstrap env config (verevon_intent_enabled / budget_usd) override the
        // seed so the static knobs still work without a session-core store.
        let seed = RoutingPolicy {
            enabled: cfg.verevon_intent_enabled,
            budget_cap_usd: cfg.verevon_intent_budget_usd,
            ..RoutingPolicy::default()
        };
        let policy = Arc::new(ArcSwap::from_pointee(seed));

        // When session-core is reachable, build the policy client and start a
        // periodic refresh loop. A successful fetch hot-swaps the live policy;
        // an empty/unreachable store leaves the seed in place (fail-soft).
        let policy_client = cfg
            .session_core_url
            .as_deref()
            .and_then(PolicyClient::from_url)
            .map(Arc::new);
        if let Some(client) = policy_client.clone() {
            let policy_handle = policy.clone();
            let refresh = Duration::from_secs(cfg.router_policy_refresh_secs.max(1));
            tokio::spawn(async move {
                loop {
                    if let Some(fetched) = client.fetch().await {
                        policy_handle.store(Arc::new(fetched));
                    }
                    tokio::time::sleep(refresh).await;
                }
            });
        }

        if cfg.verevon_intent_enabled {
            info!(
                budget_gate = budget.is_some(),
                policy_store = policy_client.is_some(),
                "verevon intent layer enabled"
            );
        }

        // Which ladder rungs this resource can actually serve. Only the Azure
        // flavors are deployment-gated, and only when they registered and were
        // given a catalog — the direct vendor APIs accept any published model
        // id, so for them there is nothing authoritative to prune against.
        let mut deployed = DeployedModels::default();
        for (name, _) in &providers {
            match name.as_str() {
                "azure-anthropic" if !cfg.azure_anthropic_deployments.is_empty() => {
                    deployed.anthropic = Some(cfg.azure_anthropic_deployments.clone());
                }
                "azure-openai" if !cfg.azure_openai_chat_deployments.is_empty() => {
                    deployed.openai = Some(cfg.azure_openai_chat_deployments.clone());
                }
                _ => {}
            }
        }

        // Residency posture, recorded per provider at boot.
        //
        // This warns rather than aborting, matching the EU embedding gate's actual
        // precedent: that gate fires only when it can *positively determine* a
        // non-EU region, and lets an unknown one through. Aborting on `Global`
        // would also abort on merely-unproven, which after the classification above
        // includes every deployment that has not yet set a region -- i.e. the
        // running stack. A crash loop is a worse outcome than an honest log line,
        // and nothing is silently misrepresented: `Global` is what gets recorded
        // and logged.
        //
        // Real enforcement belongs on the request path, where a caller asks for a
        // minimum residency and a provider that cannot meet it is skipped the way
        // a non-ZDR provider already is. That needs a residency field on
        // InferRequest, which needs the proto, and is deferred with the rest of the
        // request-side work (strategy doc Phase 2). Until then this is disclosure,
        // not a control -- and it is labelled as such rather than dressed up.
        for (name, provider) in &providers {
            let declared = provider.capabilities_dyn().residency;
            if declared > Residency::Global {
                info!(provider = %name, residency = declared.as_str(), "provider residency declared");
            } else if cfg.allow_global_residency_providers {
                info!(
                    provider = %name,
                    "provider declares no residency commitment; explicitly accepted via \
                     MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY"
                );
            } else {
                warn!(
                    provider = %name,
                    "provider declares NO residency commitment: traffic to it may be processed \
                     outside the EU/EEA. Set its region (AZURE_OPENAI_REGION / \
                     AZURE_ANTHROPIC_REGION) to an EU region, or set \
                     MODEL_PLANE_ALLOW_GLOBAL_RESIDENCY=1 to record this as accepted."
                );
            }
        }

        // Snapshot each provider's catalog once, keyed by registry id.
        let catalogs = providers
            .iter()
            .map(|(name, provider)| {
                let models = provider
                    .list_models_dyn()
                    .into_iter()
                    .map(|model| model.id)
                    .collect();
                (name.clone(), models)
            })
            .collect();

        Self {
            providers,
            max_retries: cfg.max_retries_per_provider,
            cache: Arc::new(PromptCache::new(cfg.cache_ttl_secs)),
            policy,
            policy_client,
            budget,
            residency,
            defaults: ProviderDefaults::from_config(cfg),
            deployed,
            catalogs,
        }
    }

    /// Create a fallback chain for testing with explicit providers. The intent
    /// layer is off by default here (the seed policy has `enabled = false`) so
    /// chain tests exercise raw model routing; enable it explicitly with
    /// [`FallbackChain::with_intent_enabled`].
    #[allow(dead_code)]
    pub fn new_with_providers(providers: Vec<(String, BoxedProvider)>, max_retries: u32) -> Self {
        let seed = RoutingPolicy {
            enabled: false,
            ..RoutingPolicy::default()
        };
        // Snapshot catalogs the same way `from_config` does, so an exclusive-catalog
        // provider behaves identically under test and in production.
        let catalogs = providers
            .iter()
            .map(|(name, provider)| {
                let models = provider
                    .list_models_dyn()
                    .into_iter()
                    .map(|model| model.id)
                    .collect();
                (name.clone(), models)
            })
            .collect();
        Self {
            catalogs,
            providers,
            max_retries,
            cache: Arc::new(PromptCache::new(300)),
            policy: Arc::new(ArcSwap::from_pointee(seed)),
            policy_client: None,
            budget: None,
            // Tests construct an explicit chain with no Azure deployment; the
            // residency gate is exercised via dedicated unit tests below and the
            // request region. Default = deny-by-default (allow_non_eu = false).
            residency: EmbeddingResidency::default(),
            defaults: ProviderDefaults::default(),
            // No configured catalog, so nothing is known to be undeployed and
            // the ladder is walked as written.
            deployed: DeployedModels::default(),
        }
    }

    /// Toggle the Verevon intent layer by flipping `enabled` on the live policy
    /// (test/builder helper).
    #[allow(dead_code)]
    #[must_use]
    pub fn with_intent_enabled(self, on: bool) -> Self {
        let mut policy = RoutingPolicy::clone(&self.policy.load_full());
        policy.enabled = on;
        self.policy.store(Arc::new(policy));
        self
    }

    /// Handle to the live policy (read by the HTTP GET and updated by the PUT
    /// write-through).
    #[must_use]
    pub fn policy_handle(&self) -> Arc<ArcSwap<RoutingPolicy>> {
        self.policy.clone()
    }

    /// The session-core policy client, when configured. `None` when
    /// `SESSION_CORE_URL`/`_ADDR` is unset — the HTTP PUT then returns 503.
    #[must_use]
    pub fn policy_client(&self) -> Option<Arc<PolicyClient>> {
        self.policy_client.clone()
    }

    /// Resolve a Verevon intent model id (`verevon-budget`/`-balance`/`-genius`)
    /// to a concrete model, returning a rewritten request. Returns `None` when
    /// the intent layer is off or the model is a pinned id (pass through).
    async fn resolve_intent(&self, req: &InferRequest) -> Option<InferRequest> {
        let policy = self.policy.load();
        if !policy.enabled {
            return None;
        }
        let decision = intent::resolve(
            &policy,
            &req.model,
            &req.messages,
            &req.tools,
            &req.tool_choice,
            &req.org_id,
            &req.user_id,
            // The caller's own verified token — the budget check authenticates
            // as the caller (cost-core pins org/user to the token's claims).
            req.caller_bearer.as_str(),
            self.budget.as_deref(),
        )
        .await?;
        info!(
            request_id = %req.request_id,
            mode = decision.mode.as_str(),
            complexity = decision.complexity.as_str(),
            posture = ?decision.posture,
            resolved_model = %decision.model,
            "verevon intent resolved"
        );
        let mut rewritten = req.clone();
        rewritten.model = decision.model;
        Some(rewritten)
    }

    #[allow(dead_code)]
    /// Total number of registered providers.
    #[must_use]
    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }

    /// Whether `hint` addresses the provider registered as `name`.
    ///
    /// Aliases now come from the provider's own `capabilities().aliases` rather
    /// than a fixed table here. The table hardcoded that `openai` and `azure` mean
    /// `azure-openai` and that `anthropic` means `azure-anthropic`, so a new
    /// provider was unaddressable until someone extended it — and any new
    /// OpenAI-compatible provider would have been silently captured by the
    /// `openai` alias meant for Azure.
    ///
    /// Normalisation is retained: lowercase, and `_` treated as `-`. Phase 3
    /// B-spike root cause was a caller sending `azure_openai` (underscore) while
    /// the registry id is `azure-openai`, matching zero providers and yielding
    /// `AllExhausted(0)` with no server log.
    fn hint_matches(hint: &str, name: &str, aliases: &[String]) -> bool {
        let normalise = |value: &str| value.trim().to_ascii_lowercase().replace('_', "-");
        let hint = normalise(hint);
        if hint.is_empty() {
            return true;
        }
        let name = normalise(name);
        hint == name || aliases.iter().any(|alias| normalise(alias) == hint)
    }

    /// Default chat model for a registered provider, used when the request
    /// leaves the model unspecified ("Verevon Auto"). The Azure entries come from
    /// the operator's configured deployment catalog — see [`ProviderDefaults`].
    fn default_model_for(&self, provider_name: &str) -> &str {
        match provider_name {
            "azure-openai" => &self.defaults.azure_openai,
            "azure-anthropic" => &self.defaults.azure_anthropic,
            "anthropic" => super::anthropic::DEFAULT_ANTHROPIC_MODEL,
            _ => super::openai::DEFAULT_OPENAI_MODEL,
        }
    }

    /// This provider's boot-snapshotted catalog, if it has one.
    fn catalog_for(&self, provider_name: &str) -> Option<&[String]> {
        self.catalogs.get(provider_name).map(Vec::as_slice)
    }

    /// Whether this deployment is known to be able to serve `model`. Unknown
    /// catalogs answer `true` — see [`DeployedModels`].
    fn is_deployed(&self, model: &str) -> bool {
        let catalog = if is_anthropic_model(model) {
            self.deployed.anthropic.as_ref()
        } else {
            self.deployed.openai.as_ref()
        };
        match catalog {
            Some(names) => names.iter().any(|name| name.eq_ignore_ascii_case(model)),
            None => true,
        }
    }

    /// The ordered tool-capable models to try after `resolved`, bounded by
    /// [`MAX_TOOL_LADDER_STEPS`] and pruned of rungs this resource cannot serve.
    ///
    /// Callers must only pass a model the intent layer resolved from a
    /// `verevon-*` mode. Substituting under a caller who pinned a model would
    /// answer with something they did not ask for; "the model I chose was busy"
    /// is their decision to make, not ours to paper over.
    fn tool_ladder(&self, resolved: &str) -> Vec<String> {
        let policy = self.policy.load();
        let mut ladder: Vec<String> = Vec::new();
        for candidate in &policy.tool_fallback_ladder {
            if ladder.len() >= MAX_TOOL_LADDER_STEPS {
                break;
            }
            let already_queued = candidate.eq_ignore_ascii_case(resolved)
                || ladder.iter().any(|m| m.eq_ignore_ascii_case(candidate));
            if already_queued || !self.is_deployed(candidate) {
                continue;
            }
            ladder.push(candidate.clone());
        }
        ladder
    }

    /// Perform unary inference with fallback and caching.
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every provider and retry is exhausted.
    pub async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
        // Verevon intent layer: resolve a `verevon-*` mode to a concrete model
        // (complexity + budget) before anything else. A pinned model or a
        // disabled intent layer leaves `req` untouched.
        let intent_req = self.resolve_intent(req).await;
        // The ladder exists only for a model WE chose. `intent_req.is_some()` is
        // exactly that condition — see `attempt_models`.
        let ladder = intent_req
            .as_ref()
            .map_or_else(Vec::new, |resolved| self.tool_ladder(&resolved.model));
        let req: &InferRequest = intent_req.as_ref().unwrap_or(req);

        // Check cache first
        if let Some(cached) = self.cache.get(req) {
            info!(request_id = %req.request_id, "cache hit");
            return Ok(cached);
        }

        let mut total_attempts: u32 = 0;
        let mut throttle = ThrottleState::default();

        // Two passes at most: walk every candidate model across every provider,
        // and if the only thing standing in the way was throttling with a short
        // enough retry-after, honor it once and walk them again. A second
        // throttle ends it — retrying a rate limit indefinitely is how one
        // throttled tenant becomes a stuck queue.
        for pass in 0..2u8 {
            if pass == 1 {
                let Some(wait) = throttle.affordable_wait() else {
                    break;
                };
                warn!(
                    request_id = %req.request_id,
                    wait_ms = wait.as_millis(),
                    "every matching provider was rate limited; honoring retry-after once"
                );
                tokio::time::sleep(wait).await;
                throttle = ThrottleState::default();
            }

            for model in Self::attempt_models(&req.model, &ladder) {
                if model != req.model {
                    warn!(
                        request_id = %req.request_id,
                        resolved_model = %req.model,
                        fallback_model = %model,
                        "resolved model could not be served; trying the next tool-capable model"
                    );
                }
                if let Some(response) = self
                    .infer_one_model(req, model, &mut total_attempts, &mut throttle)
                    .await
                {
                    self.cache.put(req, &response);
                    return Ok(response);
                }
            }

            if !throttle.throttled() {
                // Nothing was throttled, so waiting cannot help.
                break;
            }
        }

        if req.min_privacy_tier > PrivacyTier::Unspecified && total_attempts == 0 {
            Err(tier_unavailable_error(req.min_privacy_tier))
        } else if req.zdr && total_attempts == 0 {
            Err(ProviderError::ZdrUnavailable(
                "no matching provider deployment has verified ZDR support".to_owned(),
            ))
        } else {
            Err(throttle.exhausted_error(total_attempts))
        }
    }

    /// The models one pass may try, in order: the request's own model first,
    /// then the tool ladder. Deduplicated, so no model is asked twice in a pass.
    ///
    /// `ladder` is empty unless the intent layer resolved the model, which is
    /// what keeps a pinned model from being silently substituted.
    ///
    /// Deduplication is per pass rather than per request on purpose: the second
    /// pass is the deliberate, once-only retry after an affordable retry-after,
    /// and re-asking the same models is the whole point of it.
    fn attempt_models<'a>(model: &'a str, ladder: &'a [String]) -> Vec<&'a str> {
        let mut models = vec![model];
        for rung in ladder {
            if !models.iter().any(|m| m.eq_ignore_ascii_case(rung)) {
                models.push(rung.as_str());
            }
        }
        models
    }

    /// Try `model` against every provider that can serve it, with the configured
    /// per-provider retries. `None` means nothing served it; throttling and the
    /// attempt count accumulate into the caller's state so an exhausted chain
    /// still reports `RateLimited` rather than generic exhaustion.
    async fn infer_one_model(
        &self,
        req: &InferRequest,
        model: &str,
        total_attempts: &mut u32,
        throttle: &mut ThrottleState,
    ) -> Option<InferResponse> {
        for (name, provider) in &self.providers {
            // One capabilities() read per provider, reused by all three gates
            // below: it allocates, and the ZDR gate already paid for it.
            let caps = provider.capabilities_dyn();
            if !Self::hint_matches(&req.provider_hint, name, &caps.aliases) {
                continue;
            }
            // Skip providers that cannot serve the requested model family — a
            // `claude-*` model must not hit the OpenAI surface (it would 404 the
            // deployment) and vice-versa. Unspecified models pass (they resolve
            // to the provider's default below).
            if !provider_serves_model(&caps, self.catalog_for(name), model) {
                continue;
            }
            if req.zdr && !caps.supports_zdr {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "provider skipped: ZDR was required but is not verified for this deployment"
                );
                continue;
            }
            // Privacy-tier gate: a provider below the requested minimum is
            // skipped before any network call, mirroring the ZDR gate above.
            if !provider_meets_privacy_tier(&caps, req.min_privacy_tier) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    required_tier = req.min_privacy_tier.label(),
                    provider_tier = PrivacyTier::classify(&caps).label(),
                    "provider skipped: privacy tier below the requested minimum"
                );
                continue;
            }
            let call_req = self.request_for(req, model, name);
            for attempt in 1..=self.max_retries {
                *total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let _enter = span.enter();

                match provider.infer_dyn(call_req.as_ref()).await {
                    Ok(mut response) => {
                        info!(
                            provider = %name,
                            attempt = attempt,
                            model_used = %call_req.model,
                            provider_reported_model = %response.model_used,
                            "infer succeeded"
                        );
                        // Report the id we REQUESTED, not the id the provider
                        // echoed. Azure answers with the versioned snapshot
                        // (`gpt-4o-mini-2024-07-18`) — a response-namespace id
                        // that is not a deployment name. Callers reuse
                        // `model_used` in follow-up requests (the chat answer
                        // inherits the tool phase's model), and feeding the
                        // snapshot id back produced a guaranteed 404
                        // DeploymentNotFound. The provider-reported id stays in
                        // the log line above for traceability.
                        call_req.model.clone_into(&mut response.model_used);
                        // Stamp serving provenance (Phase-4 receipt inputs):
                        // which deployment processed this content and under
                        // what declared residency.
                        response.provider_used.clone_from(name);
                        caps.residency.as_str().clone_into(&mut response.residency);
                        return Some(response);
                    }
                    Err(ProviderError::RateLimited { retry_after_ms }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            retry_after_ms = retry_after_ms,
                            "rate limited, moving to next provider"
                        );
                        throttle.record(retry_after_ms);
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
        None
    }

    /// Forward a provider's chunk stream with `model_used` rewritten to the id
    /// this chain actually requested. See the unary normalization in
    /// [`Self::infer_one_model`] for why the provider-reported id must not
    /// escape: it is a response-namespace id (Azure's versioned snapshot) that
    /// 404s when reused as a deployment name. Chunks with an empty `model_used`
    /// pass through untouched so the "which chunks carry an id" signal is
    /// preserved.
    fn normalize_stream_model(
        mut rx: mpsc::Receiver<InferChunk>,
        requested_model: String,
    ) -> mpsc::Receiver<InferChunk> {
        let (tx, out_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(mut chunk) = rx.recv().await {
                if !chunk.model_used.is_empty() {
                    requested_model.clone_into(&mut chunk.model_used);
                }
                if tx.send(chunk).await.is_err() {
                    // Consumer hung up; dropping rx cancels the upstream too.
                    break;
                }
            }
        });
        out_rx
    }

    /// The request to send for `model` on `provider_name`.
    ///
    /// An unspecified model ("Verevon Auto") becomes that provider's configured
    /// default so an unpinned request works against whatever is deployed; a
    /// ladder rung replaces the model, and `model_used` is then normalized to
    /// this effective id on the way out (see [`Self::infer_one_model`]). Borrows
    /// the caller's request unchanged whenever no substitution is needed, so the
    /// common path still costs no clone.
    fn request_for<'a>(
        &self,
        req: &'a InferRequest,
        model: &str,
        provider_name: &str,
    ) -> std::borrow::Cow<'a, InferRequest> {
        let effective = if is_unspecified_model(model) {
            self.default_model_for(provider_name)
        } else {
            model
        };
        if effective == req.model {
            return std::borrow::Cow::Borrowed(req);
        }
        let mut rewritten = req.clone();
        effective.clone_into(&mut rewritten.model);
        std::borrow::Cow::Owned(rewritten)
    }

    /// Fill serving-provider provenance on every chunk of a stream. Final
    /// chunks carry it verbatim; non-final chunks keep empty fields so callers
    /// reading only terminal frames still see the truth about who served the
    /// turn.
    fn stamp_stream_provenance(
        mut rx: mpsc::Receiver<InferChunk>,
        provider_used: String,
        residency: String,
    ) -> mpsc::Receiver<InferChunk> {
        let (tx, out_rx) = mpsc::channel(64);
        tokio::spawn(async move {
            while let Some(mut chunk) = rx.recv().await {
                if chunk.done || chunk.provider_used.is_empty() {
                    chunk.provider_used.clone_from(&provider_used);
                    chunk.residency.clone_from(&residency);
                }
                let done = chunk.done;
                if tx.send(chunk).await.is_err() {
                    break;
                }
                if done {
                    break;
                }
            }
        });
        out_rx
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
        // Verevon intent layer — same resolution as the unary path.
        let intent_req = self.resolve_intent(req).await;
        let ladder = intent_req
            .as_ref()
            .map_or_else(Vec::new, |resolved| self.tool_ladder(&resolved.model));
        let req: &InferRequest = intent_req.as_ref().unwrap_or(req);

        let mut total_attempts: u32 = 0;
        let mut throttle = ThrottleState::default();

        // Same two-pass shape as the unary path — see the comment there. This is
        // the path plain chat streams through, so it is the one that turned a
        // recoverable 429 into a failed turn.
        for pass in 0..2u8 {
            if pass == 1 {
                let Some(wait) = throttle.affordable_wait() else {
                    break;
                };
                warn!(
                    request_id = %req.request_id,
                    wait_ms = wait.as_millis(),
                    "every matching streaming provider was rate limited; honoring retry-after once"
                );
                tokio::time::sleep(wait).await;
                throttle = ThrottleState::default();
            }

            for model in Self::attempt_models(&req.model, &ladder) {
                if model != req.model {
                    warn!(
                        request_id = %req.request_id,
                        resolved_model = %req.model,
                        fallback_model = %model,
                        "resolved model could not be streamed; trying the next tool-capable model"
                    );
                }
                if let Some(rx) = self
                    .stream_one_model(req, model, &mut total_attempts, &mut throttle)
                    .await
                {
                    return Ok(rx);
                }
            }

            if !throttle.throttled() {
                break;
            }
        }

        if req.min_privacy_tier > PrivacyTier::Unspecified && total_attempts == 0 {
            Err(tier_unavailable_error(req.min_privacy_tier))
        } else if req.zdr && total_attempts == 0 {
            Err(ProviderError::ZdrUnavailable(
                "no matching streaming provider deployment has verified ZDR support".to_owned(),
            ))
        } else {
            Err(throttle.exhausted_error(total_attempts))
        }
    }

    /// Streaming twin of [`FallbackChain::infer_one_model`].
    async fn stream_one_model(
        &self,
        req: &InferRequest,
        model: &str,
        total_attempts: &mut u32,
        throttle: &mut ThrottleState,
    ) -> Option<mpsc::Receiver<InferChunk>> {
        for (name, provider) in &self.providers {
            let caps = provider.capabilities_dyn();
            if !Self::hint_matches(&req.provider_hint, name, &caps.aliases) {
                continue;
            }
            if !provider_serves_model(&caps, self.catalog_for(name), model) {
                continue;
            }
            if req.zdr && !caps.supports_zdr {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "stream provider skipped: ZDR was required but is not verified for this deployment"
                );
                continue;
            }
            // Privacy-tier gate on the streaming path, same as unary.
            if !provider_meets_privacy_tier(&caps, req.min_privacy_tier) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    required_tier = req.min_privacy_tier.label(),
                    provider_tier = PrivacyTier::classify(&caps).label(),
                    "stream provider skipped: privacy tier below the requested minimum"
                );
                continue;
            }
            let call_req = self.request_for(req, model, name);
            for attempt in 1..=self.max_retries {
                *total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_stream_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let _enter = span.enter();

                match provider.infer_stream_dyn(call_req.as_ref()).await {
                    Ok(rx) => {
                        info!(
                            provider = %name,
                            attempt = attempt,
                            model_used = %call_req.model,
                            "infer_stream started"
                        );
                        // Same normalization as the unary path: chunks carry the
                        // provider's response-namespace id (Azure's versioned
                        // snapshot), which downstream must never reuse as a
                        // request model. Rewrite in-flight, preserving WHICH
                        // chunks carry an id — only the value is normalized.
                        // Provenance is stamped alongside so every final chunk
                        // carries the serving deployment + declared residency.
                        let residency = caps.residency.as_str().to_owned();
                        let provider_name = name.clone();
                        let rx = Self::normalize_stream_model(rx, call_req.model.clone());
                        return Some(Self::stamp_stream_provenance(
                            rx,
                            provider_name,
                            residency,
                        ));
                    }
                    Err(ProviderError::RateLimited { retry_after_ms }) => {
                        warn!(
                            provider = %name,
                            attempt = attempt,
                            retry_after_ms = retry_after_ms,
                            "rate limited, moving to next provider"
                        );
                        throttle.record(retry_after_ms);
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
        None
    }

    /// Create an embedding with provider fallback.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::ResidencyViolation`] when the EU residency gate
    /// rejects the request (deny-by-default, before any network call), or
    /// [`ProviderError::AllExhausted`] if every matching provider fails.
    pub async fn create_embedding(
        &self,
        req: &EmbedRequest,
    ) -> Result<EmbedResponse, ProviderError> {
        self.reject_non_eu_embedding_region(req)?;
        let mut total_attempts: u32 = 0;

        for (name, provider) in &self.providers {
            let caps = provider.capabilities_dyn();
            if !Self::hint_matches(&req.provider_hint, name, &caps.aliases) {
                continue;
            }
            if req.zdr && !caps.supports_zdr {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "embedding provider skipped: ZDR was required but is not verified for this deployment"
                );
                continue;
            }
            // Privacy-tier gate on the embedding path. Embeddings carry no tier
            // field on today's wire contract; honoring the chat contract's
            // semantics keeps all three chain paths consistent.
            if !provider_meets_privacy_tier(&caps, req.min_privacy_tier) {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    required_tier = req.min_privacy_tier.label(),
                    provider_tier = PrivacyTier::classify(&caps).label(),
                    "embedding provider skipped: privacy tier below the requested minimum"
                );
                continue;
            }
            for attempt in 1..=self.max_retries {
                total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_embedding_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                    // ZDR is recorded on the embedding-attempt span so the
                    // retention posture of each provider hop is observable.
                    // inference-core's provider is Azure today, so residency
                    // enforcement is Phase-4; this carries the signal end-to-end
                    // (it does NOT by itself satisfy residency).
                    zdr = req.zdr,
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

        if req.min_privacy_tier > PrivacyTier::Unspecified && total_attempts == 0 {
            Err(tier_unavailable_error(req.min_privacy_tier))
        } else if req.zdr && total_attempts == 0 {
            Err(ProviderError::ZdrUnavailable(
                "no matching embedding provider deployment has verified ZDR support".to_owned(),
            ))
        } else {
            Err(ProviderError::AllExhausted {
                attempts: total_attempts,
            })
        }
    }

    /// Return models from every registered provider, optionally filtered.
    #[must_use]
    pub fn list_models(&self, modality: &str, provider: &str) -> Vec<ModelInfo> {
        self.providers
            .iter()
            .filter(|(name, candidate)| {
                Self::hint_matches(provider, name, &candidate.capabilities_dyn().aliases)
            })
            .flat_map(|(_, provider)| provider.list_models_dyn())
            .filter(|model| modality.is_empty() || model.modality == modality)
            .collect()
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn detects_unspecified_models() {
        for m in ["", "   ", "default", "AUTO", "Verevon", "verevon-auto"] {
            assert!(is_unspecified_model(m), "{m:?} should be unspecified");
        }
        for m in ["claude-sonnet-4-20250514", "gpt-4o-mini"] {
            assert!(!is_unspecified_model(m), "{m:?} should be specified");
        }
    }

    #[test]
    fn per_provider_defaults() {
        let chain = FallbackChain::new_with_providers(Vec::new(), 1);
        assert_eq!(
            chain.default_model_for("anthropic"),
            crate::provider::anthropic::DEFAULT_ANTHROPIC_MODEL
        );
        assert_eq!(
            chain.default_model_for("openai"),
            crate::provider::openai::DEFAULT_OPENAI_MODEL
        );
        // With no configured catalog, Azure OpenAI still falls back to the
        // cost-optimizing model-router.
        assert_eq!(chain.default_model_for("azure-openai"), AZURE_MODEL_ROUTER);
        assert_eq!(chain.default_model_for("azure-openai"), "model-router");
        // Azure Anthropic defaults to the cheapest Claude deployment.
        assert_eq!(
            chain.default_model_for("azure-anthropic"),
            crate::provider::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL
        );
    }

    #[test]
    fn azure_default_honors_the_configured_deployment() {
        // The live 404: the operator's only chat deployment is `gpt-4o-mini`,
        // and the chain asked their resource for `model-router` — a deployment
        // that does not exist there — so every unspecified-model request came
        // back DeploymentNotFound.
        assert_eq!(
            azure_default_deployment(&["gpt-4o-mini".to_owned()], AZURE_MODEL_ROUTER),
            "gpt-4o-mini"
        );
        // A resource that really does host the auto-router keeps getting it,
        // wherever the operator listed it.
        assert_eq!(
            azure_default_deployment(
                &["gpt-4o-mini".to_owned(), "model-router".to_owned()],
                AZURE_MODEL_ROUTER
            ),
            "model-router"
        );
        // Nothing configured → the legacy constant, not a guess.
        assert_eq!(
            azure_default_deployment(&[], AZURE_MODEL_ROUTER),
            AZURE_MODEL_ROUTER
        );
        // Same latent mismatch on the Claude resource: a catalog without haiku
        // must not default to a deployment that isn't there.
        assert_eq!(
            azure_default_deployment(
                &["claude-sonnet-4-6".to_owned(), "claude-opus-4-8".to_owned()],
                crate::provider::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL
            ),
            "claude-sonnet-4-6"
        );
    }

    /// Capabilities for a provider of `family`, as the real providers declare
    /// them. Routing is now driven by these declarations rather than by matching
    /// the registry-name string, so the tests below assert on declarations too.
    fn caps_for(family: ModelFamily, aliases: &[&str]) -> ProviderCapabilities {
        ProviderCapabilities {
            model_family: family,
            aliases: aliases.iter().map(|a| (*a).to_owned()).collect(),
            residency: Residency::Eu,
            ..ProviderCapabilities::default()
        }
    }

    fn anthropic_caps() -> ProviderCapabilities {
        caps_for(ModelFamily::Anthropic, &["claude", "anthropic"])
    }

    fn openai_caps() -> ProviderCapabilities {
        caps_for(ModelFamily::OpenAiCompatible, &["openai", "azure"])
    }

    #[test]
    fn model_family_gating() {
        // Claude models only on the Anthropic-family providers.
        assert!(provider_serves_model(
            &anthropic_caps(),
            None,
            "claude-haiku-4-5"
        ));
        assert!(provider_serves_model(
            &anthropic_caps(),
            None,
            "claude-opus-4-8"
        ));
        assert!(!provider_serves_model(
            &openai_caps(),
            None,
            "claude-haiku-4-5"
        ));
        assert!(!provider_serves_model(
            &openai_caps(),
            None,
            "claude-sonnet-4-6"
        ));

        // Non-Claude models only on the OpenAI-compatible providers.
        assert!(provider_serves_model(&openai_caps(), None, "gpt-4o-mini"));
        assert!(provider_serves_model(&openai_caps(), None, "model-router"));
        assert!(provider_serves_model(&openai_caps(), None, "deepseek-v3-2"));
        assert!(!provider_serves_model(
            &anthropic_caps(),
            None,
            "gpt-4o-mini"
        ));
        assert!(!provider_serves_model(
            &anthropic_caps(),
            None,
            "model-router"
        ));

        // Unspecified models pass on every provider (resolve to its default).
        for caps in [anthropic_caps(), openai_caps()] {
            assert!(provider_serves_model(&caps, None, ""));
            assert!(provider_serves_model(&caps, None, "verevon-auto"));
        }
    }

    /// A sovereign provider must serve only what it actually hosts. Routing an
    /// unrecognised model to one either 404s or gets silently served from a
    /// brokered upstream outside the residency boundary the tier was sold on —
    /// and the second failure mode looks like success.
    #[test]
    fn exclusive_catalog_provider_serves_only_its_own_models() {
        let caps = ProviderCapabilities {
            model_family: ModelFamily::OpenAiCompatible,
            residency: Residency::Norway,
            exclusive_catalog: true,
            ..ProviderCapabilities::default()
        };
        let catalog = vec!["lynx-instruct-30b".to_owned(), "norskgpt-8b".to_owned()];

        assert!(provider_serves_model(
            &caps,
            Some(&catalog),
            "lynx-instruct-30b"
        ));
        // Case-insensitive, matching the non-exclusive catalog check.
        assert!(provider_serves_model(
            &caps,
            Some(&catalog),
            "LYNX-INSTRUCT-30B"
        ));
        // In-family but not hosted here: must NOT be accepted.
        assert!(!provider_serves_model(&caps, Some(&catalog), "gpt-4o-mini"));
        // Out-of-family stays refused by the family gate.
        assert!(!provider_serves_model(
            &caps,
            Some(&catalog),
            "claude-haiku-4-5"
        ));
    }

    /// An exclusive provider with no catalog must go quiet rather than become a
    /// wildcard — a misconfigured sovereign endpoint absorbing every model is the
    /// worst possible failure here.
    #[test]
    fn exclusive_catalog_with_no_catalog_serves_nothing() {
        let caps = ProviderCapabilities {
            model_family: ModelFamily::OpenAiCompatible,
            residency: Residency::Norway,
            exclusive_catalog: true,
            ..ProviderCapabilities::default()
        };
        assert!(!provider_serves_model(&caps, None, "gpt-4o-mini"));
        assert!(!provider_serves_model(&caps, Some(&[]), "gpt-4o-mini"));
        // An unspecified model still resolves to the provider's own default.
        assert!(provider_serves_model(&caps, None, ""));
    }

    #[test]
    fn anthropic_hint_matches_azure_anthropic() {
        let aliases = anthropic_caps().aliases;
        assert!(FallbackChain::hint_matches(
            "anthropic",
            "azure-anthropic",
            &aliases
        ));
        assert!(FallbackChain::hint_matches(
            "claude",
            "azure-anthropic",
            &aliases
        ));
        assert!(FallbackChain::hint_matches("claude", "anthropic", &aliases));
        assert!(FallbackChain::hint_matches(
            "azure",
            "azure-openai",
            &openai_caps().aliases
        ));
        // A claude hint must not match the OpenAI surface.
        assert!(!FallbackChain::hint_matches(
            "claude",
            "azure-openai",
            &openai_caps().aliases
        ));
    }

    #[test]
    fn provider_hint_underscore_matches_hyphen_id() {
        // Phase 3 B-spike regression: a caller sending `azure_openai` (underscore)
        // must match the `azure-openai` provider id. Before normalisation this
        // matched zero providers → AllExhausted(0) with no server log.
        let openai = openai_caps().aliases;
        let anthropic = anthropic_caps().aliases;
        assert!(FallbackChain::hint_matches(
            "azure_openai",
            "azure-openai",
            &openai
        ));
        assert!(FallbackChain::hint_matches(
            "AZURE_OPENAI",
            "azure-openai",
            &openai
        ));
        assert!(FallbackChain::hint_matches(
            "azure_anthropic",
            "azure-anthropic",
            &anthropic
        ));
        // Hyphen/underscore equivalence must not over-match across surfaces.
        assert!(!FallbackChain::hint_matches(
            "claude",
            "azure-openai",
            &openai
        ));
    }

    /// A provider declaring no aliases is addressable only by its own id. This is
    /// what stops a newly added OpenAI-compatible provider from being captured by
    /// the `openai`/`azure` aliases that the Azure deployment claims — the exact
    /// collision that made a second such provider impossible before.
    #[test]
    fn custom_provider_is_addressable_only_by_its_own_id() {
        let no_aliases: Vec<String> = Vec::new();
        assert!(FallbackChain::hint_matches(
            "bineric",
            "bineric",
            &no_aliases
        ));
        assert!(FallbackChain::hint_matches("", "bineric", &no_aliases));
        // The Azure aliases must not reach it.
        assert!(!FallbackChain::hint_matches(
            "openai",
            "bineric",
            &no_aliases
        ));
        assert!(!FallbackChain::hint_matches(
            "azure",
            "bineric",
            &no_aliases
        ));
        // And its id must not reach Azure.
        assert!(!FallbackChain::hint_matches(
            "bineric",
            "azure-openai",
            &openai_caps().aliases
        ));
    }

    /// Residency is ordered so a request can express a minimum with `>=`, and so
    /// the registration gate can reject "no commitment" with one comparison.
    #[test]
    fn residency_is_ordered_weakest_to_strongest() {
        assert!(Residency::Norway > Residency::Eu);
        assert!(Residency::Eu > Residency::Global);
        assert_eq!(Residency::default(), Residency::Global);
    }

    #[test]
    fn residency_parses_operator_tokens_and_rejects_typos() {
        assert_eq!(Residency::parse("norway"), Some(Residency::Norway));
        assert_eq!(Residency::parse("SOVEREIGN"), Some(Residency::Norway));
        assert_eq!(Residency::parse(" eu "), Some(Residency::Eu));
        assert_eq!(Residency::parse("eu_resident"), Some(Residency::Eu));
        assert_eq!(Residency::parse("global"), Some(Residency::Global));
        // A typo must not silently become "no commitment".
        assert_eq!(Residency::parse("noway"), None);
        assert_eq!(Residency::parse(""), None);
    }

    /// Records the model it was invoked with so tests can assert resolution.
    struct RecordingProvider {
        seen_model: Arc<Mutex<Option<String>>>,
        zdr_supported: bool,
        /// Declared residency; defaults to EU so the pre-existing tests exercise
        /// routing, never the residency registration posture.
        residency: Residency,
        /// Which family this double stands in for.
        ///
        /// Routing reads the family from the provider's own declaration rather
        /// than from its registry name, so a double registered as
        /// `azure-anthropic` must *say* it is Anthropic-family or the chain will
        /// correctly refuse every `claude-*` model. `Default` gives
        /// `OpenAiCompatible`, so Claude-serving doubles set this explicitly.
        family: ModelFamily,
    }

    impl Default for RecordingProvider {
        fn default() -> Self {
            Self {
                seen_model: Arc::new(Mutex::new(None)),
                zdr_supported: false,
                residency: Residency::Eu,
                family: ModelFamily::OpenAiCompatible,
            }
        }
    }

    #[async_trait::async_trait]
    impl ProviderRouter for RecordingProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                supports_zdr: self.zdr_supported,
                model_family: self.family,
                residency: self.residency,
                ..crate::provider::ProviderCapabilities::default()
            }
        }

        async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
            *self.seen_model.lock().unwrap() = Some(req.model.clone());
            Ok(InferResponse {
                request_id: req.request_id.clone(),
                content: "ok".to_owned(),
                model_used: req.model.clone(),
                stop_reason: "stop".to_owned(),
                input_tokens: 0,
                output_tokens: 0,
                tool_calls: Vec::new(),
                ..InferResponse::default()
            })
        }

        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    fn chain_with(seen: Arc<Mutex<Option<String>>>) -> FallbackChain {
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen,
            zdr_supported: false,
            residency: Residency::Eu,
            family: ModelFamily::Anthropic,
        });
        FallbackChain::new_with_providers(vec![("anthropic".to_owned(), provider)], 1)
    }

    #[tokio::test]
    async fn resolves_unspecified_model_to_provider_default() {
        let seen = Arc::new(Mutex::new(None));
        let chain = chain_with(seen.clone());
        let req = InferRequest {
            request_id: "r1".to_owned(),
            model: String::new(),
            ..Default::default()
        };
        let resp = chain.infer(&req).await.unwrap();
        assert_eq!(
            seen.lock().unwrap().as_deref(),
            Some(crate::provider::anthropic::DEFAULT_ANTHROPIC_MODEL)
        );
        // model_used reflects the resolved model, not the empty request.
        assert_eq!(
            resp.model_used,
            crate::provider::anthropic::DEFAULT_ANTHROPIC_MODEL
        );
    }

    #[tokio::test]
    async fn preserves_explicitly_pinned_model() {
        let seen = Arc::new(Mutex::new(None));
        let chain = chain_with(seen.clone());
        let req = InferRequest {
            request_id: "r2".to_owned(),
            model: "claude-opus-4-20250514".to_owned(),
            ..Default::default()
        };
        chain.infer(&req).await.unwrap();
        assert_eq!(
            seen.lock().unwrap().as_deref(),
            Some("claude-opus-4-20250514")
        );
    }

    #[tokio::test]
    async fn verevon_mode_resolves_to_concrete_model_through_the_chain() {
        // Provider registered as azure-openai (serves non-claude models); intent
        // layer on, no budget client → Unknown posture. verevon-budget + a trivial
        // prompt → Budget/Simple → the current policy table's gpt-5-nano
        // reaches the provider. This assertion must follow the versioned table
        // rather than the older cheap-fallback constant.
        let seen = Arc::new(Mutex::new(None));
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            zdr_supported: false,
            ..RecordingProvider::default()
        });
        let chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1)
                .with_intent_enabled(true);
        let req = InferRequest {
            request_id: "r3".to_owned(),
            model: "verevon-budget".to_owned(),
            messages: vec![crate::provider::ChatMessage {
                role: "user".to_owned(),
                content: "hi".to_owned(),
                name: String::new(),
            }],
            ..Default::default()
        };
        chain.infer(&req).await.unwrap();
        assert_eq!(seen.lock().unwrap().as_deref(), Some("gpt-5-nano"));
    }

    #[tokio::test]
    async fn intent_disabled_falls_back_to_provider_default() {
        // With the intent layer off, a leaked verevon-* id is still treated as
        // unspecified (is_unspecified_model), so it resolves to the provider
        // default — model-router for azure-openai — rather than being sent
        // verbatim (which would 404 the deployment).
        let seen = Arc::new(Mutex::new(None));
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            zdr_supported: false,
            ..RecordingProvider::default()
        });
        // new_with_providers defaults intent_enabled = false.
        let chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1);
        let req = InferRequest {
            request_id: "r4".to_owned(),
            model: "verevon-genius".to_owned(),
            ..Default::default()
        };
        chain.infer(&req).await.unwrap();
        assert_eq!(seen.lock().unwrap().as_deref(), Some(AZURE_MODEL_ROUTER));
    }

    #[tokio::test]
    async fn zdr_skips_unverified_provider_and_fails_closed_without_compliant_route() {
        let unverified_seen = Arc::new(Mutex::new(None));
        let verified_seen = Arc::new(Mutex::new(None));
        let unverified: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: unverified_seen.clone(),
            zdr_supported: false,
            ..RecordingProvider::default()
        });
        let verified: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: verified_seen.clone(),
            zdr_supported: true,
            ..RecordingProvider::default()
        });
        let chain = FallbackChain::new_with_providers(
            vec![
                ("openai".to_owned(), unverified),
                ("azure-openai".to_owned(), verified),
            ],
            1,
        );
        let req = InferRequest {
            request_id: "zdr-1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            zdr: true,
            ..Default::default()
        };

        chain.infer(&req).await.expect("verified ZDR provider");
        assert!(unverified_seen.lock().unwrap().is_none());
        assert!(verified_seen.lock().unwrap().is_some());

        let unavailable_seen = Arc::new(Mutex::new(None));
        let unavailable: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: unavailable_seen.clone(),
            zdr_supported: false,
            ..RecordingProvider::default()
        });
        let unavailable_chain =
            FallbackChain::new_with_providers(vec![("openai".to_owned(), unavailable)], 1);
        let error = unavailable_chain.infer(&req).await.unwrap_err();
        assert!(matches!(error, ProviderError::ZdrUnavailable(_)));
        assert!(unavailable_seen.lock().unwrap().is_none());
    }

    // ---------------------------------------------------------------------
    // Privacy-tier gating — one section per chain path.
    //
    // These are MUTATION tests by construction: each asserts BOTH that the
    // weaker provider was never reached AND the exact typed error when nothing
    // remains. Deleting any `provider_meets_privacy_tier` skip turns the first
    // assertion of the matching test into a failure (the weak provider serves);
    // deleting a `tier_unavailable_error` exhaustion branch turns the second
    // into a failure (generic AllExhausted instead of the named-tier error).
    // ---------------------------------------------------------------------

    /// Unary path: a Global provider is skipped in favor of an EU one, and a
    /// Global-only chain fails with the REQUIRED tier named.
    #[tokio::test]
    async fn tier_gate_skips_weaker_provider_on_unary_path_or_fails_naming_the_tier() {
        let weak_seen = Arc::new(Mutex::new(None));
        let strong_seen = Arc::new(Mutex::new(None));
        let weak: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: weak_seen.clone(),
            residency: Residency::Global,
            ..RecordingProvider::default()
        });
        let strong: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: strong_seen.clone(),
            residency: Residency::Eu,
            ..RecordingProvider::default()
        });
        let chain = FallbackChain::new_with_providers(
            vec![
                ("openai".to_owned(), weak),
                ("azure-openai".to_owned(), strong),
            ],
            1,
        );
        let req = InferRequest {
            request_id: "tier-u1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            min_privacy_tier: PrivacyTier::EuResident,
            ..Default::default()
        };

        let resp = chain.infer(&req).await.expect("the EU provider satisfies the minimum");
        assert!(weak_seen.lock().unwrap().is_none(), "a provider below the requested tier must be skipped BEFORE any call");
        assert!(strong_seen.lock().unwrap().is_some());
        // Provenance discloses the posture that was actually met.
        assert_eq!(resp.provider_used, "azure-openai");
        assert_eq!(resp.residency, "eu");

        // No provider left at the floor: typed error NAMING the required tier,
        // never a silent downgrade to the weaker provider.
        let weak_only: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: Arc::new(Mutex::new(None)),
            residency: Residency::Global,
            ..RecordingProvider::default()
        });
        let weak_chain =
            FallbackChain::new_with_providers(vec![("openai".to_owned(), weak_only)], 1);
        let err = weak_chain.infer(&req).await.unwrap_err();
        match err {
            ProviderError::TierUnavailable(message) => {
                assert!(
                    message.contains("eu_resident"),
                    "the error must NAME the required tier, got: {message}"
                );
            }
            other => panic!("expected TierUnavailable, got {other:?}"),
        }
    }

    /// An UNSPECIFIED minimum imposes no constraint: today's Global-catalog
    /// deployments keep serving exactly as before the tier system existed.
    #[tokio::test]
    async fn unspecifed_minimum_keeps_global_catalog_serving_unchanged() {
        let seen = Arc::new(Mutex::new(None));
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            residency: Residency::Global,
            ..RecordingProvider::default()
        });
        let chain =
            FallbackChain::new_with_providers(vec![("openai".to_owned(), provider)], 1);
        let req = InferRequest {
            request_id: "tier-u2".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            ..Default::default()
        };
        chain.infer(&req).await.expect("no constraint means no skipping");
        assert!(seen.lock().unwrap().is_some());
    }

    /// A streaming double with declared residency, emitting one final chunk so
    /// the provenance stamp can be asserted end-to-end through
    /// `stamp_stream_provenance`.
    struct TierStreamProvider {
        reached: Arc<Mutex<bool>>,
        residency: Residency,
    }

    #[async_trait::async_trait]
    impl ProviderRouter for TierStreamProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                residency: self.residency,
                ..crate::provider::ProviderCapabilities::default()
            }
        }

        async fn infer(&self, _req: &InferRequest) -> Result<InferResponse, ProviderError> {
            Err(ProviderError::UnsupportedModel(
                "stream double".to_owned(),
            ))
        }

        async fn infer_stream(
            &self,
            req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            *self.reached.lock().unwrap() = true;
            let (tx, rx) = mpsc::channel(1);
            let chunk = InferChunk {
                request_id: req.request_id.clone(),
                delta: String::new(),
                done: true,
                model_used: req.model.clone(),
                input_tokens: 1,
                output_tokens: 1,
                provider_used: String::new(),
                residency: String::new(),
            };
            tokio::spawn(async move {
                let _ = tx.send(chunk).await;
            });
            Ok(rx)
        }
    }

    /// Streaming path: same skip semantics, plus the final chunk carries WHO
    /// served the stream and under what declared residency.
    #[tokio::test]
    async fn tier_gate_skips_weaker_provider_on_stream_path_or_fails_naming_the_tier() {
        let weak_reached = Arc::new(Mutex::new(false));
        let strong_reached = Arc::new(Mutex::new(false));
        let weak: BoxedProvider = Arc::new(TierStreamProvider {
            reached: weak_reached.clone(),
            residency: Residency::Global,
        });
        let strong: BoxedProvider = Arc::new(TierStreamProvider {
            reached: strong_reached.clone(),
            residency: Residency::Eu,
        });
        let chain = FallbackChain::new_with_providers(
            vec![
                ("openai".to_owned(), weak),
                ("azure-openai".to_owned(), strong),
            ],
            1,
        );
        let req = InferRequest {
            request_id: "tier-s1".to_owned(),
            model: "gpt-4o-mini".to_owned(),
            min_privacy_tier: PrivacyTier::EuResident,
            ..Default::default()
        };

        let mut rx = chain
            .infer_stream(&req)
            .await
            .expect("the EU provider streams for the tier floor");
        let mut last = None;
        while let Some(chunk) = rx.recv().await {
            last = Some(chunk);
        }
        let final_chunk = last.expect("the stream must produce its final chunk");
        assert!(!*weak_reached.lock().unwrap(), "the weak provider must be skipped before opening any stream");
        assert!(*strong_reached.lock().unwrap());
        assert_eq!(final_chunk.provider_used, "azure-openai");
        assert_eq!(final_chunk.residency, "eu");

        // Weak-only chain: the typed, tier-naming error — also on streams.
        let only_weak_reached = Arc::new(Mutex::new(false));
        let only_weak: BoxedProvider = Arc::new(TierStreamProvider {
            reached: only_weak_reached.clone(),
            residency: Residency::Global,
        });
        let weak_chain =
            FallbackChain::new_with_providers(vec![("openai".to_owned(), only_weak)], 1);
        let err = weak_chain.infer_stream(&req).await.unwrap_err();
        assert!(!*only_weak_reached.lock().unwrap());
        assert!(matches!(err, ProviderError::TierUnavailable(message) if message.contains("eu_resident")));
    }

    /// Embedding path: the gate runs AFTER the EU-region gate but still BEFORE
    /// any network call, and exhausts with the named tier.
    #[tokio::test]
    async fn tier_gate_skips_weaker_provider_on_embedding_path_or_fails_naming_the_tier() {
        let reached = Arc::new(Mutex::new(false));
        let global_embed: BoxedProvider = Arc::new(RecordingEmbedProvider {
            reached: reached.clone(),
            zdr_supported: false,
            residency: Residency::Global,
        });
        let chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), global_embed)], 1);
        let req = EmbedRequest {
            request_id: "tier-e1".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            region: "swedencentral".to_owned(),
            min_privacy_tier: PrivacyTier::EuResident,
            ..EmbedRequest::default()
        };

        // The region gate passes (swedencentral is EU); the TIER gate is what
        // rejects — proving both gates compose and the tier one is present.
        let err = chain.create_embedding(&req).await.unwrap_err();
        assert!(!*reached.lock().unwrap(), "embedding provider below the tier must never see a network call");
        assert!(matches!(
            err,
            ProviderError::TierUnavailable(message) if message.contains("eu_resident")
        ));

        // And with no constraint, the very same deployment serves fine.
        let unconstrained = EmbedRequest {
            min_privacy_tier: PrivacyTier::Unspecified,
            ..req
        };
        chain.create_embedding(&unconstrained).await.expect("Unspecified imposes no constraint");
        assert!(*reached.lock().unwrap());
    }

    /// An embedding provider that records whether it was reached. Used to prove
    /// the residency gate rejects BEFORE any provider (network) call.
    struct RecordingEmbedProvider {
        reached: Arc<Mutex<bool>>,
        zdr_supported: bool,
        /// Declared residency; EU for the pre-existing residency-gate tests,
        /// Global for the tier-gate tests.
        residency: Residency,
    }

    #[async_trait::async_trait]
    impl ProviderRouter for RecordingEmbedProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                supports_embeddings: true,
                supports_zdr: self.zdr_supported,
                residency: self.residency,
                ..crate::provider::ProviderCapabilities::default()
            }
        }

        async fn infer(&self, _req: &InferRequest) -> Result<InferResponse, ProviderError> {
            Err(ProviderError::UnsupportedModel(
                "chat not supported".to_owned(),
            ))
        }

        async fn infer_stream(
            &self,
            _req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }

        async fn create_embedding(
            &self,
            req: &EmbedRequest,
        ) -> Result<EmbedResponse, ProviderError> {
            *self.reached.lock().unwrap() = true;
            Ok(EmbedResponse {
                request_id: req.request_id.clone(),
                vector: vec![0.0_f32; 3],
                model_used: req.model.clone(),
                provider_used: "azure-openai".to_owned(),
            })
        }
    }

    fn embed_chain(residency: EmbeddingResidency, reached: Arc<Mutex<bool>>) -> FallbackChain {
        let provider: BoxedProvider = Arc::new(RecordingEmbedProvider {
            reached,
            zdr_supported: false,
            residency: Residency::Eu,
        });
        let mut chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1);
        chain.residency = residency;
        chain
    }

    #[tokio::test]
    async fn embedding_rejects_non_eu_region_before_network_call() {
        // Deny-by-default: a non-EU requested region is rejected with a
        // ResidencyViolation BEFORE the provider (network) is ever reached.
        let reached = Arc::new(Mutex::new(false));
        let chain = embed_chain(EmbeddingResidency::default(), reached.clone());
        let req = EmbedRequest {
            request_id: "e1".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            zdr: false,
            region: "eastus2".to_owned(),
            min_privacy_tier: PrivacyTier::Unspecified,
        };
        let err = chain.create_embedding(&req).await.unwrap_err();
        assert!(
            matches!(err, ProviderError::ResidencyViolation(_)),
            "expected ResidencyViolation, got {err:?}"
        );
        assert!(
            !*reached.lock().unwrap(),
            "provider must NOT be reached when residency rejects"
        );
    }

    #[test]
    fn throttle_state_keeps_the_soonest_retry_after() {
        // The soonest retry is the one worth waiting for; a slower provider's
        // longer window must not decide the wait.
        let mut throttle = ThrottleState::default();
        throttle.record(9_000);
        throttle.record(1_500);
        throttle.record(57_000);
        assert_eq!(throttle.soonest_retry_ms, Some(1_500));
    }

    #[test]
    fn a_short_retry_after_is_waited_out() {
        let mut throttle = ThrottleState::default();
        throttle.record(1_200);
        assert_eq!(
            throttle.affordable_wait(),
            Some(Duration::from_millis(1_200))
        );
    }

    #[test]
    fn a_long_retry_after_is_reported_instead_of_waited_out() {
        // The live failure was retry_after_ms = 57_000. Holding an interactive
        // request open for a minute is worse than telling the user when to
        // return, so this must NOT become a wait.
        let mut throttle = ThrottleState::default();
        throttle.record(57_000);
        assert_eq!(throttle.affordable_wait(), None);
        assert!(matches!(
            throttle.exhausted_error(3),
            ProviderError::RateLimited {
                retry_after_ms: 57_000
            }
        ));
    }

    #[test]
    fn a_throttled_chain_reports_rate_limiting_not_generic_exhaustion() {
        // Regression guard for the live bug: retry_after_ms was discarded and the
        // chain returned AllExhausted, so a recoverable 429 reached the user as a
        // generic failure with no indication it was temporary.
        let mut throttle = ThrottleState::default();
        throttle.record(4_000);
        match throttle.exhausted_error(2) {
            ProviderError::RateLimited { retry_after_ms } => assert_eq!(retry_after_ms, 4_000),
            other => panic!("throttling must survive as RateLimited, got {other:?}"),
        }
    }

    #[test]
    fn an_unthrottled_chain_still_reports_exhaustion() {
        // Genuine breakage must not be mislabeled as throttling — nothing to wait
        // for, and a retry-after would be a fabrication.
        let throttle = ThrottleState::default();
        assert!(!throttle.throttled());
        assert!(matches!(
            throttle.exhausted_error(5),
            ProviderError::AllExhausted { attempts: 5 }
        ));
    }

    #[tokio::test]
    async fn embedding_accepts_eu_region() {
        // An EU region passes the gate and reaches the provider.
        let reached = Arc::new(Mutex::new(false));
        let chain = embed_chain(EmbeddingResidency::default(), reached.clone());
        let req = EmbedRequest {
            request_id: "e2".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            zdr: false,
            region: "swedencentral".to_owned(),
            min_privacy_tier: PrivacyTier::Unspecified,
        };
        let resp = chain.create_embedding(&req).await.unwrap();
        assert_eq!(resp.vector.len(), 3);
        assert!(
            *reached.lock().unwrap(),
            "provider should be reached for an EU region"
        );
    }

    #[tokio::test]
    async fn embedding_empty_region_uses_configured_eu_deployment() {
        // No requested region + an EU-configured deployment → allowed.
        let reached = Arc::new(Mutex::new(false));
        let residency = EmbeddingResidency {
            allow_non_eu: false,
            configured_region: "swedencentral".to_owned(),
        };
        let chain = embed_chain(residency, reached.clone());
        let req = EmbedRequest {
            request_id: "e3".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            zdr: false,
            region: String::new(),
            min_privacy_tier: PrivacyTier::Unspecified,
        };
        chain.create_embedding(&req).await.unwrap();
        assert!(*reached.lock().unwrap());
    }

    #[tokio::test]
    async fn embedding_empty_region_falls_to_non_eu_configured_deployment_rejected() {
        // No requested region but the configured deployment is non-EU and the
        // allow-flag is off → rejected before the network call.
        let reached = Arc::new(Mutex::new(false));
        let residency = EmbeddingResidency {
            allow_non_eu: false,
            configured_region: "eastus".to_owned(),
        };
        let chain = embed_chain(residency, reached.clone());
        let req = EmbedRequest {
            request_id: "e4".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            zdr: false,
            region: String::new(),
            min_privacy_tier: PrivacyTier::Unspecified,
        };
        let err = chain.create_embedding(&req).await.unwrap_err();
        assert!(matches!(err, ProviderError::ResidencyViolation(_)));
        assert!(!*reached.lock().unwrap());
    }

    #[tokio::test]
    async fn embedding_allow_flag_permits_non_eu_region() {
        // Explicit operator opt-in lets a non-EU region through (no rejection).
        let reached = Arc::new(Mutex::new(false));
        let residency = EmbeddingResidency {
            allow_non_eu: true,
            configured_region: String::new(),
        };
        let chain = embed_chain(residency, reached.clone());
        let req = EmbedRequest {
            request_id: "e5".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            zdr: false,
            region: "eastus2".to_owned(),
            min_privacy_tier: PrivacyTier::Unspecified,
        };
        chain.create_embedding(&req).await.unwrap();
        assert!(*reached.lock().unwrap());
    }

    // ---------------------------------------------------------------------
    // Tool-capable fallback ladder
    // ---------------------------------------------------------------------

    /// Serves only the models it was told it has a deployment for and rate-limits
    /// everything else, recording every model it was asked for. Models the live
    /// failure: Azure quota is per deployment, so one throttled deployment says
    /// nothing about its siblings.
    struct ThrottlingProvider {
        asked: Arc<Mutex<Vec<String>>>,
        serves: Vec<&'static str>,
        retry_after_ms: u64,
        /// Which surface this double stands in for. The ladder tests register one
        /// of each in a single chain, so the family gate has to be able to tell
        /// them apart — and it now does that from this declaration rather than
        /// from the registry name.
        family: ModelFamily,
    }

    impl ThrottlingProvider {
        fn record(&self, model: &str) -> Result<(), ProviderError> {
            self.asked.lock().unwrap().push(model.to_owned());
            if self.serves.contains(&model) {
                Ok(())
            } else {
                Err(ProviderError::RateLimited {
                    retry_after_ms: self.retry_after_ms,
                })
            }
        }
    }

    #[async_trait::async_trait]
    impl ProviderRouter for ThrottlingProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                model_family: self.family,
                residency: Residency::Eu,
                ..crate::provider::ProviderCapabilities::default()
            }
        }

        async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
            self.record(&req.model)?;
            Ok(InferResponse {
                request_id: req.request_id.clone(),
                content: "ok".to_owned(),
                model_used: req.model.clone(),
                stop_reason: "stop".to_owned(),
                input_tokens: 0,
                output_tokens: 0,
                tool_calls: Vec::new(),
                ..InferResponse::default()
            })
        }

        async fn infer_stream(
            &self,
            req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            self.record(&req.model)?;
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    /// The live topology: an Azure Foundry Claude resource plus an Azure
    /// `OpenAI` resource, with `retry_after_ms` past the interactive budget so the
    /// two-pass wait never fires and the ladder is the only way out.
    fn ladder_chain(
        asked: &Arc<Mutex<Vec<String>>>,
        claude_serves: Vec<&'static str>,
        openai_serves: Vec<&'static str>,
    ) -> FallbackChain {
        let claude: BoxedProvider = Arc::new(ThrottlingProvider {
            asked: asked.clone(),
            serves: claude_serves,
            retry_after_ms: 60_000,
            family: ModelFamily::Anthropic,
        });
        let openai: BoxedProvider = Arc::new(ThrottlingProvider {
            asked: asked.clone(),
            serves: openai_serves,
            retry_after_ms: 60_000,
            family: ModelFamily::OpenAiCompatible,
        });
        FallbackChain::new_with_providers(
            vec![
                ("azure-anthropic".to_owned(), claude),
                ("azure-openai".to_owned(), openai),
            ],
            1,
        )
        .with_intent_enabled(true)
    }

    /// A `verevon-balance` turn carrying a tool — the shape that floors to
    /// `Complex` and therefore resolves to `claude-sonnet-4-6`.
    fn tool_turn(request_id: &str, model: &str) -> InferRequest {
        InferRequest {
            request_id: request_id.to_owned(),
            model: model.to_owned(),
            messages: vec![crate::provider::ChatMessage {
                role: "user".to_owned(),
                content: "Kan du sjekke i Visma hva vi har tomt på lager?".to_owned(),
                name: String::new(),
            }],
            tools: vec![crate::provider::ToolDefinition {
                name: "mcp__visma__execute_query".to_owned(),
                description: String::new(),
                parameters_json: "{}".to_owned(),
            }],
            ..Default::default()
        }
    }

    /// A provider that answers like Azure really does: the response's model id
    /// is the versioned snapshot, not the deployment name it was asked for.
    struct VersionEchoProvider;

    #[async_trait::async_trait]
    impl ProviderRouter for VersionEchoProvider {
        async fn infer(&self, req: &InferRequest) -> Result<InferResponse, ProviderError> {
            Ok(InferResponse {
                request_id: req.request_id.clone(),
                content: "ok".to_owned(),
                model_used: format!("{}-2024-07-18", req.model),
                stop_reason: "stop".to_owned(),
                input_tokens: 0,
                output_tokens: 0,
                tool_calls: Vec::new(),
                ..InferResponse::default()
            })
        }

        async fn infer_stream(
            &self,
            req: &InferRequest,
        ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
            let (tx, rx) = mpsc::channel(4);
            let versioned = format!("{}-2024-07-18", req.model);
            let request_id = req.request_id.clone();
            tokio::spawn(async move {
                // A mid-stream delta with no model id, then the final chunk
                // carrying the versioned id — the real Azure shape.
                let _ = tx
                    .send(InferChunk {
                        request_id: request_id.clone(),
                        delta: "hei".to_owned(),
                        done: false,
                        model_used: String::new(),
                        input_tokens: 0,
                        output_tokens: 0,
                        provider_used: String::new(),
                        residency: String::new(),
                    })
                    .await;
                let _ = tx
                    .send(InferChunk {
                        request_id,
                        delta: String::new(),
                        done: true,
                        model_used: versioned,
                        input_tokens: 3,
                        output_tokens: 1,
                        provider_used: String::new(),
                        residency: String::new(),
                    })
                    .await;
            });
            Ok(rx)
        }
    }

    fn version_echo_chain() -> FallbackChain {
        let provider: BoxedProvider = Arc::new(VersionEchoProvider);
        FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1)
    }

    #[tokio::test]
    async fn model_used_is_the_requested_id_not_the_providers_versioned_snapshot() {
        // The live failure this pins: the tool loop carried model_used
        // ("gpt-4o-mini-2024-07-18") into the answer request's model slot, and
        // Azure 404'd DeploymentNotFound because that is a response-namespace
        // id, not a deployment name. model_used must therefore always be an id
        // a caller can safely request again.
        let response = version_echo_chain()
            .infer(&InferRequest {
                request_id: "norm-1".to_owned(),
                model: "gpt-4o-mini".to_owned(),
                ..Default::default()
            })
            .await
            .expect("provider serves");

        assert_eq!(response.model_used, "gpt-4o-mini");
    }

    #[tokio::test]
    async fn streamed_chunks_carry_the_requested_id_and_keep_their_shape() {
        let mut rx = version_echo_chain()
            .infer_stream(&InferRequest {
                request_id: "norm-2".to_owned(),
                model: "gpt-4o-mini".to_owned(),
                ..Default::default()
            })
            .await
            .expect("provider serves");

        let first = rx.recv().await.expect("delta chunk");
        let last = rx.recv().await.expect("final chunk");
        assert_eq!(
            first.model_used, "",
            "chunks without an id must stay without one — only the value is normalized"
        );
        assert_eq!(last.model_used, "gpt-4o-mini");
        assert_eq!(first.delta, "hei");
        assert!(last.done);
    }

    #[tokio::test]
    async fn a_throttled_tool_tier_falls_to_the_next_tool_capable_model() {
        // The live failure: claude-sonnet-4-6 is 429'd with retry_after 60s,
        // provider_serves_model keeps claude off the OpenAI surface, and the turn
        // died with "all providers exhausted". A sibling Claude deployment has
        // its own quota, so it is a real answer rather than a retry.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec!["claude-sonnet-4-5"], vec!["gpt-4o-mini"]);

        let response = chain
            .infer(&tool_turn("ladder-1", "verevon-balance"))
            .await
            .expect("the ladder must find a servable tool-capable model");

        assert_eq!(
            asked.lock().unwrap().as_slice(),
            ["claude-sonnet-4-6", "claude-sonnet-4-5"],
            "the resolved model is tried first, then the next ladder rung"
        );
        // The UI reports what actually answered, not what we intended to use.
        assert_eq!(response.model_used, "claude-sonnet-4-5");
    }

    #[tokio::test]
    async fn the_ladder_leaves_the_claude_family_only_after_exhausting_it() {
        // Every Claude deployment throttled → the OpenAI rung is still reachable,
        // because a degraded-but-tool-capable answer beats a dead turn.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec![], vec!["gpt-5-mini", "gpt-4o-mini"]);

        let response = chain
            .infer(&tool_turn("ladder-2", "verevon-balance"))
            .await
            .expect("the OpenAI rung serves once the Claude family is exhausted");

        assert_eq!(response.model_used, "gpt-5-mini");
        let asked = asked.lock().unwrap();
        let last_claude = asked
            .iter()
            .rposition(|m| m.starts_with("claude"))
            .expect("claude deployments are tried");
        let first_openai = asked
            .iter()
            .position(|m| !m.starts_with("claude"))
            .expect("an OpenAI rung is reached");
        assert!(last_claude < first_openai, "ladder order: {asked:?}");
    }

    #[tokio::test]
    async fn a_pinned_model_is_never_silently_substituted() {
        // The boundary that matters: the caller asked for THIS model. Answering
        // with a different one — with no way for them to have declined — is worse
        // than telling them it was busy. Same chain, same throttling, no ladder.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec!["claude-sonnet-4-5"], vec!["gpt-4o-mini"]);

        let error = chain
            .infer(&tool_turn("pinned-1", "claude-sonnet-4-6"))
            .await
            .unwrap_err();

        assert!(
            matches!(error, ProviderError::RateLimited { .. }),
            "expected the pinned model's own throttling, got {error:?}"
        );
        assert_eq!(
            asked.lock().unwrap().as_slice(),
            ["claude-sonnet-4-6"],
            "no model other than the pinned one may be asked"
        );
    }

    #[tokio::test]
    async fn a_pinned_model_that_the_intent_layer_is_off_for_is_also_not_substituted() {
        // Belt and braces: with the intent layer disabled nothing is "resolved",
        // so even a verevon-* id must not pick up a ladder. It falls through to
        // the per-provider default exactly as before.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let claude: BoxedProvider = Arc::new(ThrottlingProvider {
            asked: asked.clone(),
            serves: vec![],
            retry_after_ms: 60_000,
            family: ModelFamily::Anthropic,
        });
        let chain =
            FallbackChain::new_with_providers(vec![("azure-anthropic".to_owned(), claude)], 1);

        let error = chain
            .infer(&tool_turn("pinned-2", "verevon-balance"))
            .await
            .unwrap_err();

        assert!(matches!(error, ProviderError::RateLimited { .. }));
        assert_eq!(
            asked.lock().unwrap().as_slice(),
            [crate::provider::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL],
        );
    }

    #[tokio::test]
    async fn no_model_is_asked_twice_in_a_walk() {
        // An operator ladder that repeats the resolved model, and itself, must
        // not turn into extra load on already-throttled deployments.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec![], vec![]);
        let mut policy = RoutingPolicy::clone(&chain.policy.load_full());
        policy.tool_fallback_ladder = vec![
            "claude-sonnet-4-6".to_owned(),
            "claude-sonnet-4-5".to_owned(),
            "CLAUDE-SONNET-4-5".to_owned(),
            "claude-sonnet-4-6".to_owned(),
        ];
        chain.policy.store(Arc::new(policy));

        let error = chain
            .infer(&tool_turn("dedup-1", "verevon-balance"))
            .await
            .unwrap_err();

        assert!(matches!(error, ProviderError::RateLimited { .. }));
        assert_eq!(
            asked.lock().unwrap().as_slice(),
            ["claude-sonnet-4-6", "claude-sonnet-4-5"],
            "the resolved model and each rung are asked at most once"
        );
    }

    #[tokio::test]
    async fn a_fully_throttled_ladder_still_reports_rate_limiting() {
        // Regression guard for 428f580c: walking more models must not turn a
        // recoverable 429 into a generic AllExhausted. The user needs to be told
        // when to come back.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec![], vec![]);

        let error = chain
            .infer(&tool_turn("throttled-1", "verevon-balance"))
            .await
            .unwrap_err();

        match error {
            ProviderError::RateLimited { retry_after_ms } => assert_eq!(retry_after_ms, 60_000),
            other => panic!("a throttled chain must surface RateLimited, got {other:?}"),
        }
        assert!(
            asked.lock().unwrap().len() > 1,
            "the ladder should have been walked before giving up"
        );
    }

    #[tokio::test]
    async fn the_streaming_path_walks_the_ladder_too() {
        // Plain chat streams, so this is the path that actually failed in
        // production; a fix that only covered `infer` would not have helped.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec!["claude-sonnet-4-5"], vec!["gpt-4o-mini"]);

        chain
            .infer_stream(&tool_turn("stream-1", "verevon-balance"))
            .await
            .expect("the streaming ladder must find a servable model");

        assert_eq!(
            asked.lock().unwrap().as_slice(),
            ["claude-sonnet-4-6", "claude-sonnet-4-5"]
        );
    }

    #[tokio::test]
    async fn the_streaming_path_honors_a_pinned_model() {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let chain = ladder_chain(&asked, vec!["claude-sonnet-4-5"], vec!["gpt-4o-mini"]);

        let error = chain
            .infer_stream(&tool_turn("stream-2", "claude-sonnet-4-6"))
            .await
            .unwrap_err();

        assert!(matches!(error, ProviderError::RateLimited { .. }));
        assert_eq!(asked.lock().unwrap().as_slice(), ["claude-sonnet-4-6"]);
    }

    #[test]
    fn the_ladder_is_pruned_to_deployments_that_exist_and_stays_bounded() {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let mut chain = ladder_chain(&asked, vec![], vec![]);

        // Unknown catalogs prune nothing, but the step bound still holds.
        let unpruned = chain.tool_ladder("claude-sonnet-4-6");
        assert!(unpruned.len() <= MAX_TOOL_LADDER_STEPS);
        assert!(!unpruned.iter().any(|m| m == "claude-sonnet-4-6"));

        // A known catalog drops rungs this resource cannot serve — otherwise the
        // bounded walk is spent on deployments that can only 404.
        chain.deployed = DeployedModels {
            anthropic: Some(vec![
                "claude-sonnet-4-6".to_owned(),
                "claude-haiku-4-5".to_owned(),
            ]),
            openai: Some(vec!["gpt-4o-mini".to_owned()]),
        };
        assert_eq!(
            chain.tool_ladder("claude-sonnet-4-6"),
            ["claude-haiku-4-5", "gpt-4o-mini"]
        );
    }

    #[tokio::test]
    async fn unspecified_model_uses_the_configured_azure_deployment() {
        // Fault 1 end-to-end through the chain: the model the provider is asked
        // for is the operator's configured deployment, not the hardcoded router.
        let seen = Arc::new(Mutex::new(None));
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            zdr_supported: false,
            ..RecordingProvider::default()
        });
        let mut chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1);
        chain.defaults = ProviderDefaults {
            azure_openai: "gpt-4o-mini".to_owned(),
            azure_anthropic: "claude-haiku-4-5".to_owned(),
        };
        let req = InferRequest {
            request_id: "cfg-default-1".to_owned(),
            model: String::new(),
            ..Default::default()
        };

        let response = chain.infer(&req).await.unwrap();

        assert_eq!(seen.lock().unwrap().as_deref(), Some("gpt-4o-mini"));
        assert_eq!(response.model_used, "gpt-4o-mini");
    }

    #[tokio::test]
    async fn embedding_zdr_fails_before_unverified_provider_call() {
        let reached = Arc::new(Mutex::new(false));
        let chain = embed_chain(EmbeddingResidency::default(), reached.clone());
        let req = EmbedRequest {
            request_id: "e-zdr".to_owned(),
            provider_hint: "azure-openai".to_owned(),
            text: "hello".to_owned(),
            model: "text-embedding-3-large".to_owned(),
            zdr: true,
            region: "swedencentral".to_owned(),
            min_privacy_tier: PrivacyTier::Unspecified,
        };

        let error = chain.create_embedding(&req).await.unwrap_err();
        assert!(matches!(error, ProviderError::ZdrUnavailable(_)));
        assert!(!*reached.lock().unwrap());
    }
}
