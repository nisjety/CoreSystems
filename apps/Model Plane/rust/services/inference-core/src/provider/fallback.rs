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
    InferRequest, InferResponse, ModelInfo, ProviderCapabilities, ProviderError, ProviderRouter,
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
    /// The live Velion routing policy. Seeded from [`RoutingPolicy::default`] and
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

/// True when the caller didn't pin a model — empty or a "let the gateway pick"
/// sentinel. Such requests resolve to the per-provider default so "Velion Auto"
/// works against whatever provider is actually configured.
fn is_unspecified_model(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    // Empty / "default", plus any Velion intent id. The intent layer normally
    // rewrites a `velion-*` id to a concrete model before the provider loop;
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

/// Whether a registered provider can serve the requested model. Anthropic-shaped
/// providers serve only `claude-*`; `OpenAI`-shaped providers serve everything
/// else. An unspecified model is served by any provider (it resolves to that
/// provider's default). This stops the chain wasting an attempt — and emitting a
/// spurious 404 — by sending a Claude model to the `OpenAI` surface or an
/// `OpenAI` model to the Anthropic surface.
fn provider_serves_model(provider_name: &str, model: &str) -> bool {
    if is_unspecified_model(model) {
        return true;
    }
    let anthropic_provider = matches!(provider_name, "anthropic" | "azure-anthropic");
    anthropic_provider == is_anthropic_model(model)
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
                            let p = p
                                .with_zdr_confirmed(cfg.azure_openai_zdr_confirmed)
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
                                    .with_zdr_confirmed(cfg.azure_openai_zdr_confirmed)
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
        // bootstrap env config (velion_intent_enabled / budget_usd) override the
        // seed so the static knobs still work without a session-core store.
        let seed = RoutingPolicy {
            enabled: cfg.velion_intent_enabled,
            budget_cap_usd: cfg.velion_intent_budget_usd,
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

        if cfg.velion_intent_enabled {
            info!(
                budget_gate = budget.is_some(),
                policy_store = policy_client.is_some(),
                "velion intent layer enabled"
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
        Self {
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

    /// Toggle the Velion intent layer by flipping `enabled` on the live policy
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

    /// Resolve a Velion intent model id (`velion-budget`/`-balance`/`-genius`)
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
            self.budget.as_deref(),
        )
        .await?;
        info!(
            request_id = %req.request_id,
            mode = decision.mode.as_str(),
            complexity = decision.complexity.as_str(),
            posture = ?decision.posture,
            resolved_model = %decision.model,
            "velion intent resolved"
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

    fn provider_matches(name: &str, hint: &str) -> bool {
        // Normalise: lowercase + treat '_' and '-' as equivalent. Phase 3 B-spike
        // root cause: a caller sending `azure_openai` (underscore) matched zero
        // providers (registry id is `azure-openai`), yielding AllExhausted(0) with
        // no server log. Normalising both sides makes the hop robust to either form.
        let hint = hint.trim().to_ascii_lowercase().replace('_', "-");
        let name = name.to_ascii_lowercase().replace('_', "-");
        hint.is_empty()
            || hint == name
            || (hint == "openai" && name == "azure-openai")
            || (hint == "azure" && name == "azure-openai")
            || (hint == "anthropic" && name == "azure-anthropic")
            || (hint == "claude" && (name == "anthropic" || name == "azure-anthropic"))
    }

    /// Default chat model for a registered provider, used when the request
    /// leaves the model unspecified ("Velion Auto"). The Azure entries come from
    /// the operator's configured deployment catalog — see [`ProviderDefaults`].
    fn default_model_for(&self, provider_name: &str) -> &str {
        match provider_name {
            "azure-openai" => &self.defaults.azure_openai,
            "azure-anthropic" => &self.defaults.azure_anthropic,
            "anthropic" => super::anthropic::DEFAULT_ANTHROPIC_MODEL,
            _ => super::openai::DEFAULT_OPENAI_MODEL,
        }
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
    /// `velion-*` mode. Substituting under a caller who pinned a model would
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
        // Velion intent layer: resolve a `velion-*` mode to a concrete model
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

        if req.zdr && total_attempts == 0 {
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
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            // Skip providers that cannot serve the requested model family — a
            // `claude-*` model must not hit the OpenAI surface (it would 404 the
            // deployment) and vice-versa. Unspecified models pass (they resolve
            // to the provider's default below).
            if !provider_serves_model(name, model) {
                continue;
            }
            if req.zdr && !provider.capabilities_dyn().supports_zdr {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "provider skipped: ZDR was required but is not verified for this deployment"
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
    /// An unspecified model ("Velion Auto") becomes that provider's configured
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

    /// Perform streaming inference with fallback (no caching for streams).
    ///
    /// # Errors
    ///
    /// Returns `ProviderError::AllExhausted` if every provider and retry is exhausted.
    pub async fn infer_stream(
        &self,
        req: &InferRequest,
    ) -> Result<mpsc::Receiver<InferChunk>, ProviderError> {
        // Velion intent layer — same resolution as the unary path.
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

        if req.zdr && total_attempts == 0 {
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
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            if !provider_serves_model(name, model) {
                continue;
            }
            if req.zdr && !provider.capabilities_dyn().supports_zdr {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "stream provider skipped: ZDR was required but is not verified for this deployment"
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
                        return Some(Self::normalize_stream_model(rx, call_req.model.clone()));
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
        // EU embedding residency — REQUEST-time deny-by-default gate. Reject
        // BEFORE any network call when the resolved region is non-EU and the
        // operator has not explicitly opted in. The resolved region is the
        // request's `region` when set, else the configured deployment region.
        // Mirrors the speech.rs allow-flag shape but REJECTS (does not
        // warn-and-fallback): an EU/ZDR posture must fail closed.
        if !self.residency.allow_non_eu {
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
        }

        let mut total_attempts: u32 = 0;

        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            if req.zdr && !provider.capabilities_dyn().supports_zdr {
                warn!(
                    provider = %name,
                    request_id = %req.request_id,
                    "embedding provider skipped: ZDR was required but is not verified for this deployment"
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

        if req.zdr && total_attempts == 0 {
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
            .filter(|(name, _)| Self::provider_matches(name, provider))
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
        for m in ["", "   ", "default", "AUTO", "Velion", "velion-auto"] {
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

    #[test]
    fn model_family_gating() {
        // Claude models only on the Anthropic-shaped providers.
        assert!(provider_serves_model("anthropic", "claude-haiku-4-5"));
        assert!(provider_serves_model("azure-anthropic", "claude-opus-4-8"));
        assert!(!provider_serves_model("azure-openai", "claude-haiku-4-5"));
        assert!(!provider_serves_model("openai", "claude-sonnet-4-6"));

        // Non-Claude models only on the OpenAI-shaped providers.
        assert!(provider_serves_model("azure-openai", "gpt-4o-mini"));
        assert!(provider_serves_model("azure-openai", "model-router"));
        assert!(provider_serves_model("openai", "deepseek-v3-2"));
        assert!(!provider_serves_model("anthropic", "gpt-4o-mini"));
        assert!(!provider_serves_model("azure-anthropic", "model-router"));

        // Unspecified models pass on every provider (resolve to its default).
        for p in ["anthropic", "azure-anthropic", "openai", "azure-openai"] {
            assert!(provider_serves_model(p, ""));
            assert!(provider_serves_model(p, "velion-auto"));
        }
    }

    #[test]
    fn anthropic_hint_matches_azure_anthropic() {
        assert!(FallbackChain::provider_matches(
            "azure-anthropic",
            "anthropic"
        ));
        assert!(FallbackChain::provider_matches("azure-anthropic", "claude"));
        assert!(FallbackChain::provider_matches("anthropic", "claude"));
        assert!(FallbackChain::provider_matches("azure-openai", "azure"));
        // A claude hint must not match the OpenAI surface.
        assert!(!FallbackChain::provider_matches("azure-openai", "claude"));
    }

    #[test]
    fn provider_hint_underscore_matches_hyphen_id() {
        // Phase 3 B-spike regression: a caller sending `azure_openai` (underscore)
        // must match the `azure-openai` provider id. Before normalisation this
        // matched zero providers → AllExhausted(0) with no server log.
        assert!(FallbackChain::provider_matches(
            "azure-openai",
            "azure_openai"
        ));
        assert!(FallbackChain::provider_matches(
            "azure-openai",
            "AZURE_OPENAI"
        ));
        assert!(FallbackChain::provider_matches(
            "azure-anthropic",
            "azure_anthropic"
        ));
        // Hyphen/underscore equivalence must not over-match across surfaces.
        assert!(!FallbackChain::provider_matches("azure-openai", "claude"));
    }

    /// Records the model it was invoked with so tests can assert resolution.
    struct RecordingProvider {
        seen_model: Arc<Mutex<Option<String>>>,
        zdr_supported: bool,
    }

    #[async_trait::async_trait]
    impl ProviderRouter for RecordingProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                supports_zdr: self.zdr_supported,
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
    async fn velion_mode_resolves_to_concrete_model_through_the_chain() {
        // Provider registered as azure-openai (serves non-claude models); intent
        // layer on, no budget client → Unknown posture. velion-budget + a trivial
        // prompt → Budget/Simple → the current policy table's gpt-5-nano
        // reaches the provider. This assertion must follow the versioned table
        // rather than the older cheap-fallback constant.
        let seen = Arc::new(Mutex::new(None));
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            zdr_supported: false,
        });
        let chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1)
                .with_intent_enabled(true);
        let req = InferRequest {
            request_id: "r3".to_owned(),
            model: "velion-budget".to_owned(),
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
        // With the intent layer off, a leaked velion-* id is still treated as
        // unspecified (is_unspecified_model), so it resolves to the provider
        // default — model-router for azure-openai — rather than being sent
        // verbatim (which would 404 the deployment).
        let seen = Arc::new(Mutex::new(None));
        let provider: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: seen.clone(),
            zdr_supported: false,
        });
        // new_with_providers defaults intent_enabled = false.
        let chain =
            FallbackChain::new_with_providers(vec![("azure-openai".to_owned(), provider)], 1);
        let req = InferRequest {
            request_id: "r4".to_owned(),
            model: "velion-genius".to_owned(),
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
        });
        let verified: BoxedProvider = Arc::new(RecordingProvider {
            seen_model: verified_seen.clone(),
            zdr_supported: true,
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
        });
        let unavailable_chain =
            FallbackChain::new_with_providers(vec![("openai".to_owned(), unavailable)], 1);
        let error = unavailable_chain.infer(&req).await.unwrap_err();
        assert!(matches!(error, ProviderError::ZdrUnavailable(_)));
        assert!(unavailable_seen.lock().unwrap().is_none());
    }

    /// An embedding provider that records whether it was reached. Used to prove
    /// the residency gate rejects BEFORE any provider (network) call.
    struct RecordingEmbedProvider {
        reached: Arc<Mutex<bool>>,
        zdr_supported: bool,
    }

    #[async_trait::async_trait]
    impl ProviderRouter for RecordingEmbedProvider {
        fn capabilities(&self) -> crate::provider::ProviderCapabilities {
            crate::provider::ProviderCapabilities {
                supports_embeddings: true,
                supports_zdr: self.zdr_supported,
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
        });
        let openai: BoxedProvider = Arc::new(ThrottlingProvider {
            asked: asked.clone(),
            serves: openai_serves,
            retry_after_ms: 60_000,
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

    /// A `velion-balance` turn carrying a tool — the shape that floors to
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
            .infer(&tool_turn("ladder-1", "velion-balance"))
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
            .infer(&tool_turn("ladder-2", "velion-balance"))
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
        // so even a velion-* id must not pick up a ladder. It falls through to
        // the per-provider default exactly as before.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let claude: BoxedProvider = Arc::new(ThrottlingProvider {
            asked: asked.clone(),
            serves: vec![],
            retry_after_ms: 60_000,
        });
        let chain =
            FallbackChain::new_with_providers(vec![("azure-anthropic".to_owned(), claude)], 1);

        let error = chain
            .infer(&tool_turn("pinned-2", "velion-balance"))
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
            .infer(&tool_turn("dedup-1", "velion-balance"))
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
            .infer(&tool_turn("throttled-1", "velion-balance"))
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
            .infer_stream(&tool_turn("stream-1", "velion-balance"))
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
        };

        let error = chain.create_embedding(&req).await.unwrap_err();
        assert!(matches!(error, ProviderError::ZdrUnavailable(_)));
        assert!(!*reached.lock().unwrap());
    }
}
