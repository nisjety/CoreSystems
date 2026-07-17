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

/// Default chat model for a registered provider, used when the request leaves
/// the model unspecified ("Velion Auto").
///
/// * `azure-openai` → `model-router` (Azure's cost-optimizing auto-router).
/// * `azure-anthropic` → cheapest Claude deployment (Haiku).
/// * `anthropic` (direct) → first-party Claude default.
/// * `openai` (direct) → `OpenAI` chat default.
fn default_model_for(provider_name: &str) -> &'static str {
    match provider_name {
        "azure-openai" => AZURE_MODEL_ROUTER,
        "azure-anthropic" => super::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL,
        "anthropic" => super::anthropic::DEFAULT_ANTHROPIC_MODEL,
        _ => super::openai::DEFAULT_OPENAI_MODEL,
    }
}

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

        Self {
            providers,
            max_retries: cfg.max_retries_per_provider,
            cache: Arc::new(PromptCache::new(cfg.cache_ttl_secs)),
            policy,
            policy_client,
            budget,
            residency,
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
        let req: &InferRequest = intent_req.as_ref().unwrap_or(req);

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
            // Skip providers that cannot serve the requested model family — a
            // `claude-*` model must not hit the OpenAI surface (it would 404 the
            // deployment) and vice-versa. Unspecified models pass (they resolve
            // to the provider's default below).
            if !provider_serves_model(name, &req.model) {
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
            // "Velion Auto" / unspecified model → resolve to this provider's
            // default so an unpinned request works against whatever provider is
            // configured. Specified models pass through unchanged.
            let resolved_req;
            let call_req: &InferRequest = if is_unspecified_model(&req.model) {
                let mut r = req.clone();
                default_model_for(name).clone_into(&mut r.model);
                resolved_req = r;
                &resolved_req
            } else {
                req
            };
            for attempt in 1..=self.max_retries {
                total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let _enter = span.enter();

                match provider.infer_dyn(call_req).await {
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

        if req.zdr && total_attempts == 0 {
            Err(ProviderError::ZdrUnavailable(
                "no matching provider deployment has verified ZDR support".to_owned(),
            ))
        } else {
            Err(ProviderError::AllExhausted {
                attempts: total_attempts,
            })
        }
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
        let req: &InferRequest = intent_req.as_ref().unwrap_or(req);

        let mut total_attempts: u32 = 0;

        for (name, provider) in &self.providers {
            if !Self::provider_matches(name, &req.provider_hint) {
                continue;
            }
            if !provider_serves_model(name, &req.model) {
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
            // "Velion Auto" / unspecified model → resolve to this provider's default.
            let resolved_req;
            let call_req: &InferRequest = if is_unspecified_model(&req.model) {
                let mut r = req.clone();
                default_model_for(name).clone_into(&mut r.model);
                resolved_req = r;
                &resolved_req
            } else {
                req
            };
            for attempt in 1..=self.max_retries {
                total_attempts += 1;
                let span = tracing::info_span!(
                    "provider_stream_attempt",
                    provider = %name,
                    attempt = attempt,
                    request_id = %req.request_id,
                );
                let _enter = span.enter();

                match provider.infer_stream_dyn(call_req).await {
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

        if req.zdr && total_attempts == 0 {
            Err(ProviderError::ZdrUnavailable(
                "no matching streaming provider deployment has verified ZDR support".to_owned(),
            ))
        } else {
            Err(ProviderError::AllExhausted {
                attempts: total_attempts,
            })
        }
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
        assert_eq!(
            default_model_for("anthropic"),
            crate::provider::anthropic::DEFAULT_ANTHROPIC_MODEL
        );
        assert_eq!(
            default_model_for("openai"),
            crate::provider::openai::DEFAULT_OPENAI_MODEL
        );
        // Azure OpenAI defaults to the cost-optimizing model-router, not a fixed
        // chat model.
        assert_eq!(default_model_for("azure-openai"), AZURE_MODEL_ROUTER);
        assert_eq!(default_model_for("azure-openai"), "model-router");
        // Azure Anthropic defaults to the cheapest Claude deployment.
        assert_eq!(
            default_model_for("azure-anthropic"),
            crate::provider::anthropic::DEFAULT_AZURE_ANTHROPIC_MODEL
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
